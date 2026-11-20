//! Durable launch issue journal and per-attempt process runner (P1-09).
//!
//! Order for one admitted attempt, each step its own writer transaction:
//! `issue` (outbox ISSUED_UNKNOWN, attempt STARTING) → recheck stop → spawn *outside any
//! transaction* → register the verified root identity (outbox CONFIRMED, attempt
//! RUNNING) → append raw-referenced events only after their bytes are synced → record
//! OS observations → conclude with `TREE_ABSENT` or `STOP_UNCONFIRMED`.
//!
//! A spawn error leaves the launch ISSUED_UNKNOWN (ambiguity retained); nothing here
//! retries a Start. A stop request is a separate durable fact from an observed exit.
//! This module never seals a result or releases claims: `outcomes` (P1-10) does that from
//! the durable facts recorded here.
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use jojo_domain::identity::{ArtifactId, IdGenerator, OpaqueId};
use jojo_domain::payload::{Record, provider_event};
use jojo_domain::provider::{
    Capabilities, EventPayload, LaunchSpec, Stream, Support, TerminalStatus,
};
use jojo_domain::time::Clock;

use crate::model::{from_db, id_from_text, id_to_text, to_db};
use crate::process::{
    ExitObservation, ProcessIdentity, ProcessLaunch, ProcessMessage, RawEvent, Signal,
    SignalOutcome, SignalRecord, SpawnFailure, SpoolFaults, Supervised,
};
use crate::sqlite::Value;
use crate::{
    Admission, CommandError, CommandId, CommandRequest, EffectState, Engine, ReadView, Reader,
    StorageError, Tx, TxError, sha256_hex,
};

fn deny(s: impl Into<String>) -> TxError {
    TxError::Rejected(s.into())
}

fn stream_db(s: Stream) -> &'static str {
    match s {
        Stream::Stdout => "STDOUT",
        Stream::Stderr => "STDERR",
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObservationKind {
    NotSpawned,
    SpawnFailed,
    StopRequested,
    SignalSent,
    SignalRefused,
    RootExited,
    DescendantsAlive,
    StreamClosed,
    StreamTruncated,
    ProtocolFailure,
    EvidenceLoss,
    TreeAbsent,
    StopUnconfirmed,
}

impl ObservationKind {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::NotSpawned => "NOT_SPAWNED",
            Self::SpawnFailed => "SPAWN_FAILED",
            Self::StopRequested => "STOP_REQUESTED",
            Self::SignalSent => "SIGNAL_SENT",
            Self::SignalRefused => "SIGNAL_REFUSED",
            Self::RootExited => "ROOT_EXITED",
            Self::DescendantsAlive => "DESCENDANTS_ALIVE",
            Self::StreamClosed => "STREAM_CLOSED",
            Self::StreamTruncated => "STREAM_TRUNCATED",
            Self::ProtocolFailure => "PROTOCOL_FAILURE",
            Self::EvidenceLoss => "EVIDENCE_LOSS",
            Self::TreeAbsent => "TREE_ABSENT",
            Self::StopUnconfirmed => "STOP_UNCONFIRMED",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredProviderEvent {
    pub sequence: u64,
    pub kind: String,
    pub stream: String,
    pub raw_start: u64,
    pub raw_end: u64,
    pub payload: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessRecord {
    pub pid: u32,
    pub start: String,
    pub pgid: u32,
    pub role: String,
}

fn payload_kind(p: &EventPayload) -> String {
    let debug = format!("{p:?}");
    debug
        .split(|c: char| !c.is_alphanumeric())
        .next()
        .unwrap_or_default()
        .to_owned()
}

impl Tx<'_> {
    fn launched(&self, attempt: OpaqueId) -> Result<(), TxError> {
        self.conn
            .query_optional(
                "SELECT 1 FROM process_launches WHERE attempt=?1",
                &[Value::Text(&id_to_text(attempt))],
                |r| r.int(0),
            )?
            .map(drop)
            .ok_or_else(|| deny("no issued launch"))
    }

    /// Crosses the durable issue boundary for one scheduler reservation, or refuses.
    /// A refusal caused by a now-stale approval commits that stop and returns `Ok(false)`.
    pub fn issue_process_launch(&mut self, spec: &LaunchSpec) -> Result<bool, TxError> {
        spec.validate()
            .map_err(|e| deny(format!("launch spec: {e:?}")))?;
        let attempt = spec.attempt.0;
        let id = id_to_text(attempt);
        // P1-09 admits one invocation per attempt. A continuation/retry uses a new
        // attempt, so this milestone has exactly generation 1 in the owning epoch.
        if spec.engine_epoch != OpaqueId(u128::from(self.epoch.0)) || spec.dispatch_generation != 1
        {
            return Err(deny("stale launch epoch or generation"));
        }
        if self.launched(attempt).is_ok() {
            return Err(deny(
                "attempt already launched; continuation needs a new attempt",
            ));
        }
        let sealed = self
            .conn
            .query_optional(
                "SELECT sealed FROM scheduler_attempts WHERE attempt=?1 AND epoch=?2",
                &[Value::Text(&id), Value::Int(to_db(self.epoch.0)?)],
                |r| r.int(0),
            )?
            .ok_or_else(|| deny("no current-epoch scheduler reservation"))?;
        if sealed != 0 {
            return Err(deny("reservation is sealed"));
        }
        let worktree =
            crate::worktrees::load(self.conn, attempt)?.ok_or_else(|| deny("no owned worktree"))?;
        if worktree.state != "BOUND"
            || worktree.path != spec.cwd
            || worktree.output != spec.raw_output_dir
            || worktree.epoch != self.epoch
        {
            return Err(deny("launch cwd/output must be the bound owned worktree"));
        }
        if self.admission()? != Admission::Open {
            return Err(deny("admission closed"));
        }
        self.invalidate_approvals()?;
        let phase = self.conn.query_optional(
            "SELECT phase FROM approval_attempts WHERE id=?1
             AND NOT EXISTS(SELECT 1 FROM approval_stops WHERE attempt=?1)",
            &[Value::Text(&id)],
            |r| r.text(0),
        )?;
        if phase.as_deref() != Some("RESERVED") {
            // Stop (possibly just committed by approval invalidation) or no longer reserved.
            return Ok(false);
        }
        self.verify_launch_bindings(spec, &worktree)?;
        crate::recovery::verify_continuation(self.conn, spec)?;
        let key = format!("scheduler-launch:{id}");
        self.advance_effect(&key, EffectState::IssuedUnknown)?;
        self.conn.execute(
            "UPDATE approval_attempts SET phase='STARTING' WHERE id=?1 AND phase='RESERVED'",
            &[Value::Text(&id)],
        )?;
        let digest = sha256_hex(&[format!("{spec:?}").as_bytes()]);
        let event = self.append_event(
            "ProcessLaunchIssued",
            None,
            &Record::new()
                .text("attempt", id.clone())
                .text("spec", digest.clone()),
        )?;
        self.conn.execute(
            "INSERT INTO process_launches VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            &[
                Value::Text(&id),
                Value::Int(to_db(spec.dispatch_generation)?),
                Value::Int(to_db(self.epoch.0)?),
                Value::Text(&format!("{:?}", spec.fingerprint.provider)),
                Value::Text(&spec.fingerprint.parser),
                Value::Text(&spec.fingerprint.executable.display().to_string()),
                Value::Text(&spec.cwd.display().to_string()),
                Value::Text(&spec.raw_output_dir.display().to_string()),
                Value::Text(&digest),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        // D3: the version and digest of the executable this run uses.
        self.record_attempt_executable(attempt, &spec.fingerprint)?;
        Ok(true)
    }

    fn verify_launch_bindings(
        &self,
        spec: &LaunchSpec,
        worktree: &crate::WorktreeRecord,
    ) -> Result<(), TxError> {
        let attempt = id_to_text(spec.attempt.0);
        let (step, step_revision, plan, plan_revision, prd, adr, sources) = self.conn.query_one(
            "SELECT q.step,q.step_revision,q.plan,q.plan_revision,q.prd,q.adr,i.sources
             FROM scheduler_attempts a JOIN scheduler_queue q ON q.step=a.step AND q.step_revision=a.step_revision
             JOIN dependency_attempt_inputs i ON i.attempt=a.attempt WHERE a.attempt=?1",
            &[Value::Text(&attempt)],
            |r| Ok((r.text(0)?,r.int(1)?,r.text(2)?,r.int(3)?,r.text(4)?,r.text(5)?,r.text(6)?)))?;
        let bindings = &spec.bindings;
        if step != id_to_text(bindings.step_id.0)
            || from_db(step_revision)? != bindings.step_revision.0
            || plan != id_to_text(bindings.plan_id.0)
            || from_db(plan_revision)? != bindings.plan_revision.0
            || bindings.worktree_id.0 != spec.attempt.0
            || bindings.repository_id != worktree.repository
            || spec.source.common_dir != worktree.common_dir()
            || spec.source.base_commit != worktree.base
            || spec.source.candidate_source_vector != sha256_hex(&[sources.as_bytes()])
            || spec.prompt_digest != sha256_hex(&[&spec.prompt])
        {
            return Err(deny(
                "launch bindings/source/prompt differ from reservation",
            ));
        }
        let feature = self.conn.query_one("SELECT feature_id FROM artifact_versions WHERE artifact_id=?1 ORDER BY revision DESC LIMIT 1",
            &[Value::Text(&prd)], |r| r.text(0))?;
        if feature != id_to_text(bindings.feature_id.0) {
            return Err(deny("launch feature mismatch"));
        }
        let stored = crate::dependencies::load_plan(
            self.conn,
            ArtifactId(bindings.plan_id.0),
            bindings.plan_revision,
        )?
        .ok_or_else(|| deny("reserved plan missing"))?;
        let resource = stored
            .plan
            .resources
            .iter()
            .find(|r| r.step.id == bindings.step_id.0 && r.step.revision == bindings.step_revision)
            .ok_or_else(|| deny("reserved step resource profile missing"))?;
        if resource.provider != spec.fingerprint.provider
            || resource.repository != bindings.repository_id
        {
            return Err(deny(
                "launch provider/repository differs from reserved resource claims",
            ));
        }
        for (subject, decision) in [prd, adr, plan].iter().zip(spec.approval_ids) {
            let current =
                crate::approvals::applicable(self.conn, ArtifactId(id_from_text(subject)?))?;
            if current.map(|d| OpaqueId(u128::from(d.0))) != Some(decision) {
                return Err(deny("launch approval reference is stale or mismatched"));
            }
        }
        Ok(())
    }

    pub fn record_process_observation(
        &mut self,
        attempt: OpaqueId,
        kind: ObservationKind,
        detail: &str,
    ) -> Result<(), TxError> {
        self.launched(attempt)?;
        let id = id_to_text(attempt);
        let event = self.append_event(
            "ProcessObservation",
            None,
            &Record::new()
                .text("attempt", id.clone())
                .text("observation", kind.as_db()),
        )?;
        self.conn.execute(
            "INSERT INTO process_observations VALUES (?1,?2,?3,?4)",
            &[
                Value::Int(to_db(event.0)?),
                Value::Text(&id),
                Value::Text(kind.as_db()),
                Value::Text(detail),
            ],
        )?;
        Ok(())
    }

    /// Proven no child: the launching engine itself decided not to spawn (stop seen after
    /// issue) or failed before a child could exist. The issue intent becomes REJECTED.
    pub fn record_not_spawned(&mut self, attempt: OpaqueId, reason: &str) -> Result<(), TxError> {
        self.launched(attempt)?;
        if self.root_registered(attempt)? {
            return Err(deny("a process was registered"));
        }
        self.advance_effect(
            &format!("scheduler-launch:{}", id_to_text(attempt)),
            EffectState::Rejected,
        )?;
        self.record_process_observation(attempt, ObservationKind::NotSpawned, reason)
    }

    fn root_registered(&self, attempt: OpaqueId) -> Result<bool, TxError> {
        Ok(self
            .conn
            .query_optional(
                "SELECT 1 FROM process_records WHERE attempt=?1 AND role='ROOT'",
                &[Value::Text(&id_to_text(attempt))],
                |r| r.int(0),
            )?
            .is_some())
    }

    fn insert_process(
        &mut self,
        attempt: OpaqueId,
        p: &ProcessIdentity,
        role: &str,
    ) -> Result<(), TxError> {
        let id = id_to_text(attempt);
        let event = self.append_event(
            "ProcessIdentified",
            None,
            &Record::new()
                .text("attempt", id.clone())
                .text("role", role)
                .count("pid", u64::from(p.pid))
                .text("start", p.start.clone()),
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO process_records VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            &[
                Value::Text(&id),
                Value::Int(i64::from(p.pid)),
                Value::Text(&p.start),
                Value::Int(i64::from(p.pgid)),
                Value::Int(i64::from(p.ppid)),
                Value::Text(&p.boot),
                Value::Text(role),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(())
    }

    /// Registers the verified root. Returns whether a stop is already pending, in which
    /// case the caller must route the new child straight into the stop procedure.
    pub fn register_process_root(
        &mut self,
        attempt: OpaqueId,
        root: &ProcessIdentity,
    ) -> Result<bool, TxError> {
        self.launched(attempt)?;
        let id = id_to_text(attempt);
        self.insert_process(attempt, root, "ROOT")?;
        self.advance_effect(&format!("scheduler-launch:{id}"), EffectState::Confirmed)?;
        let changed = self.conn.execute(
            "UPDATE approval_attempts SET phase='RUNNING' WHERE id=?1 AND phase='STARTING'
             AND NOT EXISTS(SELECT 1 FROM approval_stops WHERE attempt=?1)",
            &[Value::Text(&id)],
        )?;
        Ok(changed == 0)
    }

    pub fn record_process_descendants(
        &mut self,
        attempt: OpaqueId,
        found: &[ProcessIdentity],
    ) -> Result<(), TxError> {
        self.launched(attempt)?;
        for p in found {
            self.insert_process(attempt, p, "DESCENDANT")?;
        }
        Ok(())
    }

    /// Durable stop intent (idempotent) plus an observation carrying this reason. A cause
    /// already recorded by the original requester (cancel, revocation, shutdown) wins.
    pub fn request_process_stop(&mut self, attempt: OpaqueId, reason: &str) -> Result<(), TxError> {
        self.launched(attempt)?;
        let cause =
            jojo_domain::outcome::StopCause::Supervisor(crate::outcomes::supervisor_stop(reason));
        self.stop_approval_attempt(&id_to_text(attempt), cause, reason)?;
        self.record_process_observation(attempt, ObservationKind::StopRequested, reason)
    }

    /// Records a synced checkpoint and the events whose raw ranges it covers, in order.
    pub fn append_provider_output(
        &mut self,
        attempt: OpaqueId,
        stream: Stream,
        durable: u64,
        events: &[(u64, RawEvent)],
    ) -> Result<(), TxError> {
        self.launched(attempt)?;
        let id = id_to_text(attempt);
        let event = self.append_event(
            "ProviderOutput",
            None,
            &Record::new()
                .text("attempt", id.clone())
                .text("stream", stream_db(stream))
                .count("durable", durable)
                .count("events", events.len() as u64),
        )?;
        let seq = Value::Int(to_db(event.0)?);
        self.conn.execute(
            "INSERT OR IGNORE INTO stream_checkpoints VALUES (?1,?2,?3,?4)",
            &[
                Value::Text(&id),
                Value::Text(stream_db(stream)),
                Value::Int(to_db(durable)?),
                seq,
            ],
        )?;
        for (sequence, e) in events {
            self.conn.execute(
                "INSERT INTO provider_events VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                &[
                    Value::Text(&id),
                    Value::Int(to_db(*sequence)?),
                    Value::Text(&payload_kind(&e.payload)),
                    Value::Text(stream_db(e.stream)),
                    Value::Int(to_db(e.start)?),
                    Value::Int(to_db(e.end)?),
                    Value::Text(&provider_event(&e.payload).encode()),
                    Value::Int(to_db(event.0)?),
                ],
            )?;
        }
        Ok(())
    }
}

impl ReadView<'_> {
    pub fn process_observations(
        &self,
        attempt: OpaqueId,
    ) -> Result<Vec<(String, String)>, StorageError> {
        Ok(self.conn.query(
            "SELECT kind,detail FROM process_observations WHERE attempt=?1 ORDER BY event",
            &[Value::Text(&id_to_text(attempt))],
            |r| Ok((r.text(0)?, r.text(1)?)),
        )?)
    }

    pub fn provider_events(
        &self,
        attempt: OpaqueId,
    ) -> Result<Vec<StoredProviderEvent>, StorageError> {
        let rows = self.conn.query(
            "SELECT sequence,kind,stream,raw_start,raw_end,payload FROM provider_events
             WHERE attempt=?1 ORDER BY sequence",
            &[Value::Text(&id_to_text(attempt))],
            |r| {
                Ok((
                    r.int(0)?,
                    r.text(1)?,
                    r.text(2)?,
                    r.int(3)?,
                    r.int(4)?,
                    r.text(5)?,
                ))
            },
        )?;
        rows.into_iter()
            .map(|(sequence, kind, stream, start, end, payload)| {
                Ok(StoredProviderEvent {
                    sequence: from_db(sequence)?,
                    kind,
                    stream,
                    raw_start: from_db(start)?,
                    raw_end: from_db(end)?,
                    payload,
                })
            })
            .collect()
    }

    pub fn stream_checkpoint(
        &self,
        attempt: OpaqueId,
        stream: Stream,
    ) -> Result<u64, StorageError> {
        let max = self.conn.query_one(
            "SELECT coalesce(max(durable),0) FROM stream_checkpoints WHERE attempt=?1 AND stream=?2",
            &[Value::Text(&id_to_text(attempt)), Value::Text(stream_db(stream))],
            |r| r.int(0),
        )?;
        from_db(max)
    }

    pub fn process_records(&self, attempt: OpaqueId) -> Result<Vec<ProcessRecord>, StorageError> {
        let rows = self.conn.query(
            "SELECT pid,start_identity,pgid,role FROM process_records WHERE attempt=?1 ORDER BY event",
            &[Value::Text(&id_to_text(attempt))],
            |r| Ok((r.int(0)?, r.text(1)?, r.int(2)?, r.text(3)?)),
        )?;
        let n = |v: i64| u32::try_from(v).map_err(|_| StorageError::corrupt("pid range"));
        rows.into_iter()
            .map(|(pid, start, pgid, role)| {
                Ok(ProcessRecord {
                    pid: n(pid)?,
                    start,
                    pgid: n(pgid)?,
                    role,
                })
            })
            .collect()
    }

    pub fn process_launch_issued(&self, attempt: OpaqueId) -> Result<bool, StorageError> {
        Ok(self
            .conn
            .query_optional(
                "SELECT 1 FROM process_launches WHERE attempt=?1",
                &[Value::Text(&id_to_text(attempt))],
                |r| r.int(0),
            )?
            .is_some())
    }
}

/// Escalation budgets are injected policy, measured with the injected clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StopBudgets {
    pub interrupt: Duration,
    pub terminate: Duration,
    pub kill: Duration,
    /// After root exit, how long closed streams may take before an unknown holder is
    /// assumed (stop unconfirmed).
    pub drain: Duration,
}

impl StopBudgets {
    /// Accepted U05 defaults: 10 s graceful, 20 s TERM, 5 s KILL verification.
    pub const ACCEPTED: Self = Self {
        interrupt: Duration::from_secs(10),
        terminate: Duration::from_secs(20),
        kill: Duration::from_secs(5),
        drain: Duration::from_secs(5),
    };
}

/// Capability-specific escalation: INT only when turn interruption is supported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StopPlan {
    pub steps: Vec<(Signal, Duration)>,
    pub drain: Duration,
}

impl StopPlan {
    pub fn new(capabilities: &Capabilities, budgets: StopBudgets) -> Self {
        let mut steps = Vec::new();
        if capabilities.turn_interrupt == Support::Supported {
            steps.push((Signal::Interrupt, budgets.interrupt));
        }
        steps.push((Signal::Terminate, budgets.terminate));
        steps.push((Signal::Kill, budgets.kill));
        Self {
            steps,
            drain: budgets.drain,
        }
    }
}

pub type SpawnHook = Box<dyn FnMut(&ProcessIdentity) + Send>;
pub type EventHook = Box<dyn FnMut(&EventPayload) + Send>;

/// Test barriers and faults. Production passes `LaunchHooks::default()`.
#[derive(Default)]
pub struct LaunchHooks {
    /// P1-13 emergency fallback when a durable shutdown intent cannot commit. Only the
    /// retained runner observes this flag and signals its lifetime handle.
    pub emergency_stop: Option<Arc<AtomicBool>>,
    pub after_issue: Option<Box<dyn FnMut() + Send>>,
    pub after_spawn: Option<SpawnHook>,
    /// Called after each provider event has been durably appended.
    pub on_event: Option<EventHook>,
    pub spool: SpoolFaults,
    pub inspector: Option<std::path::PathBuf>,
    /// P2-04.1: the guard executable armed inside the agent's process group, so that the
    /// agent cannot outlive this process. Production: the helper's own executable.
    pub guard: Option<std::path::PathBuf>,
}

pub struct LaunchContext<'a> {
    pub engine: &'a Engine,
    pub ids: &'a dyn IdGenerator,
    pub clock: &'a dyn Clock,
    /// Control loop wake-up when no output arrives; bounds stop-request latency.
    pub poll: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchOutcome {
    /// Never issued: no ISSUED_UNKNOWN intent was committed.
    Refused(String),
    /// Issued, then proven not spawned; the intent is REJECTED.
    NotSpawned(String),
    /// Issued; spawn reported failure. Ambiguity retained for recovery.
    SpawnUncertain(String),
    /// Root reaped, owned group and known descendants absent, both streams closed.
    TreeAbsent { stop_requested: bool, forced: bool },
    /// Some owned or potentially owned process could not be confirmed stopped.
    StopUnconfirmed(Vec<String>),
}

#[derive(Clone, Debug)]
pub struct LaunchReport {
    pub outcome: LaunchOutcome,
    pub root: Option<ProcessIdentity>,
    pub exit: Option<ExitObservation>,
    pub descendants: Vec<ProcessIdentity>,
    pub signals: Vec<SignalRecord>,
    pub terminal: Option<TerminalStatus>,
    pub protocol_failed: bool,
    pub evidence_lost: bool,
    pub admission_closed: bool,
    pub events: u64,
}

#[derive(Debug)]
pub enum LaunchError {
    /// The issue command itself could not be committed; nothing was spawned.
    Issue(CommandError),
}

struct Run<'a, 'c> {
    ctx: &'c LaunchContext<'a>,
    attempt: OpaqueId,
    db_failed: bool,
    report: LaunchReport,
    reader: Option<Reader>,
    next_stop_check: u64,
    emergency_stop: Option<Arc<AtomicBool>>,
}

impl Run<'_, '_> {
    fn cmd<F>(&mut self, kind: &str, f: F) -> Result<String, CommandError>
    where
        F: FnOnce(&mut Tx<'_>) -> Result<String, TxError> + Send + 'static,
    {
        let id = self.ctx.ids.next_id().map_err(|_| {
            CommandError::Rejected(TxError::Rejected("command ids exhausted".into()))
        })?;
        let request = CommandRequest::new(
            CommandId(id),
            self.ctx.engine.epoch(),
            kind,
            id_to_text(self.attempt),
        );
        self.ctx.engine.execute(request, f).map(|r| r.response)
    }

    /// Best effort after the first failure: close admission once, then stop recording.
    /// The process is still stopped; the report says evidence was lost.
    fn record<F>(&mut self, kind: &str, f: F)
    where
        F: FnOnce(&mut Tx<'_>) -> Result<String, TxError> + Send + 'static,
    {
        if self.db_failed {
            return;
        }
        if self.cmd(kind, f).is_err() {
            self.db_failed = true;
            self.report.evidence_lost = true;
            let closed = self.cmd("CloseAdmissionAfterWriteFailure", |tx| {
                tx.set_admission(Admission::Closed)?;
                Ok("closed".into())
            });
            self.report.admission_closed = closed.is_ok();
        }
    }

    fn observe(&mut self, kind: ObservationKind, detail: String) {
        let a = self.attempt;
        self.record("RecordProcessObservation", move |tx| {
            tx.record_process_observation(a, kind, &detail)?;
            Ok("observed".into())
        });
    }

    fn now(&self) -> u64 {
        self.ctx.clock.now().monotonic_nanos
    }

    /// Durable stop intent committed by anyone (user cancel, approval revocation). Checked
    /// at most once per poll interval on one cached reader; unreadable means stop.
    fn external_stop(&mut self) -> Option<String> {
        if self
            .emergency_stop
            .as_ref()
            .is_some_and(|s| s.load(Ordering::SeqCst))
        {
            return Some("shutdown persistence failure: emergency stop".into());
        }
        let now = self.now();
        if now < self.next_stop_check {
            return None;
        }
        self.next_stop_check = now.saturating_add(nanos(self.ctx.poll));
        if self.reader.is_none() {
            self.reader = self.ctx.engine.reader().ok();
        }
        let attempt = self.attempt;
        let stopped = match self.reader.as_mut() {
            Some(r) => r.read(|v| v.approval_stop_requested(attempt)),
            None => return Some("stop state unreadable".into()),
        };
        match stopped {
            Ok(false) => None,
            Ok(true) => Some("stop requested".into()),
            Err(_) => {
                self.reader = None;
                Some("stop state unreadable".into())
            }
        }
    }
}

fn nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

/// Runs one admitted attempt from issue to a concluded process observation.
pub fn run_launch(
    ctx: &LaunchContext<'_>,
    spec: &LaunchSpec,
    launch: &ProcessLaunch,
    plan: &StopPlan,
    mut hooks: LaunchHooks,
) -> Result<LaunchReport, LaunchError> {
    let attempt = spec.attempt.0;
    let mut run = Run {
        ctx,
        attempt,
        db_failed: false,
        reader: None,
        next_stop_check: 0,
        emergency_stop: hooks.emergency_stop.clone(),
        report: LaunchReport {
            outcome: LaunchOutcome::Refused(String::new()),
            root: None,
            exit: None,
            descendants: Vec::new(),
            signals: Vec::new(),
            terminal: None,
            protocol_failed: false,
            evidence_lost: false,
            admission_closed: false,
            events: 0,
        },
    };
    // Reconstruct from the frozen DTO: the journal must describe the executable,
    // limits and stream paths that actually cross the OS boundary.
    if ProcessLaunch::from_spec(spec, launch.args.clone(), launch.environment.clone()).as_ref()
        != Ok(launch)
    {
        run.report.outcome =
            LaunchOutcome::Refused("process request differs from frozen LaunchSpec".into());
        return Ok(run.report);
    }
    if run
        .emergency_stop
        .as_ref()
        .is_some_and(|s| s.load(Ordering::SeqCst))
    {
        run.report.outcome = LaunchOutcome::Refused("emergency stop before issue".into());
        return Ok(run.report);
    }
    let issued_spec = spec.clone();
    let issued = run.cmd("IssueProcessLaunch", move |tx| {
        Ok(if tx.issue_process_launch(&issued_spec)? {
            "issued".into()
        } else {
            "stopped or no longer reserved".into()
        })
    });
    match issued {
        Ok(s) if s == "issued" => {}
        Ok(reason) | Err(CommandError::Rejected(TxError::Rejected(reason))) => {
            run.report.outcome = LaunchOutcome::Refused(reason);
            return Ok(run.report);
        }
        Err(e) => return Err(LaunchError::Issue(e)),
    }
    if let Some(h) = hooks.after_issue.as_mut() {
        h();
    }
    let stopped = ctx
        .engine
        .reader()
        .and_then(|mut r| r.read(|v| v.approval_stop_requested(attempt)));
    if !matches!(stopped, Ok(false))
        || run
            .emergency_stop
            .as_ref()
            .is_some_and(|s| s.load(Ordering::SeqCst))
    {
        let reason = "stop observed after issue, before spawn".to_owned();
        let r = reason.clone();
        run.record("RecordNotSpawned", move |tx| {
            tx.record_not_spawned(attempt, &r)?;
            Ok("not spawned".into())
        });
        run.report.outcome = LaunchOutcome::NotSpawned(reason);
        return Ok(run.report);
    }
    let mut sup = match Supervised::spawn_guarded(
        launch,
        hooks.spool,
        hooks
            .inspector
            .as_deref()
            .unwrap_or(std::path::Path::new("/bin/ps")),
        hooks.guard.as_deref(),
    ) {
        Ok(s) => s,
        Err(SpawnFailure::BeforeChild(d)) => {
            let r = d.clone();
            run.record("RecordNotSpawned", move |tx| {
                tx.record_not_spawned(attempt, &r)?;
                Ok("not spawned".into())
            });
            run.report.outcome = LaunchOutcome::NotSpawned(d);
            return Ok(run.report);
        }
        Err(SpawnFailure::Uncertain(d)) => {
            run.observe(ObservationKind::SpawnFailed, d.clone());
            run.report.outcome = LaunchOutcome::SpawnUncertain(d);
            return Ok(run.report);
        }
    };
    let root = sup.root().clone();
    run.report.root = Some(root.clone());
    if let Some(h) = hooks.after_spawn.as_mut() {
        h(&root);
    }
    let registered = root.clone();
    let mut stop_requested = run
        .cmd("RegisterProcessRoot", move |tx| {
            Ok(tx.register_process_root(attempt, &registered)?.to_string())
        })
        .map(|s| s == "true");
    if stop_requested.is_err() {
        run.db_failed = true;
        run.report.evidence_lost = true;
        run.report.admission_closed = run
            .cmd("CloseAdmissionAfterWriteFailure", |tx| {
                tx.set_admission(Admission::Closed)?;
                Ok("closed".into())
            })
            .is_ok();
        stop_requested = Ok(true);
    }
    supervise(
        &mut run,
        &mut sup,
        plan,
        stop_requested.unwrap_or(true),
        run_deadline(ctx.clock, spec.timeout_millis),
        &mut hooks,
    );
    run.report.descendants = sup.known_descendants().to_vec();
    Ok(run.report)
}

fn run_deadline(clock: &dyn Clock, millis: u64) -> u64 {
    clock
        .now()
        .monotonic_nanos
        .saturating_add(millis.saturating_mul(1_000_000))
}

struct Stopping {
    step: usize,
    deadline: u64,
}

fn supervise(
    run: &mut Run<'_, '_>,
    sup: &mut Supervised,
    plan: &StopPlan,
    pending: bool,
    execution_deadline: u64,
    hooks: &mut LaunchHooks,
) {
    let attempt = run.attempt;
    let mut sequence = 0u64;
    let mut closed = [false, false];
    let mut stop_requested = false;
    let mut root_terminated = false;
    let mut inspection_failed = None;
    let mut stopping: Option<Stopping> = None;
    let mut drain_deadline: Option<u64> = None;
    let mut stop_reason: Option<String> = pending.then(|| "stop pending at registration".into());
    if let Some(error) = discover(run, sup) {
        inspection_failed = Some(error.clone());
        stop_reason = Some(error);
    }
    loop {
        let message = sup.messages().recv_timeout(run.ctx.poll);
        match message {
            Ok(ProcessMessage::Output {
                stream,
                durable,
                events,
            }) => {
                let numbered: Vec<_> = events
                    .into_iter()
                    .map(|e| {
                        sequence += 1;
                        (sequence, e)
                    })
                    .collect();
                for (_, e) in &numbered {
                    match &e.payload {
                        EventPayload::ProviderTerminal { status, .. }
                            if run.report.terminal.is_none() =>
                        {
                            run.report.terminal = Some(*status);
                        }
                        EventPayload::ProtocolViolation(why) if e.stream == Stream::Stdout => {
                            if !run.report.protocol_failed {
                                stop_reason.get_or_insert_with(|| {
                                    format!("{}{why}", crate::outcomes::PROTOCOL_STOP_PREFIX)
                                });
                            }
                            run.report.protocol_failed = true;
                        }
                        _ => {}
                    }
                }
                run.report.events += numbered.len() as u64;
                let payloads: Vec<_> = numbered.iter().map(|(_, e)| e.payload.clone()).collect();
                run.record("AppendProviderOutput", move |tx| {
                    tx.append_provider_output(attempt, stream, durable, &numbered)?;
                    Ok("appended".into())
                });
                if let Some(h) = hooks.on_event.as_mut() {
                    payloads.iter().for_each(h);
                }
            }
            Ok(ProcessMessage::Truncated { stream, at }) => {
                run.observe(
                    ObservationKind::StreamTruncated,
                    format!("{}@{at}", stream_db(stream)),
                );
            }
            Ok(ProcessMessage::Closed {
                stream,
                durable,
                discarded,
            }) => {
                closed[usize::from(stream == Stream::Stderr)] = true;
                run.observe(
                    ObservationKind::StreamClosed,
                    crate::outcomes::stream_closed_detail(stream_db(stream), durable, discarded),
                );
            }
            Ok(ProcessMessage::SpoolFailed { stream, detail }) => {
                run.report.evidence_lost = true;
                let detail = format!("{}: {detail}", stream_db(stream));
                let d = detail.clone();
                run.record("CloseAdmissionAfterEvidenceLoss", move |tx| {
                    tx.set_admission(Admission::Closed)?;
                    tx.record_process_observation(attempt, ObservationKind::EvidenceLoss, &d)?;
                    Ok("closed".into())
                });
                run.report.admission_closed |= !run.db_failed;
                stop_reason.get_or_insert_with(|| {
                    format!("{}{detail}", crate::outcomes::EVIDENCE_LOSS_STOP_PREFIX)
                });
            }
            Ok(ProcessMessage::RootTerminated) => {
                root_terminated = true;
                run.observe(
                    ObservationKind::RootExited,
                    "root terminated; wait status pending while group lifetime is pinned".into(),
                );
            }
            Ok(ProcessMessage::InspectionFailed(detail)) => {
                inspection_failed = Some(detail.clone());
                stop_reason.get_or_insert_with(|| format!("process inspection failed: {detail}"));
            }
            Ok(ProcessMessage::Exited(exit)) if run.report.exit.is_none() => {
                run.report.exit = Some(exit);
                run.observe(
                    ObservationKind::RootExited,
                    crate::outcomes::exit_detail(exit.code, exit.signal),
                );
            }
            Ok(ProcessMessage::Exited(_)) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => closed = [true, true],
        }
        if run.report.protocol_failed && !stop_requested {
            run.observe(
                ObservationKind::ProtocolFailure,
                "stdout protocol failure".into(),
            );
        }
        if stopping.is_none() && stop_reason.is_none() {
            stop_reason = if run.now() >= execution_deadline {
                Some(crate::outcomes::DEADLINE_STOP.into())
            } else {
                run.external_stop()
            };
        }
        if stopping.is_none()
            && let Some(reason) = stop_reason.clone()
        {
            stop_requested = true;
            let r = reason.clone();
            run.record("RequestProcessStop", move |tx| {
                tx.request_process_stop(attempt, &r)?;
                Ok("stop".into())
            });
            stopping = Some(Stopping {
                step: 0,
                deadline: 0,
            });
        }
        // The waiter updates its slot before sending the bounded-channel message.
        // Capture that observation even if RootTerminated was dequeued just before
        // Exited, so TREE_ABSENT can never return without its actual wait status.
        if run.report.exit.is_none()
            && let Some(exit) = sup.root_exit()
        {
            run.report.exit = Some(exit);
            run.observe(
                ObservationKind::RootExited,
                crate::outcomes::exit_detail(exit.code, exit.signal),
            );
        }
        let exited = run.report.exit.is_some();
        let root_stopped = root_terminated || exited;
        let streams_closed = closed == [true, true];
        if let Some(s) = stopping.as_mut() {
            if let Some(detail) = &inspection_failed
                && s.step > 0
                && streams_closed
            {
                let reason = format!("inspection failed: {detail}");
                run.observe(ObservationKind::StopUnconfirmed, reason.clone());
                run.report.outcome = LaunchOutcome::StopUnconfirmed(vec![reason]);
                return;
            }
            if exited && streams_closed && sup.tree().absent() {
                let forced = run
                    .report
                    .signals
                    .iter()
                    .any(|r| r.signal == Signal::Kill && r.outcome == SignalOutcome::Sent);
                run.observe(
                    ObservationKind::TreeAbsent,
                    if forced {
                        "graceful stop failed; forced stop confirmed".into()
                    } else {
                        "stopped within graceful budget".into()
                    },
                );
                run.report.outcome = LaunchOutcome::TreeAbsent {
                    stop_requested,
                    forced,
                };
                return;
            }
            if run.now() >= s.deadline {
                let Some((signal, budget)) = plan.steps.get(s.step).copied() else {
                    let tree = sup.tree();
                    let mut reasons = Vec::new();
                    if !exited {
                        reasons.push("root exit not observed".to_owned());
                    }
                    if !tree.live_group.is_empty() {
                        reasons.push(format!("group members alive: {:?}", tree.live_group));
                    }
                    if !tree.live_known.is_empty() {
                        reasons.push(format!("known descendants alive: {:?}", tree.live_known));
                    }
                    if !streams_closed {
                        reasons.push("output stream held open by an unknown process".into());
                    }
                    if let Some(e) = tree.error {
                        reasons.push(format!("inspection failed: {e}"));
                    }
                    run.observe(ObservationKind::StopUnconfirmed, reasons.join("; "));
                    run.report.outcome = LaunchOutcome::StopUnconfirmed(reasons);
                    return;
                };
                if let Some(error) = discover(run, sup) {
                    inspection_failed = Some(error);
                }
                for record in sup.signal_tree(signal) {
                    let kind = match record.outcome {
                        SignalOutcome::Sent => Some(ObservationKind::SignalSent),
                        SignalOutcome::Absent => None,
                        _ => Some(ObservationKind::SignalRefused),
                    };
                    if let Some(kind) = kind {
                        run.observe(
                            kind,
                            format!(
                                "{} {} {:?}",
                                record.signal.name(),
                                record.target,
                                record.outcome
                            ),
                        );
                    }
                    run.report.signals.push(record);
                }
                s.step += 1;
                s.deadline = run.now().saturating_add(nanos(budget));
            }
            continue;
        }
        if root_stopped {
            if drain_deadline.is_none() {
                if let Some(error) = discover(run, sup) {
                    inspection_failed = Some(error.clone());
                    stop_reason = Some(error);
                    continue;
                }
                let tree = sup.tree();
                if !tree.live_group.is_empty() || !tree.live_known.is_empty() {
                    run.observe(
                        ObservationKind::DescendantsAlive,
                        format!("group={:?} known={:?}", tree.live_group, tree.live_known),
                    );
                    stop_reason = Some(crate::outcomes::ROOT_DESCENDANTS_STOP.into());
                    continue;
                }
                drain_deadline = Some(run.now().saturating_add(nanos(plan.drain)));
            }
            if streams_closed {
                let tree = sup.tree();
                if tree.absent() {
                    if !crate::process::may_conclude_absent(exited, drain_deadline, run.now()) {
                        continue;
                    }
                    run.observe(ObservationKind::TreeAbsent, "exited without stop".into());
                    run.report.outcome = LaunchOutcome::TreeAbsent {
                        stop_requested: false,
                        forced: false,
                    };
                    return;
                }
                if exited {
                    stop_reason = Some(crate::outcomes::LATE_DESCENDANTS_STOP.into());
                }
            } else if drain_deadline.is_some_and(|d| run.now() >= d) {
                let reason = "output stream held open by an unknown process".to_owned();
                run.observe(ObservationKind::StopUnconfirmed, reason.clone());
                run.report.outcome = LaunchOutcome::StopUnconfirmed(vec![reason]);
                return;
            }
        }
    }
}

fn discover(run: &mut Run<'_, '_>, sup: &mut Supervised) -> Option<String> {
    let attempt = run.attempt;
    match sup.discover() {
        Ok(found) if !found.is_empty() => run.record("RecordProcessDescendants", move |tx| {
            tx.record_process_descendants(attempt, &found)?;
            Ok("descendants".into())
        }),
        Ok(_) => {}
        Err(e) => {
            run.observe(
                ObservationKind::SignalRefused,
                format!("inspection failed: {e}"),
            );
            return Some(e);
        }
    }
    None
}

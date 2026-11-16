//! Attempt settlement, immutable results and the attempt effect journal (P1-10).
//!
//! Order for one concluded attempt, each database step its own writer transaction:
//! `begin_attempt_settlement` (proven `TREE_ABSENT` releases only the private worktree
//! writer claim) → collect/record the candidate and publish raw spools as verified blobs
//! *outside any transaction* → `finalize_attempt_result`, which re-reads every durable
//! fact, classifies with the pure ordered rules and seals at most one result per attempt.
//!
//! Classification never trusts in-memory supervisor reports or provider prose: it uses
//! committed observations, checkpoints, provider events, stop causes, evidence rows and
//! effect states. Unproven process absence, lost coverage, unknown stop ordering or an
//! unresolved effect leaves an append-only recovery hold instead of a result.
use std::collections::BTreeSet;
use std::fs;

use jojo_domain::attempt::AttemptOutcome;
use jojo_domain::identity::{ArtifactId, IdGenerator, OpaqueId, Revision};
use jojo_domain::outcome::{
    CandidateEvidence, Classification, Coverage, ExitStatus, ProcessConclusion, SettlementFacts,
    StopCause, SupervisorStop, classify,
};
use jojo_domain::provider::{Stream, TerminalStatus};
use jojo_domain::validation::CANDIDATE_OUTPUT;

use crate::evidence::{BlobFaults, BlobRef, BlobStore};
use crate::model::{from_db, id_to_text, to_db};
use crate::sqlite::{Connection, Value};
use crate::{
    AggregateKey, CommandError, CommandId, CommandRequest, EffectState, Engine, EventSeq, ReadView,
    ReleaseBasis, StorageError, Tx, TxError, sha256_hex,
};

pub(crate) const PROTOCOL_STOP_PREFIX: &str = "protocol: ";
pub(crate) const EVIDENCE_LOSS_STOP_PREFIX: &str = "evidence loss: ";
pub(crate) const DEADLINE_STOP: &str = "execution deadline exceeded";
pub(crate) const ROOT_DESCENDANTS_STOP: &str = "root exited with live descendants";
pub(crate) const LATE_DESCENDANTS_STOP: &str = "descendants appeared after root exit";
const EFFECT_PREFIX: &str = "attempt-effect:";

fn deny(s: impl Into<String>) -> TxError {
    TxError::Rejected(s.into())
}

/// Maps a supervisor stop reason (always one of the constants above or free text).
pub(crate) fn supervisor_stop(reason: &str) -> SupervisorStop {
    if reason.starts_with(PROTOCOL_STOP_PREFIX) {
        SupervisorStop::Protocol
    } else if reason == DEADLINE_STOP {
        SupervisorStop::Deadline
    } else if reason.starts_with(EVIDENCE_LOSS_STOP_PREFIX) {
        SupervisorStop::EvidenceLoss
    } else if reason == ROOT_DESCENDANTS_STOP || reason == LATE_DESCENDANTS_STOP {
        SupervisorStop::Descendants
    } else {
        SupervisorStop::Other
    }
}

fn cause_db(c: StopCause) -> &'static str {
    match c {
        StopCause::TaskCancel => "TASK_CANCEL",
        StopCause::ApprovalInvalidated => "APPROVAL_INVALIDATED",
        StopCause::Shutdown => "SHUTDOWN",
        StopCause::Supervisor(SupervisorStop::Protocol) => "SUPERVISOR_PROTOCOL",
        StopCause::Supervisor(SupervisorStop::Deadline) => "SUPERVISOR_DEADLINE",
        StopCause::Supervisor(SupervisorStop::EvidenceLoss) => "SUPERVISOR_EVIDENCE_LOSS",
        StopCause::Supervisor(SupervisorStop::Descendants) => "SUPERVISOR_DESCENDANTS",
        StopCause::Supervisor(SupervisorStop::Other) => "SUPERVISOR_OTHER",
    }
}

fn cause_from_db(s: &str) -> Option<StopCause> {
    Some(match s {
        "TASK_CANCEL" => StopCause::TaskCancel,
        "APPROVAL_INVALIDATED" => StopCause::ApprovalInvalidated,
        "SHUTDOWN" => StopCause::Shutdown,
        "SUPERVISOR_PROTOCOL" => StopCause::Supervisor(SupervisorStop::Protocol),
        "SUPERVISOR_DEADLINE" => StopCause::Supervisor(SupervisorStop::Deadline),
        "SUPERVISOR_EVIDENCE_LOSS" => StopCause::Supervisor(SupervisorStop::EvidenceLoss),
        "SUPERVISOR_DESCENDANTS" => StopCause::Supervisor(SupervisorStop::Descendants),
        "SUPERVISOR_OTHER" => StopCause::Supervisor(SupervisorStop::Other),
        _ => return None,
    })
}

fn outcome_db(o: AttemptOutcome) -> &'static str {
    match o {
        AttemptOutcome::Succeeded => "SUCCEEDED",
        AttemptOutcome::Failed => "FAILED",
        AttemptOutcome::Cancelled => "CANCELLED",
        AttemptOutcome::Interrupted => "INTERRUPTED",
        AttemptOutcome::NotStarted => "NOT_STARTED",
    }
}

fn outcome_from_db(s: &str) -> Result<AttemptOutcome, StorageError> {
    Ok(match s {
        "SUCCEEDED" => AttemptOutcome::Succeeded,
        "FAILED" => AttemptOutcome::Failed,
        "CANCELLED" => AttemptOutcome::Cancelled,
        "INTERRUPTED" => AttemptOutcome::Interrupted,
        other => return Err(StorageError::corrupt(format!("unknown outcome {other:?}"))),
    })
}

fn stream_kind(s: Stream) -> &'static str {
    match s {
        Stream::Stdout => "STDOUT",
        Stream::Stderr => "STDERR",
    }
}

/// Status of a stored `ProviderTerminal` payload (its derived `Debug` text). Anything
/// that does not parse exactly is `Unknown`, which can never produce success.
pub(crate) fn terminal_status(payload: &str) -> TerminalStatus {
    let Some(rest) = payload.strip_prefix("ProviderTerminal { turn: \"") else {
        return TerminalStatus::Unknown;
    };
    let mut escaped = false;
    let mut end = None;
    for (i, c) in rest.char_indices() {
        match (escaped, c) {
            (true, _) => escaped = false,
            (false, '\\') => escaped = true,
            (false, '"') => {
                end = Some(i);
                break;
            }
            _ => {}
        }
    }
    let status = end
        .and_then(|i| rest[i + 1..].strip_prefix(", status: "))
        .and_then(|s| s.split(',').next());
    match status {
        Some("Success") => TerminalStatus::Success,
        Some("Error") => TerminalStatus::Error,
        Some("Interrupted") => TerminalStatus::Interrupted,
        _ => TerminalStatus::Unknown,
    }
}

fn exit_status(detail: &str) -> Option<ExitStatus> {
    fn opt(t: &str) -> Option<Option<i32>> {
        if t == "None" {
            return Some(None);
        }
        t.strip_prefix("Some(")?
            .strip_suffix(')')?
            .parse()
            .ok()
            .map(Some)
    }
    let (code, signal) = detail.strip_prefix("code=")?.split_once(" signal=")?;
    Some(ExitStatus {
        code: opt(code)?,
        signal: opt(signal)?,
    })
}

/// `STDOUT durable=N discarded=M` as recorded at stream close.
fn stream_closed(detail: &str) -> Option<(String, u64, u64)> {
    let (stream, rest) = detail.split_once(" durable=")?;
    let (durable, discarded) = rest.split_once(" discarded=")?;
    Some((
        stream.to_owned(),
        durable.parse().ok()?,
        discarded.parse().ok()?,
    ))
}

/// Evidence verified on disk by [`prepare_settlement`]. Only that function constructs it;
/// finalization refuses it if the referenced evidence changed afterwards.
#[derive(Clone, Debug)]
pub struct PreparedSettlement {
    attempt: OpaqueId,
    evidence: BTreeSet<(String, String)>,
    failures: Vec<String>,
    candidate_note: Option<String>,
}

impl PreparedSettlement {
    pub fn attempt(&self) -> OpaqueId {
        self.attempt
    }
    pub fn failures(&self) -> &[String] {
        &self.failures
    }
    pub fn candidate_note(&self) -> Option<&str> {
        self.candidate_note.as_deref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttemptResult {
    pub attempt: OpaqueId,
    pub outcome: AttemptOutcome,
    pub reason: String,
    pub stop_cause: Option<String>,
    pub exit_code: Option<i32>,
    pub exit_signal: Option<i32>,
    pub terminal: String,
    pub candidate: Option<String>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub uncertainties: Vec<String>,
    pub facts_digest: String,
    pub event: EventSeq,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Settlement {
    Sealed(Box<AttemptResult>),
    RecoveryRequired(Vec<String>),
}

/// Phase 1 remote facts are fakes only; no remote mutation adapter exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteObservation {
    Committed,
    Rejected,
    Unknown,
}

/// Claims an earlier helper left QUARANTINED are released only on recovery evidence.
fn release_basis(recovered: bool) -> ReleaseBasis {
    if recovered {
        ReleaseBasis::RecoveryResolved
    } else {
        ReleaseBasis::HolderSettled
    }
}

fn rows(c: &Connection, sql: &str, id: &str) -> Result<Vec<(String, String)>, StorageError> {
    Ok(c.query(sql, &[Value::Text(id)], |r| Ok((r.text(0)?, r.text(1)?)))?)
}

struct Gathered {
    facts: SettlementFacts,
    exit: Option<ExitStatus>,
    stop_cause: Option<String>,
    evidence: Vec<(String, String)>,
}

fn launch_epoch(c: &Connection, id: &str) -> Result<Option<u64>, StorageError> {
    c.query_optional(
        "SELECT epoch FROM process_launches WHERE attempt=?1",
        &[Value::Text(id)],
        |r| r.int(0),
    )?
    .map(from_db)
    .transpose()
}

fn gather(c: &Connection, id: &str, prepared: &PreparedSettlement) -> Result<Gathered, TxError> {
    let obs = rows(
        c,
        "SELECT kind,detail FROM process_observations WHERE attempt=?1 ORDER BY event",
        id,
    )?;
    let has = |k: &str| obs.iter().any(|(kind, _)| kind == k);
    // Supervised conclusions stay authoritative; a recovery proof only replaces a
    // supervision the lost helper never concluded.
    let conclusion = if has("NOT_SPAWNED") {
        ProcessConclusion::NotSpawned
    } else if has("TREE_ABSENT") {
        ProcessConclusion::TreeAbsent
    } else if crate::recovery::recovered_absent(c, id)? {
        ProcessConclusion::RecoveredAbsent
    } else if has("STOP_UNCONFIRMED") {
        ProcessConclusion::StopUnconfirmed
    } else if has("SPAWN_FAILED") {
        ProcessConclusion::SpawnUncertain
    } else {
        ProcessConclusion::Unconcluded
    };
    let stopped = c
        .query_optional(
            "SELECT 1 FROM approval_stops WHERE attempt=?1",
            &[Value::Text(id)],
            |r| r.int(0),
        )?
        .is_some();
    let cause = c.query_optional(
        "SELECT cause,detail FROM attempt_stop_causes WHERE attempt=?1",
        &[Value::Text(id)],
        |r| Ok((r.text(0)?, r.text(1)?)),
    )?;
    let stop = cause.as_ref().and_then(|(c, _)| cause_from_db(c));
    let events = rows(
        c,
        "SELECT kind,stream||'|'||payload FROM provider_events WHERE attempt=?1 ORDER BY sequence",
        id,
    )?;
    let terminals = events
        .iter()
        .filter(|(k, _)| k == "ProviderTerminal")
        .map(|(_, p)| terminal_status(p.split_once('|').map_or("", |(_, p)| p)))
        .collect();
    let protocol_failed = has("PROTOCOL_FAILURE")
        || events
            .iter()
            .any(|(k, p)| k == "ProtocolViolation" && p.starts_with("STDOUT|"));
    let evidence = rows(
        c,
        "SELECT kind,digest FROM attempt_evidence WHERE attempt=?1 ORDER BY kind",
        id,
    )?;
    let mut lost: Vec<String> = obs
        .iter()
        .filter(|(k, _)| k == "EVIDENCE_LOSS")
        .map(|(_, d)| d.clone())
        .collect();
    lost.extend(prepared.failures.iter().cloned());
    let mut truncated: Vec<String> = obs
        .iter()
        .filter(|(k, _)| k == "STREAM_TRUNCATED")
        .map(|(_, d)| d.clone())
        .collect();
    if events.iter().any(|(k, _)| k == "StreamTruncated") && truncated.is_empty() {
        truncated.push("parser recorded a truncated stream".into());
    }
    let mut candidate = CandidateEvidence::NotApplicable;
    if matches!(
        conclusion,
        ProcessConclusion::TreeAbsent | ProcessConclusion::RecoveredAbsent
    ) {
        for stream in [Stream::Stdout, Stream::Stderr] {
            let name = stream_kind(stream);
            let closed: Vec<_> = obs
                .iter()
                .filter(|(k, _)| k == "STREAM_CLOSED")
                .filter_map(|(_, d)| stream_closed(d))
                .filter(|(s, _, _)| s == name)
                .collect();
            let durable = from_db(c.query_one(
                "SELECT coalesce(max(durable),0) FROM stream_checkpoints WHERE attempt=?1 AND stream=?2",
                &[Value::Text(id), Value::Text(name)],
                |r| r.int(0),
            )?)?;
            match closed.as_slice() {
                [(_, d, discarded)] if *d == durable => {
                    if *discarded > 0 {
                        truncated.push(format!("{name} discarded {discarded} bytes"));
                    }
                }
                _ => lost.push(format!("{name} close/checkpoint coverage mismatch")),
            }
            if !evidence.iter().any(|(k, _)| k == name) {
                lost.push(format!("{name} log evidence not durably referenced"));
            }
        }
        candidate = match evidence.iter().find(|(k, _)| k == "CANDIDATE") {
            Some((_, d)) => CandidateEvidence::Recorded(d.clone()),
            None => CandidateEvidence::Unavailable(
                prepared
                    .candidate_note
                    .clone()
                    .unwrap_or_else(|| "candidate not collected".into()),
            ),
        };
    }
    let coverage = if let Some(first) = lost.first() {
        Coverage::Lost(if lost.len() > 1 {
            lost.join("; ")
        } else {
            first.clone()
        })
    } else if !truncated.is_empty() {
        Coverage::Truncated(truncated.join("; "))
    } else {
        Coverage::Complete
    };
    let exit = obs
        .iter()
        .filter(|(k, _)| k == "ROOT_EXITED")
        .find_map(|(_, d)| exit_status(d));
    let effects = rows(
        c,
        "SELECT f.operation_key,o.state FROM attempt_effects f JOIN outbox o
         ON o.operation_key=f.operation_key WHERE f.attempt=?1 ORDER BY f.operation_key",
        id,
    )?;
    let effect_keys = |state: &str| -> Vec<String> {
        effects
            .iter()
            .filter(|(_, s)| s == state)
            .map(|(k, _)| k.clone())
            .collect()
    };
    let mut missing_outputs = Vec::new();
    if let Some((plan, revision)) = c.query_optional(
        "SELECT plan,revision FROM dependency_attempt_inputs WHERE attempt=?1",
        &[Value::Text(id)],
        |r| Ok((r.text(0)?, r.int(1)?)),
    )? {
        let stored = crate::dependencies::load_plan(
            c,
            ArtifactId(crate::model::id_from_text(&plan)?),
            Revision(from_db(revision)?),
        )?
        .ok_or_else(|| StorageError::corrupt("attempt plan missing"))?;
        missing_outputs = stored
            .plan
            .required_outputs
            .into_iter()
            .filter(|o| o != CANDIDATE_OUTPUT)
            .collect();
    }
    Ok(Gathered {
        facts: SettlementFacts {
            conclusion,
            stop,
            stop_cause_unknown: stopped && stop.is_none(),
            terminals,
            exit,
            protocol_failed,
            coverage,
            candidate,
            unresolved_effects: effect_keys("ISSUED_UNKNOWN"),
            abandoned_effects: effect_keys("NOT_ISSUED"),
            missing_outputs,
        },
        exit,
        stop_cause: cause.map(|(c, d)| format!("{c}: {d}")),
        evidence,
    })
}

impl Tx<'_> {
    /// First stop cause wins; nothing is recorded once a result is sealed, so a stop
    /// committed after finalization cannot reorder history.
    pub(crate) fn record_stop_cause(
        &mut self,
        id: &str,
        cause: StopCause,
        detail: &str,
    ) -> Result<(), TxError> {
        let known = self.conn.query_one(
            "SELECT EXISTS(SELECT 1 FROM attempt_stop_causes WHERE attempt=?1)
                 OR EXISTS(SELECT 1 FROM attempt_results WHERE attempt=?1)",
            &[Value::Text(id)],
            |r| r.int(0),
        )?;
        if known != 0 {
            return Ok(());
        }
        let event = self.append_event(
            "AttemptStopCause",
            None,
            &format!("{id}|{}", cause_db(cause)),
        )?;
        self.conn.execute(
            "INSERT INTO attempt_stop_causes VALUES (?1,?2,?3,?4)",
            &[
                Value::Text(id),
                Value::Text(cause_db(cause)),
                Value::Text(detail),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(())
    }

    /// Stop intent from the close/shutdown coordinator: classified INTERRUPTED, never a
    /// permanent task cancellation.
    pub fn request_shutdown_stop(&mut self, attempt: OpaqueId) -> Result<(), TxError> {
        self.stop_approval_attempt(
            &id_to_text(attempt),
            StopCause::Shutdown,
            "application shutdown",
        )
    }

    /// A current-epoch launch, or an earlier one whose absence this epoch's recovery
    /// inspection proved (returns `true`). Effects never use this relaxation.
    fn settleable_launch(&self, id: &str) -> Result<bool, TxError> {
        if launch_epoch(self.conn, id)?.is_some()
            && crate::recovery::recovered_absent(self.conn, id)?
        {
            return Ok(true);
        }
        self.current_launch(id).map(|()| false)
    }

    fn current_launch(&self, id: &str) -> Result<(), TxError> {
        match launch_epoch(self.conn, id)? {
            None => Err(deny("no issued launch")),
            Some(e) if e != self.epoch.0 => Err(deny(
                "attempt belongs to an earlier engine epoch; recovery inspection required",
            )),
            Some(_) => Ok(()),
        }
    }

    fn sealed(&self, id: &str) -> Result<bool, TxError> {
        Ok(self
            .conn
            .query_optional(
                "SELECT 1 FROM attempt_results WHERE attempt=?1",
                &[Value::Text(id)],
                |r| r.int(0),
            )?
            .is_some())
    }

    /// Proven tree absence lets the engine read the private worktree: only that writer
    /// claim is released; every other claim stays until the result is sealed.
    pub fn begin_attempt_settlement(&mut self, attempt: OpaqueId) -> Result<(), TxError> {
        let id = id_to_text(attempt);
        let recovered = self.settleable_launch(&id)?;
        if self.sealed(&id)? {
            return Err(deny("attempt already finalized"));
        }
        let absent = self.conn.query_optional(
            "SELECT 1 FROM process_observations WHERE attempt=?1 AND kind='TREE_ABSENT'",
            &[Value::Text(&id)],
            |r| r.int(0),
        )?;
        if absent.is_none() && !recovered {
            return Err(deny("process tree absence is not proven"));
        }
        let holder = AggregateKey::new("scheduler_attempt", attempt);
        for claim in self.live_claims(&format!("worktree:{id}"))? {
            if claim.holder == holder {
                self.release_claim(claim.id, release_basis(recovered))?;
            }
        }
        self.append_event("AttemptSettling", None, &id)?;
        Ok(())
    }

    /// References verified bytes as one attempt evidence kind (SQL checks coverage).
    pub fn record_attempt_evidence(
        &mut self,
        attempt: OpaqueId,
        kind: &str,
        blob: &BlobRef,
    ) -> Result<(), TxError> {
        let id = id_to_text(attempt);
        self.settleable_launch(&id)?;
        self.reference_blob(blob)?;
        let event = self.append_event(
            "AttemptEvidenceReferenced",
            None,
            &format!("{id}|{kind}|{}", blob.digest),
        )?;
        self.conn.execute(
            "INSERT INTO attempt_evidence VALUES (?1,?2,?3,?4)",
            &[
                Value::Text(&id),
                Value::Text(kind),
                Value::Text(&blob.digest),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(())
    }

    /// Seals the one immutable result, or appends a recovery hold. Returns the outcome
    /// name or `RECOVERY_REQUIRED: reasons`.
    pub fn finalize_attempt_result(
        &mut self,
        prepared: &PreparedSettlement,
    ) -> Result<String, TxError> {
        let attempt = prepared.attempt;
        let id = id_to_text(attempt);
        if self.sealed(&id)? {
            return Err(deny("attempt already finalized"));
        }
        let recovered = self.settleable_launch(&id)?;
        let g = gather(self.conn, &id, prepared)?;
        if g.evidence.iter().cloned().collect::<BTreeSet<_>>() != prepared.evidence {
            return Err(deny(
                "evidence changed after verification; prepare settlement again",
            ));
        }
        let facts_digest = sha256_hex(&[format!("{:?}", g.facts).as_bytes()]);
        let (outcome, reason, uncertainties) = match classify(&g.facts) {
            Classification::RecoveryRequired(reasons) => {
                let text = reasons.join("; ");
                let event =
                    self.append_event("AttemptRecoveryRequired", None, &format!("{id}|{text}"))?;
                self.conn.execute(
                    "INSERT INTO attempt_recovery_holds VALUES (?1,?2,?3)",
                    &[
                        Value::Int(to_db(event.0)?),
                        Value::Text(&id),
                        Value::Text(&text),
                    ],
                )?;
                return Ok(format!("RECOVERY_REQUIRED: {text}"));
            }
            Classification::Final {
                outcome,
                reason,
                uncertainties,
            } => (outcome, reason, uncertainties),
        };
        let evidence = |kind: &str| {
            g.evidence
                .iter()
                .find(|(k, _)| k == kind)
                .map(|(_, d)| d.clone())
        };
        let candidate = evidence("CANDIDATE");
        let terminal = format!("{:?}", g.facts.terminals);
        let event = self.append_event(
            "AttemptResultSealed",
            None,
            &format!("{id}|{}|{reason}", outcome_db(outcome)),
        )?;
        let opt = |v: Option<i32>| v.map_or(Value::Null, |n| Value::Int(i64::from(n)));
        self.conn.execute(
            "INSERT INTO attempt_results VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            &[
                Value::Text(&id),
                Value::Text(outcome_db(outcome)),
                Value::Text(reason),
                g.stop_cause.as_deref().map_or(Value::Null, Value::Text),
                opt(g.exit.and_then(|e| e.code)),
                opt(g.exit.and_then(|e| e.signal)),
                Value::Text(&terminal),
                candidate.as_deref().map_or(Value::Null, Value::Text),
                evidence("STDOUT")
                    .as_deref()
                    .map_or(Value::Null, Value::Text),
                evidence("STDERR")
                    .as_deref()
                    .map_or(Value::Null, Value::Text),
                Value::Text(&uncertainties.join("\n")),
                Value::Text(&facts_digest),
                Value::Int(to_db(self.epoch.0)?),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        self.seal_approval_result(
            attempt,
            &format!(
                "{}: {reason}; candidate={candidate:?}; facts={facts_digest}",
                outcome_db(outcome)
            ),
        )?;
        self.conn.execute(
            "UPDATE scheduler_attempts SET sealed=1 WHERE attempt=?1 AND sealed=0",
            &[Value::Text(&id)],
        )?;
        // A finished step is never re-dispatched automatically; retry is a new decision.
        self.conn.execute(
            "UPDATE scheduler_queue SET control_revision=control_revision+1,
             intent=CASE WHEN intent='CANCEL' THEN 'CANCEL' ELSE 'HOLD' END
             WHERE (step,step_revision) IN (SELECT step,step_revision FROM scheduler_attempts WHERE attempt=?1)",
            &[Value::Text(&id)],
        )?;
        if g.facts.unresolved_effects.is_empty() {
            let holder = AggregateKey::new("scheduler_attempt", attempt);
            let claims = crate::queries::live_claims(self.conn, None)?;
            for claim in claims.into_iter().filter(|c| c.holder == holder) {
                self.release_claim(claim.id, release_basis(recovered))?;
            }
        } else {
            let text = format!(
                "claims retained: effect outcome unknown {:?}",
                g.facts.unresolved_effects
            );
            let hold = self.append_event("AttemptRecoveryRequired", None, &text)?;
            self.conn.execute(
                "INSERT INTO attempt_recovery_holds VALUES (?1,?2,?3)",
                &[
                    Value::Int(to_db(hold.0)?),
                    Value::Text(&id),
                    Value::Text(&text),
                ],
            )?;
        }
        Ok(outcome_db(outcome).into())
    }

    fn effect_attempt(&self, key: &str) -> Result<String, TxError> {
        self.conn
            .query_optional(
                "SELECT attempt FROM attempt_effects WHERE operation_key=?1",
                &[Value::Text(key)],
                |r| r.text(0),
            )?
            .ok_or_else(|| deny("not an attempt effect"))
    }

    fn effect_admissible(&self, id: &str) -> Result<(), TxError> {
        self.current_launch(id)?;
        if self.sealed(id)? {
            return Err(deny("attempt result is sealed; no further effects"));
        }
        let stopped = self.conn.query_optional(
            "SELECT 1 FROM approval_stops WHERE attempt=?1",
            &[Value::Text(id)],
            |r| r.int(0),
        )?;
        if stopped.is_some() {
            return Err(deny("attempt is stopping; no further effects"));
        }
        Ok(())
    }

    /// Journals an attempt's external effect as NOT_ISSUED. The unique key is the
    /// idempotency token: a second journal of the same operation is refused.
    pub fn journal_attempt_effect(
        &mut self,
        attempt: OpaqueId,
        operation_key: &str,
        target: &str,
        payload: &str,
    ) -> Result<(), TxError> {
        let id = id_to_text(attempt);
        if !operation_key.starts_with(EFFECT_PREFIX) || operation_key.len() <= EFFECT_PREFIX.len() {
            return Err(deny("attempt effects use the attempt-effect: namespace"));
        }
        self.effect_admissible(&id)?;
        self.enqueue_effect(operation_key, target, payload)?;
        let event = self.append_event(
            "AttemptEffectJournaled",
            None,
            &format!("{id}|{operation_key}"),
        )?;
        self.conn.execute(
            "INSERT INTO attempt_effects VALUES (?1,?2,?3)",
            &[
                Value::Text(operation_key),
                Value::Text(&id),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(())
    }

    /// Commit this BEFORE crossing the external boundary. A lost acknowledgement leaves
    /// ISSUED_UNKNOWN, which is never dispatched again.
    pub fn issue_attempt_effect(&mut self, operation_key: &str) -> Result<(), TxError> {
        let id = self.effect_attempt(operation_key)?;
        self.effect_admissible(&id)?;
        self.advance_effect(operation_key, EffectState::IssuedUnknown)?;
        self.append_event("AttemptEffectIssued", None, operation_key)?;
        Ok(())
    }

    /// Records a fake remote observation; only an observation of the exact payload can
    /// resolve ISSUED_UNKNOWN. `Unknown` is retained without changing state.
    pub fn record_effect_observation(
        &mut self,
        operation_key: &str,
        observer: &str,
        status: RemoteObservation,
        payload_digest: &str,
    ) -> Result<(), TxError> {
        self.effect_attempt(operation_key)?;
        let effect = self
            .effect(operation_key)?
            .ok_or_else(|| deny("effect missing"))?;
        if effect.payload_digest != payload_digest {
            return Err(deny("observation names a different operation payload"));
        }
        let (text, to) = match status {
            RemoteObservation::Committed => ("COMMITTED", Some(EffectState::Confirmed)),
            RemoteObservation::Rejected => ("REJECTED", Some(EffectState::Rejected)),
            RemoteObservation::Unknown => ("UNKNOWN", None),
        };
        if to.is_some() && effect.state != EffectState::IssuedUnknown {
            return Err(deny(
                "only an issued effect with unknown outcome can be resolved",
            ));
        }
        let event = self.append_event(
            "EffectObserved",
            None,
            &format!("{operation_key}|{observer}|{text}"),
        )?;
        self.conn.execute(
            "INSERT INTO effect_observations VALUES (?1,?2,?3,?4,?5)",
            &[
                Value::Int(to_db(event.0)?),
                Value::Text(operation_key),
                Value::Text(observer),
                Value::Text(text),
                Value::Text(payload_digest),
            ],
        )?;
        if let Some(to) = to {
            self.advance_effect(operation_key, to)?;
        }
        Ok(())
    }
}

impl ReadView<'_> {
    pub fn attempt_result(&self, attempt: OpaqueId) -> Result<Option<AttemptResult>, StorageError> {
        let row = self.conn.query_optional(
            "SELECT outcome,reason,stop_cause,exit_code,exit_signal,terminal,candidate,stdout,stderr,
                    uncertainties,facts_digest,event FROM attempt_results WHERE attempt=?1",
            &[Value::Text(&id_to_text(attempt))],
            |r| {
                Ok((
                    r.text(0)?,
                    r.text(1)?,
                    r.optional_text(2)?,
                    r.optional_int(3)?,
                    r.optional_int(4)?,
                    r.text(5)?,
                    r.optional_text(6)?,
                    r.optional_text(7)?,
                    r.optional_text(8)?,
                    r.text(9)?,
                    r.text(10)?,
                    r.int(11)?,
                ))
            },
        )?;
        let Some((o, reason, stop, code, signal, terminal, candidate, out, err, u, facts, event)) =
            row
        else {
            return Ok(None);
        };
        let small = |v: Option<i64>| {
            v.map(|n| i32::try_from(n).map_err(|_| StorageError::corrupt("exit range")))
                .transpose()
        };
        Ok(Some(AttemptResult {
            attempt,
            outcome: outcome_from_db(&o)?,
            reason,
            stop_cause: stop,
            exit_code: small(code)?,
            exit_signal: small(signal)?,
            terminal,
            candidate,
            stdout: out,
            stderr: err,
            uncertainties: u.lines().map(str::to_owned).collect(),
            facts_digest: facts,
            event: EventSeq(from_db(event)?),
        }))
    }

    pub fn attempt_stop_cause(
        &self,
        attempt: OpaqueId,
    ) -> Result<Option<(String, String)>, StorageError> {
        Ok(self.conn.query_optional(
            "SELECT cause,detail FROM attempt_stop_causes WHERE attempt=?1",
            &[Value::Text(&id_to_text(attempt))],
            |r| Ok((r.text(0)?, r.text(1)?)),
        )?)
    }

    pub fn attempt_evidence(
        &self,
        attempt: OpaqueId,
    ) -> Result<Vec<(String, BlobRef)>, StorageError> {
        let rows = self.conn.query(
            "SELECT e.kind,e.digest,b.size FROM attempt_evidence e JOIN evidence_blobs b
             ON b.digest=e.digest WHERE e.attempt=?1 ORDER BY e.kind",
            &[Value::Text(&id_to_text(attempt))],
            |r| Ok((r.text(0)?, r.text(1)?, r.int(2)?)),
        )?;
        rows.into_iter()
            .map(|(k, digest, size)| {
                Ok((
                    k,
                    BlobRef {
                        digest,
                        size: from_db(size)?,
                    },
                ))
            })
            .collect()
    }

    pub fn attempt_recovery_holds(&self, attempt: OpaqueId) -> Result<Vec<String>, StorageError> {
        Ok(self.conn.query(
            "SELECT reasons FROM attempt_recovery_holds WHERE attempt=?1 ORDER BY event",
            &[Value::Text(&id_to_text(attempt))],
            |r| r.text(0),
        )?)
    }

    /// Unissued attempt effects a dispatcher may still issue: never ISSUED_UNKNOWN, never
    /// for a sealed or stopping attempt.
    pub fn dispatchable_attempt_effects(&self) -> Result<Vec<String>, StorageError> {
        Ok(self.conn.query(
            "SELECT f.operation_key FROM attempt_effects f JOIN outbox o ON o.operation_key=f.operation_key
             WHERE o.state='NOT_ISSUED'
             AND NOT EXISTS(SELECT 1 FROM attempt_results r WHERE r.attempt=f.attempt)
             AND NOT EXISTS(SELECT 1 FROM approval_stops s WHERE s.attempt=f.attempt)
             ORDER BY f.operation_key",
            &[],
            |r| r.text(0),
        )?)
    }

    pub fn effect_observations(
        &self,
        operation_key: &str,
    ) -> Result<Vec<(String, String)>, StorageError> {
        Ok(self.conn.query(
            "SELECT observer,status FROM effect_observations WHERE operation_key=?1 ORDER BY event",
            &[Value::Text(operation_key)],
            |r| Ok((r.text(0)?, r.text(1)?)),
        )?)
    }
}

/// Engine, deterministic command identities and the evidence store for one settlement.
pub struct SettlementContext<'a> {
    pub engine: &'a Engine,
    pub ids: &'a dyn IdGenerator,
    pub store: &'a BlobStore,
}

impl SettlementContext<'_> {
    pub(crate) fn cmd<F>(&self, kind: &str, subject: OpaqueId, f: F) -> Result<String, String>
    where
        F: FnOnce(&mut Tx<'_>) -> Result<String, TxError> + Send + 'static,
    {
        let id = self
            .ids
            .next_id()
            .map_err(|_| "command ids exhausted".to_owned())?;
        let request = CommandRequest::new(
            CommandId(id),
            self.engine.epoch(),
            kind,
            id_to_text(subject),
        );
        match self.engine.execute(request, f) {
            Ok(r) => Ok(r.response),
            Err(CommandError::Rejected(TxError::Rejected(why))) => Err(why),
            Err(e) => Err(format!("{e:?}")),
        }
    }

    pub(crate) fn read<T>(
        &self,
        f: impl FnOnce(&ReadView<'_>) -> Result<T, StorageError>,
    ) -> Result<T, String> {
        self.engine
            .reader()
            .and_then(|mut r| r.read(f))
            .map_err(|e| e.to_string())
    }
}

/// I/O stage: begins settlement for a proven-absent tree, records the candidate and
/// publishes raw spools as blobs, then verifies every referenced blob on disk. Failures
/// are carried forward as lost coverage; nothing is reconstructed or overwritten.
pub fn prepare_settlement(
    ctx: &SettlementContext<'_>,
    attempt: OpaqueId,
    faults: BlobFaults,
) -> Result<PreparedSettlement, String> {
    let id = id_to_text(attempt);
    let (absent, spool) = ctx.read(|v| {
        let absent = v
            .process_observations(attempt)?
            .iter()
            .any(|(k, _)| k == "TREE_ABSENT")
            || crate::recovery::recovered_absent(v.conn, &id)?;
        let spool = v.conn.query_optional(
            "SELECT spool,generation FROM process_launches WHERE attempt=?1",
            &[Value::Text(&id)],
            |r| Ok((r.text(0)?, r.int(1)?)),
        )?;
        Ok((absent, spool))
    })?;
    let mut failures = Vec::new();
    let mut candidate_note = None;
    if absent {
        let (spool, generation) = spool.ok_or("no issued launch")?;
        let begun = ctx.cmd("BeginAttemptSettlement", attempt, move |tx| {
            tx.begin_attempt_settlement(attempt)?;
            Ok("settling".into())
        });
        if let Err(why) = begun
            && why != "attempt already finalized"
        {
            return Err(why);
        }
        let known: BTreeSet<String> = ctx
            .read(|v| v.attempt_evidence(attempt))?
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        if !known.contains("CANDIDATE") {
            let collected =
                crate::worktrees::collect_candidate(ctx.engine, attempt).and_then(|c| {
                    let bytes = fs::read(c.path()).map_err(|e| e.to_string())?;
                    ctx.cmd("RecordCandidate", attempt, move |tx| {
                        tx.record_candidate(&c)?;
                        Ok("candidate".into())
                    })?;
                    let blob = ctx.store.put(&bytes, faults)?;
                    ctx.cmd("ReferenceCandidate", attempt, move |tx| {
                        tx.record_attempt_evidence(attempt, "CANDIDATE", &blob)?;
                        Ok("referenced".into())
                    })
                });
            if let Err(why) = collected {
                candidate_note = Some(why);
            }
        }
        let dir = std::path::Path::new(&spool).join(format!("generation-{generation}"));
        for (stream, file) in [
            (Stream::Stdout, "stdout.raw"),
            (Stream::Stderr, "stderr.raw"),
        ] {
            let kind = stream_kind(stream);
            if known.contains(kind) {
                continue;
            }
            let published = (|| {
                let durable = ctx.read(|v| v.stream_checkpoint(attempt, stream))?;
                let bytes = fs::read(dir.join(file)).map_err(|e| format!("{kind} spool: {e}"))?;
                if bytes.len() as u64 != durable {
                    return Err(format!(
                        "{kind} spool holds {} bytes but {durable} are durably checkpointed",
                        bytes.len()
                    ));
                }
                let blob = ctx.store.put(&bytes, faults)?;
                ctx.cmd("ReferenceRawLog", attempt, move |tx| {
                    tx.record_attempt_evidence(attempt, kind, &blob)?;
                    Ok("referenced".into())
                })
            })();
            if let Err(why) = published {
                failures.push(why);
            }
        }
    }
    let evidence = ctx.read(|v| v.attempt_evidence(attempt))?;
    for (kind, blob) in &evidence {
        if let Err(why) = ctx.store.verify(blob) {
            failures.push(format!("{kind}: {why}"));
        }
    }
    Ok(PreparedSettlement {
        attempt,
        evidence: evidence.into_iter().map(|(k, b)| (k, b.digest)).collect(),
        failures,
        candidate_note,
    })
}

/// Database stage: one writer transaction seals the result or appends a recovery hold.
pub fn finalize_attempt(
    ctx: &SettlementContext<'_>,
    prepared: &PreparedSettlement,
) -> Result<Settlement, String> {
    let p = prepared.clone();
    let attempt = p.attempt;
    let response = ctx.cmd("FinalizeAttempt", attempt, move |tx| {
        tx.finalize_attempt_result(&p)
    })?;
    if let Some(reasons) = response.strip_prefix("RECOVERY_REQUIRED: ") {
        return Ok(Settlement::RecoveryRequired(
            reasons.split("; ").map(str::to_owned).collect(),
        ));
    }
    ctx.read(|v| v.attempt_result(attempt))?
        .map(|r| Settlement::Sealed(Box::new(r)))
        .ok_or_else(|| "sealed result not readable".into())
}

pub fn settle_attempt(
    ctx: &SettlementContext<'_>,
    attempt: OpaqueId,
    faults: BlobFaults,
) -> Result<Settlement, String> {
    let prepared = prepare_settlement(ctx, attempt, faults)?;
    finalize_attempt(ctx, &prepared)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jojo_domain::provider::EventPayload;

    #[test]
    fn stored_terminal_and_exit_text_parse_exactly_and_unknown_shapes_never_succeed() {
        for status in [
            TerminalStatus::Success,
            TerminalStatus::Error,
            TerminalStatus::Interrupted,
        ] {
            let payload = format!(
                "{:?}",
                EventPayload::ProviderTerminal {
                    turn: "t\", status: Success, \\".into(),
                    status,
                    raw_status: "x".into(),
                }
            );
            assert_eq!(terminal_status(&payload), status);
        }
        assert_eq!(
            terminal_status("ProviderTerminal { turn: \"t\", status: Successful }"),
            TerminalStatus::Unknown
        );
        assert_eq!(
            terminal_status("TextDelta(\"status: Success\")"),
            TerminalStatus::Unknown
        );
        assert_eq!(
            exit_status("code=Some(0) signal=None"),
            Some(ExitStatus {
                code: Some(0),
                signal: None
            })
        );
        assert_eq!(
            exit_status("code=None signal=Some(9)").map(|e| e.signal),
            Some(Some(9))
        );
        assert_eq!(exit_status("root terminated; wait status pending"), None);
        assert_eq!(
            stream_closed("STDERR durable=7 discarded=3"),
            Some(("STDERR".into(), 7, 3))
        );
        assert_eq!(supervisor_stop(DEADLINE_STOP), SupervisorStop::Deadline);
        assert_eq!(supervisor_stop("protocol: bad"), SupervisorStop::Protocol);
        assert_eq!(supervisor_stop("stop requested"), SupervisorStop::Other);
    }
}

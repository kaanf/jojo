//! Reopen inspection, recovery resolution and reviewed continuation (P1-11).
//!
//! Every reopened engine starts with admission closed. [`inspect_recovery`] reads durable
//! records of earlier epochs and re-observes processes, worktrees and raw logs without
//! mutating anything. Its result is consumed by writer commands that first re-derive the
//! durable facts and refuse a stale inspection:
//!
//! - `start_after_recovery` records the reviewed inspection and opens admission. Blocked
//!   subjects keep their QUARANTINED claims, so they still block conflicting writers.
//! - `resolve_recovery` records one offered choice. Only an unblocked, non-Hold choice
//!   writes an absence proof, which lets the P1-10 settlement seal a non-success result
//!   (never success) and release claims with `RECOVERY_RESOLVED`.
//! - `record_continuation_plan` binds a new attempt to the sealed predecessor, its
//!   candidate, reconciled effects and session policy. Launch and worktree binding check
//!   it; no provider continuation is admitted without one.
//!
//! Nothing here signals, stops or adopts an old process, replays an effect or deletes a
//! file. A live orphan stays quarantined: inspection is not a lifetime handle.
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use jojo_domain::identity::{ArtifactId, OpaqueId};
use jojo_domain::provider::{Capabilities, EventPayload, LaunchSpec, SessionRef, Support};
use jojo_domain::recovery::{
    Assessment, LaunchStage, ProcessFinding, RecoveryChoice, RecoveryFacts, SessionContinuity,
    assess,
};

use crate::model::{from_db, id_from_text, id_to_text, to_db};
use crate::outcomes::{SettlementContext, settle_attempt};
use crate::sqlite::{Connection, Value};
use crate::{
    AggregateKey, BlobFaults, Engine, EngineEpoch, HumanOrigin, IdentityCheck, ProcessIdentity,
    ReadView, ReleaseBasis, Settlement, StorageError, Tx, TxError, sha256_hex,
};

fn deny(s: impl Into<String>) -> TxError {
    TxError::Rejected(s.into())
}

/// Re-observes identities recorded by an earlier helper. Implementations never signal.
pub trait ProcessInspector {
    fn inspect(&self, recorded: &[ProcessIdentity]) -> Vec<(String, IdentityCheck)>;
}

/// Real inspection through an explicit `ps` (the verifier injects its unprivileged copy).
pub struct OsProcessInspector {
    pub ps: PathBuf,
}

impl Default for OsProcessInspector {
    fn default() -> Self {
        Self {
            ps: PathBuf::from("/bin/ps"),
        }
    }
}

impl ProcessInspector for OsProcessInspector {
    fn inspect(&self, recorded: &[ProcessIdentity]) -> Vec<(String, IdentityCheck)> {
        crate::process::inspect_recorded(&self.ps, recorded)
    }
}

/// True when this (the latest) epoch recorded an absence proof for the attempt.
pub(crate) fn recovered_absent(c: &Connection, id: &str) -> Result<bool, StorageError> {
    Ok(c.query_optional(
        "SELECT 1 FROM recovery_absence_proofs
         WHERE attempt=?1 AND epoch=(SELECT max(epoch) FROM engine_sessions)",
        &[Value::Text(id)],
        |r| r.int(0),
    )?
    .is_some())
}

/// One earlier-epoch attempt that needs a decision.
#[derive(Clone, Debug)]
pub struct RecoverySubject {
    attempt: OpaqueId,
    facts: RecoveryFacts,
    assessment: Assessment,
    findings: String,
    db_digest: String,
}

impl RecoverySubject {
    pub fn attempt(&self) -> OpaqueId {
        self.attempt
    }
    pub fn facts(&self) -> &RecoveryFacts {
        &self.facts
    }
    pub fn assessment(&self) -> &Assessment {
        &self.assessment
    }
    /// Worktree path, durable offsets and process findings, for display.
    pub fn findings(&self) -> &str {
        &self.findings
    }
}

/// Only [`inspect_recovery`] constructs this; writer commands refuse it once the durable
/// facts it was derived from have changed.
#[derive(Clone, Debug)]
pub struct RecoveryInspection {
    epoch: EngineEpoch,
    subjects: Vec<RecoverySubject>,
    other_quarantined: Vec<String>,
    digest: String,
}

impl RecoveryInspection {
    pub fn subjects(&self) -> &[RecoverySubject] {
        &self.subjects
    }
    pub fn subject(&self, attempt: OpaqueId) -> Option<&RecoverySubject> {
        self.subjects.iter().find(|s| s.attempt == attempt)
    }
    /// Quarantined claims of holders that are not subjects (they stay quarantined).
    pub fn other_quarantined(&self) -> &[String] {
        &self.other_quarantined
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

/// Unsealed or claim-holding earlier-epoch attempts, and sealed non-success attempts that
/// are still the latest of their step and have no continuation plan.
fn subject_ids(c: &Connection) -> Result<Vec<String>, StorageError> {
    Ok(c.query(
        "SELECT a.attempt FROM scheduler_attempts a
         WHERE a.epoch < (SELECT max(epoch) FROM engine_sessions) AND (a.sealed=0
          OR EXISTS(SELECT 1 FROM resource_claims c WHERE c.holder_kind='scheduler_attempt'
                    AND c.holder_id=a.attempt AND c.state<>'RELEASED')
          OR (EXISTS(SELECT 1 FROM attempt_results r WHERE r.attempt=a.attempt AND r.outcome<>'SUCCEEDED')
              AND NOT EXISTS(SELECT 1 FROM continuation_plans p WHERE p.predecessor=a.attempt)
              AND NOT EXISTS(SELECT 1 FROM scheduler_attempts l WHERE l.step=a.step
                             AND l.step_revision=a.step_revision AND l.rowid>a.rowid)))
         ORDER BY a.attempt",
        &[],
        |r| r.text(0),
    )?)
}

/// Durable facts of one subject. `canonical` covers everything a decision depends on.
struct DbFacts {
    stage: LaunchStage,
    records: Vec<ProcessIdentity>,
    launch: Option<(String, String, i64)>,
    sealed_candidate: Option<String>,
    candidates: Vec<(String, String, String)>,
    unknown_effects: Vec<String>,
    stale: Vec<String>,
    sessions: Vec<String>,
    checkpoints: Vec<(String, u64)>,
    canonical: String,
}

fn text_rows(c: &Connection, sql: &str, id: &str) -> Result<Vec<String>, StorageError> {
    Ok(c.query(sql, &[Value::Text(id)], |r| r.text(0))?)
}

fn db_facts(c: &Connection, id: &str) -> Result<DbFacts, StorageError> {
    let observations = text_rows(
        c,
        "SELECT kind FROM process_observations WHERE attempt=?1 ORDER BY event",
        id,
    )?;
    let records = c.query(
        "SELECT pid,ppid,pgid,start_identity,boot FROM process_records WHERE attempt=?1
         ORDER BY role DESC, pid",
        &[Value::Text(id)],
        |r| Ok((r.int(0)?, r.int(1)?, r.int(2)?, r.text(3)?, r.text(4)?)),
    )?;
    let records = records
        .into_iter()
        .map(|(pid, ppid, pgid, start, boot)| {
            Ok(ProcessIdentity {
                pid: u32::try_from(pid).map_err(|_| StorageError::corrupt("pid"))?,
                ppid: u32::try_from(ppid).map_err(|_| StorageError::corrupt("ppid"))?,
                pgid: u32::try_from(pgid).map_err(|_| StorageError::corrupt("pgid"))?,
                start,
                boot,
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
    let sealed = c.query_one(
        "SELECT sealed FROM scheduler_attempts WHERE attempt=?1",
        &[Value::Text(id)],
        |r| r.int(0),
    )? != 0;
    let launch_state = c.query_optional(
        "SELECT state FROM outbox WHERE operation_key='scheduler-launch:'||?1",
        &[Value::Text(id)],
        |r| r.text(0),
    )?;
    let stage = if sealed {
        LaunchStage::Sealed
    } else if observations
        .iter()
        .any(|k| k == "TREE_ABSENT" || k == "NOT_SPAWNED")
    {
        LaunchStage::Concluded
    } else if !records.is_empty() {
        LaunchStage::Spawned
    } else if launch_state.as_deref() == Some("NOT_ISSUED") {
        LaunchStage::NotIssued
    } else {
        LaunchStage::IssuedWithoutIdentity
    };
    let launch = c.query_optional(
        "SELECT provider,spool,generation FROM process_launches WHERE attempt=?1",
        &[Value::Text(id)],
        |r| Ok((r.text(0)?, r.text(1)?, r.int(2)?)),
    )?;
    let result = c.query_optional(
        "SELECT outcome,candidate FROM attempt_results WHERE attempt=?1",
        &[Value::Text(id)],
        |r| Ok((r.text(0)?, r.optional_text(1)?)),
    )?;
    let candidates = c.query(
        "SELECT digest,head,path FROM worktree_candidates WHERE attempt=?1 ORDER BY rowid",
        &[Value::Text(id)],
        |r| Ok((r.text(0)?, r.text(1)?, r.text(2)?)),
    )?;
    let effects = c.query(
        "SELECT f.operation_key,o.state FROM attempt_effects f JOIN outbox o
         ON o.operation_key=f.operation_key WHERE f.attempt=?1 ORDER BY f.operation_key",
        &[Value::Text(id)],
        |r| Ok((r.text(0)?, r.text(1)?)),
    )?;
    let (prd, adr, plan, bundle) = c.query_one(
        "SELECT prd,adr,plan,bundle FROM approval_attempts WHERE id=?1",
        &[Value::Text(id)],
        |r| Ok((r.text(0)?, r.text(1)?, r.text(2)?, r.text(3)?)),
    )?;
    let current = crate::approvals::development_hash(
        c,
        ArtifactId(id_from_text(&prd)?),
        ArtifactId(id_from_text(&adr)?),
        ArtifactId(id_from_text(&plan)?),
    )?;
    let stale = if current.as_deref() == Some(bundle.as_str()) {
        vec![]
    } else {
        vec![format!(
            "PRD/ADR/plan approvals of {plan} are no longer applicable"
        )]
    };
    let mut sessions = text_rows(
        c,
        "SELECT payload FROM provider_events WHERE attempt=?1 AND kind='SessionIdentified'
         ORDER BY sequence",
        id,
    )?;
    sessions.sort();
    sessions.dedup();
    let checkpoints = c
        .query(
            "SELECT stream,max(durable) FROM stream_checkpoints WHERE attempt=?1 GROUP BY stream
             ORDER BY stream",
            &[Value::Text(id)],
            |r| Ok((r.text(0)?, r.int(1)?)),
        )?
        .into_iter()
        .map(|(s, d)| Ok((s, from_db(d)?)))
        .collect::<Result<Vec<_>, StorageError>>()?;
    let claims = c.query(
        "SELECT claim_id,state FROM resource_claims WHERE holder_kind='scheduler_attempt'
         AND holder_id=?1 AND state<>'RELEASED' ORDER BY claim_id",
        &[Value::Text(id)],
        |r| Ok((r.int(0)?, r.text(1)?)),
    )?;
    let resolutions = text_rows(
        c,
        "SELECT choice FROM recovery_resolutions WHERE attempt=?1 ORDER BY event",
        id,
    )?;
    let unknown_effects = effects
        .iter()
        .filter(|(_, s)| s == "ISSUED_UNKNOWN")
        .map(|(k, _)| k.clone())
        .collect();
    let canonical = format!(
        "{stage:?}|{observations:?}|{records:?}|{launch_state:?}|{launch:?}|{result:?}|\
         {candidates:?}|{effects:?}|{bundle}|{current:?}|{sessions:?}|{checkpoints:?}|\
         {claims:?}|{resolutions:?}"
    );
    Ok(DbFacts {
        stage,
        records,
        launch,
        sealed_candidate: result.and_then(|(_, c)| c),
        candidates,
        unknown_effects,
        stale,
        sessions,
        checkpoints,
        canonical,
    })
}

fn db_digest(c: &Connection, id: &str) -> Result<String, StorageError> {
    Ok(sha256_hex(&[db_facts(c, id)?.canonical.as_bytes()]))
}

/// Strict parse of a stored `SessionIdentified` payload, accepted only if it formats back
/// to exactly the stored text.
fn parse_session(payload: &str) -> Option<SessionRef> {
    fn quoted(s: &str) -> Option<(String, &str)> {
        let s = s.strip_prefix('"')?;
        let (mut out, mut escaped) = (String::new(), false);
        for (i, ch) in s.char_indices() {
            match (escaped, ch) {
                (true, c) => {
                    out.push(c);
                    escaped = false;
                }
                (false, '\\') => escaped = true,
                (false, '"') => return Some((out, &s[i + 1..])),
                (false, c) => out.push(c),
            }
        }
        None
    }
    let rest = payload.strip_prefix("SessionIdentified(SessionRef { provider: ")?;
    let (provider, rest) = rest.split_once(", session: ")?;
    let provider = match provider {
        "ClaudeCode" => jojo_domain::provider::ProviderKind::ClaudeCode,
        "Codex" => jojo_domain::provider::ProviderKind::Codex,
        _ => return None,
    };
    let (session, rest) = quoted(rest)?;
    let (turn, _) = quoted(rest.strip_prefix(", turn: ")?)?;
    let s = SessionRef {
        provider,
        session,
        turn,
    };
    (format!("{:?}", EventPayload::SessionIdentified(s.clone())) == payload).then_some(s)
}

fn session_continuity(
    provider: Option<&str>,
    sessions: &[String],
    capabilities: &dyn Fn(&str) -> Option<Capabilities>,
) -> SessionContinuity {
    let session = match sessions {
        [] => return SessionContinuity::NotRecorded,
        [one] => one,
        _ => {
            return SessionContinuity::Unsupported(
                "several sessions recorded; the latest is never assumed".into(),
            );
        }
    };
    let Some(exact) = parse_session(session) else {
        return SessionContinuity::Unsupported("recorded session is not exact".into());
    };
    let Some(caps) = provider.and_then(capabilities) else {
        return SessionContinuity::Unsupported("no provider capability profile".into());
    };
    for (name, support) in [
        ("explicit sessions", caps.explicit_sessions),
        ("exact continuation", caps.exact_continuation),
        ("continuation in a new cwd", caps.continuation_cwd_change),
    ] {
        if support != Support::Supported {
            return SessionContinuity::Unsupported(format!("{name}: {support:?}"));
        }
    }
    SessionContinuity::Exact(exact)
}

fn finding(check: &IdentityCheck) -> ProcessFinding {
    match check {
        IdentityCheck::Absent => ProcessFinding::Absent,
        IdentityCheck::Alive => ProcessFinding::Alive,
        IdentityCheck::Reused => ProcessFinding::Reused,
        IdentityCheck::Unknown(e) => ProcessFinding::Unknown(e.clone()),
    }
}

/// Read-only reopen inspection. `capabilities` maps a recorded provider name (for
/// example `ClaudeCode`) to its adapter's declared capabilities, if any.
pub fn inspect_recovery(
    engine: &Engine,
    inspector: &dyn ProcessInspector,
    capabilities: &dyn Fn(&str) -> Option<Capabilities>,
) -> Result<RecoveryInspection, String> {
    let mut reader = engine.reader().map_err(|e| e.to_string())?;
    let (ids, other_quarantined) = reader
        .read(|v| {
            let ids = subject_ids(v.conn)?;
            let others = v
                .live_claims()?
                .into_iter()
                .filter(|c| {
                    !(c.holder.kind == "scheduler_attempt"
                        && ids.contains(&id_to_text(c.holder.id)))
                        && c.epoch != engine.epoch()
                })
                .map(|c| {
                    format!(
                        "{} held by {}:{}",
                        c.resource_key,
                        c.holder.kind,
                        id_to_text(c.holder.id)
                    )
                })
                .collect();
            Ok((ids, others))
        })
        .map_err(|e| e.to_string())?;
    let mut subjects = Vec::new();
    for id in ids {
        let attempt = id_from_text(&id).map_err(|e| e.to_string())?;
        let (db, record) = reader
            .read(|v| Ok((db_facts(v.conn, &id)?, v.worktree(attempt)?)))
            .map_err(|e| e.to_string())?;
        let processes: Vec<(String, ProcessFinding)> = if db.stage == LaunchStage::Spawned {
            inspector
                .inspect(&db.records)
                .iter()
                .map(|(label, check)| (label.clone(), finding(check)))
                .collect()
        } else {
            vec![]
        };
        let absent = match db.stage {
            LaunchStage::Spawned => {
                !processes.is_empty() && processes.iter().all(|(_, p)| *p == ProcessFinding::Absent)
            }
            LaunchStage::IssuedWithoutIdentity => false,
            _ => true,
        };
        let mut candidate = None;
        let worktree = match &record {
            None if matches!(db.stage, LaunchStage::NotIssued) => Ok(()),
            None => Err("owned worktree record missing".to_owned()),
            Some(r) => crate::worktrees::observe_worktree(r).and_then(|_| {
                if let Some((digest, _, path)) = db.candidates.last() {
                    crate::worktrees::CandidateSnapshot::reopen(Path::new(path), digest)
                        .map_err(|e| format!("recorded candidate packet: {e}"))?;
                }
                match (db.stage, db.candidates.last()) {
                    (LaunchStage::Sealed, _) => {
                        candidate = db.sealed_candidate.clone();
                        Ok(())
                    }
                    (LaunchStage::NotIssued, _) => Ok(()),
                    (_, _) if !absent => Ok(()),
                    (_, recorded) => {
                        let (digest, head) = crate::worktrees::recapture(r)?;
                        if let Some((d, h, _)) = recorded
                            && (*d != digest || *h != head)
                        {
                            return Err(
                                "worktree changed since its recorded candidate (HEAD or snapshot)"
                                    .into(),
                            );
                        }
                        candidate = Some(digest);
                        Ok(())
                    }
                }
            }),
        };
        let mut evidence_gaps = Vec::new();
        if let Some((_, spool, generation)) = &db.launch
            && matches!(db.stage, LaunchStage::Spawned | LaunchStage::Concluded)
        {
            let dir = Path::new(spool).join(format!("generation-{generation}"));
            for (stream, file) in [("STDOUT", "stdout.raw"), ("STDERR", "stderr.raw")] {
                let durable = db
                    .checkpoints
                    .iter()
                    .find(|(s, _)| s == stream)
                    .map_or(0, |(_, d)| *d);
                match fs::symlink_metadata(dir.join(file)) {
                    Ok(m) if m.is_file() && m.len() == durable => {}
                    Ok(m) if m.is_file() && m.len() > durable => evidence_gaps.push(format!(
                        "{stream}: {} trailing bytes beyond durable offset {durable} are not evidence",
                        m.len() - durable
                    )),
                    Ok(m) if m.is_file() => evidence_gaps.push(format!(
                        "{stream}: spool holds {} bytes but {durable} were checkpointed",
                        m.len()
                    )),
                    Ok(_) => evidence_gaps.push(format!("{stream}: spool is not a regular file")),
                    Err(e) => evidence_gaps.push(format!("{stream}: spool unreadable: {e}")),
                }
            }
        }
        let facts = RecoveryFacts {
            stage: db.stage,
            processes,
            worktree,
            stale_approvals: db.stale.clone(),
            unknown_effects: db.unknown_effects.clone(),
            evidence_gaps,
            candidate,
            session: session_continuity(
                db.launch.as_ref().map(|(p, _, _)| p.as_str()),
                &db.sessions,
                capabilities,
            ),
        };
        let assessment = assess(&facts);
        let findings = format!(
            "stage={:?}; worktree={}; durable={:?}; processes={:?}; evidence_gaps={:?}; candidate={:?}",
            facts.stage,
            record
                .as_ref()
                .map_or("none".into(), |r| r.path.display().to_string()),
            db.checkpoints,
            facts.processes,
            facts.evidence_gaps,
            facts.candidate,
        );
        subjects.push(RecoverySubject {
            attempt,
            facts,
            assessment,
            findings,
            db_digest: sha256_hex(&[db.canonical.as_bytes()]),
        });
    }
    let digest = sha256_hex(&[format!(
        "{}|{:?}|{other_quarantined:?}",
        engine.epoch().0,
        subjects
            .iter()
            .map(|s| (&s.db_digest, &s.findings, &s.assessment))
            .collect::<Vec<_>>()
    )
    .as_bytes()]);
    Ok(RecoveryInspection {
        epoch: engine.epoch(),
        subjects,
        other_quarantined,
        digest,
    })
}

fn actor(origin: &HumanOrigin) -> Result<String, TxError> {
    let (HumanOrigin::TrustedUi { actor } | HumanOrigin::FakeHumanFixture { actor }) = origin;
    if actor.trim().is_empty() {
        return Err(deny("human identity required"));
    }
    Ok(format!("{origin:?}"))
}

/// How the new attempt relates to its predecessor's provider session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionPolicy {
    FreshContext,
    /// Reuses the exact recorded session history; not the terminated process's memory.
    ExactHistory(SessionRef),
}

/// A reviewed plan for continuing a sealed, recovered attempt in a new attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContinuationPlan {
    pub successor: OpaqueId,
    pub predecessor: OpaqueId,
    pub remaining_work: String,
    /// Every effect the predecessor journaled; none may still be ISSUED_UNKNOWN.
    pub reconciled_effects: Vec<String>,
    pub session: SessionPolicy,
}

pub(crate) struct ContinuationBinding {
    pub candidate: Option<String>,
    pub base: String,
    pub predecessor_sources: String,
}

/// The reviewed plan that lets `successor` materialize `predecessor`'s candidate for the
/// same step. `None` if there is no such plan.
pub(crate) fn continuation_binding(
    c: &Connection,
    successor: OpaqueId,
    predecessor: OpaqueId,
) -> Result<Option<ContinuationBinding>, StorageError> {
    Ok(c.query_optional(
        "SELECT p.candidate,p.base,i.sources FROM continuation_plans p
         JOIN dependency_attempt_inputs i ON i.attempt=p.predecessor
         JOIN scheduler_attempts old ON old.attempt=p.predecessor
         JOIN scheduler_attempts new ON new.attempt=p.successor
         WHERE p.successor=?1 AND p.predecessor=?2
           AND new.step=old.step AND new.step_revision=old.step_revision",
        &[
            Value::Text(&id_to_text(successor)),
            Value::Text(&id_to_text(predecessor)),
        ],
        |r| {
            Ok(ContinuationBinding {
                candidate: r.optional_text(0)?,
                base: r.text(1)?,
                predecessor_sources: r.text(2)?,
            })
        },
    )?)
}

/// A launch carries provider continuation exactly when its reviewed plan says so, and
/// only for the exact recorded session of the predecessor (never "latest"/`--last`).
pub(crate) fn verify_continuation(c: &Connection, spec: &LaunchSpec) -> Result<(), TxError> {
    let plan = c.query_optional(
        "SELECT p.predecessor,p.session,p.digest,l.cwd FROM continuation_plans p
         JOIN process_launches l ON l.attempt=p.predecessor WHERE p.successor=?1",
        &[Value::Text(&id_to_text(spec.attempt.0))],
        |r| Ok((r.text(0)?, r.text(1)?, r.text(2)?, r.text(3)?)),
    )?;
    match (plan, &spec.continuation) {
        (None, None) => Ok(()),
        (None, Some(_)) => Err(deny(
            "provider continuation needs a reviewed continuation plan",
        )),
        (Some((_, session, _, _)), None) if session == FRESH => Ok(()),
        (Some(_), None) => Err(deny("continuation plan requires the exact session")),
        (Some((predecessor, session, digest, cwd)), Some(k)) => {
            let exact = format!(
                "{EXACT}{:?}",
                EventPayload::SessionIdentified(k.exact.clone())
            );
            if session != exact
                || id_to_text(k.predecessor.0) != predecessor
                || k.reviewed_plan != digest
                || k.previous_cwd != Path::new(&cwd)
            {
                return Err(deny(
                    "continuation differs from the reviewed plan's exact session",
                ));
            }
            Ok(())
        }
    }
}

const FRESH: &str = "FRESH_CONTEXT";
const EXACT: &str = "EXACT:";

impl Tx<'_> {
    fn fresh_subject<'i>(
        &self,
        inspection: &'i RecoveryInspection,
        attempt: OpaqueId,
    ) -> Result<&'i RecoverySubject, TxError> {
        if inspection.epoch != self.epoch {
            return Err(deny("inspection belongs to another engine epoch"));
        }
        let subject = inspection
            .subject(attempt)
            .ok_or_else(|| deny("attempt is not a recovery subject of this inspection"))?;
        if db_digest(self.conn, &id_to_text(attempt))? != subject.db_digest {
            return Err(deny(
                "durable facts changed since inspection; inspect again",
            ));
        }
        Ok(subject)
    }

    /// The user's Start after reviewing the inspection: records it and opens admission.
    pub fn start_after_recovery(
        &mut self,
        inspection: &RecoveryInspection,
        origin: HumanOrigin,
    ) -> Result<(), TxError> {
        let origin = actor(&origin)?;
        if inspection.epoch != self.epoch {
            return Err(deny("inspection belongs to another engine epoch"));
        }
        let current = subject_ids(self.conn)?;
        let inspected: Vec<String> = inspection
            .subjects
            .iter()
            .map(|s| id_to_text(s.attempt))
            .collect();
        if current != inspected {
            return Err(deny(
                "recovery subjects changed since inspection; inspect again",
            ));
        }
        for s in &inspection.subjects {
            self.fresh_subject(inspection, s.attempt)?;
        }
        let summary = inspection
            .subjects
            .iter()
            .map(|s| {
                format!(
                    "{}: {}",
                    id_to_text(s.attempt),
                    if s.assessment.blockers.is_empty() {
                        "unblocked".to_owned()
                    } else {
                        s.assessment.blockers.join("; ")
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let event = self.append_event("RecoveryStarted", None, &inspection.digest)?;
        self.conn.execute(
            "INSERT INTO recovery_starts VALUES (?1,?2,?3,?4,?5)",
            &[
                Value::Int(to_db(self.epoch.0)?),
                Value::Text(&inspection.digest),
                Value::Text(&summary),
                Value::Text(&origin),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        self.set_admission(crate::Admission::Open)
    }

    /// Records one offered choice for an inspected subject. Returns `HOLD`, `NOT_STARTED`,
    /// `CLAIMS_RELEASED` or `SETTLE` (the caller then runs P1-10 settlement).
    pub fn resolve_recovery(
        &mut self,
        inspection: &RecoveryInspection,
        attempt: OpaqueId,
        choice: RecoveryChoice,
        origin: HumanOrigin,
    ) -> Result<String, TxError> {
        let origin = actor(&origin)?;
        let subject = self.fresh_subject(inspection, attempt)?.clone();
        let a = &subject.assessment;
        if !a.choices.contains(&choice) {
            return Err(deny(format!(
                "{choice:?} is not offered: {}",
                a.blockers.join("; ")
            )));
        }
        let id = id_to_text(attempt);
        let event = self.append_event(
            "RecoveryResolved",
            None,
            &format!("{id}|{}", choice.as_db()),
        )?;
        self.conn.execute(
            "INSERT INTO recovery_resolutions VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            &[
                Value::Int(to_db(event.0)?),
                Value::Text(&id),
                Value::Int(to_db(self.epoch.0)?),
                Value::Text(&subject.db_digest),
                Value::Text(&subject.findings),
                Value::Text(&a.blockers.join("\n")),
                Value::Text(choice.as_db()),
                Value::Text(&origin),
            ],
        )?;
        if choice == RecoveryChoice::Hold {
            return Ok("HOLD".into());
        }
        if !a.blockers.is_empty() || !a.process_absent {
            return Err(deny("absence is not proven"));
        }
        let prove = |tx: &mut Self, basis: &str| -> Result<(), TxError> {
            tx.conn.execute(
                "INSERT OR IGNORE INTO recovery_absence_proofs VALUES (?1,?2,?3,?4)",
                &[
                    Value::Text(&id),
                    Value::Int(to_db(tx.epoch.0)?),
                    Value::Int(to_db(event.0)?),
                    Value::Text(basis),
                ],
            )?;
            Ok(())
        };
        match subject.facts.stage {
            LaunchStage::Sealed => {
                let holder = AggregateKey::new("scheduler_attempt", attempt);
                let claims = crate::queries::live_claims(self.conn, None)?;
                for claim in claims.into_iter().filter(|c| c.holder == holder) {
                    self.release_claim(claim.id, ReleaseBasis::RecoveryResolved)?;
                }
                Ok("CLAIMS_RELEASED".into())
            }
            LaunchStage::NotIssued => {
                prove(self, "launch intent never issued")?;
                self.retire_unissued_reservation(attempt)?;
                Ok("NOT_STARTED".into())
            }
            LaunchStage::Spawned | LaunchStage::Concluded => {
                prove(self, &subject.findings)?;
                self.append_event("RecoveryAbsenceProven", None, &id)?;
                Ok("SETTLE".into())
            }
            LaunchStage::IssuedWithoutIdentity => Err(deny("absence is not proven")),
        }
    }

    /// Records a reviewed continuation plan; returns its digest (the LaunchSpec's
    /// `continuation.reviewed_plan` for an exact-history continuation).
    pub fn record_continuation_plan(
        &mut self,
        plan: &ContinuationPlan,
        origin: HumanOrigin,
    ) -> Result<String, TxError> {
        let origin = actor(&origin)?;
        let pred = id_to_text(plan.predecessor);
        if plan.remaining_work.trim().is_empty() {
            return Err(deny("remaining work must be stated"));
        }
        let (outcome, candidate) = self
            .conn
            .query_optional(
                "SELECT outcome,candidate FROM attempt_results WHERE attempt=?1",
                &[Value::Text(&pred)],
                |r| Ok((r.text(0)?, r.optional_text(1)?)),
            )?
            .ok_or_else(|| deny("predecessor result is not sealed"))?;
        if outcome == "SUCCEEDED" {
            return Err(deny("a succeeded attempt is validated, not continued"));
        }
        let (resolution, choice, blockers) = self
            .conn
            .query_optional(
                "SELECT event,choice,blockers FROM recovery_resolutions WHERE attempt=?1
                 ORDER BY event DESC LIMIT 1",
                &[Value::Text(&pred)],
                |r| Ok((r.int(0)?, r.text(1)?, r.text(2)?)),
            )?
            .ok_or_else(|| deny("predecessor has no recovery resolution"))?;
        let wanted = match plan.session {
            SessionPolicy::FreshContext => RecoveryChoice::ContinueFreshContext,
            SessionPolicy::ExactHistory(_) => RecoveryChoice::ContinueWithHistory,
        };
        if choice != wanted.as_db() || !blockers.is_empty() {
            return Err(deny(format!(
                "latest recovery resolution {choice} does not allow {wanted:?}"
            )));
        }
        let facts = db_facts(self.conn, &pred)?;
        if !facts.stale.is_empty() {
            return Err(deny(facts.stale.join("; ")));
        }
        let effects: BTreeSet<String> = text_rows(
            self.conn,
            "SELECT operation_key FROM attempt_effects WHERE attempt=?1",
            &pred,
        )?
        .into_iter()
        .collect();
        if !facts.unknown_effects.is_empty() {
            return Err(deny(format!(
                "effect outcome unknown: {}",
                facts.unknown_effects.join(", ")
            )));
        }
        if plan
            .reconciled_effects
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            != effects
            || plan.reconciled_effects.len() != effects.len()
        {
            return Err(deny(
                "plan must reconcile exactly the predecessor's effects",
            ));
        }
        let session = match &plan.session {
            SessionPolicy::FreshContext => FRESH.to_owned(),
            SessionPolicy::ExactHistory(s) => {
                let stored = format!("{:?}", EventPayload::SessionIdentified(s.clone()));
                if facts.sessions != [stored.clone()] {
                    return Err(deny(
                        "session is not the predecessor's one exact recorded session",
                    ));
                }
                format!("{EXACT}{stored}")
            }
        };
        let base = match &candidate {
            Some(d) => self.conn.query_optional(
                "SELECT head FROM worktree_candidates WHERE attempt=?1 AND digest=?2",
                &[Value::Text(&pred), Value::Text(d)],
                |r| r.text(0),
            )?,
            None => self.conn.query_optional(
                "SELECT base FROM owned_worktrees WHERE attempt=?1",
                &[Value::Text(&pred)],
                |r| r.text(0),
            )?,
        }
        .ok_or_else(|| deny("predecessor source is not recorded"))?;
        let effects_text = effects.into_iter().collect::<Vec<_>>().join("\n");
        let canonical = format!(
            "jojo-continuation-v1|{}|{pred}|{candidate:?}|{base}|{}|{effects_text}|{session}",
            id_to_text(plan.successor),
            plan.remaining_work
        );
        let digest = sha256_hex(&[canonical.as_bytes()]);
        let event = self.append_event(
            "ContinuationPlanRecorded",
            None,
            &format!("{}|{pred}|{digest}", id_to_text(plan.successor)),
        )?;
        self.conn.execute(
            "INSERT INTO continuation_plans VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            &[
                Value::Text(&id_to_text(plan.successor)),
                Value::Text(&pred),
                Value::Int(resolution),
                candidate.as_deref().map_or(Value::Null, Value::Text),
                Value::Text(&base),
                Value::Text(&plan.remaining_work),
                Value::Text(&effects_text),
                Value::Text(&session),
                Value::Text(&digest),
                Value::Text(&origin),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(digest)
    }
}

impl ReadView<'_> {
    /// Recorded choices for an attempt, oldest first.
    pub fn recovery_resolutions(&self, attempt: OpaqueId) -> Result<Vec<String>, StorageError> {
        text_rows(
            self.conn,
            "SELECT choice FROM recovery_resolutions WHERE attempt=?1 ORDER BY event",
            &id_to_text(attempt),
        )
    }

    pub fn recovery_started(&self) -> Result<bool, StorageError> {
        Ok(self
            .conn
            .query_optional(
                "SELECT 1 FROM recovery_starts WHERE epoch=(SELECT max(epoch) FROM engine_sessions)",
                &[],
                |r| r.int(0),
            )?
            .is_some())
    }

    /// The recorded plan whose successor is `successor`.
    pub fn continuation_plan(
        &self,
        successor: OpaqueId,
    ) -> Result<Option<StoredContinuationPlan>, StorageError> {
        self.conn
            .query_optional(
                "SELECT predecessor,candidate,base,digest FROM continuation_plans WHERE successor=?1",
                &[Value::Text(&id_to_text(successor))],
                |r| Ok((r.text(0)?, r.optional_text(1)?, r.text(2)?, r.text(3)?)),
            )?
            .map(|(p, candidate, base, digest)| {
                Ok(StoredContinuationPlan {
                    predecessor: id_from_text(&p)?,
                    candidate,
                    base,
                    digest,
                })
            })
            .transpose()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredContinuationPlan {
    pub predecessor: OpaqueId,
    pub candidate: Option<String>,
    pub base: String,
    pub digest: String,
}

#[derive(Debug)]
pub enum RecoveryOutcome {
    Held,
    NotStarted,
    ClaimsReleased,
    Settled(Settlement),
}

/// Resolves one subject and, when absence was proven, settles it through P1-10.
pub fn recover_attempt(
    ctx: &SettlementContext<'_>,
    inspection: &RecoveryInspection,
    attempt: OpaqueId,
    choice: RecoveryChoice,
    origin: HumanOrigin,
) -> Result<RecoveryOutcome, String> {
    let inspection = inspection.clone();
    let response = ctx.cmd("ResolveRecovery", attempt, move |tx| {
        tx.resolve_recovery(&inspection, attempt, choice, origin)
    })?;
    Ok(match response.as_str() {
        "HOLD" => RecoveryOutcome::Held,
        "NOT_STARTED" => RecoveryOutcome::NotStarted,
        "CLAIMS_RELEASED" => RecoveryOutcome::ClaimsReleased,
        _ => RecoveryOutcome::Settled(settle_attempt(ctx, attempt, BlobFaults::default())?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_exact_round_tripping_session_payloads_parse() {
        let s = SessionRef {
            provider: jojo_domain::provider::ProviderKind::Codex,
            session: "a\"b\\c".into(),
            turn: "t".into(),
        };
        let stored = format!("{:?}", EventPayload::SessionIdentified(s.clone()));
        assert_eq!(parse_session(&stored), Some(s));
        assert_eq!(parse_session(&stored.replace("Codex", "Latest")), None);
        assert_eq!(parse_session(&format!("{stored} ")), None);
        let control = SessionRef {
            provider: jojo_domain::provider::ProviderKind::ClaudeCode,
            session: "a\nb".into(),
            turn: "t".into(),
        };
        let stored = format!("{:?}", EventPayload::SessionIdentified(control));
        assert_eq!(
            parse_session(&stored),
            None,
            "non-trivial escapes are refused"
        );
    }
}

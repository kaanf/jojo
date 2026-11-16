//! Ordered attempt classification (P1-10). Pure: the storage layer supplies durable facts
//! (process observations, raw-stream coverage, provider terminal records, stop causes,
//! candidate and effect state); nothing here reads provider prose or live processes.
//!
//! Precedence: unproven process absence or lost evidence never finalizes; an explicit
//! stop committed before finalization outranks a later provider terminal; only a single
//! terminal success with exit zero, complete coverage, a durable candidate and no
//! unresolved effect yields `Succeeded`.
use crate::attempt::AttemptOutcome;
use crate::provider::TerminalStatus;

/// Why the engine stopped an attempt, recorded by the writer at the first stop request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StopCause {
    /// Explicit user task cancellation (step control or attempt cancel).
    TaskCancel,
    /// Revoked or stale approval made the attempt inapplicable.
    ApprovalInvalidated,
    /// Application close/shutdown coordinator.
    Shutdown,
    /// The supervisor itself stopped the process.
    Supervisor(SupervisorStop),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupervisorStop {
    Protocol,
    Deadline,
    EvidenceLoss,
    Descendants,
    /// Inspection failure, unreadable stop state or another engine-side reason.
    Other,
}

/// The concluding process observation, if any.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessConclusion {
    Unconcluded,
    /// Spawn reported failure after issue; a child may exist.
    SpawnUncertain,
    /// Issued, then proven never spawned.
    NotSpawned,
    /// Root reaped, owned group/known descendants absent, both streams closed.
    TreeAbsent,
    StopUnconfirmed,
    /// After an unclean helper exit, a later epoch's recovery inspection found every
    /// recorded identity absent (P1-11). Supervision never concluded, so the run can
    /// only be interrupted, cancelled or failed; never a success.
    RecoveredAbsent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Coverage {
    /// Every raw byte is checkpointed, spooled and referenced by verified evidence.
    Complete,
    /// A known policy cap discarded bytes; the discarded range is recorded.
    Truncated(String),
    /// Bytes may be missing for an unknown reason (spool/DB failure, missing or changed
    /// evidence). Requires recovery classification before any result.
    Lost(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExitStatus {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

impl ExitStatus {
    pub fn is_zero(self) -> bool {
        self.code == Some(0) && self.signal.is_none()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateEvidence {
    Recorded(String),
    Unavailable(String),
    /// No process ran, so there is no candidate to collect.
    NotApplicable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementFacts {
    pub conclusion: ProcessConclusion,
    /// First durable stop cause; `None` when no stop was ever requested.
    pub stop: Option<StopCause>,
    /// A stop exists but its cause was not recorded (cannot be ordered safely).
    pub stop_cause_unknown: bool,
    pub terminals: Vec<TerminalStatus>,
    pub exit: Option<ExitStatus>,
    pub protocol_failed: bool,
    pub coverage: Coverage,
    pub candidate: CandidateEvidence,
    /// Effects whose external outcome is unknown (ISSUED_UNKNOWN).
    pub unresolved_effects: Vec<String>,
    /// Journaled effects never issued; they will not be dispatched after sealing.
    pub abandoned_effects: Vec<String>,
    /// Required outputs of the approved plan this engine cannot produce.
    pub missing_outputs: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Classification {
    Final {
        outcome: AttemptOutcome,
        reason: &'static str,
        uncertainties: Vec<String>,
    },
    RecoveryRequired(Vec<String>),
}

fn fin(outcome: AttemptOutcome, reason: &'static str, u: Vec<String>) -> Classification {
    Classification::Final {
        outcome,
        reason,
        uncertainties: u,
    }
}

fn recovery(reason: impl Into<String>) -> Classification {
    Classification::RecoveryRequired(vec![reason.into()])
}

/// The helper died before supervision concluded. Every gap (no wait status, unclosed
/// streams, lost coverage, an earlier provider terminal) is kept as an uncertainty.
fn recovered(f: &SettlementFacts) -> Classification {
    use AttemptOutcome::*;
    if !f.unresolved_effects.is_empty() {
        return recovery(format!(
            "effect outcome unknown: {}",
            f.unresolved_effects.join(", ")
        ));
    }
    let mut u = vec!["helper exited before supervision concluded".to_owned()];
    if f.stop_cause_unknown {
        u.push("stop recorded without a cause".into());
    }
    match &f.coverage {
        Coverage::Lost(why) => u.push(format!("evidence coverage lost: {why}")),
        Coverage::Truncated(why) => u.push(format!("output truncated: {why}")),
        Coverage::Complete => {}
    }
    if let CandidateEvidence::Unavailable(why) = &f.candidate {
        u.push(format!("candidate unavailable: {why}"));
    }
    if !f.terminals.is_empty() {
        u.push(format!("provider terminal evidence: {:?}", f.terminals));
    }
    u.extend(
        f.abandoned_effects
            .iter()
            .map(|k| format!("unissued effect abandoned: {k}")),
    );
    match f.stop {
        Some(StopCause::TaskCancel) => fin(Cancelled, "cancelled_before_helper_loss", u),
        Some(StopCause::Supervisor(_)) => fin(Failed, "supervisor_stop_before_helper_loss", u),
        Some(StopCause::ApprovalInvalidated | StopCause::Shutdown) | None => {
            fin(Interrupted, "helper_lost_recovered", u)
        }
    }
}

pub fn classify(f: &SettlementFacts) -> Classification {
    use AttemptOutcome::*;
    match f.conclusion {
        ProcessConclusion::Unconcluded => return recovery("process supervision not concluded"),
        ProcessConclusion::SpawnUncertain => return recovery("spawn outcome uncertain"),
        ProcessConclusion::StopUnconfirmed => return recovery("process absence unconfirmed"),
        ProcessConclusion::RecoveredAbsent => return recovered(f),
        ProcessConclusion::NotSpawned | ProcessConclusion::TreeAbsent => {}
    }
    if f.stop_cause_unknown {
        return recovery("stop recorded without a cause; ordering is unknown");
    }
    if let Coverage::Lost(why) = &f.coverage {
        return recovery(format!("evidence coverage lost: {why}"));
    }
    let mut u: Vec<String> = f
        .unresolved_effects
        .iter()
        .map(|k| format!("effect outcome unknown: {k}"))
        .chain(
            f.abandoned_effects
                .iter()
                .map(|k| format!("unissued effect abandoned: {k}")),
        )
        .collect();
    if let Coverage::Truncated(why) = &f.coverage {
        u.push(format!("output truncated: {why}"));
    }
    if let CandidateEvidence::Unavailable(why) = &f.candidate {
        u.push(format!("candidate unavailable: {why}"));
    }
    if !f.terminals.is_empty() {
        u.push(format!("provider terminal evidence: {:?}", f.terminals));
    }
    if f.conclusion == ProcessConclusion::NotSpawned {
        return match f.stop {
            Some(StopCause::TaskCancel) => fin(Cancelled, "cancelled_before_spawn", u),
            Some(StopCause::ApprovalInvalidated | StopCause::Shutdown) => {
                fin(Interrupted, "interrupted_before_spawn", u)
            }
            Some(StopCause::Supervisor(_)) | None => fin(Failed, "launch_failure", u),
        };
    }
    match f.stop {
        Some(StopCause::TaskCancel) => return fin(Cancelled, "task_cancelled", u),
        Some(StopCause::ApprovalInvalidated) => return fin(Interrupted, "approval_invalidated", u),
        Some(StopCause::Shutdown) => return fin(Interrupted, "shutdown", u),
        Some(StopCause::Supervisor(s)) => {
            return match s {
                SupervisorStop::Protocol => fin(Failed, "protocol_violation", u),
                SupervisorStop::Deadline => fin(Failed, "execution_deadline", u),
                SupervisorStop::Descendants => fin(Failed, "descendants_outlived_root", u),
                SupervisorStop::EvidenceLoss => recovery("supervisor stopped on evidence loss"),
                SupervisorStop::Other => fin(Interrupted, "supervisor_stop", u),
            };
        }
        None => {}
    }
    if f.protocol_failed || f.terminals.len() > 1 {
        return fin(Failed, "protocol_violation", u);
    }
    if matches!(f.coverage, Coverage::Truncated(_)) {
        return fin(Failed, "output_truncated", u);
    }
    let Some(exit) = f.exit else {
        return recovery("root wait status unknown");
    };
    match f.terminals.first() {
        None => fin(Failed, "protocol_incomplete", u),
        Some(TerminalStatus::Error) => fin(Failed, "provider_error", u),
        Some(TerminalStatus::Interrupted) => fin(Failed, "provider_interrupted", u),
        Some(TerminalStatus::Unknown) => fin(Failed, "provider_status_unknown", u),
        Some(TerminalStatus::Success) if !exit.is_zero() => fin(Failed, "exit_mismatch", u),
        Some(TerminalStatus::Success) => {
            if !f.unresolved_effects.is_empty() {
                return Classification::RecoveryRequired(
                    f.unresolved_effects
                        .iter()
                        .map(|k| format!("effect outcome unknown: {k}"))
                        .collect(),
                );
            }
            if let CandidateEvidence::Unavailable(why) = &f.candidate {
                return recovery(format!("durable candidate unavailable: {why}"));
            }
            if f.candidate == CandidateEvidence::NotApplicable {
                return recovery("durable candidate missing");
            }
            if !f.missing_outputs.is_empty() {
                return fin(Failed, "required_output_unavailable", u);
            }
            fin(Succeeded, "terminal_success", u)
        }
    }
}

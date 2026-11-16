//! Reopen assessment (P1-11). Pure: storage supplies durable facts plus fresh OS and
//! filesystem findings; this decides what blocks and which reviewed choices are offered.
//!
//! Absence is proven only by inspection findings or a durable conclusion. No input
//! represents a human "assume stopped" confirmation, so none can substitute for it. A
//! live, reused, unknown or unrecorded process identity, a changed or missing worktree,
//! a stale approval or an effect with unknown outcome blocks everything except Hold.
use crate::provider::SessionRef;

/// Fresh inspection of one recorded process identity (or of a recorded process group).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessFinding {
    Absent,
    /// The recorded identity is still running. Never signalled without a lifetime handle.
    Alive,
    /// The PID now names another process; the recorded identity cannot be re-verified.
    Reused,
    Unknown(String),
}

/// How far the old attempt got, from durable records only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaunchStage {
    /// Reserved; the launch intent is provably NOT_ISSUED.
    NotIssued,
    /// Issued (or spawn reported failure) without a registered root identity.
    IssuedWithoutIdentity,
    /// A root identity was registered; supervision did not conclude absence.
    Spawned,
    /// Supervision durably concluded TREE_ABSENT or NOT_SPAWNED.
    Concluded,
    /// A result is already sealed.
    Sealed,
}

/// Whether the exact recorded provider session could be continued from a new worktree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionContinuity {
    Exact(SessionRef),
    NotRecorded,
    Unsupported(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryFacts {
    pub stage: LaunchStage,
    /// `(label, finding)` per recorded identity and recorded process group.
    pub processes: Vec<(String, ProcessFinding)>,
    /// `Err(reason)` when the owned worktree or its repository is missing or changed.
    pub worktree: Result<(), String>,
    pub stale_approvals: Vec<String>,
    pub unknown_effects: Vec<String>,
    /// Gaps in raw-log or blob coverage. Reported; they can only weaken the result.
    pub evidence_gaps: Vec<String>,
    /// Digest of a reviewed snapshot (recorded or freshly captured), if one exists.
    pub candidate: Option<String>,
    pub session: SessionContinuity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum RecoveryChoice {
    ValidateOutput,
    ContinueWithHistory,
    ContinueFreshContext,
    Hold,
}

impl RecoveryChoice {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::ValidateOutput => "VALIDATE_OUTPUT",
            Self::ContinueWithHistory => "CONTINUE_WITH_HISTORY",
            Self::ContinueFreshContext => "CONTINUE_FRESH_CONTEXT",
            Self::Hold => "HOLD",
        }
    }
    /// The name the protocol uses for this choice, in both directions.
    pub fn wire(self) -> &'static str {
        match self {
            Self::ValidateOutput => "ValidateOutput",
            Self::ContinueWithHistory => "ContinueWithHistory",
            Self::ContinueFreshContext => "ContinueFreshContext",
            Self::Hold => "Hold",
        }
    }
    pub fn from_wire(name: &str) -> Option<Self> {
        [
            Self::ValidateOutput,
            Self::ContinueWithHistory,
            Self::ContinueFreshContext,
            Self::Hold,
        ]
        .into_iter()
        .find(|c| c.wire() == name)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Assessment {
    pub blockers: Vec<String>,
    /// Every process that could belong to the attempt is proven gone.
    pub process_absent: bool,
    pub choices: Vec<RecoveryChoice>,
}

pub fn assess(f: &RecoveryFacts) -> Assessment {
    let mut blockers = Vec::new();
    let process_absent = match f.stage {
        LaunchStage::NotIssued | LaunchStage::Concluded | LaunchStage::Sealed => true,
        LaunchStage::IssuedWithoutIdentity => {
            blockers.push(
                "launch issued without a recorded process identity; quarantined until OS \
                 evidence resolves it"
                    .to_owned(),
            );
            false
        }
        LaunchStage::Spawned => {
            !f.processes.is_empty()
                && f.processes
                    .iter()
                    .all(|(_, p)| *p == ProcessFinding::Absent)
        }
    };
    if f.stage == LaunchStage::Spawned && f.processes.is_empty() {
        blockers.push("no recorded process identity to inspect".into());
    }
    for (label, finding) in &f.processes {
        match finding {
            ProcessFinding::Absent => {}
            ProcessFinding::Alive => blockers.push(format!(
                "live orphan {label}: quarantined, never signalled without a lifetime handle"
            )),
            ProcessFinding::Reused => blockers.push(format!(
                "PID reused for {label}: recorded identity cannot be re-verified"
            )),
            ProcessFinding::Unknown(why) => {
                blockers.push(format!("process inspection unknown for {label}: {why}"))
            }
        }
    }
    if let Err(why) = &f.worktree {
        blockers.push(format!("worktree: {why}"));
    }
    blockers.extend(
        f.stale_approvals
            .iter()
            .map(|a| format!("stale approval: {a}")),
    );
    blockers.extend(
        f.unknown_effects
            .iter()
            .map(|k| format!("effect outcome unknown: {k}")),
    );
    let mut choices = Vec::new();
    if blockers.is_empty() {
        if f.candidate.is_some() {
            choices.push(RecoveryChoice::ValidateOutput);
        }
        if matches!(f.session, SessionContinuity::Exact(_)) {
            choices.push(RecoveryChoice::ContinueWithHistory);
        }
        choices.push(RecoveryChoice::ContinueFreshContext);
    }
    choices.push(RecoveryChoice::Hold);
    Assessment {
        blockers,
        process_absent: process_absent && f.worktree.is_ok(),
        choices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderKind;

    fn dead() -> RecoveryFacts {
        RecoveryFacts {
            stage: LaunchStage::Spawned,
            processes: vec![
                ("root".into(), ProcessFinding::Absent),
                ("group".into(), ProcessFinding::Absent),
            ],
            worktree: Ok(()),
            stale_approvals: vec![],
            unknown_effects: vec![],
            evidence_gaps: vec!["STDOUT spool shorter than checkpoint".into()],
            candidate: Some("c".into()),
            session: SessionContinuity::NotRecorded,
        }
    }

    #[test]
    fn confirmed_dead_offers_reviewed_choices_and_evidence_gaps_do_not_block() {
        let a = assess(&dead());
        assert!(a.blockers.is_empty() && a.process_absent);
        use RecoveryChoice::*;
        assert_eq!(a.choices, vec![ValidateOutput, ContinueFreshContext, Hold]);
        let mut f = dead();
        f.session = SessionContinuity::Exact(SessionRef {
            provider: ProviderKind::ClaudeCode,
            session: "s".into(),
            turn: "t".into(),
        });
        f.candidate = None;
        assert_eq!(
            assess(&f).choices,
            vec![ContinueWithHistory, ContinueFreshContext, Hold]
        );
    }

    #[test]
    fn live_reused_unknown_or_unrecorded_processes_block_all_but_hold() {
        for finding in [
            ProcessFinding::Alive,
            ProcessFinding::Reused,
            ProcessFinding::Unknown("ps failed".into()),
        ] {
            let mut f = dead();
            f.processes[0].1 = finding;
            let a = assess(&f);
            assert!(!a.process_absent && a.blockers.len() == 1, "{a:?}");
            assert_eq!(a.choices, vec![RecoveryChoice::Hold]);
        }
        let mut f = dead();
        f.stage = LaunchStage::IssuedWithoutIdentity;
        f.processes.clear();
        let a = assess(&f);
        assert!(!a.process_absent && a.choices == vec![RecoveryChoice::Hold]);
        f.stage = LaunchStage::Spawned;
        assert!(!assess(&f).process_absent);
    }

    #[test]
    fn missing_worktree_stale_approval_and_unknown_effect_each_block() {
        let mut f = dead();
        f.worktree = Err("repository moved".into());
        let a = assess(&f);
        assert!(!a.process_absent && a.choices == vec![RecoveryChoice::Hold]);
        let mut f = dead();
        f.stale_approvals = vec!["plan".into()];
        assert_eq!(assess(&f).choices, vec![RecoveryChoice::Hold]);
        let mut f = dead();
        f.unknown_effects = vec!["attempt-effect:pr".into()];
        assert_eq!(assess(&f).choices, vec![RecoveryChoice::Hold]);
    }

    #[test]
    fn absent_or_unsupported_session_offers_fresh_context_only() {
        for session in [
            SessionContinuity::NotRecorded,
            SessionContinuity::Unsupported("cwd change unsupported".into()),
        ] {
            let mut f = dead();
            f.candidate = None;
            f.session = session;
            assert_eq!(
                assess(&f).choices,
                vec![RecoveryChoice::ContinueFreshContext, RecoveryChoice::Hold]
            );
        }
    }

    #[test]
    fn recovered_absence_never_classifies_as_success() {
        use crate::attempt::AttemptOutcome;
        use crate::outcome::*;
        use crate::provider::TerminalStatus;
        let mut f = SettlementFacts {
            conclusion: ProcessConclusion::RecoveredAbsent,
            stop: None,
            stop_cause_unknown: false,
            terminals: vec![TerminalStatus::Success],
            exit: None,
            protocol_failed: false,
            coverage: Coverage::Lost("STDOUT close/checkpoint coverage mismatch".into()),
            candidate: CandidateEvidence::Recorded("c".into()),
            unresolved_effects: vec![],
            abandoned_effects: vec![],
            missing_outputs: vec![],
        };
        let Classification::Final {
            outcome,
            reason,
            uncertainties,
        } = classify(&f)
        else {
            panic!("recovered absence must finalize");
        };
        assert_eq!(
            (outcome, reason),
            (AttemptOutcome::Interrupted, "helper_lost_recovered")
        );
        assert_eq!(uncertainties.len(), 3, "{uncertainties:?}");
        f.stop = Some(StopCause::TaskCancel);
        assert!(matches!(
            classify(&f),
            Classification::Final {
                outcome: AttemptOutcome::Cancelled,
                ..
            }
        ));
        f.unresolved_effects = vec!["attempt-effect:x".into()];
        assert!(matches!(classify(&f), Classification::RecoveryRequired(_)));
    }
}

//! Close-ticket and shutdown verdict (P1-12). Pure: storage supplies durable per-target
//! facts; this derives the close state and whether a clean shutdown may be acknowledged.
//!
//! A stop request, a delivered signal or an elapsed budget is never proof. Only a durable
//! tree-absence/not-spawned conclusion or a retired never-issued reservation counts as
//! stopped, and only a sealed result with intact evidence counts as persisted. Anything
//! else fails the close: there is no input for "assume stopped".

/// Who started the close. A disconnect (GUI stdio EOF) cannot confirm anything, so its
/// session is never sealed clean even when every target stopped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseOrigin {
    User,
    Disconnect,
}

/// Durable resolution of one close ticket.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseResolution {
    Cancelled,
    Confirmed,
    NoActive,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShutdownOutcome {
    /// Clean: every local process confirmed stopped and persistence sealed.
    Ready,
    Failed,
    /// Disconnect stop that confirmed and sealed everything; the session stays unclean.
    Emergency,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseState {
    Open,
    AwaitingConfirmation,
    Stopping,
    Finalizing,
    Failed,
    Ready,
    Emergency,
}

/// The close state of the newest ticket of the current epoch.
pub fn close_state(
    resolution: Option<Option<CloseResolution>>,
    last_receipt: Option<ShutdownOutcome>,
) -> CloseState {
    match (resolution, last_receipt) {
        (None, _) | (Some(Some(CloseResolution::Cancelled)), _) => CloseState::Open,
        (Some(None), _) => CloseState::AwaitingConfirmation,
        (Some(Some(_)), Some(ShutdownOutcome::Ready)) => CloseState::Ready,
        (Some(Some(_)), Some(ShutdownOutcome::Emergency)) => CloseState::Emergency,
        (Some(Some(_)), Some(ShutdownOutcome::Failed)) => CloseState::Failed,
        (Some(Some(CloseResolution::NoActive)), None) => CloseState::Finalizing,
        (Some(Some(CloseResolution::Confirmed)), None) => CloseState::Stopping,
    }
}

/// What a target's durable records prove about its processes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetStop {
    /// No conclusion yet: the runner may still be escalating, or nobody is supervising.
    Stopping,
    /// The reservation was retired before its launch was issued; no process existed.
    NeverStarted,
    /// Tree absence or not-spawned was proven. `forced` records a sent KILL.
    Stopped { forced: bool },
    /// An owned or potentially owned process could not be confirmed stopped.
    Unconfirmed(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TargetFacts {
    pub stop: TargetStop,
    pub sealed: bool,
    pub evidence_lost: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShutdownVerdict {
    pub outcome: ShutdownOutcome,
    pub process_stop_confirmed: bool,
    pub persistence_sealed: bool,
    pub failures: Vec<String>,
}

/// `unconfirmed` lists other processes not proven stopped (for example an earlier-epoch
/// orphan without an absence proof); `unsealed` lists other unsettled durable state.
pub fn verdict(
    origin: CloseOrigin,
    targets: &[(String, TargetFacts)],
    unconfirmed: &[String],
    unsealed: &[String],
) -> ShutdownVerdict {
    let mut failures = Vec::new();
    let mut stopped = unconfirmed.is_empty();
    let mut sealed = unsealed.is_empty();
    for (id, t) in targets {
        match &t.stop {
            TargetStop::Stopping => {
                stopped = false;
                failures.push(format!("{id}: stop not concluded"));
            }
            TargetStop::Unconfirmed(why) => {
                stopped = false;
                failures.push(format!("{id}: stop unconfirmed: {why}"));
            }
            TargetStop::NeverStarted | TargetStop::Stopped { .. } => {}
        }
        if !t.sealed {
            sealed = false;
            failures.push(format!("{id}: result not sealed"));
        }
        if t.evidence_lost {
            sealed = false;
            failures.push(format!("{id}: evidence persistence failed"));
        }
    }
    failures.extend(unconfirmed.iter().cloned());
    failures.extend(unsealed.iter().cloned());
    let outcome = match (failures.is_empty(), origin) {
        (false, _) => ShutdownOutcome::Failed,
        (true, CloseOrigin::User) => ShutdownOutcome::Ready,
        (true, CloseOrigin::Disconnect) => ShutdownOutcome::Emergency,
    };
    ShutdownVerdict {
        outcome,
        process_stop_confirmed: stopped,
        persistence_sealed: sealed,
        failures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(stop: TargetStop, sealed: bool) -> (String, TargetFacts) {
        (
            "a".into(),
            TargetFacts {
                stop,
                sealed,
                evidence_lost: false,
            },
        )
    }

    #[test]
    fn only_proven_stop_and_sealed_persistence_acknowledge_a_clean_close() {
        let clean = [
            target(TargetStop::Stopped { forced: true }, true),
            target(TargetStop::NeverStarted, true),
        ];
        let v = verdict(CloseOrigin::User, &clean, &[], &[]);
        assert_eq!(v.outcome, ShutdownOutcome::Ready);
        assert!(v.process_stop_confirmed && v.persistence_sealed && v.failures.is_empty());
        assert_eq!(
            verdict(CloseOrigin::User, &[], &[], &[]).outcome,
            ShutdownOutcome::Ready
        );
        for stop in [
            TargetStop::Stopping,
            TargetStop::Unconfirmed("group alive".into()),
        ] {
            let v = verdict(CloseOrigin::User, &[target(stop, false)], &[], &[]);
            assert_eq!(v.outcome, ShutdownOutcome::Failed);
            assert!(!v.process_stop_confirmed && !v.persistence_sealed);
        }
    }

    #[test]
    fn disk_failure_or_unsettled_state_fails_even_when_processes_stopped() {
        let mut lost = target(TargetStop::Stopped { forced: false }, true);
        lost.1.evidence_lost = true;
        let v = verdict(CloseOrigin::User, &[lost], &[], &[]);
        assert_eq!(v.outcome, ShutdownOutcome::Failed);
        assert!(v.process_stop_confirmed && !v.persistence_sealed);
        let v = verdict(
            CloseOrigin::User,
            &[],
            &[],
            &["1 live claim remains".into()],
        );
        assert_eq!(v.outcome, ShutdownOutcome::Failed);
        let v = verdict(CloseOrigin::User, &[], &["orphan".into()], &[]);
        assert!(!v.process_stop_confirmed && v.outcome == ShutdownOutcome::Failed);
    }

    #[test]
    fn a_disconnect_is_never_a_clean_ready_close() {
        let t = [target(TargetStop::Stopped { forced: false }, true)];
        assert_eq!(
            verdict(CloseOrigin::Disconnect, &t, &[], &[]).outcome,
            ShutdownOutcome::Emergency
        );
        let t = [target(TargetStop::Stopping, false)];
        assert_eq!(
            verdict(CloseOrigin::Disconnect, &t, &[], &[]).outcome,
            ShutdownOutcome::Failed
        );
    }

    #[test]
    fn close_state_follows_the_ticket_and_its_receipts() {
        use CloseResolution::*;
        assert_eq!(close_state(None, None), CloseState::Open);
        assert_eq!(
            close_state(Some(None), None),
            CloseState::AwaitingConfirmation
        );
        assert_eq!(close_state(Some(Some(Cancelled)), None), CloseState::Open);
        assert_eq!(
            close_state(Some(Some(NoActive)), None),
            CloseState::Finalizing
        );
        assert_eq!(
            close_state(Some(Some(Confirmed)), None),
            CloseState::Stopping
        );
        assert_eq!(
            close_state(Some(Some(Confirmed)), Some(ShutdownOutcome::Failed)),
            CloseState::Failed
        );
        assert_eq!(
            close_state(Some(Some(Confirmed)), Some(ShutdownOutcome::Ready)),
            CloseState::Ready
        );
    }
}

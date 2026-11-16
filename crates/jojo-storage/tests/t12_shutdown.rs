//! T12 close tickets and the shutdown coordinator. Real OS processes (only the workspace's
//! `jojo-process-fixture`) run through the P1-09 runner on threads, as the helper's workers
//! would; temporary local Git repositories; no provider CLI, credential, network or remote
//! effect. Waits are event barriers; durations are only failure bounds or stop budgets.
mod common;
use common::attempts::*;
use jojo_domain::attempt::AttemptOutcome;
use jojo_domain::identity::*;
use jojo_domain::provider::*;
use jojo_domain::shutdown::{CloseState, ShutdownOutcome, TargetStop};
use jojo_storage::*;
use std::fs;
use std::sync::mpsc;
use std::time::{Duration, Instant};

const FAST: StopBudgets = StopBudgets {
    interrupt: Duration::from_millis(300),
    terminate: Duration::from_millis(300),
    kill: Duration::from_secs(5),
    drain: Duration::from_millis(300),
};

fn user() -> HumanOrigin {
    human("T12 user")
}

/// Hooks that report once the provider session is identified (the process is RUNNING).
fn on_session(tx: mpsc::Sender<u128>, a: u128) -> LaunchHooks {
    LaunchHooks {
        on_event: Some(Box::new(move |p| {
            if matches!(p, EventPayload::SessionIdentified(_)) {
                let _ = tx.send(a);
            }
        })),
        ..LaunchHooks::default()
    }
}

fn recv(rx: &mpsc::Receiver<u128>) -> u128 {
    rx.recv_timeout(Duration::from_secs(20)).unwrap()
}

impl F {
    fn prepare(&self) -> CloseStatus {
        let ids = self.ids();
        prepare_close(&self.ctx(&ids), user()).unwrap()
    }

    fn confirm(&self, ticket: OpaqueId) -> Result<CloseStatus, String> {
        let ids = self.ids();
        confirm_close(&self.ctx(&ids), ticket, user())
    }

    fn cancel(&self, ticket: OpaqueId) -> Result<CloseStatus, String> {
        let ids = self.ids();
        cancel_close(&self.ctx(&ids), ticket, user())
    }

    fn finish(&self, disconnected: bool) -> Result<ShutdownReceipt, String> {
        let ids = self.ids();
        let wait = CloseWait {
            budget: Duration::from_secs(20),
            poll: Duration::from_millis(10),
        };
        finish_close(
            &self.ctx(&ids),
            &RealClock(Instant::now()),
            wait,
            disconnected,
        )
    }

    fn status(&self) -> CloseStatus {
        let ids = self.ids();
        close_status(&self.ctx(&ids)).unwrap()
    }

    fn settle(&self, a: u128) -> AttemptResult {
        let ids = self.ids();
        match settle_attempt(&self.ctx(&ids), OpaqueId(a), BlobFaults::default()).unwrap() {
            Settlement::Sealed(r) => *r,
            other => panic!("expected a sealed result, got {other:?}"),
        }
    }

    fn effective_admission(&self) -> String {
        self.cmd(|tx| Ok(format!("{:?}", tx.admission()?)))
            .unwrap()
            .response
    }

    fn kinds(&self, a: u128) -> Vec<String> {
        self.read(|v| v.process_observations(OpaqueId(a)))
            .into_iter()
            .map(|(k, _)| k)
            .collect()
    }

    fn session(&self) -> EngineSession {
        self.read(|v| v.session())
    }
}

fn ticket(s: &CloseStatus) -> OpaqueId {
    s.ticket.as_ref().unwrap().ticket
}

fn active(s: &CloseStatus) -> Vec<u128> {
    s.active.iter().map(|w| w.attempt.0).collect()
}

fn interrupted_by_shutdown(r: &AttemptResult) {
    assert_eq!(r.outcome, AttemptOutcome::Interrupted, "{r:?}");
    assert!(
        r.stop_cause.as_deref().unwrap().contains("shutdown"),
        "{r:?}"
    );
}

#[test]
fn t12_idle_close_with_queued_only_work_needs_no_confirmation_and_seals_clean() {
    let mut f = F::new(2);
    f.run(101, "edit write:src/done.rs");
    assert_eq!(f.settle(101).outcome, AttemptOutcome::Succeeded);
    // Step 2 stays queued-only: its reservation is retired before any launch issue.
    f.cmd(|tx| {
        tx.retire_unissued_reservation(OpaqueId(102))?;
        Ok("retired".into())
    })
    .unwrap();
    let queue = f.read(|v| v.scheduler_queue());
    let s = f.prepare();
    assert_eq!(s.state, CloseState::Finalizing, "no confirmation: {s:?}");
    assert!(s.active.is_empty() && s.targets.is_empty());
    let r = f.finish(false).unwrap();
    assert_eq!(r.outcome, ShutdownOutcome::Ready);
    assert!(r.process_stop_confirmed && r.persistence_sealed && r.failures.is_empty());
    assert_eq!(r.ticket, ticket(&s));
    assert_eq!(f.session().shutdown, ShutdownMarker::Clean);
    // The clean receipt ended this writer: nothing else can be admitted or recorded.
    assert!(matches!(
        f.cmd(|_| Ok("late".into())),
        Err(CommandError::StaleEpoch { .. })
    ));
    assert_eq!(f.read(|v| v.scheduler_queue()), queue);
    f.restart();
    assert_eq!(
        f.e().startup_report().epoch.previous,
        Some((EngineEpoch(1), ShutdownMarker::Clean))
    );
    assert_eq!(f.admission(), Admission::Closed);
    assert_eq!(f.read(|v| v.scheduler_queue()), queue);
    assert_eq!(f.status().state, CloseState::Open);
}

#[test]
fn t12_active_count_changes_while_open_and_cancel_close_sends_nothing() {
    let f = F::new(2);
    // 101 exited but is still settling, so it is active work.
    f.run(101, "edit write:src/a.rs");
    let (tx, rx) = mpsc::channel();
    let report = std::thread::scope(|s| {
        let h = s.spawn(|| f.run_with(102, "wait-release", on_session(tx, 102)));
        recv(&rx);
        let first = f.prepare();
        assert_eq!(first.state, CloseState::AwaitingConfirmation);
        assert_eq!(active(&first), vec![101, 102]);
        assert_eq!(f.effective_admission(), "Closed");
        // A repeated quit shares the same operation.
        assert_eq!(ticket(&f.prepare()), ticket(&first));
        // Work finishing while the dialog is open changes the count, never confirms.
        assert_eq!(f.settle(101).outcome, AttemptOutcome::Succeeded);
        let now = f.status();
        assert_eq!(now.state, CloseState::AwaitingConfirmation);
        assert_eq!(active(&now), vec![102]);
        let queue = f.read(|v| v.scheduler_queue());
        let cancelled = f.cancel(ticket(&first)).unwrap();
        assert_eq!(cancelled.state, CloseState::Open);
        assert_eq!(f.admission(), Admission::Open);
        assert_eq!(f.effective_admission(), "Open");
        assert_eq!(f.read(|v| v.scheduler_queue()), queue);
        // The cancelled ticket expired: a late confirmation or second cancel is refused.
        for e in [f.confirm(ticket(&first)), f.cancel(ticket(&first))] {
            assert!(e.unwrap_err().contains("expired"));
        }
        let k = f.kinds(102);
        assert!(
            !k.iter()
                .any(|k| k == "STOP_REQUESTED" || k == "SIGNAL_SENT"),
            "{k:?}"
        );
        fs::write(f.barriers().join("release"), b"").unwrap();
        h.join().unwrap()
    });
    assert_eq!(
        report.outcome,
        LaunchOutcome::TreeAbsent {
            stop_requested: false,
            forced: false
        }
    );
    assert!(report.signals.is_empty());
    assert!(f.settle(102).stop_cause.is_none());
}

#[test]
fn t12_confirm_stops_two_provider_trees_and_a_test_worker_without_task_cancel() {
    let mut f = F::with_providers(&[
        ProviderKind::ClaudeCode,
        ProviderKind::Codex,
        ProviderKind::ClaudeCode,
    ]);
    let mut specs = [
        f.spec(101, 1, "wait-release"),
        f.spec(102, 2, "codex-wait-release"),
        f.spec(103, 3, "wait-release"),
    ];
    specs[2].role = Role::Validator;
    let (tx, rx) = mpsc::channel();
    let reports: Vec<LaunchReport> = std::thread::scope(|s| {
        let handles: Vec<_> = specs
            .iter()
            .map(|spec| {
                let hooks = on_session(tx.clone(), spec.attempt.0.0);
                let f = &f;
                s.spawn(move || f.run_spec(spec, hooks, FAST))
            })
            .collect();
        let mut running: Vec<u128> = (0..3).map(|_| recv(&rx)).collect();
        running.sort();
        assert_eq!(running, vec![101, 102, 103]);
        let s = f.prepare();
        assert_eq!(active(&s), vec![101, 102, 103]);
        let c = f.confirm(ticket(&s)).unwrap();
        assert_eq!(c.state, CloseState::Stopping);
        assert_eq!(c.targets.len(), 3);
        // A confirmed close cannot be cancelled: the stop is already committed.
        assert!(
            f.cancel(ticket(&s))
                .unwrap_err()
                .contains("cannot be undone")
        );
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    for r in &reports {
        assert_eq!(
            r.outcome,
            LaunchOutcome::TreeAbsent {
                stop_requested: true,
                forced: false
            }
        );
    }
    let r = f.finish(false).unwrap();
    assert_eq!(r.outcome, ShutdownOutcome::Ready, "{r:?}");
    assert_eq!(r.targets.len(), 3);
    assert_eq!(r.recovery_items.len(), 3, "interrupted work is preserved");
    for a in [101, 102, 103] {
        interrupted_by_shutdown(&f.result(a).unwrap());
    }
    // Interruption for shutdown is not a permanent task cancellation.
    assert!(
        f.read(|v| v.scheduler_queue())
            .iter()
            .all(|q| q.intent != "CANCEL")
    );
    assert_eq!(f.session().shutdown, ShutdownMarker::Clean);
    f.restart();
    assert!(f.read(|v| v.live_claims()).is_empty());
    assert_eq!(f.read(|v| v.scheduler_queue()).len(), 3);
}

#[test]
fn t12_graceful_timeout_then_verified_kill_is_a_visible_escalation() {
    let f = F::new(1);
    let (tx, rx) = mpsc::channel();
    let spec = f.spec(101, 1, "ignore-signals");
    let report = std::thread::scope(|s| {
        let h = s.spawn(|| f.run_spec(&spec, on_session(tx, 101), FAST));
        recv(&rx);
        let s = f.prepare();
        f.confirm(ticket(&s)).unwrap();
        h.join().unwrap()
    });
    assert_eq!(
        report.outcome,
        LaunchOutcome::TreeAbsent {
            stop_requested: true,
            forced: true
        }
    );
    let r = f.finish(false).unwrap();
    assert_eq!(r.outcome, ShutdownOutcome::Ready);
    assert!(r.targets[0].contains("Stopped { forced: true }"), "{r:?}");
    let obs = f.read(|v| v.process_observations(OpaqueId(101)));
    assert!(
        obs.iter()
            .any(|(k, d)| k == "TREE_ABSENT" && d == "graceful stop failed; forced stop confirmed")
    );
    interrupted_by_shutdown(&f.result(101).unwrap());
}

#[test]
fn t12_unconfirmed_descendant_stop_fails_the_close_and_keeps_claims_and_gate() {
    let f = F::new(1);
    let barriers = f.barriers();
    let go = barriers.join("go");
    let (tx, rx) = mpsc::channel();
    let hooks = LaunchHooks {
        on_event: Some(Box::new(move |p| match p {
            EventPayload::SessionIdentified(_) => fs::write(&go, b"").unwrap(),
            // The escaped holder exists before the root reports its result.
            EventPayload::ProviderTerminal { .. } => {
                let _ = tx.send(101);
            }
            _ => {}
        })),
        ..LaunchHooks::default()
    };
    let spec = f.spec(101, 1, "escaped-holder");
    let report = std::thread::scope(|s| {
        let h = s.spawn(|| f.run_spec(&spec, hooks, FAST));
        recv(&rx);
        let s = f.prepare();
        f.confirm(ticket(&s)).unwrap();
        h.join().unwrap()
    });
    let released = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(
            matches!(report.outcome, LaunchOutcome::StopUnconfirmed(_)),
            "{:?}",
            report.outcome
        );
        let r = f.finish(false).unwrap();
        assert_eq!(r.outcome, ShutdownOutcome::Failed);
        assert!(!r.process_stop_confirmed && !r.persistence_sealed);
        assert!(
            r.failures.iter().any(|x| x.contains("stop unconfirmed")),
            "{r:?}"
        );
        assert_eq!(f.status().state, CloseState::Failed);
        assert!(
            !f.claims_of(101).is_empty(),
            "claims stay with unconfirmed work"
        );
        assert_eq!(f.effective_admission(), "Closed");
        assert!(f.cancel(ticket(&f.status())).is_err());
        assert_eq!(f.session().shutdown, ShutdownMarker::Running);
        // Retrying re-evaluates only the unresolved target; it stays failed.
        let again = f.finish(false).unwrap();
        assert_eq!(again.outcome, ShutdownOutcome::Failed);
        assert!(again.event > r.event);
        assert!(f.result(101).is_none());
    }));
    fs::write(barriers.join("release-linger"), b"").unwrap();
    until(|| barriers.join("linger-released").exists());
    released.unwrap();
}

#[test]
fn t12_log_disk_failure_cannot_acknowledge_a_clean_close() {
    let f = F::new(1);
    let hooks = LaunchHooks {
        spool: SpoolFaults {
            stdout_fail_after: None,
            stderr_fail_after: Some(128 * 1024),
        },
        ..LaunchHooks::default()
    };
    let report = f.run_spec(&f.spec(101, 1, "stderr-flood"), hooks, FAST);
    assert!(report.evidence_lost);
    let s = f.prepare();
    assert_eq!(active(&s), vec![101]);
    f.confirm(ticket(&s)).unwrap();
    let r = f.finish(false).unwrap();
    assert_eq!(r.outcome, ShutdownOutcome::Failed);
    assert!(r.process_stop_confirmed && !r.persistence_sealed, "{r:?}");
    assert!(
        r.failures
            .iter()
            .any(|x| x.contains("evidence persistence failed"))
    );
    assert_eq!(f.session().shutdown, ShutdownMarker::Running);
}

#[test]
fn t12_pipe_eof_stops_without_confirmation_keeps_queued_work_and_never_seals_clean() {
    let mut f = F::new(2);
    let (tx, rx) = mpsc::channel();
    let spec = f.spec(101, 1, "wait-release");
    std::thread::scope(|s| {
        let h = s.spawn(|| f.run_spec(&spec, on_session(tx, 101), FAST));
        recv(&rx);
        // A user dialog is open when the GUI disappears; no human can answer it.
        let dialog = f.prepare();
        assert_eq!(dialog.state, CloseState::AwaitingConfirmation);
        let ids = f.ids();
        let e = emergency_stop(&f.ctx(&ids)).unwrap();
        assert_eq!(e.state, CloseState::Stopping);
        assert_ne!(ticket(&e), ticket(&dialog));
        assert_eq!(e.targets.len(), 2);
        assert!(f.confirm(ticket(&dialog)).unwrap_err().contains("expired"));
        assert!(matches!(
            h.join().unwrap().outcome,
            LaunchOutcome::TreeAbsent {
                stop_requested: true,
                ..
            }
        ));
    });
    let r = f.finish(true).unwrap();
    assert_eq!(r.outcome, ShutdownOutcome::Emergency);
    assert!(r.process_stop_confirmed && r.persistence_sealed);
    interrupted_by_shutdown(&f.result(101).unwrap());
    // The reserved-but-never-issued attempt is retired; its step stays queued.
    assert!(f.kinds(102).is_empty());
    assert_eq!(f.status().targets[1].1.stop, TargetStop::NeverStarted);
    assert_eq!(f.admission(), Admission::Closed);
    assert_eq!(f.session().shutdown, ShutdownMarker::Running);
    f.restart();
    assert_eq!(
        f.e().startup_report().epoch.previous,
        Some((EngineEpoch(1), ShutdownMarker::Unclean))
    );
    assert_eq!(f.read(|v| v.scheduler_queue()).len(), 2);
}

#[test]
fn t12_close_races_reservation_issue_and_spawn_without_untracked_work() {
    let f = F::new(3);
    // An open close gate refuses the issue boundary for a reserved attempt.
    let gate = f.prepare();
    assert_eq!(active(&gate), vec![101, 102, 103]);
    let refused = f.run_with(101, "claude-ok", LaunchHooks::default());
    assert!(matches!(refused.outcome, LaunchOutcome::Refused(ref r) if r.contains("admission")));
    assert!(!f.read(|v| v.process_launch_issued(OpaqueId(101))));
    f.cancel(ticket(&gate)).unwrap();
    // 102 is issued but not spawned, 103 spawned but not registered, when Confirm commits.
    let (at, ar) = mpsc::channel();
    let (go_issue, wait_issue) = mpsc::channel::<()>();
    let (go_spawn, wait_spawn) = mpsc::channel::<()>();
    let (at2, at3) = (at.clone(), at);
    let issue_hooks = LaunchHooks {
        after_issue: Some(Box::new(move || {
            at2.send(102).unwrap();
            wait_issue.recv().unwrap();
        })),
        ..LaunchHooks::default()
    };
    let spawn_hooks = LaunchHooks {
        after_spawn: Some(Box::new(move |_| {
            at3.send(103).unwrap();
            wait_spawn.recv().unwrap();
        })),
        ..LaunchHooks::default()
    };
    let (r102, r103) = std::thread::scope(|s| {
        let h2 = s.spawn(|| f.run_with(102, "wait-release", issue_hooks));
        let h3 = s.spawn(|| f.run_with(103, "wait-release", spawn_hooks));
        let mut at: Vec<u128> = vec![recv(&ar), recv(&ar)];
        at.sort();
        assert_eq!(at, vec![102, 103]);
        let s = f.prepare();
        assert_eq!(active(&s), vec![101, 102, 103]);
        let c = f.confirm(ticket(&s)).unwrap();
        assert_eq!(
            c.targets.len(),
            3,
            "starting work cannot vanish from tracking"
        );
        go_issue.send(()).unwrap();
        go_spawn.send(()).unwrap();
        (h2.join().unwrap(), h3.join().unwrap())
    });
    assert!(
        matches!(r102.outcome, LaunchOutcome::NotSpawned(_)),
        "{:?}",
        r102.outcome
    );
    assert!(matches!(
        r103.outcome,
        LaunchOutcome::TreeAbsent {
            stop_requested: true,
            ..
        }
    ));
    let r = f.finish(false).unwrap();
    assert_eq!(r.outcome, ShutdownOutcome::Ready, "{r:?}");
    assert_eq!(f.status().targets[0].1.stop, TargetStop::NeverStarted);
    for a in [102, 103] {
        interrupted_by_shutdown(&f.result(a).unwrap());
    }
}

#[test]
fn t12_finished_and_exited_work_keeps_its_success_under_a_racing_close() {
    let f = F::new(2);
    f.run(101, "edit write:src/a.rs");
    let sealed = f.settle(101);
    assert_eq!(sealed.outcome, AttemptOutcome::Succeeded);
    // 102 exited successfully but was not settled when Confirm committed.
    f.run(102, "edit write:src/b.rs");
    let s = f.prepare();
    assert_eq!(active(&s), vec![102]);
    f.confirm(ticket(&s)).unwrap();
    let k = f.kinds(102);
    assert!(!k.iter().any(|k| k == "STOP_REQUESTED"), "{k:?}");
    let r = f.finish(false).unwrap();
    assert_eq!(r.outcome, ShutdownOutcome::Ready, "{r:?}");
    let late = f.result(102).unwrap();
    assert_eq!(late.outcome, AttemptOutcome::Succeeded);
    assert!(late.candidate.is_some());
    assert_eq!(f.result(101).unwrap(), sealed);
}

#[test]
fn t12_expired_tickets_and_earlier_epoch_tickets_are_rejected() {
    let mut f = F::new(1);
    let old = f.prepare();
    assert_eq!(old.state, CloseState::AwaitingConfirmation);
    f.restart();
    // A new epoch is not gated by an earlier epoch's open ticket, and cannot resolve it.
    assert_eq!(f.status().state, CloseState::Open);
    for e in [f.confirm(ticket(&old)), f.cancel(ticket(&old))] {
        assert!(e.unwrap_err().contains("earlier engine epoch"));
    }
    let s = f.prepare();
    assert_ne!(ticket(&s), ticket(&old));
    // The earlier epoch's work keeps QUARANTINED claims until recovery resolves it, so a
    // clean acknowledgement is refused even though this epoch has nothing to stop.
    assert_eq!(s.state, CloseState::Finalizing);
    let r = f.finish(false).unwrap();
    assert_eq!(r.outcome, ShutdownOutcome::Failed);
    assert!(
        r.failures.iter().any(|x| x.contains("live claims remain")),
        "{r:?}"
    );
}

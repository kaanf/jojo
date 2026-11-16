//! T11 reopen inspection and safe continuation. The helper is "killed" by dropping its
//! engine without a clean shutdown at durable barriers; the database then holds exactly
//! what a dead helper would have committed. Real OS processes (only the workspace's
//! `jojo-process-fixture`), temporary local Git repositories, no provider CLI, credential,
//! network or remote effect.
mod common;
use common::attempts::*;
use jojo_domain::attempt::AttemptOutcome;
use jojo_domain::identity::*;
use jojo_domain::provider::*;
use jojo_domain::recovery::{LaunchStage, ProcessFinding, RecoveryChoice, SessionContinuity};
use jojo_domain::validation::*;
use jojo_storage::*;
use jojo_test_support::dependency::node;
use std::fs;

use RecoveryChoice::*;

fn subject(ins: &RecoveryInspection, a: u128) -> &RecoverySubject {
    ins.subject(OpaqueId(a))
        .unwrap_or_else(|| panic!("{a} is not a subject: {ins:?}"))
}

fn blocked_by(ins: &RecoveryInspection, a: u128, needle: &str) {
    let s = subject(ins, a);
    assert!(
        s.assessment().blockers.iter().any(|b| b.contains(needle)),
        "{needle:?} not in {:?}",
        s.assessment()
    );
    assert_eq!(s.assessment().choices, vec![Hold]);
}

#[test]
fn t11_reopen_stays_paused_until_a_fresh_reviewed_inspection_starts_it() {
    let mut f = F::new(1);
    let base = f.record(101).base;
    f.restart();
    assert_eq!(f.admission(), Admission::Closed);
    let err = f
        .cmd(|tx| {
            tx.set_admission(Admission::Open)?;
            Ok("open".into())
        })
        .unwrap_err();
    assert!(
        format!("{err:?}").contains("recovery inspection"),
        "{err:?}"
    );

    let ins = f.inspect(&os(), None);
    let s = subject(&ins, 101);
    assert_eq!(s.facts().stage, LaunchStage::NotIssued);
    assert_eq!(s.assessment().choices, vec![ContinueFreshContext, Hold]);
    assert!(
        f.claims_of(101)
            .iter()
            .all(|(_, st)| *st == ClaimState::Quarantined)
    );

    // Hold records a decision and changes nothing else; it also makes `ins` stale.
    assert_eq!(f.resolve(&ins, 101, Hold).unwrap(), "HOLD");
    let err = f.start_recovery(&ins).unwrap_err();
    assert!(format!("{err:?}").contains("inspect again"), "{err:?}");
    let ins = f.inspect(&os(), None);
    f.start_recovery(&ins).unwrap();
    assert_eq!(f.admission(), Admission::Open);
    assert!(f.read(|v| v.recovery_started()));
    assert!(
        f.claims_of(101)
            .iter()
            .all(|(_, st)| *st == ClaimState::Quarantined)
    );

    // The never-issued reservation is retired with recovery evidence, never replayed.
    assert_eq!(
        f.resolve(&ins, 101, ContinueFreshContext).unwrap(),
        "NOT_STARTED"
    );
    assert!(f.claims_of(101).is_empty());
    assert!(
        f.read(|v| v.approval_result(OpaqueId(101)))
            .unwrap()
            .starts_with("NOT_STARTED")
    );
    assert!(f.resolve(&ins, 101, Hold).is_err(), "stale inspection");

    // A fresh attempt on the held step cannot claim provider continuation without a plan.
    let revision = f.read(|v| v.scheduler_queue())[0].control_revision;
    f.cmd(move |tx| {
        tx.control_scheduled_step(node(1), revision, "RUN", 0)?;
        Ok("run".into())
    })
    .unwrap();
    f.provision(202, &base, None);
    let proof = inspect_worktree(f.e(), OpaqueId(202)).unwrap();
    f.cmd(move |tx| {
        Ok(format!(
            "{:?}",
            tx.schedule_owned_worktrees(2, vec![proof])?
        ))
    })
    .unwrap();
    let mut spec = f.spec(202, 1, "claude-ok");
    spec.continuation = Some(Continuation {
        predecessor: AttemptId(OpaqueId(101)),
        exact: SessionRef {
            provider: ProviderKind::ClaudeCode,
            session: "latest".into(),
            turn: "t".into(),
        },
        previous_cwd: f.record(101).path,
        reviewed_plan: "none".into(),
    });
    let err = f
        .cmd(move |tx| Ok(tx.issue_process_launch(&spec)?.to_string()))
        .unwrap_err();
    assert!(
        format!("{err:?}").contains("reviewed continuation plan"),
        "{err:?}"
    );

    // Every later reopen needs its own start.
    f.restart();
    assert!(!f.read(|v| v.recovery_started()));
    assert!(
        f.cmd(|tx| {
            tx.set_admission(Admission::Open)?;
            Ok("open".into())
        })
        .is_err()
    );
}

#[test]
fn t11_kill_after_issue_spawn_output_and_result_barriers_keeps_ambiguity_and_claims() {
    let mut f = F::new(4);
    // 101: issued, then the helper died before registering any process.
    let spec = f.spec(101, 1, "claude-ok");
    f.cmd(move |tx| Ok(tx.issue_process_launch(&spec)?.to_string()))
        .unwrap();
    // 102: spawned and still running when the helper died.
    let live = f.spawn_registered(102, "wait-release");
    // 103: spawned, wrote output, one durable checkpoint, exited after the helper died.
    let out = f.spawn_registered(103, "claude-ok");
    until(|| out.root_exit().is_some());
    f.cmd(|tx| {
        tx.append_provider_output(OpaqueId(103), Stream::Stdout, 10, &[])?;
        Ok("checkpoint".into())
    })
    .unwrap();
    drop(out);
    // 104: fully sealed before the helper died.
    f.run(104, "edit write:src/done.rs");
    let ids = f.ids();
    let sealed = match settle_attempt(&f.ctx(&ids), OpaqueId(104), BlobFaults::default()).unwrap() {
        Settlement::Sealed(r) => *r,
        other => panic!("{other:?}"),
    };
    assert_eq!(sealed.outcome, AttemptOutcome::Succeeded);
    f.restart();

    let ins = f.inspect(&os(), None);
    assert!(
        ins.subject(OpaqueId(104)).is_none(),
        "a sealed success needs no decision"
    );
    assert_eq!(
        f.result(104),
        Some(sealed),
        "results survive reopen unchanged"
    );
    assert_eq!(
        subject(&ins, 101).facts().stage,
        LaunchStage::IssuedWithoutIdentity
    );
    blocked_by(&ins, 101, "without a recorded process identity");
    let s = subject(&ins, 102);
    assert!(
        s.facts()
            .processes
            .iter()
            .any(|(_, p)| *p == ProcessFinding::Alive)
    );
    blocked_by(&ins, 102, "live orphan");
    let s = subject(&ins, 103);
    assert!(s.assessment().blockers.is_empty(), "{:?}", s.assessment());
    assert!(
        s.facts()
            .evidence_gaps
            .iter()
            .any(|g| g.contains("beyond durable offset 10")),
        "{:?}",
        s.facts().evidence_gaps
    );
    for a in [101, 102, 103] {
        let claims = f.claims_of(a);
        assert!(!claims.is_empty() && claims.iter().all(|(_, s)| *s == ClaimState::Quarantined));
    }

    // Blocked work offers only Hold, whoever asks; nothing is signalled.
    assert!(
        f.resolve(&ins, 102, ContinueFreshContext)
            .unwrap_err()
            .contains("not offered")
    );
    let ins_c = ins.clone();
    let err = f
        .cmd(move |tx| {
            tx.resolve_recovery(
                &ins_c,
                OpaqueId(101),
                ContinueFreshContext,
                HumanOrigin::TrustedUi {
                    actor: "operator: I am sure it stopped".into(),
                },
            )
        })
        .unwrap_err();
    assert!(format!("{err:?}").contains("not offered"));
    assert_eq!(f.resolve(&ins, 102, Hold).unwrap(), "HOLD");
    assert_eq!(live.verify_identity(live.root()), IdentityCheck::Alive);
    // Quarantine blocks a new writer in that repository.
    let (r, rt, b) = (f.repo.clone(), f.root.clone(), f.record(101).base);
    let state = f
        .cmd(move |tx| tx.request_worktree(OpaqueId(301), &r, &rt, &b, None))
        .unwrap()
        .response;
    assert_eq!(state, "WAITING");

    // Proven-dead work settles as interrupted, keeping every gap as an uncertainty.
    let r = f.recover(&ins, 103, ContinueFreshContext);
    assert_eq!(
        (r.outcome, r.reason.as_str()),
        (AttemptOutcome::Interrupted, "helper_lost_recovered")
    );
    assert!(
        r.uncertainties.iter().any(|u| u.contains("coverage lost")),
        "{:?}",
        r.uncertainties
    );
    assert!(r.candidate.is_some());
    assert!(f.claims_of(103).is_empty());

    // Only OS evidence resolves the orphan: once the process is gone, re-inspection
    // offers choices. (The test owns the lifetime handle; the engine never signals.)
    let root = live.root().clone();
    drop(live);
    until(|| {
        os().inspect(std::slice::from_ref(&root))
            .iter()
            .all(|(_, c)| *c == IdentityCheck::Absent)
    });
    let ins = f.inspect(&os(), None);
    assert!(subject(&ins, 102).assessment().blockers.is_empty());
    assert_eq!(
        f.recover(&ins, 102, ContinueFreshContext).outcome,
        AttemptOutcome::Interrupted
    );
    blocked_by(&ins, 101, "without a recorded process identity");
}

#[test]
fn t11_process_inspector_distinguishes_live_absent_reused_and_unknown() {
    let mut f = F::new(4);
    let live = f.spawn_registered(104, "wait-release");
    let boot = live.root().boot.clone();
    let synthetic = [
        (
            101,
            std::process::id(),
            "Thu Jan  1 00:00:00 1970",
            boot.as_str(),
        ),
        (102, 99_998, "UNKNOWN", boot.as_str()),
        (
            103,
            99_997,
            "Thu Jan  1 00:00:00 1970",
            "{ sec = 1, usec = 0 }",
        ),
    ];
    for (a, pid, start, boot) in synthetic {
        let spec = f.spec(a, a - 100, "claude-ok");
        f.cmd(move |tx| Ok(tx.issue_process_launch(&spec)?.to_string()))
            .unwrap();
        let identity = ProcessIdentity {
            pid,
            ppid: 1,
            pgid: 99_990 + (a as u32 - 100),
            start: start.into(),
            boot: boot.into(),
        };
        f.cmd(move |tx| {
            Ok(tx
                .register_process_root(OpaqueId(a), &identity)?
                .to_string())
        })
        .unwrap();
    }
    f.restart();
    let ins = f.inspect(&os(), None);
    blocked_by(&ins, 101, "PID reused");
    blocked_by(&ins, 102, "inspection unknown");
    blocked_by(&ins, 104, "live orphan");
    let s = subject(&ins, 103);
    assert!(
        s.facts()
            .processes
            .iter()
            .all(|(_, p)| *p == ProcessFinding::Absent),
        "different boot"
    );
    assert!(s.assessment().choices.contains(&ContinueFreshContext));
    // Same durable records, failed inspection: never absence.
    let failed = f.inspect(&Scripted(IdentityCheck::Unknown("ps failed".into())), None);
    blocked_by(&failed, 103, "ps failed");
    assert!(
        f.claims_of(103)
            .iter()
            .all(|(_, s)| *s == ClaimState::Quarantined)
    );
    drop(live);
}

/// Runs attempt 101 to proven absence, records its candidate and blobs, then the helper
/// dies before finalizing. Returns the fixture after reopen.
fn crashed_during_settlement() -> F {
    let mut f = F::new(1);
    f.run(101, "edit write:src/feature.rs");
    let ids = f.ids();
    prepare_settlement(&f.ctx(&ids), OpaqueId(101), BlobFaults::default()).unwrap();
    f.restart();
    f
}

#[test]
fn t11_moved_replaced_head_changed_or_dirty_worktree_blocks_recovery() {
    type Mutation = fn(&F, &WorktreeRecord);
    let cases: [(&str, Mutation, &str); 4] = [
        (
            "moved",
            |_, r| {
                let parent = r.path.parent().unwrap();
                fs::rename(parent, parent.with_extension("moved")).unwrap();
            },
            "worktree",
        ),
        (
            "common-dir replaced",
            |f, _| {
                let rp = f.dir.path().join("repo");
                fs::rename(rp.join(".git"), rp.join("old-git")).unwrap();
                git(&rp, &["init", "--template=", "--initial-branch=main"]);
            },
            "repository identity changed",
        ),
        (
            "HEAD changed",
            |_, r| {
                git(&r.path, &["add", "--all"]);
                git(&r.path, &["commit", "-m", "after candidate"]);
            },
            "since its recorded candidate",
        ),
        (
            "dirty mismatch",
            |_, r| {
                fs::write(r.path.join("src/feature.rs"), "edited after candidate\n").unwrap();
            },
            "since its recorded candidate",
        ),
    ];
    for (name, mutate, needle) in cases {
        let f = crashed_during_settlement();
        mutate(&f, &f.record(101));
        let ins = f.inspect(&os(), None);
        blocked_by(&ins, 101, needle);
        assert!(f.resolve(&ins, 101, ValidateOutput).is_err(), "{name}");
        assert!(
            f.claims_of(101)
                .iter()
                .all(|(_, s)| *s == ClaimState::Quarantined),
            "{name}"
        );
    }
}

#[test]
fn t11_unchanged_settled_worktree_recovers_its_real_result_and_validates_output() {
    let f = crashed_during_settlement();
    let ins = f.inspect(&os(), None);
    let s = subject(&ins, 101);
    assert_eq!(s.facts().stage, LaunchStage::Concluded);
    assert_eq!(
        s.assessment().choices,
        vec![ValidateOutput, ContinueFreshContext, Hold]
    );
    // Supervised absence was proven before the crash, so the P1-10 rules still apply.
    let r = f.recover(&ins, 101, ValidateOutput);
    assert_eq!(r.outcome, AttemptOutcome::Succeeded);
    let candidate = r.candidate.clone().unwrap();
    let report = ValidatorReport {
        origin: ValidatorOrigin::IndependentFixture {
            name: "t11-validator".into(),
        },
        attempt: OpaqueId(101),
        candidate,
        plan_digest: sha256_hex(&[f.plan.as_bytes()]),
        results: TESTS
            .iter()
            .map(|t| (t.to_string(), TestStatus::Passed))
            .collect(),
        raw: b"passed".to_vec(),
    };
    let ids = f.ids();
    let receipt = validate_candidate(&f.ctx(&ids), f.plan.as_bytes(), report).unwrap();
    assert!(receipt.passed, "{receipt:?}");
    assert!(f.claims_of(101).is_empty());
}

#[test]
fn t11_absent_session_or_unsupported_cwd_change_offers_fresh_context_only() {
    let mut f = F::new(2);
    f.run_and(101, "wait-release", |tx| {
        tx.request_shutdown_stop(OpaqueId(101))?;
        Ok("shutdown".into())
    });
    f.restart();
    let full = caps(Support::Supported, Support::Supported);
    let cases = [
        (None, false),
        (Some(caps(Support::Supported, Support::Unsupported)), false),
        (Some(caps(Support::Unknown, Support::Supported)), false),
        (Some(full), true),
    ];
    for (c, history) in cases {
        let ins = f.inspect(&os(), c.clone());
        let s = subject(&ins, 101);
        assert_eq!(
            matches!(s.facts().session, SessionContinuity::Exact(_)),
            history,
            "{c:?}"
        );
        assert_eq!(
            s.assessment().choices.contains(&ContinueWithHistory),
            history
        );
        assert!(s.assessment().choices.contains(&ContinueFreshContext));
        assert_eq!(
            subject(&ins, 102).facts().session,
            SessionContinuity::NotRecorded
        );
        assert_eq!(
            subject(&ins, 102).assessment().choices,
            vec![ContinueFreshContext, Hold]
        );
    }
}

#[test]
fn t11_stale_approval_blocks_continuation_and_keeps_quarantine() {
    let mut f = F::new(1);
    f.restart();
    f.decide(ArtifactId(OpaqueId(903)), DecisionAction::Revoke);
    let ins = f.inspect(&os(), None);
    blocked_by(&ins, 101, "stale approval");
    assert!(
        f.claims_of(101)
            .iter()
            .all(|(_, s)| *s == ClaimState::Quarantined)
    );
}

#[test]
fn t11_continuation_is_a_new_attempt_and_a_completed_effect_is_never_repeated() {
    let mut f = F::new(1);
    let key = "attempt-effect:fake-pr";
    let payload_digest = sha256_hex(&[b"open pr"]);
    let old = f.record(101);
    fs::write(old.path.join("src/partial.rs"), "partial work\n").unwrap();
    f.run_and(101, "wait-release", move |tx| {
        tx.journal_attempt_effect(OpaqueId(101), key, "fake-remote", "open pr")?;
        tx.issue_attempt_effect(key)?;
        tx.request_shutdown_stop(OpaqueId(101))?;
        Ok("effect issued, then close confirmed".into())
    });
    f.restart();
    let full = Some(caps(Support::Supported, Support::Supported));

    // The acknowledgement was lost: blocked until the exact fake observation arrives.
    let ins = f.inspect(&os(), full.clone());
    blocked_by(&ins, 101, key);
    assert_eq!(f.resolve(&ins, 101, Hold).unwrap(), "HOLD");
    let d = payload_digest.clone();
    f.cmd(move |tx| {
        tx.record_effect_observation(key, "fake-remote:origin", RemoteObservation::Committed, &d)?;
        Ok("observed".into())
    })
    .unwrap();
    let ins = f.inspect(&os(), full);
    let s = subject(&ins, 101);
    assert_eq!(
        s.assessment().choices,
        vec![
            ValidateOutput,
            ContinueWithHistory,
            ContinueFreshContext,
            Hold
        ]
    );
    let SessionContinuity::Exact(session) = s.facts().session.clone() else {
        panic!("exact session expected");
    };
    f.start_recovery(&ins).unwrap();
    let sealed = f.recover(&ins, 101, ContinueWithHistory);
    assert_eq!(
        (sealed.outcome, sealed.reason.as_str()),
        (AttemptOutcome::Interrupted, "shutdown")
    );
    let candidate = sealed.candidate.clone().unwrap();
    assert!(f.claims_of(101).is_empty());

    let plan = |effects: Vec<&str>, session: SessionPolicy| ContinuationPlan {
        successor: OpaqueId(201),
        predecessor: OpaqueId(101),
        remaining_work: "finish src/partial.rs".into(),
        reconciled_effects: effects.into_iter().map(String::from).collect(),
        session,
    };
    let record = |p: ContinuationPlan| {
        f.cmd(move |tx| tx.record_continuation_plan(&p, human("T11 reviewer")))
            .map(|r| r.response)
    };
    assert!(record(plan(vec![], SessionPolicy::ExactHistory(session.clone()))).is_err());
    let mut other = session.clone();
    other.session = "a-later-session".into();
    assert!(record(plan(vec![key], SessionPolicy::ExactHistory(other))).is_err());
    assert!(
        record(plan(vec![key], SessionPolicy::FreshContext)).is_err(),
        "resolution chose history"
    );
    let digest = record(plan(
        vec![key],
        SessionPolicy::ExactHistory(session.clone()),
    ))
    .unwrap();
    assert!(
        record(plan(
            vec![key],
            SessionPolicy::ExactHistory(session.clone())
        ))
        .is_err()
    );

    // The successor gets a new owned worktree materialized from the sealed candidate.
    let reference = f
        .read(|v| v.candidate_reference(OpaqueId(101), &candidate))
        .unwrap();
    let packet = CandidateSnapshot::reopen(&reference.path, &candidate).unwrap();
    let revision = f.read(|v| v.scheduler_queue())[0].control_revision;
    f.cmd(move |tx| {
        tx.control_scheduled_step(node(1), revision, "RUN", 0)?;
        Ok("run".into())
    })
    .unwrap();
    f.provision(201, &reference.head, Some((101, &packet)));
    let proof = inspect_worktree(f.e(), OpaqueId(201)).unwrap();
    let decisions = f
        .cmd(move |tx| {
            Ok(format!(
                "{:?}",
                tx.schedule_owned_worktrees(2, vec![proof])?
            ))
        })
        .unwrap()
        .response;
    assert!(decisions.contains("attempt: Some"), "{decisions}");
    let new = f.record(201);
    assert_eq!(new.state, "BOUND");
    assert_ne!(new.path, old.path);
    assert_eq!(
        fs::read_to_string(new.path.join("src/partial.rs")).unwrap(),
        "partial work\n"
    );

    let mut spec = f.spec(201, 1, "claude-ok");
    let issue = |spec: LaunchSpec| {
        f.cmd(move |tx| Ok(tx.issue_process_launch(&spec)?.to_string()))
            .map(|r| r.response)
            .map_err(|e| format!("{e:?}"))
    };
    assert!(issue(spec.clone()).unwrap_err().contains("exact session"));
    let exact = Continuation {
        predecessor: AttemptId(OpaqueId(101)),
        exact: session,
        previous_cwd: old.path.clone(),
        reviewed_plan: digest,
    };
    let mut wrong = exact.clone();
    wrong.previous_cwd = new.path.clone();
    spec.continuation = Some(wrong);
    assert!(issue(spec.clone()).unwrap_err().contains("reviewed plan"));
    spec.continuation = Some(exact);
    assert_eq!(issue(spec).unwrap(), "true");

    // The predecessor's committed effect cannot be journaled again by the successor.
    let err = f
        .cmd(move |tx| {
            tx.journal_attempt_effect(OpaqueId(201), key, "fake-remote", "open pr")?;
            Ok("replayed".into())
        })
        .unwrap_err();
    assert!(format!("{err:?}").contains(key), "{err:?}");
    assert!(f.read(|v| v.dispatchable_attempt_effects()).is_empty());

    // The original attempt, result and worktree are untouched.
    assert_eq!(f.result(101), Some(sealed));
    assert_eq!(
        fs::read_to_string(old.path.join("src/partial.rs")).unwrap(),
        "partial work\n"
    );
    assert!(f.read(|v| v.continuation_plan(OpaqueId(201))).is_some());
}

//! T15 offline feature scenario. One complete fixture PRD, ADR and typed plan reviewed
//! through the helper's production service (the SwiftUI shell's protocol), two independent
//! temporary repositories, fake Claude/Codex developers (`jojo-process-fixture` only), a
//! separate validator fixture, a typed dependency branch and close/reopen. No provider CLI,
//! credential, network or remote effect. This proves foundation behaviour, not AI quality.
mod common;
use common::attempts::*;
use common::reading;
use jojo_domain::attempt::AttemptOutcome;
use jojo_domain::dependency::*;
use jojo_domain::identity::*;
use jojo_domain::ipc::*;
use jojo_domain::provider::*;
use jojo_domain::recovery::RecoveryChoice;
use jojo_domain::shutdown::{CloseState, ShutdownOutcome};
use jojo_storage::*;
use jojo_test_support::dependency::node;
use std::fs;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use common::scenario::*;
#[test]
fn t15_end_to_end_blocks_before_every_approval_and_binds_displayed_bytes() {
    let mut s = S::new(2, &[(0, ClaudeCode), (1, Codex)]);
    s.author_prd();
    // Nothing can be queued against an unreviewed feature.
    assert!(s.queue(1, R1, vec![s.base(1)]).is_err());
    // A decision on bytes the reviewer was not shown is refused and records nothing.
    assert_eq!(s.ui_decide(PRD, "Approve", true).kind, "Error");
    assert_eq!(s.read(|v| v.current_artifact_approval(PRD)), None);
    s.approve(PRD);
    s.author_adr().unwrap();
    assert!(s.queue(1, R1, vec![s.base(1)]).is_err(), "no ADR approval");
    s.approve(ADR);
    let p = s.feature_plan(vec![]);
    s.put_plan(&p, None);
    // The reviewed plan exists but is unapproved: still blocked, also by the scheduler.
    for step in [1, 2] {
        let err = s.queue(step, R1, vec![s.base(step)]).unwrap_err();
        assert!(err.contains("relationship"), "{err}");
    }
    s.configure(2, 1, 1);
    s.provision(101, 0, &s.repos[0].1.clone(), None);
    assert!(s.schedule(&[101]).is_empty());
    assert_eq!(s.ui_decide(PLAN, "Approve", true).kind, "Error");
    s.approve(PLAN);
    s.queue_base(1);
    s.queue_base(2);
    assert_eq!(s.schedule(&[101]), vec![(1, 101)]);
    // Every decision is attributable, append-only and bound to the displayed bytes.
    for a in [PRD, ADR, PLAN] {
        let decisions = s.read(|v| v.human_decisions(a));
        assert_eq!(decisions.len(), 1, "{a:?}: {decisions:?}");
    }
}

#[test]
fn t15_cap_one_runs_mixed_providers_strictly_serially() {
    let mut s = S::new(2, &[(0, ClaudeCode), (1, Codex)]);
    s.approve_all(vec![]);
    s.queue_base(1);
    s.queue_base(2);
    s.configure(1, 1, 1);
    let (b0, b1) = (s.repos[0].1.clone(), s.repos[1].1.clone());
    s.provision(101, 0, &b0, None);
    s.provision(102, 1, &b1, None);
    let first = s.schedule(&[101, 102]);
    assert_eq!(first.len(), 1, "cap one admits exactly one: {first:?}");
    let (step, a) = first[0];
    let (other_step, b) = if a == 101 { (2, 102) } else { (1, 101) };
    let reports = s.run_parallel(&[(a, step, R1)], || {
        // While the first developer is RUNNING nothing else is admitted or bound.
        assert!(s.schedule(&[b]).is_empty());
        assert_eq!(s.record(b).state, "READY");
        assert!(s.claims_of(b).is_empty());
        assert!(s.kinds(b).is_empty(), "the second process was never issued");
        true
    });
    released(&reports);
    // Exited but unsettled work still holds the only slot.
    assert!(s.schedule(&[b]).is_empty());
    succeeded(&s.settle(a));
    assert!(s.claims_of(a).is_empty());
    assert_eq!(s.schedule(&[b]), vec![(other_step, b)]);
    released(&s.run_parallel(&[(b, other_step, R1)], || true));
    succeeded(&s.settle(b));
    // No overlap: the second result is sealed strictly after the first.
    assert!(s.result(a).unwrap().event < s.result(b).unwrap().event);
    assert_ne!(s.record(a).repository, s.record(b).repository);
}

#[test]
fn t15_cap_two_overlaps_claude_and_codex_in_independent_repositories() {
    let mut s = S::new(2, &[(0, ClaudeCode), (1, Codex)]);
    s.approve_all(vec![]);
    s.queue_base(1);
    s.queue_base(2);
    s.configure(2, 1, 1);
    let (b0, b1) = (s.repos[0].1.clone(), s.repos[1].1.clone());
    s.provision(101, 0, &b0, None);
    s.provision(102, 1, &b1, None);
    let mut admitted = s.schedule(&[101, 102]);
    admitted.sort();
    assert_eq!(admitted, vec![(1, 101), (2, 102)]);
    let reports = s.run_parallel(&[(101, 1, R1), (102, 2, R1)], || {
        // Both providers are RUNNING simultaneously and neither has completed.
        for a in [101, 102] {
            s.live(a);
            assert!(!s.claims_of(a).is_empty());
        }
        true
    });
    released(&reports);
    let (r1, r2) = (s.settle(101), s.settle(102));
    succeeded(&r1);
    succeeded(&r2);
    // Independent worktrees, outputs, repositories and immutable results.
    let (w1, w2) = (s.record(101), s.record(102));
    assert_ne!(w1.path, w2.path);
    assert_ne!(w1.output, w2.output);
    assert_ne!(w1.repository, w2.repository);
    assert_ne!(w1.common_dir(), w2.common_dir());
    assert_ne!(r1.candidate, r2.candidate);
    assert!(w1.path.join("src/step1.rs").exists() && !w1.path.join("src/step2.rs").exists());
    assert!(w2.path.join("src/step2.rs").exists() && !w2.path.join("src/step1.rs").exists());
    for (a, file) in [(101, "src/step1.rs"), (102, "src/step2.rs")] {
        for (_, blob) in s.read(|v| v.attempt_evidence(OpaqueId(a))) {
            s.store.verify(&blob).unwrap();
        }
        let receipt = s.validate(a).unwrap();
        assert!(receipt.passed, "{:?}", receipt.reasons);
        assert_eq!(receipt.changes, vec![format!("Added 000000 100644 {file}")]);
    }
    // The user's checkouts are untouched by the fake developers.
    for (repo, _) in &s.repos {
        assert_eq!(git(repo.path(), &["status", "--porcelain"]), "");
    }
}

#[test]
fn t15_pr_open_never_unlocks_and_validated_output_satisfies_the_exact_predicate() {
    let mut s = S::new(2, &[(0, ClaudeCode), (1, Codex), (0, ClaudeCode)]);
    let pr = EvidenceRequirement {
        kind: EvidenceKind::PrMerged,
        evidence_id: "t15-step1-pr".into(),
        observation_revision: R1,
        binding: EvidenceBinding {
            sources: vec![s.base(1)],
            environment: Some("test-env".into()),
            artifact_digest: None,
            contract: None,
            validation_plan: None,
            test_inventory: vec![],
            output: None,
        },
        max_age_ms: 3_600_000,
    };
    s.approve_all(vec![evidence_edge(31, 1, 3, pr.clone())]);
    s.queue_base(1);
    s.queue_base(2);
    let blocked = s.queue_base_err(3);
    assert!(blocked.contains("not dependency ready"), "{blocked}");
    // A fake PR-open fact is informational only.
    let o = observation(&pr, EvidenceStatus::PullRequestOpen);
    s.cmd(move |tx| {
        tx.record_dependency_observation(None, &o)?;
        Ok("pr open".into())
    })
    .unwrap();
    let eval = s.read(|v| {
        v.evaluate_dependencies(
            PLAN,
            R1,
            node(3),
            EvaluationTrigger::Event,
            reading(1_700_000_000_000),
        )
    });
    assert_eq!(eval.truth, Truth::Unsatisfied);
    assert!(format!("{eval:?}").contains("open PR is informational"));
    assert!(s.queue_base_err(3).contains("not dependency ready"));

    s.configure(2, 1, 1);
    let (b0, b1) = (s.repos[0].1.clone(), s.repos[1].1.clone());
    s.provision(101, 0, &b0, None);
    s.provision(102, 1, &b1, None);
    assert_eq!(s.schedule(&[101, 102]).len(), 2);
    released(&s.run_parallel(&[(101, 1, R1), (102, 2, R1)], || true));
    let step1 = s.settle(101);
    succeeded(&step1);
    succeeded(&s.settle(102));
    let receipt = s.validate(101).unwrap();
    assert!(receipt.passed, "{:?}", receipt.reasons);
    s.accept(101, receipt.event).unwrap();

    // A human reviews plan revision 2, which selects step 1's exact validated output.
    let candidate = step1.candidate.clone().unwrap();
    let reference = s
        .read(|v| v.candidate_reference(OpaqueId(101), &candidate))
        .unwrap();
    let sel = OutputSelection {
        output_id: "step-1-candidate".into(),
        producer: node(1),
        digest: candidate.clone(),
        sources: vec![Source {
            repository: s.repos[0].0.id(),
            revision: SourceRevision::Snapshot {
                base_commit: reference.head.clone(),
                digest: candidate.clone(),
            },
        }],
    };
    let [output, validated] = output_requirements(&sel, &s.validation);
    let mut p2 = s.feature_plan(vec![
        evidence_edge(32, 1, 3, output.clone()),
        evidence_edge(33, 1, 3, validated.clone()),
    ]);
    p2.inputs[2].base_sources = vec![];
    p2.inputs[2].selected_outputs = vec![sel.clone()];
    s.put_plan(&p2, Some(R1));
    let err = s.queue(3, R2, sel.sources.clone()).unwrap_err();
    assert!(!err.is_empty(), "unapproved revision cannot queue");
    s.approve(PLAN);
    let err = s.queue(3, R2, sel.sources.clone()).unwrap_err();
    assert!(
        err.contains("not dependency ready"),
        "no observation yet: {err}"
    );

    // The independent observer reports only what the validator and reviewer established.
    let receipts = s.read(|v| v.validation_receipts(OpaqueId(101)));
    assert!(receipts.last().unwrap().passed);
    assert_eq!(
        s.read(|v| v.candidate_acceptance(OpaqueId(101))),
        Some(receipt.event)
    );
    // Validation evidence alone does not unlock: the exact output must also be observed.
    let o = observation(&validated, EvidenceStatus::Verified);
    s.cmd(move |tx| {
        tx.record_dependency_observation(None, &o)?;
        Ok("validated".into())
    })
    .unwrap();
    let err = s.queue(3, R2, sel.sources.clone()).unwrap_err();
    assert!(err.contains("not dependency ready"), "{err}");
    let o = observation(&output, EvidenceStatus::Verified);
    s.cmd(move |tx| {
        tx.record_dependency_observation(None, &o)?;
        Ok("exact output".into())
    })
    .unwrap();
    // A base-commit source is not the selected output and is refused.
    assert!(s.queue(3, R2, vec![s.base(3)]).is_err());
    s.queue(3, R2, sel.sources.clone()).unwrap();

    // The dependent attempt gets a new owned worktree from the exact sealed candidate.
    let packet = CandidateSnapshot::reopen(&reference.path, &candidate).unwrap();
    s.provision(103, 0, &reference.head, Some((101, &packet)));
    assert_eq!(s.schedule(&[103]), vec![(3, 103)]);
    let w3 = s.record(103);
    assert!(w3.path.join("src/step1.rs").exists());
    assert_ne!(w3.path, s.record(101).path);
    released(&s.run_parallel(&[(103, 3, R2)], || true));
    succeeded(&s.settle(103));
    assert!(w3.path.join("src/step3.rs").exists());
    // Upstream records are unchanged by downstream work.
    assert_eq!(s.result(101), Some(step1));
}

#[test]
fn t15_revocation_mid_run_interrupts_both_and_excludes_their_outputs() {
    let mut s = S::new(2, &[(0, ClaudeCode), (1, Codex)]);
    s.approve_all(vec![]);
    s.queue_base(1);
    s.queue_base(2);
    s.configure(2, 1, 1);
    let (b0, b1) = (s.repos[0].1.clone(), s.repos[1].1.clone());
    s.provision(101, 0, &b0, None);
    s.provision(102, 1, &b1, None);
    assert_eq!(s.schedule(&[101, 102]).len(), 2);
    let reports = s.run_parallel(&[(101, 1, R1), (102, 2, R1)], || {
        s.decide(PLAN, DecisionAction::Revoke);
        false
    });
    for r in &reports {
        assert!(
            matches!(
                r.outcome,
                LaunchOutcome::TreeAbsent {
                    stop_requested: true,
                    ..
                }
            ),
            "{:?}",
            r.outcome
        );
    }
    for a in [101, 102] {
        let r = s.settle(a);
        assert_eq!(
            (r.outcome, r.reason.as_str()),
            (AttemptOutcome::Interrupted, "approval_invalidated"),
            "{r:?}"
        );
        assert!(s.claims_of(a).is_empty());
        match s.validate(a) {
            Ok(receipt) => {
                assert!(!receipt.passed);
                assert!(s.accept(a, receipt.event).is_err());
            }
            Err(e) => assert!(!e.is_empty()),
        }
        assert_eq!(s.read(|v| v.candidate_acceptance(OpaqueId(a))), None);
    }
    // Nothing downstream can start from the revoked plan.
    assert!(s.queue(1, R1, vec![s.base(1)]).is_err());
}

#[test]
fn t15_cancel_close_leaves_processes_and_confirmed_close_reopen_preserves_evidence() {
    let mut s = S::new(2, &[(0, ClaudeCode), (1, Codex)]);
    s.approve_all(vec![]);
    s.queue_base(1);
    s.queue_base(2);
    s.configure(2, 1, 1);
    let (b0, b1) = (s.repos[0].1.clone(), s.repos[1].1.clone());
    s.provision(101, 0, &b0, None);
    s.provision(102, 1, &b1, None);
    assert_eq!(s.schedule(&[101, 102]).len(), 2);
    // The helper's own worker pool runs both developers, as in the shipped helper.
    let (tx, rx) = mpsc::channel();
    for (a, step) in [(101, 1), (102, 2)] {
        let spec = s.spec(a, step, R1);
        let launch = s.launch(&spec);
        let mut hooks = on_session(tx.clone(), a);
        hooks.inspector = Some(ps());
        let plan = StopPlan::new(&caps(Support::Supported, Support::Supported), BUDGETS);
        s.ui().launch(spec, launch, plan, hooks).unwrap();
    }
    for _ in 0..2 {
        rx.recv_timeout(Duration::from_secs(20)).unwrap();
    }

    // Quit, then Cancel: nothing is stopped and both processes keep running.
    assert_eq!(s.request("PrepareClose", object([]), None).kind, "Response");
    let status = s.ui().status().unwrap();
    assert_eq!(status.state, CloseState::AwaitingConfirmation);
    let mut active: Vec<u128> = status.active.iter().map(|w| w.attempt.0).collect();
    active.sort();
    assert_eq!(active, vec![101, 102]);
    let ticket = status.ticket.unwrap().ticket;
    let r = s.request("CancelClose", object([("ticket", identity(ticket))]), None);
    assert_eq!(r.kind, "Response");
    assert_eq!(s.ui().status().unwrap().state, CloseState::Open);
    for a in [101, 102] {
        assert!(!s.read(|v| v.approval_stop_requested(OpaqueId(a))));
        s.live(a);
        let k = s.kinds(a);
        assert!(
            !k.iter()
                .any(|k| k == "STOP_REQUESTED" || k == "SIGNAL_SENT"),
            "{k:?}"
        );
    }

    // Quit, then Confirm: every active process is interrupted and the close is clean.
    s.request("PrepareClose", object([]), None);
    let ticket = s.ui().status().unwrap().ticket.unwrap().ticket;
    let r = s.request("ConfirmClose", object([("ticket", identity(ticket))]), None);
    assert_eq!(r.kind, "Response");
    let deadline = Instant::now() + Duration::from_secs(20);
    let receipt = loop {
        if let Some(r) = s.ui().finish(false).unwrap() {
            break r;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    };
    assert_eq!(receipt.outcome, ShutdownOutcome::Ready, "{receipt:?}");
    assert!(receipt.process_stop_confirmed && receipt.persistence_sealed);
    let mut before = Vec::new();
    for a in [101, 102] {
        let r = s.result(a).unwrap();
        assert_eq!(r.outcome, AttemptOutcome::Interrupted, "{r:?}");
        assert!(
            r.stop_cause.as_deref().unwrap().contains("shutdown"),
            "{r:?}"
        );
        let out = s.record(a).output.join("generation-1");
        let logs = ["stdout.raw", "stderr.raw"].map(|f| fs::read(out.join(f)).unwrap());
        assert_eq!(r.stdout.as_deref(), Some(sha256_hex(&[&logs[0]]).as_str()));
        let evidence = s.read(|v| v.attempt_evidence(OpaqueId(a)));
        before.push((r, logs, evidence, s.kinds(a)));
    }
    let queue = s.read(|v| v.scheduler_queue());
    assert!(queue.iter().all(|q| q.intent != "CANCEL"));

    // Reopen: interrupted work is shown for review; nothing is dispatched automatically.
    s.reopen();
    assert_eq!(s.admission(), Admission::Closed);
    assert!(s.read(|v| v.live_claims()).is_empty());
    let shown = s.request(
        "Recovery",
        object([("offset", number(0u64)), ("limit", number(8u64))]),
        None,
    );
    assert_eq!(shown.kind, "Response", "{shown:?}");
    let rows = format!("{:?}", get(&shown.payload, "rows").unwrap());
    // Each interrupted attempt is listed with its candidate hash and evidence location.
    for (a, (r, ..)) in [101u128, 102].into_iter().zip(&before) {
        assert!(rows.contains(&format!("{a:032x}")), "{rows}");
        assert!(rows.contains(r.candidate.as_deref().unwrap()), "{rows}");
        assert!(
            rows.contains(&s.record(a).path.display().to_string()),
            "{rows}"
        );
    }
    assert!(rows.contains("ContinueFreshContext"), "{rows}");
    assert!(rows.contains("stage=Sealed"), "{rows}");
    assert!(s.schedule(&[]).is_empty());
    for (i, a) in [101u128, 102].into_iter().enumerate() {
        let (r, logs, evidence, kinds) = &before[i];
        assert_eq!(s.result(a).as_ref(), Some(r));
        assert_eq!(&s.kinds(a), kinds, "no process was started on reopen");
        let out = s.record(a).output.join("generation-1");
        assert_eq!(
            &["stdout.raw", "stderr.raw"].map(|f| fs::read(out.join(f)).unwrap()),
            logs
        );
        assert_eq!(&s.read(|v| v.attempt_evidence(OpaqueId(a))), evidence);
        for (_, blob) in evidence {
            s.store.verify(blob).unwrap();
        }
    }
    assert_eq!(s.read(|v| v.scheduler_queue()).len(), queue.len());

    // A human starts recovery; fresh attempts run in new worktrees, originals unchanged.
    let ins = inspect_recovery(s.e(), &os(), &|_| None).unwrap();
    let start = ins.clone();
    s.cmd(move |tx| {
        tx.start_after_recovery(&start, human("T15 operator"))?;
        Ok("started".into())
    })
    .unwrap();
    for a in [101u128, 102] {
        let ins = ins.clone();
        let choice = s
            .cmd(move |tx| {
                tx.resolve_recovery(
                    &ins,
                    OpaqueId(a),
                    RecoveryChoice::ContinueFreshContext,
                    human("T15 reviewer"),
                )
            })
            .unwrap();
        assert!(!choice.response.is_empty());
    }
    for q in s.read(|v| v.scheduler_queue()) {
        let (step, revision) = (q.step, q.control_revision);
        if q.intent != "RUN" {
            s.cmd(move |tx| {
                tx.control_scheduled_step(step, revision, "RUN", 0)?;
                Ok("run".into())
            })
            .unwrap();
        }
    }
    s.provision(201, 0, &b0, None);
    s.provision(202, 1, &b1, None);
    let mut admitted = s.schedule(&[201, 202]);
    admitted.sort();
    assert_eq!(admitted, vec![(1, 201), (2, 202)]);
    released(&s.run_parallel(&[(201, 1, R1), (202, 2, R1)], || true));
    for (i, (a, fresh)) in [(101u128, 201u128), (102, 202)].into_iter().enumerate() {
        succeeded(&s.settle(fresh));
        assert_ne!(s.record(fresh).path, s.record(a).path);
        assert_eq!(s.result(a).as_ref(), Some(&before[i].0));
    }
}

#[test]
fn t15_same_repository_preprovisioned_worktrees_overlap_only_under_elevated_policy() {
    let elevated = {
        let mut s = S::new(1, &[(0, ClaudeCode), (0, Codex)]);
        s.approve_all(vec![]);
        s.queue_base(1);
        s.queue_base(2);
        // Elevated policy: the per-repository slot is raised from the default one to two.
        s.configure(2, 1, 2);
        let base = s.repos[0].1.clone();
        s.provision(101, 0, &base, None);
        s.provision(102, 0, &base, None);
        let mut admitted = s.schedule(&[101, 102]);
        admitted.sort();
        assert_eq!(admitted, vec![(1, 101), (2, 102)]);
        released(&s.run_parallel(&[(101, 1, R1), (102, 2, R1)], || {
            s.live(101);
            s.live(102);
            true
        }));
        let (r1, r2) = (s.settle(101), s.settle(102));
        succeeded(&r1);
        succeeded(&r2);
        let (w1, w2) = (s.record(101), s.record(102));
        assert_eq!(w1.common_dir(), w2.common_dir(), "one repository");
        assert_ne!(w1.path, w2.path);
        assert_ne!(w1.output, w2.output);
        assert_ne!(w1.branch, w2.branch);
        for (a, file) in [(101, "src/step1.rs"), (102, "src/step2.rs")] {
            let receipt = s.validate(a).unwrap();
            assert!(receipt.passed, "{:?}", receipt.reasons);
            assert_eq!(receipt.changes, vec![format!("Added 000000 100644 {file}")]);
        }
        assert_eq!(git(s.repos[0].0.path(), &["status", "--porcelain"]), "");
        (r1.candidate, r2.candidate)
    };
    assert_ne!(elevated.0, elevated.1);
    // The default per-repository slot of one defers the second pre-provisioned worktree.
    let mut s = S::new(1, &[(0, ClaudeCode), (0, Codex)]);
    s.approve_all(vec![]);
    s.queue_base(1);
    s.queue_base(2);
    s.configure(2, 1, 1);
    let base = s.repos[0].1.clone();
    s.provision(101, 0, &base, None);
    s.provision(102, 0, &base, None);
    let admitted = s.schedule(&[101, 102]);
    assert_eq!(admitted.len(), 1, "{admitted:?}");
    let waiting = if admitted[0].1 == 101 { 102 } else { 101 };
    assert_eq!(s.record(waiting).state, "READY");
}

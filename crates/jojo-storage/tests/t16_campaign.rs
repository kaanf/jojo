//! T16 crash, resource and durability campaign over the integrated P1-15 scenario. Faults
//! are injected at the real boundaries (writer fail points, blob publication, log spools,
//! file permissions, a real SIGKILL of a disposable helper process) and every case is judged
//! by one rule: a valid committed result, or an explicit interruption/recovery hold. Never a
//! false success, an unsafe replacement, a silently ignored claim or an auto-replayed launch.
//! Only the workspace fixture and this test binary are launched; no provider or network.
mod common;
use common::attempts::*;
use common::request;
use common::scenario::*;
use jojo_domain::attempt::AttemptOutcome;
use jojo_domain::identity::*;
use jojo_domain::ipc::*;
use jojo_domain::json::Json;
use jojo_domain::provider::*;
use jojo_domain::recovery::{LaunchStage, RecoveryChoice};
use jojo_domain::scheduler::*;
use jojo_domain::shutdown::{CloseState, ShutdownOutcome};
use jojo_domain::time::ClockReading;
use jojo_storage::*;
use jojo_test_support::dependency::node;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};
use std::{fs, os::unix::fs::PermissionsExt};

const ROLE: &str = "JOJO_T16_HELPER_ROLE";

/// One Claude step in repository 0, approved, queued and provisioned (101) at cap one.
fn single() -> S {
    let mut s = S::new(1, &[(0, ClaudeCode)]);
    s.approve_all(vec![]);
    s.queue_base(1);
    s.configure(1, 1, 1);
    let base = s.repos[0].1.clone();
    s.provision(101, 0, &base, None);
    s
}

fn reserved() -> S {
    let s = single();
    assert_eq!(s.schedule(&[101]), vec![(1, 101)]);
    s
}

fn try_schedule(s: &S, attempts: &[u128]) -> Result<usize, String> {
    let proofs = attempts
        .iter()
        .map(|a| inspect_worktree(s.e(), OpaqueId(*a)).unwrap())
        .collect();
    let config = s.config;
    s.cmd(move |tx| {
        Ok(tx
            .schedule_owned_worktrees(config, proofs)?
            .iter()
            .filter(|d| d.attempt.is_some())
            .count()
            .to_string())
    })
    .map(|r| r.response.parse().unwrap())
    .map_err(|e| format!("{e:?}"))
}

fn claim_states(s: &S, a: u128) -> Vec<ClaimState> {
    let holder = AggregateKey::new("scheduler_attempt", OpaqueId(a));
    s.read(|v| v.live_claims())
        .into_iter()
        .filter(|c| c.holder == holder)
        .map(|c| c.state)
        .collect()
}

fn inspect(s: &S, inspector: &dyn ProcessInspector) -> RecoveryInspection {
    inspect_recovery(s.e(), inspector, &|_| None).unwrap()
}

fn start_recovery(s: &S) {
    let ins = inspect(s, &os());
    s.cmd(move |tx| {
        tx.start_after_recovery(&ins, human("T16 operator"))?;
        Ok("started".into())
    })
    .unwrap();
}

fn resolve(s: &S, a: u128, choice: RecoveryChoice) -> Result<String, String> {
    let ins = inspect(s, &os());
    s.cmd(move |tx| tx.resolve_recovery(&ins, OpaqueId(a), choice, human("T16 reviewer")))
        .map(|r| r.response)
        .map_err(|e| format!("{e:?}"))
}

fn recover(s: &S, a: u128, choice: RecoveryChoice) -> AttemptResult {
    let ins = inspect(s, &os());
    let ids = s.ids();
    match recover_attempt(
        &s.ctx(&ids),
        &ins,
        OpaqueId(a),
        choice,
        human("T16 reviewer"),
    )
    .unwrap()
    {
        RecoveryOutcome::Settled(Settlement::Sealed(r)) => *r,
        other => panic!("expected a sealed result, got {other:?}"),
    }
}

/// What the campaign accepts after a fault and a helper crash.
#[derive(Debug, PartialEq)]
enum Verdict {
    /// The attempt was never reserved: no claim, launch or recovery subject.
    Nothing,
    /// No result; an explicit recovery hold with every claim quarantined.
    Hold(LaunchStage),
    /// One immutable result whose referenced evidence verifies.
    Sealed(AttemptOutcome),
}

/// The single acceptance rule, evaluated right after a crash and reopen.
fn verdict(s: &S, a: u128) -> Verdict {
    assert_eq!(s.admission(), Admission::Closed, "reopen never admits work");
    assert!(s.read(|v| v.dispatchable_attempt_effects()).is_empty());
    let ins = inspect(s, &os());
    let claims = claim_states(s, a);
    match s.result(a) {
        Some(r) => {
            for (_, blob) in s.read(|v| v.attempt_evidence(OpaqueId(a))) {
                s.store.verify(&blob).unwrap();
            }
            assert!(claims.is_empty(), "sealed work holds no claim: {claims:?}");
            if r.outcome == AttemptOutcome::Succeeded {
                assert!(r.candidate.is_some() && r.stop_cause.is_none(), "{r:?}");
                assert!(
                    ins.subject(OpaqueId(a)).is_none(),
                    "success needs no decision"
                );
            }
            let ids = s.ids();
            let again = settle_attempt(&s.ctx(&ids), OpaqueId(a), BlobFaults::default());
            assert!(again.is_err(), "a result is sealed exactly once");
            Verdict::Sealed(r.outcome)
        }
        None => match ins.subject(OpaqueId(a)) {
            None => {
                assert!(claims.is_empty(), "claims without a hold: {claims:?}");
                assert!(!s.read(|v| v.process_launch_issued(OpaqueId(a))));
                Verdict::Nothing
            }
            Some(sub) => {
                assert!(!sub.assessment().choices.is_empty());
                assert!(
                    !claims.is_empty() && claims.iter().all(|c| *c == ClaimState::Quarantined),
                    "{claims:?}"
                );
                Verdict::Hold(sub.facts().stage)
            }
        },
    }
}

/// Crashes the helper and checks that reopening launched nothing new.
fn crash_and_judge(s: &mut S, a: u128) -> Verdict {
    let launched = s.read(|v| v.process_records(OpaqueId(a))).len();
    s.crash();
    let v = verdict(s, a);
    assert_eq!(s.read(|v| v.process_records(OpaqueId(a))).len(), launched);
    v
}

fn referenced_blobs_exist(s: &S) {
    for digest in s.read(|v| v.referenced_blobs()) {
        assert!(
            s.store.path(&digest).exists(),
            "dangling reference {digest}"
        );
    }
}

#[test]
fn t16_fault_boundary_matrix_leaves_a_valid_result_or_an_explicit_hold() {
    use LaunchStage::*;
    use RecoveryChoice::*;
    // Admission: the scheduler transaction fails before its receipt or its commit.
    for point in [FailPoint::BeforeReceipt, FailPoint::BeforeCommit] {
        let mut s = single();
        s.faults.arm(point);
        assert!(try_schedule(&s, &[101]).is_err(), "{point:?}");
        assert!(claim_states(&s, 101).is_empty());
        assert_eq!(s.record(101).state, "READY");
        assert_eq!(crash_and_judge(&mut s, 101), Verdict::Nothing, "{point:?}");
        start_recovery(&s);
        // The old epoch's worktree is never reused; a fresh one is admitted exactly once.
        assert!(inspect_worktree(s.e(), OpaqueId(101)).is_err());
        let base = s.repos[0].1.clone();
        s.provision(201, 0, &base, None);
        assert_eq!(try_schedule(&s, &[201]), Ok(1), "a clean retry admits once");
    }
    // Launch: issued, then the helper died before any process identity was registered.
    let mut s = reserved();
    let spec = s.spec(101, 1, R1);
    s.cmd(move |tx| Ok(tx.issue_process_launch(&spec)?.to_string()))
        .unwrap();
    assert_eq!(
        crash_and_judge(&mut s, 101),
        Verdict::Hold(IssuedWithoutIdentity)
    );
    // Launch: spawned and registered; the developer outlives the helper.
    drop(s);
    let mut s = reserved();
    let live = s.spawn_registered(101, 1, R1);
    assert_eq!(crash_and_judge(&mut s, 101), Verdict::Hold(Spawned));
    assert!(
        !inspect(&s, &os())
            .subject(OpaqueId(101))
            .unwrap()
            .assessment()
            .process_absent
    );
    fs::write(s.barriers().join("release"), b"").unwrap();
    until(|| live.root_exit().is_some());
    drop(live);
    assert!(
        inspect(&s, &os())
            .subject(OpaqueId(101))
            .unwrap()
            .assessment()
            .process_absent
    );
    // Stop: a shutdown stop concluded, then the helper died before settlement.
    drop(s);
    let mut s = reserved();
    s.run_parallel(&[(101, 1, R1)], || {
        s.cmd(|tx| {
            tx.request_shutdown_stop(OpaqueId(101))?;
            Ok("stop".into())
        })
        .unwrap();
        false
    });
    assert_eq!(crash_and_judge(&mut s, 101), Verdict::Hold(Concluded));
    start_recovery(&s);
    let r = recover(&s, 101, ContinueFreshContext);
    assert_eq!(r.outcome, AttemptOutcome::Interrupted, "{r:?}");
    // Commit: the developer finished, then the helper died before settlement.
    drop(s);
    let mut s = reserved();
    s.run_parallel(&[(101, 1, R1)], || true);
    assert_eq!(crash_and_judge(&mut s, 101), Verdict::Hold(Concluded));
    start_recovery(&s);
    assert_eq!(
        recover(&s, 101, ValidateOutput).outcome,
        AttemptOutcome::Succeeded
    );
    // Commit: settlement fails before its receipt or its commit; nothing partial remains.
    drop(s);
    for point in [FailPoint::BeforeReceipt, FailPoint::BeforeCommit] {
        let mut s = reserved();
        s.run_parallel(&[(101, 1, R1)], || true);
        s.faults.arm(point);
        let ids = s.ids();
        assert!(settle_attempt(&s.ctx(&ids), OpaqueId(101), BlobFaults::default()).is_err());
        assert_eq!(s.result(101), None, "{point:?}");
        referenced_blobs_exist(&s);
        assert_eq!(
            crash_and_judge(&mut s, 101),
            Verdict::Hold(Concluded),
            "{point:?}"
        );
    }
    // Blob: publication fails before or after the rename; never a dangling reference.
    for (before_rename, after_rename) in [(true, false), (false, true)] {
        let mut s = reserved();
        s.run_parallel(&[(101, 1, R1)], || true);
        let ids = s.ids();
        let faults = BlobFaults {
            before_rename,
            after_rename,
        };
        let settled = settle_attempt(&s.ctx(&ids), OpaqueId(101), faults).unwrap();
        assert!(
            matches!(settled, Settlement::RecoveryRequired(_)),
            "{settled:?}"
        );
        assert_eq!(s.result(101), None);
        referenced_blobs_exist(&s);
        // Unreferenced bytes stay where publication stopped; nothing references them.
        let orphans = s.store.orphans(&s.read(|v| v.referenced_blobs())).unwrap();
        let place = if after_rename { "blobs/" } else { "tmp/" };
        assert!(orphans.iter().any(|o| o.starts_with(place)), "{orphans:?}");
        assert_eq!(crash_and_judge(&mut s, 101), Verdict::Hold(Concluded));
        start_recovery(&s);
        assert_eq!(
            recover(&s, 101, ValidateOutput).outcome,
            AttemptOutcome::Succeeded
        );
    }
    // Log: the stdout spool fails mid-run; evidence loss is never a success.
    let mut s = reserved();
    let hooks = LaunchHooks {
        spool: SpoolFaults {
            stdout_fail_after: Some(8),
            stderr_fail_after: None,
        },
        ..LaunchHooks::default()
    };
    let report = s.run_spec(&s.spec(101, 1, R1), hooks);
    assert!(report.evidence_lost, "{report:?}");
    let ids = s.ids();
    if let Ok(Settlement::Sealed(r)) =
        settle_attempt(&s.ctx(&ids), OpaqueId(101), BlobFaults::default())
    {
        assert_ne!(r.outcome, AttemptOutcome::Succeeded, "{r:?}");
    }
    match crash_and_judge(&mut s, 101) {
        Verdict::Sealed(o) => assert_ne!(o, AttemptOutcome::Succeeded),
        Verdict::Hold(_) => {}
        Verdict::Nothing => panic!("a launched attempt cannot vanish"),
    }
    // Sealed before the crash: the result survives unchanged and needs no decision.
    drop(s);
    let mut s = reserved();
    s.run_parallel(&[(101, 1, R1)], || true);
    let sealed = s.settle(101);
    assert_eq!(
        crash_and_judge(&mut s, 101),
        Verdict::Sealed(AttemptOutcome::Succeeded)
    );
    assert_eq!(s.result(101), Some(sealed));
}

/// Deterministic xorshift for seeded command schedules.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn t16_seeded_concurrent_schedules_keep_capacity_ownership_and_receipts() {
    for seed in 1..=6u64 {
        let mut s = S::new(
            2,
            &[(0, ClaudeCode), (1, Codex), (0, Codex), (1, ClaudeCode)],
        );
        s.approve_all(vec![]);
        for step in 1..=4 {
            s.queue_base(step);
        }
        s.configure(2, 1, 1);
        for a in 101..=104u128 {
            let (repo, base) = (
                s.steps[a as usize - 101].0,
                s.repos[s.steps[a as usize - 101].0].1.clone(),
            );
            s.provision(a, repo, &base, None);
        }
        let receipts = Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for t in 0..3u64 {
                let (s, receipts) = (&s, &receipts);
                scope.spawn(move || {
                    let mut rng = Rng(seed * 0x9e37_79b9 + t + 1);
                    for i in 0..12u64 {
                        let r = rng.next();
                        let step = node(1 + (r >> 8) as u128 % 4);
                        match r % 5 {
                            0 => {
                                let picks: Vec<u128> = (101..=104)
                                    .filter(|_| rng.next().is_multiple_of(2))
                                    .collect();
                                let _ = try_schedule(s, &picks);
                            }
                            1 | 2 => {
                                let intent = ["HOLD", "RUN", "CANCEL"][(r >> 16) as usize % 3];
                                if let Some(q) = s
                                    .read(|v| v.scheduler_queue())
                                    .into_iter()
                                    .find(|q| q.step == step)
                                {
                                    let rev = q.control_revision;
                                    let _ = s.cmd(move |tx| {
                                        tx.control_scheduled_step(step, rev, intent, 0)?;
                                        Ok(intent.into())
                                    });
                                }
                            }
                            3 => {
                                let expected = 1 + r % 3;
                                let _ = s.cmd(move |tx| {
                                    let v = tx.configure_resources(
                                        expected,
                                        1,
                                        BTreeMap::from([
                                            ("global".into(), 1),
                                            ("provider:claude".into(), 1),
                                            ("provider:codex".into(), 1),
                                        ]),
                                    )?;
                                    Ok(v.to_string())
                                });
                            }
                            _ => {
                                // The same command identity twice: one execution, one receipt.
                                let id =
                                    50_000 + seed as u128 * 1_000 + t as u128 * 100 + i as u128;
                                let run = || {
                                    s.e().execute(request(s.e(), id, "T16", "probe"), |tx| {
                                        Ok(format!("{:?}", tx.admission()?))
                                    })
                                };
                                let (a, b) = (run(), run());
                                if let (Ok(a), Ok(b)) = (a, b) {
                                    receipts.lock().unwrap().push((a.response, b.response));
                                }
                            }
                        }
                    }
                });
            }
        });
        for (a, b) in receipts.into_inner().unwrap() {
            assert_eq!(a, b, "seed {seed}: duplicate command changed its response");
        }
        let check = |s: &S, when: &str| {
            let claims = s.read(|v| v.live_claims());
            let holders = |key: &str| {
                claims
                    .iter()
                    .filter(|c| c.resource_key == key)
                    .map(|c| c.holder.clone())
                    .collect::<BTreeSet<_>>()
                    .len()
            };
            assert!(holders("global") <= 2, "seed {seed} {when}: {claims:?}");
            for key in ["provider:claude", "provider:codex"] {
                assert!(holders(key) <= 1, "seed {seed} {when} {key}: {claims:?}");
            }
            // Per-repository slot one: never two holders of one repository.
            let repo_keys: BTreeSet<_> = claims
                .iter()
                .map(|c| c.resource_key.clone())
                .filter(|k| k.starts_with("repository:"))
                .collect();
            for key in repo_keys {
                assert!(holders(&key) <= 1, "seed {seed} {when} {key}: {claims:?}");
            }
            // Every holder is a reserved attempt with a bound private worktree and output.
            for c in &claims {
                assert_eq!(c.holder.kind, "scheduler_attempt", "{c:?}");
                let a = c.holder.id.0;
                assert_eq!(s.record(a).state, "BOUND", "seed {seed} {when}: {c:?}");
                let own: BTreeSet<_> = claims
                    .iter()
                    .filter(|x| x.holder == c.holder)
                    .map(|x| x.resource_key.clone())
                    .collect();
                assert!(own.contains(&format!("worktree:{a:032x}")), "{own:?}");
            }
            // The append-only event log has no gaps.
            let cursor = s.read(|v| v.event_cursor()).0;
            let mut seen = 0u64;
            while seen < cursor {
                let page = s.read(|v| v.ipc_events(seen, 8));
                let Json::List(rows) = page else { panic!() };
                assert!(!rows.is_empty());
                for row in rows {
                    let Json::Int(seq) = get(&row, "seq").unwrap() else {
                        panic!()
                    };
                    assert_eq!(*seq as u64, seen + 1, "seed {seed} {when}: event gap");
                    seen += 1;
                }
            }
        };
        check(&s, "running");
        let before = s.read(|v| v.live_claims()).len();
        s.crash();
        check(&s, "after crash");
        let after = s.read(|v| v.live_claims());
        assert_eq!(after.len(), before, "seed {seed}: a crash released claims");
        assert!(after.iter().all(|c| c.state == ClaimState::Quarantined));
    }
}

#[test]
fn t16_fake_reboot_and_clock_jumps_never_free_live_ownership() {
    let mut s = S::new(1, &[(0, ClaudeCode), (0, Codex)]);
    s.approve_all(vec![]);
    s.queue_base(1);
    s.queue_base(2);
    s.configure(2, 1, 1);
    let base = s.repos[0].1.clone();
    s.provision(101, 0, &base, None);
    s.provision(102, 0, &base, None);
    assert_eq!(s.schedule(&[101, 102]).len(), 1);
    let live = s.spawn_registered(101, 1, R1);
    // Sleep/wake or reboot: the wall clock jumps forward, then backwards, in a new clock
    // epoch, across helper deaths. Expiry is never permission to release live ownership.
    for (i, jump) in [86_400_000i64, -2 * 86_400_000].into_iter().enumerate() {
        s.clock.set(ClockReading {
            unix_millis: 1_700_000_000_000 + jump,
            monotonic_nanos: 0,
            epoch: ClockEpochId(OpaqueId(10 + i as u128)),
        });
        s.crash();
        let held = claim_states(&s, 101);
        assert!(!held.is_empty() && held.iter().all(|c| *c == ClaimState::Quarantined));
        let ins = inspect(&s, &os());
        assert!(
            !ins.subject(OpaqueId(101))
                .unwrap()
                .assessment()
                .process_absent
        );
        start_recovery(&s);
        // A worktree provisioned in an earlier epoch is never silently reused.
        let old = inspect_worktree(s.e(), OpaqueId(102)).unwrap_err();
        assert!(old.contains("recovery inspection"), "{old}");
        assert!(resolve(&s, 101, RecoveryChoice::ContinueFreshContext).is_err());
    }
    // The quarantined repository claim blocks even provisioning a competitor (its request
    // waits for the exclusive administration gate, which also pauses new admissions).
    let (r, rt, b) = (s.repos[0].0.clone(), s.root.clone(), base.clone());
    s.cmd(move |tx| tx.request_worktree(OpaqueId(302), &r, &rt, &b, None))
        .unwrap();
    let err = materialize_worktree(s.e(), OpaqueId(302), None).unwrap_err();
    assert!(err.contains("exclusive"), "{err}");
    // After a real reboot the recorded identity may be unverifiable or its PID reused:
    // neither inspection releases anything, and an unknown identity blocks every choice.
    for check in [
        IdentityCheck::Unknown("boot identity changed".into()),
        IdentityCheck::Reused,
    ] {
        let ins = inspect(&s, &Scripted(check.clone()));
        let sub = ins.subject(OpaqueId(101)).unwrap();
        if matches!(check, IdentityCheck::Unknown(_)) {
            assert!(!sub.assessment().process_absent, "{check:?}");
        }
        let held = claim_states(&s, 101);
        assert!(
            held.iter().all(|c| *c == ClaimState::Quarantined),
            "{check:?}"
        );
    }
    assert!(
        live.root_exit().is_none(),
        "nothing signalled the live developer"
    );
    fs::write(s.barriers().join("release"), b"").unwrap();
    until(|| live.root_exit().is_some());
    drop(live);
    assert!(
        inspect(&s, &os())
            .subject(OpaqueId(101))
            .unwrap()
            .assessment()
            .process_absent
    );
    // Its output was never durably captured, so recovery seals no false success.
    let r = recover(&s, 101, RecoveryChoice::ContinueFreshContext);
    assert_ne!(r.outcome, AttemptOutcome::Succeeded, "{r:?}");
    assert!(claim_states(&s, 101).is_empty());
    // With the hold resolved, the waiting competitor obtains its creation ticket.
    let (r, rt, b) = (s.repos[0].0.clone(), s.root.clone(), base.clone());
    let ticket = s
        .cmd(move |tx| tx.request_worktree(OpaqueId(302), &r, &rt, &b, None))
        .unwrap();
    assert_eq!(ticket.response, "CREATING");
    let proof = materialize_worktree(s.e(), OpaqueId(302), None).unwrap();
    s.cmd(move |tx| {
        tx.seal_worktree(proof)?;
        Ok("sealed".into())
    })
    .unwrap();
    assert_eq!(try_schedule(&s, &[302]), Ok(1));
}

fn close_receipt(s: &mut S) -> ShutdownReceipt {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(r) = s.ui().finish(false).unwrap() {
            return r;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
}

/// Runs the developer through the helper's worker pool and waits until it is RUNNING.
fn worker(s: &mut S, a: u128, step: u128) {
    let (tx, rx) = mpsc::channel();
    let spec = s.spec(a, step, R1);
    let launch = s.launch(&spec);
    let hooks = LaunchHooks {
        inspector: Some(ps()),
        on_event: Some(Box::new(move |p| {
            if matches!(p, EventPayload::SessionIdentified(_)) {
                let _ = tx.send(());
            }
        })),
        ..LaunchHooks::default()
    };
    let plan = StopPlan::new(&caps(Support::Supported, Support::Supported), BUDGETS);
    s.ui().launch(spec, launch, plan, hooks).unwrap();
    rx.recv_timeout(Duration::from_secs(20)).unwrap();
}

fn confirm(s: &mut S) -> Envelope {
    let prepared = s.request("PrepareClose", object([]), None);
    assert_eq!(prepared.kind, "Response", "{prepared:?}");
    let ticket = s.ui().status().unwrap().ticket.unwrap().ticket;
    s.request("ConfirmClose", object([("ticket", identity(ticket))]), None)
}

fn chmod(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn t16_disk_failures_on_log_result_and_stop_flush_fail_the_clean_close() {
    // Log flush: the spool fails, so the close cannot acknowledge clean preservation.
    let mut s = reserved();
    let hooks = LaunchHooks {
        spool: SpoolFaults {
            stdout_fail_after: Some(8),
            stderr_fail_after: None,
        },
        ..LaunchHooks::default()
    };
    assert!(s.run_spec(&s.spec(101, 1, R1), hooks).evidence_lost);
    assert_eq!(confirm(&mut s).kind, "Response");
    let r = close_receipt(&mut s);
    assert_ne!(r.outcome, ShutdownOutcome::Ready, "{r:?}");
    assert!(!r.persistence_sealed, "{r:?}");
    assert_eq!(s.read(|v| v.session()).shutdown, ShutdownMarker::Running);
    assert_ne!(
        s.result(101).map(|r| r.outcome),
        Some(AttemptOutcome::Succeeded)
    );

    // Result flush: the evidence store refuses writes while the stopped work settles.
    drop(s);
    let mut s = reserved();
    worker(&mut s, 101, 1);
    let evidence = s.dir.path().join("evidence");
    for d in ["blobs", "tmp"] {
        chmod(&evidence.join(d), 0o500);
    }
    assert_eq!(confirm(&mut s).kind, "Response");
    let r = close_receipt(&mut s);
    for d in ["blobs", "tmp"] {
        chmod(&evidence.join(d), 0o700);
    }
    assert_ne!(r.outcome, ShutdownOutcome::Ready, "{r:?}");
    assert!(r.process_stop_confirmed && !r.persistence_sealed, "{r:?}");
    assert_eq!(s.read(|v| v.session()).shutdown, ShutdownMarker::Running);
    assert!(matches!(crash_and_judge(&mut s, 101), Verdict::Hold(_)));

    // Stop intent: committing the confirmed close fails; only an explicit unclean
    // emergency remains, which still stops the process but never claims a clean close.
    drop(s);
    let mut s = reserved();
    worker(&mut s, 101, 1);
    let prepared = s.request("PrepareClose", object([]), None);
    assert_eq!(prepared.kind, "Response", "{prepared:?}");
    let ticket = s.ui().status().unwrap().ticket.unwrap().ticket;
    s.faults.arm(FailPoint::BeforeCommit);
    let confirmed = s.request("ConfirmClose", object([("ticket", identity(ticket))]), None);
    assert_eq!(confirmed.kind, "Error", "{confirmed:?}");
    assert_ne!(s.ui().status().unwrap().state, CloseState::Stopping);
    s.ui().emergency().unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let r = loop {
        if let Some(r) = s.ui().finish(true).unwrap() {
            break r;
        }
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    };
    assert_eq!(r.outcome, ShutdownOutcome::Emergency, "{r:?}");
    assert!(r.process_stop_confirmed, "{r:?}");
    assert_ne!(s.read(|v| v.session()).shutdown, ShutdownMarker::Clean);
    match crash_and_judge(&mut s, 101) {
        Verdict::Sealed(o) => assert_ne!(o, AttemptOutcome::Succeeded),
        Verdict::Hold(_) => {}
        Verdict::Nothing => panic!("a launched attempt cannot vanish"),
    }
}

/// A cold copy of the data root: database files and the evidence store.
fn copy_data(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        if name.starts_with("jojo.sqlite3") {
            fs::copy(entry.path(), to.join(&name)).unwrap();
        }
    }
    copy_tree(&from.join("evidence"), &to.join("evidence"));
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    chmod(to, 0o700);
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn clear_data(dir: &Path) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        if entry
            .file_name()
            .into_string()
            .unwrap()
            .starts_with("jojo.sqlite3")
        {
            fs::remove_file(entry.path()).unwrap();
        }
    }
    fs::remove_dir_all(dir.join("evidence")).unwrap();
}

#[test]
fn t16_restored_data_root_keeps_integrity_and_dead_candidates_stay_inspectable() {
    let mut s = S::new(2, &[(0, ClaudeCode), (1, Codex)]);
    s.approve_all(vec![]);
    s.queue_base(1);
    s.queue_base(2);
    s.configure(2, 1, 1);
    let (b0, b1) = (s.repos[0].1.clone(), s.repos[1].1.clone());
    s.provision(101, 0, &b0, None);
    s.provision(102, 1, &b1, None);
    assert_eq!(s.schedule(&[101, 102]).len(), 2);
    worker(&mut s, 101, 1);
    worker(&mut s, 102, 2);
    assert_eq!(confirm(&mut s).kind, "Response");
    assert_eq!(close_receipt(&mut s).outcome, ShutdownOutcome::Ready);
    let results: Vec<_> = [101, 102].map(|a| s.result(a).unwrap()).into();
    // Back up the stopped helper's data root, lose it, restore it.
    drop(s.ui.take());
    let backup = s.dir.path().join("backup");
    copy_data(s.dir.path(), &backup);
    clear_data(s.dir.path());
    copy_data(&backup, s.dir.path());
    s.reopen();
    assert_eq!(s.admission(), Admission::Closed);
    for (a, before) in [101u128, 102].into_iter().zip(&results) {
        assert_eq!(s.result(a).as_ref(), Some(before));
        assert_eq!(before.outcome, AttemptOutcome::Interrupted);
        for (_, blob) in s.read(|v| v.attempt_evidence(OpaqueId(a))) {
            s.store.verify(&blob).unwrap();
        }
        // The known-dead candidate is still inspectable: its exact packet reopens.
        let candidate = before.candidate.clone().unwrap();
        let reference = s
            .read(|v| v.candidate_reference(OpaqueId(a), &candidate))
            .unwrap();
        CandidateSnapshot::reopen(&reference.path, &candidate).unwrap();
    }
    referenced_blobs_exist(&s);
    assert!(inspect(&s, &os()).subject(OpaqueId(101)).is_some());

    // Newer work after the backup: fresh attempts run and seal.
    start_recovery(&s);
    for a in [101u128, 102] {
        resolve(&s, a, RecoveryChoice::ContinueFreshContext).unwrap();
    }
    for q in s.read(|v| v.scheduler_queue()) {
        let (step, rev) = (q.step, q.control_revision);
        if q.intent != "RUN" {
            s.cmd(move |tx| {
                tx.control_scheduled_step(step, rev, "RUN", 0)?;
                Ok("run".into())
            })
            .unwrap();
        }
    }
    s.provision(201, 0, &b0, None);
    s.provision(202, 1, &b1, None);
    assert_eq!(s.schedule(&[201, 202]).len(), 2);
    s.run_parallel(&[(201, 1, R1), (202, 2, R1)], || true);
    let newer = s.settle(201);
    s.settle(202);
    let newer_tree = s.record(201).path;
    // Restoring the older backup over it: nothing newer is adopted, deleted or replayed.
    drop(s.ui.take());
    clear_data(s.dir.path());
    copy_data(&backup, s.dir.path());
    s.reopen();
    assert_eq!(s.admission(), Admission::Closed);
    assert_eq!(s.result(201), None);
    assert!(
        newer_tree.join("src/step1.rs").exists(),
        "newer work is preserved"
    );
    // The restored evidence store matches the restored database exactly.
    referenced_blobs_exist(&s);
    let orphans = s.store.orphans(&s.read(|v| v.referenced_blobs())).unwrap();
    assert!(orphans.is_empty(), "{orphans:?}");
    assert!(!s.store.path(newer.candidate.as_ref().unwrap()).exists());
    let reuse = s.cmd({
        let (r, rt) = (s.repos[0].0.clone(), s.root.clone());
        move |tx| tx.request_worktree(OpaqueId(201), &r, &rt, &b0, None)
    });
    if reuse.is_ok() {
        assert!(materialize_worktree(s.e(), OpaqueId(201), None).is_err());
    }
    assert!(
        newer_tree.join("src/step1.rs").exists(),
        "never overwritten"
    );
    // A corrupted referenced blob is detected, never trusted.
    let (_, blob) = s
        .read(|v| v.attempt_evidence(OpaqueId(101)))
        .into_iter()
        .find(|(k, _)| k == "STDOUT")
        .unwrap();
    let path = s.store.path(&blob.digest);
    chmod(&path, 0o600);
    fs::write(&path, b"tampered").unwrap();
    assert!(s.store.verify(&blob).is_err());
}

#[test]
fn t16_unknown_process_or_repository_inspection_blocks() {
    let mut s = reserved();
    let live = s.spawn_registered(101, 1, R1);
    s.crash();
    let unknown = inspect(
        &s,
        &Scripted(IdentityCheck::Unknown("ps unavailable".into())),
    );
    let sub = unknown.subject(OpaqueId(101)).unwrap();
    assert!(!sub.assessment().process_absent);
    assert!(
        !sub.assessment()
            .choices
            .contains(&RecoveryChoice::ContinueFreshContext)
    );
    let held = claim_states(&s, 101);
    assert!(!held.is_empty() && held.iter().all(|c| *c == ClaimState::Quarantined));
    fs::write(s.barriers().join("release"), b"").unwrap();
    until(|| live.root_exit().is_some());
    drop(live);
    // The repository disappears (moved by the user): its worktree cannot be verified.
    let repo = s.repos[0].0.path().to_path_buf();
    let moved = repo.with_extension("moved");
    fs::rename(&repo, &moved).unwrap();
    let ins = inspect(&s, &os());
    let sub = ins.subject(OpaqueId(101)).unwrap();
    assert!(
        !sub.assessment().blockers.is_empty(),
        "{:?}",
        sub.assessment()
    );
    assert!(resolve(&s, 101, RecoveryChoice::ValidateOutput).is_err());
    assert!(
        claim_states(&s, 101)
            .iter()
            .all(|c| *c == ClaimState::Quarantined)
    );
    fs::rename(&moved, &repo).unwrap();
    let ins = inspect(&s, &os());
    assert!(
        ins.subject(OpaqueId(101))
            .unwrap()
            .assessment()
            .process_absent
    );
}

#[test]
fn t16_cancellation_during_resource_drain_releases_the_barrier_only() {
    let mut s = S::new(1, &[(0, ClaudeCode), (0, Codex), (0, ClaudeCode)]);
    s.author_prd();
    s.approve(PRD);
    s.author_adr().unwrap();
    s.approve(ADR);
    let mut p = s.feature_plan(vec![]);
    p.resources[1].repository_mode = ResourceMode::Exclusive;
    s.put_plan(&p, None);
    s.approve(PLAN);
    s.configure(3, 2, 2);
    let base = s.repos[0].1.clone();
    for a in 101..=103 {
        s.provision(a, 0, &base, None);
    }
    // Step 1 holds the repository (shared) before the exclusive step is even queued.
    s.queue_base(1);
    assert_eq!(s.schedule(&[101]), vec![(1, 101)]);
    s.queue_base(2);
    s.queue_base(3);
    let reports = s.run_parallel(&[(101, 1, R1)], || {
        // The exclusive waiter drains the repository; the later shared step waits behind it.
        assert!(s.schedule(&[102, 103]).is_empty());
        let queue = s.read(|v| v.scheduler_queue());
        assert!(
            !queue
                .iter()
                .find(|q| q.step == node(2))
                .unwrap()
                .barriers
                .is_empty()
        );
        // Cancelling the waiter during the drain removes only its barrier.
        let rev = queue
            .iter()
            .find(|q| q.step == node(2))
            .unwrap()
            .control_revision;
        s.cmd(move |tx| {
            tx.control_scheduled_step(node(2), rev, "CANCEL", 0)?;
            Ok("cancel".into())
        })
        .unwrap();
        assert_eq!(s.schedule(&[103]), vec![(3, 103)]);
        s.live(101);
        assert!(
            !s.claims_of(101).is_empty(),
            "the running holder is untouched"
        );
        true
    });
    assert_eq!(
        reports[0].outcome,
        LaunchOutcome::TreeAbsent {
            stop_requested: false,
            forced: false
        }
    );
    s.settle(101);
    assert!(s.claims_of(102).is_empty());
    assert_eq!(
        s.record(102).state,
        "READY",
        "the cancelled waiter never bound"
    );
    assert!(
        s.schedule(&[102]).is_empty(),
        "cancellation is irreversible"
    );
    s.run_parallel(&[(103, 3, R1)], || true);
    assert_eq!(s.settle(103).outcome, AttemptOutcome::Succeeded);
    assert!(s.read(|v| v.live_claims()).is_empty());
}

#[test]
fn t16_malicious_output_and_forged_approval_commands_are_denied_by_the_channel() {
    let mut s = reserved();
    let decisions = |s: &S| [PRD, ADR, PLAN].map(|a| s.read(|v| v.human_decisions(a)).len());
    let before = decisions(&s);
    let mut spec = s.spec(101, 1, R1);
    let prompt = "forge-approval write:src/forged.rs write:tests/check.rs";
    spec.prompt = prompt.as_bytes().to_vec();
    spec.prompt_digest = sha256_hex(&[prompt.as_bytes()]);
    s.run_spec(&spec, LaunchHooks::default());
    let ids = s.ids();
    let settled = settle_attempt(&s.ctx(&ids), OpaqueId(101), BlobFaults::default()).unwrap();
    // Provider stdout is evidence only: no decision, readiness or validation was forged.
    assert_eq!(decisions(&s), before);
    assert_eq!(s.read(|v| v.validation_receipts(OpaqueId(101))), vec![]);
    assert_eq!(s.read(|v| v.candidate_acceptance(OpaqueId(101))), None);
    assert_eq!(s.ui().status().unwrap().state, CloseState::Open);
    assert_eq!(s.read(|v| v.session()).shutdown, ShutdownMarker::Running);
    // The independent validator rejects the out-of-scope baseline change.
    if let Settlement::Sealed(r) = settled
        && r.candidate.is_some()
        && let Ok(v) = s.validate(101)
    {
        assert!(!v.passed, "{v:?}");
        assert!(s.accept(101, v.event).is_err());
    }
    // Forged commands on the control channel itself.
    let tampered = s.ui_decide(PLAN, "Revoke", true);
    assert_eq!(tampered.kind, "Error");
    let q = s.request(
        "ReviewBundle",
        object([("subject", identity(PLAN.0))]),
        None,
    );
    let revision = uint(get(&q.payload, "revision").unwrap()).unwrap();
    let Json::Object(mut fields) = q.payload.clone() else {
        panic!()
    };
    fields.retain(|(k, _)| k != "revision");
    fields.push(("action".into(), text("Revoke")));
    let stale = s.request(
        "HumanDecision",
        Json::Object(fields.clone()),
        Some(revision + 1),
    );
    assert_eq!(stale.kind, "Error", "{stale:?}");
    let mut wrong_epoch = Envelope {
        major: 1,
        minor: 0,
        session: OpaqueId(5),
        epoch: s.e().epoch().0 + 1,
        request: OpaqueId(900_001),
        revision: Some(revision),
        kind: "HumanDecision".into(),
        payload: Json::Object(fields.clone()),
    };
    assert_eq!(s.ui().request(&wrong_epoch).kind, "Error");
    wrong_epoch.epoch = s.e().epoch().0;
    wrong_epoch.session = OpaqueId(6);
    assert_eq!(
        s.ui().request(&wrong_epoch).kind,
        "Error",
        "another session"
    );
    assert_eq!(decisions(&s), before, "no forged decision was recorded");
}

/// Not a test of its own: a disposable helper process for the SIGKILL test below.
#[test]
#[ignore = "helper role executed in a child process by the SIGKILL test"]
fn t16_helper_role() {
    if std::env::var(ROLE).is_err() {
        return;
    }
    let s = reserved();
    let live = s.spawn_registered(101, 1, R1);
    println!("T16-READY {}", s.dir.path().display());
    // Owns the lock, the claims and a live developer until it is killed.
    std::thread::sleep(Duration::from_secs(120));
    drop(live);
    std::process::exit(1);
}

#[test]
fn t16_real_sigkill_of_a_disposable_helper_quarantines_and_never_kills_by_name() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "t16_helper_role",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ROLE, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let (tx, rx) = mpsc::channel();
    let stdout = child.stdout.take().unwrap();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            // The harness prints the test name on the same line before the marker.
            if let Some((_, dir)) = line.split_once("T16-READY ") {
                let _ = tx.send(PathBuf::from(dir));
            }
        }
    });
    let dir = rx.recv_timeout(Duration::from_secs(60)).unwrap();
    // SIGKILL through the exact child handle: no name lookup, no cleanup code runs.
    child.kill().unwrap();
    let status = child.wait().unwrap();
    assert_eq!(
        std::os::unix::process::ExitStatusExt::signal(&status),
        Some(9)
    );
    let engine =
        Engine::start(EngineConfig::new(dir.join("jojo.sqlite3"), common::clock())).unwrap();
    let previous = engine.startup_report().epoch.previous.unwrap();
    assert_ne!(previous.1, ShutdownMarker::Clean);
    assert!(engine.epoch().0 > previous.0.0);
    let read = |f: &dyn Fn(&ReadView<'_>) -> Vec<ClaimRecord>| {
        engine.reader().unwrap().read(|v| Ok(f(v))).unwrap()
    };
    let claims = read(&|v| v.live_claims().unwrap());
    assert!(!claims.is_empty() && claims.iter().all(|c| c.state == ClaimState::Quarantined));
    let session = engine.reader().unwrap().read(|v| v.session()).unwrap();
    assert_eq!(session.admission, Admission::Closed);
    // The orphaned developer is still alive: killing the helper did not kill it by name.
    let ins = inspect_recovery(&engine, &os(), &|_| None).unwrap();
    let sub = ins.subject(OpaqueId(101)).unwrap();
    assert_eq!(sub.facts().stage, LaunchStage::Spawned);
    assert!(!sub.assessment().process_absent);
    assert_eq!(
        engine
            .reader()
            .unwrap()
            .read(|v| v.attempt_result(OpaqueId(101)))
            .unwrap(),
        None
    );
    // Release it; once proven absent the hold can be resolved by a human.
    fs::write(dir.join("barriers/release"), b"").unwrap();
    until(|| {
        inspect_recovery(&engine, &os(), &|_| None)
            .unwrap()
            .subject(OpaqueId(101))
            .unwrap()
            .assessment()
            .process_absent
    });
    let ins = inspect_recovery(&engine, &os(), &|_| None).unwrap();
    let r = engine.execute(request(&engine, 77, "T16", "resolve"), move |tx| {
        tx.resolve_recovery(
            &ins,
            OpaqueId(101),
            RecoveryChoice::Hold,
            human("T16 reviewer"),
        )
    });
    assert!(r.is_ok(), "{r:?}");
    drop(engine);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn t16_permission_boundaries_refuse_shared_or_substituted_roots() {
    let s = reserved();
    s.run_parallel(&[(101, 1, R1)], || true);
    s.settle(101);
    let tmp = s.dir.path().join("permissions");
    fs::create_dir(&tmp).unwrap();
    // A group/world-accessible or symlinked data root is refused before any database opens.
    let shared = tmp.join("shared-data");
    fs::create_dir(&shared).unwrap();
    chmod(&shared, 0o755);
    assert!(prepare_data_root(&shared).is_err());
    let link = tmp.join("linked-data");
    let private = tmp.join("private-data");
    fs::create_dir(&private).unwrap();
    chmod(&private, 0o700);
    std::os::unix::fs::symlink(&private, &link).unwrap();
    assert!(prepare_data_root(&link).is_err());
    // A world-writable worktree root is refused.
    let owned = tmp.join("owned-shared");
    fs::create_dir(&owned).unwrap();
    chmod(&owned, 0o777);
    assert!(WorktreeRoot::inspect(&owned).is_err());
    // The production data root is created private; the database lives only inside it.
    let root = tmp.join("data");
    let db = prepare_data_root(&root).unwrap();
    assert_eq!(db.parent().unwrap(), root);
    assert_eq!(fs::metadata(&root).unwrap().permissions().mode() & 0o077, 0);
    // The evidence store is private to the user.
    for p in [
        s.dir.path().join("evidence"),
        s.dir.path().join("evidence/blobs"),
    ] {
        let mode = fs::metadata(&p).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "{p:?} is {mode:o}");
    }
    let evidence = s.read(|v| v.attempt_evidence(OpaqueId(101)));
    assert!(!evidence.is_empty());
    for (_, blob) in evidence {
        let mode = fs::metadata(s.store.path(&blob.digest))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "blob {} is {mode:o}", blob.digest);
    }
}

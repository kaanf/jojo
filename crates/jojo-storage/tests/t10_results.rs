//! T10 immutable outcomes, independent validation and evidence. Real OS processes (only
//! the workspace's `jojo-process-fixture`), temporary local Git repositories and on-disk
//! evidence blobs; no provider CLI, credential, network or remote effect.
mod common;
use common::{TempDir, request, start};
use jojo_domain::attempt::{AttemptBindings, AttemptOutcome};
use jojo_domain::dependency::*;
use jojo_domain::identity::*;
use jojo_domain::provider::*;
use jojo_domain::scheduler::*;
use jojo_domain::stream::CLAUDE_STREAM_JSON;
use jojo_domain::time::{Clock, ClockReading};
use jojo_domain::validation::*;
use jojo_storage::*;
use jojo_test_support::{
    COMPLETE_PRD_JSON, SequenceIds,
    dependency::{node, plan},
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Barrier, mpsc};
use std::time::{Duration, Instant};
use std::{fs, os::unix::fs::PermissionsExt};

const FIXTURE: &str = env!("CARGO_BIN_EXE_jojo-process-fixture");
const TESTS: [&str; 2] = ["unit::check", "unit::lib"];

fn inspector() -> PathBuf {
    std::env::var_os("JOJO_TEST_PS")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/bin/ps".into())
}

struct RealClock(Instant);
impl Clock for RealClock {
    fn now(&self) -> ClockReading {
        ClockReading {
            unix_millis: 1_700_000_000_000,
            monotonic_nanos: self.0.elapsed().as_nanos() as u64,
            epoch: ClockEpochId(OpaqueId(9)),
        }
    }
}

// Real process tests run one at a time so CPU contention cannot eat stop budgets.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("/usr/bin/git")
        .env_clear()
        .envs([
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z"),
            ("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z"),
        ])
        .args([
            "-c",
            "user.name=T10",
            "-c",
            "user.email=t10@example.invalid",
        ])
        .args(["-c", "core.hooksPath=/dev/null", "-C"])
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_owned()
}

/// The reviewed validation plan for steps `1..=n`: write `src`, keep `tests` identical.
fn validation_plan(n: usize) -> ValidationPlan {
    ValidationPlan {
        steps: (1..=n as u128)
            .map(|i| StepValidation {
                step: node(i),
                write_scope: vec![ScopePath::parse("src").unwrap()],
                baseline: vec![ScopePath::parse("tests").unwrap()],
                tests: TESTS.map(String::from).to_vec(),
                command: vec![],
            })
            .collect(),
    }
}

/// One engine, one repository and `n` reserved attempts (101..) with bound worktrees,
/// under a typed plan whose `validation_digest` binds [`validation_plan`].
struct F {
    _serial: std::sync::MutexGuard<'static, ()>,
    dir: TempDir,
    engine: Option<Engine>,
    store: BlobStore,
    next: AtomicU64,
    namespace: AtomicU64,
    paths: Vec<(PathBuf, PathBuf)>,
    plan: String,
}

impl F {
    fn new(n: usize) -> Self {
        let serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let dir = TempDir::new("t10");
        let rp = dir.path().join("repo");
        for d in ["src", "tests", "docs"] {
            fs::create_dir_all(rp.join(d)).unwrap();
        }
        git(&rp, &["init", "--template=", "--initial-branch=main"]);
        for (p, body) in [
            ("file", "base\n"),
            (".gitignore", "*.log\n"),
            ("src/lib.rs", "pub fn f() {}\n"),
            ("tests/check.rs", "fn check() { assert!(true) }\n"),
            ("docs/readme.md", "docs\n"),
            ("script.sh", "true\n"),
        ] {
            fs::write(rp.join(p), body).unwrap();
        }
        git(&rp, &["add", "--all"]);
        git(&rp, &["commit", "-m", "fixture"]);
        let base = git(&rp, &["rev-parse", "HEAD"]);
        let repo = LocalRepository::inspect(&rp).unwrap();
        let owned = dir.path().join("owned");
        fs::create_dir(&owned).unwrap();
        fs::set_permissions(&owned, fs::Permissions::from_mode(0o700)).unwrap();
        let root = WorktreeRoot::inspect(&owned).unwrap();
        let e = start(&dir.db());
        let store = BlobStore::for_engine(&e).unwrap();
        let mut f = Self {
            _serial: serial,
            dir,
            engine: Some(e),
            store,
            next: AtomicU64::new(1),
            namespace: AtomicU64::new(1000),
            paths: vec![],
            plan: validation_plan(n).canonical(),
        };
        let ids: Vec<u128> = (0..n as u128).map(|i| 101 + i).collect();
        for a in &ids {
            let (r, rt, b) = (repo.clone(), root.clone(), base.clone());
            let a = *a;
            f.cmd(move |tx| tx.request_worktree(OpaqueId(a), &r, &rt, &b, None))
                .unwrap();
            let proof = materialize_worktree(f.e(), OpaqueId(a), None).unwrap();
            f.cmd(move |tx| {
                tx.seal_worktree(proof)?;
                Ok("sealed".into())
            })
            .unwrap();
        }
        let config = f.queue(&repo, &base, n as u32);
        let proofs = ids
            .iter()
            .map(|a| inspect_worktree(f.e(), OpaqueId(*a)).unwrap())
            .collect();
        f.cmd(move |tx| {
            Ok(format!(
                "{:?}",
                tx.schedule_owned_worktrees(config, proofs)?
            ))
        })
        .unwrap();
        for a in ids {
            let r = f.record(a);
            assert_eq!(r.state, "BOUND");
            f.paths.push((r.path, r.output));
        }
        f
    }

    fn e(&self) -> &Engine {
        self.engine.as_ref().unwrap()
    }

    fn ids(&self) -> SequenceIds {
        SequenceIds::new(self.namespace.fetch_add(1, Ordering::SeqCst), 1)
    }

    fn record(&self, a: u128) -> WorktreeRecord {
        self.read(|v| v.worktree(OpaqueId(a))).unwrap()
    }

    fn read<T>(&self, f: impl FnOnce(&ReadView<'_>) -> Result<T, StorageError>) -> T {
        self.e().reader().unwrap().read(f).unwrap()
    }

    fn cmd<FN>(&self, f: FN) -> Result<Receipt, CommandError>
    where
        FN: FnOnce(&mut Tx<'_>) -> Result<String, TxError> + Send + 'static,
    {
        let id = self.next.fetch_add(1, Ordering::SeqCst) as u128;
        self.e().execute(request(self.e(), id, "T10", "fixture"), f)
    }

    fn decide(&self, a: ArtifactId, action: DecisionAction) {
        let b = self.read(|v| v.review_bundle(a)).unwrap();
        let id = self.next.fetch_add(1, Ordering::SeqCst) as u128;
        self.e()
            .human_decide(
                CommandId(OpaqueId(id)),
                HumanOrigin::FakeHumanFixture {
                    actor: "T10 human".into(),
                },
                b,
                action,
            )
            .unwrap();
    }

    fn queue(&self, repo: &LocalRepository, base: &str, n: u32) -> u64 {
        let (prd, adr, pid) = (
            ArtifactId(OpaqueId(901)),
            ArtifactId(OpaqueId(902)),
            ArtifactId(OpaqueId(903)),
        );
        let p = PreparedPrd::import(COMPLETE_PRD_JSON.as_bytes()).unwrap();
        self.cmd(move |tx| {
            tx.create_artifact(
                prd,
                FeatureId(OpaqueId(1)),
                &p,
                &Provenance::new(Author::Human, "T10"),
            )?;
            Ok("prd".into())
        })
        .unwrap();
        self.decide(prd, DecisionAction::Approve);
        self.cmd(move |tx| {
            tx.put_review_document(adr, None, prd, None, "T10 ADR", "fixture reviewed")?;
            Ok("adr".into())
        })
        .unwrap();
        self.decide(adr, DecisionAction::Approve);
        let mut p = plan(&(1..=n as u128).collect::<Vec<_>>(), vec![]);
        p.schema = 2;
        p.required_outputs = vec![CANDIDATE_OUTPUT.into()];
        p.validation_digest = sha256_hex(&[self.plan.as_bytes()]);
        for i in 0..n as usize {
            p.inputs[i].base_sources[0] = Source {
                repository: repo.id(),
                revision: SourceRevision::Commit(base.to_owned()),
            };
            p.resources.push(StepResources {
                step: node(i as u128 + 1),
                repository: repo.id(),
                provider: ProviderKind::ClaudeCode,
                priority: 0,
                additional: vec![],
                repository_mode: ResourceMode::Shared,
            });
        }
        for a in [prd, adr] {
            let approval = self.read(|v| v.current_artifact_approval(a)).unwrap();
            p.artifacts.nodes.push(NodeRef {
                id: a.0,
                revision: approval.revision,
            });
            p.approvals.push(approval);
        }
        let copy = p.clone();
        self.cmd(move |tx| {
            tx.put_execution_plan(pid, None, prd, adr, &copy, "T10 reviewed")?;
            Ok("plan".into())
        })
        .unwrap();
        self.decide(pid, DecisionAction::Approve);
        for (i, input) in p.inputs.into_iter().enumerate() {
            self.cmd(move |tx| {
                tx.queue_step(
                    prd,
                    adr,
                    pid,
                    Revision(1),
                    node(i as u128 + 1),
                    &SourceVector {
                        sources: input.base_sources,
                        environment: Some("test-env".into()),
                    },
                )?;
                Ok("queued".into())
            })
            .unwrap();
        }
        self.cmd(move |tx| {
            tx.configure_resources(
                1,
                n,
                std::collections::BTreeMap::from([
                    ("global".into(), n),
                    ("provider:claude".into(), n),
                    ("provider:codex".into(), n),
                ]),
            )?;
            tx.set_admission(Admission::Open)?;
            Ok("config".into())
        })
        .unwrap();
        2
    }

    fn barriers(&self) -> PathBuf {
        let b = self.dir.path().join("barriers");
        let _ = fs::create_dir(&b);
        b
    }

    fn spec(&self, i: usize, scenario: &str) -> LaunchSpec {
        let (cwd, out) = self.paths[i].clone();
        let n = 101 + i as u128;
        let record = self.record(n);
        let vector = self.read(|v| v.attempt_source_vector(OpaqueId(n))).unwrap();
        let approvals = [901, 902, 903].map(|subject| {
            OpaqueId(u128::from(
                self.read(|v| v.applicable_decision(ArtifactId(OpaqueId(subject))))
                    .unwrap()
                    .0,
            ))
        });
        LaunchSpec {
            attempt: AttemptId(OpaqueId(n)),
            bindings: AttemptBindings {
                feature_id: FeatureId(OpaqueId(1)),
                plan_id: PlanId(OpaqueId(903)),
                plan_revision: Revision(1),
                step_id: StepId(OpaqueId(i as u128 + 1)),
                step_revision: Revision(1),
                repository_id: record.repository,
                worktree_id: WorktreeId(OpaqueId(n)),
            },
            engine_epoch: OpaqueId(u128::from(self.e().epoch().0)),
            dispatch_generation: 1,
            fingerprint: ProviderFingerprint {
                provider: ProviderKind::ClaudeCode,
                executable: FIXTURE.into(),
                version: "fixture-v1".into(),
                schema: PROVIDER_SCHEMA,
                parser: CLAUDE_STREAM_JSON.into(),
                configuration: "config-digest".into(),
            },
            source: SourceBinding {
                common_dir: record.common_dir().to_owned(),
                base_commit: record.base.clone(),
                candidate_source_vector: sha256_hex(&[vector.canonical().as_bytes()]),
                instruction_digest: "instructions".into(),
            },
            prompt_digest: sha256_hex(&[scenario.as_bytes()]),
            prompt: scenario.as_bytes().to_vec(),
            continuation: None,
            role: Role::Developer,
            permissions: PermissionEnvelope {
                approved_digest: "permission".into(),
                tools: vec![],
                filesystem_roots: vec![cwd.clone()],
                network: false,
                hooks: false,
                background: false,
            },
            cwd,
            environment_allowlist: vec!["LANG".into()],
            config_references: vec![],
            timeout_millis: 60_000,
            max_output_bytes: 1024 * 1024,
            max_frame_bytes: 64 * 1024,
            raw_output_dir: out,
            approval_ids: approvals,
            required_outputs: vec![CANDIDATE_OUTPUT.into()],
            validation_plan: self.plan.clone(),
        }
    }

    fn run_with(&self, i: usize, scenario: &str, mut hooks: LaunchHooks) -> LaunchReport {
        hooks.inspector.get_or_insert_with(inspector);
        let spec = self.spec(i, scenario);
        let ids = self.ids();
        let clock = RealClock(Instant::now());
        let ctx = LaunchContext {
            engine: self.e(),
            ids: &ids,
            clock: &clock,
            poll: Duration::from_millis(10),
        };
        let caps = Capabilities {
            evidence: "fixture".into(),
            structured_events: Support::Supported,
            explicit_sessions: Support::Supported,
            exact_continuation: Support::Unsupported,
            incremental_input: Support::Unsupported,
            question_ids: Support::Unsupported,
            permission_response: Support::Unsupported,
            turn_interrupt: Support::Supported,
            process_tree: Support::Unknown,
            continuation_cwd_change: Support::Unsupported,
            auth_status: Support::Unknown,
            workspace_policy: Support::Unknown,
        };
        let budgets = StopBudgets {
            interrupt: Duration::from_secs(3),
            terminate: Duration::from_secs(3),
            kill: Duration::from_secs(5),
            drain: Duration::from_millis(500),
        };
        let launch = ProcessLaunch::from_spec(
            &spec,
            vec![self.barriers().display().to_string()],
            vec![("LANG".into(), "C".into())],
        )
        .unwrap();
        run_launch(&ctx, &spec, &launch, &StopPlan::new(&caps, budgets), hooks).unwrap()
    }

    /// Runs to proven tree absence.
    fn run(&self, i: usize, scenario: &str) -> LaunchReport {
        let r = self.run_with(i, scenario, LaunchHooks::default());
        assert!(
            matches!(r.outcome, LaunchOutcome::TreeAbsent { .. }),
            "{:?}",
            r.outcome
        );
        r
    }

    /// Runs `scenario`; once the session is identified, `during` commits from this thread.
    fn run_and<FN>(&self, i: usize, scenario: &str, during: FN) -> LaunchReport
    where
        FN: FnOnce(&mut Tx<'_>) -> Result<String, TxError> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let hooks = LaunchHooks {
            on_event: Some(Box::new(move |p| {
                if matches!(p, EventPayload::SessionIdentified(_)) {
                    let _ = tx.send(());
                }
            })),
            ..LaunchHooks::default()
        };
        std::thread::scope(|s| {
            let h = s.spawn(|| self.run_with(i, scenario, hooks));
            rx.recv_timeout(Duration::from_secs(20)).unwrap();
            self.cmd(during).unwrap();
            h.join().unwrap()
        })
    }

    fn ctx<'a>(&'a self, ids: &'a SequenceIds) -> SettlementContext<'a> {
        SettlementContext {
            engine: self.e(),
            ids,
            store: &self.store,
        }
    }

    fn settle_with(&self, a: u128, faults: BlobFaults) -> Result<Settlement, String> {
        let ids = self.ids();
        settle_attempt(&self.ctx(&ids), OpaqueId(a), faults)
    }

    fn sealed(&self, a: u128) -> AttemptResult {
        match self.settle_with(a, BlobFaults::default()).unwrap() {
            Settlement::Sealed(r) => *r,
            other => panic!("expected a sealed result, got {other:?}"),
        }
    }

    fn recovery(&self, a: u128) -> Vec<String> {
        match self.settle_with(a, BlobFaults::default()).unwrap() {
            Settlement::RecoveryRequired(r) => r,
            other => panic!("expected recovery, got {other:?}"),
        }
    }

    fn claims_of(&self, a: u128) -> Vec<String> {
        let holder = AggregateKey::new("scheduler_attempt", OpaqueId(a));
        self.read(|v| v.live_claims())
            .into_iter()
            .filter(|c| c.holder == holder)
            .map(|c| c.resource_key)
            .collect()
    }

    fn result(&self, a: u128) -> Option<AttemptResult> {
        self.read(|v| v.attempt_result(OpaqueId(a)))
    }

    fn report(&self, a: u128, results: &[(&str, TestStatus)]) -> ValidatorReport {
        let candidate = self.result(a).unwrap().candidate.unwrap();
        let raw = format!("{results:?}").into_bytes();
        ValidatorReport {
            origin: ValidatorOrigin::IndependentFixture {
                name: "t10-validator".into(),
            },
            attempt: OpaqueId(a),
            candidate,
            plan_digest: sha256_hex(&[self.plan.as_bytes()]),
            results: results.iter().map(|(t, s)| (t.to_string(), *s)).collect(),
            raw,
        }
    }

    fn validate(&self, plan: &str, report: ValidatorReport) -> Result<ValidationReceipt, String> {
        let ids = self.ids();
        validate_candidate(&self.ctx(&ids), plan.as_bytes(), report)
    }

    fn accept(&self, a: u128, receipt: EventSeq) -> Result<Receipt, CommandError> {
        self.cmd(move |tx| {
            tx.accept_candidate(
                OpaqueId(a),
                receipt,
                HumanOrigin::FakeHumanFixture {
                    actor: "T10 reviewer".into(),
                },
            )?;
            Ok("accepted".into())
        })
    }
}

const ALL_PASS: [(&str, TestStatus); 2] = [
    ("unit::check", TestStatus::Passed),
    ("unit::lib", TestStatus::Passed),
];

#[test]
fn t10_success_seals_one_immutable_result_with_verified_evidence_and_releases_claims() {
    let mut f = F::new(1);
    f.run(0, "edit write:src/feature.rs");
    assert!(f.claims_of(101).len() > 2, "claims held until settled");
    let r = f.sealed(101);
    assert_eq!(
        (
            r.outcome,
            r.reason.as_str(),
            r.exit_code,
            r.stop_cause.as_deref()
        ),
        (AttemptOutcome::Succeeded, "terminal_success", Some(0), None)
    );
    let evidence = f.read(|v| v.attempt_evidence(OpaqueId(101)));
    assert_eq!(
        evidence.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        ["CANDIDATE", "STDERR", "STDOUT"]
    );
    for (_, blob) in &evidence {
        f.store.verify(blob).unwrap();
    }
    let stdout = fs::read(f.paths[0].1.join("generation-1/stdout.raw")).unwrap();
    assert_eq!(r.stdout.as_deref(), Some(sha256_hex(&[&stdout]).as_str()));
    assert_eq!(
        r.candidate,
        evidence
            .iter()
            .find(|(k, _)| k == "CANDIDATE")
            .map(|(_, b)| b.digest.clone())
    );
    assert!(f.claims_of(101).is_empty(), "settled holder released");
    assert!(
        f.read(|v| v.approval_result(OpaqueId(101)))
            .unwrap()
            .starts_with("SUCCEEDED")
    );
    let queue = f.read(|v| v.scheduler_queue());
    assert_eq!(
        queue[0].intent, "HOLD",
        "a finished step is never re-dispatched"
    );
    // One result per attempt: a second settlement or finalization is refused.
    let again = f.settle_with(101, BlobFaults::default()).unwrap_err();
    assert!(again.contains("already finalized"), "{again}");
    // The result, evidence and refusal survive a clean restart in a new epoch.
    let before = f.result(101);
    f.engine.take().unwrap().shutdown().unwrap();
    f.engine = Some(start(&f.dir.db()));
    assert_eq!(f.result(101), before);
    for (_, blob) in f.read(|v| v.attempt_evidence(OpaqueId(101))) {
        f.store.verify(&blob).unwrap();
    }
    assert!(f.settle_with(101, BlobFaults::default()).is_err());
}

#[test]
fn t10_exit_zero_without_terminal_with_complete_coverage_is_protocol_incomplete() {
    let f = F::new(1);
    f.run(0, "exit-zero-no-terminal");
    let r = f.sealed(101);
    assert_eq!(
        (r.outcome, r.reason.as_str(), r.exit_code),
        (AttemptOutcome::Failed, "protocol_incomplete", Some(0))
    );
    assert!(r.candidate.is_some(), "candidate preserved for inspection");
    assert!(f.claims_of(101).is_empty());
}

#[test]
fn t10_terminal_success_with_nonzero_exit_fails() {
    let f = F::new(1);
    let report = f.run(0, "claude-ok-exit-3");
    assert_eq!(report.terminal, Some(TerminalStatus::Success));
    let r = f.sealed(101);
    assert_eq!(
        (r.outcome, r.reason.as_str(), r.exit_code),
        (AttemptOutcome::Failed, "exit_mismatch", Some(3))
    );
}

#[test]
fn t10_cancel_before_finalization_wins_and_cancel_after_finalization_changes_nothing() {
    let f = F::new(2);
    f.run(0, "claude-ok");
    f.cmd(|tx| {
        tx.cancel_approved_attempt(OpaqueId(101))?;
        Ok("cancel".into())
    })
    .unwrap();
    let r = f.sealed(101);
    assert_eq!(
        (r.outcome, r.reason.as_str()),
        (AttemptOutcome::Cancelled, "task_cancelled")
    );
    assert!(r.stop_cause.unwrap().starts_with("TASK_CANCEL"));
    assert!(
        r.uncertainties.iter().any(|u| u.contains("Success")),
        "later terminal success retained as evidence: {:?}",
        r.uncertainties
    );
    assert!(r.candidate.is_some());

    f.run(1, "claude-ok");
    let sealed = f.sealed(102);
    assert_eq!(sealed.outcome, AttemptOutcome::Succeeded);
    f.cmd(|tx| {
        tx.cancel_approved_attempt(OpaqueId(102))?;
        Ok("late cancel".into())
    })
    .unwrap();
    assert_eq!(f.result(102), Some(sealed), "sealed result is immutable");
    assert_eq!(f.read(|v| v.attempt_stop_cause(OpaqueId(102))), None);
}

#[test]
fn t10_shutdown_stop_without_terminal_is_interrupted() {
    let f = F::new(1);
    let report = f.run_and(0, "wait-release", |tx| {
        tx.request_shutdown_stop(OpaqueId(101))?;
        Ok("shutdown".into())
    });
    assert!(matches!(
        report.outcome,
        LaunchOutcome::TreeAbsent {
            stop_requested: true,
            ..
        }
    ));
    assert_eq!(report.terminal, None);
    let r = f.sealed(101);
    assert_eq!(
        (r.outcome, r.reason.as_str()),
        (AttemptOutcome::Interrupted, "shutdown")
    );
    assert!(r.stop_cause.unwrap().starts_with("SHUTDOWN"));
}

#[test]
fn t10_revocation_mid_run_interrupts_and_excludes_acceptance() {
    let f = F::new(1);
    let (tx, rx) = mpsc::channel();
    let hooks = LaunchHooks {
        on_event: Some(Box::new(move |p| {
            if matches!(p, EventPayload::SessionIdentified(_)) {
                let _ = tx.send(());
            }
        })),
        ..LaunchHooks::default()
    };
    let report = std::thread::scope(|s| {
        let h = s.spawn(|| f.run_with(0, "wait-release", hooks));
        rx.recv_timeout(Duration::from_secs(20)).unwrap();
        f.decide(ArtifactId(OpaqueId(903)), DecisionAction::Revoke);
        h.join().unwrap()
    });
    assert!(matches!(
        report.outcome,
        LaunchOutcome::TreeAbsent {
            stop_requested: true,
            ..
        }
    ));
    let r = f.sealed(101);
    assert_eq!(
        (r.outcome, r.reason.as_str()),
        (AttemptOutcome::Interrupted, "approval_invalidated")
    );
    assert!(f.accept(101, EventSeq(1)).is_err());
}

#[test]
fn t10_concurrent_finalizers_have_exactly_one_winner() {
    let f = F::new(1);
    f.run(0, "claude-ok");
    let ids = f.ids();
    let prepared = prepare_settlement(&f.ctx(&ids), OpaqueId(101), BlobFaults::default()).unwrap();
    assert!(prepared.failures().is_empty(), "{:?}", prepared.failures());
    let gate = Barrier::new(2);
    let results: Vec<_> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let ids = f.ids();
                let (gate, prepared, f) = (&gate, &prepared, &f);
                s.spawn(move || {
                    gate.wait();
                    finalize_attempt(&f.ctx(&ids), prepared)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let winners = results
        .iter()
        .filter(|r| matches!(r, Ok(Settlement::Sealed(_))))
        .count();
    assert_eq!(winners, 1, "{results:?}");
    assert!(
        results
            .iter()
            .any(|r| r.as_ref().is_err_and(|e| e.contains("already finalized")))
    );
}

#[test]
fn t10_tampered_spool_is_lost_coverage_and_stays_recovery_required_with_claims() {
    let f = F::new(1);
    f.run(0, "claude-ok");
    let spool = f.paths[0].1.join("generation-1/stdout.raw");
    let bytes = fs::read(&spool).unwrap();
    fs::write(&spool, &bytes[..bytes.len() - 1]).unwrap();
    let reasons = f.recovery(101);
    assert!(
        reasons.iter().any(|r| r.contains("durably checkpointed")),
        "{reasons:?}"
    );
    assert_eq!(f.result(101), None, "no result while coverage is lost");
    assert!(!f.claims_of(101).is_empty(), "claims retained");
    assert_eq!(f.read(|v| v.attempt_recovery_holds(OpaqueId(101))).len(), 1);
    // The truncated log is never re-published or reconstructed as evidence.
    assert!(
        f.read(|v| v.attempt_evidence(OpaqueId(101)))
            .iter()
            .all(|(k, _)| k != "STDOUT")
    );
}

#[test]
fn t10_missing_or_corrupt_referenced_blob_blocks_and_is_never_reconstructed() {
    let f = F::new(2);
    for (i, a) in [(0, 101u128), (1, 102)] {
        f.run(i, "claude-ok");
        let ids = f.ids();
        let first = prepare_settlement(&f.ctx(&ids), OpaqueId(a), BlobFaults::default()).unwrap();
        assert!(first.failures().is_empty());
        let (_, blob) = f
            .read(|v| v.attempt_evidence(OpaqueId(a)))
            .into_iter()
            .find(|(k, _)| k == "STDOUT")
            .unwrap();
        let path = f.store.path(&blob.digest);
        if a == 101 {
            fs::remove_file(&path).unwrap();
        } else {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            let mut bytes = fs::read(&path).unwrap();
            bytes[0] ^= 1;
            fs::write(&path, bytes).unwrap();
        }
        let reasons = f.recovery(a);
        assert!(
            reasons
                .iter()
                .any(|r| r.contains("STDOUT") && r.contains(&blob.digest)),
            "{reasons:?}"
        );
        assert_eq!(f.result(a), None);
        if a == 101 {
            assert!(!path.exists(), "missing evidence is not reconstructed");
        }
    }
}

#[test]
fn t10_blob_publication_faults_leave_orphans_never_dangling_references() {
    let f = F::new(1);
    f.run(0, "claude-ok");
    let reasons = match f
        .settle_with(
            101,
            BlobFaults {
                before_rename: true,
                ..BlobFaults::default()
            },
        )
        .unwrap()
    {
        Settlement::RecoveryRequired(r) => r,
        other => panic!("{other:?}"),
    };
    assert!(reasons.iter().any(|r| r.contains("before blob rename")));
    assert!(f.read(|v| v.attempt_evidence(OpaqueId(101))).is_empty());
    let referenced = f.read(|v| v.referenced_blobs());
    let orphans = f.store.orphans(&referenced).unwrap();
    assert!(orphans.iter().all(|o| o.starts_with("tmp/")), "{orphans:?}");
    assert!(!orphans.is_empty());

    assert!(matches!(
        f.settle_with(
            101,
            BlobFaults {
                after_rename: true,
                ..BlobFaults::default()
            }
        )
        .unwrap(),
        Settlement::RecoveryRequired(_)
    ));
    let referenced = f.read(|v| v.referenced_blobs());
    assert!(
        referenced.is_empty(),
        "no reference without a completed publication"
    );
    let published = f.store.orphans(&referenced).unwrap();
    assert!(published.iter().any(|o| o.starts_with("blobs/")));
    // A clean retry reuses the verified orphan bytes and seals normally.
    let r = f.sealed(101);
    assert_eq!(r.outcome, AttemptOutcome::Succeeded);
    let referenced = f.read(|v| v.referenced_blobs());
    assert!(
        f.store
            .orphans(&referenced)
            .unwrap()
            .iter()
            .all(|o| o.starts_with("tmp/"))
    );
}

#[test]
fn t10_process_unknown_cannot_finalize_and_keeps_claims() {
    let f = F::new(1);
    let barriers = f.barriers();
    let go = barriers.join("go");
    let hooks = LaunchHooks {
        on_event: Some(Box::new(move |p| {
            if matches!(p, EventPayload::SessionIdentified(_)) {
                fs::write(&go, b"").unwrap();
            }
        })),
        ..LaunchHooks::default()
    };
    let report = f.run_with(0, "escaped-holder", hooks);
    fs::write(barriers.join("release-linger"), b"").unwrap();
    let watchdog = Instant::now();
    while !barriers.join("linger-released").exists() {
        assert!(watchdog.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(report.outcome, LaunchOutcome::StopUnconfirmed(_)));
    let reasons = f.recovery(101);
    assert!(
        reasons.iter().any(|r| r.contains("unconfirmed")),
        "{reasons:?}"
    );
    assert_eq!(f.result(101), None);
    assert!(f.claims_of(101).iter().any(|k| k.starts_with("worktree:")));
    assert!(
        f.cmd(|tx| {
            tx.begin_attempt_settlement(OpaqueId(101))?;
            Ok("begin".into())
        })
        .is_err()
    );
}

#[test]
fn t10_effect_committed_but_ack_lost_stays_issued_unknown_and_is_never_replayed() {
    let mut f = F::new(2);
    let key = "attempt-effect:fake-push";
    let digest = sha256_hex(&[b"push main"]);
    f.run(0, "claude-ok");
    f.cmd(move |tx| {
        tx.journal_attempt_effect(OpaqueId(101), key, "fake-remote", "push main")?;
        Ok("journal".into())
    })
    .unwrap();
    assert_eq!(f.read(|v| v.dispatchable_attempt_effects()), vec![key]);
    f.cmd(move |tx| {
        tx.issue_attempt_effect(key)?;
        Ok("issued".into())
    })
    .unwrap();
    // The fake remote committed, but the acknowledgement was lost: no observation.
    assert!(f.read(|v| v.dispatchable_attempt_effects()).is_empty());
    assert!(
        f.cmd(move |tx| {
            tx.issue_attempt_effect(key)?;
            Ok("again".into())
        })
        .is_err()
    );
    assert!(
        f.cmd(move |tx| {
            tx.journal_attempt_effect(OpaqueId(101), key, "fake-remote", "push main")?;
            Ok("replay".into())
        })
        .is_err()
    );
    let reasons = f.recovery(101);
    assert!(reasons.iter().any(|r| r.contains(key)), "{reasons:?}");
    assert!(!f.claims_of(101).is_empty());
    for (observer, status, d) in [
        (
            "fake-remote:origin",
            RemoteObservation::Unknown,
            digest.clone(),
        ),
        (
            "fake-remote:origin",
            RemoteObservation::Committed,
            sha256_hex(&[b"other"]),
        ),
        ("real-remote", RemoteObservation::Committed, digest.clone()),
    ] {
        let _ = f.cmd(move |tx| {
            tx.record_effect_observation(key, observer, status, &d)?;
            Ok("observed".into())
        });
    }
    let effect = f
        .read(|v| v.effects())
        .into_iter()
        .find(|e| e.operation_key == key)
        .unwrap();
    assert_eq!(
        effect.state,
        EffectState::IssuedUnknown,
        "only an exact fake observation resolves"
    );
    assert_eq!(f.read(|v| v.effect_observations(key)).len(), 1);
    let d = digest.clone();
    f.cmd(move |tx| {
        tx.record_effect_observation(key, "fake-remote:origin", RemoteObservation::Committed, &d)?;
        Ok("observed".into())
    })
    .unwrap();
    assert_eq!(f.sealed(101).outcome, AttemptOutcome::Succeeded);
    assert!(f.claims_of(101).is_empty());
    assert!(
        f.cmd(|tx| {
            tx.journal_attempt_effect(OpaqueId(101), "attempt-effect:late", "fake-remote", "x")?;
            Ok("late".into())
        })
        .is_err(),
        "a sealed attempt journals no further effects"
    );

    // Same ambiguity across an unclean restart: the new epoch neither replays the effect
    // nor finalizes the old attempt; recovery inspection owns it.
    let key2 = "attempt-effect:fake-tag";
    f.run(1, "claude-ok");
    f.cmd(move |tx| {
        tx.journal_attempt_effect(OpaqueId(102), key2, "fake-remote", "tag v1")?;
        tx.issue_attempt_effect(key2)?;
        Ok("issued".into())
    })
    .unwrap();
    drop(f.engine.take());
    f.engine = Some(start(&f.dir.db()));
    let effect = f
        .read(|v| v.effects())
        .into_iter()
        .find(|e| e.operation_key == key2)
        .unwrap();
    assert_eq!(effect.state, EffectState::IssuedUnknown);
    assert!(f.read(|v| v.dispatchable_attempt_effects()).is_empty());
    assert!(
        f.cmd(move |tx| {
            tx.issue_attempt_effect(key2)?;
            Ok("again".into())
        })
        .is_err()
    );
    let err = f.settle_with(102, BlobFaults::default()).unwrap_err();
    assert!(err.contains("earlier engine epoch"), "{err}");
    assert_eq!(f.result(102), None);
}

#[test]
fn t10_in_scope_candidate_passes_independent_validation_and_acceptance_is_separate() {
    let f = F::new(1);
    f.run(0, "edit write:src/feature.rs");
    assert_eq!(f.sealed(101).outcome, AttemptOutcome::Succeeded);
    assert_eq!(f.read(|v| v.candidate_acceptance(OpaqueId(101))), None);
    let receipt = f.validate(&f.plan, f.report(101, &ALL_PASS)).unwrap();
    assert!(receipt.passed, "{:?}", receipt.reasons);
    assert_eq!(receipt.changes, vec!["Added 000000 100644 src/feature.rs"]);
    assert_eq!(
        f.read(|v| v.candidate_acceptance(OpaqueId(101))),
        None,
        "a receipt is not acceptance"
    );
    f.accept(101, receipt.event).unwrap();
    assert_eq!(
        f.read(|v| v.candidate_acceptance(OpaqueId(101))),
        Some(receipt.event)
    );
    assert!(
        f.accept(101, receipt.event).is_err(),
        "acceptance is append-only once"
    );
}

#[test]
fn t10_fake_developer_cannot_change_baseline_or_scope_and_pass() {
    let f = F::new(1);
    f.run(
        0,
        "edit write:src/feature.rs write:tests/check.rs write:docs/new.md",
    );
    let r = f.sealed(101);
    assert_eq!(
        r.outcome,
        AttemptOutcome::Succeeded,
        "invocation success is separate"
    );
    let receipt = f.validate(&f.plan, f.report(101, &ALL_PASS)).unwrap();
    assert!(!receipt.passed);
    for expected in [
        "out-of-scope change \"tests/check.rs\"",
        "out-of-scope change \"docs/new.md\"",
        "baseline modified \"tests/check.rs\"",
    ] {
        assert!(
            receipt.reasons.iter().any(|r| r == expected),
            "{:?}",
            receipt.reasons
        );
    }
    assert!(f.accept(101, receipt.event).is_err());
}

#[test]
fn t10_repository_evidence_catches_deletes_modes_renames_symlinks_and_sibling_prefixes() {
    let f = F::new(1);
    f.run(
        0,
        "edit delete:docs/readme.md chmod:script.sh delete:src/lib.rs write:src2/lib.rs link:src/link=../file",
    );
    assert_eq!(f.sealed(101).outcome, AttemptOutcome::Succeeded);
    let receipt = f.validate(&f.plan, f.report(101, &ALL_PASS)).unwrap();
    assert_eq!(
        receipt.changes,
        vec![
            "Deleted 100644 000000 docs/readme.md",
            "Modified 100644 100755 script.sh",
            "Deleted 100644 000000 src/lib.rs",
            "Added 000000 120000 src/link",
            "Added 000000 100644 src2/lib.rs",
        ]
    );
    let denied: Vec<_> = receipt
        .reasons
        .iter()
        .filter(|r| r.starts_with("out-of-scope"))
        .collect();
    assert_eq!(
        denied,
        [
            "out-of-scope change \"docs/readme.md\"",
            "out-of-scope change \"script.sh\"",
            "out-of-scope change \"src2/lib.rs\"",
        ]
    );
    assert!(!receipt.passed);
}

#[test]
fn t10_zero_missing_failed_forged_and_stale_validations_fail_readiness() {
    let f = F::new(1);
    f.run(0, "edit write:src/feature.rs");
    f.sealed(101);
    let first = f.validate(&f.plan, f.report(101, &ALL_PASS)).unwrap();
    assert!(first.passed);
    let cases: Vec<(Vec<(&str, TestStatus)>, &str)> = vec![
        (vec![], "no tests executed"),
        (
            vec![("unit::lib", TestStatus::Passed)],
            "missing test \"unit::check\"",
        ),
        (
            vec![
                ("unit::check", TestStatus::Passed),
                ("unit::lib", TestStatus::Failed),
            ],
            "test \"unit::lib\" Failed",
        ),
        (
            vec![
                ("unit::check", TestStatus::Passed),
                ("unit::lib", TestStatus::Passed),
                ("unit::new", TestStatus::Passed),
            ],
            "unexpected test \"unit::new\"",
        ),
    ];
    for (results, reason) in cases {
        let receipt = f.validate(&f.plan, f.report(101, &results)).unwrap();
        assert!(
            !receipt.passed && receipt.reasons.iter().any(|r| r == reason),
            "{:?}",
            receipt.reasons
        );
    }
    assert!(
        f.accept(101, first.event).is_err(),
        "an older receipt cannot be accepted"
    );
    // A plan with a narrower inventory is not the reviewed plan, even if internally valid.
    let mut forged = validation_plan(1);
    forged.steps[0].tests = vec!["unit::lib".into()];
    let mut report = f.report(101, &[("unit::lib", TestStatus::Passed)]);
    report.plan_digest = sha256_hex(&[forged.canonical().as_bytes()]);
    let receipt = f.validate(&forged.canonical(), report).unwrap();
    assert!(
        receipt
            .reasons
            .iter()
            .any(|r| r == "validation plan is not the approved plan")
    );
    let mut report = f.report(101, &ALL_PASS);
    report.plan_digest = "0".repeat(64);
    let receipt = f.validate(&f.plan, report).unwrap();
    assert_eq!(receipt.reasons, vec!["validator ran a different plan"]);
    // Fresh pass, then the plan approval is revoked before acceptance: stale.
    let fresh = f.validate(&f.plan, f.report(101, &ALL_PASS)).unwrap();
    assert!(fresh.passed);
    f.decide(ArtifactId(OpaqueId(903)), DecisionAction::Revoke);
    assert!(f.accept(101, fresh.event).is_err());
    let stale = f.validate(&f.plan, f.report(101, &ALL_PASS)).unwrap();
    assert!(
        stale.reasons.iter().any(|r| r.contains("stale")),
        "{:?}",
        stale.reasons
    );
    assert!(
        f.validate("[\"jojo-validation-plan-v1\",[]]", f.report(101, &ALL_PASS))
            .is_err()
    );
}

#[test]
fn t10_unsupported_or_changed_worktree_state_fails_closed_without_receipt() {
    let f = F::new(2);
    // Ignored files cannot be faithfully preserved: no candidate, no success, claims kept.
    f.run(0, "edit write:src/feature.rs write:src/debug.log");
    let reasons = f.recovery(101);
    assert!(reasons.iter().any(|r| r.contains("ignored")), "{reasons:?}");
    assert_eq!(f.result(101), None);
    assert!(!f.claims_of(101).is_empty());
    assert!(
        f.record(101).path.join("src/debug.log").exists(),
        "user work is retained, never cleaned"
    );
    // A worktree edited after sealing no longer reproduces the candidate: refused.
    f.run(1, "edit write:src/feature.rs");
    assert_eq!(f.sealed(102).outcome, AttemptOutcome::Succeeded);
    fs::write(f.record(102).path.join("src/later.rs"), b"late\n").unwrap();
    let err = f.validate(&f.plan, f.report(102, &ALL_PASS)).unwrap_err();
    assert!(err.contains("does not reproduce"), "{err}");
    assert!(f.read(|v| v.validation_receipts(OpaqueId(102))).is_empty());
    assert!(f.record(102).path.join("src/later.rs").exists());
}

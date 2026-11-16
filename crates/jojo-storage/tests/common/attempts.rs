//! Shared T11/T12 attempt fixture: one temporary Git repository, approved PRD/ADR/plan,
//! one queued step per provider and reserved attempts (101..) with bound owned worktrees,
//! run through the real P1-09 runner on the workspace's `jojo-process-fixture` only.
use super::{TempDir, request, start};
use jojo_domain::attempt::AttemptBindings;
use jojo_domain::dependency::*;
use jojo_domain::identity::*;
use jojo_domain::provider::*;
use jojo_domain::recovery::RecoveryChoice;
use jojo_domain::scheduler::*;
use jojo_domain::stream::{CLAUDE_STREAM_JSON, CODEX_EXEC_JSON};
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
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::{fs, os::unix::fs::PermissionsExt};

pub const FIXTURE: &str = env!("CARGO_BIN_EXE_jojo-process-fixture");
pub const BUDGETS: StopBudgets = StopBudgets {
    interrupt: Duration::from_secs(3),
    terminate: Duration::from_secs(3),
    kill: Duration::from_secs(5),
    drain: Duration::from_millis(500),
};
pub const TESTS: [&str; 2] = ["unit::check", "unit::lib"];

pub fn ps() -> PathBuf {
    std::env::var_os("JOJO_TEST_PS")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/bin/ps".into())
}

pub fn os() -> OsProcessInspector {
    OsProcessInspector { ps: ps() }
}

/// Scripted inspector: the same durable records yield whatever the OS reports.
pub struct Scripted(pub IdentityCheck);
impl ProcessInspector for Scripted {
    fn inspect(&self, recorded: &[ProcessIdentity]) -> Vec<(String, IdentityCheck)> {
        recorded
            .iter()
            .map(|p| (format!("pid:{}", p.pid), self.0.clone()))
            .collect()
    }
}

pub fn caps(history: Support, cwd_change: Support) -> Capabilities {
    Capabilities {
        evidence: "fixture".into(),
        structured_events: Support::Supported,
        explicit_sessions: history,
        exact_continuation: history,
        incremental_input: Support::Unsupported,
        question_ids: Support::Unsupported,
        permission_response: Support::Unsupported,
        turn_interrupt: Support::Supported,
        process_tree: Support::Unknown,
        continuation_cwd_change: cwd_change,
        auth_status: Support::Unknown,
        workspace_policy: Support::Unknown,
    }
}

pub fn human(actor: &str) -> HumanOrigin {
    HumanOrigin::FakeHumanFixture {
        actor: actor.into(),
    }
}

pub struct RealClock(pub Instant);
impl Clock for RealClock {
    fn now(&self) -> ClockReading {
        ClockReading {
            unix_millis: 1_700_000_000_000,
            monotonic_nanos: self.0.elapsed().as_nanos() as u64,
            epoch: ClockEpochId(OpaqueId(9)),
        }
    }
}

pub static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub fn git(path: &Path, args: &[&str]) -> String {
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
            "user.name=T11",
            "-c",
            "user.email=t11@example.invalid",
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

pub fn validation_plan(n: usize) -> ValidationPlan {
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

/// One repository and `n` reserved attempts (101..) with bound worktrees, one step each.
pub struct F {
    pub providers: Vec<ProviderKind>,
    _serial: std::sync::MutexGuard<'static, ()>,
    pub dir: TempDir,
    pub engine: Option<Engine>,
    pub store: BlobStore,
    pub next: AtomicU64,
    pub namespace: AtomicU64,
    pub plan: String,
    pub repo: LocalRepository,
    pub root: WorktreeRoot,
}

impl F {
    pub fn new(n: usize) -> Self {
        Self::with_providers(&vec![ProviderKind::ClaudeCode; n])
    }

    /// One step and attempt (101..) per provider, in order.
    pub fn with_providers(providers: &[ProviderKind]) -> Self {
        let n = providers.len();
        let serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let dir = TempDir::new("t11");
        let rp = dir.path().join("repo");
        for d in ["src", "tests"] {
            fs::create_dir_all(rp.join(d)).unwrap();
        }
        git(&rp, &["init", "--template=", "--initial-branch=main"]);
        for (p, body) in [
            ("src/lib.rs", "pub fn f() {}\n"),
            ("tests/check.rs", "fn check() {}\n"),
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
        let f = Self {
            providers: providers.to_vec(),
            _serial: serial,
            dir,
            engine: Some(e),
            store,
            next: AtomicU64::new(1),
            namespace: AtomicU64::new(1000),
            plan: validation_plan(n).canonical(),
            repo,
            root,
        };
        let ids: Vec<u128> = (0..n as u128).map(|i| 101 + i).collect();
        for a in &ids {
            f.provision(*a, &base, None);
        }
        let config = f.queue(&base, n as u32);
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
            assert_eq!(f.record(a).state, "BOUND");
        }
        f
    }

    pub fn provision(&self, a: u128, base: &str, predecessor: Option<(u128, &CandidateSnapshot)>) {
        let (r, rt, b) = (self.repo.clone(), self.root.clone(), base.to_owned());
        let p = predecessor.map(|(p, _)| OpaqueId(p));
        self.cmd(move |tx| tx.request_worktree(OpaqueId(a), &r, &rt, &b, p))
            .unwrap();
        let proof =
            materialize_worktree(self.e(), OpaqueId(a), predecessor.map(|(_, c)| c)).unwrap();
        self.cmd(move |tx| {
            tx.seal_worktree(proof)?;
            Ok("sealed".into())
        })
        .unwrap();
    }

    pub fn e(&self) -> &Engine {
        self.engine.as_ref().unwrap()
    }

    /// The helper dies: no clean shutdown is recorded, then a new helper starts.
    pub fn restart(&mut self) {
        drop(self.engine.take());
        self.engine = Some(start(&self.dir.db()));
        self.store = BlobStore::for_engine(self.e()).unwrap();
    }

    pub fn ids(&self) -> SequenceIds {
        SequenceIds::new(self.namespace.fetch_add(1, Ordering::SeqCst), 1)
    }

    pub fn record(&self, a: u128) -> WorktreeRecord {
        self.read(|v| v.worktree(OpaqueId(a))).unwrap()
    }

    pub fn read<T>(&self, f: impl FnOnce(&ReadView<'_>) -> Result<T, StorageError>) -> T {
        self.e().reader().unwrap().read(f).unwrap()
    }

    pub fn cmd<FN>(&self, f: FN) -> Result<Receipt, CommandError>
    where
        FN: FnOnce(&mut Tx<'_>) -> Result<String, TxError> + Send + 'static,
    {
        let id = self.next.fetch_add(1, Ordering::SeqCst) as u128;
        self.e().execute(request(self.e(), id, "T11", "fixture"), f)
    }

    pub fn decide(&self, a: ArtifactId, action: DecisionAction) {
        let b = self.read(|v| v.review_bundle(a)).unwrap();
        let id = self.next.fetch_add(1, Ordering::SeqCst) as u128;
        self.e()
            .human_decide(CommandId(OpaqueId(id)), human("T11 human"), b, action)
            .unwrap();
    }

    pub fn queue(&self, base: &str, n: u32) -> u64 {
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
                &Provenance::new(Author::Human, "T11"),
            )?;
            Ok("prd".into())
        })
        .unwrap();
        self.decide(prd, DecisionAction::Approve);
        self.cmd(move |tx| {
            tx.put_review_document(adr, None, prd, None, "T11 ADR", "fixture reviewed")?;
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
                repository: self.repo.id(),
                revision: SourceRevision::Commit(base.to_owned()),
            };
            p.resources.push(StepResources {
                step: node(i as u128 + 1),
                repository: self.repo.id(),
                provider: self.providers[i],
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
            tx.put_execution_plan(pid, None, prd, adr, &copy, "T11 reviewed")?;
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

    pub fn barriers(&self) -> PathBuf {
        let b = self.dir.path().join("barriers");
        let _ = fs::create_dir(&b);
        b
    }

    /// LaunchSpec of attempt `a` for step `step` in the current epoch.
    pub fn spec(&self, a: u128, step: u128, scenario: &str) -> LaunchSpec {
        let provider = self.providers[step as usize - 1];
        let record = self.record(a);
        let vector = self.read(|v| v.attempt_source_vector(OpaqueId(a))).unwrap();
        let approvals = [901, 902, 903].map(|subject| {
            OpaqueId(u128::from(
                self.read(|v| v.applicable_decision(ArtifactId(OpaqueId(subject))))
                    .unwrap()
                    .0,
            ))
        });
        LaunchSpec {
            attempt: AttemptId(OpaqueId(a)),
            bindings: AttemptBindings {
                feature_id: FeatureId(OpaqueId(1)),
                plan_id: PlanId(OpaqueId(903)),
                plan_revision: Revision(1),
                step_id: StepId(OpaqueId(step)),
                step_revision: Revision(1),
                repository_id: record.repository,
                worktree_id: WorktreeId(OpaqueId(a)),
            },
            engine_epoch: OpaqueId(u128::from(self.e().epoch().0)),
            dispatch_generation: 1,
            fingerprint: ProviderFingerprint {
                provider,
                executable: FIXTURE.into(),
                version: "fixture-v1".into(),
                schema: PROVIDER_SCHEMA,
                parser: match provider {
                    ProviderKind::ClaudeCode => CLAUDE_STREAM_JSON,
                    ProviderKind::Codex => CODEX_EXEC_JSON,
                }
                .into(),
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
                filesystem_roots: vec![record.path.clone()],
                network: false,
                hooks: false,
                background: false,
            },
            cwd: record.path,
            environment_allowlist: vec!["LANG".into()],
            config_references: vec![],
            timeout_millis: 60_000,
            max_output_bytes: 1024 * 1024,
            max_frame_bytes: 64 * 1024,
            raw_output_dir: record.output,
            approval_ids: approvals,
            required_outputs: vec![CANDIDATE_OUTPUT.into()],
            validation_plan: self.plan.clone(),
        }
    }

    pub fn launch(&self, spec: &LaunchSpec) -> ProcessLaunch {
        ProcessLaunch::from_spec(
            spec,
            vec![self.barriers().display().to_string()],
            vec![("LANG".into(), "C".into())],
        )
        .unwrap()
    }

    pub fn run_with(&self, a: u128, scenario: &str, hooks: LaunchHooks) -> LaunchReport {
        self.run_spec(&self.spec(a, a - 100, scenario), hooks, BUDGETS)
    }

    /// Runs one (possibly adjusted) spec through the real P1-09 runner.
    pub fn run_spec(
        &self,
        spec: &LaunchSpec,
        mut hooks: LaunchHooks,
        budgets: StopBudgets,
    ) -> LaunchReport {
        hooks.inspector.get_or_insert_with(ps);
        let ids = self.ids();
        let clock = RealClock(Instant::now());
        let ctx = LaunchContext {
            engine: self.e(),
            ids: &ids,
            clock: &clock,
            poll: Duration::from_millis(10),
        };
        let caps = caps(Support::Supported, Support::Supported);
        let launch = self.launch(spec);
        run_launch(&ctx, spec, &launch, &StopPlan::new(&caps, budgets), hooks).unwrap()
    }

    pub fn run(&self, a: u128, scenario: &str) {
        let r = self.run_with(a, scenario, LaunchHooks::default());
        assert!(
            matches!(r.outcome, LaunchOutcome::TreeAbsent { .. }),
            "{:?}",
            r.outcome
        );
    }

    /// Runs `scenario`; once the session is identified, `during` commits from this thread.
    pub fn run_and<FN>(&self, a: u128, scenario: &str, during: FN) -> LaunchReport
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
            let h = s.spawn(|| self.run_with(a, scenario, hooks));
            rx.recv_timeout(Duration::from_secs(20)).unwrap();
            self.cmd(during).unwrap();
            h.join().unwrap()
        })
    }

    /// Issues the launch and spawns it outside the runner, as a helper would before dying.
    pub fn spawn_registered(&self, a: u128, scenario: &str) -> Supervised {
        let spec = self.spec(a, a - 100, scenario);
        let issued = spec.clone();
        self.cmd(move |tx| Ok(tx.issue_process_launch(&issued)?.to_string()))
            .unwrap();
        let sup =
            Supervised::spawn_with_inspector(&self.launch(&spec), SpoolFaults::default(), &ps())
                .unwrap();
        let root = sup.root().clone();
        self.cmd(move |tx| Ok(tx.register_process_root(OpaqueId(a), &root)?.to_string()))
            .unwrap();
        sup
    }

    pub fn ctx<'a>(&'a self, ids: &'a SequenceIds) -> SettlementContext<'a> {
        SettlementContext {
            engine: self.e(),
            ids,
            store: &self.store,
        }
    }

    pub fn inspect(
        &self,
        inspector: &dyn ProcessInspector,
        caps: Option<Capabilities>,
    ) -> RecoveryInspection {
        inspect_recovery(self.e(), inspector, &|p| {
            (p == "ClaudeCode").then(|| caps.clone()).flatten()
        })
        .unwrap()
    }

    pub fn resolve(
        &self,
        ins: &RecoveryInspection,
        a: u128,
        choice: RecoveryChoice,
    ) -> Result<String, String> {
        let ins = ins.clone();
        match self
            .cmd(move |tx| tx.resolve_recovery(&ins, OpaqueId(a), choice, human("T11 reviewer")))
        {
            Ok(r) => Ok(r.response),
            Err(e) => Err(format!("{e:?}")),
        }
    }

    pub fn recover(
        &self,
        ins: &RecoveryInspection,
        a: u128,
        choice: RecoveryChoice,
    ) -> AttemptResult {
        let ids = self.ids();
        match recover_attempt(
            &self.ctx(&ids),
            ins,
            OpaqueId(a),
            choice,
            human("T11 reviewer"),
        )
        .unwrap()
        {
            RecoveryOutcome::Settled(Settlement::Sealed(r)) => *r,
            other => panic!("expected a sealed result, got {other:?}"),
        }
    }

    pub fn start_recovery(&self, ins: &RecoveryInspection) -> Result<Receipt, CommandError> {
        let ins = ins.clone();
        self.cmd(move |tx| {
            tx.start_after_recovery(&ins, human("T11 operator"))?;
            Ok("started".into())
        })
    }

    pub fn claims_of(&self, a: u128) -> Vec<(String, ClaimState)> {
        let holder = AggregateKey::new("scheduler_attempt", OpaqueId(a));
        self.read(|v| v.live_claims())
            .into_iter()
            .filter(|c| c.holder == holder)
            .map(|c| (c.resource_key, c.state))
            .collect()
    }

    pub fn result(&self, a: u128) -> Option<AttemptResult> {
        self.read(|v| v.attempt_result(OpaqueId(a)))
    }

    pub fn admission(&self) -> Admission {
        self.e()
            .reader()
            .unwrap()
            .snapshot()
            .unwrap()
            .session
            .admission
    }
}

/// Bounded failure watchdog; not a timing assumption.
pub fn until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < deadline, "condition not reached");
        std::thread::sleep(Duration::from_millis(10));
    }
}

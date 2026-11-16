//! Shared T15/T16 scenario: two (or one) temporary repositories, reviewed PRD/ADR/plan
//! through the helper service, fake Claude/Codex developers on the workspace fixture,
//! a separate validator fixture and close/reopen. Test-only; no provider or network.
use super::attempts::*;
use super::{TempDir, reading, request};
use jojo_domain::attempt::AttemptOutcome;
use jojo_domain::dependency::*;
use jojo_domain::identity::*;
use jojo_domain::ipc::*;
use jojo_domain::json::Json;
use jojo_domain::provider::*;
use jojo_domain::scheduler::*;
use jojo_domain::validation::*;
use jojo_helper::service::*;
use jojo_storage::*;
use jojo_test_support::{
    COMPLETE_PRD_JSON, FakeClock, SequenceIds,
    dependency::{edge, node, plan},
};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};
use std::{fs, os::unix::fs::PermissionsExt};

pub use ProviderKind::{ClaudeCode, Codex};

pub const PRD: ArtifactId = ArtifactId(OpaqueId(901));
pub const ADR: ArtifactId = ArtifactId(OpaqueId(902));
pub const PLAN: ArtifactId = ArtifactId(OpaqueId(903));
pub const R1: Revision = Revision(1);
pub const R2: Revision = Revision(2);
pub const ALL_PASS: [(&str, TestStatus); 2] = [
    ("unit::check", TestStatus::Passed),
    ("unit::lib", TestStatus::Passed),
];

/// Hooks that report once the provider session is identified (the process is RUNNING).
pub fn on_session(tx: mpsc::Sender<u128>, a: u128) -> LaunchHooks {
    LaunchHooks {
        on_event: Some(Box::new(move |p| {
            if matches!(p, EventPayload::SessionIdentified(_)) {
                let _ = tx.send(a);
            }
        })),
        ..LaunchHooks::default()
    }
}

/// An engine with injectable writer fail points and a controllable audit clock.
pub fn engine(db: &Path, faults: &FaultPlan, clock: &Arc<FakeClock>) -> Engine {
    let mut config = EngineConfig::new(db, clock.clone());
    config.faults = faults.clone();
    Engine::start(config).expect("engine starts")
}

/// A distinct temporary repository per index: different content, so different bases.
pub fn repository(path: &Path, i: usize) -> (LocalRepository, String) {
    for d in ["src", "tests"] {
        fs::create_dir_all(path.join(d)).unwrap();
    }
    git(path, &["init", "--template=", "--initial-branch=main"]);
    fs::write(path.join("src/lib.rs"), format!("pub fn repo_{i}() {{}}\n")).unwrap();
    fs::write(path.join("tests/check.rs"), "fn check() {}\n").unwrap();
    git(path, &["add", "--all"]);
    git(path, &["commit", "-m", "fixture"]);
    let base = git(path, &["rev-parse", "HEAD"]);
    (LocalRepository::inspect(path).unwrap(), base)
}

/// The scenario: steps are (repository index, fake developer); attempts are chosen per test.
pub struct S {
    pub _serial: std::sync::MutexGuard<'static, ()>,
    pub dir: TempDir,
    pub ui: Option<Service>,
    pub store: BlobStore,
    pub next: AtomicU64,
    pub requests: u128,
    pub namespace: AtomicU64,
    pub repos: Vec<(LocalRepository, String)>,
    pub root: WorktreeRoot,
    pub steps: Vec<(usize, ProviderKind)>,
    pub validation: String,
    pub config: u64,
    /// Survive reopen: one-shot writer fail points and the engine's audit clock.
    pub faults: FaultPlan,
    pub clock: Arc<FakeClock>,
}

impl S {
    pub fn new(repos: usize, steps: &[(usize, ProviderKind)]) -> Self {
        let serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let dir = TempDir::new("t15");
        let repos = (0..repos)
            .map(|i| repository(&dir.path().join(format!("repository {i}")), i))
            .collect();
        let owned = dir.path().join("owned");
        fs::create_dir(&owned).unwrap();
        fs::set_permissions(&owned, fs::Permissions::from_mode(0o700)).unwrap();
        let root = WorktreeRoot::inspect(&owned).unwrap();
        let (faults, clock) = (FaultPlan::default(), super::clock());
        let engine = Arc::new(engine(&dir.db(), &faults, &clock));
        let store = BlobStore::for_engine(&engine).unwrap();
        let mut s = Self {
            faults,
            clock,
            _serial: serial,
            ui: Some(Service::new(engine).unwrap()),
            dir,
            store,
            next: AtomicU64::new(1),
            requests: 0,
            namespace: AtomicU64::new(1000),
            repos,
            root,
            steps: steps.to_vec(),
            validation: validation_plan(steps.len()).canonical(),
            config: 1,
        };
        assert_eq!(s.request("Hello", object([]), None).kind, "Response");
        s
    }

    pub fn e(&self) -> &Engine {
        &self.ui.as_ref().unwrap().engine
    }

    pub fn ui(&mut self) -> &mut Service {
        self.ui.as_mut().unwrap()
    }

    /// The helper exits after its shutdown receipt; a new helper opens the same data root.
    pub fn reopen(&mut self) {
        drop(self.ui.take());
        let engine = Arc::new(engine(&self.dir.db(), &self.faults, &self.clock));
        self.store = BlobStore::for_engine(&engine).unwrap();
        self.ui = Some(Service::new(engine).unwrap());
        assert_eq!(self.request("Hello", object([]), None).kind, "Response");
    }

    /// The helper dies: no close or shutdown receipt is recorded, then a new helper starts.
    pub fn crash(&mut self) {
        self.reopen();
    }

    pub fn read<T>(&self, f: impl FnOnce(&ReadView<'_>) -> Result<T, StorageError>) -> T {
        self.e().reader().unwrap().read(f).unwrap()
    }

    pub fn cmd<FN>(&self, f: FN) -> Result<Receipt, CommandError>
    where
        FN: FnOnce(&mut Tx<'_>) -> Result<String, TxError> + Send + 'static,
    {
        let id = self.next.fetch_add(1, Ordering::SeqCst) as u128;
        self.e().execute(request(self.e(), id, "T15", "fixture"), f)
    }

    pub fn ids(&self) -> SequenceIds {
        SequenceIds::new(self.namespace.fetch_add(1, Ordering::SeqCst), 1)
    }

    pub fn ctx<'a>(&'a self, ids: &'a SequenceIds) -> SettlementContext<'a> {
        SettlementContext {
            engine: self.e(),
            ids,
            store: &self.store,
        }
    }

    /// One protocol request on the helper control channel, as the native shell sends it.
    pub fn request(&mut self, kind: &str, payload: Json, revision: Option<u64>) -> Envelope {
        self.requests += 1;
        let e = Envelope {
            major: 1,
            minor: 0,
            session: OpaqueId(5),
            epoch: self.e().epoch().0,
            request: OpaqueId(100_000 + self.requests),
            revision,
            kind: kind.into(),
            payload,
        };
        self.ui().request(&e)
    }

    /// Displays the exact review bundle, then decides on precisely those displayed bytes.
    pub fn ui_decide(&mut self, subject: ArtifactId, action: &str, tamper: bool) -> Envelope {
        let q = self.request(
            "ReviewBundle",
            object([("subject", identity(subject.0))]),
            None,
        );
        assert_eq!(q.kind, "Response", "{q:?}");
        let revision = uint(get(&q.payload, "revision").unwrap()).unwrap();
        let Json::Object(mut fields) = q.payload else {
            panic!("review bundle payload")
        };
        fields.retain(|(k, _)| k != "revision");
        if tamper {
            fields
                .iter_mut()
                .find(|(k, _)| k == "displayed_hash")
                .unwrap()
                .1 = text("bytes the reviewer never saw");
        }
        fields.push(("action".into(), text(action)));
        self.request("HumanDecision", Json::Object(fields), Some(revision))
    }

    pub fn approve(&mut self, subject: ArtifactId) {
        let r = self.ui_decide(subject, "Approve", false);
        assert_eq!(r.kind, "Response", "{r:?}");
    }

    /// A synthetic human decision from another thread while developers run.
    pub fn decide(&self, subject: ArtifactId, action: DecisionAction) {
        let b = self.read(|v| v.review_bundle(subject)).unwrap();
        let id = self.next.fetch_add(1, Ordering::SeqCst) as u128;
        self.e()
            .human_decide(CommandId(OpaqueId(id)), human("T15 human"), b, action)
            .unwrap();
    }

    pub fn author_prd(&self) {
        let p = PreparedPrd::import(COMPLETE_PRD_JSON.as_bytes()).unwrap();
        self.cmd(move |tx| {
            tx.create_artifact(
                PRD,
                FeatureId(OpaqueId(1)),
                &p,
                &Provenance::new(Author::Human, "T15 complete fixture PRD and use cases"),
            )?;
            Ok("prd".into())
        })
        .unwrap();
    }

    pub fn author_adr(&self) -> Result<Receipt, CommandError> {
        self.cmd(|tx| {
            tx.put_review_document(
                ADR,
                None,
                PRD,
                None,
                "T15 ADR: helper-owned SQLite, owned worktrees, fake providers",
                "fixture reviewed",
            )?;
            Ok("adr".into())
        })
    }

    pub fn base(&self, step: u128) -> Source {
        let (repo, base) = &self.repos[self.steps[step as usize - 1].0];
        Source {
            repository: repo.id(),
            revision: SourceRevision::Commit(base.clone()),
        }
    }

    /// The reviewed schema-2 plan: one input and resource envelope per step.
    pub fn feature_plan(&self, edges: Vec<Edge>) -> ExecutionPlan {
        let n = self.steps.len() as u128;
        let mut p = plan(&(1..=n).collect::<Vec<_>>(), edges);
        p.schema = 2;
        p.required_outputs = vec![CANDIDATE_OUTPUT.into()];
        p.validation_digest = sha256_hex(&[self.validation.as_bytes()]);
        for (i, (repo, provider)) in self.steps.iter().enumerate() {
            p.inputs[i].base_sources = vec![self.base(i as u128 + 1)];
            p.resources.push(StepResources {
                step: node(i as u128 + 1),
                repository: self.repos[*repo].0.id(),
                provider: *provider,
                priority: 0,
                additional: vec![],
                repository_mode: ResourceMode::Shared,
            });
        }
        for a in [PRD, ADR] {
            let approval = self.read(|v| v.current_artifact_approval(a)).unwrap();
            p.artifacts.nodes.push(NodeRef {
                id: a.0,
                revision: approval.revision,
            });
            p.approvals.push(approval);
        }
        p
    }

    pub fn put_plan(&self, p: &ExecutionPlan, expected: Option<Revision>) {
        let copy = p.clone();
        self.cmd(move |tx| {
            tx.put_execution_plan(PLAN, expected, PRD, ADR, &copy, "T15 reviewed plan")?;
            Ok("plan".into())
        })
        .unwrap();
    }

    /// PRD, ADR and plan authored and approved through the UI path.
    pub fn approve_all(&mut self, edges: Vec<Edge>) {
        self.author_prd();
        self.approve(PRD);
        self.author_adr().unwrap();
        self.approve(ADR);
        let p = self.feature_plan(edges);
        self.put_plan(&p, None);
        self.approve(PLAN);
    }

    pub fn queue(
        &self,
        step: u128,
        revision: Revision,
        sources: Vec<Source>,
    ) -> Result<(), String> {
        self.cmd(move |tx| {
            tx.queue_step(
                PRD,
                ADR,
                PLAN,
                revision,
                node(step),
                &SourceVector {
                    sources,
                    environment: Some("test-env".into()),
                },
            )?;
            Ok("queued".into())
        })
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
    }

    pub fn queue_base(&self, step: u128) {
        self.queue(step, R1, vec![self.base(step)]).unwrap();
    }

    pub fn queue_base_err(&self, step: u128) -> String {
        self.queue(step, R1, vec![self.base(step)]).unwrap_err()
    }

    pub fn configure(&mut self, global: u32, provider: u32, repository: u32) {
        let expected = self.config;
        let r = self
            .cmd(move |tx| {
                let revision = tx.configure_resources(
                    expected,
                    repository,
                    BTreeMap::from([
                        ("global".into(), global),
                        ("provider:claude".into(), provider),
                        ("provider:codex".into(), provider),
                    ]),
                )?;
                tx.set_admission(Admission::Open)?;
                Ok(revision.to_string())
            })
            .unwrap();
        self.config = r.response.parse().unwrap();
    }

    pub fn provision(
        &self,
        a: u128,
        repo: usize,
        base: &str,
        predecessor: Option<(u128, &CandidateSnapshot)>,
    ) {
        let (r, rt, b) = (
            self.repos[repo].0.clone(),
            self.root.clone(),
            base.to_owned(),
        );
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

    /// One admission scan over freshly inspected pre-provisioned worktrees: (step, attempt).
    pub fn schedule(&self, attempts: &[u128]) -> Vec<(u128, u128)> {
        let proofs = attempts
            .iter()
            .map(|a| inspect_worktree(self.e(), OpaqueId(*a)).unwrap())
            .collect();
        let config = self.config;
        let r = self
            .cmd(move |tx| {
                Ok(tx
                    .schedule_owned_worktrees(config, proofs)?
                    .iter()
                    .filter_map(|d| d.attempt.map(|a| format!("{}={}", d.step.id.0, a.0)))
                    .collect::<Vec<_>>()
                    .join(","))
            })
            .unwrap();
        r.response
            .split(',')
            .filter(|p| !p.is_empty())
            .map(|p| {
                let (s, a) = p.split_once('=').unwrap();
                (s.parse().unwrap(), a.parse().unwrap())
            })
            .collect()
    }

    pub fn record(&self, a: u128) -> WorktreeRecord {
        self.read(|v| v.worktree(OpaqueId(a))).unwrap()
    }

    pub fn barriers(&self) -> std::path::PathBuf {
        let b = self.dir.path().join("barriers");
        let _ = fs::create_dir(&b);
        b
    }

    /// The fake developer for a step: RUNNING, wait for `release`, edit, report success.
    pub fn prompt(&self, step: u128) -> String {
        let who = match self.steps[step as usize - 1].1 {
            ClaudeCode => "claude",
            Codex => "codex",
        };
        format!("{who}-develop write:src/step{step}.rs")
    }

    pub fn spec(&self, a: u128, step: u128, plan_revision: Revision) -> LaunchSpec {
        let provider = self.steps[step as usize - 1].1;
        let scenario = self.prompt(step);
        let record = self.record(a);
        let vector = self.read(|v| v.attempt_source_vector(OpaqueId(a))).unwrap();
        let approvals = [PRD, ADR, PLAN].map(|subject| {
            OpaqueId(u128::from(
                self.read(|v| v.applicable_decision(subject)).unwrap().0,
            ))
        });
        LaunchSpec {
            attempt: AttemptId(OpaqueId(a)),
            bindings: jojo_domain::attempt::AttemptBindings {
                feature_id: FeatureId(OpaqueId(1)),
                plan_id: PlanId(PLAN.0),
                plan_revision,
                step_id: StepId(OpaqueId(step)),
                step_revision: R1,
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
                    ClaudeCode => jojo_domain::stream::CLAUDE_STREAM_JSON,
                    Codex => jojo_domain::stream::CODEX_EXEC_JSON,
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
            prompt: scenario.into_bytes(),
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
            validation_plan: self.validation.clone(),
        }
    }

    /// Issues and spawns a developer outside the runner and registers its root, as a
    /// helper does just before dying. The caller owns the returned supervision handle.
    pub fn spawn_registered(&self, a: u128, step: u128, revision: Revision) -> Supervised {
        let spec = self.spec(a, step, revision);
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

    pub fn launch(&self, spec: &LaunchSpec) -> ProcessLaunch {
        ProcessLaunch::from_spec(
            spec,
            vec![self.barriers().display().to_string()],
            vec![("LANG".into(), "C".into())],
        )
        .unwrap()
    }

    /// The real P1-09 runner for one spec.
    pub fn run_spec(&self, spec: &LaunchSpec, mut hooks: LaunchHooks) -> LaunchReport {
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
        run_launch(
            &ctx,
            spec,
            &self.launch(spec),
            &StopPlan::new(&caps, BUDGETS),
            hooks,
        )
        .unwrap()
    }

    /// Starts every job, waits until all of them are RUNNING, then runs `during`; the shared
    /// `release` barrier is created only if `during` returns true. Barriers, not sleeps.
    pub fn run_parallel(
        &self,
        jobs: &[(u128, u128, Revision)],
        during: impl FnOnce() -> bool,
    ) -> Vec<LaunchReport> {
        let specs: Vec<_> = jobs.iter().map(|(a, s, r)| self.spec(*a, *s, *r)).collect();
        let (tx, rx) = mpsc::channel();
        // Runner-reported order across all jobs: (attempt, true = RUNNING, false = finished).
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let reports = std::thread::scope(|scope| {
            let handles: Vec<_> = specs
                .iter()
                .map(|spec| {
                    let (a, tx, order) = (spec.attempt.0.0, tx.clone(), order.clone());
                    let hooks = LaunchHooks {
                        on_event: Some(Box::new(move |p| match p {
                            EventPayload::SessionIdentified(_) => {
                                order.lock().unwrap().push((a, true));
                                let _ = tx.send(a);
                            }
                            EventPayload::ProviderTerminal { .. }
                            | EventPayload::ProcessExited { .. } => {
                                order.lock().unwrap().push((a, false))
                            }
                            _ => {}
                        })),
                        ..LaunchHooks::default()
                    };
                    scope.spawn(move || self.run_spec(spec, hooks))
                })
                .collect();
            let mut running: Vec<u128> = jobs
                .iter()
                .map(|_| rx.recv_timeout(Duration::from_secs(20)).unwrap())
                .collect();
            running.sort();
            let mut expected: Vec<u128> = jobs.iter().map(|j| j.0).collect();
            expected.sort();
            assert_eq!(running, expected, "every developer is RUNNING first");
            // A failed assertion still releases the developers so the test fails promptly.
            let release = std::panic::catch_unwind(std::panic::AssertUnwindSafe(during));
            if release.as_ref().map_or(true, |r| *r) {
                fs::write(self.barriers().join("release"), b"").unwrap();
            }
            let reports = handles.into_iter().map(|h| h.join().unwrap()).collect();
            if let Err(panic) = release {
                std::panic::resume_unwind(panic);
            }
            reports
        });
        let _ = fs::remove_file(self.barriers().join("release"));
        // Every developer reached RUNNING before any of them finished.
        let order = order.lock().unwrap().clone();
        let last_running = order.iter().rposition(|(_, running)| *running).unwrap();
        let first_finished = order.iter().position(|(_, running)| !*running);
        assert!(
            first_finished.is_none_or(|f| f > last_running),
            "overlap not proven: {order:?}"
        );
        reports
    }

    pub fn settle(&self, a: u128) -> AttemptResult {
        let ids = self.ids();
        match settle_attempt(&self.ctx(&ids), OpaqueId(a), BlobFaults::default()).unwrap() {
            Settlement::Sealed(r) => *r,
            other => panic!("expected a sealed result, got {other:?}"),
        }
    }

    pub fn result(&self, a: u128) -> Option<AttemptResult> {
        self.read(|v| v.attempt_result(OpaqueId(a)))
    }

    /// The separate validator fixture: its own identity, the approved plan, all tests.
    pub fn validate(&self, a: u128) -> Result<ValidationReceipt, String> {
        let candidate = self.result(a).unwrap().candidate.unwrap();
        let report = ValidatorReport {
            origin: ValidatorOrigin::IndependentFixture {
                name: "t15-validator".into(),
            },
            attempt: OpaqueId(a),
            candidate,
            plan_digest: sha256_hex(&[self.validation.as_bytes()]),
            results: ALL_PASS.iter().map(|(t, s)| (t.to_string(), *s)).collect(),
            raw: format!("{ALL_PASS:?}").into_bytes(),
        };
        let ids = self.ids();
        validate_candidate(&self.ctx(&ids), self.validation.as_bytes(), report)
    }

    pub fn accept(&self, a: u128, receipt: EventSeq) -> Result<Receipt, CommandError> {
        self.cmd(move |tx| {
            tx.accept_candidate(OpaqueId(a), receipt, human("T15 reviewer"))?;
            Ok("accepted".into())
        })
    }

    pub fn claims_of(&self, a: u128) -> Vec<String> {
        let holder = AggregateKey::new("scheduler_attempt", OpaqueId(a));
        self.read(|v| v.live_claims())
            .into_iter()
            .filter(|c| c.holder == holder)
            .map(|c| c.resource_key)
            .collect()
    }

    pub fn kinds(&self, a: u128) -> Vec<String> {
        self.read(|v| v.process_observations(OpaqueId(a)))
            .into_iter()
            .map(|(k, _)| k)
            .collect()
    }

    pub fn admission(&self) -> Admission {
        self.read(|v| v.session()).admission
    }

    /// The launch was issued and its root registered, and no exit/absence checkpoint or
    /// result exists. `run_parallel` separately proves runner-reported ordering.
    pub fn live(&self, a: u128) {
        assert!(self.read(|v| v.process_launch_issued(OpaqueId(a))), "{a}");
        assert!(
            !self.read(|v| v.process_records(OpaqueId(a))).is_empty(),
            "{a}: no registered root"
        );
        let k = self.kinds(a);
        assert!(
            !k.iter().any(|k| k == "ROOT_EXITED" || k == "TREE_ABSENT"),
            "{a}: {k:?}"
        );
        assert_eq!(self.result(a), None);
    }
}

pub fn succeeded(r: &AttemptResult) {
    assert_eq!(
        (r.outcome, r.reason.as_str(), r.stop_cause.as_deref()),
        (AttemptOutcome::Succeeded, "terminal_success", None),
        "{r:?}"
    );
}

pub fn released(reports: &[LaunchReport]) {
    for r in reports {
        assert_eq!(
            r.outcome,
            LaunchOutcome::TreeAbsent {
                stop_requested: false,
                forced: false
            }
        );
        assert!(r.signals.is_empty());
    }
}

/// Requirements a dependent step names for one exact, independently validated output.
pub fn output_requirements(sel: &OutputSelection, validation: &str) -> [EvidenceRequirement; 2] {
    let binding = |output: Option<OutputSelection>, plan: Option<String>, tests: Vec<String>| {
        EvidenceBinding {
            sources: sel.sources.clone(),
            environment: Some("test-env".into()),
            artifact_digest: None,
            contract: None,
            validation_plan: plan,
            test_inventory: tests,
            output,
        }
    };
    let requirement = |kind, id: &str, binding| EvidenceRequirement {
        kind,
        evidence_id: id.into(),
        observation_revision: R1,
        binding,
        max_age_ms: 3_600_000,
    };
    [
        requirement(
            EvidenceKind::OutputAvailable,
            "t15-step1-output",
            binding(Some(sel.clone()), None, vec![]),
        ),
        requirement(
            EvidenceKind::ValidationPassed,
            "t15-step1-validation",
            binding(
                None,
                Some(sha256_hex(&[validation.as_bytes()])),
                TESTS.map(String::from).to_vec(),
            ),
        ),
    ]
}

/// An independent observation exactly matching `r`, observed on the engine's clock.
pub fn observation(r: &EvidenceRequirement, status: EvidenceStatus) -> Observation {
    let observed = reading(1_700_000_000_000);
    Observation {
        schema: DEPENDENCY_SCHEMA,
        id: r.evidence_id.clone(),
        revision: r.observation_revision,
        kind: r.kind,
        binding: r.binding.clone(),
        status,
        observer: "t15 independent observer".into(),
        observed,
        expires_at_unix_ms: observed.unix_millis + 3_600_000,
    }
}

pub fn evidence_edge(id: u128, from: u128, to: u128, r: EvidenceRequirement) -> Edge {
    edge(
        id,
        from,
        to,
        Predicate::Evidence {
            schema: DEPENDENCY_SCHEMA,
            requirement: r.into(),
        },
    )
}

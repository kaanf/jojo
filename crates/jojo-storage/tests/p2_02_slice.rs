//! P2-02: the slice driven by helper protocol commands only. Every state change in these
//! tests is a protocol 1.1 frame handled by the production `Service`; the tests read the
//! database only to assert. The "Codex" is a private copy of `jojo-codex-fixture` behind
//! the production `CodexAdapter`. The installed Codex CLI is never named, located or
//! executed; no credential, network or inference is involved.
mod common;
use common::attempts::{SERIAL, git, ps, until};
use common::{TempDir, start};
use jojo_domain::attempt::AttemptOutcome;
use jojo_domain::codex::CODEX_VERSION;
use jojo_domain::identity::*;
use jojo_domain::ipc::*;
use jojo_domain::json::Json;
use jojo_domain::provider::{AuthMode, ProviderKind};
use jojo_domain::validation::{ScopePath, StepValidation, ValidationPlan};
use jojo_helper::service::*;
use jojo_storage::*;
use jojo_test_support::{COMPLETE_PRD_JSON, dependency::node};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};

const CODEX_FIXTURE: &str = env!("CARGO_BIN_EXE_jojo-codex-fixture");
const FEATURE: u128 = 700;
const PRD: u128 = 701;
const ADR: u128 = 702;
const PLAN: u128 = 703;
const VALIDATION: u128 = 704;
const STEP: u128 = 1;
const DEVELOP: &str =
    "Keep the change inside src/.\nfixture: develop src/feature.rs\nNo other file changes.";

fn chmod(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn hex(id: u128) -> String {
    format!("{id:032x}")
}

/// A helper service over a temporary data root, one disposable root holding one sample
/// repository, and a private fixture executable with an empty selected Codex home.
struct H {
    _serial: std::sync::MutexGuard<'static, ()>,
    dir: TempDir,
    base: PathBuf,
    repository: PathBuf,
    commit: String,
    fixture: PathBuf,
    home: PathBuf,
    service: Option<Service>,
    next: u128,
}

type Hooks = Box<dyn FnMut() -> LaunchHooks + Send + Sync>;

impl H {
    fn new() -> Self {
        let serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let dir = TempDir::new("p2-02");
        let base = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir(base.join("disposable")).unwrap();
        chmod(&base.join("disposable"), 0o700);
        let repository = base.join("disposable/sample");
        let commit = sample_repository(&repository);
        fs::create_dir(base.join("bin")).unwrap();
        let fixture = base.join("bin/codex");
        fs::copy(CODEX_FIXTURE, &fixture).unwrap();
        chmod(&fixture, 0o755);
        let home = base.join("codex-home");
        fs::create_dir(&home).unwrap();
        chmod(&home, 0o700);
        let mut h = Self {
            _serial: serial,
            dir,
            base,
            repository,
            commit,
            fixture,
            home,
            service: None,
            next: 10,
        };
        h.open(Some(h.codex()), Some(ps()), None);
        h
    }

    fn codex(&self) -> CodexAdapterConfig {
        CodexAdapterConfig {
            executable: self.fixture.clone(),
            version: CODEX_VERSION.into(),
            sha256: sha256_hex(&[&fs::read(&self.fixture).unwrap()]),
            home: self.home.clone(),
            auth: AuthMode::NativeSubscription,
        }
    }

    fn slice(&self, codex: Option<CodexAdapterConfig>) -> SliceConfig {
        SliceConfig {
            disposable_root: self.base.join("disposable"),
            worktree_root: self.base.join("owned"),
            codex,
        }
    }

    /// A helper (re)starts on the same data root: a new engine epoch and a new session.
    fn open(
        &mut self,
        codex: Option<CodexAdapterConfig>,
        inspector: Option<PathBuf>,
        launch_hooks: Option<Hooks>,
    ) {
        drop(self.service.take());
        let engine = Arc::new(start(&self.dir.db()));
        let config = ServiceConfig {
            slice: Some(self.slice(codex)),
            inspector,
            launch_hooks,
        };
        self.service = Some(Service::with_config(engine, config).unwrap());
        let hello = self.send("Hello", object([]));
        assert_eq!(hello.kind, "Response");
        assert_eq!(get(&hello.payload, "slice").unwrap(), &Json::Bool(true));
    }

    /// The helper dies without a close: its workers have ended, nothing is shut down.
    fn crash(&mut self, inspector: Option<PathBuf>) {
        until(|| self.s().workers.reap());
        self.open(Some(self.codex()), inspector, None);
    }

    fn s(&mut self) -> &mut Service {
        self.service.as_mut().unwrap()
    }

    fn frame(&mut self, request: u128, minor: u64, kind: &str, payload: Json) -> Envelope {
        let e = Envelope {
            major: 1,
            minor,
            session: OpaqueId(5),
            epoch: self.s().engine.epoch().0,
            request: OpaqueId(request),
            revision: None,
            kind: kind.into(),
            payload,
        };
        self.s().request(&e)
    }

    fn send(&mut self, kind: &str, payload: Json) -> Envelope {
        self.next += 1;
        self.frame(self.next, 1, kind, payload)
    }

    /// The receipt's response text of an accepted command.
    fn ok(&mut self, kind: &str, payload: Json) -> String {
        let r = self.send(kind, payload);
        assert_eq!(r.kind, "Response", "{kind}: {:?}", r.payload);
        string(get(&r.payload, "response").unwrap())
            .unwrap()
            .to_owned()
    }

    /// The error code of a refused command.
    fn refused(&mut self, kind: &str, payload: Json) -> String {
        let r = self.send(kind, payload);
        assert_eq!(r.kind, "Error", "{kind}: {:?}", r.payload);
        string(get(&r.payload, "code").unwrap()).unwrap().to_owned()
    }

    fn read<T>(&mut self, f: impl FnOnce(&ReadView<'_>) -> Result<T, StorageError>) -> T {
        self.s().engine.reader().unwrap().read(f).unwrap()
    }

    fn register(&mut self) -> String {
        let path = self.repository.display().to_string();
        self.ok("RegisterRepository", object([("path", text(path))]))
    }

    fn bundle(&self, repository: &str, provider: &str, adr: &str) -> Json {
        let validation = ValidationPlan {
            steps: vec![StepValidation {
                step: node(STEP),
                write_scope: vec![ScopePath::parse("src").unwrap()],
                baseline: vec![ScopePath::parse("tests").unwrap()],
                tests: vec!["unit::check".into()],
                command: vec![],
            }],
        };
        object([
            ("feature", identity(OpaqueId(FEATURE))),
            ("prd_id", identity(OpaqueId(PRD))),
            ("adr_id", identity(OpaqueId(ADR))),
            ("plan_id", identity(OpaqueId(PLAN))),
            ("validation_id", identity(OpaqueId(VALIDATION))),
            ("prd", text(COMPLETE_PRD_JSON)),
            ("adr", text(adr)),
            ("step", identity(OpaqueId(STEP))),
            ("repository", text(repository)),
            ("base", text(self.commit.clone())),
            ("provider", text(provider)),
            ("validation", text(validation.canonical())),
        ])
    }

    /// Displays the exact review bundle, then decides on precisely those bytes.
    fn decide(&mut self, subject: u128, action: &str) -> Envelope {
        let q = self.send(
            "ReviewBundle",
            object([("subject", identity(OpaqueId(subject)))]),
        );
        assert_eq!(q.kind, "Response", "{q:?}");
        let revision = uint(get(&q.payload, "revision").unwrap()).unwrap();
        let Json::Object(mut fields) = q.payload else {
            panic!("review bundle payload")
        };
        fields.retain(|(k, _)| k != "revision");
        fields.push(("action".into(), text(action)));
        self.next += 1;
        let e = Envelope {
            major: 1,
            minor: 1,
            session: OpaqueId(5),
            epoch: self.s().engine.epoch().0,
            request: OpaqueId(self.next),
            revision: Some(revision),
            kind: "HumanDecision".into(),
            payload: Json::Object(fields),
        };
        self.s().request(&e)
    }

    fn approve(&mut self, subject: u128) {
        let r = self.decide(subject, "Approve");
        assert_eq!(r.kind, "Response", "{r:?}");
    }

    fn approved(&mut self, subject: u128) -> bool {
        self.read(|v| v.current_artifact_approval(ArtifactId(OpaqueId(subject))))
            .is_some()
    }

    /// Imports the bundle and approves every draft, one exact revision at a time.
    fn import_and_approve(&mut self, repository: &str, provider: &str) {
        let bundle = self.bundle(repository, provider, DEVELOP);
        for (stage, subjects) in [
            ("PRD", vec![PRD]),
            ("ADR", vec![ADR]),
            ("PLAN", vec![VALIDATION, PLAN]),
        ] {
            let response = self.ok("ImportBundle", bundle.clone());
            assert!(
                response.starts_with(&format!("stage={stage} ")),
                "{response}"
            );
            for subject in subjects {
                self.approve(subject);
            }
        }
        assert_eq!(self.ok("ImportBundle", bundle), "stage=COMPLETE awaiting=");
    }

    fn inspect(&mut self) -> (String, u64) {
        let r = self.send("RecoveryInspect", object([]));
        assert_eq!(r.kind, "Response", "{r:?}");
        (
            string(get(&r.payload, "digest").unwrap())
                .unwrap()
                .to_owned(),
            uint(get(&r.payload, "subjects").unwrap()).unwrap(),
        )
    }

    /// The human reviews the fresh inspection and starts: admission opens.
    fn open_admission(&mut self) {
        let (digest, _) = self.inspect();
        self.ok("RecoveryStart", object([("digest", text(digest))]));
    }

    fn start_payload(&self) -> Json {
        object([
            ("prd", identity(OpaqueId(PRD))),
            ("adr", identity(OpaqueId(ADR))),
            ("plan", identity(OpaqueId(PLAN))),
            ("validation", identity(OpaqueId(VALIDATION))),
            ("step", identity(OpaqueId(STEP))),
        ])
    }

    fn start(&mut self, command: u128) -> Envelope {
        let payload = self.start_payload();
        self.frame(command, 1, "Start", payload)
    }

    fn launches(&mut self) -> usize {
        self.events("ProcessLaunchIssued")
    }

    fn events(&mut self, kind: &str) -> usize {
        let mut reader = self.s().engine.reader().unwrap();
        reader
            .events_after(EventSeq(0), 10_000)
            .unwrap()
            .iter()
            .filter(|e| e.kind == kind)
            .count()
    }

    fn result(&mut self, attempt: u128) -> Option<AttemptResult> {
        self.read(|v| v.attempt_result(OpaqueId(attempt)))
    }

    fn settled(&mut self, attempt: u128) -> AttemptResult {
        until(|| self.s().workers.reap());
        assert!(!self.s().workers.failed(), "worker failed");
        self.result(attempt).expect("sealed result")
    }

    fn worktree(&mut self, attempt: u128) -> Option<WorktreeRecord> {
        self.read(|v| v.worktree(OpaqueId(attempt)))
    }
}

/// One commit with a source file and a test file; returns the commit.
fn sample_repository(path: &Path) -> String {
    for d in ["src", "tests"] {
        fs::create_dir_all(path.join(d)).unwrap();
    }
    git(path, &["init", "--template=", "--initial-branch=main"]);
    fs::write(path.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
    fs::write(path.join("tests/check.rs"), "fn check() {}\n").unwrap();
    git(path, &["add", "--all"]);
    git(path, &["commit", "-m", "sample"]);
    git(path, &["rev-parse", "HEAD"])
}

fn dispatch(e: &Envelope) -> &str {
    assert_eq!(e.kind, "Response", "{:?}", e.payload);
    string(get(&e.payload, "dispatch").unwrap()).unwrap()
}

fn code(e: &Envelope) -> &str {
    assert_eq!(e.kind, "Error", "{:?}", e.payload);
    string(get(&e.payload, "code").unwrap()).unwrap()
}

#[test]
fn p2_02_protocol_only_slice_registers_imports_approves_starts_and_settles() {
    let mut h = H::new();
    let repository = h.register();
    assert_eq!(repository.len(), 32);
    // Registration is idempotent and records one repository.
    assert_eq!(h.register(), repository);
    assert_eq!(h.events("LocalRepositoryRegistered"), 1);

    // Import creates drafts only. Each stage waits for an exact-revision human approval,
    // and sending the bundle again changes nothing.
    let bundle = h.bundle(&repository, "Codex", DEVELOP);
    let prd = format!("stage=PRD awaiting={}", hex(PRD));
    assert_eq!(h.ok("ImportBundle", bundle.clone()), prd);
    assert_eq!(h.ok("ImportBundle", bundle.clone()), prd);
    assert!(!h.approved(PRD));
    assert!(code(&h.start(900)).contains("AdmissionClosed"));
    h.open_admission();
    assert_eq!(
        code(&h.start(900)),
        "StartRefused: missing or stale approval: PRD"
    );
    h.approve(PRD);
    let adr = format!("stage=ADR awaiting={}", hex(ADR));
    assert_eq!(h.ok("ImportBundle", bundle.clone()), adr);
    assert!(h.approved(PRD) && !h.approved(ADR));
    assert_eq!(
        code(&h.start(900)),
        "StartRefused: missing or stale approval: ADR"
    );
    h.approve(ADR);
    assert_eq!(
        h.ok("ImportBundle", bundle.clone()),
        format!("stage=PLAN awaiting={},{}", hex(VALIDATION), hex(PLAN))
    );
    assert!(!h.approved(PLAN) && !h.approved(VALIDATION));
    // The plan is stored but not approved: the query has nothing to return.
    let ask = object([("after", Json::Null)]);
    assert_eq!(h.send("ApprovedPlan", ask.clone()).payload, Json::Null);
    assert_eq!(
        code(&h.start(900)),
        "StartRefused: missing or stale approval: plan"
    );
    h.approve(PLAN);
    assert_eq!(
        h.ok("ImportBundle", bundle.clone()),
        format!("stage=PLAN awaiting={}", hex(VALIDATION))
    );
    assert_eq!(
        code(&h.start(900)),
        "StartRefused: missing or stale approval: validation plan"
    );
    h.approve(VALIDATION);
    assert_eq!(
        h.ok("ImportBundle", bundle.clone()),
        "stage=COMPLETE awaiting="
    );
    // The approved plan as the core stores it: one step, no edge, and every identity a
    // Start needs. The validation plan is found by the digest the plan binds.
    let plan = h.send("ApprovedPlan", ask).payload;
    let field = |v: &Json, k: &str| get(v, k).unwrap().clone();
    assert_eq!(field(&plan, "plan"), text(hex(PLAN)));
    assert_eq!(field(&plan, "revision"), number(1));
    assert_eq!(field(&plan, "prd"), text(hex(PRD)));
    assert_eq!(field(&plan, "adr"), text(hex(ADR)));
    assert_eq!(field(&plan, "validation"), text(hex(VALIDATION)));
    assert_eq!(field(&plan, "edges"), Json::List(vec![]));
    assert_eq!(field(&plan, "next"), Json::Null);
    let Json::List(steps) = field(&plan, "steps") else {
        panic!("steps is a list")
    };
    assert_eq!(steps.len(), 1);
    assert_eq!(field(&steps[0], "step"), text(hex(STEP)));
    assert_eq!(field(&steps[0], "provider"), text("Codex"));
    assert_eq!(field(&steps[0], "repository"), text(repository.clone()));
    let Json::List(sources) = field(&steps[0], "sources") else {
        panic!("sources is a list")
    };
    assert_eq!(field(&sources[0], "commit"), text(h.commit.clone()));
    // A bundle that differs from the imported drafts is refused, never merged.
    let other = h.bundle(&repository, "Codex", "a different decision");
    assert_eq!(
        h.refused("ImportBundle", other),
        "bundle differs from the imported ADR"
    );
    // Nothing has run: every refusal above happened before anything was committed.
    assert_eq!(h.launches(), 0);
    assert!(h.worktree(900).is_none());
    assert_eq!(
        h.send("Receipt", object([("command", identity(OpaqueId(900)))]))
            .payload,
        Json::Null
    );

    // One human Start: reservation, owned worktree, binding and supervised launch.
    let started = h.start(900);
    assert_eq!(dispatch(&started), "launched");
    assert_eq!(
        get(&started.payload, "attempt").unwrap(),
        &identity(OpaqueId(900))
    );
    let result = h.settled(900);
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.outcome, AttemptOutcome::Succeeded, "{result:?}");
    assert!(result.candidate.is_some(), "{result:?}");
    let record = h.worktree(900).unwrap();
    assert!(record.path.starts_with(h.base.join("owned")));
    assert_eq!(
        fs::read_to_string(record.path.join("src/feature.rs")).unwrap(),
        "written by fixture\n"
    );
    // The user's repository was not written to.
    assert!(!h.repository.join("src/feature.rs").exists());
    assert_eq!(git(&h.repository, &["status", "--porcelain"]), "");
    assert_eq!(git(&h.repository, &["rev-parse", "HEAD"]), h.commit);
    // Exactly one launch, of the configured adapter's executable, with the one variable.
    assert_eq!(h.launches(), 1);
    let stdout = fs::read_to_string(record.output.join("generation-1/stdout.raw")).unwrap();
    assert!(stdout.contains("\"text\":\"developed\""), "{stdout}");
    assert_eq!(fs::read_dir(&h.home).unwrap().count(), 0);
    // The 1.1 commands do not exist for a 1.0 frame.
    let payload = h.start_payload();
    assert_eq!(code(&h.frame(901, 0, "Start", payload)), "UnknownType");
    assert_eq!(
        code(&h.frame(902, 0, "RecoveryInspect", object([]))),
        "UnknownType"
    );
}

#[test]
fn p2_02_start_is_refused_without_admission_approval_configuration_or_exact_payload() {
    let mut h = H::new();
    let repository = h.register();
    h.import_and_approve(&repository, "Codex");
    // Admission is closed until the human starts after a fresh, reviewed inspection.
    assert_eq!(code(&h.start(900)), "AdmissionClosed");
    assert_eq!(
        h.refused("RecoveryStart", object([("digest", text("0".repeat(64)))])),
        "StaleInspection: inspect again"
    );
    assert_eq!(code(&h.start(900)), "AdmissionClosed");
    h.open_admission();

    // No executable, argument or variable can travel in a frame.
    for extra in ["executable", "argv", "environment", "codex_home", "attempt"] {
        let Json::Object(mut fields) = h.start_payload() else {
            unreachable!()
        };
        fields.push((extra.into(), text("/bin/sh")));
        assert_eq!(
            code(&h.frame(900, 1, "Start", Json::Object(fields))),
            "InvalidPayload: unexpected or missing fields",
            "{extra}"
        );
    }
    let path = h.repository.display().to_string();
    assert_eq!(
        h.refused(
            "RegisterRepository",
            object([("path", text(path)), ("argv", text("--version"))])
        ),
        "InvalidPayload: unexpected or missing fields"
    );
    // Helper identities (high half) cannot be used by a client.
    assert_eq!(code(&h.start(1 << 127)), "ReservedIdentity");
    // A subject that is not the approved plan, or a step the plan does not contain.
    let wrong = object([
        ("prd", identity(OpaqueId(PRD))),
        ("adr", identity(OpaqueId(ADR))),
        ("plan", identity(OpaqueId(VALIDATION))),
        ("validation", identity(OpaqueId(VALIDATION))),
        ("step", identity(OpaqueId(STEP))),
    ]);
    assert_eq!(
        code(&h.frame(900, 1, "Start", wrong)),
        "StartRefused: approved subject is not an execution plan"
    );
    let wrong = object([
        ("prd", identity(OpaqueId(PRD))),
        ("adr", identity(OpaqueId(ADR))),
        ("plan", identity(OpaqueId(PLAN))),
        ("validation", identity(OpaqueId(VALIDATION))),
        ("step", identity(OpaqueId(77))),
    ]);
    assert_eq!(
        code(&h.frame(900, 1, "Start", wrong)),
        "StartRefused: step is not part of the approved plan"
    );

    // A revoked approval is stale for everything downstream of it.
    assert_eq!(h.decide(PRD, "Revoke").kind, "Response");
    assert!(
        code(&h.start(900)).starts_with("StartRefused: missing or stale approval"),
        "{:?}",
        h.start(900).payload
    );
    assert_eq!(h.launches(), 0);
    assert!(h.worktree(900).is_none());
    assert_eq!(h.events("SchedulerReady"), 0);

    // A helper started without the slice configuration offers none of it.
    let engine = h.s().engine.clone();
    let mut plain = Service::new(engine).unwrap();
    let frame = |kind: &str, minor: u64, request: u128, payload: Json, s: &Service| Envelope {
        major: 1,
        minor,
        session: OpaqueId(9),
        epoch: s.engine.epoch().0,
        request: OpaqueId(request),
        revision: None,
        kind: kind.into(),
        payload,
    };
    let hello = plain.request(&frame("Hello", 1, 1, object([]), &plain));
    assert_eq!(get(&hello.payload, "slice").unwrap(), &Json::Bool(false));
    let start = plain.request(&frame("Start", 1, 950, h.start_payload(), &plain));
    assert_eq!(code(&start), "SliceNotConfigured");
    let path = h.repository.display().to_string();
    let register = plain.request(&frame(
        "RegisterRepository",
        1,
        951,
        object([("path", text(path))]),
        &plain,
    ));
    assert_eq!(code(&register), "SliceNotConfigured");
}

#[test]
fn p2_02_repository_outside_the_disposable_root_is_rejected() {
    let mut h = H::new();
    let outside = h.base.join("elsewhere");
    sample_repository(&outside);
    let register = |h: &mut H, path: &Path| {
        h.refused(
            "RegisterRepository",
            object([("path", text(path.display().to_string()))]),
        )
    };
    assert_eq!(
        register(&mut h, &outside),
        "RepositoryOutsideDisposableRoot"
    );
    // A link inside the root that leads out of it resolves to where it really is.
    let link = h.base.join("disposable/link");
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    assert_eq!(register(&mut h, &link), "RepositoryOutsideDisposableRoot");
    // A worktree inside the root whose repository (common directory) lives outside.
    let linked = h.base.join("disposable/linked");
    git(
        &outside,
        &[
            "worktree",
            "add",
            "--detach",
            linked.to_str().unwrap(),
            "HEAD",
        ],
    );
    assert_eq!(register(&mut h, &linked), "RepositoryOutsideDisposableRoot");
    // The root itself, a traversal, a relative path, a missing path and a plain directory.
    let root = h.base.join("disposable");
    assert_eq!(register(&mut h, &root), "RepositoryOutsideDisposableRoot");
    let traversal = h.base.join("disposable/sample/../../elsewhere");
    assert_eq!(
        register(&mut h, &traversal),
        "RepositoryOutsideDisposableRoot"
    );
    assert!(register(&mut h, Path::new("disposable/sample")).contains("absolute path required"));
    assert!(register(&mut h, &root.join("absent")).starts_with("repository: "));
    fs::create_dir(root.join("plain")).unwrap();
    assert!(!register(&mut h, &root.join("plain")).is_empty());
    assert_eq!(h.events("LocalRepositoryRegistered"), 0);

    // An unregistered repository cannot be named by a bundle either.
    let foreign = LocalRepository::inspect(&outside).unwrap();
    let bundle = h.bundle(&hex(foreign.id().0.0), "Codex", DEVELOP);
    assert_eq!(
        h.refused("ImportBundle", bundle),
        "bundle repository is not registered"
    );
    // The root must be a private directory made for this purpose.
    chmod(&root, 0o755);
    let sample = h.repository.clone();
    assert!(register(&mut h, &sample).contains("private directory"));
    chmod(&root, 0o700);
    assert_eq!(h.register().len(), 32);
}

#[test]
fn p2_02_duplicate_replayed_and_lost_start_never_launch_twice_and_capacity_stays_one() {
    let mut h = H::new();
    // The runner is held at its first provider event so the run stays active.
    let (entered, waiting) = mpsc::channel::<()>();
    let (release, gate) = mpsc::channel::<()>();
    let (entered, gate) = (Mutex::new(entered), Arc::new(Mutex::new(gate)));
    let hooks: Hooks = Box::new(move || {
        let (entered, gate) = (entered.lock().unwrap().clone(), gate.clone());
        let mut first = true;
        LaunchHooks {
            on_event: Some(Box::new(move |_| {
                if std::mem::take(&mut first) {
                    let _ = entered.send(());
                    let _ = gate.lock().unwrap().recv();
                }
            })),
            ..LaunchHooks::default()
        }
    });
    h.open(Some(h.codex()), Some(ps()), Some(hooks));
    let repository = h.register();
    h.import_and_approve(&repository, "Codex");
    h.open_admission();

    assert_eq!(dispatch(&h.start(900)), "launched");
    waiting
        .recv_timeout(std::time::Duration::from_secs(20))
        .unwrap();
    // While it runs: the same frame is answered from its receipt; a new Start is refused.
    let replay = h.start(900);
    assert_eq!(dispatch(&replay), "not repeated");
    assert_eq!(get(&replay.payload, "replayed").unwrap(), &Json::Bool(true));
    assert_eq!(
        code(&h.start(901)),
        "CapacityFull: 1 attempts and 0 lead turn use all 1 units; a Start waits"
    );
    // The same identity with another payload is a conflict, not a second command.
    let other = object([
        ("prd", identity(OpaqueId(PRD))),
        ("adr", identity(OpaqueId(ADR))),
        ("plan", identity(OpaqueId(PLAN))),
        ("validation", identity(OpaqueId(VALIDATION))),
        ("step", identity(OpaqueId(2))),
    ]);
    assert!(code(&h.frame(900, 1, "Start", other)).contains("Conflict"));
    // A lost response is reconciled by the receipt, which exists and names the attempt.
    let receipt = h.send("Receipt", object([("command", identity(OpaqueId(900)))]));
    assert_eq!(
        string(get(&receipt.payload, "response").unwrap()).unwrap(),
        format!("accepted attempt={}", hex(900))
    );
    release.send(()).unwrap();
    assert_eq!(h.settled(900).outcome, AttemptOutcome::Succeeded);

    // After it finished: replay still repeats nothing, and the step cannot start again.
    assert_eq!(dispatch(&h.start(900)), "not repeated");
    assert_eq!(
        code(&h.start(902)),
        "DuplicateStart: this step was already started"
    );
    assert_eq!(h.launches(), 1);
    assert!(h.worktree(901).is_none() && h.worktree(902).is_none());
    assert_eq!(
        h.send("Receipt", object([("command", identity(OpaqueId(902)))]))
            .payload,
        Json::Null
    );
    // Replays of the other commands return their receipts too.
    let path = h.repository.display().to_string();
    let register = object([("path", text(path))]);
    let first = h.frame(5000, 1, "RegisterRepository", register.clone());
    assert_eq!(get(&first.payload, "replayed").unwrap(), &Json::Bool(false));
    let again = h.frame(5000, 1, "RegisterRepository", register);
    assert_eq!(get(&again.payload, "replayed").unwrap(), &Json::Bool(true));
}

#[test]
fn p2_02_crash_between_issue_and_spawn_is_held_and_never_relaunched() {
    let mut h = H::new();
    // The helper "dies" right after the launch intent is durably issued.
    let hooks: Hooks = Box::new(|| LaunchHooks {
        after_issue: Some(Box::new(|| panic!("helper killed between issue and spawn"))),
        ..LaunchHooks::default()
    });
    h.open(Some(h.codex()), Some(ps()), Some(hooks));
    let repository = h.register();
    h.import_and_approve(&repository, "Codex");
    h.open_admission();
    assert_eq!(dispatch(&h.start(900)), "launched");
    until(|| h.s().workers.reap());
    assert!(h.s().workers.failed());
    let output = h.worktree(900).unwrap().output;
    h.crash(Some(ps()));

    // New epoch: nothing was spawned, and nothing proves it. The launch stays unknown.
    assert!(!output.join("generation-1").exists());
    assert_eq!(h.launches(), 1);
    assert!(h.result(900).is_none());
    // The lost Start is answered from its receipt in the new epoch; it never runs again.
    assert_eq!(dispatch(&h.start(900)), "not repeated");
    assert_eq!(code(&h.start(901)), "AdmissionClosed");
    let (digest, subjects) = h.inspect();
    assert_eq!(subjects, 1);
    let resolve = |h: &mut H, digest: &str, choice: &str| {
        h.send(
            "RecoveryResolve",
            object([
                ("digest", text(digest)),
                ("attempt", identity(OpaqueId(900))),
                ("choice", text(choice)),
            ]),
        )
    };
    // No human statement substitutes for OS evidence: only Hold is offered.
    for choice in [
        "ValidateOutput",
        "ContinueWithHistory",
        "ContinueFreshContext",
    ] {
        let r = resolve(&mut h, &digest, choice);
        assert!(
            code(&r).contains("is not offered"),
            "{choice}: {:?}",
            r.payload
        );
    }
    assert_eq!(code(&resolve(&mut h, &digest, "Adopt")), "UnknownChoice");
    assert_eq!(
        code(&resolve(&mut h, &"0".repeat(64), "Hold")),
        "StaleInspection: inspect again"
    );
    let held = resolve(&mut h, &digest, "Hold");
    assert_eq!(
        string(get(&held.payload, "response").unwrap()).unwrap(),
        "HOLD"
    );
    // The human may open admission after review, but the held attempt still blocks Start
    // and keeps its claims: capacity one is not freed by a decision.
    let (digest, subjects) = h.inspect();
    assert_eq!(subjects, 1);
    h.ok("RecoveryStart", object([("digest", text(digest))]));
    assert_eq!(
        code(&h.start(901)),
        "RecoveryHold: unresolved recovery subjects"
    );
    assert_eq!(h.launches(), 1);
    assert!(h.worktree(901).is_none());
    let claims = h
        .s()
        .engine
        .reader()
        .unwrap()
        .snapshot()
        .unwrap()
        .live_claims;
    assert!(
        claims.iter().any(|c| c.state == ClaimState::Quarantined),
        "{claims:?}"
    );
}

#[test]
fn p2_02_recovery_after_forced_interruption_needs_os_evidence_before_it_settles() {
    let mut h = H::new();
    // The helper dies mid-run; its supervision handle kills the owned group on the way.
    let hooks: Hooks = Box::new(|| LaunchHooks {
        on_event: Some(Box::new(|_| {
            panic!("helper killed while the run is active")
        })),
        ..LaunchHooks::default()
    });
    h.open(Some(h.codex()), Some(ps()), Some(hooks));
    let repository = h.register();
    h.import_and_approve(&repository, "Codex");
    h.open_admission();
    assert_eq!(dispatch(&h.start(900)), "launched");
    until(|| h.s().workers.reap());
    assert!(h.s().workers.failed());
    assert!(h.result(900).is_none());

    let resolve = |h: &mut H, choice: &str| {
        let (digest, subjects) = h.inspect();
        assert_eq!(subjects, 1);
        h.send(
            "RecoveryResolve",
            object([
                ("digest", text(digest)),
                ("attempt", identity(OpaqueId(900))),
                ("choice", text(choice)),
            ]),
        )
    };
    // A helper that cannot inspect processes knows nothing: Hold is the only choice.
    h.crash(Some(h.base.join("no-such-inspector")));
    for choice in [
        "ValidateOutput",
        "ContinueWithHistory",
        "ContinueFreshContext",
    ] {
        let r = resolve(&mut h, choice);
        assert!(
            code(&r).contains("is not offered"),
            "{choice}: {:?}",
            r.payload
        );
    }
    assert_eq!(resolve(&mut h, "Hold").kind, "Response");
    assert!(h.result(900).is_none());

    // With real OS evidence that the recorded process is gone, output can be validated.
    h.crash(Some(ps()));
    assert_eq!(code(&h.start(901)), "AdmissionClosed");
    // A continuation is a new reviewed attempt; none is offered without supported history.
    let unsafe_continuation = resolve(&mut h, "ContinueWithHistory");
    assert!(code(&unsafe_continuation).contains("is not offered"));
    let settled = resolve(&mut h, "ValidateOutput");
    assert_eq!(
        string(get(&settled.payload, "response").unwrap()).unwrap(),
        "SETTLE",
        "{:?}",
        settled.payload
    );
    let result = h.result(900).expect("settled from durable evidence");
    assert_ne!(result.outcome, AttemptOutcome::Succeeded, "{result:?}");
    // Settling is not the end of review: the sealed attempt stays a subject, and
    // admission stays closed, until a second decision on a fresh inspection closes it.
    assert_eq!(code(&h.start(901)), "AdmissionClosed");
    let released = resolve(&mut h, "ValidateOutput");
    assert_eq!(
        string(get(&released.payload, "response").unwrap()).unwrap(),
        "CLAIMS_RELEASED"
    );
    // A run that did not succeed remains a recovery subject (the core keeps it until a
    // reviewed continuation exists, which this protocol does not offer). The human may
    // open admission, but the interrupted step is never silently run again.
    let (digest, subjects) = h.inspect();
    assert_eq!(subjects, 1);
    h.ok("RecoveryStart", object([("digest", text(digest))]));
    assert_eq!(
        code(&h.start(901)),
        "RecoveryHold: unresolved recovery subjects"
    );
    assert_eq!(h.launches(), 1);
}

#[test]
fn p2_02_only_the_configured_codex_adapter_can_launch() {
    let mut h = H::new();
    let repository = h.register();
    h.import_and_approve(&repository, "ClaudeCode");
    h.open_admission();
    // A plan for a provider without a production adapter cannot be started.
    assert_eq!(code(&h.start(900)), "NoProductionAdapter: ClaudeCode");

    // Codex plan, but the helper's adapter is absent, mismatched or not a native binary.
    drop(h);
    let mut h = H::new();
    let repository = h.register();
    h.import_and_approve(&repository, "Codex");
    h.open_admission();
    let good = h.codex();
    let fixture_script = h.base.join("bin/launcher");
    fs::write(&fixture_script, "#!/bin/sh\nexit 99\n").unwrap();
    chmod(&fixture_script, 0o755);
    let cases: Vec<(Option<CodexAdapterConfig>, &str)> = vec![
        (None, "NoProductionAdapter: Codex is not configured"),
        (
            Some(CodexAdapterConfig {
                sha256: "0".repeat(64),
                ..good.clone()
            }),
            "AdapterRejected: Digest",
        ),
        (
            Some(CodexAdapterConfig {
                version: "codex-cli 0.170.0".into(),
                ..good.clone()
            }),
            "AdapterRejected: Version",
        ),
        (
            Some(CodexAdapterConfig {
                sha256: sha256_hex(&[&fs::read(&fixture_script).unwrap()]),
                executable: fixture_script.clone(),
                ..good.clone()
            }),
            "AdapterRejected: Executable",
        ),
        (
            Some(CodexAdapterConfig {
                auth: AuthMode::Unknown,
                ..good.clone()
            }),
            "AdapterRejected: Authentication",
        ),
    ];
    for (i, (codex, expected)) in cases.into_iter().enumerate() {
        let engine = h.s().engine.clone();
        let mut service = Service::with_config(
            engine,
            ServiceConfig {
                slice: Some(h.slice(codex)),
                inspector: Some(ps()),
                launch_hooks: None,
            },
        )
        .unwrap();
        let frame = |kind: &str, request: u128, payload: Json, s: &Service| Envelope {
            major: 1,
            minor: 1,
            session: OpaqueId(40 + i as u128),
            epoch: s.engine.epoch().0,
            request: OpaqueId(request),
            revision: None,
            kind: kind.into(),
            payload,
        };
        let hello = frame("Hello", 1, object([]), &service);
        assert_eq!(service.request(&hello).kind, "Response");
        let start = frame("Start", 910 + i as u128, h.start_payload(), &service);
        let refused = service.request(&start);
        assert!(
            code(&refused).starts_with(expected),
            "{:?}",
            refused.payload
        );
        assert!(service.workers.reap());
    }
    // Nothing was launched by any of them.
    assert_eq!(h.launches(), 0);
    // Every refusal came before acceptance: no receipt, queue entry or worktree exists.
    assert_eq!(h.events("SchedulerReady"), 0);
    for command in 910..915 {
        assert!(h.worktree(command).is_none());
    }
}

#[test]
fn r01_recovery_rows_carry_the_assessment_as_structured_fields_for_1_1_only() {
    let mut h = H::new();
    let hooks: Hooks = Box::new(|| LaunchHooks {
        after_issue: Some(Box::new(|| panic!("helper killed between issue and spawn"))),
        ..LaunchHooks::default()
    });
    h.open(Some(h.codex()), Some(ps()), Some(hooks));
    let repository = h.register();
    h.import_and_approve(&repository, "Codex");
    h.open_admission();
    assert_eq!(dispatch(&h.start(900)), "launched");
    until(|| h.s().workers.reap());
    h.crash(Some(ps()));

    let page = || object([("offset", number(0)), ("limit", number(8))]);
    let row = |e: &Envelope| -> Vec<(String, Json)> {
        let Json::List(rows) = get(&e.payload, "rows").unwrap() else {
            panic!("rows is a list")
        };
        assert_eq!(rows.len(), 1);
        let Json::Object(fields) = &rows[0] else {
            panic!("a row is an object")
        };
        fields.clone()
    };
    // A 1.0 frame gets the row exactly as before.
    let old = row(&h.frame(5_000, 0, "Recovery", page()));
    let names = |fields: &[(String, Json)]| -> Vec<String> {
        fields.iter().map(|(k, _)| k.clone()).collect()
    };
    assert_eq!(
        names(&old),
        ["attempt", "assessment", "findings", "detail_complete"]
    );
    // A 1.1 frame gets the same four fields, then the assessment as structured fields.
    let new = row(&h.frame(5_001, 1, "Recovery", page()));
    assert_eq!(new[..4], old[..]);
    assert_eq!(
        names(&new[4..]),
        ["blockers", "blockers_complete", "process_absent", "choices"]
    );
    let field = |k: &str| &new.iter().find(|(name, _)| name == k).unwrap().1;
    let Json::List(blockers) = field("blockers") else {
        panic!("blockers is a list")
    };
    // The structured fields say what the debug text says, without parsing it.
    let assessment = string(field("assessment")).unwrap();
    assert!(!blockers.is_empty());
    for blocker in blockers {
        assert!(
            assessment.contains(string(blocker).unwrap()),
            "{assessment}"
        );
    }
    assert_eq!(field("blockers_complete"), &Json::Bool(true));
    assert_eq!(field("process_absent"), &Json::Bool(false));
    assert_eq!(field("choices"), &Json::List(vec![text("Hold")]));
    assert!(assessment.ends_with("choices: [Hold] }"), "{assessment}");
}

// M1: the same slice with a Claude Code step. "Claude Code" is the same private fixture
// executable, which speaks the Claude Code stream dialect when it is launched with the
// Claude Code argument vector. The installed CLI is never named, located or executed.
use jojo_domain::claude::{CLAUDE_VERSION, print_argv};

impl H {
    /// A helper whose launcher configured Claude Code: the fixture, and an empty home.
    fn with_claude() -> Self {
        let mut h = Self::new();
        let home = h.base.join("claude-home");
        fs::create_dir(&home).unwrap();
        chmod(&home, 0o700);
        let config = h.claude();
        h.s().claude = Some(config);
        h
    }

    fn claude(&self) -> ClaudeAdapterConfig {
        ClaudeAdapterConfig {
            executable: self.fixture.clone(),
            version: CLAUDE_VERSION.into(),
            sha256: sha256_hex(&[&fs::read(&self.fixture).unwrap()]),
            home: self.base.join("claude-home"),
            user: "tester".into(),
            auth: AuthMode::NativeSubscription,
        }
    }

    /// Imports and approves a Claude Code bundle whose ADR carries the fixture scenario,
    /// opens admission and starts attempt 900. Returns its standard output once settled.
    fn run_claude(&mut self, scenario: &str) -> (AttemptResult, String) {
        let repository = self.register();
        let adr =
            format!("Keep the change inside src/.\nfixture: {scenario}\nNo other file changes.");
        let bundle = self.bundle(&repository, "ClaudeCode", &adr);
        for subjects in [vec![PRD], vec![ADR], vec![VALIDATION, PLAN]] {
            self.ok("ImportBundle", bundle.clone());
            for subject in subjects {
                self.approve(subject);
            }
        }
        self.open_admission();
        assert_eq!(dispatch(&self.start(900)), "launched");
        let result = self.settled(900);
        let output = self.worktree(900).unwrap().output;
        let stdout = fs::read_to_string(output.join("generation-1/stdout.raw")).unwrap();
        (result, stdout)
    }
}

#[test]
fn m1_claude_code_step_runs_through_the_claude_adapter_and_settles() {
    let mut h = H::with_claude();
    let (result, stdout) = h.run_claude("develop src/feature.rs");
    assert_eq!(result.exit_code, Some(0));
    assert_eq!(result.outcome, AttemptOutcome::Succeeded, "{result:?}");
    assert!(result.candidate.is_some(), "{result:?}");
    let record = h.worktree(900).unwrap();
    assert!(record.path.starts_with(h.base.join("owned")));
    assert_eq!(
        fs::read_to_string(record.path.join("src/feature.rs")).unwrap(),
        "written by fixture\n"
    );
    assert!(stdout.contains("\"text\":\"developed\""), "{stdout}");
    // The user's repository and both provider homes were not written to.
    assert_eq!(git(&h.repository, &["status", "--porcelain"]), "");
    assert_eq!(fs::read_dir(h.base.join("claude-home")).unwrap().count(), 0);
    assert_eq!(fs::read_dir(&h.home).unwrap().count(), 0);
    assert_eq!(h.launches(), 1);
    // The same command again is answered from its receipt; nothing runs twice.
    assert_eq!(dispatch(&h.start(900)), "not repeated");
    assert_eq!(h.launches(), 1);
}

#[test]
fn m1_claude_code_child_receives_exactly_the_golden_arguments_and_two_variables() {
    let mut h = H::with_claude();
    let (_, stdout) = h.run_claude("report");
    let home = h.base.join("claude-home");
    let reported = format!(
        "argv={:?} variables={:?}",
        print_argv(),
        [format!("HOME={}", home.display()), "USER=tester".to_owned()]
    );
    // The fixture prints what it was started with, JSON-escaped inside a text block.
    let escaped = reported.replace('\\', "\\\\").replace('"', "\\\"");
    assert!(stdout.contains(&escaped), "{stdout}");
}

#[test]
fn m1_claude_code_runs_outside_the_envelope_or_without_sign_in_never_succeed() {
    for scenario in [
        "auth-error",
        "tool-not-approved",
        "init-not-approved",
        "unknown-kind",
        "exit-zero-no-terminal",
    ] {
        let mut h = H::with_claude();
        let (result, stdout) = h.run_claude(scenario);
        assert_ne!(
            result.outcome,
            AttemptOutcome::Succeeded,
            "{scenario}: {result:?}\n{stdout}"
        );
    }
}

// D3: the serving helper never hashes an executable on its control thread. A Start that
// finds the digest unknown is answered "not now" while it is computed in the background.
#[test]
fn d3_a_start_never_waits_for_an_executable_digest() {
    let mut h = H::with_claude();
    h.s().background_checks = true;
    let repository = h.register();
    let adr =
        "Keep the change inside src/.\nfixture: develop src/feature.rs\nNo other file changes.";
    let bundle = h.bundle(&repository, "ClaudeCode", adr);
    for subjects in [vec![PRD], vec![ADR], vec![VALIDATION, PLAN]] {
        h.ok("ImportBundle", bundle.clone());
        for subject in subjects {
            h.approve(subject);
        }
    }
    h.open_admission();
    // The fixture copy is private to this test, so its digest is not known yet.
    let first = h.start(900);
    assert_eq!(first.kind, "Error", "{:?}", first.payload);
    let code = string(get(&first.payload, "code").unwrap())
        .unwrap()
        .to_owned();
    assert!(code.starts_with("ExecutableChecking"), "{code}");
    assert_eq!(h.launches(), 0);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while begin_check(&h.fixture) != ExecutableCheck::Known {
        assert!(
            std::time::Instant::now() < deadline,
            "digest never became known"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(dispatch(&h.start(900)), "launched");
    assert_eq!(h.settled(900).outcome, AttemptOutcome::Succeeded);
    assert_eq!(h.launches(), 1);
}

// D3: a Claude Code step whose executable the helper discovers on the PATH it was given,
// learning its version and digest; nothing about the executable is configured.
#[test]
fn d3_a_discovered_claude_code_executable_runs_a_start() {
    let mut h = H::with_claude();
    h.s().claude = None;
    let path = h.base.join("path");
    let real = h.base.join("install/claude");
    fs::create_dir_all(real.parent().unwrap()).unwrap();
    fs::create_dir(&path).unwrap();
    fs::copy(&h.fixture, &real).unwrap();
    chmod(&real, 0o755);
    std::os::unix::fs::symlink(&real, path.join("claude")).unwrap();
    let unpinned = Unpinned {
        executable: None,
        home: h.base.join("claude-home"),
        user: "tester".into(),
    };
    h.s().discovered = Some(Discovered {
        discovery: Discovery::with_search_path(path.display().to_string()),
        codex: None,
        claude: Some(unpinned),
    });
    let repository = h.register();
    let adr =
        "Keep the change inside src/.\nfixture: develop src/feature.rs\nNo other file changes.";
    let bundle = h.bundle(&repository, "ClaudeCode", adr);
    for subjects in [vec![PRD], vec![ADR], vec![VALIDATION, PLAN]] {
        h.ok("ImportBundle", bundle.clone());
        for subject in subjects {
            h.approve(subject);
        }
    }
    h.open_admission();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let launched = loop {
        let r = h.start(900);
        if r.kind != "Error" {
            break r;
        }
        let code = string(get(&r.payload, "code").unwrap()).unwrap().to_owned();
        assert!(code.starts_with("ExecutableChecking"), "{code}");
        assert!(std::time::Instant::now() < deadline, "never found");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!(dispatch(&launched), "launched");
    assert_eq!(h.settled(900).outcome, AttemptOutcome::Succeeded);
    // The run was frozen against the binary the link resolves to.
    let found = h
        .s()
        .discovered
        .as_ref()
        .unwrap()
        .find(ProviderKind::ClaudeCode);
    assert!(
        matches!(found, Some(Lookup::Found(ref f)) if f.path == real && f.profiled),
        "{found:?}"
    );
    // The Executables query reports the discovered file and the run that used it.
    let r = h.frame(9000, 4, "Executables", object([]));
    assert_eq!(r.kind, "Response", "{:?}", r.payload);
    let Json::List(rows) = get(&r.payload, "providers").unwrap() else {
        panic!("providers")
    };
    let claude = &rows[0];
    let field = |name: &str| string(get(claude, name).unwrap()).unwrap().to_owned();
    assert_eq!(
        (field("source"), field("state")),
        ("discovered".into(), "ready".into())
    );
    assert_eq!(field("entry"), path.join("claude").display().to_string());
    assert_eq!(field("path"), real.display().to_string());
    let last = get(claude, "last_run").unwrap();
    assert_eq!(
        string(get(last, "path").unwrap()).unwrap(),
        real.display().to_string()
    );
    assert_eq!(get(claude, "change").unwrap(), &Json::Null);
    // Codex is pinned by this helper's launcher.
    assert_eq!(string(get(&rows[1], "source").unwrap()).unwrap(), "pinned");
}

#[test]
fn d3_a_provider_whose_executable_is_not_found_is_refused_with_the_reason() {
    let mut h = H::with_claude();
    h.s().claude = None;
    let path = h.base.join("empty-path");
    fs::create_dir(&path).unwrap();
    h.s().discovered = Some(Discovered {
        discovery: Discovery::with_search_path(path.display().to_string()),
        codex: None,
        claude: Some(Unpinned {
            executable: None,
            home: h.base.join("claude-home"),
            user: "tester".into(),
        }),
    });
    let repository = h.register();
    let bundle = h.bundle(&repository, "ClaudeCode", "fixture: ok");
    for subjects in [vec![PRD], vec![ADR], vec![VALIDATION, PLAN]] {
        h.ok("ImportBundle", bundle.clone());
        for subject in subjects {
            h.approve(subject);
        }
    }
    h.open_admission();
    let code = h.refused("Start", h.start_payload());
    assert!(
        code.starts_with(
            "ExecutableRefused: ClaudeCode: `claude` is not on the login shell's PATH"
        ),
        "{code}"
    );
    assert_eq!(h.launches(), 0);
}

#[test]
fn m1_only_the_configured_claude_code_adapter_can_launch() {
    let mut h = H::with_claude();
    let repository = h.register();
    h.import_and_approve(&repository, "ClaudeCode");
    h.open_admission();
    let good = h.claude();
    let script = h.base.join("bin/launcher");
    fs::write(&script, "#!/bin/sh\nexit 99\n").unwrap();
    chmod(&script, 0o755);
    let cases: Vec<(Option<ClaudeAdapterConfig>, &str)> = vec![
        (None, "NoProductionAdapter: ClaudeCode"),
        (
            Some(ClaudeAdapterConfig {
                sha256: "0".repeat(64),
                ..good.clone()
            }),
            "AdapterRejected: Digest",
        ),
        (
            Some(ClaudeAdapterConfig {
                version: "2.1.312 (Claude Code)".into(),
                ..good.clone()
            }),
            "AdapterRejected: Version",
        ),
        (
            Some(ClaudeAdapterConfig {
                sha256: sha256_hex(&[&fs::read(&script).unwrap()]),
                executable: script.clone(),
                ..good.clone()
            }),
            "AdapterRejected: Executable",
        ),
        (
            Some(ClaudeAdapterConfig {
                auth: AuthMode::ApiKey,
                ..good.clone()
            }),
            "AdapterRejected: Authentication",
        ),
        (
            Some(ClaudeAdapterConfig {
                home: h.base.join("no-such-home"),
                ..good.clone()
            }),
            "AdapterRejected: Home",
        ),
    ];
    for (i, (claude, expected)) in cases.into_iter().enumerate() {
        h.s().claude = claude;
        let refused = code(&h.start(900 + i as u128)).to_owned();
        assert!(refused.starts_with(expected), "{expected}: {refused}");
    }
    // An account name that is not a plain name is refused where the request crosses the
    // launch boundary: after the command committed, and before anything was started.
    for (i, claude) in [ClaudeAdapterConfig {
        user: "two words".into(),
        ..good.clone()
    }]
    .into_iter()
    .enumerate()
    {
        h.s().claude = Some(claude);
        let refused = code(&h.start(950 + i as u128)).to_owned();
        assert!(
            refused.starts_with("StartFailed: AdapterRejected: Launch(Environment("),
            "{refused}"
        );
    }
    assert_eq!(h.launches(), 0);
}

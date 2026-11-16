//! P2-03: host validation, result inspection and human acceptance, driven by helper
//! protocol frames handled by the production `Service`. The developer run is a private
//! copy of `jojo-codex-fixture` behind the production `CodexAdapter`; the validation
//! command is whatever the approved validation plan names, run by the production runner
//! under the OS network sandbox. No real provider, credential or network is involved, and
//! no test reads a test result from provider output.
mod common;
use common::attempts::{git, ps, until};
use common::{TempDir, start};
use jojo_domain::attempt::AttemptOutcome;
use jojo_domain::codex::CODEX_VERSION;
use jojo_domain::identity::*;
use jojo_domain::ipc::*;
use jojo_domain::json::Json;
use jojo_domain::provider::AuthMode;
use jojo_domain::validation::{ScopePath, StepValidation, ValidationPlan};
use jojo_helper::service::*;
use jojo_storage::*;
use jojo_test_support::{COMPLETE_PRD_JSON, dependency::node};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const CODEX_FIXTURE: &str = env!("CARGO_BIN_EXE_jojo-codex-fixture");
const FEATURE: u128 = 700;
const PRD: u128 = 701;
const ADR: u128 = 702;
const PLAN: u128 = 703;
const VALIDATION: u128 = 704;
const STEP: u128 = 1;
const IN_SCOPE: &str = "src/feature.rs";
/// Passes only if the materialized tree holds the candidate's file.
const CHECK: &[&str] = &[
    "/usr/bin/grep",
    "-q",
    "written by fixture",
    "src/feature.rs",
];

fn chmod(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn hex(id: u128) -> String {
    format!("{id:032x}")
}

/// A helper service over a temporary data root, one disposable root holding one sample
/// repository, and a private fixture executable with an empty selected Codex home.
struct H {
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
        let dir = TempDir::new("p2-03");
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

    fn read<T>(&mut self, f: impl FnOnce(&ReadView<'_>) -> Result<T, StorageError>) -> T {
        self.s().engine.reader().unwrap().read(f).unwrap()
    }

    fn register(&mut self) -> String {
        let path = self.repository.display().to_string();
        self.ok("RegisterRepository", object([("path", text(path))]))
    }

    fn bundle(&self, repository: &str, develop: &str, command: &[&str]) -> Json {
        let provider = "Codex";
        let adr = format!("Decision.\nfixture: develop {develop}\nEnd.");
        let validation = ValidationPlan {
            steps: vec![StepValidation {
                step: node(STEP),
                write_scope: vec![ScopePath::parse("src").unwrap()],
                baseline: vec![ScopePath::parse("tests").unwrap()],
                tests: vec!["unit::check".into()],
                command: command.iter().map(|a| a.to_string()).collect(),
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

    /// Imports the bundle and approves every draft, one exact revision at a time.
    fn import_and_approve(&mut self, repository: &str, develop: &str, command: &[&str]) {
        let bundle = self.bundle(repository, develop, command);
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

/// What one settled developer run left behind.
struct Run {
    h: H,
    attempt: u128,
    candidate: String,
}

/// Registers, imports, approves, starts and settles one developer run that writes
/// `develop`, with `command` as the approved validation command.
fn run(develop: &str, command: &[&str]) -> Run {
    run_in(H::new(), develop, command)
}

fn run_in(mut h: H, develop: &str, command: &[&str]) -> Run {
    let repository = h.register();
    h.import_and_approve(&repository, develop, command);
    h.open_admission();
    assert_eq!(dispatch(&h.start(900)), "launched");
    let result = h.settled(900);
    assert_eq!(result.outcome, AttemptOutcome::Succeeded, "{result:?}");
    let candidate = result.candidate.expect("sealed candidate");
    Run {
        h,
        attempt: 900,
        candidate,
    }
}

impl Run {
    fn subject(&self) -> Vec<(&'static str, Json)> {
        vec![
            ("attempt", identity(OpaqueId(self.attempt))),
            ("candidate", text(self.candidate.clone())),
        ]
    }

    fn validate_frame(&mut self, command: u128) -> Envelope {
        let payload = object(self.subject());
        self.h.frame(command, 1, "Validate", payload)
    }

    /// One requested validation, waited to its durable end.
    fn validate(&mut self, command: u128) -> String {
        let r = self.validate_frame(command);
        assert_eq!(r.kind, "Response", "{:?}", r.payload);
        assert_eq!(get(&r.payload, "validation").unwrap(), &text("started"));
        self.wait();
        self.state()
    }

    fn wait(&mut self) {
        until(|| self.h.s().workers.reap());
        assert!(!self.h.s().workers.failed(), "validation worker failed");
    }

    fn view(&mut self) -> Json {
        let r = self.h.send(
            "Candidate",
            object([("attempt", identity(OpaqueId(self.attempt)))]),
        );
        assert_eq!(r.kind, "Response", "{:?}", r.payload);
        r.payload
    }

    fn validation(&mut self, key: &str) -> Json {
        get(get(&self.view(), "validation").unwrap(), key)
            .unwrap()
            .clone()
    }

    fn state(&mut self) -> String {
        string(&self.validation("state")).unwrap().to_owned()
    }

    fn detail(&mut self) -> String {
        string(&self.validation("detail")).unwrap().to_owned()
    }

    fn reasons(&mut self) -> Vec<String> {
        let Json::List(items) = self.validation("reasons") else {
            panic!("reasons")
        };
        items
            .iter()
            .map(|r| string(r).unwrap().to_owned())
            .collect()
    }

    fn receipt(&mut self) -> Json {
        self.validation("receipt")
    }

    fn decide(&mut self, command: u128, action: &str, receipt: Json) -> Envelope {
        let mut fields = self.subject();
        fields.push(("receipt", receipt));
        fields.push(("action", text(action)));
        self.h
            .frame(command, 1, "CandidateDecision", object(fields))
    }

    fn changes(&mut self) -> Envelope {
        let mut fields = self.subject();
        fields.push(("offset", number(0)));
        fields.push(("limit", number(8)));
        self.h.send("CandidateChanges", object(fields))
    }

    fn rows(&mut self) -> Vec<(String, String)> {
        let r = self.changes();
        assert_eq!(r.kind, "Response", "{:?}", r.payload);
        let Json::List(rows) = get(&r.payload, "rows").unwrap() else {
            panic!("rows")
        };
        rows.iter()
            .map(|row| {
                (
                    string(get(row, "path").unwrap()).unwrap().to_owned(),
                    string(get(row, "scope").unwrap()).unwrap().to_owned(),
                )
            })
            .collect()
    }

    fn paged(&mut self, kind: &str, mut fields: Vec<(&'static str, Json)>) -> String {
        fields.push(("offset", number(0)));
        let r = self.h.send(kind, object(fields));
        assert_eq!(r.kind, "Response", "{kind}: {:?}", r.payload);
        string(get(&r.payload, "text").unwrap()).unwrap().to_owned()
    }

    fn diff(&mut self) -> String {
        let fields = self.subject();
        self.paged("CandidateDiff", fields)
    }

    fn report(&mut self) -> String {
        let receipt = self.receipt();
        let attempt = identity(OpaqueId(self.attempt));
        self.paged(
            "ValidationReport",
            vec![("attempt", attempt), ("receipt", receipt)],
        )
    }

    /// The sample repository is exactly as it was committed.
    fn source_untouched(&self) {
        assert_eq!(git(&self.h.repository, &["status", "--porcelain"]), "");
        assert_eq!(
            git(&self.h.repository, &["rev-parse", "HEAD"]),
            self.h.commit
        );
    }

    fn never_accepted(&mut self) {
        assert_eq!(self.h.events("CandidateAccepted"), 0);
        assert_eq!(
            string(get(&self.view(), "decision").unwrap()).unwrap(),
            "NONE"
        );
    }
}

fn budget(timeout_millis: u64) -> ValidationBudget {
    ValidationBudget {
        timeout: std::time::Duration::from_millis(timeout_millis),
        terminate: std::time::Duration::from_secs(2),
        kill: std::time::Duration::from_secs(5),
        drain: std::time::Duration::from_secs(5),
        max_output_bytes: 1024 * 1024,
    }
}

#[test]
fn p2_03_in_scope_change_passes_host_validation_and_is_accepted_by_the_human() {
    let mut r = run(IN_SCOPE, CHECK);
    // The provider finished successfully. That is not a validation result.
    let view = r.view();
    assert_eq!(string(get(&view, "outcome").unwrap()).unwrap(), "Succeeded");
    assert_eq!(r.state(), "NOT_RUN");
    let unvalidated = r.decide(801, "Accept", number(1));
    assert_eq!(
        code(&unvalidated),
        "StaleCandidate: the receipt does not validate this sealed candidate"
    );
    r.never_accepted();

    assert_eq!(r.validate(810), "PASS");
    assert!(r.reasons().is_empty());
    assert_eq!(r.detail(), "validation command exited with code 0");
    assert_eq!(r.validation("candidate"), text(r.candidate.clone()));
    let report = r.report();
    for line in [
        "validator: jojo-host-validator/1",
        &format!("candidate: {}", r.candidate),
        "network: denied by OS sandbox",
        "test: unit::check Passed",
    ] {
        assert!(report.contains(line), "{line}: {report}");
    }
    assert_eq!(r.rows(), [(IN_SCOPE.to_owned(), "ALLOWED".to_owned())]);
    assert_eq!(get(&r.changes().payload, "scope").unwrap(), &text("PASS"));
    let diff = r.diff();
    assert!(diff.contains("untracked new file src/feature.rs"), "{diff}");
    assert!(diff.contains("+written by fixture"), "{diff}");

    // Validation ran in its own directory under the data root, never in the developer
    // worktree, and the source repository is untouched.
    let worktree = r.h.worktree(900).unwrap().path;
    let copy = r.h.base.join("validation").join(hex(810)).join("tree");
    assert!(copy.join(IN_SCOPE).is_file());
    assert!(!copy.starts_with(&worktree) && !worktree.starts_with(&copy));
    r.source_untouched();

    // The same command again repeats nothing; a lost response is found by `Receipt`.
    let again = r.validate_frame(810);
    assert_eq!(
        get(&again.payload, "validation").unwrap(),
        &text("not repeated")
    );
    assert_eq!(get(&again.payload, "replayed").unwrap(), &Json::Bool(true));
    assert_eq!(r.h.events("ValidationRequested"), 1);
    assert_eq!(r.h.events("ValidationRecorded"), 1);
    let found =
        r.h.send("Receipt", object([("command", identity(OpaqueId(810)))]));
    assert_eq!(
        string(get(&found.payload, "response").unwrap()).unwrap(),
        format!("validation requested attempt={}", hex(900))
    );

    // After a helper restart the verdict is still the durable one, and the candidate can
    // be validated again in the new epoch.
    r.h.open(Some(r.h.codex()), Some(ps()), None);
    assert_eq!(r.state(), "PASS");
    assert_eq!(r.validate(811), "PASS");
    let receipt = r.receipt();

    // Acceptance names the candidate and the receipt that were reviewed.
    let mut wrong = r.subject();
    wrong[1] = ("candidate", text("0".repeat(64)));
    wrong.push(("receipt", receipt.clone()));
    wrong.push(("action", text("Accept")));
    let stale = r.h.frame(820, 1, "CandidateDecision", object(wrong));
    assert!(code(&stale).starts_with("CandidateChanged: "), "{stale:?}");
    let older = r.decide(821, "Accept", number(1));
    assert!(code(&older).starts_with("StaleCandidate"), "{older:?}");
    r.never_accepted();
    let accepted = r.decide(822, "Accept", receipt.clone());
    assert_eq!(
        string(get(&accepted.payload, "response").unwrap()).unwrap(),
        "candidate accepted"
    );
    let replay = r.decide(822, "Accept", receipt.clone());
    assert_eq!(get(&replay.payload, "replayed").unwrap(), &Json::Bool(true));
    assert_eq!(r.h.events("CandidateAccepted"), 1);
    assert_eq!(
        string(get(&r.view(), "decision").unwrap()).unwrap(),
        "ACCEPTED"
    );
    // Decided is final.
    assert!(code(&r.decide(823, "Reject", Json::Null)).starts_with("CandidateDecided"));
    assert!(code(&r.validate_frame(824)).starts_with("CandidateDecided"));
    r.source_untouched();
}

#[test]
fn p2_03_out_of_scope_write_fails_scope_and_the_command_is_never_run() {
    let h = H::new();
    let marker = h.base.join("command-ran");
    let touch = ["/usr/bin/touch", marker.to_str().unwrap()];
    let mut r = run_in(h, "README.md", &touch);
    assert_eq!(r.validate(810), "FAIL");
    assert!(
        r.reasons()
            .contains(&"out-of-scope change \"README.md\"".to_owned()),
        "{:?}",
        r.reasons()
    );
    assert!(r.detail().contains("was not run"), "{}", r.detail());
    assert!(!marker.exists(), "the validation command was executed");
    assert_eq!(
        r.rows(),
        [("README.md".to_owned(), "OUT_OF_SCOPE".to_owned())]
    );
    assert_eq!(get(&r.changes().payload, "scope").unwrap(), &text("FAIL"));
    let receipt = r.receipt();
    assert_eq!(
        code(&r.decide(820, "Accept", receipt)),
        "only the latest passing validation can be accepted"
    );
    r.never_accepted();
    // A human may still reject it, once.
    let rejected = r.decide(821, "Reject", Json::Null);
    assert_eq!(rejected.kind, "Response", "{:?}", rejected.payload);
    assert_eq!(
        string(get(&r.view(), "decision").unwrap()).unwrap(),
        "REJECTED"
    );
    assert!(code(&r.decide(822, "Reject", Json::Null)).starts_with("CandidateDecided"));
    r.source_untouched();
}

#[test]
fn p2_03_protected_baseline_test_modification_fails_and_the_command_is_never_run() {
    let h = H::new();
    let marker = h.base.join("command-ran");
    let touch = ["/usr/bin/touch", marker.to_str().unwrap()];
    let mut r = run_in(h, "tests/check.rs", &touch);
    assert_eq!(r.validate(810), "FAIL");
    assert!(
        r.reasons()
            .contains(&"baseline modified \"tests/check.rs\"".to_owned()),
        "{:?}",
        r.reasons()
    );
    assert!(!marker.exists(), "the validation command was executed");
    assert_eq!(
        r.rows(),
        [("tests/check.rs".to_owned(), "BASELINE".to_owned())]
    );
    let receipt = r.receipt();
    assert_eq!(
        code(&r.decide(820, "Accept", receipt)),
        "only the latest passing validation can be accepted"
    );
    r.never_accepted();
}

#[test]
fn p2_03_failing_validation_command_fails_closed() {
    let mut r = run(IN_SCOPE, &["/usr/bin/false"]);
    assert_eq!(r.validate(810), "FAIL");
    assert_eq!(r.reasons(), ["test \"unit::check\" Failed"]);
    assert_eq!(r.detail(), "validation command exited with code 1");
    assert!(r.report().contains("test: unit::check Failed"));
    let receipt = r.receipt();
    assert_eq!(
        code(&r.decide(820, "Accept", receipt)),
        "only the latest passing validation can be accepted"
    );
    r.never_accepted();
}

#[test]
fn p2_03_hung_validator_is_stopped_by_its_budget_and_fails_closed() {
    let mut r = run(IN_SCOPE, &["/bin/sleep", "600"]);
    r.h.s().validation_budget = budget(300);
    let began = std::time::Instant::now();
    assert_eq!(r.validate(810), "FAIL");
    assert!(began.elapsed() < std::time::Duration::from_secs(15));
    assert!(
        r.detail()
            .contains("exceeded its 300 ms budget and was stopped"),
        "{}",
        r.detail()
    );
    assert_eq!(r.reasons(), ["test \"unit::check\" Failed"]);
    let receipt = r.receipt();
    assert_eq!(
        code(&r.decide(820, "Accept", receipt)),
        "only the latest passing validation can be accepted"
    );
    r.never_accepted();
}

#[test]
fn p2_03_tampered_sealed_candidate_is_never_validated_or_accepted() {
    let mut r = run(IN_SCOPE, CHECK);
    let candidate = r.candidate.clone();
    let packet =
        r.h.read(|v| v.candidate_reference(OpaqueId(900), &candidate))
            .unwrap()
            .path;
    chmod(&packet, 0o600);
    let mut bytes = fs::read(&packet).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(&packet, bytes).unwrap();

    assert_eq!(r.validate(810), "UNKNOWN");
    assert!(
        r.detail().contains("sealed candidate packet rejected"),
        "{}",
        r.detail()
    );
    assert_eq!(r.receipt(), Json::Null);
    assert_eq!(r.h.events("ValidationRecorded"), 0);
    assert!(code(&r.changes()).starts_with("CandidateUnavailable: sealed candidate packet"));
    assert!(code(&r.decide(820, "Accept", number(1))).starts_with("CandidateChanged: "));
    r.never_accepted();
}

#[test]
fn p2_03_candidate_changed_after_validation_cannot_be_accepted() {
    let mut r = run(IN_SCOPE, CHECK);
    assert_eq!(r.validate(810), "PASS");
    let receipt = r.receipt();
    // The retained worktree no longer reproduces what was sealed and validated.
    let worktree = r.h.worktree(900).unwrap().path;
    fs::write(worktree.join(IN_SCOPE), "changed after validation\n").unwrap();
    let refused = r.decide(820, "Accept", receipt.clone());
    assert!(
        code(&refused).starts_with("CandidateChanged: worktree does not reproduce"),
        "{refused:?}"
    );
    assert!(code(&r.changes()).starts_with("CandidateUnavailable: "));
    // Validating again gives no verdict either, and the earlier pass is not current.
    assert_eq!(r.validate(811), "UNKNOWN");
    fs::write(worktree.join(IN_SCOPE), "written by fixture\n").unwrap();
    assert_eq!(
        code(&r.decide(821, "Accept", receipt)),
        "only the latest passing validation can be accepted"
    );
    r.never_accepted();
}

#[test]
fn p2_03_stale_approval_refuses_validation_and_acceptance() {
    let mut r = run(IN_SCOPE, CHECK);
    assert_eq!(r.validate(810), "PASS");
    let receipt = r.receipt();
    assert_eq!(r.h.decide(PRD, "Revoke").kind, "Response");
    assert_eq!(
        code(&r.decide(820, "Accept", receipt)),
        "validation is stale against current approvals"
    );
    assert_eq!(
        code(&r.validate_frame(811)),
        "approval is stale, revoked or invalidated"
    );
    assert_eq!(r.h.events("ValidationRequested"), 1);
    r.never_accepted();
}

#[test]
fn p2_03_validate_refuses_stale_candidates_extra_fields_old_clients_and_plans_without_a_command() {
    // The P2-02 plan shape: no command. Nothing can be validated, so nothing can pass.
    let mut r = run(IN_SCOPE, &[]);
    assert_eq!(
        code(&r.validate_frame(810)),
        "approved validation plan names no command for this step"
    );
    assert_eq!(r.state(), "NOT_RUN");
    let mut other = r.subject();
    other[1] = ("candidate", text("0".repeat(64)));
    assert_eq!(
        code(&r.h.frame(811, 1, "Validate", object(other))),
        "StaleCandidate: this is not the sealed candidate"
    );
    // No executable, argument or path can be sent with the request.
    let mut extra = r.subject();
    extra.push(("command", text("/bin/sh")));
    assert_eq!(
        code(&r.h.frame(812, 1, "Validate", object(extra))),
        "InvalidPayload: unexpected or missing fields"
    );
    let payload = object(r.subject());
    assert_eq!(code(&r.h.frame(813, 0, "Validate", payload)), "UnknownType");
    let unknown = object([
        ("attempt", identity(OpaqueId(12345))),
        ("candidate", text("0".repeat(64))),
    ]);
    assert_eq!(
        code(&r.h.frame(814, 1, "Validate", unknown)),
        "validation needs a sealed attempt result"
    );
    assert_eq!(r.h.events("ValidationRequested"), 0);
    r.never_accepted();
}

#[test]
fn p2_03_closing_while_validation_is_active_stops_the_validator_and_leaves_no_verdict() {
    let mut r = run(IN_SCOPE, &["/bin/sleep", "600"]);
    let started = r.validate_frame(810);
    assert_eq!(
        get(&started.payload, "validation").unwrap(),
        &text("started")
    );
    assert_eq!(r.state(), "UNKNOWN");
    assert_eq!(r.detail(), "validation is running");
    // One at a time, and a running validation is never a pass.
    assert_eq!(
        code(&r.validate_frame(811)),
        "CapacityOne: a run or validation is active"
    );
    assert!(code(&r.decide(820, "Accept", number(1))).starts_with("StaleCandidate"));
    // Wait until the validator process exists, then the human closes the app.
    let spool = r.h.base.join("validation").join(hex(810)).join("spool");
    until(|| spool.is_dir());
    let began = std::time::Instant::now();
    let prepared = r.h.send("PrepareClose", object([]));
    assert_eq!(prepared.kind, "Response", "{:?}", prepared.payload);
    let status = r.h.send("CloseStatus", object([]));
    let ticket = get(&status.payload, "ticket").unwrap().clone();
    let confirmed = r.h.send("ConfirmClose", object([("ticket", ticket)]));
    assert_eq!(confirmed.kind, "Response", "{:?}", confirmed.payload);
    let mut receipt = None;
    until(|| {
        receipt = r.h.s().finish(false).unwrap();
        receipt.is_some()
    });
    assert!(began.elapsed() < std::time::Duration::from_secs(15));
    let receipt = receipt.unwrap();
    assert!(receipt.process_stop_confirmed && receipt.persistence_sealed);
    assert!(!r.h.s().workers.failed());

    // Reopened: no verdict was recorded and nothing can be accepted.
    r.h.open(Some(r.h.codex()), Some(ps()), None);
    assert_eq!(r.state(), "UNKNOWN");
    assert!(r.detail().contains("interrupted"), "{}", r.detail());
    assert_eq!(r.receipt(), Json::Null);
    assert_eq!(r.h.events("ValidationRecorded"), 0);
    assert!(code(&r.decide(821, "Accept", number(1))).starts_with("StaleCandidate"));
    r.never_accepted();
    r.source_untouched();
}

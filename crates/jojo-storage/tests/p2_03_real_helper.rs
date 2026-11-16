//! P2-03, and the P2-02 limitation it closes: the whole slice through the real helper
//! executable and real pipes. Registration, import, approval, Start, supervised launch and
//! settlement, host validation, inspection and human acceptance are all protocol frames
//! sent to a `jojo-helper --serve` process; this test never opens the database.
//!
//! The helper inspects processes with the system `ps`, which the OS sandbox of the
//! verification gate cannot execute, so this test is ignored by default and the gate runs
//! it in a separate step outside that sandbox. It needs no network: the "Codex" is a
//! private copy of `jojo-codex-fixture`, and the validation command passes only if the
//! validator's own sandbox denies it a socket connection.
mod common;
use common::TempDir;
use common::attempts::git;
use jojo_domain::codex::CODEX_VERSION;
use jojo_domain::identity::OpaqueId;
use jojo_domain::ipc::*;
use jojo_domain::json::Json;
use jojo_domain::validation::{ScopePath, StepValidation, ValidationPlan};
use jojo_storage::sha256_hex;
use jojo_test_support::{COMPLETE_PRD_JSON, dependency::node};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const CODEX_FIXTURE: &str = env!("CARGO_BIN_EXE_jojo-codex-fixture");
/// Exit 0 only if the candidate file is present and a loopback connection is refused by
/// the operating system (not merely unanswered).
const PROBE: &str = "import errno, socket, sys\n\
s = socket.socket()\n\
r = s.connect_ex(('127.0.0.1', 9))\n\
ok = open('src/feature.rs').read() == 'written by fixture\\n'\n\
sys.exit(0 if ok and r in (errno.EPERM, errno.EACCES) else 1)\n";

struct Pipe {
    child: Child,
    rx: mpsc::Receiver<Envelope>,
    epoch: u64,
    next: u128,
}

impl Pipe {
    fn frame(&mut self, kind: &str, payload: Json, revision: Option<u64>) -> Envelope {
        self.next += 1;
        self.frame_as(self.next, kind, payload, revision)
    }

    fn frame_as(
        &mut self,
        request: u128,
        kind: &str,
        payload: Json,
        revision: Option<u64>,
    ) -> Envelope {
        let e = Envelope {
            major: 1,
            minor: 1,
            session: OpaqueId(7),
            epoch: self.epoch,
            request: OpaqueId(request),
            revision,
            kind: kind.into(),
            payload,
        };
        let stdin = self.child.stdin.as_mut().unwrap();
        stdin.write_all(encode(&e.json()).as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
        let reply = self.rx.recv_timeout(Duration::from_secs(30)).unwrap();
        assert_eq!(reply.request, e.request, "{kind}: {reply:?}");
        reply
    }

    fn ok(&mut self, kind: &str, payload: Json) -> Json {
        let r = self.frame(kind, payload, None);
        assert_eq!(r.kind, "Response", "{kind}: {:?}", r.payload);
        r.payload
    }

    fn approve(&mut self, subject: u128) {
        let bundle = self.ok(
            "ReviewBundle",
            object([("subject", identity(OpaqueId(subject)))]),
        );
        let revision = uint(get(&bundle, "revision").unwrap()).unwrap();
        let Json::Object(mut fields) = bundle else {
            panic!("review bundle")
        };
        fields.retain(|(k, _)| k != "revision");
        fields.push(("action".into(), text("Approve")));
        let r = self.frame("HumanDecision", Json::Object(fields), Some(revision));
        assert_eq!(r.kind, "Response", "{:?}", r.payload);
    }

    fn candidate(&mut self) -> Json {
        self.ok("Candidate", object([("attempt", identity(OpaqueId(900)))]))
    }

    fn until(&mut self, what: &str, mut done: impl FnMut(&Json) -> bool) -> Json {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let view = self.candidate();
            if done(&view) {
                return view;
            }
            assert!(Instant::now() < deadline, "{what}: {view:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn state(view: &Json) -> &str {
    string(get(get(view, "validation").unwrap(), "state").unwrap()).unwrap()
}

fn private(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
#[ignore = "needs the system ps; the P2-03 gate runs it outside the OS network sandbox"]
fn p2_03_real_helper_runs_start_to_settlement_validation_and_acceptance_over_pipes() {
    let dir = TempDir::new("p2-03-real");
    let base = fs::canonicalize(dir.path()).unwrap();
    private(&base.join("disposable"));
    let repository = base.join("disposable/sample");
    for d in ["src", "tests"] {
        fs::create_dir_all(repository.join(d)).unwrap();
    }
    git(
        &repository,
        &["init", "--template=", "--initial-branch=main"],
    );
    fs::write(repository.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
    fs::write(repository.join("tests/check.rs"), "fn check() {}\n").unwrap();
    git(&repository, &["add", "--all"]);
    git(&repository, &["commit", "-m", "sample"]);
    let commit = git(&repository, &["rev-parse", "HEAD"]);
    fs::create_dir(base.join("bin")).unwrap();
    let fixture = base.join("bin/codex");
    fs::copy(CODEX_FIXTURE, &fixture).unwrap();
    private(&base.join("codex-home"));
    let helper = Path::new(CODEX_FIXTURE).with_file_name("jojo-helper");
    assert!(helper.is_file(), "build the workspace first: {helper:?}");

    let mut child = Command::new(&helper)
        .args(["--serve", "--data-root"])
        .arg(base.join("data"))
        .arg("--worktree-root")
        .arg(base.join("worktrees"))
        .arg("--disposable-root")
        .arg(base.join("disposable"))
        .arg("--codex-executable")
        .arg(&fixture)
        .args(["--codex-version", CODEX_VERSION, "--codex-sha256"])
        .arg(sha256_hex(&[&fs::read(&fixture).unwrap()]))
        .arg("--codex-home")
        .arg(base.join("codex-home"))
        .args(["--codex-auth", "native-subscription"])
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let e = Envelope::parse(line.as_bytes()).expect("protocol frames only");
            if tx.send(e).is_err() {
                break;
            }
        }
    });
    let mut p = Pipe {
        child,
        rx,
        epoch: 0,
        next: 10,
    };
    let hello = p.frame("Hello", object([]), None);
    assert_eq!(hello.kind, "Response");
    p.epoch = hello.epoch;
    assert_eq!(get(&hello.payload, "slice").unwrap(), &Json::Bool(true));
    for name in ["Validate", "CandidateDecision"] {
        let commands = string(get(&hello.payload, "commands").unwrap()).unwrap();
        assert!(commands.split(' ').any(|c| c == name), "{name}");
    }

    let registered = p.ok(
        "RegisterRepository",
        object([("path", text(repository.display().to_string()))]),
    );
    let repository_id = string(get(&registered, "response").unwrap())
        .unwrap()
        .to_owned();
    let validation = ValidationPlan {
        steps: vec![StepValidation {
            step: node(1),
            write_scope: vec![ScopePath::parse("src").unwrap()],
            baseline: vec![ScopePath::parse("tests").unwrap()],
            tests: vec!["probe".into()],
            command: vec!["/usr/bin/python3".into(), "-c".into(), PROBE.into()],
        }],
    };
    let bundle = object([
        ("feature", identity(OpaqueId(700))),
        ("prd_id", identity(OpaqueId(701))),
        ("adr_id", identity(OpaqueId(702))),
        ("plan_id", identity(OpaqueId(703))),
        ("validation_id", identity(OpaqueId(704))),
        ("prd", text(COMPLETE_PRD_JSON)),
        (
            "adr",
            text("Decision.\nfixture: develop src/feature.rs\nEnd."),
        ),
        ("step", identity(OpaqueId(1))),
        ("repository", text(repository_id)),
        ("base", text(commit.clone())),
        ("provider", text("Codex")),
        ("validation", text(validation.canonical())),
    ]);
    for subjects in [vec![701], vec![702], vec![704, 703]] {
        p.ok("ImportBundle", bundle.clone());
        for subject in subjects {
            p.approve(subject);
        }
    }
    let inspection = p.ok("RecoveryInspect", object([]));
    let digest = get(&inspection, "digest").unwrap().clone();
    p.ok("RecoveryStart", object([("digest", digest)]));

    // One human Start: the real helper launches the fixture under its own supervision.
    let start = object([
        ("prd", identity(OpaqueId(701))),
        ("adr", identity(OpaqueId(702))),
        ("plan", identity(OpaqueId(703))),
        ("validation", identity(OpaqueId(704))),
        ("step", identity(OpaqueId(1))),
    ]);
    let started = p.frame_as(900, "Start", start.clone(), None);
    assert_eq!(started.kind, "Response", "{:?}", started.payload);
    assert_eq!(
        get(&started.payload, "dispatch").unwrap(),
        &text("launched")
    );
    let again = p.frame_as(900, "Start", start, None);
    assert_eq!(
        get(&again.payload, "dispatch").unwrap(),
        &text("not repeated")
    );
    let sealed = p.until("settlement", |v| get(v, "sealed") == Ok(&Json::Bool(true)));
    assert_eq!(
        string(get(&sealed, "outcome").unwrap()).unwrap(),
        "Succeeded"
    );
    assert_eq!(state(&sealed), "NOT_RUN");
    let candidate = get(&sealed, "candidate").unwrap().clone();
    let log = p.ok(
        "Log",
        object([
            ("attempt", identity(OpaqueId(900))),
            ("sequence", number(1)),
            ("offset", number(0)),
        ]),
    );
    assert_ne!(log, Json::Null);

    // Host validation in the real helper: its own sandbox must deny the probe's socket.
    let subject = || {
        vec![
            ("attempt", identity(OpaqueId(900))),
            ("candidate", candidate.clone()),
        ]
    };
    let requested = p.frame_as(910, "Validate", object(subject()), None);
    assert_eq!(requested.kind, "Response", "{:?}", requested.payload);
    let passed = p.until("validation", |v| !matches!(state(v), "UNKNOWN" | "NOT_RUN"));
    assert_eq!(state(&passed), "PASS", "{passed:?}");
    let receipt = get(get(&passed, "validation").unwrap(), "receipt")
        .unwrap()
        .clone();
    let mut page = subject();
    page.extend([("offset", number(0)), ("limit", number(8))]);
    let changes = p.ok("CandidateChanges", object(page));
    assert_eq!(get(&changes, "scope").unwrap(), &text("PASS"));
    assert_eq!(get(&changes, "total").unwrap(), &number(1));

    let mut decision = subject();
    decision.extend([("receipt", receipt), ("action", text("Accept"))]);
    let accepted = p.frame_as(920, "CandidateDecision", object(decision), None);
    assert_eq!(accepted.kind, "Response", "{:?}", accepted.payload);
    let view = p.candidate();
    assert_eq!(string(get(&view, "decision").unwrap()).unwrap(), "ACCEPTED");

    // The sample repository was never written to.
    assert_eq!(git(&repository, &["status", "--porcelain"]), "");
    assert_eq!(git(&repository, &["rev-parse", "HEAD"]), commit);

    // A clean close: the helper exits on its own after a READY shutdown.
    // Nothing is running, so the close needs no confirmation.
    p.ok("PrepareClose", object([]));
    let ready = p.rx.recv_timeout(Duration::from_secs(30)).unwrap();
    assert_eq!(ready.kind, "ShutdownReady", "{:?}", ready.payload);
    let deadline = Instant::now() + Duration::from_secs(30);
    let exit = loop {
        if let Some(status) = p.child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "helper did not exit");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(exit.success(), "{exit:?}");
}

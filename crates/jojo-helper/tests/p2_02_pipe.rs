//! P2-02 over the real helper binary and real pipes: protocol 1.1 negotiation, refusals
//! and the reviewed start, with the slice options given on the helper command line. No
//! provider executable is configured here and nothing is launched.
use jojo_domain::{identity::OpaqueId, ipc::*, json::Json};
use jojo_storage::*;
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Pipe {
    child: Child,
    rx: mpsc::Receiver<Envelope>,
    root: PathBuf,
    epoch: u64,
    next: u128,
}
impl Pipe {
    fn start(slice: bool) -> Self {
        let root = std::env::temp_dir()
            .join(format!(
                "jojo-p2-02-pipe-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ))
            .canonicalize_parent();
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("disposable")).unwrap();
        std::fs::set_permissions(
            root.join("disposable"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_jojo-helper"));
        command
            .args(["--serve", "--data-root"])
            .arg(root.join("data"))
            .arg("--worktree-root")
            .arg(root.join("worktrees"));
        if slice {
            command
                .arg("--disposable-root")
                .arg(root.join("disposable"));
        }
        let mut child = command
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
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
        Self {
            child,
            rx,
            root,
            epoch: 0,
            next: 1,
        }
    }
    fn send(&mut self, minor: u64, kind: &str, payload: Json) -> Envelope {
        self.next += 1;
        let e = Envelope {
            major: 1,
            minor,
            session: OpaqueId(1),
            epoch: self.epoch,
            request: OpaqueId(self.next),
            revision: None,
            kind: kind.into(),
            payload,
        };
        let stdin = self.child.stdin.as_mut().unwrap();
        stdin.write_all(encode(&e.json()).as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
        let reply = self.rx.recv_timeout(Duration::from_secs(20)).unwrap();
        assert_eq!(reply.request, e.request);
        reply
    }
    fn hello(&mut self, minor: u64) -> Envelope {
        let e = self.send(minor, "Hello", object([]));
        assert_eq!(e.kind, "Response");
        self.epoch = e.epoch;
        e
    }
    fn code(&mut self, kind: &str, payload: Json) -> String {
        let e = self.send(1, kind, payload);
        assert_eq!(e.kind, "Error", "{kind}: {:?}", e.payload);
        string(get(&e.payload, "code").unwrap()).unwrap().to_owned()
    }
}
trait CanonicalParent {
    fn canonicalize_parent(self) -> PathBuf;
}
impl CanonicalParent for PathBuf {
    /// The temporary directory resolved, so path comparisons are on real paths.
    fn canonicalize_parent(self) -> PathBuf {
        let name = self.file_name().unwrap().to_owned();
        self.parent().unwrap().canonicalize().unwrap().join(name)
    }
}
impl Drop for Pipe {
    fn drop(&mut self) {
        self.child.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.child.try_wait().unwrap().is_none() {
            if Instant::now() > deadline {
                self.child.kill().unwrap();
                self.child.wait().unwrap();
                break;
            }
            std::thread::yield_now();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn start() -> Json {
    object([
        ("prd", identity(OpaqueId(701))),
        ("adr", identity(OpaqueId(702))),
        ("plan", identity(OpaqueId(703))),
        ("validation", identity(OpaqueId(704))),
        ("step", identity(OpaqueId(1))),
    ])
}

#[test]
fn p2_02_real_pipe_negotiates_1_1_and_keeps_1_0_clients_on_1_0() {
    let mut p = Pipe::start(true);
    let hello = p.hello(1);
    assert_eq!((hello.major, hello.minor), (1, 1));
    assert_eq!(
        get(&hello.payload, "compatible").unwrap(),
        &Json::Bool(true)
    );
    assert_eq!(get(&hello.payload, "slice").unwrap(), &Json::Bool(true));
    let commands = string(get(&hello.payload, "commands").unwrap()).unwrap();
    for name in [
        "RegisterRepository",
        "ImportBundle",
        "Start",
        "RecoveryResolve",
        "RecoveryStart",
    ] {
        assert!(commands.split(' ').any(|c| c == name), "{name}");
    }
    // A 1.0 frame gets 1.0 answers: the same lists as before and no new command.
    let old = p.send(0, "Hello", object([]));
    assert_eq!((old.major, old.minor), (1, 0));
    assert_eq!(
        string(get(&old.payload, "commands").unwrap()).unwrap(),
        "PrepareClose CancelClose ConfirmClose CancelAttempt HumanDecision"
    );
    let refused = p.send(0, "Start", start());
    assert_eq!((refused.kind.as_str(), refused.minor), ("Error", 0));
    assert_eq!(
        string(get(&refused.payload, "code").unwrap()).unwrap(),
        "UnknownType"
    );
    // A later minor is diagnostics only.
    let newer = p.send(5, "Hello", object([]));
    assert_eq!(
        get(&newer.payload, "compatible").unwrap(),
        &Json::Bool(false)
    );
    assert_eq!(
        p.code("Start", start()),
        "ProtocolMismatch: diagnostics only"
    );
}

#[test]
fn p2_02_real_pipe_refuses_start_until_reviewed_and_never_launches() {
    let mut p = Pipe::start(true);
    p.hello(1);
    assert_eq!(p.code("Start", start()), "AdmissionClosed");
    // Paths outside the configured disposable root, whatever they are.
    for path in ["/", "/usr/bin", "/tmp", "relative/path"] {
        let code = p.code("RegisterRepository", object([("path", text(path))]));
        assert!(
            code.starts_with("RepositoryOutsideDisposableRoot"),
            "{path}: {code}"
        );
    }
    let outside = p.root.join("data").display().to_string();
    assert_eq!(
        p.code("RegisterRepository", object([("path", text(outside))])),
        "RepositoryOutsideDisposableRoot"
    );
    // The human reviews a fresh inspection; only then is admission open.
    let stale = object([("digest", text("0".repeat(64)))]);
    assert_eq!(
        p.code("RecoveryStart", stale),
        "StaleInspection: inspect again"
    );
    let inspection = p.send(1, "RecoveryInspect", object([]));
    assert_eq!(get(&inspection.payload, "subjects").unwrap(), &number(0));
    let digest = string(get(&inspection.payload, "digest").unwrap())
        .unwrap()
        .to_owned();
    let opened = p.send(1, "RecoveryStart", object([("digest", text(digest))]));
    assert_eq!(opened.kind, "Response", "{:?}", opened.payload);
    // Open admission is not approval: nothing is imported, so nothing can start.
    assert_eq!(
        p.code("Start", start()),
        "StartRefused: missing or stale approval: PRD"
    );
    let mut extra = start();
    if let Json::Object(fields) = &mut extra {
        fields.push(("executable".into(), text("/bin/sh")));
    }
    assert_eq!(
        p.code("Start", extra),
        "InvalidPayload: unexpected or missing fields"
    );
    drop(p.child.stdin.take());
    let deadline = Instant::now() + Duration::from_secs(20);
    while p.child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "helper did not exit");
        std::thread::yield_now();
    }
    let e = Engine::start(EngineConfig::new(
        p.root.join("data/jojo.sqlite3"),
        std::sync::Arc::new(jojo_helper::SystemClock::new()),
    ))
    .unwrap();
    let events = e.reader().unwrap().events_after(EventSeq(0), 1000).unwrap();
    assert!(events.iter().any(|e| e.kind == "RecoveryStarted"));
    assert!(!events.iter().any(|e| e.kind == "ProcessLaunchIssued"
        || e.kind == "LocalRepositoryRegistered"
        || e.kind == "SchedulerReady"));
}

#[test]
fn p2_02_real_pipe_without_slice_options_offers_no_slice() {
    let mut p = Pipe::start(false);
    let hello = p.hello(1);
    assert_eq!(get(&hello.payload, "slice").unwrap(), &Json::Bool(false));
    assert_eq!(
        p.code("RegisterRepository", object([("path", text("/tmp"))])),
        "SliceNotConfigured"
    );
    let inspection = p.send(1, "RecoveryInspect", object([]));
    let digest = string(get(&inspection.payload, "digest").unwrap())
        .unwrap()
        .to_owned();
    assert_eq!(
        p.send(1, "RecoveryStart", object([("digest", text(digest))]))
            .kind,
        "Response"
    );
    assert_eq!(p.code("Start", start()), "SliceNotConfigured");
}

//! Real stdio protocol, singleton, EOF and crash boundaries. No GUI or provider launch.
use jojo_domain::{identity::OpaqueId, ipc::*, json::Json};
use jojo_storage::*;
use std::{
    io::{BufRead, BufReader, Write},
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
}
impl Pipe {
    fn start() -> Self {
        let root = std::env::temp_dir().join(format!(
            "jojo-t13-pipe-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&root).unwrap();
        Self::at(root)
    }
    fn at(root: PathBuf) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_jojo-helper"))
            .args(["--serve", "--data-root"])
            .arg(root.join("data"))
            .arg("--worktree-root")
            .arg(root.join("worktrees"))
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        let e = Envelope::parse(line.as_bytes())
                            .expect("stdout must be bounded protocol only");
                        if tx.send(e).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Self { child, rx, root }
    }
    fn send(&mut self, n: u128, epoch: u64, kind: &str, payload: Json) {
        let e = Envelope {
            major: 1,
            minor: 0,
            session: OpaqueId(1),
            epoch,
            request: OpaqueId(n),
            revision: None,
            kind: kind.into(),
            payload,
        };
        let b = encode(&e.json());
        let stdin = self.child.stdin.as_mut().unwrap();
        for chunk in b.as_bytes().chunks(7) {
            stdin.write_all(chunk).unwrap();
        }
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    }
    fn recv(&self) -> Envelope {
        self.rx.recv_timeout(Duration::from_secs(20)).unwrap()
    }
    fn hello(&mut self) -> u64 {
        self.send(1, 0, "Hello", object([]));
        let e = self.recv();
        assert_eq!(e.kind, "Response");
        e.epoch
    }
    fn wait(&mut self) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code().unwrap_or(-1);
            }
            assert!(Instant::now() < deadline, "helper did not exit");
            std::thread::yield_now();
        }
    }
}
impl Drop for Pipe {
    fn drop(&mut self) {
        self.child.stdin.take();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self.child.try_wait().unwrap().is_some() {
                break;
            }
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
#[test]
fn t13_real_pipe_idle_ready_is_flushed_before_helper_exit() {
    let mut p = Pipe::start();
    let epoch = p.hello();
    p.send(2, epoch, "PrepareClose", object([]));
    let ack = p.recv();
    assert_eq!(
        get(&ack.payload, "acknowledged").unwrap(),
        &Json::Bool(true)
    );
    assert_eq!(
        get(&ack.payload, "operation_completed").unwrap(),
        &Json::Bool(false)
    );
    let ready = p.recv();
    assert_eq!(ready.kind, "ShutdownReady");
    assert_eq!(
        get(&ready.payload, "persistence_sealed").unwrap(),
        &Json::Bool(true)
    );
    assert_eq!(p.wait(), 0);
    let e = Engine::start(EngineConfig::new(
        p.root.join("data/jojo.sqlite3"),
        std::sync::Arc::new(jojo_helper::SystemClock::new()),
    ))
    .unwrap();
    assert_eq!(
        e.startup_report().epoch.previous.unwrap().1,
        ShutdownMarker::Clean
    );
}
#[test]
fn t13_real_pipe_eof_records_emergency_and_never_ready() {
    let mut p = Pipe::start();
    p.hello();
    p.child.stdin.take();
    assert_eq!(p.wait(), 0);
    assert!(p.rx.try_iter().all(|e| e.kind != "ShutdownReady"));
    let e = Engine::start(EngineConfig::new(
        p.root.join("data/jojo.sqlite3"),
        std::sync::Arc::new(jojo_helper::SystemClock::new()),
    ))
    .unwrap();
    assert_ne!(
        e.startup_report().epoch.previous.unwrap().1,
        ShutdownMarker::Clean
    );
    assert_eq!(
        e.reader().unwrap().snapshot().unwrap().session.admission,
        Admission::Closed
    );
}
#[test]
fn t13_real_pipe_singleton_blocks_replacement_and_crash_has_no_ready() {
    let mut p = Pipe::start();
    let epoch = p.hello();
    let mut other = Command::new(env!("CARGO_BIN_EXE_jojo-helper"))
        .args(["--serve", "--data-root"])
        .arg(p.root.join("data"))
        .arg("--worktree-root")
        .arg(p.root.join("worktrees"))
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    assert_eq!(other.wait().unwrap().code(), Some(3));
    // A crash, not a close: keep stdin open until the helper is reaped so it can never
    // observe EOF (and begin an orderly emergency stop) before SIGKILL takes effect.
    let stdin = p.child.stdin.take();
    p.child.kill().unwrap();
    p.child.wait().unwrap();
    drop(stdin);
    assert!(p.rx.try_iter().all(|e| e.kind != "ShutdownReady"));
    let e = Engine::start(EngineConfig::new(
        p.root.join("data/jojo.sqlite3"),
        std::sync::Arc::new(jojo_helper::SystemClock::new()),
    ))
    .unwrap();
    assert!(e.epoch().0 > epoch);
    assert_ne!(
        e.startup_report().epoch.previous.unwrap().1,
        ShutdownMarker::Clean
    );
}
#[test]
fn t13_bulk_query_flood_cannot_starve_close_control() {
    let mut p = Pipe::start();
    let epoch = p.hello();
    for n in 2..14 {
        p.send(
            n,
            epoch,
            "Events",
            object([("after", number(0)), ("limit", number(8))]),
        );
    }
    p.send(14, epoch, "PrepareClose", object([]));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let e = p.recv();
        if e.kind == "ShutdownReady" {
            break;
        }
        assert!(Instant::now() < deadline);
    }
    assert_eq!(p.wait(), 0);
}
#[test]
fn t13_invalid_frame_stops_admission_without_clean_ack() {
    let mut p = Pipe::start();
    p.hello();
    p.child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"{invalid}\n")
        .unwrap();
    assert_eq!(p.wait(), 0);
    assert!(p.rx.try_iter().all(|e| e.kind != "ShutdownReady"));
}

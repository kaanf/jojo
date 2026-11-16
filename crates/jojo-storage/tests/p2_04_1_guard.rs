//! P2-04.1: an agent must not outlive the process that supervises it. Real OS processes,
//! but only the workspace's own `jojo-process-fixture`, this test binary re-executed as a
//! disposable supervisor, and the workspace's `jojo-helper` in its guard mode (started by
//! the supervisor, never by a test). No provider CLI, credential or network.
//!
//! "The supervisor is killed" is a real SIGKILL through the exact child handle. Whether
//! anything survived is read back from the OS with `inspect_recorded`, the same
//! inspection recovery uses: a process group with any live member is never "absent".
use jojo_domain::stream::CLAUDE_STREAM_JSON;
use jojo_storage::*;
use std::io::{BufRead, BufReader};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const FIXTURE: &str = env!("CARGO_BIN_EXE_jojo-process-fixture");
const ROLE: &str = "JOJO_P2_04_1_ROLE";
const DIR: &str = "JOJO_P2_04_1_DIR";

// Real process tests run one at a time so CPU contention cannot eat the budgets.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn inspector() -> PathBuf {
    std::env::var_os("JOJO_TEST_PS")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/bin/ps".into())
}

/// The guard is the helper executable itself, built next to the fixture.
fn guard() -> PathBuf {
    let helper = Path::new(FIXTURE).with_file_name("jojo-helper");
    assert!(helper.is_file(), "build the workspace first: {helper:?}");
    helper
}

fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "jojo-p2-04-1-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("barriers")).unwrap();
    std::fs::canonicalize(dir).unwrap()
}

fn launch(dir: &Path, scenario: &str) -> ProcessLaunch {
    ProcessLaunch {
        executable: FIXTURE.into(),
        args: vec![dir.join("barriers").display().to_string()],
        cwd: dir.to_owned(),
        environment: vec![],
        stdin: scenario.as_bytes().to_vec(),
        spool: dir.join("spool"),
        parser: CLAUDE_STREAM_JSON.into(),
        limits: StreamLimits {
            max_frame_bytes: 64 * 1024,
            max_output_bytes: 1024 * 1024,
            checkpoint_bytes: CHECKPOINT_BYTES,
        },
    }
}

fn until(what: &str, seconds: u64, mut done: impl FnMut() -> bool) -> Duration {
    let started = Instant::now();
    while !done() {
        assert!(
            started.elapsed() < Duration::from_secs(seconds),
            "timed out: {what}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    started.elapsed()
}

fn show(tag: &str, p: &ProcessIdentity) {
    println!(
        "P2041-ID\t{tag}\t{}\t{}\t{}\t{}\t{}",
        p.pid, p.ppid, p.pgid, p.start, p.boot
    );
}

/// Not a test of its own. Re-executed by the tests below as a disposable process:
///
/// * `supervise <scenario> <guarded|unguarded> <plain|after-term>`: owns one supervised
///   fixture, reports its identities and then does nothing until it is killed;
/// * `guard <leader> <lifeline bytes>`: runs the guard body itself against a named leader.
#[test]
#[ignore = "disposable supervisor or guard role, executed in a child process"]
fn p2_04_1_role() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    let words: Vec<&str> = role.split(' ').collect();
    if words[0] == "none" {
        return;
    }
    if words[0] == "guard" {
        let leader = match words[1] {
            "self" => std::process::id(),
            other => other.parse().unwrap(),
        };
        let code = run_agent_guard(leader, &inspector(), words[2].as_bytes());
        std::process::exit(i32::from(code));
    }
    let dir = PathBuf::from(std::env::var(DIR).unwrap());
    let guard = (words[2] == "guarded").then(guard);
    let mut sup = Supervised::spawn_guarded(
        &launch(&dir, words[1]),
        SpoolFaults::default(),
        &inspector(),
        guard.as_deref(),
    )
    .unwrap();
    // The fixture announces itself on stdout once it has read its scenario.
    loop {
        let message = sup.messages().recv_timeout(Duration::from_secs(30));
        if matches!(message, Ok(ProcessMessage::Output { .. })) {
            break;
        }
        assert!(message.is_ok(), "{message:?}");
    }
    if words[3] == "after-term" {
        // The fixture announces itself just before it becomes the shell that ignores TERM;
        // give that shell time to install its trap.
        std::thread::sleep(Duration::from_secs(1));
        // A graceful stop was begun: TERM to the whole group ends the guard as well. The
        // supervisor must replace it while the agent is still there.
        let sent = sup.signal_tree(Signal::Terminate);
        assert_eq!(sent[0].outcome, SignalOutcome::Sent);
        until("guard replaced after TERM", 10, || sup.guards().len() == 2);
    }
    // Let a descendant the fixture starts become visible, then record what is known.
    std::thread::sleep(Duration::from_millis(300));
    for known in sup.discover().unwrap() {
        show("known", &known);
    }
    show("root", sup.root());
    println!("P2041-READY guards={}", sup.guards().len());
    std::thread::sleep(Duration::from_secs(120));
    std::process::exit(1);
}

struct Supervisor {
    child: Child,
    dir: PathBuf,
    root: ProcessIdentity,
    known: Vec<ProcessIdentity>,
    guards: usize,
}

fn role(value: &str, dir: &Path) -> Command {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args([
        "--exact",
        "p2_04_1_role",
        "--ignored",
        "--nocapture",
        "--test-threads=1",
    ])
    .env(ROLE, value)
    .env(DIR, dir)
    .stdin(Stdio::null())
    .stderr(Stdio::null());
    c
}

fn supervisor(scenario: &str, guarded: bool, mode: &str) -> Supervisor {
    let dir = scratch(scenario);
    let how = if guarded { "guarded" } else { "unguarded" };
    let mut child = role(&format!("supervise {scenario} {how} {mode}"), &dir)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = tx.send(line);
        }
    });
    let (mut root, mut known) = (None, Vec::new());
    let guards = loop {
        let line = rx
            .recv_timeout(Duration::from_secs(60))
            .expect("supervisor role did not become ready");
        if let Some((_, id)) = line.split_once("P2041-ID\t") {
            let f: Vec<&str> = id.split('\t').collect();
            let identity = ProcessIdentity {
                pid: f[1].parse().unwrap(),
                ppid: f[2].parse().unwrap(),
                pgid: f[3].parse().unwrap(),
                start: f[4].into(),
                boot: f[5].into(),
            };
            if f[0] == "root" {
                root = Some(identity);
            } else {
                known.push(identity);
            }
        } else if let Some((_, n)) = line.split_once("P2041-READY guards=") {
            break n.parse().unwrap();
        }
    };
    Supervisor {
        child,
        dir,
        root: root.unwrap(),
        known,
        guards,
    }
}

impl Supervisor {
    /// SIGKILL through the exact child handle: no cleanup code runs in the supervisor.
    fn kill(&mut self) {
        self.child.kill().unwrap();
        let status = self.child.wait().unwrap();
        assert_eq!(status.signal(), Some(9));
    }

    /// The agent's process and its whole group (which includes any guard), from the OS.
    fn group(&self) -> Vec<(String, IdentityCheck)> {
        inspect_recorded(&inspector(), std::slice::from_ref(&self.root))
    }

    fn group_gone(&self) -> bool {
        self.group()
            .iter()
            .all(|(_, c)| *c == IdentityCheck::Absent)
    }

    fn alive(&self, p: &ProcessIdentity) -> bool {
        inspect_recorded(&inspector(), std::slice::from_ref(p))[0].1 == IdentityCheck::Alive
    }

    /// Lets every fixture process that is still waiting on a barrier end by itself.
    fn release(&self) {
        for name in ["release", "release-linger"] {
            std::fs::write(self.dir.join("barriers").join(name), b"").unwrap();
        }
    }
}

#[test]
fn p2_04_1_without_a_guard_the_agent_outlives_a_killed_supervisor() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // The P2-04 finding, reproduced: nothing tells the agent that its supervisor died.
    let mut s = supervisor("wait-release", false, "plain");
    assert_eq!(s.guards, 0);
    s.kill();
    std::thread::sleep(Duration::from_secs(3));
    assert!(s.alive(&s.root), "{:?}", s.group());
    assert!(!s.group_gone());
    // It ends only because the fixture is told to.
    s.release();
    until("released orphan gone", 20, || s.group_gone());
}

#[test]
fn p2_04_1_a_guarded_agent_is_stopped_when_its_supervisor_is_killed() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let mut s = supervisor("wait-release", true, "plain");
    assert_eq!(s.guards, 1);
    assert!(s.alive(&s.root));
    s.kill();
    // Never released: only the guard can have ended it, and the guard ended itself too.
    let took = until("agent and guard gone", 15, || s.group_gone());
    assert!(took < Duration::from_secs(5), "{took:?}");
    assert!(!s.dir.join("barriers/release").exists());
}

#[test]
fn p2_04_1_an_agent_that_ignores_term_is_killed_after_the_grace_period() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // The fixture becomes a shell loop that ignores INT and TERM, with children.
    let mut s = supervisor("ignore-signals", true, "plain");
    s.kill();
    let took = until("term-ignoring agent gone", 20, || s.group_gone());
    // It was given the grace period after TERM, and then the group was killed.
    assert!(
        took > Duration::from_millis(1500) && took < Duration::from_secs(10),
        "{took:?}"
    );
}

#[test]
fn p2_04_1_a_guard_ended_by_a_graceful_stop_is_replaced_and_still_works() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // TERM to the group ends the first guard; the agent ignores it. The supervisor then
    // dies before it could escalate to KILL.
    let mut s = supervisor("ignore-signals", true, "after-term");
    assert_eq!(s.guards, 2);
    assert!(s.alive(&s.root));
    s.kill();
    until("agent gone after the replaced guard fired", 20, || {
        s.group_gone()
    });
}

#[test]
fn p2_04_1_a_descendant_in_the_group_is_stopped_after_the_agent_itself_has_exited() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // The agent exits at once and leaves a child in its process group.
    let mut s = supervisor("child-alive", true, "plain");
    let child = s.known.iter().find(|k| k.pgid == s.root.pgid).cloned();
    let child = child.expect("the in-group descendant was discovered");
    assert!(!s.alive(&s.root) && s.alive(&child), "{:?}", s.group());
    // The guard is not a descendant of the agent and is never recorded as one.
    assert_eq!(s.known.len(), 1, "{:?}", s.known);
    s.kill();
    until("in-group descendant gone", 15, || s.group_gone());
    assert!(!s.alive(&child));
    assert!(!s.dir.join("barriers/linger-released").exists());
}

#[test]
fn p2_04_1_a_descendant_that_left_the_group_is_stopped_by_the_guard_through_the_parent_chain() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // M0: a child that put itself in its own process group, as real provider tool
    // processes do. The agent is alive when the supervisor dies, so the guard can still
    // reach the child from the leader.
    let mut s = supervisor("escaped-known", true, "plain");
    let escaped = s.known.iter().find(|k| k.pgid != s.root.pgid).cloned();
    let escaped = escaped.expect("the escaped descendant was discovered");
    assert!(s.alive(&escaped));
    s.kill();
    until("agent group gone", 15, || s.group_gone());
    until("escaped descendant gone", 15, || !s.alive(&escaped));
    // Stopped by the guard, not by the fixture's release barrier.
    assert!(!s.dir.join("barriers/linger-released").exists());
}

#[test]
fn p2_04_1_a_normal_exit_releases_the_guard_and_no_signal_is_sent() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let dir = scratch("normal");
    let mut sup = Supervised::spawn_guarded(
        &launch(&dir, "wait-release"),
        SpoolFaults::default(),
        &inspector(),
        Some(&guard()),
    )
    .unwrap();
    let root = sup.root().clone();
    assert_eq!(sup.guards().len(), 1);
    // The guard is a member of the group but not part of the agent's tree.
    assert_eq!(sup.discover().unwrap(), vec![]);
    assert_eq!(sup.tree().live_group, vec![root.pid]);
    std::fs::write(dir.join("barriers/release"), b"").unwrap();
    let started = Instant::now();
    let exit = loop {
        match sup
            .messages()
            .recv_timeout(Duration::from_secs(30))
            .unwrap()
        {
            ProcessMessage::Exited(exit) => break exit,
            _ => assert!(started.elapsed() < Duration::from_secs(30)),
        }
    };
    // The fixture's own exit status, not a signal: the guard left without firing.
    assert_eq!((exit.code, exit.signal), (Some(7), None));
    assert!(sup.tree().absent(), "{:?}", sup.tree());
    assert_eq!(sup.guards().len(), 1, "no guard was replaced");
    let after = inspect_recorded(&inspector(), std::slice::from_ref(&root));
    assert!(
        after.iter().all(|(_, c)| *c == IdentityCheck::Absent),
        "{after:?}"
    );
}

#[test]
fn p2_04_1_a_launch_whose_guard_cannot_be_armed_is_killed_and_reported_uncertain() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let dir = scratch("unarmed");
    let missing = dir.join("no-such-guard");
    let failed = Supervised::spawn_guarded(
        &launch(&dir, "wait-release"),
        SpoolFaults::default(),
        &inspector(),
        Some(&missing),
    );
    let Err(SpawnFailure::Uncertain(why)) = failed else {
        panic!("an unguarded launch was returned");
    };
    assert!(why.contains("agent guard"), "{why}");
    // The fixture was started and killed: nothing it wrote was ever read.
    let spooled = std::fs::read(dir.join("spool/stdout.raw")).unwrap();
    assert!(spooled.is_empty(), "{spooled:?}");
}

fn guard_role(leader: &str, lifeline: &str) -> std::process::ExitStatus {
    // Its own process group, like a guard that was (wrongly) started outside the group.
    role(&format!("guard {leader} {lifeline}"), Path::new("/"))
        .stdout(Stdio::null())
        .process_group(0)
        .status()
        .unwrap()
}

#[test]
fn p2_04_1_a_guard_that_is_not_in_the_named_group_signals_nothing() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // Named leader: this test process, which is alive and in another group. A guard that
    // trusted the number would send it TERM, and this test would not finish.
    let me = std::process::id().to_string();
    assert_eq!(guard_role(&me, "eof").code(), Some(3));
    // A PID that has been recycled or never existed is no different: the guard's own
    // group is not that number, so nothing is sent. PID 1 and 0 are refused outright.
    let mut gone = role("none", Path::new("/"))
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let recycled = gone.id().to_string();
    gone.wait().unwrap();
    for leader in [recycled.as_str(), "1", "0"] {
        assert_eq!(guard_role(leader, "eof").code(), Some(3), "{leader}");
    }
}

#[test]
fn p2_04_1_the_guard_body_fires_only_inside_its_group_and_only_without_a_release() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    // Control for the test above: the same call, as the leader of its own group, does
    // signal on end-of-file (TERM to the leader, which is itself)...
    let fired = guard_role("self", "eof");
    assert_eq!((fired.code(), fired.signal()), (None, Some(15)));
    // ...and leaves quietly when the lifeline carried the release byte.
    let released = guard_role("self", "R");
    assert_eq!((released.code(), released.signal()), (Some(0), None));
}

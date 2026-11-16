//! T02: real process death. A child process (this test binary re-executed in a fixture
//! role) owns the database and is killed with SIGKILL at a controlled point; the parent
//! then becomes the owner. Synchronisation is an explicit READY line on the child's
//! stdout; the 60-second receive timeout is only a failure bound, never a pacing device.
//! No provider, network or installed CLI is involved: the only executable launched is
//! this test binary itself.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use common::{TempDir, clock, create, key, request, request_at, start};
use jojo_domain::identity::Revision;
use jojo_storage::{
    Admission, ClaimMode, ClaimState, CommandError, Engine, EngineConfig, EngineEpoch, EventSeq,
    ShutdownMarker, StorageError, Tx, TxError,
};

const ROLE: &str = "JOJO_T02_CHILD_ROLE";
const DATABASE: &str = "JOJO_T02_CHILD_DB";
const READY: &str = "JOJO-T02-CHILD-READY";

/// The committed command every child runs first.
fn reserve(tx: &mut Tx<'_>) -> Result<String, TxError> {
    let attempt = key("attempt", 1);
    let revision = tx.create_aggregate(&attempt, "RESERVED")?;
    tx.append_event("AttemptReserved", Some((&attempt, revision)), "")?;
    tx.acquire_claim("worktree:1", ClaimMode::Exclusive, &attempt)?;
    tx.enqueue_effect("launch:1", "process", "argv")?;
    tx.set_admission(Admission::Open)?;
    Ok("reserved".into())
}

/// The command a child is killed in the middle of.
fn start_second_attempt(tx: &mut Tx<'_>) -> Result<String, TxError> {
    let attempt = key("attempt", 2);
    let revision = tx.create_aggregate(&attempt, "RESERVED")?;
    tx.append_event("AttemptReserved", Some((&attempt, revision)), "")?;
    tx.update_aggregate(&key("attempt", 1), Revision(1), "STARTING")?;
    tx.acquire_claim("worktree:2", ClaimMode::Exclusive, &attempt)?;
    tx.enqueue_effect("launch:2", "process", "argv")?;
    Ok("second".into())
}

fn announce_ready_and_wait_to_be_killed() -> ! {
    // libtest leaves "test child_fixture ... " unterminated; start a fresh line.
    println!("\n{READY}");
    std::io::stdout().flush().unwrap();
    // The parent never writes and kills this process; EOF means the parent died.
    let mut sink = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut sink);
    std::process::exit(86);
}

/// Not a test of its own: the body only runs in a child spawned by the tests below.
#[test]
#[ignore = "fixture role executed in a child process by the crash tests"]
fn child_fixture() {
    let (Ok(role), Ok(db)) = (std::env::var(ROLE), std::env::var(DATABASE)) else {
        return;
    };
    let engine = Engine::start(EngineConfig::new(db, clock())).expect("child owns the database");
    engine
        .execute(request(&engine, 1, "reserve", "attempt-1"), reserve)
        .expect("child commits its first command");
    match role.as_str() {
        "idle-owner" => announce_ready_and_wait_to_be_killed(),
        "killed-inside-transaction" => {
            let _ = engine.execute(request(&engine, 2, "second", "attempt-2"), |tx| {
                start_second_attempt(tx)?;
                // All mutations of this command are written but not committed.
                announce_ready_and_wait_to_be_killed()
            });
            unreachable!("the child is killed inside the transaction");
        }
        other => panic!("unknown fixture role {other}"),
    }
}

fn spawn_child(role: &str, db: &Path) -> Child {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "child_fixture",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ROLE, role)
        .env(DATABASE, db)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn fixture child");
    let stdout = child.stdout.take().unwrap();
    let (ready, signal) = mpsc::channel();
    std::thread::spawn(move || {
        let seen = BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .any(|line| line == READY);
        let _ = ready.send(seen);
    });
    match signal.recv_timeout(Duration::from_secs(60)) {
        Ok(true) => child,
        outcome => {
            let _ = child.kill();
            panic!(
                "fixture child did not become ready: {outcome:?} {:?}",
                child.wait()
            );
        }
    }
}

fn kill(mut child: Child) {
    // `Child::wait` closes the child's stdin before reaping. The fixture exits with 86 on
    // stdin EOF, and a posted SIGKILL does not stop another thread of the child from
    // seeing that EOF first (observed on macOS under load: exit 86 instead of signal 9).
    // Keep the pipe open until the child is reaped so SIGKILL is the only possible death.
    let stdin = child.stdin.take();
    child.kill().expect("SIGKILL delivered");
    let status = child.wait().expect("child reaped");
    drop(stdin);
    assert_eq!(
        status.signal(),
        Some(9),
        "the child died by SIGKILL, not by exiting"
    );
}

#[test]
fn ownership_is_exclusive_across_processes_and_is_freed_by_death_not_by_time() {
    let dir = TempDir::new("crash-owner");
    let child = spawn_child("idle-owner", &dir.db());

    let denied = Engine::start(EngineConfig::new(dir.db(), clock()));
    let Err(StorageError::OwnershipHeld { owner_hint, .. }) = denied else {
        panic!("a live owner in another process must exclude this one");
    };
    assert!(
        owner_hint.contains(&format!("pid={} epoch=1", child.id())),
        "{owner_hint}"
    );

    kill(child);

    let engine = start(&dir.db());
    let report = &engine.startup_report().epoch;
    assert_eq!(report.epoch, EngineEpoch(2));
    assert_eq!(
        report.previous,
        Some((EngineEpoch(1), ShutdownMarker::Unclean))
    );
    assert_eq!(report.quarantined_claims, 1);
    let snapshot = engine.reader().unwrap().snapshot().unwrap();
    assert_eq!(
        snapshot.session.admission,
        Admission::Closed,
        "closed after crash recovery"
    );
    assert_eq!(snapshot.live_claims[0].state, ClaimState::Quarantined);
    // The dead owner's claim keeps excluding new writers of that worktree.
    create(&engine, 10, &key("attempt", 9));
    let retake = engine.execute(request(&engine, 11, "claim", ""), |tx| {
        tx.acquire_claim("worktree:1", ClaimMode::Exclusive, &key("attempt", 9))?;
        Ok(String::new())
    });
    assert!(matches!(
        retake,
        Err(CommandError::Rejected(TxError::ClaimConflict { .. }))
    ));
}

#[test]
fn a_helper_killed_inside_a_transaction_leaves_only_whole_committed_commands() {
    let dir = TempDir::new("crash-transaction");
    kill(spawn_child("killed-inside-transaction", &dir.db()));

    let engine = start(&dir.db());
    assert_eq!(engine.epoch(), EngineEpoch(2));
    let mut reader = engine.reader().unwrap();
    let snapshot = reader.snapshot().unwrap();
    // Command 1 is fully there; the interrupted command 2 left nothing at all.
    assert_eq!(snapshot.aggregates.len(), 1);
    assert_eq!(snapshot.aggregates[0].key, key("attempt", 1));
    assert_eq!(
        (
            snapshot.aggregates[0].revision,
            snapshot.aggregates[0].state.as_str()
        ),
        (Revision(1), "RESERVED")
    );
    assert_eq!(snapshot.live_claims.len(), 1);
    assert_eq!(snapshot.live_claims[0].resource_key, "worktree:1");
    assert_eq!(snapshot.effects.len(), 1);
    assert_eq!(snapshot.effects[0].operation_key, "launch:1");
    assert_eq!(
        reader.receipt(request(&engine, 2, "", "").id).unwrap(),
        None
    );
    let events = reader.events_after(EventSeq(0), 100).unwrap();
    let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(kinds, ["EngineStarted", "AttemptReserved", "EngineStarted"]);
    assert_eq!(
        events.iter().map(|e| e.seq.0).collect::<Vec<_>>(),
        [1, 2, 3]
    );

    // Duplicate command after restart: the original receipt (epoch 1) is returned and the
    // handler does not run, although the retry is addressed to the new epoch.
    let replay = engine
        .execute(request(&engine, 1, "reserve", "attempt-1"), |_| {
            panic!("a replayed command must not execute")
        })
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(
        (replay.epoch, replay.response.as_str(), replay.event_seq),
        (EngineEpoch(1), "reserved", EventSeq(2))
    );
    // Same ID, different payload, after restart: explicit conflict.
    let conflict = engine.execute(request(&engine, 1, "reserve", "attempt-other"), reserve);
    assert!(matches!(conflict, Err(CommandError::CommandIdConflict(_))));
    // A retry still addressed to the dead epoch is refused rather than replayed or run.
    let stale = engine.execute(
        request_at(EngineEpoch(1), 2, "second", "attempt-2"),
        start_second_attempt,
    );
    assert!(matches!(stale, Err(CommandError::StaleEpoch { .. })));
    assert_eq!(reader.snapshot().unwrap(), snapshot);

    // The interrupted command, retried against the new epoch, now commits exactly once.
    let second = engine
        .execute(
            request(&engine, 2, "second", "attempt-2"),
            start_second_attempt,
        )
        .unwrap();
    assert!(!second.replayed);
    assert_eq!(second.event_seq, EventSeq(4));
    assert_eq!(reader.snapshot().unwrap().aggregates.len(), 2);
}

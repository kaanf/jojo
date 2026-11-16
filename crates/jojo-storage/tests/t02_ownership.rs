//! T02: OS-backed single ownership, engine epochs, closed startup admission, quarantine
//! instead of timeout-based release, and rejection of stale-epoch mutations.
//! (Ownership across real processes and SIGKILL is covered in `t02_crash.rs`.)

mod common;

use common::{TempDir, bump, clock, create, key, reading, request, request_at, start};
use jojo_domain::identity::Revision;
use jojo_storage::{
    Admission, ClaimMode, ClaimState, CommandError, Engine, EngineConfig, EngineEpoch, EventSeq,
    ReleaseBasis, ShutdownMarker, StorageError, TxError,
};

#[test]
fn a_second_helper_is_denied_while_the_first_owns_the_database() {
    let dir = TempDir::new("second-owner");
    let owner = start(&dir.db());
    create(&owner, 1, &key("counter", 1));
    let mut reader = owner.reader().unwrap();
    let before = reader.snapshot().unwrap();

    let denied = Engine::start(EngineConfig::new(dir.db(), clock()));
    let Err(StorageError::OwnershipHeld {
        lock_path,
        owner_hint,
    }) = denied
    else {
        panic!("second owner must be refused");
    };
    assert_eq!(lock_path, owner.lock_path());
    assert!(
        owner_hint.contains(&format!("pid={} epoch=1", std::process::id())),
        "{owner_hint}"
    );

    // A different spelling of the same database (through a symlinked directory) is the
    // same database and is refused too.
    let alias = dir.path().with_extension("alias");
    std::os::unix::fs::symlink(dir.path(), &alias).unwrap();
    let via_alias = Engine::start(EngineConfig::new(alias.join("jojo.sqlite3"), clock()));
    std::fs::remove_file(&alias).unwrap();
    assert!(matches!(via_alias, Err(StorageError::OwnershipHeld { .. })));

    // The refused starts wrote nothing: no new epoch, no event, same state; the owner works.
    assert_eq!(reader.snapshot().unwrap(), before);
    assert_eq!(before.session.epoch, EngineEpoch(1));
    bump(&owner, 2, &key("counter", 1), Revision(1)).unwrap();
}

#[test]
fn a_replacement_starts_a_new_epoch_with_admission_closed_and_quarantines_old_claims() {
    let dir = TempDir::new("replacement");
    let test_clock = clock();
    let config = || EngineConfig::new(dir.db(), test_clock.clone());

    let first = Engine::start(config()).unwrap();
    assert_eq!(first.epoch(), EngineEpoch(1));
    assert_eq!(first.startup_report().epoch.previous, None);
    let first_session = first.reader().unwrap().snapshot().unwrap().session;
    assert_eq!(
        first_session.admission,
        Admission::Closed,
        "admission is closed at first start"
    );
    assert_eq!(first_session.shutdown, ShutdownMarker::Running);

    // The first owner opens admission and reserves work for two attempts...
    for attempt in [1u128, 2] {
        create(&first, attempt, &key("attempt", attempt));
    }
    first
        .execute(request(&first, 10, "admit", ""), |tx| {
            tx.set_admission(Admission::Open)?;
            tx.acquire_claim("worktree:1", ClaimMode::Exclusive, &key("attempt", 1))?;
            tx.acquire_claim("repo:a", ClaimMode::Shared, &key("attempt", 1))?;
            tx.acquire_claim("repo:a", ClaimMode::Shared, &key("attempt", 2))?;
            Ok(String::new())
        })
        .unwrap();
    assert_eq!(
        first
            .reader()
            .unwrap()
            .snapshot()
            .unwrap()
            .session
            .admission,
        Admission::Open
    );
    // ...and dies without a clean shutdown (in-process stand-in for a crash; a real
    // SIGKILL of a separate process is exercised in t02_crash.rs).
    drop(first);

    let second = Engine::start(config()).unwrap();
    let report = second.startup_report().epoch.clone();
    assert_eq!(report.epoch, EngineEpoch(2));
    assert_eq!(
        report.previous,
        Some((EngineEpoch(1), ShutdownMarker::Unclean))
    );
    assert_eq!(report.quarantined_claims, 3);
    let mut reader = second.reader().unwrap();
    let snapshot = reader.snapshot().unwrap();
    assert_eq!(snapshot.session.epoch, EngineEpoch(2));
    assert_eq!(
        snapshot.session.admission,
        Admission::Closed,
        "the dead owner's open admission is not inherited"
    );
    assert_eq!(snapshot.live_claims.len(), 3);
    assert!(
        snapshot
            .live_claims
            .iter()
            .all(|c| c.state == ClaimState::Quarantined && c.epoch == EngineEpoch(1))
    );
    let started = reader.events_after(EventSeq(0), 100).unwrap();
    let started = started
        .iter()
        .filter(|e| e.kind == "EngineStarted")
        .collect::<Vec<_>>();
    assert_eq!(started.len(), 2);
    assert_eq!(
        started[1].payload,
        "previous_epoch=1;previous_shutdown=UNCLEAN;quarantined_claims=3"
    );

    // No amount of elapsed time frees a quarantined claim: jump the clock ten years.
    test_clock.set(reading(1_700_000_000_000 + 10 * 365 * 24 * 3_600_000));
    create(&second, 20, &key("attempt", 3));
    let take_worktree = |id: u128| {
        second.execute(request(&second, id, "claim", &id.to_string()), |tx| {
            tx.acquire_claim("worktree:1", ClaimMode::Exclusive, &key("attempt", 3))?;
            Ok(String::new())
        })
    };
    assert!(matches!(
        take_worktree(21),
        Err(CommandError::Rejected(TxError::ClaimConflict { .. }))
    ));
    // Quarantined shared holders still block exclusive administration of their repository.
    let admin = second.execute(request(&second, 22, "admin", ""), |tx| {
        tx.acquire_claim("repo:a", ClaimMode::Exclusive, &key("attempt", 3))?;
        Ok(String::new())
    });
    assert!(
        matches!(admin, Err(CommandError::Rejected(TxError::ClaimConflict { ref blocking, .. })) if blocking.len() == 2)
    );

    // Unresolved quarantine is not a clean shutdown.
    let worktree_claim = snapshot
        .live_claims
        .iter()
        .find(|c| c.resource_key == "worktree:1")
        .unwrap()
        .id;
    second
        .execute(request(&second, 23, "resolve", ""), move |tx| {
            tx.release_claim(worktree_claim, ReleaseBasis::RecoveryResolved)?;
            Ok(String::new())
        })
        .unwrap();
    take_worktree(24).expect("released on recorded evidence, the worktree can be claimed");
    assert!(matches!(
        second.shutdown(),
        Err(StorageError::LiveClaimsAtShutdown(3))
    ));

    // The refused clean shutdown leaves an unclean session; the third start says so.
    let third = Engine::start(config()).unwrap();
    assert_eq!(
        third.startup_report().epoch.previous,
        Some((EngineEpoch(2), ShutdownMarker::Unclean))
    );
    assert_eq!(
        third.startup_report().epoch.quarantined_claims,
        1,
        "attempt 3's new claim"
    );
}

#[test]
fn a_clean_shutdown_is_recorded_and_the_next_start_is_still_closed() {
    let dir = TempDir::new("clean");
    let first = start(&dir.db());
    first
        .execute(request(&first, 1, "open", ""), |tx| {
            tx.set_admission(Admission::Open)?;
            Ok(String::new())
        })
        .unwrap();
    first.shutdown().unwrap();
    let second = start(&dir.db());
    let report = &second.startup_report().epoch;
    assert_eq!(
        report.previous,
        Some((EngineEpoch(1), ShutdownMarker::Clean))
    );
    assert_eq!(report.quarantined_claims, 0);
    let mut reader = second.reader().unwrap();
    assert_eq!(
        reader.snapshot().unwrap().session.admission,
        Admission::Closed
    );
    let kinds: Vec<String> = reader
        .events_after(EventSeq(0), 10)
        .unwrap()
        .into_iter()
        .map(|e| e.kind)
        .collect();
    assert_eq!(kinds, ["EngineStarted", "EngineStopped", "EngineStarted"]);
}

#[test]
fn a_command_addressed_to_an_old_epoch_is_rejected_without_effect() {
    let dir = TempDir::new("old-epoch-command");
    let first = start(&dir.db());
    let counter = key("counter", 1);
    create(&first, 1, &counter);
    drop(first);
    let second = start(&dir.db());
    let mut reader = second.reader().unwrap();
    let before = reader.snapshot().unwrap();
    // A late callback from epoch 1 (and one from a not-yet-existing epoch) reaches epoch 2.
    for (id, epoch) in [(2u128, 1u64), (3, 3)] {
        let target = counter.clone();
        let error = second
            .execute(request_at(EngineEpoch(epoch), id, "bump", ""), move |tx| {
                tx.update_aggregate(&target, Revision(1), "late")?;
                Ok(String::new())
            })
            .unwrap_err();
        assert!(
            matches!(error, CommandError::StaleEpoch { command_epoch, current_epoch, .. }
                if command_epoch == EngineEpoch(epoch) && current_epoch == EngineEpoch(2)),
            "{error:?}"
        );
    }
    assert_eq!(reader.snapshot().unwrap(), before);
    // The current epoch is unaffected and still accepts correctly addressed commands.
    bump(&second, 4, &counter, Revision(1)).unwrap();
}

#[test]
fn a_superseded_engine_cannot_mutate_state_even_if_the_os_lock_was_bypassed() {
    // Deleting the lock file while it is held defeats the OS lock: a second helper locks a
    // fresh file. The database fence must then stop the old engine on its next write.
    let dir = TempDir::new("zombie");
    let old = start(&dir.db());
    let counter = key("counter", 1);
    create(&old, 1, &counter);
    old.execute(request(&old, 2, "claim", ""), {
        let counter = counter.clone();
        move |tx| {
            tx.set_admission(Admission::Open)?;
            tx.acquire_claim("worktree:1", ClaimMode::Exclusive, &counter)?;
            Ok(String::new())
        }
    })
    .unwrap();
    std::fs::remove_file(old.lock_path()).unwrap();

    let new = start(&dir.db());
    assert_eq!(new.epoch(), EngineEpoch(2));
    let mut reader = new.reader().unwrap();
    let before = reader.snapshot().unwrap();
    assert_eq!(before.live_claims[0].state, ClaimState::Quarantined);

    let error = bump(&old, 3, &counter, Revision(1)).unwrap_err();
    assert!(
        matches!(error, CommandError::StaleEpoch { writer_epoch, current_epoch, .. }
            if writer_epoch == EngineEpoch(1) && current_epoch == EngineEpoch(2)),
        "{error:?}"
    );
    // Once fenced, the old writer is stopped for good — also for replays and shutdown.
    assert!(matches!(
        bump(&old, 4, &counter, Revision(1)),
        Err(CommandError::WriterStopped)
    ));
    let replay = old.execute(request(&old, 1, "create", &format!("{counter:?}")), |_| {
        Ok(String::new())
    });
    assert!(matches!(replay, Err(CommandError::WriterStopped)));
    assert!(matches!(old.shutdown(), Err(StorageError::WriterStopped)));

    assert_eq!(
        reader.snapshot().unwrap(),
        before,
        "the old epoch changed nothing"
    );
    assert_eq!(before.session.shutdown, ShutdownMarker::Running);
    bump(&new, 5, &counter, Revision(1)).unwrap();
}

#[test]
fn ownership_can_be_handed_over_repeatedly_and_epochs_only_increase() {
    let dir = TempDir::new("epochs");
    let shared_clock = clock();
    let mut epochs = Vec::new();
    for round in 0..5 {
        let engine = Engine::start(EngineConfig::new(dir.db(), shared_clock.clone())).unwrap();
        epochs.push(engine.epoch().0);
        if round % 2 == 0 {
            engine.shutdown().unwrap();
        }
    }
    assert_eq!(epochs, [1, 2, 3, 4, 5]);
}

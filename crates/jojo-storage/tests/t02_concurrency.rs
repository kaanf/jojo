//! T02: revision compare-and-swap and command idempotency through the engine's writer.
//! (The same races driven over two raw connections, without the writer queue, are unit
//! tests in `src/writer.rs`.)

mod common;

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use common::{TempDir, bump, create, key, request, start};
use jojo_domain::identity::Revision;
use jojo_storage::{ClaimMode, CommandError, EventSeq, TxError};

#[test]
fn concurrent_updates_with_the_same_expected_revision_have_exactly_one_winner() {
    let dir = TempDir::new("cas");
    let engine = start(&dir.db());
    let counter = key("counter", 1);
    create(&engine, 1, &counter);
    const CONTENDERS: u128 = 8;
    for round in 0..20u64 {
        let expected = Revision(round + 1);
        let barrier = Barrier::new(CONTENDERS as usize);
        let outcomes = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..CONTENDERS)
                .map(|contender| {
                    let (engine, counter, barrier) = (&engine, &counter, &barrier);
                    scope.spawn(move || {
                        let id = 100 + u128::from(round) * CONTENDERS + contender;
                        barrier.wait();
                        bump(engine, id, counter, expected)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(
            outcomes.iter().filter(|o| o.is_ok()).count(),
            1,
            "round {round}"
        );
        for loser in outcomes.iter().filter_map(|o| o.as_ref().err()) {
            assert!(
                matches!(
                    loser,
                    CommandError::Rejected(TxError::RevisionConflict { expected: e, actual: Some(a), .. })
                        if *e == expected && *a == Revision(expected.0 + 1)
                ),
                "{loser:?}"
            );
        }
    }
    let mut reader = engine.reader().unwrap();
    let snapshot = reader.snapshot().unwrap();
    assert_eq!(snapshot.aggregates.len(), 1);
    assert_eq!(snapshot.aggregates[0].revision, Revision(21));
    assert_eq!(snapshot.aggregates[0].state, "21");
    // EngineStarted + Created + exactly one Bumped per round; losers appended nothing.
    let events = reader.events_after(EventSeq(0), 1_000).unwrap();
    assert_eq!(events.len(), 22);
    assert_eq!(events.iter().filter(|e| e.kind == "Bumped").count(), 20);
}

#[test]
fn a_stale_expected_revision_is_rejected_and_changes_nothing() {
    let dir = TempDir::new("stale-revision");
    let engine = start(&dir.db());
    let counter = key("counter", 1);
    create(&engine, 1, &counter);
    bump(&engine, 2, &counter, Revision(1)).unwrap();
    let mut reader = engine.reader().unwrap();
    let before = reader.snapshot().unwrap();
    let stale = bump(&engine, 3, &counter, Revision(1)).unwrap_err();
    assert!(matches!(
        stale,
        CommandError::Rejected(TxError::RevisionConflict {
            actual: Some(Revision(2)),
            ..
        })
    ));
    let missing = bump(&engine, 4, &key("counter", 404), Revision(1)).unwrap_err();
    assert!(matches!(
        missing,
        CommandError::Rejected(TxError::RevisionConflict { actual: None, .. })
    ));
    assert_eq!(reader.snapshot().unwrap(), before);
    // A rejected command leaves no receipt, so the same ID can be retried correctly.
    assert_eq!(
        reader.receipt(request(&engine, 3, "", "").id).unwrap(),
        None
    );
}

#[test]
fn replaying_a_command_returns_the_original_receipt_without_repeating_effects() {
    let dir = TempDir::new("replay");
    let engine = start(&dir.db());
    let attempt = key("attempt", 1);
    let runs = Arc::new(AtomicUsize::new(0));
    let submit = || {
        let (attempt, runs) = (attempt.clone(), runs.clone());
        engine.execute(request(&engine, 7, "reserve", "attempt-1"), move |tx| {
            runs.fetch_add(1, Ordering::SeqCst);
            let revision = tx.create_aggregate(&attempt, "RESERVED")?;
            tx.append_event(
                "AttemptReserved",
                Some((&attempt, revision)),
                &jojo_domain::payload::Record::new(),
            )?;
            tx.acquire_claim("worktree:1", ClaimMode::Exclusive, &attempt)?;
            tx.enqueue_effect("launch:attempt-1", "process", "argv")?;
            Ok("reserved".into())
        })
    };
    let original = submit().unwrap();
    assert!(!original.replayed);
    let mut reader = engine.reader().unwrap();
    let committed = reader.snapshot().unwrap();
    for _ in 0..3 {
        let replay = submit().unwrap();
        assert!(replay.replayed);
        assert_eq!(
            (
                replay.command_id,
                replay.epoch,
                &replay.response,
                replay.event_seq
            ),
            (
                original.command_id,
                original.epoch,
                &original.response,
                original.event_seq
            )
        );
    }
    assert_eq!(
        runs.load(Ordering::SeqCst),
        1,
        "the handler ran exactly once"
    );
    assert_eq!(
        reader.snapshot().unwrap(),
        committed,
        "replays wrote nothing"
    );
    assert_eq!(committed.live_claims.len(), 1);
    assert_eq!(committed.effects.len(), 1);
    let stored = reader.receipt(original.command_id).unwrap().unwrap();
    assert_eq!(stored.response, "reserved");
}

#[test]
fn reusing_a_command_id_with_a_different_request_is_an_explicit_conflict() {
    let dir = TempDir::new("id-conflict");
    let engine = start(&dir.db());
    let counter = key("counter", 1);
    create(&engine, 1, &counter);
    let mut reader = engine.reader().unwrap();
    let before = reader.snapshot().unwrap();
    let runs = Arc::new(AtomicUsize::new(0));
    // Same ID as `create` above (1) with a different payload, then a different kind.
    for (kind, payload) in [("create", "other payload"), ("other-kind", "")] {
        let runs = runs.clone();
        let error = engine
            .execute(request(&engine, 1, kind, payload), move |_| {
                runs.fetch_add(1, Ordering::SeqCst);
                Ok("must not run".into())
            })
            .unwrap_err();
        assert!(
            matches!(error, CommandError::CommandIdConflict(id) if id.0.0 == 1),
            "{error:?}"
        );
    }
    assert_eq!(runs.load(Ordering::SeqCst), 0);
    assert_eq!(reader.snapshot().unwrap(), before);
    let original = reader
        .receipt(request(&engine, 1, "", "").id)
        .unwrap()
        .unwrap();
    assert_eq!(original.response, "created");
}

#[test]
fn concurrent_duplicate_commands_commit_one_logical_operation() {
    let dir = TempDir::new("concurrent-duplicate");
    let engine = start(&dir.db());
    let attempt = key("attempt", 1);
    let handler_runs = Arc::new(AtomicUsize::new(0));
    let barrier = Barrier::new(8);
    let receipts = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let (engine, barrier) = (&engine, &barrier);
                let (attempt, handler_runs) = (attempt.clone(), handler_runs.clone());
                scope.spawn(move || {
                    barrier.wait();
                    engine
                        .execute(request(engine, 42, "reserve", "same"), move |tx| {
                            handler_runs.fetch_add(1, Ordering::SeqCst);
                            let revision = tx.create_aggregate(&attempt, "RESERVED")?;
                            tx.append_event(
                                "AttemptReserved",
                                Some((&attempt, revision)),
                                &jojo_domain::payload::Record::new(),
                            )?;
                            Ok("reserved".into())
                        })
                        .unwrap()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(receipts.len(), 8, "every caller got an answer");
    assert_eq!(
        handler_runs.load(Ordering::SeqCst),
        1,
        "one logical operation ran"
    );
    assert_eq!(receipts.iter().filter(|r| !r.replayed).count(), 1);
    let distinct: BTreeSet<_> = receipts
        .iter()
        .map(|r| (r.response.clone(), r.event_seq, r.epoch))
        .collect();
    assert_eq!(distinct.len(), 1, "all callers observe the same receipt");
    let mut reader = engine.reader().unwrap();
    let events = reader.events_after(EventSeq(0), 100).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == "AttemptReserved")
            .count(),
        1
    );
}

#[test]
fn many_independent_attempts_commit_in_parallel_without_a_singleton_slot() {
    // Sixteen attempts for two provider identities across four repositories reserve at
    // once. Each takes its own exclusive worktree claim plus a shared claim on its
    // repository's activity gate. Nothing in storage assumes one current attempt.
    let dir = TempDir::new("multi-attempt");
    let engine = start(&dir.db());
    let barrier = Barrier::new(16);
    std::thread::scope(|scope| {
        for index in 0..16u128 {
            let (engine, barrier) = (&engine, &barrier);
            scope.spawn(move || {
                let attempt = key("attempt", index);
                let provider = if index % 2 == 0 {
                    "claude-code"
                } else {
                    "codex"
                };
                barrier.wait();
                engine
                    .execute(
                        request(engine, 500 + index, "reserve", &index.to_string()),
                        move |tx| {
                            let revision = tx.create_aggregate(&attempt, provider)?;
                            tx.append_event(
                                "AttemptReserved",
                                Some((&attempt, revision)),
                                &jojo_domain::payload::Record::new().text("provider", provider),
                            )?;
                            tx.acquire_claim(
                                &format!("worktree:{index}"),
                                ClaimMode::Exclusive,
                                &attempt,
                            )?;
                            tx.acquire_claim(
                                &format!("repo-activity:{}", index % 4),
                                ClaimMode::Shared,
                                &attempt,
                            )?;
                            tx.enqueue_effect(&format!("launch:{index}"), "process", provider)?;
                            Ok(String::new())
                        },
                    )
                    .unwrap();
            });
        }
    });
    let mut reader = engine.reader().unwrap();
    let snapshot = reader.snapshot().unwrap();
    assert_eq!(snapshot.aggregates.len(), 16);
    assert_eq!(snapshot.live_claims.len(), 32);
    assert_eq!(snapshot.effects.len(), 16);
    for repository in 0..4 {
        let holders = snapshot
            .live_claims
            .iter()
            .filter(|c| c.resource_key == format!("repo-activity:{repository}"))
            .count();
        assert_eq!(holders, 4, "four attempts share repository {repository}");
    }
    // A second writer for an already-owned worktree is refused; the owner is unaffected.
    let intruder = key("attempt", 99);
    let error = engine
        .execute(request(&engine, 900, "reserve", "intruder"), move |tx| {
            tx.create_aggregate(&intruder, "codex")?;
            tx.acquire_claim("worktree:3", ClaimMode::Exclusive, &intruder)?;
            Ok(String::new())
        })
        .unwrap_err();
    assert!(matches!(
        error,
        CommandError::Rejected(TxError::ClaimConflict { ref blocking, .. }) if blocking.len() == 1
    ));
    assert_eq!(reader.snapshot().unwrap(), snapshot);
}

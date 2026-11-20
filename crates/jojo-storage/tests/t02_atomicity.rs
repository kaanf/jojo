//! T02: a failed writer transaction leaves no partial state, event, claim, outbox entry
//! or receipt; and the claim/outbox primitives enforce their rules inside the transaction.

mod common;

use common::{TempDir, bump, clock, create, key, reading, request, start};
use jojo_domain::identity::Revision;
use jojo_storage::{
    Admission, ClaimMode, ClaimState, CommandError, EffectState, Engine, EngineConfig, EventSeq,
    FailPoint, FaultPlan, ReleaseBasis, StorageError, Tx, TxError,
};

/// Touches every kind of record a command can write: a new aggregate, a compare-and-swap
/// on an existing one, two events, a claim, an outbox entry and the admission gate.
fn mutate_everything(tx: &mut Tx<'_>) -> Result<(), TxError> {
    let attempt = key("attempt", 50);
    let created = tx.create_aggregate(&attempt, "RESERVED")?;
    tx.append_event(
        "AttemptReserved",
        Some((&attempt, created)),
        &jojo_domain::payload::Record::new(),
    )?;
    let step = key("step", 1);
    let bumped = tx.update_aggregate(&step, Revision(1), "2")?;
    tx.append_event(
        "Bumped",
        Some((&step, bumped)),
        &jojo_domain::payload::Record::new().text("state", "2"),
    )?;
    tx.acquire_claim("worktree:50", ClaimMode::Exclusive, &attempt)?;
    tx.enqueue_effect("launch:50", "process", "argv")?;
    tx.set_admission(Admission::Open)?;
    Ok(())
}

#[test]
fn every_kind_of_forced_failure_rolls_back_all_mutations_and_the_receipt() {
    let dir = TempDir::new("atomic");
    let faults = FaultPlan::default();
    let mut config = EngineConfig::new(dir.db(), clock());
    config.faults = faults.clone();
    let engine = Engine::start(config).unwrap();
    create(&engine, 1, &key("step", 1));
    let mut reader = engine.reader().unwrap();
    let baseline = reader.snapshot().unwrap();
    let all_events =
        |reader: &mut jojo_storage::Reader| reader.events_after(EventSeq(0), 1_000).unwrap();
    let baseline_events = all_events(&mut reader);

    type Ending = fn(&mut Tx<'_>) -> Result<(), TxError>;
    let endings: [(&str, Ending, Option<FailPoint>); 6] = [
        (
            "handler rejection",
            |_| Err(TxError::Rejected("no".into())),
            None,
        ),
        // A real SQLite constraint failure: the claim holder does not exist (foreign key).
        (
            "foreign key violation",
            |tx| {
                tx.acquire_claim("worktree:51", ClaimMode::Exclusive, &key("attempt", 404))
                    .map(drop)
            },
            None,
        ),
        (
            "stale revision at the end",
            |tx| {
                tx.update_aggregate(&key("step", 1), Revision(1), "x")
                    .map(drop)
            },
            None,
        ),
        (
            "claim conflict at the end",
            |tx| {
                tx.acquire_claim("worktree:50", ClaimMode::Shared, &key("step", 1))
                    .map(drop)
            },
            None,
        ),
        (
            "fault before receipt",
            |_| Ok(()),
            Some(FailPoint::BeforeReceipt),
        ),
        (
            "fault before commit",
            |_| Ok(()),
            Some(FailPoint::BeforeCommit),
        ),
    ];
    for (index, (name, ending, fault)) in endings.into_iter().enumerate() {
        if let Some(point) = fault {
            faults.arm(point);
        }
        let id = 100 + index as u128;
        let error = engine
            .execute(request(&engine, id, "everything", name), move |tx| {
                mutate_everything(tx)?;
                ending(tx)?;
                Ok("done".into())
            })
            .unwrap_err();
        match fault {
            Some(point) => {
                assert!(
                    matches!(error, CommandError::InjectedFault(p) if p == point),
                    "{name}"
                )
            }
            None => assert!(
                matches!(error, CommandError::Rejected(_)),
                "{name}: {error:?}"
            ),
        }
        assert_eq!(
            reader.snapshot().unwrap(),
            baseline,
            "{name}: projections changed"
        );
        assert_eq!(
            all_events(&mut reader),
            baseline_events,
            "{name}: events changed"
        );
        assert_eq!(
            reader.receipt(request(&engine, id, "", "").id).unwrap(),
            None,
            "{name}: a receipt survived the rollback"
        );
    }

    // The same command then commits normally: nothing was half-recorded, the event
    // sequence continues without a gap, and all of its effects appear exactly once.
    let receipt = engine
        .execute(
            request(&engine, 100, "everything", "handler rejection"),
            |tx| {
                mutate_everything(tx)?;
                Ok("done".into())
            },
        )
        .unwrap();
    assert!(!receipt.replayed);
    let after = reader.snapshot().unwrap();
    assert_eq!(
        after.as_of_event_seq,
        EventSeq(baseline.as_of_event_seq.0 + 2)
    );
    assert_eq!(receipt.event_seq, after.as_of_event_seq);
    assert_eq!(after.aggregates.len(), baseline.aggregates.len() + 1);
    assert_eq!(after.live_claims.len(), 1);
    assert_eq!(after.effects.len(), 1);
    assert_eq!(after.effects[0].state, EffectState::NotIssued);
    assert_eq!(after.session.admission, Admission::Open);
    let sequence: Vec<u64> = all_events(&mut reader).iter().map(|e| e.seq.0).collect();
    assert_eq!(sequence, (1..=sequence.len() as u64).collect::<Vec<_>>());
}

#[test]
fn a_panicking_handler_rolls_back_and_stops_the_writer() {
    let dir = TempDir::new("panic");
    let engine = start(&dir.db());
    create(&engine, 1, &key("step", 1));
    let mut reader = engine.reader().unwrap();
    let baseline = reader.snapshot().unwrap();
    let error = engine
        .execute(request(&engine, 2, "everything", ""), |tx| {
            mutate_everything(tx)?;
            panic!("injected handler panic (expected in this test)");
        })
        .unwrap_err();
    assert!(matches!(error, CommandError::WriterStopped));
    // Fail closed: the writer accepts nothing more instead of continuing in an unknown state.
    let later = bump(&engine, 3, &key("step", 1), Revision(1)).unwrap_err();
    assert!(matches!(later, CommandError::WriterStopped));
    assert_eq!(reader.snapshot().unwrap(), baseline);
    drop(reader);
    assert!(matches!(
        engine.shutdown(),
        Err(StorageError::WriterStopped)
    ));
    // The next owner sees an unclean predecessor and exactly the pre-panic state.
    let successor = start(&dir.db());
    let report = successor.startup_report();
    assert_eq!(report.epoch.epoch.0, 2);
    assert_eq!(
        report.epoch.previous.map(|(_, marker)| marker),
        Some(jojo_storage::ShutdownMarker::Unclean)
    );
    let snapshot = successor.reader().unwrap().snapshot().unwrap();
    assert_eq!(snapshot.aggregates, baseline.aggregates);
    assert!(snapshot.live_claims.is_empty() && snapshot.effects.is_empty());
}

#[test]
fn claims_are_all_or_nothing_and_shared_exclusive_rules_hold() {
    let dir = TempDir::new("claims");
    let engine = start(&dir.db());
    for id in 1..=3 {
        create(&engine, id, &key("attempt", id));
    }
    let claim = |id: u128, attempt: u128, wanted: Vec<(&'static str, ClaimMode)>| {
        engine.execute(
            request(&engine, id, "claim", &format!("{attempt}:{wanted:?}")),
            move |tx| {
                let holder = key("attempt", attempt);
                let ids: Vec<String> = wanted
                    .iter()
                    .map(|(resource, mode)| {
                        tx.acquire_claim(resource, *mode, &holder)
                            .map(|c| c.0.to_string())
                    })
                    .collect::<Result<_, _>>()?;
                Ok(ids.join(","))
            },
        )
    };
    use ClaimMode::{Exclusive, Shared};
    claim(10, 1, vec![("repo:a", Shared), ("worktree:1", Exclusive)]).unwrap();
    claim(11, 2, vec![("repo:a", Shared), ("worktree:2", Exclusive)]).unwrap();
    // Attempt 3 needs a free slot *and* attempt 1's worktree: it must get neither.
    let partial = claim(
        12,
        3,
        vec![("provider:codex", Exclusive), ("worktree:1", Exclusive)],
    );
    assert!(matches!(
        partial,
        Err(CommandError::Rejected(TxError::ClaimConflict { .. }))
    ));
    // Exclusive administration cannot start while shared holders exist; the same holder
    // cannot claim one resource twice.
    assert!(claim(13, 3, vec![("repo:a", Exclusive)]).is_err());
    assert!(claim(14, 1, vec![("repo:a", Shared)]).is_err());
    let mut reader = engine.reader().unwrap();
    let live = reader.snapshot().unwrap().live_claims;
    assert_eq!(live.len(), 4, "the refused commands left no partial claim");
    assert!(live.iter().all(|c| c.state == ClaimState::Active));
    assert!(!live.iter().any(|c| c.resource_key == "provider:codex"));

    // Releasing is explicit and final; a released claim cannot be released again.
    let worktree_1 = live
        .iter()
        .find(|c| c.resource_key == "worktree:1")
        .unwrap()
        .id;
    let release = |id: u128| {
        engine.execute(
            request(&engine, id, "release", &id.to_string()),
            move |tx| {
                tx.release_claim(worktree_1, ReleaseBasis::HolderSettled)?;
                Ok(String::new())
            },
        )
    };
    release(20).unwrap();
    assert!(matches!(
        release(21),
        Err(CommandError::Rejected(TxError::ClaimNotLive(id))) if id == worktree_1
    ));
    claim(
        22,
        3,
        vec![("provider:codex", Exclusive), ("worktree:1", Exclusive)],
    )
    .unwrap();
    assert_eq!(reader.snapshot().unwrap().live_claims.len(), 5);
}

#[test]
fn outbox_effects_follow_the_issue_state_machine_and_are_never_reissued() {
    let dir = TempDir::new("outbox");
    let engine = start(&dir.db());
    let step = |id: u128, body: fn(&mut Tx<'_>) -> Result<(), TxError>| {
        engine.execute(request(&engine, id, "effect", &id.to_string()), move |tx| {
            body(tx)?;
            Ok(String::new())
        })
    };
    step(1, |tx| tx.enqueue_effect("launch:1", "process", "argv")).unwrap();
    assert!(matches!(
        step(2, |tx| tx.enqueue_effect("launch:1", "process", "other")),
        Err(CommandError::Rejected(TxError::EffectExists(_)))
    ));
    // Skipping the issue boundary, or confirming something never issued, is refused.
    for (id, to) in [
        (3, EffectState::Confirmed),
        (4, EffectState::Rejected),
        (5, EffectState::NotIssued),
    ] {
        let outcome = engine.execute(
            request(&engine, id, "advance", &format!("{to:?}")),
            move |tx| {
                tx.advance_effect("launch:1", to)?;
                Ok(String::new())
            },
        );
        assert!(
            matches!(
                outcome,
                Err(CommandError::Rejected(
                    TxError::InvalidEffectTransition { .. }
                ))
            ),
            "{to:?}"
        );
    }
    step(6, |tx| {
        tx.advance_effect("launch:1", EffectState::IssuedUnknown)
    })
    .unwrap();
    // Once issued, the effect can never return to a dispatchable state.
    assert!(
        step(7, |tx| tx
            .advance_effect("launch:1", EffectState::IssuedUnknown))
        .is_err()
    );
    step(8, |tx| {
        tx.advance_effect("launch:1", EffectState::Confirmed)
    })
    .unwrap();
    assert!(step(9, |tx| tx.advance_effect("launch:1", EffectState::Rejected)).is_err());
    assert!(matches!(
        step(10, |tx| tx
            .advance_effect("launch:missing", EffectState::IssuedUnknown)),
        Err(CommandError::Rejected(TxError::EffectMissing(_)))
    ));
    let effects = engine.reader().unwrap().snapshot().unwrap().effects;
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].state, EffectState::Confirmed);
    assert_eq!(effects[0].payload, "argv");
    assert_eq!(
        effects[0].payload_digest,
        jojo_storage::sha256_hex(&[b"argv"])
    );
}

#[test]
fn events_must_describe_the_revision_they_were_committed_with() {
    let dir = TempDir::new("event-revision");
    let engine = start(&dir.db());
    let counter = key("counter", 1);
    create(&engine, 1, &counter);
    let lying = counter.clone();
    let error = engine
        .execute(request(&engine, 2, "lie", ""), move |tx| {
            tx.append_event(
                "Bumped",
                Some((&lying, Revision(5))),
                &jojo_domain::payload::Record::new().text("state", "5"),
            )?;
            Ok(String::new())
        })
        .unwrap_err();
    assert!(matches!(
        error,
        CommandError::Rejected(TxError::RevisionConflict { .. })
    ));
    // Timestamps come from the injected clock and are audit data only.
    let events = engine
        .reader()
        .unwrap()
        .events_after(EventSeq(0), 10)
        .unwrap();
    assert!(
        events
            .iter()
            .all(|e| e.recorded_at_unix_ms == reading(1_700_000_000_000).unix_millis)
    );
    assert_eq!(events.len(), 2);
}

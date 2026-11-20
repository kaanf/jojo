//! T02: a snapshot and its event cursor come from one consistent read. Events after the
//! cursor are exactly the ones the snapshot does not reflect — no gap, no overlap.

mod common;

use std::collections::BTreeMap;
use std::sync::Barrier;

use common::{TempDir, bump, create, key, start};
use jojo_domain::identity::Revision;
use jojo_storage::{AggregateKey, EventRecord, EventSeq, Snapshot};

/// Rebuilds the expected aggregate revisions/states at `cursor` from the event log alone.
fn replay(events: &[EventRecord], cursor: EventSeq) -> BTreeMap<AggregateKey, (Revision, String)> {
    let mut expected = BTreeMap::new();
    for event in events.iter().filter(|e| e.seq <= cursor) {
        if let Some((aggregate, revision)) = &event.aggregate {
            expected.insert(
                aggregate.clone(),
                (
                    *revision,
                    jojo_domain::payload::member(&event.payload, "state"),
                ),
            );
        }
    }
    expected
}

fn projected(snapshot: &Snapshot) -> BTreeMap<AggregateKey, (Revision, String)> {
    snapshot
        .aggregates
        .iter()
        .map(|a| (a.key.clone(), (a.revision, a.state.clone())))
        .collect()
}

#[test]
fn a_commit_during_a_read_is_invisible_to_it_and_is_the_next_event_after_its_cursor() {
    let dir = TempDir::new("snapshot-interleave");
    let engine = start(&dir.db());
    let counter = key("counter", 1);
    create(&engine, 1, &counter);
    let mut reader = engine.reader().unwrap();

    let (cursor, seen_revision, later_events, cursor_again) = reader
        .read(|view| {
            let cursor = view.event_cursor()?;
            // Two commands commit while this read is open.
            let first = bump(&engine, 2, &counter, Revision(1)).expect("commit during read");
            bump(&engine, 3, &counter, Revision(2)).expect("commit during read");
            assert!(first.event_seq > cursor);
            let aggregate = view.aggregate(&counter)?.expect("aggregate exists");
            Ok((
                cursor,
                aggregate.revision,
                view.events_after(cursor, 100)?,
                view.event_cursor()?,
            ))
        })
        .unwrap();
    assert_eq!(
        seen_revision,
        Revision(1),
        "the open read still sees its point in time"
    );
    assert!(later_events.is_empty());
    assert_eq!(cursor_again, cursor);

    // Resuming from the cursor delivers exactly the two missed events, in order.
    let missed = reader.events_after(cursor, 100).unwrap();
    assert_eq!(
        missed.iter().map(|e| e.seq.0).collect::<Vec<_>>(),
        [cursor.0 + 1, cursor.0 + 2]
    );
    assert_eq!(
        missed
            .iter()
            .map(|e| e.aggregate.clone().unwrap().1)
            .collect::<Vec<_>>(),
        [Revision(2), Revision(3)]
    );
    let fresh = reader.snapshot().unwrap();
    assert_eq!(fresh.as_of_event_seq, EventSeq(cursor.0 + 2));
    assert_eq!(fresh.aggregates[0].revision, Revision(3));
    // Bounded replay: a limit returns a prefix, and continuing from it loses nothing.
    let page = reader.events_after(EventSeq(0), 2).unwrap();
    assert_eq!(page.len(), 2);
    let rest = reader.events_after(page[1].seq, 100).unwrap();
    assert_eq!(rest.first().map(|e| e.seq.0), Some(3));
}

#[test]
fn every_snapshot_taken_under_concurrent_writes_matches_the_log_at_its_cursor() {
    const WRITERS: u128 = 4;
    const COMMANDS: u64 = 40;
    let dir = TempDir::new("snapshot-stress");
    let engine = start(&dir.db());
    let shared = key("shared", 0);
    create(&engine, 1, &shared);
    for writer in 0..WRITERS {
        create(&engine, 10 + writer, &key("own", writer));
    }
    let barrier = Barrier::new(WRITERS as usize);
    let snapshots: Vec<Snapshot> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..WRITERS)
            .map(|writer| {
                let (engine, barrier, shared) = (&engine, &barrier, &shared);
                scope.spawn(move || {
                    let own = key("own", writer);
                    let mut reader = engine.reader().unwrap();
                    let mut taken = Vec::new();
                    barrier.wait();
                    for step in 0..COMMANDS {
                        let id = 1_000 * (u128::from(step) + 1) + writer * 10;
                        let receipt = bump(engine, id, &own, Revision(step + 1)).unwrap();
                        // Contend on the shared aggregate too; losing the race is fine.
                        let seen = reader.snapshot().unwrap();
                        let shared_revision = seen
                            .aggregates
                            .iter()
                            .find(|a| a.key == *shared)
                            .unwrap()
                            .revision;
                        let _ = bump(engine, id + 1, shared, shared_revision);
                        assert!(
                            seen.as_of_event_seq >= receipt.event_seq,
                            "a snapshot taken after an acknowledged commit includes it"
                        );
                        taken.push(seen);
                    }
                    taken
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect()
    });
    assert_eq!(snapshots.len(), (WRITERS as usize) * (COMMANDS as usize));

    let mut reader = engine.reader().unwrap();
    let log = reader.events_after(EventSeq(0), 100_000).unwrap();
    let sequence: Vec<u64> = log.iter().map(|e| e.seq.0).collect();
    assert_eq!(
        sequence,
        (1..=log.len() as u64).collect::<Vec<_>>(),
        "gapless and ordered"
    );
    for snapshot in &snapshots {
        assert_eq!(
            projected(snapshot),
            replay(&log, snapshot.as_of_event_seq),
            "snapshot at cursor {:?} disagrees with the log",
            snapshot.as_of_event_seq
        );
    }
    let distinct_cursors: std::collections::BTreeSet<_> =
        snapshots.iter().map(|s| s.as_of_event_seq).collect();
    assert!(
        distinct_cursors.len() > COMMANDS as usize,
        "snapshots were spread over the run"
    );
    // Per-aggregate revisions in the log are strictly sequential: no lost or duplicate update.
    let mut last: BTreeMap<AggregateKey, u64> = BTreeMap::new();
    for event in &log {
        if let Some((aggregate, revision)) = &event.aggregate {
            let previous = last.insert(aggregate.clone(), revision.0).unwrap_or(0);
            assert_eq!(revision.0, previous + 1, "{aggregate:?}");
        }
    }
    assert_eq!(last[&key("own", 0)], COMMANDS + 1);
}

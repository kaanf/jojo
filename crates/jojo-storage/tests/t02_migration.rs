//! T02: versioned migrations; a failed migration leaves a usable prior database and a
//! verified backup, creates no epoch and starts no writer.

mod common;

use common::{TempDir, bump, clock, create, key, start};
use jojo_domain::identity::Revision;
use jojo_storage::{
    Engine, EngineConfig, EngineEpoch, EventSeq, Migration, MigrationPlan, ShutdownMarker,
    StorageError,
};

fn plan_with(extra: &[(&'static str, &'static str)]) -> MigrationPlan {
    let mut steps = MigrationPlan::production().steps().to_vec();
    for (name, sql) in extra {
        steps.push(Migration {
            version: steps.len() as u32 + 1,
            name,
            sql,
        });
    }
    MigrationPlan::new(steps).unwrap()
}

fn start_with(db: &std::path::Path, plan: MigrationPlan) -> Result<Engine, StorageError> {
    let mut config = EngineConfig::new(db, clock());
    config.migrations = plan;
    Engine::start(config)
}

const GOOD_NEXT: (&str, &str) = (
    "notes",
    "CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL) STRICT;
     INSERT INTO notes (body) SELECT kind FROM events;",
);
/// Valid DDL and DML first, then a statement that fails: the whole step must vanish.
const BROKEN_NEXT: (&str, &str) = (
    "broken",
    "CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT NOT NULL) STRICT;
     INSERT INTO notes (body) VALUES ('partial');
     UPDATE aggregates SET state = 'corrupted by migration', revision = revision + 1;
     CREATE TABLE aggregates (duplicate INTEGER);",
);

#[test]
fn a_fresh_database_is_created_at_the_latest_version_without_a_backup() {
    let dir = TempDir::new("fresh");
    let engine = start(&dir.db());
    let report = &engine.startup_report().migration;
    assert_eq!((report.from_version, report.to_version), (0, 12));
    assert_eq!(report.backup, None);
    assert!(engine.startup_report().sqlite_version_number >= 3_037_000);
    assert_eq!(
        engine
            .reader()
            .unwrap()
            .snapshot()
            .unwrap()
            .session
            .schema_version,
        12
    );
}

#[test]
fn a_failed_migration_preserves_the_prior_database_and_a_recoverable_backup() {
    let dir = TempDir::new("migration-failure");
    let counter = key("counter", 1);
    {
        let engine = start(&dir.db());
        create(&engine, 1, &counter);
        bump(&engine, 2, &counter, Revision(1)).unwrap();
        engine.shutdown().unwrap();
    }

    let failure = start_with(&dir.db(), plan_with(&[BROKEN_NEXT]));
    let Err(StorageError::MigrationFailed {
        from_version,
        failed_version,
        backup,
        cause,
    }) = failure
    else {
        panic!("the broken migration must fail the start");
    };
    assert_eq!((from_version, failed_version), (12, 13));
    assert!(cause.message.contains("already exists"), "{cause}");
    let backup = backup.expect("an existing database is backed up before migrating");
    assert!(backup.exists());

    // No worker survived the failed start (ownership is free again), no epoch was created
    // for it, and the prior version opens with exactly the prior data.
    let engine = start(&dir.db());
    let report = engine.startup_report();
    assert_eq!(
        (report.migration.from_version, report.migration.to_version),
        (12, 12)
    );
    assert_eq!(report.epoch.epoch, EngineEpoch(2));
    assert_eq!(
        report.epoch.previous,
        Some((EngineEpoch(1), ShutdownMarker::Clean))
    );
    let mut reader = engine.reader().unwrap();
    let snapshot = reader.snapshot().unwrap();
    assert_eq!(snapshot.aggregates.len(), 1);
    assert_eq!(
        (
            snapshot.aggregates[0].revision,
            snapshot.aggregates[0].state.as_str()
        ),
        (Revision(2), "2")
    );
    // Receipts survived: the original command replays instead of running again.
    let replay = bump(&engine, 2, &counter, Revision(1)).unwrap();
    assert!(replay.replayed && replay.epoch == EngineEpoch(1));
    let events_before_upgrade = reader.events_after(EventSeq(0), 100).unwrap().len();
    drop(reader);
    engine.shutdown().unwrap();

    // None of the broken step's DDL remained: a correct version 13 that creates the same
    // table applies cleanly afterwards (it would fail if `notes` had been left behind).
    let upgraded = start_with(&dir.db(), plan_with(&[GOOD_NEXT])).unwrap();
    let report = upgraded.startup_report();
    assert_eq!(
        (report.migration.from_version, report.migration.to_version),
        (12, 13)
    );
    let second_backup = report.migration.backup.clone().expect("upgrade backup");
    assert_ne!(
        second_backup, backup,
        "an earlier backup is never overwritten"
    );
    assert_eq!(
        upgraded.reader().unwrap().snapshot().unwrap().aggregates,
        snapshot.aggregates
    );
    assert!(events_before_upgrade > 0);
    drop(upgraded);

    // The first backup is a complete, independently usable version-12 database.
    let restored = TempDir::new("migration-restore");
    std::fs::copy(&backup, restored.db()).unwrap();
    let from_backup = start(&restored.db());
    assert_eq!(from_backup.startup_report().migration.from_version, 12);
    assert_eq!(from_backup.epoch(), EngineEpoch(2));
    let restored_snapshot = from_backup.reader().unwrap().snapshot().unwrap();
    assert_eq!(restored_snapshot.aggregates, snapshot.aggregates);
    assert!(
        bump(&from_backup, 2, &counter, Revision(1))
            .unwrap()
            .replayed
    );
}

#[test]
fn a_multi_step_upgrade_stops_at_the_last_whole_version() {
    let dir = TempDir::new("multi-step");
    start(&dir.db()).shutdown().unwrap();
    let failure = start_with(&dir.db(), plan_with(&[GOOD_NEXT, BROKEN_NEXT]));
    assert!(matches!(
        failure,
        Err(StorageError::MigrationFailed {
            from_version: 13,
            failed_version: 14,
            backup: Some(_),
            ..
        })
    ));
    // Version 13 was committed as a whole; a helper that knows version 13 can run.
    let engine = start_with(&dir.db(), plan_with(&[GOOD_NEXT])).unwrap();
    let report = &engine.startup_report().migration;
    assert_eq!(
        (
            report.from_version,
            report.to_version,
            report.backup.is_some()
        ),
        (13, 13, false)
    );
}

#[test]
fn a_newer_schema_is_refused_untouched() {
    let dir = TempDir::new("too-new");
    start_with(&dir.db(), plan_with(&[GOOD_NEXT]))
        .unwrap()
        .shutdown()
        .unwrap();
    let old_helper = Engine::start(EngineConfig::new(dir.db(), clock()));
    assert!(matches!(
        old_helper,
        Err(StorageError::SchemaTooNew {
            found: 13,
            supported: 12
        })
    ));
    // The refusal created no epoch and released ownership.
    let engine = start_with(&dir.db(), plan_with(&[GOOD_NEXT])).unwrap();
    assert_eq!(engine.epoch(), EngineEpoch(2));
}

#[test]
fn a_file_that_is_not_a_database_is_refused_and_left_byte_identical() {
    let dir = TempDir::new("not-a-database");
    let garbage = b"this is not a SQLite database, it is somebody's notes file\n".repeat(40);
    std::fs::write(dir.db(), &garbage).unwrap();
    let refused = Engine::start(EngineConfig::new(dir.db(), clock()));
    assert!(
        matches!(refused, Err(StorageError::Sqlite(_))),
        "{:?}",
        refused.err()
    );
    assert_eq!(std::fs::read(dir.db()).unwrap(), garbage);
}

#[test]
fn migration_plans_must_be_contiguous_from_version_one() {
    assert!(matches!(
        MigrationPlan::new(vec![]),
        Err(StorageError::InvalidMigrationPlan(_))
    ));
    let mut steps = MigrationPlan::production().steps().to_vec();
    steps.push(Migration {
        version: 14,
        name: "gap",
        sql: "",
    });
    assert!(matches!(
        MigrationPlan::new(steps),
        Err(StorageError::InvalidMigrationPlan(_))
    ));
    assert_eq!(MigrationPlan::production().latest(), 12);
}

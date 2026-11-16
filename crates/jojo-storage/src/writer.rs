//! The authoritative writer: one connection, one engine epoch, one transaction at a time.
//!
//! Correctness does not depend on callers being serialized in memory. Every command runs
//! in `BEGIN IMMEDIATE … COMMIT`, which takes SQLite's database-wide write lock first, and
//! every check (epoch fence, receipt lookup, revision compare-and-swap, claim conflicts) is
//! made inside that transaction against committed data. The queue in `engine.rs` only
//! keeps callers from contending on that lock.

use std::path::Path;
use std::sync::{Arc, Mutex};

use jojo_domain::time::Clock;

use crate::error::{CommandError, FailPoint, StorageError, TxError};
use crate::model::{
    CommandRequest, EngineEpoch, EngineSession, EventSeq, Receipt, ShutdownMarker, id_to_text,
    to_db,
};
use crate::queries;
use crate::sqlite::{Connection, OpenMode, Value};
use crate::tx::Tx;

/// How long a statement waits for SQLite's write lock before reporting `SQLITE_BUSY`.
/// This bounds lock contention only; it is never used to infer that another party died.
const BUSY_TIMEOUT_MS: u32 = 5_000;

/// Test-controlled failure injection. Empty (and inert) in production configurations.
#[derive(Clone, Debug, Default)]
pub struct FaultPlan(Arc<Mutex<Vec<FailPoint>>>);

impl FaultPlan {
    /// Makes the next transaction that reaches `point` fail there, once.
    pub fn arm(&self, point: FailPoint) {
        self.0.lock().expect("fault plan poisoned").push(point);
    }

    fn fire(&self, point: FailPoint) -> Result<(), CommandError> {
        let mut armed = self.0.lock().expect("fault plan poisoned");
        match armed.iter().position(|candidate| *candidate == point) {
            Some(index) => {
                armed.remove(index);
                Err(CommandError::InjectedFault(point))
            }
            None => Ok(()),
        }
    }
}

/// Opens the read-write connection used for migration and by the writer.
///
/// `recognise` runs before the only persistent setting (the journal mode) is changed, so
/// a file that turns out not to be a Jojo database is refused without being modified.
pub(crate) fn open_writer_connection(
    db_path: &Path,
    recognise: impl FnOnce(&Connection) -> Result<(), StorageError>,
) -> Result<Connection, StorageError> {
    let conn = Connection::open(db_path, OpenMode::Create)?;
    conn.set_busy_timeout(BUSY_TIMEOUT_MS)?;
    recognise(&conn)?;
    let journal = conn.query_one("PRAGMA journal_mode = WAL", &[], |row| row.text(0))?;
    if journal != "wal" {
        return Err(StorageError::corrupt(format!(
            "write-ahead logging unavailable (journal mode {journal})"
        )));
    }
    // FULL + F_FULLFSYNC: a reported commit has been flushed through the drive cache on macOS.
    conn.execute_batch(
        "PRAGMA synchronous = FULL; PRAGMA fullfsync = ON; PRAGMA checkpoint_fullfsync = ON;
         PRAGMA foreign_keys = ON;",
    )?;
    require_foreign_keys(&conn)?;
    Ok(conn)
}

/// Opens a query-only connection for consistent snapshots.
pub(crate) fn open_reader_connection(db_path: &Path) -> Result<Connection, StorageError> {
    let conn = Connection::open(db_path, OpenMode::Existing)?;
    conn.set_busy_timeout(BUSY_TIMEOUT_MS)?;
    conn.execute_batch("PRAGMA query_only = ON; PRAGMA foreign_keys = ON;")?;
    Ok(conn)
}

fn require_foreign_keys(conn: &Connection) -> Result<(), StorageError> {
    if conn.query_one("PRAGMA foreign_keys", &[], |row| row.int(0))? != 1 {
        return Err(StorageError::corrupt(
            "SQLite foreign key enforcement is unavailable",
        ));
    }
    Ok(())
}

/// What establishing a new epoch found and did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EpochReport {
    pub epoch: EngineEpoch,
    /// The preceding session as it was sealed (`Clean`, or `Unclean` if it never recorded
    /// a clean shutdown). `None` for the first epoch of a database.
    pub previous: Option<(EngineEpoch, ShutdownMarker)>,
    /// Claims that were still `ACTIVE` under an earlier epoch and are now `QUARANTINED`.
    pub quarantined_claims: u64,
}

pub(crate) struct Writer {
    conn: Connection,
    epoch: EngineEpoch,
    clock: Arc<dyn Clock>,
    faults: FaultPlan,
    stopped: bool,
}

enum Step {
    Commit(Receipt),
    Replay(Receipt),
}

impl Writer {
    /// Persists a new engine epoch and returns the writer bound to it. In one transaction:
    /// seal the previous session, insert the next epoch with admission closed, quarantine
    /// every claim an earlier epoch left active, and append an `EngineStarted` event.
    /// Requires the ownership lock; runs before anything else can write.
    pub(crate) fn establish(
        conn: Connection,
        clock: Arc<dyn Clock>,
        faults: FaultPlan,
        schema_version: u32,
    ) -> Result<(Self, EpochReport), StorageError> {
        let now = clock.now().unix_millis;
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<EpochReport, StorageError> {
            let previous = queries::latest_session(&conn)?;
            let mut sealed = None;
            if let Some(EngineSession {
                epoch, shutdown, ..
            }) = previous
            {
                let marker = if shutdown == ShutdownMarker::Running {
                    conn.execute(
                        "UPDATE engine_sessions
                         SET shutdown = 'UNCLEAN', admission = 'CLOSED', ended_at_unix_ms = ?1
                         WHERE epoch = ?2",
                        &[Value::Int(now), Value::Int(to_db(epoch.0)?)],
                    )?;
                    ShutdownMarker::Unclean
                } else {
                    shutdown
                };
                sealed = Some((epoch, marker));
            }
            let epoch = EngineEpoch(sealed.map_or(1, |(previous, _)| previous.0 + 1));
            conn.execute(
                "INSERT INTO engine_sessions
                     (epoch, started_at_unix_ms, owner_pid, schema_version, admission, shutdown)
                 VALUES (?1, ?2, ?3, ?4, 'CLOSED', 'RUNNING')",
                &[
                    Value::Int(to_db(epoch.0)?),
                    Value::Int(now),
                    Value::Int(i64::from(std::process::id())),
                    Value::Int(i64::from(schema_version)),
                ],
            )?;
            // The previous owner is gone, but processes it launched may not be. Their
            // claims stay live as QUARANTINED; only recorded evidence releases them.
            let quarantined_claims = conn.execute(
                "UPDATE resource_claims SET state = 'QUARANTINED' WHERE state = 'ACTIVE'",
                &[],
            )?;
            let payload = format!(
                "previous_epoch={};previous_shutdown={};quarantined_claims={quarantined_claims}",
                sealed.map_or(0, |(previous, _)| previous.0),
                sealed.map_or("NONE", |(_, marker)| marker.as_db()),
            );
            conn.execute(
                "INSERT INTO events (epoch, kind, payload, recorded_at_unix_ms)
                 VALUES (?1, 'EngineStarted', ?2, ?3)",
                &[
                    Value::Int(to_db(epoch.0)?),
                    Value::Text(&payload),
                    Value::Int(now),
                ],
            )?;
            conn.execute_batch("COMMIT")?;
            Ok(EpochReport {
                epoch,
                previous: sealed,
                quarantined_claims,
            })
        })();
        let report = match result {
            Ok(report) => report,
            Err(error) => {
                if conn.in_transaction() {
                    let _ = conn.execute_batch("ROLLBACK");
                }
                return Err(error);
            }
        };
        let writer = Self {
            conn,
            epoch: report.epoch,
            clock,
            faults,
            stopped: false,
        };
        Ok((writer, report))
    }

    /// Executes one command atomically and idempotently.
    ///
    /// * same ID, same request → the stored receipt, handler not run, nothing written;
    /// * same ID, different request → `CommandIdConflict`;
    /// * handler error or injected fault → full rollback, no receipt;
    /// * success → handler mutations and the receipt commit together.
    pub(crate) fn apply<F>(
        &mut self,
        request: &CommandRequest,
        handler: F,
    ) -> Result<Receipt, CommandError>
    where
        F: FnOnce(&mut Tx<'_>) -> Result<String, TxError>,
    {
        if self.stopped {
            return Err(CommandError::WriterStopped);
        }
        if request.epoch != self.epoch {
            // A request (for example a late callback) addressed to another engine epoch.
            return Err(CommandError::StaleEpoch {
                command_epoch: request.epoch,
                writer_epoch: self.epoch,
                current_epoch: self.epoch,
            });
        }
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        match self.apply_in_transaction(request, handler) {
            Ok(Step::Commit(receipt)) => match self.conn.execute_batch("COMMIT") {
                Ok(()) => Ok(receipt),
                Err(error) => {
                    // The commit did not happen. Stop writing: a failing commit means the
                    // storage is not trustworthy enough to keep admitting mutations.
                    self.rollback();
                    self.stopped = true;
                    Err(error.into())
                }
            },
            Ok(Step::Replay(receipt)) => {
                self.rollback();
                Ok(receipt)
            }
            Err(error) => {
                self.rollback();
                Err(error)
            }
        }
    }

    fn apply_in_transaction<F>(
        &mut self,
        request: &CommandRequest,
        handler: F,
    ) -> Result<Step, CommandError>
    where
        F: FnOnce(&mut Tx<'_>) -> Result<String, TxError>,
    {
        self.fence(request.epoch)?;
        let digest = request.digest();
        if let Some((receipt, stored_digest)) = queries::receipt(&self.conn, request.id)? {
            return if stored_digest == digest {
                Ok(Step::Replay(receipt))
            } else {
                Err(CommandError::CommandIdConflict(request.id))
            };
        }
        let now = self.clock.now();
        let now_unix_ms = now.unix_millis;
        let command_id = id_to_text(request.id.0);
        let mut tx = Tx {
            conn: &self.conn,
            epoch: self.epoch,
            now_unix_ms,
            now,
            command_id: command_id.clone(),
        };
        let response = handler(&mut tx).map_err(CommandError::Rejected)?;
        self.faults.fire(FailPoint::BeforeReceipt)?;
        let event_seq: EventSeq = queries::event_cursor(&self.conn)?;
        self.conn.execute(
            "INSERT INTO commands (command_id, kind, request_digest, epoch, response, event_seq,
                                   committed_at_unix_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            &[
                Value::Text(&command_id),
                Value::Text(&request.kind),
                Value::Text(&digest),
                Value::Int(to_db(self.epoch.0)?),
                Value::Text(&response),
                Value::Int(to_db(event_seq.0)?),
                Value::Int(now_unix_ms),
            ],
        )?;
        self.faults.fire(FailPoint::BeforeCommit)?;
        Ok(Step::Commit(Receipt {
            command_id: request.id,
            epoch: self.epoch,
            response,
            event_seq,
            replayed: false,
        }))
    }

    /// Database-backed fence, evaluated while holding the write lock: this writer may
    /// mutate only while its epoch is the newest session and still running. If a newer
    /// epoch exists this writer is a leftover of a superseded engine and stops for good.
    fn fence(&mut self, command_epoch: EngineEpoch) -> Result<(), CommandError> {
        let current = queries::latest_session(&self.conn)?
            .ok_or_else(|| StorageError::corrupt("no engine session"))?;
        if current.epoch == self.epoch && current.shutdown == ShutdownMarker::Running {
            return Ok(());
        }
        self.stopped = true;
        Err(CommandError::StaleEpoch {
            command_epoch,
            writer_epoch: self.epoch,
            current_epoch: current.epoch,
        })
    }

    fn rollback(&mut self) {
        if self.conn.in_transaction() && self.conn.execute_batch("ROLLBACK").is_err() {
            self.stopped = true;
        }
    }

    /// Seals this session as cleanly shut down and stops the writer. Refused while this
    /// engine still has live claims: unsettled work is not a clean shutdown, and the next
    /// start must then treat the session as unclean and quarantine those claims.
    pub(crate) fn mark_clean_shutdown(&mut self) -> Result<(), StorageError> {
        if self.stopped {
            return Err(StorageError::WriterStopped);
        }
        let now = self.clock.now().unix_millis;
        self.conn.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<(), StorageError> {
            let current = queries::latest_session(&self.conn)?
                .ok_or_else(|| StorageError::corrupt("no engine session"))?;
            if current.epoch != self.epoch || current.shutdown != ShutdownMarker::Running {
                return Err(StorageError::WriterStopped);
            }
            let live = queries::live_claims(&self.conn, None)?.len() as u64;
            if live > 0 {
                return Err(StorageError::LiveClaimsAtShutdown(live));
            }
            let epoch = Value::Int(to_db(self.epoch.0)?);
            self.conn.execute(
                "UPDATE engine_sessions
                 SET shutdown = 'CLEAN', admission = 'CLOSED', ended_at_unix_ms = ?1
                 WHERE epoch = ?2",
                &[Value::Int(now), epoch],
            )?;
            self.conn.execute(
                "INSERT INTO events (epoch, kind, payload, recorded_at_unix_ms)
                 VALUES (?1, 'EngineStopped', 'shutdown=CLEAN', ?2)",
                &[epoch, Value::Int(now)],
            )?;
            self.conn.execute_batch("COMMIT")?;
            Ok(())
        })();
        if result.is_err() {
            self.rollback();
        }
        // Whatever happened, this writer accepts no further commands.
        self.stopped = true;
        result
    }

    #[cfg(test)]
    pub(crate) fn attach_for_test(
        db_path: &Path,
        epoch: EngineEpoch,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            conn: open_writer_connection(db_path, |_| Ok(())).unwrap(),
            epoch,
            clock,
            faults: FaultPlan::default(),
            stopped: false,
        }
    }
}

#[cfg(test)]
mod tests {
    //! These tests deliberately bypass the ownership lock and the writer queue and drive
    //! two independent connections at once. They prove that the *database transaction* —
    //! not an in-memory mutex — is what decides compare-and-swap and idempotency races.

    use std::sync::Barrier;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use jojo_domain::identity::{ClockEpochId, OpaqueId, Revision};
    use jojo_domain::time::ClockReading;
    use jojo_test_support::FakeClock;

    use super::*;
    use crate::model::{AggregateKey, CommandId};
    use crate::schema::{MigrationPlan, migrate};

    fn clock() -> Arc<dyn Clock> {
        Arc::new(FakeClock::new(ClockReading {
            unix_millis: 1_000,
            monotonic_nanos: 0,
            epoch: ClockEpochId(OpaqueId(1)),
        }))
    }

    struct Fixture {
        directory: std::path::PathBuf,
        first: Writer,
        second: Writer,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    fn two_writers(name: &str) -> Fixture {
        let directory =
            std::env::temp_dir().join(format!("jojo-writer-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let db_path = directory.join("jojo.sqlite3");
        let conn = open_writer_connection(&db_path, |_| Ok(())).unwrap();
        migrate(&conn, &db_path, &MigrationPlan::production()).unwrap();
        let (first, report) = Writer::establish(conn, clock(), FaultPlan::default(), 1).unwrap();
        let second = Writer::attach_for_test(&db_path, report.epoch, clock());
        Fixture {
            directory,
            first,
            second,
        }
    }

    fn command(id: u128, kind: &str, payload: &str) -> CommandRequest {
        CommandRequest::new(CommandId(OpaqueId(id)), EngineEpoch(1), kind, payload)
    }

    #[test]
    fn two_connections_racing_on_one_revision_yield_exactly_one_winner_every_round() {
        let mut fixture = two_writers("cas");
        let key = AggregateKey::new("counter", OpaqueId(7));
        let create_key = key.clone();
        fixture
            .first
            .apply(&command(1, "create", ""), move |tx| {
                tx.create_aggregate(&create_key, "0")?;
                Ok(String::new())
            })
            .unwrap();
        let Fixture { first, second, .. } = &mut fixture;
        for round in 0..40u64 {
            let expected = Revision(round + 1);
            let barrier = Barrier::new(2);
            let outcomes = std::thread::scope(|scope| {
                let handles: Vec<_> = [(&mut *first, 0u128), (&mut *second, 1u128)]
                    .into_iter()
                    .map(|(writer, side)| {
                        let (key, barrier) = (key.clone(), &barrier);
                        scope.spawn(move || {
                            let id = 1_000 + u128::from(round) * 2 + side;
                            barrier.wait();
                            writer.apply(&command(id, "bump", &side.to_string()), move |tx| {
                                tx.update_aggregate(&key, expected, &side.to_string())?;
                                Ok(side.to_string())
                            })
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|handle| handle.join().unwrap())
                    .collect::<Vec<_>>()
            });
            let winners = outcomes.iter().filter(|outcome| outcome.is_ok()).count();
            assert_eq!(winners, 1, "round {round}: {outcomes:?}");
            let loser = outcomes.iter().find_map(|o| o.as_ref().err()).unwrap();
            assert!(
                matches!(
                    loser,
                    CommandError::Rejected(TxError::RevisionConflict { actual: Some(actual), .. })
                        if *actual == Revision(expected.0 + 1)
                ),
                "round {round}: {loser:?}"
            );
        }
        let stored = queries::aggregate(&fixture.first.conn, &key)
            .unwrap()
            .unwrap();
        assert_eq!(stored.revision, Revision(41));
        // The loser of every round left no receipt behind: 1 create + 40 winners.
        let receipts = fixture
            .first
            .conn
            .query_one("SELECT count(*) FROM commands", &[], |r| r.int(0))
            .unwrap();
        assert_eq!(receipts, 41);
    }

    #[test]
    fn two_connections_submitting_one_command_run_its_effects_once() {
        let mut fixture = two_writers("dedupe");
        let key = AggregateKey::new("counter", OpaqueId(9));
        let runs = AtomicUsize::new(0);
        let barrier = Barrier::new(2);
        let Fixture { first, second, .. } = &mut fixture;
        let receipts = std::thread::scope(|scope| {
            let handles: Vec<_> = [first, second]
                .into_iter()
                .map(|writer| {
                    let (key, runs, barrier) = (key.clone(), &runs, &barrier);
                    scope.spawn(move || {
                        barrier.wait();
                        writer
                            .apply(&command(5, "create", "payload"), move |tx| {
                                runs.fetch_add(1, Ordering::SeqCst);
                                tx.create_aggregate(&key, "made")?;
                                tx.append_event("Created", Some((&key, Revision(1))), "")?;
                                Ok("created".into())
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
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        assert_eq!(receipts.iter().filter(|r| r.replayed).count(), 1);
        assert_eq!(receipts[0].response, receipts[1].response);
        assert_eq!(receipts[0].event_seq, receipts[1].event_seq);
        let events = queries::events_after(&fixture.first.conn, EventSeq(0), 100).unwrap();
        assert_eq!(events.iter().filter(|e| e.kind == "Created").count(), 1);
    }
}

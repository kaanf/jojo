//! Engine lifetime: ownership → migration → new epoch (admission closed) → writer queue.
//!
//! The engine runs one writer thread that drains a queue of commands; callers on any
//! number of threads submit commands and block until theirs has committed or failed. The
//! thread serializes *state transactions only*: nothing about it limits how many attempts,
//! providers or repositories those transactions describe. Reads never go through the queue.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread::JoinHandle;

use jojo_domain::time::Clock;

use crate::error::{CommandError, StorageError, TxError};
use crate::model::{
    AggregateKey, AggregateRecord, ClaimRecord, CommandId, CommandRequest, EffectRecord,
    EngineEpoch, EngineSession, EventRecord, EventSeq, Receipt, Snapshot,
};
use crate::ownership::OwnershipLock;
use crate::queries;
use crate::schema::{MigrationPlan, MigrationReport, migrate, recognise};
use crate::sqlite::{Connection, library_version_number};
use crate::tx::Tx;
use crate::writer::{
    EpochReport, FaultPlan, Writer, open_reader_connection, open_writer_connection,
};

pub struct EngineConfig {
    pub db_path: PathBuf,
    /// Supplies audit timestamps only. Time never drives ownership, fencing or releases.
    pub clock: Arc<dyn Clock>,
    pub migrations: MigrationPlan,
    pub faults: FaultPlan,
}

impl EngineConfig {
    pub fn new(db_path: impl Into<PathBuf>, clock: Arc<dyn Clock>) -> Self {
        Self {
            db_path: db_path.into(),
            clock,
            migrations: MigrationPlan::production(),
            faults: FaultPlan::default(),
        }
    }
}

/// What starting the engine found and did. Recovery inspection (P1-11) consumes this.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StartupReport {
    pub epoch: EpochReport,
    pub migration: MigrationReport,
    pub sqlite_version_number: i32,
}

type Job = Box<dyn FnOnce(&mut Writer) + Send>;

/// The single owner and writer of one database for the lifetime of this value.
///
/// While an engine is alive, code in the same process must not open the database file
/// directly (`std::fs`): closing such a descriptor would drop SQLite's POSIX locks on it.
/// All access goes through this engine and its readers.
pub struct Engine {
    // Field order is drop order after `Drop::drop`: the lock is released last, after the
    // writer thread has been joined and its connection closed.
    jobs: Option<mpsc::Sender<Job>>,
    writer_thread: Option<JoinHandle<()>>,
    report: StartupReport,
    lock: OwnershipLock,
}

impl Engine {
    /// Becomes the owner of the database or fails without touching it.
    ///
    /// Order matters: (1) take the OS ownership lock; (2) open and migrate — on failure no
    /// epoch is created and no writer starts; (3) persist the new epoch with admission
    /// closed; (4) only then start accepting commands.
    pub fn start(config: EngineConfig) -> Result<Self, StorageError> {
        let mut lock = OwnershipLock::acquire(&config.db_path)?;
        let conn = open_writer_connection(lock.db_path(), |conn| {
            recognise(conn, &config.migrations).map(drop)
        })?;
        let migration = migrate(&conn, lock.db_path(), &config.migrations)?;
        let (writer, epoch) =
            Writer::establish(conn, config.clock, config.faults, migration.to_version)?;
        lock.describe_owner(&format!(
            "pid={} epoch={}\n",
            std::process::id(),
            epoch.epoch.0
        ))?;
        let (jobs, queue) = mpsc::channel::<Job>();
        let writer_thread = std::thread::Builder::new()
            .name("jojo-writer".into())
            .spawn(move || {
                let mut writer = writer;
                for job in queue {
                    job(&mut writer);
                }
            })
            .map_err(|e| StorageError::io("starting writer thread", e))?;
        Ok(Self {
            jobs: Some(jobs),
            writer_thread: Some(writer_thread),
            report: StartupReport {
                epoch,
                migration,
                sqlite_version_number: library_version_number(),
            },
            lock,
        })
    }

    pub fn epoch(&self) -> EngineEpoch {
        self.report.epoch.epoch
    }

    pub fn startup_report(&self) -> &StartupReport {
        &self.report
    }

    pub fn db_path(&self) -> &Path {
        self.lock.db_path()
    }

    pub fn lock_path(&self) -> &Path {
        self.lock.lock_path()
    }

    /// Submits a command to the writer and waits for its durable outcome.
    ///
    /// The handler runs on the writer thread inside the transaction. It must only use the
    /// [`Tx`] it is given; if it panics, the transaction is rolled back and the writer stops
    /// (later commands fail with `WriterStopped`) rather than continuing in an unknown state.
    pub fn execute<F>(&self, request: CommandRequest, handler: F) -> Result<Receipt, CommandError>
    where
        F: FnOnce(&mut Tx<'_>) -> Result<String, TxError> + Send + 'static,
    {
        self.submit(move |writer| writer.apply(&request, handler))
            .unwrap_or(Err(CommandError::WriterStopped))
    }

    /// Records a clean shutdown and releases ownership. If this is not called (crash, kill,
    /// drop) the session stays unsealed and the next start treats it as unclean.
    pub fn shutdown(self) -> Result<(), StorageError> {
        self.submit(Writer::mark_clean_shutdown)
            .unwrap_or(Err(StorageError::WriterStopped))
    }

    /// A new query-only connection for consistent snapshots and event replay.
    pub fn reader(&self) -> Result<Reader, StorageError> {
        Ok(Reader {
            conn: open_reader_connection(self.lock.db_path())?,
        })
    }

    fn submit<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Writer) -> T + Send + 'static,
    ) -> Option<T> {
        let (reply, outcome) = mpsc::sync_channel(1);
        let job: Job = Box::new(move |writer| {
            let _ = reply.send(work(writer));
        });
        self.jobs.as_ref()?.send(job).ok()?;
        // `recv` fails only if the writer thread died (handler panic) before replying.
        outcome.recv().ok()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Close the queue, let the writer finish the job it is running, and close its
        // connection before the ownership lock (a later field) is released.
        self.jobs.take();
        if let Some(thread) = self.writer_thread.take() {
            let _ = thread.join();
        }
    }
}

/// Query-only access. Each `read` call is one SQLite read transaction, i.e. one consistent
/// point-in-time view that concurrent commits cannot change.
pub struct Reader {
    conn: Connection,
}

impl Reader {
    /// Runs `body` against a single consistent view of the database.
    pub fn read<T>(
        &mut self,
        body: impl FnOnce(&ReadView<'_>) -> Result<T, StorageError>,
    ) -> Result<T, StorageError> {
        if self.conn.in_transaction() {
            // A previous `body` panicked mid-read; discard its stale view.
            self.conn.execute_batch("ROLLBACK")?;
        }
        self.conn.execute_batch("BEGIN")?;
        let result = body(&ReadView { conn: &self.conn });
        let ended = self.conn.execute_batch("ROLLBACK");
        let value = result?;
        ended?;
        Ok(value)
    }

    /// Projections plus the event cursor they are consistent with.
    pub fn snapshot(&mut self) -> Result<Snapshot, StorageError> {
        self.read(|view| {
            Ok(Snapshot {
                session: view.session()?,
                as_of_event_seq: view.event_cursor()?,
                aggregates: view.aggregates()?,
                live_claims: view.live_claims()?,
                effects: view.effects()?,
            })
        })
    }

    /// Up to `limit` committed events after `cursor`, in order and without gaps.
    pub fn events_after(
        &mut self,
        cursor: EventSeq,
        limit: u32,
    ) -> Result<Vec<EventRecord>, StorageError> {
        self.read(|view| view.events_after(cursor, limit))
    }

    /// The stored receipt of a committed command, if any. Lets a client that lost a
    /// response learn the outcome instead of re-issuing the action.
    pub fn receipt(&mut self, id: CommandId) -> Result<Option<Receipt>, StorageError> {
        self.read(|view| view.receipt(id))
    }
}

/// One consistent point-in-time view. All methods observe the same committed state.
pub struct ReadView<'r> {
    pub(crate) conn: &'r Connection,
}

impl ReadView<'_> {
    pub fn session(&self) -> Result<EngineSession, StorageError> {
        queries::latest_session(self.conn)?
            .ok_or_else(|| StorageError::corrupt("no engine session"))
    }

    pub fn event_cursor(&self) -> Result<EventSeq, StorageError> {
        queries::event_cursor(self.conn)
    }

    pub fn aggregate(&self, key: &AggregateKey) -> Result<Option<AggregateRecord>, StorageError> {
        queries::aggregate(self.conn, key)
    }

    pub fn aggregates(&self) -> Result<Vec<AggregateRecord>, StorageError> {
        queries::aggregates(self.conn)
    }

    pub fn live_claims(&self) -> Result<Vec<ClaimRecord>, StorageError> {
        queries::live_claims(self.conn, None)
    }

    pub fn effects(&self) -> Result<Vec<EffectRecord>, StorageError> {
        queries::effects(self.conn, None)
    }

    pub fn events_after(
        &self,
        cursor: EventSeq,
        limit: u32,
    ) -> Result<Vec<EventRecord>, StorageError> {
        queries::events_after(self.conn, cursor, limit)
    }

    pub fn receipt(&self, id: CommandId) -> Result<Option<Receipt>, StorageError> {
        Ok(queries::receipt(self.conn, id)?.map(|(receipt, _)| receipt))
    }
}

#[cfg(test)]
mod tests {
    use jojo_domain::identity::{ClockEpochId, OpaqueId};
    use jojo_domain::time::ClockReading;
    use jojo_test_support::FakeClock;

    use super::*;
    use crate::sqlite::OpenMode;

    #[test]
    fn a_foreign_sqlite_database_is_refused_and_left_byte_identical() {
        let directory =
            std::env::temp_dir().join(format!("jojo-engine-foreign-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let db_path = directory.join("somebody-elses.sqlite3");
        {
            let foreign = Connection::open(&db_path, OpenMode::Create).unwrap();
            foreign
                .execute_batch("CREATE TABLE theirs (x INTEGER); INSERT INTO theirs VALUES (1);")
                .unwrap();
        }
        let before = std::fs::read(&db_path).unwrap();
        let clock = Arc::new(FakeClock::new(ClockReading {
            unix_millis: 0,
            monotonic_nanos: 0,
            epoch: ClockEpochId(OpaqueId(1)),
        }));
        let refused = Engine::start(EngineConfig::new(&db_path, clock));
        assert!(matches!(refused, Err(StorageError::ForeignDatabase)));
        // Not even the journal mode was switched: no WAL sidecar, identical bytes.
        assert_eq!(std::fs::read(&db_path).unwrap(), before);
        assert!(!directory.join("somebody-elses.sqlite3-wal").exists());
        std::fs::remove_dir_all(&directory).unwrap();
    }
}

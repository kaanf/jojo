//! Structured errors. Three layers: infrastructure (`StorageError`), a rejected mutation
//! inside a transaction (`TxError`) and the outcome of a submitted command (`CommandError`).

use std::fmt;
use std::path::PathBuf;

use jojo_domain::identity::Revision;

use crate::model::{AggregateKey, ClaimId, CommandId, EffectState, EngineEpoch};
use crate::sqlite::SqliteError;

#[derive(Debug)]
pub enum StorageError {
    /// Another live helper holds the OS ownership lock for this database.
    OwnershipHeld {
        lock_path: PathBuf,
        /// Informational owner metadata read from the lock file; never used for decisions.
        owner_hint: String,
    },
    Io {
        context: String,
        source: std::io::Error,
    },
    Sqlite(SqliteError),
    /// The file is a SQLite database that was not created by Jojo.
    ForeignDatabase,
    SchemaTooNew {
        found: u32,
        supported: u32,
    },
    /// R-04: a database written before the typed payloads is never upgraded. The helper
    /// names the file; the human moves it aside and Jojo starts a new one. It is never
    /// deleted or changed here.
    SchemaTooOld {
        found: u32,
        oldest: u32,
        path: PathBuf,
    },
    InvalidMigrationPlan(String),
    /// A migration step failed and was rolled back. The database is still at `from_version`
    /// and usable by a helper that supports it; `backup` is a verified copy taken before
    /// any step ran (absent only when the database was brand new).
    MigrationFailed {
        from_version: u32,
        failed_version: u32,
        backup: Option<PathBuf>,
        cause: SqliteError,
    },
    /// Stored data violates an invariant this code relies on.
    Corrupt(String),
    DataRootNotPrivate(PathBuf),
    /// A clean shutdown was refused because this many claims are still live.
    LiveClaimsAtShutdown(u64),
    /// The single writer has stopped (fenced, failed or shut down); nothing more is written.
    WriterStopped,
}

impl StorageError {
    pub(crate) fn corrupt(message: impl Into<String>) -> Self {
        Self::Corrupt(message.into())
    }

    pub(crate) fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

impl From<SqliteError> for StorageError {
    fn from(error: SqliteError) -> Self {
        Self::Sqlite(error)
    }
}

/// A mutation refused inside a writer transaction. Returning it from a command handler
/// rolls the whole transaction back.
#[derive(Debug)]
pub enum TxError {
    /// Compare-and-swap failed. `actual` is `None` when the aggregate does not exist.
    RevisionConflict {
        key: AggregateKey,
        expected: Revision,
        actual: Option<Revision>,
    },
    AggregateExists(AggregateKey),
    /// The requested claim conflicts with live (active or quarantined) claims.
    ClaimConflict {
        resource_key: String,
        blocking: Vec<ClaimId>,
    },
    ClaimNotLive(ClaimId),
    EffectExists(String),
    EffectMissing(String),
    InvalidEffectTransition {
        operation_key: String,
        from: EffectState,
        to: EffectState,
    },
    /// A domain-level refusal decided by the command handler.
    Rejected(String),
    Storage(StorageError),
}

impl From<StorageError> for TxError {
    fn from(error: StorageError) -> Self {
        Self::Storage(error)
    }
}

impl From<SqliteError> for TxError {
    fn from(error: SqliteError) -> Self {
        Self::Storage(error.into())
    }
}

/// Points at which a test can force a writer transaction to fail before it commits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailPoint {
    /// After the handler's mutations, before the command receipt is written.
    BeforeReceipt,
    /// After the receipt is written, immediately before `COMMIT`.
    BeforeCommit,
}

#[derive(Debug)]
pub enum CommandError {
    /// The request or the writer belongs to an engine epoch that is no longer current.
    StaleEpoch {
        command_epoch: EngineEpoch,
        writer_epoch: EngineEpoch,
        current_epoch: EngineEpoch,
    },
    /// The command ID was already committed with a different request.
    CommandIdConflict(CommandId),
    /// The handler refused; nothing was committed and no receipt exists.
    Rejected(TxError),
    InjectedFault(FailPoint),
    WriterStopped,
    Storage(StorageError),
}

impl From<StorageError> for CommandError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::WriterStopped => Self::WriterStopped,
            other => Self::Storage(other),
        }
    }
}

impl From<SqliteError> for CommandError {
    fn from(error: SqliteError) -> Self {
        Self::Storage(error.into())
    }
}

macro_rules! debug_display {
    ($($type:ty),+) => {
        $(
            impl fmt::Display for $type {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    write!(f, "{self:?}")
                }
            }

            impl std::error::Error for $type {}
        )+
    };
}

debug_display!(StorageError, TxError, CommandError);

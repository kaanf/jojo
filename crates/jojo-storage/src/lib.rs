//! Authoritative SQLite persistence for the Jojo helper (P1-02).
//!
//! One helper process owns one database: an OS lock admits a single owner, every start
//! persists a new engine epoch with admission closed, and all mutations go through one
//! writer whose transactions carry revision compare-and-swap, command receipts, events,
//! resource claims and outbox records atomically.
//!
//! P1-03 adds immutable artifact versions with content/rendered digests and persisted
//! clarification questions/answers on the same primitives (`artifacts`).
//!
//! P1-06 adds transactional resource admission and persisted fairness (`scheduler`).
//! P1-07 adds owned local Git worktrees and candidate packets (`worktrees`).
//! P1-09 adds the launch issue journal and per-attempt runner (`launch`) over OS process
//! supervision with bounded raw-stream spooling (`process`).
//! P1-10 adds ordered settlement into immutable results with content-addressed evidence
//! and the attempt effect journal (`outcomes`, `evidence`) plus independent validation
//! receipts and candidate acceptance (`validation`). P1-11 adds reopen inspection, recovery
//! resolution, the recovery start gate on admission and reviewed continuation plans
//! (`recovery`). P1-12 adds close tickets, the close gate over admission and the shutdown
//! coordinator with READY/FAILED receipts (`shutdown`). Not provided here: IPC. A committed outbox
//! record or claim is a durable *intent*; SQLite commits are never atomic with process
//! launches or filesystem writes.

mod approvals;
mod artifacts;
pub use approvals::*;
mod scheduler;
pub use scheduler::*;
mod dependencies;
pub use dependencies::*;
mod digest;
mod engine;
mod error;
mod model;
mod ownership;
mod queries;
mod schema;
mod sqlite;
mod tx;
mod writer;

pub use artifacts::{
    ARTIFACT_KIND, Answer, ArtifactType, ArtifactVersion, Author, MAX_TEXT_BYTES, PreparedPrd,
    Provenance, QUESTION_KIND, Question, QuestionDisposition, ReadinessBlocker, ReviewReadiness,
};
pub use digest::sha256_hex;
pub use engine::{Engine, EngineConfig, ReadView, Reader, StartupReport};
pub use error::{CommandError, FailPoint, StorageError, TxError};
pub use model::{
    Admission, AggregateKey, AggregateRecord, ClaimId, ClaimMode, ClaimRecord, ClaimState,
    CommandId, CommandRequest, EffectRecord, EffectState, EngineEpoch, EngineSession, EventRecord,
    EventSeq, Receipt, ReleaseBasis, ShutdownMarker, Snapshot,
};
pub use ownership::{DATABASE_FILE_NAME, OwnershipLock, prepare_data_root};
pub use schema::{APPLICATION_ID, Migration, MigrationPlan, MigrationReport};
pub use sqlite::{SqliteError, library_version_number as sqlite_version_number};
pub use tx::Tx;
pub use writer::{EpochReport, FaultPlan};

mod worktrees;
pub use worktrees::*;

mod launch;
pub use launch::*;
mod process;
pub use process::*;
mod claude;
pub use claude::*;
mod codex;
pub use codex::*;
mod discovery;
mod external;
pub use external::*;
pub mod remote;
pub use discovery::{Discovery, FoundExecutable, Lookup, command_name, profiled_version};
mod executable_records;
pub use executable_records::{ExecutableChange, ExecutableRecord};
mod executables;
pub use executables::{ExecutableCheck, begin_check, executable_digest};
mod slice;
pub use slice::*;
mod projects;
mod runner;
pub use runner::*;
mod details;
pub use details::project_detail;
mod features;
mod overview;
mod repairs;
pub use features::FEATURE_ROWS;
pub use overview::OVERVIEW_ROWS;
pub use repairs::*;

mod turns;
pub use turns::*;

mod evidence;
pub use evidence::*;
mod outcomes;
pub use outcomes::*;
mod validation;
pub use validation::*;
mod validator;
pub use validator::*;
mod recovery;
pub use recovery::*;
mod shutdown;
pub use shutdown::*;

mod ipc_queries;

//! Storage-level identities and records. Opaque payloads are text owned by later tasks.

use jojo_domain::identity::{OpaqueId, Revision};

use crate::digest::sha256_hex;
use crate::error::StorageError;

/// One helper lifetime. Persisted before any admission; strictly increasing per database.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EngineEpoch(pub u64);

/// Position in the global append-only event order. `EventSeq(0)` precedes every event.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EventSeq(pub u64);

/// Client-chosen idempotency key of one mutating command.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CommandId(pub OpaqueId);

/// Durable identity of one resource claim; also its fencing token.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ClaimId(pub u64);

/// Identity of a mutable, revisioned aggregate. There is no singleton aggregate kind:
/// any number of attempts, steps or features coexist as separate keys.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AggregateKey {
    pub kind: String,
    pub id: OpaqueId,
}

impl AggregateKey {
    pub fn new(kind: impl Into<String>, id: OpaqueId) -> Self {
        Self {
            kind: kind.into(),
            id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AggregateRecord {
    pub key: AggregateKey,
    pub revision: Revision,
    pub state: String,
    pub updated_epoch: EngineEpoch,
}

/// Durable admission gate of one engine session. Every session starts `Closed`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Admission {
    Closed,
    Open,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShutdownMarker {
    Running,
    Clean,
    /// The session never recorded a clean shutdown; a successor sealed it.
    Unclean,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EngineSession {
    pub epoch: EngineEpoch,
    pub admission: Admission,
    pub shutdown: ShutdownMarker,
    pub schema_version: u32,
    pub owner_pid: u32,
    pub started_at_unix_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimMode {
    Shared,
    Exclusive,
}

/// `Quarantined` claims belong to a dead engine epoch whose holder may still be alive.
/// They keep blocking exactly like `Active` claims until released on evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimState {
    Active,
    Quarantined,
    Released,
}

/// Why a claim may be released. There is deliberately no timeout/expiry variant: elapsed
/// time is never evidence that a holder process has terminated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleaseBasis {
    /// The holder's outcome was durably settled with process absence established.
    HolderSettled,
    /// The reservation was revoked and proven never to have crossed the issue boundary.
    NeverIssued,
    /// A recovery resolution recorded evidence that the holder is gone.
    RecoveryResolved,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaimRecord {
    pub id: ClaimId,
    pub resource_key: String,
    pub mode: ClaimMode,
    pub holder: AggregateKey,
    pub epoch: EngineEpoch,
    pub state: ClaimState,
}

/// Outbox issue state: `NotIssued → IssuedUnknown → Confirmed | Rejected`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectState {
    NotIssued,
    IssuedUnknown,
    Confirmed,
    Rejected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EffectRecord {
    pub operation_key: String,
    pub target: String,
    pub payload: String,
    pub payload_digest: String,
    pub state: EffectState,
    pub created_epoch: EngineEpoch,
    pub updated_epoch: EngineEpoch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventRecord {
    pub seq: EventSeq,
    pub epoch: EngineEpoch,
    pub command_id: Option<CommandId>,
    pub kind: String,
    pub aggregate: Option<(AggregateKey, Revision)>,
    pub payload: String,
    pub recorded_at_unix_ms: i64,
}

/// A mutating request. `epoch` is the engine epoch the sender believes is current; it is a
/// fence, not part of the request identity, so a retry after a restart keeps its digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandRequest {
    pub id: CommandId,
    pub epoch: EngineEpoch,
    pub kind: String,
    pub payload: String,
}

impl CommandRequest {
    pub fn new(
        id: CommandId,
        epoch: EngineEpoch,
        kind: impl Into<String>,
        payload: impl Into<String>,
    ) -> Self {
        Self {
            id,
            epoch,
            kind: kind.into(),
            payload: payload.into(),
        }
    }

    /// SHA-256 over the length-prefixed kind and the payload (unambiguous concatenation).
    pub fn digest(&self) -> String {
        let kind_length = (self.kind.len() as u64).to_be_bytes();
        sha256_hex(&[&kind_length, self.kind.as_bytes(), self.payload.as_bytes()])
    }
}

/// The durable result of a committed command. A replay returns the same stored values
/// with `replayed = true`; nothing else about the receipt changes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Receipt {
    pub command_id: CommandId,
    /// Epoch in which the command originally committed.
    pub epoch: EngineEpoch,
    pub response: String,
    /// Global event cursor at the moment the command committed.
    pub event_seq: EventSeq,
    pub replayed: bool,
}

/// A consistent read of projections together with the event cursor they correspond to.
/// Every event with `seq <= as_of_event_seq` is reflected; no later event is.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub session: EngineSession,
    pub as_of_event_seq: EventSeq,
    pub aggregates: Vec<AggregateRecord>,
    pub live_claims: Vec<ClaimRecord>,
    pub effects: Vec<EffectRecord>,
}

pub(crate) fn id_to_text(id: OpaqueId) -> String {
    format!("{:032x}", id.0)
}

pub(crate) fn id_from_text(text: &str) -> Result<OpaqueId, StorageError> {
    if text.len() != 32 {
        return Err(StorageError::corrupt(format!(
            "malformed identity {text:?}"
        )));
    }
    u128::from_str_radix(text, 16)
        .map(OpaqueId)
        .map_err(|_| StorageError::corrupt(format!("malformed identity {text:?}")))
}

pub(crate) fn to_db(value: u64) -> Result<i64, StorageError> {
    i64::try_from(value).map_err(|_| StorageError::corrupt(format!("{value} exceeds i64")))
}

pub(crate) fn from_db(value: i64) -> Result<u64, StorageError> {
    u64::try_from(value).map_err(|_| StorageError::corrupt(format!("negative value {value}")))
}

macro_rules! text_enum {
    ($type:ty { $($variant:path => $text:literal),+ $(,)? }) => {
        impl $type {
            // Not every enum is both written and read by Rust code (some values are
            // only produced by SQL), so one direction may be unused.
            #[allow(dead_code)]
            pub(crate) fn as_db(self) -> &'static str {
                match self { $($variant => $text),+ }
            }

            #[allow(dead_code)]
            pub(crate) fn from_db(text: &str) -> Result<Self, StorageError> {
                match text {
                    $($text => Ok($variant),)+
                    other => Err(StorageError::corrupt(format!(
                        "unknown {} value {other:?}", stringify!($type)
                    ))),
                }
            }
        }
    };
}

pub(crate) use text_enum;

text_enum!(Admission { Admission::Closed => "CLOSED", Admission::Open => "OPEN" });
text_enum!(ShutdownMarker {
    ShutdownMarker::Running => "RUNNING",
    ShutdownMarker::Clean => "CLEAN",
    ShutdownMarker::Unclean => "UNCLEAN",
});
text_enum!(ClaimMode { ClaimMode::Shared => "SHARED", ClaimMode::Exclusive => "EXCLUSIVE" });
text_enum!(ClaimState {
    ClaimState::Active => "ACTIVE",
    ClaimState::Quarantined => "QUARANTINED",
    ClaimState::Released => "RELEASED",
});
text_enum!(ReleaseBasis {
    ReleaseBasis::HolderSettled => "HOLDER_SETTLED",
    ReleaseBasis::NeverIssued => "NEVER_ISSUED",
    ReleaseBasis::RecoveryResolved => "RECOVERY_RESOLVED",
});
text_enum!(EffectState {
    EffectState::NotIssued => "NOT_ISSUED",
    EffectState::IssuedUnknown => "ISSUED_UNKNOWN",
    EffectState::Confirmed => "CONFIRMED",
    EffectState::Rejected => "REJECTED",
});

//! Mutation primitives available to a command handler inside the one writer transaction.
//!
//! Everything a handler does through `Tx` — revision changes, events, claims, effects —
//! commits together with the command receipt or not at all. `Tx` exposes database
//! primitives only: slow I/O, process launches and filesystem writes cannot happen here
//! and are never atomic with the commit.

use jojo_domain::identity::Revision;
pub use jojo_domain::payload::Record;

use crate::digest::sha256_hex;
use crate::error::{StorageError, TxError};
use crate::model::{
    Admission, AggregateKey, AggregateRecord, ClaimId, ClaimMode, ClaimRecord, EffectRecord,
    EffectState, EngineEpoch, EventSeq, ReleaseBasis, from_db, id_to_text, to_db,
};
use crate::queries;
use crate::sqlite::{Connection, Value};

pub struct Tx<'w> {
    pub(crate) conn: &'w Connection,
    pub(crate) epoch: EngineEpoch,
    pub(crate) now_unix_ms: i64,
    pub(crate) now: jojo_domain::time::ClockReading,
    pub(crate) command_id: String,
}

impl Tx<'_> {
    /// The engine epoch performing this transaction (already verified to be current).
    pub fn epoch(&self) -> EngineEpoch {
        self.epoch
    }

    pub fn aggregate(&self, key: &AggregateKey) -> Result<Option<AggregateRecord>, TxError> {
        Ok(queries::aggregate(self.conn, key)?)
    }

    /// Creates an aggregate at revision 1.
    pub fn create_aggregate(
        &mut self,
        key: &AggregateKey,
        state: &str,
    ) -> Result<Revision, TxError> {
        if queries::aggregate(self.conn, key)?.is_some() {
            return Err(TxError::AggregateExists(key.clone()));
        }
        self.conn.execute(
            "INSERT INTO aggregates (kind, id, revision, state, updated_epoch)
             VALUES (?1, ?2, 1, ?3, ?4)",
            &[
                Value::Text(&key.kind),
                Value::Text(&id_to_text(key.id)),
                Value::Text(state),
                Value::Int(to_db(self.epoch.0)?),
            ],
        )?;
        Ok(Revision(1))
    }

    /// Compare-and-swap: replaces the state only if the stored revision equals `expected`,
    /// and advances the revision by one. The comparison is part of the `UPDATE` itself.
    pub fn update_aggregate(
        &mut self,
        key: &AggregateKey,
        expected: Revision,
        state: &str,
    ) -> Result<Revision, TxError> {
        let changed = self.conn.execute(
            "UPDATE aggregates SET revision = revision + 1, state = ?1, updated_epoch = ?2
             WHERE kind = ?3 AND id = ?4 AND revision = ?5",
            &[
                Value::Text(state),
                Value::Int(to_db(self.epoch.0)?),
                Value::Text(&key.kind),
                Value::Text(&id_to_text(key.id)),
                Value::Int(to_db(expected.0)?),
            ],
        )?;
        if changed == 1 {
            return Ok(Revision(expected.0 + 1));
        }
        Err(TxError::RevisionConflict {
            key: key.clone(),
            expected,
            actual: queries::aggregate(self.conn, key)?.map(|record| record.revision),
        })
    }

    /// Appends one immutable event. When an aggregate is named, `revision` must be the
    /// revision that aggregate has right now in this transaction, so events cannot
    /// misreport the state they describe. The payload is a typed record (R-04), never text.
    pub fn append_event(
        &mut self,
        kind: &str,
        aggregate: Option<(&AggregateKey, Revision)>,
        payload: &Record,
    ) -> Result<EventSeq, TxError> {
        let payload = payload.encode();
        let aggregate_id = aggregate.map(|(key, _)| id_to_text(key.id));
        let (agg_kind, agg_id, agg_revision) = match (aggregate, &aggregate_id) {
            (Some((key, revision)), Some(id)) => {
                let stored = queries::aggregate(self.conn, key)?.map(|record| record.revision);
                if stored != Some(revision) {
                    return Err(TxError::RevisionConflict {
                        key: key.clone(),
                        expected: revision,
                        actual: stored,
                    });
                }
                (
                    Value::Text(&key.kind),
                    Value::Text(id),
                    Value::Int(to_db(revision.0)?),
                )
            }
            _ => (Value::Null, Value::Null, Value::Null),
        };
        let seq = self.conn.query_one(
            "INSERT INTO events (epoch, command_id, kind, aggregate_kind, aggregate_id,
                                 aggregate_revision, payload, recorded_at_unix_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) RETURNING seq",
            &[
                Value::Int(to_db(self.epoch.0)?),
                Value::Text(&self.command_id),
                Value::Text(kind),
                agg_kind,
                agg_id,
                agg_revision,
                Value::Text(&payload),
                Value::Int(self.now_unix_ms),
            ],
            |row| row.int(0),
        )?;
        Ok(EventSeq(from_db(seq)?))
    }

    /// Effective admission of the current engine session: the stored RUN/HOLD intent,
    /// closed while a P1-12 close gate (an open or confirmed close ticket) is layered over it.
    pub fn admission(&self) -> Result<Admission, TxError> {
        let session = queries::latest_session(self.conn)?
            .ok_or_else(|| StorageError::corrupt("no engine session"))?;
        if crate::shutdown::close_gate(self.conn, self.epoch)? {
            return Ok(Admission::Closed);
        }
        Ok(session.admission)
    }

    /// Sets the stored admission intent of the current session. A reopened database (any
    /// earlier session) opens only after this epoch recorded a recovery start (P1-11); the
    /// first epoch of a new database has nothing to recover. A close gate is a separate
    /// layer: opening the intent does not pass it, and cancelling a close does not change it.
    pub fn set_admission(&mut self, admission: Admission) -> Result<(), TxError> {
        if admission == Admission::Open {
            let gated = self.conn.query_one(
                "SELECT EXISTS(SELECT 1 FROM engine_sessions WHERE epoch < ?1)
                    AND NOT EXISTS(SELECT 1 FROM recovery_starts WHERE epoch = ?1)",
                &[Value::Int(to_db(self.epoch.0)?)],
                |r| r.int(0),
            )?;
            if gated != 0 {
                return Err(TxError::Rejected(
                    "reopened engine: recovery inspection and start required".into(),
                ));
            }
        }
        self.conn.execute(
            "UPDATE engine_sessions SET admission = ?1 WHERE epoch = ?2",
            &[
                Value::Text(admission.as_db()),
                Value::Int(to_db(self.epoch.0)?),
            ],
        )?;
        Ok(())
    }

    /// Live (active or quarantined) claims on one resource. Counting these inside the
    /// transaction is how a later scheduler checks capacities authoritatively.
    pub fn live_claims(&self, resource_key: &str) -> Result<Vec<ClaimRecord>, TxError> {
        Ok(queries::live_claims(self.conn, Some(resource_key))?)
    }

    /// Records a claim for `holder`, or refuses if it conflicts with any live claim:
    /// exclusive tolerates no other live claim, shared tolerates only shared, and one holder
    /// holds a resource at most once. Quarantined claims block exactly like active ones.
    pub fn acquire_claim(
        &mut self,
        resource_key: &str,
        mode: ClaimMode,
        holder: &AggregateKey,
    ) -> Result<ClaimId, TxError> {
        let blocking: Vec<ClaimId> = queries::live_claims(self.conn, Some(resource_key))?
            .into_iter()
            .filter(|held| {
                mode == ClaimMode::Exclusive
                    || held.mode == ClaimMode::Exclusive
                    || held.holder == *holder
            })
            .map(|held| held.id)
            .collect();
        if !blocking.is_empty() {
            return Err(TxError::ClaimConflict {
                resource_key: resource_key.to_string(),
                blocking,
            });
        }
        let id = self.conn.query_one(
            "INSERT INTO resource_claims (resource_key, mode, holder_kind, holder_id, epoch,
                                          state, acquired_at_unix_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, 'ACTIVE', ?6) RETURNING claim_id",
            &[
                Value::Text(resource_key),
                Value::Text(mode.as_db()),
                Value::Text(&holder.kind),
                Value::Text(&id_to_text(holder.id)),
                Value::Int(to_db(self.epoch.0)?),
                Value::Int(self.now_unix_ms),
            ],
            |row| row.int(0),
        )?;
        Ok(ClaimId(from_db(id)?))
    }

    /// Releases a live claim on recorded evidence. There is no time-based release.
    pub fn release_claim(&mut self, id: ClaimId, basis: ReleaseBasis) -> Result<(), TxError> {
        let changed = self.conn.execute(
            "UPDATE resource_claims SET state = 'RELEASED', release_basis = ?1, released_epoch = ?2
             WHERE claim_id = ?3 AND state <> 'RELEASED'",
            &[
                Value::Text(basis.as_db()),
                Value::Int(to_db(self.epoch.0)?),
                Value::Int(to_db(id.0)?),
            ],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(TxError::ClaimNotLive(id))
        }
    }

    pub fn effect(&self, operation_key: &str) -> Result<Option<EffectRecord>, TxError> {
        Ok(queries::effects(self.conn, Some(operation_key))?.pop())
    }

    /// Records the intent to perform an external effect as `NOT_ISSUED`. Committing this is
    /// not performing the effect; a dispatcher acts later, outside any transaction.
    pub fn enqueue_effect(
        &mut self,
        operation_key: &str,
        target: &str,
        payload: &str,
    ) -> Result<(), TxError> {
        if self.effect(operation_key)?.is_some() {
            return Err(TxError::EffectExists(operation_key.to_string()));
        }
        let epoch = Value::Int(to_db(self.epoch.0)?);
        self.conn.execute(
            "INSERT INTO outbox (operation_key, target, payload, payload_digest, state,
                                 created_epoch, updated_epoch)
             VALUES (?1, ?2, ?3, ?4, 'NOT_ISSUED', ?5, ?5)",
            &[
                Value::Text(operation_key),
                Value::Text(target),
                Value::Text(payload),
                Value::Text(&sha256_hex(&[payload.as_bytes()])),
                epoch,
            ],
        )?;
        Ok(())
    }

    /// Moves an effect along `NOT_ISSUED → ISSUED_UNKNOWN → CONFIRMED | REJECTED`.
    /// `ISSUED_UNKNOWN` must be committed *before* crossing the external boundary.
    pub fn advance_effect(&mut self, operation_key: &str, to: EffectState) -> Result<(), TxError> {
        let current = self
            .effect(operation_key)?
            .ok_or_else(|| TxError::EffectMissing(operation_key.to_string()))?;
        let allowed = matches!(
            (current.state, to),
            (EffectState::NotIssued, EffectState::IssuedUnknown)
                | (
                    EffectState::IssuedUnknown,
                    EffectState::Confirmed | EffectState::Rejected
                )
        );
        if !allowed {
            return Err(TxError::InvalidEffectTransition {
                operation_key: operation_key.to_string(),
                from: current.state,
                to,
            });
        }
        self.conn.execute(
            "UPDATE outbox SET state = ?1, updated_epoch = ?2 WHERE operation_key = ?3",
            &[
                Value::Text(to.as_db()),
                Value::Int(to_db(self.epoch.0)?),
                Value::Text(operation_key),
            ],
        )?;
        Ok(())
    }
}

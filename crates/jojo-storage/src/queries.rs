//! Read statements shared by the writer transaction and by read snapshots. Every function
//! reads through the connection it is given, so it sees that connection's transaction.

use jojo_domain::identity::Revision;

use crate::error::StorageError;
use crate::model::{
    Admission, AggregateKey, AggregateRecord, ClaimId, ClaimMode, ClaimRecord, ClaimState,
    CommandId, EffectRecord, EffectState, EngineEpoch, EngineSession, EventRecord, EventSeq,
    Receipt, ShutdownMarker, from_db, id_from_text, id_to_text, to_db,
};
use crate::sqlite::{Connection, Row, SqliteError, Value};

type Raw<T> = Result<T, SqliteError>;

fn key_from(kind: String, id: &str) -> Result<AggregateKey, StorageError> {
    Ok(AggregateKey {
        kind,
        id: id_from_text(id)?,
    })
}

/// The newest engine session; `None` only before the first epoch was ever established.
pub(crate) fn latest_session(conn: &Connection) -> Result<Option<EngineSession>, StorageError> {
    conn.query_optional(
        "SELECT epoch, admission, shutdown, schema_version, owner_pid, started_at_unix_ms
         FROM engine_sessions ORDER BY epoch DESC LIMIT 1",
        &[],
        |r| -> Raw<_> {
            Ok((
                r.int(0)?,
                r.text(1)?,
                r.text(2)?,
                r.int(3)?,
                r.int(4)?,
                r.int(5)?,
            ))
        },
    )?
    .map(|(epoch, admission, shutdown, schema, pid, started)| {
        Ok(EngineSession {
            epoch: EngineEpoch(from_db(epoch)?),
            admission: Admission::from_db(&admission)?,
            shutdown: ShutdownMarker::from_db(&shutdown)?,
            schema_version: u32::try_from(schema)
                .map_err(|_| StorageError::corrupt("schema version out of range"))?,
            owner_pid: u32::try_from(pid)
                .map_err(|_| StorageError::corrupt("owner pid out of range"))?,
            started_at_unix_ms: started,
        })
    })
    .transpose()
}

/// Highest committed (or, inside a writer transaction, appended) event sequence number.
pub(crate) fn event_cursor(conn: &Connection) -> Result<EventSeq, StorageError> {
    let seq = conn.query_one("SELECT coalesce(max(seq), 0) FROM events", &[], |r| {
        r.int(0)
    })?;
    Ok(EventSeq(from_db(seq)?))
}

const AGGREGATE_COLUMNS: &str = "kind, id, revision, state, updated_epoch";

fn aggregate_row(r: &Row<'_>) -> Raw<(String, String, i64, String, i64)> {
    Ok((r.text(0)?, r.text(1)?, r.int(2)?, r.text(3)?, r.int(4)?))
}

fn aggregate_record(
    (kind, id, revision, state, epoch): (String, String, i64, String, i64),
) -> Result<AggregateRecord, StorageError> {
    Ok(AggregateRecord {
        key: key_from(kind, &id)?,
        revision: Revision(from_db(revision)?),
        state,
        updated_epoch: EngineEpoch(from_db(epoch)?),
    })
}

pub(crate) fn aggregate(
    conn: &Connection,
    key: &AggregateKey,
) -> Result<Option<AggregateRecord>, StorageError> {
    conn.query_optional(
        &format!("SELECT {AGGREGATE_COLUMNS} FROM aggregates WHERE kind = ?1 AND id = ?2"),
        &[Value::Text(&key.kind), Value::Text(&id_to_text(key.id))],
        aggregate_row,
    )?
    .map(aggregate_record)
    .transpose()
}

pub(crate) fn aggregates(conn: &Connection) -> Result<Vec<AggregateRecord>, StorageError> {
    conn.query(
        &format!("SELECT {AGGREGATE_COLUMNS} FROM aggregates ORDER BY kind, id"),
        &[],
        aggregate_row,
    )?
    .into_iter()
    .map(aggregate_record)
    .collect()
}

/// Live (active or quarantined) claims, optionally restricted to one resource.
pub(crate) fn live_claims(
    conn: &Connection,
    resource_key: Option<&str>,
) -> Result<Vec<ClaimRecord>, StorageError> {
    let filter = resource_key.map_or(Value::Null, Value::Text);
    conn.query(
        "SELECT claim_id, resource_key, mode, holder_kind, holder_id, epoch, state
         FROM resource_claims
         WHERE state <> 'RELEASED' AND (?1 IS NULL OR resource_key = ?1)
         ORDER BY claim_id",
        &[filter],
        |r| -> Raw<_> {
            Ok((
                r.int(0)?,
                r.text(1)?,
                r.text(2)?,
                r.text(3)?,
                r.text(4)?,
                r.int(5)?,
                r.text(6)?,
            ))
        },
    )?
    .into_iter()
    .map(
        |(id, resource_key, mode, holder_kind, holder_id, epoch, state)| {
            Ok(ClaimRecord {
                id: ClaimId(from_db(id)?),
                resource_key,
                mode: ClaimMode::from_db(&mode)?,
                holder: key_from(holder_kind, &holder_id)?,
                epoch: EngineEpoch(from_db(epoch)?),
                state: ClaimState::from_db(&state)?,
            })
        },
    )
    .collect()
}

/// Effects, optionally restricted to one operation key.
pub(crate) fn effects(
    conn: &Connection,
    operation_key: Option<&str>,
) -> Result<Vec<EffectRecord>, StorageError> {
    let filter = operation_key.map_or(Value::Null, Value::Text);
    conn.query(
        "SELECT operation_key, target, payload, payload_digest, state, created_epoch, updated_epoch
         FROM outbox WHERE (?1 IS NULL OR operation_key = ?1) ORDER BY operation_key",
        &[filter],
        |r| -> Raw<_> {
            Ok((
                r.text(0)?,
                r.text(1)?,
                r.text(2)?,
                r.text(3)?,
                r.text(4)?,
                r.int(5)?,
                r.int(6)?,
            ))
        },
    )?
    .into_iter()
    .map(
        |(operation_key, target, payload, payload_digest, state, created, updated)| {
            Ok(EffectRecord {
                operation_key,
                target,
                payload,
                payload_digest,
                state: EffectState::from_db(&state)?,
                created_epoch: EngineEpoch(from_db(created)?),
                updated_epoch: EngineEpoch(from_db(updated)?),
            })
        },
    )
    .collect()
}

/// Up to `limit` events with `seq > cursor`, in sequence order.
pub(crate) fn events_after(
    conn: &Connection,
    cursor: EventSeq,
    limit: u32,
) -> Result<Vec<EventRecord>, StorageError> {
    conn.query(
        "SELECT seq, epoch, command_id, kind, aggregate_kind, aggregate_id, aggregate_revision,
                payload, recorded_at_unix_ms
         FROM events WHERE seq > ?1 ORDER BY seq LIMIT ?2",
        &[Value::Int(to_db(cursor.0)?), Value::Int(i64::from(limit))],
        |r| -> Raw<_> {
            Ok((
                r.int(0)?,
                r.int(1)?,
                r.optional_text(2)?,
                r.text(3)?,
                r.optional_text(4)?,
                r.optional_text(5)?,
                r.optional_int(6)?,
                r.text(7)?,
                r.int(8)?,
            ))
        },
    )?
    .into_iter()
    .map(
        |(seq, epoch, command_id, kind, agg_kind, agg_id, agg_revision, payload, recorded)| {
            let aggregate = match (agg_kind, agg_id, agg_revision) {
                (Some(kind), Some(id), Some(revision)) => {
                    Some((key_from(kind, &id)?, Revision(from_db(revision)?)))
                }
                _ => None,
            };
            Ok(EventRecord {
                seq: EventSeq(from_db(seq)?),
                epoch: EngineEpoch(from_db(epoch)?),
                command_id: command_id
                    .map(|id| id_from_text(&id).map(CommandId))
                    .transpose()?,
                kind,
                aggregate,
                payload,
                recorded_at_unix_ms: recorded,
            })
        },
    )
    .collect()
}

/// The stored receipt of a committed command together with its request digest.
pub(crate) fn receipt(
    conn: &Connection,
    id: CommandId,
) -> Result<Option<(Receipt, String)>, StorageError> {
    conn.query_optional(
        "SELECT epoch, response, event_seq, request_digest FROM commands WHERE command_id = ?1",
        &[Value::Text(&id_to_text(id.0))],
        |r| -> Raw<_> { Ok((r.int(0)?, r.text(1)?, r.int(2)?, r.text(3)?)) },
    )?
    .map(|(epoch, response, event_seq, digest)| {
        Ok((
            Receipt {
                command_id: id,
                epoch: EngineEpoch(from_db(epoch)?),
                response,
                event_seq: EventSeq(from_db(event_seq)?),
                replayed: true,
            },
            digest,
        ))
    })
    .transpose()
}

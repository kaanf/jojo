//! Bounded P1-13 read projections. SQL limits apply before allocating rows or payloads.
use crate::model::{id_to_text, to_db};
use crate::sqlite::Value;
use crate::{ReadView, StorageError};
use jojo_domain::identity::OpaqueId;
use jojo_domain::ipc::{identity, number, object, text};
use jojo_domain::json::Json;

fn bad(s: &str) -> StorageError {
    StorageError::corrupt(s)
}
impl ReadView<'_> {
    /// Each page binds to one committed cursor. Concurrent mutation invalidates subsequent
    /// pages rather than quietly mixing snapshots. First page uses `None`.
    pub fn ipc_snapshot(
        &self,
        section: &str,
        after: &str,
        cursor: Option<u64>,
        limit: u32,
    ) -> Result<Json, StorageError> {
        if limit == 0 || limit > 8 || after.len() > 256 {
            return Err(bad("invalid page"));
        }
        let seq = self.event_cursor()?.0;
        if cursor.is_some_and(|c| c != seq) {
            return Err(bad("SnapshotChanged: reload"));
        }
        let sql = match section {
            "aggregates" => {
                "SELECT kind||':'||id,substr(state,1,512),length(state)>512 FROM aggregates WHERE kind||':'||id>?1 ORDER BY kind||':'||id LIMIT ?2"
            }
            "claims" => {
                "SELECT printf('%020d',claim_id),substr(resource_key||' '||holder_kind||':'||holder_id||' '||state,1,512),length(resource_key||holder_kind)>400 FROM resource_claims WHERE state<>'RELEASED' AND printf('%020d',claim_id)>?1 ORDER BY claim_id LIMIT ?2"
            }
            "effects" => {
                "SELECT operation_key,substr(target||' '||state,1,512),length(target)>480 FROM outbox WHERE operation_key>?1 ORDER BY operation_key LIMIT ?2"
            }
            _ => return Err(bad("unknown snapshot section")),
        };
        let rows = self.conn.query(
            sql,
            &[Value::Text(after), Value::Int(i64::from(limit))],
            |r| Ok((r.text(0)?, r.text(1)?, r.int(2)?)),
        )?;
        let next = rows.last().map(|r| r.0.clone());
        let session = self.session()?;
        Ok(object([
            ("as_of_event_seq", number(seq)),
            ("epoch", number(session.epoch.0)),
            ("admission", text(format!("{:?}", session.admission))),
            ("shutdown", text(format!("{:?}", session.shutdown))),
            ("section", text(section)),
            ("next", next.map_or(Json::Null, text)),
            (
                "rows",
                Json::List(
                    rows.into_iter()
                        .map(|(key, value, truncated)| {
                            object([
                                ("key", text(key)),
                                ("value", text(value)),
                                ("detail_complete", Json::Bool(truncated == 0)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]))
    }
    /// Events use a bounded detail projection, explicitly marked when the detail needs a
    /// separate chunk query. Sequence/aggregate identity are never omitted.
    pub fn ipc_events(&self, after: u64, limit: u32) -> Result<Json, StorageError> {
        if limit == 0 || limit > 8 {
            return Err(bad("invalid event page"));
        }
        let rows=self.conn.query("SELECT seq,epoch,kind,aggregate_kind,aggregate_id,aggregate_revision,substr(payload,1,512),length(payload)>512 FROM events WHERE seq>?1 ORDER BY seq LIMIT ?2",&[Value::Int(to_db(after)?),Value::Int(i64::from(limit))],|r|Ok((r.int(0)?,r.int(1)?,r.text(2)?,r.optional_text(3)?,r.optional_text(4)?,r.optional_int(5)?,r.text(6)?,r.int(7)?)))?;
        Ok(Json::List(
            rows.into_iter()
                .map(|(seq, epoch, kind, ak, ai, rev, payload, truncated)| {
                    object([
                        ("seq", Json::Int(seq)),
                        ("epoch", Json::Int(epoch)),
                        ("type", text(kind)),
                        ("aggregate_kind", ak.map_or(Json::Null, text)),
                        ("aggregate_id", ai.map_or(Json::Null, text)),
                        ("revision", rev.map_or(Json::Null, Json::Int)),
                        ("payload", text(payload)),
                        ("detail_complete", Json::Bool(truncated == 0)),
                    ])
                })
                .collect(),
        ))
    }
    /// UTF-8 character cursor over durable normalized log records. Never reads an arbitrary
    /// client path or an uncommitted spool tail. Raw spools remain evidence, not stdout.
    pub fn ipc_log(
        &self,
        attempt: OpaqueId,
        sequence: u64,
        offset: u64,
    ) -> Result<Json, StorageError> {
        let row=self.conn.query_optional("SELECT substr(payload,?3+1,2048),length(payload),kind FROM provider_events WHERE attempt=?1 AND sequence=?2",&[Value::Text(&id_to_text(attempt)),Value::Int(to_db(sequence)?),Value::Int(to_db(offset)?)],|r|Ok((r.text(0)?,r.int(1)?,r.text(2)?)))?;
        Ok(match row {
            None => Json::Null,
            Some((chunk, total, kind)) => object([
                ("attempt", identity(attempt)),
                ("sequence", number(sequence)),
                ("offset", number(offset)),
                (
                    "next",
                    number(offset.saturating_add(chunk.chars().count() as u64)),
                ),
                ("total", Json::Int(total)),
                ("type", text(kind)),
                ("text", text(chunk)),
            ]),
        })
    }
    pub fn ipc_event_detail(&self, seq: u64, offset: u64) -> Result<Json, StorageError> {
        let row = self.conn.query_optional(
            "SELECT substr(payload,?2+1,2048),length(payload) FROM events WHERE seq=?1",
            &[Value::Int(to_db(seq)?), Value::Int(to_db(offset)?)],
            |r| Ok((r.text(0)?, r.int(1)?)),
        )?;
        Ok(row.map_or(Json::Null, |(chunk, total)| {
            object([
                ("seq", number(seq)),
                (
                    "next",
                    number(offset.saturating_add(chunk.chars().count() as u64)),
                ),
                ("total", Json::Int(total)),
                ("text", text(chunk)),
            ])
        }))
    }
}

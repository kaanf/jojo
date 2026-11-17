//! Bounded P1-13 read projections. SQL limits apply before allocating rows or payloads.
use crate::model::{id_from_text, id_to_text, to_db};
use crate::sqlite::Value;
use crate::{ReadView, StorageError, StoredExecutionPlan, sha256_hex};
use jojo_domain::dependency::{ApprovalBinding, Edge, NodeRef, Predicate, Source, SourceRevision};
use jojo_domain::identity::{ArtifactId, OpaqueId};
use jojo_domain::ipc::{identity, number, object, text};
use jojo_domain::json::Json;

fn bad(s: &str) -> StorageError {
    StorageError::corrupt(s)
}
fn source_json(s: &Source) -> Json {
    let (commit, snapshot) = match &s.revision {
        SourceRevision::Commit(commit) => (commit, Json::Null),
        SourceRevision::Snapshot {
            base_commit,
            digest,
        } => (base_commit, text(digest.clone())),
    };
    object([
        ("repository", identity(s.repository.0)),
        ("commit", text(commit.clone())),
        ("snapshot", snapshot),
    ])
}
/// A step as the plan stores it. A plan stores no name, text or state of a step.
fn step_json(stored: &StoredExecutionPlan, step: &NodeRef) -> Json {
    let resources = stored.plan.resources.iter().find(|r| r.step == *step);
    let inputs = stored.plan.inputs.iter().find(|i| i.step == *step);
    let resource = |f: &dyn Fn(&jojo_domain::scheduler::StepResources) -> Json| {
        resources.map_or(Json::Null, f)
    };
    object([
        ("step", identity(step.id)),
        ("revision", number(step.revision.0)),
        ("provider", resource(&|r| text(format!("{:?}", r.provider)))),
        ("repository", resource(&|r| identity(r.repository.0))),
        ("priority", resource(&|r| number(r.priority))),
        (
            "repository_mode",
            resource(&|r| text(format!("{:?}", r.repository_mode))),
        ),
        (
            "environment",
            inputs
                .and_then(|i| i.environment.clone())
                .map_or(Json::Null, text),
        ),
        (
            "sources",
            Json::List(inputs.map_or(vec![], |i| i.base_sources.iter().map(source_json).collect())),
        ),
    ])
}
/// An edge with the kind of its predicate and what the predicate names.
fn edge_json(e: &Edge) -> Json {
    let (predicate, artifact, artifact_revision, evidence, evidence_kind) = match &e.predicate {
        Predicate::ArtifactApproved { binding, .. } => (
            "ArtifactApproved",
            identity(binding.artifact.0),
            number(binding.revision.0),
            Json::Null,
            Json::Null,
        ),
        Predicate::Evidence { requirement, .. } => (
            "Evidence",
            Json::Null,
            Json::Null,
            text(requirement.evidence_id.clone()),
            text(format!("{:?}", requirement.kind)),
        ),
    };
    object([
        ("edge", identity(e.id)),
        ("prerequisite", identity(e.prerequisite.id)),
        ("dependent", identity(e.dependent.id)),
        ("predicate", text(predicate)),
        ("artifact", artifact),
        ("artifact_revision", artifact_revision),
        ("evidence", evidence),
        ("evidence_kind", evidence_kind),
    ])
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
    /// The first plan after `after`, in identity order, that holds a current approval.
    fn approved_plan_after(
        &self,
        after: &str,
    ) -> Result<Option<(StoredExecutionPlan, ApprovalBinding)>, StorageError> {
        let mut cursor = after.to_owned();
        loop {
            let ids = self.conn.query(
                "SELECT DISTINCT plan FROM execution_plans WHERE plan>?1 ORDER BY plan LIMIT 8",
                &[Value::Text(&cursor)],
                |r| r.text(0),
            )?;
            let Some(last) = ids.last().cloned() else {
                return Ok(None);
            };
            for id in &ids {
                let id = ArtifactId(id_from_text(id)?);
                if let Some(approval) = self.current_artifact_approval(id)?
                    && let Some(stored) = self.execution_plan(id, approval.revision)?
                {
                    return Ok(Some((stored, approval)));
                }
            }
            cursor = last;
        }
    }
    /// The approved revision of one execution plan, whole: its steps, edges and the
    /// approvals it binds. `Null` when no plan after `after` is approved; `next` is the
    /// cursor of the following approved plan, when there is one. Read-only, and only what
    /// the core stores: the validation plan is found by the digest the plan binds. A newer
    /// revision that is not approved makes the approval stale, so such a plan is not sent.
    pub fn ipc_approved_plan(&self, after: Option<ArtifactId>) -> Result<Json, StorageError> {
        let after = after.map_or(String::new(), |a| id_to_text(a.0));
        let Some((stored, approval)) = self.approved_plan_after(&after)? else {
            return Ok(Json::Null);
        };
        let plan = id_to_text(stored.subject.0);
        let more = self.approved_plan_after(&plan)?.is_some();
        let (prd, adr) = self.conn.query_one(
            "SELECT prd,adr FROM review_documents WHERE id=?1 AND revision=?2",
            &[Value::Text(&plan), Value::Int(to_db(stored.revision.0)?)],
            |r| Ok((r.text(0)?, r.optional_text(1)?)),
        )?;
        let adr = adr.ok_or_else(|| bad("execution plan without ADR"))?;
        let documents = self.conn.query(
            "SELECT d.id,d.content FROM review_documents d WHERE d.prd=?1 AND d.adr=?2 AND d.id<>?3 AND d.revision=(SELECT max(revision) FROM review_documents h WHERE h.id=d.id) ORDER BY d.id",
            &[Value::Text(&prd), Value::Text(&adr), Value::Text(&plan)],
            |r| Ok((r.text(0)?, r.text(1)?)),
        )?;
        let mut bound = documents
            .into_iter()
            .filter(|(_, body)| sha256_hex(&[body.as_bytes()]) == stored.plan.validation_digest)
            .map(|(id, _)| id);
        let validation = match (bound.next(), bound.next()) {
            (Some(id), None) => text(id),
            _ => Json::Null,
        };
        Ok(object([
            ("plan", text(plan.clone())),
            ("revision", number(stored.revision.0)),
            ("digest", text(stored.digest.clone())),
            ("decision", number(approval.decision)),
            ("prd", text(prd)),
            ("adr", text(adr)),
            ("validation", validation),
            (
                "validation_digest",
                text(stored.plan.validation_digest.clone()),
            ),
            (
                "steps",
                Json::List(
                    stored
                        .plan
                        .execution
                        .nodes
                        .iter()
                        .map(|n| step_json(&stored, n))
                        .collect(),
                ),
            ),
            (
                "edges",
                Json::List(stored.plan.execution.edges.iter().map(edge_json).collect()),
            ),
            (
                "approvals",
                Json::List(
                    stored
                        .plan
                        .approvals
                        .iter()
                        .map(|a| {
                            object([
                                ("artifact", identity(a.artifact.0)),
                                ("revision", number(a.revision.0)),
                                ("decision", number(a.decision)),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("next", if more { text(plan) } else { Json::Null }),
        ]))
    }
    /// Events use a bounded detail projection, explicitly marked when the detail needs a
    /// separate chunk query. Sequence/aggregate identity are never omitted. `attributed`
    /// (protocol 1.6) adds the time an event was recorded and who caused it; `summarized`
    /// (1.7) adds a sentence that says what it recorded.
    pub fn ipc_events(
        &self,
        after: u64,
        limit: u32,
        attributed: bool,
        summarized: bool,
    ) -> Result<Json, StorageError> {
        if limit == 0 || limit > 8 {
            return Err(bad("invalid event page"));
        }
        let rows=self.conn.query("SELECT seq,epoch,kind,aggregate_kind,aggregate_id,aggregate_revision,substr(payload,1,512),length(payload)>512,recorded_at_unix_ms,command_id FROM events WHERE seq>?1 ORDER BY seq LIMIT ?2",&[Value::Int(to_db(after)?),Value::Int(i64::from(limit))],|r|Ok(((r.int(0)?,r.int(1)?,r.text(2)?,r.optional_text(3)?,r.optional_text(4)?,r.optional_int(5)?,r.text(6)?,r.int(7)?),(r.int(8)?,r.optional_text(9)?))))?;
        let mut out = Vec::new();
        for ((seq, epoch, kind, ak, ai, rev, payload, truncated), (at, command)) in rows {
            let summary = if summarized {
                Some(crate::features::event_summary(
                    self.conn,
                    seq,
                    epoch,
                    &kind,
                    ai.as_deref(),
                    &payload,
                )?)
            } else {
                None
            };
            let Json::Object(mut fields) = object([
                ("seq", Json::Int(seq)),
                ("epoch", Json::Int(epoch)),
                ("type", text(kind)),
                ("aggregate_kind", ak.map_or(Json::Null, text)),
                ("aggregate_id", ai.map_or(Json::Null, text)),
                ("revision", rev.map_or(Json::Null, Json::Int)),
                ("payload", text(payload)),
                ("detail_complete", Json::Bool(truncated == 0)),
            ]) else {
                unreachable!("an event is an object")
            };
            if attributed {
                let actor = crate::overview::event_actor(self.conn, seq, command.as_deref())?;
                fields.push(("at_unix_ms".into(), Json::Int(at)));
                fields.push(("actor".into(), text(actor)));
            }
            if let Some(summary) = summary {
                fields.push(("summary".into(), text(summary)));
            }
            out.push(Json::Object(fields));
        }
        Ok(Json::List(out))
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

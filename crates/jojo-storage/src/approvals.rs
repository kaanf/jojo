//! Human command boundary and conservative, derived approval applicability.
use crate::artifacts::{review_readiness, version};
use crate::model::{from_db, id_from_text, id_to_text, to_db};
use crate::sqlite::{Connection, Value};
use crate::{
    AggregateKey, CommandError, CommandId, CommandRequest, Engine, EventSeq, ReadView, Receipt,
    StorageError, Tx, TxError, sha256_hex,
};
use jojo_domain::identity::{ArtifactId, OpaqueId, Revision};
use jojo_domain::outcome::StopCause;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecisionAction {
    Approve,
    Reject,
    Revoke,
}
/// Supplied only by the host's trusted UI entry point, never decoded from provider data.
#[derive(Clone, Debug)]
pub enum HumanOrigin {
    TrustedUi { actor: String },
    FakeHumanFixture { actor: String },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewBundle {
    pub subject: ArtifactId,
    pub revision: Revision,
    pub content_hash: String,
    pub displayed_hash: String,
    pub context_hash: String,
    pub upstream: Vec<EventSeq>,
    pub prior_decision: Option<EventSeq>,
    pub hash: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HumanDecision {
    pub seq: EventSeq,
    pub subject: ArtifactId,
    pub bundle: String,
    pub action: String,
    pub origin: String,
    pub command: String,
    pub revision: Revision,
    pub content_hash: String,
    pub displayed_hash: String,
    pub upstream: String,
}
fn deny(s: &str) -> TxError {
    TxError::Rejected(s.into())
}
pub(crate) fn hash(fields: &[&str]) -> String {
    let mut bytes = Vec::new();
    for f in fields {
        bytes.extend_from_slice(&(f.len() as u64).to_be_bytes());
        bytes.extend_from_slice(f.as_bytes());
    }
    sha256_hex(&[&bytes])
}
pub(crate) fn decisions(
    c: &Connection,
    id: ArtifactId,
) -> Result<Vec<HumanDecision>, StorageError> {
    c.query("SELECT seq,bundle,action,origin,command,revision,content_hash,displayed_hash,upstream FROM human_decisions WHERE subject=?1 ORDER BY seq", &[Value::Text(&id_to_text(id.0))], |r| Ok((r.int(0)?,r.text(1)?,r.text(2)?,r.text(3)?,r.text(4)?,r.int(5)?,r.text(6)?,r.text(7)?,r.text(8)?)))?.into_iter().map(|(seq,bundle,action,origin,command,revision,content_hash,displayed_hash,upstream)| Ok(HumanDecision {seq:EventSeq(from_db(seq)?),subject:id,bundle,action,origin,command,revision:Revision(from_db(revision)?),content_hash,displayed_hash,upstream})).collect()
}
pub(crate) fn approval_inputs_ready(c: &Connection, id: ArtifactId) -> Result<bool, StorageError> {
    if version(c, id, None)?.is_some() {
        return Ok(review_readiness(c, id)?.is_some_and(|r| r.is_ready()));
    }
    let row=c.query_optional(
        "SELECT prd,adr,prd_decision,adr_decision FROM review_documents WHERE id=?1 ORDER BY revision DESC LIMIT 1",
        &[Value::Text(&id_to_text(id.0))],
        |r|Ok((r.text(0)?,r.optional_text(1)?,r.int(2)?,r.optional_int(3)?)),
    )?;
    let Some((prd, adr, pd, ad)) = row else {
        return Ok(false);
    };
    if applicable(c, ArtifactId(id_from_text(&prd)?))? != Some(EventSeq(from_db(pd)?)) {
        return Ok(false);
    }
    if let Some(adr) = adr {
        let Some(ad) = ad else { return Ok(false) };
        if applicable(c, ArtifactId(id_from_text(&adr)?))? != Some(EventSeq(from_db(ad)?)) {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(crate) fn applicable(c: &Connection, id: ArtifactId) -> Result<Option<EventSeq>, StorageError> {
    if !approval_inputs_ready(c, id)? {
        return Ok(None);
    }
    let Some(b) = bundle(c, id)? else {
        return Ok(None);
    };
    Ok(decisions(c, id)?
        .last()
        .filter(|d| d.action == "Approve" && d.bundle == b.hash)
        .map(|d| d.seq))
}
pub(crate) fn development_hash(
    c: &Connection,
    prd: ArtifactId,
    adr: ArtifactId,
    plan: ArtifactId,
) -> Result<Option<String>, StorageError> {
    let relation = c.query_optional(
        "SELECT prd,adr,kind FROM review_documents WHERE id=?1 ORDER BY revision DESC LIMIT 1",
        &[Value::Text(&id_to_text(plan.0))],
        |r| Ok((r.text(0)?, r.optional_text(1)?, r.text(2)?)),
    )?;
    if relation != Some((id_to_text(prd.0), Some(id_to_text(adr.0)), "PLAN".into())) {
        return Ok(None);
    }
    let mut ds = Vec::new();
    for id in [prd, adr, plan] {
        let Some(d) = applicable(c, id)? else {
            return Ok(None);
        };
        ds.push(d);
    }
    Ok(Some(hash(&[&format!("{ds:?}")])))
}

pub(crate) fn bundle(c: &Connection, id: ArtifactId) -> Result<Option<ReviewBundle>, StorageError> {
    let prior = decisions(c, id)?.last().map(|d| d.seq);
    // A question revision includes disposition and the newest immutable answer.
    // Bind even deferred questions, so no answer edit can silently preserve approval.
    let context = if let Some(v) = version(c, id, None)? {
        c.query(
            "SELECT q.question_id,a.revision FROM clarification_questions q
             JOIN aggregates a ON a.kind=q.question_kind AND a.id=q.question_id
             WHERE q.feature_id=?1 AND (q.artifact_id IS NULL OR q.artifact_id=?2)
             ORDER BY q.question_id",
            &[
                Value::Text(&id_to_text(v.feature.0)),
                Value::Text(&id_to_text(id.0)),
            ],
            |r| Ok((r.text(0)?, r.int(1)?)),
        )?
    } else {
        Vec::new()
    };
    let context_hash = hash(&[
        &format!("{context:?}"),
        &approval_inputs_ready(c, id)?.to_string(),
    ]);
    let (revision, content_hash, displayed_hash, upstream) = if let Some(v) = version(c, id, None)?
    {
        (v.revision, v.content_digest, v.rendered_digest, vec![])
    } else {
        let row=c.query_optional("SELECT revision,prd,adr,content,rendered,prd_decision,adr_decision FROM review_documents WHERE id=?1 ORDER BY revision DESC LIMIT 1", &[Value::Text(&id_to_text(id.0))], |r|Ok((r.int(0)?,r.text(1)?,r.optional_text(2)?,r.text(3)?,r.text(4)?,r.int(5)?,r.optional_int(6)?)))?;
        let Some((rev, prd, adr, content, rendered, pd, ad)) = row else {
            return Ok(None);
        };
        let pd = EventSeq(from_db(pd)?);
        let _ = prd;
        let mut upstream = vec![pd];
        if let Some(adr) = adr {
            let Some(ad) = ad else { return Ok(None) };
            let ad = EventSeq(from_db(ad)?);
            let _ = adr;
            upstream.push(ad);
        }
        (
            Revision(from_db(rev)?),
            hash(&[&content]),
            hash(&[&rendered]),
            upstream,
        )
    };
    // Prior decision is a CAS token, excluded from content identity so an approval
    // does not invalidate itself when it becomes the newest decision.
    let digest = hash(&[
        &id_to_text(id.0),
        &revision.0.to_string(),
        &content_hash,
        &displayed_hash,
        &context_hash,
        &format!("{upstream:?}"),
    ]);
    Ok(Some(ReviewBundle {
        subject: id,
        revision,
        content_hash,
        displayed_hash,
        context_hash,
        upstream,
        prior_decision: prior,
        hash: digest,
    }))
}
impl Engine {
    /// Explicit trusted command entry point. Provider DTOs and Markdown have no path here.
    pub fn human_decide(
        &self,
        id: CommandId,
        origin: HumanOrigin,
        review: ReviewBundle,
        action: DecisionAction,
    ) -> Result<Receipt, CommandError> {
        let payload = format!("{origin:?}|{review:?}|{action:?}");
        self.execute(CommandRequest::new(id,self.epoch(),"HumanDecision",payload),move|tx|{
            if bundle(tx.conn,review.subject)?.as_ref()!=Some(&review){return Err(deny("stale review bundle"))}
            if action==DecisionAction::Approve && !approval_inputs_ready(tx.conn,review.subject)? {return Err(deny("review is incomplete or upstream decisions are inapplicable"))}
            let actor=match &origin{HumanOrigin::TrustedUi{actor}|HumanOrigin::FakeHumanFixture{actor}=>actor};
            if actor.trim().is_empty(){return Err(deny("human identity required"))}
            if decisions(tx.conn,review.subject)?.last().is_some_and(|d|d.bundle==review.hash && d.action==format!("{action:?}")){return Err(deny("duplicate decision"))}
            let seq=tx.append_event("HumanDecision",None,&format!("{review:?}|{action:?}|{origin:?}"))?;
            tx.conn.execute("INSERT INTO human_decisions (seq,subject,bundle,action,origin,command,revision,content_hash,displayed_hash,upstream,prior_decision) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",&[Value::Int(to_db(seq.0)?),Value::Text(&id_to_text(review.subject.0)),Value::Text(&review.hash),Value::Text(&format!("{action:?}")),Value::Text(&format!("{origin:?}")),Value::Text(&tx.command_id),Value::Int(to_db(review.revision.0)?),Value::Text(&review.content_hash),Value::Text(&review.displayed_hash),Value::Text(&format!("{:?}",review.upstream)),review.prior_decision.map(|d|to_db(d.0)).transpose()?.map_or(Value::Null,Value::Int)])?;
            tx.invalidate_approvals()?;
            Ok(seq.0.to_string())
        })
    }
}
impl Tx<'_> {
    /// Append a bounded review document. Body is canonical UTF-8 text: every byte,
    /// including whitespace, is bound. Plan graph/source/policy inputs belong in body;
    /// interpreting their schema belongs to P1-05.
    pub fn put_review_document(
        &mut self,
        id: ArtifactId,
        expected: Option<Revision>,
        prd: ArtifactId,
        adr: Option<ArtifactId>,
        body: &str,
        displayed: &str,
    ) -> Result<Revision, TxError> {
        if body.trim().is_empty()
            || displayed.trim().is_empty()
            || body.len() > 262144
            || displayed.len() > 262144
            || body.contains('\0')
            || displayed.contains('\0')
        {
            return Err(deny("invalid review document"));
        }
        if id == prd || adr == Some(id) || version(self.conn, id, None)?.is_some() {
            return Err(deny("invalid subject identity"));
        }
        if version(self.conn, prd, None)?.is_none() {
            return Err(deny("PRD required"));
        }
        let pd = applicable(self.conn, prd)?.ok_or_else(|| deny("approved PRD required"))?;
        let ad = if let Some(a) = adr {
            let parent = self.conn.query_optional(
                "SELECT prd,kind FROM review_documents WHERE id=?1 ORDER BY revision DESC LIMIT 1",
                &[Value::Text(&id_to_text(a.0))],
                |r| Ok((r.text(0)?, r.text(1)?)),
            )?;
            if parent != Some((id_to_text(prd.0), "ADR".into())) {
                return Err(deny("applicable ADR for this PRD required"));
            }
            Some(applicable(self.conn, a)?.ok_or_else(|| deny("approved ADR required"))?)
        } else {
            None
        };
        let kind = if adr.is_some() { "PLAN" } else { "ADR" };
        let old = self.conn.query_optional(
            "SELECT kind,prd,adr FROM review_documents WHERE id=?1 ORDER BY revision DESC LIMIT 1",
            &[Value::Text(&id_to_text(id.0))],
            |r| Ok((r.text(0)?, r.text(1)?, r.optional_text(2)?)),
        )?;
        if old.is_some_and(|v| v != (kind.into(), id_to_text(prd.0), adr.map(|a| id_to_text(a.0))))
        {
            return Err(deny("subject kind and parents are immutable"));
        }
        let key = AggregateKey::new("review_document", id.0);
        let rev = match expected {
            Some(r) => self.update_aggregate(&key, r, &hash(&[body, displayed]))?,
            None => self.create_aggregate(&key, &hash(&[body, displayed]))?,
        };
        let event = self.append_event("ReviewDocumentCreated", Some((&key, rev)), body)?;
        self.conn.execute(
            "INSERT INTO review_documents VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            &[
                Value::Text(&id_to_text(id.0)),
                Value::Int(to_db(rev.0)?),
                Value::Text(if adr.is_some() { "PLAN" } else { "ADR" }),
                Value::Text(&id_to_text(prd.0)),
                adr.map(|a| id_to_text(a.0))
                    .as_deref()
                    .map_or(Value::Null, Value::Text),
                Value::Text(body),
                Value::Text(displayed),
                Value::Int(to_db(pd.0)?),
                ad.map(|a| to_db(a.0))
                    .transpose()?
                    .map_or(Value::Null, Value::Int),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        self.invalidate_approvals()?;
        Ok(rev)
    }
    pub fn review_bundle(&self, id: ArtifactId) -> Result<Option<ReviewBundle>, TxError> {
        Ok(bundle(self.conn, id)?)
    }
    pub(crate) fn development_hash(
        &self,
        prd: ArtifactId,
        adr: ArtifactId,
        plan: ArtifactId,
    ) -> Result<Option<String>, TxError> {
        Ok(development_hash(self.conn, prd, adr, plan)?)
    }
    pub fn reserve_approved_attempt(
        &mut self,
        id: OpaqueId,
        prd: ArtifactId,
        adr: ArtifactId,
        plan: ArtifactId,
    ) -> Result<(), TxError> {
        if self.has_execution_plan(plan)? {
            return Err(deny("typed plan requires exact step and source bindings"));
        }
        self.reserve_approval_with_dependencies_checked(id, prd, adr, plan)
    }
    pub(crate) fn reserve_approval_with_dependencies_checked(
        &mut self,
        id: OpaqueId,
        prd: ArtifactId,
        adr: ArtifactId,
        plan: ArtifactId,
    ) -> Result<(), TxError> {
        let h = self
            .development_hash(prd, adr, plan)?
            .ok_or_else(|| deny("all three applicable gates required"))?;
        self.conn.execute(
            "INSERT INTO approval_attempts VALUES (?1,?2,?3,?4,?5,'RESERVED')",
            &[
                Value::Text(&id_to_text(id)),
                Value::Text(&id_to_text(prd.0)),
                Value::Text(&id_to_text(adr.0)),
                Value::Text(&id_to_text(plan.0)),
                Value::Text(&h),
            ],
        )?;
        Ok(())
    }
    pub fn start_approved_attempt(&mut self, id: OpaqueId) -> Result<(), TxError> {
        if self
            .conn
            .query_optional(
                "SELECT 1 FROM scheduler_attempts WHERE attempt=?1",
                &[Value::Text(&id_to_text(id))],
                |r| r.int(0),
            )?
            .is_some()
        {
            return Err(deny(
                "scheduler reservation requires future dispatcher revalidation",
            ));
        }
        self.invalidate_approvals()?;
        let n=self.conn.execute("UPDATE approval_attempts SET phase='STARTING' WHERE id=?1 AND phase='RESERVED' AND NOT EXISTS(SELECT 1 FROM approval_stops WHERE attempt=?1)",&[Value::Text(&id_to_text(id))])?;
        if n != 1 {
            return Err(deny("attempt cannot start"));
        }
        Ok(())
    }
    /// Records the issue boundary for an already authorized attempt. This does not
    /// launch a process; the offline caller supplies the lifecycle fact.
    pub fn mark_approval_attempt_running(&mut self, id: OpaqueId) -> Result<(), TxError> {
        self.invalidate_approvals()?;
        let changed = self.conn.execute(
            "UPDATE approval_attempts SET phase='RUNNING' WHERE id=?1 AND phase='STARTING'
             AND NOT EXISTS(SELECT 1 FROM approval_stops WHERE attempt=?1)",
            &[Value::Text(&id_to_text(id))],
        )?;
        if changed != 1 {
            return Err(deny("attempt cannot become running"));
        }
        self.append_event("ApprovalAttemptRunning", None, &id_to_text(id))?;
        Ok(())
    }
    pub fn cancel_approved_attempt(&mut self, id: OpaqueId) -> Result<(), TxError> {
        self.stop_approval_attempt(&id_to_text(id), StopCause::TaskCancel, "attempt cancelled")
    }
    pub(crate) fn stop_approval_attempt(
        &mut self,
        id: &str,
        cause: StopCause,
        detail: &str,
    ) -> Result<(), TxError> {
        let event = self.append_event("ApprovalStopRequested", None, id)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO approval_stops VALUES (?1,?2)",
            &[Value::Text(id), Value::Int(to_db(event.0)?)],
        )?;
        self.record_stop_cause(id, cause, detail)?;
        self.conn.execute(
            "UPDATE approval_attempts SET phase='STOP_REQUESTED' WHERE id=?1 AND phase<>'FINISHED'",
            &[Value::Text(id)],
        )?;
        self.retire_stopped_scheduler_reservation(id)?;
        Ok(())
    }
    pub(crate) fn invalidate_approvals(&mut self) -> Result<(), TxError> {
        let rows=self.conn.query("SELECT id,prd,adr,plan,bundle FROM approval_attempts WHERE NOT EXISTS(SELECT 1 FROM approval_stops WHERE attempt=id)",&[],|r|Ok((r.text(0)?,r.text(1)?,r.text(2)?,r.text(3)?,r.text(4)?)))?;
        for (id, p, a, l, h) in rows {
            if self.development_hash(
                ArtifactId(id_from_text(&p)?),
                ArtifactId(id_from_text(&a)?),
                ArtifactId(id_from_text(&l)?),
            )? != Some(h)
            {
                self.stop_approval_attempt(
                    &id,
                    StopCause::ApprovalInvalidated,
                    "approval revoked or no longer applicable",
                )?;
            }
        }
        self.reconcile_dependencies(crate::EvaluationTrigger::Event)?;
        Ok(())
    }
    /// Stores bytes once; applicability is separate and can later become false.
    pub fn seal_approval_result(&mut self, id: OpaqueId, body: &str) -> Result<(), TxError> {
        self.conn.execute(
            "INSERT INTO approval_results VALUES (?1,?2)",
            &[Value::Text(&id_to_text(id)), Value::Text(body)],
        )?;
        self.conn.execute(
            "UPDATE approval_attempts SET phase='FINISHED' WHERE id=?1",
            &[Value::Text(&id_to_text(id))],
        )?;
        Ok(())
    }
}
impl ReadView<'_> {
    pub fn review_bundle(&self, id: ArtifactId) -> Result<Option<ReviewBundle>, StorageError> {
        bundle(self.conn, id)
    }
    pub fn applicable_decision(&self, id: ArtifactId) -> Result<Option<EventSeq>, StorageError> {
        applicable(self.conn, id)
    }
    pub fn human_decisions(&self, id: ArtifactId) -> Result<Vec<HumanDecision>, StorageError> {
        decisions(self.conn, id)
    }
    pub fn approval_stop_requested(&self, id: OpaqueId) -> Result<bool, StorageError> {
        Ok(self
            .conn
            .query_optional(
                "SELECT event FROM approval_stops WHERE attempt=?1",
                &[Value::Text(&id_to_text(id))],
                |r| r.int(0),
            )?
            .is_some())
    }
    pub fn approval_result(&self, id: OpaqueId) -> Result<Option<String>, StorageError> {
        Ok(self.conn.query_optional(
            "SELECT body FROM approval_results WHERE attempt=?1",
            &[Value::Text(&id_to_text(id))],
            |r| r.text(0),
        )?)
    }
    pub fn approval_result_applicable(&self, id: OpaqueId) -> Result<bool, StorageError> {
        Ok(self.approval_result(id)?.is_some() && !self.approval_stop_requested(id)?)
    }
}

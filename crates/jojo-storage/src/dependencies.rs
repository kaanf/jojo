//! Durable plan/fact snapshots, shared evaluator entry points and stop propagation.
//! No scheduler, SCM observer, deployment adapter or filesystem materializer.
use crate::approvals::{applicable, bundle};
use crate::model::{from_db, id_from_text, id_to_text, to_db};
use crate::sqlite::{Connection, Value};
use crate::{ReadView, StorageError, Tx, TxError, sha256_hex};
use jojo_domain::dependency::*;
use jojo_domain::identity::{ArtifactId, OpaqueId, Revision};
use jojo_domain::time::ClockReading;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvaluationTrigger {
    Event,
    Reconciliation,
    StartupRecovery,
    PreLaunch,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredExecutionPlan {
    pub subject: ArtifactId,
    pub revision: Revision,
    pub digest: String,
    pub plan: ExecutionPlan,
}
fn deny(s: impl Into<String>) -> TxError {
    TxError::Rejected(s.into())
}
fn current_approval(
    c: &Connection,
    id: ArtifactId,
) -> Result<Option<ApprovalBinding>, StorageError> {
    let Some(decision) = applicable(c, id)? else {
        return Ok(None);
    };
    let b = bundle(c, id)?
        .ok_or_else(|| StorageError::corrupt("applicable decision without bundle"))?;
    Ok(Some(ApprovalBinding {
        artifact: id,
        revision: b.revision,
        canonical_hash: b.content_hash,
        bundle_hash: b.hash,
        decision: decision.0,
    }))
}
pub(crate) fn load_plan(
    c: &Connection,
    id: ArtifactId,
    revision: Revision,
) -> Result<Option<StoredExecutionPlan>, StorageError> {
    let row = c.query_optional(
        "SELECT canonical,digest FROM execution_plans WHERE plan=?1 AND revision=?2",
        &[
            Value::Text(&id_to_text(id.0)),
            Value::Int(to_db(revision.0)?),
        ],
        |r| Ok((r.text(0)?, r.text(1)?)),
    )?;
    row.map(|(body, digest)| {
        if sha256_hex(&[body.as_bytes()]) != digest {
            return Err(StorageError::corrupt("execution plan digest mismatch"));
        }
        let plan = ExecutionPlan::import(body.as_bytes()).map_err(StorageError::corrupt)?;
        if plan.canonical() != body {
            return Err(StorageError::corrupt("noncanonical execution plan"));
        }
        Ok(StoredExecutionPlan {
            subject: id,
            revision,
            digest,
            plan,
        })
    })
    .transpose()
}
fn observations(c: &Connection) -> Result<Vec<Observation>, StorageError> {
    c.query("SELECT canonical,digest FROM dependency_observations o WHERE revision=(SELECT max(revision) FROM dependency_observations h WHERE h.id=o.id) ORDER BY id",&[],|r|Ok((r.text(0)?,r.text(1)?)))?.into_iter().map(|(body,digest)|{
        if sha256_hex(&[body.as_bytes()])!=digest{return Err(StorageError::corrupt("observation digest mismatch"))}
        let o=Observation::import(body.as_bytes()).map_err(StorageError::corrupt)?;
        if o.canonical()!=body{return Err(StorageError::corrupt("noncanonical observation"))}Ok(o)
    }).collect()
}
fn facts(c: &Connection, p: &ExecutionPlan) -> Result<Facts, StorageError> {
    let mut facts = Facts::default();
    for o in observations(c)? {
        facts.observations.insert(o.id.clone(), o);
    }
    let mut artifacts: BTreeSet<_> = p.approvals.iter().map(|a| a.artifact).collect();
    for e in p.artifacts.edges.iter().chain(&p.execution.edges) {
        if let Predicate::ArtifactApproved { binding, .. } = &e.predicate {
            artifacts.insert(binding.artifact);
        }
    }
    for id in artifacts {
        if let Some(a) = current_approval(c, id)? {
            facts.approvals.insert(id, a);
        }
    }
    Ok(facts)
}
fn blocked(reason: &str) -> StepEvaluation {
    StepEvaluation {
        truth: Truth::Unknown,
        reasons: vec![Evaluation {
            truth: Truth::Unknown,
            reason: reason.into(),
            evidence_id: None,
            observation_revision: None,
            sources: vec![],
            observed_sources: vec![],
            artifact: None,
            expires_at_unix_ms: None,
        }],
    }
}
fn evaluate(
    c: &Connection,
    id: ArtifactId,
    revision: Revision,
    step: NodeRef,
    now: ClockReading,
) -> Result<StepEvaluation, StorageError> {
    let Some(p) = load_plan(c, id, revision)? else {
        return Ok(blocked("execution plan revision missing"));
    };
    let Some(approved) = current_approval(c, id)? else {
        return Ok(blocked("execution plan approval missing or stale"));
    };
    if approved.revision != revision {
        return Ok(blocked("execution plan is not current"));
    }
    Ok(evaluate_step(&p.plan, step, &facts(c, &p.plan)?, now))
}
impl Tx<'_> {
    pub fn current_artifact_approval(
        &self,
        id: ArtifactId,
    ) -> Result<Option<ApprovalBinding>, TxError> {
        Ok(current_approval(self.conn, id)?)
    }
    pub(crate) fn has_execution_plan(&self, id: ArtifactId) -> Result<bool, TxError> {
        Ok(self
            .conn
            .query_optional(
                "SELECT 1 FROM execution_plans WHERE plan=?1 LIMIT 1",
                &[Value::Text(&id_to_text(id.0))],
                |r| r.int(0),
            )?
            .is_some())
    }
    pub(crate) fn scheduler_sources_match(
        &self,
        plan: ArtifactId,
        revision: Revision,
        step: NodeRef,
        observed: &SourceVector,
    ) -> Result<bool, TxError> {
        let p = load_plan(self.conn, plan, revision)?.ok_or_else(|| deny("plan missing"))?;
        Ok(materialize(
            &p.plan,
            step,
            &facts(self.conn, &p.plan)?,
            self.now,
            observed,
        )
        .is_ok())
    }
    /// Validates both graphs before any mutation. Canonical graph bytes are the
    /// existing human-review document body, not a second approval authority.
    pub fn put_execution_plan(
        &mut self,
        id: ArtifactId,
        expected: Option<Revision>,
        prd: ArtifactId,
        adr: ArtifactId,
        plan: &ExecutionPlan,
        displayed: &str,
    ) -> Result<Revision, TxError> {
        plan.validate().map_err(deny)?;
        for parent in [prd, adr] {
            let binding = current_approval(self.conn, parent)?
                .ok_or_else(|| deny("applicable PRD and ADR decisions required"))?;
            if !plan.approvals.contains(&binding)
                || !plan.artifacts.nodes.contains(&NodeRef {
                    id: parent.0,
                    revision: binding.revision,
                })
            {
                return Err(deny(
                    "plan must bind approved PRD/ADR revisions and graph nodes",
                ));
            }
        }
        for a in &plan.approvals {
            if current_approval(self.conn, a.artifact)?.as_ref() != Some(a) {
                return Err(deny("stale artifact binding"));
            }
        }
        for n in &plan.artifacts.nodes {
            if !plan
                .approvals
                .iter()
                .any(|a| a.artifact.0 == n.id && a.revision == n.revision)
            {
                return Err(deny("artifact node requires exact applicable decision"));
            }
        }
        let canonical = plan.canonical();
        // Bound the canonical import as strictly as later reads before committing.
        ExecutionPlan::import(canonical.as_bytes()).map_err(deny)?;
        let revision =
            self.put_review_document(id, expected, prd, Some(adr), &canonical, displayed)?;
        let digest = sha256_hex(&[canonical.as_bytes()]);
        let event = self.append_event("ExecutionPlanCreated", None, &digest)?;
        self.conn.execute(
            "INSERT INTO execution_plans VALUES (?1,?2,?3,?4,?5)",
            &[
                Value::Text(&id_to_text(id.0)),
                Value::Int(to_db(revision.0)?),
                Value::Text(&canonical),
                Value::Text(&digest),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        for graph in [&plan.artifacts, &plan.execution] {
            let kind = format!("{:?}", graph.kind);
            for n in &graph.nodes {
                self.conn.execute(
                    "INSERT INTO graph_nodes VALUES (?1,?2,?3,?4,?5)",
                    &[
                        Value::Text(&id_to_text(id.0)),
                        Value::Int(to_db(revision.0)?),
                        Value::Text(&kind),
                        Value::Text(&id_to_text(n.id)),
                        Value::Int(to_db(n.revision.0)?),
                    ],
                )?;
            }
            for e in &graph.edges {
                self.conn.execute(
                    "INSERT INTO graph_edges VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    &[
                        Value::Text(&id_to_text(id.0)),
                        Value::Int(to_db(revision.0)?),
                        Value::Text(&kind),
                        Value::Text(&id_to_text(e.id)),
                        Value::Text(&id_to_text(e.prerequisite.id)),
                        Value::Int(to_db(e.prerequisite.revision.0)?),
                        Value::Text(&id_to_text(e.dependent.id)),
                        Value::Int(to_db(e.dependent.revision.0)?),
                        Value::Text(&e.predicate.canonical()),
                    ],
                )?;
            }
        }
        self.reconcile_dependencies(EvaluationTrigger::Event)?;
        Ok(revision)
    }
    /// Controlled independent observations only. This is not a provider event handler.
    /// Failed reads are persisted as LookupFailed, not converted to success/absence.
    pub fn record_dependency_observation(
        &mut self,
        expected: Option<Revision>,
        o: &Observation,
    ) -> Result<(), TxError> {
        if o.id.trim().is_empty()
            || o.observer.trim().is_empty()
            || o.schema != DEPENDENCY_SCHEMA
            || o.expires_at_unix_ms <= o.observed.unix_millis
        {
            return Err(deny("invalid observation identity/schema/freshness"));
        }
        let latest = self
            .conn
            .query_optional(
                "SELECT max(revision) FROM dependency_observations WHERE id=?1",
                &[Value::Text(&o.id)],
                |r| r.optional_int(0),
            )?
            .flatten()
            .map(from_db)
            .transpose()?
            .map(Revision);
        if latest != expected || o.revision.0 != expected.map_or(1, |r| r.0.saturating_add(1)) {
            return Err(deny("observation revision CAS conflict"));
        }
        let body = o.canonical();
        Observation::import(body.as_bytes()).map_err(deny)?;
        let event = self.append_event(
            "DependencyObservationRecorded",
            None,
            &format!("{}@{}", o.id, o.revision.0),
        )?;
        self.conn.execute(
            "INSERT INTO dependency_observations VALUES (?1,?2,?3,?4,?5)",
            &[
                Value::Text(&o.id),
                Value::Int(to_db(o.revision.0)?),
                Value::Text(&body),
                Value::Text(&sha256_hex(&[body.as_bytes()])),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        self.reconcile_dependencies(EvaluationTrigger::Event)?;
        Ok(())
    }
    pub fn evaluate_dependencies(
        &self,
        id: ArtifactId,
        revision: Revision,
        step: NodeRef,
        _trigger: EvaluationTrigger,
    ) -> Result<StepEvaluation, TxError> {
        Ok(evaluate(self.conn, id, revision, step, self.now)?)
    }
    pub fn reserve_dependency_attempt(
        &mut self,
        attempt: OpaqueId,
        prd: ArtifactId,
        adr: ArtifactId,
        plan: ArtifactId,
        step: NodeRef,
        observed_bases: &SourceVector,
    ) -> Result<(), TxError> {
        if let Some(a) = current_approval(self.conn, plan)?
            && load_plan(self.conn, plan, a.revision)?.is_some_and(|p| p.plan.schema == 2)
        {
            return Err(deny("schema-2 plan requires scheduler admission"));
        }
        self.reserve_dependency_attempt_for_scheduler(attempt, prd, adr, plan, step, observed_bases)
    }
    pub(crate) fn reserve_dependency_attempt_for_scheduler(
        &mut self,
        attempt: OpaqueId,
        prd: ArtifactId,
        adr: ArtifactId,
        plan: ArtifactId,
        step: NodeRef,
        observed_bases: &SourceVector,
    ) -> Result<(), TxError> {
        let approved =
            current_approval(self.conn, plan)?.ok_or_else(|| deny("approved plan required"))?;
        let p = load_plan(self.conn, plan, approved.revision)?
            .ok_or_else(|| deny("typed execution plan required"))?;
        let evaluated = evaluate(self.conn, plan, p.revision, step, self.now)?;
        if evaluated.truth != Truth::Satisfied {
            return Err(deny(format!("dependencies block: {evaluated:?}")));
        }
        let sources = materialize(
            &p.plan,
            step,
            &facts(self.conn, &p.plan)?,
            self.now,
            observed_bases,
        )
        .map_err(deny)?;
        self.reserve_approval_with_dependencies_checked(attempt, prd, adr, plan)?;
        self.conn.execute("INSERT INTO dependency_attempt_inputs (attempt,plan,revision,step,step_revision,sources,evaluation_digest) VALUES (?1,?2,?3,?4,?5,?6,?7)",&[Value::Text(&id_to_text(attempt)),Value::Text(&id_to_text(plan.0)),Value::Int(to_db(p.revision.0)?),Value::Text(&id_to_text(step.id)),Value::Int(to_db(step.revision.0)?),Value::Text(&sources.canonical()),Value::Text(&sha256_hex(&[format!("{evaluated:?}").as_bytes()]))])?;
        self.append_event(
            "DependencyInputsBound",
            None,
            &format!("{attempt:?}|{sources:?}"),
        )?;
        Ok(())
    }
    /// Revalidate a frozen attempt against fresh local source observations. A blocked
    /// verdict is returned successfully so its stop intent commits with the verdict;
    /// callers must not turn a blocked verdict into a transaction error/rollback.
    pub fn revalidate_dependency_attempt(
        &mut self,
        attempt: OpaqueId,
        observed: &SourceVector,
        _trigger: EvaluationTrigger,
    ) -> Result<StepEvaluation, TxError> {
        let row=self.conn.query_optional(
            "SELECT plan,revision,step,step_revision,sources FROM dependency_attempt_inputs WHERE attempt=?1",
            &[Value::Text(&id_to_text(attempt))],
            |r|Ok((r.text(0)?,r.int(1)?,r.text(2)?,r.int(3)?,r.text(4)?)),
        )?.ok_or_else(||deny("dependency attempt missing"))?;
        let (id, revision, step, step_revision, frozen) = row;
        let id = ArtifactId(id_from_text(&id)?);
        let revision = Revision(from_db(revision)?);
        let step = NodeRef {
            id: id_from_text(&step)?,
            revision: Revision(from_db(step_revision)?),
        };
        let mut result = evaluate(self.conn, id, revision, step, self.now)?;
        if result.truth == Truth::Satisfied {
            let p = load_plan(self.conn, id, revision)?.ok_or_else(|| deny("plan missing"))?;
            match materialize(
                &p.plan,
                step,
                &facts(self.conn, &p.plan)?,
                self.now,
                observed,
            ) {
                Ok(current) if current.canonical() == frozen => {}
                Ok(_) => result = blocked("frozen source vector changed"),
                Err(reason) => result = blocked(&reason),
            }
        }
        let stopped = self
            .conn
            .query_optional(
                "SELECT 1 FROM approval_stops WHERE attempt=?1",
                &[Value::Text(&id_to_text(attempt))],
                |r| r.int(0),
            )?
            .is_some();
        if stopped {
            return Ok(blocked("attempt has durable stop intent; no restart"));
        }
        if result.truth != Truth::Satisfied {
            self.append_event(
                "DependencyInvalidated",
                None,
                &format!("{attempt:?}|{result:?}"),
            )?;
            self.stop_approval_attempt(
                &id_to_text(attempt),
                jojo_domain::outcome::StopCause::ApprovalInvalidated,
                "dependency no longer satisfied",
            )?;
        }
        Ok(result)
    }
    /// Event, sweep, recovery and prelaunch all reach `evaluate`, with one supplied
    /// clock sample per writer command. No attempts are replaced or restarted here.
    pub fn reconcile_dependencies(&mut self, _trigger: EvaluationTrigger) -> Result<(), TxError> {
        let rows=self.conn.query("SELECT attempt,plan,revision,step,step_revision FROM dependency_attempt_inputs WHERE NOT EXISTS(SELECT 1 FROM approval_stops s WHERE s.attempt=dependency_attempt_inputs.attempt)",&[],|r|Ok((r.text(0)?,r.text(1)?,r.int(2)?,r.text(3)?,r.int(4)?)))?;
        for (attempt, plan, revision, step, step_revision) in rows {
            let result = evaluate(
                self.conn,
                ArtifactId(id_from_text(&plan)?),
                Revision(from_db(revision)?),
                NodeRef {
                    id: id_from_text(&step)?,
                    revision: Revision(from_db(step_revision)?),
                },
                self.now,
            )?;
            if result.truth != Truth::Satisfied {
                self.append_event(
                    "DependencyInvalidated",
                    None,
                    &format!("{attempt}|{result:?}"),
                )?;
                self.stop_approval_attempt(
                    &attempt,
                    jojo_domain::outcome::StopCause::ApprovalInvalidated,
                    "dependency no longer satisfied",
                )?;
            }
        }
        Ok(())
    }
}
impl ReadView<'_> {
    pub fn current_artifact_approval(
        &self,
        id: ArtifactId,
    ) -> Result<Option<ApprovalBinding>, StorageError> {
        current_approval(self.conn, id)
    }
    pub fn execution_plan(
        &self,
        id: ArtifactId,
        revision: Revision,
    ) -> Result<Option<StoredExecutionPlan>, StorageError> {
        load_plan(self.conn, id, revision)
    }
    pub fn dependency_observations(&self) -> Result<Vec<Observation>, StorageError> {
        observations(self.conn)
    }
    pub fn evaluate_dependencies(
        &self,
        id: ArtifactId,
        revision: Revision,
        step: NodeRef,
        _trigger: EvaluationTrigger,
        now: ClockReading,
    ) -> Result<StepEvaluation, StorageError> {
        evaluate(self.conn, id, revision, step, now)
    }
    pub fn materialize_sources(
        &self,
        id: ArtifactId,
        revision: Revision,
        step: NodeRef,
        now: ClockReading,
        observed_bases: &SourceVector,
    ) -> Result<Result<SourceVector, String>, StorageError> {
        if evaluate(self.conn, id, revision, step, now)?.truth != Truth::Satisfied {
            return Ok(Err("plan/dependency approval missing or stale".into()));
        }
        let p = load_plan(self.conn, id, revision)?
            .ok_or_else(|| StorageError::corrupt("missing evaluated plan"))?;
        Ok(materialize(
            &p.plan,
            step,
            &facts(self.conn, &p.plan)?,
            now,
            observed_bases,
        ))
    }
}

impl ReadView<'_> {
    pub fn attempt_source_vector(
        &self,
        attempt: OpaqueId,
    ) -> Result<Option<SourceVector>, StorageError> {
        self.conn
            .query_optional(
                "SELECT sources FROM dependency_attempt_inputs WHERE attempt=?1",
                &[Value::Text(&id_to_text(attempt))],
                |r| r.text(0),
            )?
            .map(|s| SourceVector::import(s.as_bytes()).map_err(StorageError::corrupt))
            .transpose()
    }
    pub fn dependency_observation(
        &self,
        id: &str,
        revision: Revision,
    ) -> Result<Option<Observation>, StorageError> {
        self.conn
            .query_optional(
                "SELECT canonical,digest FROM dependency_observations WHERE id=?1 AND revision=?2",
                &[Value::Text(id), Value::Int(to_db(revision.0)?)],
                |r| Ok((r.text(0)?, r.text(1)?)),
            )?
            .map(|(s, d)| {
                if sha256_hex(&[s.as_bytes()]) != d {
                    return Err(StorageError::corrupt("observation digest mismatch"));
                }
                Observation::import(s.as_bytes()).map_err(StorageError::corrupt)
            })
            .transpose()
    }
}

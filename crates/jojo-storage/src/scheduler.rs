//! P1-06: short writer-only admission decisions. No dispatch or process effects.
use crate::dependencies::load_plan;
use crate::model::{from_db, id_from_text, id_to_text, to_db};
use crate::sqlite::{Connection, Value};
use crate::*;
use jojo_domain::dependency::{NodeRef, SourceVector, Truth};
use jojo_domain::identity::{ArtifactId, OpaqueId, Revision};
use jojo_domain::scheduler::{
    ResourceMode, ResourceNeed, StepResources, effective_priority, repository_key,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceConfig {
    pub revision: u64,
    pub default_repository: u32,
    pub capacities: BTreeMap<String, u32>,
}
impl ResourceConfig {
    fn capacity(&self, key: &str) -> u64 {
        self.capacities
            .get(key)
            .copied()
            .unwrap_or_else(|| {
                if key.starts_with("repository:") {
                    self.default_repository
                } else {
                    0
                }
            })
            .into()
    }
}
fn config(c: &Connection) -> Result<ResourceConfig, StorageError> {
    let (revision, default) = c.query_one(
        "SELECT revision,default_repository FROM scheduler_configs ORDER BY revision DESC LIMIT 1",
        &[],
        |r| Ok((r.int(0)?, r.int(1)?)),
    )?;
    let capacities=c.query("SELECT resource,capacity FROM scheduler_capacities WHERE revision=?1 ORDER BY resource",&[Value::Int(revision)],|r|Ok((r.text(0)?,r.int(1)? as u32)))?.into_iter().collect();
    Ok(ResourceConfig {
        revision: from_db(revision)?,
        default_repository: default as u32,
        capacities,
    })
}
fn deny(s: impl Into<String>) -> TxError {
    TxError::Rejected(s.into())
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueStatus {
    pub step: NodeRef,
    pub ready_sequence: u64,
    pub ready_unix_ms: i64,
    pub eligible_nanos: u64,
    pub control_revision: Revision,
    pub intent: String,
    pub skips: u64,
    pub barriers: Vec<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmissionDecision {
    pub step: NodeRef,
    pub attempt: Option<OpaqueId>,
    pub reason: String,
}
#[derive(Clone, Debug)]
struct Candidate {
    status: QueueStatus,
    plan: ArtifactId,
    revision: Revision,
    prd: ArtifactId,
    adr: ArtifactId,
    sources: SourceVector,
    clock: String,
    tick: u64,
    cooldown: i64,
    was_eligible: bool,
    resources: StepResources,
}
fn candidates(c: &Connection) -> Result<Vec<Candidate>, StorageError> {
    let rows=c.query("SELECT step,step_revision,plan,plan_revision,prd,adr,sources,ready_seq,ready_unix,clock_epoch,last_tick,eligible_nanos,intent,cooldown_until,skips,control_revision,was_eligible FROM scheduler_queue ORDER BY ready_seq,step",&[],|r|Ok((r.text(0)?,r.int(1)?,r.text(2)?,r.int(3)?,r.text(4)?,r.text(5)?,r.text(6)?,r.int(7)?,r.int(8)?,r.text(9)?,r.int(10)?,r.int(11)?,r.text(12)?,r.int(13)?,r.int(14)?,r.int(15)?,r.int(16)?)))?;
    rows.into_iter().map(|(step,sr,plan,rev,prd,adr,sources,seq,unix,clock,tick,age,intent,cooldown,skips,control_revision,was_eligible)|{
        let step=NodeRef{id:id_from_text(&step)?,revision:Revision(from_db(sr)?)};
        let plan=ArtifactId(id_from_text(&plan)?);let revision=Revision(from_db(rev)?);
        let p=load_plan(c,plan,revision)?.ok_or_else(||StorageError::corrupt("queue plan missing"))?;
        let resources=p.plan.resources.into_iter().find(|r|r.step==step).ok_or_else(||StorageError::corrupt("queue resource envelope missing"))?;
        let barriers=c.query("SELECT resource FROM scheduler_barriers WHERE step=?1 AND step_revision=?2 ORDER BY resource",&[Value::Text(&id_to_text(step.id)),Value::Int(sr)],|r|r.text(0))?;
        Ok(Candidate{status:QueueStatus{step,ready_sequence:from_db(seq)?,ready_unix_ms:unix,eligible_nanos:from_db(age)?,control_revision:Revision(from_db(control_revision)?),intent,skips:from_db(skips)?,barriers},plan,revision,prd:ArtifactId(id_from_text(&prd)?),adr:ArtifactId(id_from_text(&adr)?),sources:SourceVector::import(sources.as_bytes()).map_err(StorageError::corrupt)?,clock,tick:from_db(tick)?,cooldown,was_eligible:was_eligible==1,resources})
    }).collect()
}
impl ReadView<'_> {
    pub fn resource_config(&self) -> Result<ResourceConfig, StorageError> {
        config(self.conn)
    }
    pub fn scheduler_queue(&self) -> Result<Vec<QueueStatus>, StorageError> {
        Ok(candidates(self.conn)?
            .into_iter()
            .map(|c| c.status)
            .collect())
    }
}
impl Tx<'_> {
    pub(crate) fn resource_config_now(&self) -> Result<ResourceConfig, StorageError> {
        config(self.conn)
    }
    /// Full immutable configuration snapshot; the writer receipt supplies command CAS/idempotency.
    pub fn configure_resources(
        &mut self,
        expected: u64,
        default_repository: u32,
        capacities: BTreeMap<String, u32>,
    ) -> Result<u64, TxError> {
        if config(self.conn)?.revision != expected {
            return Err(deny("stale resource configuration"));
        }
        if !["global", "provider:claude", "provider:codex"]
            .iter()
            .all(|k| capacities.contains_key(*k))
            || capacities
                .keys()
                .any(|k| k.is_empty() || k.len() > 200 || k.contains('\0'))
        {
            return Err(deny("invalid resource configuration"));
        }
        let revision = expected
            .checked_add(1)
            .ok_or_else(|| deny("configuration overflow"))?;
        self.conn.execute(
            "INSERT INTO scheduler_configs VALUES (?1,?2)",
            &[
                Value::Int(to_db(revision)?),
                Value::Int(default_repository.into()),
            ],
        )?;
        for (key, value) in capacities {
            self.conn.execute(
                "INSERT INTO scheduler_capacities VALUES (?1,?2,?3)",
                &[
                    Value::Int(to_db(revision)?),
                    Value::Text(&key),
                    Value::Int(value.into()),
                ],
            )?;
        }
        self.append_event("ResourceConfigurationChanged", None, &revision.to_string())?;
        Ok(revision)
    }
    /// First readiness observation fixes queue order forever. Duplicate/stale queue commands
    /// cannot replace reviewed inputs or reset age. Capacity is checked only at admission.
    pub fn queue_step(
        &mut self,
        prd: ArtifactId,
        adr: ArtifactId,
        plan: ArtifactId,
        revision: Revision,
        step: NodeRef,
        observed: &SourceVector,
    ) -> Result<(), TxError> {
        if self.development_hash(prd, adr, plan)?.is_none() {
            return Err(deny("applicable plan/PRD/ADR relationship required"));
        }
        let p = load_plan(self.conn, plan, revision)?.ok_or_else(|| deny("plan missing"))?;
        if p.plan.schema != 2 || !p.plan.resources.iter().any(|r| r.step == step) {
            return Err(deny("reviewed schema-2 resource envelope required"));
        }
        if self
            .evaluate_dependencies(plan, revision, step, EvaluationTrigger::Event)?
            .truth
            != Truth::Satisfied
        {
            return Err(deny("step is not dependency ready"));
        }
        if !self.scheduler_sources_match(plan, revision, step, observed)? {
            return Err(deny("source observations do not match reviewed inputs"));
        }
        // Exact local observations are checked again during admission by P1-05.
        let event = self.append_event(
            "SchedulerReady",
            None,
            &format!("{plan:?}|{revision:?}|{step:?}"),
        )?;
        self.conn.execute("INSERT INTO scheduler_queue (step,step_revision,plan,plan_revision,prd,adr,sources,ready_seq,ready_unix,clock_epoch,last_tick) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",&[
            Value::Text(&id_to_text(step.id)),Value::Int(to_db(step.revision.0)?),Value::Text(&id_to_text(plan.0)),Value::Int(to_db(revision.0)?),Value::Text(&id_to_text(prd.0)),Value::Text(&id_to_text(adr.0)),Value::Text(&observed.canonical()),Value::Int(to_db(event.0)?),Value::Int(self.now_unix_ms),Value::Text(&id_to_text(self.now.epoch.0)),Value::Int(to_db(self.now.monotonic_nanos)?)])?;
        Ok(())
    }
    /// RUN/HOLD/CANCEL are explicit control commands. CANCEL is irreversible for this
    /// step revision and requests stop before any cooldown or capacity processing.
    pub fn control_scheduled_step(
        &mut self,
        step: NodeRef,
        expected: Revision,
        intent: &str,
        cooldown_until: i64,
    ) -> Result<(), TxError> {
        if !matches!(intent, "RUN" | "HOLD" | "CANCEL") {
            return Err(deny("invalid scheduler intent"));
        }
        let id = id_to_text(step.id);
        let rev = to_db(step.revision.0)?;
        let n=self.conn.execute("UPDATE scheduler_queue SET intent=?1,cooldown_until=?2,control_revision=control_revision+1,was_eligible=0 WHERE step=?3 AND step_revision=?4 AND control_revision=?5 AND intent<>'CANCEL'",&[Value::Text(intent),Value::Int(cooldown_until),Value::Text(&id),Value::Int(rev),Value::Int(to_db(expected.0)?)])?;
        if n != 1 {
            return Err(deny("step missing or cancelled"));
        }
        if intent != "RUN" {
            self.conn.execute(
                "DELETE FROM scheduler_barriers WHERE step=?1 AND step_revision=?2",
                &[Value::Text(&id), Value::Int(rev)],
            )?;
        }
        if intent == "CANCEL" {
            let attempts=self.conn.query("SELECT attempt FROM scheduler_attempts WHERE step=?1 AND step_revision=?2 AND sealed=0",&[Value::Text(&id),Value::Int(rev)],|r|r.text(0))?;
            for attempt in attempts {
                self.stop_approval_attempt(
                    &attempt,
                    jojo_domain::outcome::StopCause::TaskCancel,
                    "step cancelled",
                )?;
            }
        }
        self.append_event(
            "SchedulerControl",
            None,
            &format!("{step:?}|{intent}|{cooldown_until}"),
        )?;
        Ok(())
    }
    fn deficits(
        &self,
        needs: &[ResourceNeed],
        cfg: &ResourceConfig,
    ) -> Result<Vec<String>, TxError> {
        let mut blocked = vec![];
        for n in needs {
            let held = self.live_claims(&n.key)?;
            let admin_waiting = self.conn.query_one(
                "SELECT count(*) FROM owned_worktrees WHERE 'repository:' || repository=?1 AND state IN ('WAITING','CREATING','BLOCKED')",
                &[Value::Text(&n.key)], |r| r.int(0))? > 0;
            if admin_waiting {
                blocked.push(n.key.clone());
                continue;
            }
            let mut occupancy = 0u64;
            for h in &held {
                let units = self
                    .conn
                    .query_optional(
                        "SELECT units FROM scheduler_claim_units WHERE claim=?1",
                        &[Value::Int(to_db(h.id.0)?)],
                        |r| r.int(0),
                    )?
                    .unwrap_or(1);
                occupancy = occupancy.saturating_add(from_db(units)?);
            }
            if n.units > cfg.capacity(&n.key)
                || occupancy.saturating_add(n.units) > cfg.capacity(&n.key)
                || (!held.is_empty()
                    && (n.mode == ResourceMode::Exclusive
                        || held.iter().any(|h| h.mode == ClaimMode::Exclusive)))
            {
                blocked.push(n.key.clone());
            }
        }
        Ok(blocked)
    }
    fn set_barriers(&mut self, c: &mut Candidate, keys: Vec<String>) -> Result<(), TxError> {
        for key in keys {
            self.conn.execute(
                "INSERT OR IGNORE INTO scheduler_barriers VALUES (?1,?2,?3)",
                &[
                    Value::Text(&id_to_text(c.status.step.id)),
                    Value::Int(to_db(c.status.step.revision.0)?),
                    Value::Text(&key),
                ],
            )?;
            if !c.status.barriers.contains(&key) {
                c.status.barriers.push(key);
            }
        }
        Ok(())
    }
    /// Attempts are caller-supplied deterministic identities, consumed only on admission.
    /// One command may fill several slots; the writer transaction commits decisions,
    /// complete claims, frozen inputs, cursor, barriers and launch intents together.
    pub fn schedule(
        &mut self,
        expected_config: u64,
        attempts: &[OpaqueId],
    ) -> Result<Vec<AdmissionDecision>, TxError> {
        self.schedule_with_bindings(expected_config, attempts, None)
    }
    pub(crate) fn schedule_with_bindings(
        &mut self,
        expected_config: u64,
        attempts: &[OpaqueId],
        bindings: Option<&BTreeMap<OpaqueId, jojo_domain::identity::RepositoryId>>,
    ) -> Result<Vec<AdmissionDecision>, TxError> {
        let cfg = config(self.conn)?;
        if cfg.revision != expected_config {
            return Err(deny("stale resource configuration"));
        }
        self.invalidate_approvals()?; // revocation precedes cooldown
        let all = candidates(self.conn)?;
        let mut eligible = vec![];
        let mut decisions = vec![];
        for mut c in all {
            let active=self.conn.query_optional("SELECT 1 FROM dependency_attempt_inputs i JOIN approval_attempts a ON a.id=i.attempt WHERE i.step=?1 AND i.step_revision=?2 AND a.phase<>'FINISHED'",&[Value::Text(&id_to_text(c.status.step.id)),Value::Int(to_db(c.status.step.revision.0)?)],|r|r.int(0))?.is_some();
            let dependency = self
                .evaluate_dependencies(
                    c.plan,
                    c.revision,
                    c.status.step,
                    EvaluationTrigger::Reconciliation,
                )?
                .truth
                == Truth::Satisfied;
            let cooling =
                self.provider_is_cooling(c.resources.provider)? || c.cooldown > self.now_unix_ms;
            let allowed = c.status.intent == "RUN" && dependency && !active;
            let delta =
                if allowed && c.was_eligible && !cooling && c.clock == id_to_text(self.now.epoch.0)
                {
                    self.now.monotonic_nanos.saturating_sub(c.tick)
                } else {
                    0
                };
            c.status.eligible_nanos = c
                .status
                .eligible_nanos
                .saturating_add(delta)
                .min(i64::MAX as u64);
            self.conn.execute("UPDATE scheduler_queue SET clock_epoch=?1,last_tick=?2,eligible_nanos=?3,was_eligible=?6 WHERE step=?4 AND step_revision=?5",&[Value::Text(&id_to_text(self.now.epoch.0)),Value::Int(to_db(self.now.monotonic_nanos)?),Value::Int(to_db(c.status.eligible_nanos)?),Value::Text(&id_to_text(c.status.step.id)),Value::Int(to_db(c.status.step.revision.0)?),Value::Int(i64::from(allowed && !cooling))])?;
            if !allowed || cooling {
                self.conn.execute(
                    "DELETE FROM scheduler_barriers WHERE step=?1 AND step_revision=?2",
                    &[
                        Value::Text(&id_to_text(c.status.step.id)),
                        Value::Int(to_db(c.status.step.revision.0)?),
                    ],
                )?;
                decisions.push(AdmissionDecision {
                    step: c.status.step,
                    attempt: None,
                    reason: if c.status.intent != "RUN" {
                        c.status.intent.clone()
                    } else if !dependency {
                        "dependency/approval".into()
                    } else if active {
                        "unsealed attempt".into()
                    } else {
                        "cooldown".into()
                    },
                });
                continue;
            }
            // An exclusive repository waiter establishes its drain gate immediately.
            if c.resources.repository_mode == ResourceMode::Exclusive {
                let key = repository_key(c.resources.repository);
                self.set_barriers(&mut c, vec![key])?;
            }
            eligible.push(c);
        }
        if self.admission()? != Admission::Open {
            decisions.extend(eligible.into_iter().map(|c| AdmissionDecision {
                step: c.status.step,
                attempt: None,
                reason: "admission gate closed".into(),
            }));
            self.append_event(
                "SchedulerScan",
                None,
                &format!(
                    "config={expected_config}|clock={:?}|{decisions:?}",
                    self.now
                ),
            )?;
            return Ok(decisions);
        }
        let mut cursor =
            self.conn
                .query_one("SELECT repository FROM scheduler_cursor", &[], |r| {
                    r.text(0)
                })?;
        let mut ids: Vec<_> = attempts.to_vec();
        let mut pending: BTreeSet<usize> = (0..eligible.len()).collect();
        loop {
            let repos: BTreeSet<_> = pending
                .iter()
                .map(|i| repository_key(eligible[*i].resources.repository))
                .collect();
            let order: Vec<_> = repos
                .range((
                    std::ops::Bound::Excluded(cursor.clone()),
                    std::ops::Bound::Unbounded,
                ))
                .chain(repos.range(..=cursor.clone()))
                .cloned()
                .collect();
            let mut progress = false;
            for repo in order {
                let mut queue: Vec<_> = pending
                    .iter()
                    .copied()
                    .filter(|i| repository_key(eligible[*i].resources.repository) == repo)
                    .collect();
                queue.sort_by_key(|i| {
                    let c = &eligible[*i];
                    (
                        std::cmp::Reverse(effective_priority(
                            c.resources.priority,
                            c.status.eligible_nanos,
                        )),
                        c.status.ready_sequence,
                        c.status.step.id,
                    )
                });
                for i in queue {
                    let c = &eligible[i];
                    let needs = c.resources.needs();
                    let barrier = pending.iter().copied().filter(|j| *j != i).any(|j| {
                        let b = &eligible[j];
                        ((b.status.ready_sequence, b.status.step.id)
                            < (c.status.ready_sequence, c.status.step.id)
                            || (b.resources.repository_mode == ResourceMode::Exclusive
                                && c.status.barriers.is_empty()))
                            && b.status
                                .barriers
                                .iter()
                                .any(|key| needs.iter().any(|n| &n.key == key))
                    });
                    if barrier || !self.deficits(&needs, &cfg)?.is_empty() {
                        continue;
                    }
                    let before: Vec<_> = pending
                        .iter()
                        .copied()
                        .map(|j| Ok((j, self.deficits(&eligible[j].resources.needs(), &cfg)?)))
                        .collect::<Result<_, TxError>>()?;
                    let position = ids.iter().position(|id| {
                        bindings.is_none_or(|b| b.get(id) == Some(&c.resources.repository))
                    });
                    let Some(position) = position else {
                        if bindings.is_none() {
                            return Ok(decisions);
                        }
                        continue;
                    };
                    let attempt = ids.remove(position);
                    // P1-05 validates current approval, exact plan, source and all typed predicates.
                    self.reserve_dependency_attempt_for_scheduler(
                        attempt,
                        c.prd,
                        c.adr,
                        c.plan,
                        c.status.step,
                        &c.sources,
                    )?;
                    let holder = AggregateKey::new("scheduler_attempt", attempt);
                    self.create_aggregate(&holder, "RESERVED")?;
                    self.conn.execute("INSERT INTO scheduler_attempts (attempt,step,step_revision,epoch,config_revision) VALUES (?1,?2,?3,?4,?5)",&[Value::Text(&id_to_text(attempt)),Value::Text(&id_to_text(c.status.step.id)),Value::Int(to_db(c.status.step.revision.0)?),Value::Int(to_db(self.epoch.0)?),Value::Int(to_db(cfg.revision)?)])?;
                    for n in &needs {
                        let claim = self.acquire_claim(
                            &n.key,
                            if n.mode == ResourceMode::Shared {
                                ClaimMode::Shared
                            } else {
                                ClaimMode::Exclusive
                            },
                            &holder,
                        )?;
                        self.conn.execute(
                            "INSERT INTO scheduler_claim_units VALUES (?1,?2)",
                            &[Value::Int(to_db(claim.0)?), Value::Int(to_db(n.units)?)],
                        )?;
                    }
                    self.enqueue_effect(
                        &format!("scheduler-launch:{}", id_to_text(attempt)),
                        "provider-launch",
                        &format!(
                            "{attempt:?}|{:?}|{:?}|{}",
                            self.epoch, c.resources.provider, c.revision.0
                        ),
                    )?;
                    self.append_event(
                        "SchedulerReserved",
                        None,
                        &format!(
                            "{attempt:?}|{:?}|{needs:?}|clock={:?}",
                            c.status.step, self.now
                        ),
                    )?;
                    decisions.push(AdmissionDecision {
                        step: c.status.step,
                        attempt: Some(attempt),
                        reason: "reserved".into(),
                    });
                    let admitted_seq = c.status.ready_sequence;
                    self.conn.execute(
                        "DELETE FROM scheduler_barriers WHERE step=?1 AND step_revision=?2",
                        &[
                            Value::Text(&id_to_text(c.status.step.id)),
                            Value::Int(to_db(c.status.step.revision.0)?),
                        ],
                    )?;
                    pending.remove(&i);
                    // Count only an actual younger conflicting admission. Recompute shortages
                    // after admission; retain only contested keys, never partial physical holds.
                    for (j, deficits) in before {
                        if !pending.contains(&j) {
                            continue;
                        }
                        if eligible[j].status.ready_sequence >= admitted_seq {
                            continue;
                        }
                        let contested: Vec<_> = deficits
                            .into_iter()
                            .filter(|key| needs.iter().any(|n| &n.key == key))
                            .collect();
                        if contested.is_empty() {
                            continue;
                        }
                        eligible[j].status.skips = (eligible[j].status.skips + 1).min(3);
                        self.conn.execute("UPDATE scheduler_queue SET skips=?1 WHERE step=?2 AND step_revision=?3",&[Value::Int(to_db(eligible[j].status.skips)?),Value::Text(&id_to_text(eligible[j].status.step.id)),Value::Int(to_db(eligible[j].status.step.revision.0)?)])?;
                        if eligible[j].status.skips == 3 {
                            self.set_barriers(&mut eligible[j], contested)?;
                            self.append_event(
                                "SchedulerDrainBarrier",
                                None,
                                &format!("{:?}", eligible[j].status),
                            )?;
                        }
                    }
                    cursor = repo.clone();
                    self.conn.execute(
                        "UPDATE scheduler_cursor SET repository=?1",
                        &[Value::Text(&cursor)],
                    )?;
                    progress = true;
                    break;
                }
            }
            if !progress {
                break;
            }
        }
        for i in pending {
            let c = &eligible[i];
            decisions.push(AdmissionDecision {
                step: c.status.step,
                attempt: None,
                reason: format!(
                    "capacity or drain barrier: {:?}",
                    self.deficits(&c.resources.needs(), &cfg)?
                ),
            });
        }
        self.append_event(
            "SchedulerScan",
            None,
            &format!(
                "config={expected_config}|clock={:?}|{decisions:?}",
                self.now
            ),
        )?;
        Ok(decisions)
    }
}

impl Tx<'_> {
    /// Proven pre-issue settlement only. No API here releases issued/unknown work.
    /// A launch dispatcher must check both the stop record and reservation seal.
    pub fn retire_unissued_reservation(&mut self, attempt: OpaqueId) -> Result<(), TxError> {
        let id = id_to_text(attempt);
        let row = self
            .conn
            .query_optional(
                "SELECT step,step_revision,epoch,sealed FROM scheduler_attempts WHERE attempt=?1",
                &[Value::Text(&id)],
                |r| Ok((r.text(0)?, r.int(1)?, r.int(2)?, r.int(3)?)),
            )?
            .ok_or_else(|| deny("reservation missing"))?;
        let recovered = crate::recovery::recovered_absent(self.conn, &id)?;
        if row.3 != 0 || (from_db(row.2)? != self.epoch.0 && !recovered) {
            return Err(deny("reservation sealed or belongs to recovery epoch"));
        }
        let key = format!("scheduler-launch:{id}");
        if self
            .effect(&key)?
            .is_none_or(|e| e.state != EffectState::NotIssued)
        {
            return Err(deny("launch may have been issued"));
        }
        // STOP is committed with the tombstone; the original outbox intent remains
        // inspectable and cannot authorize dispatch after this reservation is sealed.
        let event = self.append_event("SchedulerNotStarted", None, &id)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO approval_stops VALUES (?1,?2)",
            &[Value::Text(&id), Value::Int(to_db(event.0)?)],
        )?;
        self.seal_approval_result(attempt, "NOT_STARTED: durable launch intent never issued")?;
        self.conn.execute(
            "UPDATE scheduler_attempts SET sealed=1 WHERE attempt=?1",
            &[Value::Text(&id)],
        )?;
        self.conn.execute("UPDATE scheduler_queue SET control_revision=control_revision+1,intent=CASE WHEN intent='CANCEL' THEN 'CANCEL' ELSE 'HOLD' END WHERE step=?1 AND step_revision=?2",&[Value::Text(&row.0),Value::Int(row.1)])?;
        let holder = AggregateKey::new("scheduler_attempt", attempt);
        let claims = crate::queries::live_claims(self.conn, None)?;
        let basis = if recovered {
            ReleaseBasis::RecoveryResolved
        } else {
            ReleaseBasis::NeverIssued
        };
        for claim in claims.into_iter().filter(|c| c.holder == holder) {
            self.release_claim(claim.id, basis)?;
        }
        Ok(())
    }
    pub(crate) fn retire_stopped_scheduler_reservation(&mut self, id: &str) -> Result<(), TxError> {
        let exists = self
            .conn
            .query_optional(
                "SELECT 1 FROM scheduler_attempts WHERE attempt=?1 AND sealed=0 AND epoch=?2",
                &[Value::Text(id), Value::Int(to_db(self.epoch.0)?)],
                |r| r.int(0),
            )?
            .is_some();
        if exists
            && self
                .effect(&format!("scheduler-launch:{id}"))?
                .is_some_and(|e| e.state == EffectState::NotIssued)
        {
            self.retire_unissued_reservation(id_from_text(id)?)?;
        }
        Ok(())
    }
}

impl Tx<'_> {
    /// Advisory liveness only. Expiry has no bearing on occupancy or release authority.
    pub fn record_scheduler_heartbeat(
        &mut self,
        attempt: OpaqueId,
        expires_at_unix_ms: i64,
    ) -> Result<(), TxError> {
        let id = id_to_text(attempt);
        let owned = self
            .conn
            .query_optional(
                "SELECT 1 FROM scheduler_attempts WHERE attempt=?1 AND epoch=?2 AND sealed=0",
                &[Value::Text(&id), Value::Int(to_db(self.epoch.0)?)],
                |r| r.int(0),
            )?
            .is_some();
        if !owned || expires_at_unix_ms <= self.now_unix_ms {
            return Err(deny("stale heartbeat identity/deadline"));
        }
        let event = self.append_event(
            "SchedulerHeartbeatHint",
            None,
            &format!("{id}|{expires_at_unix_ms}"),
        )?;
        self.conn.execute(
            "INSERT INTO scheduler_heartbeats VALUES (?1,?2,?3,?4,?5)",
            &[
                Value::Text(&id),
                Value::Int(to_db(self.epoch.0)?),
                Value::Int(self.now_unix_ms),
                Value::Int(expires_at_unix_ms),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(())
    }
}

impl Tx<'_> {
    /// Provider-local cooldown. An uncertain clock scope keeps this provider held until
    /// a fresh observation/explicit clear; other providers remain schedulable.
    pub fn set_provider_cooldown(
        &mut self,
        provider: jojo_domain::provider::ProviderKind,
        expected: u64,
        until_unix_ms: i64,
    ) -> Result<(), TxError> {
        let key = jojo_domain::scheduler::provider_key(provider);
        let current = self.conn.query_one(
            "SELECT coalesce(max(revision),0) FROM scheduler_provider_cooldowns WHERE provider=?1",
            &[Value::Text(key)],
            |r| r.int(0),
        )?;
        if from_db(current)? != expected {
            return Err(deny("stale provider cooldown"));
        }
        let delay = until_unix_ms.saturating_sub(self.now_unix_ms).max(0) as u64;
        let tick = if until_unix_ms == 0 {
            0
        } else {
            self.now
                .monotonic_nanos
                .saturating_add(delay.saturating_mul(1_000_000))
        };
        // Exclude the cooldown interval from aging even if no scan occurs before
        // its deadline. Original sequence/timestamp and already earned age persist.
        for c in candidates(self.conn)?
            .into_iter()
            .filter(|c| c.resources.provider == provider)
        {
            self.conn.execute(
                "UPDATE scheduler_queue SET was_eligible=0 WHERE step=?1 AND step_revision=?2",
                &[
                    Value::Text(&id_to_text(c.status.step.id)),
                    Value::Int(to_db(c.status.step.revision.0)?),
                ],
            )?;
        }
        let event = self.append_event(
            "ProviderCooldown",
            None,
            &format!("{key}|{until_unix_ms}|clock={:?}", self.now),
        )?;
        self.conn.execute(
            "INSERT INTO scheduler_provider_cooldowns VALUES (?1,?2,?3,?4,?5,?6)",
            &[
                Value::Text(key),
                Value::Int(to_db(
                    expected
                        .checked_add(1)
                        .ok_or_else(|| deny("cooldown revision overflow"))?,
                )?),
                Value::Text(&id_to_text(self.now.epoch.0)),
                Value::Int(until_unix_ms),
                Value::Int(to_db(tick)?),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(())
    }
    fn provider_is_cooling(
        &self,
        provider: jojo_domain::provider::ProviderKind,
    ) -> Result<bool, TxError> {
        let row=self.conn.query_optional("SELECT clock_epoch,until_unix,until_tick FROM scheduler_provider_cooldowns WHERE provider=?1 ORDER BY revision DESC LIMIT 1",&[Value::Text(jojo_domain::scheduler::provider_key(provider))],|r|Ok((r.text(0)?,r.int(1)?,r.int(2)?)))?;
        Ok(row.is_some_and(|(epoch, unix, tick)| {
            unix != 0
                && (epoch != id_to_text(self.now.epoch.0)
                    || unix > self.now_unix_ms
                    || tick > self.now.monotonic_nanos as i64)
        }))
    }
}

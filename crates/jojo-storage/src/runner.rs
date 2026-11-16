//! M3: the plan runner's durable state (schema 13) and the one view of a run that both the
//! helper's runner and the `PlanRun` query read.
//!
//! A run is an approved lead plan revision that the human started. Approving a plan does
//! not run it. The runner starts each ready step through the same path as a human Start,
//! so every check of a Start applies; it never retries a step: an attempt that fails, a
//! validation that fails, a rejected candidate or an integration conflict leaves the step
//! for the human (D2: repair is M4), and only the steps that need it stop.
use crate::model::{from_db, id_from_text, id_to_text, to_db};
use crate::sqlite::{Connection, Value};
use crate::*;
use jojo_domain::dependency::NodeRef;
use jojo_domain::identity::{ArtifactId, OpaqueId, RepositoryId, Revision};
use jojo_domain::ipc::{identity, number, object, text};
use jojo_domain::json::Json;
use jojo_domain::planner::{self, LeadReply, TurnKind};
use jojo_domain::provider::ProviderKind;
use jojo_domain::validation::ValidationPlan;

fn deny(s: impl Into<String>) -> TxError {
    TxError::Rejected(s.into())
}

/// What the human does to a run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlanControl {
    /// No new step starts; running attempts continue.
    Pause,
    Resume,
    /// Final: queued steps are cancelled and running attempts are stopped.
    Cancel,
}
impl PlanControl {
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "Pause" => Some(Self::Pause),
            "Resume" => Some(Self::Resume),
            "Cancel" => Some(Self::Cancel),
            _ => None,
        }
    }
}

/// Where one step of a run stands. Derived from durable records only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepState {
    /// A prerequisite is not integrated yet.
    Waiting,
    /// A blocking question about this step is unanswered.
    Blocked,
    /// Everything it needs is there; it starts when capacity allows.
    Ready,
    Running,
    /// Host validation is queued or running.
    Validating,
    /// Validation passed; the human accepts or rejects the candidate.
    Review,
    /// Accepted and not yet on the plan branch.
    Accepted,
    Integrated,
    /// The accepted candidate does not apply to the plan branch.
    Conflict,
    /// The attempt, its validation or the candidate failed.
    Failed,
    /// The runner could not start it.
    Refused,
    Cancelled,
}
impl StepState {
    pub fn name(self) -> &'static str {
        match self {
            Self::Waiting => "Waiting",
            Self::Blocked => "Blocked",
            Self::Ready => "Ready",
            Self::Running => "Running",
            Self::Validating => "Validating",
            Self::Review => "Review",
            Self::Accepted => "Accepted",
            Self::Integrated => "Integrated",
            Self::Conflict => "Conflict",
            Self::Failed => "Failed",
            Self::Refused => "Refused",
            Self::Cancelled => "Cancelled",
        }
    }
    /// The step needs the human and holds back everything that depends on it.
    pub fn stopped(self) -> bool {
        matches!(self, Self::Conflict | Self::Failed | Self::Refused)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunStep {
    pub step: NodeRef,
    pub key: String,
    pub title: String,
    pub kind: Option<String>,
    pub provider: ProviderKind,
    pub depends_on: Vec<OpaqueId>,
    pub write_scope: Vec<String>,
    pub state: StepState,
    pub detail: String,
    pub attempt: Option<OpaqueId>,
    pub candidate: Option<String>,
    pub receipt: Option<u64>,
    pub commit: Option<String>,
    /// The event of the human's acceptance; integration follows this order.
    pub accepted: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanRunView {
    pub plan: ArtifactId,
    pub revision: Revision,
    pub subjects: SliceSubjects,
    pub repository: RepositoryId,
    pub base: String,
    pub branch: String,
    /// The newest control: `RUNNING`, `PAUSED` or `CANCELLED`.
    pub control: String,
    /// `RUNNING`, `PAUSED`, `CANCELLED`, `HALTED` (nothing more can happen without the
    /// human) or `DONE` (every step is on the plan branch).
    pub state: String,
    /// The plan branch's newest commit: the base, or the last integration.
    pub head: String,
    pub steps: Vec<RunStep>,
}

/// The plan branch of a run: local, under `jojo/`, never pushed by the runner. The whole
/// identity: identities the helper makes share their first half.
pub fn plan_branch(plan: ArtifactId) -> String {
    format!("jojo/plan/{}", id_to_text(plan.0))
}

fn scopes_overlap(a: &str, b: &str) -> bool {
    let inside = |x: &str, y: &str| x == y || x.starts_with(&format!("{y}/"));
    inside(a, b) || inside(b, a)
}

impl PlanRunView {
    /// Steps the runner may start now, in plan order. A step whose write scope overlaps a
    /// step that is running or not yet on the plan branch waits for it.
    pub fn ready(&self) -> Vec<&RunStep> {
        if self.control != "RUNNING" {
            return vec![];
        }
        let busy: Vec<&RunStep> = self
            .steps
            .iter()
            .filter(|s| {
                matches!(
                    s.state,
                    StepState::Running
                        | StepState::Validating
                        | StepState::Review
                        | StepState::Accepted
                )
            })
            .collect();
        let mut chosen: Vec<&RunStep> = Vec::new();
        for step in self.steps.iter().filter(|s| s.state == StepState::Ready) {
            let overlaps = |other: &&RunStep| {
                step.write_scope
                    .iter()
                    .any(|a| other.write_scope.iter().any(|b| scopes_overlap(a, b)))
            };
            if !busy.iter().any(overlaps) && !chosen.iter().any(overlaps) {
                chosen.push(step);
            }
        }
        chosen
    }

    pub fn json(&self) -> Json {
        let key_of = |id: &OpaqueId| {
            self.steps
                .iter()
                .find(|s| s.step.id == *id)
                .map_or(String::new(), |s| s.key.clone())
        };
        let opt = |v: &Option<String>| v.as_ref().map_or(Json::Null, |s| text(s.clone()));
        object([
            ("plan", identity(self.plan.0)),
            ("revision", number(self.revision.0)),
            ("state", text(self.state.clone())),
            ("control", text(self.control.clone())),
            ("repository", identity(self.repository.0)),
            ("base", text(self.base.clone())),
            ("branch", text(self.branch.clone())),
            ("head", text(self.head.clone())),
            (
                "steps",
                Json::List(
                    self.steps
                        .iter()
                        .map(|s| {
                            object([
                                ("step", identity(s.step.id)),
                                ("key", text(s.key.clone())),
                                ("title", text(s.title.clone())),
                                ("kind", opt(&s.kind)),
                                (
                                    "provider",
                                    text(jojo_domain::project::provider_name(s.provider)),
                                ),
                                (
                                    "depends_on",
                                    Json::List(
                                        s.depends_on.iter().map(|d| text(key_of(d))).collect(),
                                    ),
                                ),
                                ("state", text(s.state.name())),
                                (
                                    "detail",
                                    text(s.detail.chars().take(512).collect::<String>()),
                                ),
                                ("attempt", s.attempt.map_or(Json::Null, identity)),
                                ("candidate", opt(&s.candidate)),
                                ("receipt", s.receipt.map_or(Json::Null, number)),
                                ("commit", opt(&s.commit)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

/// The lead plan a plan identity was drafted as: its turn, subjects, repository and base.
fn lead_subjects(
    c: &Connection,
    plan: ArtifactId,
) -> Result<Option<(String, SliceSubjects, RepositoryId, String)>, StorageError> {
    let row = c.query_optional(
        "SELECT s.turn,s.adr,s.validation,t.repository,t.base,t.feature FROM lead_plan_subjects s
         JOIN lead_turns t ON t.turn=s.turn WHERE s.plan=?1",
        &[Value::Text(&id_to_text(plan.0))],
        |r| {
            Ok((
                r.text(0)?,
                r.text(1)?,
                r.text(2)?,
                r.text(3)?,
                r.text(4)?,
                r.text(5)?,
            ))
        },
    )?;
    let Some((turn, adr, validation, repository, base, feature)) = row else {
        return Ok(None);
    };
    let prds = c.query(
        "SELECT DISTINCT artifact_id FROM artifact_versions WHERE feature_id=?1 LIMIT 2",
        &[Value::Text(&feature)],
        |r| r.text(0),
    )?;
    let [prd] = prds.as_slice() else {
        return Ok(None);
    };
    Ok(Some((
        turn,
        SliceSubjects {
            prd: ArtifactId(id_from_text(prd)?),
            adr: ArtifactId(id_from_text(&adr)?),
            plan,
            validation: ArtifactId(id_from_text(&validation)?),
        },
        RepositoryId(id_from_text(&repository)?),
        base,
    )))
}

fn control(c: &Connection, plan: &str, revision: i64) -> Result<String, StorageError> {
    Ok(c.query_one(
        "SELECT state FROM plan_run_controls WHERE plan=?1 AND revision=?2 ORDER BY ordinal DESC LIMIT 1",
        &[Value::Text(plan), Value::Int(revision)],
        |r| r.text(0),
    )?)
}

/// Every run, newest revision first per plan; `running` keeps only those whose newest
/// control is RUNNING.
fn runs(c: &Connection, running: bool) -> Result<Vec<(String, i64)>, StorageError> {
    let all = c.query(
        "SELECT plan,revision FROM plan_runs ORDER BY plan, revision DESC",
        &[],
        |r| Ok((r.text(0)?, r.int(1)?)),
    )?;
    let mut out = Vec::new();
    for (plan, revision) in all {
        if !running || control(c, &plan, revision)? == "RUNNING" {
            out.push((plan, revision));
        }
    }
    Ok(out)
}

/// A step's state, its detail, attempt, candidate, validation receipt and plan commit.
type StepFacts = (
    StepState,
    String,
    Option<OpaqueId>,
    Option<String>,
    Option<u64>,
    Option<String>,
);

fn step_state(
    v: &ReadView<'_>,
    plan: &str,
    revision: i64,
    step: NodeRef,
    cancelled: bool,
) -> Result<StepFacts, StorageError> {
    let c = v.conn;
    let id = id_to_text(step.id);
    let refusal = c.query_optional(
        "SELECT detail FROM plan_step_refusals WHERE plan=?1 AND revision=?2 AND step=?3",
        &[Value::Text(plan), Value::Int(revision), Value::Text(&id)],
        |r| r.text(0),
    )?;
    let attempt = c
        .query_optional(
            "SELECT attempt FROM scheduler_attempts WHERE step=?1 AND step_revision=?2
             ORDER BY rowid DESC LIMIT 1",
            &[Value::Text(&id), Value::Int(to_db(step.revision.0)?)],
            |r| r.text(0),
        )?
        .map(|a| id_from_text(&a))
        .transpose()?;
    let Some(attempt) = attempt else {
        if let Some(detail) = refusal {
            return Ok((StepState::Refused, detail, None, None, None, None));
        }
        if cancelled {
            return Ok((StepState::Cancelled, String::new(), None, None, None, None));
        }
        let question = c.query_optional(
            "SELECT q.prompt FROM step_questions q
                 JOIN aggregates a ON a.kind='step_question' AND a.id=q.question
                 WHERE q.step=?1 AND a.state='BLOCKING'
                   AND NOT EXISTS(SELECT 1 FROM step_answers s WHERE s.question=q.question)
                 LIMIT 1",
            &[Value::Text(&id)],
            |r| r.text(0),
        )?;
        return Ok(match question {
            Some(q) => (StepState::Blocked, q, None, None, None, None),
            None => (StepState::Ready, String::new(), None, None, None, None),
        });
    };
    let some = Some(attempt);
    let Some(result) = v.attempt_result(attempt)? else {
        // A Start that was committed but could not launch is refused, not running.
        return Ok(match refusal {
            Some(detail) => (StepState::Refused, detail, some, None, None, None),
            None => (StepState::Running, String::new(), some, None, None, None),
        });
    };
    if result.outcome != jojo_domain::attempt::AttemptOutcome::Succeeded {
        let detail = format!("attempt {:?}: {}", result.outcome, result.reason);
        return Ok((StepState::Failed, detail, some, None, None, None));
    }
    let Some(candidate) = result.candidate.clone() else {
        let detail = "the attempt changed nothing".to_owned();
        return Ok((StepState::Failed, detail, some, None, None, None));
    };
    let cand = Some(candidate.clone());
    let integration = c.query_optional(
        "SELECT outcome,commit_id,detail FROM plan_integrations WHERE attempt=?1",
        &[Value::Text(&id_to_text(attempt))],
        |r| Ok((r.text(0)?, r.optional_text(1)?, r.text(2)?)),
    )?;
    let receipt = match v.validation_state(attempt)? {
        ValidationState::Passed(e) | ValidationState::Failed(e) => Some(e.0),
        _ => None,
    };
    match integration {
        Some((outcome, commit, _)) if outcome == "INTEGRATED" => {
            return Ok((
                StepState::Integrated,
                String::new(),
                some,
                cand,
                receipt,
                commit,
            ));
        }
        Some((_, _, detail)) => {
            return Ok((StepState::Conflict, detail, some, cand, receipt, None));
        }
        None => {}
    }
    if v.candidate_rejected(attempt)? {
        let detail = "the human rejected the candidate".to_owned();
        return Ok((StepState::Failed, detail, some, cand, receipt, None));
    }
    if v.candidate_acceptance(attempt)?.is_some() {
        return Ok((
            StepState::Accepted,
            String::new(),
            some,
            cand,
            receipt,
            None,
        ));
    }
    Ok(match v.validation_state(attempt)? {
        ValidationState::NotRun => (
            StepState::Validating,
            "queued".into(),
            some,
            cand,
            None,
            None,
        ),
        ValidationState::Unknown(why) => {
            let running = v
                .validation_runs(attempt)?
                .last()
                .is_some_and(|r| r.ended.is_none());
            if running {
                (StepState::Validating, why, some, cand, None, None)
            } else {
                let detail = format!("validation ended without a verdict: {why}");
                (StepState::Failed, detail, some, cand, None, None)
            }
        }
        ValidationState::Passed(_) => (StepState::Review, String::new(), some, cand, receipt, None),
        ValidationState::Failed(_) => {
            let detail = "host validation failed".to_owned();
            (StepState::Failed, detail, some, cand, receipt, None)
        }
    })
}

fn view(v: &ReadView<'_>, plan: &str, revision: i64) -> Result<PlanRunView, StorageError> {
    let c = v.conn;
    let (prd, adr, validation, repository, base, branch) = c.query_one(
        "SELECT prd,adr,validation,repository,base,branch FROM plan_runs WHERE plan=?1 AND revision=?2",
        &[Value::Text(plan), Value::Int(revision)],
        |r| Ok((r.text(0)?, r.text(1)?, r.text(2)?, r.text(3)?, r.text(4)?, r.text(5)?)),
    )?;
    let subject = ArtifactId(id_from_text(plan)?);
    let stored = crate::dependencies::load_plan(c, subject, Revision(from_db(revision)?))?
        .ok_or_else(|| StorageError::corrupt("plan run without its plan"))?;
    let turn = c.query_one(
        "SELECT turn FROM lead_plan_subjects WHERE plan=?1",
        &[Value::Text(plan)],
        |r| r.text(0),
    )?;
    let names = c.query(
        "SELECT step,key,title FROM lead_plan_steps WHERE turn=?1 ORDER BY ordinal",
        &[Value::Text(&turn)],
        |r| Ok((r.text(0)?, r.text(1)?, r.text(2)?)),
    )?;
    let reply = c.query_one(
        "SELECT reply FROM lead_turn_results WHERE turn=?1",
        &[Value::Text(&turn)],
        |r| r.text(0),
    )?;
    let kinds: Vec<Option<String>> = match planner::parse_reply(TurnKind::Plan, &reply) {
        Ok(LeadReply::Plan(draft)) => draft.steps.into_iter().map(|s| s.kind).collect(),
        _ => vec![],
    };
    let document = v
        .review_document_content(ArtifactId(id_from_text(&validation)?))?
        .ok_or_else(|| StorageError::corrupt("plan run without its validation plan"))?;
    let checks = ValidationPlan::import(document.as_bytes()).map_err(StorageError::corrupt)?;
    let control = control(c, plan, revision)?;
    let cancelled = control == "CANCELLED";
    let mut steps = Vec::new();
    for (ordinal, (step, key, title)) in names.into_iter().enumerate() {
        let node = NodeRef {
            id: id_from_text(&step)?,
            revision: Revision(1),
        };
        let provider = stored
            .plan
            .resources
            .iter()
            .find(|r| r.step == node)
            .map(|r| r.provider)
            .ok_or_else(|| StorageError::corrupt("plan step without resources"))?;
        let depends_on: Vec<OpaqueId> = stored
            .plan
            .execution
            .edges
            .iter()
            .filter(|e| e.dependent == node)
            .map(|e| e.prerequisite.id)
            .collect();
        let write_scope = checks
            .step(node)
            .map(|s| {
                s.write_scope
                    .iter()
                    .map(|p| p.as_str().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        let (state, detail, attempt, candidate, receipt, commit) =
            step_state(v, plan, revision, node, cancelled)?;
        let accepted = match attempt {
            Some(a) => c
                .query_optional(
                    "SELECT event FROM candidate_acceptances WHERE attempt=?1",
                    &[Value::Text(&id_to_text(a))],
                    |r| r.int(0),
                )?
                .map(from_db)
                .transpose()?,
            None => None,
        };
        steps.push(RunStep {
            step: node,
            key,
            title,
            kind: kinds.get(ordinal).cloned().flatten(),
            provider,
            depends_on,
            write_scope,
            state,
            detail,
            attempt,
            candidate,
            receipt,
            commit,
            accepted,
        });
    }
    // A step is ready only when every prerequisite is on the plan branch; a step behind a
    // stopped one stays waiting, and says so.
    for i in 0..steps.len() {
        if steps[i].state != StepState::Ready {
            continue;
        }
        let waiting: Vec<String> = steps[i]
            .depends_on
            .iter()
            .filter_map(|d| steps.iter().find(|s| s.step.id == *d))
            .filter(|s| s.state != StepState::Integrated)
            .map(|s| s.key.clone())
            .collect();
        if !waiting.is_empty() {
            steps[i].state = StepState::Waiting;
            steps[i].detail = format!("waits for {}", waiting.join(", "));
        }
    }
    let head = c
        .query_optional(
            "SELECT commit_id FROM plan_integrations WHERE plan=?1 AND revision=?2
             AND outcome='INTEGRATED' ORDER BY ordinal DESC LIMIT 1",
            &[Value::Text(plan), Value::Int(revision)],
            |r| r.text(0),
        )?
        .unwrap_or_else(|| base.clone());
    let state = run_state(&control, &steps);
    Ok(PlanRunView {
        plan: subject,
        revision: Revision(from_db(revision)?),
        subjects: SliceSubjects {
            prd: ArtifactId(id_from_text(&prd)?),
            adr: ArtifactId(id_from_text(&adr)?),
            plan: subject,
            validation: ArtifactId(id_from_text(&validation)?),
        },
        repository: RepositoryId(id_from_text(&repository)?),
        base,
        branch,
        control,
        state,
        head,
        steps,
    })
}

/// DONE when every step is integrated; HALTED when every step that is not integrated is
/// stopped or waits behind a stopped one.
fn run_state(control: &str, steps: &[RunStep]) -> String {
    if steps.iter().all(|s| s.state == StepState::Integrated) {
        return "DONE".into();
    }
    if control != "RUNNING" {
        return control.into();
    }
    let mut stuck: Vec<OpaqueId> = Vec::new();
    for s in steps {
        if s.state.stopped() || s.depends_on.iter().any(|d| stuck.contains(d)) {
            stuck.push(s.step.id);
        }
    }
    let halted = !stuck.is_empty()
        && steps
            .iter()
            .all(|s| s.state == StepState::Integrated || stuck.contains(&s.step.id));
    if halted { "HALTED" } else { "RUNNING" }.into()
}

impl Tx<'_> {
    /// The human starts an approved lead plan. Every document of the plan must be
    /// approved now; a revision is run once.
    pub fn run_plan(&mut self, plan: ArtifactId) -> Result<String, TxError> {
        self.invalidate_approvals()?;
        let (_, subjects, repository, base) = lead_subjects(self.conn, plan)?
            .ok_or_else(|| deny("RunRefused: not a plan a lead drafted"))?;
        for (name, id) in [
            ("requirements", subjects.prd),
            ("ADR", subjects.adr),
            ("validation plan", subjects.validation),
            ("plan", subjects.plan),
        ] {
            if self.current_artifact_approval(id)?.is_none() {
                return Err(deny(format!(
                    "RunRefused: missing or stale approval: {name}"
                )));
            }
        }
        let revision = self
            .current_artifact_approval(plan)?
            .ok_or_else(|| deny("RunRefused: missing or stale approval: plan"))?
            .revision;
        if crate::dependencies::load_plan(self.conn, plan, revision)?.is_none() {
            return Err(deny(
                "RunRefused: the approved revision is not an execution plan",
            ));
        }
        let plan_text = id_to_text(plan.0);
        let rev = to_db(revision.0)?;
        if self
            .conn
            .query_optional(
                "SELECT 1 FROM plan_runs WHERE plan=?1 AND revision=?2",
                &[Value::Text(&plan_text), Value::Int(rev)],
                |r| r.int(0),
            )?
            .is_some()
        {
            return Err(deny(
                "RunRefused: this plan revision was already started; resume it instead",
            ));
        }
        let event = self.append_event("PlanRunStarted", None, &format!("{plan_text}|{rev}"))?;
        self.conn.execute(
            "INSERT INTO plan_runs VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            &[
                Value::Text(&plan_text),
                Value::Int(rev),
                Value::Text(&id_to_text(subjects.prd.0)),
                Value::Text(&id_to_text(subjects.adr.0)),
                Value::Text(&id_to_text(subjects.validation.0)),
                Value::Text(&id_to_text(repository.0)),
                Value::Text(&base),
                Value::Text(&plan_branch(plan)),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        self.plan_control_row(&plan_text, rev, "RUNNING", "started by the human")?;
        Ok(format!("plan run revision={}", revision.0))
    }

    fn plan_control_row(
        &mut self,
        plan: &str,
        revision: i64,
        state: &str,
        reason: &str,
    ) -> Result<(), TxError> {
        let event = self.append_event(
            "PlanRunControl",
            None,
            &format!("{plan}|{revision}|{state}|{reason}"),
        )?;
        self.conn.execute(
            "INSERT INTO plan_run_controls
             SELECT ?1,?2,coalesce(max(ordinal),0)+1,?3,?4,?5 FROM plan_run_controls
             WHERE plan=?1 AND revision=?2",
            &[
                Value::Text(plan),
                Value::Int(revision),
                Value::Text(state),
                Value::Text(reason),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(())
    }

    /// Pause, resume or cancel the newest run of `plan`.
    pub fn control_plan(
        &mut self,
        plan: ArtifactId,
        action: PlanControl,
    ) -> Result<String, TxError> {
        let plan_text = id_to_text(plan.0);
        let revision = self
            .conn
            .query_optional(
                "SELECT max(revision) FROM plan_runs WHERE plan=?1",
                &[Value::Text(&plan_text)],
                |r| r.optional_int(0),
            )?
            .flatten()
            .ok_or_else(|| deny("UnknownPlanRun: this plan was never started"))?;
        let current = control(self.conn, &plan_text, revision)?;
        let next = match (action, current.as_str()) {
            (_, "CANCELLED") => return Err(deny("PlanRunCancelled: the run was cancelled")),
            (PlanControl::Pause, "RUNNING") => "PAUSED",
            (PlanControl::Resume, "PAUSED") => "RUNNING",
            (PlanControl::Cancel, _) => "CANCELLED",
            (_, state) => return Ok(format!("plan run already {state}")),
        };
        self.plan_control_row(&plan_text, revision, next, "by the human")?;
        if next == "CANCELLED" {
            // Queued steps are cancelled; their unsealed attempts are asked to stop.
            let queued = self.conn.query(
                "SELECT q.step,q.step_revision,q.control_revision FROM scheduler_queue q
                 WHERE q.plan=?1 AND q.plan_revision=?2 AND q.intent<>'CANCEL'",
                &[Value::Text(&plan_text), Value::Int(revision)],
                |r| Ok((r.text(0)?, r.int(1)?, r.int(2)?)),
            )?;
            for (step, step_revision, control_revision) in queued {
                let node = NodeRef {
                    id: id_from_text(&step)?,
                    revision: Revision(from_db(step_revision)?),
                };
                self.control_scheduled_step(
                    node,
                    Revision(from_db(control_revision)?),
                    "CANCEL",
                    0,
                )?;
            }
        }
        Ok(format!("plan run {next}"))
    }

    /// A helper that starts again finds every run paused: the human resumes it after the
    /// recovery review. Returns how many were paused.
    pub fn pause_running_plans(&mut self, reason: &str) -> Result<u64, TxError> {
        let running = runs(self.conn, true)?;
        for (plan, revision) in &running {
            self.plan_control_row(plan, *revision, "PAUSED", reason)?;
        }
        Ok(running.len() as u64)
    }

    /// The runner could not start `step`; it is not tried again.
    pub fn refuse_plan_step(
        &mut self,
        plan: ArtifactId,
        revision: Revision,
        step: OpaqueId,
        detail: &str,
    ) -> Result<(), TxError> {
        let detail: String = detail.chars().take(1024).collect();
        let event = self.append_event(
            "PlanStepRefused",
            None,
            &format!("{}|{}|{detail}", id_to_text(plan.0), id_to_text(step)),
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO plan_step_refusals VALUES (?1,?2,?3,?4,?5)",
            &[
                Value::Text(&id_to_text(plan.0)),
                Value::Int(to_db(revision.0)?),
                Value::Text(&id_to_text(step)),
                Value::Text(&detail),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(())
    }
}

impl ReadView<'_> {
    /// Runs whose newest control is RUNNING, as the runner sees them.
    pub fn running_plans(&self) -> Result<Vec<PlanRunView>, StorageError> {
        runs(self.conn, true)?
            .into_iter()
            .map(|(plan, revision)| view(self, &plan, revision))
            .collect()
    }
    /// The newest run of `plan`, if it was ever started.
    pub fn plan_run(&self, plan: ArtifactId) -> Result<Option<PlanRunView>, StorageError> {
        let plan_text = id_to_text(plan.0);
        let revision = self
            .conn
            .query_optional(
                "SELECT max(revision) FROM plan_runs WHERE plan=?1",
                &[Value::Text(&plan_text)],
                |r| r.optional_int(0),
            )?
            .flatten();
        revision
            .map(|revision| view(self, &plan_text, revision))
            .transpose()
    }
    /// M3 (protocol 1.3): the newest run of one plan, or `null`.
    pub fn ipc_plan_run(&self, plan: ArtifactId) -> Result<Json, StorageError> {
        Ok(self.plan_run(plan)?.map_or(Json::Null, |v| v.json()))
    }
    pub fn plans_running(&self) -> Result<bool, StorageError> {
        Ok(!runs(self.conn, true)?.is_empty())
    }
}

/// E8 (M3): the commit a step of a run starts from: the newest commit on the plan
/// branch, which holds the base and every step integrated so far. `None` without a run.
pub(crate) fn run_head(
    c: &Connection,
    plan: ArtifactId,
    revision: Revision,
) -> Result<Option<String>, StorageError> {
    let p = Value::Text(&id_to_text(plan.0));
    let r = Value::Int(to_db(revision.0)?);
    let Some(base) = c.query_optional(
        "SELECT base FROM plan_runs WHERE plan=?1 AND revision=?2",
        &[p, r],
        |row| row.text(0),
    )?
    else {
        return Ok(None);
    };
    let head = c.query_optional(
        "SELECT commit_id FROM plan_integrations WHERE plan=?1 AND revision=?2
         AND outcome='INTEGRATED' ORDER BY ordinal DESC LIMIT 1",
        &[p, r],
        |row| row.text(0),
    )?;
    Ok(Some(head.unwrap_or(base)))
}

/// E8 (M3): a step of a run may start from a commit other than the approved base only
/// when that commit is one this run integrated onto its plan branch and every
/// prerequisite of the step was integrated at or before it. The plan's own evidence edges
/// are evaluated as approved; this decides only the source.
pub(crate) fn run_source_allowed(
    c: &Connection,
    plan: ArtifactId,
    revision: Revision,
    approved: &jojo_domain::dependency::ExecutionPlan,
    step: NodeRef,
    observed: &jojo_domain::dependency::SourceVector,
) -> Result<bool, StorageError> {
    use jojo_domain::dependency::SourceRevision;
    let p = id_to_text(plan.0);
    let r = to_db(revision.0)?;
    let Some(repository) = c.query_optional(
        "SELECT repository FROM plan_runs WHERE plan=?1 AND revision=?2",
        &[Value::Text(&p), Value::Int(r)],
        |row| row.text(0),
    )?
    else {
        return Ok(false);
    };
    let input = approved.inputs.iter().find(|i| i.step == step);
    let [source] = observed.sources.as_slice() else {
        return Ok(false);
    };
    let SourceRevision::Commit(commit) = &source.revision else {
        return Ok(false);
    };
    if id_to_text(source.repository.0) != repository
        || input.is_none_or(|i| i.environment != observed.environment)
    {
        return Ok(false);
    }
    let ordinal = |column: &str, value: &str| -> Result<Option<i64>, StorageError> {
        Ok(c.query_optional(
            &format!(
                "SELECT ordinal FROM plan_integrations WHERE plan=?1 AND revision=?2
                 AND outcome='INTEGRATED' AND {column}=?3"
            ),
            &[Value::Text(&p), Value::Int(r), Value::Text(value)],
            |row| row.int(0),
        )?)
    };
    let Some(at) = ordinal("commit_id", commit)? else {
        return Ok(false);
    };
    for e in approved
        .execution
        .edges
        .iter()
        .filter(|e| e.dependent == step)
    {
        match ordinal("step", &id_to_text(e.prerequisite.id))? {
            Some(k) if k <= at => {}
            _ => return Ok(false),
        }
    }
    Ok(true)
}

impl Tx<'_> {
    /// Records the integration of an accepted step exactly once. When it is on the plan
    /// branch, the evidence its dependents wait for is observed: the host validation of
    /// exactly this step passed and the human accepted it.
    pub fn record_integration(
        &mut self,
        plan: ArtifactId,
        revision: Revision,
        step: OpaqueId,
        attempt: OpaqueId,
        parent: &str,
        outcome: &Integration,
    ) -> Result<String, TxError> {
        if self.candidate_accepted(attempt)?.is_none() {
            return Err(deny("only an accepted candidate is integrated"));
        }
        let head = run_head(self.conn, plan, revision)?
            .ok_or_else(|| deny("UnknownPlanRun: this plan was never started"))?;
        if head != parent {
            return Err(deny("the plan branch moved; integrate again"));
        }
        let (name, commit, detail) = match outcome {
            Integration::Integrated { commit } => ("INTEGRATED", Some(commit.as_str()), ""),
            Integration::Conflict { detail } => ("CONFLICT", None, detail.as_str()),
        };
        let detail: String = detail.chars().take(1024).collect();
        let (p, r) = (id_to_text(plan.0), to_db(revision.0)?);
        let event = self.append_event(
            "PlanStepIntegrated",
            None,
            &format!("{p}|{}|{name}|{}", id_to_text(step), commit.unwrap_or("-")),
        )?;
        self.conn.execute(
            "INSERT INTO plan_integrations
             SELECT ?1,?2,?3,?4,coalesce(max(ordinal),0)+1,?5,?6,?7,?8,?9
             FROM plan_integrations WHERE plan=?1 AND revision=?2",
            &[
                Value::Text(&p),
                Value::Int(r),
                Value::Text(&id_to_text(step)),
                Value::Text(&id_to_text(attempt)),
                Value::Text(parent),
                Value::Text(name),
                commit.map_or(Value::Null, Value::Text),
                Value::Text(&detail),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        if commit.is_none() {
            return Ok("conflict".into());
        }
        let stored = crate::dependencies::load_plan(self.conn, plan, revision)?
            .ok_or_else(|| deny("plan run without its plan"))?;
        let wanted = format!("validation:{}", id_to_text(step));
        let mut seen = Vec::new();
        for e in &stored.plan.execution.edges {
            let jojo_domain::dependency::Predicate::Evidence { requirement, .. } = &e.predicate
            else {
                continue;
            };
            if e.prerequisite.id != step
                || requirement.evidence_id != wanted
                || requirement.kind != jojo_domain::dependency::EvidenceKind::ValidationPassed
                || seen.contains(&requirement.evidence_id)
            {
                continue;
            }
            seen.push(requirement.evidence_id.clone());
            let observation = jojo_domain::dependency::Observation {
                schema: jojo_domain::dependency::DEPENDENCY_SCHEMA,
                id: requirement.evidence_id.clone(),
                revision: requirement.observation_revision,
                kind: requirement.kind,
                binding: requirement.binding.clone(),
                status: jojo_domain::dependency::EvidenceStatus::Verified,
                observer: "jojo plan integration (host validation and human acceptance)".into(),
                observed: self.now,
                expires_at_unix_ms: self
                    .now
                    .unix_millis
                    .saturating_add(i64::try_from(requirement.max_age_ms).unwrap_or(i64::MAX)),
            };
            self.record_dependency_observation(None, &observation)?;
        }
        Ok("integrated".into())
    }

    fn candidate_accepted(&self, attempt: OpaqueId) -> Result<Option<i64>, TxError> {
        Ok(self.conn.query_optional(
            "SELECT receipt FROM candidate_acceptances WHERE attempt=?1",
            &[Value::Text(&id_to_text(attempt))],
            |r| r.int(0),
        )?)
    }
}

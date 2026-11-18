//! M3: the plan runner's durable state (schema 13) and the one view of a run that both the
//! helper's runner and the `PlanRun` query read.
//!
//! A run is an approved lead plan revision that the human started. Approving a plan does
//! not run it. The runner starts each ready step through the same path as a human Start,
//! so every check of a Start applies. M4 (D2): a step whose own work failed is repaired
//! at most twice, each time as a new attempt decided and recorded before it starts; a
//! step whose host validation passed is reviewed by the other provider; anything else,
//! and a failure past the limit, is a hold that waits for the human. Only the steps that
//! need it stop.
use crate::model::{from_db, id_from_text, id_to_text, to_db};
use crate::sqlite::{Connection, Value};
use crate::*;
use jojo_domain::dependency::NodeRef;
use jojo_domain::identity::{ArtifactId, OpaqueId, RepositoryId, Revision};
use jojo_domain::ipc::{identity, number, object, text};
use jojo_domain::json::Json;
use jojo_domain::planner::{self, LeadReply, TurnKind};
use jojo_domain::provider::ProviderKind;
use jojo_domain::repair::{Failure, MAX_REPAIRS};
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

/// M6: how the human starts a step that waits for them again. Each is a decision of its
/// own; an attempt started this way that fails is held again, never repaired.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryAction {
    /// A new attempt told what failed, like a repair.
    Repair,
    /// A fresh attempt from the plan branch as it is now.
    Restart,
}
impl RetryAction {
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "Repair" => Some(Self::Repair),
            "Restart" => Some(Self::Restart),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Repair => "Repair",
            Self::Restart => "Restart",
        }
    }
    fn stored(self) -> &'static str {
        match self {
            Self::Repair => "REPAIR",
            Self::Restart => "RESTART",
        }
    }
    fn from_stored(text: &str) -> Result<Self, StorageError> {
        match text {
            "REPAIR" => Ok(Self::Repair),
            "RESTART" => Ok(Self::Restart),
            other => Err(StorageError::corrupt(format!("retry action {other}"))),
        }
    }
    /// How the step's detail names it.
    fn words(self) -> &'static str {
        match self {
            Self::Repair => "the human's repair",
            Self::Restart => "the human's restart",
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
    /// M4: a repair attempt is decided or running.
    Repairing,
    /// Host validation is queued or running.
    Validating,
    /// M4: validation passed; the other provider's review is queued or running.
    Reviewing,
    /// Validation (and the review, when it is on) passed; the human accepts or rejects.
    Review,
    /// Accepted and not yet on the plan branch.
    Accepted,
    Integrated,
    /// The accepted candidate does not apply to the plan branch.
    Conflict,
    /// The attempt, its validation, its review or the candidate failed; the runner has
    /// not decided yet between a repair and a hold.
    Failed,
    /// M4: the step waits for the human; the detail says why.
    NeedsHuman,
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
            Self::Repairing => "Repairing",
            Self::Validating => "Validating",
            Self::Reviewing => "Reviewing",
            Self::Review => "Review",
            Self::Accepted => "Accepted",
            Self::Integrated => "Integrated",
            Self::Conflict => "Conflict",
            Self::Failed => "Failed",
            Self::NeedsHuman => "NeedsHuman",
            Self::Refused => "Refused",
            Self::Cancelled => "Cancelled",
        }
    }
    /// The step needs the human and holds back everything that depends on it.
    pub fn stopped(self) -> bool {
        matches!(self, Self::Conflict | Self::NeedsHuman | Self::Refused)
    }
}

/// M4: the review of a step's newest candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepReview {
    pub review: OpaqueId,
    pub provider: ProviderKind,
    /// `RUNNING` until it ends, then `APPROVED`, `REJECTED`, `FAILED`, `CANCELLED` or
    /// `INTERRUPTED`.
    pub state: String,
    pub notes: String,
    pub objection: Option<String>,
    pub detail: String,
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
    /// M4: repairs recorded for the step so far.
    pub repairs: u8,
    /// M4: the repair the newest attempt is, as "ordinal of limit".
    pub repair: Option<(u8, u8)>,
    /// M4: a repair that was decided and has not started: the attempt it starts as.
    pub pending_repair: Option<OpaqueId>,
    /// M4: why the newest attempt failed, while the step is `Failed`; M6: also while it is
    /// held for that failure.
    pub failure: Option<Failure>,
    /// M4: the review of the newest attempt's candidate, once one was started.
    pub review: Option<StepReview>,
    /// M6: the human's retry the newest attempt is, or will start as while it waits.
    pub retry: Option<RetryAction>,
}

impl RunStep {
    /// M6: what the human may do to start this step again. Nothing in a cancelled run; a
    /// repair needs a failure to tell the next attempt.
    pub fn retry_actions(&self, control: &str) -> Vec<RetryAction> {
        if control == "CANCELLED" {
            return vec![];
        }
        match self.state {
            StepState::NeedsHuman if self.failure.is_some() && self.attempt.is_some() => {
                vec![RetryAction::Repair, RetryAction::Restart]
            }
            StepState::NeedsHuman | StepState::Conflict | StepState::Refused => {
                vec![RetryAction::Restart]
            }
            _ => vec![],
        }
    }
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
    /// M4 (D2): the project's newest settings: repairs per step and the reviewer.
    pub repair_limit: u8,
    pub review_enabled: bool,
    pub steps: Vec<RunStep>,
}

/// The plan branch of a run: local, under `jojo/`, never pushed by the runner. The whole
/// identity: identities the helper makes share their first half.
pub fn plan_branch(plan: ArtifactId) -> String {
    format!("jojo/plan/{}", id_to_text(plan.0))
}

/// M4: attempts the `Repairs` answer lists at most.
const MAX_REPAIR_ROWS: usize = 64;

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
                        | StepState::Repairing
                        | StepState::Validating
                        | StepState::Reviewing
                        | StepState::Review
                        | StepState::Accepted
                        | StepState::Failed
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
            ("repair_limit", number(self.repair_limit.into())),
            ("review_enabled", Json::Bool(self.review_enabled)),
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
                                ("repairs", number(s.repairs.into())),
                                (
                                    "repair",
                                    s.repair.map_or(Json::Null, |(n, of)| {
                                        object([
                                            ("ordinal", number(n.into())),
                                            ("limit", number(of.into())),
                                        ])
                                    }),
                                ),
                                ("review", s.review.as_ref().map_or(Json::Null, review_json)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

fn review_json(r: &StepReview) -> Json {
    let cut = |s: &str, n: usize| text(s.chars().take(n).collect::<String>());
    object([
        ("review", identity(r.review)),
        (
            "provider",
            text(jojo_domain::project::provider_name(r.provider)),
        ),
        ("state", text(r.state.clone())),
        ("notes", cut(&r.notes, 2048)),
        (
            "objection",
            r.objection.as_deref().map_or(Json::Null, |o| cut(o, 512)),
        ),
        ("detail", cut(&r.detail, 512)),
    ])
}

/// M4 (D2): the repair limit and whether the reviewer is on, from the newest revision of
/// the project the repository belongs to; two repairs and the reviewer without one.
pub(crate) fn repair_policy(
    c: &Connection,
    repository: RepositoryId,
) -> Result<(u8, bool), StorageError> {
    Ok(crate::projects::of_repository(c, repository)?
        .map_or((MAX_REPAIRS, true), |(_, _, record)| {
            (record.repair_limit, record.review)
        }))
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

/// What is known about one step, from durable records only.
struct Facts {
    state: StepState,
    detail: String,
    attempt: Option<OpaqueId>,
    candidate: Option<String>,
    receipt: Option<u64>,
    commit: Option<String>,
    repairs: u8,
    repair: Option<(u8, u8)>,
    pending_repair: Option<OpaqueId>,
    failure: Option<Failure>,
    review: Option<StepReview>,
    retry: Option<RetryAction>,
}

impl Facts {
    fn new(state: StepState, detail: impl Into<String>) -> Self {
        Self {
            state,
            detail: detail.into(),
            attempt: None,
            candidate: None,
            receipt: None,
            commit: None,
            repairs: 0,
            repair: None,
            pending_repair: None,
            failure: None,
            review: None,
            retry: None,
        }
    }
}

/// M4: the review of one attempt's candidate, if one was started.
fn attempt_review(c: &Connection, attempt: &str) -> Result<Option<StepReview>, StorageError> {
    let row = c.query_optional(
        "SELECT r.review,r.provider,x.outcome,x.notes,x.objection,x.detail
         FROM step_reviews r LEFT JOIN step_review_results x ON x.review=r.review
         WHERE r.attempt=?1",
        &[Value::Text(attempt)],
        |r| {
            Ok((
                r.text(0)?,
                r.text(1)?,
                r.optional_text(2)?,
                r.optional_text(3)?,
                r.optional_text(4)?,
                r.optional_text(5)?,
            ))
        },
    )?;
    row.map(|(review, provider, outcome, notes, objection, detail)| {
        Ok(StepReview {
            review: id_from_text(&review)?,
            provider: jojo_domain::project::provider_named(&provider)
                .ok_or_else(|| StorageError::corrupt("review provider"))?,
            state: outcome.unwrap_or_else(|| "RUNNING".into()),
            notes: notes.unwrap_or_default(),
            objection,
            detail: detail.unwrap_or_default(),
        })
    })
    .transpose()
}

/// A step's state with its attempt, candidate, validation receipt, plan commit, repairs
/// and review.
fn step_state(
    v: &ReadView<'_>,
    plan: &str,
    revision: i64,
    step: NodeRef,
    cancelled: bool,
    review_on: bool,
) -> Result<Facts, StorageError> {
    let c = v.conn;
    let id = id_to_text(step.id);
    // M6: the human's newest retry of the step: the attempt it starts as, its action,
    // whether it started and why its start was refused. Once the human retried, only that
    // retry's refusal counts; the runner's own refusal came before it.
    let retry = c.query_optional(
        "SELECT r.attempt,r.action,
                EXISTS(SELECT 1 FROM scheduler_attempts s WHERE s.attempt=r.attempt),
                (SELECT x.detail FROM plan_step_retry_refusals x WHERE x.attempt=r.attempt)
         FROM plan_step_retries r WHERE plan=?1 AND revision=?2 AND step=?3
         ORDER BY event DESC LIMIT 1",
        &[Value::Text(plan), Value::Int(revision), Value::Text(&id)],
        |r| Ok((r.text(0)?, r.text(1)?, r.int(2)? == 1, r.optional_text(3)?)),
    )?;
    let refusal = match &retry {
        Some((_, _, _, refused)) => refused.clone(),
        None => c.query_optional(
            "SELECT detail FROM plan_step_refusals WHERE plan=?1 AND revision=?2 AND step=?3",
            &[Value::Text(plan), Value::Int(revision), Value::Text(&id)],
            |r| r.text(0),
        )?,
    };
    // M4: the repairs of the step, oldest first: ordinal, limit and the attempt it is.
    let repairs = c.query(
        "SELECT ordinal,repair_limit,attempt,
                EXISTS(SELECT 1 FROM scheduler_attempts s WHERE s.attempt=p.attempt)
         FROM plan_step_repairs p WHERE plan=?1 AND revision=?2 AND step=?3 ORDER BY ordinal",
        &[Value::Text(plan), Value::Int(revision), Value::Text(&id)],
        |r| Ok((r.int(0)?, r.int(1)?, r.text(2)?, r.int(3)? == 1)),
    )?;
    let count = u8::try_from(repairs.len()).map_err(|_| StorageError::corrupt("repairs"))?;
    let small = |n: i64| u8::try_from(n).map_err(|_| StorageError::corrupt("repair ordinal"));
    let attempt = c
        .query_optional(
            "SELECT attempt FROM scheduler_attempts WHERE step=?1 AND step_revision=?2
             ORDER BY rowid DESC LIMIT 1",
            &[Value::Text(&id), Value::Int(to_db(step.revision.0)?)],
            |r| r.text(0),
        )?
        .map(|a| id_from_text(&a))
        .transpose()?;
    let with = |mut f: Facts| {
        f.repairs = count;
        f.attempt = f.attempt.or(attempt);
        f
    };
    if let Some((pending, action, false, _)) = &retry {
        let action = RetryAction::from_stored(action)?;
        let mut f = match (&refusal, cancelled) {
            (Some(detail), _) => Facts::new(StepState::Refused, detail.clone()),
            (None, true) => Facts::new(StepState::Cancelled, ""),
            (None, false) => {
                let mut f = Facts::new(
                    StepState::Repairing,
                    format!("{}: waiting to start", action.words()),
                );
                f.pending_repair = Some(id_from_text(pending)?);
                f
            }
        };
        f.retry = Some(action);
        return Ok(with(f));
    }
    // A repair that waits to start; a retry of the human's, which always comes later,
    // replaces it.
    if let Some((ordinal, limit, pending, false)) = repairs.last().filter(|_| retry.is_none()) {
        let mut f = match (&refusal, cancelled) {
            (Some(detail), _) => Facts::new(StepState::Refused, detail.clone()),
            (None, true) => Facts::new(StepState::Cancelled, ""),
            (None, false) => {
                let mut f = Facts::new(
                    StepState::Repairing,
                    format!("repair {ordinal} of {limit}: waiting to start"),
                );
                f.pending_repair = Some(id_from_text(pending)?);
                f
            }
        };
        f.repair = Some((small(*ordinal)?, small(*limit)?));
        return Ok(with(f));
    }
    let Some(attempt) = attempt else {
        if let Some(detail) = refusal {
            return Ok(with(Facts::new(StepState::Refused, detail)));
        }
        if cancelled {
            return Ok(with(Facts::new(StepState::Cancelled, "")));
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
        return Ok(with(match question {
            Some(q) => Facts::new(StepState::Blocked, q),
            None => Facts::new(StepState::Ready, ""),
        }));
    };
    let text_id = id_to_text(attempt);
    let repair = repairs
        .iter()
        .find(|(_, _, a, _)| *a == text_id)
        .map(|(n, of, _, _)| Ok::<_, StorageError>((small(*n)?, small(*of)?)))
        .transpose()?;
    let hold = c.query_optional(
        "SELECT reason FROM plan_step_holds WHERE attempt=?1",
        &[Value::Text(&text_id)],
        |r| r.text(0),
    )?;
    let retried = match &retry {
        Some((a, action, _, _)) if *a == text_id => Some(RetryAction::from_stored(action)?),
        _ => None,
    };
    let base = |state: StepState, detail: String| {
        let mut f = Facts::new(state, detail);
        f.repair = repair;
        f.retry = retried;
        with(f)
    };
    // A failure is a hold once the runner recorded one, and pending until it decides. A held
    // step keeps its failure, so that the human can have it repaired (M6).
    let failed = |failure: Failure, detail: String| match &hold {
        Some(reason) => {
            let mut f = base(StepState::NeedsHuman, reason.clone());
            f.failure = Some(failure);
            f
        }
        None if cancelled && failure == Failure::Cancelled => base(StepState::Cancelled, detail),
        None => {
            let mut f = base(StepState::Failed, detail);
            f.failure = Some(failure);
            f
        }
    };
    let Some(result) = v.attempt_result(attempt)? else {
        // A Start that was committed but could not launch is refused, not running.
        return Ok(match (refusal, repair) {
            (Some(detail), _) => base(StepState::Refused, detail),
            (None, Some((n, of))) => {
                base(StepState::Repairing, format!("repair {n} of {of}: running"))
            }
            (None, None) => match retried {
                Some(RetryAction::Repair) => base(
                    StepState::Repairing,
                    format!("{}: running", RetryAction::Repair.words()),
                ),
                Some(RetryAction::Restart) => {
                    base(StepState::Running, RetryAction::Restart.words().to_owned())
                }
                None => base(StepState::Running, String::new()),
            },
        });
    };
    use jojo_domain::attempt::AttemptOutcome;
    if result.outcome != AttemptOutcome::Succeeded {
        let detail = format!("attempt {:?}: {}", result.outcome, result.reason);
        let failure = match result.outcome {
            AttemptOutcome::Cancelled => Failure::Cancelled,
            AttemptOutcome::Interrupted => Failure::Interrupted,
            _ => Failure::AttemptFailed {
                reason: result.reason.clone(),
            },
        };
        return Ok(failed(failure, detail));
    }
    let Some(candidate) = result.candidate.clone() else {
        return Ok(failed(
            Failure::NoChange,
            "the attempt changed nothing".into(),
        ));
    };
    let validation = v.validation_state(attempt)?;
    let receipt = match &validation {
        ValidationState::Passed(e) | ValidationState::Failed(e) => Some(e.0),
        _ => None,
    };
    let complete = |mut f: Facts| {
        f.candidate = Some(candidate.clone());
        f.receipt = f.receipt.or(receipt);
        f
    };
    let integration = c.query_optional(
        "SELECT outcome,commit_id,detail FROM plan_integrations WHERE attempt=?1",
        &[Value::Text(&text_id)],
        |r| Ok((r.text(0)?, r.optional_text(1)?, r.text(2)?)),
    )?;
    let review = attempt_review(c, &text_id)?;
    let reviewed = |mut f: Facts| {
        f.review = review.clone();
        complete(f)
    };
    match integration {
        Some((outcome, commit, _)) if outcome == "INTEGRATED" => {
            let mut f = base(StepState::Integrated, String::new());
            f.commit = commit;
            return Ok(reviewed(f));
        }
        Some((_, _, detail)) => return Ok(reviewed(base(StepState::Conflict, detail))),
        None => {}
    }
    if v.candidate_rejected(attempt)? {
        let f = failed(
            Failure::HumanRejected,
            "the human rejected the candidate".into(),
        );
        return Ok(reviewed(f));
    }
    if v.candidate_acceptance(attempt)?.is_some() {
        return Ok(reviewed(base(StepState::Accepted, String::new())));
    }
    Ok(reviewed(match validation {
        ValidationState::NotRun => base(StepState::Validating, "queued".into()),
        ValidationState::Unknown(why) => {
            let running = v
                .validation_runs(attempt)?
                .last()
                .is_some_and(|r| r.ended.is_none());
            if running {
                base(StepState::Validating, why)
            } else {
                let detail = format!("validation ended without a verdict: {why}");
                failed(Failure::ValidationUnknown { detail: why }, detail)
            }
        }
        ValidationState::Failed(e) => {
            let reasons = v
                .validation_receipts(attempt)?
                .into_iter()
                .find(|r| r.event == e)
                .map(|r| r.reasons)
                .unwrap_or_default();
            failed(
                Failure::ValidationFailed {
                    reasons,
                    output: String::new(),
                },
                "host validation failed".into(),
            )
        }
        ValidationState::Passed(_) => {
            let name = |p: ProviderKind| jojo_domain::project::provider_name(p);
            match &review {
                // A review that could not start holds the step.
                None if hold.is_some() => {
                    base(StepState::NeedsHuman, hold.clone().unwrap_or_default())
                }
                // A review that started still decides, even if the reviewer was switched
                // off since.
                None if !review_on => base(StepState::Review, String::new()),
                None => base(StepState::Reviewing, "queued".into()),
                Some(r) => match r.state.as_str() {
                    "RUNNING" => base(
                        StepState::Reviewing,
                        format!("{} reviews", name(r.provider)),
                    ),
                    "APPROVED" => base(
                        StepState::Review,
                        format!("approved by {}", name(r.provider)),
                    ),
                    "REJECTED" => failed(
                        Failure::ReviewRejected {
                            notes: r.notes.clone(),
                        },
                        format!("rejected by {}", name(r.provider)),
                    ),
                    other => failed(
                        Failure::ReviewUnknown {
                            detail: format!("{other}: {}", r.detail),
                        },
                        format!("the review ended without a verdict ({other})"),
                    ),
                },
            }
        }
    }))
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
    let repository_id = RepositoryId(id_from_text(&repository)?);
    let (repair_limit, review_enabled) = repair_policy(c, repository_id)?;
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
        let f = step_state(v, plan, revision, node, cancelled, review_enabled)?;
        let accepted = match f.attempt {
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
            state: f.state,
            detail: f.detail,
            attempt: f.attempt,
            candidate: f.candidate,
            receipt: f.receipt,
            commit: f.commit,
            accepted,
            repairs: f.repairs,
            repair: f.repair,
            pending_repair: f.pending_repair,
            failure: f.failure,
            review: f.review,
            retry: f.retry,
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
        repository: repository_id,
        base,
        branch,
        control,
        state,
        head,
        repair_limit,
        review_enabled,
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

/// M6: the longest note the human may give a retry, in characters.
pub const MAX_RETRY_NOTE_CHARS: usize = 4096;

/// M6: what the human asks for: start again a step that waits for them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RetryRequest {
    pub plan: ArtifactId,
    pub step: OpaqueId,
    /// The step's newest attempt as the human saw it; `None` for a step that never started.
    pub held: Option<OpaqueId>,
    pub action: RetryAction,
    /// The identity the new attempt starts under.
    pub attempt: OpaqueId,
    pub note: String,
    /// What the new attempt is told: the failure for a repair, then the human's note. Empty
    /// for a restart without a note.
    pub section: String,
}

/// M6: the retry an attempt starts as: the attempt it follows and what it is told.
pub(crate) fn retry_of(
    c: &Connection,
    attempt: OpaqueId,
) -> Result<Option<(Option<String>, String)>, StorageError> {
    Ok(c.query_optional(
        "SELECT held,section FROM plan_step_retries WHERE attempt=?1",
        &[Value::Text(&id_to_text(attempt))],
        |r| Ok((r.optional_text(0)?, r.text(1)?)),
    )?)
}

impl Tx<'_> {
    /// M6: records the human's retry of a step that waits for them, before it starts. The
    /// step must still be as the human saw it (the same newest attempt) and the action one
    /// it allows. The runner then starts it under the identity recorded here.
    pub fn retry_plan_step(&mut self, r: &RetryRequest) -> Result<String, TxError> {
        if r.note.chars().count() > MAX_RETRY_NOTE_CHARS {
            return Err(deny("NoteTooLong: a retry note is at most 4096 characters"));
        }
        let (revision, held) = {
            let view = ReadView { conn: self.conn };
            let Some(run) = view.plan_run(r.plan)? else {
                return Err(deny("UnknownPlanRun: this plan was never started"));
            };
            let Some(step) = run.steps.iter().find(|s| s.step.id == r.step) else {
                return Err(deny("UnknownStep: the plan run has no such step"));
            };
            if step.attempt != r.held {
                return Err(deny(
                    "StaleRetry: the step has another newest attempt than the one shown",
                ));
            }
            if !step.retry_actions(&run.control).contains(&r.action) {
                return Err(deny(format!(
                    "RetryRefused: a {} step of a {} run cannot be given a {}",
                    step.state.name(),
                    run.control,
                    r.action.name().to_lowercase()
                )));
            }
            (to_db(run.revision.0)?, step.attempt.map(id_to_text))
        };
        let (plan, step, attempt) = (
            id_to_text(r.plan.0),
            id_to_text(r.step),
            id_to_text(r.attempt),
        );
        let event = self.append_event(
            "PlanStepRetried",
            None,
            &format!(
                "{plan}|{step}|{}|{}|{attempt}",
                r.action.name(),
                held.as_deref().unwrap_or("")
            ),
        )?;
        self.conn.execute(
            "INSERT INTO plan_step_retries VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            &[
                Value::Text(&plan),
                Value::Int(revision),
                Value::Text(&step),
                held.as_deref().map_or(Value::Null, Value::Text),
                Value::Text(&attempt),
                Value::Text(r.action.stored()),
                Value::Text(&r.note),
                Value::Text(&r.section),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        // As for a repair: the settled step's queue row runs again under this decision.
        self.conn.execute(
            "UPDATE scheduler_queue SET control_revision=control_revision+1,intent='RUN',
             was_eligible=0 WHERE step=?1 AND intent='HOLD'",
            &[Value::Text(&step)],
        )?;
        Ok(format!("{} recorded", r.action.name().to_lowercase()))
    }

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
            // M4: a running review of the run is asked to stop as well.
            self.stop_plan_reviews(&plan_text, revision)?;
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
        // M6: once the human retried the step, the refusal is that retry's.
        let retried = self.conn.query_optional(
            "SELECT r.attempt FROM plan_step_retries r WHERE plan=?1 AND revision=?2 AND step=?3
             AND NOT EXISTS(SELECT 1 FROM plan_step_retry_refusals x WHERE x.attempt=r.attempt)
             ORDER BY event DESC LIMIT 1",
            &[
                Value::Text(&id_to_text(plan.0)),
                Value::Int(to_db(revision.0)?),
                Value::Text(&id_to_text(step)),
            ],
            |r| r.text(0),
        )?;
        if let Some(attempt) = retried {
            self.conn.execute(
                "INSERT INTO plan_step_retry_refusals VALUES (?1,?2,?3)",
                &[
                    Value::Text(&attempt),
                    Value::Text(&detail),
                    Value::Int(to_db(event.0)?),
                ],
            )?;
            return Ok(());
        }
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
    /// M4 (protocol 1.5): every attempt of a plan run with its repair, hold and review,
    /// newest run first; the newest attempt of a step also carries the step's state.
    pub fn ipc_repairs(&self) -> Result<Json, StorageError> {
        let c = self.conn;
        let mut rows = Vec::new();
        for (plan, revision) in runs(c, false)? {
            let run = view(self, &plan, revision)?;
            for step in &run.steps {
                let attempts = c.query(
                    "SELECT attempt FROM scheduler_attempts WHERE step=?1 ORDER BY rowid DESC",
                    &[Value::Text(&id_to_text(step.step.id))],
                    |r| r.text(0),
                )?;
                for (index, attempt) in attempts.iter().enumerate() {
                    let id = id_from_text(attempt)?;
                    let repair = crate::repairs::repair_of(c, id)?;
                    let hold = c.query_optional(
                        "SELECT reason FROM plan_step_holds WHERE attempt=?1",
                        &[Value::Text(attempt)],
                        |r| r.text(0),
                    )?;
                    let review = attempt_review(c, attempt)?;
                    let newest = index == 0 && step.pending_repair.is_none();
                    rows.push(object([
                        ("attempt", identity(id)),
                        ("plan", identity(run.plan.0)),
                        ("step", identity(step.step.id)),
                        ("key", text(step.key.clone())),
                        (
                            "provider",
                            text(jojo_domain::project::provider_name(step.provider)),
                        ),
                        (
                            "state",
                            if newest {
                                text(step.state.name())
                            } else {
                                Json::Null
                            },
                        ),
                        (
                            "repair",
                            repair.as_ref().map_or(Json::Null, |r| {
                                object([
                                    ("ordinal", number(r.ordinal.into())),
                                    ("limit", number(r.limit.into())),
                                    (
                                        "failure",
                                        text(r.failure.chars().take(512).collect::<String>()),
                                    ),
                                ])
                            }),
                        ),
                        (
                            "hold",
                            hold.map_or(Json::Null, |h| {
                                text(h.chars().take(512).collect::<String>())
                            }),
                        ),
                        ("review", review.as_ref().map_or(Json::Null, review_json)),
                    ]));
                    if rows.len() >= MAX_REPAIR_ROWS {
                        return Ok(object([
                            ("attempts", Json::List(rows)),
                            ("complete", Json::Bool(false)),
                        ]));
                    }
                }
            }
        }
        Ok(object([
            ("attempts", Json::List(rows)),
            ("complete", Json::Bool(true)),
        ]))
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

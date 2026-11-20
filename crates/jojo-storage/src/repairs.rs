//! M4: the durable side of repair and review (schema 15).
//!
//! A repair is decided and recorded before it starts: the failed attempt, the repair's
//! ordinal, the limit in force and the prompt section that tells the next attempt what
//! failed. The repair then starts through the same path as any Start, under the attempt
//! identity recorded here, which is the only way a step that already ran starts again.
//! A hold records that a step waits for the human, and why.
//!
//! A review is one read-only turn of the other provider on a step whose host validation
//! passed. Its prompt is composed by the helper from the approved documents and the
//! candidate's diff, stored with it and frozen by its digest; its verdict is stored as the
//! reviewer gave it. A verdict approves nothing.
use crate::digest::sha256_hex;
use crate::engine::{Engine, ReadView};
use crate::error::{StorageError, TxError};
use crate::launch::LaunchContext;
use crate::model::{from_db, id_from_text, id_to_text, to_db};
use crate::sqlite::{Connection, Value};
use crate::turns::{
    TURN_MAX_FRAME_BYTES, TURN_MAX_OUTPUT_BYTES, TURN_TIMEOUT_MILLIS, TurnConclusion, TurnRun,
    command, turn_directory,
};
use crate::tx::Tx;
use crate::{Admission, LocalRepository, ValidationState};
use jojo_domain::identity::{ArtifactId, OpaqueId, RepositoryId, Revision};
use jojo_domain::payload::Record;
use jojo_domain::planner::{self, LeadReply, TurnKind, TurnSpec};
use jojo_domain::project::{provider_name, provider_named};
use jojo_domain::provider::{MAX_PROMPT_BYTES, ProviderFingerprint, ProviderKind, TerminalStatus};
use jojo_domain::repair::MAX_REPAIRS;
use jojo_domain::review::Verdict;

fn deny(s: impl Into<String>) -> TxError {
    TxError::Rejected(s.into())
}

/// The other provider: the one that reviews a step `provider` did.
pub fn reviewer_of(provider: ProviderKind) -> ProviderKind {
    match provider {
        ProviderKind::Codex => ProviderKind::ClaudeCode,
        ProviderKind::ClaudeCode => ProviderKind::Codex,
    }
}

/// A repair as recorded: what the attempt it names is told.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepairRecord {
    pub ordinal: u8,
    pub limit: u8,
    pub failed: OpaqueId,
    pub attempt: OpaqueId,
    pub failure: String,
    pub section: String,
}

/// What the runner asks the core to record as a step's next repair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepairRequest {
    pub plan: ArtifactId,
    pub revision: Revision,
    pub step: OpaqueId,
    /// The step's newest attempt, which failed.
    pub failed: OpaqueId,
    /// The identity the repair attempt starts under.
    pub attempt: OpaqueId,
    pub ordinal: u8,
    pub limit: u8,
    /// One line: why the failed attempt is repaired.
    pub failure: String,
    pub section: String,
}

/// What the runner asks the core to accept as a review turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewRequest {
    pub review: OpaqueId,
    pub plan: ArtifactId,
    pub revision: Revision,
    pub step: OpaqueId,
    pub attempt: OpaqueId,
    pub provider: ProviderKind,
    pub repository: RepositoryId,
    /// The commit the repository's HEAD named when the review was accepted.
    pub base: String,
    pub prompt: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReviewOutcome {
    Approved,
    Rejected,
    Failed,
    Cancelled,
    Interrupted,
}

impl ReviewOutcome {
    pub fn name(self) -> &'static str {
        match self {
            Self::Approved => "APPROVED",
            Self::Rejected => "REJECTED",
            Self::Failed => "FAILED",
            Self::Cancelled => "CANCELLED",
            Self::Interrupted => "INTERRUPTED",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewRecord {
    pub review: OpaqueId,
    /// The attempt reviewed; `None` for a document's advisory review.
    pub attempt: Option<OpaqueId>,
    pub provider: ProviderKind,
    pub repository: RepositoryId,
    pub base: String,
    pub prompt_digest: String,
    pub stop_requested: bool,
    /// `None` while it runs.
    pub outcome: Option<String>,
}

/// Follow-ups: where a review is kept. A plan step's (M4) and an advisory one (an attempt
/// started by hand, or a drafted document) run the same way and are sealed the same way;
/// only a plan step's verdict moves anything.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReviewTables {
    pub(crate) reviews: &'static str,
    results: &'static str,
    stops: &'static str,
    pub(crate) executables: &'static str,
    finished: &'static str,
    stop_requested: &'static str,
}

pub(crate) const STEP_REVIEWS: ReviewTables = ReviewTables {
    reviews: "step_reviews",
    results: "step_review_results",
    stops: "step_review_stops",
    executables: "review_executables",
    finished: "StepReviewFinished",
    stop_requested: "StepReviewStopRequested",
};

pub(crate) const ADVISORY_REVIEWS: ReviewTables = ReviewTables {
    reviews: "advisory_reviews",
    results: "advisory_review_results",
    stops: "advisory_review_stops",
    executables: "advisory_review_executables",
    finished: "AdvisoryReviewFinished",
    stop_requested: "AdvisoryReviewStopRequested",
};

/// The tables that hold review `id`, if any does.
pub(crate) fn tables_of(c: &Connection, id: &str) -> Result<Option<ReviewTables>, StorageError> {
    for tables in [STEP_REVIEWS, ADVISORY_REVIEWS] {
        let found = c.query_optional(
            &format!("SELECT 1 FROM {} WHERE review=?1", tables.reviews),
            &[Value::Text(id)],
            |r| r.int(0),
        )?;
        if found.is_some() {
            return Ok(Some(tables));
        }
    }
    Ok(None)
}

fn review(c: &Connection, id: OpaqueId) -> Result<Option<ReviewRecord>, StorageError> {
    let id = id_to_text(id);
    let Some(t) = tables_of(c, &id)? else {
        return Ok(None);
    };
    let row = c.query_optional(
        &format!(
            "SELECT r.review,r.attempt,r.provider,r.repository,r.base,r.prompt_digest,
                EXISTS(SELECT 1 FROM {stops} s WHERE s.review=r.review),
                (SELECT outcome FROM {results} x WHERE x.review=r.review)
             FROM {reviews} r WHERE r.review=?1",
            stops = t.stops,
            results = t.results,
            reviews = t.reviews
        ),
        &[Value::Text(&id)],
        |r| {
            Ok((
                r.text(0)?,
                r.optional_text(1)?,
                r.text(2)?,
                r.text(3)?,
                r.text(4)?,
                r.text(5)?,
                r.int(6)? == 1,
                r.optional_text(7)?,
            ))
        },
    )?;
    row.map(
        |(review, attempt, provider, repository, base, digest, stop, outcome)| {
            Ok(ReviewRecord {
                review: id_from_text(&review)?,
                attempt: attempt.as_deref().map(id_from_text).transpose()?,
                provider: provider_named(&provider)
                    .ok_or_else(|| StorageError::corrupt("review provider"))?,
                repository: RepositoryId(id_from_text(&repository)?),
                base,
                prompt_digest: digest,
                stop_requested: stop,
                outcome,
            })
        },
    )
    .transpose()
}

/// A review of either kind that has no result yet. One runs at a time.
pub(crate) fn unfinished_review(c: &Connection) -> Result<Option<String>, StorageError> {
    unfinished(c)
}

fn unfinished(c: &Connection) -> Result<Option<String>, StorageError> {
    for t in [STEP_REVIEWS, ADVISORY_REVIEWS] {
        let found = c.query_optional(
            &format!(
                "SELECT r.review FROM {} r
                 WHERE NOT EXISTS(SELECT 1 FROM {} x WHERE x.review=r.review) LIMIT 1",
                t.reviews, t.results
            ),
            &[],
            |r| r.text(0),
        )?;
        if found.is_some() {
            return Ok(found);
        }
    }
    Ok(None)
}

/// The newest scheduler attempt of a step, and whether it is sealed.
fn newest_attempt(c: &Connection, step: &str) -> Result<Option<(String, bool)>, StorageError> {
    Ok(c.query_optional(
        "SELECT attempt,sealed FROM scheduler_attempts WHERE step=?1 ORDER BY rowid DESC LIMIT 1",
        &[Value::Text(step)],
        |r| Ok((r.text(0)?, r.int(1)? == 1)),
    )?)
}

fn run_exists(c: &Connection, plan: &str, revision: i64) -> Result<bool, StorageError> {
    Ok(c.query_optional(
        "SELECT 1 FROM plan_runs WHERE plan=?1 AND revision=?2",
        &[Value::Text(plan), Value::Int(revision)],
        |r| r.int(0),
    )?
    .is_some())
}

/// M4: the repair an attempt is, if it is one.
pub(crate) fn repair_of(
    c: &Connection,
    attempt: OpaqueId,
) -> Result<Option<RepairRecord>, StorageError> {
    let row = c.query_optional(
        "SELECT ordinal,repair_limit,failed,attempt,failure,section FROM plan_step_repairs
         WHERE attempt=?1",
        &[Value::Text(&id_to_text(attempt))],
        |r| {
            Ok((
                r.int(0)?,
                r.int(1)?,
                r.text(2)?,
                r.text(3)?,
                r.text(4)?,
                r.text(5)?,
            ))
        },
    )?;
    let small = |n: i64| u8::try_from(n).map_err(|_| StorageError::corrupt("repair ordinal"));
    row.map(|(ordinal, limit, failed, attempt, failure, section)| {
        Ok(RepairRecord {
            ordinal: small(ordinal)?,
            limit: small(limit)?,
            failed: id_from_text(&failed)?,
            attempt: id_from_text(&attempt)?,
            failure,
            section,
        })
    })
    .transpose()
}

/// M4: unfinished reviews take one unit of the global capacity each, like a lead turn.
pub(crate) fn active_reviews(c: &Connection) -> Result<u64, StorageError> {
    from_db(c.query_one(
        "SELECT (SELECT count(*) FROM step_reviews r
                 WHERE NOT EXISTS(SELECT 1 FROM step_review_results x WHERE x.review=r.review))
              + (SELECT count(*) FROM advisory_reviews r
                 WHERE NOT EXISTS(SELECT 1 FROM advisory_review_results x WHERE x.review=r.review))",
        &[],
        |r| r.int(0),
    )?)
}

impl Tx<'_> {
    /// Records the next repair of a step whose newest attempt failed. The ordinal must be
    /// the next one, within the limit and never above two (D2); a held attempt is not
    /// repaired; an attempt is repaired at most once.
    pub fn record_repair(&mut self, r: &RepairRequest) -> Result<String, TxError> {
        let (plan, revision) = (id_to_text(r.plan.0), to_db(r.revision.0)?);
        if !run_exists(self.conn, &plan, revision)? {
            return Err(deny("UnknownPlanRun: this plan was never started"));
        }
        let step = id_to_text(r.step);
        let failed = id_to_text(r.failed);
        match newest_attempt(self.conn, &step)? {
            Some((newest, true)) if newest == failed => {}
            _ => {
                return Err(deny(
                    "RepairRefused: the failed attempt is not the step's newest sealed attempt",
                ));
            }
        }
        let held = self.conn.query_optional(
            "SELECT 1 FROM plan_step_holds WHERE attempt=?1",
            &[Value::Text(&failed)],
            |row| row.int(0),
        )?;
        if held.is_some() {
            return Err(deny("RepairRefused: the attempt waits for the human"));
        }
        let done: i64 = self.conn.query_one(
            "SELECT count(*) FROM plan_step_repairs WHERE plan=?1 AND revision=?2 AND step=?3",
            &[Value::Text(&plan), Value::Int(revision), Value::Text(&step)],
            |row| row.int(0),
        )?;
        if i64::from(r.ordinal) != done + 1 || r.ordinal > r.limit || r.limit > MAX_REPAIRS {
            return Err(deny(format!(
                "RepairRefused: repair {} of {} after {done}",
                r.ordinal, r.limit
            )));
        }
        let failure: String = r.failure.chars().take(1024).collect();
        let event = self.append_event(
            "PlanStepRepairDecided",
            None,
            &Record::new()
                .text("plan", plan.clone())
                .text("step", step.clone())
                .int("ordinal", i64::from(r.ordinal))
                .int("limit", i64::from(r.limit))
                .text("failed", failed.clone())
                .id("attempt", r.attempt),
        )?;
        self.conn.execute(
            "INSERT INTO plan_step_repairs VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            &[
                Value::Text(&plan),
                Value::Int(revision),
                Value::Text(&step),
                Value::Int(i64::from(r.ordinal)),
                Value::Int(i64::from(r.limit)),
                Value::Text(&failed),
                Value::Text(&id_to_text(r.attempt)),
                Value::Text(&failure),
                Value::Text(&r.section),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        // A settled step is held in the queue: a retry is a new decision, and this record is
        // that decision. A cancelled step stays cancelled.
        self.conn.execute(
            "UPDATE scheduler_queue SET control_revision=control_revision+1,intent='RUN',
             was_eligible=0 WHERE step=?1 AND intent='HOLD'",
            &[Value::Text(&step)],
        )?;
        Ok(format!("repair {} of {}", r.ordinal, r.limit))
    }

    /// Records that a step waits for the human because of its newest attempt. Once.
    pub fn hold_plan_step(
        &mut self,
        plan: ArtifactId,
        revision: Revision,
        step: OpaqueId,
        attempt: OpaqueId,
        reason: &str,
    ) -> Result<String, TxError> {
        let (plan, revision) = (id_to_text(plan.0), to_db(revision.0)?);
        if !run_exists(self.conn, &plan, revision)? {
            return Err(deny("UnknownPlanRun: this plan was never started"));
        }
        let (step, attempt) = (id_to_text(step), id_to_text(attempt));
        if newest_attempt(self.conn, &step)?.is_none_or(|(a, _)| a != attempt) {
            return Err(deny("HoldRefused: not the step's newest attempt"));
        }
        let held = self.conn.query_optional(
            "SELECT 1 FROM plan_step_holds WHERE attempt=?1",
            &[Value::Text(&attempt)],
            |row| row.int(0),
        )?;
        if held.is_some() {
            return Ok("already held".into());
        }
        let reason: String = reason.chars().take(1024).collect();
        let event = self.append_event(
            "PlanStepHeld",
            None,
            &Record::new()
                .text("plan", plan.clone())
                .text("step", step.clone())
                .text("attempt", attempt.clone())
                .text("reason", reason.clone()),
        )?;
        self.conn.execute(
            "INSERT INTO plan_step_holds VALUES (?1,?2,?3,?4,?5,?6)",
            &[
                Value::Text(&plan),
                Value::Int(revision),
                Value::Text(&step),
                Value::Text(&attempt),
                Value::Text(&reason),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok("held".into())
    }

    /// The durable acceptance of one review. Re-reads admission and capacity; the
    /// candidate's host validation must have passed, no other review may run and the
    /// reviewer must be the provider that did not do the step.
    pub fn accept_review(&mut self, r: &ReviewRequest) -> Result<String, TxError> {
        if self.admission()? != Admission::Open {
            return Err(deny("AdmissionClosed"));
        }
        if unfinished(self.conn)?.is_some() {
            return Err(deny("ReviewRefused: another review is running"));
        }
        self.capacity_left("a review")?;
        let (plan, revision) = (id_to_text(r.plan.0), to_db(r.revision.0)?);
        if !run_exists(self.conn, &plan, revision)? {
            return Err(deny("UnknownPlanRun: this plan was never started"));
        }
        let attempt = id_to_text(r.attempt);
        if !matches!(
            crate::validation::state(self.conn, &attempt)?,
            ValidationState::Passed(_)
        ) {
            return Err(deny("ReviewRefused: the host validation has not passed"));
        }
        let developer = self.conn.query_optional(
            "SELECT provider FROM process_launches WHERE attempt=?1",
            &[Value::Text(&attempt)],
            |row| row.text(0),
        )?;
        if developer.as_deref().and_then(provider_kind) == Some(r.provider) {
            return Err(deny(
                "ReviewRefused: a step is reviewed by the other provider",
            ));
        }
        if r.prompt.is_empty() || r.prompt.len() > MAX_PROMPT_BYTES || r.prompt.contains('\0') {
            return Err(deny(
                "PromptTooLarge: the review prompt is empty or too large",
            ));
        }
        let review = id_to_text(r.review);
        let event = self.append_event(
            "StepReviewAccepted",
            None,
            &Record::new()
                .text("review", review.clone())
                .text("plan", plan.clone())
                .id("step", r.step)
                .text("attempt", attempt.clone())
                .text("provider", provider_name(r.provider)),
        )?;
        self.conn.execute(
            "INSERT INTO step_reviews VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            &[
                Value::Text(&review),
                Value::Text(&plan),
                Value::Int(revision),
                Value::Text(&id_to_text(r.step)),
                Value::Text(&attempt),
                Value::Text(provider_name(r.provider)),
                Value::Text(&id_to_text(r.repository.0)),
                Value::Text(&r.base),
                Value::Text(&r.prompt),
                Value::Text(&sha256_hex(&[r.prompt.as_bytes()])),
                Value::Int(to_db(self.epoch.0)?),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(format!("accepted review={review}"))
    }

    pub fn request_review_stop(&mut self, id: OpaqueId) -> Result<(), TxError> {
        let Some(record) = review(self.conn, id)? else {
            return Err(deny("UnknownReview"));
        };
        if record.outcome.is_some() {
            return Err(deny("ReviewFinished"));
        }
        if !record.stop_requested {
            let t = tables_of(self.conn, &id_to_text(id))?.ok_or_else(|| deny("UnknownReview"))?;
            let event =
                self.append_event(t.stop_requested, None, &Record::new().id("review", id))?;
            self.conn.execute(
                &format!("INSERT INTO {} VALUES (?1,?2)", t.stops),
                &[Value::Text(&id_to_text(id)), Value::Int(to_db(event.0)?)],
            )?;
        }
        Ok(())
    }

    /// Seals a review once. A verdict is stored with the reply it came from.
    pub fn finish_review(
        &mut self,
        id: OpaqueId,
        outcome: ReviewOutcome,
        detail: &str,
        verdict: Option<(&str, &Verdict)>,
    ) -> Result<String, TxError> {
        let Some(record) = review(self.conn, id)? else {
            return Err(deny("UnknownReview"));
        };
        if record.outcome.is_some() {
            return Err(deny("ReviewFinished"));
        }
        let outcome = match (outcome, verdict) {
            (ReviewOutcome::Approved | ReviewOutcome::Rejected, None) => {
                return Err(deny("a verdict outcome needs the verdict"));
            }
            (_, Some((_, v))) if v.approve => ReviewOutcome::Approved,
            (_, Some(_)) => ReviewOutcome::Rejected,
            (other, None) => other,
        };
        let detail: String = detail.chars().take(1024).collect();
        let t = tables_of(self.conn, &id_to_text(id))?.ok_or_else(|| deny("UnknownReview"))?;
        let event = self.append_event(
            t.finished,
            None,
            &Record::new()
                .id("review", id)
                .text("outcome", outcome.name()),
        )?;
        let (notes, objection, reply) = match verdict {
            Some((raw, v)) => (v.notes.as_str(), v.objection.as_deref(), Some(raw)),
            None => ("", None, None),
        };
        self.conn.execute(
            &format!("INSERT INTO {} VALUES (?1,?2,?3,?4,?5,?6,?7)", t.results),
            &[
                Value::Text(&id_to_text(id)),
                Value::Text(outcome.name()),
                Value::Text(notes),
                objection.map_or(Value::Null, Value::Text),
                Value::Text(&detail),
                reply.map_or(Value::Null, Value::Text),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(outcome.name().to_owned())
    }

    /// A cancelled run asks its running review to stop.
    pub(crate) fn stop_plan_reviews(&mut self, plan: &str, revision: i64) -> Result<(), TxError> {
        let running = self.conn.query(
            "SELECT r.review FROM step_reviews r WHERE r.plan=?1 AND r.revision=?2
             AND NOT EXISTS(SELECT 1 FROM step_review_results x WHERE x.review=r.review)",
            &[Value::Text(plan), Value::Int(revision)],
            |row| row.text(0),
        )?;
        for id in running {
            self.request_review_stop(id_from_text(&id)?)?;
        }
        Ok(())
    }

    /// A review whose helper ended first is closed as interrupted by the next helper.
    pub fn close_interrupted_reviews(&mut self) -> Result<u64, TxError> {
        let mut closed = 0;
        while let Some(id) = unfinished(self.conn)? {
            self.finish_review(
                id_from_text(&id)?,
                ReviewOutcome::Interrupted,
                "the helper ended before the review did",
                None,
            )?;
            closed += 1;
        }
        Ok(closed)
    }
}

/// A launch records its provider as `Codex` or `ClaudeCode`.
fn provider_kind(text: &str) -> Option<ProviderKind> {
    provider_named(text)
}

/// What a review prompt is made of, read from the approved documents of a run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewMaterial {
    pub key: String,
    pub title: String,
    pub prd_revision: u64,
    pub prd: String,
    pub adr: String,
    pub write_scope: Vec<String>,
    pub baseline: Vec<String>,
    pub command: Vec<String>,
}

impl ReadView<'_> {
    /// The approved documents and the step as the developer was given them.
    pub fn review_material(
        &self,
        subjects: &crate::SliceSubjects,
        step: jojo_domain::dependency::NodeRef,
    ) -> Result<Option<ReviewMaterial>, StorageError> {
        let body = |id: ArtifactId| {
            self.conn.query_optional(
                "SELECT content FROM review_documents WHERE id=?1 ORDER BY revision DESC LIMIT 1",
                &[Value::Text(&id_to_text(id.0))],
                |r| r.text(0),
            )
        };
        let names = self.conn.query_optional(
            "SELECT key,title FROM lead_plan_steps WHERE step=?1",
            &[Value::Text(&id_to_text(step.id))],
            |r| Ok((r.text(0)?, r.text(1)?)),
        )?;
        let (Some((key, title)), Some(prd), Some(adr), Some(validation)) = (
            names,
            self.artifact_version(subjects.prd, None)?,
            body(subjects.adr)?,
            body(subjects.validation)?,
        ) else {
            return Ok(None);
        };
        let plan = jojo_domain::validation::ValidationPlan::import(validation.as_bytes())
            .map_err(StorageError::corrupt)?;
        let Some(checks) = plan.step(step) else {
            return Ok(None);
        };
        let paths = |p: &[jojo_domain::validation::ScopePath]| {
            p.iter().map(|p| p.as_str().to_owned()).collect()
        };
        Ok(Some(ReviewMaterial {
            key,
            title,
            prd_revision: prd.revision.0,
            prd: prd.rendered,
            adr,
            write_scope: paths(&checks.write_scope),
            baseline: paths(&checks.baseline),
            command: checks.command.clone(),
        }))
    }
    pub fn review(&self, id: OpaqueId) -> Result<Option<ReviewRecord>, StorageError> {
        review(self.conn, id)
    }
    pub fn review_prompt(&self, id: OpaqueId) -> Result<Option<String>, StorageError> {
        let id = id_to_text(id);
        let Some(t) = tables_of(self.conn, &id)? else {
            return Ok(None);
        };
        Ok(self.conn.query_optional(
            &format!("SELECT prompt FROM {} WHERE review=?1", t.reviews),
            &[Value::Text(&id)],
            |r| r.text(0),
        )?)
    }
    pub fn unfinished_review(&self) -> Result<Option<OpaqueId>, StorageError> {
        unfinished(self.conn)?
            .map(|id| id_from_text(&id))
            .transpose()
    }
    /// M6: what a retry of the human's tells its attempt; `None` when nothing.
    pub fn retry_section(&self, attempt: OpaqueId) -> Result<Option<String>, StorageError> {
        Ok(crate::runner::retry_of(self.conn, attempt)?
            .map(|(_, section)| section)
            .filter(|s| !s.is_empty()))
    }
    pub fn repair_of(&self, attempt: OpaqueId) -> Result<Option<RepairRecord>, StorageError> {
        repair_of(self.conn, attempt)
    }
}

/// The frozen request of an accepted review, like a lead turn's: the stored prompt, run
/// read-only in the registered repository.
pub fn review_spec(
    engine: &Engine,
    id: OpaqueId,
    repository: &LocalRepository,
    fingerprint: &ProviderFingerprint,
    variables: Vec<String>,
) -> Result<TurnSpec, String> {
    let mut reader = engine.reader().map_err(|e| e.to_string())?;
    let (record, prompt) = reader
        .read(|v| Ok((v.review(id)?, v.review_prompt(id)?)))
        .map_err(|e| e.to_string())?;
    let (Some(record), Some(prompt)) = (record, prompt) else {
        return Err("accepted review is missing".into());
    };
    if record.repository != repository.id() || record.provider != fingerprint.provider {
        return Err("review repository or provider differs from the accepted review".into());
    }
    if sha256_hex(&[prompt.as_bytes()]) != record.prompt_digest {
        return Err("stored prompt differs from its digest".into());
    }
    Ok(TurnSpec {
        turn: id,
        kind: TurnKind::Review,
        fingerprint: fingerprint.clone(),
        prompt: prompt.into_bytes(),
        prompt_digest: record.prompt_digest,
        repository: repository.path().to_owned(),
        base_commit: record.base,
        raw_output_dir: turn_directory(engine, id)?,
        environment_allowlist: variables,
        timeout_millis: TURN_TIMEOUT_MILLIS,
        max_output_bytes: TURN_MAX_OUTPUT_BYTES,
        max_frame_bytes: TURN_MAX_FRAME_BYTES,
        resume: None,
    })
}

/// Seals a concluded review run. Only a clean run whose last message is a verdict
/// decides anything; a process that could not be confirmed stopped also closes admission.
pub fn settle_review(
    ctx: &LaunchContext<'_>,
    spec: &TurnSpec,
    run: &TurnRun,
) -> Result<ReviewOutcome, String> {
    let clean = run.conclusion == TurnConclusion::TreeAbsent
        && run.stop.is_none()
        && !run.protocol_failed
        && !run.evidence_lost
        && run.terminal == Some(TerminalStatus::Success)
        && run.exit.is_some_and(|e| e.code == Some(0));
    let (outcome, detail, verdict) = match (&run.conclusion, clean) {
        (TurnConclusion::TreeAbsent, true) => match run.reply.as_deref() {
            None => (
                ReviewOutcome::Failed,
                "the run ended without a message".to_owned(),
                None,
            ),
            Some(text) => match planner::parse_reply(TurnKind::Review, text) {
                Ok(LeadReply::Review(v)) => (
                    ReviewOutcome::Approved,
                    String::new(),
                    Some((text.to_owned(), v)),
                ),
                Ok(_) => (ReviewOutcome::Failed, "not a verdict".to_owned(), None),
                Err(error) => (
                    ReviewOutcome::Failed,
                    format!("the last message is not a verdict: {error:?}"),
                    None,
                ),
            },
        },
        (_, _) if run.cancelled => (
            ReviewOutcome::Cancelled,
            "stopped by the human".to_owned(),
            None,
        ),
        (TurnConclusion::NotSpawned(why), _) => {
            (ReviewOutcome::Failed, format!("not spawned: {why}"), None)
        }
        (TurnConclusion::StopUnconfirmed(reasons), _) => (
            ReviewOutcome::Failed,
            format!("stop unconfirmed: {}", reasons.join("; ")),
            None,
        ),
        (TurnConclusion::TreeAbsent, false) => (
            ReviewOutcome::Failed,
            run.stop.clone().unwrap_or_else(|| {
                format!(
                    "the run did not end in success: terminal={:?} exit={:?}",
                    run.terminal, run.exit
                )
            }),
            None,
        ),
    };
    let unconfirmed = matches!(run.conclusion, TurnConclusion::StopUnconfirmed(_));
    let id = spec.turn;
    let sealed = command(ctx, "FinishStepReview", id, move |tx| {
        if unconfirmed {
            tx.set_admission(Admission::Closed)?;
        }
        tx.finish_review(
            id,
            outcome,
            &detail,
            verdict.as_ref().map(|(raw, v)| (raw.as_str(), v)),
        )
    })?;
    match sealed.as_str() {
        "APPROVED" => Ok(ReviewOutcome::Approved),
        "REJECTED" => Ok(ReviewOutcome::Rejected),
        _ => Ok(outcome),
    }
}

//! What the Dashboard shows (protocol 1.7): each feature's place in its lifecycle and what
//! blocks it, a sentence for each event, and for each attempt its place among its step's
//! attempts, its repository and project, its host validation and its newest provider
//! record. Read-only and derived from what the core already records; nothing is stored.
use crate::model::{id_from_text, to_db};
use crate::overview::{
    approval_state, cut, document_feature, feature_name, identity_text, optional, time_of,
};
use crate::sqlite::{Connection, Value};
use crate::validation::{ValidationState, rejected, state};
use crate::{ReadView, StorageError};
use jojo_domain::identity::{ArtifactId, RepositoryId};
use jojo_domain::ipc::{number, object, text};
use jojo_domain::json::Json;

/// Features per page of `Features`.
pub const FEATURE_ROWS: usize = 16;
/// Blockers listed per feature.
const FEATURE_BLOCKERS: usize = 8;

/// One lifecycle stage of a feature: a state the app draws as a dot, and a word.
struct Stage {
    /// `Done`, `Current`, `Running`, `NeedsYou`, `Blocked`, `Failed`, `Stale`, `Unknown`
    /// or `Off` (not reached).
    state: &'static str,
    label: Option<String>,
}

impl Stage {
    fn off() -> Self {
        Self {
            state: "Off",
            label: None,
        }
    }
    fn new(state: &'static str, label: impl Into<String>) -> Self {
        Self {
            state,
            label: Some(label.into()),
        }
    }
    fn done() -> Self {
        Self::new("Done", "Done")
    }
    fn json(&self) -> Json {
        object([
            ("state", text(self.state)),
            ("label", optional(self.label.clone())),
        ])
    }
}

/// "n word", or the word alone for one.
fn counted(n: usize, word: &str) -> String {
    if n == 1 {
        word.to_owned()
    } else {
        format!("{n} {word}")
    }
}

/// The last component of a repository's path.
pub(crate) fn repository_name(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    trimmed.rsplit('/').next().unwrap_or(trimmed).to_owned()
}

/// A repository and the project it belongs to, by identity and name; `null` when unknown.
fn place(
    c: &Connection,
    repository: Option<String>,
) -> Result<[(&'static str, Json); 4], StorageError> {
    let Some(repository) = repository else {
        return Ok([
            ("repository", Json::Null),
            ("repository_name", Json::Null),
            ("project", Json::Null),
            ("project_name", Json::Null),
        ]);
    };
    let path = c.query_optional(
        "SELECT path FROM local_repositories WHERE id=?1",
        &[Value::Text(&repository)],
        |r| r.text(0),
    )?;
    let project = crate::projects::of_repository(c, RepositoryId(id_from_text(&repository)?))?;
    Ok([
        ("repository", identity_text(&repository)?),
        (
            "repository_name",
            optional(path.as_deref().map(repository_name)),
        ),
        (
            "project",
            project
                .as_ref()
                .map_or(Json::Null, |(id, _, _)| jojo_domain::ipc::identity(*id)),
        ),
        (
            "project_name",
            optional(project.map(|(_, _, p)| cut(&p.name))),
        ),
    ])
}

/// The repository a feature works in: its chat's, else its newest plan run's.
fn feature_repository(c: &Connection, feature: &str) -> Result<Option<String>, StorageError> {
    Ok(c.query_optional(
        "SELECT coalesce(
           (SELECT repository FROM lead_turns WHERE feature=?1 ORDER BY ordinal LIMIT 1),
           (SELECT r.repository FROM plan_runs r JOIN artifact_versions v ON v.artifact_id=r.prd
             WHERE v.feature_id=?1 ORDER BY r.event DESC LIMIT 1))",
        &[Value::Text(feature)],
        |r| r.optional_text(0),
    )?
    .flatten())
}

/// The repository an attempt worked in: its feature's, else its plan run's.
fn attempt_repository(
    c: &Connection,
    plan: Option<&str>,
    feature: Option<&str>,
) -> Result<Option<String>, StorageError> {
    if let Some(repository) = feature
        .map(|f| feature_repository(c, f))
        .transpose()?
        .flatten()
    {
        return Ok(Some(repository));
    }
    let Some(plan) = plan else { return Ok(None) };
    Ok(c.query_optional(
        "SELECT repository FROM plan_runs WHERE plan=?1 ORDER BY revision DESC LIMIT 1",
        &[Value::Text(plan)],
        |r| r.text(0),
    )?)
}

/// An attempt's place among its step's attempts, from 1.
fn attempt_ordinal(c: &Connection, attempt: &str) -> Result<i64, StorageError> {
    Ok(c.query_one(
        "SELECT count(*) FROM scheduler_attempts a JOIN scheduler_attempts b ON b.step=a.step
         WHERE a.attempt=?1 AND b.rowid<=a.rowid",
        &[Value::Text(attempt)],
        |r| r.int(0),
    )?)
}

/// What a provider record says the agent did, in a few words.
fn activity_text(kind: &str) -> String {
    match kind {
        "ProcessStarted" => "Process started",
        "SessionIdentified" => "Session opened",
        "TurnStarted" => "Turn started",
        "TextDelta" => "Writing",
        "ToolStarted" => "Using a tool",
        "ToolFinished" => "Tool finished",
        "QuestionRequested" => "Asked a question",
        "PermissionRequested" => "Asked for permission",
        "UsageObserved" => "Reported usage",
        "ProviderTerminal" => "Finished its turn",
        "ProviderError" => "Reported an error",
        "InterruptAcknowledged" => "Acknowledged the stop",
        "ProcessExited" => "Exited",
        "StreamTruncated" => "Output cut short",
        "ProtocolViolation" => "Left the stream profile",
        "Informational" => "Reported status",
        other => return other.to_owned(),
    }
    .to_owned()
}

/// Protocol 1.7 fields of one `Attempts` row: `ordinal`, the repository and project,
/// `validation` (its candidate's newest host validation and the human's decision) and
/// `activity` (its newest provider record).
pub(crate) fn attempt_details(
    c: &Connection,
    attempt: &str,
    plan: Option<&str>,
    feature: Option<&str>,
) -> Result<Vec<(&'static str, Json)>, StorageError> {
    let mut fields = vec![("ordinal", Json::Int(attempt_ordinal(c, attempt)?))];
    fields.extend(place(c, attempt_repository(c, plan, feature)?)?);
    let candidate = c
        .query_optional(
            "SELECT candidate FROM attempt_results WHERE attempt=?1",
            &[Value::Text(attempt)],
            |r| r.optional_text(0),
        )?
        .flatten();
    let validation = match candidate {
        None => Json::Null,
        Some(_) => {
            let (verdict, at) = match state(c, attempt)? {
                ValidationState::NotRun => ("NotRun", Json::Null),
                ValidationState::Unknown(_) => ("Unknown", Json::Null),
                ValidationState::Passed(seq) => ("Passed", time_of(c, to_db(seq.0)?)?),
                ValidationState::Failed(seq) => ("Failed", time_of(c, to_db(seq.0)?)?),
            };
            let accepted = c
                .query_optional(
                    "SELECT 1 FROM candidate_acceptances WHERE attempt=?1",
                    &[Value::Text(attempt)],
                    |r| r.int(0),
                )?
                .is_some();
            let decision = if accepted {
                text("Accepted")
            } else if rejected(c, attempt)? {
                text("Rejected")
            } else {
                Json::Null
            };
            object([
                ("state", text(verdict)),
                ("at_unix_ms", at),
                ("decision", decision),
            ])
        }
    };
    fields.push(("validation", validation));
    let newest = c.query_optional(
        "SELECT kind, event FROM provider_events WHERE attempt=?1 ORDER BY sequence DESC LIMIT 1",
        &[Value::Text(attempt)],
        |r| Ok((r.text(0)?, r.int(1)?)),
    )?;
    fields.push((
        "activity",
        match newest {
            None => Json::Null,
            Some((kind, event)) => object([
                ("text", text(activity_text(&kind))),
                ("at_unix_ms", time_of(c, event)?),
            ]),
        },
    ));
    Ok(fields)
}

/// The step's key and title, "T4 · Title", or "A step" for a step of an imported plan.
fn step_label(c: &Connection, step: &str) -> Result<String, StorageError> {
    Ok(c.query_optional(
        "SELECT key,title FROM lead_plan_steps WHERE step=?1 LIMIT 1",
        &[Value::Text(step)],
        |r| Ok((r.text(0)?, r.text(1)?)),
    )?
    .map_or_else(
        || "A step".into(),
        |(key, title)| format!("{key} · {}", cut(&title)),
    ))
}

/// "T4 · attempt 2", or "Attempt 2" without a key; `None` for an unknown attempt.
pub(crate) fn attempt_label(c: &Connection, attempt: &str) -> Result<Option<String>, StorageError> {
    let row = c.query_optional(
        "SELECT a.step, (SELECT key FROM lead_plan_steps WHERE step=a.step LIMIT 1)
         FROM scheduler_attempts a WHERE a.attempt=?1",
        &[Value::Text(attempt)],
        |r| Ok((r.text(0)?, r.optional_text(1)?)),
    )?;
    let Some((_, key)) = row else { return Ok(None) };
    let n = attempt_ordinal(c, attempt)?;
    Ok(Some(match key {
        Some(key) => format!("{key} · attempt {n}"),
        None => format!("Attempt {n}"),
    }))
}

/// A document as a person names it: a requirements document by its title, the others by
/// kind and feature.
fn document_label(c: &Connection, document: &str) -> Result<String, StorageError> {
    let prd = c.query_optional(
        "SELECT content FROM artifact_versions WHERE artifact_id=?1 ORDER BY revision DESC LIMIT 1",
        &[Value::Text(document)],
        |r| r.text(0),
    )?;
    if let Some(content) = prd {
        return Ok(crate::overview::prd_title(&content).unwrap_or_else(|| "Requirements".into()));
    }
    if let Some(title) = crate::briefs::brief_title(c, document)? {
        return Ok(title);
    }
    let kind = c.query_optional(
        "SELECT kind, EXISTS(SELECT 1 FROM execution_plans WHERE plan=?1) FROM review_documents
         WHERE id=?1 LIMIT 1",
        &[Value::Text(document)],
        |r| Ok((r.text(0)?, r.int(1)? != 0)),
    )?;
    let kind = match kind {
        Some((k, _)) if k == "ADR" => "ADR",
        Some((_, true)) => "Plan",
        Some(_) => "Validation plan",
        None => "Document",
    };
    let feature = crate::overview::document_feature(c, document)?;
    let name = feature.map(|f| feature_name(c, &f)).transpose()?.flatten();
    Ok(match name {
        Some(name) => format!("{kind} · {name}"),
        None => kind.to_owned(),
    })
}

/// A repository's directory name, or "a repository".
fn repository_label(c: &Connection, repository: &str) -> Result<String, StorageError> {
    let path = c.query_optional(
        "SELECT path FROM local_repositories WHERE id=?1",
        &[Value::Text(repository)],
        |r| r.text(0),
    )?;
    Ok(path
        .as_deref()
        .map_or_else(|| "a repository".into(), repository_name))
}

/// A plan's feature name, or "a plan".
fn plan_label(c: &Connection, plan: &str) -> Result<String, StorageError> {
    let feature = crate::overview::document_feature(c, plan)?;
    Ok(feature
        .map(|f| feature_name(c, &f))
        .transpose()?
        .flatten()
        .unwrap_or_else(|| "a plan".into()))
}

/// "WorktreeReady" as "Worktree ready".
fn humanized(kind: &str) -> String {
    let mut out = String::new();
    for (i, ch) in kind.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push(' ');
            out.extend(ch.to_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

fn outcome_words(outcome: &str) -> &'static str {
    match outcome {
        "SUCCEEDED" | "REPLIED" => "succeeded",
        "FAILED" => "failed",
        "CANCELLED" => "was cancelled",
        _ => "was interrupted",
    }
}

fn provider_words(provider: &str) -> &str {
    if provider == "ClaudeCode" {
        "Claude Code"
    } else {
        provider
    }
}

/// One sentence that says what an event recorded, with the names a person knows. An
/// event type without a sentence of its own is named by its type, and by the attempt it
/// is about when the first field of its payload names one.
pub(crate) fn event_summary(
    c: &Connection,
    seq: i64,
    epoch: i64,
    kind: &str,
    aggregate: Option<&str>,
    payload: &str,
) -> Result<String, StorageError> {
    let fields: Vec<&str> = payload.split('|').collect();
    let field = |i: usize| fields.get(i).copied().unwrap_or("");
    let attempt = |i: usize| -> Result<String, StorageError> {
        Ok(attempt_label(c, field(i))?.unwrap_or_else(|| "An attempt".into()))
    };
    let sentence = match kind {
        "EngineStarted" => format!("Session {epoch} started"),
        "EngineStopped" if payload.contains("CLEAN") && !payload.contains("UNCLEAN") => {
            "Session stopped cleanly".into()
        }
        "EngineStopped" => "Session stopped".into(),
        "ProcessLaunchIssued" => {
            let provider = c.query_optional(
                "SELECT provider FROM process_launches WHERE attempt=?1",
                &[Value::Text(field(0))],
                |r| r.text(0),
            )?;
            match provider {
                Some(p) => format!("{} started on {}", attempt(0)?, provider_words(&p)),
                None => format!("{} started", attempt(0)?),
            }
        }
        "AttemptResultSealed" => format!("{} {}", attempt(0)?, outcome_words(field(1))),
        "AttemptRecoveryRequired" => format!("{} needs recovery", attempt(0)?),
        "AttemptStopCause" => format!("Stop of {} requested", attempt(0)?),
        "ValidationRequested" => format!("Host validation of {} requested", attempt(0)?),
        "ValidationEnded" => {
            let status = field(2)
                .parse::<i64>()
                .ok()
                .map(|receipt| {
                    c.query_optional(
                        "SELECT status FROM validation_receipts WHERE event=?1",
                        &[Value::Int(receipt)],
                        |r| r.text(0),
                    )
                })
                .transpose()?
                .flatten();
            let verdict = match status.as_deref() {
                Some("PASSED") => "passed",
                Some("FAILED") => "failed",
                _ => "ended without a verdict",
            };
            format!("Host validation of {} {verdict}", attempt(0)?)
        }
        "CandidateAccepted" => format!("{} accepted", attempt(0)?),
        "CandidateRejected" => format!("{} rejected", attempt(0)?),
        "HumanDecision" => {
            let row = c.query_optional(
                "SELECT subject, revision, action FROM human_decisions WHERE seq=?1",
                &[Value::Int(seq)],
                |r| Ok((r.text(0)?, r.int(1)?, r.text(2)?)),
            )?;
            match row {
                Some((subject, revision, action)) => {
                    let verb = match action.as_str() {
                        "Approve" => "approved",
                        "Reject" => "rejected",
                        _ => "revoked",
                    };
                    format!("{} rev {revision} {verb}", document_label(c, &subject)?)
                }
                None => "Decision recorded".into(),
            }
        }
        "ArtifactVersionCreated" => {
            let row = c.query_optional(
                "SELECT artifact_id, revision FROM artifact_versions WHERE created_event_seq=?1",
                &[Value::Int(seq)],
                |r| Ok((r.text(0)?, r.int(1)?)),
            )?;
            match row {
                Some((id, revision)) => {
                    format!("{} rev {revision} drafted", document_label(c, &id)?)
                }
                None => "Requirements drafted".into(),
            }
        }
        "ReviewDocumentCreated" => {
            let row = c.query_optional(
                "SELECT id, revision FROM review_documents WHERE event=?1 LIMIT 1",
                &[Value::Int(seq)],
                |r| Ok((r.text(0)?, r.int(1)?)),
            )?;
            match row {
                Some((id, revision)) => {
                    format!("{} rev {revision} proposed", document_label(c, &id)?)
                }
                None => "Document proposed".into(),
            }
        }
        "LeadTurnAccepted" => {
            let feature = field(1).strip_prefix("feature=").unwrap_or("");
            let name = feature_name(c, feature)?.unwrap_or_else(|| "a feature".into());
            let turn = if field(2) == "Plan" {
                "Planning"
            } else {
                "Discovery"
            };
            format!(
                "{turn} turn for {name} started on {}",
                provider_words(field(3))
            )
        }
        "LeadTurnFinished" => {
            let feature = c.query_optional(
                "SELECT feature FROM lead_turns WHERE turn=?1",
                &[Value::Text(field(0))],
                |r| r.text(0),
            )?;
            let name = feature.map(|f| feature_name(c, &f)).transpose()?.flatten();
            let verb = if field(1) == "REPLIED" {
                "replied"
            } else {
                outcome_words(field(1))
            };
            format!(
                "Lead turn for {} {verb}",
                name.unwrap_or_else(|| "a feature".into())
            )
        }
        "QuestionAsked" | "QuestionAnswered" => {
            let feature = aggregate
                .map(|q| {
                    c.query_optional(
                        "SELECT feature_id FROM clarification_questions WHERE question_id=?1",
                        &[Value::Text(q)],
                        |r| r.text(0),
                    )
                })
                .transpose()?
                .flatten();
            let name = feature.map(|f| feature_name(c, &f)).transpose()?.flatten();
            let name = name.unwrap_or_else(|| "a feature".into());
            if kind == "QuestionAsked" {
                format!("Question asked about {name}")
            } else {
                format!("Question about {name} answered")
            }
        }
        "PlanRunStarted" => format!("Plan run of {} started", plan_label(c, field(0))?),
        "PlanRunControl" => {
            let verb = match field(2) {
                "PAUSED" => "paused",
                "CANCELLED" => "cancelled",
                _ => "running",
            };
            format!("Plan run of {} {verb}", plan_label(c, field(0))?)
        }
        "PlanStepIntegrated" => {
            format!(
                "{} integrated onto the plan branch",
                step_label(c, field(1))?
            )
        }
        "PlanStepHeld" => format!("{} waits for you", step_label(c, field(1))?),
        "PlanStepRefused" => format!("{} could not start", step_label(c, field(1))?),
        "PlanStepRepairDecided" => format!("{} will be repaired", step_label(c, field(1))?),
        "StepReviewAccepted" => {
            format!(
                "Review of {} started on {}",
                attempt(3)?,
                provider_words(field(4))
            )
        }
        "StepReviewFinished" => {
            let reviewed = c.query_optional(
                "SELECT attempt FROM step_reviews WHERE review=?1",
                &[Value::Text(field(0))],
                |r| r.text(0),
            )?;
            let of = match reviewed {
                Some(a) => attempt_label(c, &a)?.unwrap_or_else(|| "an attempt".into()),
                None => "an attempt".into(),
            };
            format!("Review of {of}: {}", field(1).to_lowercase())
        }
        "BriefTurnAccepted" => format!(
            "Project brief of {} started on {}",
            repository_label(c, field(2))?,
            provider_words(field(3))
        ),
        "BriefTurnFinished" => {
            let repository = c.query_optional(
                "SELECT repository FROM brief_turns WHERE turn=?1",
                &[Value::Text(field(0))],
                |r| r.text(0),
            )?;
            let verb = if field(1) == "REPLIED" {
                "replied"
            } else {
                outcome_words(field(1))
            };
            format!(
                "Project brief turn of {} {verb}",
                repository_label(c, repository.as_deref().unwrap_or(""))?
            )
        }
        "ProjectBriefCreated" => {
            let revision = field(1).strip_prefix("revision=").unwrap_or("?");
            format!("{} rev {revision} proposed", document_label(c, field(0))?)
        }
        "RecoveryResolved" => format!("{} resolved in recovery", attempt(0)?),
        "RecoveryStarted" => "Admission reopened after the recovery review".into(),
        "ProjectSaved" => {
            let name = id_from_text(field(0))
                .ok()
                .map(|id| crate::projects::latest(c, id))
                .transpose()?
                .flatten()
                .map(|(_, p)| cut(&p.name));
            format!("Project {} saved", name.unwrap_or_else(|| "record".into()))
        }
        "LocalRepositoryRegistered" => {
            let path = c.query_optional(
                "SELECT path FROM local_repositories WHERE id=?1",
                &[Value::Text(field(0))],
                |r| r.text(0),
            )?;
            format!(
                "Repository {} registered",
                path.as_deref().map_or_else(|| "?".into(), repository_name)
            )
        }
        "ExecutableRecorded" => format!("{} executable recorded", provider_words(field(1))),
        "ExecutableChanged" => format!("{} executable changed", provider_words(field(0))),
        _ => match attempt_label(c, field(0))? {
            Some(label) => format!("{} · {label}", humanized(kind)),
            None => humanized(kind),
        },
    };
    Ok(cut(&sentence))
}

/// Protocol 1.11: the kind of thing an event recorded, as Activity & Audit filters it:
/// `Session`, `Attempt`, `Recovery`, `Validation`, `Decision`, `Approval`, `Git`,
/// `Evidence`, `Chat`, `Document`, `Plan`, `Setup`, `Error` or `Core`.
pub(crate) fn event_category(kind: &str, payload: &str) -> &'static str {
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| kind.starts_with(p));
    match kind {
        "EngineStopped" if payload.contains("UNCLEAN") => "Error",
        "PlanStepRefused" | "WorktreeBlocked" | "NotReady" | "SchedulerNotStarted" => "Error",
        "ExternalActionObserved" if payload.contains("REJECTED") => "Error",
        "HumanDecision"
        | "ExternalActionRequested"
        | "ExternalActionAuthorized"
        | "ExternalActionDeclined"
        | "PermissionRequested" => "Approval",
        "CandidateAccepted"
        | "CandidateRejected"
        | "QuestionAnswered"
        | "QuestionDispositionChanged"
        | "StepQuestionAnswered"
        | "StepQuestionDispositionChanged"
        | "PlanRunControl"
        | "SchedulerControl" => "Decision",
        "PlanStepIntegrated" | "LocalRepositoryRegistered" => "Git",
        "ContinuationPlanRecorded" => "Recovery",
        "ProjectSaved" | "ResourceConfigurationChanged" => "Setup",
        "ArtifactVersionCreated"
        | "ReviewDocumentCreated"
        | "ProjectBriefCreated"
        | "ExecutionPlanCreated"
        | "ValidationPlanStored" => "Document",
        "UsageObserved" | "EffectObserved" => "Attempt",
        _ if starts(&["Engine", "Shutdown", "CloseTicket", "AdmissionClosed"]) => "Session",
        _ if starts(&["Recovery", "AttemptRecovery"]) => "Recovery",
        _ if starts(&["Validation", "StepReview", "Review"]) => "Validation",
        _ if starts(&["ExternalAction", "Worktree"]) => "Git",
        _ if starts(&["Evidence"]) => "Evidence",
        _ if starts(&["Executable", "BriefTurn"]) => "Setup",
        _ if starts(&["LeadTurn", "Question", "StepQuestion", "Turn", "Tool"]) => "Chat",
        _ if starts(&["PlanRun", "PlanStep", "Dependency"]) => "Plan",
        _ if starts(&[
            "Attempt",
            "Process",
            "Scheduler",
            "ApprovalAttempt",
            "ApprovalStop",
        ]) =>
        {
            "Attempt"
        }
        _ => "Core",
    }
}

/// Protocol 1.11: the project an event concerns, found through the first identity it
/// names (the subject a decision or a document version of this event records, its
/// aggregate, then each payload field) that leads to a registered repository: an
/// attempt's worktree, a repository, a feature, a document, a plan run, an external
/// action, or a project itself. `None` when nothing leads to one.
pub(crate) fn event_project(
    c: &Connection,
    seq: i64,
    aggregate: Option<&str>,
    payload: &str,
) -> Result<Option<(String, String)>, StorageError> {
    let recorded = c
        .query_optional(
            "SELECT coalesce(
               (SELECT subject FROM human_decisions WHERE seq=?1),
               (SELECT artifact_id FROM artifact_versions WHERE created_event_seq=?1))",
            &[Value::Int(seq)],
            |r| r.optional_text(0),
        )?
        .flatten();
    let candidates = recorded
        .as_deref()
        .into_iter()
        .chain(aggregate)
        .chain(payload.split('|'))
        .map(str::trim)
        .filter(|f| {
            (f.len() == 32 && f.bytes().all(|b| b.is_ascii_hexdigit()))
                || f.starts_with("external:")
        });
    for field in candidates {
        if field.starts_with("external:") {
            let repository = c.query_optional(
                "SELECT repository FROM external_actions WHERE operation_key=?1",
                &[Value::Text(field)],
                |r| r.text(0),
            )?;
            if let Some(found) = project_of(c, repository)? {
                return Ok(Some(found));
            }
            continue;
        }
        if let Some(record) = crate::projects::latest(c, id_from_text(field)?)? {
            return Ok(Some((field.to_owned(), cut(&record.1.name))));
        }
        let repository = c
            .query_optional(
                "SELECT coalesce(
                   (SELECT repository FROM owned_worktrees WHERE attempt=?1),
                   (SELECT id FROM local_repositories WHERE id=?1),
                   (SELECT repository FROM brief_turns WHERE turn=?1),
                   (SELECT repository FROM project_briefs WHERE id=?1 LIMIT 1),
                   (SELECT repository FROM plan_runs WHERE plan=?1 ORDER BY revision DESC LIMIT 1))",
                &[Value::Text(field)],
                |r| r.optional_text(0),
            )?
            .flatten();
        let repository = match repository {
            Some(r) => Some(r),
            None => {
                let feature = match document_feature(c, field)? {
                    Some(f) => Some(f),
                    None => c.query_optional(
                        "SELECT feature FROM lead_turns WHERE feature=?1 OR turn=?1 LIMIT 1",
                        &[Value::Text(field)],
                        |r| r.text(0),
                    )?,
                };
                feature
                    .map(|f| feature_repository(c, &f))
                    .transpose()?
                    .flatten()
            }
        };
        if let Some(found) = project_of(c, repository)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// The project a repository belongs to at its newest revision, by identity and name.
fn project_of(
    c: &Connection,
    repository: Option<String>,
) -> Result<Option<(String, String)>, StorageError> {
    let Some(repository) = repository else {
        return Ok(None);
    };
    Ok(
        crate::projects::of_repository(c, RepositoryId(id_from_text(&repository)?))?
            .map(|(id, _, record)| (crate::model::id_to_text(id), cut(&record.name))),
    )
}

/// Protocol 1.8: when a lead turn was accepted and when its result was recorded.
pub(crate) fn turn_times(
    c: &Connection,
    turn: jojo_domain::identity::OpaqueId,
) -> Result<[(&'static str, Json); 2], StorageError> {
    let id = crate::model::id_to_text(turn);
    let (started, ended) = c.query_one(
        "SELECT (SELECT event FROM lead_turns WHERE turn=?1),
                (SELECT event FROM lead_turn_results WHERE turn=?1)",
        &[Value::Text(&id)],
        |r| Ok((r.optional_int(0)?, r.optional_int(1)?)),
    )?;
    Ok([
        (
            "started_at_unix_ms",
            started.map_or(Ok(Json::Null), |s| time_of(c, s))?,
        ),
        (
            "ended_at_unix_ms",
            ended.map_or(Ok(Json::Null), |s| time_of(c, s))?,
        ),
    ])
}

/// Assumptions listed per feature.
const FEATURE_ASSUMPTIONS: usize = 32;

/// Per assumption id over the revisions in order: its newest statement, the revision it
/// first appeared in, and the first revision after its last appearance (`None` while the
/// newest revision has it).
fn merge_assumptions(
    revisions: &[(i64, Vec<(String, String)>)],
) -> Vec<(String, String, i64, Option<i64>)> {
    let mut seen: Vec<(String, String, i64, Option<i64>)> = Vec::new();
    for (revision, assumptions) in revisions {
        for entry in seen.iter_mut() {
            if entry.3.is_none() && !assumptions.iter().any(|(id, _)| *id == entry.0) {
                entry.3 = Some(*revision);
            }
        }
        for (id, statement) in assumptions {
            match seen.iter_mut().find(|e| e.0 == *id) {
                Some(entry) => {
                    entry.1 = statement.clone();
                    entry.3 = None;
                }
                None => seen.push((id.clone(), statement.clone(), *revision, None)),
            }
        }
    }
    seen
}

/// Protocol 1.8: the assumptions of a feature's requirements document. Those of the
/// newest revision are `Current`, with the revision they first appeared in; one an
/// earlier revision had and the newest does not is `Dropped`, with the revision that
/// dropped it.
pub(crate) fn feature_assumptions(
    c: &Connection,
    feature: jojo_domain::identity::FeatureId,
) -> Result<Json, StorageError> {
    let versions = c.query(
        "SELECT revision, content FROM artifact_versions WHERE feature_id=?1
           AND artifact_id=(SELECT artifact_id FROM artifact_versions WHERE feature_id=?1
                            ORDER BY created_event_seq DESC LIMIT 1)
         ORDER BY revision",
        &[Value::Text(&crate::model::id_to_text(feature.0))],
        |r| Ok((r.int(0)?, r.text(1)?)),
    )?;
    let revisions: Vec<(i64, Vec<(String, String)>)> = versions
        .iter()
        .filter_map(|(revision, content)| {
            let document = jojo_domain::prd::PrdDocument::import(content.as_bytes()).ok()?;
            Some((
                *revision,
                document
                    .assumptions
                    .into_iter()
                    .map(|a| (a.id, cut(&a.statement)))
                    .collect(),
            ))
        })
        .collect();
    let seen = merge_assumptions(&revisions);
    Ok(Json::List(
        seen.into_iter()
            .take(FEATURE_ASSUMPTIONS)
            .map(|(id, statement, since, dropped)| {
                object([
                    ("id", text(id)),
                    ("statement", text(statement)),
                    (
                        "state",
                        text(if dropped.is_some() {
                            "Dropped"
                        } else {
                            "Current"
                        }),
                    ),
                    ("revision", Json::Int(dropped.unwrap_or(since))),
                ])
            })
            .collect(),
    ))
}

/// Blocking questions about a feature whose newest answer is missing or not a human's.
fn open_questions(c: &Connection, feature: &str) -> Result<Vec<(String, String)>, StorageError> {
    Ok(c.query(
        "SELECT q.question_id, q.prompt FROM clarification_questions q
         JOIN aggregates a ON a.kind=q.question_kind AND a.id=q.question_id
         WHERE q.feature_id=?1 AND a.state='BLOCKING'
           AND coalesce((SELECT author FROM clarification_answers x WHERE x.question_id=q.question_id
                         ORDER BY x.ordinal DESC LIMIT 1), '')<>'HUMAN'
         ORDER BY q.created_event_seq",
        &[Value::Text(feature)],
        |r| Ok((r.text(0)?, r.text(1)?)),
    )?)
}

/// The newest attempt of each step of a feature's plans, with what decides the stages.
struct StepAttempt {
    live: bool,
    outcome: Option<String>,
    held: bool,
    attempt: String,
    candidate: bool,
}

fn feature_attempts(
    c: &Connection,
    prd: &str,
    epoch: i64,
) -> Result<Vec<StepAttempt>, StorageError> {
    Ok(c.query(
        "SELECT a.attempt, a.epoch, r.outcome, r.candidate IS NOT NULL,
           EXISTS(SELECT 1 FROM attempt_recovery_holds h WHERE h.attempt=a.attempt)
         FROM scheduler_attempts a JOIN dependency_attempt_inputs d ON d.attempt=a.attempt
         LEFT JOIN attempt_results r ON r.attempt=a.attempt
         WHERE d.plan IN (SELECT id FROM review_documents WHERE prd=?1 AND kind='PLAN')
           AND a.rowid=(SELECT max(b.rowid) FROM scheduler_attempts b WHERE b.step=a.step)
         ORDER BY a.rowid",
        &[Value::Text(prd)],
        |r| {
            let outcome = r.optional_text(2)?;
            let held = r.int(4)? != 0;
            Ok(StepAttempt {
                live: outcome.is_none() && !held && r.int(1)? == epoch,
                outcome,
                held,
                attempt: r.text(0)?,
                candidate: r.int(3)? != 0,
            })
        },
    )?)
}

impl ReadView<'_> {
    /// Features newest first, at most [`FEATURE_ROWS`] numbered below `before` (the
    /// `number` of a row; `None` for the newest). A feature's number is its place in the
    /// order features first appeared, from 1.
    pub fn ipc_features(&self, before: Option<u64>) -> Result<Json, StorageError> {
        let c = self.conn;
        let epoch = to_db(self.session()?.epoch.0)?;
        let all = c.query(
            "SELECT feature FROM (
               SELECT feature, event AS seq FROM lead_turns
               UNION ALL SELECT feature_id, created_event_seq FROM artifact_versions)
             GROUP BY feature ORDER BY min(seq)",
            &[],
            |r| r.text(0),
        )?;
        let before = before.map_or(Ok(i64::MAX), to_db)?;
        let numbered: Vec<(i64, String)> = all
            .into_iter()
            .enumerate()
            .map(|(i, f)| (i as i64 + 1, f))
            .filter(|(n, _)| *n < before)
            .collect();
        let more = numbered.len() > FEATURE_ROWS;
        let mut out = Vec::new();
        for (number_, feature) in numbered.iter().rev().take(FEATURE_ROWS) {
            out.push(self.feature_row(*number_, feature, epoch)?);
        }
        let next = if more {
            numbered
                .iter()
                .rev()
                .nth(FEATURE_ROWS - 1)
                .map_or(Json::Null, |(n, _)| Json::Int(*n))
        } else {
            Json::Null
        };
        Ok(object([("features", Json::List(out)), ("next", next)]))
    }

    fn feature_row(&self, number_: i64, feature: &str, epoch: i64) -> Result<Json, StorageError> {
        let c = self.conn;
        let running_turn = c.query_optional(
            "SELECT t.kind FROM lead_turns t WHERE t.feature=?1
               AND NOT EXISTS(SELECT 1 FROM lead_turn_results r WHERE r.turn=t.turn)
             ORDER BY t.ordinal DESC LIMIT 1",
            &[Value::Text(feature)],
            |r| r.text(0),
        )?;
        let turns = c.query_one(
            "SELECT count(*) FROM lead_turns WHERE feature=?1",
            &[Value::Text(feature)],
            |r| r.int(0),
        )?;
        let questions = open_questions(c, feature)?;
        let prd = c.query_optional(
            "SELECT artifact_id, revision FROM artifact_versions WHERE feature_id=?1
             ORDER BY created_event_seq DESC LIMIT 1",
            &[Value::Text(feature)],
            |r| Ok((r.text(0)?, r.int(1)?)),
        )?;
        let mut awaiting = 0usize;

        let discovery = if !questions.is_empty() {
            Stage::new(
                "NeedsYou",
                format!("{} open", counted(questions.len(), "Q")),
            )
        } else if running_turn.as_deref() == Some("Discover") {
            Stage::new("Running", "Running")
        } else if prd.is_some() {
            Stage::done()
        } else if turns > 0 {
            Stage::new("Current", "In discussion")
        } else {
            Stage::off()
        };

        let specification = match &prd {
            None => Stage::off(),
            Some((id, revision)) => {
                let (approval, stale, _) = approval_state(c, ArtifactId(id_from_text(id)?), true)?;
                if approval == "Pending" {
                    awaiting += 1;
                }
                match (approval, stale) {
                    ("Approved", _) => Stage::done(),
                    ("Pending", true) => Stage::new("Stale", "Stale"),
                    ("Pending", false) => Stage::new("NeedsYou", format!("PRD rev {revision}")),
                    ("Rejected", _) => Stage::new("Failed", "Rejected"),
                    ("Revoked", _) => Stage::new("Failed", "Revoked"),
                    ("NotReady", _) => Stage::new("Current", "Drafting"),
                    _ => Stage::off(),
                }
            }
        };

        let documents = match &prd {
            None => Vec::new(),
            Some((id, _)) => c.query(
                "SELECT id, kind, max(revision) FROM review_documents WHERE prd=?1
                 GROUP BY id ORDER BY kind, min(event)",
                &[Value::Text(id)],
                |r| Ok((r.text(0)?, r.text(1)?, r.int(2)?)),
            )?,
        };
        let mut decided = Vec::new();
        for (id, kind, revision) in &documents {
            let (approval, stale, _) = approval_state(c, ArtifactId(id_from_text(id)?), false)?;
            if approval == "Pending" {
                awaiting += 1;
            }
            let label = if kind == "ADR" { "ADR" } else { "Plan" };
            decided.push((approval, stale, format!("{label} rev {revision}")));
        }
        let planning = if running_turn.as_deref() == Some("Plan") {
            Stage::new("Running", "Planning")
        } else if decided.is_empty() {
            Stage::off()
        } else if let Some((_, _, label)) = decided.iter().find(|(a, s, _)| *a == "Pending" && !s) {
            Stage::new("NeedsYou", label.clone())
        } else if decided.iter().any(|(a, s, _)| *a == "Pending" && *s) {
            Stage::new("Stale", "Stale")
        } else if let Some((a, _, _)) = decided
            .iter()
            .find(|(a, _, _)| *a == "Rejected" || *a == "Revoked")
        {
            Stage::new("Failed", *a)
        } else if decided.iter().all(|(a, _, _)| *a == "Approved") {
            Stage::done()
        } else {
            Stage::off()
        };

        let run_plan = match &prd {
            None => None,
            Some((id, _)) => c.query_optional(
                "SELECT plan FROM plan_runs WHERE prd=?1 ORDER BY event DESC LIMIT 1",
                &[Value::Text(id)],
                |r| r.text(0),
            )?,
        };
        let run = run_plan
            .map(|p| id_from_text(&p).map(ArtifactId))
            .transpose()?
            .map(|p| self.plan_run(p))
            .transpose()?
            .flatten();
        let attempts = match &prd {
            None => Vec::new(),
            Some((id, _)) => feature_attempts(c, id, epoch)?,
        };
        let live = attempts.iter().filter(|a| a.live).count();

        let mut blockers = Vec::new();
        for (question, prompt) in &questions {
            blockers.push(object([
                ("kind", text("Question")),
                ("question", identity_text(question)?),
                ("step", Json::Null),
                ("key", Json::Null),
                ("title", Json::Null),
                ("reason", text(cut(prompt))),
            ]));
        }
        let execution = match &run {
            Some(run) => {
                let states: Vec<&str> = run.steps.iter().map(|s| s.state.name()).collect();
                let count = |names: &[&str]| states.iter().filter(|s| names.contains(s)).count();
                let running = count(&["Running", "Repairing", "Validating", "Reviewing"]);
                let stuck = count(&["Blocked", "NeedsHuman", "Conflict", "Refused"]);
                let review = count(&["Review"]);
                for step in &run.steps {
                    if ["Blocked", "NeedsHuman", "Conflict", "Refused"].contains(&step.state.name())
                    {
                        blockers.push(object([
                            ("kind", text("Step")),
                            ("question", Json::Null),
                            ("step", jojo_domain::ipc::identity(step.step.id)),
                            ("key", text(step.key.clone())),
                            ("title", text(cut(&step.title))),
                            ("reason", text(cut(&step.detail))),
                        ]));
                    }
                }
                match run.state.as_str() {
                    "DONE" => Stage::done(),
                    _ if running > 0 => Stage::new("Running", format!("{running} running")),
                    _ if stuck > 0 => Stage::new("Blocked", format!("{stuck} blocked")),
                    _ if review > 0 => Stage::new("NeedsYou", format!("{review} to review")),
                    "PAUSED" => Stage::new("NeedsYou", "Paused"),
                    "CANCELLED" => Stage::new("Failed", "Cancelled"),
                    "HALTED" => Stage::new("Blocked", "Halted"),
                    _ => Stage::new("Current", "In progress"),
                }
            }
            None if attempts.is_empty() => Stage::off(),
            None if live > 0 => Stage::new("Running", format!("{live} running")),
            None if attempts.iter().any(|a| a.held) => Stage::new("Blocked", "Held"),
            None if attempts
                .iter()
                .any(|a| a.outcome.as_deref() == Some("SUCCEEDED")) =>
            {
                Stage::done()
            }
            None if attempts.iter().all(|a| a.outcome.is_some()) => Stage::new("Failed", "Failed"),
            None => Stage::new("Blocked", "Unsupervised"),
        };

        let (mut failed, mut unknown, mut review, mut not_run, mut accepted) = (0, 0, 0, 0, 0);
        for a in attempts.iter().filter(|a| a.candidate) {
            match state(c, &a.attempt)? {
                ValidationState::Failed(_) => failed += 1,
                ValidationState::Unknown(_) => unknown += 1,
                ValidationState::NotRun => not_run += 1,
                ValidationState::Passed(_) => {
                    let taken = c
                        .query_optional(
                            "SELECT 1 FROM candidate_acceptances WHERE attempt=?1",
                            &[Value::Text(&a.attempt)],
                            |r| r.int(0),
                        )?
                        .is_some();
                    if taken {
                        accepted += 1;
                    } else if !rejected(c, &a.attempt)? {
                        review += 1;
                    }
                }
            }
        }
        awaiting += review;
        let verification = if failed + unknown + review + not_run + accepted == 0 {
            if attempts.iter().any(|a| a.candidate) {
                Stage::new("Failed", "Rejected")
            } else {
                Stage::off()
            }
        } else if failed > 0 {
            Stage::new("Failed", format!("{failed} failed"))
        } else if unknown > 0 {
            Stage::new("Unknown", format!("{unknown} unknown"))
        } else if review > 0 {
            Stage::new("NeedsYou", format!("{review} to accept"))
        } else if not_run > 0 {
            Stage::new("NeedsYou", format!("{not_run} not run"))
        } else {
            Stage::done()
        };
        let done = match &run {
            Some(run) => run.state == "DONE",
            None => execution.state == "Done" && verification.state == "Done",
        };

        let blocked = blockers.len();
        blockers.truncate(FEATURE_BLOCKERS);
        let mut fields = vec![
            ("feature", identity_text(feature)?),
            ("number", Json::Int(number_)),
            ("name", optional(feature_name(c, feature)?)),
        ];
        fields.extend(place(c, feature_repository(c, feature)?)?);
        fields.extend([
            (
                "stages",
                object([
                    ("discovery", discovery.json()),
                    ("specification", specification.json()),
                    ("planning", planning.json()),
                    ("execution", execution.json()),
                    ("verification", verification.json()),
                ]),
            ),
            ("running", number(live as u64)),
            ("blocked", number(blocked as u64)),
            ("awaiting", number(awaiting as u64)),
            ("done", Json::Bool(done)),
            ("blockers", Json::List(blockers)),
        ]);
        Ok(object(fields))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        activity_text, counted, event_category, humanized, merge_assumptions, repository_name,
    };

    #[test]
    fn s6_every_event_kind_has_a_category() {
        for (kind, payload, category) in [
            ("EngineStarted", "", "Session"),
            ("EngineStopped", "shutdown=UNCLEAN", "Error"),
            ("EngineStopped", "shutdown=CLEAN", "Session"),
            ("HumanDecision", "", "Approval"),
            ("ExternalActionAuthorized", "", "Approval"),
            ("ExternalActionIssued", "", "Git"),
            ("ExternalActionObserved", "REJECTED", "Error"),
            ("CandidateAccepted", "", "Decision"),
            ("QuestionAnswered", "", "Decision"),
            ("ValidationEnded", "", "Validation"),
            ("StepReviewFinished", "", "Validation"),
            ("RecoveryResolved", "", "Recovery"),
            ("AttemptRecoveryRequired", "", "Recovery"),
            ("EvidenceBlobReferenced", "", "Evidence"),
            ("PlanStepIntegrated", "", "Git"),
            ("WorktreeReady", "", "Git"),
            ("PlanStepRefused", "", "Error"),
            ("PlanRunStarted", "", "Plan"),
            ("LeadTurnAccepted", "", "Chat"),
            ("ArtifactVersionCreated", "", "Document"),
            ("ProjectSaved", "", "Setup"),
            ("ExecutableChanged", "", "Setup"),
            ("ProcessLaunchIssued", "", "Attempt"),
            ("SchedulerReserved", "", "Attempt"),
            ("SomethingNew", "", "Core"),
        ] {
            assert_eq!(event_category(kind, payload), category, "{kind} {payload}");
        }
    }

    #[test]
    fn s3_words_for_names_counts_and_records() {
        assert_eq!(humanized("WorktreeReady"), "Worktree ready");
        assert_eq!(humanized("EngineStarted"), "Engine started");
        assert_eq!(repository_name("/Users/k/Code/crew-api/"), "crew-api");
        assert_eq!(repository_name("crew"), "crew");
        assert_eq!(counted(1, "Q"), "Q");
        assert_eq!(counted(3, "Q"), "3 Q");
        assert_eq!(activity_text("ToolStarted"), "Using a tool");
        assert_eq!(activity_text("Unseen"), "Unseen");
    }

    #[test]
    fn s4_an_assumption_a_newer_revision_drops_is_dropped_from_that_revision() {
        let a = |id: &str, s: &str| (id.to_owned(), s.to_owned());
        let merged = merge_assumptions(&[
            (1, vec![a("A1", "Links work"), a("A2", "Capacity 8")]),
            (
                2,
                vec![a("A1", "Links work on both stores"), a("A3", "Guests join")],
            ),
            (
                3,
                vec![a("A1", "Links work on both stores"), a("A3", "Guests join")],
            ),
        ]);
        assert_eq!(
            merged,
            [
                ("A1".into(), "Links work on both stores".into(), 1, None),
                ("A2".into(), "Capacity 8".into(), 1, Some(2)),
                ("A3".into(), "Guests join".into(), 2, None),
            ]
        );
    }
}

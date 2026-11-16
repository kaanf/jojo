//! Lead agent turns (M2, Feature Chat): acceptance, the supervised read-only run and what
//! a reply becomes.
//!
//! A turn is not an attempt. It has no approved plan, no worktree, no candidate and no
//! external effect, so it does not go through the scheduler, the launch-issue journal or
//! settlement. What it shares with an attempt is the part that keeps a provider process
//! in hand: the same `ProcessLaunch` checks, the same guarded spawn, the same signal
//! escalation and the same proof that the process tree is gone before anything is
//! concluded. The loop in [`run_turn`] is the one in `launch.rs` without its journal; the
//! raw streams of the run stay in the turn's private directory beside the database.
//!
//! One turn runs at a time, and never beside an attempt. Its prompt is composed inside
//! the accepting transaction, only from what the human typed and what the core stores,
//! and is kept with the turn. A reply creates drafts and questions authored by the model;
//! it approves nothing. A turn whose helper ended first is closed as interrupted by the
//! next helper; it holds nothing that needs recovery.
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::RecvTimeoutError;

use jojo_domain::dependency::{
    ApprovalBinding, DEPENDENCY_SCHEMA, Edge, EvidenceBinding, EvidenceKind, EvidenceRequirement,
    ExecutionPlan, Graph, GraphKind, NodeRef, Predicate, Source, SourceRevision, StepInputs,
};
use jojo_domain::identity::{ArtifactId, FeatureId, OpaqueId, QuestionId, RepositoryId, Revision};
use jojo_domain::ipc::{identity, number, object, text};
use jojo_domain::json::Json;
use jojo_domain::planner::{
    self, Exchange, LeadReply, MAX_HUMAN_TEXT_BYTES, PlanDraft, TurnKind, TurnSpec,
    discover_prompt, plan_prompt,
};
use jojo_domain::protocol::{ChatPart, QuestionAction};
use jojo_domain::provider::{
    EventPayload, MAX_PROMPT_BYTES, ProviderFingerprint, ProviderKind, Stream, TerminalStatus,
};
use jojo_domain::scheduler::{ResourceMode, StepResources};
use jojo_domain::validation::CANDIDATE_OUTPUT;

use crate::artifacts::{Author, PreparedPrd, Provenance, QuestionDisposition, version};
use crate::digest::sha256_hex;
use crate::engine::{Engine, ReadView};
use crate::error::{CommandError, StorageError, TxError};
use crate::launch::{LaunchContext, LaunchHooks, StopPlan};
use crate::model::AggregateKey;
use crate::model::text_enum;
use crate::model::{
    Admission, CommandId, CommandRequest, EngineEpoch, EventSeq, from_db, id_from_text, id_to_text,
    to_db,
};
use crate::process::{
    ExitObservation, ProcessLaunch, ProcessMessage, Signal, SignalOutcome, SpawnFailure, Supervised,
};
use crate::slice::{SLICE_ENVIRONMENT, SLICE_PERMISSION};
use crate::sqlite::{Connection, Value};
use crate::tx::Tx;
use jojo_domain::project::ProjectRecord;

/// A turn reads and thinks; ten minutes is far beyond what was observed (9 and 16 s).
pub const TURN_TIMEOUT_MILLIS: u64 = 10 * 60 * 1000;
pub const TURN_MAX_OUTPUT_BYTES: u64 = 16 * 1024 * 1024;
pub const TURN_MAX_FRAME_BYTES: u64 = 1024 * 1024;
/// Identities a reply may need: a PRD or three plan subjects, five questions, six steps
/// and the fifteen edges six ordered steps can have at most.
pub const TURN_REPLY_IDS: usize = 32;
/// How old the evidence that a prerequisite step passed its validation may be when a
/// dependent step is admitted. A draft value; the human approves it with the plan.
pub const STEP_EVIDENCE_MAX_AGE_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// Aggregate kind of a question about one step; its state is the question's disposition.
pub const STEP_QUESTION_KIND: &str = "step_question";
const MAX_DETAIL_BYTES: usize = 1024;
/// Bytes of turn rows one chat page carries before it stops (one row always fits).
pub const CHAT_PAGE_BYTES: usize = 40 * 1024;
/// Characters of a message or answer a chat page carries before it cuts them.
pub const CHAT_EXCERPT_CHARS: usize = 1024;
/// Rows of each list the chat index carries, and characters of each text in it.
pub const INDEX_ROWS: usize = 32;
pub const INDEX_TEXT_CHARS: usize = 160;
/// Characters one chat text chunk carries: at most 32 KiB of UTF-8.
pub const CHAT_TEXT_CHARS: usize = 8192;

fn deny(s: impl Into<String>) -> TxError {
    TxError::Rejected(s.into())
}

/// What the human asked for. Nothing here names an executable, an argument or a variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurnRequest {
    /// The command identity: one command, one turn, at most one run.
    pub turn: OpaqueId,
    pub feature: FeatureId,
    pub kind: TurnKind,
    pub repository: RepositoryId,
    pub provider: ProviderKind,
    /// The objective for the first turn of a feature, a note (possibly empty) after it.
    pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TurnOutcome {
    /// The run ended cleanly and its last message was a reply of the turn's schema.
    Replied,
    Failed,
    Cancelled,
    /// The helper that ran the turn ended before the turn did.
    Interrupted,
}

text_enum!(TurnOutcome {
    TurnOutcome::Replied => "REPLIED",
    TurnOutcome::Failed => "FAILED",
    TurnOutcome::Cancelled => "CANCELLED",
    TurnOutcome::Interrupted => "INTERRUPTED",
});

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurnResult {
    pub outcome: TurnOutcome,
    pub detail: String,
    /// The lead's words to the human; empty unless the turn replied.
    pub summary: String,
    /// The PRD version this turn created, if it created one.
    pub prd: Option<(ArtifactId, Revision)>,
    pub event: EventSeq,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurnRecord {
    pub turn: OpaqueId,
    pub feature: FeatureId,
    pub ordinal: u64,
    pub kind: TurnKind,
    pub repository: RepositoryId,
    pub base: String,
    pub provider: ProviderKind,
    pub message: String,
    pub prompt_digest: String,
    pub epoch: EngineEpoch,
    pub event: EventSeq,
    pub stop_requested: bool,
    pub result: Option<TurnResult>,
}

fn provider_text(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Codex => "Codex",
        ProviderKind::ClaudeCode => "ClaudeCode",
    }
}

const TURN_COLUMNS: &str = "t.turn,t.feature,t.ordinal,t.kind,t.repository,t.base,t.provider,\
    t.message,t.prompt_digest,t.epoch,t.event,\
    EXISTS(SELECT 1 FROM lead_turn_stops s WHERE s.turn=t.turn),\
    r.outcome,r.detail,r.summary,r.prd,r.prd_revision,r.event \
    FROM lead_turns t LEFT JOIN lead_turn_results r ON r.turn=t.turn";

type TurnRow = (
    (String, String, i64, String, String, String),
    (String, String, String, i64, i64, i64),
    (Option<String>, Option<String>, Option<String>),
    (Option<String>, Option<i64>, Option<i64>),
);

fn turn_row(r: &crate::sqlite::Row<'_>) -> Result<TurnRow, crate::sqlite::SqliteError> {
    Ok((
        (
            r.text(0)?,
            r.text(1)?,
            r.int(2)?,
            r.text(3)?,
            r.text(4)?,
            r.text(5)?,
        ),
        (
            r.text(6)?,
            r.text(7)?,
            r.text(8)?,
            r.int(9)?,
            r.int(10)?,
            r.int(11)?,
        ),
        (
            r.optional_text(12)?,
            r.optional_text(13)?,
            r.optional_text(14)?,
        ),
        (
            r.optional_text(15)?,
            r.optional_int(16)?,
            r.optional_int(17)?,
        ),
    ))
}

fn turn_record(row: TurnRow) -> Result<TurnRecord, StorageError> {
    let ((turn, feature, ordinal, kind, repository, base), b, c, d) = row;
    let (provider, message, prompt_digest, epoch, event, stop) = b;
    let corrupt = |what: &str| StorageError::corrupt(format!("lead turn with unknown {what}"));
    let result = match (c, d) {
        ((Some(outcome), Some(detail), Some(summary)), (prd, revision, Some(result_event))) => {
            Some(TurnResult {
                outcome: TurnOutcome::from_db(&outcome)?,
                detail,
                summary,
                prd: match (prd, revision) {
                    (Some(prd), Some(revision)) => Some((
                        ArtifactId(id_from_text(&prd)?),
                        Revision(from_db(revision)?),
                    )),
                    _ => None,
                },
                event: EventSeq(from_db(result_event)?),
            })
        }
        _ => None,
    };
    Ok(TurnRecord {
        turn: id_from_text(&turn)?,
        feature: FeatureId(id_from_text(&feature)?),
        ordinal: from_db(ordinal)?,
        kind: TurnKind::named(&kind).ok_or_else(|| corrupt("kind"))?,
        repository: RepositoryId(id_from_text(&repository)?),
        base,
        provider: match provider.as_str() {
            "Codex" => ProviderKind::Codex,
            "ClaudeCode" => ProviderKind::ClaudeCode,
            _ => return Err(corrupt("provider")),
        },
        message,
        prompt_digest,
        epoch: EngineEpoch(from_db(epoch)?),
        event: EventSeq(from_db(event)?),
        stop_requested: stop != 0,
        result,
    })
}

fn turn(conn: &Connection, id: OpaqueId) -> Result<Option<TurnRecord>, StorageError> {
    conn.query_optional(
        &format!("SELECT {TURN_COLUMNS} WHERE t.turn=?1"),
        &[Value::Text(&id_to_text(id))],
        turn_row,
    )?
    .map(turn_record)
    .transpose()
}

fn feature_turns(conn: &Connection, feature: FeatureId) -> Result<Vec<TurnRecord>, StorageError> {
    conn.query(
        &format!("SELECT {TURN_COLUMNS} WHERE t.feature=?1 ORDER BY t.ordinal"),
        &[Value::Text(&id_to_text(feature.0))],
        turn_row,
    )?
    .into_iter()
    .map(turn_record)
    .collect()
}

fn excerpt(body: &str) -> (Json, Json) {
    let cut: String = body.chars().take(CHAT_EXCERPT_CHARS).collect();
    let complete = cut.len() == body.len();
    (text(cut), Json::Bool(complete))
}

/// A stable identity for the edge from one step to another.
fn edge_id(prerequisite: OpaqueId, dependent: OpaqueId) -> OpaqueId {
    let digest = sha256_hex(&[
        b"jojo.lead-plan-edge/1",
        id_to_text(prerequisite).as_bytes(),
        id_to_text(dependent).as_bytes(),
    ]);
    OpaqueId(u128::from_str_radix(&digest[..32], 16).unwrap_or(0) | 1)
}

/// The execution plan of a lead draft: one node per step, one edge per dependency whose
/// predicate is that the prerequisite step passed its host validation, every step from
/// the same repository and base commit the turn read, on its own provider.
fn lead_plan(
    draft: &PlanDraft,
    steps: &[OpaqueId],
    repository: RepositoryId,
    base: &str,
    validation: &str,
    approvals: Vec<ApprovalBinding>,
    providers: &[ProviderKind],
) -> ExecutionPlan {
    let node = |id: OpaqueId| NodeRef {
        id,
        revision: Revision(1),
    };
    let source = Source {
        repository,
        revision: SourceRevision::Commit(base.to_owned()),
    };
    let validation_digest = sha256_hex(&[validation.as_bytes()]);
    let mut edges = Vec::new();
    for (index, step) in draft.steps.iter().enumerate() {
        for key in &step.depends_on {
            let Some(from) = draft.steps.iter().position(|s| &s.key == key) else {
                continue;
            };
            edges.push(Edge {
                id: edge_id(steps[from], steps[index]),
                prerequisite: node(steps[from]),
                dependent: node(steps[index]),
                predicate: Predicate::Evidence {
                    schema: DEPENDENCY_SCHEMA,
                    requirement: Box::new(EvidenceRequirement {
                        kind: EvidenceKind::ValidationPassed,
                        evidence_id: format!("validation:{}", id_to_text(steps[from])),
                        observation_revision: Revision(1),
                        binding: EvidenceBinding {
                            sources: vec![source.clone()],
                            environment: None,
                            artifact_digest: None,
                            contract: None,
                            validation_plan: Some(validation_digest.clone()),
                            test_inventory: draft.steps[from].tests.clone(),
                            output: None,
                        },
                        max_age_ms: STEP_EVIDENCE_MAX_AGE_MS,
                    }),
                },
            });
        }
    }
    ExecutionPlan {
        schema: 2,
        artifacts: Graph {
            kind: GraphKind::Artifact,
            nodes: approvals
                .iter()
                .map(|a| NodeRef {
                    id: a.artifact.0,
                    revision: a.revision,
                })
                .collect(),
            edges: vec![],
        },
        execution: Graph {
            kind: GraphKind::Execution,
            nodes: steps.iter().map(|id| node(*id)).collect(),
            edges,
        },
        approvals,
        inputs: steps
            .iter()
            .map(|id| StepInputs {
                step: node(*id),
                environment: Some(SLICE_ENVIRONMENT.into()),
                selected_outputs: vec![],
                base_sources: vec![source.clone()],
            })
            .collect(),
        permission_digest: sha256_hex(&[SLICE_PERMISSION.as_bytes()]),
        validation_digest,
        required_outputs: vec![CANDIDATE_OUTPUT.into()],
        resources: draft
            .steps
            .iter()
            .zip(steps)
            .zip(providers)
            .map(|((_, id), provider)| StepResources {
                step: node(*id),
                repository,
                provider: *provider,
                priority: 0,
                additional: vec![],
                repository_mode: ResourceMode::Shared,
            })
            .collect(),
    }
}

/// The text a reviewer is shown for a lead plan; the canonical graph is the bound body.
fn plan_text(
    draft: &PlanDraft,
    steps: &[OpaqueId],
    repository: RepositoryId,
    base: &str,
    validation: ArtifactId,
    providers: &[ProviderKind],
    project: Option<(&str, u64)>,
) -> String {
    let mut out = format!(
        "Execution plan: {} steps, drafted by the lead agent\nrepository: {}\nbase commit: {base}\n\
         permission: {SLICE_PERMISSION}\nvalidation plan: {}\nrequired output: {CANDIDATE_OUTPUT}\n",
        steps.len(),
        id_to_text(repository.0),
        id_to_text(validation.0),
    );
    if let Some((project, revision)) = project {
        out.push_str(&format!("project: {project} revision {revision}\n"));
    }
    for ((step, id), provider) in draft.steps.iter().zip(steps).zip(providers) {
        let list = |paths: &[jojo_domain::validation::ScopePath]| {
            paths
                .iter()
                .map(|p| p.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };
        out.push_str(&format!(
            "\nstep {} ({}): {}\nprovider: {:?}\ndepends on: {}\nwrite scope: {}\n\
             baseline: {}\ncheck: {} = {}\n",
            step.key,
            id_to_text(*id),
            step.title,
            provider,
            if step.depends_on.is_empty() {
                "nothing".to_owned()
            } else {
                step.depends_on.join(", ")
            },
            list(&step.write_scope),
            list(&step.baseline),
            step.tests.join(", "),
            step.command.join(" "),
        ));
        if let Some(kind) = &step.kind {
            out.push_str(&format!("kind: {kind}\n"));
        }
    }
    out
}

/// M3: the project revision a plan turn was accepted under, if its repository has one.
fn turn_project(
    conn: &Connection,
    turn: OpaqueId,
) -> Result<Option<(OpaqueId, u64, ProjectRecord)>, StorageError> {
    let row = conn.query_optional(
        "SELECT project,revision FROM lead_plan_projects WHERE turn=?1",
        &[Value::Text(&id_to_text(turn))],
        |r| Ok((r.text(0)?, r.int(1)?)),
    )?;
    let Some((project, revision)) = row else {
        return Ok(None);
    };
    let (project, revision) = (id_from_text(&project)?, from_db(revision)?);
    let record = crate::projects::revision(conn, project, revision)?
        .ok_or_else(|| StorageError::corrupt("pinned project revision missing"))?;
    Ok(Some((project, revision, record)))
}

/// One turn as a chat page shows it, with the questions it asked.
fn chat_row(conn: &Connection, record: &TurnRecord) -> Result<Json, StorageError> {
    let questions = conn
        .query(
            "SELECT q.question_id, a.revision, q.prompt, a.state,
                    (SELECT answer.body FROM clarification_answers answer
                     WHERE answer.question_id=q.question_id ORDER BY answer.ordinal DESC LIMIT 1),
                    (SELECT answer.author FROM clarification_answers answer
                     WHERE answer.question_id=q.question_id ORDER BY answer.ordinal DESC LIMIT 1)
             FROM lead_turn_questions l
             JOIN clarification_questions q ON q.question_id=l.question
             JOIN aggregates a ON a.kind=q.question_kind AND a.id=q.question_id
             WHERE l.turn=?1 ORDER BY q.created_event_seq",
            &[Value::Text(&id_to_text(record.turn))],
            |r| {
                Ok((
                    r.text(0)?,
                    r.int(1)?,
                    r.text(2)?,
                    r.text(3)?,
                    r.optional_text(4)?,
                    r.optional_text(5)?,
                ))
            },
        )?
        .into_iter()
        .map(|(id, revision, prompt, state, answer, author)| {
            let (answer, complete) = match &answer {
                Some(body) => excerpt(body),
                None => (Json::Null, Json::Bool(true)),
            };
            Ok(object([
                ("question", identity(id_from_text(&id)?)),
                ("revision", number(from_db(revision)?)),
                ("prompt", text(prompt)),
                ("blocking", Json::Bool(state == "BLOCKING")),
                ("answer", answer),
                ("answer_complete", complete),
                ("answered_by", author.map_or(Json::Null, text)),
                ("step", Json::Null),
            ]))
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
    let turn_id = Value::Text(&id_to_text(record.turn));
    let mut questions = questions;
    for (id, revision, prompt, state, answer, key) in conn.query(
        "SELECT q.question, a.revision, q.prompt, a.state,
                (SELECT answer.body FROM step_answers answer
                 WHERE answer.question=q.question ORDER BY answer.ordinal DESC LIMIT 1),
                s.key
         FROM step_questions q
         JOIN aggregates a ON a.kind='step_question' AND a.id=q.question
         JOIN lead_plan_steps s ON s.step=q.step
         WHERE q.turn=?1 ORDER BY q.event",
        std::slice::from_ref(&turn_id),
        |r| {
            Ok((
                r.text(0)?,
                r.int(1)?,
                r.text(2)?,
                r.text(3)?,
                r.optional_text(4)?,
                r.text(5)?,
            ))
        },
    )? {
        let answered = answer.is_some();
        let (answer, complete) = match &answer {
            Some(body) => excerpt(body),
            None => (Json::Null, Json::Bool(true)),
        };
        questions.push(object([
            ("question", identity(id_from_text(&id)?)),
            ("revision", number(from_db(revision)?)),
            ("prompt", text(prompt)),
            ("blocking", Json::Bool(state == "BLOCKING")),
            ("answer", answer),
            ("answer_complete", complete),
            (
                "answered_by",
                if answered { text("HUMAN") } else { Json::Null },
            ),
            ("step", text(key)),
        ]));
    }
    let subjects = conn.query_optional(
        "SELECT adr,plan,validation,
                EXISTS(SELECT 1 FROM execution_plans p WHERE p.plan=s.plan)
         FROM lead_plan_subjects s WHERE s.turn=?1",
        std::slice::from_ref(&turn_id),
        |r| Ok((r.text(0)?, r.text(1)?, r.text(2)?, r.int(3)?)),
    )?;
    let plan = match subjects {
        None => Json::Null,
        Some((adr, plan, validation, stored)) => {
            let steps = conn
                .query(
                    "SELECT step,key,title FROM lead_plan_steps WHERE turn=?1 ORDER BY ordinal",
                    std::slice::from_ref(&turn_id),
                    |r| Ok((r.text(0)?, r.text(1)?, r.text(2)?)),
                )?
                .into_iter()
                .map(|(step, key, title)| {
                    Ok(object([
                        ("step", identity(id_from_text(&step)?)),
                        ("key", text(key)),
                        ("title", text(title)),
                    ]))
                })
                .collect::<Result<Vec<_>, StorageError>>()?;
            object([
                ("adr", identity(id_from_text(&adr)?)),
                ("plan", identity(id_from_text(&plan)?)),
                ("validation", identity(id_from_text(&validation)?)),
                ("drafted", Json::Bool(stored != 0)),
                ("steps", Json::List(steps)),
            ])
        }
    };
    let (message, message_complete) = excerpt(&record.message);
    let (state, detail, summary, prd) = match &record.result {
        None if record.stop_requested => ("Stopping", String::new(), String::new(), Json::Null),
        None => ("Running", String::new(), String::new(), Json::Null),
        Some(result) => (
            match result.outcome {
                TurnOutcome::Replied => "Replied",
                TurnOutcome::Failed => "Failed",
                TurnOutcome::Cancelled => "Cancelled",
                TurnOutcome::Interrupted => "Interrupted",
            },
            result.detail.clone(),
            result.summary.clone(),
            result.prd.map_or(Json::Null, |(prd, revision)| {
                object([
                    ("document", identity(prd.0)),
                    ("revision", number(revision.0)),
                ])
            }),
        ),
    };
    Ok(object([
        ("turn", identity(record.turn)),
        ("ordinal", number(record.ordinal)),
        ("kind", text(record.kind.name())),
        ("provider", text(provider_text(record.provider))),
        ("base", text(record.base.clone())),
        ("message", message),
        ("message_complete", message_complete),
        ("state", text(state)),
        ("detail", text(detail)),
        ("summary", text(summary)),
        ("prd", prd),
        ("plan", plan),
        ("questions", Json::List(questions)),
    ]))
}

fn unfinished(conn: &Connection) -> Result<Option<String>, StorageError> {
    Ok(conn.query_optional(
        "SELECT t.turn FROM lead_turns t
         WHERE NOT EXISTS(SELECT 1 FROM lead_turn_results r WHERE r.turn=t.turn) LIMIT 1",
        &[],
        |r| r.text(0),
    )?)
}

/// The feature-level questions of a feature, oldest first, each with its newest answer
/// when a human gave it.
fn exchanges(conn: &Connection, feature: FeatureId) -> Result<Vec<Exchange>, StorageError> {
    Ok(conn
        .query(
            "SELECT q.prompt, a.state,
                    (SELECT CASE WHEN answer.author='HUMAN' THEN answer.body END
                     FROM clarification_answers answer WHERE answer.question_id=q.question_id
                     ORDER BY answer.ordinal DESC LIMIT 1)
             FROM clarification_questions q
             JOIN aggregates a ON a.kind=q.question_kind AND a.id=q.question_id
             WHERE q.feature_id=?1 ORDER BY q.created_event_seq, q.question_id",
            &[Value::Text(&id_to_text(feature.0))],
            |r| Ok((r.text(0)?, r.text(1)?, r.optional_text(2)?)),
        )?
        .into_iter()
        .map(|(prompt, state, answer)| Exchange {
            prompt,
            blocking: state == "BLOCKING",
            answer,
        })
        .collect())
}

/// The one PRD of a feature, if it has one. A feature with several is not a chat feature.
fn feature_prd(conn: &Connection, feature: FeatureId) -> Result<Option<ArtifactId>, TxError> {
    let found = conn.query(
        "SELECT DISTINCT artifact_id FROM artifact_versions WHERE feature_id=?1 LIMIT 2",
        &[Value::Text(&id_to_text(feature.0))],
        |r| r.text(0),
    )?;
    match found.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(ArtifactId(id_from_text(one)?))),
        _ => Err(deny(
            "TurnRefused: the feature has more than one requirements document",
        )),
    }
}

impl Tx<'_> {
    /// The durable acceptance of one turn. Re-reads admission and capacity inside the
    /// writer transaction, composes the prompt from stored content and keeps it with the
    /// turn. Nothing is launched here. `base` is the commit the repository's HEAD names.
    pub fn accept_turn(&mut self, request: &TurnRequest, base: &str) -> Result<String, TxError> {
        if self.admission()? != Admission::Open {
            return Err(deny("AdmissionClosed"));
        }
        // One turn at a time; a turn takes one unit of the global capacity (D4).
        if unfinished(self.conn)?.is_some() {
            return Err(deny("TurnRefused: another lead turn is running"));
        }
        self.capacity_left("a lead turn")?;
        let repository = id_to_text(request.repository.0);
        if self
            .conn
            .query_optional(
                "SELECT 1 FROM local_repositories WHERE id=?1",
                &[Value::Text(&repository)],
                |r| r.int(0),
            )?
            .is_none()
        {
            return Err(deny("TurnRefused: the repository is not registered"));
        }
        let message = request.message.as_str();
        if message.len() > MAX_HUMAN_TEXT_BYTES || message.contains('\0') {
            return Err(deny("TurnRefused: the message is too long or contains NUL"));
        }
        let earlier = feature_turns(self.conn, request.feature)?;
        let (objective, note) = match earlier.first() {
            None => {
                if request.kind != TurnKind::Discover {
                    return Err(deny("TurnRefused: a feature begins with a discovery turn"));
                }
                if !message.chars().any(char::is_alphanumeric) {
                    return Err(deny("TurnRefused: the first turn needs an objective"));
                }
                (message.to_owned(), None)
            }
            Some(first) => {
                if first.repository != request.repository {
                    return Err(deny(
                        "TurnRefused: the feature belongs to another repository",
                    ));
                }
                let note = message
                    .chars()
                    .any(char::is_alphanumeric)
                    .then_some(message);
                (first.message.clone(), note)
            }
        };
        let list = exchanges(self.conn, request.feature)?;
        // M3: what the project says goes into every turn; a plan turn keeps the revision.
        let project = crate::projects::of_repository(self.conn, request.repository)?;
        let section = project
            .as_ref()
            .map(|(_, _, record)| record.prompt_section());
        let prompt = match request.kind {
            TurnKind::Discover => {
                let draft = match feature_prd(self.conn, request.feature)? {
                    Some(prd) => version(self.conn, prd, None)?.map(|v| v.content),
                    None => None,
                };
                discover_prompt(
                    &objective,
                    &list,
                    draft.as_deref(),
                    note,
                    section.as_deref(),
                )
            }
            TurnKind::Plan => {
                let approved = match feature_prd(self.conn, request.feature)? {
                    Some(prd) if crate::approvals::applicable(self.conn, prd)?.is_some() => {
                        version(self.conn, prd, None)?
                    }
                    _ => None,
                };
                let Some(prd) = approved else {
                    return Err(deny(
                        "TurnRefused: a plan turn needs the approved requirements document",
                    ));
                };
                plan_prompt(
                    &objective,
                    &list,
                    prd.revision.0,
                    &prd.rendered,
                    note,
                    section.as_deref(),
                )
            }
        };
        if prompt.len() > MAX_PROMPT_BYTES {
            return Err(deny(
                "PromptTooLarge: the stored content exceeds the prompt limit",
            ));
        }
        let id = id_to_text(request.turn);
        let feature = id_to_text(request.feature.0);
        let event = self.append_event(
            "LeadTurnAccepted",
            None,
            &format!(
                "{id}|feature={feature}|{}|{}",
                request.kind.name(),
                provider_text(request.provider)
            ),
        )?;
        self.conn.execute(
            "INSERT INTO lead_turns VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            &[
                Value::Text(&id),
                Value::Text(&feature),
                Value::Int(to_db(earlier.len() as u64 + 1)?),
                Value::Text(request.kind.name()),
                Value::Text(&repository),
                Value::Text(base),
                Value::Text(provider_text(request.provider)),
                Value::Text(message),
                Value::Text(&prompt),
                Value::Text(&sha256_hex(&[prompt.as_bytes()])),
                Value::Int(to_db(self.epoch.0)?),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        if let (TurnKind::Plan, Some((project, revision, _))) = (request.kind, &project) {
            self.conn.execute(
                "INSERT INTO lead_plan_projects VALUES (?1,?2,?3)",
                &[
                    Value::Text(&id),
                    Value::Text(&id_to_text(*project)),
                    Value::Int(to_db(*revision)?),
                ],
            )?;
        }
        Ok(format!("accepted turn={id}"))
    }

    /// The human's stop of a running turn. The runner sees it within one poll interval.
    pub fn request_turn_stop(&mut self, id: OpaqueId) -> Result<(), TxError> {
        let Some(record) = turn(self.conn, id)? else {
            return Err(deny("UnknownTurn"));
        };
        if record.result.is_some() {
            return Err(deny("TurnFinished"));
        }
        if !record.stop_requested {
            let event = self.append_event("LeadTurnStopRequested", None, &id_to_text(id))?;
            self.conn.execute(
                "INSERT INTO lead_turn_stops VALUES (?1,?2)",
                &[Value::Text(&id_to_text(id)), Value::Int(to_db(event.0)?)],
            )?;
        }
        Ok(())
    }

    /// Seals a turn. A reply becomes questions and a PRD version in the same transaction,
    /// all authored by the model and none approved; any other outcome stores nothing but
    /// the outcome. `ids` are fresh identities for what the reply creates.
    pub fn finish_turn(
        &mut self,
        id: OpaqueId,
        outcome: TurnOutcome,
        detail: &str,
        reply: Option<(&str, &LeadReply)>,
        ids: &[OpaqueId],
    ) -> Result<String, TxError> {
        let Some(record) = turn(self.conn, id)? else {
            return Err(deny("UnknownTurn"));
        };
        if record.result.is_some() {
            return Err(deny("TurnFinished"));
        }
        if (outcome == TurnOutcome::Replied) != reply.is_some() {
            return Err(deny("a turn replied exactly when it has a reply"));
        }
        let text = id_to_text(id);
        let provenance = Provenance::new(Author::Model, format!("lead turn {text}"));
        let mut ids = ids.iter().copied();
        let mut next = || ids.next().ok_or_else(|| deny("reply identities exhausted"));
        let mut created = None;
        let mut summary = "";
        match reply {
            Some((_, LeadReply::Discovery(discovery))) if record.kind == TurnKind::Discover => {
                summary = &discovery.summary;
                let known = exchanges(self.conn, record.feature)?;
                for question in &discovery.questions {
                    // The lead was told not to repeat; a repeat is dropped, not stored twice.
                    if known.iter().any(|k| k.prompt == question.prompt) {
                        continue;
                    }
                    let question_id = QuestionId(next()?);
                    self.ask_question(
                        question_id,
                        record.feature,
                        None,
                        &question.prompt,
                        if question.blocking {
                            QuestionDisposition::Blocking
                        } else {
                            QuestionDisposition::Deferred
                        },
                        &provenance,
                    )?;
                    self.conn.execute(
                        "INSERT INTO lead_turn_questions VALUES (?1,?2)",
                        &[Value::Text(&id_to_text(question_id.0)), Value::Text(&text)],
                    )?;
                }
                if let Some(document) = &discovery.prd {
                    let prepared = PreparedPrd::from_document(document);
                    let existing = match feature_prd(self.conn, record.feature)? {
                        Some(prd) => version(self.conn, prd, None)?,
                        None => None,
                    };
                    created = match existing {
                        None => {
                            let prd = ArtifactId(next()?);
                            let v =
                                self.create_artifact(prd, record.feature, &prepared, &provenance)?;
                            Some((prd, v.revision))
                        }
                        // The same content again is not a new version.
                        Some(v) if v.content_digest == prepared.content_digest() => None,
                        Some(v) => {
                            let revised = self.revise_artifact(
                                v.artifact,
                                v.revision,
                                &prepared,
                                &provenance,
                            )?;
                            Some((v.artifact, revised.revision))
                        }
                    };
                }
            }
            Some((_, LeadReply::Plan(draft))) if record.kind == TurnKind::Plan => {
                summary = &draft.summary;
                let known = exchanges(self.conn, record.feature)?;
                let feature_questions: Vec<_> = draft
                    .questions
                    .iter()
                    .filter(|q| q.step.is_none() && !known.iter().any(|k| k.prompt == q.prompt))
                    .collect();
                if !feature_questions.is_empty() {
                    // A question about the whole feature is part of what the PRD approval
                    // binds: asking it makes that approval stale, so nothing is drafted on
                    // top of it. The next plan turn starts from the answers.
                    for question in feature_questions {
                        let question_id = QuestionId(next()?);
                        self.ask_question(
                            question_id,
                            record.feature,
                            None,
                            &question.prompt,
                            if question.blocking {
                                QuestionDisposition::Blocking
                            } else {
                                QuestionDisposition::Deferred
                            },
                            &provenance,
                        )?;
                        self.conn.execute(
                            "INSERT INTO lead_turn_questions VALUES (?1,?2)",
                            &[Value::Text(&id_to_text(question_id.0)), Value::Text(&text)],
                        )?;
                    }
                } else {
                    let prd = feature_prd(self.conn, record.feature)?
                        .filter(|prd| {
                            crate::approvals::applicable(self.conn, *prd).is_ok_and(|d| d.is_some())
                        })
                        .ok_or_else(|| deny("the requirements are no longer approved"))?;
                    // M3: every step must have a provider under the pinned project
                    // before anything is drafted; a reply that cannot is refused.
                    let project = turn_project(self.conn, id)?;
                    draft
                        .providers(project.as_ref().map(|(_, _, r)| r))
                        .map_err(deny)?;
                    let (adr, plan, validation) = (
                        ArtifactId(next()?),
                        ArtifactId(next()?),
                        ArtifactId(next()?),
                    );
                    self.conn.execute(
                        "INSERT INTO lead_plan_subjects VALUES (?1,?2,?3,?4)",
                        &[
                            Value::Text(&text),
                            Value::Text(&id_to_text(adr.0)),
                            Value::Text(&id_to_text(plan.0)),
                            Value::Text(&id_to_text(validation.0)),
                        ],
                    )?;
                    let mut steps = Vec::new();
                    for (ordinal, step) in draft.steps.iter().enumerate() {
                        let id = next()?;
                        steps.push((step.key.clone(), id));
                        self.conn.execute(
                            "INSERT INTO lead_plan_steps VALUES (?1,?2,?3,?4,?5)",
                            &[
                                Value::Text(&text),
                                Value::Int(to_db(ordinal as u64 + 1)?),
                                Value::Text(&id_to_text(id)),
                                Value::Text(&step.key),
                                Value::Text(&step.title),
                            ],
                        )?;
                    }
                    for question in draft.questions.iter().filter(|q| q.step.is_some()) {
                        let step = steps
                            .iter()
                            .find(|(key, _)| Some(key) == question.step.as_ref())
                            .map(|(_, id)| *id)
                            .ok_or_else(|| deny("a step question names no step"))?;
                        let question_id = next()?;
                        let key = AggregateKey::new(STEP_QUESTION_KIND, question_id);
                        let disposition = if question.blocking {
                            QuestionDisposition::Blocking
                        } else {
                            QuestionDisposition::Deferred
                        };
                        let revision = self.create_aggregate(&key, disposition.as_db())?;
                        let event = self.append_event(
                            "StepQuestionAsked",
                            Some((&key, revision)),
                            &id_to_text(step),
                        )?;
                        self.conn.execute(
                            "INSERT INTO step_questions VALUES (?1,?2,?3,?4,?5,?6)",
                            &[
                                Value::Text(&id_to_text(question_id)),
                                Value::Text(&text),
                                Value::Text(&id_to_text(step)),
                                Value::Text(&question.prompt),
                                Value::Int(i64::from(question.blocking)),
                                Value::Int(to_db(event.0)?),
                            ],
                        )?;
                    }
                    // The ADR is a draft at once; the plan and its validation follow when
                    // the human has approved the ADR (`advance_lead_plans`).
                    // The project's instructions are appended by the core, not the model,
                    // so that they are approved with the ADR and reach every attempt.
                    let mut body = draft.adr.clone();
                    if let Some((project, revision, record)) = &project {
                        body.push_str(&record.adr_section(&id_to_text(*project), *revision));
                    }
                    self.put_review_document(adr, None, prd, None, &body, &body)?;
                }
            }
            Some(_) => return Err(deny("the reply is not of the turn's kind")),
            None => {}
        }
        let mut detail = detail.to_owned();
        if detail.len() > MAX_DETAIL_BYTES {
            let mut end = MAX_DETAIL_BYTES;
            while !detail.is_char_boundary(end) {
                end -= 1;
            }
            detail.truncate(end);
        }
        let event = self.append_event(
            "LeadTurnFinished",
            None,
            &format!("{text}|{}", outcome.as_db()),
        )?;
        let prd = created.map(|(prd, _)| id_to_text(prd.0));
        self.conn.execute(
            "INSERT INTO lead_turn_results VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            &[
                Value::Text(&text),
                Value::Text(outcome.as_db()),
                Value::Text(&detail),
                Value::Text(summary),
                Value::Text(reply.map_or("", |(raw, _)| raw)),
                prd.as_deref().map_or(Value::Null, Value::Text),
                match created {
                    Some((_, revision)) => Value::Int(to_db(revision.0)?),
                    None => Value::Null,
                },
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(outcome.as_db().to_owned())
    }

    /// M2: the human's answer to, or disposition of, one question a lead turn asked, at
    /// the revision the human was shown. Questions not asked in a Feature Chat are not
    /// reachable here.
    pub fn answer_chat_question(
        &mut self,
        question: QuestionId,
        revision: Revision,
        action: &QuestionAction,
    ) -> Result<String, TxError> {
        let id = Value::Text(&id_to_text(question.0));
        let step_question = self
            .conn
            .query_optional(
                "SELECT 1 FROM step_questions WHERE question=?1",
                std::slice::from_ref(&id),
                |r| r.int(0),
            )?
            .is_some();
        if step_question {
            return self.answer_step_question(question.0, revision, action);
        }
        if self
            .conn
            .query_optional(
                "SELECT 1 FROM lead_turn_questions WHERE question=?1",
                &[id],
                |r| r.int(0),
            )?
            .is_none()
        {
            return Err(deny("UnknownQuestion: not asked in a Feature Chat"));
        }
        let revision = match action {
            QuestionAction::Answer(body) => {
                let human = Provenance::new(Author::Human, "answered in Feature Chat");
                self.answer_question(question, revision, body, &human)?
            }
            QuestionAction::Defer => {
                self.set_question_disposition(question, revision, QuestionDisposition::Deferred)?
            }
            QuestionAction::Block => {
                self.set_question_disposition(question, revision, QuestionDisposition::Blocking)?
            }
        };
        Ok(format!("question revision={}", revision.0))
    }

    /// A question about one step: the same three actions, on its own aggregate. It never
    /// touches what an approval binds; it only decides whether that step may start.
    fn answer_step_question(
        &mut self,
        question: OpaqueId,
        expected: Revision,
        action: &QuestionAction,
    ) -> Result<String, TxError> {
        let key = AggregateKey::new(STEP_QUESTION_KIND, question);
        let state = self
            .aggregate(&key)?
            .map(|record| record.state)
            .ok_or_else(|| deny("UnknownQuestion"))?;
        let (state, kind) = match action {
            QuestionAction::Answer(body) => {
                if body.len() > crate::artifacts::MAX_TEXT_BYTES
                    || body.contains('\0')
                    || !body.chars().any(char::is_alphanumeric)
                {
                    return Err(deny("answer is empty or too long"));
                }
                (state, "StepQuestionAnswered")
            }
            QuestionAction::Defer => (
                QuestionDisposition::Deferred.as_db().to_owned(),
                "StepQuestionDispositionChanged",
            ),
            QuestionAction::Block => (
                QuestionDisposition::Blocking.as_db().to_owned(),
                "StepQuestionDispositionChanged",
            ),
        };
        let revision = self.update_aggregate(&key, expected, &state)?;
        let event = self.append_event(kind, Some((&key, revision)), &state)?;
        if let QuestionAction::Answer(body) = action {
            self.conn.execute(
                "INSERT INTO step_answers
                 SELECT ?1, coalesce(max(ordinal),0)+1, ?2, ?3 FROM step_answers WHERE question=?1",
                &[
                    Value::Text(&id_to_text(question)),
                    Value::Text(body),
                    Value::Int(to_db(event.0)?),
                ],
            )?;
        }
        Ok(format!("question revision={}", revision.0))
    }

    /// Turns each lead plan draft whose ADR the human has approved into its validation
    /// plan and execution plan, both unapproved drafts. Returns how many were advanced.
    /// Called after a human decision; a draft that is complete is left alone.
    pub fn advance_lead_plans(&mut self) -> Result<u64, TxError> {
        let pending = self.conn.query(
            "SELECT s.turn,s.adr,s.plan,s.validation FROM lead_plan_subjects s
             WHERE NOT EXISTS(SELECT 1 FROM execution_plans p WHERE p.plan=s.plan)",
            &[],
            |r| Ok((r.text(0)?, r.text(1)?, r.text(2)?, r.text(3)?)),
        )?;
        let mut advanced = 0;
        for (turn_text, adr, plan, validation) in pending {
            let (adr, plan, validation) = (
                ArtifactId(id_from_text(&adr)?),
                ArtifactId(id_from_text(&plan)?),
                ArtifactId(id_from_text(&validation)?),
            );
            let Some(adr_approval) = self.current_artifact_approval(adr)? else {
                continue;
            };
            let record = turn(self.conn, id_from_text(&turn_text)?)?
                .ok_or_else(|| deny("plan draft without its turn"))?;
            let Some(prd) = feature_prd(self.conn, record.feature)? else {
                continue;
            };
            let Some(prd_approval) = self.current_artifact_approval(prd)? else {
                continue;
            };
            let reply = self.conn.query_one(
                "SELECT reply FROM lead_turn_results WHERE turn=?1",
                &[Value::Text(&turn_text)],
                |r| r.text(0),
            )?;
            let Ok(LeadReply::Plan(draft)) = planner::parse_reply(TurnKind::Plan, &reply) else {
                return Err(deny("stored plan reply no longer reads as a plan"));
            };
            let steps: Vec<OpaqueId> = self
                .conn
                .query(
                    "SELECT step FROM lead_plan_steps WHERE turn=?1 ORDER BY ordinal",
                    &[Value::Text(&turn_text)],
                    |r| r.text(0),
                )?
                .iter()
                .map(|id| id_from_text(id))
                .collect::<Result<_, _>>()?;
            let project = turn_project(self.conn, record.turn)?;
            let providers = draft
                .providers(project.as_ref().map(|(_, _, r)| r))
                .map_err(deny)?;
            let checks = draft.validation(&steps).map_err(deny)?;
            let canonical = checks.canonical();
            self.put_review_document(validation, None, prd, Some(adr), &canonical, &canonical)?;
            let built = lead_plan(
                &draft,
                &steps,
                record.repository,
                &record.base,
                &canonical,
                vec![prd_approval, adr_approval],
                &providers,
            );
            let pinned = project.as_ref().map(|(p, r, _)| (id_to_text(*p), *r));
            let shown = plan_text(
                &draft,
                &steps,
                record.repository,
                &record.base,
                validation,
                &providers,
                pinned.as_ref().map(|(p, r)| (p.as_str(), *r)),
            );
            self.put_execution_plan(plan, None, prd, adr, &built, &shown)?;
            advanced += 1;
        }
        Ok(advanced)
    }

    /// Closes every turn an earlier helper left unfinished. Such a turn held no worktree,
    /// claim or effect; its process was the earlier helper's to stop.
    pub fn close_interrupted_turns(&mut self) -> Result<u64, TxError> {
        let stale = self.conn.query(
            "SELECT t.turn FROM lead_turns t WHERE t.epoch<?1
             AND NOT EXISTS(SELECT 1 FROM lead_turn_results r WHERE r.turn=t.turn)",
            &[Value::Int(to_db(self.epoch.0)?)],
            |r| r.text(0),
        )?;
        for id in &stale {
            self.finish_turn(
                id_from_text(id)?,
                TurnOutcome::Interrupted,
                "the helper that ran this turn ended before the turn did",
                None,
                &[],
            )?;
        }
        Ok(stale.len() as u64)
    }
}

impl ReadView<'_> {
    pub fn turn(&self, id: OpaqueId) -> Result<Option<TurnRecord>, StorageError> {
        turn(self.conn, id)
    }

    /// Every turn of a feature, oldest first.
    pub fn feature_turns(&self, feature: FeatureId) -> Result<Vec<TurnRecord>, StorageError> {
        feature_turns(self.conn, feature)
    }

    /// The prompt a turn was accepted with, exactly as it is written to standard input.
    pub fn turn_prompt(&self, id: OpaqueId) -> Result<Option<String>, StorageError> {
        Ok(self.conn.query_optional(
            "SELECT prompt FROM lead_turns WHERE turn=?1",
            &[Value::Text(&id_to_text(id))],
            |r| r.text(0),
        )?)
    }

    /// The path a repository was registered with.
    pub fn local_repository_path(&self, id: RepositoryId) -> Result<Option<PathBuf>, StorageError> {
        Ok(self
            .conn
            .query_optional(
                "SELECT path FROM local_repositories WHERE id=?1",
                &[Value::Text(&id_to_text(id.0))],
                |r| r.text(0),
            )?
            .map(PathBuf::from))
    }

    /// M2 (protocol 1.2): one page of a feature's chat. Turns after the ordinal `after`,
    /// oldest first, as many as fit in a bounded page (at least one); each with the
    /// questions it asked and their newest answers. Long texts are cut and say so; the
    /// whole text is read with [`Self::ipc_chat_text`]. Also the feature's requirements
    /// document as the core stores it, and the turn that is running, if any.
    pub fn ipc_feature_chat(&self, feature: FeatureId, after: u64) -> Result<Json, StorageError> {
        let turns = feature_turns(self.conn, feature)?;
        let mut rows = Vec::new();
        let mut used = 0usize;
        let mut next = Json::Null;
        for record in turns.iter().filter(|t| t.ordinal > after) {
            let row = chat_row(self.conn, record)?;
            let size = jojo_domain::ipc::encode(&row).len();
            if !rows.is_empty() && used + size > CHAT_PAGE_BYTES {
                next = number(record.ordinal - 1);
                break;
            }
            used += size;
            rows.push(row);
        }
        let running = turns
            .iter()
            .find(|t| t.result.is_none())
            .map_or(Json::Null, |t| {
                object([
                    ("turn", identity(t.turn)),
                    ("stop_requested", Json::Bool(t.stop_requested)),
                ])
            });
        let prd = match feature_prd(self.conn, feature).map_err(|e| match e {
            TxError::Storage(e) => e,
            other => StorageError::corrupt(format!("{other:?}")),
        })? {
            None => Json::Null,
            Some(prd) => {
                let head = version(self.conn, prd, None)?
                    .ok_or_else(|| StorageError::corrupt("document without a version"))?;
                let readiness = crate::artifacts::review_readiness(self.conn, prd)?
                    .ok_or_else(|| StorageError::corrupt("document without readiness"))?;
                let approved = crate::approvals::applicable(self.conn, prd)?.is_some();
                object([
                    ("document", identity(prd.0)),
                    ("revision", number(head.revision.0)),
                    ("author", text(head.provenance.author.as_db())),
                    ("ready", Json::Bool(readiness.is_ready())),
                    ("blockers", number(readiness.blockers.len() as u64)),
                    ("approved", Json::Bool(approved)),
                    ("length", number(head.rendered.chars().count() as u64)),
                ])
            }
        };
        Ok(object([
            ("feature", identity(feature.0)),
            ("turns", Json::List(rows)),
            ("next", next),
            ("running", running),
            ("prd", prd),
        ]))
    }

    /// M2 (protocol 1.2): one chunk of a long chat text from a character offset, or
    /// `null` when the subject has no such text.
    pub fn ipc_chat_text(
        &self,
        subject: OpaqueId,
        part: ChatPart,
        offset: u64,
    ) -> Result<Json, StorageError> {
        let id = Value::Text(&id_to_text(subject));
        let body = match part {
            ChatPart::Message | ChatPart::Prompt => self.conn.query_optional(
                if part == ChatPart::Message {
                    "SELECT message FROM lead_turns WHERE turn=?1"
                } else {
                    "SELECT prompt FROM lead_turns WHERE turn=?1"
                },
                &[id],
                |r| r.text(0),
            )?,
            ChatPart::Reply => self.conn.query_optional(
                "SELECT reply FROM lead_turn_results WHERE turn=?1 AND reply<>''",
                &[id],
                |r| r.text(0),
            )?,
            ChatPart::Answer => self.conn.query_optional(
                "SELECT body FROM (
                   SELECT a.body, a.ordinal FROM clarification_answers a
                   JOIN lead_turn_questions l ON l.question=a.question_id WHERE a.question_id=?1
                   UNION ALL
                   SELECT body, ordinal FROM step_answers WHERE question=?1)
                 ORDER BY ordinal DESC LIMIT 1",
                &[id],
                |r| r.text(0),
            )?,
            ChatPart::Prd => version(self.conn, ArtifactId(subject), None)?.map(|v| v.rendered),
        };
        let Some(body) = body else {
            return Ok(Json::Null);
        };
        let total = body.chars().count() as u64;
        let chunk: String = body
            .chars()
            .skip(usize::try_from(offset).unwrap_or(usize::MAX))
            .take(CHAT_TEXT_CHARS)
            .collect();
        let next = offset.saturating_add(chunk.chars().count() as u64);
        Ok(object([
            ("subject", identity(subject)),
            ("offset", number(offset)),
            ("next", number(next)),
            ("total", number(total)),
            ("complete", Json::Bool(next >= total)),
            ("text", text(chunk)),
        ]))
    }

    /// M2 (protocol 1.2): what a client needs to find its way to a chat. The registered
    /// repositories, in identity order, and the features that have turns, newest first,
    /// each list bounded and saying whether it is complete. Read-only.
    pub fn ipc_chat_index(&self) -> Result<Json, StorageError> {
        let repositories = self.conn.query(
            "SELECT id,path FROM local_repositories ORDER BY id LIMIT ?1",
            &[Value::Int(to_db(INDEX_ROWS as u64 + 1)?)],
            |r| Ok((r.text(0)?, r.text(1)?)),
        )?;
        let features = self.conn.query(
            "SELECT t.feature, max(t.event) AS latest FROM lead_turns t
             GROUP BY t.feature ORDER BY latest DESC LIMIT ?1",
            &[Value::Int(to_db(INDEX_ROWS as u64 + 1)?)],
            |r| r.text(0),
        )?;
        let repository_rows = repositories
            .iter()
            .take(INDEX_ROWS)
            .map(|(id, path)| {
                let cut: String = path.chars().take(INDEX_TEXT_CHARS).collect();
                Ok(object([
                    ("repository", identity(id_from_text(id)?)),
                    ("path_complete", Json::Bool(cut.len() == path.len())),
                    ("path", text(cut)),
                ]))
            })
            .collect::<Result<Vec<_>, StorageError>>()?;
        let mut feature_rows = Vec::new();
        for feature in features.iter().take(INDEX_ROWS) {
            let turns = feature_turns(self.conn, FeatureId(id_from_text(feature)?))?;
            let (Some(first), Some(last)) = (turns.first(), turns.last()) else {
                continue;
            };
            let cut: String = first.message.chars().take(INDEX_TEXT_CHARS).collect();
            feature_rows.push(object([
                ("feature", identity(first.feature.0)),
                ("repository", identity(first.repository.0)),
                (
                    "objective_complete",
                    Json::Bool(cut.len() == first.message.len()),
                ),
                ("objective", text(cut)),
                ("turns", number(turns.len() as u64)),
                ("running", Json::Bool(last.result.is_none())),
            ]));
        }
        Ok(object([
            ("repositories", Json::List(repository_rows)),
            (
                "repositories_complete",
                Json::Bool(repositories.len() <= INDEX_ROWS),
            ),
            ("features", Json::List(feature_rows)),
            (
                "features_complete",
                Json::Bool(features.len() <= INDEX_ROWS),
            ),
        ]))
    }

    /// Whether a lead plan draft waits for its ADR approval to become a plan.
    pub fn lead_plans_pending(&self) -> Result<bool, StorageError> {
        Ok(self
            .conn
            .query_optional(
                "SELECT 1 FROM lead_plan_subjects s
                 WHERE NOT EXISTS(SELECT 1 FROM execution_plans p WHERE p.plan=s.plan) LIMIT 1",
                &[],
                |r| r.int(0),
            )?
            .is_some())
    }

    /// The one turn that has no result yet, if there is one.
    pub fn unfinished_turn(&self) -> Result<Option<OpaqueId>, StorageError> {
        unfinished(self.conn)?
            .map(|id| id_from_text(&id))
            .transpose()
    }
}

/// The private directory of one turn's raw streams, beside the database.
fn turn_directory(engine: &Engine, id: OpaqueId) -> Result<PathBuf, String> {
    let data = engine
        .db_path()
        .parent()
        .ok_or("database has no data root")?;
    let turns = data.join("turns");
    let private = |path: &std::path::Path| {
        std::fs::DirBuilder::new()
            .recursive(false)
            .mode(0o700)
            .create(path)
    };
    if !turns.is_dir() {
        private(&turns).map_err(|e| format!("turn directory: {e}"))?;
    }
    let directory = turns.join(id_to_text(id));
    private(&directory).map_err(|e| format!("turn directory: {e}"))?;
    Ok(directory)
}

/// The frozen request of an accepted turn. Every field is read from durable state or
/// from the adapter's fingerprint; creates the turn's private output directory.
pub fn turn_spec(
    engine: &Engine,
    id: OpaqueId,
    repository: &crate::LocalRepository,
    fingerprint: &ProviderFingerprint,
    variables: Vec<String>,
) -> Result<TurnSpec, String> {
    let mut reader = engine.reader().map_err(|e| e.to_string())?;
    let (record, prompt) = reader
        .read(|v| Ok((v.turn(id)?, v.turn_prompt(id)?)))
        .map_err(|e| e.to_string())?;
    let (Some(record), Some(prompt)) = (record, prompt) else {
        return Err("accepted turn is missing".into());
    };
    if record.repository != repository.id() || record.provider != fingerprint.provider {
        return Err("turn repository or provider differs from the accepted turn".into());
    }
    if sha256_hex(&[prompt.as_bytes()]) != record.prompt_digest {
        return Err("stored prompt differs from its digest".into());
    }
    Ok(TurnSpec {
        turn: id,
        kind: record.kind,
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
    })
}

/// How the supervised run of a turn ended.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TurnConclusion {
    /// No child existed.
    NotSpawned(String),
    /// Root reaped, group and known descendants absent, both streams closed.
    TreeAbsent,
    /// Some owned or potentially owned process could not be confirmed stopped.
    StopUnconfirmed(Vec<String>),
}

#[derive(Clone, Debug)]
pub struct TurnRun {
    pub conclusion: TurnConclusion,
    /// Why a stop was requested, if one was.
    pub stop: Option<String>,
    /// The human asked for the stop.
    pub cancelled: bool,
    pub exit: Option<ExitObservation>,
    pub terminal: Option<TerminalStatus>,
    pub protocol_failed: bool,
    pub evidence_lost: bool,
    /// The last message on standard output.
    pub reply: Option<String>,
}

fn command<F>(ctx: &LaunchContext<'_>, kind: &str, id: OpaqueId, f: F) -> Result<String, String>
where
    F: FnOnce(&mut Tx<'_>) -> Result<String, TxError> + Send + 'static,
{
    let command = ctx
        .ids
        .next_id()
        .map_err(|_| "command ids exhausted".to_owned())?;
    let request = CommandRequest::new(CommandId(command), ctx.engine.epoch(), kind, id_to_text(id));
    match ctx.engine.execute(request, f) {
        Ok(r) => Ok(r.response),
        Err(CommandError::Rejected(TxError::Rejected(why))) => Err(why),
        Err(e) => Err(format!("{e:?}")),
    }
}

fn nanos(d: std::time::Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

/// Runs one accepted turn from spawn to a concluded process tree. The loop is the one
/// that supervises an attempt (`launch.rs`), without its journal: a stop is requested on
/// the deadline, a human stop, a protocol failure, lost evidence or a descendant that
/// outlived the root, and is escalated through `plan` until the tree is proven absent.
pub fn run_turn(
    ctx: &LaunchContext<'_>,
    spec: &TurnSpec,
    launch: &ProcessLaunch,
    plan: &StopPlan,
    hooks: LaunchHooks,
) -> TurnRun {
    let id = spec.turn;
    let mut run = TurnRun {
        conclusion: TurnConclusion::NotSpawned(String::new()),
        stop: None,
        cancelled: false,
        exit: None,
        terminal: None,
        protocol_failed: false,
        evidence_lost: false,
        reply: None,
    };
    let emergency = || {
        hooks
            .emergency_stop
            .as_ref()
            .is_some_and(|s| s.load(Ordering::SeqCst))
    };
    let stop_requested = |reader: &mut Option<crate::Reader>| -> Result<bool, ()> {
        if reader.is_none() {
            *reader = ctx.engine.reader().ok();
        }
        let Some(r) = reader.as_mut() else {
            return Err(());
        };
        let stopped = r.read(|v| Ok(v.turn(id)?.is_none_or(|t| t.stop_requested)));
        if stopped.is_err() {
            *reader = None;
        }
        stopped.map_err(drop)
    };
    let mut reader = None;
    if ProcessLaunch::for_turn(spec, launch.args.clone(), launch.environment.clone()).as_ref()
        != Ok(launch)
    {
        run.conclusion =
            TurnConclusion::NotSpawned("process request differs from the frozen turn".into());
        return run;
    }
    if emergency() || stop_requested(&mut reader) != Ok(false) {
        run.cancelled = true;
        run.conclusion = TurnConclusion::NotSpawned("stop requested before spawn".into());
        return run;
    }
    let mut sup = match Supervised::spawn_guarded(
        launch,
        hooks.spool,
        hooks
            .inspector
            .as_deref()
            .unwrap_or(std::path::Path::new("/bin/ps")),
        hooks.guard.as_deref(),
    ) {
        Ok(s) => s,
        Err(SpawnFailure::BeforeChild(d)) => {
            run.conclusion = TurnConclusion::NotSpawned(d);
            return run;
        }
        Err(SpawnFailure::Uncertain(d)) => {
            run.conclusion = TurnConclusion::StopUnconfirmed(vec![format!("spawn uncertain: {d}")]);
            return run;
        }
    };
    let root = sup.root().clone();
    let spawned = format!(
        "{}|pid={}|pgid={}|start={}",
        id_to_text(id),
        root.pid,
        root.pgid,
        root.start
    );
    // The one record of the run before its result: which process this turn is.
    let recorded = command(ctx, "RecordLeadTurnSpawn", id, move |tx| {
        tx.append_event("LeadTurnSpawned", None, &spawned)?;
        Ok("spawned".into())
    });
    let now = || ctx.clock.now().monotonic_nanos;
    let deadline = now().saturating_add(spec.timeout_millis.saturating_mul(1_000_000));
    let mut closed = [false, false];
    let mut root_terminated = false;
    let mut inspection_failed: Option<String> = None;
    let mut stopping: Option<(usize, u64)> = None;
    let mut drain_deadline: Option<u64> = None;
    let mut forced = false;
    let mut next_stop_check = 0u64;
    let mut stop_reason: Option<String> = recorded.err().map(|e| format!("evidence loss: {e}"));
    run.evidence_lost = stop_reason.is_some();
    if let Err(error) = sup.discover() {
        inspection_failed = Some(error.clone());
        stop_reason.get_or_insert(format!("process inspection failed: {error}"));
    }
    loop {
        match sup.messages().recv_timeout(ctx.poll) {
            Ok(ProcessMessage::Output { stream, events, .. }) => {
                for event in events {
                    if stream != Stream::Stdout {
                        continue;
                    }
                    match event.payload {
                        EventPayload::TextDelta(text) => run.reply = Some(text),
                        EventPayload::ProviderTerminal { status, .. } if run.terminal.is_none() => {
                            run.terminal = Some(status);
                        }
                        EventPayload::ProtocolViolation(why) => {
                            run.protocol_failed = true;
                            stop_reason.get_or_insert(format!("protocol failure: {why}"));
                        }
                        _ => {}
                    }
                }
            }
            Ok(ProcessMessage::Truncated { .. }) => {}
            Ok(ProcessMessage::Closed { stream, .. }) => {
                closed[usize::from(stream == Stream::Stderr)] = true;
            }
            Ok(ProcessMessage::SpoolFailed { detail, .. }) => {
                run.evidence_lost = true;
                stop_reason.get_or_insert(format!("evidence loss: {detail}"));
            }
            Ok(ProcessMessage::RootTerminated) => root_terminated = true,
            Ok(ProcessMessage::InspectionFailed(detail)) => {
                stop_reason.get_or_insert(format!("process inspection failed: {detail}"));
                inspection_failed = Some(detail);
            }
            Ok(ProcessMessage::Exited(exit)) => {
                run.exit.get_or_insert(exit);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => closed = [true, true],
        }
        if stopping.is_none() && stop_reason.is_none() {
            if now() >= deadline {
                stop_reason = Some("deadline exceeded".into());
            } else if emergency() {
                stop_reason = Some("the helper is closing".into());
            } else if now() >= next_stop_check {
                next_stop_check = now().saturating_add(nanos(ctx.poll));
                match stop_requested(&mut reader) {
                    Ok(false) => {}
                    Ok(true) => {
                        run.cancelled = true;
                        stop_reason = Some("stop requested".into());
                    }
                    Err(()) => stop_reason = Some("stop state unreadable".into()),
                }
            }
        }
        if stopping.is_none() && stop_reason.is_some() {
            stopping = Some((0, 0));
        }
        if run.exit.is_none() {
            run.exit = sup.root_exit();
        }
        let exited = run.exit.is_some();
        let streams_closed = closed == [true, true];
        if let Some((step, step_deadline)) = stopping.as_mut() {
            if let Some(detail) = &inspection_failed
                && *step > 0
                && streams_closed
            {
                run.conclusion =
                    TurnConclusion::StopUnconfirmed(vec![format!("inspection failed: {detail}")]);
                break;
            }
            if exited && streams_closed && sup.tree().absent() {
                run.conclusion = TurnConclusion::TreeAbsent;
                break;
            }
            if now() >= *step_deadline {
                let Some((signal, budget)) = plan.steps.get(*step).copied() else {
                    let tree = sup.tree();
                    let mut reasons = Vec::new();
                    if !exited {
                        reasons.push("root exit not observed".to_owned());
                    }
                    if !tree.live_group.is_empty() {
                        reasons.push(format!("group members alive: {:?}", tree.live_group));
                    }
                    if !tree.live_known.is_empty() {
                        reasons.push(format!("known descendants alive: {:?}", tree.live_known));
                    }
                    if !streams_closed {
                        reasons.push("output stream held open by an unknown process".into());
                    }
                    if let Some(e) = tree.error {
                        reasons.push(format!("inspection failed: {e}"));
                    }
                    run.conclusion = TurnConclusion::StopUnconfirmed(reasons);
                    break;
                };
                if let Err(error) = sup.discover() {
                    inspection_failed = Some(error);
                }
                forced |= sup
                    .signal_tree(signal)
                    .iter()
                    .any(|r| r.signal == Signal::Kill && r.outcome == SignalOutcome::Sent);
                *step += 1;
                *step_deadline = now().saturating_add(nanos(budget));
            }
            continue;
        }
        if root_terminated || exited {
            if drain_deadline.is_none() {
                if let Err(error) = sup.discover() {
                    inspection_failed = Some(error.clone());
                    stop_reason = Some(format!("process inspection failed: {error}"));
                    continue;
                }
                let tree = sup.tree();
                if !tree.live_group.is_empty() || !tree.live_known.is_empty() {
                    stop_reason = Some("descendants outlived the root".into());
                    continue;
                }
                drain_deadline = Some(now().saturating_add(nanos(plan.drain)));
            }
            if streams_closed {
                if sup.tree().absent() {
                    run.conclusion = TurnConclusion::TreeAbsent;
                    break;
                }
                if exited {
                    stop_reason = Some("descendants appeared after the root exited".into());
                }
            } else if drain_deadline.is_some_and(|d| now() >= d) {
                run.conclusion = TurnConclusion::StopUnconfirmed(vec![
                    "output stream held open by an unknown process".into(),
                ]);
                break;
            }
        }
    }
    if forced {
        stop_reason.get_or_insert("forced stop".into());
    }
    run.stop = stop_reason;
    run
}

/// Seals a concluded run. Only a clean, unstopped, error-free run whose last message is a
/// reply of the turn's schema replies; everything else stores an outcome and a reason.
/// A process that could not be confirmed stopped also closes admission.
pub fn settle_turn(
    ctx: &LaunchContext<'_>,
    spec: &TurnSpec,
    run: &TurnRun,
) -> Result<TurnOutcome, String> {
    let clean = run.conclusion == TurnConclusion::TreeAbsent
        && run.stop.is_none()
        && !run.protocol_failed
        && !run.evidence_lost
        && run.terminal == Some(TerminalStatus::Success)
        && run.exit.is_some_and(|e| e.code == Some(0));
    let (outcome, detail, reply) = match (&run.conclusion, clean) {
        (TurnConclusion::TreeAbsent, true) => match run.reply.as_deref() {
            None => (
                TurnOutcome::Failed,
                "the run ended without a message".to_owned(),
                None,
            ),
            Some(text) => match planner::parse_reply(spec.kind, text) {
                Ok(reply) => (
                    TurnOutcome::Replied,
                    String::new(),
                    Some((text.to_owned(), reply)),
                ),
                Err(error) => (
                    TurnOutcome::Failed,
                    format!("the last message is not a reply: {error:?}"),
                    None,
                ),
            },
        },
        (TurnConclusion::NotSpawned(why), _) if run.cancelled => {
            (TurnOutcome::Cancelled, why.clone(), None)
        }
        (TurnConclusion::NotSpawned(why), _) => {
            (TurnOutcome::Failed, format!("not spawned: {why}"), None)
        }
        (TurnConclusion::StopUnconfirmed(reasons), _) => (
            TurnOutcome::Failed,
            format!("stop unconfirmed: {}", reasons.join("; ")),
            None,
        ),
        (TurnConclusion::TreeAbsent, false) if run.cancelled => (
            TurnOutcome::Cancelled,
            "stopped by the human".to_owned(),
            None,
        ),
        (TurnConclusion::TreeAbsent, false) => (
            TurnOutcome::Failed,
            run.stop.clone().unwrap_or_else(|| {
                format!(
                    "the run did not end in success: terminal={:?} exit={:?}",
                    run.terminal, run.exit
                )
            }),
            None,
        ),
    };
    let ids: Vec<OpaqueId> = (0..TURN_REPLY_IDS)
        .map(|_| ctx.ids.next_id())
        .collect::<Result<_, _>>()
        .map_err(|_| "reply identities exhausted".to_owned())?;
    let unconfirmed = matches!(run.conclusion, TurnConclusion::StopUnconfirmed(_));
    let id = spec.turn;
    let applied = {
        let (detail, reply) = (detail.clone(), reply.clone());
        command(ctx, "FinishLeadTurn", id, move |tx| {
            if unconfirmed {
                tx.set_admission(Admission::Closed)?;
            }
            tx.finish_turn(
                id,
                outcome,
                &detail,
                reply.as_ref().map(|(raw, parsed)| (raw.as_str(), parsed)),
                &ids,
            )
        })
    };
    match applied {
        Ok(_) => Ok(outcome),
        // A reply the core refuses to store (for example a PRD above a bound) is a
        // failed turn, not a lost one.
        Err(why) if outcome == TurnOutcome::Replied => {
            let detail = format!("the reply could not be stored: {why}");
            command(ctx, "FinishLeadTurn", id, move |tx| {
                tx.finish_turn(id, TurnOutcome::Failed, &detail, None, &[])
            })?;
            Ok(TurnOutcome::Failed)
        }
        Err(why) => Err(why),
    }
}

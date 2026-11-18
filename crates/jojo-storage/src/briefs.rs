//! Step 7 (New Project): project briefs and the turns that write them (schema 17).
//!
//! A brief turn is a lead turn of its own, like a review: one read-only run in one
//! registered repository, accepted with its prompt, supervised by the same loop as a chat
//! turn (`turns::run_turn`) and sealed here. It belongs to a repository, not to a feature,
//! so it never appears in a Feature Chat. Its reply becomes the next immutable revision of
//! the repository's one brief, authored by the model and approved by nothing: the human
//! decides it like any document (`human_decisions`, subject = the brief). Only a brief
//! whose newest revision is approved is carried into later turns of that repository.
use crate::digest::sha256_hex;
use crate::engine::{Engine, ReadView};
use crate::error::{StorageError, TxError};
use crate::launch::LaunchContext;
use crate::model::{from_db, id_from_text, id_to_text, to_db};
use crate::overview::{optional, time_of};
use crate::sqlite::{Connection, Value};
use crate::turns::{
    TURN_MAX_FRAME_BYTES, TURN_MAX_OUTPUT_BYTES, TURN_TIMEOUT_MILLIS, TurnConclusion, TurnOutcome,
    TurnRun, command, turn_directory,
};
use crate::tx::Tx;
use crate::{Admission, LocalRepository};
use jojo_domain::brief::{Brief, brief_prompt, parse_brief};
use jojo_domain::identity::{ArtifactId, OpaqueId, RepositoryId};
use jojo_domain::ipc::{identity, object, text};
use jojo_domain::json::Json;
use jojo_domain::planner::{MAX_HUMAN_TEXT_BYTES, TurnKind, TurnSpec};
use jojo_domain::project::{provider_name, provider_named};
use jojo_domain::provider::{MAX_PROMPT_BYTES, ProviderFingerprint, ProviderKind, TerminalStatus};

fn deny(s: impl Into<String>) -> TxError {
    TxError::Rejected(s.into())
}

/// What the human asked for: a brief of one registered repository on one provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BriefRequest {
    /// The command identity: one command, one turn, at most one run.
    pub turn: OpaqueId,
    /// The identity a repository's first brief gets; later turns revise that brief.
    pub brief: OpaqueId,
    pub repository: RepositoryId,
    pub provider: ProviderKind,
    /// What the human says they bring; may be empty.
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BriefTurnRecord {
    pub turn: OpaqueId,
    pub brief: OpaqueId,
    pub repository: RepositoryId,
    pub provider: ProviderKind,
    pub base: String,
    pub prompt_digest: String,
    pub stop_requested: bool,
    /// `None` while it runs.
    pub outcome: Option<String>,
}

fn brief_turn(c: &Connection, id: OpaqueId) -> Result<Option<BriefTurnRecord>, StorageError> {
    let row = c.query_optional(
        "SELECT t.turn,t.brief,t.repository,t.provider,t.base,t.prompt_digest,
                EXISTS(SELECT 1 FROM brief_turn_stops s WHERE s.turn=t.turn),
                (SELECT outcome FROM brief_turn_results r WHERE r.turn=t.turn)
         FROM brief_turns t WHERE t.turn=?1",
        &[Value::Text(&id_to_text(id))],
        |r| {
            Ok((
                (r.text(0)?, r.text(1)?, r.text(2)?, r.text(3)?),
                (r.text(4)?, r.text(5)?, r.int(6)? == 1, r.optional_text(7)?),
            ))
        },
    )?;
    row.map(
        |((turn, brief, repository, provider), (base, digest, stop, outcome))| {
            Ok(BriefTurnRecord {
                turn: id_from_text(&turn)?,
                brief: id_from_text(&brief)?,
                repository: RepositoryId(id_from_text(&repository)?),
                provider: provider_named(&provider)
                    .ok_or_else(|| StorageError::corrupt("brief provider"))?,
                base,
                prompt_digest: digest,
                stop_requested: stop,
                outcome,
            })
        },
    )
    .transpose()
}

fn unfinished(c: &Connection) -> Result<Option<String>, StorageError> {
    Ok(c.query_optional(
        "SELECT t.turn FROM brief_turns t
         WHERE NOT EXISTS(SELECT 1 FROM brief_turn_results r WHERE r.turn=t.turn) LIMIT 1",
        &[],
        |r| r.text(0),
    )?)
}

/// Unfinished brief turns take one unit of the global capacity each, like a lead turn.
pub(crate) fn active_briefs(c: &Connection) -> Result<u64, StorageError> {
    from_db(c.query_one(
        "SELECT count(*) FROM brief_turns t
         WHERE NOT EXISTS(SELECT 1 FROM brief_turn_results r WHERE r.turn=t.turn)",
        &[],
        |r| r.int(0),
    )?)
}

/// Whether a brief turn is running; a chat turn waits for it and it for a chat turn.
pub(crate) fn brief_running(c: &Connection) -> Result<bool, StorageError> {
    Ok(unfinished(c)?.is_some())
}

/// The identity of a repository's brief, if a brief turn of it was ever accepted.
fn brief_of(c: &Connection, repository: &str) -> Result<Option<String>, StorageError> {
    Ok(c.query_optional(
        "SELECT brief FROM brief_turns WHERE repository=?1 ORDER BY event LIMIT 1",
        &[Value::Text(repository)],
        |r| r.text(0),
    )?)
}

/// The newest revision of a brief: revision, canonical content, rendered text, event.
pub(crate) fn newest(
    c: &Connection,
    id: &str,
) -> Result<Option<(i64, String, String, i64)>, StorageError> {
    Ok(c.query_optional(
        "SELECT revision,content,rendered,event FROM project_briefs WHERE id=?1
         ORDER BY revision DESC LIMIT 1",
        &[Value::Text(id)],
        |r| Ok((r.int(0)?, r.text(1)?, r.text(2)?, r.int(3)?)),
    )?)
}

/// Whether `id` names a brief.
pub(crate) fn is_brief(c: &Connection, id: &str) -> Result<bool, StorageError> {
    Ok(c.query_optional(
        "SELECT 1 FROM project_briefs WHERE id=?1 LIMIT 1",
        &[Value::Text(id)],
        |r| r.int(0),
    )?
    .is_some())
}

/// A brief's title: "Project brief: <repository name>".
pub(crate) fn brief_title(c: &Connection, id: &str) -> Result<Option<String>, StorageError> {
    let path = c.query_optional(
        "SELECT l.path FROM project_briefs b JOIN local_repositories l ON l.id=b.repository
         WHERE b.id=?1 LIMIT 1",
        &[Value::Text(id)],
        |r| r.text(0),
    )?;
    Ok(path.map(|p| format!("Project brief: {}", crate::features::repository_name(&p))))
}

/// The rendered text of a repository's brief when its newest revision is approved; what
/// later discovery and plan turns of that repository are given.
pub(crate) fn approved_brief(
    c: &Connection,
    repository: RepositoryId,
) -> Result<Option<String>, StorageError> {
    let Some(id) = brief_of(c, &id_to_text(repository.0))? else {
        return Ok(None);
    };
    let Some((_, _, rendered, _)) = newest(c, &id)? else {
        return Ok(None);
    };
    let subject = ArtifactId(id_from_text(&id)?);
    Ok(crate::approvals::applicable(c, subject)?.map(|_| rendered))
}

impl Tx<'_> {
    /// The durable acceptance of one brief turn. Re-reads admission and capacity inside
    /// the writer transaction; no other lead turn may run. The prompt is composed here
    /// from the repository's name, the project section and what the human typed, and is
    /// kept with the turn. `base` is the commit the repository's HEAD names.
    pub fn accept_brief(&mut self, r: &BriefRequest, base: &str) -> Result<String, TxError> {
        if self.admission()? != Admission::Open {
            return Err(deny("AdmissionClosed"));
        }
        if unfinished(self.conn)?.is_some() || self.unfinished_lead_turn()? {
            return Err(deny("TurnRefused: another lead turn is running"));
        }
        self.capacity_left("a lead turn")?;
        let repository = id_to_text(r.repository.0);
        let path = self.conn.query_optional(
            "SELECT path FROM local_repositories WHERE id=?1",
            &[Value::Text(&repository)],
            |row| row.text(0),
        )?;
        let Some(path) = path else {
            return Err(deny("TurnRefused: the repository is not registered"));
        };
        let message = r.message.as_str();
        if message.len() > MAX_HUMAN_TEXT_BYTES || message.contains('\0') {
            return Err(deny("TurnRefused: the message is too long or contains NUL"));
        }
        let note = message
            .chars()
            .any(char::is_alphanumeric)
            .then_some(message);
        let project = crate::projects::of_repository(self.conn, r.repository)?;
        let section = project
            .as_ref()
            .map(|(_, _, record)| record.prompt_section());
        let name = crate::features::repository_name(&path);
        let prompt = brief_prompt(&name, note, section.as_deref());
        if prompt.len() > MAX_PROMPT_BYTES {
            return Err(deny(
                "PromptTooLarge: the stored content exceeds the prompt limit",
            ));
        }
        let brief = brief_of(self.conn, &repository)?.unwrap_or_else(|| id_to_text(r.brief));
        let turn = id_to_text(r.turn);
        let event = self.append_event(
            "BriefTurnAccepted",
            None,
            &format!(
                "{turn}|brief={brief}|{repository}|{}",
                provider_name(r.provider)
            ),
        )?;
        self.conn.execute(
            "INSERT INTO brief_turns VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            &[
                Value::Text(&turn),
                Value::Text(&brief),
                project
                    .as_ref()
                    .map(|(p, _, _)| id_to_text(*p))
                    .as_deref()
                    .map_or(Value::Null, Value::Text),
                match &project {
                    Some((_, revision, _)) => Value::Int(to_db(*revision)?),
                    None => Value::Null,
                },
                Value::Text(&repository),
                Value::Text(base),
                Value::Text(provider_name(r.provider)),
                Value::Text(message),
                Value::Text(&prompt),
                Value::Text(&sha256_hex(&[prompt.as_bytes()])),
                Value::Int(to_db(self.epoch.0)?),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(format!("accepted turn={turn}"))
    }

    fn unfinished_lead_turn(&self) -> Result<bool, StorageError> {
        Ok(self
            .conn
            .query_optional(
                "SELECT 1 FROM lead_turns t
                 WHERE NOT EXISTS(SELECT 1 FROM lead_turn_results r WHERE r.turn=t.turn) LIMIT 1",
                &[],
                |r| r.int(0),
            )?
            .is_some())
    }

    /// The human's stop of a running brief turn. The runner sees it within one poll.
    pub fn request_brief_stop(&mut self, id: OpaqueId) -> Result<(), TxError> {
        let Some(record) = brief_turn(self.conn, id)? else {
            return Err(deny("UnknownTurn"));
        };
        if record.outcome.is_some() {
            return Err(deny("TurnFinished"));
        }
        if !record.stop_requested {
            let event = self.append_event("BriefTurnStopRequested", None, &id_to_text(id))?;
            self.conn.execute(
                "INSERT INTO brief_turn_stops VALUES (?1,?2)",
                &[Value::Text(&id_to_text(id)), Value::Int(to_db(event.0)?)],
            )?;
        }
        Ok(())
    }

    /// Whether `id` is a brief turn; `CancelTurn` names chat and brief turns alike.
    pub fn is_brief_turn(&self, id: OpaqueId) -> Result<bool, StorageError> {
        Ok(brief_turn(self.conn, id)?.is_some())
    }

    /// Seals a brief turn once. A reply becomes the next revision of the repository's
    /// brief in the same transaction; the same content again is not a new revision.
    pub fn finish_brief(
        &mut self,
        id: OpaqueId,
        outcome: TurnOutcome,
        detail: &str,
        reply: Option<(&str, &Brief)>,
    ) -> Result<String, TxError> {
        let Some(record) = brief_turn(self.conn, id)? else {
            return Err(deny("UnknownTurn"));
        };
        if record.outcome.is_some() {
            return Err(deny("TurnFinished"));
        }
        if (outcome == TurnOutcome::Replied) != reply.is_some() {
            return Err(deny("a turn replied exactly when it has a reply"));
        }
        let turn = id_to_text(id);
        let brief = id_to_text(record.brief);
        let mut created = None;
        let mut summary = "";
        if let Some((_, parsed)) = reply {
            summary = &parsed.summary;
            let content = parsed.canonical_json();
            let previous = newest(self.conn, &brief)?;
            if previous
                .as_ref()
                .is_none_or(|(_, old, _, _)| *old != content)
            {
                let revision = previous.map_or(1, |(r, _, _, _)| r + 1);
                let path = self.conn.query_one(
                    "SELECT path FROM local_repositories WHERE id=?1",
                    &[Value::Text(&id_to_text(record.repository.0))],
                    |r| r.text(0),
                )?;
                let rendered = parsed.render_markdown(&crate::features::repository_name(&path));
                let event = self.append_event(
                    "ProjectBriefCreated",
                    None,
                    &format!("{brief}|revision={revision}|turn={turn}"),
                )?;
                self.conn.execute(
                    "INSERT INTO project_briefs VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    &[
                        Value::Text(&brief),
                        Value::Int(revision),
                        Value::Text(&id_to_text(record.repository.0)),
                        Value::Text(&turn),
                        Value::Text(&content),
                        Value::Text(&rendered),
                        Value::Int(to_db(event.0)?),
                    ],
                )?;
                self.invalidate_approvals()?;
                created = Some(revision);
            }
        }
        let detail: String = detail.chars().take(1024).collect();
        let event = self.append_event(
            "BriefTurnFinished",
            None,
            &format!("{turn}|{}", outcome.as_db()),
        )?;
        self.conn.execute(
            "INSERT INTO brief_turn_results VALUES (?1,?2,?3,?4,?5,?6,?7)",
            &[
                Value::Text(&turn),
                Value::Text(outcome.as_db()),
                Value::Text(&detail),
                Value::Text(summary),
                Value::Text(reply.map_or("", |(raw, _)| raw)),
                created.map_or(Value::Null, Value::Int),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(outcome.as_db().to_owned())
    }

    /// A brief turn whose helper ended first is closed as interrupted by the next helper.
    pub fn close_interrupted_briefs(&mut self) -> Result<u64, TxError> {
        let mut closed = 0;
        while let Some(id) = unfinished(self.conn)? {
            self.finish_brief(
                id_from_text(&id)?,
                TurnOutcome::Interrupted,
                "the helper ended before the brief turn did",
                None,
            )?;
            closed += 1;
        }
        Ok(closed)
    }
}

impl ReadView<'_> {
    pub fn brief_turn(&self, id: OpaqueId) -> Result<Option<BriefTurnRecord>, StorageError> {
        brief_turn(self.conn, id)
    }
    pub fn brief_prompt(&self, id: OpaqueId) -> Result<Option<String>, StorageError> {
        Ok(self.conn.query_optional(
            "SELECT prompt FROM brief_turns WHERE turn=?1",
            &[Value::Text(&id_to_text(id))],
            |r| r.text(0),
        )?)
    }
    /// Protocol 1.12: a repository's brief and its newest brief turn ([`brief_json`]).
    pub fn ipc_brief(&self, repository: RepositoryId) -> Result<Json, StorageError> {
        brief_json(self.conn, repository)
    }
    /// Every registered repository with the canonical path it was registered under.
    pub fn registered_repositories(&self) -> Result<Vec<(RepositoryId, String)>, StorageError> {
        self.conn
            .query(
                "SELECT id,path FROM local_repositories ORDER BY path",
                &[],
                |r| Ok((r.text(0)?, r.text(1)?)),
            )?
            .into_iter()
            .map(|(id, path)| Ok((RepositoryId(id_from_text(&id)?), path)))
            .collect()
    }
    pub fn unfinished_brief(&self) -> Result<Option<OpaqueId>, StorageError> {
        unfinished(self.conn)?
            .map(|id| id_from_text(&id))
            .transpose()
    }
}

/// Protocol 1.12: a repository's brief as Projects and New Project show it. The newest
/// revision with its approval state and rows, and the newest brief turn with its state;
/// either is null when there is none.
pub(crate) fn brief_json(c: &Connection, repository: RepositoryId) -> Result<Json, StorageError> {
    let repository_text = id_to_text(repository.0);
    let turn = c.query_optional(
        "SELECT t.turn,t.provider,t.event,r.outcome,r.detail,r.event,
                EXISTS(SELECT 1 FROM brief_turn_stops s WHERE s.turn=t.turn)
         FROM brief_turns t LEFT JOIN brief_turn_results r ON r.turn=t.turn
         WHERE t.repository=?1 ORDER BY t.event DESC LIMIT 1",
        &[Value::Text(&repository_text)],
        |r| {
            Ok((
                (r.text(0)?, r.text(1)?, r.int(2)?),
                (r.optional_text(3)?, r.optional_text(4)?, r.optional_int(5)?),
                r.int(6)? == 1,
            ))
        },
    )?;
    let turn = match turn {
        None => Json::Null,
        Some(((id, provider, started), (outcome, detail, ended), stopping)) => {
            let state = match outcome.as_deref() {
                Some("REPLIED") => "Replied",
                Some("FAILED") => "Failed",
                Some("CANCELLED") => "Cancelled",
                Some(_) => "Interrupted",
                None if stopping => "Stopping",
                None => "Running",
            };
            object([
                ("turn", identity(id_from_text(&id)?)),
                ("provider", text(provider)),
                ("state", text(state)),
                ("detail", optional(detail.filter(|d| !d.is_empty()))),
                ("started_at_unix_ms", time_of(c, started)?),
                (
                    "ended_at_unix_ms",
                    ended.map_or(Ok(Json::Null), |e| time_of(c, e))?,
                ),
            ])
        }
    };
    let id = brief_of(c, &repository_text)?;
    let current = match &id {
        Some(id) => newest(c, id)?,
        None => None,
    };
    let brief = match (id, current) {
        (Some(id), Some((revision, content, _, event))) => {
            let subject = ArtifactId(id_from_text(&id)?);
            let (approval, _, _) = crate::overview::approval_state(c, subject, false)?;
            let parsed = parse_brief(&content)
                .map_err(|e| StorageError::corrupt(format!("stored brief does not read: {e:?}")))?;
            let rows = parsed
                .rows
                .iter()
                .map(|row| {
                    object([
                        ("key", text(row.key.clone())),
                        ("text", text(row.text.clone())),
                        (
                            "sources",
                            Json::List(row.sources.iter().map(|s| text(s.clone())).collect()),
                        ),
                        ("warning", Json::Bool(row.warning)),
                    ])
                })
                .collect();
            object([
                ("document", identity(subject.0)),
                ("revision", Json::Int(revision)),
                ("approval", text(approval)),
                ("summary", text(parsed.summary.clone())),
                ("rows", Json::List(rows)),
                (
                    "test_command",
                    parsed.test_command.as_ref().map_or(Json::Null, |command| {
                        Json::List(command.iter().map(|a| text(a.clone())).collect())
                    }),
                ),
                ("created_at_unix_ms", time_of(c, event)?),
            ])
        }
        _ => Json::Null,
    };
    Ok(object([("brief", brief), ("turn", turn)]))
}

/// The frozen request of an accepted brief turn: the stored prompt, run read-only in the
/// registered repository.
pub fn brief_spec(
    engine: &Engine,
    id: OpaqueId,
    repository: &LocalRepository,
    fingerprint: &ProviderFingerprint,
    variables: Vec<String>,
) -> Result<TurnSpec, String> {
    let mut reader = engine.reader().map_err(|e| e.to_string())?;
    let (record, prompt) = reader
        .read(|v| Ok((v.brief_turn(id)?, v.brief_prompt(id)?)))
        .map_err(|e| e.to_string())?;
    let (Some(record), Some(prompt)) = (record, prompt) else {
        return Err("accepted brief turn is missing".into());
    };
    if record.repository != repository.id() || record.provider != fingerprint.provider {
        return Err("brief repository or provider differs from the accepted turn".into());
    }
    if sha256_hex(&[prompt.as_bytes()]) != record.prompt_digest {
        return Err("stored prompt differs from its digest".into());
    }
    Ok(TurnSpec {
        turn: id,
        kind: TurnKind::Brief,
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

/// Seals a concluded brief run. Only a clean run whose last message is a brief replies;
/// a process that could not be confirmed stopped also closes admission.
pub fn settle_brief(
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
            Some(raw) => match parse_brief(raw) {
                Ok(brief) => (
                    TurnOutcome::Replied,
                    String::new(),
                    Some((raw.to_owned(), brief)),
                ),
                Err(error) => (
                    TurnOutcome::Failed,
                    format!("the last message is not a brief: {error:?}"),
                    None,
                ),
            },
        },
        (_, _) if run.cancelled => (
            TurnOutcome::Cancelled,
            "stopped by the human".to_owned(),
            None,
        ),
        (TurnConclusion::NotSpawned(why), _) => {
            (TurnOutcome::Failed, format!("not spawned: {why}"), None)
        }
        (TurnConclusion::StopUnconfirmed(reasons), _) => (
            TurnOutcome::Failed,
            format!("stop unconfirmed: {}", reasons.join("; ")),
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
    let unconfirmed = matches!(run.conclusion, TurnConclusion::StopUnconfirmed(_));
    let id = spec.turn;
    let applied = {
        let (detail, reply) = (detail.clone(), reply.clone());
        command(ctx, "FinishBriefTurn", id, move |tx| {
            if unconfirmed {
                tx.set_admission(Admission::Closed)?;
            }
            tx.finish_brief(
                id,
                outcome,
                &detail,
                reply.as_ref().map(|(raw, parsed)| (raw.as_str(), parsed)),
            )
        })
    };
    match applied {
        Ok(_) => Ok(outcome),
        Err(why) if outcome == TurnOutcome::Replied => {
            let detail = format!("the reply could not be stored: {why}");
            command(ctx, "FinishBriefTurn", id, move |tx| {
                tx.finish_brief(id, TurnOutcome::Failed, &detail, None)
            })?;
            Ok(TurnOutcome::Failed)
        }
        Err(why) => Err(why),
    }
}

//! User-facing data (protocol 1.6): names of features, tasks and documents, the live
//! state of an attempt, what waits for the human, and the time and actor of an event.
//! Read-only and derived from what the core already records; nothing here is stored.
use crate::approvals::{applicable, approval_inputs_ready, bundle, decisions};
use crate::model::{id_from_text, to_db};
use crate::sqlite::{Connection, Value};
use crate::turns::INDEX_TEXT_CHARS;
use crate::validation::{ValidationState, rejected, state};
use crate::{ReadView, StorageError};
use jojo_domain::identity::ArtifactId;
use jojo_domain::ipc::{identity, number, object, text};
use jojo_domain::json::Json;
use jojo_domain::prd::PrdDocument;

/// Rows per page of `Attempts` and `Documents`, and the most pending candidates listed.
pub const OVERVIEW_ROWS: usize = 16;

fn cut(s: &str) -> String {
    s.trim().chars().take(INDEX_TEXT_CHARS).collect()
}

fn optional(value: Option<String>) -> Json {
    value.map_or(Json::Null, text)
}

fn identity_text(value: &str) -> Result<Json, StorageError> {
    Ok(identity(id_from_text(value)?))
}

fn time_of(c: &Connection, seq: i64) -> Result<Json, StorageError> {
    Ok(c.query_optional(
        "SELECT recorded_at_unix_ms FROM events WHERE seq=?1",
        &[Value::Int(seq)],
        |r| r.int(0),
    )?
    .map_or(Json::Null, Json::Int))
}

/// The title of a requirements document, when its stored content reads as one.
fn prd_title(content: &str) -> Option<String> {
    PrdDocument::import(content.as_bytes())
        .ok()
        .map(|d| cut(&d.title))
        .filter(|t| !t.is_empty())
}

/// A feature's name: the title of its newest requirements document, else the first line
/// of the first message of its chat. At most 160 characters.
pub(crate) fn feature_name(c: &Connection, feature: &str) -> Result<Option<String>, StorageError> {
    let prd = c.query_optional(
        "SELECT content FROM artifact_versions WHERE feature_id=?1 AND artifact_type='PRD'
         ORDER BY created_event_seq DESC LIMIT 1",
        &[Value::Text(feature)],
        |r| r.text(0),
    )?;
    if let Some(title) = prd.as_deref().and_then(prd_title) {
        return Ok(Some(title));
    }
    let message = c.query_optional(
        "SELECT message FROM lead_turns WHERE feature=?1 ORDER BY ordinal LIMIT 1",
        &[Value::Text(feature)],
        |r| r.text(0),
    )?;
    Ok(message.and_then(|m| m.lines().map(cut).find(|line| !line.is_empty())))
}

/// The feature a document belongs to: its own for a requirements document, its
/// requirements document's for an ADR or a plan.
fn document_feature(c: &Connection, document: &str) -> Result<Option<String>, StorageError> {
    Ok(c.query_optional(
        "SELECT v.feature_id FROM artifact_versions v
         WHERE v.artifact_id=coalesce(
             (SELECT prd FROM review_documents WHERE id=?1 ORDER BY revision DESC LIMIT 1), ?1)
         LIMIT 1",
        &[Value::Text(document)],
        |r| r.text(0),
    )?)
}

/// The key and title a lead plan gave a step; an imported plan has neither.
fn step_names(c: &Connection, step: &str) -> Result<(Json, Json), StorageError> {
    let row = c.query_optional(
        "SELECT key,title FROM lead_plan_steps WHERE step=?1 LIMIT 1",
        &[Value::Text(step)],
        |r| Ok((r.text(0)?, r.text(1)?)),
    )?;
    Ok(row.map_or((Json::Null, Json::Null), |(key, title)| {
        (text(key), text(cut(&title)))
    }))
}

fn feature_fields(c: &Connection, feature: Option<String>) -> Result<(Json, Json), StorageError> {
    Ok(match feature {
        None => (Json::Null, Json::Null),
        Some(f) => (identity_text(&f)?, optional(feature_name(c, &f)?)),
    })
}

/// The lifecycle state of an attempt, from the records of its run. "Unsupervised" is an
/// unsealed attempt reserved by an earlier helper: no helper watches it now and the
/// Recovery Center decides it.
fn attempt_state(flags: &AttemptFlags, current_epoch: i64) -> &'static str {
    match flags.outcome.as_deref() {
        Some("SUCCEEDED") => return "Succeeded",
        Some("FAILED") => return "Failed",
        Some("CANCELLED") => return "Cancelled",
        Some(_) => return "Interrupted",
        None => {}
    }
    if flags.held {
        "Held"
    } else if flags.epoch < current_epoch {
        "Unsupervised"
    } else if flags.settling {
        "Settling"
    } else if flags.stopping {
        "Stopping"
    } else if flags.running {
        "Running"
    } else if flags.launched {
        "Launching"
    } else if flags.worktree {
        "Preparing"
    } else {
        "Reserved"
    }
}

struct AttemptFlags {
    epoch: i64,
    outcome: Option<String>,
    held: bool,
    settling: bool,
    stopping: bool,
    running: bool,
    launched: bool,
    worktree: bool,
}

/// The approval state of a document's newest revision, whether an earlier revision was
/// decided, and the event of the decision that applies to this revision.
fn approval_state(
    c: &Connection,
    id: ArtifactId,
    prd: bool,
) -> Result<(&'static str, bool, Option<i64>), StorageError> {
    let Some(current) = bundle(c, id)? else {
        return Ok(("Waiting", false, None));
    };
    let last = decisions(c, id)?.pop();
    if let Some(d) = last.as_ref().filter(|d| d.bundle == current.hash) {
        let seq = Some(to_db(d.seq.0)?);
        return Ok(match d.action.as_str() {
            "Approve" if applicable(c, id)?.is_some() => ("Approved", false, seq),
            "Approve" => ("Waiting", false, seq),
            "Reject" => ("Rejected", false, seq),
            _ => ("Revoked", false, seq),
        });
    }
    if !approval_inputs_ready(c, id)? {
        return Ok((if prd { "NotReady" } else { "Waiting" }, false, None));
    }
    Ok(("Pending", last.is_some(), None))
}

impl ReadView<'_> {
    /// Attempts newest first, at most [`OVERVIEW_ROWS`] reserved before `before` (the
    /// `order` of a row; `None` for the newest).
    pub fn ipc_attempts(&self, before: Option<u64>) -> Result<Json, StorageError> {
        let c = self.conn;
        let current = to_db(self.session()?.epoch.0)?;
        let before = before.map_or(Ok(i64::MAX), to_db)?;
        let rows = c.query(
            "SELECT a.rowid, a.attempt, a.step, a.epoch,
               (SELECT plan FROM dependency_attempt_inputs WHERE attempt=a.attempt),
               (SELECT provider FROM process_launches WHERE attempt=a.attempt
                ORDER BY generation DESC LIMIT 1),
               (SELECT min(issue_event) FROM process_launches WHERE attempt=a.attempt),
               r.outcome, r.event,
               EXISTS(SELECT 1 FROM attempt_recovery_holds WHERE attempt=a.attempt),
               EXISTS(SELECT 1 FROM process_observations
                      WHERE attempt=a.attempt AND kind='TREE_ABSENT'),
               EXISTS(SELECT 1 FROM attempt_stop_causes WHERE attempt=a.attempt),
               EXISTS(SELECT 1 FROM process_records WHERE attempt=a.attempt AND role='ROOT'),
               EXISTS(SELECT 1 FROM owned_worktrees WHERE attempt=a.attempt)
             FROM scheduler_attempts a LEFT JOIN attempt_results r ON r.attempt=a.attempt
             WHERE a.rowid<?1 ORDER BY a.rowid DESC LIMIT ?2",
            &[
                Value::Int(before),
                Value::Int(to_db(OVERVIEW_ROWS as u64 + 1)?),
            ],
            |r| {
                Ok((
                    (r.int(0)?, r.text(1)?, r.text(2)?, r.optional_text(4)?),
                    (r.optional_text(5)?, r.optional_int(6)?, r.optional_int(8)?),
                    AttemptFlags {
                        epoch: r.int(3)?,
                        outcome: r.optional_text(7)?,
                        held: r.int(9)? != 0,
                        settling: r.int(10)? != 0,
                        stopping: r.int(11)? != 0,
                        running: r.int(12)? != 0,
                        launched: r.optional_int(6)?.is_some(),
                        worktree: r.int(13)? != 0,
                    },
                ))
            },
        )?;
        let more = rows.len() > OVERVIEW_ROWS;
        let mut out = Vec::new();
        for ((order, attempt, step, plan), (provider, started, ended), flags) in
            rows.into_iter().take(OVERVIEW_ROWS)
        {
            let (key, title) = step_names(c, &step)?;
            let feature = match &plan {
                Some(plan) => document_feature(c, plan)?,
                None => None,
            };
            let (feature, feature_name) = feature_fields(c, feature)?;
            out.push(object([
                ("order", Json::Int(order)),
                ("attempt", identity_text(&attempt)?),
                ("step", identity_text(&step)?),
                ("key", key),
                ("title", title),
                (
                    "plan",
                    plan.as_deref().map_or(Ok(Json::Null), identity_text)?,
                ),
                ("feature", feature),
                ("feature_name", feature_name),
                ("provider", optional(provider)),
                ("state", text(attempt_state(&flags, current))),
                (
                    "started_at_unix_ms",
                    started.map_or(Ok(Json::Null), |s| time_of(c, s))?,
                ),
                (
                    "ended_at_unix_ms",
                    ended.map_or(Ok(Json::Null), |s| time_of(c, s))?,
                ),
            ]));
        }
        let next = if more {
            out.last()
                .and_then(|row| jojo_domain::ipc::get(row, "order").ok().cloned())
                .unwrap_or(Json::Null)
        } else {
            Json::Null
        };
        Ok(object([("attempts", Json::List(out)), ("next", next)]))
    }

    /// Requirements documents, ADRs and plans newest first, at most [`OVERVIEW_ROWS`]
    /// whose newest revision was created before event `before`, each with its approval
    /// state; and, whatever the page, the candidates that wait for the human's decision
    /// and the number of things that wait (`pending`).
    pub fn ipc_documents(&self, before: Option<u64>) -> Result<Json, StorageError> {
        let c = self.conn;
        let before = before.map_or(Ok(i64::MAX), to_db)?;
        let all = c.query(
            "SELECT id, kind, created FROM (
               SELECT artifact_id AS id, 'PRD' AS kind, max(created_event_seq) AS created
                 FROM artifact_versions WHERE artifact_type='PRD' GROUP BY artifact_id
               UNION ALL
               SELECT id, kind, max(event) FROM review_documents GROUP BY id)
             ORDER BY created DESC",
            &[],
            |r| Ok((r.text(0)?, r.text(1)?, r.int(2)?)),
        )?;
        let mut pending = 0u64;
        let mut out = Vec::new();
        let mut more = false;
        for (id, kind, created) in &all {
            let subject = ArtifactId(id_from_text(id)?);
            let prd = kind == "PRD";
            let (approval, stale, decided) = approval_state(c, subject, prd)?;
            if approval == "Pending" {
                pending += 1;
            }
            if *created >= before {
                continue;
            }
            if out.len() == OVERVIEW_ROWS {
                more = true;
                continue;
            }
            let (kind, revision, title, author) = if prd {
                c.query_one(
                    "SELECT revision,content,author FROM artifact_versions
                     WHERE artifact_id=?1 ORDER BY revision DESC LIMIT 1",
                    &[Value::Text(id)],
                    |r| Ok(("Prd", r.int(0)?, prd_title(&r.text(1)?), r.text(2)?)),
                )
                .map(|(k, rev, t, a)| (k, rev, t, Some(a)))?
            } else {
                let revision = c.query_one(
                    "SELECT max(revision) FROM review_documents WHERE id=?1",
                    &[Value::Text(id)],
                    |r| r.int(0),
                )?;
                let plan = c
                    .query_optional(
                        "SELECT 1 FROM execution_plans WHERE plan=?1 LIMIT 1",
                        &[Value::Text(id)],
                        |r| r.int(0),
                    )?
                    .is_some();
                let kind = match (kind.as_str(), plan) {
                    ("ADR", _) => "Adr",
                    (_, true) => "ExecutionPlan",
                    _ => "ValidationPlan",
                };
                (kind, revision, None, None)
            };
            let (feature, feature_name) = feature_fields(c, document_feature(c, id)?)?;
            out.push(object([
                ("document", identity_text(id)?),
                ("kind", text(kind)),
                ("revision", Json::Int(revision)),
                ("title", optional(title)),
                ("feature", feature),
                ("feature_name", feature_name),
                (
                    "author",
                    optional(author.map(|a| if a == "HUMAN" { "Human" } else { "Model" }.into())),
                ),
                ("approval", text(approval)),
                ("stale", Json::Bool(stale)),
                ("created_at_unix_ms", time_of(c, *created)?),
                (
                    "decided_at_unix_ms",
                    decided.map_or(Ok(Json::Null), |s| time_of(c, s))?,
                ),
                ("order", Json::Int(*created)),
            ]));
        }
        let candidates = self.pending_candidates()?;
        pending += candidates.len() as u64;
        let listed: Vec<Json> = candidates.into_iter().take(OVERVIEW_ROWS).collect();
        let next = if more {
            out.last()
                .and_then(|row| jojo_domain::ipc::get(row, "order").ok().cloned())
                .unwrap_or(Json::Null)
        } else {
            Json::Null
        };
        Ok(object([
            ("documents", Json::List(out)),
            ("next", next),
            ("candidates", Json::List(listed)),
            ("pending", number(pending)),
        ]))
    }

    /// Sealed candidates whose newest host validation passed and that the human has
    /// neither accepted nor rejected, newest first.
    fn pending_candidates(&self) -> Result<Vec<Json>, StorageError> {
        let c = self.conn;
        let rows = c.query(
            "SELECT r.attempt, a.step,
               (SELECT plan FROM dependency_attempt_inputs WHERE attempt=r.attempt)
             FROM attempt_results r JOIN scheduler_attempts a ON a.attempt=r.attempt
             WHERE r.candidate IS NOT NULL
               AND NOT EXISTS(SELECT 1 FROM candidate_acceptances x WHERE x.attempt=r.attempt)
             ORDER BY r.event DESC",
            &[],
            |r| Ok((r.text(0)?, r.text(1)?, r.optional_text(2)?)),
        )?;
        let mut out = Vec::new();
        for (attempt, step, plan) in rows {
            let ValidationState::Passed(receipt) = state(c, &attempt)? else {
                continue;
            };
            if rejected(c, &attempt)? {
                continue;
            }
            let (key, title) = step_names(c, &step)?;
            let feature = match &plan {
                Some(plan) => document_feature(c, plan)?,
                None => None,
            };
            let (feature, feature_name) = feature_fields(c, feature)?;
            let receipt = to_db(receipt.0)?;
            out.push(object([
                ("attempt", identity_text(&attempt)?),
                ("key", key),
                ("title", title),
                ("feature", feature),
                ("feature_name", feature_name),
                ("receipt", Json::Int(receipt)),
                ("validated_at_unix_ms", time_of(c, receipt)?),
            ]));
        }
        Ok(out)
    }
}

/// Who caused an event: the human through a client command (a command identity whose top
/// bit is clear; the helper's own commands use reserved identities), the provider whose
/// output it records (a provider stream record, a lead turn that replied, a review that
/// gave a verdict, and every event of that same command), else the core.
pub(crate) fn event_actor(
    c: &Connection,
    seq: i64,
    command: Option<&str>,
) -> Result<&'static str, StorageError> {
    let Some(command) = command else {
        return Ok("Core");
    };
    if command.chars().next().is_some_and(|d| d < '8') {
        return Ok("Human");
    }
    let provider = c.query_optional(
        "SELECT coalesce(
           (SELECT l.provider FROM provider_events p JOIN process_launches l
              ON l.attempt=p.attempt AND l.generation=(SELECT max(generation)
                 FROM process_launches WHERE attempt=p.attempt)
            WHERE p.event=?1),
           (SELECT t.provider FROM events x JOIN lead_turn_results r ON r.event=x.seq
              JOIN lead_turns t ON t.turn=r.turn
            WHERE x.command_id=?2 AND r.outcome='REPLIED'),
           (SELECT v.provider FROM events x JOIN step_review_results r ON r.event=x.seq
              JOIN step_reviews v ON v.review=r.review
            WHERE x.command_id=?2 AND r.outcome IN ('APPROVED','REJECTED')))",
        &[Value::Int(seq), Value::Text(command)],
        |r| r.optional_text(0),
    )?;
    Ok(match provider.flatten().as_deref() {
        Some("ClaudeCode") => "ClaudeCode",
        Some("Codex") => "Codex",
        _ => "Core",
    })
}

#[cfg(test)]
mod tests {
    use super::{AttemptFlags, attempt_state};

    fn flags(epoch: i64) -> AttemptFlags {
        AttemptFlags {
            epoch,
            outcome: None,
            held: false,
            settling: false,
            stopping: false,
            running: false,
            launched: false,
            worktree: false,
        }
    }

    #[test]
    fn s2_an_attempt_state_follows_its_newest_record() {
        let mut f = flags(2);
        assert_eq!(attempt_state(&f, 2), "Reserved");
        f.worktree = true;
        assert_eq!(attempt_state(&f, 2), "Preparing");
        f.launched = true;
        assert_eq!(attempt_state(&f, 2), "Launching");
        f.running = true;
        assert_eq!(attempt_state(&f, 2), "Running");
        f.stopping = true;
        assert_eq!(attempt_state(&f, 2), "Stopping");
        f.settling = true;
        assert_eq!(attempt_state(&f, 2), "Settling");
        // Reserved by an earlier helper and not sealed: nobody supervises it now.
        assert_eq!(attempt_state(&f, 3), "Unsupervised");
        f.held = true;
        assert_eq!(attempt_state(&f, 3), "Held");
        // A sealed result wins over everything else.
        for (outcome, state) in [
            ("SUCCEEDED", "Succeeded"),
            ("FAILED", "Failed"),
            ("CANCELLED", "Cancelled"),
            ("INTERRUPTED", "Interrupted"),
        ] {
            f.outcome = Some(outcome.into());
            assert_eq!(attempt_state(&f, 3), state);
        }
    }
}

//! User-facing data (protocol 1.6): names of features, tasks and documents, the live
//! state of an attempt, what waits for the human, and the time and actor of an event.
//! Read-only and derived from what the core already records; nothing here is stored.
//! Protocol 1.9 adds one document's revisions, text and decisions (`Document`) and the
//! newest decisions over all documents (`Decisions`).
use crate::approvals::{applicable, approval_inputs_ready, bundle, decisions, hash};
use crate::model::{id_from_text, to_db};
use crate::sqlite::{Connection, Value};
use crate::turns::{CHAT_TEXT_CHARS, INDEX_TEXT_CHARS};
use crate::validation::{ValidationState, rejected, state};
use crate::{ReadView, StorageError};
use jojo_domain::identity::ArtifactId;
use jojo_domain::ipc::{identity, number, object, text};
use jojo_domain::json::Json;
use jojo_domain::prd::{Incomplete, PrdDocument};

/// Rows per page of `Attempts` and `Documents`, and the most pending candidates listed.
pub const OVERVIEW_ROWS: usize = 16;

/// The most revisions, decisions, findings and criteria one `Document` answer lists.
pub const DOCUMENT_ROWS: usize = 32;

pub(crate) fn cut(s: &str) -> String {
    s.trim().chars().take(INDEX_TEXT_CHARS).collect()
}

pub(crate) fn optional(value: Option<String>) -> Json {
    value.map_or(Json::Null, text)
}

pub(crate) fn identity_text(value: &str) -> Result<Json, StorageError> {
    Ok(identity(id_from_text(value)?))
}

pub(crate) fn time_of(c: &Connection, seq: i64) -> Result<Json, StorageError> {
    Ok(c.query_optional(
        "SELECT recorded_at_unix_ms FROM events WHERE seq=?1",
        &[Value::Int(seq)],
        |r| r.int(0),
    )?
    .map_or(Json::Null, Json::Int))
}

/// The title of a requirements document, when its stored content reads as one.
pub(crate) fn prd_title(content: &str) -> Option<String> {
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
pub(crate) fn document_feature(
    c: &Connection,
    document: &str,
) -> Result<Option<String>, StorageError> {
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
pub(crate) fn step_names(c: &Connection, step: &str) -> Result<(Json, Json), StorageError> {
    let row = c.query_optional(
        "SELECT key,title FROM lead_plan_steps WHERE step=?1 LIMIT 1",
        &[Value::Text(step)],
        |r| Ok((r.text(0)?, r.text(1)?)),
    )?;
    Ok(row.map_or((Json::Null, Json::Null), |(key, title)| {
        (text(key), text(cut(&title)))
    }))
}

pub(crate) fn feature_fields(
    c: &Connection,
    feature: Option<String>,
) -> Result<(Json, Json), StorageError> {
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

/// The wire kind of a review document: an ADR, or a plan that is an execution plan when
/// `execution_plans` holds it and a validation plan otherwise.
pub(crate) fn review_kind(
    c: &Connection,
    id: &str,
    kind: &str,
) -> Result<&'static str, StorageError> {
    if kind == "ADR" {
        return Ok("Adr");
    }
    let plan = c
        .query_optional(
            "SELECT 1 FROM execution_plans WHERE plan=?1 LIMIT 1",
            &[Value::Text(id)],
            |r| r.int(0),
        )?
        .is_some();
    Ok(if plan {
        "ExecutionPlan"
    } else {
        "ValidationPlan"
    })
}

/// The approval state of a document's newest revision, whether an earlier revision was
/// decided, and the event of the decision that applies to this revision.
pub(crate) fn approval_state(
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
    /// `order` of a row; `None` for the newest). `detailed` (protocol 1.7) adds each
    /// attempt's ordinal, repository and project, host validation and newest activity.
    pub fn ipc_attempts(&self, before: Option<u64>, detailed: bool) -> Result<Json, StorageError> {
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
            let details = if detailed {
                crate::features::attempt_details(c, &attempt, plan.as_deref(), feature.as_deref())?
            } else {
                Vec::new()
            };
            let (feature, feature_name) = feature_fields(c, feature)?;
            let Json::Object(mut row) = object([
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
            ]) else {
                unreachable!("an attempt row is an object")
            };
            row.extend(details.into_iter().map(|(k, v)| (k.into(), v)));
            out.push(Json::Object(row));
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
    /// M5 (protocol 1.10, `external`): also the push and pull request requests that wait
    /// for the human's decision, counted in `pending`.
    pub fn ipc_documents(&self, before: Option<u64>, external: bool) -> Result<Json, StorageError> {
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
                (review_kind(c, id, kind)?, revision, None, None)
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
        let mut fields = vec![
            ("documents", Json::List(out)),
            ("next", next),
            ("candidates", Json::List(listed)),
        ];
        if external {
            let requests = self.pending_external()?;
            pending += requests.len() as u64;
            fields.push((
                "external",
                Json::List(requests.into_iter().take(OVERVIEW_ROWS).collect()),
            ));
        }
        fields.push(("pending", number(pending)));
        Ok(object(fields))
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

/// One stored revision of a document, whichever table holds it.
struct RevisionRow {
    revision: i64,
    content: String,
    rendered: String,
    digest: String,
    created: i64,
    author: Option<String>,
}

/// Every revision of a document, oldest first, and its stored kind (`PRD`, `ADR`, `PLAN`).
fn revision_rows(c: &Connection, id: &str) -> Result<(String, Vec<RevisionRow>), StorageError> {
    let prd = c.query(
        "SELECT revision,content,rendered,content_digest,created_event_seq,author
         FROM artifact_versions WHERE artifact_id=?1 AND artifact_type='PRD' ORDER BY revision",
        &[Value::Text(id)],
        |r| {
            Ok(RevisionRow {
                revision: r.int(0)?,
                content: r.text(1)?,
                rendered: r.text(2)?,
                digest: r.text(3)?,
                created: r.int(4)?,
                author: Some(r.text(5)?),
            })
        },
    )?;
    if !prd.is_empty() {
        return Ok(("PRD".into(), prd));
    }
    let mut kind = String::new();
    let rows = c.query(
        "SELECT revision,content,rendered,event,kind FROM review_documents
         WHERE id=?1 ORDER BY revision",
        &[Value::Text(id)],
        |r| {
            Ok((
                RevisionRow {
                    revision: r.int(0)?,
                    content: r.text(1)?,
                    rendered: r.text(2)?,
                    digest: String::new(),
                    created: r.int(3)?,
                    author: None,
                },
                r.text(4)?,
            ))
        },
    )?;
    let rows = rows
        .into_iter()
        .map(|(mut row, k)| {
            row.digest = hash(&[&row.content]);
            kind = k;
            row
        })
        .collect();
    Ok((kind, rows))
}

fn decision_word(action: &str) -> &'static str {
    match action {
        "Approve" => "Approved",
        "Reject" => "Rejected",
        _ => "Revoked",
    }
}

fn finding_word(problem: Incomplete) -> &'static str {
    match problem {
        Incomplete::Empty => "Empty",
        Incomplete::Placeholder => "Placeholder",
        Incomplete::UnknownReference => "UnknownReference",
        Incomplete::OpenBlocker => "OpenBlocker",
    }
}

/// The documents drafted on `id` (as their requirements document or their ADR), newest
/// revision of each.
fn downstream(c: &Connection, id: &str) -> Result<Vec<Json>, StorageError> {
    let rows = c.query(
        "SELECT id, kind, max(revision) FROM review_documents WHERE prd=?1 OR adr=?1
         GROUP BY id ORDER BY max(event)",
        &[Value::Text(id)],
        |r| Ok((r.text(0)?, r.text(1)?, r.int(2)?)),
    )?;
    let mut out = Vec::new();
    for (other, kind, revision) in rows.into_iter().take(DOCUMENT_ROWS) {
        let subject = ArtifactId(id_from_text(&other)?);
        let (approval, _, _) = approval_state(c, subject, false)?;
        out.push(object([
            ("document", identity_text(&other)?),
            ("kind", text(review_kind(c, &other, &kind)?)),
            ("revision", Json::Int(revision)),
            ("approval", text(approval)),
        ]));
    }
    Ok(out)
}

/// A document named by a review document's `prd` or `adr` column.
fn upstream_row(c: &Connection, id: &str) -> Result<Json, StorageError> {
    let (kind, rows) = revision_rows(c, id)?;
    let newest = rows.last();
    let wire = if kind == "PRD" {
        "Prd"
    } else {
        review_kind(c, id, &kind)?
    };
    let subject = ArtifactId(id_from_text(id)?);
    let (approval, _, _) = approval_state(c, subject, kind == "PRD")?;
    Ok(object([
        ("document", identity_text(id)?),
        ("kind", text(wire)),
        (
            "revision",
            newest.map_or(Json::Null, |r| Json::Int(r.revision)),
        ),
        (
            "title",
            optional(
                newest
                    .filter(|_| kind == "PRD")
                    .and_then(|r| prd_title(&r.content)),
            ),
        ),
        ("approval", text(approval)),
    ]))
}

impl ReadView<'_> {
    /// Protocol 1.9: one document. Every revision with the decision recorded on it, the
    /// approval state of the newest, the decisions newest first, what it was drafted on
    /// and what was drafted on it; for the chosen revision (`None`: the newest) its
    /// rendered text from `offset` (at most 8192 characters) and, for a requirements
    /// document, its sections counted, its findings and its acceptance criteria with the
    /// use cases that name them. `null` when the core has no such document.
    pub fn ipc_document(
        &self,
        document: ArtifactId,
        revision: Option<u64>,
        offset: usize,
    ) -> Result<Json, StorageError> {
        let c = self.conn;
        let id = crate::model::id_to_text(document.0);
        let (stored, rows) = revision_rows(c, &id)?;
        let Some(newest) = rows.last() else {
            return Ok(Json::Null);
        };
        let prd = stored == "PRD";
        let chosen = match revision {
            None => newest,
            Some(r) => {
                let Some(row) = rows.iter().find(|row| row.revision as u64 == r) else {
                    return Ok(Json::Null);
                };
                row
            }
        };
        let kind = if prd {
            "Prd"
        } else {
            review_kind(c, &id, &stored)?
        };
        let (approval, stale, _) = approval_state(c, document, prd)?;
        let current = bundle(c, document)?.map(|b| b.hash);
        let all = decisions(c, document)?;
        let applying = all
            .last()
            .filter(|d| Some(&d.bundle) == current.as_ref())
            .map(|d| d.seq);
        let mut history = Vec::new();
        for d in all.iter().rev().take(DOCUMENT_ROWS) {
            let seq = to_db(d.seq.0)?;
            history.push(object([
                ("revision", number(d.revision.0)),
                ("decision", text(decision_word(&d.action))),
                ("at_unix_ms", time_of(c, seq)?),
                ("digest", text(d.content_hash.clone())),
                ("applies", Json::Bool(Some(d.seq) == applying)),
            ]));
        }
        let mut revisions = Vec::new();
        for row in rows.iter().rev().take(DOCUMENT_ROWS) {
            let last = all
                .iter()
                .rev()
                .find(|d| d.revision.0 as i64 == row.revision);
            revisions.push(object([
                ("revision", Json::Int(row.revision)),
                ("created_at_unix_ms", time_of(c, row.created)?),
                ("digest", text(row.digest.clone())),
                (
                    "author",
                    optional(
                        row.author
                            .as_ref()
                            .map(|a| if a == "HUMAN" { "Human" } else { "Model" }.to_owned()),
                    ),
                ),
                (
                    "decision",
                    optional(last.map(|d| decision_word(&d.action).to_owned())),
                ),
                (
                    "decided_at_unix_ms",
                    match last {
                        Some(d) => time_of(c, to_db(d.seq.0)?)?,
                        None => Json::Null,
                    },
                ),
                ("superseded", Json::Bool(row.revision < newest.revision)),
            ]));
        }
        let parsed = if prd {
            PrdDocument::import(chosen.content.as_bytes()).ok()
        } else {
            None
        };
        let counts = parsed.as_ref().map_or(Json::Null, |d| {
            object([
                ("goal", number(u64::from(!d.goal.trim().is_empty()))),
                ("non_goals", number(d.non_goals.len() as u64)),
                ("actors", number(d.actors.len() as u64)),
                ("use_cases", number(d.use_cases.len() as u64)),
                (
                    "acceptance_criteria",
                    number(d.acceptance_criteria.len() as u64),
                ),
                ("nfrs", number(d.nfrs.len() as u64)),
                ("assumptions", number(d.assumptions.len() as u64)),
                ("blockers", number(d.blockers.len() as u64)),
            ])
        });
        let findings = parsed.as_ref().map_or(Json::Null, |d| {
            Json::List(
                d.validate()
                    .into_iter()
                    .take(DOCUMENT_ROWS)
                    .map(|f| {
                        object([
                            ("path", text(f.path)),
                            ("problem", text(finding_word(f.problem))),
                        ])
                    })
                    .collect(),
            )
        });
        let criteria = parsed.as_ref().map_or(Json::Null, |d| {
            Json::List(
                d.acceptance_criteria
                    .iter()
                    .take(DOCUMENT_ROWS)
                    .map(|ac| {
                        let cases = d
                            .use_cases
                            .iter()
                            .filter(|u| u.acceptance_ids.contains(&ac.id))
                            .map(|u| text(u.id.clone()))
                            .collect();
                        object([
                            ("id", text(ac.id.clone())),
                            ("statement", text(cut(&ac.statement))),
                            ("use_cases", Json::List(cases)),
                        ])
                    })
                    .collect(),
            )
        });
        let links = c.query_optional(
            "SELECT prd, adr FROM review_documents WHERE id=?1 AND revision=?2",
            &[Value::Text(&id), Value::Int(chosen.revision)],
            |r| Ok((r.text(0)?, r.optional_text(1)?)),
        )?;
        let mut upstream = Vec::new();
        if let Some((prd_id, adr_id)) = links {
            upstream.push(upstream_row(c, &prd_id)?);
            if let Some(adr_id) = adr_id {
                upstream.push(upstream_row(c, &adr_id)?);
            }
        }
        let body = &chosen.rendered;
        let total = body.chars().count();
        let chunk: String = body.chars().skip(offset).take(CHAT_TEXT_CHARS).collect();
        let next = offset.saturating_add(chunk.chars().count());
        let (feature, feature_name) = feature_fields(c, document_feature(c, &id)?)?;
        Ok(object([
            ("document", identity(document.0)),
            ("kind", text(kind)),
            (
                "title",
                optional(if prd {
                    prd_title(&chosen.content)
                } else {
                    None
                }),
            ),
            ("feature", feature),
            ("feature_name", feature_name),
            ("revision", Json::Int(chosen.revision)),
            ("newest", Json::Int(newest.revision)),
            ("approval", text(approval)),
            ("stale", Json::Bool(stale)),
            ("revisions", Json::List(revisions)),
            ("history", Json::List(history)),
            ("upstream", Json::List(upstream)),
            ("downstream", Json::List(downstream(c, &id)?)),
            ("counts", counts),
            ("findings", findings),
            ("criteria", criteria),
            ("offset", number(offset as u64)),
            ("next", number(next as u64)),
            ("total", number(total as u64)),
            ("complete", Json::Bool(next >= total)),
            ("text", text(chunk)),
        ]))
    }

    /// Protocol 1.9: decisions on documents newest first, at most [`OVERVIEW_ROWS`]
    /// recorded before event `before`; each says whether it still applies (an approval
    /// stops applying when a newer revision or a later decision replaces it).
    pub fn ipc_decisions(&self, before: Option<u64>) -> Result<Json, StorageError> {
        let c = self.conn;
        let before = before.map_or(Ok(i64::MAX), to_db)?;
        let rows = c.query(
            "SELECT seq, subject, revision, action FROM human_decisions WHERE seq<?1
             ORDER BY seq DESC LIMIT ?2",
            &[Value::Int(before), Value::Int(OVERVIEW_ROWS as i64 + 1)],
            |r| Ok((r.int(0)?, r.text(1)?, r.int(2)?, r.text(3)?)),
        )?;
        let more = rows.len() > OVERVIEW_ROWS;
        let mut out = Vec::new();
        for (seq, subject, revision, action) in rows.into_iter().take(OVERVIEW_ROWS) {
            let (stored, revs) = revision_rows(c, &subject)?;
            let prd = stored == "PRD";
            let kind = if prd {
                "Prd"
            } else if stored.is_empty() {
                "Unknown"
            } else {
                review_kind(c, &subject, &stored)?
            };
            let title = revs
                .iter()
                .find(|r| r.revision == revision)
                .filter(|_| prd)
                .and_then(|r| prd_title(&r.content));
            let document = ArtifactId(id_from_text(&subject)?);
            let current = bundle(c, document)?.map(|b| b.hash);
            let applies = decisions(c, document)?.last().is_some_and(|d| {
                to_db(d.seq.0).ok() == Some(seq) && Some(&d.bundle) == current.as_ref()
            });
            let (feature, feature_name) = feature_fields(c, document_feature(c, &subject)?)?;
            out.push(object([
                ("order", Json::Int(seq)),
                ("document", identity_text(&subject)?),
                ("kind", text(kind)),
                ("title", optional(title)),
                ("feature", feature),
                ("feature_name", feature_name),
                ("revision", Json::Int(revision)),
                ("decision", text(decision_word(&action))),
                ("applies", Json::Bool(applies)),
                ("at_unix_ms", time_of(c, seq)?),
            ]));
        }
        let next = if more {
            out.last()
                .and_then(|row| jojo_domain::ipc::get(row, "order").ok().cloned())
                .unwrap_or(Json::Null)
        } else {
            Json::Null
        };
        Ok(object([("decisions", Json::List(out)), ("next", next)]))
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

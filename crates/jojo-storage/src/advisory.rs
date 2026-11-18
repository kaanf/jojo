//! Follow-ups (protocol 1.14, schema 19): advisory reviews.
//!
//! On the human's request, the provider that did not do the work reads it once, read-only,
//! in the registered repository at its HEAD, like a plan step's review (M4): the sealed
//! candidate of an attempt started by hand, or one revision of a requirements document,
//! architecture decision or execution plan a lead drafted. Its prompt is composed by the
//! helper from what the core stores, kept with it and frozen by its digest; its verdict is
//! stored as the reviewer gave it. An advisory verdict approves nothing, rejects nothing
//! and holds nothing back: the human still decides where they always do. It runs and is
//! sealed through the same functions as a step's review (`repairs.rs`), one review of
//! either kind at a time, and takes one unit of the global capacity.
use crate::digest::sha256_hex;
use crate::engine::ReadView;
use crate::error::{StorageError, TxError};
use crate::model::{id_from_text, id_to_text, to_db};
use crate::repairs::{ADVISORY_REVIEWS, reviewer_of};
use crate::sqlite::{Connection, Value};
use crate::tx::Tx;
use crate::{Admission, SliceSubjects};
use jojo_domain::dependency::NodeRef;
use jojo_domain::identity::{ArtifactId, OpaqueId, RepositoryId, Revision};
use jojo_domain::ipc::{identity, object, text};
use jojo_domain::json::Json;
use jojo_domain::project::{provider_name, provider_named};
use jojo_domain::provider::{MAX_PROMPT_BYTES, ProviderKind};

fn deny(s: impl Into<String>) -> TxError {
    TxError::Rejected(s.into())
}

/// What an advisory review reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdvisorySubject {
    Attempt {
        attempt: OpaqueId,
        candidate: String,
    },
    Document {
        document: ArtifactId,
        revision: u64,
    },
}

/// What the helper asks the core to accept as an advisory review.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdvisoryRequest {
    pub review: OpaqueId,
    pub subject: AdvisorySubject,
    pub provider: ProviderKind,
    pub repository: RepositoryId,
    /// The commit the repository's HEAD named when the review was accepted.
    pub base: String,
    pub prompt: String,
}

/// An attempt started by hand, as its review needs it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttemptReviewSubjects {
    pub subjects: SliceSubjects,
    pub step: NodeRef,
    pub repository: RepositoryId,
    /// The provider that did the attempt; the other one reviews.
    pub developer: ProviderKind,
}

/// A drafted document, as its review needs it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DocumentReviewSubjects {
    /// "requirements document", "architecture decision" or "execution plan".
    pub kind: &'static str,
    pub body: String,
    /// For an architecture decision or a plan: the requirements it was drafted on.
    pub requirements: Option<(u64, String)>,
    pub repository: RepositoryId,
    /// The lead's provider; the other one reviews.
    pub drafter: ProviderKind,
}

/// Why the human may not ask for a review of `attempt`'s `candidate`, or what it reads.
fn attempt_subjects(
    c: &Connection,
    attempt: OpaqueId,
    candidate: &str,
) -> Result<Result<AttemptReviewSubjects, String>, StorageError> {
    let a = id_to_text(attempt);
    let sealed = c.query_optional(
        "SELECT candidate FROM attempt_results WHERE attempt=?1",
        &[Value::Text(&a)],
        |r| r.optional_text(0),
    )?;
    match sealed {
        None => return Ok(Err("ReviewRefused: the attempt has not ended".into())),
        Some(found) if found.as_deref() != Some(candidate) => {
            return Ok(Err(
                "ReviewRefused: the candidate is not the attempt's sealed candidate".into(),
            ));
        }
        Some(_) => {}
    }
    // A plan run's step is reviewed by the runner (M4); this is for a step started by hand.
    let by_runner = c
        .query_optional(
            "SELECT 1 FROM commands WHERE command_id=?1 AND kind='PlanStepStart'",
            &[Value::Text(&a)],
            |r| r.int(0),
        )?
        .is_some();
    if by_runner {
        return Ok(Err("ReviewRefused: a plan run reviews its own steps".into()));
    }
    let row = c.query_optional(
        "SELECT p.plan, s.step, s.step_revision, l.provider FROM approval_attempts p
         JOIN scheduler_attempts s ON s.attempt=p.id
         JOIN process_launches l ON l.attempt=p.id WHERE p.id=?1",
        &[Value::Text(&a)],
        |r| Ok((r.text(0)?, r.text(1)?, r.int(2)?, r.text(3)?)),
    )?;
    let Some((plan, step, step_revision, provider)) = row else {
        return Ok(Err("ReviewRefused: the attempt was never launched".into()));
    };
    let Some(developer) = provider_named(&provider) else {
        return Err(StorageError::corrupt("attempt provider"));
    };
    let Some((_, subjects, repository, _)) =
        crate::runner::lead_subjects(c, ArtifactId(id_from_text(&plan)?))?
    else {
        return Ok(Err(
            "ReviewRefused: only a step of a plan the lead drafted is reviewed".into(),
        ));
    };
    Ok(Ok(AttemptReviewSubjects {
        subjects,
        step: NodeRef {
            id: id_from_text(&step)?,
            revision: Revision(crate::model::from_db(step_revision)?),
        },
        repository,
        developer,
    }))
}

/// Why the human may not ask for a review of `document` at `revision`, or what it reads.
fn document_subjects(
    c: &Connection,
    document: ArtifactId,
    revision: u64,
) -> Result<Result<DocumentReviewSubjects, String>, StorageError> {
    let id = id_to_text(document.0);
    let r = to_db(revision)?;
    let prd = c.query_optional(
        "SELECT v.rendered, t.provider, t.repository FROM artifact_versions v
         JOIN lead_turn_results x ON x.prd=v.artifact_id AND x.prd_revision=v.revision
         JOIN lead_turns t ON t.turn=x.turn
         WHERE v.artifact_id=?1 AND v.revision=?2 AND v.artifact_type='PRD'",
        &[Value::Text(&id), Value::Int(r)],
        |row| Ok((row.text(0)?, row.text(1)?, row.text(2)?)),
    )?;
    let (kind, body, provider, repository, requirements) = match prd {
        Some((body, provider, repository)) => {
            ("requirements document", body, provider, repository, None)
        }
        None => {
            let drafted = c.query_optional(
                "SELECT d.rendered, d.kind, d.prd, t.provider, t.repository
                 FROM review_documents d
                 JOIN lead_plan_subjects s ON s.adr=d.id OR s.plan=d.id
                 JOIN lead_turns t ON t.turn=s.turn
                 WHERE d.id=?1 AND d.revision=?2",
                &[Value::Text(&id), Value::Int(r)],
                |row| {
                    Ok((
                        row.text(0)?,
                        row.text(1)?,
                        row.text(2)?,
                        row.text(3)?,
                        row.text(4)?,
                    ))
                },
            )?;
            let Some((body, stored, prd, provider, repository)) = drafted else {
                return Ok(Err(
                    "ReviewRefused: only a requirements document, architecture decision or \
                     plan a lead drafted is reviewed"
                        .into(),
                ));
            };
            let requirements = crate::artifacts::version(c, ArtifactId(id_from_text(&prd)?), None)?
                .map(|v| (v.revision.0, v.rendered));
            let kind = if stored == "ADR" {
                "architecture decision"
            } else {
                "execution plan"
            };
            (kind, body, provider, repository, requirements)
        }
    };
    let Some(drafter) = provider_named(&provider) else {
        return Err(StorageError::corrupt("lead turn provider"));
    };
    Ok(Ok(DocumentReviewSubjects {
        kind,
        body,
        requirements,
        repository: RepositoryId(id_from_text(&repository)?),
        drafter,
    }))
}

/// The newest advisory review of a subject, as the attempt and document details carry it
/// at 1.14, with whether the human may ask for one now and, if not, why.
fn advisory_json(
    c: &Connection,
    newest: Option<String>,
    offer: Result<ProviderKind, String>,
) -> Result<Json, StorageError> {
    let review = match newest {
        None => Json::Null,
        Some(review) => {
            let (provider, event, document_revision) = c.query_one(
                "SELECT provider, event, document_revision FROM advisory_reviews WHERE review=?1",
                &[Value::Text(&review)],
                |r| Ok((r.text(0)?, r.int(1)?, r.optional_int(2)?)),
            )?;
            let result = c.query_optional(
                "SELECT outcome, notes, objection, detail, event FROM advisory_review_results
                 WHERE review=?1",
                &[Value::Text(&review)],
                |r| {
                    Ok((
                        r.text(0)?,
                        r.text(1)?,
                        r.optional_text(2)?,
                        r.text(3)?,
                        r.int(4)?,
                    ))
                },
            )?;
            let stopping = c
                .query_optional(
                    "SELECT 1 FROM advisory_review_stops WHERE review=?1",
                    &[Value::Text(&review)],
                    |r| r.int(0),
                )?
                .is_some();
            let (state, notes, objection, detail, finished) = match result {
                None if stopping => ("Stopping", String::new(), None, String::new(), Json::Null),
                None => ("Running", String::new(), None, String::new(), Json::Null),
                Some((outcome, notes, objection, detail, at)) => (
                    match outcome.as_str() {
                        "APPROVED" => "Approved",
                        "REJECTED" => "Rejected",
                        "CANCELLED" => "Cancelled",
                        "INTERRUPTED" => "Interrupted",
                        _ => "Failed",
                    },
                    notes,
                    objection,
                    detail,
                    crate::overview::time_of(c, at)?,
                ),
            };
            object([
                ("review", identity(id_from_text(&review)?)),
                ("provider", text(provider)),
                ("state", text(state)),
                ("notes", text(notes)),
                ("objection", objection.map_or(Json::Null, text)),
                ("detail", text(detail)),
                ("revision", document_revision.map_or(Json::Null, Json::Int)),
                ("requested_at_unix_ms", crate::overview::time_of(c, event)?),
                ("finished_at_unix_ms", finished),
            ])
        }
    };
    let running = crate::repairs::unfinished_review(c)?.is_some();
    let (available, reviewer, reason) = match offer {
        Ok(_) if running => (
            false,
            Json::Null,
            text("Another review is running; ask again when it ends."),
        ),
        Ok(developer) => (
            true,
            text(provider_name(reviewer_of(developer))),
            Json::Null,
        ),
        Err(why) => (
            false,
            Json::Null,
            text(why.trim_start_matches("ReviewRefused: ").to_owned()),
        ),
    };
    Ok(object([
        ("review", review),
        ("available", Json::Bool(available)),
        ("reviewer", reviewer),
        ("reason", reason),
    ]))
}

impl ReadView<'_> {
    pub fn attempt_review_subjects(
        &self,
        attempt: OpaqueId,
        candidate: &str,
    ) -> Result<Result<AttemptReviewSubjects, String>, StorageError> {
        attempt_subjects(self.conn, attempt, candidate)
    }

    pub fn document_review_subjects(
        &self,
        document: ArtifactId,
        revision: u64,
    ) -> Result<Result<DocumentReviewSubjects, String>, StorageError> {
        document_subjects(self.conn, document, revision)
    }

    /// Protocol 1.14: the advisory review of an attempt's sealed candidate, for its detail.
    pub(crate) fn attempt_advisory(&self, attempt: OpaqueId) -> Result<Json, StorageError> {
        let c = self.conn;
        let a = id_to_text(attempt);
        let newest = c.query_optional(
            "SELECT review FROM advisory_reviews WHERE attempt=?1 ORDER BY event DESC LIMIT 1",
            &[Value::Text(&a)],
            |r| r.text(0),
        )?;
        let candidate = c
            .query_optional(
                "SELECT candidate FROM attempt_results WHERE attempt=?1",
                &[Value::Text(&a)],
                |r| r.optional_text(0),
            )?
            .flatten();
        let offer = match candidate {
            None => Err("ReviewRefused: the attempt has no sealed candidate".to_owned()),
            Some(candidate) => attempt_subjects(c, attempt, &candidate)?.map(|s| s.developer),
        };
        advisory_json(c, newest, offer)
    }

    /// Protocol 1.14: the advisory review of one revision of a document, for its detail.
    pub(crate) fn document_advisory(
        &self,
        document: ArtifactId,
        revision: u64,
    ) -> Result<Json, StorageError> {
        let c = self.conn;
        let newest = c.query_optional(
            "SELECT review FROM advisory_reviews WHERE document=?1 AND document_revision=?2
             ORDER BY event DESC LIMIT 1",
            &[
                Value::Text(&id_to_text(document.0)),
                Value::Int(to_db(revision)?),
            ],
            |r| r.text(0),
        )?;
        let offer = document_subjects(c, document, revision)?.map(|s| s.drafter);
        advisory_json(c, newest, offer)
    }
}

impl Tx<'_> {
    /// The durable acceptance of one advisory review. Re-reads admission, capacity and the
    /// subject; no other review may run and the reviewer must be the provider that did
    /// not do the work.
    pub fn accept_advisory_review(&mut self, r: &AdvisoryRequest) -> Result<String, TxError> {
        if self.admission()? != Admission::Open {
            return Err(deny("AdmissionClosed"));
        }
        if crate::repairs::unfinished_review(self.conn)?.is_some() {
            return Err(deny("ReviewRefused: another review is running"));
        }
        self.capacity_left("a review")?;
        let (other, repository) = match &r.subject {
            AdvisorySubject::Attempt { attempt, candidate } => {
                let s = attempt_subjects(self.conn, *attempt, candidate)?.map_err(deny)?;
                (s.developer, s.repository)
            }
            AdvisorySubject::Document { document, revision } => {
                let s = document_subjects(self.conn, *document, *revision)?.map_err(deny)?;
                (s.drafter, s.repository)
            }
        };
        if r.provider != reviewer_of(other) {
            return Err(deny(
                "ReviewRefused: the work is reviewed by the other provider",
            ));
        }
        if r.repository != repository {
            return Err(deny("ReviewRefused: the repository differs"));
        }
        if r.prompt.is_empty() || r.prompt.len() > MAX_PROMPT_BYTES || r.prompt.contains('\0') {
            return Err(deny(
                "PromptTooLarge: the review prompt is empty or too large",
            ));
        }
        let review = id_to_text(r.review);
        let (subject, attempt, candidate, document, revision) = match &r.subject {
            AdvisorySubject::Attempt { attempt, candidate } => (
                "ATTEMPT",
                Some(id_to_text(*attempt)),
                Some(candidate.clone()),
                None,
                None,
            ),
            AdvisorySubject::Document { document, revision } => (
                "DOCUMENT",
                None,
                None,
                Some(id_to_text(document.0)),
                Some(to_db(*revision)?),
            ),
        };
        let event = self.append_event(
            "AdvisoryReviewAccepted",
            None,
            &format!(
                "{review}|{subject}|{}|{}",
                attempt.as_deref().or(document.as_deref()).unwrap_or("-"),
                provider_name(r.provider)
            ),
        )?;
        self.conn.execute(
            &format!(
                "INSERT INTO {} VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                ADVISORY_REVIEWS.reviews
            ),
            &[
                Value::Text(&review),
                Value::Text(subject),
                attempt.as_deref().map_or(Value::Null, Value::Text),
                candidate.as_deref().map_or(Value::Null, Value::Text),
                document.as_deref().map_or(Value::Null, Value::Text),
                revision.map_or(Value::Null, Value::Int),
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
}

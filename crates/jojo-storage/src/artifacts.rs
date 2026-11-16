//! Immutable artifact versions and clarification questions/answers (P1-03).
//!
//! An artifact is a revisioned aggregate whose revision *is* its version number: creating
//! or editing it advances the revision by compare-and-swap and inserts one immutable
//! `artifact_versions` row holding the canonical content, the exact rendered review bytes
//! and the SHA-256 of each. Nothing here ever rewrites a version.
//!
//! Review readiness is a computed, structural precondition for human review. It is not an
//! approval and it does not claim the content is right; decisions belong to P1-04.

use jojo_domain::identity::{ArtifactId, FeatureId, QuestionId, Revision};
use jojo_domain::prd::{Finding, PRD_SCHEMA_VERSION, PrdDocument, PrdError};

use crate::digest::sha256_hex;
use crate::engine::ReadView;
use crate::error::{StorageError, TxError};
use crate::model::{
    AggregateKey, EngineEpoch, EventSeq, from_db, id_from_text, id_to_text, text_enum, to_db,
};
use crate::queries;
use crate::sqlite::{Connection, Row, SqliteError, Value};
use crate::tx::Tx;

/// Aggregate kind of an artifact; its revision is the artifact's newest version.
pub const ARTIFACT_KIND: &str = "artifact";
/// Aggregate kind of a clarification question; its state is the question's disposition.
pub const QUESTION_KIND: &str = "question";
/// Largest question prompt, answer body or provenance note, in bytes.
pub const MAX_TEXT_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactType {
    Prd,
}

/// Who authored a record. Stored with every version, question and answer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Author {
    Human,
    Model,
}

/// Authorship plus free text: where the content came from, including the source and
/// licence notice of any adapted template. Stored verbatim and never rewritten.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Provenance {
    pub author: Author,
    pub note: String,
}

impl Provenance {
    pub fn new(author: Author, note: impl Into<String>) -> Self {
        Self {
            author,
            note: note.into(),
        }
    }
}

/// `Blocking` questions must have a human answer before review; `Deferred` ones were
/// explicitly set aside and do not block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuestionDisposition {
    Blocking,
    Deferred,
}

text_enum!(ArtifactType { ArtifactType::Prd => "PRD" });
text_enum!(Author { Author::Human => "HUMAN", Author::Model => "MODEL" });
text_enum!(QuestionDisposition {
    QuestionDisposition::Blocking => "BLOCKING",
    QuestionDisposition::Deferred => "DEFERRED",
});

/// PRD content that passed strict import, in canonical and rendered form with digests.
/// Built outside the write transaction; the only way content enters `artifact_versions`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedPrd {
    content: String,
    content_digest: String,
    rendered: String,
    rendered_digest: String,
}

impl PreparedPrd {
    /// Imports bounded JSON. Malformed, ambiguous, oversized or mis-shaped input is refused
    /// here; structurally incomplete content is accepted as a draft and reported by
    /// [`ReviewReadiness`] instead.
    pub fn import(input: &[u8]) -> Result<Self, PrdError> {
        PrdDocument::import(input).map(|document| Self::from_document(&document))
    }

    /// The prepared form of a document that already passed strict import.
    pub fn from_document(document: &PrdDocument) -> Self {
        let content = document.canonical_json();
        let rendered = document.render_markdown();
        Self {
            content_digest: sha256_hex(&[content.as_bytes()]),
            rendered_digest: sha256_hex(&[rendered.as_bytes()]),
            content,
            rendered,
        }
    }

    /// Canonical typed content; `content_digest` is the SHA-256 of exactly these bytes.
    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn content_digest(&self) -> &str {
        &self.content_digest
    }

    /// The Markdown shown for review; `rendered_digest` is the SHA-256 of these bytes.
    pub fn rendered(&self) -> &str {
        &self.rendered
    }

    pub fn rendered_digest(&self) -> &str {
        &self.rendered_digest
    }
}

/// One immutable version. `(artifact, revision)` is its identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactVersion {
    pub artifact: ArtifactId,
    pub revision: Revision,
    pub parent: Option<Revision>,
    pub feature: FeatureId,
    pub artifact_type: ArtifactType,
    pub schema_version: u32,
    pub content: String,
    pub content_digest: String,
    pub rendered: String,
    pub rendered_digest: String,
    pub provenance: Provenance,
    pub created_event: EventSeq,
    pub created_epoch: EngineEpoch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Question {
    pub id: QuestionId,
    /// Advances with every answer and disposition change.
    pub revision: Revision,
    pub feature: FeatureId,
    /// The artifact the question is about, or `None` for the feature as a whole.
    pub artifact: Option<ArtifactId>,
    pub prompt: String,
    pub provenance: Provenance,
    pub disposition: QuestionDisposition,
    pub created_event: EventSeq,
}

/// Answers are versioned: a later answer supersedes, never replaces, an earlier one.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Answer {
    pub question: QuestionId,
    pub ordinal: u64,
    pub body: String,
    pub provenance: Provenance,
    pub created_event: EventSeq,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReadinessBlocker {
    /// A structural finding in the content, with its field path.
    Incomplete(Finding),
    /// A blocking question whose newest answer is missing or not from a human.
    UnansweredQuestion(QuestionId),
}

/// Whether one exact version may be offered for human review, and if not, why.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewReadiness {
    pub artifact: ArtifactId,
    pub revision: Revision,
    pub content_digest: String,
    pub rendered_digest: String,
    pub blockers: Vec<ReadinessBlocker>,
}

impl ReviewReadiness {
    pub fn is_ready(&self) -> bool {
        self.blockers.is_empty()
    }
}

fn artifact_key(id: ArtifactId) -> AggregateKey {
    AggregateKey::new(ARTIFACT_KIND, id.0)
}

fn question_key(id: QuestionId) -> AggregateKey {
    AggregateKey::new(QUESTION_KIND, id.0)
}

fn rejected(message: impl Into<String>) -> TxError {
    TxError::Rejected(message.into())
}

fn bounded(what: &str, text: &str, required: bool) -> Result<(), TxError> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(rejected(format!("{what} exceeds {MAX_TEXT_BYTES} bytes")));
    }
    if text.contains('\0') {
        return Err(rejected(format!("{what} contains NUL")));
    }
    if required && !text.chars().any(char::is_alphanumeric) {
        return Err(rejected(format!("{what} is empty")));
    }
    Ok(())
}

impl Tx<'_> {
    /// Creates an artifact with its first version.
    pub fn create_artifact(
        &mut self,
        id: ArtifactId,
        feature: FeatureId,
        prepared: &PreparedPrd,
        provenance: &Provenance,
    ) -> Result<ArtifactVersion, TxError> {
        bounded("provenance", &provenance.note, false)?;
        if self
            .aggregate(&AggregateKey::new("review_document", id.0))?
            .is_some()
        {
            return Err(rejected("subject identity already used"));
        }
        let revision = self.create_aggregate(&artifact_key(id), &prepared.content_digest)?;
        self.insert_version(id, revision, feature, prepared, provenance)
    }

    /// Records an edit as a new version on top of `expected`, which must be the newest
    /// version. Earlier versions are untouched. Identical content is not an edit.
    pub fn revise_artifact(
        &mut self,
        id: ArtifactId,
        expected: Revision,
        prepared: &PreparedPrd,
        provenance: &Provenance,
    ) -> Result<ArtifactVersion, TxError> {
        bounded("provenance", &provenance.note, false)?;
        let key = artifact_key(id);
        let parent = match version(self.conn, id, Some(expected))? {
            Some(parent) => parent,
            None => {
                return Err(TxError::RevisionConflict {
                    actual: self.aggregate(&key)?.map(|record| record.revision),
                    key,
                    expected,
                });
            }
        };
        let revision = self.update_aggregate(&key, expected, &prepared.content_digest)?;
        if parent.content_digest == prepared.content_digest {
            return Err(rejected("content is identical to the current version"));
        }
        self.insert_version(id, revision, parent.feature, prepared, provenance)
    }

    fn insert_version(
        &mut self,
        id: ArtifactId,
        revision: Revision,
        feature: FeatureId,
        prepared: &PreparedPrd,
        provenance: &Provenance,
    ) -> Result<ArtifactVersion, TxError> {
        let parent = (revision.0 > 1).then(|| Revision(revision.0 - 1));
        let created_event = self.append_event(
            "ArtifactVersionCreated",
            Some((&artifact_key(id), revision)),
            &format!(
                "type=PRD;content_sha256={};rendered_sha256={}",
                prepared.content_digest, prepared.rendered_digest
            ),
        )?;
        self.conn.execute(
            "INSERT INTO artifact_versions
                 (artifact_kind, artifact_id, revision, parent_revision, feature_id, artifact_type,
                  schema_version, content, content_digest, rendered, rendered_digest, author,
                  provenance, created_event_seq, created_epoch)
             VALUES ('artifact', ?1, ?2, ?3, ?4, 'PRD', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            &[
                Value::Text(&id_to_text(id.0)),
                Value::Int(to_db(revision.0)?),
                match parent {
                    Some(parent) => Value::Int(to_db(parent.0)?),
                    None => Value::Null,
                },
                Value::Text(&id_to_text(feature.0)),
                Value::Int(i64::from(PRD_SCHEMA_VERSION)),
                Value::Text(&prepared.content),
                Value::Text(&prepared.content_digest),
                Value::Text(&prepared.rendered),
                Value::Text(&prepared.rendered_digest),
                Value::Text(provenance.author.as_db()),
                Value::Text(&provenance.note),
                Value::Int(to_db(created_event.0)?),
                Value::Int(to_db(self.epoch.0)?),
            ],
        )?;
        self.invalidate_approvals()?;
        Ok(ArtifactVersion {
            artifact: id,
            revision,
            parent,
            feature,
            artifact_type: ArtifactType::Prd,
            schema_version: PRD_SCHEMA_VERSION,
            content: prepared.content.clone(),
            content_digest: prepared.content_digest.clone(),
            rendered: prepared.rendered.clone(),
            rendered_digest: prepared.rendered_digest.clone(),
            provenance: provenance.clone(),
            created_event,
            created_epoch: self.epoch,
        })
    }

    /// Persists a clarification question. `artifact`, when given, must exist.
    pub fn ask_question(
        &mut self,
        id: QuestionId,
        feature: FeatureId,
        artifact: Option<ArtifactId>,
        prompt: &str,
        disposition: QuestionDisposition,
        provenance: &Provenance,
    ) -> Result<Revision, TxError> {
        bounded("question", prompt, true)?;
        bounded("provenance", &provenance.note, false)?;
        if let Some(artifact) = artifact
            && self.aggregate(&artifact_key(artifact))?.is_none()
        {
            return Err(rejected("question refers to an unknown artifact"));
        }
        let key = question_key(id);
        let revision = self.create_aggregate(&key, disposition.as_db())?;
        let event = self.append_event("QuestionAsked", Some((&key, revision)), "")?;
        let artifact_id = artifact.map(|artifact| id_to_text(artifact.0));
        self.conn.execute(
            "INSERT INTO clarification_questions
                 (question_kind, question_id, feature_id, artifact_kind, artifact_id, prompt,
                  author, provenance, created_event_seq, created_epoch)
             VALUES ('question', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            &[
                Value::Text(&id_to_text(id.0)),
                Value::Text(&id_to_text(feature.0)),
                artifact_id
                    .as_ref()
                    .map_or(Value::Null, |_| Value::Text(ARTIFACT_KIND)),
                artifact_id.as_deref().map_or(Value::Null, Value::Text),
                Value::Text(prompt),
                Value::Text(provenance.author.as_db()),
                Value::Text(&provenance.note),
                Value::Int(to_db(event.0)?),
                Value::Int(to_db(self.epoch.0)?),
            ],
        )?;
        self.invalidate_approvals()?;
        Ok(revision)
    }

    /// Appends the next answer to a question at revision `expected`. The disposition is
    /// unchanged; whether the question still blocks is computed from the newest answer.
    pub fn answer_question(
        &mut self,
        id: QuestionId,
        expected: Revision,
        body: &str,
        provenance: &Provenance,
    ) -> Result<Revision, TxError> {
        bounded("answer", body, true)?;
        bounded("provenance", &provenance.note, false)?;
        let key = question_key(id);
        let state = self.aggregate(&key)?.map(|record| record.state);
        let revision = self.update_aggregate(&key, expected, state.as_deref().unwrap_or(""))?;
        let event = self.append_event(
            "QuestionAnswered",
            Some((&key, revision)),
            provenance.author.as_db(),
        )?;
        self.conn.execute(
            "INSERT INTO clarification_answers
                 (question_id, ordinal, body, author, provenance, created_event_seq, created_epoch)
             SELECT ?1, coalesce(max(ordinal), 0) + 1, ?2, ?3, ?4, ?5, ?6
             FROM clarification_answers WHERE question_id = ?1",
            &[
                Value::Text(&id_to_text(id.0)),
                Value::Text(body),
                Value::Text(provenance.author.as_db()),
                Value::Text(&provenance.note),
                Value::Int(to_db(event.0)?),
                Value::Int(to_db(self.epoch.0)?),
            ],
        )?;
        self.invalidate_approvals()?;
        Ok(revision)
    }

    /// Defers a blocking question or makes a deferred one blocking again.
    pub fn set_question_disposition(
        &mut self,
        id: QuestionId,
        expected: Revision,
        disposition: QuestionDisposition,
    ) -> Result<Revision, TxError> {
        let key = question_key(id);
        let revision = self.update_aggregate(&key, expected, disposition.as_db())?;
        self.append_event(
            "QuestionDispositionChanged",
            Some((&key, revision)),
            disposition.as_db(),
        )?;
        self.invalidate_approvals()?;
        Ok(revision)
    }

    /// Review readiness of the newest version, evaluated inside this transaction.
    pub fn review_readiness(&self, id: ArtifactId) -> Result<Option<ReviewReadiness>, TxError> {
        Ok(review_readiness(self.conn, id)?)
    }
}

impl ReadView<'_> {
    /// One version; `revision = None` selects the newest.
    pub fn artifact_version(
        &self,
        id: ArtifactId,
        revision: Option<Revision>,
    ) -> Result<Option<ArtifactVersion>, StorageError> {
        version(self.conn, id, revision)
    }

    /// Every version of an artifact, oldest first.
    pub fn artifact_versions(&self, id: ArtifactId) -> Result<Vec<ArtifactVersion>, StorageError> {
        versions(self.conn, id, None)
    }

    pub fn question(&self, id: QuestionId) -> Result<Option<Question>, StorageError> {
        question(self.conn, id)
    }

    /// Every answer to a question, oldest first.
    pub fn answers(&self, id: QuestionId) -> Result<Vec<Answer>, StorageError> {
        answers(self.conn, id)
    }

    /// Review readiness of the newest version; `None` if the artifact does not exist.
    pub fn review_readiness(
        &self,
        id: ArtifactId,
    ) -> Result<Option<ReviewReadiness>, StorageError> {
        review_readiness(self.conn, id)
    }
}

type Raw<T> = Result<T, SqliteError>;

fn provenance(author: &str, note: String) -> Result<Provenance, StorageError> {
    Ok(Provenance {
        author: Author::from_db(author)?,
        note,
    })
}

fn versions(
    conn: &Connection,
    id: ArtifactId,
    revision: Option<Revision>,
) -> Result<Vec<ArtifactVersion>, StorageError> {
    let filter = match revision {
        Some(revision) => Value::Int(to_db(revision.0)?),
        None => Value::Null,
    };
    conn.query(
        "SELECT revision, parent_revision, feature_id, artifact_type, schema_version, content,
                content_digest, rendered, rendered_digest, author, provenance,
                created_event_seq, created_epoch
         FROM artifact_versions
         WHERE artifact_id = ?1 AND (?2 IS NULL OR revision = ?2) ORDER BY revision",
        &[Value::Text(&id_to_text(id.0)), filter],
        |r: &Row<'_>| -> Raw<_> {
            Ok((
                (r.int(0)?, r.optional_int(1)?, r.text(2)?, r.text(3)?),
                (r.int(4)?, r.text(5)?, r.text(6)?, r.text(7)?, r.text(8)?),
                (r.text(9)?, r.text(10)?, r.int(11)?, r.int(12)?),
            ))
        },
    )?
    .into_iter()
    .map(|(identity, body, origin)| {
        let (revision, parent, feature, artifact_type) = identity;
        let (schema_version, content, content_digest, rendered, rendered_digest) = body;
        let (author, note, created_event, created_epoch) = origin;
        // The digests identify the stored bytes; a mismatch means the row was altered
        // outside this code and nothing may be reviewed or approved against it.
        if sha256_hex(&[content.as_bytes()]) != content_digest
            || sha256_hex(&[rendered.as_bytes()]) != rendered_digest
        {
            return Err(StorageError::corrupt(format!(
                "artifact version {}@{revision} does not match its digests",
                id_to_text(id.0)
            )));
        }
        Ok(ArtifactVersion {
            artifact: id,
            revision: Revision(from_db(revision)?),
            parent: parent.map(from_db).transpose()?.map(Revision),
            feature: FeatureId(id_from_text(&feature)?),
            artifact_type: ArtifactType::from_db(&artifact_type)?,
            schema_version: u32::try_from(schema_version)
                .map_err(|_| StorageError::corrupt("artifact schema version out of range"))?,
            content,
            content_digest,
            rendered,
            rendered_digest,
            provenance: provenance(&author, note)?,
            created_event: EventSeq(from_db(created_event)?),
            created_epoch: EngineEpoch(from_db(created_epoch)?),
        })
    })
    .collect()
}

/// One version, or the newest. The newest version must be the aggregate's revision.
pub(crate) fn version(
    conn: &Connection,
    id: ArtifactId,
    revision: Option<Revision>,
) -> Result<Option<ArtifactVersion>, StorageError> {
    let head = match queries::aggregate(conn, &artifact_key(id))? {
        Some(record) => record.revision,
        None => return Ok(None),
    };
    let found = versions(conn, id, Some(revision.unwrap_or(head)))?.pop();
    if found.is_none() && revision.is_none_or(|revision| revision == head) {
        return Err(StorageError::corrupt(format!(
            "artifact {} has no version row for revision {}",
            id_to_text(id.0),
            head.0
        )));
    }
    Ok(found)
}

fn question(conn: &Connection, id: QuestionId) -> Result<Option<Question>, StorageError> {
    let Some(aggregate) = queries::aggregate(conn, &question_key(id))? else {
        return Ok(None);
    };
    let row = conn.query_optional(
        "SELECT feature_id, artifact_id, prompt, author, provenance, created_event_seq
         FROM clarification_questions WHERE question_id = ?1",
        &[Value::Text(&id_to_text(id.0))],
        |r: &Row<'_>| -> Raw<_> {
            Ok((
                r.text(0)?,
                r.optional_text(1)?,
                r.text(2)?,
                r.text(3)?,
                r.text(4)?,
                r.int(5)?,
            ))
        },
    )?;
    let Some((feature, artifact, prompt, author, note, created_event)) = row else {
        return Err(StorageError::corrupt(
            "question aggregate without a question",
        ));
    };
    Ok(Some(Question {
        id,
        revision: aggregate.revision,
        feature: FeatureId(id_from_text(&feature)?),
        artifact: artifact
            .map(|artifact| id_from_text(&artifact).map(ArtifactId))
            .transpose()?,
        prompt,
        provenance: provenance(&author, note)?,
        disposition: QuestionDisposition::from_db(&aggregate.state)?,
        created_event: EventSeq(from_db(created_event)?),
    }))
}

fn answers(conn: &Connection, id: QuestionId) -> Result<Vec<Answer>, StorageError> {
    conn.query(
        "SELECT ordinal, body, author, provenance, created_event_seq
         FROM clarification_answers WHERE question_id = ?1 ORDER BY ordinal",
        &[Value::Text(&id_to_text(id.0))],
        |r: &Row<'_>| -> Raw<_> { Ok((r.int(0)?, r.text(1)?, r.text(2)?, r.text(3)?, r.int(4)?)) },
    )?
    .into_iter()
    .map(|(ordinal, body, author, note, created_event)| {
        Ok(Answer {
            question: id,
            ordinal: from_db(ordinal)?,
            body,
            provenance: provenance(&author, note)?,
            created_event: EventSeq(from_db(created_event)?),
        })
    })
    .collect()
}

/// Blocking questions about this artifact or its feature as a whole whose newest answer
/// is missing or was not given by a human.
fn unanswered_questions(
    conn: &Connection,
    version: &ArtifactVersion,
) -> Result<Vec<QuestionId>, StorageError> {
    conn.query(
        "SELECT q.question_id FROM clarification_questions q
         JOIN aggregates a ON a.kind = q.question_kind AND a.id = q.question_id
         WHERE q.feature_id = ?1 AND (q.artifact_id IS NULL OR q.artifact_id = ?2)
           AND a.state = 'BLOCKING'
           AND coalesce((SELECT author FROM clarification_answers answer
                         WHERE answer.question_id = q.question_id
                         ORDER BY answer.ordinal DESC LIMIT 1), '') <> 'HUMAN'
         ORDER BY q.created_event_seq",
        &[
            Value::Text(&id_to_text(version.feature.0)),
            Value::Text(&id_to_text(version.artifact.0)),
        ],
        |r: &Row<'_>| r.text(0),
    )?
    .into_iter()
    .map(|id| id_from_text(&id).map(QuestionId))
    .collect()
}

pub(crate) fn review_readiness(
    conn: &Connection,
    id: ArtifactId,
) -> Result<Option<ReviewReadiness>, StorageError> {
    let Some(head) = version(conn, id, None)? else {
        return Ok(None);
    };
    let document = PrdDocument::import(head.content.as_bytes()).map_err(|error| {
        StorageError::corrupt(format!("stored artifact content is not a PRD: {error:?}"))
    })?;
    let blockers = document
        .validate()
        .into_iter()
        .map(ReadinessBlocker::Incomplete)
        .chain(
            unanswered_questions(conn, &head)?
                .into_iter()
                .map(ReadinessBlocker::UnansweredQuestion),
        )
        .collect();
    Ok(Some(ReviewReadiness {
        artifact: id,
        revision: head.revision,
        content_digest: head.content_digest,
        rendered_digest: head.rendered_digest,
        blockers,
    }))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use jojo_domain::identity::OpaqueId;

    use super::*;
    use crate::schema::{MigrationPlan, migrate};
    use crate::sqlite::OpenMode;

    const PRD: &str = r#"{"schema_version":1,"title":"T","goal":"G","non_goals":[],"actors":[],
        "use_cases":[],"acceptance_criteria":[],"nfrs":[],"assumptions":[],"blockers":[]}"#;

    /// A stored version whose bytes no longer match its digests is reported as corrupt
    /// instead of being shown for review. Only reachable by defeating the schema triggers.
    #[test]
    fn a_version_altered_behind_the_schema_is_refused_on_read() {
        let conn = Connection::open(Path::new(":memory:"), OpenMode::Create).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
        migrate(&conn, Path::new("unused"), &MigrationPlan::production()).unwrap();
        conn.execute_batch(
            "INSERT INTO engine_sessions VALUES (1, 0, 1, 2, 'CLOSED', 'RUNNING', NULL);
             INSERT INTO commands VALUES ('00000000000000000000000000000009', 'k',
                 '0000000000000000000000000000000000000000000000000000000000000000', 1, 'r', 1, 0);",
        )
        .unwrap();
        let id = ArtifactId(OpaqueId(1));
        let mut tx = Tx {
            conn: &conn,
            epoch: EngineEpoch(1),
            now_unix_ms: 0,
            now: jojo_domain::time::ClockReading {
                unix_millis: 0,
                monotonic_nanos: 0,
                epoch: jojo_domain::identity::ClockEpochId(OpaqueId(1)),
            },
            command_id: "00000000000000000000000000000009".into(),
        };
        let prepared = PreparedPrd::import(PRD.as_bytes()).unwrap();
        let provenance = Provenance::new(Author::Human, "");
        let created = tx
            .create_artifact(id, FeatureId(OpaqueId(2)), &prepared, &provenance)
            .unwrap();
        assert_eq!(version(&conn, id, None).unwrap(), Some(created));

        for column in ["rendered", "content"] {
            conn.execute_batch(&format!(
                "DROP TRIGGER artifact_versions_immutable;
                 UPDATE artifact_versions SET {column} = {column} || ' ';"
            ))
            .unwrap();
            assert!(matches!(
                version(&conn, id, None),
                Err(StorageError::Corrupt(_))
            ));
            assert!(matches!(
                review_readiness(&conn, id),
                Err(StorageError::Corrupt(_))
            ));
            conn.execute_batch(&format!(
                "UPDATE artifact_versions SET {column} = rtrim({column});
                 CREATE TRIGGER artifact_versions_immutable BEFORE UPDATE ON artifact_versions
                 BEGIN SELECT RAISE(ABORT, 'artifact versions are immutable'); END;"
            ))
            .unwrap();
            assert!(version(&conn, id, None).unwrap().is_some());
        }
    }
}

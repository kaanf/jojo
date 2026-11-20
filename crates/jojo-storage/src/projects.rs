//! M3: project records (schema 12). A record is saved whole under the human's command as
//! a new, immutable revision. Saving one also sets the scheduler's parallel capacity
//! (D4): one number for the whole helper, not split by provider and with no limit per
//! repository, so the last saved record decides it.
use crate::model::{from_db, id_from_text, id_to_text, to_db};
use crate::sqlite::{Connection, Value};
use crate::*;
use jojo_domain::identity::{OpaqueId, RepositoryId};
use jojo_domain::ipc::{identity, number, object};
use jojo_domain::json::Json;
use jojo_domain::payload::Record;
use jojo_domain::project::{MAX_PROJECTS, ProjectRecord};

fn deny(s: impl Into<String>) -> TxError {
    TxError::Rejected(s.into())
}

/// The newest revision of a project.
pub(crate) fn latest(
    c: &Connection,
    project: OpaqueId,
) -> Result<Option<(u64, ProjectRecord)>, StorageError> {
    let row = c.query_optional(
        "SELECT revision,body FROM project_revisions WHERE project=?1 ORDER BY revision DESC LIMIT 1",
        &[Value::Text(&id_to_text(project))],
        |r| Ok((r.int(0)?, r.text(1)?)),
    )?;
    row.map(|(revision, body)| {
        Ok((
            from_db(revision)?,
            ProjectRecord::import(body.as_bytes()).map_err(StorageError::corrupt)?,
        ))
    })
    .transpose()
}

/// One exact revision of a project.
pub(crate) fn revision(
    c: &Connection,
    project: OpaqueId,
    revision: u64,
) -> Result<Option<ProjectRecord>, StorageError> {
    let body = c.query_optional(
        "SELECT body FROM project_revisions WHERE project=?1 AND revision=?2",
        &[
            Value::Text(&id_to_text(project)),
            Value::Int(to_db(revision)?),
        ],
        |r| r.text(0),
    )?;
    body.map(|b| ProjectRecord::import(b.as_bytes()).map_err(StorageError::corrupt))
        .transpose()
}

/// The project a registered repository belongs to, at its newest revision.
pub(crate) fn of_repository(
    c: &Connection,
    repository: RepositoryId,
) -> Result<Option<(OpaqueId, u64, ProjectRecord)>, StorageError> {
    let project = c.query_optional(
        "SELECT project FROM project_repositories WHERE repository=?1",
        &[Value::Text(&id_to_text(repository.0))],
        |r| r.text(0),
    )?;
    let Some(project) = project else {
        return Ok(None);
    };
    let project = id_from_text(&project)?;
    Ok(latest(c, project)?.map(|(revision, record)| (project, revision, record)))
}

impl Tx<'_> {
    /// Saves `record` as the next revision of `project`; `expected` is the revision the
    /// human edited (0 for a new project). Returns the new revision.
    pub fn save_project(
        &mut self,
        project: OpaqueId,
        expected: u64,
        record: &ProjectRecord,
    ) -> Result<u64, TxError> {
        record.validate().map_err(deny)?;
        let current = latest(self.conn, project)?.map_or(0, |(r, _)| r);
        if current != expected {
            return Err(deny("StaleProject: the project changed; read it again"));
        }
        if current == 0 {
            let count = self.conn.query_one(
                "SELECT count(DISTINCT project) FROM project_revisions",
                &[],
                |r| r.int(0),
            )?;
            if from_db(count)? >= MAX_PROJECTS as u64 {
                return Err(deny("InvalidProject: at most eight projects"));
            }
        }
        let text_id = id_to_text(project);
        for repository in &record.repositories {
            let repo = id_to_text(repository.0);
            if self
                .conn
                .query_optional(
                    "SELECT 1 FROM local_repositories WHERE id=?1",
                    &[Value::Text(&repo)],
                    |r| r.int(0),
                )?
                .is_none()
            {
                return Err(deny("InvalidProject: a repository is not registered"));
            }
            let owner = self.conn.query_optional(
                "SELECT project FROM project_repositories WHERE repository=?1",
                &[Value::Text(&repo)],
                |r| r.text(0),
            )?;
            if owner.is_some_and(|o| o != text_id) {
                return Err(deny(
                    "InvalidProject: a repository belongs to another project",
                ));
            }
        }
        let next = current + 1;
        let body = record.canonical();
        let event = self.append_event(
            "ProjectSaved",
            None,
            &Record::new()
                .text("project", text_id.clone())
                .count("revision", next),
        )?;
        self.conn.execute(
            "INSERT INTO project_revisions VALUES (?1,?2,?3,?4,?5)",
            &[
                Value::Text(&text_id),
                Value::Int(to_db(next)?),
                Value::Text(&body),
                Value::Text(&sha256_hex(&[body.as_bytes()])),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        self.conn.execute(
            "DELETE FROM project_repositories WHERE project=?1",
            &[Value::Text(&text_id)],
        )?;
        for repository in &record.repositories {
            self.conn.execute(
                "INSERT INTO project_repositories VALUES (?1,?2)",
                &[
                    Value::Text(&id_to_text(repository.0)),
                    Value::Text(&text_id),
                ],
            )?;
        }
        self.apply_capacity(record.capacity)?;
        Ok(next)
    }

    /// D4: `capacity` attempts in all. The provider limits equal it, so the total is not
    /// split by provider, and so does the default per repository, so there is no
    /// repository limit. Other configured resources are kept. Nothing is written when the
    /// configuration already says so.
    fn apply_capacity(&mut self, capacity: u32) -> Result<(), TxError> {
        let cfg = self.resource_config_now()?;
        let mut capacities = cfg.capacities.clone();
        for key in ["global", "provider:claude", "provider:codex"] {
            capacities.insert(key.to_owned(), capacity);
        }
        if capacities == cfg.capacities && cfg.default_repository == capacity {
            return Ok(());
        }
        self.configure_resources(cfg.revision, capacity, capacities)?;
        Ok(())
    }
}

impl ReadView<'_> {
    /// The newest stored text of a review document (an ADR, a plan, a validation plan).
    pub fn review_document_content(
        &self,
        id: jojo_domain::identity::ArtifactId,
    ) -> Result<Option<String>, StorageError> {
        Ok(self.conn.query_optional(
            "SELECT content FROM review_documents WHERE id=?1 ORDER BY revision DESC LIMIT 1",
            &[Value::Text(&id_to_text(id.0))],
            |r| r.text(0),
        )?)
    }
    pub fn project(&self, project: OpaqueId) -> Result<Option<(u64, ProjectRecord)>, StorageError> {
        latest(self.conn, project)
    }
    pub fn project_of_repository(
        &self,
        repository: RepositoryId,
    ) -> Result<Option<(OpaqueId, u64, ProjectRecord)>, StorageError> {
        of_repository(self.conn, repository)
    }
    /// M3 (protocol 1.3): every project at its newest revision, in identity order, and the
    /// scheduler's capacity as configured now.
    pub fn ipc_projects(&self) -> Result<Json, StorageError> {
        let ids = self.conn.query(
            "SELECT DISTINCT project FROM project_revisions ORDER BY project",
            &[],
            |r| r.text(0),
        )?;
        let mut projects = Vec::new();
        for id in ids {
            let project = id_from_text(&id)?;
            let Some((revision, record)) = latest(self.conn, project)? else {
                continue;
            };
            let Json::Object(mut fields) = record.to_json() else {
                unreachable!("a record is an object")
            };
            fields.insert(0, ("revision".into(), number(revision)));
            fields.insert(0, ("project".into(), identity(project)));
            projects.push(Json::Object(fields));
        }
        let cfg = self.resource_config()?;
        Ok(object([
            ("projects", Json::List(projects)),
            (
                "capacity",
                number(cfg.capacities.get("global").copied().unwrap_or(0).into()),
            ),
            ("configuration", number(cfg.revision)),
        ]))
    }
}

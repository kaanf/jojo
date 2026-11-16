//! Schema versions and migrations. This module owns the production schema; nothing else
//! issues DDL. `PRAGMA user_version` is the schema version and changes in the same
//! transaction as the DDL of its step, so a database is always exactly at one version.

use std::fs::File;
use std::path::{Path, PathBuf};

use crate::error::StorageError;
use crate::sqlite::{Connection, OpenMode, SqliteError, Value};

/// `PRAGMA application_id` marking a Jojo database ("JoJo").
pub const APPLICATION_ID: i64 = 0x4A6F_4A6F;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    /// A batch of statements without transaction control; the runner wraps the batch and
    /// the version bump in one `BEGIN IMMEDIATE … COMMIT`.
    pub sql: &'static str,
}

/// An ordered list of steps whose versions are exactly `1..=latest`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationPlan(Vec<Migration>);

impl MigrationPlan {
    /// The schema shipped with this build. Later tasks append steps; shipped steps are
    /// never edited.
    pub fn production() -> Self {
        Self(vec![
            Migration {
                version: 1,
                name: "core writer tables",
                sql: SCHEMA_V1,
            },
            Migration {
                version: 2,
                name: "artifact versions and clarification questions",
                sql: SCHEMA_V2,
            },
            Migration {
                version: 3,
                name: "human decisions and invalidation",
                sql: SCHEMA_V3,
            },
            Migration {
                version: 4,
                name: "dependency graphs and observations",
                sql: SCHEMA_V4,
            },
            Migration {
                version: 5,
                name: "resource admission and fairness",
                sql: SCHEMA_V5,
            },
            Migration {
                version: 6,
                name: "owned local Git worktrees",
                sql: SCHEMA_V6,
            },
            Migration {
                version: 7,
                name: "process launches, identities and raw streams",
                sql: SCHEMA_V7,
            },
            Migration {
                version: 8,
                name: "immutable results, evidence, effects and validation",
                sql: SCHEMA_V8,
            },
            Migration {
                version: 9,
                name: "recovery inspection, resolution and continuation plans",
                sql: SCHEMA_V9,
            },
            Migration {
                version: 10,
                name: "close tickets, shutdown targets and receipts",
                sql: SCHEMA_V10,
            },
        ])
    }

    /// A custom plan. Production code uses [`MigrationPlan::production`]; this constructor
    /// exists so tests can exercise upgrade and failure paths.
    pub fn new(steps: Vec<Migration>) -> Result<Self, StorageError> {
        if steps.is_empty() {
            return Err(StorageError::InvalidMigrationPlan("empty plan".into()));
        }
        for (index, step) in steps.iter().enumerate() {
            if u32::try_from(index + 1) != Ok(step.version) {
                return Err(StorageError::InvalidMigrationPlan(format!(
                    "step {} has version {}; versions must be 1..=n in order",
                    index + 1,
                    step.version
                )));
            }
        }
        Ok(Self(steps))
    }

    pub fn steps(&self) -> &[Migration] {
        &self.0
    }

    pub fn latest(&self) -> u32 {
        self.0.last().map_or(0, |step| step.version)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationReport {
    pub from_version: u32,
    pub to_version: u32,
    /// Verified pre-migration copy, present whenever an existing database was upgraded.
    pub backup: Option<PathBuf>,
}

pub(crate) fn user_version(connection: &Connection) -> Result<u32, StorageError> {
    let version = connection.query_one("PRAGMA user_version", &[], |row| row.int(0))?;
    u32::try_from(version).map_err(|_| StorageError::corrupt("negative schema version"))
}

/// Read-only admission check: the file must be empty or a Jojo database at a version this
/// plan knows. Returns the current schema version. Changes nothing.
pub(crate) fn recognise(
    connection: &Connection,
    plan: &MigrationPlan,
) -> Result<u32, StorageError> {
    let version = user_version(connection)?;
    let application_id = connection.query_one("PRAGMA application_id", &[], |row| row.int(0))?;
    let objects = connection.query_one("SELECT count(*) FROM sqlite_schema", &[], |r| r.int(0))?;
    let ours = if version == 0 {
        objects == 0
    } else {
        application_id == APPLICATION_ID
    };
    if !ours {
        return Err(StorageError::ForeignDatabase);
    }
    if version > plan.latest() {
        return Err(StorageError::SchemaTooNew {
            found: version,
            supported: plan.latest(),
        });
    }
    Ok(version)
}

/// Brings the database to the plan's latest version, or fails leaving it at a whole
/// earlier version. Must be called under the ownership lock, before any writer starts.
pub(crate) fn migrate(
    connection: &Connection,
    db_path: &Path,
    plan: &MigrationPlan,
) -> Result<MigrationReport, StorageError> {
    let from_version = recognise(connection, plan)?;
    let latest = plan.latest();
    let mut report = MigrationReport {
        from_version,
        to_version: from_version,
        backup: None,
    };
    if from_version == latest {
        return Ok(report);
    }
    if from_version > 0 {
        report.backup = Some(backup(connection, db_path, from_version)?);
    }
    for step in plan.steps().iter().filter(|s| s.version > from_version) {
        apply(connection, step).map_err(|cause| StorageError::MigrationFailed {
            from_version: report.to_version,
            failed_version: step.version,
            backup: report.backup.clone(),
            cause,
        })?;
        report.to_version = step.version;
    }
    Ok(report)
}

fn apply(connection: &Connection, step: &Migration) -> Result<(), SqliteError> {
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| {
        connection.execute_batch(step.sql)?;
        if !connection.in_transaction() {
            return Err(SqliteError::misuse(
                "migration step ended its own transaction",
            ));
        }
        connection.execute_batch(&format!(
            "PRAGMA application_id = {APPLICATION_ID}; PRAGMA user_version = {};",
            step.version
        ))?;
        let violations = connection.query("PRAGMA foreign_key_check", &[], |_| Ok(()))?;
        if !violations.is_empty() {
            return Err(SqliteError::misuse("migration left foreign key violations"));
        }
        connection.execute_batch("COMMIT")
    })();
    if result.is_err() && connection.in_transaction() {
        // Best effort: if this also fails the connection is dropped by the caller, which
        // rolls the transaction back at close.
        let _ = connection.execute_batch("ROLLBACK");
    }
    result
}

/// Writes a consistent copy of the whole database next to it and verifies the copy.
/// Backups are never deleted or overwritten automatically.
fn backup(connection: &Connection, db_path: &Path, version: u32) -> Result<PathBuf, StorageError> {
    let file_name = db_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| StorageError::corrupt("database file name is not UTF-8"))?;
    let target = (1..=999)
        .map(|n| db_path.with_file_name(format!("{file_name}.pre-migration-v{version}.{n}.bak")))
        .find(|candidate| !candidate.exists())
        .ok_or_else(|| StorageError::corrupt("too many existing migration backups"))?;
    let target_text = target
        .to_str()
        .ok_or_else(|| StorageError::corrupt("backup path is not UTF-8"))?;
    connection.execute("VACUUM INTO ?1", &[Value::Text(target_text)])?;
    File::open(&target)
        .and_then(|file| file.sync_all())
        .map_err(|e| StorageError::io("syncing migration backup", e))?;
    let copy = Connection::open(&target, OpenMode::Existing)?;
    let integrity = copy.query_one("PRAGMA integrity_check", &[], |row| row.text(0))?;
    if integrity != "ok" || user_version(&copy)? != version {
        return Err(StorageError::corrupt(format!(
            "migration backup failed verification: {integrity}"
        )));
    }
    Ok(target)
}

/// Version 1: the minimum core tables of the authoritative writer.
///
/// Triggers make the append-only and state-machine rules hold for *any* connection, not
/// only for well-behaved Rust code: history cannot be rewritten by a stray statement.
const SCHEMA_V1: &str = r#"
CREATE TABLE engine_sessions (
    epoch              INTEGER PRIMARY KEY CHECK (epoch >= 1),
    started_at_unix_ms INTEGER NOT NULL,
    owner_pid          INTEGER NOT NULL,
    schema_version     INTEGER NOT NULL,
    admission          TEXT NOT NULL CHECK (admission IN ('CLOSED', 'OPEN')),
    shutdown           TEXT NOT NULL CHECK (shutdown IN ('RUNNING', 'CLEAN', 'UNCLEAN')),
    ended_at_unix_ms   INTEGER
) STRICT;

-- A session starts as the next epoch, running, with admission closed, and only after every
-- earlier session has been sealed. Hence at most one session is ever RUNNING.
CREATE TRIGGER engine_sessions_start BEFORE INSERT ON engine_sessions
WHEN NEW.admission <> 'CLOSED' OR NEW.shutdown <> 'RUNNING' OR NEW.ended_at_unix_ms IS NOT NULL
  OR NEW.epoch IS NOT (SELECT coalesce(max(epoch), 0) + 1 FROM engine_sessions)
  OR EXISTS (SELECT 1 FROM engine_sessions WHERE shutdown = 'RUNNING')
BEGIN SELECT RAISE(ABORT, 'engine session must start as the next epoch with admission closed'); END;

-- A sealed session is immutable; sealing closes admission in the same statement.
CREATE TRIGGER engine_sessions_seal BEFORE UPDATE ON engine_sessions
WHEN OLD.shutdown <> 'RUNNING'
  OR NEW.epoch <> OLD.epoch OR NEW.started_at_unix_ms <> OLD.started_at_unix_ms
  OR NEW.owner_pid <> OLD.owner_pid OR NEW.schema_version <> OLD.schema_version
  OR (NEW.shutdown <> 'RUNNING' AND NEW.admission <> 'CLOSED')
BEGIN SELECT RAISE(ABORT, 'engine session history is immutable'); END;

CREATE TRIGGER engine_sessions_keep BEFORE DELETE ON engine_sessions
BEGIN SELECT RAISE(ABORT, 'engine session history is append-only'); END;

CREATE TABLE aggregates (
    kind          TEXT NOT NULL CHECK (length(kind) > 0),
    id            TEXT NOT NULL CHECK (length(id) = 32),
    revision      INTEGER NOT NULL CHECK (revision >= 1),
    state         TEXT NOT NULL,
    updated_epoch INTEGER NOT NULL REFERENCES engine_sessions (epoch),
    PRIMARY KEY (kind, id)
) STRICT, WITHOUT ROWID;

CREATE TRIGGER aggregates_first_revision BEFORE INSERT ON aggregates
WHEN NEW.revision <> 1
BEGIN SELECT RAISE(ABORT, 'aggregate must be created at revision 1'); END;

CREATE TRIGGER aggregates_next_revision BEFORE UPDATE ON aggregates
WHEN NEW.revision <> OLD.revision + 1 OR NEW.kind <> OLD.kind OR NEW.id <> OLD.id
BEGIN SELECT RAISE(ABORT, 'aggregate revision must advance by exactly one'); END;

-- Deleting and recreating an aggregate would restart its revisions (ABA); forbid it.
CREATE TRIGGER aggregates_keep BEFORE DELETE ON aggregates
BEGIN SELECT RAISE(ABORT, 'aggregates are never deleted'); END;

CREATE TABLE commands (
    command_id           TEXT PRIMARY KEY CHECK (length(command_id) = 32),
    kind                 TEXT NOT NULL,
    request_digest       TEXT NOT NULL CHECK (length(request_digest) = 64),
    epoch                INTEGER NOT NULL REFERENCES engine_sessions (epoch),
    response             TEXT NOT NULL,
    event_seq            INTEGER NOT NULL CHECK (event_seq >= 0),
    committed_at_unix_ms INTEGER NOT NULL
) STRICT, WITHOUT ROWID;

CREATE TRIGGER commands_immutable BEFORE UPDATE ON commands
BEGIN SELECT RAISE(ABORT, 'command receipts are immutable'); END;

CREATE TRIGGER commands_keep BEFORE DELETE ON commands
BEGIN SELECT RAISE(ABORT, 'command receipts are append-only'); END;

-- AUTOINCREMENT never reuses a sequence number. The deferred foreign key lets an event be
-- appended before its command receipt inside one transaction while guaranteeing at commit
-- that no event exists without the receipt of the command that caused it.
CREATE TABLE events (
    seq                 INTEGER PRIMARY KEY AUTOINCREMENT,
    epoch               INTEGER NOT NULL REFERENCES engine_sessions (epoch),
    command_id          TEXT REFERENCES commands (command_id) DEFERRABLE INITIALLY DEFERRED,
    kind                TEXT NOT NULL CHECK (length(kind) > 0),
    aggregate_kind      TEXT,
    aggregate_id        TEXT,
    aggregate_revision  INTEGER,
    payload             TEXT NOT NULL,
    recorded_at_unix_ms INTEGER NOT NULL,
    CHECK ((aggregate_kind IS NULL) = (aggregate_id IS NULL)
       AND (aggregate_kind IS NULL) = (aggregate_revision IS NULL)),
    FOREIGN KEY (aggregate_kind, aggregate_id) REFERENCES aggregates (kind, id)
) STRICT;

CREATE TRIGGER events_immutable BEFORE UPDATE ON events
BEGIN SELECT RAISE(ABORT, 'events are immutable'); END;

CREATE TRIGGER events_keep BEFORE DELETE ON events
BEGIN SELECT RAISE(ABORT, 'events are append-only'); END;

-- No expiry/deadline column exists on purpose: a claim ends only by an explicit release
-- with a recorded basis, never because time passed.
CREATE TABLE resource_claims (
    claim_id            INTEGER PRIMARY KEY AUTOINCREMENT,
    resource_key        TEXT NOT NULL CHECK (length(resource_key) > 0),
    mode                TEXT NOT NULL CHECK (mode IN ('SHARED', 'EXCLUSIVE')),
    holder_kind         TEXT NOT NULL,
    holder_id           TEXT NOT NULL,
    epoch               INTEGER NOT NULL REFERENCES engine_sessions (epoch),
    state               TEXT NOT NULL CHECK (state IN ('ACTIVE', 'QUARANTINED', 'RELEASED')),
    release_basis       TEXT CHECK (release_basis IN
                            ('HOLDER_SETTLED', 'NEVER_ISSUED', 'RECOVERY_RESOLVED')),
    acquired_at_unix_ms INTEGER NOT NULL,
    released_epoch      INTEGER REFERENCES engine_sessions (epoch),
    CHECK ((state = 'RELEASED') = (release_basis IS NOT NULL)),
    CHECK ((state = 'RELEASED') = (released_epoch IS NOT NULL)),
    FOREIGN KEY (holder_kind, holder_id) REFERENCES aggregates (kind, id)
) STRICT;

CREATE INDEX resource_claims_live ON resource_claims (resource_key) WHERE state <> 'RELEASED';

CREATE UNIQUE INDEX resource_claims_one_per_holder
    ON resource_claims (resource_key, holder_kind, holder_id) WHERE state <> 'RELEASED';

-- Live means ACTIVE or QUARANTINED. An exclusive claim tolerates no other live claim.
CREATE TRIGGER resource_claims_admit BEFORE INSERT ON resource_claims
WHEN NEW.state <> 'ACTIVE'
  OR EXISTS (SELECT 1 FROM resource_claims held
             WHERE held.resource_key = NEW.resource_key AND held.state <> 'RELEASED'
               AND (NEW.mode = 'EXCLUSIVE' OR held.mode = 'EXCLUSIVE'))
BEGIN SELECT RAISE(ABORT, 'resource claim conflicts with a live claim'); END;

-- ACTIVE -> QUARANTINED | RELEASED, QUARANTINED -> RELEASED. Nothing returns to ACTIVE.
CREATE TRIGGER resource_claims_lifecycle BEFORE UPDATE ON resource_claims
WHEN NOT ((OLD.state = 'ACTIVE' AND NEW.state IN ('QUARANTINED', 'RELEASED'))
       OR (OLD.state = 'QUARANTINED' AND NEW.state = 'RELEASED'))
  OR NEW.claim_id <> OLD.claim_id OR NEW.resource_key <> OLD.resource_key
  OR NEW.mode <> OLD.mode OR NEW.holder_kind <> OLD.holder_kind
  OR NEW.holder_id <> OLD.holder_id OR NEW.epoch <> OLD.epoch
BEGIN SELECT RAISE(ABORT, 'invalid resource claim transition'); END;

CREATE TRIGGER resource_claims_keep BEFORE DELETE ON resource_claims
BEGIN SELECT RAISE(ABORT, 'resource claims are never deleted'); END;

CREATE TABLE outbox (
    operation_key  TEXT PRIMARY KEY CHECK (length(operation_key) > 0),
    target         TEXT NOT NULL,
    payload        TEXT NOT NULL,
    payload_digest TEXT NOT NULL CHECK (length(payload_digest) = 64),
    state          TEXT NOT NULL CHECK (state IN
                       ('NOT_ISSUED', 'ISSUED_UNKNOWN', 'CONFIRMED', 'REJECTED')),
    created_epoch  INTEGER NOT NULL REFERENCES engine_sessions (epoch),
    updated_epoch  INTEGER NOT NULL REFERENCES engine_sessions (epoch)
) STRICT, WITHOUT ROWID;

CREATE TRIGGER outbox_start BEFORE INSERT ON outbox
WHEN NEW.state <> 'NOT_ISSUED'
BEGIN SELECT RAISE(ABORT, 'effect must be recorded as NOT_ISSUED'); END;

-- NOT_ISSUED -> ISSUED_UNKNOWN -> CONFIRMED | REJECTED; the operation itself never changes.
CREATE TRIGGER outbox_lifecycle BEFORE UPDATE ON outbox
WHEN NOT ((OLD.state = 'NOT_ISSUED' AND NEW.state = 'ISSUED_UNKNOWN')
       OR (OLD.state = 'ISSUED_UNKNOWN' AND NEW.state IN ('CONFIRMED', 'REJECTED')))
  OR NEW.operation_key <> OLD.operation_key OR NEW.target <> OLD.target
  OR NEW.payload <> OLD.payload OR NEW.payload_digest <> OLD.payload_digest
  OR NEW.created_epoch <> OLD.created_epoch
BEGIN SELECT RAISE(ABORT, 'invalid effect transition'); END;

CREATE TRIGGER outbox_keep BEFORE DELETE ON outbox
BEGIN SELECT RAISE(ABORT, 'effects are never deleted'); END;
"#;

/// Version 2 (P1-03): immutable artifact versions and clarification questions/answers.
///
/// An artifact and a question are `aggregates` rows (kinds `artifact` and `question`), so
/// their edits reuse revision compare-and-swap. The tables below hold what never changes.
const SCHEMA_V2: &str = r#"
-- `revision` is the revision of the artifact aggregate that this version created, and
-- the aggregate's state is the content digest of its newest version.
CREATE TABLE artifact_versions (
    artifact_kind     TEXT NOT NULL CHECK (artifact_kind = 'artifact'),
    artifact_id       TEXT NOT NULL CHECK (length(artifact_id) = 32),
    revision          INTEGER NOT NULL CHECK (revision >= 1),
    parent_revision   INTEGER,
    feature_id        TEXT NOT NULL CHECK (length(feature_id) = 32),
    artifact_type     TEXT NOT NULL CHECK (artifact_type IN ('PRD')),
    schema_version    INTEGER NOT NULL CHECK (schema_version >= 1),
    content           TEXT NOT NULL CHECK (length(content) > 0),
    content_digest    TEXT NOT NULL CHECK (length(content_digest) = 64),
    rendered          TEXT NOT NULL CHECK (length(rendered) > 0),
    rendered_digest   TEXT NOT NULL CHECK (length(rendered_digest) = 64),
    author            TEXT NOT NULL CHECK (author IN ('HUMAN', 'MODEL')),
    provenance        TEXT NOT NULL,
    created_event_seq INTEGER NOT NULL REFERENCES events (seq),
    created_epoch     INTEGER NOT NULL REFERENCES engine_sessions (epoch),
    PRIMARY KEY (artifact_id, revision),
    CHECK (parent_revision IS (CASE WHEN revision = 1 THEN NULL ELSE revision - 1 END)),
    FOREIGN KEY (artifact_kind, artifact_id) REFERENCES aggregates (kind, id),
    FOREIGN KEY (artifact_id, parent_revision) REFERENCES artifact_versions (artifact_id, revision)
) STRICT, WITHOUT ROWID;

-- A version can only be appended as the aggregate's current revision, carrying the digest
-- the aggregate names, for the same feature and type as its parent.
CREATE TRIGGER artifact_versions_append BEFORE INSERT ON artifact_versions
WHEN NEW.revision IS NOT (SELECT revision FROM aggregates
                          WHERE kind = 'artifact' AND id = NEW.artifact_id)
  OR NEW.content_digest IS NOT (SELECT state FROM aggregates
                                WHERE kind = 'artifact' AND id = NEW.artifact_id)
  OR EXISTS (SELECT 1 FROM artifact_versions parent
             WHERE parent.artifact_id = NEW.artifact_id
               AND parent.revision = NEW.parent_revision
               AND (parent.feature_id <> NEW.feature_id
                 OR parent.artifact_type <> NEW.artifact_type))
BEGIN SELECT RAISE(ABORT, 'artifact version must extend the current revision'); END;

CREATE TRIGGER artifact_versions_immutable BEFORE UPDATE ON artifact_versions
BEGIN SELECT RAISE(ABORT, 'artifact versions are immutable'); END;

CREATE TRIGGER artifact_versions_keep BEFORE DELETE ON artifact_versions
BEGIN SELECT RAISE(ABORT, 'artifact versions are never deleted'); END;

-- The question text and context never change; its BLOCKING/DEFERRED disposition is the
-- state of its aggregate and changes only by compare-and-swap.
CREATE TABLE clarification_questions (
    question_kind     TEXT NOT NULL CHECK (question_kind = 'question'),
    question_id       TEXT PRIMARY KEY CHECK (length(question_id) = 32),
    feature_id        TEXT NOT NULL CHECK (length(feature_id) = 32),
    artifact_kind     TEXT CHECK (artifact_kind = 'artifact'),
    artifact_id       TEXT,
    prompt            TEXT NOT NULL CHECK (length(prompt) > 0),
    author            TEXT NOT NULL CHECK (author IN ('HUMAN', 'MODEL')),
    provenance        TEXT NOT NULL,
    created_event_seq INTEGER NOT NULL REFERENCES events (seq),
    created_epoch     INTEGER NOT NULL REFERENCES engine_sessions (epoch),
    CHECK ((artifact_kind IS NULL) = (artifact_id IS NULL)),
    FOREIGN KEY (question_kind, question_id) REFERENCES aggregates (kind, id),
    FOREIGN KEY (artifact_kind, artifact_id) REFERENCES aggregates (kind, id)
) STRICT, WITHOUT ROWID;

CREATE INDEX clarification_questions_feature ON clarification_questions (feature_id);

CREATE TRIGGER clarification_questions_immutable BEFORE UPDATE ON clarification_questions
BEGIN SELECT RAISE(ABORT, 'clarification questions are immutable'); END;

CREATE TRIGGER clarification_questions_keep BEFORE DELETE ON clarification_questions
BEGIN SELECT RAISE(ABORT, 'clarification questions are never deleted'); END;

CREATE TRIGGER question_disposition_insert BEFORE INSERT ON aggregates
WHEN NEW.kind = 'question' AND NEW.state NOT IN ('BLOCKING', 'DEFERRED')
BEGIN SELECT RAISE(ABORT, 'question disposition must be BLOCKING or DEFERRED'); END;

CREATE TRIGGER question_disposition_update BEFORE UPDATE ON aggregates
WHEN NEW.kind = 'question' AND NEW.state NOT IN ('BLOCKING', 'DEFERRED')
BEGIN SELECT RAISE(ABORT, 'question disposition must be BLOCKING or DEFERRED'); END;

-- Answers are versioned by ordinal: a later answer supersedes an earlier one, which stays.
CREATE TABLE clarification_answers (
    question_id       TEXT NOT NULL REFERENCES clarification_questions (question_id),
    ordinal           INTEGER NOT NULL CHECK (ordinal >= 1),
    body              TEXT NOT NULL CHECK (length(body) > 0),
    author            TEXT NOT NULL CHECK (author IN ('HUMAN', 'MODEL')),
    provenance        TEXT NOT NULL,
    created_event_seq INTEGER NOT NULL REFERENCES events (seq),
    created_epoch     INTEGER NOT NULL REFERENCES engine_sessions (epoch),
    PRIMARY KEY (question_id, ordinal)
) STRICT, WITHOUT ROWID;

CREATE TRIGGER clarification_answers_append BEFORE INSERT ON clarification_answers
WHEN NEW.ordinal IS NOT (SELECT coalesce(max(ordinal), 0) + 1 FROM clarification_answers
                         WHERE question_id = NEW.question_id)
BEGIN SELECT RAISE(ABORT, 'answers are appended in order'); END;

CREATE TRIGGER clarification_answers_immutable BEFORE UPDATE ON clarification_answers
BEGIN SELECT RAISE(ABORT, 'clarification answers are immutable'); END;

CREATE TRIGGER clarification_answers_keep BEFORE DELETE ON clarification_answers
BEGIN SELECT RAISE(ABORT, 'clarification answers are never deleted'); END;
"#;

const SCHEMA_V3: &str = r#"
CREATE TABLE review_documents (
 id TEXT NOT NULL, revision INTEGER NOT NULL, kind TEXT NOT NULL CHECK(kind IN ('ADR','PLAN')),
 prd TEXT NOT NULL, adr TEXT, content TEXT NOT NULL, rendered TEXT NOT NULL,
 prd_decision INTEGER NOT NULL, adr_decision INTEGER,
 event INTEGER NOT NULL REFERENCES events(seq), PRIMARY KEY(id,revision));
CREATE TABLE human_decisions (
 seq INTEGER PRIMARY KEY REFERENCES events(seq), subject TEXT NOT NULL,
 revision INTEGER NOT NULL, content_hash TEXT NOT NULL, displayed_hash TEXT NOT NULL,
 upstream TEXT NOT NULL, prior_decision INTEGER, bundle TEXT NOT NULL, action TEXT NOT NULL CHECK(action IN ('Approve','Reject','Revoke')),
 origin TEXT NOT NULL, command TEXT NOT NULL REFERENCES commands(command_id) DEFERRABLE INITIALLY DEFERRED);
CREATE TABLE approval_attempts (
 id TEXT PRIMARY KEY, prd TEXT NOT NULL, adr TEXT NOT NULL, plan TEXT NOT NULL,
 bundle TEXT NOT NULL, phase TEXT NOT NULL CHECK(phase IN ('RESERVED','STARTING','RUNNING','STOP_REQUESTED','FINISHED')));
CREATE TABLE approval_stops (
 attempt TEXT PRIMARY KEY REFERENCES approval_attempts(id), event INTEGER NOT NULL REFERENCES events(seq));
CREATE TABLE approval_results (
 attempt TEXT PRIMARY KEY REFERENCES approval_attempts(id), body TEXT NOT NULL);
CREATE TRIGGER review_documents_immutable BEFORE UPDATE ON review_documents BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER review_documents_keep BEFORE DELETE ON review_documents BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER human_decisions_immutable BEFORE UPDATE ON human_decisions BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER human_decisions_keep BEFORE DELETE ON human_decisions BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER approval_stops_immutable BEFORE UPDATE ON approval_stops BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER approval_stops_keep BEFORE DELETE ON approval_stops BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER approval_results_immutable BEFORE UPDATE ON approval_results BEGIN SELECT RAISE(ABORT,'immutable'); END;
CREATE TRIGGER approval_results_keep BEFORE DELETE ON approval_results BEGIN SELECT RAISE(ABORT,'immutable'); END;
"#;

const SCHEMA_V4: &str = r#"
CREATE TABLE execution_plans (
 plan TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0), canonical TEXT NOT NULL,
 digest TEXT NOT NULL CHECK(length(digest)=64), event INTEGER NOT NULL REFERENCES events(seq),
 PRIMARY KEY(plan,revision), FOREIGN KEY(plan,revision) REFERENCES review_documents(id,revision));
CREATE TABLE graph_nodes (
 plan TEXT NOT NULL, revision INTEGER NOT NULL, graph TEXT NOT NULL CHECK(graph IN ('Artifact','Execution')),
 node TEXT NOT NULL, node_revision INTEGER NOT NULL CHECK(node_revision>0),
 PRIMARY KEY(plan,revision,graph,node,node_revision), UNIQUE(plan,revision,graph,node),
 FOREIGN KEY(plan,revision) REFERENCES execution_plans(plan,revision));
CREATE TABLE graph_edges (
 plan TEXT NOT NULL, revision INTEGER NOT NULL, graph TEXT NOT NULL, edge TEXT NOT NULL,
 prerequisite TEXT NOT NULL, prerequisite_revision INTEGER NOT NULL,
 dependent TEXT NOT NULL, dependent_revision INTEGER NOT NULL, predicate TEXT NOT NULL,
 PRIMARY KEY(plan,revision,graph,edge),
 UNIQUE(plan,revision,graph,prerequisite,prerequisite_revision,dependent,dependent_revision,predicate),
 CHECK(prerequisite<>dependent),
 FOREIGN KEY(plan,revision,graph,prerequisite,prerequisite_revision) REFERENCES graph_nodes(plan,revision,graph,node,node_revision),
 FOREIGN KEY(plan,revision,graph,dependent,dependent_revision) REFERENCES graph_nodes(plan,revision,graph,node,node_revision));
CREATE TABLE dependency_observations (
 id TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0), canonical TEXT NOT NULL,
 digest TEXT NOT NULL CHECK(length(digest)=64), event INTEGER NOT NULL REFERENCES events(seq),
 PRIMARY KEY(id,revision));
CREATE TABLE dependency_attempt_inputs (
 attempt TEXT PRIMARY KEY REFERENCES approval_attempts(id), plan TEXT NOT NULL, revision INTEGER NOT NULL,
 step TEXT NOT NULL, step_revision INTEGER NOT NULL, graph TEXT NOT NULL DEFAULT 'Execution' CHECK(graph='Execution'),
 sources TEXT NOT NULL, evaluation_digest TEXT NOT NULL,
 FOREIGN KEY(plan,revision,graph,step,step_revision) REFERENCES graph_nodes(plan,revision,graph,node,node_revision));

CREATE TRIGGER execution_plans_immutable BEFORE UPDATE ON execution_plans BEGIN SELECT RAISE(ABORT,'immutable dependency history'); END;
CREATE TRIGGER execution_plans_keep BEFORE DELETE ON execution_plans BEGIN SELECT RAISE(ABORT,'immutable dependency history'); END;
CREATE TRIGGER graph_nodes_immutable BEFORE UPDATE ON graph_nodes BEGIN SELECT RAISE(ABORT,'immutable dependency history'); END;
CREATE TRIGGER graph_nodes_keep BEFORE DELETE ON graph_nodes BEGIN SELECT RAISE(ABORT,'immutable dependency history'); END;
CREATE TRIGGER graph_edges_immutable BEFORE UPDATE ON graph_edges BEGIN SELECT RAISE(ABORT,'immutable dependency history'); END;
CREATE TRIGGER graph_edges_keep BEFORE DELETE ON graph_edges BEGIN SELECT RAISE(ABORT,'immutable dependency history'); END;
CREATE TRIGGER dependency_observations_immutable BEFORE UPDATE ON dependency_observations BEGIN SELECT RAISE(ABORT,'immutable dependency history'); END;
CREATE TRIGGER dependency_observations_keep BEFORE DELETE ON dependency_observations BEGIN SELECT RAISE(ABORT,'immutable dependency history'); END;
CREATE TRIGGER dependency_attempt_inputs_immutable BEFORE UPDATE ON dependency_attempt_inputs BEGIN SELECT RAISE(ABORT,'immutable dependency history'); END;
CREATE TRIGGER dependency_attempt_inputs_keep BEFORE DELETE ON dependency_attempt_inputs BEGIN SELECT RAISE(ABORT,'immutable dependency history'); END;
"#;

const SCHEMA_V5: &str = r#"
CREATE TABLE scheduler_configs (revision INTEGER PRIMARY KEY CHECK(revision>0), default_repository INTEGER NOT NULL CHECK(default_repository BETWEEN 0 AND 4294967295));
CREATE TABLE scheduler_capacities (revision INTEGER NOT NULL REFERENCES scheduler_configs(revision), resource TEXT NOT NULL, capacity INTEGER NOT NULL CHECK(capacity BETWEEN 0 AND 4294967295), PRIMARY KEY(revision,resource));
INSERT INTO scheduler_configs VALUES (1,1);
INSERT INTO scheduler_capacities VALUES (1,'global',1),(1,'provider:claude',1),(1,'provider:codex',1);
CREATE TRIGGER scheduler_configs_immutable BEFORE UPDATE ON scheduler_configs BEGIN SELECT RAISE(ABORT,'immutable configuration'); END;
CREATE TRIGGER scheduler_configs_keep BEFORE DELETE ON scheduler_configs BEGIN SELECT RAISE(ABORT,'immutable configuration'); END;
CREATE TRIGGER scheduler_capacities_immutable BEFORE UPDATE ON scheduler_capacities BEGIN SELECT RAISE(ABORT,'immutable configuration'); END;
CREATE TRIGGER scheduler_capacities_keep BEFORE DELETE ON scheduler_capacities BEGIN SELECT RAISE(ABORT,'immutable configuration'); END;
CREATE TRIGGER scheduler_config_sequence BEFORE INSERT ON scheduler_configs WHEN NEW.revision<>(SELECT coalesce(max(revision),0)+1 FROM scheduler_configs) BEGIN SELECT RAISE(ABORT,'configuration revision sequence'); END;
CREATE TABLE scheduler_heartbeats (attempt TEXT NOT NULL REFERENCES scheduler_attempts(attempt), epoch INTEGER NOT NULL REFERENCES engine_sessions(epoch), observed_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, event INTEGER PRIMARY KEY REFERENCES events(seq));
CREATE TABLE scheduler_provider_cooldowns (provider TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0), clock_epoch TEXT NOT NULL, until_unix INTEGER NOT NULL, until_tick INTEGER NOT NULL, event INTEGER NOT NULL REFERENCES events(seq), PRIMARY KEY(provider,revision));
CREATE TRIGGER scheduler_cooldown_immutable BEFORE UPDATE ON scheduler_provider_cooldowns BEGIN SELECT RAISE(ABORT,'immutable provider cooldown'); END;
CREATE TRIGGER scheduler_cooldown_keep BEFORE DELETE ON scheduler_provider_cooldowns BEGIN SELECT RAISE(ABORT,'retained provider cooldown'); END;
CREATE TABLE scheduler_cursor (singleton INTEGER PRIMARY KEY CHECK(singleton=1), repository TEXT NOT NULL);
INSERT INTO scheduler_cursor VALUES (1,'');
CREATE TABLE scheduler_queue (
 step TEXT NOT NULL, step_revision INTEGER NOT NULL, plan TEXT NOT NULL, plan_revision INTEGER NOT NULL,
 prd TEXT NOT NULL, adr TEXT NOT NULL, sources TEXT NOT NULL,
 ready_seq INTEGER NOT NULL UNIQUE REFERENCES events(seq), ready_unix INTEGER NOT NULL,
 clock_epoch TEXT NOT NULL, last_tick INTEGER NOT NULL, eligible_nanos INTEGER NOT NULL DEFAULT 0, was_eligible INTEGER NOT NULL DEFAULT 1 CHECK(was_eligible IN (0,1)),
 control_revision INTEGER NOT NULL DEFAULT 1 CHECK(control_revision>0),
 intent TEXT NOT NULL DEFAULT 'RUN' CHECK(intent IN ('RUN','HOLD','CANCEL')),
 cooldown_until INTEGER NOT NULL DEFAULT 0, skips INTEGER NOT NULL DEFAULT 0 CHECK(skips BETWEEN 0 AND 3),
 PRIMARY KEY(step,step_revision));
CREATE TRIGGER scheduler_queue_identity BEFORE UPDATE ON scheduler_queue WHEN NEW.step<>OLD.step OR NEW.step_revision<>OLD.step_revision OR NEW.plan<>OLD.plan OR NEW.plan_revision<>OLD.plan_revision OR NEW.prd<>OLD.prd OR NEW.adr<>OLD.adr OR NEW.sources<>OLD.sources OR NEW.ready_seq<>OLD.ready_seq OR NEW.ready_unix<>OLD.ready_unix OR (NEW.intent<>OLD.intent AND NEW.control_revision<>OLD.control_revision+1) OR (OLD.intent='CANCEL' AND NEW.intent<>'CANCEL') BEGIN SELECT RAISE(ABORT,'scheduler queue identity/control'); END;
CREATE TRIGGER scheduler_queue_keep BEFORE DELETE ON scheduler_queue BEGIN SELECT RAISE(ABORT,'retained queue order'); END;
CREATE TRIGGER dependency_single_unsealed BEFORE INSERT ON dependency_attempt_inputs WHEN EXISTS(SELECT 1 FROM dependency_attempt_inputs i JOIN approval_attempts a ON a.id=i.attempt WHERE i.step=NEW.step AND i.step_revision=NEW.step_revision AND a.phase<>'FINISHED') BEGIN SELECT RAISE(ABORT,'one unsealed step attempt'); END;
CREATE TABLE scheduler_barriers (step TEXT NOT NULL, step_revision INTEGER NOT NULL, resource TEXT NOT NULL,
 PRIMARY KEY(step,step_revision,resource), FOREIGN KEY(step,step_revision) REFERENCES scheduler_queue(step,step_revision));
CREATE TABLE scheduler_attempts (
 attempt TEXT PRIMARY KEY REFERENCES approval_attempts(id), step TEXT NOT NULL, step_revision INTEGER NOT NULL,
 epoch INTEGER NOT NULL REFERENCES engine_sessions(epoch), config_revision INTEGER NOT NULL REFERENCES scheduler_configs(revision),
 sealed INTEGER NOT NULL DEFAULT 0 CHECK(sealed IN (0,1)),
 FOREIGN KEY(step,step_revision) REFERENCES scheduler_queue(step,step_revision));
CREATE UNIQUE INDEX one_unsealed_scheduler_attempt ON scheduler_attempts(step,step_revision) WHERE sealed=0;
CREATE TRIGGER scheduler_launch_issue_guard BEFORE UPDATE ON outbox
 WHEN NEW.operation_key LIKE 'scheduler-launch:%' AND NEW.state='ISSUED_UNKNOWN'
 AND NOT EXISTS(SELECT 1 FROM scheduler_attempts a JOIN approval_attempts p ON p.id=a.attempt
 WHERE a.attempt=substr(NEW.operation_key,18) AND a.sealed=0 AND a.epoch=NEW.updated_epoch
 AND p.phase='RESERVED' AND NOT EXISTS(SELECT 1 FROM approval_stops s WHERE s.attempt=a.attempt)
 AND EXISTS(SELECT 1 FROM engine_sessions e WHERE e.epoch=a.epoch AND e.admission='OPEN' AND e.shutdown='RUNNING'))
 BEGIN SELECT RAISE(ABORT,'scheduler launch is revoked, stopped, gated or stale'); END;
CREATE TABLE scheduler_claim_units (claim INTEGER PRIMARY KEY REFERENCES resource_claims(claim_id), units INTEGER NOT NULL CHECK(units>0));
CREATE TRIGGER scheduler_heartbeats_immutable BEFORE UPDATE ON scheduler_heartbeats BEGIN SELECT RAISE(ABORT,'immutable heartbeat observation'); END;
CREATE TRIGGER scheduler_heartbeats_keep BEFORE DELETE ON scheduler_heartbeats BEGIN SELECT RAISE(ABORT,'retained heartbeat observation'); END;
CREATE TRIGGER scheduler_attempt_identity BEFORE UPDATE ON scheduler_attempts WHEN NEW.attempt<>OLD.attempt OR NEW.step<>OLD.step OR NEW.step_revision<>OLD.step_revision OR NEW.epoch<>OLD.epoch OR NEW.config_revision<>OLD.config_revision OR OLD.sealed=1 BEGIN SELECT RAISE(ABORT,'immutable reservation identity'); END;
CREATE TRIGGER scheduler_attempt_keep BEFORE DELETE ON scheduler_attempts BEGIN SELECT RAISE(ABORT,'retained reservation'); END;
CREATE TRIGGER scheduler_claim_units_immutable BEFORE UPDATE ON scheduler_claim_units BEGIN SELECT RAISE(ABORT,'immutable claim units'); END;
CREATE TRIGGER scheduler_claim_units_keep BEFORE DELETE ON scheduler_claim_units BEGIN SELECT RAISE(ABORT,'retained claim units'); END;
"#;

const SCHEMA_V6: &str = r#"
CREATE TABLE local_repositories (
 id TEXT PRIMARY KEY, path TEXT NOT NULL, common TEXT NOT NULL UNIQUE, filesystem TEXT NOT NULL
);
CREATE TRIGGER local_repository_immutable BEFORE UPDATE ON local_repositories
BEGIN SELECT RAISE(ABORT,'immutable repository registration'); END;
CREATE TRIGGER local_repository_keep BEFORE DELETE ON local_repositories
BEGIN SELECT RAISE(ABORT,'retained repository registration'); END;
CREATE TABLE owned_worktrees (
 attempt TEXT PRIMARY KEY,
 repository TEXT NOT NULL REFERENCES local_repositories(id), epoch INTEGER NOT NULL REFERENCES engine_sessions(epoch),
 state TEXT NOT NULL CHECK(state IN ('WAITING','CREATING','READY','BOUND','BLOCKED')),
 root TEXT NOT NULL, root_identity TEXT NOT NULL,
 repo_path TEXT NOT NULL, common TEXT NOT NULL, common_identity TEXT NOT NULL,
 base TEXT NOT NULL, predecessor TEXT REFERENCES owned_worktrees(attempt),
 claim INTEGER REFERENCES resource_claims(claim_id), marker TEXT NOT NULL UNIQUE, snapshot TEXT,
 UNIQUE(root,attempt),
 CHECK((state='WAITING' AND claim IS NULL) OR (state<>'WAITING' AND claim IS NOT NULL))
);
CREATE TABLE worktree_candidates (
 attempt TEXT NOT NULL REFERENCES owned_worktrees(attempt), digest TEXT NOT NULL,
 head TEXT NOT NULL, path TEXT NOT NULL UNIQUE, PRIMARY KEY(attempt,digest)
);
CREATE TRIGGER worktree_candidate_immutable BEFORE UPDATE ON worktree_candidates
BEGIN SELECT RAISE(ABORT,'immutable candidate'); END;
CREATE TRIGGER worktree_candidate_keep BEFORE DELETE ON worktree_candidates
BEGIN SELECT RAISE(ABORT,'retained candidate'); END;
CREATE TRIGGER worktree_identity_immutable BEFORE UPDATE ON owned_worktrees
WHEN NEW.attempt<>OLD.attempt OR NEW.repository<>OLD.repository OR NEW.epoch<>OLD.epoch
 OR NEW.root<>OLD.root OR NEW.root_identity<>OLD.root_identity OR NEW.repo_path<>OLD.repo_path
 OR NEW.common<>OLD.common OR NEW.common_identity<>OLD.common_identity OR NEW.base<>OLD.base
 OR NEW.predecessor IS NOT OLD.predecessor OR NEW.marker<>OLD.marker
 OR (NEW.claim IS NOT OLD.claim AND OLD.state<>'WAITING')
 OR (NEW.snapshot IS NOT OLD.snapshot AND NOT (OLD.state='CREATING' AND NEW.state='READY'))
BEGIN SELECT RAISE(ABORT,'immutable worktree identity'); END;
CREATE TRIGGER worktree_no_delete BEFORE DELETE ON owned_worktrees
BEGIN SELECT RAISE(ABORT,'retained worktree ownership'); END;
CREATE TRIGGER worktree_lifecycle BEFORE UPDATE ON owned_worktrees
WHEN NOT ((OLD.state='WAITING' AND NEW.state='CREATING') OR
 (OLD.state='CREATING' AND NEW.state IN ('READY','BLOCKED')) OR
 (OLD.state='READY' AND NEW.state='BOUND'))
BEGIN SELECT RAISE(ABORT,'invalid worktree lifecycle'); END;
"#;

/// Version 7 (P1-09): launch issue journal, process identities/observations and
/// raw-stream checkpoints. Events may only reference bytes already durably checkpointed.
const SCHEMA_V7: &str = r#"
CREATE TABLE process_launches (
 attempt TEXT PRIMARY KEY REFERENCES approval_attempts(id),
 generation INTEGER NOT NULL CHECK(generation>0),
 epoch INTEGER NOT NULL REFERENCES engine_sessions(epoch),
 provider TEXT NOT NULL, parser TEXT NOT NULL, executable TEXT NOT NULL, cwd TEXT NOT NULL, spool TEXT NOT NULL,
 spec_digest TEXT NOT NULL CHECK(length(spec_digest)=64),
 issue_event INTEGER NOT NULL UNIQUE REFERENCES events(seq));
CREATE TRIGGER process_launch_issued BEFORE INSERT ON process_launches
WHEN NOT EXISTS(SELECT 1 FROM outbox WHERE operation_key='scheduler-launch:'||NEW.attempt
 AND state='ISSUED_UNKNOWN' AND updated_epoch=NEW.epoch)
BEGIN SELECT RAISE(ABORT,'launch must follow the durable issue boundary'); END;
CREATE TABLE process_records (
 attempt TEXT NOT NULL REFERENCES process_launches(attempt),
 pid INTEGER NOT NULL CHECK(pid>0), start_identity TEXT NOT NULL CHECK(length(start_identity)>0),
 pgid INTEGER NOT NULL CHECK(pgid>0), ppid INTEGER NOT NULL, boot TEXT NOT NULL,
 role TEXT NOT NULL CHECK(role IN ('ROOT','DESCENDANT')),
 event INTEGER NOT NULL REFERENCES events(seq), PRIMARY KEY(attempt,pid,start_identity));
CREATE UNIQUE INDEX process_one_root ON process_records(attempt) WHERE role='ROOT';
CREATE TABLE process_observations (
 event INTEGER PRIMARY KEY REFERENCES events(seq),
 attempt TEXT NOT NULL REFERENCES process_launches(attempt),
 kind TEXT NOT NULL CHECK(kind IN ('NOT_SPAWNED','SPAWN_FAILED','STOP_REQUESTED','SIGNAL_SENT',
  'SIGNAL_REFUSED','ROOT_EXITED','DESCENDANTS_ALIVE','STREAM_CLOSED','STREAM_TRUNCATED',
  'PROTOCOL_FAILURE','EVIDENCE_LOSS','TREE_ABSENT','STOP_UNCONFIRMED')),
 detail TEXT NOT NULL);
CREATE TRIGGER process_observation_concluded BEFORE INSERT ON process_observations
WHEN EXISTS(SELECT 1 FROM process_observations WHERE attempt=NEW.attempt
 AND kind IN ('NOT_SPAWNED','TREE_ABSENT','STOP_UNCONFIRMED'))
BEGIN SELECT RAISE(ABORT,'process supervision already concluded'); END;
CREATE TRIGGER process_tree_absent_proof BEFORE INSERT ON process_observations
WHEN NEW.kind='TREE_ABSENT' AND (
 NOT EXISTS(SELECT 1 FROM process_records WHERE attempt=NEW.attempt AND role='ROOT')
 OR NOT EXISTS(SELECT 1 FROM process_observations WHERE attempt=NEW.attempt AND kind='ROOT_EXITED')
 OR (SELECT count(*) FROM process_observations WHERE attempt=NEW.attempt AND kind='STREAM_CLOSED')<>2)
BEGIN SELECT RAISE(ABORT,'tree absence needs a registered root, its exit and both streams closed'); END;
CREATE TRIGGER process_spawn_failure_ambiguous BEFORE INSERT ON process_observations
WHEN NEW.kind='SPAWN_FAILED' AND EXISTS(SELECT 1 FROM process_records WHERE attempt=NEW.attempt)
BEGIN SELECT RAISE(ABORT,'spawn failure after registration'); END;
CREATE TABLE stream_checkpoints (
 attempt TEXT NOT NULL REFERENCES process_launches(attempt),
 stream TEXT NOT NULL CHECK(stream IN ('STDOUT','STDERR')),
 durable INTEGER NOT NULL CHECK(durable>=0), event INTEGER NOT NULL REFERENCES events(seq),
 PRIMARY KEY(attempt,stream,durable));
CREATE TRIGGER stream_checkpoint_monotonic BEFORE INSERT ON stream_checkpoints
WHEN NEW.durable < coalesce((SELECT max(durable) FROM stream_checkpoints
 WHERE attempt=NEW.attempt AND stream=NEW.stream),0)
BEGIN SELECT RAISE(ABORT,'stream checkpoints only advance'); END;
CREATE TABLE provider_events (
 attempt TEXT NOT NULL REFERENCES process_launches(attempt), sequence INTEGER NOT NULL CHECK(sequence>0),
 kind TEXT NOT NULL, stream TEXT NOT NULL CHECK(stream IN ('STDOUT','STDERR')),
 raw_start INTEGER NOT NULL CHECK(raw_start>=0), raw_end INTEGER NOT NULL, payload TEXT NOT NULL,
 event INTEGER NOT NULL REFERENCES events(seq), PRIMARY KEY(attempt,sequence), CHECK(raw_end>=raw_start));
CREATE TRIGGER provider_event_durable_order BEFORE INSERT ON provider_events
WHEN NEW.sequence<>coalesce((SELECT max(sequence) FROM provider_events WHERE attempt=NEW.attempt),0)+1
 OR NEW.raw_end > coalesce((SELECT max(durable) FROM stream_checkpoints
  WHERE attempt=NEW.attempt AND stream=NEW.stream),-1)
BEGIN SELECT RAISE(ABORT,'provider event must follow durable raw bytes in sequence'); END;
CREATE TRIGGER process_launches_immutable BEFORE UPDATE ON process_launches BEGIN SELECT RAISE(ABORT,'immutable process history'); END;
CREATE TRIGGER process_launches_keep BEFORE DELETE ON process_launches BEGIN SELECT RAISE(ABORT,'immutable process history'); END;
CREATE TRIGGER process_records_immutable BEFORE UPDATE ON process_records BEGIN SELECT RAISE(ABORT,'immutable process history'); END;
CREATE TRIGGER process_records_keep BEFORE DELETE ON process_records BEGIN SELECT RAISE(ABORT,'immutable process history'); END;
CREATE TRIGGER process_observations_immutable BEFORE UPDATE ON process_observations BEGIN SELECT RAISE(ABORT,'immutable process history'); END;
CREATE TRIGGER process_observations_keep BEFORE DELETE ON process_observations BEGIN SELECT RAISE(ABORT,'immutable process history'); END;
CREATE TRIGGER stream_checkpoints_immutable BEFORE UPDATE ON stream_checkpoints BEGIN SELECT RAISE(ABORT,'immutable process history'); END;
CREATE TRIGGER stream_checkpoints_keep BEFORE DELETE ON stream_checkpoints BEGIN SELECT RAISE(ABORT,'immutable process history'); END;
CREATE TRIGGER provider_events_immutable BEFORE UPDATE ON provider_events BEGIN SELECT RAISE(ABORT,'immutable process history'); END;
CREATE TRIGGER provider_events_keep BEFORE DELETE ON provider_events BEGIN SELECT RAISE(ABORT,'immutable process history'); END;
"#;

/// Version 8 (P1-10): stop causes, immutable attempt results, content-addressed evidence
/// references, attempt effect journal/observations, validation receipts and acceptance.
/// A result needs proven process absence; success additionally needs complete durable
/// evidence and no effect whose outcome is unknown.
const SCHEMA_V8: &str = r#"
CREATE TABLE attempt_stop_causes (
 attempt TEXT PRIMARY KEY REFERENCES approval_attempts(id),
 cause TEXT NOT NULL CHECK(cause IN ('TASK_CANCEL','APPROVAL_INVALIDATED','SHUTDOWN',
  'SUPERVISOR_PROTOCOL','SUPERVISOR_DEADLINE','SUPERVISOR_EVIDENCE_LOSS','SUPERVISOR_DESCENDANTS','SUPERVISOR_OTHER')),
 detail TEXT NOT NULL, event INTEGER NOT NULL REFERENCES events(seq));
CREATE TABLE evidence_blobs (
 digest TEXT PRIMARY KEY CHECK(length(digest)=64), size INTEGER NOT NULL CHECK(size>=0),
 event INTEGER NOT NULL REFERENCES events(seq));
CREATE TABLE attempt_evidence (
 attempt TEXT NOT NULL REFERENCES process_launches(attempt),
 kind TEXT NOT NULL CHECK(kind IN ('STDOUT','STDERR','CANDIDATE')),
 digest TEXT NOT NULL REFERENCES evidence_blobs(digest),
 event INTEGER NOT NULL REFERENCES events(seq), PRIMARY KEY(attempt,kind));
CREATE TABLE attempt_effects (
 operation_key TEXT PRIMARY KEY REFERENCES outbox(operation_key),
 attempt TEXT NOT NULL REFERENCES process_launches(attempt),
 event INTEGER NOT NULL REFERENCES events(seq));
CREATE TABLE effect_observations (
 event INTEGER PRIMARY KEY REFERENCES events(seq),
 operation_key TEXT NOT NULL REFERENCES attempt_effects(operation_key),
 observer TEXT NOT NULL CHECK(observer LIKE 'fake-remote:_%'),
 status TEXT NOT NULL CHECK(status IN ('COMMITTED','REJECTED','UNKNOWN')),
 payload_digest TEXT NOT NULL CHECK(length(payload_digest)=64));
CREATE TABLE attempt_results (
 attempt TEXT PRIMARY KEY REFERENCES process_launches(attempt),
 outcome TEXT NOT NULL CHECK(outcome IN ('SUCCEEDED','FAILED','CANCELLED','INTERRUPTED')),
 reason TEXT NOT NULL CHECK(length(reason)>0), stop_cause TEXT, exit_code INTEGER, exit_signal INTEGER,
 terminal TEXT NOT NULL, candidate TEXT, stdout TEXT, stderr TEXT, uncertainties TEXT NOT NULL,
 facts_digest TEXT NOT NULL CHECK(length(facts_digest)=64),
 epoch INTEGER NOT NULL REFERENCES engine_sessions(epoch),
 event INTEGER NOT NULL UNIQUE REFERENCES events(seq));
CREATE TABLE attempt_recovery_holds (
 event INTEGER PRIMARY KEY REFERENCES events(seq),
 attempt TEXT NOT NULL REFERENCES process_launches(attempt), reasons TEXT NOT NULL CHECK(length(reasons)>0));
CREATE TABLE validation_plans (
 digest TEXT PRIMARY KEY CHECK(length(digest)=64), canonical TEXT NOT NULL,
 event INTEGER NOT NULL REFERENCES events(seq));
CREATE TABLE validation_receipts (
 event INTEGER PRIMARY KEY REFERENCES events(seq),
 attempt TEXT NOT NULL REFERENCES attempt_results(attempt), candidate TEXT NOT NULL,
 plan TEXT NOT NULL REFERENCES validation_plans(digest), base TEXT NOT NULL,
 validator TEXT NOT NULL CHECK(length(validator)>0), report TEXT NOT NULL REFERENCES evidence_blobs(digest),
 changes TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN ('PASSED','FAILED')),
 reasons TEXT NOT NULL, approval_bundle TEXT NOT NULL);
CREATE TABLE candidate_acceptances (
 attempt TEXT PRIMARY KEY REFERENCES attempt_results(attempt),
 receipt INTEGER NOT NULL REFERENCES validation_receipts(event), origin TEXT NOT NULL,
 event INTEGER NOT NULL UNIQUE REFERENCES events(seq));

CREATE TRIGGER attempt_evidence_bound BEFORE INSERT ON attempt_evidence
WHEN EXISTS(SELECT 1 FROM attempt_results WHERE attempt=NEW.attempt)
 OR (NEW.kind IN ('STDOUT','STDERR') AND (SELECT size FROM evidence_blobs WHERE digest=NEW.digest)
  <> coalesce((SELECT max(durable) FROM stream_checkpoints WHERE attempt=NEW.attempt AND stream=NEW.kind),0))
 OR (NEW.kind='CANDIDATE' AND NOT EXISTS(SELECT 1 FROM worktree_candidates WHERE attempt=NEW.attempt AND digest=NEW.digest))
BEGIN SELECT RAISE(ABORT,'evidence must match durable coverage or a recorded candidate and precede the result'); END;
CREATE TRIGGER attempt_effect_unsealed BEFORE INSERT ON attempt_effects
WHEN EXISTS(SELECT 1 FROM attempt_results WHERE attempt=NEW.attempt)
 OR NOT EXISTS(SELECT 1 FROM outbox WHERE operation_key=NEW.operation_key AND state='NOT_ISSUED')
BEGIN SELECT RAISE(ABORT,'effects are journaled unissued for an unsealed attempt'); END;
CREATE TRIGGER attempt_result_proof BEFORE INSERT ON attempt_results
WHEN NOT EXISTS(SELECT 1 FROM process_observations WHERE attempt=NEW.attempt AND kind IN ('TREE_ABSENT','NOT_SPAWNED'))
 OR (NEW.outcome='SUCCEEDED' AND (NEW.candidate IS NULL OR NEW.stop_cause IS NOT NULL
  OR NEW.exit_code IS NOT 0 OR NEW.exit_signal IS NOT NULL
  OR NOT EXISTS(SELECT 1 FROM process_observations WHERE attempt=NEW.attempt AND kind='TREE_ABSENT')
  OR NOT EXISTS(SELECT 1 FROM attempt_evidence WHERE attempt=NEW.attempt AND kind='CANDIDATE' AND digest=NEW.candidate)
  OR (SELECT count(*) FROM attempt_evidence WHERE attempt=NEW.attempt AND kind IN ('STDOUT','STDERR'))<>2
  OR EXISTS(SELECT 1 FROM attempt_effects f JOIN outbox o ON o.operation_key=f.operation_key
   WHERE f.attempt=NEW.attempt AND o.state='ISSUED_UNKNOWN')))
BEGIN SELECT RAISE(ABORT,'result needs proven process absence; success needs complete durable evidence'); END;
CREATE TRIGGER validation_receipt_bound BEFORE INSERT ON validation_receipts
WHEN NEW.status='PASSED' AND (NEW.reasons<>'' OR NOT EXISTS(SELECT 1 FROM attempt_results
 WHERE attempt=NEW.attempt AND candidate=NEW.candidate))
BEGIN SELECT RAISE(ABORT,'passed validation must bind the sealed candidate'); END;
CREATE TRIGGER candidate_acceptance_bound BEFORE INSERT ON candidate_acceptances
WHEN NOT EXISTS(SELECT 1 FROM validation_receipts v JOIN attempt_results r ON r.attempt=v.attempt
 WHERE v.event=NEW.receipt AND v.attempt=NEW.attempt AND v.status='PASSED' AND r.outcome='SUCCEEDED')
BEGIN SELECT RAISE(ABORT,'acceptance needs a passed validation of a succeeded attempt'); END;
CREATE TRIGGER attempt_stop_causes_immutable BEFORE UPDATE ON attempt_stop_causes BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER attempt_stop_causes_keep BEFORE DELETE ON attempt_stop_causes BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER evidence_blobs_immutable BEFORE UPDATE ON evidence_blobs BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER evidence_blobs_keep BEFORE DELETE ON evidence_blobs BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER attempt_evidence_immutable BEFORE UPDATE ON attempt_evidence BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER attempt_evidence_keep BEFORE DELETE ON attempt_evidence BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER attempt_effects_immutable BEFORE UPDATE ON attempt_effects BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER attempt_effects_keep BEFORE DELETE ON attempt_effects BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER effect_observations_immutable BEFORE UPDATE ON effect_observations BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER effect_observations_keep BEFORE DELETE ON effect_observations BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER attempt_results_immutable BEFORE UPDATE ON attempt_results BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER attempt_results_keep BEFORE DELETE ON attempt_results BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER attempt_recovery_holds_immutable BEFORE UPDATE ON attempt_recovery_holds BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER attempt_recovery_holds_keep BEFORE DELETE ON attempt_recovery_holds BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER validation_plans_immutable BEFORE UPDATE ON validation_plans BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER validation_plans_keep BEFORE DELETE ON validation_plans BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER validation_receipts_immutable BEFORE UPDATE ON validation_receipts BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER validation_receipts_keep BEFORE DELETE ON validation_receipts BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER candidate_acceptances_immutable BEFORE UPDATE ON candidate_acceptances BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
CREATE TRIGGER candidate_acceptances_keep BEFORE DELETE ON candidate_acceptances BEGIN SELECT RAISE(ABORT,'immutable result history'); END;
"#;

/// Version 9 (P1-11): reopen start receipts, recovery resolutions, absence proofs from
/// recovery inspection and reviewed continuation plans. A recovery proof can support a
/// non-success result only; success still needs supervised TREE_ABSENT evidence.
const SCHEMA_V9: &str = r#"
CREATE TABLE recovery_starts (
 epoch INTEGER PRIMARY KEY REFERENCES engine_sessions(epoch),
 inspection TEXT NOT NULL CHECK(length(inspection)=64), subjects TEXT NOT NULL,
 origin TEXT NOT NULL, event INTEGER NOT NULL UNIQUE REFERENCES events(seq));
CREATE TABLE recovery_resolutions (
 event INTEGER PRIMARY KEY REFERENCES events(seq),
 attempt TEXT NOT NULL REFERENCES approval_attempts(id),
 epoch INTEGER NOT NULL REFERENCES engine_sessions(epoch),
 facts TEXT NOT NULL CHECK(length(facts)=64), findings TEXT NOT NULL, blockers TEXT NOT NULL,
 choice TEXT NOT NULL CHECK(choice IN ('VALIDATE_OUTPUT','CONTINUE_WITH_HISTORY','CONTINUE_FRESH_CONTEXT','HOLD')),
 origin TEXT NOT NULL);
CREATE TABLE recovery_absence_proofs (
 attempt TEXT PRIMARY KEY REFERENCES approval_attempts(id),
 epoch INTEGER NOT NULL REFERENCES engine_sessions(epoch),
 resolution INTEGER NOT NULL UNIQUE REFERENCES recovery_resolutions(event), basis TEXT NOT NULL);
CREATE TRIGGER recovery_absence_needs_unblocked_resolution BEFORE INSERT ON recovery_absence_proofs
WHEN NOT EXISTS(SELECT 1 FROM recovery_resolutions r WHERE r.event=NEW.resolution
 AND r.attempt=NEW.attempt AND r.epoch=NEW.epoch AND r.blockers='' AND r.choice<>'HOLD')
 OR EXISTS(SELECT 1 FROM scheduler_attempts a WHERE a.attempt=NEW.attempt AND a.epoch>=NEW.epoch)
BEGIN SELECT RAISE(ABORT,'absence proof needs an unblocked resolution of an earlier-epoch attempt'); END;
CREATE TABLE continuation_plans (
 successor TEXT PRIMARY KEY, predecessor TEXT NOT NULL UNIQUE REFERENCES attempt_results(attempt),
 resolution INTEGER NOT NULL REFERENCES recovery_resolutions(event),
 candidate TEXT, base TEXT NOT NULL, remaining TEXT NOT NULL CHECK(length(remaining)>0),
 effects TEXT NOT NULL, session TEXT NOT NULL, digest TEXT NOT NULL UNIQUE CHECK(length(digest)=64),
 origin TEXT NOT NULL, event INTEGER NOT NULL UNIQUE REFERENCES events(seq),
 CHECK(successor<>predecessor));
CREATE TRIGGER continuation_plan_bound BEFORE INSERT ON continuation_plans
WHEN NOT EXISTS(SELECT 1 FROM recovery_resolutions r WHERE r.event=NEW.resolution AND r.attempt=NEW.predecessor
 AND r.blockers='' AND r.choice IN ('CONTINUE_WITH_HISTORY','CONTINUE_FRESH_CONTEXT'))
 OR EXISTS(SELECT 1 FROM attempt_results WHERE attempt=NEW.predecessor AND (outcome='SUCCEEDED' OR candidate IS NOT NEW.candidate))
 OR EXISTS(SELECT 1 FROM approval_attempts WHERE id=NEW.successor)
BEGIN SELECT RAISE(ABORT,'continuation needs an unblocked continue resolution, the sealed candidate and a new attempt'); END;
DROP TRIGGER attempt_result_proof;
CREATE TRIGGER attempt_result_proof BEFORE INSERT ON attempt_results
WHEN NOT (EXISTS(SELECT 1 FROM process_observations WHERE attempt=NEW.attempt AND kind IN ('TREE_ABSENT','NOT_SPAWNED'))
  OR (NEW.outcome<>'SUCCEEDED' AND EXISTS(SELECT 1 FROM recovery_absence_proofs WHERE attempt=NEW.attempt AND epoch=NEW.epoch)))
 OR (NEW.outcome='SUCCEEDED' AND (NEW.candidate IS NULL OR NEW.stop_cause IS NOT NULL
  OR NEW.exit_code IS NOT 0 OR NEW.exit_signal IS NOT NULL
  OR NOT EXISTS(SELECT 1 FROM process_observations WHERE attempt=NEW.attempt AND kind='TREE_ABSENT')
  OR NOT EXISTS(SELECT 1 FROM attempt_evidence WHERE attempt=NEW.attempt AND kind='CANDIDATE' AND digest=NEW.candidate)
  OR (SELECT count(*) FROM attempt_evidence WHERE attempt=NEW.attempt AND kind IN ('STDOUT','STDERR'))<>2
  OR EXISTS(SELECT 1 FROM attempt_effects f JOIN outbox o ON o.operation_key=f.operation_key
   WHERE f.attempt=NEW.attempt AND o.state='ISSUED_UNKNOWN')))
BEGIN SELECT RAISE(ABORT,'result needs proven process absence; success needs complete durable evidence'); END;
CREATE TRIGGER recovery_starts_immutable BEFORE UPDATE ON recovery_starts BEGIN SELECT RAISE(ABORT,'immutable recovery history'); END;
CREATE TRIGGER recovery_starts_keep BEFORE DELETE ON recovery_starts BEGIN SELECT RAISE(ABORT,'immutable recovery history'); END;
CREATE TRIGGER recovery_resolutions_immutable BEFORE UPDATE ON recovery_resolutions BEGIN SELECT RAISE(ABORT,'immutable recovery history'); END;
CREATE TRIGGER recovery_resolutions_keep BEFORE DELETE ON recovery_resolutions BEGIN SELECT RAISE(ABORT,'immutable recovery history'); END;
CREATE TRIGGER recovery_absence_proofs_immutable BEFORE UPDATE ON recovery_absence_proofs BEGIN SELECT RAISE(ABORT,'immutable recovery history'); END;
CREATE TRIGGER recovery_absence_proofs_keep BEFORE DELETE ON recovery_absence_proofs BEGIN SELECT RAISE(ABORT,'immutable recovery history'); END;
CREATE TRIGGER continuation_plans_immutable BEFORE UPDATE ON continuation_plans BEGIN SELECT RAISE(ABORT,'immutable recovery history'); END;
CREATE TRIGGER continuation_plans_keep BEFORE DELETE ON continuation_plans BEGIN SELECT RAISE(ABORT,'immutable recovery history'); END;
"#;

/// P1-12: close tickets bound to one running epoch, their single resolution, the frozen
/// shutdown target set and append-only shutdown receipts. An unresolved or confirmed
/// ticket is a close gate layered over the session admission: no reservation and no
/// launch issue may pass it. READY needs no live claim and every target sealed.
const SCHEMA_V10: &str = r#"
CREATE TABLE close_tickets (
 ticket TEXT PRIMARY KEY, epoch INTEGER NOT NULL REFERENCES engine_sessions(epoch),
 origin TEXT NOT NULL CHECK(origin IN ('USER','DISCONNECT')), actor TEXT NOT NULL,
 active TEXT NOT NULL, event INTEGER NOT NULL UNIQUE REFERENCES events(seq));
CREATE TRIGGER close_ticket_current_and_single BEFORE INSERT ON close_tickets
WHEN NOT EXISTS(SELECT 1 FROM engine_sessions WHERE epoch=NEW.epoch AND shutdown='RUNNING')
 OR EXISTS(SELECT 1 FROM close_tickets t WHERE t.epoch=NEW.epoch
  AND NOT EXISTS(SELECT 1 FROM close_resolutions r WHERE r.ticket=t.ticket AND r.resolution='CANCELLED'))
BEGIN SELECT RAISE(ABORT,'close ticket needs the running epoch and no other open close'); END;
CREATE TABLE close_resolutions (
 ticket TEXT PRIMARY KEY REFERENCES close_tickets(ticket),
 resolution TEXT NOT NULL CHECK(resolution IN ('CANCELLED','CONFIRMED','NO_ACTIVE')),
 shutdown TEXT UNIQUE, actor TEXT NOT NULL, event INTEGER NOT NULL UNIQUE REFERENCES events(seq),
 CHECK((resolution='CANCELLED')=(shutdown IS NULL)));
CREATE TRIGGER close_resolution_current BEFORE INSERT ON close_resolutions
WHEN NOT EXISTS(SELECT 1 FROM close_tickets t JOIN engine_sessions e ON e.epoch=t.epoch
 WHERE t.ticket=NEW.ticket AND e.shutdown='RUNNING')
 OR (NEW.resolution='CANCELLED' AND EXISTS(SELECT 1 FROM close_tickets WHERE ticket=NEW.ticket AND origin='DISCONNECT'))
BEGIN SELECT RAISE(ABORT,'close ticket expired or belongs to an earlier epoch'); END;
CREATE TABLE shutdown_targets (
 shutdown TEXT NOT NULL REFERENCES close_resolutions(shutdown),
 attempt TEXT NOT NULL REFERENCES approval_attempts(id),
 event INTEGER NOT NULL REFERENCES events(seq), PRIMARY KEY(shutdown, attempt));
CREATE TABLE shutdown_receipts (
 event INTEGER PRIMARY KEY REFERENCES events(seq),
 shutdown TEXT NOT NULL REFERENCES close_resolutions(shutdown),
 outcome TEXT NOT NULL CHECK(outcome IN ('READY','FAILED','EMERGENCY')),
 process_stop_confirmed INTEGER NOT NULL CHECK(process_stop_confirmed IN (0,1)),
 persistence_sealed INTEGER NOT NULL CHECK(persistence_sealed IN (0,1)),
 targets TEXT NOT NULL, recovery_items TEXT NOT NULL, failures TEXT NOT NULL,
 CHECK((outcome='FAILED')=(failures<>'')),
 CHECK(outcome='FAILED' OR (process_stop_confirmed=1 AND persistence_sealed=1)));
CREATE UNIQUE INDEX one_final_shutdown_receipt ON shutdown_receipts(shutdown) WHERE outcome<>'FAILED';
CREATE TRIGGER shutdown_receipt_proof BEFORE INSERT ON shutdown_receipts
WHEN NEW.outcome<>'FAILED' AND (
  EXISTS(SELECT 1 FROM resource_claims WHERE state<>'RELEASED')
  OR EXISTS(SELECT 1 FROM shutdown_targets t WHERE t.shutdown=NEW.shutdown
   AND NOT EXISTS(SELECT 1 FROM scheduler_attempts a WHERE a.attempt=t.attempt AND a.sealed=1))
  OR (NEW.outcome='READY' AND (SELECT t.origin FROM close_resolutions r JOIN close_tickets t
   ON t.ticket=r.ticket WHERE r.shutdown=NEW.shutdown) IS NOT 'USER'))
BEGIN SELECT RAISE(ABORT,'clean shutdown needs sealed targets, no live claim and a user close'); END;
CREATE TRIGGER close_gate_blocks_reservation BEFORE INSERT ON scheduler_attempts
WHEN EXISTS(SELECT 1 FROM close_tickets t WHERE t.epoch=NEW.epoch
 AND NOT EXISTS(SELECT 1 FROM close_resolutions r WHERE r.ticket=t.ticket AND r.resolution='CANCELLED'))
BEGIN SELECT RAISE(ABORT,'close gate: no new reservation'); END;
CREATE TRIGGER close_gate_blocks_launch_issue BEFORE UPDATE ON outbox
WHEN NEW.operation_key LIKE 'scheduler-launch:%' AND NEW.state='ISSUED_UNKNOWN'
 AND EXISTS(SELECT 1 FROM close_tickets t WHERE t.epoch=NEW.updated_epoch
  AND NOT EXISTS(SELECT 1 FROM close_resolutions r WHERE r.ticket=t.ticket AND r.resolution='CANCELLED'))
BEGIN SELECT RAISE(ABORT,'close gate: no launch may cross the issue boundary'); END;
CREATE TRIGGER close_tickets_immutable BEFORE UPDATE ON close_tickets BEGIN SELECT RAISE(ABORT,'immutable shutdown history'); END;
CREATE TRIGGER close_tickets_keep BEFORE DELETE ON close_tickets BEGIN SELECT RAISE(ABORT,'immutable shutdown history'); END;
CREATE TRIGGER close_resolutions_immutable BEFORE UPDATE ON close_resolutions BEGIN SELECT RAISE(ABORT,'immutable shutdown history'); END;
CREATE TRIGGER close_resolutions_keep BEFORE DELETE ON close_resolutions BEGIN SELECT RAISE(ABORT,'immutable shutdown history'); END;
CREATE TRIGGER shutdown_targets_immutable BEFORE UPDATE ON shutdown_targets BEGIN SELECT RAISE(ABORT,'immutable shutdown history'); END;
CREATE TRIGGER shutdown_targets_keep BEFORE DELETE ON shutdown_targets BEGIN SELECT RAISE(ABORT,'immutable shutdown history'); END;
CREATE TRIGGER shutdown_receipts_immutable BEFORE UPDATE ON shutdown_receipts BEGIN SELECT RAISE(ABORT,'immutable shutdown history'); END;
CREATE TRIGGER shutdown_receipts_keep BEFORE DELETE ON shutdown_receipts BEGIN SELECT RAISE(ABORT,'immutable shutdown history'); END;
"#;

#[cfg(test)]
mod tests {
    //! Raw-SQL tests: the schema itself (not only the Rust API) refuses to rewrite history
    //! or to take a forbidden state transition, whichever connection issues the statement.

    use super::*;

    fn database() -> Connection {
        let conn = Connection::open(Path::new(":memory:"), OpenMode::Create).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON").unwrap();
        let report = migrate(&conn, Path::new("unused"), &MigrationPlan::production()).unwrap();
        assert_eq!((report.from_version, report.to_version), (0, 10));
        conn.execute_batch(
            "INSERT INTO engine_sessions VALUES (1, 0, 1, 1, 'CLOSED', 'RUNNING', NULL);
             INSERT INTO aggregates VALUES ('attempt', '00000000000000000000000000000001', 1, 's', 1);
             INSERT INTO commands VALUES ('00000000000000000000000000000009', 'k',
                 '0000000000000000000000000000000000000000000000000000000000000000', 1, 'r', 1, 0);
             INSERT INTO events (epoch, command_id, kind, payload, recorded_at_unix_ms)
                 VALUES (1, '00000000000000000000000000000009', 'E', '', 0);
             INSERT INTO resource_claims (resource_key, mode, holder_kind, holder_id, epoch, state,
                                          acquired_at_unix_ms)
                 VALUES ('worktree:1', 'EXCLUSIVE', 'attempt', '00000000000000000000000000000001',
                         1, 'ACTIVE', 0);
             INSERT INTO outbox VALUES ('op', 't', 'p',
                 '0000000000000000000000000000000000000000000000000000000000000000',
                 'NOT_ISSUED', 1, 1);",
        )
        .unwrap();
        conn
    }

    fn refused(conn: &Connection, sql: &str) {
        let error = conn
            .execute_batch(sql)
            .expect_err(&format!("statement must be refused: {sql}"));
        assert!(error.is_constraint(), "{sql}: {error}");
    }

    #[test]
    fn process_history_follows_issue_boundary_durable_bytes_and_is_immutable_in_sql() {
        let c = database();
        c.execute_batch("INSERT INTO approval_attempts VALUES ('a1','p','a','l','b','STARTING');
             INSERT INTO outbox VALUES ('scheduler-launch:a1','t','p','0000000000000000000000000000000000000000000000000000000000000000','NOT_ISSUED',1,1);").unwrap();
        let launch = "INSERT INTO process_launches VALUES ('a1',1,1,'ClaudeCode','p','/x','/w','/o','0000000000000000000000000000000000000000000000000000000000000000',1)";
        refused(&c, launch);
        // Setup only: the scheduler guard on this transition has its own tests.
        c.execute_batch(
            "DROP TRIGGER scheduler_launch_issue_guard;
             UPDATE outbox SET state='ISSUED_UNKNOWN' WHERE operation_key='scheduler-launch:a1';",
        )
        .unwrap();
        c.execute_batch(launch).unwrap();
        refused(
            &c,
            "INSERT INTO process_observations VALUES (1,'a1','TREE_ABSENT','no proof')",
        );
        refused(
            &c,
            "INSERT INTO provider_events VALUES ('a1',1,'TextDelta','STDOUT',0,5,'x',1)",
        );
        c.execute_batch(
            "INSERT INTO stream_checkpoints VALUES ('a1','STDOUT',5,1);
             INSERT INTO provider_events VALUES ('a1',1,'TextDelta','STDOUT',0,5,'x',1);
             INSERT INTO process_records VALUES ('a1',10,'start',10,1,'boot','ROOT',1);",
        )
        .unwrap();
        for sql in [
            "INSERT INTO provider_events VALUES ('a1',3,'TextDelta','STDOUT',0,5,'x',1)",
            "INSERT INTO provider_events VALUES ('a1',2,'TextDelta','STDOUT',5,6,'x',1)",
            "INSERT INTO stream_checkpoints VALUES ('a1','STDOUT',4,1)",
            "INSERT INTO process_records VALUES ('a1',11,'s',11,1,'boot','ROOT',1)",
            "UPDATE process_launches SET cwd='/elsewhere'",
            "UPDATE provider_events SET payload='forged'",
            "DELETE FROM stream_checkpoints",
            "DELETE FROM process_records",
            "UPDATE process_records SET start_identity='reused'",
        ] {
            refused(&c, sql);
        }
        c.execute_batch(
            "INSERT INTO process_observations VALUES (1,'a1','STOP_UNCONFIRMED','escaped');
             INSERT INTO events (epoch, command_id, kind, payload, recorded_at_unix_ms)
                 VALUES (1, '00000000000000000000000000000009', 'E', '', 0);",
        )
        .unwrap();
        refused(
            &c,
            "INSERT INTO process_observations VALUES (2,'a1','ROOT_EXITED','late')",
        );
        refused(&c, "UPDATE process_observations SET kind='TREE_ABSENT'");
        refused(&c, "DELETE FROM process_observations");
    }

    #[test]
    fn recovery_proofs_resolutions_and_continuation_plans_are_guarded_in_sql() {
        let c = database();
        let zero = "0".repeat(64);
        c.execute_batch(&format!(
            "UPDATE engine_sessions SET shutdown='UNCLEAN', ended_at_unix_ms=0 WHERE epoch=1;
             INSERT INTO engine_sessions VALUES (2, 0, 1, 9, 'CLOSED', 'RUNNING', NULL);
             INSERT INTO approval_attempts VALUES ('a1','p','a','l','b','STARTING'),('a2','p','a','l','b','RESERVED');
             INSERT INTO outbox VALUES ('scheduler-launch:a1','t','p','{zero}','NOT_ISSUED',1,1);
             DROP TRIGGER scheduler_launch_issue_guard;
             UPDATE outbox SET state='ISSUED_UNKNOWN' WHERE operation_key='scheduler-launch:a1';
             INSERT INTO process_launches VALUES ('a1',1,1,'ClaudeCode','p','/x','/w','/o','{zero}',1);
             INSERT INTO events (epoch, command_id, kind, payload, recorded_at_unix_ms)
                 VALUES (2,'00000000000000000000000000000009','E','',0),
                        (2,'00000000000000000000000000000009','E','',0),
                        (2,'00000000000000000000000000000009','E','',0),
                        (2,'00000000000000000000000000000009','E','',0),
                        (2,'00000000000000000000000000000009','E','',0);
             INSERT INTO recovery_resolutions VALUES
                 (2,'a1',2,'{zero}','f','','HOLD','h'),
                 (3,'a1',2,'{zero}','f','live orphan','HOLD','h'),
                 (4,'a1',2,'{zero}','f','','CONTINUE_FRESH_CONTEXT','h');"
        ))
        .unwrap();
        let result = |outcome: &str| {
            format!(
                "INSERT INTO attempt_results VALUES ('a1','{outcome}','r',NULL,NULL,NULL,'[]',NULL,
                 NULL,NULL,'','{zero}',2,5)"
            )
        };
        // No result without proven absence; a Hold or blocked resolution proves nothing.
        refused(&c, &result("INTERRUPTED"));
        refused(
            &c,
            "INSERT INTO recovery_absence_proofs VALUES ('a1',2,2,'b')",
        );
        refused(
            &c,
            "INSERT INTO recovery_absence_proofs VALUES ('a1',2,3,'b')",
        );
        refused(
            &c,
            "INSERT INTO recovery_absence_proofs VALUES ('a1',1,4,'b')",
        );
        c.execute_batch("INSERT INTO recovery_absence_proofs VALUES ('a1',2,4,'b')")
            .unwrap();
        // Recovered absence never supports success.
        refused(&c, &result("SUCCEEDED"));
        c.execute_batch(&result("INTERRUPTED")).unwrap();
        let plan = |successor: &str, resolution: u8, candidate: &str| {
            format!(
                "INSERT INTO continuation_plans VALUES ('{successor}','a1',{resolution},{candidate},
                 'base','rest','','FRESH_CONTEXT','{zero}','h',4)"
            )
        };
        refused(&c, &plan("s1", 2, "NULL"));
        refused(&c, &plan("s1", 4, "'other'"));
        refused(&c, &plan("a2", 4, "NULL"));
        refused(&c, &plan("a1", 4, "NULL"));
        c.execute_batch(&plan("s1", 4, "NULL")).unwrap();
        c.execute_batch(&format!(
            "INSERT INTO recovery_starts VALUES (2,'{zero}','none','h',3)"
        ))
        .unwrap();
        for sql in [
            "UPDATE recovery_resolutions SET blockers=''",
            "DELETE FROM recovery_resolutions",
            "UPDATE recovery_absence_proofs SET basis='x'",
            "DELETE FROM recovery_absence_proofs",
            "UPDATE continuation_plans SET remaining='x'",
            "DELETE FROM continuation_plans",
            "UPDATE recovery_starts SET subjects='x'",
            "DELETE FROM recovery_starts",
        ] {
            refused(&c, sql);
        }
    }

    #[test]
    fn close_tickets_gate_admission_and_clean_receipts_are_guarded_in_sql() {
        let c = database();
        let zero = "0".repeat(64);
        let refused_by = |sql: &str, needle: &str| {
            let error = c.execute_batch(sql).expect_err(sql);
            assert!(error.to_string().contains(needle), "{sql}: {error}");
        };
        c.execute_batch(&format!(
            "INSERT INTO events (epoch, command_id, kind, payload, recorded_at_unix_ms)
                 VALUES (1,'00000000000000000000000000000009','E','',0),
                        (1,'00000000000000000000000000000009','E','',0),
                        (1,'00000000000000000000000000000009','E','',0),
                        (1,'00000000000000000000000000000009','E','',0),
                        (1,'00000000000000000000000000000009','E','',0);
             INSERT INTO approval_attempts VALUES ('a1','p','a','l','b','RESERVED');
             INSERT INTO outbox VALUES ('scheduler-launch:a1','t','p','{zero}','NOT_ISSUED',1,1);
             DROP TRIGGER scheduler_launch_issue_guard;
             INSERT INTO close_tickets VALUES ('t1',1,'USER','u','',2);"
        ))
        .unwrap();
        let open = "no other open close";
        refused_by(
            "INSERT INTO close_tickets VALUES ('t2',1,'USER','u','',3)",
            open,
        );
        refused_by(
            "INSERT INTO close_tickets VALUES ('t2',2,'USER','u','',3)",
            open,
        );
        let issue =
            "UPDATE outbox SET state='ISSUED_UNKNOWN' WHERE operation_key='scheduler-launch:a1'";
        refused_by(issue, "no launch may cross the issue boundary");
        // Cancelling removes only this gate; the issue boundary opens again.
        c.execute_batch("INSERT INTO close_resolutions VALUES ('t1','CANCELLED',NULL,'u',3)")
            .unwrap();
        c.execute_batch(issue).unwrap();
        c.execute_batch("INSERT INTO close_tickets VALUES ('t2',1,'DISCONNECT','d','',4)")
            .unwrap();
        refused_by(
            "INSERT INTO close_resolutions VALUES ('t2','CANCELLED',NULL,'u',5)",
            "expired",
        );
        refused(
            &c,
            "INSERT INTO close_resolutions VALUES ('t2','CANCELLED','s2','u',5)",
        );
        c.execute_batch(
            "INSERT INTO close_resolutions VALUES ('t2','CONFIRMED','s2','d',5);
             INSERT INTO shutdown_targets VALUES ('s2','a1',5);",
        )
        .unwrap();
        refused_by(
            "INSERT INTO scheduler_attempts VALUES ('a1','s',1,1,1,0)",
            "no new reservation",
        );
        // A live claim (and a disconnect origin) refuse every clean receipt.
        let receipt = |outcome: &str, flags: &str, failures: &str| {
            format!(
                "INSERT INTO shutdown_receipts VALUES (6,'s2','{outcome}',{flags},'','','{failures}')"
            )
        };
        let clean = "clean shutdown needs";
        refused_by(&receipt("READY", "1,1", ""), clean);
        refused_by(&receipt("EMERGENCY", "1,1", ""), clean);
        refused(&c, &receipt("READY", "0,1", ""));
        refused(&c, &receipt("FAILED", "0,0", ""));
        c.execute_batch(&receipt("FAILED", "0,0", "stop unconfirmed"))
            .unwrap();
        for sql in [
            "UPDATE close_tickets SET actor='x'",
            "DELETE FROM close_tickets",
            "UPDATE close_resolutions SET resolution='CANCELLED'",
            "DELETE FROM close_resolutions",
            "UPDATE shutdown_targets SET attempt='x'",
            "DELETE FROM shutdown_targets",
            "UPDATE shutdown_receipts SET outcome='READY'",
            "DELETE FROM shutdown_receipts",
        ] {
            refused(&c, sql);
        }
    }

    #[test]
    fn results_evidence_effects_and_validation_are_guarded_and_immutable_in_sql() {
        let c = database();
        let zero = "0".repeat(64);
        let one = "1".repeat(64);
        c.execute_batch(&format!(
            "INSERT INTO approval_attempts VALUES ('a1','p','a','l','b','STARTING');
             INSERT INTO outbox VALUES ('scheduler-launch:a1','t','p','{zero}','NOT_ISSUED',1,1);
             DROP TRIGGER scheduler_launch_issue_guard;
             UPDATE outbox SET state='ISSUED_UNKNOWN' WHERE operation_key='scheduler-launch:a1';
             INSERT INTO process_launches VALUES ('a1',1,1,'ClaudeCode','p','/x','/w','/o','{zero}',1);
             INSERT INTO events (epoch, command_id, kind, payload, recorded_at_unix_ms)
                 VALUES (1,'00000000000000000000000000000009','E','',0),
                        (1,'00000000000000000000000000000009','E','',0),
                        (1,'00000000000000000000000000000009','E','',0);
             INSERT INTO evidence_blobs VALUES ('{zero}',5,1),('{one}',0,1);"
        ))
        .unwrap();
        let result = |outcome: &str, candidate: &str| {
            format!(
                "INSERT INTO attempt_results VALUES ('a1','{outcome}','r',NULL,0,NULL,'[]',{candidate},
                 NULL,NULL,'','{zero}',1,2)"
            )
        };
        // Process-unknown cannot finalize: no concluding absence/not-spawned proof yet.
        refused(&c, &result("FAILED", "NULL"));
        refused(
            &c,
            "INSERT INTO attempt_evidence VALUES ('a1','STDOUT','0000000000000000000000000000000000000000000000000000000000000000',1)",
        );
        refused(
            &c,
            "INSERT INTO attempt_evidence VALUES ('a1','CANDIDATE','1111111111111111111111111111111111111111111111111111111111111111',1)",
        );
        c.execute_batch(&format!(
            "INSERT INTO attempt_evidence VALUES ('a1','STDERR','{one}',1);
             INSERT INTO outbox VALUES ('attempt-effect:x','t','p','{zero}','NOT_ISSUED',1,1);
             INSERT INTO attempt_effects VALUES ('attempt-effect:x','a1',1);
             INSERT INTO process_observations VALUES (2,'a1','NOT_SPAWNED','stop');"
        ))
        .unwrap();
        // Success needs tree absence, candidate and complete logs, whatever a caller claims.
        refused(&c, &result("SUCCEEDED", &format!("'{one}'")));
        c.execute_batch(&result("FAILED", "NULL")).unwrap();
        for sql in [
            result("CANCELLED", "NULL"),
            "UPDATE attempt_results SET outcome='SUCCEEDED'".into(),
            "DELETE FROM attempt_results".into(),
            "UPDATE attempt_evidence SET digest='x'".into(),
            "DELETE FROM evidence_blobs".into(),
            "DELETE FROM attempt_effects".into(),
            format!("INSERT INTO attempt_evidence VALUES ('a1','STDOUT','{one}',1)"),
            format!(
                "INSERT INTO outbox VALUES ('attempt-effect:y','t','p','{zero}','NOT_ISSUED',1,1); INSERT INTO attempt_effects VALUES ('attempt-effect:y','a1',1)"
            ),
            format!(
                "INSERT INTO effect_observations VALUES (3,'attempt-effect:x','real-remote','COMMITTED','{zero}')"
            ),
        ] {
            refused(&c, &sql);
        }
        c.execute_batch(&format!(
            "INSERT INTO validation_plans VALUES ('{zero}','plan',1);"
        ))
        .unwrap();
        refused(
            &c,
            &format!(
                "INSERT INTO validation_receipts VALUES (3,'a1','{one}','{zero}','base','v','{zero}','','PASSED','','b')"
            ),
        );
        c.execute_batch(&format!("INSERT INTO validation_receipts VALUES (3,'a1','{one}','{zero}','base','v','{zero}','','FAILED','scope','b')")).unwrap();
        for sql in [
            "INSERT INTO candidate_acceptances VALUES ('a1',3,'human',3)",
            "UPDATE validation_receipts SET status='PASSED'",
            "DELETE FROM validation_receipts",
            "DELETE FROM validation_plans",
        ] {
            refused(&c, sql);
        }
    }

    #[test]
    fn worktree_ownership_lifecycle_and_candidates_are_immutable_in_sql() {
        let c = database();
        c.execute_batch("INSERT INTO local_repositories VALUES ('repo','/repo','/repo/.git','1:2'); INSERT INTO owned_worktrees(attempt,repository,epoch,state,root,root_identity,repo_path,common,common_identity,base,marker) VALUES ('run','repo',1,'WAITING','/owned','1:3','/repo','/repo/.git','1:2','base','marker');").unwrap();
        for sql in [
            "UPDATE local_repositories SET common='/other'",
            "DELETE FROM local_repositories",
            "DELETE FROM owned_worktrees",
            "UPDATE owned_worktrees SET attempt='other'",
            "UPDATE owned_worktrees SET root='/outside'",
            "UPDATE owned_worktrees SET epoch=2",
            "UPDATE owned_worktrees SET state='READY'",
            "UPDATE owned_worktrees SET snapshot='fake'",
        ] {
            refused(&c, sql);
        }
        c.execute_batch("UPDATE owned_worktrees SET state='CREATING',claim=1; UPDATE owned_worktrees SET state='READY',snapshot='digest'; INSERT INTO worktree_candidates VALUES ('run','digest','base','/owned/candidate');").unwrap();
        for sql in [
            "UPDATE owned_worktrees SET state='BOUND',snapshot='changed'",
            "UPDATE owned_worktrees SET state='BOUND',claim=NULL",
            "UPDATE worktree_candidates SET head='changed'",
            "DELETE FROM worktree_candidates",
        ] {
            refused(&c, sql);
        }
        c.execute_batch("UPDATE owned_worktrees SET state='BOUND'")
            .unwrap();
        refused(&c, "UPDATE owned_worktrees SET state='READY'");
    }

    #[test]
    fn scheduler_configuration_and_claim_quantities_are_durable_and_versioned() {
        let c = database();
        assert_eq!(
            c.query_one(
                "SELECT capacity FROM scheduler_capacities WHERE resource='global'",
                &[],
                |r| r.int(0)
            )
            .unwrap(),
            1
        );
        refused(&c, "INSERT INTO scheduler_configs VALUES (3,1)");
        c.execute_batch("INSERT INTO scheduler_configs VALUES (2,0); INSERT INTO scheduler_capacities VALUES (2,'global',0); INSERT INTO scheduler_claim_units VALUES (1,2);").unwrap();
        for sql in [
            "UPDATE scheduler_configs SET default_repository=9",
            "DELETE FROM scheduler_configs",
            "UPDATE scheduler_capacities SET capacity=9",
            "DELETE FROM scheduler_capacities",
            "UPDATE scheduler_claim_units SET units=99",
            "DELETE FROM scheduler_claim_units",
            "INSERT INTO scheduler_capacities VALUES (2,'bad',-1)",
            "INSERT INTO scheduler_capacities VALUES (2,'overflow',4294967296)",
        ] {
            refused(&c, sql);
        }
    }
    #[test]
    fn single_unsealed_step_constraint_also_covers_legacy_dependency_attempts() {
        let c = database();
        c.execute_batch("INSERT INTO review_documents VALUES ('p',1,'PLAN','prd','adr','body','display',1,1,1);
             INSERT INTO execution_plans VALUES ('p',1,'[]','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',1);
             INSERT INTO graph_nodes VALUES ('p',1,'Execution','n1',1);
             INSERT INTO approval_attempts VALUES ('run1','prd','adr','p','bundle','RESERVED'),('run2','prd','adr','p','bundle','RESERVED');
             INSERT INTO dependency_attempt_inputs VALUES ('run1','p',1,'n1',1,'Execution','sources','evaluation');").unwrap();
        refused(
            &c,
            "INSERT INTO dependency_attempt_inputs VALUES ('run2','p',1,'n1',1,'Execution','sources','evaluation')",
        );
        c.execute_batch("UPDATE approval_attempts SET phase='FINISHED' WHERE id='run1'; INSERT INTO dependency_attempt_inputs VALUES ('run2','p',1,'n1',1,'Execution','sources','evaluation');").unwrap();
    }

    #[test]
    fn approval_history_documents_stop_intents_and_results_are_immutable_in_sql() {
        let conn = database();
        conn.execute_batch(
            "INSERT INTO review_documents VALUES ('adr',1,'ADR','prd',NULL,'body','display',1,NULL,1);
             INSERT INTO human_decisions VALUES (1,'prd',1,'canonical','displayed','[]',NULL,'bundle','Approve','fake human','00000000000000000000000000000009');
             INSERT INTO approval_attempts VALUES ('run','prd','adr','plan','bundle','RUNNING');
             INSERT INTO approval_stops VALUES ('run',1);
             INSERT INTO approval_results VALUES ('run','sealed bytes');",
        ).unwrap();
        for (table, column) in [
            ("review_documents", "content"),
            ("human_decisions", "bundle"),
            ("approval_stops", "attempt"),
            ("approval_results", "body"),
        ] {
            refused(&conn, &format!("UPDATE {table} SET {column}='changed'"));
            refused(&conn, &format!("DELETE FROM {table}"));
        }
    }

    #[test]
    fn dependency_history_is_immutable_and_graph_foreign_keys_refuse_unknown_endpoints() {
        let c = database();
        c.execute_batch(
            "INSERT INTO review_documents VALUES ('p',1,'PLAN','prd','adr','body','display',1,1,1);
             INSERT INTO execution_plans VALUES ('p',1,'[]','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',1);
             INSERT INTO graph_nodes VALUES ('p',1,'Execution','n1',1),('p',1,'Execution','n2',1);
             INSERT INTO graph_edges VALUES ('p',1,'Execution','e','n1',1,'n2',1,'predicate');
             INSERT INTO dependency_observations VALUES ('fact',1,'[]','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',1);
             INSERT INTO approval_attempts VALUES ('run','prd','adr','p','bundle','RUNNING');
             INSERT INTO dependency_attempt_inputs VALUES ('run','p',1,'n2',1,'Execution','sources','evaluation');"
        ).unwrap();
        refused(
            &c,
            "INSERT INTO graph_edges VALUES ('p',1,'Execution','missing','n1',1,'missing',1,'predicate')",
        );
        refused(
            &c,
            "INSERT INTO graph_edges VALUES ('p',1,'Execution','self','n1',1,'n1',1,'predicate')",
        );
        refused(
            &c,
            "INSERT INTO graph_edges VALUES ('p',1,'Execution','duplicate','n1',1,'n2',1,'predicate')",
        );
        for (table, column) in [
            ("execution_plans", "canonical"),
            ("graph_nodes", "node"),
            ("graph_edges", "predicate"),
            ("dependency_observations", "canonical"),
            ("dependency_attempt_inputs", "sources"),
        ] {
            refused(&c, &format!("UPDATE {table} SET {column}='changed'"));
            refused(&c, &format!("DELETE FROM {table}"));
        }
    }

    #[test]
    fn history_tables_reject_update_and_delete() {
        let conn = database();
        for sql in [
            "UPDATE events SET payload = 'rewritten'",
            "DELETE FROM events",
            "UPDATE commands SET response = 'rewritten'",
            "DELETE FROM commands",
            "DELETE FROM engine_sessions",
            "DELETE FROM aggregates",
            "DELETE FROM resource_claims",
            "DELETE FROM outbox",
        ] {
            refused(&conn, sql);
        }
    }

    #[test]
    fn engine_sessions_start_closed_in_order_and_are_immutable_once_sealed() {
        let conn = database();
        // A second running session, a skipped epoch or an initially open gate are refused.
        refused(
            &conn,
            "INSERT INTO engine_sessions VALUES (2, 0, 1, 1, 'CLOSED', 'RUNNING', NULL)",
        );
        refused(
            &conn,
            "UPDATE engine_sessions SET shutdown = 'UNCLEAN', admission = 'OPEN'",
        );
        conn.execute_batch("UPDATE engine_sessions SET admission = 'OPEN'")
            .unwrap();
        refused(&conn, "UPDATE engine_sessions SET shutdown = 'CLEAN'");
        conn.execute_batch("UPDATE engine_sessions SET shutdown = 'UNCLEAN', admission = 'CLOSED'")
            .unwrap();
        refused(&conn, "UPDATE engine_sessions SET admission = 'OPEN'");
        refused(&conn, "UPDATE engine_sessions SET shutdown = 'RUNNING'");
        refused(
            &conn,
            "INSERT INTO engine_sessions VALUES (3, 0, 1, 1, 'CLOSED', 'RUNNING', NULL)",
        );
        refused(
            &conn,
            "INSERT INTO engine_sessions VALUES (2, 0, 1, 1, 'OPEN', 'RUNNING', NULL)",
        );
        refused(
            &conn,
            "INSERT INTO engine_sessions VALUES (2, 0, 1, 1, 'CLOSED', 'CLEAN', NULL)",
        );
        conn.execute_batch(
            "INSERT INTO engine_sessions VALUES (2, 0, 1, 1, 'CLOSED', 'RUNNING', NULL)",
        )
        .unwrap();
    }

    #[test]
    fn revisions_claims_and_effects_only_move_forward() {
        let conn = database();
        refused(&conn, "UPDATE aggregates SET revision = revision + 2");
        refused(&conn, "UPDATE aggregates SET state = 'silent change'");
        refused(
            &conn,
            "INSERT INTO aggregates VALUES ('attempt', '00000000000000000000000000000002', 5, 's', 1)",
        );
        conn.execute_batch("UPDATE aggregates SET revision = revision + 1, state = 'ok'")
            .unwrap();

        // A second live claim on an exclusively held resource is refused by the schema.
        refused(
            &conn,
            "INSERT INTO resource_claims (resource_key, mode, holder_kind, holder_id, epoch, state,
                                          acquired_at_unix_ms)
             VALUES ('worktree:1', 'SHARED', 'attempt', '00000000000000000000000000000001', 1,
                     'ACTIVE', 0)",
        );
        refused(&conn, "UPDATE resource_claims SET state = 'RELEASED'");
        conn.execute_batch("UPDATE resource_claims SET state = 'QUARANTINED'")
            .unwrap();
        refused(&conn, "UPDATE resource_claims SET state = 'ACTIVE'");
        conn.execute_batch(
            "UPDATE resource_claims SET state = 'RELEASED', release_basis = 'RECOVERY_RESOLVED',
                                        released_epoch = 1",
        )
        .unwrap();
        refused(
            &conn,
            "UPDATE resource_claims SET state = 'QUARANTINED', release_basis = NULL, released_epoch = NULL",
        );
        refused(
            &conn,
            "UPDATE resource_claims SET release_basis = 'TIMEOUT'",
        );

        refused(&conn, "UPDATE outbox SET state = 'CONFIRMED'");
        refused(
            &conn,
            "INSERT INTO outbox VALUES ('op2', 't', 'p',
            '0000000000000000000000000000000000000000000000000000000000000000', 'CONFIRMED', 1, 1)",
        );
        conn.execute_batch("UPDATE outbox SET state = 'ISSUED_UNKNOWN'")
            .unwrap();
        refused(&conn, "UPDATE outbox SET state = 'NOT_ISSUED'");
        refused(&conn, "UPDATE outbox SET payload = 'different operation'");
        conn.execute_batch("UPDATE outbox SET state = 'REJECTED'")
            .unwrap();
        refused(&conn, "UPDATE outbox SET state = 'CONFIRMED'");
    }

    #[test]
    fn artifact_versions_questions_and_answers_cannot_be_rewritten_or_misordered() {
        let conn = database();
        let digest = "1".repeat(64);
        let version = |revision: u32, parent: &str, feature: &str, digest: &str| {
            format!(
                "INSERT INTO artifact_versions VALUES ('artifact',
                     '000000000000000000000000000000a1', {revision}, {parent},
                     '{feature}', 'PRD', 1, 'content', '{digest}', 'rendered', '{digest}',
                     'HUMAN', 'note', 1, 1)"
            )
        };
        let feature = "000000000000000000000000000000f1";
        conn.execute_batch(&format!(
            "INSERT INTO aggregates VALUES
                 ('artifact', '000000000000000000000000000000a1', 1, '{digest}', 1)"
        ))
        .unwrap();
        // The digest must be the one the aggregate names; the first version has no parent.
        refused(&conn, &version(1, "NULL", feature, &"2".repeat(64)));
        refused(&conn, &version(1, "1", feature, &digest));
        conn.execute_batch(&version(1, "NULL", feature, &digest))
            .unwrap();
        refused(&conn, "UPDATE artifact_versions SET content = 'rewritten'");
        refused(&conn, "UPDATE artifact_versions SET rendered = 'rewritten'");
        refused(&conn, "DELETE FROM artifact_versions");
        // A version is only ever appended at the aggregate's current revision.
        refused(&conn, &version(2, "1", feature, &digest));
        refused(&conn, &version(3, "2", feature, &digest));
        conn.execute_batch("UPDATE aggregates SET revision = 2 WHERE kind = 'artifact'")
            .unwrap();
        refused(&conn, &version(2, "NULL", feature, &digest));
        refused(
            &conn,
            &version(2, "1", "000000000000000000000000000000f2", &digest),
        );
        conn.execute_batch(&version(2, "1", feature, &digest))
            .unwrap();

        refused(
            &conn,
            "INSERT INTO aggregates VALUES
                 ('question', '000000000000000000000000000000b1', 1, 'ANSWERED', 1)",
        );
        conn.execute_batch(
            "INSERT INTO aggregates VALUES
                 ('question', '000000000000000000000000000000b1', 1, 'BLOCKING', 1);
             INSERT INTO clarification_questions VALUES
                 ('question', '000000000000000000000000000000b1',
                  '000000000000000000000000000000f1', NULL, NULL, 'Which?', 'MODEL', '', 1, 1);",
        )
        .unwrap();
        refused(
            &conn,
            "UPDATE aggregates SET revision = 2, state = 'DONE' WHERE kind = 'question'",
        );
        refused(
            &conn,
            "INSERT INTO clarification_questions VALUES
                 ('question', '000000000000000000000000000000b2',
                  '000000000000000000000000000000f1', NULL, NULL, 'Orphan?', 'HUMAN', '', 1, 1)",
        );
        let answer = |ordinal: u32| {
            format!(
                "INSERT INTO clarification_answers VALUES
                     ('000000000000000000000000000000b1', {ordinal}, 'This.', 'HUMAN', '', 1, 1)"
            )
        };
        refused(&conn, &answer(2));
        conn.execute_batch(&answer(1)).unwrap();
        refused(&conn, &answer(1));
        refused(&conn, &answer(3));
        conn.execute_batch(&answer(2)).unwrap();
        for sql in [
            "UPDATE clarification_questions SET prompt = 'rewritten'",
            "DELETE FROM clarification_questions",
            "UPDATE clarification_answers SET body = 'rewritten'",
            "DELETE FROM clarification_answers",
        ] {
            refused(&conn, sql);
        }
    }

    #[test]
    fn foreign_keys_are_enforced_including_event_causation_at_commit() {
        let conn = database();
        refused(
            &conn,
            "INSERT INTO aggregates VALUES ('a', '00000000000000000000000000000003', 1, 's', 99)",
        );
        refused(
            &conn,
            "INSERT INTO events (epoch, kind, aggregate_kind, aggregate_id, aggregate_revision,
                                 payload, recorded_at_unix_ms)
             VALUES (1, 'E', 'attempt', '000000000000000000000000000000ff', 1, '', 0)",
        );
        // The causation key is deferred: the insert is accepted, the commit is not.
        conn.execute_batch(
            "BEGIN;
             INSERT INTO events (epoch, command_id, kind, payload, recorded_at_unix_ms)
             VALUES (1, '000000000000000000000000000000aa', 'E', '', 0);",
        )
        .unwrap();
        refused(&conn, "COMMIT");
        conn.execute_batch("ROLLBACK").unwrap();
        let events = conn
            .query_one("SELECT count(*) FROM events", &[], |r| r.int(0))
            .unwrap();
        assert_eq!(events, 1);
    }

    #[test]
    fn databases_not_created_by_jojo_are_refused_before_any_change() {
        let plan = MigrationPlan::production();
        let foreign = Connection::open(Path::new(":memory:"), OpenMode::Create).unwrap();
        foreign
            .execute_batch("CREATE TABLE somebody_elses (x INTEGER)")
            .unwrap();
        assert!(matches!(
            migrate(&foreign, Path::new("unused"), &plan),
            Err(StorageError::ForeignDatabase)
        ));
        let versioned = Connection::open(Path::new(":memory:"), OpenMode::Create).unwrap();
        versioned.execute_batch("PRAGMA user_version = 1").unwrap();
        assert!(matches!(
            migrate(&versioned, Path::new("unused"), &plan),
            Err(StorageError::ForeignDatabase)
        ));
        let tables = foreign
            .query_one("SELECT count(*) FROM sqlite_schema", &[], |r| r.int(0))
            .unwrap();
        assert_eq!(tables, 1);
    }

    #[test]
    fn a_step_that_ends_its_own_transaction_is_reported_as_a_failed_migration() {
        let conn = Connection::open(Path::new(":memory:"), OpenMode::Create).unwrap();
        let plan = MigrationPlan::new(vec![Migration {
            version: 1,
            name: "escapes",
            sql: "CREATE TABLE t (x INTEGER); COMMIT;",
        }])
        .unwrap();
        assert!(matches!(
            migrate(&conn, Path::new("unused"), &plan),
            Err(StorageError::MigrationFailed {
                failed_version: 1,
                ..
            })
        ));
        assert_eq!(user_version(&conn).unwrap(), 0);
    }
}

//! M5: the push of a finished plan branch and the draft pull request of a pushed one
//! (schema 16, protocol 1.10). Each is an outbox effect with its own record:
//!
//! requested (`NOT_ISSUED`) → decided by the human on its exact payload → issued once
//! (`ISSUED_UNKNOWN`, committed before anything leaves the machine) → resolved only by an
//! observation (`CONFIRMED` or `REJECTED`).
//!
//! An issued action whose outcome was not observed (a lost answer, a helper that stopped)
//! stays `ISSUED_UNKNOWN`: it is never sent again (D2). The human inspects the remote,
//! which records what it shows, and may then mark the action as not performed, after which
//! a new request can be made. The title and body of a pull request are built here from
//! what the core recorded, never by a model, and are part of the payload the human
//! authorizes.
use crate::model::{id_from_text, id_to_text, to_db};
use crate::overview::{document_feature, feature_fields, identity_text, optional, time_of};
use crate::runner::PlanRunView;
use crate::sqlite::{Connection, Value};
use crate::*;
use jojo_domain::identity::{ArtifactId, RepositoryId, Revision};
use jojo_domain::ipc::{encode, identity, number, object, text};
use jojo_domain::json::Json;
use jojo_domain::payload::Record;
use jojo_domain::prd::PrdDocument;
use jojo_domain::provider::ProviderKind;

/// The remote every push goes to.
pub const REMOTE: &str = "origin";
/// Actions and runs the `ExternalActions` answer lists at most.
pub const EXTERNAL_ROWS: usize = 32;
const RUN_ROWS: usize = 16;
/// The most characters of a pull request body. A request's body is listed in full while
/// it waits for the decision, so that what the human authorizes is what they read.
pub const BODY_CHARS: usize = 4096;

fn deny(s: impl Into<String>) -> TxError {
    TxError::Rejected(s.into())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalKind {
    Push,
    PullRequest,
}
impl ExternalKind {
    fn db(self) -> &'static str {
        match self {
            Self::Push => "PUSH",
            Self::PullRequest => "PULL_REQUEST",
        }
    }
    fn from_db(s: &str) -> Result<Self, StorageError> {
        match s {
            "PUSH" => Ok(Self::Push),
            "PULL_REQUEST" => Ok(Self::PullRequest),
            _ => Err(StorageError::corrupt("unknown external action kind")),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Push => "Push",
            Self::PullRequest => "PullRequest",
        }
    }
    fn word(self) -> &'static str {
        match self {
            Self::Push => "push",
            Self::PullRequest => "pr",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalStatus {
    Committed,
    Rejected,
    Unknown,
}
impl ExternalStatus {
    fn db(self) -> &'static str {
        match self {
            Self::Committed => "COMMITTED",
            Self::Rejected => "REJECTED",
            Self::Unknown => "UNKNOWN",
        }
    }
}
fn status_word(db: &str) -> &'static str {
    match db {
        "COMMITTED" => "Committed",
        "REJECTED" => "Rejected",
        _ => "Unknown",
    }
}

/// What a dispatch or an inspection saw.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalObservation {
    /// `git:push`, `gh:pr-create`, `git:ls-remote`, `gh:pr-list` or `fake-remote:…`.
    pub observer: String,
    pub status: ExternalStatus,
    pub detail: String,
    pub number: Option<u64>,
    pub url: Option<String>,
}

/// One external action as the core recorded it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalAction {
    pub key: String,
    pub kind: ExternalKind,
    pub plan: ArtifactId,
    pub revision: Revision,
    pub sequence: u64,
    pub repository: RepositoryId,
    pub remote: String,
    pub url: String,
    pub branch: String,
    pub commit: String,
    pub base: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
    pub push: Option<String>,
    pub digest: String,
    pub outbox: EffectState,
    /// `AUTHORIZED` or `DECLINED`, once the human decided.
    pub decision: Option<String>,
    pub event: i64,
}
impl ExternalAction {
    /// Waiting for the human's decision.
    pub fn requested(&self) -> bool {
        self.outbox == EffectState::NotIssued && self.decision.is_none()
    }
    /// Requested or issued without a known outcome: no second one may start.
    fn open(&self) -> bool {
        self.requested() || self.outbox == EffectState::IssuedUnknown
    }
}

fn load(c: &Connection, key: &str) -> Result<Option<ExternalAction>, StorageError> {
    let row = c.query_optional(
        "SELECT a.kind,a.plan,a.revision,a.sequence,a.repository,a.remote,a.url,a.branch,
                a.commit_id,a.base,a.title,a.body,a.push,o.payload_digest,o.state,
                (SELECT decision FROM external_action_decisions d WHERE d.operation_key=a.operation_key),
                a.event
         FROM external_actions a JOIN outbox o ON o.operation_key=a.operation_key
         WHERE a.operation_key=?1",
        &[Value::Text(key)],
        |r| {
            Ok((
                (r.text(0)?, r.text(1)?, r.int(2)?, r.int(3)?, r.text(4)?, r.text(5)?),
                (r.text(6)?, r.text(7)?, r.text(8)?, r.optional_text(9)?),
                (r.optional_text(10)?, r.optional_text(11)?, r.optional_text(12)?),
                (r.text(13)?, r.text(14)?, r.optional_text(15)?, r.int(16)?),
            ))
        },
    )?;
    let Some((
        (kind, plan, revision, sequence, repository, remote),
        (url, branch, commit, base),
        (title, body, push),
        (digest, state, decision, event),
    )) = row
    else {
        return Ok(None);
    };
    Ok(Some(ExternalAction {
        key: key.to_owned(),
        kind: ExternalKind::from_db(&kind)?,
        plan: ArtifactId(id_from_text(&plan)?),
        revision: Revision(u64::try_from(revision).map_err(|_| StorageError::corrupt("revision"))?),
        sequence: u64::try_from(sequence).map_err(|_| StorageError::corrupt("sequence"))?,
        repository: RepositoryId(id_from_text(&repository)?),
        remote,
        url,
        branch,
        commit,
        base,
        title,
        body,
        push,
        digest,
        outbox: EffectState::from_db(&state)?,
        decision,
        event,
    }))
}

fn keys(c: &Connection, plan: &str, kind: ExternalKind) -> Result<Vec<String>, StorageError> {
    Ok(c.query(
        "SELECT operation_key FROM external_actions WHERE plan=?1 AND kind=?2 ORDER BY event DESC",
        &[Value::Text(plan), Value::Text(kind.db())],
        |r| r.text(0),
    )?)
}

fn actions(
    c: &Connection,
    plan: &str,
    kind: ExternalKind,
) -> Result<Vec<ExternalAction>, StorageError> {
    keys(c, plan, kind)?
        .iter()
        .filter_map(|k| load(c, k).transpose())
        .collect()
}

/// The URL forms a push may go to: HTTPS without credentials in it, or a local path.
pub fn url_refusal(url: &str) -> Option<String> {
    if let Some(rest) = url.strip_prefix("https://") {
        let authority = rest.split('/').next().unwrap_or("");
        if authority.contains('@') {
            return Some(
                "origin carries credentials in its URL; Jojo neither stores nor sends them".into(),
            );
        }
        return None;
    }
    if url.starts_with("file://") || url.starts_with('/') {
        return None;
    }
    Some(format!(
        "origin is {url}; Jojo pushes over HTTPS with git's own credential helper (an SSH \
         remote needs an agent the helper does not get)"
    ))
}

fn branch_name(base: &str) -> bool {
    !base.is_empty()
        && base.len() <= 200
        && !base.starts_with('-')
        && !base.contains("..")
        && base
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.'))
}

fn provider_words(p: ProviderKind) -> &'static str {
    match p {
        ProviderKind::ClaudeCode => "Claude Code",
        ProviderKind::Codex => "Codex",
    }
}

fn short(commit: &str) -> &str {
    &commit[..commit.len().min(12)]
}

/// The title and body of the draft pull request of a finished run: the approved
/// requirements' title, goal and acceptance criteria, each step with its provider, host
/// validation and commit, and where it came from. Deterministic; no model writes it.
pub fn pull_request_text(
    c: &Connection,
    run: &PlanRunView,
) -> Result<(String, String), StorageError> {
    let content = c.query_optional(
        "SELECT content FROM artifact_versions WHERE artifact_id=?1 ORDER BY revision DESC LIMIT 1",
        &[Value::Text(&id_to_text(run.subjects.prd.0))],
        |r| r.text(0),
    )?;
    let prd = content.and_then(|c| PrdDocument::import(c.as_bytes()).ok());
    let plan = id_to_text(run.plan.0);
    let title = prd
        .as_ref()
        .map(|d| d.title.trim().to_owned())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| format!("Jojo plan {}", &plan[..8]))
        .chars()
        .take(256)
        .collect::<String>();
    let cell = |s: &str| s.replace(['|', '\n'], " ");
    let mut body = String::new();
    if let Some(d) = &prd {
        if !d.goal.trim().is_empty() {
            body.push_str(d.goal.trim());
            body.push_str("\n\n");
        }
        if !d.acceptance_criteria.is_empty() {
            body.push_str("## Acceptance criteria\n\n");
            for criterion in &d.acceptance_criteria {
                body.push_str(&format!(
                    "- {}: {}\n",
                    criterion.id,
                    criterion.statement.trim()
                ));
            }
            body.push('\n');
        }
    }
    body.push_str("## Steps\n\n| Step | Title | Provider | Host validation | Commit |\n|---|---|---|---|---|\n");
    for step in &run.steps {
        body.push_str(&format!(
            "| `{}` | {} | {} | {} | {} |\n",
            cell(&step.key),
            cell(&step.title),
            provider_words(step.provider),
            step.receipt.map_or("not recorded".to_owned(), |r| format!(
                "passed, receipt {r}"
            )),
            step.commit
                .as_deref()
                .map_or("not integrated".to_owned(), |c| format!("`{}`", short(c))),
        ));
    }
    body.push_str(&format!(
        "\n## Provenance\n\nJojo plan `{plan}` revision {}; branch `{}` from `{}` to `{}`. \
         Each step was validated on the host and accepted by a human. This draft was opened \
         by Jojo after a separate human authorization.\n",
        run.revision.0,
        run.branch,
        short(&run.base),
        short(&run.head),
    ));
    Ok((title, body.chars().take(BODY_CHARS).collect()))
}

/// Why a run's branch cannot be pushed now, or `None` when it can.
fn push_refusal(c: &Connection, run: &PlanRunView) -> Result<Option<String>, StorageError> {
    if run.state != "DONE" {
        return Ok(Some(format!(
            "the run is {}; only a finished run (every step on the plan branch) is pushed",
            run.state
        )));
    }
    let plan = id_to_text(run.plan.0);
    for a in actions(c, &plan, ExternalKind::Push)? {
        if a.open() {
            return Ok(Some(format!("a push of this plan waits: {}", a.key)));
        }
        if a.outbox == EffectState::Confirmed && a.commit == run.head {
            return Ok(Some("the plan branch is pushed".into()));
        }
    }
    Ok(None)
}

/// The confirmed push of a run's head, and why a pull request cannot be opened now.
fn pull_request_refusal(
    c: &Connection,
    run: &PlanRunView,
) -> Result<(Option<ExternalAction>, Option<String>), StorageError> {
    let plan = id_to_text(run.plan.0);
    let pushed = actions(c, &plan, ExternalKind::Push)?
        .into_iter()
        .find(|a| {
            a.outbox == EffectState::Confirmed && a.commit == run.head && a.revision == run.revision
        });
    if pushed.is_none() {
        return Ok((None, Some("the plan branch is not pushed yet".into())));
    }
    for a in actions(c, &plan, ExternalKind::PullRequest)? {
        if a.open() {
            return Ok((
                pushed,
                Some(format!("a pull request of this plan waits: {}", a.key)),
            ));
        }
        if a.outbox == EffectState::Confirmed {
            return Ok((pushed, Some("the draft pull request is open".into())));
        }
    }
    Ok((pushed, None))
}

impl Tx<'_> {
    /// The human asks for a push (`url`: the repository's origin) or a draft pull request
    /// (`base`: the remote's default branch) of the newest run of `plan`. Nothing is
    /// performed; the request waits for its decision. Returns the operation key.
    pub fn request_external(
        &mut self,
        plan: ArtifactId,
        kind: ExternalKind,
        url: &str,
        base: Option<&str>,
    ) -> Result<String, TxError> {
        let view = ReadView { conn: self.conn };
        let run = view
            .plan_run(plan)?
            .ok_or_else(|| deny("ExternalRefused: this plan was never started"))?;
        let plan_text = id_to_text(plan.0);
        let (payload, target, push, title, body) = match kind {
            ExternalKind::Push => {
                if let Some(why) = push_refusal(self.conn, &run)? {
                    return Err(deny(format!("ExternalRefused: {why}")));
                }
                if let Some(why) = url_refusal(url) {
                    return Err(deny(format!("ExternalRefused: {why}")));
                }
                let payload = encode(&object([
                    ("kind", text("Push")),
                    ("remote", text(REMOTE)),
                    ("url", text(url)),
                    ("ref", text(format!("refs/heads/{}", run.branch))),
                    ("commit", text(run.head.clone())),
                ]));
                let target = format!(
                    "push {} to {REMOTE} {url} refs/heads/{}",
                    short(&run.head),
                    run.branch
                );
                (payload, target, None, None, None)
            }
            ExternalKind::PullRequest => {
                let (pushed, refusal) = pull_request_refusal(self.conn, &run)?;
                if let Some(why) = refusal {
                    return Err(deny(format!("ExternalRefused: {why}")));
                }
                let pushed = pushed.ok_or_else(|| deny("ExternalRefused: not pushed"))?;
                let base = base.filter(|b| branch_name(b)).ok_or_else(|| {
                    deny("ExternalRefused: the remote's default branch is not a plain branch name")
                })?;
                let (title, body) = pull_request_text(self.conn, &run)?;
                let payload = encode(&object([
                    ("kind", text("PullRequest")),
                    ("url", text(pushed.url.clone())),
                    ("base", text(base)),
                    ("head", text(run.branch.clone())),
                    ("commit", text(run.head.clone())),
                    ("title", text(title.clone())),
                    ("body", text(body.clone())),
                    ("push", text(pushed.key.clone())),
                ]));
                let target = format!(
                    "draft pull request {} into {base} at {}",
                    run.branch, pushed.url
                );
                (
                    payload,
                    target,
                    Some((pushed.key, base.to_owned())),
                    Some(title),
                    Some(body),
                )
            }
        };
        let url = match kind {
            ExternalKind::Push => url.to_owned(),
            ExternalKind::PullRequest => {
                let push = push.as_ref().map(|(k, _)| k.as_str()).unwrap_or("");
                load(self.conn, push)?.map(|a| a.url).unwrap_or_default()
            }
        };
        let sequence = self.conn.query_one(
            "SELECT count(*)+1 FROM external_actions WHERE kind=?1 AND plan=?2 AND revision=?3 AND commit_id=?4",
            &[
                Value::Text(kind.db()),
                Value::Text(&plan_text),
                Value::Int(to_db(run.revision.0)?),
                Value::Text(&run.head),
            ],
            |r| r.int(0),
        )?;
        let key = format!(
            "external:{}:{plan_text}:{}:{sequence}",
            kind.word(),
            run.head
        );
        self.enqueue_effect(&key, &target, &payload)?;
        let event = self.append_event(
            "ExternalActionRequested",
            None,
            &Record::new()
                .text("key", key.clone())
                .text("action", kind.name())
                .text("plan", plan_text.clone()),
        )?;
        let (push, base) = push.map_or((None, None), |(k, b)| (Some(k), Some(b)));
        fn opt(v: &Option<String>) -> Value<'_> {
            v.as_deref().map_or(Value::Null, Value::Text)
        }
        self.conn.execute(
            "INSERT INTO external_actions VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
            &[
                Value::Text(&key),
                Value::Text(kind.db()),
                Value::Text(&plan_text),
                Value::Int(to_db(run.revision.0)?),
                Value::Int(sequence),
                Value::Text(&id_to_text(run.repository.0)),
                Value::Text(REMOTE),
                Value::Text(&url),
                Value::Text(&run.branch),
                Value::Text(&run.head),
                opt(&base),
                opt(&title),
                opt(&body),
                opt(&push),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        Ok(key)
    }

    /// The human's decision on the exact payload `digest`. An authorization re-checks what
    /// the request was made on and issues the action in this same transaction, before
    /// anything leaves the machine; a decline ends it. Either is made once.
    pub fn decide_external(
        &mut self,
        key: &str,
        digest: &str,
        authorize: bool,
        actor: &str,
    ) -> Result<String, TxError> {
        let action =
            load(self.conn, key)?.ok_or_else(|| deny("UnknownExternalAction: no such request"))?;
        if !action.requested() {
            return Err(deny("ExternalDecided: this request was already decided"));
        }
        if action.digest != digest {
            return Err(deny(
                "StaleExternal: the request is not the one reviewed; read it again",
            ));
        }
        if authorize {
            let view = ReadView { conn: self.conn };
            let run = view
                .plan_run(action.plan)?
                .ok_or_else(|| deny("ExternalRefused: the plan run is gone"))?;
            if run.revision != action.revision || run.head != action.commit || run.state != "DONE" {
                return Err(deny(
                    "StaleExternal: the plan branch moved since the request; request it again",
                ));
            }
            if action.kind == ExternalKind::PullRequest {
                let push = action.push.as_deref().unwrap_or("");
                let pushed = load(self.conn, push)?;
                if pushed.is_none_or(|p| p.outbox != EffectState::Confirmed) {
                    return Err(deny("ExternalRefused: the plan branch is not pushed"));
                }
            }
        }
        let (decision, kind) = if authorize {
            ("AUTHORIZED", "ExternalActionAuthorized")
        } else {
            ("DECLINED", "ExternalActionDeclined")
        };
        let event = self.append_event(
            kind,
            None,
            &Record::new().text("key", key).text("actor", actor),
        )?;
        self.conn.execute(
            "INSERT INTO external_action_decisions VALUES (?1,?2,?3,?4,?5)",
            &[
                Value::Text(key),
                Value::Text(decision),
                Value::Text(digest),
                Value::Text(actor),
                Value::Int(to_db(event.0)?),
            ],
        )?;
        if !authorize {
            return Ok(format!("{} declined", action.kind.name()));
        }
        self.advance_effect(key, EffectState::IssuedUnknown)?;
        self.append_event(
            "ExternalActionIssued",
            None,
            &Record::new().text("key", key),
        )?;
        Ok(format!("{} authorized; issued once", action.kind.name()))
    }

    /// Records what a dispatch or an inspection saw. Only an issued action with an unknown
    /// outcome can be resolved; `Unknown` is kept without changing it.
    pub fn observe_external(
        &mut self,
        key: &str,
        observation: &ExternalObservation,
    ) -> Result<String, TxError> {
        let action =
            load(self.conn, key)?.ok_or_else(|| deny("UnknownExternalAction: no such action"))?;
        if action.outbox != EffectState::IssuedUnknown {
            return Err(deny(
                "ExternalSettled: only an issued action with unknown outcome is observed",
            ));
        }
        if observation.observer.starts_with("human:") {
            return Err(deny("a human resolution is not an observation"));
        }
        self.observation_row(&action, observation)?;
        match observation.status {
            ExternalStatus::Committed => self.advance_effect(key, EffectState::Confirmed)?,
            ExternalStatus::Rejected => self.advance_effect(key, EffectState::Rejected)?,
            ExternalStatus::Unknown => {}
        }
        Ok(format!("observed {}", observation.status.db()))
    }

    /// The human asks to read the remote again for an issued action whose outcome is
    /// unknown. Reading changes nothing remote; what it shows is recorded as observed.
    pub fn begin_external_inspection(&mut self, key: &str) -> Result<String, TxError> {
        let action =
            load(self.conn, key)?.ok_or_else(|| deny("UnknownExternalAction: no such action"))?;
        if action.outbox != EffectState::IssuedUnknown {
            return Err(deny("ExternalSettled: the outcome is known"));
        }
        self.append_event(
            "ExternalActionInspected",
            None,
            &Record::new().text("key", key),
        )?;
        Ok("inspection started".into())
    }

    /// The human marks an issued action whose outcome stays unknown as not performed,
    /// after the remote was inspected at least once. A new request may follow.
    pub fn resolve_external(&mut self, key: &str, actor: &str) -> Result<String, TxError> {
        let action =
            load(self.conn, key)?.ok_or_else(|| deny("UnknownExternalAction: no such action"))?;
        if action.outbox != EffectState::IssuedUnknown {
            return Err(deny("ExternalSettled: the outcome is known"));
        }
        let inspected = self.conn.query_one(
            "SELECT count(*) FROM external_action_observations WHERE operation_key=?1
             AND status='UNKNOWN' AND (observer LIKE 'git:ls-remote%' OR observer LIKE 'gh:pr-list%')",
            &[Value::Text(key)],
            |r| r.int(0),
        )?;
        if inspected == 0 {
            return Err(deny(
                "InspectFirst: inspect the remote before marking the action not performed",
            ));
        }
        self.observation_row(
            &action,
            &ExternalObservation {
                observer: "human:not-performed".into(),
                status: ExternalStatus::Rejected,
                detail: format!("marked not performed by {actor} after inspecting the remote"),
                number: None,
                url: None,
            },
        )?;
        self.advance_effect(key, EffectState::Rejected)?;
        Ok("marked not performed".into())
    }

    fn observation_row(
        &mut self,
        action: &ExternalAction,
        o: &ExternalObservation,
    ) -> Result<(), TxError> {
        let detail: String = o.detail.chars().take(1024).collect();
        let event = self.append_event(
            "ExternalActionObserved",
            None,
            &Record::new()
                .text("key", action.key.clone())
                .text("observer", o.observer.clone())
                .text("status", o.status.db()),
        )?;
        self.conn.execute(
            "INSERT INTO external_action_observations VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            &[
                Value::Int(to_db(event.0)?),
                Value::Text(&action.key),
                Value::Text(&o.observer),
                Value::Text(o.status.db()),
                Value::Text(&action.digest),
                Value::Text(&detail),
                o.number
                    .map_or(Ok(Value::Null), |n| to_db(n).map(Value::Int))?,
                o.url.as_deref().map_or(Value::Null, Value::Text),
            ],
        )?;
        Ok(())
    }
}

/// How the helper sees an issued action without an observed outcome: still running in
/// this process, or unknown.
pub type InFlight<'a> = &'a dyn Fn(&str) -> bool;

impl ReadView<'_> {
    pub fn external_action(&self, key: &str) -> Result<Option<ExternalAction>, StorageError> {
        load(self.conn, key)
    }

    /// Where a registered repository is checked out.
    pub fn repository_path(
        &self,
        id: RepositoryId,
    ) -> Result<Option<std::path::PathBuf>, StorageError> {
        Ok(self
            .conn
            .query_optional(
                "SELECT path FROM local_repositories WHERE id=?1",
                &[Value::Text(&id_to_text(id.0))],
                |r| r.text(0),
            )?
            .map(std::path::PathBuf::from))
    }

    /// Issued actions whose outcome is not known, oldest first.
    pub fn unresolved_external(&self) -> Result<Vec<String>, StorageError> {
        Ok(self.conn.query(
            "SELECT a.operation_key FROM external_actions a JOIN outbox o ON o.operation_key=a.operation_key
             WHERE o.state='ISSUED_UNKNOWN' ORDER BY a.event",
            &[],
            |r| r.text(0),
        )?)
    }

    fn action_json(
        &self,
        a: &ExternalAction,
        in_flight: InFlight<'_>,
    ) -> Result<Json, StorageError> {
        let c = self.conn;
        let observations = c.query(
            "SELECT observer,status,detail,number,url,event FROM external_action_observations
             WHERE operation_key=?1 ORDER BY event",
            &[Value::Text(&a.key)],
            |r| {
                Ok((
                    r.text(0)?,
                    r.text(1)?,
                    r.text(2)?,
                    r.optional_int(3)?,
                    r.optional_text(4)?,
                    r.int(5)?,
                ))
            },
        )?;
        let decided = c.query_optional(
            "SELECT event FROM external_action_decisions WHERE operation_key=?1",
            &[Value::Text(&a.key)],
            |r| r.int(0),
        )?;
        let state = match a.outbox {
            EffectState::NotIssued if a.decision.as_deref() == Some("DECLINED") => "Declined",
            EffectState::NotIssued => "Requested",
            EffectState::IssuedUnknown if in_flight(&a.key) => "InFlight",
            EffectState::IssuedUnknown => "Unknown",
            EffectState::Confirmed => "Committed",
            EffectState::Rejected => "Rejected",
        };
        let last = observations.last();
        let committed = observations.iter().rev().find(|o| o.1 == "COMMITTED");
        let mut rows = Vec::new();
        for (observer, status, detail, _, _, event) in &observations {
            rows.push(object([
                ("observer", text(observer.clone())),
                ("status", text(status_word(status))),
                ("detail", text(detail.clone())),
                ("at_unix_ms", time_of(c, *event)?),
            ]));
        }
        let (feature, feature_name) =
            feature_fields(c, document_feature(c, &id_to_text(a.plan.0))?)?;
        Ok(object([
            ("action", text(a.key.clone())),
            ("kind", text(a.kind.name())),
            ("plan", identity(a.plan.0)),
            ("revision", number(a.revision.0)),
            ("sequence", number(a.sequence)),
            ("feature", feature),
            ("feature_name", feature_name),
            ("repository", identity(a.repository.0)),
            ("remote", text(a.remote.clone())),
            ("url", text(a.url.clone())),
            ("branch", text(a.branch.clone())),
            ("commit", text(a.commit.clone())),
            ("base", optional(a.base.clone())),
            ("title", optional(a.title.clone())),
            // Only a request carries its body: the frame stays bounded.
            (
                "body",
                optional(a.body.clone().filter(|_| state == "Requested")),
            ),
            ("push", optional(a.push.clone())),
            ("digest", text(a.digest.clone())),
            ("state", text(state)),
            (
                "decision",
                optional(a.decision.as_deref().map(|d| {
                    if d == "AUTHORIZED" {
                        "Authorized"
                    } else {
                        "Declined"
                    }
                    .to_owned()
                })),
            ),
            ("requested_at_unix_ms", time_of(c, a.event)?),
            (
                "decided_at_unix_ms",
                decided.map_or(Ok(Json::Null), |e| time_of(c, e))?,
            ),
            (
                "outcome",
                last.map_or(Json::Null, |o| text(status_word(&o.1))),
            ),
            ("detail", text(last.map_or(String::new(), |o| o.2.clone()))),
            (
                "number",
                committed.and_then(|o| o.3).map_or(Json::Null, Json::Int),
            ),
            (
                "pull_request_url",
                optional(committed.and_then(|o| o.4.clone())),
            ),
            ("observations", Json::List(rows)),
        ]))
    }

    /// M5 (protocol 1.10): the newest runs with what may be requested for each, and the
    /// newest external actions, of one plan or of all.
    pub fn ipc_external_actions(
        &self,
        plan: Option<ArtifactId>,
        in_flight: InFlight<'_>,
    ) -> Result<Json, StorageError> {
        let c = self.conn;
        let filter = plan.map(|p| id_to_text(p.0));
        let filter_value = filter.as_deref().map_or(Value::Null, Value::Text);
        let plans = c.query(
            "SELECT plan FROM plan_runs WHERE ?1 IS NULL OR plan=?1
             GROUP BY plan ORDER BY max(event) DESC LIMIT ?2",
            &[filter_value, Value::Int(RUN_ROWS as i64)],
            |r| r.text(0),
        )?;
        let mut runs = Vec::new();
        for plan in &plans {
            let Some(run) = self.plan_run(ArtifactId(id_from_text(plan)?))? else {
                continue;
            };
            let newest = |kind| -> Result<Json, StorageError> {
                Ok(keys(c, plan, kind)?
                    .into_iter()
                    .next()
                    .map_or(Json::Null, text))
            };
            let push_why = push_refusal(c, &run)?;
            let (_, pr_why) = pull_request_refusal(c, &run)?;
            let (feature, feature_name) = feature_fields(c, document_feature(c, plan)?)?;
            runs.push(object([
                ("plan", identity_text(plan)?),
                ("revision", number(run.revision.0)),
                ("feature", feature),
                ("feature_name", feature_name),
                ("repository", identity(run.repository.0)),
                ("branch", text(run.branch.clone())),
                ("base", text(run.base.clone())),
                ("head", text(run.head.clone())),
                ("state", text(run.state.clone())),
                ("push", newest(ExternalKind::Push)?),
                ("pull_request", newest(ExternalKind::PullRequest)?),
                ("can_push", Json::Bool(push_why.is_none())),
                ("push_refusal", optional(push_why)),
                ("can_open_pull_request", Json::Bool(pr_why.is_none())),
                ("pull_request_refusal", optional(pr_why)),
            ]));
        }
        let keys = c.query(
            "SELECT operation_key FROM external_actions WHERE ?1 IS NULL OR plan=?1
             ORDER BY event DESC LIMIT ?2",
            &[filter_value, Value::Int(EXTERNAL_ROWS as i64)],
            |r| r.text(0),
        )?;
        let mut listed = Vec::new();
        for key in keys {
            if let Some(a) = load(c, &key)? {
                listed.push(self.action_json(&a, in_flight)?);
            }
        }
        Ok(object([
            ("runs", Json::List(runs)),
            ("actions", Json::List(listed)),
        ]))
    }

    /// Requests that wait for the human's decision, newest first (the Approval Inbox).
    pub(crate) fn pending_external(&self) -> Result<Vec<Json>, StorageError> {
        let c = self.conn;
        let keys = c.query(
            "SELECT a.operation_key FROM external_actions a JOIN outbox o ON o.operation_key=a.operation_key
             WHERE o.state='NOT_ISSUED'
               AND NOT EXISTS(SELECT 1 FROM external_action_decisions d WHERE d.operation_key=a.operation_key)
             ORDER BY a.event DESC",
            &[],
            |r| r.text(0),
        )?;
        let mut out = Vec::new();
        for key in keys {
            let Some(a) = load(c, &key)? else { continue };
            let (feature, feature_name) =
                feature_fields(c, document_feature(c, &id_to_text(a.plan.0))?)?;
            out.push(object([
                ("action", text(a.key.clone())),
                ("kind", text(a.kind.name())),
                ("plan", identity(a.plan.0)),
                ("feature", feature),
                ("feature_name", feature_name),
                ("branch", text(a.branch.clone())),
                ("commit", text(a.commit.clone())),
                ("base", optional(a.base.clone())),
                ("title", optional(a.title.clone())),
                ("requested_at_unix_ms", time_of(c, a.event)?),
            ]));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn m5_only_https_without_credentials_or_a_local_path_is_pushed_to() {
        assert_eq!(url_refusal("https://github.com/kaanf/jojo.git"), None);
        assert_eq!(url_refusal("file:///tmp/remote.git"), None);
        assert_eq!(url_refusal("/tmp/remote.git"), None);
        assert!(
            url_refusal("https://kaanf:ghp_x@github.com/kaanf/jojo.git")
                .unwrap()
                .contains("credentials")
        );
        assert!(
            url_refusal("git@github.com:kaanf/jojo.git")
                .unwrap()
                .contains("SSH")
        );
        assert!(url_refusal("ssh://git@github.com/kaanf/jojo.git").is_some());
        assert!(branch_name("main") && branch_name("release/1.0"));
        assert!(
            !branch_name("-f") && !branch_name("a..b") && !branch_name("") && !branch_name("a b")
        );
    }
}

//! P2-02 orchestration primitives for the first vertical slice: one disposable repository,
//! one feature bundle, one approved step, one supervised run.
//!
//! Everything here composes existing authority and adds none. A bundle is imported as
//! drafts through the same artifact and review-document commands a human edit uses, so
//! importing never approves. A start is accepted in one writer transaction that re-reads
//! admission and every approval, then goes through the unchanged worktree, scheduler and
//! launch-issue commands. No function here takes an executable, an argument vector or an
//! environment value: those come only from the adapter configured at helper start.
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use jojo_domain::attempt::AttemptBindings;
use jojo_domain::dependency::{
    ApprovalBinding, ExecutionPlan, Graph, GraphKind, NodeRef, Source, SourceRevision,
    SourceVector, StepInputs,
};
use jojo_domain::identity::{
    ArtifactId, AttemptId, FeatureId, OpaqueId, PlanId, RepositoryId, Revision, StepId, WorktreeId,
};
use jojo_domain::provider::{
    LaunchSpec, MAX_PROMPT_BYTES, PermissionEnvelope, ProviderFingerprint, ProviderKind, Role,
    SourceBinding,
};
use jojo_domain::scheduler::{ResourceMode, StepResources};
use jojo_domain::validation::{CANDIDATE_OUTPUT, ValidationPlan};

use crate::model::{id_from_text, id_to_text};
use crate::sqlite::Value;
use crate::{
    Admission, Author, Engine, LocalRepository, PreparedPrd, Provenance, ReadView,
    SettlementContext, StorageError, Tx, TxError, WorktreeRoot, inspect_worktree,
    materialize_worktree, sha256_hex,
};

/// What the plan's permission digest covers. The Codex adapter enforces the same envelope.
pub const SLICE_PERMISSION: &str = "jojo.slice.permission/1: one developer run; owned worktree only; \
     sandbox workspace-write; no agent tool network, hooks or background work";
/// Environment identity of slice step inputs.
pub const SLICE_ENVIRONMENT: &str = "jojo.slice.environment/1";
pub const SLICE_TIMEOUT_MILLIS: u64 = 30 * 60 * 1000;
pub const SLICE_MAX_OUTPUT_BYTES: u64 = 64 * 1024 * 1024;
pub const SLICE_MAX_FRAME_BYTES: u64 = 1024 * 1024;
const MAX_ADR_BYTES: usize = 32 * 1024;

fn deny(s: impl Into<String>) -> TxError {
    TxError::Rejected(s.into())
}

/// The configured root under which a repository may be registered. A private directory
/// created for this purpose; never a home, project or system directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisposableRoot {
    path: PathBuf,
}

impl DisposableRoot {
    pub fn inspect(path: &Path) -> Result<Self, String> {
        let real = std::fs::canonicalize(path).map_err(|e| format!("disposable root: {e}"))?;
        let listed = std::fs::symlink_metadata(&real).map_err(|e| e.to_string())?;
        if !path.is_absolute()
            || !listed.is_dir()
            || listed.permissions().mode() & 0o077 != 0
            || real.components().count() < 4
        {
            return Err(
                "disposable root must be an existing private directory (0700), at least three levels deep"
                    .into(),
            );
        }
        Ok(Self { path: real })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Inspects one explicitly named repository. Both its working directory and its Git
    /// common directory must resolve to somewhere strictly inside this root.
    pub fn repository(&self, path: &Path) -> Result<LocalRepository, String> {
        if !path.is_absolute() {
            return Err("RepositoryOutsideDisposableRoot: absolute path required".into());
        }
        let real = std::fs::canonicalize(path).map_err(|e| format!("repository: {e}"))?;
        let inside = |p: &Path| p != self.path && p.starts_with(&self.path);
        if !inside(&real) {
            return Err("RepositoryOutsideDisposableRoot".into());
        }
        let repository = LocalRepository::inspect(&real)?;
        if !inside(repository.path()) || !inside(repository.common()) {
            return Err("RepositoryOutsideDisposableRoot".into());
        }
        Ok(repository)
    }
}

/// The helper's owned worktree root: created private if absent, never adopted otherwise.
pub fn prepare_worktree_root(path: &Path) -> Result<WorktreeRoot, String> {
    if std::fs::symlink_metadata(path).is_err() {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(|e| format!("worktree root: {e}"))?;
    }
    WorktreeRoot::inspect(path)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SliceSubjects {
    pub prd: ArtifactId,
    pub adr: ArtifactId,
    pub plan: ArtifactId,
    pub validation: ArtifactId,
}

impl SliceSubjects {
    fn all(&self) -> [(&'static str, ArtifactId); 4] {
        [
            ("PRD", self.prd),
            ("ADR", self.adr),
            ("plan", self.plan),
            ("validation plan", self.validation),
        ]
    }
}

/// One feature bundle: a PRD, an ADR, a one-step execution plan and its validation plan.
#[derive(Clone, Debug)]
pub struct FeatureBundle {
    feature: FeatureId,
    subjects: SliceSubjects,
    prd: PreparedPrd,
    adr: String,
    step: NodeRef,
    repository: RepositoryId,
    base: String,
    provider: ProviderKind,
    validation: ValidationPlan,
}

impl FeatureBundle {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        feature: FeatureId,
        subjects: SliceSubjects,
        prd: PreparedPrd,
        adr: String,
        step: OpaqueId,
        repository: RepositoryId,
        base: String,
        provider: ProviderKind,
        validation: ValidationPlan,
    ) -> Result<Self, String> {
        let step = NodeRef {
            id: step,
            revision: Revision(1),
        };
        let ids = subjects.all().map(|(_, id)| id.0);
        if (0..4).any(|i| ids[i + 1..].contains(&ids[i])) || ids.contains(&feature.0) {
            return Err("bundle subject identities must be distinct".into());
        }
        if adr.trim().is_empty() || adr.len() > MAX_ADR_BYTES || adr.contains('\0') {
            return Err("ADR text must be non-empty and bounded".into());
        }
        if !matches!(base.len(), 40 | 64) || !base.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("exact base commit required".into());
        }
        validation.validate()?;
        if validation.steps.len() != 1 || validation.steps[0].step != step {
            return Err("validation plan must cover exactly the one plan step".into());
        }
        Ok(Self {
            feature,
            subjects,
            prd,
            adr,
            step,
            repository,
            base,
            provider,
            validation,
        })
    }

    fn plan(&self, approvals: Vec<ApprovalBinding>) -> ExecutionPlan {
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
                nodes: vec![self.step],
                edges: vec![],
            },
            approvals,
            inputs: vec![StepInputs {
                step: self.step,
                environment: Some(SLICE_ENVIRONMENT.into()),
                selected_outputs: vec![],
                base_sources: vec![Source {
                    repository: self.repository,
                    revision: SourceRevision::Commit(self.base.clone()),
                }],
            }],
            permission_digest: sha256_hex(&[SLICE_PERMISSION.as_bytes()]),
            validation_digest: sha256_hex(&[self.validation.canonical().as_bytes()]),
            required_outputs: vec![CANDIDATE_OUTPUT.into()],
            resources: vec![StepResources {
                step: self.step,
                repository: self.repository,
                provider: self.provider,
                priority: 0,
                additional: vec![],
                repository_mode: ResourceMode::Shared,
            }],
        }
    }

    /// The text a reviewer is shown for the plan; the canonical graph is the bound body.
    fn plan_text(&self) -> String {
        format!(
            "Execution plan: one step\nstep: {}\nprovider: {:?}\nrepository: {}\nbase commit: {}\n\
             permission: {SLICE_PERMISSION}\nvalidation plan: {}\nrequired output: {CANDIDATE_OUTPUT}\n",
            id_to_text(self.step.id),
            self.provider,
            id_to_text(self.repository.0),
            self.base,
            id_to_text(self.subjects.validation.0),
        )
    }
}

fn document(tx: &Tx<'_>, id: ArtifactId) -> Result<Option<(String, String)>, TxError> {
    Ok(tx.conn.query_optional(
        "SELECT kind,content FROM review_documents WHERE id=?1 ORDER BY revision DESC LIMIT 1",
        &[Value::Text(&id_to_text(id.0))],
        |r| Ok((r.text(0)?, r.text(1)?)),
    )?)
}

impl Tx<'_> {
    /// Advances one bundle as far as existing approvals allow and creates drafts only.
    /// The core requires an approved PRD before an ADR and both before a plan, so the same
    /// bundle is sent again after each human decision. Content that differs from what was
    /// already imported is refused; nothing is ever revised or approved here.
    ///
    /// Returns `stage=<PRD|ADR|PLAN|COMPLETE> awaiting=<subject ids needing approval>`.
    pub fn import_feature_bundle(&mut self, bundle: &FeatureBundle) -> Result<String, TxError> {
        let s = bundle.subjects;
        let awaiting = |stage: &str, ids: &[ArtifactId]| {
            let ids: Vec<_> = ids.iter().map(|i| id_to_text(i.0)).collect();
            format!("stage={stage} awaiting={}", ids.join(","))
        };
        if self
            .conn
            .query_optional(
                "SELECT 1 FROM local_repositories WHERE id=?1",
                &[Value::Text(&id_to_text(bundle.repository.0))],
                |r| r.int(0),
            )?
            .is_none()
        {
            return Err(deny("bundle repository is not registered"));
        }
        let imported = self.conn.query_optional(
            "SELECT content_digest,feature_id FROM artifact_versions WHERE artifact_id=?1
             ORDER BY revision DESC LIMIT 1",
            &[Value::Text(&id_to_text(s.prd.0))],
            |r| Ok((r.text(0)?, r.text(1)?)),
        )?;
        match imported {
            None => {
                self.create_artifact(
                    s.prd,
                    bundle.feature,
                    &bundle.prd,
                    &Provenance::new(Author::Human, "imported feature bundle (unapproved draft)"),
                )?;
                return Ok(awaiting("PRD", &[s.prd]));
            }
            Some((digest, feature)) => {
                if digest != bundle.prd.content_digest() || feature != id_to_text(bundle.feature.0)
                {
                    return Err(deny("bundle differs from the imported PRD"));
                }
            }
        }
        let Some(prd) = self.current_artifact_approval(s.prd)? else {
            return Ok(awaiting("PRD", &[s.prd]));
        };
        match document(self, s.adr)? {
            None => {
                self.put_review_document(s.adr, None, s.prd, None, &bundle.adr, &bundle.adr)?;
                return Ok(awaiting("ADR", &[s.adr]));
            }
            Some((kind, body)) if kind != "ADR" || body != bundle.adr => {
                return Err(deny("bundle differs from the imported ADR"));
            }
            Some(_) => {}
        }
        let Some(adr) = self.current_artifact_approval(s.adr)? else {
            return Ok(awaiting("ADR", &[s.adr]));
        };
        let validation = bundle.validation.canonical();
        match document(self, s.validation)? {
            None => {
                self.put_review_document(
                    s.validation,
                    None,
                    s.prd,
                    Some(s.adr),
                    &validation,
                    &validation,
                )?;
            }
            Some((_, body)) if body != validation => {
                return Err(deny("bundle differs from the imported validation plan"));
            }
            Some(_) => {}
        }
        if self.has_execution_plan(s.plan)? {
            let stored = self
                .conn
                .query_one(
                    "SELECT canonical FROM execution_plans WHERE plan=?1 ORDER BY revision DESC LIMIT 1",
                    &[Value::Text(&id_to_text(s.plan.0))],
                    |r| r.text(0),
                )
                .map_err(StorageError::from)?;
            let stored = ExecutionPlan::import(stored.as_bytes()).map_err(deny)?;
            if stored != bundle.plan(stored.approvals.clone()) {
                return Err(deny("bundle differs from the imported execution plan"));
            }
        } else {
            let plan = bundle.plan(vec![prd, adr]);
            self.put_execution_plan(s.plan, None, s.prd, s.adr, &plan, &bundle.plan_text())?;
        }
        let mut pending = Vec::new();
        for id in [s.validation, s.plan] {
            if self.current_artifact_approval(id)?.is_none() {
                pending.push(id);
            }
        }
        Ok(if pending.is_empty() {
            "stage=COMPLETE awaiting=".into()
        } else {
            awaiting("PLAN", &pending)
        })
    }
}

/// One approved step as the core currently records it. Read-only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SliceStep {
    pub plan_revision: Revision,
    pub provider: ProviderKind,
    pub repository: RepositoryId,
    pub repository_path: PathBuf,
    pub base: String,
}

fn step_of(
    plan: &ExecutionPlan,
    step: NodeRef,
) -> Result<(ProviderKind, RepositoryId, String, SourceVector), String> {
    let resources = plan
        .resources
        .iter()
        .find(|r| r.step == step)
        .ok_or("step is not part of the approved plan")?;
    let inputs = plan
        .inputs
        .iter()
        .find(|i| i.step == step)
        .ok_or("step has no approved inputs")?;
    let [
        Source {
            repository,
            revision: SourceRevision::Commit(base),
        },
    ] = inputs.base_sources.as_slice()
    else {
        return Err("slice step needs exactly one base commit".into());
    };
    if plan.schema != 2 || *repository != resources.repository {
        return Err("slice step repository/resource mismatch".into());
    }
    Ok((
        resources.provider,
        *repository,
        base.clone(),
        SourceVector {
            sources: inputs.base_sources.clone(),
            environment: inputs.environment.clone(),
        },
    ))
}

impl ReadView<'_> {
    /// The approved plan step a Start names, or the reason it cannot be started.
    pub fn slice_step(
        &self,
        subjects: &SliceSubjects,
        step: NodeRef,
    ) -> Result<Result<SliceStep, String>, StorageError> {
        for (name, id) in subjects.all() {
            if self.current_artifact_approval(id)?.is_none() {
                return Ok(Err(format!("missing or stale approval: {name}")));
            }
        }
        let Some(approval) = self.current_artifact_approval(subjects.plan)? else {
            return Ok(Err("missing or stale approval: plan".into()));
        };
        let Some(stored) = self.execution_plan(subjects.plan, approval.revision)? else {
            return Ok(Err("approved subject is not an execution plan".into()));
        };
        let (provider, repository, base, _) = match step_of(&stored.plan, step) {
            Ok(found) => found,
            Err(why) => return Ok(Err(why)),
        };
        let path = self.conn.query_optional(
            "SELECT path FROM local_repositories WHERE id=?1",
            &[Value::Text(&id_to_text(repository.0))],
            |r| r.text(0),
        )?;
        Ok(match path {
            Some(path) => Ok(SliceStep {
                plan_revision: approval.revision,
                provider,
                repository,
                repository_path: path.into(),
                base,
            }),
            None => Err("plan repository is not registered".into()),
        })
    }
}

#[derive(Clone, Debug)]
pub struct StartRequest {
    /// The Start command identity: one command, one attempt, at most one launch.
    pub attempt: OpaqueId,
    pub subjects: SliceSubjects,
    pub step: NodeRef,
}

impl Tx<'_> {
    /// The durable acceptance of one human Start. Re-reads admission, capacity and every
    /// approval inside the writer transaction, queues the step if this is its first
    /// start, and takes the worktree ticket. Nothing is launched here.
    pub fn accept_start(
        &mut self,
        request: &StartRequest,
        repository: &LocalRepository,
        root: &WorktreeRoot,
    ) -> Result<String, TxError> {
        if self.admission()? != Admission::Open {
            return Err(deny("AdmissionClosed"));
        }
        self.invalidate_approvals()?;
        let s = request.subjects;
        for (name, id) in s.all() {
            if self.current_artifact_approval(id)?.is_none() {
                return Err(deny(format!("missing or stale approval: {name}")));
            }
        }
        let approval = self
            .current_artifact_approval(s.plan)?
            .ok_or_else(|| deny("missing or stale approval: plan"))?;
        let stored = crate::dependencies::load_plan(self.conn, s.plan, approval.revision)?
            .ok_or_else(|| deny("approved subject is not an execution plan"))?;
        let (_, planned, base, sources) = step_of(&stored.plan, request.step).map_err(deny)?;
        if planned != repository.id() {
            return Err(deny("repository differs from the approved plan"));
        }
        let validation = document(self, s.validation)?
            .ok_or_else(|| deny("validation plan missing"))?
            .1;
        let parsed = ValidationPlan::import(validation.as_bytes()).map_err(deny)?;
        if sha256_hex(&[validation.as_bytes()]) != stored.plan.validation_digest
            || parsed.step(request.step).is_none()
        {
            return Err(deny("validation plan is not the one the plan binds"));
        }
        // Capacity one, stated directly: no other attempt may be reserved, running,
        // stopping or held for recovery. The scheduler enforces its own limits as well.
        let active = self
            .conn
            .query_one(
                "SELECT COUNT(*) FROM approval_attempts WHERE phase<>'FINISHED'",
                &[],
                |r| r.int(0),
            )
            .map_err(StorageError::from)?;
        if active != 0 {
            return Err(deny("CapacityOne: another attempt is active or held"));
        }
        let step_id = id_to_text(request.step.id);
        let revision = Value::Int(crate::model::to_db(request.step.revision.0)?);
        if self
            .conn
            .query_optional(
                "SELECT 1 FROM scheduler_attempts WHERE step=?1 AND step_revision=?2 LIMIT 1",
                &[Value::Text(&step_id), revision],
                |r| r.int(0),
            )?
            .is_some()
        {
            return Err(deny("DuplicateStart: this step was already started"));
        }
        let revision = Value::Int(crate::model::to_db(request.step.revision.0)?);
        if self
            .conn
            .query_optional(
                "SELECT 1 FROM scheduler_queue WHERE step=?1 AND step_revision=?2",
                &[Value::Text(&step_id), revision],
                |r| r.int(0),
            )?
            .is_none()
        {
            self.queue_step(
                s.prd,
                s.adr,
                s.plan,
                approval.revision,
                request.step,
                &sources,
            )?;
        }
        let state = self.request_worktree(request.attempt, repository, root, &base, None)?;
        if state != "CREATING" {
            return Err(deny(format!(
                "worktree administration unavailable ({state})"
            )));
        }
        Ok(format!("accepted attempt={}", id_to_text(request.attempt)))
    }
}

/// After acceptance: create and seal the owned worktree, then reserve the scheduler
/// envelope and bind the worktree in one admission scan. Filesystem work is outside any
/// transaction; each durable step is its own command, as in Phase 1.
pub fn provision_and_reserve(
    ctx: &SettlementContext<'_>,
    request: &StartRequest,
) -> Result<(), String> {
    let attempt = request.attempt;
    let proof = match materialize_worktree(ctx.engine, attempt, None) {
        Ok(proof) => proof,
        Err(why) => {
            let reason = why.clone();
            let _ = ctx.cmd("BlockWorktree", attempt, move |tx| {
                tx.block_worktree(attempt, &reason)?;
                Ok("blocked".into())
            });
            return Err(format!("worktree: {why}"));
        }
    };
    ctx.cmd("SealWorktree", attempt, move |tx| {
        tx.seal_worktree(proof)?;
        Ok("sealed".into())
    })?;
    let inspected = inspect_worktree(ctx.engine, attempt)?;
    let config = ctx.read(|v| v.resource_config())?.revision;
    let step = request.step;
    let admitted = ctx.cmd("ScheduleStart", attempt, move |tx| {
        let decisions = tx.schedule_owned_worktrees(config, vec![inspected])?;
        Ok(decisions.iter().find(|d| d.step == step).map_or(
            "no decision for this step".to_owned(),
            |d| {
                if d.attempt == Some(attempt) {
                    "ADMITTED".into()
                } else {
                    d.reason.clone()
                }
            },
        ))
    })?;
    if admitted != "ADMITTED" {
        return Err(format!("NotAdmitted: {admitted}"));
    }
    Ok(())
}

/// A reservation that will not be launched is retired through the existing pre-issue
/// path; it never touches a launch that may have been issued.
pub fn retire_unlaunched(ctx: &SettlementContext<'_>, attempt: OpaqueId) -> Result<(), String> {
    ctx.cmd("RetireUnlaunched", attempt, move |tx| {
        tx.retire_unissued_reservation(attempt)?;
        Ok("retired".into())
    })
    .map(drop)
}

/// The frozen spec for a reserved, bound attempt. Every field is read from durable state;
/// the prompt is composed only from the approved PRD, ADR and validation-plan bytes.
pub fn slice_launch_spec(
    engine: &Engine,
    request: &StartRequest,
    fingerprint: &ProviderFingerprint,
    variables: Vec<String>,
) -> Result<LaunchSpec, String> {
    let s = request.subjects;
    let mut reader = engine.reader().map_err(|e| e.to_string())?;
    let found = reader
        .read(|v| {
            let record = v.worktree(request.attempt)?;
            let vector = v.attempt_source_vector(request.attempt)?;
            let mut decisions = Vec::new();
            for id in [s.prd, s.adr, s.plan] {
                decisions.push(v.applicable_decision(id)?);
            }
            let approval = v.current_artifact_approval(s.plan)?;
            let plan = match &approval {
                Some(a) => v.execution_plan(s.plan, a.revision)?,
                None => None,
            };
            let prd = v.artifact_version(s.prd, None)?;
            let body = |id: ArtifactId| {
                v.conn.query_optional(
                    "SELECT content FROM review_documents WHERE id=?1 ORDER BY revision DESC LIMIT 1",
                    &[Value::Text(&id_to_text(id.0))],
                    |r| r.text(0),
                )
            };
            Ok((record, vector, decisions, plan, prd, body(s.adr)?, body(s.validation)?))
        })
        .map_err(|e| e.to_string())?;
    let (Some(record), Some(vector), decisions, Some(plan), Some(prd), Some(adr), Some(validation)) =
        found
    else {
        return Err("reserved attempt state is incomplete".into());
    };
    let approvals: Vec<OpaqueId> = decisions
        .into_iter()
        .map(|d| d.map(|seq| OpaqueId(u128::from(seq.0))))
        .collect::<Option<_>>()
        .ok_or("missing or stale approval")?;
    let scope = ValidationPlan::import(validation.as_bytes())?;
    let scope = scope
        .step(request.step)
        .ok_or("validation plan does not cover the step")?;
    let list = |paths: &[jojo_domain::validation::ScopePath]| {
        paths
            .iter()
            .map(|p| format!("- {}\n", p.as_str()))
            .collect::<String>()
    };
    let prompt = format!(
        "# Task\nImplement the approved change described below in this repository working \
         directory.\n\n# Product requirements (approved revision {})\n{}\n\n# Architecture \
         decision (approved)\n{}\n\n# Scope\nYou may modify only:\n{}Do not modify:\n{}",
        prd.revision.0,
        prd.rendered,
        adr,
        list(&scope.write_scope),
        list(&scope.baseline),
    )
    .into_bytes();
    if prompt.len() > MAX_PROMPT_BYTES {
        return Err("approved documents exceed the prompt limit".into());
    }
    let digest = sha256_hex(&[&prompt]);
    Ok(LaunchSpec {
        attempt: AttemptId(request.attempt),
        bindings: AttemptBindings {
            feature_id: prd.feature,
            plan_id: PlanId(s.plan.0),
            plan_revision: plan.revision,
            step_id: StepId(request.step.id),
            step_revision: request.step.revision,
            repository_id: record.repository,
            worktree_id: WorktreeId(request.attempt),
        },
        engine_epoch: OpaqueId(u128::from(engine.epoch().0)),
        dispatch_generation: 1,
        fingerprint: fingerprint.clone(),
        source: SourceBinding {
            common_dir: record.common_dir().to_owned(),
            base_commit: record.base.clone(),
            candidate_source_vector: sha256_hex(&[vector.canonical().as_bytes()]),
            instruction_digest: digest.clone(),
        },
        prompt_digest: digest,
        prompt,
        continuation: None,
        role: Role::Developer,
        permissions: PermissionEnvelope {
            approved_digest: plan.plan.permission_digest.clone(),
            tools: vec![],
            filesystem_roots: vec![record.path.clone()],
            network: false,
            hooks: false,
            background: false,
        },
        cwd: record.path,
        environment_allowlist: variables,
        config_references: vec![],
        timeout_millis: SLICE_TIMEOUT_MILLIS,
        max_output_bytes: SLICE_MAX_OUTPUT_BYTES,
        max_frame_bytes: SLICE_MAX_FRAME_BYTES,
        raw_output_dir: record.output,
        approval_ids: [approvals[0], approvals[1], approvals[2]],
        required_outputs: plan.plan.required_outputs.clone(),
        validation_plan: validation,
    })
}

/// `RepositoryId` from its protocol text, for callers outside this crate.
pub fn repository_id(text: &str) -> Result<RepositoryId, String> {
    id_from_text(text)
        .map(|id| RepositoryId(OpaqueId(id.0)))
        .map_err(|e| e.to_string())
}

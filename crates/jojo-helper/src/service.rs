//! P1-13 helper runtime. Workers retain P1-09 lifetime handles; IPC never signals a PID.
//!
//! P2-02 (protocol 1.1) adds repository registration, feature-bundle import, Start and
//! recovery resolution. Each is a thin mapping onto existing core commands. A Start
//! carries subject identities only: the executable, its arguments and its environment
//! come from the Codex adapter configured when the helper was started, never from IPC.
use crate::SystemClock;
use jojo_domain::codex::CODEX_HOME_VARIABLE;
use jojo_domain::dependency::NodeRef;
use jojo_domain::identity::{ArtifactId, FeatureId, RepositoryId, Revision};
use jojo_domain::identity::{IdGenerationError, IdGenerator, OpaqueId};
use jojo_domain::ipc::*;
use jojo_domain::json::Json;
use jojo_domain::paths::LocalRoots;
use jojo_domain::provider::LaunchSpec;
use jojo_domain::provider::{AuthMode, ProviderAdapter, ProviderKind};
use jojo_domain::recovery::RecoveryChoice;
use jojo_domain::shutdown::{CloseState, ShutdownOutcome};
use jojo_domain::validation::ValidationPlan;
use jojo_storage::*;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Epoch-scoped internal identities occupy the high half; client IDs use the low half.
/// Unique counter, not wall time, supplies all runtime IDs within the singleton epoch.
pub struct RuntimeIds {
    epoch: u64,
    next: AtomicU64,
}
impl RuntimeIds {
    pub fn new(epoch: u64) -> Self {
        Self {
            epoch,
            next: AtomicU64::new(1),
        }
    }
}
impl IdGenerator for RuntimeIds {
    fn next_id(&self) -> Result<OpaqueId, IdGenerationError> {
        let n = self
            .next
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1))
            .map_err(|_| IdGenerationError::Exhausted)?;
        Ok(OpaqueId(
            (1u128 << 127) | (u128::from(self.epoch) << 64) | u128::from(n),
        ))
    }
}

/// One helper owns many runners; each runner settles through P1-10 after conclusion.
/// A worker's panic is retained as a failure, never treated as proof of absence.
pub struct WorkerRuntime {
    pub engine: Arc<Engine>,
    pub ids: Arc<RuntimeIds>,
    pub clock: Arc<SystemClock>,
}

pub struct Workers {
    handles: Vec<JoinHandle<Result<(), String>>>,
    failed: bool,
    emergency_stop: Arc<AtomicBool>,
}
impl Default for Workers {
    fn default() -> Self {
        Self::new()
    }
}
impl Workers {
    pub fn new() -> Self {
        Self {
            handles: Vec::new(),
            failed: false,
            emergency_stop: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn launch(
        &mut self,
        runtime: WorkerRuntime,
        spec: LaunchSpec,
        launch: ProcessLaunch,
        plan: StopPlan,
        mut hooks: LaunchHooks,
    ) -> Result<(), String> {
        hooks.emergency_stop = Some(self.emergency_stop.clone());
        let WorkerRuntime { engine, ids, clock } = runtime;
        let handle = std::thread::Builder::new()
            .name("jojo-worker".into())
            .spawn(move || {
                let ctx = LaunchContext {
                    engine: &engine,
                    ids: ids.as_ref(),
                    clock: clock.as_ref(),
                    poll: Duration::from_millis(10),
                };
                run_launch(&ctx, &spec, &launch, &plan, hooks).map_err(|e| format!("{e:?}"))?;
                let store = BlobStore::for_engine(&engine)?;
                settle_attempt(
                    &SettlementContext {
                        engine: &engine,
                        ids: ids.as_ref(),
                        store: &store,
                    },
                    spec.attempt.0,
                    BlobFaults::default(),
                )?;
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        self.handles.push(handle);
        Ok(())
    }
    /// P2-03: a host validation run. It is not a provider attempt, but it is work this
    /// helper owns: capacity, close and drop wait for it exactly as for a launch.
    pub fn validate(
        &mut self,
        work: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) -> Result<(), String> {
        let handle = std::thread::Builder::new()
            .name("jojo-validator".into())
            .spawn(work)
            .map_err(|e| e.to_string())?;
        self.handles.push(handle);
        Ok(())
    }
    pub fn reap(&mut self) -> bool {
        let mut i = 0;
        while i < self.handles.len() {
            if self.handles[i].is_finished() {
                let h = self.handles.swap_remove(i);
                if !matches!(h.join(), Ok(Ok(()))) {
                    self.failed = true;
                }
            } else {
                i += 1;
            }
        }
        self.handles.is_empty()
    }
    pub fn failed(&self) -> bool {
        self.failed
    }
}

/// Fixed at helper start by its launcher. No protocol command can change it.
#[derive(Clone, Debug)]
pub struct SliceConfig {
    pub disposable_root: PathBuf,
    pub worktree_root: PathBuf,
    pub codex: Option<CodexAdapterConfig>,
}
impl SliceConfig {
    pub fn new(roots: &LocalRoots, options: crate::SliceOptions) -> Self {
        Self {
            disposable_root: options.disposable_root,
            worktree_root: roots.worktrees().to_path_buf(),
            codex: options.codex.map(|c| CodexAdapterConfig {
                executable: c.executable,
                version: c.version,
                sha256: c.sha256,
                home: c.home,
                // The launcher's flag is the human's attestation; nothing is inspected.
                auth: AuthMode::NativeSubscription,
            }),
        }
    }
}
#[derive(Default)]
pub struct ServiceConfig {
    pub slice: Option<SliceConfig>,
    /// Test injection of the process inspector (the verifier's unprivileged `ps`).
    pub inspector: Option<PathBuf>,
    /// Test barriers and faults for launches started by `Start`. Production: `None`.
    pub launch_hooks: Option<Box<dyn FnMut() -> LaunchHooks + Send + Sync>>,
}
impl ServiceConfig {
    pub fn slice(slice: SliceConfig) -> Self {
        Self {
            slice: Some(slice),
            ..Self::default()
        }
    }
}

pub struct Service {
    pub engine: Arc<Engine>,
    pub ids: Arc<RuntimeIds>,
    pub clock: Arc<SystemClock>,
    pub workers: Workers,
    store: BlobStore,
    session: Option<OpaqueId>,
    compatible: bool,
    finalized: bool,
    recovery: RecoveryInspection,
    config: ServiceConfig,
    /// Budget of the host validation command. Fixed in production; tests shorten it.
    pub validation_budget: ValidationBudget,
    /// P2-04.1: this helper's own executable, started as the guard of every agent and
    /// validation process group so that neither can outlive this process. Set by the
    /// helper executable before it serves; `None` only in in-process tests.
    pub guard: Option<PathBuf>,
    /// Stop flag of the latest validation run, set on close, emergency and drop.
    validation_stop: Arc<AtomicBool>,
}
impl Service {
    pub fn new(engine: Arc<Engine>) -> Result<Self, String> {
        Self::with_config(engine, ServiceConfig::default())
    }
    pub fn with_config(engine: Arc<Engine>, config: ServiceConfig) -> Result<Self, String> {
        let ids = Arc::new(RuntimeIds::new(engine.epoch().0));
        let store = BlobStore::for_engine(&engine)?;
        let inspector = config
            .inspector
            .clone()
            .map_or_else(OsProcessInspector::default, |ps| OsProcessInspector { ps });
        // Inspection never opens admission or adopts/signals old processes.
        let recovery = inspect_recovery(&engine, &inspector, &|_| None)?;
        Ok(Self {
            engine,
            ids,
            clock: Arc::new(SystemClock::new()),
            workers: Workers::new(),
            store,
            session: None,
            compatible: false,
            finalized: false,
            recovery,
            config,
            validation_budget: ValidationBudget::DEFAULT,
            guard: None,
            validation_stop: Arc::new(AtomicBool::new(false)),
        })
    }
    /// A fresh read-only inspection. Resolution and the reviewed start bind its digest.
    fn reinspect(&mut self) -> Result<(), String> {
        let inspector = self
            .config
            .inspector
            .clone()
            .map_or_else(OsProcessInspector::default, |ps| OsProcessInspector { ps });
        self.recovery = inspect_recovery(&self.engine, &inspector, &|_| None)?;
        Ok(())
    }
    pub fn launch(
        &mut self,
        spec: LaunchSpec,
        launch: ProcessLaunch,
        plan: StopPlan,
        hooks: LaunchHooks,
    ) -> Result<(), String> {
        self.workers.launch(
            WorkerRuntime {
                engine: self.engine.clone(),
                ids: self.ids.clone(),
                clock: self.clock.clone(),
            },
            spec,
            launch,
            plan,
            hooks,
        )
    }
    fn ctx(&self) -> SettlementContext<'_> {
        SettlementContext {
            engine: &self.engine,
            ids: self.ids.as_ref(),
            store: &self.store,
        }
    }
    pub fn emergency(&mut self) -> Result<(), String> {
        self.validation_stop.store(true, Ordering::SeqCst);
        self.finalized = false;
        let result = emergency_stop(&self.ctx()).map(|_| ());
        if result.is_err() {
            self.workers.emergency_stop.store(true, Ordering::SeqCst);
        }
        result
    }
    pub fn status(&self) -> Result<CloseStatus, String> {
        close_status(&self.ctx())
    }
    /// All dispatch is on the control thread. Queries are bounded; worker output never
    /// enters this queue. A receipt acknowledges only the committed command.
    pub fn request(&mut self, e: &Envelope) -> Envelope {
        let result = self.handle(e);
        let result = result.and_then(|p| {
            if encode(&p).len() > MAX_FRAME - 1024 {
                Err("PayloadTooLarge: use bounded queries".into())
            } else {
                Ok(p)
            }
        });
        let (kind, payload) = match result {
            Ok(p) => ("Response", p),
            Err(s) => ("Error", object([("code", text(s))])),
        };
        Envelope {
            major: MAJOR as u64,
            // A 1.0 client keeps seeing 1.0 frames.
            minor: e.minor.min(MINOR as u64),
            session: e.session,
            epoch: self.engine.epoch().0,
            request: e.request,
            revision: None,
            kind: kind.into(),
            payload,
        }
    }
    fn handle(&mut self, e: &Envelope) -> Result<Json, String> {
        if e.kind == "Hello" {
            if self.session.is_some_and(|s| s != e.session) {
                return Err("SessionMismatch".into());
            }
            self.session = Some(e.session);
            self.compatible = e.major == MAJOR as u64 && e.minor <= MINOR as u64;
            return Ok(object([
                ("compatible", Json::Bool(self.compatible)),
                ("epoch", number(self.engine.epoch().0)),
                ("max_frame", number(MAX_FRAME as u64)),
                ("max_page", number(8)),
                ("admission", text("closed until reviewed recovery start")),
                (
                    "commands",
                    text(if e.minor >= 1 {
                        "PrepareClose CancelClose ConfirmClose CancelAttempt HumanDecision RegisterRepository ImportBundle Start RecoveryResolve RecoveryStart Validate CandidateDecision"
                    } else {
                        "PrepareClose CancelClose ConfirmClose CancelAttempt HumanDecision"
                    }),
                ),
                (
                    "queries",
                    text(if e.minor >= 1 {
                        "Snapshot Events EventDetail Log Receipt CloseStatus ReviewBundle ShutdownReceipt Recovery RecoveryInspect Candidate CandidateChanges CandidateDiff ValidationReport"
                    } else {
                        "Snapshot Events EventDetail Log Receipt CloseStatus ReviewBundle ShutdownReceipt Recovery"
                    }),
                ),
                (
                    "slice",
                    Json::Bool(e.minor >= 1 && self.config.slice.is_some()),
                ),
            ]));
        }
        if self.session != Some(e.session) {
            return Err("HandshakeRequired".into());
        }
        if e.epoch != self.engine.epoch().0 {
            return Err("EpochMismatch".into());
        }
        let p = &e.payload;
        let mut reader = self.engine.reader().map_err(|e| e.to_string())?;
        match e.kind.as_str() {
            "Snapshot" => {
                let section = string(get(p, "section")?)?;
                let after = string(get(p, "after")?)?;
                let cursor = match get(p, "cursor")? {
                    Json::Null => None,
                    v => Some(uint(v)?),
                };
                let limit = u32::try_from(uint(get(p, "limit")?)?).map_err(|_| "Limit")?;
                reader
                    .read(|v| v.ipc_snapshot(section, after, cursor, limit))
                    .map_err(|e| e.to_string())
            }
            "Events" => {
                let after = uint(get(p, "after")?)?;
                let limit = u32::try_from(uint(get(p, "limit")?)?).map_err(|_| "Limit")?;
                reader
                    .read(|v| v.ipc_events(after, limit))
                    .map_err(|e| e.to_string())
            }
            "EventDetail" => {
                let seq = uint(get(p, "seq")?)?;
                let offset = uint(get(p, "offset")?)?;
                reader
                    .read(|v| v.ipc_event_detail(seq, offset))
                    .map_err(|e| e.to_string())
            }
            "Log" => {
                let a = id(get(p, "attempt")?)?;
                let seq = uint(get(p, "sequence")?)?;
                let offset = uint(get(p, "offset")?)?;
                reader
                    .read(|v| v.ipc_log(a, seq, offset))
                    .map_err(|e| e.to_string())
            }
            "Receipt" => {
                let id = CommandId(id(get(p, "command")?)?);
                reader
                    .receipt(id)
                    .map(|r| r.map_or(Json::Null, receipt))
                    .map_err(|e| e.to_string())
            }
            "ShutdownReceipt" => {
                let ticket = id(get(p, "ticket")?)?;
                reader
                    .read(|v| v.shutdown_receipt(ticket))
                    .map(|r| r.as_ref().map_or(Json::Null, shutdown_json))
                    .map_err(|e| e.to_string())
            }
            "Recovery" => {
                let offset = usize::try_from(uint(get(p, "offset")?)?).map_err(|_| "Offset")?;
                let limit = uint(get(p, "limit")?)?;
                if limit == 0 || limit > 8 {
                    return Err("Limit".into());
                }
                Ok(object([
                    ("digest", text(self.recovery.digest())),
                    (
                        "rows",
                        Json::List(
                            self.recovery
                                .subjects()
                                .iter()
                                .skip(offset)
                                .take(limit as usize)
                                .map(|s| {
                                    object([
                                        ("attempt", identity(s.attempt())),
                                        ("assessment", text(format!("{:?}", s.assessment()))),
                                        (
                                            "findings",
                                            text(
                                                s.findings().chars().take(512).collect::<String>(),
                                            ),
                                        ),
                                        (
                                            "detail_complete",
                                            Json::Bool(s.findings().chars().count() <= 512),
                                        ),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                ]))
            }
            "CloseStatus" => Ok(status_json(self.status()?)),
            "ReviewBundle" => {
                let a = jojo_domain::identity::ArtifactId(id(get(p, "subject")?)?);
                let b = reader
                    .read(|v| v.review_bundle(a))
                    .map_err(|e| e.to_string())?;
                Ok(b.map_or(Json::Null, |b| {
                    object([
                        ("subject", identity(b.subject.0)),
                        ("revision", number(b.revision.0)),
                        ("hash", text(b.hash)),
                        ("content_hash", text(b.content_hash)),
                        ("displayed_hash", text(b.displayed_hash)),
                        ("context_hash", text(b.context_hash)),
                        (
                            "upstream",
                            Json::List(b.upstream.into_iter().map(|s| number(s.0)).collect()),
                        ),
                        (
                            "prior_decision",
                            b.prior_decision.map_or(Json::Null, |s| number(s.0)),
                        ),
                    ])
                }))
            }
            "HumanDecision" => {
                if !matches!(p,Json::Object(f) if f.len()==8) {
                    return Err("InvalidDecisionPayload".into());
                }
                if !self.compatible || e.major != MAJOR as u64 || e.minor > MINOR as u64 {
                    return Err("ProtocolMismatch: diagnostics only".into());
                }
                if e.request.0 >> 127 != 0 {
                    return Err("ReservedIdentity".into());
                }
                let a = jojo_domain::identity::ArtifactId(id(get(p, "subject")?)?);
                let review = ReviewBundle {
                    subject: a,
                    revision: jojo_domain::identity::Revision(
                        e.revision.ok_or("RevisionRequired")?,
                    ),
                    hash: string(get(p, "hash")?)?.into(),
                    content_hash: string(get(p, "content_hash")?)?.into(),
                    displayed_hash: string(get(p, "displayed_hash")?)?.into(),
                    context_hash: string(get(p, "context_hash")?)?.into(),
                    upstream: match get(p, "upstream")? {
                        Json::List(v) if v.len() <= 32 => v
                            .iter()
                            .map(|v| uint(v).map(EventSeq))
                            .collect::<Result<_, _>>()?,
                        _ => return Err("InvalidUpstream".into()),
                    },
                    prior_decision: match get(p, "prior_decision")? {
                        Json::Null => None,
                        v => Some(EventSeq(uint(v)?)),
                    },
                };
                let action = match string(get(p, "action")?)? {
                    "Approve" => DecisionAction::Approve,
                    "Reject" => DecisionAction::Reject,
                    "Revoke" => DecisionAction::Revoke,
                    _ => return Err("UnknownDecision".into()),
                };
                self.engine
                    .human_decide(
                        CommandId(e.request),
                        HumanOrigin::TrustedUi {
                            actor: format!("stdio:{:032x}", e.session.0),
                        },
                        review,
                        action,
                    )
                    .map(receipt)
                    .map_err(|e| format!("{e:?}"))
            }
            "PrepareClose" | "CancelClose" | "ConfirmClose" | "CancelAttempt" => {
                if !self.compatible || e.major != MAJOR as u64 || e.minor > MINOR as u64 {
                    return Err("ProtocolMismatch: diagnostics only".into());
                }
                if e.request.0 >> 127 != 0 {
                    return Err("ReservedIdentity".into());
                }
                // Envelope revision and full canonical payload are in the durable digest.
                let payload = encode(&object([
                    ("session", identity(e.session)),
                    ("revision", e.revision.map_or(Json::Null, number)),
                    ("payload", e.payload.clone()),
                ]));
                let request = CommandRequest::new(
                    CommandId(e.request),
                    self.engine.epoch(),
                    &e.kind,
                    payload,
                );
                let origin = HumanOrigin::TrustedUi {
                    actor: format!("stdio:{:032x}", e.session.0),
                };
                let ticket = if matches!(e.kind.as_str(), "CancelClose" | "ConfirmClose") {
                    Some(id(get(p, "ticket")?)?)
                } else {
                    None
                };
                let attempt = if e.kind == "CancelAttempt" {
                    Some(id(get(p, "attempt")?)?)
                } else {
                    None
                };
                let t = self.ids.next_id().map_err(|_| "IDs exhausted")?;
                let s = self.ids.next_id().map_err(|_| "IDs exhausted")?;
                let kind = e.kind.clone();
                let result = self
                    .engine
                    .execute(request, move |tx| match kind.as_str() {
                        "PrepareClose" => tx.prepare_close(t, s, &origin),
                        "CancelClose" => tx.cancel_close(
                            ticket.ok_or_else(|| TxError::Rejected("ticket".into()))?,
                            &origin,
                        ),
                        "ConfirmClose" => tx.confirm_close(
                            ticket.ok_or_else(|| TxError::Rejected("ticket".into()))?,
                            s,
                            &origin,
                        ),
                        _ => {
                            tx.cancel_approved_attempt(
                                attempt.ok_or_else(|| TxError::Rejected("attempt".into()))?,
                            )?;
                            Ok("cancel committed; completion requires result".into())
                        }
                    })
                    .map_err(|e| format!("{e:?}"));
                if result.as_ref().is_ok_and(|r| !r.replayed) {
                    self.finalized = false;
                }
                result.map(receipt)
            }
            // Protocol 1.1. A 1.0 frame naming these gets the 1.0 answer below.
            "RecoveryInspect" if e.minor >= 1 => {
                drop(reader);
                self.reinspect()?;
                Ok(object([
                    ("digest", text(self.recovery.digest())),
                    ("subjects", number(self.recovery.subjects().len() as u64)),
                    (
                        "other_quarantined",
                        number(self.recovery.other_quarantined().len() as u64),
                    ),
                ]))
            }
            "RegisterRepository" | "ImportBundle" | "Start" | "RecoveryResolve"
            | "RecoveryStart" | "Validate" | "CandidateDecision"
                if e.minor >= 1 =>
            {
                drop(reader);
                self.slice_command(e)
            }
            "Candidate" | "CandidateChanges" | "CandidateDiff" | "ValidationReport"
                if e.minor >= 1 =>
            {
                drop(reader);
                self.review_query(e)
            }
            _ => Err("UnknownType".into()),
        }
    }
    /// Protocol 1.1 mutations. Each commits under the client's command identity, so a
    /// repeated frame returns the original receipt and repeats nothing.
    fn slice_command(&mut self, e: &Envelope) -> Result<Json, String> {
        if !self.compatible || e.major != MAJOR as u64 || e.minor > MINOR as u64 {
            return Err("ProtocolMismatch: diagnostics only".into());
        }
        if e.request.0 >> 127 != 0 {
            return Err("ReservedIdentity".into());
        }
        let p = &e.payload;
        // Exact field sets: a frame carrying anything else (an executable, arguments, a
        // variable) is refused outright rather than having the extra field ignored.
        let fields: &[&str] = match e.kind.as_str() {
            "RegisterRepository" => &["path"],
            "ImportBundle" => &[
                "feature",
                "prd_id",
                "adr_id",
                "plan_id",
                "validation_id",
                "prd",
                "adr",
                "step",
                "repository",
                "base",
                "provider",
                "validation",
            ],
            "Start" => &["prd", "adr", "plan", "validation", "step"],
            "RecoveryResolve" => &["digest", "attempt", "choice"],
            "Validate" => &["attempt", "candidate"],
            "CandidateDecision" => &["attempt", "candidate", "receipt", "action"],
            _ => &["digest"],
        };
        if !matches!(p, Json::Object(f) if f.len() == fields.len()
            && fields.iter().all(|k| f.iter().any(|(name, _)| name == k)))
        {
            return Err("InvalidPayload: unexpected or missing fields".into());
        }
        let payload = encode(&object([
            ("session", identity(e.session)),
            ("revision", e.revision.map_or(Json::Null, number)),
            ("payload", e.payload.clone()),
        ]));
        let request =
            CommandRequest::new(CommandId(e.request), self.engine.epoch(), &e.kind, payload);
        let origin = HumanOrigin::TrustedUi {
            actor: format!("stdio:{:032x}", e.session.0),
        };
        let rejected = |e: CommandError| match e {
            CommandError::Rejected(TxError::Rejected(why)) => why,
            other => format!("{other:?}"),
        };
        let subject = |key: &str| get(p, key).and_then(id).map(ArtifactId);
        // A command that was already committed, in this or an earlier epoch, is answered
        // from its receipt alone: no check is re-run and nothing is dispatched again.
        let known = self
            .engine
            .reader()
            .and_then(|mut r| r.receipt(request.id))
            .map_err(|e| e.to_string())?;
        if known.is_some() {
            let r = self
                .engine
                .execute(request, |_| Err(TxError::Rejected("replay only".into())))
                .map_err(rejected)?;
            let Json::Object(mut fields) = receipt(r) else {
                unreachable!("receipt is an object")
            };
            if e.kind == "Start" {
                fields.push(("attempt".into(), identity(e.request)));
                fields.push(("dispatch".into(), text("not repeated")));
            }
            if e.kind == "Validate" {
                fields.push(("validation".into(), text("not repeated")));
            }
            return Ok(Json::Object(fields));
        }
        match e.kind.as_str() {
            "RegisterRepository" => {
                let slice = self.config.slice.as_ref().ok_or("SliceNotConfigured")?;
                let path = string(get(p, "path")?)?;
                if path.len() > 4096 || path.contains('\0') {
                    return Err("InvalidRepositoryPath".into());
                }
                let root = DisposableRoot::inspect(&slice.disposable_root)?;
                let repository = root.repository(Path::new(path))?;
                self.engine
                    .execute(request, move |tx| {
                        let id = tx.register_local_repository(&repository)?;
                        Ok(format!("{:032x}", id.0.0))
                    })
                    .map(receipt)
                    .map_err(rejected)
            }
            "ImportBundle" => {
                let subjects = SliceSubjects {
                    prd: subject("prd_id")?,
                    adr: subject("adr_id")?,
                    plan: subject("plan_id")?,
                    validation: subject("validation_id")?,
                };
                let prd = PreparedPrd::import(string(get(p, "prd")?)?.as_bytes())
                    .map_err(|e| format!("InvalidPrd: {e:?}"))?;
                let validation = ValidationPlan::import(string(get(p, "validation")?)?.as_bytes())
                    .map_err(|e| format!("InvalidValidationPlan: {e}"))?;
                let provider = match string(get(p, "provider")?)? {
                    "Codex" => ProviderKind::Codex,
                    "ClaudeCode" => ProviderKind::ClaudeCode,
                    _ => return Err("UnknownProvider".into()),
                };
                let bundle = FeatureBundle::new(
                    FeatureId(id(get(p, "feature")?)?),
                    subjects,
                    prd,
                    string(get(p, "adr")?)?.to_owned(),
                    id(get(p, "step")?)?,
                    RepositoryId(id(get(p, "repository")?)?),
                    string(get(p, "base")?)?.to_owned(),
                    provider,
                    validation,
                )
                .map_err(|e| format!("InvalidBundle: {e}"))?;
                self.engine
                    .execute(request, move |tx| tx.import_feature_bundle(&bundle))
                    .map(receipt)
                    .map_err(rejected)
            }
            "Start" => {
                let start = StartRequest {
                    attempt: e.request,
                    subjects: SliceSubjects {
                        prd: subject("prd")?,
                        adr: subject("adr")?,
                        plan: subject("plan")?,
                        validation: subject("validation")?,
                    },
                    step: NodeRef {
                        id: id(get(p, "step")?)?,
                        revision: Revision(1),
                    },
                };
                self.start(request, start)
            }
            "RecoveryResolve" => {
                if string(get(p, "digest")?)? != self.recovery.digest() {
                    return Err("StaleInspection: inspect again".into());
                }
                let attempt = id(get(p, "attempt")?)?;
                let choice = match string(get(p, "choice")?)? {
                    "ValidateOutput" => RecoveryChoice::ValidateOutput,
                    "ContinueWithHistory" => RecoveryChoice::ContinueWithHistory,
                    "ContinueFreshContext" => RecoveryChoice::ContinueFreshContext,
                    "Hold" => RecoveryChoice::Hold,
                    _ => return Err("UnknownChoice".into()),
                };
                let inspection = self.recovery.clone();
                // The core decides what the inspected OS and repository evidence offers;
                // a choice it does not offer is refused there, whatever the human sent.
                let r = self
                    .engine
                    .execute(request, move |tx| {
                        tx.resolve_recovery(&inspection, attempt, choice, origin)
                    })
                    .map_err(rejected)?;
                let settled = if !r.replayed && r.response == "SETTLE" {
                    settle_attempt(&self.ctx(), attempt, BlobFaults::default())
                        .map(|_| ())
                        .map_err(|e| format!("SettlementFailed: {e}"))
                } else {
                    Ok(())
                };
                // Facts changed: the old digest must not authorize anything further.
                self.reinspect()?;
                settled.map(|()| receipt(r))
            }
            // P2-03. The command names the sealed candidate digest the human is looking
            // at; the validation command itself comes only from the approved plan.
            "Validate" => {
                let attempt = id(get(p, "attempt")?)?;
                let candidate = digest(get(p, "candidate")?)?;
                if !self.workers.reap() {
                    return Err("CapacityOne: a run or validation is active".into());
                }
                let root = validation_root(&self.engine)?;
                let run = e.request;
                let sealed = candidate.clone();
                let r = self
                    .engine
                    .execute(request, move |tx| {
                        tx.begin_validation(run, attempt, &sealed)
                    })
                    .map_err(rejected)?;
                let Json::Object(mut fields) = receipt(r.clone()) else {
                    unreachable!("receipt is an object")
                };
                if r.replayed {
                    fields.push(("validation".into(), text("not repeated")));
                    return Ok(Json::Object(fields));
                }
                self.finalized = false;
                self.validation_stop = Arc::new(AtomicBool::new(false));
                let cfg = ValidatorConfig {
                    root,
                    inspector: self.config.inspector.clone(),
                    guard: self.guard.clone(),
                    budget: self.validation_budget,
                    clock: self.clock.clone(),
                    stop: self.validation_stop.clone(),
                };
                let (engine, ids) = (self.engine.clone(), self.ids.clone());
                let started = self.workers.validate(move || {
                    let store = BlobStore::for_engine(&engine)?;
                    let ctx = SettlementContext {
                        engine: &engine,
                        ids: ids.as_ref(),
                        store: &store,
                    };
                    run_validation(&ctx, &cfg, run, attempt, &candidate)
                });
                if let Err(why) = started {
                    abandon_validation(&self.ctx(), run, attempt, &why)?;
                    return Err(format!("ValidationNotStarted: {why}"));
                }
                fields.push(("validation".into(), text("started")));
                Ok(Json::Object(fields))
            }
            "CandidateDecision" => {
                let attempt = id(get(p, "attempt")?)?;
                let candidate = digest(get(p, "candidate")?)?;
                match (string(get(p, "action")?)?, get(p, "receipt")?) {
                    ("Accept", receipt_seq) => {
                        let reviewed = EventSeq(uint(receipt_seq).map_err(|_| "ReceiptRequired")?);
                        // The candidate must still be the sealed one on disk, now.
                        verify_sealed_candidate(&self.engine, attempt, &candidate)
                            .map_err(|why| format!("CandidateChanged: {why}"))?;
                        self.engine
                            .execute(request, move |tx| {
                                tx.accept_reviewed_candidate(
                                    attempt, &candidate, reviewed, origin,
                                )?;
                                Ok("candidate accepted".into())
                            })
                            .map(receipt)
                            .map_err(rejected)
                    }
                    ("Reject", Json::Null) => self
                        .engine
                        .execute(request, move |tx| {
                            tx.reject_candidate(attempt, &candidate, origin)?;
                            Ok("candidate rejected".into())
                        })
                        .map(receipt)
                        .map_err(rejected),
                    _ => Err("UnknownDecision".into()),
                }
            }
            "RecoveryStart" => {
                if string(get(p, "digest")?)? != self.recovery.digest() {
                    return Err("StaleInspection: inspect again".into());
                }
                let inspection = self.recovery.clone();
                self.engine
                    .execute(request, move |tx| {
                        tx.start_after_recovery(&inspection, origin)?;
                        Ok("admission open after reviewed recovery".into())
                    })
                    .map(receipt)
                    .map_err(rejected)
            }
            _ => Err("UnknownType".into()),
        }
    }
    /// P2-03 read-only review queries. Changed paths and the diff are derived from the
    /// repository each time, after the sealed digest is re-checked; nothing here reads a
    /// path, file list or test result supplied by a provider or by the client.
    fn review_query(&mut self, e: &Envelope) -> Result<Json, String> {
        let p = &e.payload;
        let fields: &[&str] = match e.kind.as_str() {
            "Candidate" => &["attempt"],
            "CandidateChanges" => &["attempt", "candidate", "offset", "limit"],
            "CandidateDiff" => &["attempt", "candidate", "offset"],
            _ => &["attempt", "receipt", "offset"],
        };
        if !matches!(p, Json::Object(f) if f.len() == fields.len()
            && fields.iter().all(|k| f.iter().any(|(name, _)| name == k)))
        {
            return Err("InvalidPayload: unexpected or missing fields".into());
        }
        let attempt = id(get(p, "attempt")?)?;
        let mut reader = self.engine.reader().map_err(|e| e.to_string())?;
        let page = |body: &str, offset: u64, complete: bool| {
            let total = body.chars().count() as u64;
            let chunk: String = body.chars().skip(offset as usize).take(4096).collect();
            object([
                ("attempt", identity(attempt)),
                ("offset", number(offset)),
                (
                    "next",
                    number(offset.saturating_add(chunk.chars().count() as u64)),
                ),
                ("total", number(total)),
                ("complete", Json::Bool(complete)),
                ("text", text(chunk)),
            ])
        };
        match e.kind.as_str() {
            "Candidate" => reader
                .read(|v| v.ipc_candidate(attempt))
                .map_err(|e| e.to_string()),
            "CandidateChanges" => {
                let candidate = digest(get(p, "candidate")?)?;
                let offset = usize::try_from(uint(get(p, "offset")?)?).map_err(|_| "Offset")?;
                let limit = uint(get(p, "limit")?)?;
                if limit == 0 || limit > u64::from(MAX_PAGE) {
                    return Err("Limit".into());
                }
                let plan = reader
                    .read(|v| v.attempt_validation_plan(attempt))
                    .map_err(|e| e.to_string())?;
                drop(reader);
                let changes = verify_sealed_candidate(&self.engine, attempt, &candidate)
                    .map_err(|why| format!("CandidateUnavailable: {why}"))?;
                let entry = plan.and_then(|(_, step, text)| {
                    ValidationPlan::import(text.as_bytes())
                        .ok()
                        .and_then(|plan| plan.step(step).cloned())
                });
                // Without the approved scope there is no verdict, and never a pass.
                let verdict = entry
                    .as_ref()
                    .map(|s| jojo_domain::validation::check_scope(changes.changes(), s));
                let scope = |path: &str| match &verdict {
                    None => "UNKNOWN",
                    Some(v) if v.baseline_changed.iter().any(|b| b == path) => "BASELINE",
                    Some(v) if v.denied.iter().any(|d| d == path) => "OUT_OF_SCOPE",
                    Some(_) => "ALLOWED",
                };
                Ok(object([
                    ("attempt", identity(attempt)),
                    ("candidate", text(candidate)),
                    ("base", text(changes.base())),
                    ("total", number(changes.changes().len() as u64)),
                    (
                        "scope",
                        text(match &verdict {
                            None => "UNKNOWN",
                            Some(v) if v.passed() => "PASS",
                            Some(_) => "FAIL",
                        }),
                    ),
                    (
                        "rows",
                        Json::List(
                            changes
                                .changes()
                                .iter()
                                .skip(offset)
                                .take(limit as usize)
                                .map(|c| {
                                    object([
                                        (
                                            "path",
                                            text(c.path.chars().take(512).collect::<String>()),
                                        ),
                                        (
                                            "detail_complete",
                                            Json::Bool(c.path.chars().count() <= 512),
                                        ),
                                        ("kind", text(format!("{:?}", c.kind))),
                                        ("scope", text(scope(&c.path))),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                ]))
            }
            "CandidateDiff" => {
                let candidate = digest(get(p, "candidate")?)?;
                let offset = uint(get(p, "offset")?)?;
                drop(reader);
                let (diff, complete) = candidate_diff(&self.engine, attempt, &candidate)
                    .map_err(|why| format!("CandidateUnavailable: {why}"))?;
                Ok(page(&diff, offset, complete))
            }
            _ => {
                let event = uint(get(p, "receipt")?)?;
                let offset = uint(get(p, "offset")?)?;
                let found = reader
                    .read(|v| v.validation_receipts(attempt))
                    .map_err(|e| e.to_string())?
                    .into_iter()
                    .find(|r| r.event.0 == event);
                let Some(found) = found else {
                    return Ok(Json::Null);
                };
                let bytes = self
                    .store
                    .read(&found.report)
                    .map_err(|e| format!("ReportUnavailable: {e}"))?;
                Ok(page(&String::from_utf8_lossy(&bytes), offset, true))
            }
        }
    }
    /// One human Start: accept durably, then provision, reserve, bind and launch through
    /// the configured adapter. Every refusal that can be known beforehand happens before
    /// anything is committed.
    fn start(&mut self, request: CommandRequest, start: StartRequest) -> Result<Json, String> {
        let with = |r: Receipt, dispatch: &str| {
            let Json::Object(mut fields) = receipt(r) else {
                unreachable!("receipt is an object")
            };
            fields.push(("attempt".into(), identity(start.attempt)));
            fields.push(("dispatch".into(), text(dispatch)));
            Json::Object(fields)
        };
        let rejected = |e: CommandError| match e {
            CommandError::Rejected(TxError::Rejected(why)) => why,
            other => format!("{other:?}"),
        };
        let mut reader = self.engine.reader().map_err(|e| e.to_string())?;
        let slice = self.config.slice.clone().ok_or("SliceNotConfigured")?;
        if !self.workers.reap() {
            return Err("CapacityOne: a run is active".into());
        }
        let snapshot = reader.snapshot().map_err(|e| e.to_string())?;
        if snapshot.session.admission != Admission::Open {
            return Err("AdmissionClosed".into());
        }
        self.reinspect()?;
        if !self.recovery.subjects().is_empty() {
            return Err("RecoveryHold: unresolved recovery subjects".into());
        }
        let step = reader
            .read(|v| v.slice_step(&start.subjects, start.step))
            .map_err(|e| e.to_string())?
            .map_err(|why| format!("StartRefused: {why}"))?;
        drop(reader);
        // The only production adapter. No other provider can be launched by Start.
        if step.provider != ProviderKind::Codex {
            return Err(format!("NoProductionAdapter: {:?}", step.provider));
        }
        let codex = slice
            .codex
            .ok_or("NoProductionAdapter: Codex is not configured")?;
        let adapter = CodexAdapter::new(codex)
            .and_then(|adapter| adapter.ready().map(|()| adapter))
            .map_err(|e| format!("AdapterRejected: {e:?}"))?;
        let repository =
            DisposableRoot::inspect(&slice.disposable_root)?.repository(&step.repository_path)?;
        if repository.id() != step.repository {
            return Err("StartRefused: repository identity changed".into());
        }
        let root = prepare_worktree_root(&slice.worktree_root)?;
        let accepted = start.clone();
        let r = self
            .engine
            .execute(request, move |tx| {
                tx.accept_start(&accepted, &repository, &root)
            })
            .map_err(rejected)?;
        if r.replayed {
            return Ok(with(r, "not repeated"));
        }
        self.finalized = false;
        match self.dispatch(&start, &adapter) {
            Ok(()) => Ok(with(r, "launched")),
            Err(why) => Err(format!(
                "StartFailed: {why}; command {:032x} is committed and nothing was launched",
                start.attempt.0
            )),
        }
    }
    fn dispatch(&mut self, start: &StartRequest, adapter: &CodexAdapter) -> Result<(), String> {
        provision_and_reserve(&self.ctx(), start)?;
        let prepared = slice_launch_spec(
            &self.engine,
            start,
            adapter.fingerprint(),
            vec![CODEX_HOME_VARIABLE.to_owned()],
        )
        .and_then(|spec| {
            adapter
                .prepare_launch(&spec)
                .map(|launch| (spec, launch))
                .map_err(|e| format!("AdapterRejected: {e:?}"))
        });
        let (spec, launch) = match prepared {
            Ok(prepared) => prepared,
            Err(why) => {
                retire_unlaunched(&self.ctx(), start.attempt)?;
                return Err(why);
            }
        };
        let mut hooks = self
            .config
            .launch_hooks
            .as_mut()
            .map_or_else(LaunchHooks::default, |make| make());
        if hooks.inspector.is_none() {
            hooks.inspector = self.config.inspector.clone();
        }
        if hooks.guard.is_none() {
            hooks.guard = self.guard.clone();
        }
        // Unknown interrupt support: signal-only stop plan with the accepted budgets.
        let plan = StopPlan::new(adapter.capabilities(), StopBudgets::ACCEPTED);
        self.launch(spec, launch, plan, hooks)
    }
    pub fn finish(&mut self, disconnected: bool) -> Result<Option<ShutdownReceipt>, String> {
        // Closing: an active validator is stopped; it records no verdict.
        self.validation_stop.store(true, Ordering::SeqCst);
        if !self.workers.reap() {
            return Ok(None);
        }
        let s = self.status()?;
        if self.finalized {
            return Ok(s.receipt);
        }
        if !matches!(
            s.state,
            CloseState::Stopping | CloseState::Finalizing | CloseState::Failed
        ) {
            return Ok(None);
        }
        if self.workers.failed() {
            return Err("worker failed; recovery required".into());
        }
        self.finalized = true;
        finish_close(
            &self.ctx(),
            self.clock.as_ref(),
            CloseWait {
                budget: Duration::ZERO,
                poll: Duration::from_millis(10),
            },
            disconnected,
        )
        .map(Some)
    }
}
/// A lowercase SHA-256 digest as protocol text.
fn digest(value: &Json) -> Result<String, String> {
    let s = string(value)?;
    if s.len() != 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("expected a SHA-256 digest".into());
    }
    Ok(s.to_owned())
}
fn receipt(r: Receipt) -> Json {
    object([
        ("acknowledged", Json::Bool(true)),
        ("operation_completed", Json::Bool(false)),
        ("command", identity(r.command_id.0)),
        ("epoch", number(r.epoch.0)),
        ("event_seq", number(r.event_seq.0)),
        ("response", text(r.response)),
        ("replayed", Json::Bool(r.replayed)),
    ])
}
pub fn shutdown_json(r: &ShutdownReceipt) -> Json {
    object([
        ("ticket", identity(r.ticket)),
        ("shutdown", identity(r.shutdown)),
        ("event_seq", number(r.event.0)),
        ("outcome", text(format!("{:?}", r.outcome))),
        (
            "process_stop_confirmed",
            Json::Bool(r.process_stop_confirmed),
        ),
        ("persistence_sealed", Json::Bool(r.persistence_sealed)),
        ("targets", Json::List(r.targets.iter().map(text).collect())),
        (
            "recovery_items",
            Json::List(r.recovery_items.iter().map(text).collect()),
        ),
        (
            "failures",
            Json::List(r.failures.iter().map(text).collect()),
        ),
    ])
}
fn status_json(s: CloseStatus) -> Json {
    object([
        ("state", text(format!("{:?}", s.state))),
        (
            "ticket",
            s.ticket.map_or(Json::Null, |t| identity(t.ticket)),
        ),
        ("active_count", number(s.active.len() as u64)),
        (
            "receipt",
            s.receipt.as_ref().map_or(Json::Null, shutdown_json),
        ),
    ])
}

fn control(e: &Envelope) -> bool {
    matches!(
        e.kind.as_str(),
        "PrepareClose"
            | "CancelClose"
            | "ConfirmClose"
            | "CancelAttempt"
            | "HumanDecision"
            | "Hello"
            | "Start"
            | "RecoveryResolve"
            | "RecoveryStart"
            | "Validate"
            | "CandidateDecision"
    )
}

/// Dedicated bounded input/output channels. A full bulk queue returns Busy; it cannot
/// prevent reading the next control frame. EOF and output failure set an independent
/// emergency flag even if every response queue is full. There is no log push stream.
pub fn serve(
    mut service: Service,
    mut input: impl Read + Send + 'static,
    mut output: impl Write + Send + 'static,
) -> u8 {
    let lost = Arc::new(AtomicBool::new(false));
    let (ct, cr) = mpsc::sync_channel::<Envelope>(16);
    let (bt, br) = mpsc::sync_channel::<Envelope>(16);
    let (ot, or) = mpsc::sync_channel::<(Envelope, mpsc::SyncSender<bool>)>(16);
    let read_lost = lost.clone();
    let read_output = ot.clone();
    std::thread::spawn(move || {
        let mut decoder = Decoder::default();
        let mut buf = [0u8; 1];
        loop {
            match input.read(&mut buf) {
                Ok(0) => {
                    let _ = decoder.finish();
                    break;
                }
                Ok(_) => match decoder.push(buf[0]) {
                    Ok(Some(e)) => {
                        let tx = if control(&e) { &ct } else { &bt };
                        if let Err(error) = tx.try_send(e) {
                            let e = match error {
                                mpsc::TrySendError::Full(e) => e,
                                mpsc::TrySendError::Disconnected(_) => break,
                            };
                            if control(&e) {
                                break;
                            }
                            let reply = Envelope {
                                major: 1,
                                minor: 0,
                                session: e.session,
                                epoch: e.epoch,
                                request: e.request,
                                revision: None,
                                kind: "Error".into(),
                                payload: object([("code", text("Busy: request not accepted"))]),
                            };
                            let (ack, _) = mpsc::sync_channel(1);
                            if read_output.try_send((reply, ack)).is_err() {
                                break;
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(reason) => {
                        read_lost.store(true, Ordering::SeqCst);
                        let reply = Envelope {
                            major: 1,
                            minor: 0,
                            session: OpaqueId(0),
                            epoch: 0,
                            request: OpaqueId(0),
                            revision: None,
                            kind: "Error".into(),
                            payload: object([("code", text(format!("ProtocolError: {reason}")))]),
                        };
                        let (ack, _) = mpsc::sync_channel(1);
                        let _ = read_output.try_send((reply, ack));
                        break;
                    }
                },
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        read_lost.store(true, Ordering::SeqCst);
    });
    let write_lost = lost.clone();
    std::thread::spawn(move || {
        for (e, ack) in or {
            let bytes = encode(&e.json());
            let ok = bytes.len() <= MAX_FRAME
                && output
                    .write_all(bytes.as_bytes())
                    .and_then(|_| output.write_all(b"\n"))
                    .and_then(|_| output.flush())
                    .is_ok();
            let _ = ack.send(ok);
            if !ok {
                write_lost.store(true, Ordering::SeqCst);
                break;
            }
        }
    });
    let mut emergency = false;
    let mut emergency_failed = false;
    let mut last_failure = None;
    let mut waiting_since = None;
    loop {
        if lost.load(Ordering::SeqCst) && !emergency {
            emergency = true;
            emergency_failed = service.emergency().is_err();
        }
        if !emergency {
            let e = cr.try_recv().ok().or_else(|| br.try_recv().ok());
            if let Some(e) = e {
                let response = service.request(&e);
                let (ack, _) = mpsc::sync_channel(1);
                if ot.try_send((response, ack)).is_err() {
                    lost.store(true, Ordering::SeqCst);
                }
            }
        }
        if emergency_failed && service.workers.reap() {
            return 4;
        }
        let s = service.status();
        if s.as_ref().is_ok_and(|s| {
            matches!(
                s.state,
                CloseState::Stopping | CloseState::Finalizing | CloseState::Failed
            )
        }) || emergency
        {
            let since = waiting_since.get_or_insert_with(Instant::now);
            match service.finish(emergency) {
                Ok(Some(r)) => {
                    if emergency {
                        return if r.process_stop_confirmed && r.persistence_sealed {
                            0
                        } else {
                            4
                        };
                    }
                    if last_failure != Some(r.event.0) {
                        let ready = r.outcome == ShutdownOutcome::Ready;
                        let event = Envelope {
                            major: 1,
                            minor: 0,
                            session: service.session.unwrap_or(OpaqueId(0)),
                            epoch: service.engine.epoch().0,
                            request: OpaqueId(0),
                            revision: None,
                            kind: if ready {
                                "ShutdownReady"
                            } else {
                                "ShutdownFailure"
                            }
                            .into(),
                            payload: shutdown_json(&r),
                        };
                        let (ack, rx) = mpsc::sync_channel(1);
                        if ot.try_send((event, ack)).is_err() {
                            lost.store(true, Ordering::SeqCst);
                        } else if ready {
                            // Only a committed READY and successful channel flush allow exit 0.
                            return if rx.recv_timeout(Duration::from_secs(2)) == Ok(true)
                                && !lost.load(Ordering::SeqCst)
                            {
                                0
                            } else {
                                4
                            };
                        }
                        last_failure = Some(r.event.0);
                    }
                }
                Err(_) => {
                    if service.workers.reap() {
                        return 4;
                    }
                }
                Ok(None) => {}
            }
            if since.elapsed() > Duration::from_secs(45) && !service.workers.reap() {
                // A stalled thread is not absence proof. Retain engine ownership and
                // continue handling controls; never detach an active child on timeout.
                if !emergency && last_failure.is_none() {
                    let event = Envelope {
                        major: 1,
                        minor: 0,
                        session: service.session.unwrap_or(OpaqueId(0)),
                        epoch: service.engine.epoch().0,
                        request: OpaqueId(0),
                        revision: None,
                        kind: "ShutdownFailure".into(),
                        payload: object([(
                            "code",
                            text("WorkerStalled: helper retains ownership"),
                        )]),
                    };
                    let (ack, _) = mpsc::sync_channel(1);
                    let _ = ot.try_send((event, ack));
                    last_failure = Some(0);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        if !self.workers.reap() {
            let _ = self.emergency();
        }
    }
}

//! P1-13 helper runtime. Workers retain P1-09 lifetime handles; IPC never signals a PID.
//!
//! P2-02 (protocol 1.1) adds repository registration, feature-bundle import, Start and
//! recovery resolution. Each is a thin mapping onto existing core commands. A Start
//! carries subject identities only: the executable, its arguments and its environment
//! come from the Codex adapter configured when the helper was started, never from IPC.
use crate::SystemClock;
use jojo_domain::claude::CLAUDE_VARIABLES;
use jojo_domain::codex::CODEX_HOME_VARIABLE;
use jojo_domain::dependency::NodeRef;
use jojo_domain::identity::Revision;
use jojo_domain::identity::{IdGenerationError, IdGenerator, OpaqueId};
use jojo_domain::ipc::*;
use jojo_domain::json::Json;
use jojo_domain::paths::LocalRoots;
use jojo_domain::planner::TurnSpec;
use jojo_domain::protocol::{
    self, CandidateVerdict, Class, CloseCommand, DecisionVerb, RequestKind,
};
use jojo_domain::provider::LaunchSpec;
use jojo_domain::provider::{AuthMode, ProviderAdapter, ProviderKind};
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
    /// M2: one lead agent turn. It is not an attempt and is not settled like one; capacity,
    /// close and drop wait for it exactly as for a launch. `stop` ends it early.
    pub fn turn(
        &mut self,
        runtime: WorkerRuntime,
        spec: TurnSpec,
        launch: ProcessLaunch,
        plan: StopPlan,
        mut hooks: LaunchHooks,
        stop: Arc<AtomicBool>,
    ) -> Result<(), String> {
        hooks.emergency_stop = Some(stop);
        let WorkerRuntime { engine, ids, clock } = runtime;
        let handle = std::thread::Builder::new()
            .name("jojo-turn".into())
            .spawn(move || {
                let ctx = LaunchContext {
                    engine: &engine,
                    ids: ids.as_ref(),
                    clock: clock.as_ref(),
                    poll: Duration::from_millis(10),
                };
                let run = run_turn(&ctx, &spec, &launch, &plan, hooks);
                settle_turn(&ctx, &spec, &run).map(drop)
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
    /// M1: the one Claude Code executable a Start may launch, fixed by the helper's
    /// launcher like the Codex one in `SliceConfig`. `None`: no Claude Code step can start.
    pub claude: Option<ClaudeAdapterConfig>,
    /// Stop flag of the latest validation run, set on close, emergency and drop.
    validation_stop: Arc<AtomicBool>,
    /// M2: stop flag of the latest lead agent turn, set on close, emergency and drop.
    turn_stop: Arc<AtomicBool>,
}

/// The launcher's flags as the adapter's configuration.
pub fn claude_config(options: crate::ClaudeOptions) -> ClaudeAdapterConfig {
    ClaudeAdapterConfig {
        executable: options.executable,
        version: options.version,
        sha256: options.sha256,
        home: options.home,
        user: options.user,
        // The launcher's flag is the human's attestation; nothing is inspected.
        auth: AuthMode::NativeSubscription,
    }
}

/// The production adapters. A Start reaches a process only through one of them.
enum Adapter {
    Codex(CodexAdapter),
    Claude(ClaudeAdapter),
}
impl Adapter {
    fn fingerprint(&self) -> &jojo_domain::provider::ProviderFingerprint {
        match self {
            Self::Codex(a) => a.fingerprint(),
            Self::Claude(a) => a.fingerprint(),
        }
    }
    /// The names of the provider variables its launch envelope carries.
    fn variables(&self) -> Vec<String> {
        match self {
            Self::Codex(_) => vec![CODEX_HOME_VARIABLE.to_owned()],
            Self::Claude(_) => CLAUDE_VARIABLES.map(str::to_owned).to_vec(),
        }
    }
    fn prepare_launch(&self, spec: &LaunchSpec) -> Result<ProcessLaunch, String> {
        match self {
            Self::Codex(a) => a
                .prepare_launch(spec)
                .map_err(|e| format!("AdapterRejected: {e:?}")),
            Self::Claude(a) => a
                .prepare_launch(spec)
                .map_err(|e| format!("AdapterRejected: {e:?}")),
        }
    }
    fn capabilities(&self) -> &jojo_domain::provider::Capabilities {
        match self {
            Self::Codex(a) => a.capabilities(),
            Self::Claude(a) => a.capabilities(),
        }
    }
    fn turn_fingerprint(&self) -> jojo_domain::provider::ProviderFingerprint {
        match self {
            Self::Codex(a) => a.turn_fingerprint(),
            Self::Claude(a) => a.turn_fingerprint(),
        }
    }
    fn prepare_turn(&self, spec: &TurnSpec) -> Result<ProcessLaunch, String> {
        match self {
            Self::Codex(a) => a
                .prepare_turn(spec)
                .map_err(|e| format!("AdapterRejected: {e:?}")),
            Self::Claude(a) => a
                .prepare_turn(spec)
                .map_err(|e| format!("AdapterRejected: {e:?}")),
        }
    }
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
            claude: None,
            validation_stop: Arc::new(AtomicBool::new(false)),
            turn_stop: Arc::new(AtomicBool::new(false)),
        })
        .and_then(|service| {
            // M2: a turn an earlier helper left unfinished held no worktree, claim or
            // effect; it is closed as interrupted before anything is served.
            let stale = service
                .engine
                .reader()
                .and_then(|mut r| r.read(|v| v.unfinished_turn()))
                .map_err(|e| e.to_string())?;
            if let Some(turn) = stale {
                service.command("CloseInterruptedTurns", turn, |tx| {
                    tx.close_interrupted_turns().map(|n| format!("closed {n}"))
                })?;
            }
            Ok(service)
        })
    }
    /// One internal command under a runtime identity.
    fn command<F>(&self, kind: &str, subject: OpaqueId, f: F) -> Result<String, String>
    where
        F: FnOnce(&mut Tx<'_>) -> Result<String, TxError> + Send + 'static,
    {
        let id = self.ids.next_id().map_err(|_| "IDs exhausted")?;
        let request = CommandRequest::new(
            CommandId(id),
            self.engine.epoch(),
            kind,
            format!("{:032x}", subject.0),
        );
        match self.engine.execute(request, f) {
            Ok(r) => Ok(r.response),
            Err(CommandError::Rejected(TxError::Rejected(why))) => Err(why),
            Err(e) => Err(format!("{e:?}")),
        }
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
        self.turn_stop.store(true, Ordering::SeqCst);
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
        let named = RequestKind::of_frame(&e.kind, e.minor);
        if named == Some(RequestKind::Hello) {
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
                ("commands", text(protocol::names(Class::Mutation, e.minor))),
                ("queries", text(protocol::names(Class::Query, e.minor))),
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
        // A request newer than the frame's minor does not exist for that frame.
        let Some(kind) = named else {
            return Err("UnknownType".into());
        };
        match kind {
            RequestKind::Hello => unreachable!("answered above"),
            RequestKind::Snapshot => {
                let q = protocol::SnapshotQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_snapshot(&q.section, &q.after, q.cursor, q.limit))
                    .map_err(|e| e.to_string())
            }
            RequestKind::Events => {
                let q = protocol::EventsQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_events(q.after, q.limit))
                    .map_err(|e| e.to_string())
            }
            RequestKind::EventDetail => {
                let q = protocol::EventDetailQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_event_detail(q.seq, q.offset))
                    .map_err(|e| e.to_string())
            }
            RequestKind::Log => {
                let q = protocol::LogQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_log(q.attempt, q.sequence, q.offset))
                    .map_err(|e| e.to_string())
            }
            RequestKind::Receipt => {
                let q = protocol::ReceiptQuery::parse(p)?;
                reader
                    .receipt(CommandId(q.command))
                    .map(|r| r.map_or(Json::Null, receipt))
                    .map_err(|e| e.to_string())
            }
            RequestKind::ShutdownReceipt => {
                let q = protocol::ShutdownReceiptQuery::parse(p)?;
                reader
                    .read(|v| v.shutdown_receipt(q.ticket))
                    .map(|r| r.as_ref().map_or(Json::Null, shutdown_json))
                    .map_err(|e| e.to_string())
            }
            RequestKind::Recovery => {
                let q = protocol::RecoveryQuery::parse(p)?;
                // Protocol 1.1 rows also carry the assessment as structured fields.
                let structured = e.minor >= 1;
                Ok(object([
                    ("digest", text(self.recovery.digest())),
                    (
                        "rows",
                        Json::List(
                            self.recovery
                                .subjects()
                                .iter()
                                .skip(q.offset)
                                .take(q.limit)
                                .map(|s| {
                                    let mut row = vec![
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
                                    ];
                                    if structured {
                                        row.extend(protocol::assessment_fields(s.assessment()));
                                    }
                                    object(row)
                                })
                                .collect(),
                        ),
                    ),
                ]))
            }
            RequestKind::CloseStatus => Ok(status_json(self.status()?)),
            RequestKind::ReviewBundle => {
                let q = protocol::ReviewBundleQuery::parse(p)?;
                let b = reader
                    .read(|v| v.review_bundle(q.subject))
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
            RequestKind::ApprovedPlan => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                let q = protocol::ApprovedPlanQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_approved_plan(q.after))
                    .map_err(|e| e.to_string())
            }
            RequestKind::HumanDecision => {
                if !kind.shaped(p) {
                    return Err("InvalidDecisionPayload".into());
                }
                if !self.compatible || e.major != MAJOR as u64 || e.minor > MINOR as u64 {
                    return Err("ProtocolMismatch: diagnostics only".into());
                }
                if e.request.0 >> 127 != 0 {
                    return Err("ReservedIdentity".into());
                }
                let d = protocol::DecisionCommand::parse(p, e.revision)?;
                let review = ReviewBundle {
                    subject: d.subject,
                    revision: d.revision,
                    hash: d.hash,
                    content_hash: d.content_hash,
                    displayed_hash: d.displayed_hash,
                    context_hash: d.context_hash,
                    upstream: d.upstream.into_iter().map(EventSeq).collect(),
                    prior_decision: d.prior_decision.map(EventSeq),
                };
                let action = match d.action {
                    DecisionVerb::Approve => DecisionAction::Approve,
                    DecisionVerb::Reject => DecisionAction::Reject,
                    DecisionVerb::Revoke => DecisionAction::Revoke,
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
            RequestKind::PrepareClose
            | RequestKind::CancelClose
            | RequestKind::ConfirmClose
            | RequestKind::CancelAttempt => {
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
                let command = CloseCommand::parse(kind, p)?;
                let t = self.ids.next_id().map_err(|_| "IDs exhausted")?;
                let s = self.ids.next_id().map_err(|_| "IDs exhausted")?;
                let result = self
                    .engine
                    .execute(request, move |tx| match command {
                        CloseCommand::Prepare => tx.prepare_close(t, s, &origin),
                        CloseCommand::Cancel { ticket } => tx.cancel_close(ticket, &origin),
                        CloseCommand::Confirm { ticket } => tx.confirm_close(ticket, s, &origin),
                        CloseCommand::CancelAttempt { attempt } => {
                            tx.cancel_approved_attempt(attempt)?;
                            Ok("cancel committed; completion requires result".into())
                        }
                    })
                    .map_err(|e| format!("{e:?}"));
                if result.as_ref().is_ok_and(|r| !r.replayed) {
                    self.finalized = false;
                }
                result.map(receipt)
            }
            // Protocol 1.1. A 1.0 frame naming these got the 1.0 answer above.
            RequestKind::RecoveryInspect => {
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
            // Protocol 1.2 (M2): Feature Chat.
            RequestKind::FeatureChat => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                let q = protocol::FeatureChatQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_feature_chat(q.feature, q.after))
                    .map_err(|e| e.to_string())
            }
            RequestKind::ChatText => {
                if !kind.shaped(p) {
                    return Err("InvalidPayload: unexpected or missing fields".into());
                }
                let q = protocol::ChatTextQuery::parse(p)?;
                reader
                    .read(|v| v.ipc_chat_text(q.subject, q.part, q.offset))
                    .map_err(|e| e.to_string())
            }
            RequestKind::ChatTurn | RequestKind::AnswerQuestion | RequestKind::CancelTurn => {
                drop(reader);
                self.slice_command(kind, e)
            }
            RequestKind::RegisterRepository
            | RequestKind::ImportBundle
            | RequestKind::Start
            | RequestKind::RecoveryResolve
            | RequestKind::RecoveryStart
            | RequestKind::Validate
            | RequestKind::CandidateDecision => {
                drop(reader);
                self.slice_command(kind, e)
            }
            RequestKind::Candidate
            | RequestKind::CandidateChanges
            | RequestKind::CandidateDiff
            | RequestKind::ValidationReport => {
                drop(reader);
                self.review_query(kind, e)
            }
        }
    }
    /// Protocol 1.1 mutations. Each commits under the client's command identity, so a
    /// repeated frame returns the original receipt and repeats nothing.
    fn slice_command(&mut self, kind: RequestKind, e: &Envelope) -> Result<Json, String> {
        if !self.compatible || e.major != MAJOR as u64 || e.minor > MINOR as u64 {
            return Err("ProtocolMismatch: diagnostics only".into());
        }
        if e.request.0 >> 127 != 0 {
            return Err("ReservedIdentity".into());
        }
        let p = &e.payload;
        // Exact field sets: a frame carrying anything else (an executable, arguments, a
        // variable) is refused outright rather than having the extra field ignored.
        if !kind.shaped(p) {
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
            if kind == RequestKind::Start {
                fields.push(("attempt".into(), identity(e.request)));
                fields.push(("dispatch".into(), text("not repeated")));
            }
            if kind == RequestKind::Validate {
                fields.push(("validation".into(), text("not repeated")));
            }
            if kind == RequestKind::ChatTurn {
                fields.push(("turn".into(), identity(e.request)));
                fields.push(("dispatch".into(), text("not repeated")));
            }
            return Ok(Json::Object(fields));
        }
        match kind {
            RequestKind::RegisterRepository => {
                let slice = self.config.slice.as_ref().ok_or("SliceNotConfigured")?;
                let c = protocol::RegisterRepositoryCommand::parse(p)?;
                let root = DisposableRoot::inspect(&slice.disposable_root)?;
                let repository = root.repository(Path::new(&c.path))?;
                self.engine
                    .execute(request, move |tx| {
                        let id = tx.register_local_repository(&repository)?;
                        Ok(format!("{:032x}", id.0.0))
                    })
                    .map(receipt)
                    .map_err(rejected)
            }
            RequestKind::ImportBundle => {
                let c = protocol::ImportBundleCommand::parse(p)?;
                let bundle = FeatureBundle::new(
                    c.feature,
                    SliceSubjects {
                        prd: c.prd_id,
                        adr: c.adr_id,
                        plan: c.plan_id,
                        validation: c.validation_id,
                    },
                    PreparedPrd::from_document(&c.prd),
                    c.adr,
                    c.step,
                    c.repository,
                    c.base,
                    c.provider,
                    c.validation,
                )
                .map_err(|e| format!("InvalidBundle: {e}"))?;
                self.engine
                    .execute(request, move |tx| tx.import_feature_bundle(&bundle))
                    .map(receipt)
                    .map_err(rejected)
            }
            RequestKind::Start => {
                let c = protocol::StartCommand::parse(p)?;
                let start = StartRequest {
                    attempt: e.request,
                    subjects: SliceSubjects {
                        prd: c.prd,
                        adr: c.adr,
                        plan: c.plan,
                        validation: c.validation,
                    },
                    step: NodeRef {
                        id: c.step,
                        revision: Revision(1),
                    },
                };
                self.start(request, start)
            }
            RequestKind::RecoveryResolve => {
                let protocol::RecoveryResolveCommand { attempt, choice } =
                    protocol::RecoveryResolveCommand::parse(p, self.recovery.digest())?;
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
            RequestKind::Validate => {
                let protocol::ValidateCommand { attempt, candidate } =
                    protocol::ValidateCommand::parse(p)?;
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
            RequestKind::CandidateDecision => {
                let protocol::CandidateDecisionCommand {
                    attempt,
                    candidate,
                    verdict,
                } = protocol::CandidateDecisionCommand::parse(p)?;
                match verdict {
                    CandidateVerdict::Accept { receipt: reviewed } => {
                        let reviewed = EventSeq(reviewed);
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
                    CandidateVerdict::Reject => self
                        .engine
                        .execute(request, move |tx| {
                            tx.reject_candidate(attempt, &candidate, origin)?;
                            Ok("candidate rejected".into())
                        })
                        .map(receipt)
                        .map_err(rejected),
                }
            }
            RequestKind::ChatTurn => {
                let c = protocol::ChatTurnCommand::parse(p)?;
                self.chat_turn(request, e.request, c)
            }
            RequestKind::AnswerQuestion => {
                let protocol::AnswerQuestionCommand {
                    question,
                    revision,
                    action,
                } = protocol::AnswerQuestionCommand::parse(p)?;
                self.engine
                    .execute(request, move |tx| {
                        tx.answer_chat_question(question, revision, &action)
                    })
                    .map(receipt)
                    .map_err(rejected)
            }
            RequestKind::CancelTurn => {
                let protocol::CancelTurnCommand { turn } = protocol::CancelTurnCommand::parse(p)?;
                self.engine
                    .execute(request, move |tx| {
                        tx.request_turn_stop(turn)?;
                        Ok("stop committed; the turn ends with its result".into())
                    })
                    .map(receipt)
                    .map_err(rejected)
            }
            RequestKind::RecoveryStart => {
                protocol::reviewed_inspection(p, self.recovery.digest())?;
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
    fn review_query(&mut self, kind: RequestKind, e: &Envelope) -> Result<Json, String> {
        let p = &e.payload;
        if !kind.shaped(p) {
            return Err("InvalidPayload: unexpected or missing fields".into());
        }
        let attempt = protocol::attempt(p)?;
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
        match kind {
            RequestKind::Candidate => reader
                .read(|v| v.ipc_candidate(attempt))
                .map_err(|e| e.to_string()),
            RequestKind::CandidateChanges => {
                let protocol::CandidateChangesQuery {
                    candidate,
                    offset,
                    limit,
                    ..
                } = protocol::CandidateChangesQuery::parse(p)?;
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
                                .take(limit)
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
            RequestKind::CandidateDiff => {
                let protocol::CandidateDiffQuery {
                    candidate, offset, ..
                } = protocol::CandidateDiffQuery::parse(p)?;
                drop(reader);
                let (diff, complete) = candidate_diff(&self.engine, attempt, &candidate)
                    .map_err(|why| format!("CandidateUnavailable: {why}"))?;
                Ok(page(&diff, offset, complete))
            }
            _ => {
                let protocol::ValidationReportQuery {
                    receipt: event,
                    offset,
                    ..
                } = protocol::ValidationReportQuery::parse(p)?;
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
        let adapter = self.adapter(step.provider, &slice)?;
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
    /// The two production adapters. A provider whose executable the launcher did not
    /// configure cannot be launched by Start or by a turn.
    fn adapter(&self, provider: ProviderKind, slice: &SliceConfig) -> Result<Adapter, String> {
        match provider {
            ProviderKind::Codex => {
                let codex = slice
                    .codex
                    .clone()
                    .ok_or("NoProductionAdapter: Codex is not configured")?;
                CodexAdapter::new(codex)
                    .and_then(|adapter| adapter.ready().map(|()| adapter))
                    .map(Adapter::Codex)
                    .map_err(|e| format!("AdapterRejected: {e:?}"))
            }
            ProviderKind::ClaudeCode => {
                let claude = self
                    .claude
                    .clone()
                    .ok_or_else(|| format!("NoProductionAdapter: {provider:?}"))?;
                ClaudeAdapter::new(claude)
                    .and_then(|adapter| adapter.ready().map(|()| adapter))
                    .map(Adapter::Claude)
                    .map_err(|e| format!("AdapterRejected: {e:?}"))
            }
        }
    }
    /// M2: one lead agent turn. Accepted durably with its prompt, then run read-only in
    /// the registered repository through the configured adapter. A turn that cannot be
    /// launched after acceptance is sealed as failed at once, so it never holds capacity.
    fn chat_turn(
        &mut self,
        request: CommandRequest,
        turn: OpaqueId,
        c: protocol::ChatTurnCommand,
    ) -> Result<Json, String> {
        let with = |r: Receipt, dispatch: &str| {
            let Json::Object(mut fields) = receipt(r) else {
                unreachable!("receipt is an object")
            };
            fields.push(("turn".into(), identity(turn)));
            fields.push(("dispatch".into(), text(dispatch)));
            Json::Object(fields)
        };
        let rejected = |e: CommandError| match e {
            CommandError::Rejected(TxError::Rejected(why)) => why,
            other => format!("{other:?}"),
        };
        let slice = self.config.slice.clone().ok_or("SliceNotConfigured")?;
        if !self.workers.reap() {
            return Err("CapacityOne: a run is active".into());
        }
        let adapter = self.adapter(c.provider, &slice)?;
        let path = self
            .engine
            .reader()
            .and_then(|mut r| r.read(|v| v.local_repository_path(c.repository)))
            .map_err(|e| e.to_string())?
            .ok_or("TurnRefused: the repository is not registered")?;
        let repository = DisposableRoot::inspect(&slice.disposable_root)?.repository(&path)?;
        if repository.id() != c.repository {
            return Err("TurnRefused: repository identity changed".into());
        }
        let base = repository_head(&repository).map_err(|e| format!("TurnRefused: {e}"))?;
        let accepted = TurnRequest {
            turn,
            feature: c.feature,
            kind: c.kind,
            repository: c.repository,
            provider: c.provider,
            message: c.message,
        };
        let r = self
            .engine
            .execute(request, move |tx| tx.accept_turn(&accepted, &base))
            .map_err(rejected)?;
        if r.replayed {
            return Ok(with(r, "not repeated"));
        }
        self.finalized = false;
        match self.dispatch_turn(turn, &repository, &adapter) {
            Ok(()) => Ok(with(r, "launched")),
            Err(why) => {
                let detail = format!("not spawned: {why}");
                self.command("FinishLeadTurn", turn, move |tx| {
                    tx.finish_turn(turn, TurnOutcome::Failed, &detail, None, &[])
                })?;
                Err(format!(
                    "TurnFailed: {why}; command {:032x} is committed and nothing was launched",
                    turn.0
                ))
            }
        }
    }
    fn dispatch_turn(
        &mut self,
        turn: OpaqueId,
        repository: &LocalRepository,
        adapter: &Adapter,
    ) -> Result<(), String> {
        let spec = turn_spec(
            &self.engine,
            turn,
            repository,
            &adapter.turn_fingerprint(),
            adapter.variables(),
        )?;
        let launch = adapter.prepare_turn(&spec)?;
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
        let plan = StopPlan::new(adapter.capabilities(), StopBudgets::ACCEPTED);
        self.turn_stop = Arc::new(AtomicBool::new(false));
        self.workers.turn(
            WorkerRuntime {
                engine: self.engine.clone(),
                ids: self.ids.clone(),
                clock: self.clock.clone(),
            },
            spec,
            launch,
            plan,
            hooks,
            self.turn_stop.clone(),
        )
    }
    fn dispatch(&mut self, start: &StartRequest, adapter: &Adapter) -> Result<(), String> {
        provision_and_reserve(&self.ctx(), start)?;
        let prepared = slice_launch_spec(
            &self.engine,
            start,
            adapter.fingerprint(),
            adapter.variables(),
        )
        .and_then(|spec| adapter.prepare_launch(&spec).map(|launch| (spec, launch)));
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
        // Closing: an active validator is stopped; it records no verdict. A running lead
        // turn is stopped too and sealed as failed.
        self.validation_stop.store(true, Ordering::SeqCst);
        self.turn_stop.store(true, Ordering::SeqCst);
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
    RequestKind::named(&e.kind).is_some_and(|k| k.spec().control)
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

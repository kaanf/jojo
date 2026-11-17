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

mod commands;
mod queries;
mod runner;

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

/// What a worker thread does. M3 runs several at once: attempts up to the capacity, at
/// most one lead turn and at most one host validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Work {
    Attempt,
    Turn,
    Validation,
}

pub struct Workers {
    handles: Vec<(Work, JoinHandle<Result<(), String>>)>,
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
        self.handles.push((Work::Attempt, handle));
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
        self.handles.push((Work::Turn, handle));
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
        self.handles.push((Work::Validation, handle));
        Ok(())
    }
    pub fn reap(&mut self) -> bool {
        let mut i = 0;
        while i < self.handles.len() {
            if self.handles[i].1.is_finished() {
                let (_, h) = self.handles.swap_remove(i);
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
    /// Reaps finished threads, then counts the live ones of `work`.
    pub fn running(&mut self, work: Work) -> usize {
        self.reap();
        self.handles.iter().filter(|(w, _)| *w == work).count()
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
    /// D3: the serving helper computes executable digests on a background thread, so a
    /// Start or a turn never waits for one; it answers `ExecutableChecking` instead.
    /// Off in in-process tests, which hash in place (once per file identity).
    pub background_checks: bool,
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
    /// D3: turns background checks on and starts computing the digest of every configured
    /// executable, so that the first Start finds it known.
    pub fn begin_executable_checks(&mut self) {
        self.background_checks = true;
        let codex = self.config.slice.as_ref().and_then(|s| s.codex.as_ref());
        let paths = codex.map(|c| &c.executable).into_iter();
        for path in paths.chain(self.claude.as_ref().map(|c| &c.executable)) {
            let _ = jojo_storage::begin_check(path);
        }
    }
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
            background_checks: false,
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
            // M3: a run an earlier helper was running comes back paused; the human
            // resumes it after the recovery review.
            let running = service
                .engine
                .reader()
                .and_then(|mut r| r.read(|v| v.plans_running()))
                .map_err(|e| e.to_string())?;
            if running {
                service.command("PausePlanRuns", OpaqueId(0), |tx| {
                    tx.pause_running_plans("the helper started again")
                        .map(|n| format!("paused {n}"))
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
            return self.hello(e);
        }
        if self.session != Some(e.session) {
            return Err("HandshakeRequired".into());
        }
        if e.epoch != self.engine.epoch().0 {
            return Err("EpochMismatch".into());
        }
        let mut reader = self.engine.reader().map_err(|e| e.to_string())?;
        // A request newer than the frame's minor does not exist for that frame.
        let Some(kind) = named else {
            return Err("UnknownType".into());
        };
        match kind {
            RequestKind::Hello => unreachable!("answered above"),
            RequestKind::HumanDecision => {
                drop(reader);
                self.human_decision(kind, e)
            }
            RequestKind::PrepareClose
            | RequestKind::CancelClose
            | RequestKind::ConfirmClose
            | RequestKind::CancelAttempt => {
                drop(reader);
                self.close_command(kind, e)
            }
            // Protocol 1.1. A 1.0 frame naming these got the 1.0 answer above.
            RequestKind::RecoveryInspect => {
                drop(reader);
                self.recovery_inspect()
            }
            RequestKind::RegisterRepository
            | RequestKind::ImportBundle
            | RequestKind::Start
            | RequestKind::RecoveryResolve
            | RequestKind::RecoveryStart
            | RequestKind::Validate
            | RequestKind::CandidateDecision
            | RequestKind::ChatTurn
            | RequestKind::AnswerQuestion
            | RequestKind::CancelTurn
            | RequestKind::SaveProject
            | RequestKind::RunPlan
            | RequestKind::ControlPlan => {
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
            _ => self.query(kind, e, &mut reader),
        }
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
    let mut last_tick = Instant::now();
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
        // M3: the plan runner, between requests. A failed pass is retried next time.
        if !emergency && last_tick.elapsed() >= runner::TICK {
            last_tick = Instant::now();
            let _ = service.tick();
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
        for (_, h) in self.handles.drain(..) {
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

//! Mutations of the helper: the human decision, the close commands and the protocol 1.1
//! and 1.2 commands, one function each. Split out of `handle` and `slice_command`
//! (R-02, helper part); the order of every check is unchanged.
use super::*;

fn rejected(e: CommandError) -> String {
    match e {
        CommandError::Rejected(TxError::Rejected(why)) => why,
        other => format!("{other:?}"),
    }
}

impl Service {
    pub(super) fn human_decision(
        &mut self,
        kind: RequestKind,
        e: &Envelope,
    ) -> Result<Json, String> {
        let p = &e.payload;
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
        let decided = self
            .engine
            .human_decide(
                CommandId(e.request),
                HumanOrigin::TrustedUi {
                    actor: format!("stdio:{:032x}", e.session.0),
                },
                review,
                action,
            )
            .map_err(|e| format!("{e:?}"))?;
        // M2: an approved ADR lets a lead plan draft become its plan and
        // validation plan, both unapproved. Nothing runs when nothing waits.
        let pending = self
            .engine
            .reader()
            .and_then(|mut r| r.read(|v| v.lead_plans_pending()))
            .map_err(|e| e.to_string())?;
        let advanced = if pending && !decided.replayed {
            self.command("AdvanceLeadPlans", d.subject.0, |tx| {
                tx.advance_lead_plans().map(|n| format!("advanced {n}"))
            })
        } else {
            Ok(String::new())
        };
        // The decision is committed either way; a draft that could not become a
        // plan is reported beside it and stays as it was.
        let Json::Object(mut fields) = receipt(decided) else {
            unreachable!("receipt is an object")
        };
        if let Err(why) = advanced {
            fields.push(("lead_plan".into(), text(format!("PlanNotDrafted: {why}"))));
        }
        Ok(Json::Object(fields))
    }
    pub(super) fn close_command(
        &mut self,
        kind: RequestKind,
        e: &Envelope,
    ) -> Result<Json, String> {
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
        let request =
            CommandRequest::new(CommandId(e.request), self.engine.epoch(), &e.kind, payload);
        let origin = HumanOrigin::TrustedUi {
            actor: format!("stdio:{:032x}", e.session.0),
        };
        let command = CloseCommand::parse(kind, &e.payload)?;
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
    /// Protocol 1.1 mutations. Each commits under the client's command identity, so a
    /// repeated frame returns the original receipt and repeats nothing.
    pub(super) fn slice_command(
        &mut self,
        kind: RequestKind,
        e: &Envelope,
    ) -> Result<Json, String> {
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
        // A command that was already committed, in this or an earlier epoch, is answered
        // from its receipt alone: no check is re-run and nothing is dispatched again.
        let known = self
            .engine
            .reader()
            .and_then(|mut r| r.receipt(request.id))
            .map_err(|e| e.to_string())?;
        if known.is_some() {
            return self.replayed(kind, e, request);
        }
        match kind {
            RequestKind::RegisterRepository => self.register_repository(request, p),
            RequestKind::ImportBundle => self.import_bundle(request, p),
            RequestKind::Start => self.start_command(request, e),
            RequestKind::RecoveryResolve => self.recovery_resolve(request, p, origin),
            RequestKind::Validate => self.validate(request, e),
            RequestKind::CandidateDecision => self.candidate_decision(request, p, origin),
            RequestKind::ChatTurn => {
                let c = protocol::ChatTurnCommand::parse(p)?;
                self.chat_turn(request, e.request, c)
            }
            RequestKind::AnswerQuestion => self.answer_question(request, p),
            RequestKind::CancelTurn => self.cancel_turn(request, p),
            RequestKind::RecoveryStart => self.recovery_start(request, p, origin),
            RequestKind::SaveProject => self.save_project(request, p),
            RequestKind::RunPlan => {
                let protocol::PlanCommand { plan } = protocol::PlanCommand::parse(p)?;
                self.engine
                    .execute(request, move |tx| tx.run_plan(plan))
                    .map(receipt)
                    .map_err(rejected)
            }
            RequestKind::ControlPlan => {
                let protocol::ControlPlanCommand { plan, action } =
                    protocol::ControlPlanCommand::parse(p)?;
                let action = PlanControl::named(&action).ok_or("UnknownAction")?;
                self.engine
                    .execute(request, move |tx| tx.control_plan(plan, action))
                    .map(receipt)
                    .map_err(rejected)
            }
            _ => Err("UnknownType".into()),
        }
    }
    fn replayed(
        &mut self,
        kind: RequestKind,
        e: &Envelope,
        request: CommandRequest,
    ) -> Result<Json, String> {
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
        Ok(Json::Object(fields))
    }
    fn register_repository(&mut self, request: CommandRequest, p: &Json) -> Result<Json, String> {
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
    fn import_bundle(&mut self, request: CommandRequest, p: &Json) -> Result<Json, String> {
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
    fn start_command(&mut self, request: CommandRequest, e: &Envelope) -> Result<Json, String> {
        let c = protocol::StartCommand::parse(&e.payload)?;
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
    fn recovery_resolve(
        &mut self,
        request: CommandRequest,
        p: &Json,
        origin: HumanOrigin,
    ) -> Result<Json, String> {
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
    /// P2-03. The command names the sealed candidate digest the human is looking at;
    /// the validation command itself comes only from the approved plan.
    fn validate(&mut self, request: CommandRequest, e: &Envelope) -> Result<Json, String> {
        let protocol::ValidateCommand { attempt, candidate } =
            protocol::ValidateCommand::parse(&e.payload)?;
        self.start_validation(request, e.request, attempt, candidate)
    }
    /// One host validation run under the command `request`, the human's or the runner's.
    pub(super) fn start_validation(
        &mut self,
        request: CommandRequest,
        run: OpaqueId,
        attempt: OpaqueId,
        candidate: String,
    ) -> Result<Json, String> {
        // M3: attempts may run beside it; host validations run one at a time.
        if self.workers.running(Work::Validation) != 0 {
            return Err("ValidationBusy: another host validation is running".into());
        }
        let root = validation_root(&self.engine)?;
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
    fn candidate_decision(
        &mut self,
        request: CommandRequest,
        p: &Json,
        origin: HumanOrigin,
    ) -> Result<Json, String> {
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
                        tx.accept_reviewed_candidate(attempt, &candidate, reviewed, origin)?;
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
    fn answer_question(&mut self, request: CommandRequest, p: &Json) -> Result<Json, String> {
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
    fn cancel_turn(&mut self, request: CommandRequest, p: &Json) -> Result<Json, String> {
        let protocol::CancelTurnCommand { turn } = protocol::CancelTurnCommand::parse(p)?;
        self.engine
            .execute(request, move |tx| {
                tx.request_turn_stop(turn)?;
                Ok("stop committed; the turn ends with its result".into())
            })
            .map(receipt)
            .map_err(rejected)
    }
    /// M3 (protocol 1.3): a whole project record as its next revision. It also sets the
    /// helper's parallel capacity (D4).
    fn save_project(&mut self, request: CommandRequest, p: &Json) -> Result<Json, String> {
        let protocol::SaveProjectCommand {
            project,
            revision,
            record,
        } = protocol::SaveProjectCommand::parse(p)?;
        self.engine
            .execute(request, move |tx| {
                tx.save_project(project, revision, &record)
                    .map(|next| format!("project revision={next}"))
            })
            .map(receipt)
            .map_err(rejected)
    }
    fn recovery_start(
        &mut self,
        request: CommandRequest,
        p: &Json,
        origin: HumanOrigin,
    ) -> Result<Json, String> {
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
    /// One human Start: accept durably, then provision, reserve, bind and launch through
    /// the configured adapter. Every refusal that can be known beforehand happens before
    /// anything is committed.
    pub(super) fn start(
        &mut self,
        request: CommandRequest,
        start: StartRequest,
    ) -> Result<Json, String> {
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
        self.workers.reap();
        let snapshot = reader.snapshot().map_err(|e| e.to_string())?;
        if snapshot.session.admission != Admission::Open {
            return Err("AdmissionClosed".into());
        }
        self.reinspect()?;
        if !self.recovery.subjects().is_empty() {
            return Err("RecoveryHold: unresolved recovery subjects".into());
        }
        // M3: capacity is the core's (D4); it is checked here first, read-only, and again
        // when the Start is accepted.
        reader
            .read(|v| v.capacity_left("a Start"))
            .map_err(|e| e.to_string())??;
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
    /// The two production adapters. A provider the launcher did not configure cannot be
    /// launched by Start or by a turn. A pinned executable must be the profiled release
    /// with the pinned digest; a discovered one is found and checked again here (D3).
    fn adapter(&self, provider: ProviderKind, slice: &SliceConfig) -> Result<Adapter, String> {
        let pinned = |version: &str| {
            if version == profiled_version(provider) {
                Ok(())
            } else {
                Err("AdapterRejected: Version".to_owned())
            }
        };
        match provider {
            ProviderKind::Codex => {
                let codex = match slice.codex.clone() {
                    Some(codex) => {
                        pinned(&codex.version)?;
                        self.executable_known(provider, &codex.executable)?;
                        codex
                    }
                    None => {
                        let (found, unpinned) = self
                            .found(provider)?
                            .ok_or("NoProductionAdapter: Codex is not configured")?;
                        CodexAdapterConfig {
                            executable: found.path,
                            version: found.version,
                            sha256: found.sha256,
                            home: unpinned.home,
                            auth: AuthMode::NativeSubscription,
                        }
                    }
                };
                CodexAdapter::new(codex)
                    .and_then(|adapter| adapter.ready().map(|()| adapter))
                    .map(Adapter::Codex)
                    .map_err(|e| format!("AdapterRejected: {e:?}"))
            }
            ProviderKind::ClaudeCode => {
                let claude = match self.claude.clone() {
                    Some(claude) => {
                        pinned(&claude.version)?;
                        self.executable_known(provider, &claude.executable)?;
                        claude
                    }
                    None => {
                        let (found, unpinned) = self
                            .found(provider)?
                            .ok_or_else(|| format!("NoProductionAdapter: {provider:?}"))?;
                        ClaudeAdapterConfig {
                            executable: found.path,
                            version: found.version,
                            sha256: found.sha256,
                            home: unpinned.home,
                            user: unpinned.user,
                            auth: AuthMode::NativeSubscription,
                        }
                    }
                };
                ClaudeAdapter::new(claude)
                    .and_then(|adapter| adapter.ready().map(|()| adapter))
                    .map(Adapter::Claude)
                    .map_err(|e| format!("AdapterRejected: {e:?}"))
            }
        }
    }
    /// D3: the discovered executable of a provider the launcher did not pin, as it is now.
    /// `None` when the provider is not discovered at all.
    fn found(&self, provider: ProviderKind) -> Result<Option<(FoundExecutable, Unpinned)>, String> {
        let Some(discovered) = &self.discovered else {
            return Ok(None);
        };
        let (Some(lookup), Some(unpinned)) =
            (discovered.find(provider), discovered.provider(provider))
        else {
            return Ok(None);
        };
        match lookup {
            Lookup::Checking => Err(format!(
                "ExecutableChecking: the {provider:?} executable is being found and checked; try again shortly"
            )),
            Lookup::Refused(why) => Err(format!("ExecutableRefused: {provider:?}: {why}")),
            Lookup::Found(found) => Ok(Some((found, unpinned.clone()))),
        }
    }
    /// D3: with background checks on, an executable whose digest is not known yet is
    /// hashed off the control thread and the request is answered "not now".
    fn executable_known(&self, provider: ProviderKind, path: &Path) -> Result<(), String> {
        if self.background_checks
            && jojo_storage::begin_check(path) == jojo_storage::ExecutableCheck::Checking
        {
            return Err(format!(
                "ExecutableChecking: the {provider:?} executable's digest is being computed; try again shortly"
            ));
        }
        Ok(())
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
        // M3: the core decides capacity (D4) when it accepts the turn.
        if self.workers.running(Work::Turn) != 0 {
            return Err("TurnRefused: another lead turn is running".into());
        }
        self.engine
            .reader()
            .and_then(|mut r| r.read(|v| v.capacity_left("a lead turn")))
            .map_err(|e| e.to_string())??;
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
}

//! M3: the plan runner. One pass (`tick`) reads every running plan, starts the host
//! validation of a step whose attempt succeeded, and starts each ready step up to the
//! capacity. A step is started through the same `start` path as a human Start, under a
//! runtime command identity, so every check of a Start applies and the core decides the
//! capacity (D4). A step the core refuses is recorded once and left for the human.
//!
//! M4 (D2): a step that failed is either repaired, at most twice and always as a new
//! attempt whose prompt carries the failure, or held for the human; the pure policy in
//! `jojo_domain::repair` decides which. A step whose host validation passed is reviewed
//! once by the other provider before the human decides. Neither a repair nor a review is
//! ever an external action; nothing here pushes or opens a pull request.
use super::*;
use jojo_domain::provider::MAX_PROMPT_BYTES;
use jojo_domain::repair::{self, Failure, RepairDecision, RepairPolicy};
use jojo_domain::review::{ReviewInput, review_prompt};

/// How often the serve loop runs the runner.
pub const TICK: Duration = Duration::from_millis(250);

/// A refusal that says "not now" rather than "never": the step is tried again later.
fn transient(why: &str) -> bool {
    ["CapacityFull", "AdmissionClosed", "RecoveryHold", "Busy"]
        .iter()
        .any(|code| why.starts_with(code))
}

impl Service {
    /// One pass of the runner. Does nothing unless a plan run is RUNNING, admission is
    /// open and no recovery subject is left.
    pub fn tick(&mut self) -> Result<(), String> {
        if self.config.slice.is_none() {
            return Ok(());
        }
        self.workers.reap();
        let mut reader = self.engine.reader().map_err(|e| e.to_string())?;
        let mut runs = reader
            .read(|v| v.running_plans())
            .map_err(|e| e.to_string())?;
        if runs.is_empty() {
            return Ok(());
        }
        let admission = reader
            .snapshot()
            .map_err(|e| e.to_string())?
            .session
            .admission;
        drop(reader);
        if admission != Admission::Open || !self.recovery.subjects().is_empty() {
            return Ok(());
        }
        // Accepted steps go onto their plan branch first, one at a time and each on the
        // branch as the previous one left it, so that what depends on them can start in
        // this same pass.
        while let Some((run, step)) = runs
            .iter()
            .flat_map(|run| run.steps.iter().map(move |s| (run, s)))
            .filter(|(_, s)| s.state == StepState::Accepted)
            .min_by_key(|(_, s)| s.accepted)
            .map(|(run, s)| (run.clone(), s.clone()))
        {
            self.runner_integrate(&run, &step)?;
            runs = self.running_plans()?;
        }
        // M4: every failed step is repaired or held before anything else starts, so that a
        // repair is decided once and in plan order.
        let mut decided = false;
        for run in &runs {
            for step in run.steps.iter().filter(|s| s.state == StepState::Failed) {
                self.runner_decide(run, step)?;
                decided = true;
            }
        }
        if decided {
            runs = self.running_plans()?;
        }
        for run in &runs {
            for step in &run.steps {
                if step.state == StepState::Validating
                    && step.detail == "queued"
                    && self.workers.running(Work::Validation) == 0
                {
                    let (Some(attempt), Some(candidate)) = (step.attempt, step.candidate.clone())
                    else {
                        continue;
                    };
                    self.runner_validation(attempt, candidate)?;
                }
            }
        }
        for run in &runs {
            for step in &run.steps {
                if step.state == StepState::Reviewing
                    && step.detail == "queued"
                    && self.workers.running(Work::Review) == 0
                {
                    self.runner_review(run, step)?;
                }
            }
        }
        for run in &runs {
            // M4: a decided repair starts before new steps, under the identity recorded
            // with it, through the same Start.
            let repairs = run
                .steps
                .iter()
                .filter_map(|s| s.pending_repair.map(|a| (s.step, a)));
            let ready = run.ready().into_iter().map(|s| (s.step, None));
            let starts: Vec<(NodeRef, Option<OpaqueId>)> =
                repairs.map(|(s, a)| (s, Some(a))).chain(ready).collect();
            for (step, attempt) in starts {
                match self.runner_start(run, step, attempt) {
                    Ok(()) => {}
                    // D3: only this provider waits; a step of the other may still start.
                    Err(why) if why.starts_with("ExecutableChecking") => {}
                    Err(why) if transient(&why) => return Ok(()),
                    Err(why) => {
                        let (plan, revision, id) = (run.plan, run.revision, step.id);
                        self.command("RefusePlanStep", id, move |tx| {
                            tx.refuse_plan_step(plan, revision, id, &why)
                                .map(|()| "refused".into())
                        })?;
                    }
                }
            }
        }
        Ok(())
    }

    fn running_plans(&self) -> Result<Vec<PlanRunView>, String> {
        self.engine
            .reader()
            .and_then(|mut r| r.read(|v| v.running_plans()))
            .map_err(|e| e.to_string())
    }

    /// `attempt` is the identity a decided repair starts as; a first start takes a new one.
    fn runner_start(
        &mut self,
        run: &PlanRunView,
        step: NodeRef,
        attempt: Option<OpaqueId>,
    ) -> Result<(), String> {
        let attempt = match attempt {
            Some(attempt) => attempt,
            None => self.ids.next_id().map_err(|_| "IDs exhausted")?,
        };
        let request = CommandRequest::new(
            CommandId(attempt),
            self.engine.epoch(),
            "PlanStepStart",
            format!("{:032x}|{:032x}", run.plan.0.0, step.id.0),
        );
        let start = StartRequest {
            attempt,
            subjects: run.subjects,
            step,
        };
        self.start(request, start).map(drop)
    }

    /// M4: a failed step is repaired or held, as the pure policy decides with the
    /// project's limit; the decision is recorded before anything starts.
    fn runner_decide(&mut self, run: &PlanRunView, step: &RunStep) -> Result<(), String> {
        let (Some(failure), Some(failed)) = (step.failure.clone(), step.attempt) else {
            return Ok(());
        };
        let policy = RepairPolicy {
            limit: run.repair_limit,
        };
        let (plan, revision, id) = (run.plan, run.revision, step.step.id);
        match repair::decide(&failure, step.repairs, policy) {
            RepairDecision::Hold { reason } => self
                .command("HoldPlanStep", id, move |tx| {
                    tx.hold_plan_step(plan, revision, id, failed, &reason)
                })
                .map(drop),
            RepairDecision::Repair { ordinal, limit } => {
                let (failure, changed) = self.failure_evidence(failed, failure)?;
                let request = RepairRequest {
                    plan,
                    revision,
                    step: id,
                    failed,
                    attempt: self.ids.next_id().map_err(|_| "IDs exhausted")?,
                    ordinal,
                    limit,
                    failure: format!("{}: {}", step.detail, summary(&failure)),
                    section: repair::repair_section(ordinal, limit, &failure, &changed),
                };
                self.command("RepairPlanStep", id, move |tx| tx.record_repair(&request))
                    .map(drop)
            }
        }
    }

    /// The failure as the next attempt is told it: a failed validation with the output
    /// the host recorded, and the paths the failed attempt changed.
    fn failure_evidence(
        &self,
        attempt: OpaqueId,
        failure: Failure,
    ) -> Result<(Failure, Vec<String>), String> {
        let receipts = self
            .engine
            .reader()
            .and_then(|mut r| r.read(|v| v.validation_receipts(attempt)))
            .map_err(|e| e.to_string())?;
        let changed = receipts
            .last()
            .map(|r| r.changes.clone())
            .unwrap_or_default();
        let failure = match failure {
            Failure::ValidationFailed { reasons, .. } => {
                // A report that cannot be read is said to be missing, never invented.
                let output = receipts
                    .last()
                    .map(|r| match self.store.read(&r.report) {
                        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                        Err(why) => format!("(the validation report could not be read: {why})"),
                    })
                    .unwrap_or_default();
                Failure::ValidationFailed { reasons, output }
            }
            other => other,
        };
        Ok((failure, changed))
    }

    /// M4: the review of a step whose host validation passed, by the provider that did
    /// not do it. A review that cannot start for good holds the step.
    fn runner_review(&mut self, run: &PlanRunView, step: &RunStep) -> Result<(), String> {
        let (Some(attempt), Some(candidate)) = (step.attempt, step.candidate.clone()) else {
            return Ok(());
        };
        match self.start_review(run, step, attempt, &candidate) {
            Ok(()) => Ok(()),
            Err(why) if transient(&why) || why.starts_with("ExecutableChecking") => Ok(()),
            Err(why) => {
                let (plan, revision, id) = (run.plan, run.revision, step.step.id);
                let reason = format!("the review could not start: {why}");
                self.command("HoldPlanStep", id, move |tx| {
                    tx.hold_plan_step(plan, revision, id, attempt, &reason)
                })
                .map(drop)
            }
        }
    }

    fn start_review(
        &mut self,
        run: &PlanRunView,
        step: &RunStep,
        attempt: OpaqueId,
        candidate: &str,
    ) -> Result<(), String> {
        let slice = self.config.slice.clone().ok_or("SliceNotConfigured")?;
        let reviewer = reviewer_of(step.provider);
        let adapter = self.adapter(reviewer, &slice)?;
        let mut reader = self.engine.reader().map_err(|e| e.to_string())?;
        reader
            .read(|v| v.capacity_left("a review"))
            .map_err(|e| e.to_string())??;
        let material = reader
            .read(|v| v.review_material(&run.subjects, step.step))
            .map_err(|e| e.to_string())?
            .ok_or("ReviewRefused: the approved documents of the step are missing")?;
        let path = reader
            .read(|v| v.local_repository_path(run.repository))
            .map_err(|e| e.to_string())?
            .ok_or("ReviewRefused: the repository is not registered")?;
        drop(reader);
        let repository = DisposableRoot::inspect(&slice.disposable_root)?.repository(&path)?;
        if repository.id() != run.repository {
            return Err("ReviewRefused: repository identity changed".into());
        }
        let base = repository_head(&repository).map_err(|e| format!("ReviewRefused: {e}"))?;
        let (diff, _) = candidate_diff(&self.engine, attempt, candidate)
            .map_err(|e| format!("ReviewRefused: the diff could not be read: {e}"))?;
        let input = ReviewInput {
            key: &material.key,
            title: &material.title,
            prd_revision: material.prd_revision,
            prd: &material.prd,
            adr: &material.adr,
            write_scope: &material.write_scope,
            baseline: &material.baseline,
            command: &material.command,
            diff: &diff,
            repairs: step.repair.map_or(0, |(n, _)| n),
        };
        let review = self.ids.next_id().map_err(|_| "IDs exhausted")?;
        let request = ReviewRequest {
            review,
            plan: run.plan,
            revision: run.revision,
            step: step.step.id,
            attempt,
            provider: reviewer,
            repository: run.repository,
            base,
            prompt: review_prompt(&input, MAX_PROMPT_BYTES),
        };
        // D3: the review records the version and digest of the executable it will run.
        let fingerprint = adapter.fingerprint().clone();
        self.command("AcceptStepReview", review, move |tx| {
            let receipt = tx.accept_review(&request)?;
            tx.record_review_executable(review, &fingerprint)?;
            Ok(receipt)
        })?;
        self.finalized = false;
        if let Err(why) = self.dispatch_review(review, &repository, &adapter) {
            let detail = format!("not spawned: {why}");
            self.command("FinishStepReview", review, move |tx| {
                tx.finish_review(review, ReviewOutcome::Failed, &detail, None)
            })?;
        }
        Ok(())
    }

    fn dispatch_review(
        &mut self,
        review: OpaqueId,
        repository: &LocalRepository,
        adapter: &Adapter,
    ) -> Result<(), String> {
        let spec = review_spec(
            &self.engine,
            review,
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
        self.review_stop = Arc::new(AtomicBool::new(false));
        self.workers.review(
            WorkerRuntime {
                engine: self.engine.clone(),
                ids: self.ids.clone(),
                clock: self.clock.clone(),
            },
            spec,
            launch,
            plan,
            hooks,
            self.review_stop.clone(),
        )
    }

    /// One accepted step onto the plan branch, recorded once: as a commit or a conflict.
    fn runner_integrate(&mut self, run: &PlanRunView, step: &RunStep) -> Result<(), String> {
        let (Some(attempt), Some(candidate)) = (step.attempt, step.candidate.clone()) else {
            return Ok(());
        };
        let message = format!(
            "{}: {}\n\nJojo plan {:032x} revision {}, step {:032x}, attempt {:032x}, \
             candidate {candidate}",
            step.key, step.title, run.plan.0.0, run.revision.0, step.step.id.0, attempt.0
        );
        let name = format!(
            "{}-{:032x}",
            &run.branch.trim_start_matches("jojo/plan/")[..16],
            self.ids.next_id().map_err(|_| "IDs exhausted")?.0
        );
        // A failure that is not a clean conflict is recorded as one too: nothing moved,
        // and the human decides.
        let outcome = integrate_candidate(
            &self.engine,
            attempt,
            &candidate,
            &run.branch,
            &run.head,
            &message,
            &name,
        )
        .unwrap_or_else(|why| Integration::Conflict {
            detail: format!("integration failed: {why}"),
        });
        let (plan, revision, id, parent) = (run.plan, run.revision, step.step.id, run.head.clone());
        self.command("IntegratePlanStep", id, move |tx| {
            tx.record_integration(plan, revision, id, attempt, &parent, &outcome)
        })
        .map(drop)
    }

    fn runner_validation(&mut self, attempt: OpaqueId, candidate: String) -> Result<(), String> {
        let run = self.ids.next_id().map_err(|_| "IDs exhausted")?;
        let request = CommandRequest::new(
            CommandId(run),
            self.engine.epoch(),
            "PlanStepValidate",
            format!("{:032x}|{candidate}", attempt.0),
        );
        // A validation that cannot begin shows as the step's state; the pass goes on.
        let _ = self.start_validation(request, run, attempt, candidate);
        Ok(())
    }
}

/// One line about a failure for the repair record.
fn summary(failure: &Failure) -> String {
    let line = match failure {
        Failure::AttemptFailed { reason } => reason.clone(),
        Failure::ValidationFailed { reasons, .. } => reasons.join("; "),
        Failure::ReviewRejected { notes } => notes.clone(),
        other => format!("{other:?}"),
    };
    line.lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(512)
        .collect()
}

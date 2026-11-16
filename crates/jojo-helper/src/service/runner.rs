//! M3: the plan runner. One pass (`tick`) reads every running plan, starts the host
//! validation of a step whose attempt succeeded, and starts each ready step up to the
//! capacity. A step is started through the same `start` path as a human Start, under a
//! runtime command identity, so every check of a Start applies and the core decides the
//! capacity (D4). Nothing here retries: a step the core refuses is recorded once and left
//! for the human.
use super::*;

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
        let runs = reader
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
            for step in run.ready() {
                match self.runner_start(run, step.step) {
                    Ok(()) => {}
                    Err(why) if transient(&why) => return Ok(()),
                    Err(why) => {
                        let (plan, revision, id) = (run.plan, run.revision, step.step.id);
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

    fn runner_start(&mut self, run: &PlanRunView, step: NodeRef) -> Result<(), String> {
        let attempt = self.ids.next_id().map_err(|_| "IDs exhausted")?;
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

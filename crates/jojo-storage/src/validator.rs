//! Host validation runner (P2-03).
//!
//! Jojo verifies a candidate itself. For one requested run it: reads the approved
//! validation plan the attempt is bound to; derives the changed paths from the repository
//! (never from the agent); materializes the approved base plus the sealed candidate packet
//! in a new private directory under the data root; runs exactly the approved argument
//! vector there through the supervised process runner, with the network denied by the OS
//! sandbox, a cleared environment and a time and output budget; and records a receipt
//! bound to the candidate digest. The verdict is the command's exit status as observed by
//! this process. Nothing an agent printed is read.
//!
//! The command is not started when the change is outside the approved write scope or
//! touches a baseline path: such a candidate fails without its code being executed.
//! A run that cannot produce a verdict (tampered packet, worktree that no longer
//! reproduces the sealed digest, shutdown, unconfirmed stop) ends without a receipt and is
//! reported as UNKNOWN, never as a pass.
use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use jojo_domain::identity::OpaqueId;
use jojo_domain::stream::CODEX_EXEC_JSON;
use jojo_domain::time::Clock;
use jojo_domain::validation::{TestStatus, ValidationPlan, check_scope};

use crate::evidence::BlobFaults;
use crate::model::id_to_text;
use crate::outcomes::SettlementContext;
use crate::process::{
    CHECKPOINT_BYTES, ProcessLaunch, ProcessMessage, Signal, SpoolFaults, StreamLimits, Supervised,
};
use crate::validation::{ValidatorOrigin, ValidatorReport};
use crate::worktrees::{inspect_candidate_changes, materialize_validation_tree};
use crate::{Engine, sha256_hex};

pub const HOST_VALIDATOR: &str = "jojo-host-validator/1";
const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
/// Everything except the network. The same profile the verification gates run under.
pub const VALIDATION_SANDBOX_PROFILE: &str = "(version 1) (allow default) (deny network*)";
/// Bytes of each output stream kept in the report (the end of the stream).
const REPORT_TAIL_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidationBudget {
    pub timeout: Duration,
    pub terminate: Duration,
    pub kill: Duration,
    pub drain: Duration,
    pub max_output_bytes: u64,
}

impl ValidationBudget {
    pub const DEFAULT: Self = Self {
        timeout: Duration::from_secs(600),
        terminate: Duration::from_secs(20),
        kill: Duration::from_secs(5),
        drain: Duration::from_secs(5),
        max_output_bytes: 8 * 1024 * 1024,
    };
}

pub struct ValidatorConfig {
    /// Private directory under the data root; each run gets a new subdirectory.
    pub root: PathBuf,
    /// Test injection of the process inspector. Production: `None` (the system `ps`).
    pub inspector: Option<PathBuf>,
    /// P2-04.1: guard executable for the validation command's process group, so that
    /// candidate code cannot outlive the helper. Production: the helper's own executable.
    pub guard: Option<PathBuf>,
    pub budget: ValidationBudget,
    /// Budgets are measured with the injected clock's monotonic reading.
    pub clock: Arc<dyn Clock>,
    /// Set by the helper on shutdown: the validator is stopped and no verdict is recorded.
    pub stop: Arc<AtomicBool>,
}

/// `<data root>/validation`, created private on first use.
pub fn validation_root(engine: &Engine) -> Result<PathBuf, String> {
    let data = engine
        .db_path()
        .parent()
        .ok_or("database has no parent directory")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let root = data.join("validation");
    match fs::symlink_metadata(&root) {
        Ok(m) if m.is_dir() && m.permissions().mode() & 0o077 == 0 => {}
        Ok(_) => return Err("validation root is not a private directory".into()),
        Err(_) => fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .map_err(|e| e.to_string())?,
    }
    Ok(root)
}

#[derive(Debug)]
struct Execution {
    /// `None`: no pass or fail could be observed.
    passed: bool,
    detail: String,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn tail(path: &Path) -> Vec<u8> {
    let bytes = fs::read(path).unwrap_or_default();
    bytes[bytes.len().saturating_sub(REPORT_TAIL_BYTES)..].to_vec()
}

/// Runs the approved argument vector in `tree` and waits until its whole process tree is
/// gone. `Err` means no verdict: the run was interrupted or the stop was not confirmed.
fn execute(
    cfg: &ValidatorConfig,
    dir: &Path,
    tree: &Path,
    command: &[String],
) -> Result<Execution, String> {
    let private = |name: &str| -> Result<String, String> {
        let p = dir.join(name);
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&p)
            .map_err(|e| e.to_string())?;
        p.to_str()
            .map(str::to_owned)
            .ok_or_else(|| "non-UTF8 path".into())
    };
    let mut args = vec!["-p".to_owned(), VALIDATION_SANDBOX_PROFILE.to_owned()];
    args.extend(command.iter().cloned());
    let launch = ProcessLaunch {
        executable: SANDBOX_EXEC.into(),
        args,
        cwd: tree.to_owned(),
        // Nothing inherited: no credentials, tokens, proxies or user configuration.
        environment: vec![
            ("PATH".into(), "/usr/bin:/bin".into()),
            ("HOME".into(), private("home")?),
            ("TMPDIR".into(), private("tmp")?),
            ("LC_ALL".into(), "C".into()),
        ],
        stdin: Vec::new(),
        spool: dir.join("spool"),
        // Output is evidence only; the framing profile's events are discarded.
        parser: CODEX_EXEC_JSON.into(),
        limits: StreamLimits {
            max_frame_bytes: 64 * 1024,
            max_output_bytes: cfg.budget.max_output_bytes,
            checkpoint_bytes: CHECKPOINT_BYTES,
        },
    };
    let mut sup = Supervised::spawn_guarded(
        &launch,
        SpoolFaults::default(),
        cfg.inspector.as_deref().unwrap_or(Path::new("/bin/ps")),
        cfg.guard.as_deref(),
    )
    .map_err(|e| format!("validator was not started: {e:?}"))?;
    let now = || cfg.clock.now().monotonic_nanos;
    let after = |d: Duration| now().saturating_add(u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
    let deadline = after(cfg.budget.timeout);
    let steps = [
        (Signal::Terminate, cfg.budget.terminate),
        (Signal::Kill, cfg.budget.kill),
    ];
    let mut closed = [false, false];
    let mut exit = None;
    let mut root_terminated = false;
    let mut truncated = false;
    // Why the validator is being stopped; `fatal` causes produce no verdict.
    let mut cause: Option<(String, bool)> = None;
    let mut stopping: Option<(usize, u64)> = None;
    let mut drain: Option<u64> = None;
    loop {
        match sup.messages().recv_timeout(Duration::from_millis(10)) {
            Ok(ProcessMessage::Closed { stream, .. }) => {
                closed[usize::from(stream == jojo_domain::provider::Stream::Stderr)] = true;
            }
            Ok(ProcessMessage::Truncated { .. }) => truncated = true,
            Ok(ProcessMessage::SpoolFailed { detail, .. }) => {
                cause.get_or_insert((
                    format!("validator output could not be kept: {detail}"),
                    true,
                ));
            }
            Ok(ProcessMessage::RootTerminated) => root_terminated = true,
            Ok(ProcessMessage::InspectionFailed(detail)) => {
                cause.get_or_insert((format!("process inspection failed: {detail}"), true));
            }
            Ok(ProcessMessage::Exited(e)) => {
                exit.get_or_insert(e);
            }
            Ok(ProcessMessage::Output { .. }) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => closed = [true, true],
        }
        if exit.is_none() {
            exit = sup.root_exit();
        }
        if cause.is_none() {
            if cfg.stop.load(Ordering::SeqCst) {
                cause = Some(("validation interrupted by shutdown".into(), true));
            } else if now() >= deadline {
                cause = Some((
                    format!(
                        "validation command exceeded its {} ms budget and was stopped",
                        cfg.budget.timeout.as_millis()
                    ),
                    false,
                ));
            }
        }
        let streams_closed = closed == [true, true];
        if cause.is_some() {
            let (step, at) = stopping.get_or_insert((0, now()));
            if exit.is_some() && streams_closed && sup.tree().absent() {
                break;
            }
            if now() >= *at {
                let Some((signal, budget)) = steps.get(*step) else {
                    return Err(format!(
                        "{}; validator stop is unconfirmed after TERM and KILL",
                        cause.map(|c| c.0).unwrap_or_default()
                    ));
                };
                let _ = sup.discover();
                sup.signal_tree(*signal);
                *step += 1;
                *at = after(*budget);
            }
            continue;
        }
        if root_terminated || exit.is_some() {
            if drain.is_none() {
                if let Err(e) = sup.discover() {
                    cause = Some((format!("process inspection failed: {e}"), true));
                    continue;
                }
                let t = sup.tree();
                if !t.live_group.is_empty() || !t.live_known.is_empty() {
                    cause = Some((
                        "validation command left processes running; they were stopped".into(),
                        false,
                    ));
                    continue;
                }
                drain = Some(after(cfg.budget.drain));
            }
            if streams_closed {
                if sup.tree().absent() {
                    break;
                }
                if exit.is_some() {
                    cause = Some((
                        "validation command left processes running; they were stopped".into(),
                        false,
                    ));
                }
            } else if drain.is_some_and(|d| now() >= d) {
                return Err("validator output is held open by an unknown process".into());
            }
        }
    }
    let (stdout, stderr) = (
        tail(&sup.spool().join("stdout.raw")),
        tail(&sup.spool().join("stderr.raw")),
    );
    match cause {
        Some((why, true)) => Err(format!("{why}; the validator process tree is gone")),
        Some((why, false)) => Ok(Execution {
            passed: false,
            detail: why,
            stdout,
            stderr,
        }),
        None => {
            let (code, signal) = exit.map_or((None, None), |e| (e.code, e.signal));
            let passed = code == Some(0) && signal.is_none() && !truncated;
            Ok(Execution {
                passed,
                detail: if truncated {
                    "validation command output exceeded its budget".into()
                } else if let Some(s) = signal {
                    format!("validation command was killed by signal {s}")
                } else {
                    format!("validation command exited with code {}", code.unwrap_or(-1))
                },
                stdout,
                stderr,
            })
        }
    }
}

fn verdict(
    ctx: &SettlementContext<'_>,
    cfg: &ValidatorConfig,
    run: OpaqueId,
    attempt: OpaqueId,
    candidate: &str,
) -> Result<(), String> {
    let (digest, step, text) = ctx
        .read(|v| v.attempt_validation_plan(attempt))?
        .ok_or("approved validation plan is missing")?;
    if sha256_hex(&[text.as_bytes()]) != digest {
        return Err("validation plan bytes are not the approved bytes".into());
    }
    let plan = ValidationPlan::import(text.as_bytes())?;
    let entry = plan
        .step(step)
        .filter(|s| !s.command.is_empty())
        .ok_or("approved validation plan names no command for this step")?
        .clone();
    // Repository evidence first: the worktree must reproduce the sealed digest.
    let evidence = inspect_candidate_changes(ctx.engine, attempt, candidate)?;
    let scope = check_scope(evidence.changes(), &entry);
    let (results, detail, stdout, stderr) = if scope.passed() {
        let dir = cfg.root.join(id_to_text(run));
        let tree = materialize_validation_tree(ctx.engine, attempt, candidate, &dir)?;
        let done = execute(cfg, &dir, &tree, &entry.command)?;
        let status = if done.passed {
            TestStatus::Passed
        } else {
            TestStatus::Failed
        };
        (
            vec![(entry.tests[0].clone(), status)],
            done.detail,
            done.stdout,
            done.stderr,
        )
    } else {
        (
            Vec::new(),
            "validation command was not run: the change is outside the approved scope".to_owned(),
            Vec::new(),
            Vec::new(),
        )
    };
    // The candidate that was validated is still the sealed candidate.
    if inspect_candidate_changes(ctx.engine, attempt, candidate)? != evidence {
        return Err("candidate changed during validation".into());
    }
    let mut raw = format!(
        "jojo-validator-report/1\nvalidator: {HOST_VALIDATOR}\nattempt: {}\ncandidate: {candidate}\n\
         plan: {digest}\ncommand: {:?}\nnetwork: denied by OS sandbox\nenvironment: cleared\n\
         result: {detail}\n",
        id_to_text(attempt),
        entry.command,
    )
    .into_bytes();
    for (name, status) in &results {
        raw.extend(format!("test: {name} {status:?}\n").into_bytes());
    }
    for (name, bytes) in [("stdout", &stdout), ("stderr", &stderr)] {
        raw.extend(format!("--- {name} (last {} bytes)\n", bytes.len()).into_bytes());
        raw.extend_from_slice(bytes);
        if !bytes.ends_with(b"\n") && !bytes.is_empty() {
            raw.push(b'\n');
        }
    }
    let report = ValidatorReport {
        origin: ValidatorOrigin::HostRunner {
            name: HOST_VALIDATOR.into(),
        },
        attempt,
        candidate: candidate.to_owned(),
        plan_digest: digest,
        results,
        raw,
    };
    let blob = ctx.store.put(&report.raw, BlobFaults::default())?;
    // The receipt and the end of the run are one transaction.
    ctx.cmd("RecordValidation", attempt, move |tx| {
        let event = tx.record_validation(&plan, &evidence, &report, &blob)?;
        tx.end_validation(run, attempt, Some(event), &detail)?;
        Ok(event.0.to_string())
    })
    .map(drop)
}

/// Performs one requested run (see [`crate::Tx::begin_validation`]) to its durable end:
/// a receipt, or the recorded reason no verdict exists. `Err` only if that end itself
/// could not be committed.
pub fn run_validation(
    ctx: &SettlementContext<'_>,
    cfg: &ValidatorConfig,
    run: OpaqueId,
    attempt: OpaqueId,
    candidate: &str,
) -> Result<(), String> {
    match verdict(ctx, cfg, run, attempt, candidate) {
        Ok(()) => Ok(()),
        Err(why) => ctx
            .cmd("EndValidation", attempt, move |tx| {
                tx.end_validation(run, attempt, None, &why)?;
                Ok("ended without a verdict".into())
            })
            .map(drop),
    }
}

/// Ends a committed run that could not be handed to a runner thread.
pub fn abandon_validation(
    ctx: &SettlementContext<'_>,
    run: OpaqueId,
    attempt: OpaqueId,
    why: &str,
) -> Result<(), String> {
    let why = why.to_owned();
    ctx.cmd("EndValidation", attempt, move |tx| {
        tx.end_validation(run, attempt, None, &why)?;
        Ok("ended without a verdict".into())
    })
    .map(drop)
}

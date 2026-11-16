//! OS process supervision for one admitted launch (P1-09). No database access here.
//!
//! One `std::process` child per launch, in its own process group, with an explicit
//! executable, cwd, cleared environment plus allowlisted values, piped stdio and the
//! prompt on stdin. Every descriptor Rust or SQLite opens is close-on-exec, so the child
//! inherits only its three pipes (T09 verifies this from inside the child).
//!
//! Each output pipe has its own reader thread that writes raw bytes to an exclusive spool
//! file *before* framing/normalizing them, and reports only bytes it has synced. Readers
//! keep draining after a spool cap or spool failure (discarding), so a flood never wedges
//! the child on a full pipe while it is being stopped. Messages go through one bounded
//! channel; control (signals, inspection) never waits on that channel.
//!
//! The group leader is kept unreaped until no live group member remains. Its PID/PGID
//! therefore cannot be recycled while group signals are allowed. Inspection failures
//! produce UNKNOWN, never proof of absence.
//!
//! M0: real provider CLIs start their tool processes in process groups of their own
//! (observed in P2-05), which a group signal never reaches. A descendant that left the
//! group is signalled only if this supervisor discovered it through the parent chain, and
//! only through [`signal_escaped`]: freeze it, check PID and start time again, then
//! signal. A start-time check is not an OS lifetime handle, so this is best effort; an
//! identity that does not match is resumed and refused. Any other inspection-only PID is
//! never signalled.
//!
//! P2-04.1: the supervisor holding that handle can itself be killed. A launch may therefore
//! be given a *guard*: a second instance of the helper executable, started as a member of
//! the owned process group, whose standard input is a pipe only this process can write.
//! When this process ends for any reason the kernel closes the pipe, and the guard sends
//! TERM to the leader, waits briefly and sends KILL to the group it is a member of. Being
//! a member is what makes that safe: the group identity cannot be recycled while the guard
//! lives, and it verifies its own membership from the OS before signalling. A guard that
//! ends early (a TERM sent to the group ends it too) is replaced while the group lives;
//! one that cannot be kept is reported as an inspection failure, which stops the run.
//! M0: before that KILL the guard also stops the descendants that left the group and are
//! still reachable from the leader through the parent chain, with the same freeze-and-check.
//! One whose parent chain is already broken (the agent died first) is out of its reach.
use std::collections::BTreeSet;
use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use jojo_domain::planner::{self, TurnSpec};
use jojo_domain::provider::{EventPayload, LaunchSpec, ProviderError, ProviderKind, Stream};
use jojo_domain::stream::{LineFramer, StreamNormalizer};

pub const MAX_ARGS: usize = 256;
pub const MAX_ARG_BYTES: usize = 4096;
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_OUTPUT_BYTES: u64 = 256 * 1024 * 1024;
/// Unsynced bytes after which a reader syncs and reports a checkpoint without a record.
pub const CHECKPOINT_BYTES: u64 = 1024 * 1024;
const CHANNEL_DEPTH: usize = 8;
const CHUNK: usize = 64 * 1024;
const REAP_POLL: Duration = Duration::from_millis(20);
pub const MAX_KNOWN_DESCENDANTS: usize = 1024;
/// First argument of a guard process; the helper executable runs [`run_agent_guard`] for it.
pub const GUARD_ARGUMENT: &str = "--agent-guard";
/// Guards a supervisor starts for one group before it gives up and reports the loss.
pub const MAX_GUARD_REARMS: u32 = 16;
/// Lifeline byte: the group is confirmed stopped, leave without signalling.
const GUARD_RELEASE: u8 = b'R';
/// After TERM to the leader the guard waits at most this many 50 ms polls before KILL.
const GUARD_GRACE_POLLS: u32 = 40;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub ppid: u32,
    pub pgid: u32,
    /// `ps` start time (`lstart`, UTC); `UNKNOWN` if the process could not be inspected.
    pub start: String,
    pub boot: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExitObservation {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Signal {
    Interrupt,
    Terminate,
    Kill,
}

impl Signal {
    pub fn name(self) -> &'static str {
        match self {
            Self::Interrupt => "INT",
            Self::Terminate => "TERM",
            Self::Kill => "KILL",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SignalOutcome {
    Sent,
    /// Target identity no longer exists; nothing was signalled.
    Absent,
    /// Not signalled: the PID now belongs to a different process.
    RefusedReused,
    /// Not signalled: ownership could not be verified (or inspection failed).
    RefusedUnverified(String),
    Failed(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignalRecord {
    pub signal: Signal,
    /// `group:<pgid>` or `pid:<pid>`.
    pub target: String,
    pub outcome: SignalOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdentityCheck {
    Alive,
    Reused,
    Absent,
    Unknown(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StreamLimits {
    pub max_frame_bytes: usize,
    /// Per-stream spool cap; bytes beyond it are drained and discarded.
    pub max_output_bytes: u64,
    pub checkpoint_bytes: u64,
}

/// Test fault: the spool write that would pass this many bytes fails.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SpoolFaults {
    pub stdout_fail_after: Option<u64>,
    pub stderr_fail_after: Option<u64>,
}

/// Fully explicit OS request derived from a validated LaunchSpec.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessLaunch {
    pub executable: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub environment: Vec<(String, String)>,
    pub stdin: Vec<u8>,
    /// Exclusive per-generation spool directory, created at spawn.
    pub spool: PathBuf,
    pub parser: String,
    pub limits: StreamLimits,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchRejection {
    Spec(ProviderError),
    Argument(String),
    Environment(String),
    Executable(String),
    WorkingDirectory(String),
}

impl ProcessLaunch {
    /// Structural and filesystem checks done before any issue journal entry. Arguments
    /// come from the adapter's translation; environment values only for allowlisted names.
    pub fn from_spec(
        spec: &LaunchSpec,
        args: Vec<String>,
        environment: Vec<(String, String)>,
    ) -> Result<Self, LaunchRejection> {
        spec.validate().map_err(LaunchRejection::Spec)?;
        let normalizer = StreamNormalizer::for_parser(&spec.fingerprint.parser)
            .map_err(LaunchRejection::Spec)?;
        if normalizer.provider() != spec.fingerprint.provider {
            return Err(LaunchRejection::Spec(ProviderError::FingerprintMismatch));
        }
        // P2-01: a request for the production Codex profile is held to the Codex launch
        // contract here, so no caller can issue it with other arguments or bytes.
        if spec.fingerprint.parser == jojo_domain::codex::CODEX_EXEC_JSONL {
            crate::codex::verify_launch(spec, &args, &environment)?;
        }
        // M1: the same for the production Claude Code profile.
        if spec.fingerprint.parser == jojo_domain::claude::CLAUDE_PRINT_STREAM_JSON {
            crate::claude::verify_launch(spec, &args, &environment)?;
        }
        Self::assemble(
            &spec.fingerprint.executable,
            args,
            environment,
            &spec.environment_allowlist,
            &spec.cwd,
            spec.prompt.clone(),
            spec.raw_output_dir
                .join(format!("generation-{}", spec.dispatch_generation)),
            &spec.fingerprint.parser,
            spec.max_frame_bytes,
            spec.max_output_bytes,
        )
    }

    /// M2: the OS request of one lead agent turn. The same checks as [`Self::from_spec`],
    /// against the turn contract: the read-only argument vector of the spec's provider,
    /// its own variables, the approved executable bytes, and the repository as working
    /// directory. Nothing is journaled here and nothing is spawned.
    pub fn for_turn(
        spec: &TurnSpec,
        args: Vec<String>,
        environment: Vec<(String, String)>,
    ) -> Result<Self, LaunchRejection> {
        planner::check_turn(spec).map_err(LaunchRejection::Spec)?;
        let fingerprint = &spec.fingerprint;
        let normalizer =
            StreamNormalizer::for_parser(&fingerprint.parser).map_err(LaunchRejection::Spec)?;
        if normalizer.provider() != fingerprint.provider {
            return Err(LaunchRejection::Spec(ProviderError::FingerprintMismatch));
        }
        if planner::turn_argv(spec).as_ref() != Ok(&args) {
            return Err(LaunchRejection::Argument(
                "not the allowlisted lead agent argument vector".into(),
            ));
        }
        let home = planner::check_turn_environment(spec, &environment)
            .map_err(|_| LaunchRejection::Environment(spec.environment_allowlist.join(" ")))?;
        crate::codex::inspect_home(home).map_err(LaunchRejection::Environment)?;
        // Lexical separation was checked on the spec; repeat it on what the paths resolve to.
        for other in [&spec.repository, &spec.raw_output_dir] {
            if std::fs::canonicalize(other).is_ok_and(|real| home.starts_with(&real)) {
                return Err(LaunchRejection::Environment(
                    "home lies inside the repository or output directory".into(),
                ));
            }
        }
        let digest = crate::codex::inspect_executable(&fingerprint.executable)
            .map_err(LaunchRejection::Executable)?;
        let configured = match fingerprint.provider {
            ProviderKind::ClaudeCode => {
                jojo_domain::claude::configured_digest(&fingerprint.configuration)
            }
            ProviderKind::Codex => {
                jojo_domain::codex::configured_digest(&fingerprint.configuration)
            }
        };
        if configured != Some(digest.as_str()) {
            return Err(LaunchRejection::Executable(
                "digest differs from the approved fingerprint".into(),
            ));
        }
        Self::assemble(
            &fingerprint.executable,
            args,
            environment,
            &spec.environment_allowlist,
            &spec.repository,
            spec.prompt.clone(),
            spec.raw_output_dir.join("generation-1"),
            &fingerprint.parser,
            spec.max_frame_bytes,
            spec.max_output_bytes,
        )
    }

    /// The checks every request shares, and the request itself.
    #[allow(clippy::too_many_arguments)]
    fn assemble(
        exe: &Path,
        args: Vec<String>,
        environment: Vec<(String, String)>,
        allowlist: &[String],
        cwd: &Path,
        stdin: Vec<u8>,
        spool: PathBuf,
        parser: &str,
        max_frame_bytes: u64,
        max_output_bytes: u64,
    ) -> Result<Self, LaunchRejection> {
        if args.len() > MAX_ARGS
            || args
                .iter()
                .any(|a| a.len() > MAX_ARG_BYTES || a.contains('\0'))
        {
            return Err(LaunchRejection::Argument("argument limits".into()));
        }
        let mut names = BTreeSet::new();
        for (name, value) in &environment {
            if !allowlist.contains(name)
                || !names.insert(name)
                || value.len() > MAX_ARG_BYTES
                || value.contains('\0')
            {
                return Err(LaunchRejection::Environment(name.clone()));
            }
        }
        let meta = std::fs::metadata(exe)
            .map_err(|e| LaunchRejection::Executable(format!("{}: {e}", exe.display())))?;
        if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
            return Err(LaunchRejection::Executable("not an executable file".into()));
        }
        if !std::fs::metadata(cwd).is_ok_and(|m| m.is_dir()) {
            return Err(LaunchRejection::WorkingDirectory(cwd.display().to_string()));
        }
        if max_frame_bytes > MAX_FRAME_BYTES as u64 || max_output_bytes > MAX_OUTPUT_BYTES {
            return Err(LaunchRejection::Spec(ProviderError::Limit));
        }
        let frame = usize::try_from(max_frame_bytes)
            .map_err(|_| LaunchRejection::Spec(ProviderError::Limit))?;
        Ok(Self {
            executable: exe.to_owned(),
            args,
            cwd: cwd.to_owned(),
            environment,
            stdin,
            spool,
            parser: parser.to_owned(),
            limits: StreamLimits {
                max_frame_bytes: frame,
                max_output_bytes,
                checkpoint_bytes: CHECKPOINT_BYTES,
            },
        })
    }
}

/// A raw byte range of one stream plus the payload normalized from it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawEvent {
    pub stream: Stream,
    pub start: u64,
    pub end: u64,
    pub payload: EventPayload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProcessMessage {
    /// `durable` bytes of `stream` are written and synced; every event range ends at or
    /// before it.
    Output {
        stream: Stream,
        durable: u64,
        events: Vec<RawEvent>,
    },
    Truncated {
        stream: Stream,
        at: u64,
    },
    Closed {
        stream: Stream,
        durable: u64,
        discarded: u64,
    },
    SpoolFailed {
        stream: Stream,
        detail: String,
    },
    /// The root is a zombie, deliberately held until its group has stopped.
    RootTerminated,
    InspectionFailed(String),
    Exited(ExitObservation),
}

#[derive(Debug)]
pub enum SpawnFailure {
    /// Failed before any child could exist (spool creation, pipes not created).
    BeforeChild(String),
    /// `spawn` reported failure; whether a child briefly existed is not provable here.
    Uncertain(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TreeObservation {
    pub root_exit: Option<ExitObservation>,
    /// Live, non-zombie members of the owned process group.
    pub live_group: Vec<u32>,
    /// Known descendants outside the group that are still the same process.
    pub live_known: Vec<u32>,
    /// Known descendant PIDs now held by a different process.
    pub reused: Vec<u32>,
    pub error: Option<String>,
}

impl TreeObservation {
    pub fn absent(&self) -> bool {
        self.root_exit.is_some()
            && self.live_group.is_empty()
            && self.live_known.is_empty()
            && self.error.is_none()
    }
}

struct RootSlot {
    child: Child,
    exit: Option<ExitObservation>,
    guard: Option<Guard>,
}

/// The dead-man's switch of one owned group (P2-04.1). A guard is a child of this
/// supervisor placed *inside* the owned process group, holding the read end of a pipe
/// whose only write end is here. If this process dies, however it dies, the kernel closes
/// that pipe and the guard stops the group it is a member of. Its own membership keeps
/// the group identity from being recycled, so it never signals on an inspected number.
struct Guard {
    executable: PathBuf,
    armed: Option<(Child, ChildStdin)>,
    /// Earlier guards, kept unreaped so that every guard PID stays ours until the end.
    retired: Vec<Child>,
    /// Every guard ever started for this group: never an agent process.
    pids: Vec<u32>,
    rearms: u32,
    reported: bool,
}

/// Starts one guard as a member of the group led by `leader`. The caller holds the
/// unreaped leader, so that group is still the one it created.
fn arm(guard: &Path, ps: &Path, leader: u32) -> Result<(Child, ChildStdin), String> {
    let group = i32::try_from(leader).map_err(|_| "leader identity out of range".to_owned())?;
    let mut child = Command::new(guard)
        .arg(GUARD_ARGUMENT)
        .arg(leader.to_string())
        .arg(ps)
        .current_dir("/")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(group)
        .spawn()
        .map_err(|e| format!("agent guard: {e}"))?;
    match child.stdin.take() {
        Some(lifeline) => Ok((child, lifeline)),
        None => {
            let _ = child.kill();
            let _ = child.wait();
            Err("agent guard: no lifeline".into())
        }
    }
}

impl Guard {
    /// Waits briefly for every guard to end, kills what remains through its own direct
    /// child handle, and reaps them all.
    fn reap(&mut self) {
        if let Some((child, lifeline)) = self.armed.take() {
            drop(lifeline);
            self.retired.push(child);
        }
        for mut child in self.retired.drain(..) {
            let mut gone = false;
            for _ in 0..100 {
                if !matches!(child.try_wait(), Ok(None)) {
                    gone = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            if !gone {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }

    /// The group is confirmed stopped: the armed guard leaves without signalling.
    fn release(&mut self) {
        if let Some((_, lifeline)) = self.armed.as_mut() {
            let _ = lifeline.write_all(&[GUARD_RELEASE]);
        }
        self.reap();
    }

    /// Replaces a guard that has ended while the group still has something to guard
    /// (a TERM or INT sent to the group ends the guard too). Returns a failure to report
    /// once, when no guard can be kept alive.
    fn upkeep(&mut self, ps: &Path, leader: u32, group_alive: bool) -> Option<String> {
        if let Some((child, _)) = self.armed.as_mut() {
            if matches!(child.try_wait(), Ok(None)) {
                return None;
            }
            if let Some((child, lifeline)) = self.armed.take() {
                drop(lifeline);
                self.retired.push(child);
            }
        }
        if !group_alive {
            return None;
        }
        if self.rearms < MAX_GUARD_REARMS {
            self.rearms += 1;
            match arm(&self.executable, ps, leader) {
                Ok((child, lifeline)) => {
                    self.pids.push(child.id());
                    self.armed = Some((child, lifeline));
                    return None;
                }
                Err(_) => self.rearms = MAX_GUARD_REARMS,
            }
        }
        if self.reported {
            return None;
        }
        self.reported = true;
        Some("agent guard lost and could not be re-armed".into())
    }
}

/// Body of a guard process (`<helper> --agent-guard <leader> <ps>`), started by
/// [`arm`] inside the group led by `leader` with the lifeline on standard input.
///
/// It first proves, from the OS, that it really is a member of that group, and signals
/// nothing otherwise. While it lives, that group cannot be recycled and no new process
/// can be given the leader's PID, so both targets below are the owned group or nothing.
/// End-of-file without the release byte means the supervisor is gone: TERM to the leader,
/// a bounded wait, then KILL to the whole group, which includes this process.
pub fn run_agent_guard(leader: u32, ps: &Path, mut lifeline: impl Read) -> u8 {
    let member = |ps: &Path| -> bool {
        let me = std::process::id().to_string();
        ps.to_str()
            .ok_or_else(|| "invalid inspector path".to_owned())
            .and_then(|ps| system(ps, &["-o", "pgid=", "-p", &me]))
            .is_ok_and(|raw| String::from_utf8_lossy(&raw).trim().parse() == Ok(leader))
    };
    if leader < 2 || !member(ps) {
        return 3;
    }
    let mut released = false;
    let mut buf = [0u8; 16];
    loop {
        match lifeline.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => released |= buf[..n].contains(&GUARD_RELEASE),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    if released {
        return 0;
    }
    if !member(ps) {
        return 3;
    }
    // Taken while the leader is still there, so that the parent chain is intact.
    let mut escaped = snapshot(ps).map_or(Vec::new(), |rows| escaped_from(&rows, leader));
    let leader_pid = leader.to_string();
    let _ = kill(Signal::Terminate, &leader_pid);
    for _ in 0..GUARD_GRACE_POLLS {
        if system("/bin/kill", &["-s", "0", "--", &leader_pid]).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // The agent may have stopped its own tools on TERM; whatever is left is stopped here,
    // before the group KILL that also ends this process.
    if let Ok(rows) = snapshot(ps) {
        for found in escaped_from(&rows, leader) {
            if !escaped.contains(&found) {
                escaped.push(found);
            }
        }
        for (pid, start) in escaped {
            let identity = ProcessIdentity {
                pid,
                ppid: 0,
                pgid: 0,
                start,
                boot: String::new(),
            };
            if check(&rows, &identity) == IdentityCheck::Alive {
                let _ = signal_escaped(ps, &identity, Signal::Kill);
            }
        }
    }
    let _ = kill(Signal::Kill, &format!("-{leader}"));
    4
}

pub struct Supervised {
    root: ProcessIdentity,
    slot: Arc<Mutex<RootSlot>>,
    known: Vec<ProcessIdentity>,
    messages: Receiver<ProcessMessage>,
    spool: PathBuf,
    ps: PathBuf,
    abandon: Arc<AtomicBool>,
}

#[derive(Clone, Debug)]
struct Row {
    pid: u32,
    ppid: u32,
    pgid: u32,
    zombie: bool,
    start: String,
}

const SYSTEM_OUTPUT_BYTES: u64 = 4 * 1024 * 1024;

fn system(program: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let mut child = Command::new(program)
        .env_clear()
        .envs([("LC_ALL", "C"), ("TZ", "UTC")])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    let (out_tx, out_rx) = sync_channel(1);
    let (err_tx, err_rx) = sync_channel(1);
    let stdout = child.stdout.take().ok_or("missing inspector stdout")?;
    let stderr = child.stderr.take().ok_or("missing inspector stderr")?;
    let out_thread = std::thread::Builder::new()
        .name("jojo-os-stdout".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            let read = stdout.take(SYSTEM_OUTPUT_BYTES + 1).read_to_end(&mut bytes);
            let _ = out_tx.send(read.map(|_| bytes));
        });
    let err_thread = std::thread::Builder::new()
        .name("jojo-os-stderr".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            let read = stderr.take(4097).read_to_end(&mut bytes);
            let _ = err_tx.send(read.map(|_| bytes));
        });
    if out_thread.is_err() || err_thread.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err("system reader thread unavailable".into());
    }
    let mut status = None;
    for _ in 0..200 {
        match child.try_wait() {
            Ok(Some(exit)) => {
                status = Some(exit);
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("system wait: {e}"));
            }
        }
    }
    let Some(status) = status else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("{program}: inspection deadline exceeded"));
    };
    let stdout = out_rx
        .recv_timeout(Duration::from_millis(100))
        .map_err(|_| "system stdout deadline")?
        .map_err(|e| format!("system stdout: {e}"))?;
    let stderr = err_rx
        .recv_timeout(Duration::from_millis(100))
        .map_err(|_| "system stderr deadline")?
        .map_err(|e| format!("system stderr: {e}"))?;
    if !status.success() {
        return Err(format!(
            "{program}: {}",
            String::from_utf8_lossy(&stderr).trim()
        ));
    }
    if stdout.len() as u64 > SYSTEM_OUTPUT_BYTES || stderr.len() > 4096 {
        return Err("system output limit exceeded".into());
    }
    Ok(stdout)
}

fn boot_identity() -> Result<String, String> {
    static BOOT: OnceLock<Result<String, String>> = OnceLock::new();
    BOOT.get_or_init(|| {
        let raw = system("/usr/sbin/sysctl", &["-n", "kern.boottime"])?;
        Ok(String::from_utf8_lossy(&raw).trim().to_owned())
    })
    .clone()
}

fn snapshot(ps: &Path) -> Result<Vec<Row>, String> {
    let raw = system(
        ps.to_str().ok_or("invalid inspector path")?,
        &["-axo", "pid=,ppid=,pgid=,stat=,lstart="],
    )?;
    let text = String::from_utf8(raw).map_err(|_| "ps output is not UTF-8".to_owned())?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            let num = |i: usize| -> Result<u32, String> {
                f.get(i)
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| format!("unparsable ps row: {line}"))
            };
            if f.len() != 9 {
                return Err(format!("unparsable ps row: {line}"));
            }
            Ok(Row {
                pid: num(0)?,
                ppid: num(1)?,
                pgid: num(2)?,
                zombie: f[3].starts_with('Z'),
                start: f[4..].join(" "),
            })
        })
        .collect()
}

fn kill(signal: Signal, target: &str) -> SignalOutcome {
    match system("/bin/kill", &["-s", signal.name(), "--", target]) {
        Ok(_) => SignalOutcome::Sent,
        Err(e) if e.contains("No such process") => SignalOutcome::Absent,
        Err(e) => SignalOutcome::Failed(e),
    }
}

fn check(rows: &[Row], identity: &ProcessIdentity) -> IdentityCheck {
    if identity.start == "UNKNOWN" {
        return IdentityCheck::Unknown("process start identity was not observed".into());
    }
    match rows.iter().find(|r| r.pid == identity.pid) {
        None => IdentityCheck::Absent,
        Some(r) if r.start != identity.start => IdentityCheck::Reused,
        Some(r) if r.zombie => IdentityCheck::Absent,
        Some(_) => IdentityCheck::Alive,
    }
}

/// Signals one descendant that left the owned group (M0). The caller has just seen the
/// identity alive. STOP comes first: a stopped process does not exit on its own, so its
/// PID is not handed to another program between the check below and the signal. If the
/// second check does not match, the process is resumed and nothing else is sent. A
/// descendant that leads a group of its own is signalled as that group.
fn signal_escaped(ps: &Path, identity: &ProcessIdentity, signal: Signal) -> SignalRecord {
    let pid = identity.pid.to_string();
    let record = |target: String, outcome| SignalRecord {
        signal,
        target,
        outcome,
    };
    let own = format!("pid:{pid}");
    match system("/bin/kill", &["-s", "STOP", "--", &pid]) {
        Ok(_) => {}
        Err(e) if e.contains("No such process") => return record(own, SignalOutcome::Absent),
        Err(e) => return record(own, SignalOutcome::Failed(e)),
    }
    let resume = |target: &str| {
        let _ = system("/bin/kill", &["-s", "CONT", "--", target]);
    };
    let rows = match snapshot(ps) {
        Ok(rows) => rows,
        Err(e) => {
            resume(&pid);
            return record(own, SignalOutcome::RefusedUnverified(e));
        }
    };
    match check(&rows, identity) {
        IdentityCheck::Alive => {}
        IdentityCheck::Absent => return record(own, SignalOutcome::Absent),
        IdentityCheck::Reused => {
            resume(&pid);
            return record(own, SignalOutcome::RefusedReused);
        }
        IdentityCheck::Unknown(e) => {
            resume(&pid);
            return record(own, SignalOutcome::RefusedUnverified(e));
        }
    }
    let leads = rows
        .iter()
        .any(|r| r.pid == identity.pid && r.pgid == identity.pid);
    let (target, label) = if leads {
        (format!("-{pid}"), format!("group:{pid}"))
    } else {
        (pid.clone(), own)
    };
    let outcome = kill(signal, &target);
    if signal != Signal::Kill {
        resume(&target);
    }
    record(label, outcome)
}

/// Live processes outside the group led by `leader` that descend from it through the
/// parent chain, as `(pid, start)`. Used by the guard, which has no registry of its own.
fn escaped_from(rows: &[Row], leader: u32) -> Vec<(u32, String)> {
    let mut chain: BTreeSet<u32> = BTreeSet::from([leader]);
    loop {
        let before = chain.len();
        for r in rows {
            if r.pgid == leader || chain.contains(&r.ppid) {
                chain.insert(r.pid);
            }
        }
        if chain.len() == before {
            break;
        }
    }
    rows.iter()
        .filter(|r| chain.contains(&r.pid) && r.pgid != leader && !r.zombie)
        .map(|r| (r.pid, r.start.clone()))
        .collect()
}

/// Fresh inspection of one recorded identity, including the boot it belongs to.
pub fn verify_identity(identity: &ProcessIdentity) -> IdentityCheck {
    match boot_identity() {
        Ok(boot) if boot != identity.boot => return IdentityCheck::Absent,
        Err(e) => return IdentityCheck::Unknown(e),
        Ok(_) => {}
    }
    match snapshot(Path::new("/bin/ps")) {
        Ok(rows) => check(&rows, identity),
        Err(e) => IdentityCheck::Unknown(e),
    }
}

/// Reopen inspection of identities recorded by an earlier helper (P1-11), from one
/// snapshot of the explicit observer. A recorded process group with any live member
/// (a survivor or a recycled PGID; indistinguishable) is `Unknown`, never absent. A
/// different boot proves absence. Nothing is signalled.
pub fn inspect_recorded(ps: &Path, recorded: &[ProcessIdentity]) -> Vec<(String, IdentityCheck)> {
    let mut groups: Vec<(u32, &str)> = recorded.iter().map(|p| (p.pgid, p.boot.as_str())).collect();
    groups.sort_unstable();
    groups.dedup();
    let boot = boot_identity();
    let rows = snapshot(ps);
    let label = |p: &ProcessIdentity| format!("pid:{} start:{}", p.pid, p.start);
    let mut found = Vec::new();
    for p in recorded {
        let check = match (&boot, &rows) {
            (Err(e), _) => IdentityCheck::Unknown(e.clone()),
            (Ok(b), _) if *b != p.boot => IdentityCheck::Absent,
            (_, Err(e)) => IdentityCheck::Unknown(e.clone()),
            (_, Ok(rows)) => check(rows, p),
        };
        found.push((label(p), check));
    }
    for (pgid, group_boot) in groups {
        let check = match (&boot, &rows) {
            (Err(e), _) => IdentityCheck::Unknown(e.clone()),
            (Ok(b), _) if b != group_boot => IdentityCheck::Absent,
            (_, Err(e)) => IdentityCheck::Unknown(e.clone()),
            (_, Ok(rows)) if rows.iter().any(|r| r.pgid == pgid && !r.zombie) => {
                IdentityCheck::Unknown("recorded process group has live members".into())
            }
            _ => IdentityCheck::Absent,
        };
        found.push((format!("group:{pgid}"), check));
    }
    found
}

/// Inspection is not a lifetime handle. Never signal a bare PID, even on a match.
/// Only `Supervised::signal_tree` may signal, while holding its unreaped leader.
pub fn signal_verified(identity: &ProcessIdentity, signal: Signal) -> SignalRecord {
    let outcome = match verify_identity(identity) {
        IdentityCheck::Alive => {
            SignalOutcome::RefusedUnverified("no retained process lifetime handle".into())
        }
        IdentityCheck::Absent => SignalOutcome::Absent,
        IdentityCheck::Reused => SignalOutcome::RefusedReused,
        IdentityCheck::Unknown(e) => SignalOutcome::RefusedUnverified(e),
    };
    SignalRecord {
        signal,
        target: format!("pid:{}", identity.pid),
        outcome,
    }
}

fn private_dir(path: &Path) -> std::io::Result<()> {
    DirBuilder::new().recursive(false).mode(0o700).create(path)
}

fn spool_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

struct Pump {
    stream: Stream,
    file: Option<File>,
    limits: StreamLimits,
    fail_after: Option<u64>,
    normalizer: Option<(LineFramer, StreamNormalizer)>,
    tx: SyncSender<ProcessMessage>,
}

impl Pump {
    fn run(mut self, mut pipe: impl Read) {
        let (mut written, mut durable, mut discarded) = (0u64, 0u64, 0u64);
        let mut truncated = false;
        let mut buf = vec![0u8; CHUNK];
        loop {
            let n = match pipe.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    self.fail(format!("pipe read: {e}"));
                    break;
                }
            };
            let room = self.limits.max_output_bytes.saturating_sub(written);
            let keep = usize::try_from(room).map_or(n, |r| r.min(n));
            discarded += (n - keep) as u64;
            let mut events = Vec::new();
            if keep > 0 && self.file.is_some() {
                if let Err(e) = self.write(&buf[..keep], written) {
                    self.fail(e);
                    discarded += keep as u64;
                    continue;
                }
                written += keep as u64;
                if let Some((framer, normalizer)) = &mut self.normalizer {
                    for frame in framer.push(&buf[..keep]) {
                        for payload in normalizer.frame(&frame) {
                            events.push(RawEvent {
                                stream: self.stream,
                                start: frame.start,
                                end: frame.end,
                                payload,
                            });
                        }
                    }
                }
            } else if keep > 0 {
                discarded += keep as u64;
            }
            if keep < n && !truncated && self.file.is_some() {
                truncated = true;
                if let Some((_, normalizer)) = &mut self.normalizer {
                    for payload in normalizer.output_truncated() {
                        events.push(RawEvent {
                            stream: self.stream,
                            start: written,
                            end: written,
                            payload,
                        });
                    }
                } else {
                    events.push(RawEvent {
                        stream: self.stream,
                        start: written,
                        end: written,
                        payload: EventPayload::StreamTruncated,
                    });
                }
                let _ = self.tx.send(ProcessMessage::Truncated {
                    stream: self.stream,
                    at: written,
                });
            }
            if (!events.is_empty() || written - durable >= self.limits.checkpoint_bytes)
                && written > durable
            {
                match self.sync() {
                    Ok(()) => durable = written,
                    Err(e) => {
                        self.fail(e);
                        continue;
                    }
                }
            }
            let report = !events.is_empty() || durable == written && written > 0 && keep > 0;
            if report
                && self
                    .tx
                    .send(ProcessMessage::Output {
                        stream: self.stream,
                        durable,
                        events,
                    })
                    .is_err()
            {
                // Receiver gone: keep draining so the child is never blocked on us.
                self.file = None;
            }
        }
        let mut events = Vec::new();
        if self.file.is_some()
            && let Some((framer, normalizer)) = &mut self.normalizer
            && let Some(frame) = framer.finish()
        {
            for payload in normalizer.frame(&frame) {
                events.push(RawEvent {
                    stream: self.stream,
                    start: frame.start,
                    end: frame.end,
                    payload,
                });
            }
        }
        let final_checkpoint_changed = written > durable;
        if self.file.is_some() && final_checkpoint_changed {
            match self.sync() {
                Ok(()) => durable = written,
                Err(e) => self.fail(e),
            }
        }
        // A quiet stream still has durable raw evidence. Publish its final offset
        // even when no NDJSON payload was emitted (e.g. small stderr or blank lines).
        if (!events.is_empty() || final_checkpoint_changed) && self.file.is_some() {
            let _ = self.tx.send(ProcessMessage::Output {
                stream: self.stream,
                durable,
                events,
            });
        }
        let _ = self.tx.send(ProcessMessage::Closed {
            stream: self.stream,
            durable,
            discarded,
        });
    }

    fn write(&mut self, bytes: &[u8], written: u64) -> Result<(), String> {
        if self
            .fail_after
            .is_some_and(|limit| written + bytes.len() as u64 > limit)
        {
            return Err("injected spool write failure".into());
        }
        let file = self.file.as_mut().ok_or("spool closed")?;
        file.write_all(bytes)
            .map_err(|e| format!("spool write: {e}"))
    }

    fn sync(&mut self) -> Result<(), String> {
        let file = self.file.as_mut().ok_or("spool closed")?;
        file.sync_data().map_err(|e| format!("spool sync: {e}"))
    }

    fn fail(&mut self, detail: String) {
        if self.file.take().is_some() {
            let _ = self.tx.send(ProcessMessage::SpoolFailed {
                stream: self.stream,
                detail,
            });
        }
    }
}

impl Supervised {
    /// Spawns outside any database transaction; the caller has already journaled the
    /// issue boundary. Returns once the root identity has been inspected.
    pub fn spawn(launch: &ProcessLaunch, faults: SpoolFaults) -> Result<Self, SpawnFailure> {
        Self::spawn_with_inspector(launch, faults, Path::new("/bin/ps"))
    }

    /// Explicit observer injection for deterministic fixtures. Production uses `spawn`.
    /// The verifier supplies a private non-setuid copy of the host ps with an ad-hoc
    /// signature; this keeps the entire test process inside OS network denial.
    pub fn spawn_with_inspector(
        launch: &ProcessLaunch,
        faults: SpoolFaults,
        ps: &Path,
    ) -> Result<Self, SpawnFailure> {
        Self::spawn_guarded(launch, faults, ps, None)
    }

    /// As [`Self::spawn_with_inspector`], with a guard executable (the helper itself)
    /// armed inside the new group before anything else happens. A launch whose guard
    /// cannot be armed and verified is killed at once and reported uncertain.
    pub fn spawn_guarded(
        launch: &ProcessLaunch,
        faults: SpoolFaults,
        ps: &Path,
        guard: Option<&Path>,
    ) -> Result<Self, SpawnFailure> {
        if launch.limits.max_frame_bytes == 0
            || launch.limits.max_frame_bytes > MAX_FRAME_BYTES
            || launch.limits.max_output_bytes < launch.limits.max_frame_bytes as u64
            || launch.limits.max_output_bytes > MAX_OUTPUT_BYTES
            || launch.limits.checkpoint_bytes == 0
            || launch.stdin.len() > jojo_domain::provider::MAX_PROMPT_BYTES
        {
            return Err(SpawnFailure::BeforeChild(
                "process stream/prompt limits".into(),
            ));
        }
        let normalizer = StreamNormalizer::for_parser(&launch.parser)
            .map_err(|e| SpawnFailure::BeforeChild(format!("parser: {e:?}")))?;
        private_dir(&launch.spool)
            .map_err(|e| SpawnFailure::BeforeChild(format!("spool directory: {e}")))?;
        let stdout_file = spool_file(&launch.spool.join("stdout.raw"))
            .map_err(|e| SpawnFailure::BeforeChild(format!("stdout spool: {e}")))?;
        let stderr_file = spool_file(&launch.spool.join("stderr.raw"))
            .map_err(|e| SpawnFailure::BeforeChild(format!("stderr spool: {e}")))?;
        File::open(&launch.spool)
            .and_then(|f| f.sync_all())
            .and_then(|()| {
                File::open(
                    launch
                        .spool
                        .parent()
                        .ok_or_else(|| std::io::Error::other("spool has no parent"))?,
                )
                .and_then(|f| f.sync_all())
            })
            .map_err(|e| SpawnFailure::BeforeChild(format!("spool directory sync: {e}")))?;
        let boot = boot_identity().map_err(SpawnFailure::BeforeChild)?;
        let mut child = Command::new(&launch.executable)
            .args(&launch.args)
            .current_dir(&launch.cwd)
            .env_clear()
            .envs(
                launch
                    .environment
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str())),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|e| SpawnFailure::Uncertain(e.to_string()))?;
        let pid = child.id();
        let armed = guard.map(|executable| (executable, arm(executable, ps, pid)));
        // The child is unreaped until the waiter starts, so this PID is still ours.
        let rows = snapshot(ps).ok();
        let guard = match armed {
            None => None,
            Some((executable, armed)) => {
                // The guard must be seen alive inside the new group, or there is no launch.
                let verified = armed.and_then(|(mut guard, lifeline)| {
                    let id = guard.id();
                    if rows.as_ref().is_some_and(|rows| {
                        rows.iter()
                            .any(|r| r.pid == id && r.pgid == pid && !r.zombie)
                    }) {
                        Ok((guard, lifeline))
                    } else {
                        let _ = guard.kill();
                        let _ = guard.wait();
                        Err("agent guard was not observed inside the owned group".to_owned())
                    }
                });
                match verified {
                    Ok((guard, lifeline)) => Some(Guard {
                        executable: executable.to_owned(),
                        pids: vec![guard.id()],
                        armed: Some((guard, lifeline)),
                        retired: Vec::new(),
                        rearms: 0,
                        reported: false,
                    }),
                    Err(e) => {
                        let _ = kill(Signal::Kill, &format!("-{pid}"));
                        let _ = child.wait();
                        return Err(SpawnFailure::Uncertain(e));
                    }
                }
            }
        };
        let start = rows
            .and_then(|rows| rows.into_iter().find(|r| r.pid == pid))
            .map_or_else(|| "UNKNOWN".to_owned(), |r| r.start);
        let root = ProcessIdentity {
            pid,
            ppid: std::process::id(),
            pgid: pid,
            start,
            boot,
        };
        let (tx, messages) = sync_channel(CHANNEL_DEPTH);
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let prompt = launch.stdin.clone();
        let threads = [
            std::thread::Builder::new()
                .name("jojo-stdin".into())
                .spawn(move || {
                    if let Some(mut pipe) = stdin {
                        let _ = pipe.write_all(&prompt);
                    }
                }),
            {
                let pump = Pump {
                    stream: Stream::Stdout,
                    file: Some(stdout_file),
                    limits: launch.limits,
                    fail_after: faults.stdout_fail_after,
                    normalizer: Some((LineFramer::new(launch.limits.max_frame_bytes), normalizer)),
                    tx: tx.clone(),
                };
                std::thread::Builder::new()
                    .name("jojo-stdout".into())
                    .spawn(move || {
                        if let Some(pipe) = stdout {
                            pump.run(pipe)
                        }
                    })
            },
            {
                let pump = Pump {
                    stream: Stream::Stderr,
                    file: Some(stderr_file),
                    limits: launch.limits,
                    fail_after: faults.stderr_fail_after,
                    normalizer: None,
                    tx: tx.clone(),
                };
                std::thread::Builder::new()
                    .name("jojo-stderr".into())
                    .spawn(move || {
                        if let Some(pipe) = stderr {
                            pump.run(pipe)
                        }
                    })
            },
        ];
        let slot = Arc::new(Mutex::new(RootSlot {
            child,
            exit: None,
            guard,
        }));
        let abandon = Arc::new(AtomicBool::new(false));
        let waiter = {
            let slot = Arc::clone(&slot);
            let abandon = Arc::clone(&abandon);
            let ps = ps.to_owned();
            std::thread::Builder::new()
                .name("jojo-reaper".into())
                .spawn(move || {
                    let mut announced = false;
                    let mut inspection_failed = false;
                    let mut abandoned_polls = 0;
                    loop {
                        // No caller may signal after Drop releases ownership. At that point
                        // reaping no longer risks a later signal to a recycled PGID.
                        let abandoned = abandon.load(Ordering::Acquire);
                        let rows = if abandoned { None } else { Some(snapshot(&ps)) };
                        let mut root_terminated = false;
                        let mut can_reap = abandoned;
                        if let Some(rows) = rows {
                            match rows {
                                Ok(rows) => {
                                    root_terminated = rows.iter().any(|r| r.pid == pid && r.zombie);
                                    // Guards are read after the snapshot: one that is in
                                    // these rows is already in the list.
                                    let mut s = slot.lock().unwrap_or_else(|p| p.into_inner());
                                    let guards =
                                        s.guard.as_ref().map_or(Vec::new(), |g| g.pids.clone());
                                    let members = rows.iter().any(|r| {
                                        r.pgid == pid && !r.zombie && !guards.contains(&r.pid)
                                    });
                                    can_reap = root_terminated && !members;
                                    if s.exit.is_none()
                                        && let Some(lost) = s
                                            .guard
                                            .as_mut()
                                            .and_then(|g| g.upkeep(&ps, pid, members))
                                    {
                                        drop(s);
                                        let _ = tx.send(ProcessMessage::InspectionFailed(lost));
                                    }
                                }
                                Err(e) if !inspection_failed => {
                                    inspection_failed = true;
                                    let _ = tx.send(ProcessMessage::InspectionFailed(e));
                                }
                                Err(_) => {}
                            }
                        }
                        if root_terminated && !announced {
                            announced = true;
                            let _ = tx.send(ProcessMessage::RootTerminated);
                        }
                        if can_reap {
                            let exit = {
                                let mut s = slot.lock().unwrap_or_else(|p| p.into_inner());
                                if let Some(guard) = s.guard.as_mut() {
                                    // Nothing is left to guard (or ownership was given up
                                    // after a KILL of the group): no signal from here on.
                                    if abandoned {
                                        guard.reap();
                                    } else {
                                        guard.release();
                                    }
                                }
                                match s.child.try_wait() {
                                    Ok(Some(status)) => {
                                        let exit = ExitObservation {
                                            code: status.code(),
                                            signal: status.signal(),
                                        };
                                        s.exit = Some(exit);
                                        Some(exit)
                                    }
                                    _ => None,
                                }
                            };
                            if let Some(exit) = exit {
                                let _ = tx.send(ProcessMessage::Exited(exit));
                                return;
                            }
                        }
                        if abandoned {
                            abandoned_polls += 1;
                            // A failed OS kill cannot be turned into proof of death, nor
                            // retain a reaper thread forever. Caller already recorded UNKNOWN.
                            if abandoned_polls >= 250 {
                                return;
                            }
                        }
                        std::thread::sleep(REAP_POLL);
                    }
                })
        };
        let supervised = Self {
            root,
            slot,
            known: Vec::new(),
            messages,
            spool: launch.spool.clone(),
            ps: ps.to_owned(),
            abandon,
        };
        // A thread that failed to start leaves a child we cannot observe: kill it now
        // (leader unreaped, so the group is still ours) and report the launch uncertain.
        if threads.iter().any(Result::is_err) || waiter.is_err() {
            let _ = supervised.signal_tree(Signal::Kill);
            if waiter.is_err() {
                // No reaper exists. Kill the still-owned direct Child without spawning
                // another thread, then make a bounded effort to reap it ourselves.
                // The result remains uncertain even when this direct child is reaped.
                let mut root = supervised.slot.lock().unwrap_or_else(|p| p.into_inner());
                if let Some(guard) = root.guard.as_mut() {
                    guard.reap();
                }
                let _ = root.child.kill();
                for _ in 0..100 {
                    match root.child.try_wait() {
                        Ok(Some(status)) => {
                            root.exit = Some(ExitObservation {
                                code: status.code(),
                                signal: status.signal(),
                            });
                            break;
                        }
                        Ok(None) => std::thread::sleep(Duration::from_millis(5)),
                        Err(_) => break,
                    }
                }
            }
            return Err(SpawnFailure::Uncertain("supervision thread failed".into()));
        }
        Ok(supervised)
    }

    pub fn root(&self) -> &ProcessIdentity {
        &self.root
    }

    pub fn known_descendants(&self) -> &[ProcessIdentity] {
        &self.known
    }

    pub fn spool(&self) -> &Path {
        &self.spool
    }

    pub fn messages(&self) -> &Receiver<ProcessMessage> {
        &self.messages
    }

    pub fn root_exit(&self) -> Option<ExitObservation> {
        self.slot.lock().unwrap_or_else(|p| p.into_inner()).exit
    }

    /// PIDs of this supervisor's own guard processes, alive or not. They are members of
    /// the owned group but not part of the agent's tree. Read it after taking a snapshot.
    pub fn guards(&self) -> Vec<u32> {
        let slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
        slot.guard.as_ref().map_or(Vec::new(), |g| g.pids.clone())
    }

    /// Adds group members and ppid-chain descendants visible now to the known registry and
    /// returns the newly discovered identities. A descendant that left the group is known
    /// only if seen here while its parent chain was still visible.
    pub fn discover(&mut self) -> Result<Vec<ProcessIdentity>, String> {
        let boot = boot_identity()?;
        let rows = snapshot(&self.ps)?;
        let guards = self.guards();
        // After reaping, that numeric PID/PGID is no longer an ownership anchor.
        if self.root_exit().is_some() {
            return Ok(Vec::new());
        }
        let mut chain: BTreeSet<u32> = BTreeSet::from([self.root.pid]);
        for k in &self.known {
            if check(&rows, k) == IdentityCheck::Alive {
                chain.insert(k.pid);
            }
        }
        let mut found = Vec::new();
        loop {
            let before = found.len();
            for r in &rows {
                if r.zombie
                    || r.pid == self.root.pid
                    || guards.contains(&r.pid)
                    || self.known.iter().chain(&found).any(|k| k.pid == r.pid)
                {
                    continue;
                }
                if r.pgid == self.root.pgid || chain.contains(&r.ppid) {
                    chain.insert(r.pid);
                    if self.known.len() + found.len() >= MAX_KNOWN_DESCENDANTS {
                        return Err("known descendant registry limit exceeded".into());
                    }
                    found.push(ProcessIdentity {
                        pid: r.pid,
                        ppid: r.ppid,
                        pgid: r.pgid,
                        start: r.start.clone(),
                        boot: boot.clone(),
                    });
                }
            }
            if found.len() == before {
                break;
            }
        }
        self.known.extend(found.iter().cloned());
        Ok(found)
    }

    /// Inspect a recorded identity using this supervisor's explicit observer.
    pub fn verify_identity(&self, identity: &ProcessIdentity) -> IdentityCheck {
        match boot_identity() {
            Ok(boot) if boot != identity.boot => return IdentityCheck::Absent,
            Err(e) => return IdentityCheck::Unknown(e),
            Ok(_) => {}
        }
        match snapshot(&self.ps) {
            Ok(rows) => check(&rows, identity),
            Err(e) => IdentityCheck::Unknown(e),
        }
    }

    /// A snapshot match authorizes a signal only for a descendant this supervisor
    /// discovered outside its group, and then only through [`signal_escaped`]. Any other
    /// unpinned PID, the root included, is refused.
    pub fn signal_verified(&self, identity: &ProcessIdentity, signal: Signal) -> SignalRecord {
        let escaped = identity.pgid != self.root.pgid && self.known.contains(identity);
        let outcome = match self.verify_identity(identity) {
            IdentityCheck::Alive if escaped => return signal_escaped(&self.ps, identity, signal),
            IdentityCheck::Alive => {
                SignalOutcome::RefusedUnverified("no retained process lifetime handle".into())
            }
            IdentityCheck::Absent => SignalOutcome::Absent,
            IdentityCheck::Reused => SignalOutcome::RefusedReused,
            IdentityCheck::Unknown(e) => SignalOutcome::RefusedUnverified(e),
        };
        SignalRecord {
            signal,
            target: format!("pid:{}", identity.pid),
            outcome,
        }
    }

    /// Only an unreaped child pins the numeric PGID against recycling. Never fall
    /// back to a snapshot-matched member after reaping. Known descendants outside the
    /// group go through [`Self::signal_verified`].
    pub fn signal_tree(&self, signal: Signal) -> Vec<SignalRecord> {
        let slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
        let outcome = if slot.exit.is_none() {
            kill(signal, &format!("-{}", self.root.pgid))
        } else {
            SignalOutcome::RefusedUnverified("group leader already reaped".into())
        };
        drop(slot);
        let mut records = vec![SignalRecord {
            signal,
            target: format!("group:{}", self.root.pgid),
            outcome,
        }];
        for k in self.known.iter().filter(|k| k.pgid != self.root.pgid) {
            records.push(self.signal_verified(k, signal));
        }
        records
    }

    pub fn tree(&self) -> TreeObservation {
        let root_exit = self.root_exit();
        let mut obs = TreeObservation {
            root_exit,
            ..TreeObservation::default()
        };
        match snapshot(&self.ps) {
            Err(e) => obs.error = Some(e),
            Ok(rows) => {
                let guards = self.guards();
                obs.live_group = rows
                    .iter()
                    .filter(|r| r.pgid == self.root.pgid && !r.zombie && !guards.contains(&r.pid))
                    .map(|r| r.pid)
                    .collect();
                for k in self.known.iter().filter(|k| k.pgid != self.root.pgid) {
                    match check(&rows, k) {
                        IdentityCheck::Alive => obs.live_known.push(k.pid),
                        IdentityCheck::Reused => obs.reused.push(k.pid),
                        _ => {}
                    }
                }
            }
        }
        obs
    }
}

impl Drop for Supervised {
    /// Best-effort only (e.g. the caller failed or panicked): kill the still-owned group.
    /// This is not a confirmed stop and records nothing.
    fn drop(&mut self) {
        let slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
        if slot.exit.is_none() {
            let _ = kill(Signal::Kill, &format!("-{}", self.root.pgid));
        }
        self.abandon.store(true, Ordering::Release);
    }
}

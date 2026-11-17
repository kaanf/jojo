//! Provider-neutral data only. No launch, credentials, persistence or approval authority.
use crate::attempt::AttemptBindings;
use crate::identity::{AttemptId, OpaqueId, Revision};
use crate::time::ClockReading;
use std::path::PathBuf;

pub const PROVIDER_SCHEMA: u32 = 1;
pub const MAX_PROMPT_BYTES: usize = 64 * 1024;
/// Longest `--version` text a fingerprint may record.
pub const MAX_VERSION_BYTES: usize = 128;

/// D3: a CLI's version is a recorded fact, not a gate. Any one line of printable text is
/// accepted; a release other than the one a stream profile was written against runs
/// under that profile, which fails closed on anything it does not know.
pub fn plain_version(version: &str) -> bool {
    !version.trim().is_empty()
        && version.len() <= MAX_VERSION_BYTES
        && version.bytes().all(|b| (0x20..0x7f).contains(&b))
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ProviderKind {
    ClaudeCode,
    Codex,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Support {
    Supported,
    Unsupported,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthMode {
    NativeSubscription,
    NotSignedIn,
    UnsupportedMode,
    ApiKey,
    Unknown,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Readiness {
    Ready,
    Unavailable,
    Unsupported,
    Unknown,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Capabilities {
    pub evidence: String,
    pub structured_events: Support,
    pub explicit_sessions: Support,
    pub exact_continuation: Support,
    pub incremental_input: Support,
    pub question_ids: Support,
    pub permission_response: Support,
    pub turn_interrupt: Support,
    pub process_tree: Support,
    pub continuation_cwd_change: Support,
    pub auth_status: Support,
    pub workspace_policy: Support,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderFingerprint {
    pub provider: ProviderKind,
    pub executable: PathBuf,
    pub version: String,
    pub schema: u32,
    pub parser: String,
    pub configuration: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionRef {
    pub provider: ProviderKind,
    pub session: String,
    pub turn: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Continuation {
    pub predecessor: AttemptId,
    pub exact: SessionRef,
    pub previous_cwd: PathBuf,
    pub reviewed_plan: String,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Role {
    Developer,
    Validator,
    /// M2: the lead agent of a Feature Chat turn. Read-only; it is never launched from a
    /// `LaunchSpec`, only from a `planner::TurnSpec`.
    Planner,
}
/// Product approvals are references only; tool permission cannot create these.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PermissionEnvelope {
    pub approved_digest: String,
    pub tools: Vec<String>,
    pub filesystem_roots: Vec<PathBuf>,
    pub network: bool,
    pub hooks: bool,
    pub background: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceBinding {
    pub common_dir: PathBuf,
    pub base_commit: String,
    pub candidate_source_vector: String,
    pub instruction_digest: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaunchSpec {
    pub attempt: AttemptId,
    pub bindings: AttemptBindings,
    pub engine_epoch: OpaqueId,
    pub dispatch_generation: u64,
    pub fingerprint: ProviderFingerprint,
    pub source: SourceBinding,
    pub prompt_digest: String,
    pub prompt: Vec<u8>,
    pub continuation: Option<Continuation>,
    pub role: Role,
    pub permissions: PermissionEnvelope,
    pub cwd: PathBuf,
    /// Names only, never inherited secret values.
    pub environment_allowlist: Vec<String>,
    pub config_references: Vec<String>,
    pub timeout_millis: u64,
    pub max_output_bytes: u64,
    pub max_frame_bytes: u64,
    pub raw_output_dir: PathBuf,
    pub approval_ids: [OpaqueId; 3],
    pub required_outputs: Vec<String>,
    pub validation_plan: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderError {
    FingerprintMismatch,
    NotReady,
    AuthenticationDenied,
    InvalidSpec,
    UnsupportedContinuation,
    UnsupportedInput,
    UnsupportedPermission,
    UnsupportedInterrupt,
    Correlation,
    Limit,
    Cancelled,
    Protocol,
}
impl LaunchSpec {
    /// P2-01 launch-envelope amendment: the production Codex profile, and only it, may
    /// name the one non-secret path variable that locates an explicitly selected Codex
    /// home. Every other provider, parser and name keeps the original three-name rule.
    ///
    /// M1: the production Claude Code profile, and only it, may name the home directory
    /// and the account name its sign-in is stored under. Neither is a secret.
    fn provider_variable(&self, name: &str) -> bool {
        (name == crate::codex::CODEX_HOME_VARIABLE
            && self.fingerprint.provider == ProviderKind::Codex
            && self.fingerprint.parser == crate::codex::CODEX_EXEC_JSONL)
            || (crate::claude::CLAUDE_VARIABLES.contains(&name)
                && self.fingerprint.provider == ProviderKind::ClaudeCode
                && self.fingerprint.parser == crate::claude::CLAUDE_PRINT_STREAM_JSON)
    }

    /// Structural boundary, not scheduler admission, approval or repository validation.
    pub fn validate(&self) -> Result<(), ProviderError> {
        let absolute = |p: &PathBuf| {
            p.is_absolute()
                && !p
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
        };
        let text = [
            &self.fingerprint.version,
            &self.fingerprint.parser,
            &self.fingerprint.configuration,
            &self.source.base_commit,
            &self.source.candidate_source_vector,
            &self.source.instruction_digest,
            &self.prompt_digest,
            &self.permissions.approved_digest,
            &self.validation_plan,
        ];
        if self.fingerprint.schema != PROVIDER_SCHEMA
            || self.dispatch_generation == 0
            || self.bindings.plan_revision.0 == 0
            || self.bindings.step_revision.0 == 0
            || text.iter().any(|s| s.trim().is_empty())
            || self.prompt.is_empty()
            || self.prompt.len() > MAX_PROMPT_BYTES
            || self.timeout_millis == 0
            || self.max_frame_bytes == 0
            || self.max_frame_bytes > self.max_output_bytes
            || self.required_outputs.is_empty()
            || self.required_outputs.iter().any(|s| s.trim().is_empty())
            || ![
                &self.cwd,
                &self.raw_output_dir,
                &self.source.common_dir,
                &self.fingerprint.executable,
            ]
            .into_iter()
            .all(absolute)
            || self.permissions.filesystem_roots.is_empty()
            || !self.permissions.filesystem_roots.iter().all(absolute)
            || self.permissions.network
            || self.permissions.hooks
            || self.permissions.background
            || self.environment_allowlist.iter().any(|n| {
                !matches!(n.as_str(), "LANG" | "LC_ALL" | "TZ") && !self.provider_variable(n)
            })
            || self.config_references.iter().any(|s| s.trim().is_empty())
        {
            return Err(ProviderError::InvalidSpec);
        }
        if let Some(c) = &self.continuation
            && (c.predecessor == self.attempt
                || c.exact.provider != self.fingerprint.provider
                || c.exact.session.trim().is_empty()
                || c.exact.turn.trim().is_empty()
                || c.reviewed_plan.trim().is_empty()
                || !absolute(&c.previous_cwd))
        {
            return Err(ProviderError::InvalidSpec);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stream {
    Stdout,
    Stderr,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawReference {
    pub blob: String,
    pub stream: Stream,
    pub start: u64,
    pub end: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalStatus {
    Success,
    Error,
    Interrupted,
    Unknown,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventPayload {
    ProcessStarted {
        process: String,
    },
    SessionIdentified(SessionRef),
    TurnStarted {
        turn: String,
    },
    TextDelta(String),
    ToolStarted {
        id: String,
    },
    ToolFinished {
        id: String,
    },
    QuestionRequested {
        id: String,
        revision: Revision,
    },
    PermissionRequested {
        id: String,
        revision: Revision,
        deadline_millis: i64,
    },
    UsageObserved {
        units: u64,
    },
    ProviderTerminal {
        turn: String,
        status: TerminalStatus,
        raw_status: String,
    },
    ProviderError(String),
    InterruptAcknowledged,
    ProcessExited {
        code: Option<i32>,
    },
    StreamTruncated,
    ProtocolViolation(String),
    Informational(String),
}
/// Diagnostic evidence only. No result-sealing, claim-release or approval operation exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NormalizedEvent {
    pub attempt: AttemptId,
    pub process_generation: u64,
    pub adapter_version: String,
    pub schema: u32,
    pub sequence: u64,
    pub raw: RawReference,
    pub observed: ClockReading,
    pub payload: EventPayload,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InputTarget {
    pub attempt: AttemptId,
    pub generation: u64,
    pub session: SessionRef,
    pub request: String,
    pub expected_revision: Revision,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolDecision {
    Deny,
    AllowOnce,
}
/// Supplied by the trusted caller; a provider event cannot construct a product approval.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolPermissionAnswer {
    pub target: InputTarget,
    pub decision: ToolDecision,
    pub observed_millis: i64,
}

/// Translation/preflight only. Process management and durable admission belong to the caller.
pub trait ProviderAdapter {
    type Prepared;
    fn capabilities(&self) -> &Capabilities;
    fn prepare(&self, spec: LaunchSpec) -> Result<Self::Prepared, ProviderError>;
}

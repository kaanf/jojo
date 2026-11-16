//! Claude Code `--print` launch contract (M1). Pure translation and structural checks: no
//! I/O, no launch, no credentials, no ambient process state.
//!
//! Jojo can express exactly one Claude Code argument vector. [`print_argv`] takes nothing,
//! so no caller can add an option: permission bypass, extra directories, MCP servers,
//! plugins, settings, model or session selection and every subcommand have no
//! representation here. [`check_print_argv`] is the same rule read backwards, applied
//! again where the request crosses the OS boundary. The working directory is the owned
//! worktree; the prompt is not an argument, the runner writes it to standard input.
//!
//! The vector asks the CLI for its own restricted mode with exactly five file tools. What
//! that does was observed on the pinned release on 2026-11-16 (three runs in a temporary
//! repository, kept under `~/JojoDisposable/m1-probes`): edits inside the working
//! directory are applied; writes outside it, a write to `.git/config` and a shell command
//! are refused by the CLI; with only `HOME` in the environment the CLI is not signed in,
//! with `HOME` and `USER` it is. The matching stream profile
//! ([`CLAUDE_PRINT_STREAM_JSON`]) re-checks the tool list, the permission mode, the
//! absence of MCP servers and of an API key on the first record of every run, and fails
//! closed on anything it does not model.
use crate::provider::{Capabilities, LaunchSpec, ProviderError, ProviderKind, Role, Support};
use std::path::{Component, Path, PathBuf};

/// Parser fingerprint of the production Claude Code profile. Distinct from the synthetic one.
pub const CLAUDE_PRINT_STREAM_JSON: &str = "jojo.claude.print-stream-json.2.1.311/1";
/// The only release this profile was written against (`claude --version` text).
pub const CLAUDE_VERSION: &str = "2.1.311 (Claude Code)";
/// The two variables of the Claude Code launch envelope, in this order: the home
/// directory that holds the CLI's state and the account name its sign-in is stored
/// under. A path and a name, never a secret.
pub const CLAUDE_VARIABLES: [&str; 2] = ["HOME", "USER"];
/// Prefix of `ProviderFingerprint::configuration`; the executable digest follows it.
pub const CLAUDE_CONFIGURATION: &str = "jojo.claude.print/1";
/// The built-in tools of a run, as the CLI lists them on its first record: file tools
/// only. Nothing that runs a command or reaches the network.
pub const CLAUDE_TOOLS: [&str; 5] = ["Edit", "Glob", "Grep", "Read", "Write"];
/// The permission mode of a run: edits inside the working directory are applied; nothing
/// else is granted, and nothing that would ask is answered.
pub const CLAUDE_PERMISSION_MODE: &str = "acceptEdits";
const MAX_USER_BYTES: usize = 64;

/// Options and subcommands that must never reach a Claude Code process. The allowlist
/// below is what enforces this; the list exists so tests and gates can name what is
/// excluded.
pub const PROHIBITED_ARGUMENTS: [&str; 36] = [
    "--dangerously-skip-permissions",
    "--allow-dangerously-skip-permissions",
    "bypassPermissions",
    "auto",
    "dontAsk",
    "--add-dir",
    "--mcp-config",
    "--plugin-dir",
    "--plugin-url",
    "--settings",
    "--setting-sources",
    "--agents",
    "--agent",
    "--allowedTools",
    "--allowed-tools",
    "--system-prompt",
    "--append-system-prompt",
    "--model",
    "--fallback-model",
    "--betas",
    "--bare",
    "--chrome",
    "--ide",
    "--bg",
    "--background",
    "--cloud",
    "--remote-control",
    "-c",
    "--continue",
    "-r",
    "--resume",
    "--session-id",
    "-w",
    "--worktree",
    "auth",
    "setup-token",
];

/// Absolute and already in normal form: no `.`/`..`, doubled or trailing separator.
fn plain_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
        && path.components().collect::<PathBuf>().as_os_str() == path.as_os_str()
}

/// The complete argument vector for one fresh, unsaved, restricted `--print` run in the
/// process working directory.
pub fn print_argv() -> Vec<String> {
    [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--input-format",
        "text",
        "--restricted",
        "--strict-mcp-config",
        "--tools",
        "Read,Edit,Write,Glob,Grep",
        "--permission-mode",
        CLAUDE_PERMISSION_MODE,
        "--permission-prompts",
        "none",
        "--no-session-persistence",
        "--disable-slash-commands",
        "--no-chrome",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Parser fingerprint of a lead agent turn (M2): the same record shapes as
/// [`CLAUDE_PRINT_STREAM_JSON`], held to the read-only envelope below.
pub const CLAUDE_DISCOVERY_STREAM_JSON: &str = "jojo.claude.discovery-stream-json.2.1.311/1";
/// The built-in tools of a lead agent turn, as the CLI lists them on its first record:
/// nothing that writes, runs a command or reaches the network.
pub const CLAUDE_DISCOVERY_TOOLS: [&str; 3] = ["Glob", "Grep", "Read"];
/// The permission mode a lead agent turn asks for. Nothing is granted beyond the tools;
/// with `--permission-prompts none` anything that would ask is denied.
pub const CLAUDE_DISCOVERY_PERMISSION_MODE: &str = "manual";
/// What the pinned release reports on its first record for that mode (observed
/// 2026-11-16, `~/JojoDisposable/m2-probes/20261116T161559Z/claude-a`).
pub const CLAUDE_DISCOVERY_REPORTED_MODE: &str = "default";

/// The complete argument vector for one fresh, unsaved, restricted, read-only `--print`
/// run in the process working directory: [`print_argv`] with the three read tools and
/// without the mode that applies edits.
pub fn discovery_argv() -> Vec<String> {
    [
        "-p",
        "--output-format",
        "stream-json",
        "--verbose",
        "--input-format",
        "text",
        "--restricted",
        "--strict-mcp-config",
        "--tools",
        "Read,Glob,Grep",
        "--permission-mode",
        CLAUDE_DISCOVERY_PERMISSION_MODE,
        "--permission-prompts",
        "none",
        "--no-session-persistence",
        "--disable-slash-commands",
        "--no-chrome",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Accepts only the byte-exact vector [`discovery_argv`] produces.
pub fn check_discovery_argv(args: &[String]) -> Result<(), ProviderError> {
    if discovery_argv() == args {
        Ok(())
    } else {
        Err(ProviderError::InvalidSpec)
    }
}

/// Accepts only the byte-exact vector [`print_argv`] produces.
pub fn check_print_argv(args: &[String]) -> Result<(), ProviderError> {
    if print_argv() == args {
        Ok(())
    } else {
        Err(ProviderError::InvalidSpec)
    }
}

/// `jojo.claude.print/1;sha256=<digest>` for a lower-case hexadecimal SHA-256.
pub fn configuration(sha256: &str) -> Option<String> {
    let text = format!("{CLAUDE_CONFIGURATION};sha256={sha256}");
    configured_digest(&text).is_some().then_some(text)
}

/// The executable digest bound in a Claude Code fingerprint configuration.
pub fn configured_digest(configuration: &str) -> Option<&str> {
    configuration
        .strip_prefix(CLAUDE_CONFIGURATION)?
        .strip_prefix(";sha256=")
        .filter(|d| {
            d.len() == 64
                && d.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

/// Structural rules of the production Claude Code profile, on top of
/// `LaunchSpec::validate`. One fresh developer run in the owned worktree; nothing the CLI
/// cannot be held to.
pub fn check_spec(spec: &LaunchSpec) -> Result<(), ProviderError> {
    spec.validate()?;
    let fingerprint = &spec.fingerprint;
    if fingerprint.provider != ProviderKind::ClaudeCode
        || fingerprint.parser != CLAUDE_PRINT_STREAM_JSON
        || fingerprint.version != CLAUDE_VERSION
        || configured_digest(&fingerprint.configuration).is_none()
    {
        return Err(ProviderError::FingerprintMismatch);
    }
    // `--resume`/`--continue` are not part of this contract; a continuation is never guessed.
    if spec.continuation.is_some() {
        return Err(ProviderError::UnsupportedContinuation);
    }
    // The file tools are confined to the working directory and their list is fixed by
    // the argument vector: any other approved envelope would be a promise this launch
    // cannot keep.
    if spec.role != Role::Developer
        || !spec.permissions.tools.is_empty()
        || spec.permissions.filesystem_roots != [spec.cwd.clone()]
        || !plain_absolute(&spec.cwd)
    {
        return Err(ProviderError::UnsupportedPermission);
    }
    if spec.environment_allowlist != CLAUDE_VARIABLES || !spec.config_references.is_empty() {
        return Err(ProviderError::InvalidSpec);
    }
    Ok(())
}

/// The child receives exactly two variables: the explicitly selected home and the account
/// name. Returns the home. The worktree may lie under the home (it usually does); the
/// home may not lie inside anything the run may write or Jojo records.
pub fn check_environment<'a>(
    spec: &LaunchSpec,
    environment: &'a [(String, String)],
) -> Result<&'a Path, ProviderError> {
    let [(home_name, home), (user_name, user)] = environment else {
        return Err(ProviderError::InvalidSpec);
    };
    let home = Path::new(home);
    let inside = |other: &Path| home.starts_with(other);
    let plain_user = !user.is_empty()
        && user.len() <= MAX_USER_BYTES
        && user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if [home_name.as_str(), user_name.as_str()] != CLAUDE_VARIABLES
        || !plain_absolute(home)
        || home.parent().is_none()
        || !plain_user
        || inside(&spec.cwd)
        || inside(&spec.raw_output_dir)
        || inside(&spec.source.common_dir)
    {
        return Err(ProviderError::InvalidSpec);
    }
    Ok(home)
}

/// What was observed on the pinned release, and nothing more. UNKNOWN is not a yes: no
/// input, permission answer, interrupt or continuation is offered on this basis.
pub fn observed_capabilities() -> Capabilities {
    Capabilities {
        evidence: format!(
            "{CLAUDE_VERSION}: --help text and three --print runs in a temporary repository \
             (2026-11-16): stream-json records, working-directory confinement of the file \
             tools and refusal of a shell command were observed"
        ),
        structured_events: Support::Supported,
        explicit_sessions: Support::Unknown,
        exact_continuation: Support::Unknown,
        incremental_input: Support::Unknown,
        question_ids: Support::Unknown,
        permission_response: Support::Unknown,
        turn_interrupt: Support::Unknown,
        process_tree: Support::Unknown,
        continuation_cwd_change: Support::Unknown,
        auth_status: Support::Unknown,
        workspace_policy: Support::Supported,
    }
}

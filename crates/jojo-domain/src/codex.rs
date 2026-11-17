//! Codex `exec` launch contract (P2-01). Pure translation and structural checks: no I/O,
//! no launch, no credentials, no ambient process state.
//!
//! Jojo can express exactly one Codex argument vector. [`exec_argv`] takes the owned
//! worktree and nothing else, so no caller can add an option: sandbox bypass, persisted
//! trust, extra writable directories, alternative providers, configuration overrides,
//! session selection and login commands have no representation here. [`check_exec_argv`]
//! is the same rule read backwards, applied again where the request crosses the OS
//! boundary.
//!
//! The profile is pinned to one installed release. Its help text was read during planning;
//! its runtime behaviour was not observed, so every capability is recorded UNKNOWN and the
//! matching stream profile fails closed on anything it does not model.
use crate::provider::{Capabilities, LaunchSpec, ProviderError, ProviderKind, Role, Support};
use std::path::{Component, Path, PathBuf};

/// Parser fingerprint of the production Codex profile. Distinct from the synthetic one.
pub const CODEX_EXEC_JSONL: &str = "jojo.codex.exec-jsonl.0.169.0/1";
/// The only release this profile was written against (`codex --version` text).
pub const CODEX_VERSION: &str = "codex-cli 0.169.0";
/// The one provider variable of the Codex launch envelope: a path, never a secret.
pub const CODEX_HOME_VARIABLE: &str = "CODEX_HOME";
/// Prefix of `ProviderFingerprint::configuration`; the executable digest follows it.
pub const CODEX_CONFIGURATION: &str = "jojo.codex.exec/1";

/// Options and subcommands that must never reach a Codex process. The allowlist below is
/// what enforces this; the list exists so tests and gates can name what is excluded.
pub const PROHIBITED_ARGUMENTS: [&str; 26] = [
    "--dangerously-bypass-approvals-and-sandbox",
    "--dangerously-bypass-hook-trust",
    "--approve-for-me",
    "--trust",
    "--worktree",
    "--add-dir",
    "--oss",
    "--local-provider",
    "--enable",
    "--disable",
    "-c",
    "--config",
    "--last",
    "--skip-git-repo-check",
    "--full-auto",
    "-m",
    "--model",
    "-p",
    "--profile",
    "--with-api-key",
    "--with-access-token",
    "danger-full-access",
    "resume",
    "fork",
    "login",
    "doctor",
];

/// Absolute and already in normal form: no `.`/`..`, doubled or trailing separator.
fn plain_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
        && path.components().collect::<PathBuf>().as_os_str() == path.as_os_str()
}

/// The complete argument vector for one fresh, ephemeral, sandboxed `exec` run in
/// `worktree`. The prompt is not an argument: the runner writes it to standard input.
pub fn exec_argv(worktree: &Path) -> Result<Vec<String>, ProviderError> {
    let directory = worktree
        .to_str()
        .filter(|_| plain_absolute(worktree))
        .ok_or(ProviderError::InvalidSpec)?;
    Ok([
        "exec",
        "--json",
        "--color",
        "never",
        "--sandbox",
        "workspace-write",
        "--ephemeral",
        "--ignore-user-config",
        "--ignore-rules",
        "-C",
        directory,
    ]
    .map(str::to_owned)
    .to_vec())
}

/// Parser fingerprint of a lead agent turn (M2): the same record shapes as
/// [`CODEX_EXEC_JSONL`], without the item kind that changes a path.
pub const CODEX_DISCOVERY_JSONL: &str = "jojo.codex.discovery-jsonl.0.169.0/1";

/// The complete argument vector for one fresh, ephemeral `exec` run in `repository` under
/// the CLI's read-only sandbox: [`exec_argv`] with the other sandbox policy.
pub fn discovery_argv(repository: &Path) -> Result<Vec<String>, ProviderError> {
    let mut argv = exec_argv(repository)?;
    argv[5] = "read-only".into();
    Ok(argv)
}

/// Accepts only the byte-exact vector [`discovery_argv`] produces for `repository`.
pub fn check_discovery_argv(args: &[String], repository: &Path) -> Result<(), ProviderError> {
    match discovery_argv(repository) {
        Ok(golden) if golden == args => Ok(()),
        _ => Err(ProviderError::InvalidSpec),
    }
}

/// Accepts only the byte-exact vector [`exec_argv`] produces for `worktree`.
pub fn check_exec_argv(args: &[String], worktree: &Path) -> Result<(), ProviderError> {
    match exec_argv(worktree) {
        Ok(golden) if golden == args => Ok(()),
        _ => Err(ProviderError::InvalidSpec),
    }
}

/// `jojo.codex.exec/1;sha256=<digest>` for a lower-case hexadecimal SHA-256.
pub fn configuration(sha256: &str) -> Option<String> {
    let text = format!("{CODEX_CONFIGURATION};sha256={sha256}");
    configured_digest(&text).is_some().then_some(text)
}

/// The executable digest bound in a Codex fingerprint configuration.
pub fn configured_digest(configuration: &str) -> Option<&str> {
    configuration
        .strip_prefix(CODEX_CONFIGURATION)?
        .strip_prefix(";sha256=")
        .filter(|d| {
            d.len() == 64
                && d.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

/// Structural rules of the production Codex profile, on top of `LaunchSpec::validate`.
/// One fresh developer run in the owned worktree; nothing the CLI cannot be held to.
pub fn check_spec(spec: &LaunchSpec) -> Result<(), ProviderError> {
    spec.validate()?;
    let fingerprint = &spec.fingerprint;
    if fingerprint.provider != ProviderKind::Codex
        || fingerprint.parser != CODEX_EXEC_JSONL
        || !crate::provider::plain_version(&fingerprint.version)
        || configured_digest(&fingerprint.configuration).is_none()
    {
        return Err(ProviderError::FingerprintMismatch);
    }
    // `exec resume`/`fork` are not part of this contract; a continuation is never guessed.
    if spec.continuation.is_some() {
        return Err(ProviderError::UnsupportedContinuation);
    }
    // The sandbox grants the working root only, and `exec` takes no tool allowlist: any
    // other approved envelope would be a promise this launch cannot keep.
    if spec.role != Role::Developer
        || !spec.permissions.tools.is_empty()
        || spec.permissions.filesystem_roots != [spec.cwd.clone()]
    {
        return Err(ProviderError::UnsupportedPermission);
    }
    if spec.environment_allowlist != [CODEX_HOME_VARIABLE] || !spec.config_references.is_empty() {
        return Err(ProviderError::InvalidSpec);
    }
    exec_argv(&spec.cwd).map(drop)
}

/// The child receives exactly one variable: the explicitly selected Codex home. Returns
/// that path. The home must lie outside everything the run may write or Jojo records.
pub fn check_environment<'a>(
    spec: &LaunchSpec,
    environment: &'a [(String, String)],
) -> Result<&'a Path, ProviderError> {
    let [(name, value)] = environment else {
        return Err(ProviderError::InvalidSpec);
    };
    let home = Path::new(value);
    let overlaps = |other: &Path| home.starts_with(other) || other.starts_with(home);
    if name != CODEX_HOME_VARIABLE
        || !plain_absolute(home)
        || home.parent().is_none()
        || overlaps(&spec.cwd)
        || overlaps(&spec.raw_output_dir)
        || overlaps(&spec.source.common_dir)
    {
        return Err(ProviderError::InvalidSpec);
    }
    Ok(home)
}

/// Nothing about the installed release has been observed at run time. UNKNOWN is not a
/// yes: no input, permission answer, interrupt or continuation is offered on this basis.
pub fn unverified_capabilities() -> Capabilities {
    Capabilities {
        evidence: format!(
            "{CODEX_VERSION}: --version and --help text only (2026-11-15); never executed by Jojo"
        ),
        structured_events: Support::Unknown,
        explicit_sessions: Support::Unknown,
        exact_continuation: Support::Unknown,
        incremental_input: Support::Unknown,
        question_ids: Support::Unknown,
        permission_response: Support::Unknown,
        turn_interrupt: Support::Unknown,
        process_tree: Support::Unknown,
        continuation_cwd_change: Support::Unknown,
        auth_status: Support::Unknown,
        workspace_policy: Support::Unknown,
    }
}

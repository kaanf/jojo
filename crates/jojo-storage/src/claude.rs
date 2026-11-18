//! Production Claude Code adapter (M1): filesystem validation and translation only.
//!
//! Nothing here starts a process, reads a credential or any file inside the home
//! directory, or opens a network connection. The adapter turns a frozen `LaunchSpec` into
//! the one `ProcessLaunch` the Claude Code contract allows (`jojo_domain::claude`) after
//! proving that the configured executable is the approved file, by the same rules as the
//! Codex adapter: an explicit, fully resolved path to a native executable that only its
//! owner can modify, whose SHA-256 equals the digest bound in the provider fingerprint.
//!
//! The version is the `--version` text that executable discovery recorded for that file (D3),
//! or the one the launcher pinned; this adapter executes nothing.
//! Sign-in state is attested by the human and never inspected here; the stream profile
//! additionally refuses a run whose first record reports an API key. Every check is
//! repeated by [`verify_launch`] when the request is rebuilt at the launch boundary.
use std::path::PathBuf;

use jojo_domain::claude::{
    CLAUDE_PRINT_STREAM_JSON, CLAUDE_VARIABLES, check_environment, check_print_argv, check_spec,
    configuration, configured_digest, observed_capabilities, print_argv,
};
use jojo_domain::provider::{
    AuthMode, Capabilities, LaunchSpec, PROVIDER_SCHEMA, ProviderAdapter, ProviderError,
    ProviderFingerprint, ProviderKind,
};

use crate::codex::{inspect_executable, inspect_home};
use crate::process::{LaunchRejection, ProcessLaunch};

/// Everything is supplied explicitly by the human-approved configuration. No field is
/// discovered from the ambient process, and none holds a secret.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClaudeAdapterConfig {
    /// The native binary itself, fully resolved.
    pub executable: PathBuf,
    /// `claude --version` text as recorded by the human for this file.
    pub version: String,
    /// Approved lower-case hexadecimal SHA-256 of the executable.
    pub sha256: String,
    /// The home directory the CLI keeps its state under. Located, never read.
    pub home: PathBuf,
    /// The account name the CLI's sign-in is stored under.
    pub user: String,
    /// Sign-in mode confirmed by the human. Anything but a native subscription denies.
    pub auth: AuthMode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClaudeRejection {
    Executable(String),
    /// The version text is not one plain line. Another release than the one the parser
    /// profile was written against is accepted and recorded (D3).
    Version,
    /// The file's bytes are not the approved ones.
    Digest,
    Home(String),
    /// Sign-in mode is not a confirmed native subscription.
    Authentication,
    /// The spec was frozen against a different executable, version, digest or parser.
    Fingerprint,
    Launch(LaunchRejection),
}

#[derive(Clone, Debug)]
pub struct ClaudeAdapter {
    fingerprint: ProviderFingerprint,
    home: PathBuf,
    user: String,
    auth: AuthMode,
    capabilities: Capabilities,
}

/// The Claude Code launch contract at the OS boundary. `ProcessLaunch::from_spec` calls
/// this for every request that names the production Claude Code parser, before any issue
/// journal entry.
pub(crate) fn verify_launch(
    spec: &LaunchSpec,
    args: &[String],
    environment: &[(String, String)],
) -> Result<(), LaunchRejection> {
    check_spec(spec).map_err(LaunchRejection::Spec)?;
    check_print_argv(args).map_err(|_| {
        LaunchRejection::Argument("not the allowlisted Claude Code argument vector".into())
    })?;
    let home = check_environment(spec, environment)
        .map_err(|_| LaunchRejection::Environment(CLAUDE_VARIABLES.join(" ")))?;
    inspect_home(home).map_err(LaunchRejection::Environment)?;
    // Lexical separation was checked on the spec; repeat it on what the paths resolve to.
    for other in [&spec.cwd, &spec.raw_output_dir, &spec.source.common_dir] {
        if std::fs::canonicalize(other).is_ok_and(|real| home.starts_with(&real)) {
            return Err(LaunchRejection::Environment(
                "home lies inside the worktree, repository or output directory".into(),
            ));
        }
    }
    let digest =
        inspect_executable(&spec.fingerprint.executable).map_err(LaunchRejection::Executable)?;
    if configured_digest(&spec.fingerprint.configuration) != Some(digest.as_str()) {
        return Err(LaunchRejection::Executable(
            "digest differs from the approved fingerprint".into(),
        ));
    }
    Ok(())
}

impl ClaudeAdapter {
    /// Validates the configured executable and home. Reads the executable's bytes once to
    /// hash them; executes nothing.
    pub fn new(config: ClaudeAdapterConfig) -> Result<Self, ClaudeRejection> {
        if !jojo_domain::provider::plain_version(&config.version) {
            return Err(ClaudeRejection::Version);
        }
        let digest = inspect_executable(&config.executable).map_err(ClaudeRejection::Executable)?;
        if digest != config.sha256 {
            return Err(ClaudeRejection::Digest);
        }
        inspect_home(&config.home).map_err(ClaudeRejection::Home)?;
        Ok(Self {
            fingerprint: ProviderFingerprint {
                provider: ProviderKind::ClaudeCode,
                executable: config.executable,
                version: config.version,
                schema: PROVIDER_SCHEMA,
                parser: CLAUDE_PRINT_STREAM_JSON.into(),
                configuration: configuration(&digest).ok_or(ClaudeRejection::Digest)?,
            },
            home: config.home,
            user: config.user,
            auth: config.auth,
            capabilities: observed_capabilities(),
        })
    }

    /// The fingerprint a `LaunchSpec` must be frozen against.
    pub fn fingerprint(&self) -> &ProviderFingerprint {
        &self.fingerprint
    }

    /// Whether a launch may be prepared at all: sign-in must be an attested native
    /// subscription. Checked again by [`Self::prepare_launch`].
    pub fn ready(&self) -> Result<(), ClaudeRejection> {
        if self.auth != AuthMode::NativeSubscription {
            return Err(ClaudeRejection::Authentication);
        }
        Ok(())
    }

    /// Frozen spec → the one allowed OS request. No side effect; nothing is spawned.
    pub fn prepare_launch(&self, spec: &LaunchSpec) -> Result<ProcessLaunch, ClaudeRejection> {
        if spec.fingerprint != self.fingerprint {
            return Err(ClaudeRejection::Fingerprint);
        }
        self.ready()?;
        let Some(home) = self.home.to_str() else {
            return Err(ClaudeRejection::Home("path is not UTF-8".into()));
        };
        let [home_name, user_name] = CLAUDE_VARIABLES;
        let environment = vec![
            (home_name.to_owned(), home.to_owned()),
            (user_name.to_owned(), self.user.clone()),
        ];
        ProcessLaunch::from_spec(spec, print_argv(), environment).map_err(ClaudeRejection::Launch)
    }

    /// The fingerprint a lead agent turn (M2) is frozen against: this adapter's, with the
    /// turn parser in place of the developer one.
    pub fn turn_fingerprint(&self) -> ProviderFingerprint {
        let mut fingerprint = self.fingerprint.clone();
        fingerprint.parser = jojo_domain::planner::turn_parser(fingerprint.provider).into();
        fingerprint
    }

    /// Frozen turn → the one allowed read-only OS request. No side effect.
    pub fn prepare_turn(
        &self,
        spec: &jojo_domain::planner::TurnSpec,
    ) -> Result<ProcessLaunch, ClaudeRejection> {
        if spec.fingerprint != self.turn_fingerprint() {
            return Err(ClaudeRejection::Fingerprint);
        }
        self.ready()?;
        let Some(home) = self.home.to_str() else {
            return Err(ClaudeRejection::Home("path is not UTF-8".into()));
        };
        let [home_name, user_name] = CLAUDE_VARIABLES;
        let environment = vec![
            (home_name.to_owned(), home.to_owned()),
            (user_name.to_owned(), self.user.clone()),
        ];
        // Follow-ups: a chat turn keeps or continues its session; the planner says how.
        let args = jojo_domain::planner::turn_argv(spec)
            .map_err(|e| ClaudeRejection::Launch(LaunchRejection::Spec(e)))?;
        ProcessLaunch::for_turn(spec, args, environment).map_err(ClaudeRejection::Launch)
    }
}

impl ProviderAdapter for ClaudeAdapter {
    type Prepared = ProcessLaunch;

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn prepare(&self, spec: LaunchSpec) -> Result<ProcessLaunch, ProviderError> {
        self.prepare_launch(&spec)
            .map_err(|rejection| match rejection {
                ClaudeRejection::Authentication => ProviderError::AuthenticationDenied,
                ClaudeRejection::Home(_) => ProviderError::NotReady,
                ClaudeRejection::Launch(LaunchRejection::Spec(e)) => e,
                ClaudeRejection::Launch(
                    LaunchRejection::Argument(_)
                    | LaunchRejection::Environment(_)
                    | LaunchRejection::WorkingDirectory(_),
                ) => ProviderError::InvalidSpec,
                ClaudeRejection::Executable(_)
                | ClaudeRejection::Version
                | ClaudeRejection::Digest
                | ClaudeRejection::Fingerprint
                | ClaudeRejection::Launch(LaunchRejection::Executable(_)) => {
                    ProviderError::FingerprintMismatch
                }
            })
    }
}

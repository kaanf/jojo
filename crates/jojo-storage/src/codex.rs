//! Production Codex adapter (P2-01): filesystem validation and translation only.
//!
//! Nothing here starts a process, reads a credential, a Keychain item or any file inside
//! the Codex home, or opens a network connection. The adapter turns a frozen `LaunchSpec`
//! into the one `ProcessLaunch` the Codex contract allows (`jojo_domain::codex`) after
//! proving that the configured executable is the approved file:
//!
//! * an explicit, fully resolved absolute path — never a `PATH` lookup, a symbolic link
//!   or a script launcher (the Homebrew `codex` entry is a Node script that cannot run
//!   under the cleared launch environment and would execute whatever `node` resolves to);
//! * a regular native executable that only its owner can modify;
//! * bytes whose SHA-256 equals the digest bound in the provider fingerprint, so an
//!   update of the installed package invalidates the fingerprint.
//!
//! The version is an attested value bound to that digest: Jojo does not run `--version`.
//! Sign-in state is likewise attested by the human, never inspected. Every check is
//! repeated by [`verify_launch`] when the request is rebuilt at the launch boundary, so a
//! launch that names this profile cannot be issued with other arguments, another
//! environment or a changed executable, whichever code constructs it.
use std::fs::File;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

use jojo_domain::codex::{
    CODEX_EXEC_JSONL, CODEX_HOME_VARIABLE, CODEX_VERSION, check_environment, check_exec_argv,
    check_spec, configuration, configured_digest, exec_argv, unverified_capabilities,
};
use jojo_domain::provider::{
    AuthMode, Capabilities, LaunchSpec, PROVIDER_SCHEMA, ProviderAdapter, ProviderError,
    ProviderFingerprint, ProviderKind,
};

use crate::digest::sha256_reader;
use crate::process::{LaunchRejection, ProcessLaunch};

/// Upper bound on the executable that is hashed; a larger file is refused, not sampled.
pub const MAX_CODEX_EXECUTABLE_BYTES: u64 = 1024 * 1024 * 1024;

/// Everything is supplied explicitly by the human-approved configuration. No field is
/// discovered from the ambient process, and none holds a secret.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexAdapterConfig {
    /// The native binary itself, fully resolved.
    pub executable: PathBuf,
    /// `codex --version` text as recorded by the human for this file.
    pub version: String,
    /// Approved lower-case hexadecimal SHA-256 of the executable.
    pub sha256: String,
    /// The explicitly selected Codex home. Located, never read.
    pub home: PathBuf,
    /// Sign-in mode confirmed by the human. Anything but a native subscription denies.
    pub auth: AuthMode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodexRejection {
    Executable(String),
    /// Not the release the parser profile was written against.
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
pub struct CodexAdapter {
    fingerprint: ProviderFingerprint,
    home: PathBuf,
    auth: AuthMode,
    capabilities: Capabilities,
}

fn resolved(path: &Path) -> bool {
    path.is_absolute() && std::fs::canonicalize(path).is_ok_and(|real| real == path)
}

/// SHA-256 of a native executable that passes every structural check.
fn inspect_executable(path: &Path) -> Result<String, String> {
    let listed = std::fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if listed.file_type().is_symlink() {
        return Err("symbolic link; configure the native binary itself".into());
    }
    if !listed.is_file() {
        return Err("not a regular file".into());
    }
    if !resolved(path) {
        return Err("path is not absolute and fully resolved".into());
    }
    let mode = listed.permissions().mode();
    if mode & 0o111 == 0 || mode & 0o6022 != 0 {
        return Err("must be executable, not set-id and writable by its owner only".into());
    }
    if listed.len() > MAX_CODEX_EXECUTABLE_BYTES {
        return Err("larger than the inspection limit".into());
    }
    let mut file = File::open(path).map_err(|e| format!("open: {e}"))?;
    let opened = file.metadata().map_err(|e| format!("inspect: {e}"))?;
    if (opened.dev(), opened.ino()) != (listed.dev(), listed.ino()) {
        return Err("replaced during inspection".into());
    }
    let (digest, head) = sha256_reader(&mut file, MAX_CODEX_EXECUTABLE_BYTES)
        .map_err(|e| format!("read: {e}"))?
        .ok_or("empty, truncated or larger than the inspection limit")?;
    // Mach-O 64-bit, or a universal (fat) container. A `#!` launcher is not accepted.
    if !matches!(
        head,
        [0xcf, 0xfa, 0xed, 0xfe] | [0xca, 0xfe, 0xba, 0xbe] | [0xca, 0xfe, 0xba, 0xbf]
    ) {
        return Err("not a native executable; script launchers are refused".into());
    }
    Ok(digest)
}

/// The home is only located: its entries are never listed, opened or hashed.
fn inspect_home(home: &Path) -> Result<(), String> {
    let listed = std::fs::symlink_metadata(home).map_err(|e| format!("{}: {e}", home.display()))?;
    if !listed.is_dir() || !resolved(home) {
        return Err("must be an existing, fully resolved directory".into());
    }
    if listed.permissions().mode() & 0o002 != 0 {
        return Err("must not be world-writable".into());
    }
    Ok(())
}

/// The Codex launch contract at the OS boundary. `ProcessLaunch::from_spec` calls this for
/// every request that names the production Codex parser, before any issue journal entry.
pub(crate) fn verify_launch(
    spec: &LaunchSpec,
    args: &[String],
    environment: &[(String, String)],
) -> Result<(), LaunchRejection> {
    check_spec(spec).map_err(LaunchRejection::Spec)?;
    check_exec_argv(args, &spec.cwd).map_err(|_| {
        LaunchRejection::Argument("not the allowlisted Codex argument vector".into())
    })?;
    let home = check_environment(spec, environment)
        .map_err(|_| LaunchRejection::Environment(CODEX_HOME_VARIABLE.into()))?;
    inspect_home(home).map_err(LaunchRejection::Environment)?;
    // Lexical separation was checked on the spec; repeat it on what the paths resolve to.
    for other in [&spec.cwd, &spec.raw_output_dir, &spec.source.common_dir] {
        if std::fs::canonicalize(other)
            .is_ok_and(|real| real.starts_with(home) || home.starts_with(&real))
        {
            return Err(LaunchRejection::Environment(
                "Codex home overlaps the worktree, repository or output directory".into(),
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

impl CodexAdapter {
    /// Validates the configured executable and home. Reads the executable's bytes once to
    /// hash them; executes nothing.
    pub fn new(config: CodexAdapterConfig) -> Result<Self, CodexRejection> {
        if config.version != CODEX_VERSION {
            return Err(CodexRejection::Version);
        }
        let digest = inspect_executable(&config.executable).map_err(CodexRejection::Executable)?;
        if digest != config.sha256 {
            return Err(CodexRejection::Digest);
        }
        inspect_home(&config.home).map_err(CodexRejection::Home)?;
        Ok(Self {
            fingerprint: ProviderFingerprint {
                provider: ProviderKind::Codex,
                executable: config.executable,
                version: config.version,
                schema: PROVIDER_SCHEMA,
                parser: CODEX_EXEC_JSONL.into(),
                configuration: configuration(&digest).ok_or(CodexRejection::Digest)?,
            },
            home: config.home,
            auth: config.auth,
            capabilities: unverified_capabilities(),
        })
    }

    /// The fingerprint a `LaunchSpec` must be frozen against.
    pub fn fingerprint(&self) -> &ProviderFingerprint {
        &self.fingerprint
    }

    /// Whether a launch may be prepared at all: sign-in must be an attested native
    /// subscription. Checked again by [`Self::prepare_launch`].
    pub fn ready(&self) -> Result<(), CodexRejection> {
        if self.auth != AuthMode::NativeSubscription {
            return Err(CodexRejection::Authentication);
        }
        Ok(())
    }

    /// Frozen spec → the one allowed OS request. No side effect; nothing is spawned.
    pub fn prepare_launch(&self, spec: &LaunchSpec) -> Result<ProcessLaunch, CodexRejection> {
        if spec.fingerprint != self.fingerprint {
            return Err(CodexRejection::Fingerprint);
        }
        if self.auth != AuthMode::NativeSubscription {
            return Err(CodexRejection::Authentication);
        }
        let args =
            exec_argv(&spec.cwd).map_err(|e| CodexRejection::Launch(LaunchRejection::Spec(e)))?;
        let Some(home) = self.home.to_str() else {
            return Err(CodexRejection::Home("path is not UTF-8".into()));
        };
        let environment = vec![(CODEX_HOME_VARIABLE.to_owned(), home.to_owned())];
        ProcessLaunch::from_spec(spec, args, environment).map_err(CodexRejection::Launch)
    }
}

impl ProviderAdapter for CodexAdapter {
    type Prepared = ProcessLaunch;

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    fn prepare(&self, spec: LaunchSpec) -> Result<ProcessLaunch, ProviderError> {
        self.prepare_launch(&spec)
            .map_err(|rejection| match rejection {
                CodexRejection::Authentication => ProviderError::AuthenticationDenied,
                CodexRejection::Home(_) => ProviderError::NotReady,
                CodexRejection::Launch(LaunchRejection::Spec(e)) => e,
                CodexRejection::Launch(
                    LaunchRejection::Argument(_)
                    | LaunchRejection::Environment(_)
                    | LaunchRejection::WorkingDirectory(_),
                ) => ProviderError::InvalidSpec,
                CodexRejection::Executable(_)
                | CodexRejection::Version
                | CodexRejection::Digest
                | CodexRejection::Fingerprint
                | CodexRejection::Launch(LaunchRejection::Executable(_)) => {
                    ProviderError::FingerprintMismatch
                }
            })
    }
}

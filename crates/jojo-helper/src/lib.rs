//! Helper boundary: configuration parsing (P1-01) and the engine ownership probe (P1-02).
//! No scheduler, IPC protocol, provider adapter or child-process services.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use jojo_domain::identity::{ClockEpochId, OpaqueId};
use jojo_domain::paths::{LocalRoots, RootError};
use jojo_domain::time::{Clock, ClockReading};
use jojo_storage::{Engine, EngineConfig, ShutdownMarker, StorageError, prepare_data_root};

pub mod service;

#[derive(Debug, Eq, PartialEq)]
pub enum Invocation {
    Serve(LocalRoots),
    /// `--serve` with the P2-02 slice configuration. Everything a Start may launch is
    /// fixed here, by whoever starts the helper; none of it can arrive over the protocol.
    ServeSlice(LocalRoots, SliceOptions),
    Help,
    Version,
    CheckConfig(LocalRoots),
    /// Own the database until standard input reaches end-of-file, then shut down cleanly.
    EngineProbe(LocalRoots),
    /// P2-04.1: `--agent-guard <leader> <inspector>`. Started only by a serving helper,
    /// inside the process group it guards; never part of the protocol.
    AgentGuard {
        leader: u32,
        inspector: PathBuf,
    },
}

/// The one root under which a repository may be registered, and optionally the one
/// Codex executable a Start may launch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SliceOptions {
    pub disposable_root: PathBuf,
    pub codex: Option<CodexOptions>,
}

/// Human-approved Codex identity: explicit native binary, recorded version, approved
/// digest, explicitly selected home, and the human's attestation of native sign-in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexOptions {
    pub executable: PathBuf,
    pub version: String,
    pub sha256: String,
    pub home: PathBuf,
}

#[derive(Debug, Eq, PartialEq)]
pub enum ConfigError {
    Usage,
    UnknownArgument,
    DuplicateArgument,
    MissingValue,
    MissingHome,
    InvalidRoots(RootError),
}

/// Pure argument parsing. HOME is explicitly supplied by the executable boundary.
/// Explicit roots work without HOME; no directories are created or inspected.
pub fn parse_invocation(
    arguments: impl IntoIterator<Item = OsString>,
    home: Option<PathBuf>,
) -> Result<Invocation, ConfigError> {
    let mut arguments = arguments.into_iter();
    let first = arguments.next().ok_or(ConfigError::Usage)?;
    if first == "--help" || first == "--version" {
        if arguments.next().is_some() {
            return Err(ConfigError::UnknownArgument);
        }
        return Ok(if first == "--help" {
            Invocation::Help
        } else {
            Invocation::Version
        });
    }
    if first == jojo_storage::GUARD_ARGUMENT {
        let leader = arguments.next().and_then(|v| v.into_string().ok());
        let leader = leader
            .and_then(|v| v.parse().ok())
            .ok_or(ConfigError::Usage)?;
        let inspector = PathBuf::from(arguments.next().ok_or(ConfigError::MissingValue)?);
        if arguments.next().is_some() || !inspector.is_absolute() {
            return Err(ConfigError::UnknownArgument);
        }
        return Ok(Invocation::AgentGuard { leader, inspector });
    }
    let command: fn(LocalRoots) -> Invocation = if first == "--check-config" {
        Invocation::CheckConfig
    } else if first == "--serve" {
        Invocation::Serve
    } else if first == "--engine-probe" {
        Invocation::EngineProbe
    } else {
        return Err(ConfigError::UnknownArgument);
    };
    let serve = first == "--serve";
    let mut data = None;
    let mut worktrees = None;
    // Slice options, in order: disposable root, Codex executable, version, digest, home,
    // sign-in attestation. Accepted only with --serve.
    let mut slice: [Option<OsString>; 6] = Default::default();
    const SLICE: [&str; 6] = [
        "--disposable-root",
        "--codex-executable",
        "--codex-version",
        "--codex-sha256",
        "--codex-home",
        "--codex-auth",
    ];
    while let Some(argument) = arguments.next() {
        let value = arguments.next();
        if let Some(i) = SLICE.iter().position(|name| argument == *name) {
            if !serve {
                return Err(ConfigError::UnknownArgument);
            }
            if slice[i].is_some() {
                return Err(ConfigError::DuplicateArgument);
            }
            slice[i] = Some(value.ok_or(ConfigError::MissingValue)?);
            continue;
        }
        let destination = if argument == "--data-root" {
            &mut data
        } else if argument == "--worktree-root" {
            &mut worktrees
        } else {
            return Err(ConfigError::UnknownArgument);
        };
        if destination.is_some() {
            return Err(ConfigError::DuplicateArgument);
        }
        *destination = Some(PathBuf::from(value.ok_or(ConfigError::MissingValue)?));
    }
    let [disposable, executable, version, sha256, codex_home, auth] = slice;
    let text = |v: OsString| v.into_string().map_err(|_| ConfigError::Usage);
    let codex = match (executable, version, sha256, codex_home, auth) {
        (None, None, None, None, None) => None,
        (Some(executable), Some(version), Some(sha256), Some(codex_home), Some(auth)) => {
            // The only accepted attestation. There is no API-key or unknown mode to pass.
            if auth != "native-subscription" {
                return Err(ConfigError::Usage);
            }
            Some(CodexOptions {
                executable: executable.into(),
                version: text(version)?,
                sha256: text(sha256)?,
                home: codex_home.into(),
            })
        }
        _ => return Err(ConfigError::Usage),
    };
    let slice = match (disposable, codex) {
        (None, None) => None,
        (Some(root), codex) => Some(SliceOptions {
            disposable_root: root.into(),
            codex,
        }),
        (None, Some(_)) => return Err(ConfigError::Usage),
    };
    let defaults = if data.is_none() || worktrees.is_none() {
        Some(
            LocalRoots::for_macos_home(home.as_deref().ok_or(ConfigError::MissingHome)?)
                .map_err(ConfigError::InvalidRoots)?,
        )
    } else {
        None
    };
    let data = data
        .or_else(|| defaults.as_ref().map(|r| r.data().to_path_buf()))
        .ok_or(ConfigError::MissingHome)?;
    let worktrees = worktrees
        .or_else(|| defaults.as_ref().map(|r| r.worktrees().to_path_buf()))
        .ok_or(ConfigError::MissingHome)?;
    let roots = LocalRoots::new(data, worktrees).map_err(ConfigError::InvalidRoots)?;
    Ok(match slice {
        Some(options) => Invocation::ServeSlice(roots, options),
        None => command(roots),
    })
}

/// Exit status of [`run_engine_probe`] when another live helper owns the database.
pub const EXIT_OWNERSHIP_HELD: u8 = 3;
/// Exit status of [`run_engine_probe`] for any other storage failure.
pub const EXIT_STORAGE_FAILURE: u8 = 4;

/// Wall-clock audit timestamps for the helper process. Monotonic readings are scoped to
/// this process (the epoch identity is its start time) and are not comparable across runs.
pub struct SystemClock {
    started: Instant,
    epoch: ClockEpochId,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            epoch: ClockEpochId(OpaqueId(unix_nanos())),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

fn unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos())
}

impl Clock for SystemClock {
    fn now(&self) -> ClockReading {
        ClockReading {
            unix_millis: i64::try_from(unix_nanos() / 1_000_000).unwrap_or(i64::MAX),
            monotonic_nanos: u64::try_from(self.started.elapsed().as_nanos()).unwrap_or(u64::MAX),
            epoch: self.epoch,
        }
    }
}

/// Becomes the single owner of the database under the data root, reports the new engine
/// epoch on `log`, holds ownership until `input` reaches end-of-file (the GUI going away),
/// then records a clean shutdown. Input bytes are ignored: there is no command protocol
/// yet (P1-13), admission is never opened here and nothing is launched.
///
/// Returns the process exit status: 0 after a clean shutdown, [`EXIT_OWNERSHIP_HELD`]
/// when another helper is alive (nothing is written), otherwise [`EXIT_STORAGE_FAILURE`].
pub fn run_engine_probe(roots: &LocalRoots, mut input: impl Read, mut log: impl Write) -> u8 {
    let started = prepare_data_root(roots.data()).and_then(|db_path| {
        Engine::start(EngineConfig::new(db_path, Arc::new(SystemClock::new())))
    });
    let engine = match started {
        Ok(engine) => engine,
        Err(StorageError::OwnershipHeld { owner_hint, .. }) => {
            let _ = writeln!(
                log,
                "jojo-helper: database is owned by another helper ({owner_hint}); not starting"
            );
            return EXIT_OWNERSHIP_HELD;
        }
        Err(error) => {
            let _ = writeln!(log, "jojo-helper: storage failure: {error}");
            return EXIT_STORAGE_FAILURE;
        }
    };
    let report = engine.startup_report();
    let previous = match report.epoch.previous {
        None => "none",
        Some((_, ShutdownMarker::Clean)) => "clean",
        Some(_) => "unclean",
    };
    let admission = engine
        .reader()
        .and_then(|mut reader| reader.snapshot())
        .map(|snapshot| format!("{:?}", snapshot.session.admission).to_lowercase());
    let admission = match admission {
        Ok(admission) => admission,
        Err(error) => {
            let _ = writeln!(log, "jojo-helper: storage failure: {error}");
            return EXIT_STORAGE_FAILURE;
        }
    };
    let _ = writeln!(
        log,
        "jojo-helper: engine ready epoch={} schema={} admission={admission} previous={previous} quarantined_claims={}",
        report.epoch.epoch.0, report.migration.to_version, report.epoch.quarantined_claims
    );
    // Hold ownership for as long as the controlling pipe is open.
    let _ = std::io::copy(&mut input, &mut std::io::sink());
    match engine.shutdown() {
        Ok(()) => {
            let _ = writeln!(log, "jojo-helper: clean shutdown recorded");
            0
        }
        Err(error) => {
            let _ = writeln!(log, "jojo-helper: shutdown not clean: {error}");
            EXIT_STORAGE_FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn explicit_roots_do_not_require_a_host_home() {
        let invocation = parse_invocation(
            args(&[
                "--check-config",
                "--data-root",
                "/test/data",
                "--worktree-root",
                "/test/worktrees",
            ]),
            None,
        )
        .unwrap();
        let Invocation::CheckConfig(roots) = invocation else {
            panic!("wrong invocation")
        };
        assert_eq!(roots.data(), std::path::Path::new("/test/data"));
    }

    #[test]
    fn incomplete_configuration_does_not_guess_a_real_home() {
        assert_eq!(
            parse_invocation(args(&["--check-config"]), None),
            Err(ConfigError::MissingHome)
        );
    }

    #[test]
    fn missing_duplicate_and_unrecognized_arguments_fail_closed() {
        assert_eq!(
            parse_invocation(args(&["--check-config", "--data-root"]), None),
            Err(ConfigError::MissingValue)
        );
        assert_eq!(
            parse_invocation(
                args(&["--check-config", "--data-root", "/a", "--data-root", "/b"]),
                None
            ),
            Err(ConfigError::DuplicateArgument)
        );
        assert_eq!(
            parse_invocation(args(&["--run"]), None),
            Err(ConfigError::UnknownArgument)
        );
        assert_eq!(parse_invocation(args(&[]), None), Err(ConfigError::Usage));
    }

    #[test]
    fn the_engine_probe_takes_the_same_explicit_roots() {
        let invocation = parse_invocation(
            args(&[
                "--engine-probe",
                "--data-root",
                "/test/data",
                "--worktree-root",
                "/test/worktrees",
            ]),
            None,
        )
        .unwrap();
        assert!(matches!(invocation, Invocation::EngineProbe(_)));
        assert_eq!(
            parse_invocation(args(&["--engine-probe"]), None),
            Err(ConfigError::MissingHome)
        );
    }

    #[test]
    fn default_roots_are_derived_from_only_the_supplied_test_home() {
        let actual =
            parse_invocation(args(&["--check-config"]), Some("/test/home".into())).unwrap();
        assert_eq!(
            actual,
            Invocation::CheckConfig(
                LocalRoots::for_macos_home(std::path::Path::new("/test/home")).unwrap()
            )
        );
    }

    const SLICE: [&str; 17] = [
        "--serve",
        "--data-root",
        "/test/data",
        "--worktree-root",
        "/test/worktrees",
        "--disposable-root",
        "/test/disposable",
        "--codex-executable",
        "/test/bin/codex",
        "--codex-version",
        "codex-cli 0.169.0",
        "--codex-sha256",
        "00",
        "--codex-home",
        "/test/codex-home",
        "--codex-auth",
        "native-subscription",
    ];

    #[test]
    fn p2_04_1_guard_mode_takes_exactly_a_leader_and_an_absolute_inspector() {
        let parse = |args: &[&str]| parse_invocation(args.iter().map(OsString::from), None);
        assert_eq!(
            parse(&["--agent-guard", "4242", "/bin/ps"]),
            Ok(Invocation::AgentGuard {
                leader: 4242,
                inspector: PathBuf::from("/bin/ps")
            })
        );
        for bad in [
            &["--agent-guard"][..],
            &["--agent-guard", "4242"],
            &["--agent-guard", "-1", "/bin/ps"],
            &["--agent-guard", "codex", "/bin/ps"],
            &["--agent-guard", "4242", "ps"],
            &["--agent-guard", "4242", "/bin/ps", "extra"],
            &["--serve", "--agent-guard", "4242"],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn p2_02_slice_options_are_explicit_complete_and_only_for_serve() {
        let Ok(Invocation::ServeSlice(roots, options)) = parse_invocation(args(&SLICE), None)
        else {
            panic!("slice invocation")
        };
        assert_eq!(roots.data(), std::path::Path::new("/test/data"));
        assert_eq!(
            options,
            SliceOptions {
                disposable_root: "/test/disposable".into(),
                codex: Some(CodexOptions {
                    executable: "/test/bin/codex".into(),
                    version: "codex-cli 0.169.0".into(),
                    sha256: "00".into(),
                    home: "/test/codex-home".into(),
                }),
            }
        );
        // A disposable root alone enables registration and import, with no adapter.
        let Ok(Invocation::ServeSlice(_, options)) = parse_invocation(args(&SLICE[..7]), None)
        else {
            panic!("slice invocation")
        };
        assert_eq!(options.codex, None);
        // Without slice options the invocation is exactly the Phase 1 one.
        assert!(matches!(
            parse_invocation(args(&SLICE[..5]), None),
            Ok(Invocation::Serve(_))
        ));
        // An incomplete Codex identity, an adapter without a disposable root, any sign-in
        // attestation other than a native subscription, and duplicates are refused.
        for end in [9, 11, 13, 15] {
            assert_eq!(
                parse_invocation(args(&SLICE[..end]), None),
                Err(ConfigError::Usage),
                "{end}"
            );
        }
        let without_root: Vec<&str> = SLICE[..5].iter().chain(&SLICE[7..]).copied().collect();
        assert_eq!(
            parse_invocation(args(&without_root), None),
            Err(ConfigError::Usage)
        );
        for auth in ["api-key", "unknown", "", "NATIVE-SUBSCRIPTION"] {
            let mut wrong = SLICE.to_vec();
            wrong[16] = auth;
            assert_eq!(
                parse_invocation(args(&wrong), None),
                Err(ConfigError::Usage),
                "{auth:?}"
            );
        }
        let mut twice = SLICE.to_vec();
        twice.extend(["--disposable-root", "/other"]);
        assert_eq!(
            parse_invocation(args(&twice), None),
            Err(ConfigError::DuplicateArgument)
        );
        assert_eq!(
            parse_invocation(args(&SLICE[..16]), None),
            Err(ConfigError::MissingValue)
        );
        // Not accepted by any other mode, and no free-form launch option exists.
        for mode in ["--check-config", "--engine-probe"] {
            let mut other = SLICE.to_vec();
            other[0] = mode;
            assert_eq!(
                parse_invocation(args(&other), None),
                Err(ConfigError::UnknownArgument)
            );
        }
        for unknown in [
            "--codex-arg",
            "--codex-env",
            "--executable",
            "--process-inspector",
        ] {
            let mut extra = SLICE.to_vec();
            extra.extend([unknown, "x"]);
            assert_eq!(
                parse_invocation(args(&extra), None),
                Err(ConfigError::UnknownArgument),
                "{unknown}"
            );
        }
    }
}

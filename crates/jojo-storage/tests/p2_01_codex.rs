//! P2-01 Codex adapter and launch envelope, offline. The "Codex" here is always a private
//! copy of the workspace's `jojo-codex-fixture`, launched through the real adapter, issue
//! journal and supervised runner on a temporary Git repository. The installed Codex CLI
//! is never named, located or executed; no credential, network or inference is involved.
mod common;
use common::attempts::*;
use jojo_domain::codex::*;
use jojo_domain::identity::*;
use jojo_domain::provider::*;
use jojo_storage::*;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const CODEX_FIXTURE: &str = env!("CARGO_BIN_EXE_jojo-codex-fixture");
const ATTEMPT: u128 = 101;
const ROLE: &str = "JOJO_P2_01_ROLE";
/// Set by the gate: every executable these tests get past the issue journal is appended.
const LEDGER: &str = "JOJO_P2_01_LEDGER";
const CANARY: &str = "jojo-p2-01-canary-not-a-credential";
const CANARIES: [&str; 9] = [
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "OPENAI_ORGANIZATION",
    "CODEX_API_KEY",
    "CODEX_ACCESS_TOKEN",
    "CODEX_HOME",
    "HOME",
    "HTTPS_PROXY",
    "NODE_OPTIONS",
];

fn chmod(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

/// One reserved Codex attempt with a bound worktree, a private fixture executable and an
/// empty selected Codex home, all under one fully resolved temporary directory.
struct C {
    f: F,
    base: PathBuf,
    fixture: PathBuf,
    home: PathBuf,
    sha256: String,
}

impl C {
    fn new() -> Self {
        let f = F::with_providers(&[ProviderKind::Codex]);
        let base = fs::canonicalize(f.dir.path()).unwrap();
        fs::create_dir(base.join("bin")).unwrap();
        let fixture = base.join("bin/codex");
        fs::copy(CODEX_FIXTURE, &fixture).unwrap();
        chmod(&fixture, 0o755);
        let home = base.join("codex-home");
        fs::create_dir(&home).unwrap();
        chmod(&home, 0o700);
        let sha256 = sha256_hex(&[&fs::read(&fixture).unwrap()]);
        Self {
            f,
            base,
            fixture,
            home,
            sha256,
        }
    }

    fn config(&self) -> CodexAdapterConfig {
        CodexAdapterConfig {
            executable: self.fixture.clone(),
            version: CODEX_VERSION.into(),
            sha256: self.sha256.clone(),
            home: self.home.clone(),
            auth: AuthMode::NativeSubscription,
        }
    }

    fn adapter(&self) -> CodexAdapter {
        CodexAdapter::new(self.config()).unwrap()
    }

    /// The reserved attempt's spec, frozen against the adapter's fingerprint.
    fn spec(&self, scenario: &str) -> LaunchSpec {
        let mut spec = self.f.spec(ATTEMPT, 1, scenario);
        spec.fingerprint = self.adapter().fingerprint().clone();
        spec.environment_allowlist = vec![CODEX_HOME_VARIABLE.into()];
        spec
    }

    fn environment(&self) -> Vec<(String, String)> {
        vec![(
            CODEX_HOME_VARIABLE.to_owned(),
            self.home.display().to_string(),
        )]
    }

    fn golden(&self, spec: &LaunchSpec) -> Vec<String> {
        exec_argv(&spec.cwd).unwrap()
    }

    /// Issue journal → supervised runner, exactly as production would drive them.
    fn launch(&self, spec: &LaunchSpec, launch: &ProcessLaunch) -> LaunchReport {
        let ids = self.f.ids();
        let clock = RealClock(Instant::now());
        let ctx = LaunchContext {
            engine: self.f.e(),
            ids: &ids,
            clock: &clock,
            poll: Duration::from_millis(10),
        };
        let plan = StopPlan::new(&unverified_capabilities(), BUDGETS);
        let hooks = LaunchHooks {
            inspector: Some(ps()),
            ..LaunchHooks::default()
        };
        let report = run_launch(&ctx, spec, launch, &plan, hooks).unwrap();
        // Launch ledger: what was actually handed to the OS, by content.
        if let Some(ledger) = std::env::var_os(LEDGER)
            && !matches!(report.outcome, LaunchOutcome::Refused(_))
        {
            let digest = fs::read(&launch.executable)
                .map(|bytes| sha256_hex(&[&bytes]))
                .unwrap_or_else(|_| "unreadable".into());
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(ledger)
                .unwrap();
            let scenario = String::from_utf8_lossy(&spec.prompt);
            writeln!(file, "{digest}\t{scenario}").unwrap();
        }
        report
    }

    fn run(&self, scenario: &str) -> (LaunchReport, ProcessLaunch) {
        let spec = self.spec(scenario);
        let launch = self.adapter().prepare_launch(&spec).unwrap();
        (self.launch(&spec, &launch), launch)
    }

    fn issued(&self) -> bool {
        self.f.read(|v| v.process_launch_issued(OpaqueId(ATTEMPT)))
    }

    fn events(&self) -> Vec<StoredProviderEvent> {
        self.f.read(|v| v.provider_events(OpaqueId(ATTEMPT)))
    }

    fn kinds(&self) -> Vec<String> {
        self.events().into_iter().map(|e| e.kind).collect()
    }

    fn observed(&self, kind: &str) -> Vec<String> {
        self.f
            .read(|v| v.process_observations(OpaqueId(ATTEMPT)))
            .into_iter()
            .filter(|(k, _)| k == kind)
            .map(|(_, detail)| detail)
            .collect()
    }
}

fn absent(report: &LaunchReport) -> bool {
    matches!(report.outcome, LaunchOutcome::TreeAbsent { .. })
}

fn text(payload: &str) -> String {
    jojo_domain::payload::provider_event(&EventPayload::TextDelta(payload.into())).encode()
}

#[test]
fn p2_01_adapter_prepares_the_golden_launch_and_the_child_sees_exactly_it() {
    let c = C::new();
    let adapter = c.adapter();
    let fingerprint = adapter.fingerprint();
    assert_eq!(fingerprint.provider, ProviderKind::Codex);
    assert_eq!(fingerprint.executable, c.fixture);
    assert_eq!(fingerprint.version, "codex-cli 0.169.0");
    assert_eq!(fingerprint.parser, "jojo.codex.exec-jsonl.0.169.0/1");
    assert_eq!(
        fingerprint.configuration,
        format!("jojo.codex.exec/1;sha256={}", c.sha256)
    );
    assert_eq!(adapter.capabilities(), &unverified_capabilities());
    // Unknown interrupt support: the stop plan is signal-only, TERM then KILL.
    let plan = StopPlan::new(adapter.capabilities(), BUDGETS);
    assert_eq!(
        plan.steps.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
        [Signal::Terminate, Signal::Kill]
    );

    let spec = c.spec("report");
    let launch = adapter.prepare_launch(&spec).unwrap();
    let worktree = spec.cwd.display().to_string();
    let golden = [
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
        worktree.as_str(),
    ];
    assert_eq!(launch.args, golden);
    assert_eq!(launch.executable, c.fixture);
    assert_eq!(launch.cwd, spec.cwd);
    assert_eq!(launch.environment, c.environment());
    assert_eq!(launch.stdin, b"report");
    assert_eq!(launch.parser, CODEX_EXEC_JSONL);
    assert_eq!(adapter.prepare(spec.clone()), Ok(launch.clone()));
    assert!(!c.issued());

    let report = c.launch(&spec, &launch);
    assert!(absent(&report), "{:?}", report.outcome);
    assert_eq!(report.terminal, Some(TerminalStatus::Success));
    assert!(!report.protocol_failed && !report.evidence_lost);
    assert!(c.issued());
    let seen = format!(
        "argv={:?} variables={:?}",
        golden,
        [format!("CODEX_HOME={}", c.home.display())]
    );
    let event = c.events().into_iter().find(|e| e.kind == "TextDelta");
    assert_eq!(event.unwrap().payload, text(&seen));
    // The selected home was located, never written to or populated.
    assert_eq!(fs::read_dir(&c.home).unwrap().count(), 0);
}

/// Not a test of its own: runs only in the child spawned by the test below, whose
/// environment carries provider credentials, routing and home variables.
#[test]
#[ignore = "fixture role executed in a child process by the inherited-environment test"]
fn p2_01_canary_child() {
    if std::env::var(ROLE).as_deref() != Ok("canary") {
        return;
    }
    for name in CANARIES {
        assert_eq!(std::env::var(name).as_deref(), Ok(CANARY), "{name}");
    }
    let c = C::new();
    let (report, launch) = c.run("report");
    assert_eq!(report.terminal, Some(TerminalStatus::Success));
    assert_eq!(launch.environment, c.environment());
    let event = c.events().into_iter().find(|e| e.kind == "TextDelta");
    let payload = event.unwrap().payload;
    let only = format!("variables=[\\\"CODEX_HOME={}\\\"]", c.home.display());
    assert!(payload.contains(&only), "{payload}");
    assert!(!payload.contains(CANARY), "{payload}");
    for name in CANARIES.iter().filter(|n| **n != "CODEX_HOME") {
        let entry = format!("\"{name}=");
        assert!(!payload.contains(&entry), "{name} reached the child");
    }
}

#[test]
fn p2_01_inherited_provider_variables_never_reach_the_child() {
    let child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "p2_01_canary_child", "--ignored"])
        .env(ROLE, "canary")
        .envs(CANARIES.map(|name| (name, CANARY)))
        .output()
        .unwrap();
    let output = String::from_utf8_lossy(&child.stdout);
    assert!(
        child.status.success() && output.contains("test p2_01_canary_child ... ok"),
        "{output}\n{}",
        String::from_utf8_lossy(&child.stderr)
    );
}

#[test]
fn p2_01_symlink_script_wrong_digest_wrong_version_or_missing_executable_is_rejected() {
    let c = C::new();
    let digest_of = |path: &Path| sha256_hex(&[&fs::read(path).unwrap()]);
    let rejected = |config: CodexAdapterConfig| CodexAdapter::new(config).unwrap_err();
    let executable = |fixture_variant: PathBuf| {
        let sha256 = fs::read(&fixture_variant)
            .map(|b| sha256_hex(&[&b]))
            .unwrap_or_else(|_| c.sha256.clone());
        rejected(CodexAdapterConfig {
            executable: fixture_variant,
            sha256,
            ..c.config()
        })
    };
    let because = |rejection: CodexRejection, why: &str| match rejection {
        CodexRejection::Executable(detail) => assert!(detail.contains(why), "{detail}"),
        other => panic!("expected an executable rejection ({why}), got {other:?}"),
    };

    // A symbolic link to the real fixture: the Homebrew `bin/codex` shape.
    let link = c.base.join("bin/codex-link");
    std::os::unix::fs::symlink(&c.fixture, &link).unwrap();
    because(executable(link.clone()), "symbolic link");
    // A script launcher with a matching digest. It is hashed, never run.
    let script = c.base.join("bin/codex.js");
    fs::write(
        &script,
        "#!/usr/bin/env node\nprocess.exit(99) // fixture launcher\n",
    )
    .unwrap();
    chmod(&script, 0o755);
    because(executable(script.clone()), "script launchers are refused");
    because(
        executable(c.base.join("bin/absent")),
        "No such file or directory",
    );
    because(executable(c.base.join("bin")), "not a regular file");
    because(executable(PathBuf::from("bin/codex")), "No such file");
    // Reached through an unresolved directory link, or not in normal form.
    std::os::unix::fs::symlink(c.base.join("bin"), c.base.join("bin-link")).unwrap();
    because(
        executable(c.base.join("bin-link/codex")),
        "not absolute and fully resolved",
    );
    because(
        executable(c.base.join("bin/../bin/codex")),
        "not absolute and fully resolved",
    );
    for mode in [0o644, 0o775, 0o757, 0o4755, 0o2755] {
        let copy = c.base.join(format!("bin/codex-{mode:o}"));
        fs::copy(&c.fixture, &copy).unwrap();
        chmod(&copy, mode);
        // Under the gate's OS sandbox a set-id bit is silently not applied; such a file
        // is then an ordinary 0755 executable and there is nothing to refuse.
        if fs::metadata(&copy).unwrap().permissions().mode() & 0o7777 == mode {
            because(executable(copy), "writable by its owner only");
        }
    }
    let empty = c.base.join("bin/empty");
    fs::write(&empty, b"").unwrap();
    chmod(&empty, 0o755);
    because(executable(empty), "inspection limit");

    // Right file, wrong approved digest; a version text that is not one plain line. D3:
    // another release is recorded, not refused (a pinned one is refused by the helper).
    let other = "0".repeat(64);
    assert_eq!(
        rejected(CodexAdapterConfig {
            sha256: other,
            ..c.config()
        }),
        CodexRejection::Digest
    );
    for version in ["", "  ", "codex-cli\n0.170.0", &"9".repeat(129)] {
        assert_eq!(
            rejected(CodexAdapterConfig {
                version: version.into(),
                ..c.config()
            }),
            CodexRejection::Version,
            "{version:?}"
        );
    }
    for version in ["codex-cli 0.170.0", "codex-cli 0.169.1", "fixture-v1"] {
        let adapter = CodexAdapter::new(CodexAdapterConfig {
            version: version.into(),
            ..c.config()
        })
        .unwrap();
        assert_eq!(adapter.fingerprint().version, version);
    }
    // The selected home must be a real, resolved, non-world-writable directory.
    let home_link = c.base.join("home-link");
    std::os::unix::fs::symlink(&c.home, &home_link).unwrap();
    let open_home = c.base.join("open-home");
    fs::create_dir(&open_home).unwrap();
    chmod(&open_home, 0o777);
    for home in [
        home_link,
        open_home,
        c.base.join("no-home"),
        c.fixture.clone(),
    ] {
        let rejection = rejected(CodexAdapterConfig {
            home: home.clone(),
            ..c.config()
        });
        assert!(
            matches!(rejection, CodexRejection::Home(_)),
            "{home:?}: {rejection:?}"
        );
    }

    // A spec frozen against anything but the adapter's fingerprint is refused, and a
    // forged fingerprint is caught again where the OS request is built.
    let adapter = c.adapter();
    let spec = c.spec("ok");
    let forged = |edit: &dyn Fn(&mut ProviderFingerprint)| {
        let mut s = spec.clone();
        edit(&mut s.fingerprint);
        s
    };
    let forgeries: Vec<(LaunchSpec, &str)> = vec![
        (forged(&|f| f.executable = link.clone()), "symbolic link"),
        (
            forged(&|f| {
                f.executable = script.clone();
                f.configuration = configuration(&digest_of(&script)).unwrap();
            }),
            "script launchers are refused",
        ),
        (
            forged(&|f| f.configuration = configuration(&"0".repeat(64)).unwrap()),
            "digest differs from the approved fingerprint",
        ),
    ];
    for (bad, why) in forgeries {
        assert_eq!(
            adapter.prepare_launch(&bad),
            Err(CodexRejection::Fingerprint)
        );
        assert_eq!(
            adapter.prepare(bad.clone()),
            Err(ProviderError::FingerprintMismatch)
        );
        match ProcessLaunch::from_spec(&bad, c.golden(&bad), c.environment()) {
            Err(LaunchRejection::Executable(detail)) => assert!(detail.contains(why), "{detail}"),
            other => panic!("{why}: {other:?}"),
        }
    }
    let two_lines = forged(&|f| f.version = "codex-cli\n0.170.0".into());
    assert_eq!(
        ProcessLaunch::from_spec(&two_lines, c.golden(&spec), c.environment()),
        Err(LaunchRejection::Spec(ProviderError::FingerprintMismatch))
    );
    assert!(!c.issued());
}

#[test]
fn p2_01_executable_changed_after_approval_is_refused_before_the_issue_journal() {
    let c = C::new();
    let spec = c.spec("ok");
    let launch = c.adapter().prepare_launch(&spec).unwrap();
    let original = fs::read(&c.fixture).unwrap();
    let refused = |c: &C, what: &str| {
        let report = c.launch(&spec, &launch);
        assert_eq!(
            report.outcome,
            LaunchOutcome::Refused("process request differs from frozen LaunchSpec".into()),
            "{what}"
        );
        assert!(!c.issued(), "{what}: crossed the issue boundary");
        assert!(report.root.is_none() && report.events == 0, "{what}");
        assert!(!launch.spool.exists(), "{what}: spool was created");
        assert!(c.adapter_error().is_some(), "{what}: adapter still accepts");
    };

    // An update of the installed package changes the bytes.
    let mut updated = original.clone();
    updated.extend_from_slice(b"updated");
    fs::write(&c.fixture, &updated).unwrap();
    refused(&c, "changed bytes");
    // The path now resolves to a launcher script, then to a link, then to nothing.
    fs::write(&c.fixture, "#!/usr/bin/env node\n").unwrap();
    refused(&c, "script launcher");
    fs::remove_file(&c.fixture).unwrap();
    let elsewhere = c.base.join("bin/elsewhere");
    fs::write(&elsewhere, &original).unwrap();
    chmod(&elsewhere, 0o755);
    std::os::unix::fs::symlink(&elsewhere, &c.fixture).unwrap();
    refused(&c, "symbolic link");
    fs::remove_file(&c.fixture).unwrap();
    refused(&c, "missing file");

    // Restored byte for byte, the same frozen request is issued once and runs.
    fs::write(&c.fixture, &original).unwrap();
    chmod(&c.fixture, 0o755);
    let report = c.launch(&spec, &launch);
    assert!(absent(&report), "{:?}", report.outcome);
    assert_eq!(report.terminal, Some(TerminalStatus::Success));
    assert!(c.issued());
    // One attempt, one launch: a second Start of the same attempt is refused.
    let again = c.launch(&spec, &launch);
    assert!(
        matches!(again.outcome, LaunchOutcome::Refused(_)),
        "{:?}",
        again.outcome
    );
}

// D3: the digest is remembered per file identity and computed off the calling thread;
// a rewrite of the same length is still a new identity.
#[test]
fn d3_executable_digest_is_remembered_per_file_identity() {
    let c = C::new();
    let known = |path: &Path| {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            match begin_check(path) {
                ExecutableCheck::Known => return,
                ExecutableCheck::Checking => {}
                other => panic!("{other:?}"),
            }
            assert!(Instant::now() < deadline, "digest never became known");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    // A private copy: nothing in this process has hashed it.
    assert_eq!(begin_check(&c.fixture), ExecutableCheck::Checking);
    known(&c.fixture);
    assert_eq!(begin_check(&c.fixture), ExecutableCheck::Known);
    assert!(CodexAdapter::new(c.config()).is_ok());
    // Same length, other bytes: the identity changes and the old digest no longer holds.
    let mut changed = fs::read(&c.fixture).unwrap();
    let last = changed.len() - 1;
    changed[last] ^= 0xff;
    fs::write(&c.fixture, &changed).unwrap();
    assert_ne!(begin_check(&c.fixture), ExecutableCheck::Known);
    known(&c.fixture);
    assert_eq!(c.adapter_error(), Some(CodexRejection::Digest));
    // A file that fails a structural check is reported, never hashed.
    let link = c.base.join("bin/link");
    std::os::unix::fs::symlink(&c.fixture, &link).unwrap();
    assert!(matches!(begin_check(&link), ExecutableCheck::Unusable(_)));
}

impl C {
    fn adapter_error(&self) -> Option<CodexRejection> {
        CodexAdapter::new(self.config()).err()
    }
}

#[test]
fn p2_01_arguments_environment_and_envelope_cannot_be_altered_at_the_launch_boundary() {
    let c = C::new();
    let adapter = c.adapter();
    let spec = c.spec("ok");
    let golden = c.golden(&spec);
    let launch = adapter.prepare_launch(&spec).unwrap();
    let build = |args: Vec<String>, environment: Vec<(String, String)>| {
        ProcessLaunch::from_spec(&spec, args, environment)
    };
    assert_eq!(build(golden.clone(), c.environment()), Ok(launch.clone()));

    // Arguments: only the golden vector crosses, whoever builds the request.
    let mut vectors: Vec<Vec<String>> = vec![vec![], golden[..golden.len() - 1].to_vec()];
    for prohibited in PROHIBITED_ARGUMENTS {
        let mut appended = golden.clone();
        appended.push(prohibited.into());
        let mut first = golden.clone();
        first.insert(1, prohibited.into());
        vectors.extend([appended, first]);
    }
    let mut sandbox = golden.clone();
    sandbox[5] = "danger-full-access".into();
    let mut elsewhere = golden.clone();
    elsewhere[10] = c.base.display().to_string();
    vectors.extend([sandbox, elsewhere]);
    for args in vectors {
        assert!(
            matches!(
                build(args.clone(), c.environment()),
                Err(LaunchRejection::Argument(_))
            ),
            "{args:?}"
        );
        let mut altered = launch.clone();
        altered.args = args;
        let report = c.launch(&spec, &altered);
        assert!(matches!(report.outcome, LaunchOutcome::Refused(_)));
    }

    // Environment: exactly the selected home, outside the worktree, really a directory.
    let pair = |name: &str, value: &Path| (name.to_owned(), value.display().to_string());
    let inside = spec.cwd.join("codex-home");
    fs::create_dir(&inside).unwrap();
    let link = c.base.join("home-link");
    std::os::unix::fs::symlink(&c.home, &link).unwrap();
    let environments: Vec<Vec<(String, String)>> = vec![
        vec![],
        vec![
            pair(CODEX_HOME_VARIABLE, &c.home),
            pair("OPENAI_API_KEY", Path::new(CANARY)),
        ],
        vec![
            pair(CODEX_HOME_VARIABLE, &c.home),
            pair("LANG", Path::new("C")),
        ],
        vec![pair("HOME", &c.home)],
        vec![pair("OPENAI_API_KEY", Path::new(CANARY))],
        vec![pair(CODEX_HOME_VARIABLE, &inside)],
        vec![pair(
            CODEX_HOME_VARIABLE,
            &fs::canonicalize(&inside).unwrap(),
        )],
        vec![pair(CODEX_HOME_VARIABLE, &c.base)],
        vec![pair(CODEX_HOME_VARIABLE, &link)],
        vec![pair(CODEX_HOME_VARIABLE, &c.base.join("absent"))],
        vec![pair(CODEX_HOME_VARIABLE, &c.fixture)],
        vec![pair(CODEX_HOME_VARIABLE, Path::new("codex-home"))],
    ];
    for environment in environments {
        assert!(
            matches!(
                build(golden.clone(), environment.clone()),
                Err(LaunchRejection::Environment(_))
            ),
            "{environment:?}"
        );
        let mut altered = launch.clone();
        altered.environment = environment;
        let report = c.launch(&spec, &altered);
        assert!(matches!(report.outcome, LaunchOutcome::Refused(_)));
    }

    // Envelope: nothing the amendment did not name is accepted for this profile.
    let altered = |edit: &dyn Fn(&mut LaunchSpec)| {
        let mut s = spec.clone();
        edit(&mut s);
        s
    };
    let continuation = Continuation {
        predecessor: AttemptId(OpaqueId(7)),
        exact: SessionRef {
            provider: ProviderKind::Codex,
            session: "fixture-thread".into(),
            turn: "turn-1".into(),
        },
        previous_cwd: spec.cwd.clone(),
        reviewed_plan: "plan".into(),
    };
    use ProviderError::*;
    let specs: Vec<(&str, LaunchSpec, ProviderError)> = vec![
        (
            "continuation",
            altered(&|s| s.continuation = Some(continuation.clone())),
            UnsupportedContinuation,
        ),
        (
            "agent network",
            altered(&|s| s.permissions.network = true),
            InvalidSpec,
        ),
        (
            "hooks",
            altered(&|s| s.permissions.hooks = true),
            InvalidSpec,
        ),
        (
            "background",
            altered(&|s| s.permissions.background = true),
            InvalidSpec,
        ),
        (
            "second variable",
            altered(&|s| s.environment_allowlist.push("HOME".into())),
            InvalidSpec,
        ),
        (
            "locale variable",
            altered(&|s| s.environment_allowlist.push("LANG".into())),
            InvalidSpec,
        ),
        (
            "config reference",
            altered(&|s| s.config_references.push("profile".into())),
            InvalidSpec,
        ),
        (
            "validator",
            altered(&|s| s.role = Role::Validator),
            UnsupportedPermission,
        ),
        (
            "extra root",
            altered(&|s| s.permissions.filesystem_roots.push(c.base.clone())),
            UnsupportedPermission,
        ),
        (
            "tools",
            altered(&|s| s.permissions.tools.push("shell".into())),
            UnsupportedPermission,
        ),
    ];
    for (name, bad, expected) in specs {
        assert_eq!(
            adapter.prepare_launch(&bad),
            Err(CodexRejection::Launch(LaunchRejection::Spec(
                expected.clone()
            ))),
            "{name}"
        );
        assert_eq!(adapter.prepare(bad), Err(expected), "{name}");
    }

    // Sign-in mode is attested, never inspected; anything unconfirmed denies the launch.
    for auth in [
        AuthMode::Unknown,
        AuthMode::NotSignedIn,
        AuthMode::UnsupportedMode,
        AuthMode::ApiKey,
    ] {
        let denied = CodexAdapter::new(CodexAdapterConfig { auth, ..c.config() }).unwrap();
        assert_eq!(
            denied.prepare_launch(&spec),
            Err(CodexRejection::Authentication)
        );
        assert_eq!(
            denied.prepare(spec.clone()),
            Err(ProviderError::AuthenticationDenied)
        );
    }
    assert!(!c.issued());
    assert!(!launch.spool.exists());
}

#[test]
fn p2_01_fixture_stream_and_split_records_succeed_with_raw_evidence() {
    let c = C::new();
    let (report, launch) = c.run("ok");
    assert!(absent(&report), "{:?}", report.outcome);
    assert_eq!(report.terminal, Some(TerminalStatus::Success));
    assert_eq!(report.exit.and_then(|e| e.code), Some(0));
    assert!(!report.protocol_failed && !report.evidence_lost);
    assert_eq!(
        c.kinds(),
        [
            "SessionIdentified",
            "TurnStarted",
            "ToolStarted",
            "ToolFinished",
            "Informational",
            "TextDelta",
            "UsageObserved",
            "ProviderTerminal"
        ]
    );
    // Every event points into retained raw bytes.
    let raw = fs::read(launch.spool.join("stdout.raw")).unwrap();
    assert!(raw.starts_with(b"{\"type\":\"thread.started\""));
    assert!(
        c.events()
            .iter()
            .all(|e| e.raw_start < e.raw_end && e.raw_end <= raw.len() as u64)
    );

    drop(c);
    let c = C::new();
    let (report, _) = c.run("split");
    assert_eq!(report.terminal, Some(TerminalStatus::Success));
    assert!(!report.protocol_failed);
    let event = c.events().into_iter().find(|e| e.kind == "TextDelta");
    assert_eq!(event.unwrap().payload, text("a🙂b"));
}

#[test]
fn p2_01_malformed_oversized_and_unknown_output_fail_closed() {
    for (scenario, why) in [
        ("malformed", "record is not JSON"),
        ("oversized", "record exceeds frame limit"),
        ("unknown-kind", "unknown record type"),
        ("auth-error-text", "record is not JSON"),
    ] {
        let c = C::new();
        let (report, _) = c.run(scenario);
        assert!(report.protocol_failed, "{scenario}");
        assert_ne!(report.terminal, Some(TerminalStatus::Success), "{scenario}");
        assert!(
            !matches!(report.outcome, LaunchOutcome::Refused(_)),
            "{scenario}"
        );
        let events = c.events();
        let violation = events.iter().find(|e| e.kind == "ProtocolViolation");
        assert!(
            violation.is_some_and(|e| e.payload.contains(why)),
            "{scenario}: {events:?}"
        );
        assert!(
            !events
                .iter()
                .any(|e| e.kind == "ProviderTerminal" && e.payload.contains("Success")),
            "{scenario}"
        );
        assert_eq!(
            c.observed("PROTOCOL_FAILURE").len(),
            1,
            "{scenario}: one durable protocol failure"
        );
    }
}

#[test]
fn p2_01_failed_turn_auth_error_and_missing_terminal_are_never_success() {
    let c = C::new();
    let (report, _) = c.run("turn-failed");
    assert_eq!(report.terminal, Some(TerminalStatus::Error));
    assert_eq!(report.exit.and_then(|e| e.code), Some(1));
    assert!(absent(&report) && !report.protocol_failed);

    drop(c);
    let c = C::new();
    let (report, launch) = c.run("auth-error");
    assert_eq!(report.terminal, Some(TerminalStatus::Error));
    assert_eq!(report.exit.and_then(|e| e.code), Some(1));
    let errors: Vec<_> = c
        .events()
        .into_iter()
        .filter(|e| e.kind == "ProviderError")
        .collect();
    assert_eq!(errors.len(), 2);
    assert!(errors[0].payload.contains("401 Unauthorized"));
    // Diagnostic text stays evidence on stderr; it is never parsed as protocol.
    let stderr = fs::read_to_string(launch.spool.join("stderr.raw")).unwrap();
    assert!(stderr.contains("not signed in"));

    drop(c);
    let c = C::new();
    let (report, _) = c.run("error-then-completed");
    assert_eq!(report.terminal, Some(TerminalStatus::Unknown));

    // Exit status zero is not a result.
    drop(c);
    let c = C::new();
    let (report, _) = c.run("exit-zero-no-terminal");
    assert!(absent(&report), "{:?}", report.outcome);
    assert_eq!(report.exit.and_then(|e| e.code), Some(0));
    assert_eq!(report.terminal, None);
    assert!(!report.protocol_failed);
    assert!(!c.kinds().iter().any(|k| k == "ProviderTerminal"));
}

#[test]
fn p2_01_stderr_flood_is_capped_drained_and_recorded() {
    let c = C::new();
    let spec = c.spec("stderr-flood");
    assert_eq!(spec.max_output_bytes, 1024 * 1024);
    let launch = c.adapter().prepare_launch(&spec).unwrap();
    let started = Instant::now();
    let report = c.launch(&spec, &launch);
    // 4 MiB against a 1 MiB cap: the surplus is drained, so the child is never wedged.
    assert!(absent(&report), "{:?}", report.outcome);
    assert!(started.elapsed() < Duration::from_secs(20));
    assert_eq!(report.exit.and_then(|e| e.code), Some(0));
    let stderr = fs::metadata(launch.spool.join("stderr.raw")).unwrap();
    assert_eq!(stderr.len(), 1024 * 1024);
    assert_eq!(c.observed("STREAM_TRUNCATED"), ["STDERR@1048576"]);
    assert!(
        c.observed("STREAM_CLOSED")
            .iter()
            .any(|d| d == r#"{"discarded":3145728,"durable":1048576,"stream":"STDERR","v":1}"#)
    );
    // Stdout stayed complete and separately accounted.
    assert!(!report.protocol_failed && !report.evidence_lost);
    assert_eq!(report.terminal, Some(TerminalStatus::Success));
}

// D3: executable discovery over a given PATH, with private fixture copies as the CLIs.
// No login shell runs; the fixture answers `--version` by its file name.
fn settled(d: &Discovery, provider: ProviderKind, environment: &[(String, String)]) -> Lookup {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        match d.find(provider, None, environment) {
            Lookup::Checking => {}
            other => return other,
        }
        assert!(Instant::now() < deadline, "discovery never settled");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn installed(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::copy(CODEX_FIXTURE, path).unwrap();
    chmod(path, 0o755);
}

#[test]
fn d3_discovery_takes_the_first_path_entry_resolves_its_link_and_learns_version_and_digest() {
    let c = C::new();
    let claude = [
        ("HOME".to_owned(), c.home.display().to_string()),
        ("USER".to_owned(), "tester".to_owned()),
    ];
    let (first, second) = (c.base.join("first"), c.base.join("second"));
    fs::create_dir(&first).unwrap();
    fs::create_dir(&second).unwrap();
    // A versioned install directory and a link to it, as a cask installs it.
    let real = c.base.join("Caskroom/claude-code/2.1.311/claude");
    installed(&real);
    std::os::unix::fs::symlink(&real, second.join("claude")).unwrap();
    let search = format!(
        "relative/bin::{}:{}:/nonexistent",
        first.display(),
        second.display()
    );
    let d = Discovery::with_search_path(search);
    let digest = sha256_hex(&[&fs::read(&real).unwrap()]);
    assert_eq!(
        d.find(ProviderKind::ClaudeCode, None, &claude),
        Lookup::Checking
    );
    assert_eq!(
        settled(&d, ProviderKind::ClaudeCode, &claude),
        Lookup::Found(FoundExecutable {
            entry: Some(second.join("claude")),
            path: real.clone(),
            version: jojo_domain::claude::CLAUDE_VERSION.into(),
            sha256: digest.clone(),
            profiled: true,
        })
    );
    // An entry earlier on PATH wins, and a release no profile was written against is
    // found and marked, not refused.
    let next = c.base.join("Caskroom/claude-code/2.9.0/claude-next");
    installed(&next);
    std::os::unix::fs::symlink(&next, first.join("claude")).unwrap();
    let Lookup::Found(found) = settled(&d, ProviderKind::ClaudeCode, &claude) else {
        panic!("not found")
    };
    assert_eq!(
        (found.entry, found.path),
        (Some(first.join("claude")), next.clone())
    );
    assert_eq!(
        (found.version.as_str(), found.profiled),
        ("2.9.0 (Claude Code)", false)
    );
    // The same file rewritten in place is checked again and gets its new digest.
    let mut bytes = fs::read(&next).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(&next, &bytes).unwrap();
    assert_eq!(
        d.find(ProviderKind::ClaudeCode, None, &claude),
        Lookup::Checking
    );
    let Lookup::Found(changed) = settled(&d, ProviderKind::ClaudeCode, &claude) else {
        panic!("not found")
    };
    assert_eq!(changed.sha256, sha256_hex(&[&bytes]));
    assert_ne!(changed.sha256, digest);
    // A launcher option replaces the lookup.
    let given = d.find(ProviderKind::ClaudeCode, Some(&real), &claude);
    assert!(
        matches!(given, Lookup::Found(ref f) if f.entry.is_none() && f.path == real),
        "{given:?}"
    );
}

#[test]
fn d3_the_codex_npm_launcher_is_resolved_to_the_binary_its_package_ships() {
    let c = C::new();
    let codex = c.environment();
    let package = c.base.join("lib/node_modules/@openai/codex");
    let launcher = package.join("bin/codex.js");
    fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    fs::write(&launcher, "#!/usr/bin/env node\n").unwrap();
    chmod(&launcher, 0o755);
    let (platform, triple) = if cfg!(target_arch = "aarch64") {
        ("codex-darwin-arm64", "aarch64-apple-darwin")
    } else {
        ("codex-darwin-x64", "x86_64-apple-darwin")
    };
    let binary = package
        .join("node_modules/@openai")
        .join(platform)
        .join("vendor")
        .join(triple)
        .join("bin/codex");
    let bin = c.base.join("homebrew/bin");
    fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(
        "../../lib/node_modules/@openai/codex/bin/codex.js",
        bin.join("codex"),
    )
    .unwrap();
    let d = Discovery::with_search_path(bin.display().to_string());
    // Without the platform binary the launcher is a script, and is refused.
    let Lookup::Refused(why) = settled(&d, ProviderKind::Codex, &codex) else {
        panic!("a script launcher was accepted")
    };
    assert!(why.contains("script launcher"), "{why}");
    installed(&binary);
    let Lookup::Found(found) = settled(&d, ProviderKind::Codex, &codex) else {
        panic!("not found")
    };
    assert_eq!((found.entry, found.path), (Some(bin.join("codex")), binary));
    assert_eq!(found.version, CODEX_VERSION);
    assert!(found.profiled);
    // The same layout means nothing for Claude Code.
    std::os::unix::fs::symlink(&launcher, bin.join("claude")).unwrap();
    assert!(matches!(
        settled(&d, ProviderKind::ClaudeCode, &codex),
        Lookup::Refused(_)
    ));
}

#[test]
fn d3_a_missing_cli_or_one_without_a_usable_version_is_refused() {
    let c = C::new();
    let codex = c.environment();
    let bin = c.base.join("path");
    fs::create_dir(&bin).unwrap();
    let d = Discovery::with_search_path(bin.display().to_string());
    let Lookup::Refused(why) = settled(&d, ProviderKind::Codex, &codex) else {
        panic!("a missing executable was found")
    };
    assert!(why.contains("not on the login shell's PATH"), "{why}");
    let mute = c.base.join("install/codex-mute");
    installed(&mute);
    std::os::unix::fs::symlink(&mute, bin.join("codex")).unwrap();
    let fresh = Discovery::with_search_path(bin.display().to_string());
    let Lookup::Refused(why) = settled(&fresh, ProviderKind::Codex, &codex) else {
        panic!("a CLI without a version was accepted")
    };
    assert!(why.contains("no usable version"), "{why}");
}

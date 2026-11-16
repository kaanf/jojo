//! T09 process manager and raw-stream durability. Real OS processes, but only the
//! workspace's own `jojo-process-fixture`; no provider CLI, credential or network.
//! Readiness/exit use event and file barriers; elapsed time only bounds failures.
mod common;
use common::{TempDir, request, start};
use jojo_domain::attempt::AttemptBindings;
use jojo_domain::dependency::*;
use jojo_domain::identity::*;
use jojo_domain::provider::*;
use jojo_domain::scheduler::*;
use jojo_domain::stream::{CLAUDE_STREAM_JSON, CODEX_EXEC_JSON};
use jojo_domain::time::{Clock, ClockReading};
use jojo_storage::*;
use jojo_test_support::{
    COMPLETE_PRD_JSON, SequenceIds,
    dependency::{node, plan},
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use std::{fs, os::unix::fs::PermissionsExt};

const FIXTURE: &str = env!("CARGO_BIN_EXE_jojo-process-fixture");

fn inspector() -> PathBuf {
    std::env::var_os("JOJO_TEST_PS")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/bin/ps".into())
}

fn release_linger(dir: &Path) {
    fs::write(dir.join("release-linger"), b"").unwrap();
    let watchdog = Instant::now();
    while !dir.join("linger-released").exists() {
        assert!(
            watchdog.elapsed() < Duration::from_secs(5),
            "escaped fixture did not acknowledge release"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

struct RealClock(Instant);
impl Clock for RealClock {
    fn now(&self) -> ClockReading {
        ClockReading {
            unix_millis: 1_700_000_000_000,
            monotonic_nanos: self.0.elapsed().as_nanos() as u64,
            epoch: ClockEpochId(OpaqueId(9)),
        }
    }
}

// Real process tests run one at a time so CPU contention cannot eat stop budgets.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn git(path: &Path, args: &[&str]) -> String {
    let output = Command::new("/usr/bin/git")
        .env_clear()
        .envs([
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z"),
            ("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z"),
        ])
        .args([
            "-c",
            "user.name=T09",
            "-c",
            "user.email=t09@example.invalid",
            "-c",
        ])
        .args(["core.hooksPath=/dev/null", "-C"])
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}");
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_owned()
}

/// One engine, one repository and `n` reserved attempts (101..) with bound worktrees.
struct F {
    _serial: std::sync::MutexGuard<'static, ()>,
    dir: TempDir,
    e: Engine,
    next: AtomicU64,
    paths: Vec<(PathBuf, PathBuf)>,
}

impl F {
    fn new(n: usize) -> Self {
        Self::configured(n, false)
    }
    fn mixed(n: usize) -> Self {
        Self::configured(n, true)
    }
    fn configured(n: usize, mixed: bool) -> Self {
        let serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let dir = TempDir::new("t09");
        let rp = dir.path().join("repo");
        fs::create_dir(&rp).unwrap();
        git(&rp, &["init", "--template=", "--initial-branch=main"]);
        fs::write(rp.join("file"), "base\n").unwrap();
        git(&rp, &["add", "--all"]);
        git(&rp, &["commit", "-m", "fixture"]);
        let base = git(&rp, &["rev-parse", "HEAD"]);
        let repo = LocalRepository::inspect(&rp).unwrap();
        let owned = dir.path().join("owned");
        fs::create_dir(&owned).unwrap();
        fs::set_permissions(&owned, fs::Permissions::from_mode(0o700)).unwrap();
        let root = WorktreeRoot::inspect(&owned).unwrap();
        let e = start(&dir.db());
        let mut f = Self {
            _serial: serial,
            dir,
            e,
            next: AtomicU64::new(1),
            paths: vec![],
        };
        let ids: Vec<u128> = (0..n as u128).map(|i| 101 + i).collect();
        for a in &ids {
            let (r, rt, b) = (repo.clone(), root.clone(), base.clone());
            let a = *a;
            f.cmd(move |tx| tx.request_worktree(OpaqueId(a), &r, &rt, &b, None))
                .unwrap();
            let proof = materialize_worktree(&f.e, OpaqueId(a), None).unwrap();
            f.cmd(move |tx| {
                tx.seal_worktree(proof)?;
                Ok("sealed".into())
            })
            .unwrap();
        }
        let config = f.queue(&repo, &base, n as u32, mixed);
        let proofs = ids
            .iter()
            .map(|a| inspect_worktree(&f.e, OpaqueId(*a)).unwrap())
            .collect();
        let reserved = f
            .cmd(move |tx| {
                Ok(format!(
                    "{:?}",
                    tx.schedule_owned_worktrees(config, proofs)?
                ))
            })
            .unwrap();
        assert_eq!(reserved.response.matches("reserved").count(), n);
        for a in ids {
            let r = f.record(a);
            assert_eq!(r.state, "BOUND");
            f.paths.push((r.path, r.output));
        }
        f
    }

    fn record(&self, a: u128) -> WorktreeRecord {
        self.e
            .reader()
            .unwrap()
            .read(|v| v.worktree(OpaqueId(a)))
            .unwrap()
            .unwrap()
    }

    fn cmd<FN>(&self, f: FN) -> Result<Receipt, CommandError>
    where
        FN: FnOnce(&mut Tx<'_>) -> Result<String, TxError> + Send + 'static,
    {
        let id = self.next.fetch_add(1, Ordering::SeqCst) as u128;
        self.e.execute(request(&self.e, id, "T09", "fixture"), f)
    }

    fn approve(&self, a: ArtifactId) {
        let b = self
            .e
            .reader()
            .unwrap()
            .read(|v| v.review_bundle(a))
            .unwrap()
            .unwrap();
        let id = self.next.fetch_add(1, Ordering::SeqCst) as u128;
        self.e
            .human_decide(
                CommandId(OpaqueId(id)),
                HumanOrigin::FakeHumanFixture {
                    actor: "T09 human".into(),
                },
                b,
                DecisionAction::Approve,
            )
            .unwrap();
    }

    fn queue(&self, repo: &LocalRepository, base: &str, n: u32, mixed: bool) -> u64 {
        let (prd, adr, pid) = (
            ArtifactId(OpaqueId(901)),
            ArtifactId(OpaqueId(902)),
            ArtifactId(OpaqueId(903)),
        );
        let p = PreparedPrd::import(COMPLETE_PRD_JSON.as_bytes()).unwrap();
        self.cmd(move |tx| {
            tx.create_artifact(
                prd,
                FeatureId(OpaqueId(1)),
                &p,
                &Provenance::new(Author::Human, "T09"),
            )?;
            Ok("prd".into())
        })
        .unwrap();
        self.approve(prd);
        self.cmd(move |tx| {
            tx.put_review_document(adr, None, prd, None, "T09 ADR", "fixture reviewed")?;
            Ok("adr".into())
        })
        .unwrap();
        self.approve(adr);
        let mut p = plan(&(1..=n as u128).collect::<Vec<_>>(), vec![]);
        p.schema = 2;
        for i in 0..n as usize {
            p.inputs[i].base_sources[0] = Source {
                repository: repo.id(),
                revision: SourceRevision::Commit(base.to_owned()),
            };
            p.resources.push(StepResources {
                step: node(i as u128 + 1),
                repository: repo.id(),
                provider: if !mixed || i % 2 == 0 {
                    ProviderKind::ClaudeCode
                } else {
                    ProviderKind::Codex
                },
                priority: 0,
                additional: vec![],
                repository_mode: ResourceMode::Shared,
            });
        }
        for a in [prd, adr] {
            let approval = self
                .e
                .reader()
                .unwrap()
                .read(|v| v.current_artifact_approval(a))
                .unwrap()
                .unwrap();
            p.artifacts.nodes.push(NodeRef {
                id: a.0,
                revision: approval.revision,
            });
            p.approvals.push(approval);
        }
        let copy = p.clone();
        self.cmd(move |tx| {
            tx.put_execution_plan(pid, None, prd, adr, &copy, "T09 reviewed")?;
            Ok("plan".into())
        })
        .unwrap();
        self.approve(pid);
        for (i, input) in p.inputs.into_iter().enumerate() {
            self.cmd(move |tx| {
                tx.queue_step(
                    prd,
                    adr,
                    pid,
                    Revision(1),
                    node(i as u128 + 1),
                    &SourceVector {
                        sources: input.base_sources,
                        environment: Some("test-env".into()),
                    },
                )?;
                Ok("queued".into())
            })
            .unwrap();
        }
        self.cmd(move |tx| {
            tx.configure_resources(
                1,
                n,
                std::collections::BTreeMap::from([
                    ("global".into(), n),
                    ("provider:claude".into(), n),
                    ("provider:codex".into(), n),
                ]),
            )?;
            tx.set_admission(Admission::Open)?;
            Ok("config".into())
        })
        .unwrap();
        2
    }

    fn barriers(&self) -> PathBuf {
        let b = self.dir.path().join("barriers");
        let _ = fs::create_dir(&b);
        b
    }

    fn spec(&self, i: usize, parser: &str, scenario: &str) -> LaunchSpec {
        let (cwd, out) = self.paths[i].clone();
        let n = 101 + i as u128;
        let record = self.record(n);
        let vector = self
            .e
            .reader()
            .unwrap()
            .read(|v| v.attempt_source_vector(OpaqueId(n)))
            .unwrap()
            .unwrap();
        let approvals = [901, 902, 903].map(|subject| {
            OpaqueId(u128::from(
                self.e
                    .reader()
                    .unwrap()
                    .read(|v| v.applicable_decision(ArtifactId(OpaqueId(subject))))
                    .unwrap()
                    .unwrap()
                    .0,
            ))
        });
        let provider = if parser == CODEX_EXEC_JSON {
            ProviderKind::Codex
        } else {
            ProviderKind::ClaudeCode
        };
        LaunchSpec {
            attempt: AttemptId(OpaqueId(n)),
            bindings: AttemptBindings {
                feature_id: FeatureId(OpaqueId(1)),
                plan_id: PlanId(OpaqueId(903)),
                plan_revision: Revision(1),
                step_id: StepId(OpaqueId(i as u128 + 1)),
                step_revision: Revision(1),
                repository_id: record.repository,
                worktree_id: WorktreeId(OpaqueId(n)),
            },
            engine_epoch: OpaqueId(u128::from(self.e.epoch().0)),
            dispatch_generation: 1,
            fingerprint: ProviderFingerprint {
                provider,
                executable: FIXTURE.into(),
                version: "fixture-v1".into(),
                schema: PROVIDER_SCHEMA,
                parser: parser.into(),
                configuration: "config-digest".into(),
            },
            source: SourceBinding {
                common_dir: record.common_dir().to_owned(),
                base_commit: record.base.clone(),
                candidate_source_vector: sha256_hex(&[vector.canonical().as_bytes()]),
                instruction_digest: "instructions".into(),
            },
            prompt_digest: sha256_hex(&[scenario.as_bytes()]),
            prompt: scenario.as_bytes().to_vec(),
            continuation: None,
            role: Role::Developer,
            permissions: PermissionEnvelope {
                approved_digest: "permission".into(),
                tools: vec![],
                filesystem_roots: vec![cwd.clone()],
                network: false,
                hooks: false,
                background: false,
            },
            cwd,
            environment_allowlist: vec!["LANG".into()],
            config_references: vec![],
            timeout_millis: 60_000,
            max_output_bytes: 1024 * 1024,
            max_frame_bytes: 64 * 1024,
            raw_output_dir: out,
            approval_ids: approvals,
            required_outputs: vec!["candidate".into()],
            validation_plan: "validation".into(),
        }
    }

    fn launch(&self, spec: &LaunchSpec) -> ProcessLaunch {
        let args = vec![self.barriers().display().to_string()];
        ProcessLaunch::from_spec(spec, args, vec![("LANG".into(), "C".into())]).unwrap()
    }

    fn run(&self, spec: &LaunchSpec, budgets: StopBudgets, mut hooks: LaunchHooks) -> LaunchReport {
        hooks.inspector.get_or_insert_with(inspector);
        let ids = SequenceIds::new(100 + self.next.fetch_add(1, Ordering::SeqCst), 1);
        let clock = RealClock(Instant::now());
        let ctx = LaunchContext {
            engine: &self.e,
            ids: &ids,
            clock: &clock,
            poll: Duration::from_millis(10),
        };
        let plan = StopPlan::new(&caps(Support::Supported), budgets);
        run_launch(&ctx, spec, &self.launch(spec), &plan, hooks).unwrap()
    }

    fn observations(&self, a: u128) -> Vec<(String, String)> {
        self.e
            .reader()
            .unwrap()
            .read(|v| v.process_observations(OpaqueId(a)))
            .unwrap()
    }

    fn kinds(&self, a: u128) -> Vec<String> {
        self.observations(a).into_iter().map(|(k, _)| k).collect()
    }

    fn events(&self, a: u128) -> Vec<StoredProviderEvent> {
        self.e
            .reader()
            .unwrap()
            .read(|v| v.provider_events(OpaqueId(a)))
            .unwrap()
    }

    fn effect(&self, a: u128) -> EffectState {
        let key = format!("scheduler-launch:{a:032x}");
        self.e
            .reader()
            .unwrap()
            .read(|v| v.effects())
            .unwrap()
            .into_iter()
            .find(|e| e.operation_key == key)
            .unwrap()
            .state
    }

    fn cancel(&self, a: u128) {
        self.cmd(move |tx| {
            tx.cancel_approved_attempt(OpaqueId(a))?;
            Ok("cancel".into())
        })
        .unwrap();
    }
}

fn caps(interrupt: Support) -> Capabilities {
    Capabilities {
        evidence: "fixture".into(),
        structured_events: Support::Supported,
        explicit_sessions: Support::Supported,
        exact_continuation: Support::Unsupported,
        incremental_input: Support::Unsupported,
        question_ids: Support::Unsupported,
        permission_response: Support::Unsupported,
        turn_interrupt: interrupt,
        process_tree: Support::Unknown,
        continuation_cwd_change: Support::Unsupported,
        auth_status: Support::Unknown,
        workspace_policy: Support::Unknown,
    }
}

const BUDGETS: StopBudgets = StopBudgets {
    interrupt: Duration::from_secs(3),
    terminate: Duration::from_secs(3),
    kill: Duration::from_secs(5),
    drain: Duration::from_secs(3),
};

fn on_session(tx: mpsc::Sender<()>) -> Option<EventHook> {
    Some(Box::new(move |p| {
        if matches!(p, EventPayload::SessionIdentified(_)) {
            let _ = tx.send(());
        }
    }))
}

/// Every stored event references bytes that are synced in the spool and inside the
/// recorded checkpoint; returns the stdout spool.
fn assert_raw_coverage(f: &F, a: u128, i: usize) -> Vec<u8> {
    let spool = f.paths[i].1.join("generation-1");
    let stdout = fs::read(spool.join("stdout.raw")).unwrap();
    let stderr = fs::read(spool.join("stderr.raw")).unwrap();
    let (out_cp, err_cp) =
        f.e.reader()
            .unwrap()
            .read(|v| {
                Ok((
                    v.stream_checkpoint(OpaqueId(a), Stream::Stdout)?,
                    v.stream_checkpoint(OpaqueId(a), Stream::Stderr)?,
                ))
            })
            .unwrap();
    assert_eq!(
        out_cp,
        stdout.len() as u64,
        "stdout checkpoint equals synced spool"
    );
    assert_eq!(err_cp, stderr.len() as u64);
    for (n, e) in f.events(a).iter().enumerate() {
        assert_eq!(e.sequence, n as u64 + 1);
        if e.stream == "STDOUT" {
            assert!(e.raw_end <= out_cp);
        } else {
            // Stderr is never parsed; its only event is an explicit truncation marker.
            assert_eq!((e.kind.as_str(), e.raw_end), ("StreamTruncated", err_cp));
        }
    }
    stdout
}

#[test]
fn t09_claude_stream_is_issued_spawned_registered_and_captured_before_normalizing() {
    let f = F::new(1);
    let spec = f.spec(0, CLAUDE_STREAM_JSON, "claude-ok");
    let r = f.run(&spec, BUDGETS, LaunchHooks::default());
    assert_eq!(
        r.outcome,
        LaunchOutcome::TreeAbsent {
            stop_requested: false,
            forced: false
        }
    );
    assert_eq!(r.terminal, Some(TerminalStatus::Success));
    assert_eq!(
        r.exit,
        Some(ExitObservation {
            code: Some(0),
            signal: None
        })
    );
    assert!(!r.protocol_failed && r.signals.is_empty());
    assert_eq!(f.effect(101), EffectState::Confirmed);
    let stdout = assert_raw_coverage(&f, 101, 0);
    let events = f.events(101);
    let kinds: Vec<_> = events.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "SessionIdentified",
            "TurnStarted",
            "TextDelta",
            "ToolStarted",
            "ToolFinished",
            "Informational",
            "UsageObserved",
            "ProviderTerminal"
        ]
    );
    // The text event points at exactly the raw record that carried it.
    let text = &events[2];
    let raw = &stdout[text.raw_start as usize..text.raw_end as usize];
    assert!(std::str::from_utf8(raw).unwrap().contains("héllo 🙂"));
    assert!(text.payload.contains("héllo 🙂"));
    let obs = f.kinds(101);
    assert_eq!(obs.last().unwrap(), "TREE_ABSENT");
    assert!(obs.contains(&"ROOT_EXITED".into()) && !obs.contains(&"STOP_REQUESTED".into()));
    let records =
        f.e.reader()
            .unwrap()
            .read(|v| v.process_records(OpaqueId(101)))
            .unwrap();
    assert_eq!(records[0].role, "ROOT");
    assert_eq!(records[0].pid, r.root.as_ref().unwrap().pid);
    assert_eq!(records[0].pgid, records[0].pid);
    assert_ne!(records[0].start, "UNKNOWN");
    // Process evidence is not a result: nothing is sealed and no claim is released.
    let reader = &mut f.e.reader().unwrap();
    assert_eq!(
        reader.read(|v| v.approval_result(OpaqueId(101))).unwrap(),
        None
    );
    assert!(
        f.e.reader()
            .unwrap()
            .read(|v| v.live_claims())
            .unwrap()
            .iter()
            .any(|c| c.holder.id == OpaqueId(101))
    );
}

#[test]
fn t09_codex_failure_and_exit_zero_without_terminal_are_never_success() {
    let f = F::mixed(2);
    let r = f.run(
        &f.spec(1, CODEX_EXEC_JSON, "codex-failed"),
        BUDGETS,
        LaunchHooks::default(),
    );
    assert_eq!(r.terminal, Some(TerminalStatus::Error));
    assert_eq!(r.exit.unwrap().code, Some(1));
    assert!(
        f.events(102)
            .iter()
            .any(|e| e.payload == "ProviderError(\"synthetic failure\")")
    );
    let r = f.run(
        &f.spec(0, CLAUDE_STREAM_JSON, "exit-zero-no-terminal"),
        BUDGETS,
        LaunchHooks::default(),
    );
    assert_eq!(r.exit.unwrap().code, Some(0));
    assert_eq!(r.terminal, None, "exit zero alone is not a terminal record");
    assert_eq!(
        r.outcome,
        LaunchOutcome::TreeAbsent {
            stop_requested: false,
            forced: false
        }
    );
}

#[test]
fn t09_ready_and_exit_barriers_without_sleeps() {
    let f = F::new(1);
    let release = f.barriers().join("release");
    let hooks = LaunchHooks {
        on_event: Some(Box::new(move |p| {
            if matches!(p, EventPayload::SessionIdentified(_)) {
                fs::write(&release, b"").unwrap();
            }
        })),
        ..LaunchHooks::default()
    };
    let r = f.run(
        &f.spec(0, CLAUDE_STREAM_JSON, "wait-release"),
        BUDGETS,
        hooks,
    );
    assert_eq!(r.exit.unwrap().code, Some(7));
    assert_eq!(r.terminal, Some(TerminalStatus::Success));
    assert_eq!(
        r.outcome,
        LaunchOutcome::TreeAbsent {
            stop_requested: false,
            forced: false
        }
    );
}

#[test]
fn t09_fragmented_utf8_ndjson_from_a_real_pipe_is_reassembled() {
    let f = F::new(1);
    let r = f.run(
        &f.spec(0, CLAUDE_STREAM_JSON, "fragmented"),
        BUDGETS,
        LaunchHooks::default(),
    );
    assert!(!r.protocol_failed);
    assert_eq!(r.terminal, Some(TerminalStatus::Success));
    let stdout = assert_raw_coverage(&f, 101, 0);
    assert!(std::str::from_utf8(&stdout).unwrap().contains("a🙂b"));
    assert!(
        f.events(101)
            .iter()
            .any(|e| e.payload == "TextDelta(\"a🙂b\")")
    );
}

#[test]
fn t09_malformed_output_requests_stop_and_blocks_later_success() {
    let f = F::new(1);
    let r = f.run(
        &f.spec(0, CLAUDE_STREAM_JSON, "malformed"),
        BUDGETS,
        LaunchHooks::default(),
    );
    assert!(r.protocol_failed);
    assert_eq!(r.terminal, None);
    assert_eq!(
        r.outcome,
        LaunchOutcome::TreeAbsent {
            stop_requested: true,
            forced: false
        }
    );
    let obs = f.observations(101);
    let stop = obs
        .iter()
        .position(|(k, d)| k == "STOP_REQUESTED" && d.starts_with("protocol:"))
        .unwrap();
    let int = obs
        .iter()
        .position(|(k, d)| k == "SIGNAL_SENT" && d.starts_with("INT group:"))
        .unwrap();
    let exit = obs.iter().position(|(k, _)| k == "ROOT_EXITED").unwrap();
    assert!(
        stop < int && int < exit,
        "stop request precedes signal precedes observed death"
    );
    assert!(f.kinds(101).contains(&"PROTOCOL_FAILURE".into()));
    assert!(f.events(101).iter().any(|e| {
        e.payload
            .contains("terminal success after protocol failure")
    }));
    assert_eq!(r.exit.unwrap().signal, Some(2));
}

#[test]
fn t09_oversized_unterminated_and_duplicate_records_fail_closed() {
    let f = F::new(3);
    let r = f.run(
        &f.spec(0, CLAUDE_STREAM_JSON, "oversized"),
        BUDGETS,
        LaunchHooks::default(),
    );
    assert!(r.protocol_failed && r.terminal.is_none());
    assert!(f.events(101).iter().any(|e| e.kind == "StreamTruncated"));
    assert_eq!(
        r.outcome,
        LaunchOutcome::TreeAbsent {
            stop_requested: true,
            forced: false
        }
    );
    let r = f.run(
        &f.spec(1, CLAUDE_STREAM_JSON, "unterminated"),
        BUDGETS,
        LaunchHooks::default(),
    );
    assert!(r.protocol_failed && r.terminal.is_none());
    assert!(
        f.events(102)
            .last()
            .unwrap()
            .payload
            .contains("unterminated")
    );
    let r = f.run(
        &f.spec(2, CLAUDE_STREAM_JSON, "duplicate-terminal"),
        BUDGETS,
        LaunchHooks::default(),
    );
    assert!(r.protocol_failed);
    assert!(
        f.events(103)
            .iter()
            .any(|e| e.payload.contains("duplicate terminal"))
    );
}

#[test]
fn t09_stderr_flood_is_capped_drained_and_cannot_block_stop() {
    let f = F::new(1);
    let (tx, ready) = mpsc::channel();
    let spec = f.spec(0, CLAUDE_STREAM_JSON, "stderr-flood");
    std::thread::scope(|s| {
        let runner = s.spawn(|| {
            f.run(
                &spec,
                BUDGETS,
                LaunchHooks {
                    on_event: on_session(tx),
                    ..LaunchHooks::default()
                },
            )
        });
        ready.recv().unwrap();
        let requested = Instant::now();
        f.cancel(101);
        let r = runner.join().unwrap();
        let latency = requested.elapsed();
        assert!(
            latency < Duration::from_secs(3),
            "stop latency {latency:?} under flood"
        );
        assert_eq!(
            r.outcome,
            LaunchOutcome::TreeAbsent {
                stop_requested: true,
                forced: false
            }
        );
        assert!(
            !r.protocol_failed,
            "stderr is captured, never parsed as protocol"
        );
    });
    let spool = f.paths[0].1.join("generation-1");
    assert_eq!(
        fs::metadata(spool.join("stderr.raw")).unwrap().len(),
        1024 * 1024
    );
    let obs = f.observations(101);
    assert!(
        obs.iter()
            .any(|(k, d)| k == "STREAM_TRUNCATED" && d == "STDERR@1048576")
    );
    let closed = obs
        .iter()
        .find(|(k, d)| k == "STREAM_CLOSED" && d.starts_with("STDERR"))
        .unwrap();
    assert!(
        !closed.1.ends_with("discarded=0"),
        "flood beyond the cap was drained: {}",
        closed.1
    );
    assert_raw_coverage(&f, 101, 0);
}

#[test]
fn t09_cancel_at_reservation_issue_and_registration_barriers() {
    // Reservation: stop before issue retires the reservation; nothing is journaled.
    let f = F::new(1);
    f.cancel(101);
    let r = f.run(
        &f.spec(0, CLAUDE_STREAM_JSON, "claude-ok"),
        BUDGETS,
        LaunchHooks::default(),
    );
    assert!(matches!(r.outcome, LaunchOutcome::Refused(_)));
    assert!(
        !f.e.reader()
            .unwrap()
            .read(|v| v.process_launch_issued(OpaqueId(101)))
            .unwrap()
    );
    assert_eq!(f.effect(101), EffectState::NotIssued);
    drop(f);
    // Issued: stop after ISSUED_UNKNOWN but before spawn is proven not spawned.
    for barrier in ["issued", "registration"] {
        let f = F::new(1);
        let (hit, wait) = mpsc::channel::<()>();
        let (ack, acked) = mpsc::channel::<()>();
        let mut hooks = LaunchHooks::default();
        let gate = move || {
            hit.send(()).unwrap();
            acked.recv().unwrap();
        };
        if barrier == "issued" {
            hooks.after_issue = Some(Box::new(gate));
        } else {
            hooks.after_spawn = Some(Box::new(move |_| gate()));
        }
        let spec = f.spec(0, CLAUDE_STREAM_JSON, "wait-release");
        std::thread::scope(|s| {
            let runner = s.spawn(|| f.run(&spec, BUDGETS, hooks));
            wait.recv().unwrap();
            assert_eq!(f.effect(101), EffectState::IssuedUnknown);
            f.cancel(101);
            ack.send(()).unwrap();
            let r = runner.join().unwrap();
            if barrier == "issued" {
                assert!(matches!(r.outcome, LaunchOutcome::NotSpawned(_)));
                assert!(r.root.is_none());
                assert_eq!(f.effect(101), EffectState::Rejected);
                assert_eq!(f.kinds(101), ["NOT_SPAWNED"]);
            } else {
                // The registered child is routed straight into stop; never left running.
                assert_eq!(
                    r.outcome,
                    LaunchOutcome::TreeAbsent {
                        stop_requested: true,
                        forced: false
                    }
                );
                assert_eq!(f.effect(101), EffectState::Confirmed);
                let k = f.kinds(101);
                let stop = k.iter().position(|k| k == "STOP_REQUESTED").unwrap();
                assert!(stop < k.iter().position(|k| k == "ROOT_EXITED").unwrap());
            }
        });
    }
}

#[test]
fn t09_no_transaction_is_held_across_spawn() {
    let f = F::new(1);
    let (hit, wait) = mpsc::channel::<u32>();
    let (ack, acked) = mpsc::channel::<()>();
    let release = f.barriers().join("release");
    let hooks = LaunchHooks {
        after_spawn: Some(Box::new(move |root| {
            hit.send(root.pid).unwrap();
            acked.recv().unwrap();
        })),
        ..LaunchHooks::default()
    };
    let spec = f.spec(0, CLAUDE_STREAM_JSON, "wait-release");
    std::thread::scope(|s| {
        let runner = s.spawn(|| f.run(&spec, BUDGETS, hooks));
        let pid = wait.recv().unwrap();
        // The child exists, the issue journal is committed and the writer is free.
        assert_eq!(f.effect(101), EffectState::IssuedUnknown);
        assert!(
            f.e.reader()
                .unwrap()
                .read(|v| v.process_records(OpaqueId(101)))
                .unwrap()
                .is_empty()
        );
        let key = AggregateKey::new("t09-probe", OpaqueId(5));
        f.cmd(move |tx| {
            tx.create_aggregate(&key, "free")?;
            Ok("writer free during spawn".into())
        })
        .unwrap();
        assert!(pid > 0);
        ack.send(()).unwrap();
        fs::write(&release, b"").unwrap();
        let r = runner.join().unwrap();
        assert_eq!(
            r.outcome,
            LaunchOutcome::TreeAbsent {
                stop_requested: false,
                forced: false
            }
        );
    });
}

#[test]
fn t09_cli_exit_with_live_child_is_not_absent_until_the_tree_is_stopped() {
    let f = F::new(1);
    let r = f.run(
        &f.spec(0, CLAUDE_STREAM_JSON, "child-alive"),
        BUDGETS,
        LaunchHooks::default(),
    );
    assert_eq!(
        r.terminal,
        Some(TerminalStatus::Success),
        "terminal success is evidence only"
    );
    assert_eq!(r.exit.unwrap().code, Some(0));
    assert_eq!(
        r.outcome,
        LaunchOutcome::TreeAbsent {
            stop_requested: true,
            forced: false
        }
    );
    let k = f.kinds(101);
    let alive = k.iter().position(|k| k == "DESCENDANTS_ALIVE").unwrap();
    assert!(k.iter().position(|k| k == "ROOT_EXITED").unwrap() < alive);
    assert!(alive < k.iter().position(|k| k == "TREE_ABSENT").unwrap());
    let records =
        f.e.reader()
            .unwrap()
            .read(|v| v.process_records(OpaqueId(101)))
            .unwrap();
    assert!(
        records
            .iter()
            .any(|p| p.role == "DESCENDANT" && p.pgid == records[0].pgid)
    );
    // The terminated root stays unreaped until its group has stopped, pinning the PGID.
    assert!(
        r.signals
            .iter()
            .any(|s| s.target.starts_with("group:") && s.outcome == SignalOutcome::Sent)
    );
}

#[test]
fn t09_signal_ignoring_root_needs_verified_kill_and_is_reported_forced() {
    let f = F::new(1);
    let budgets = StopBudgets {
        interrupt: Duration::from_millis(300),
        terminate: Duration::from_millis(300),
        ..BUDGETS
    };
    let (tx, ready) = mpsc::channel();
    let spec = f.spec(0, CLAUDE_STREAM_JSON, "ignore-signals");
    std::thread::scope(|s| {
        let runner = s.spawn(|| {
            f.run(
                &spec,
                budgets,
                LaunchHooks {
                    on_event: on_session(tx),
                    ..LaunchHooks::default()
                },
            )
        });
        ready.recv().unwrap();
        f.cancel(101);
        let r = runner.join().unwrap();
        assert_eq!(
            r.outcome,
            LaunchOutcome::TreeAbsent {
                stop_requested: true,
                forced: true
            }
        );
        let sent: Vec<_> = r
            .signals
            .iter()
            .filter(|s| s.outcome == SignalOutcome::Sent)
            .map(|s| s.signal)
            .collect();
        assert_eq!(sent, [Signal::Interrupt, Signal::Terminate, Signal::Kill]);
        assert_eq!(r.exit.unwrap().signal, Some(9));
    });
    let (_, detail) = f.observations(101).pop().unwrap();
    assert_eq!(detail, "graceful stop failed; forced stop confirmed");
}

#[test]
fn t09_escaped_descendant_holding_output_produces_unconfirmed_stop() {
    let f = F::new(1);
    let barriers = f.barriers();
    let go = barriers.join("go");
    let budgets = StopBudgets {
        drain: Duration::from_millis(500),
        ..BUDGETS
    };
    let hooks = LaunchHooks {
        on_event: Some(Box::new(move |p| {
            if matches!(p, EventPayload::SessionIdentified(_)) {
                fs::write(&go, b"").unwrap();
            }
        })),
        ..LaunchHooks::default()
    };
    let r = f.run(
        &f.spec(0, CLAUDE_STREAM_JSON, "escaped-holder"),
        budgets,
        hooks,
    );
    release_linger(&barriers);
    let LaunchOutcome::StopUnconfirmed(reasons) = &r.outcome else {
        panic!(
            "escaped holder must not be reported absent: {:?}",
            r.outcome
        );
    };
    assert!(
        reasons
            .iter()
            .any(|x| x.contains("held open by an unknown process"))
    );
    let k = f.kinds(101);
    assert_eq!(k.last().unwrap(), "STOP_UNCONFIRMED");
    assert!(!k.contains(&"TREE_ABSENT".into()));
    // Concluded: a later absence claim cannot be appended.
    let late = f.cmd(|tx| {
        tx.record_process_observation(OpaqueId(101), ObservationKind::TreeAbsent, "late")?;
        Ok("late".into())
    });
    assert!(late.is_err());
}

#[test]
fn t09_wrong_start_time_or_boot_is_never_signalled() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let dir = TempDir::new("t09-identity");
    let launch = ProcessLaunch {
        executable: FIXTURE.into(),
        args: vec![dir.path().display().to_string()],
        cwd: dir.path().to_owned(),
        environment: vec![],
        stdin: b"wait-release".to_vec(),
        spool: dir.path().join("spool"),
        parser: CLAUDE_STREAM_JSON.into(),
        limits: StreamLimits {
            max_frame_bytes: 4096,
            max_output_bytes: 65536,
            checkpoint_bytes: 4096,
        },
    };
    let sup =
        Supervised::spawn_with_inspector(&launch, SpoolFaults::default(), &inspector()).unwrap();
    let real = sup.root().clone();
    assert_eq!(sup.verify_identity(&real), IdentityCheck::Alive);
    // Even an exact start-time match is only a snapshot, never a signal authority.
    assert!(matches!(
        sup.signal_verified(&real, Signal::Kill).outcome,
        SignalOutcome::RefusedUnverified(_)
    ));
    let unknown = ProcessIdentity {
        start: "UNKNOWN".into(),
        ..real.clone()
    };
    assert!(matches!(
        sup.verify_identity(&unknown),
        IdentityCheck::Unknown(_)
    ));
    assert!(matches!(
        sup.signal_verified(&unknown, Signal::Kill).outcome,
        SignalOutcome::RefusedUnverified(_)
    ));
    let reused = ProcessIdentity {
        start: "Thu Jan 1 00:00:00 1970".into(),
        ..real.clone()
    };
    assert_eq!(
        sup.signal_verified(&reused, Signal::Kill).outcome,
        SignalOutcome::RefusedReused
    );
    let other_boot = ProcessIdentity {
        boot: "another boot".into(),
        ..real.clone()
    };
    assert_eq!(
        sup.signal_verified(&other_boot, Signal::Kill).outcome,
        SignalOutcome::Absent
    );
    let tree = sup.tree();
    assert!(
        tree.root_exit.is_none() && tree.live_group.contains(&real.pid),
        "still alive"
    );
    fs::write(dir.path().join("release"), b"").unwrap();
    loop {
        match sup
            .messages()
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
        {
            ProcessMessage::Exited(e) => {
                assert_eq!(e.code, Some(7));
                break;
            }
            _ => continue,
        }
    }
    assert!(matches!(
        sup.signal_tree(Signal::Kill)[0].outcome,
        SignalOutcome::RefusedUnverified(_)
    ));
    // After reaping, the same identity is absent and still never signalled blindly.
    assert_eq!(
        sup.signal_verified(&real, Signal::Kill).outcome,
        SignalOutcome::Absent
    );
}

#[test]
fn t09_child_inherits_only_stdio_never_database_or_lock_descriptors() {
    let f = F::new(1);
    // The engine holds the SQLite database, WAL and the ownership lock open right now.
    let _reader = f.e.reader().unwrap();
    let r = f.run(
        &f.spec(0, CLAUDE_STREAM_JSON, "fd-report"),
        BUDGETS,
        LaunchHooks::default(),
    );
    assert_eq!(r.terminal, Some(TerminalStatus::Success));
    let text = f
        .events(101)
        .into_iter()
        .find(|e| e.kind == "TextDelta")
        .unwrap();
    assert_eq!(text.payload, "TextDelta(\"fds=[]\")");
}

#[test]
fn t09_injected_spool_write_failure_closes_admission_and_stops() {
    let f = F::new(1);
    let spec = f.spec(0, CLAUDE_STREAM_JSON, "stderr-flood");
    let hooks = LaunchHooks {
        spool: SpoolFaults {
            stdout_fail_after: None,
            stderr_fail_after: Some(128 * 1024),
        },
        ..LaunchHooks::default()
    };
    let r = f.run(&spec, BUDGETS, hooks);
    assert!(r.evidence_lost && r.admission_closed);
    assert_eq!(
        r.outcome,
        LaunchOutcome::TreeAbsent {
            stop_requested: true,
            forced: false
        }
    );
    assert_eq!(
        f.e.reader()
            .unwrap()
            .read(|v| v.session())
            .unwrap()
            .admission,
        Admission::Closed
    );
    let obs = f.observations(101);
    assert!(
        obs.iter()
            .any(|(k, d)| k == "EVIDENCE_LOSS" && d.contains("injected"))
    );
    assert!(
        obs.iter()
            .any(|(k, d)| k == "STOP_REQUESTED" && d.starts_with("evidence loss"))
    );
}

#[test]
fn t09_schema_refuses_undurable_event_ranges_and_unproven_absence() {
    let f = F::new(1);
    let r = f.run(
        &f.spec(0, CLAUDE_STREAM_JSON, "claude-ok"),
        BUDGETS,
        LaunchHooks::default(),
    );
    assert!(matches!(r.outcome, LaunchOutcome::TreeAbsent { .. }));
    let durable =
        f.e.reader()
            .unwrap()
            .read(|v| v.stream_checkpoint(OpaqueId(101), Stream::Stdout))
            .unwrap();
    let next = f.events(101).len() as u64 + 1;
    // An event whose raw bytes extend past the durable checkpoint is rejected by SQL.
    let beyond = f.cmd(move |tx| {
        let e = RawEvent {
            stream: Stream::Stdout,
            start: durable,
            end: durable + 10,
            payload: EventPayload::TextDelta("x".into()),
        };
        tx.append_provider_output(OpaqueId(101), Stream::Stdout, durable, &[(next, e)])?;
        Ok("beyond".into())
    });
    assert!(beyond.is_err());
    // Checkpoints only advance; history cannot be rewritten.
    let back = f.cmd(move |tx| {
        tx.append_provider_output(
            OpaqueId(101),
            Stream::Stdout,
            durable.saturating_sub(1),
            &[],
        )?;
        Ok("back".into())
    });
    assert!(back.is_err());
    // A second attempt at the same reservation cannot be issued (no automatic restart).
    let again = f.spec(0, CLAUDE_STREAM_JSON, "claude-ok");
    assert!(
        f.cmd(move |tx| Ok(tx.issue_process_launch(&again)?.to_string()))
            .is_err()
    );
}

#[test]
fn t09_mixed_provider_attempts_run_concurrently_with_independent_streams() {
    let f = F::mixed(2);
    let (tx, ready) = mpsc::channel();
    let release = f.barriers().join("release");
    let (a, b) = (
        f.spec(0, CLAUDE_STREAM_JSON, "wait-release"),
        f.spec(1, CODEX_EXEC_JSON, "codex-wait-release"),
    );
    std::thread::scope(|s| {
        let (t1, t2) = (tx.clone(), tx);
        let ra = s.spawn(|| {
            f.run(
                &a,
                BUDGETS,
                LaunchHooks {
                    on_event: on_session(t1),
                    ..LaunchHooks::default()
                },
            )
        });
        let rb = s.spawn(|| {
            f.run(
                &b,
                BUDGETS,
                LaunchHooks {
                    on_event: on_session(t2),
                    ..LaunchHooks::default()
                },
            )
        });
        // Both providers are live before either is released.
        ready.recv().unwrap();
        ready.recv().unwrap();
        fs::write(&release, b"").unwrap();
        let (ra, rb) = (ra.join().unwrap(), rb.join().unwrap());
        assert_eq!(ra.terminal, Some(TerminalStatus::Success));
        assert_eq!(rb.terminal, Some(TerminalStatus::Success));
        assert_ne!(ra.root.unwrap().pgid, rb.root.unwrap().pgid);
    });
    assert!(
        f.events(101)
            .iter()
            .any(|e| e.payload.contains("ClaudeCode"))
    );
    assert!(f.events(102).iter().any(|e| e.payload.contains("Codex")));
    assert_raw_coverage(&f, 101, 0);
    assert_raw_coverage(&f, 102, 1);
}

#[test]
fn t09_inspection_failure_returns_unknown_within_stop_budget_and_keeps_claims() {
    let f = F::new(1);
    let started = Instant::now();
    let r = f.run(
        &f.spec(0, CLAUDE_STREAM_JSON, "wait-release"),
        BUDGETS,
        LaunchHooks {
            inspector: Some(f.dir.path().join("missing-inspector")),
            ..LaunchHooks::default()
        },
    );
    assert!(
        matches!(r.outcome, LaunchOutcome::StopUnconfirmed(_)),
        "{r:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(r.root.as_ref().unwrap().start, "UNKNOWN");
    assert!(f.kinds(101).contains(&"STOP_UNCONFIRMED".into()));
    assert!(!f.kinds(101).contains(&"TREE_ABSENT".into()));
    assert!(
        f.e.reader()
            .unwrap()
            .read(|v| v.live_claims())
            .unwrap()
            .iter()
            .any(|c| c.holder.id == OpaqueId(101))
    );
}

#[test]
fn t09_os_request_must_match_frozen_spec_before_issue() {
    let f = F::new(1);
    let spec = f.spec(0, CLAUDE_STREAM_JSON, "claude-ok");
    let mut launch = f.launch(&spec);
    launch.stdin = b"stderr-flood".to_vec();
    let ids = SequenceIds::new(99000, 1);
    let clock = RealClock(Instant::now());
    let ctx = LaunchContext {
        engine: &f.e,
        ids: &ids,
        clock: &clock,
        poll: Duration::from_millis(10),
    };
    let plan = StopPlan::new(&caps(Support::Supported), BUDGETS);
    let r = run_launch(&ctx, &spec, &launch, &plan, LaunchHooks::default()).unwrap();
    assert!(matches!(r.outcome, LaunchOutcome::Refused(_)));
    assert!(r.root.is_none());
    assert!(!launch.spool.exists());
    assert!(f.kinds(101).is_empty());
}

#[test]
fn t09_unreaped_zombie_pins_group_until_live_child_is_stopped() {
    let _serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let dir = TempDir::new("t09-pin");
    let launch = ProcessLaunch {
        executable: FIXTURE.into(),
        args: vec![dir.path().display().to_string()],
        cwd: dir.path().to_owned(),
        environment: vec![],
        stdin: b"child-alive".to_vec(),
        spool: dir.path().join("spool"),
        parser: CLAUDE_STREAM_JSON.into(),
        limits: StreamLimits {
            max_frame_bytes: 4096,
            max_output_bytes: 65536,
            checkpoint_bytes: 4096,
        },
    };
    let mut sup =
        Supervised::spawn_with_inspector(&launch, SpoolFaults::default(), &inspector()).unwrap();
    loop {
        if matches!(
            sup.messages()
                .recv_timeout(Duration::from_secs(10))
                .unwrap(),
            ProcessMessage::RootTerminated
        ) {
            break;
        }
    }
    // The root is dead but not reaped: its numeric PID/PGID still belongs to us.
    assert!(sup.root_exit().is_none());
    let found = sup.discover().unwrap();
    assert!(!found.is_empty());
    let child = found[0].clone();
    assert_eq!(sup.verify_identity(&child), IdentityCheck::Alive);
    assert_eq!(
        sup.signal_tree(Signal::Terminate)[0].outcome,
        SignalOutcome::Sent
    );
    loop {
        if let ProcessMessage::Exited(exit) = sup
            .messages()
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
        {
            assert_eq!(exit.code, Some(0));
            break;
        }
    }
    assert_eq!(sup.verify_identity(&child), IdentityCheck::Absent);
    assert!(sup.tree().absent());
    assert!(matches!(
        sup.signal_tree(Signal::Kill)[0].outcome,
        SignalOutcome::RefusedUnverified(_)
    ));
}

#[test]
fn t09_known_escaped_child_is_signalled_after_an_identity_check_and_confirmed_absent() {
    let f = F::new(1);
    let (tx, ready) = mpsc::channel();
    let spec = f.spec(0, CLAUDE_STREAM_JSON, "escaped-known");
    let budgets = StopBudgets {
        interrupt: Duration::from_millis(100),
        terminate: Duration::from_millis(100),
        kill: Duration::from_millis(100),
        drain: Duration::from_millis(100),
    };
    let r = std::thread::scope(|s| {
        let runner = s.spawn(|| {
            f.run(
                &spec,
                budgets,
                LaunchHooks {
                    on_event: on_session(tx),
                    ..LaunchHooks::default()
                },
            )
        });
        ready.recv_timeout(Duration::from_secs(10)).unwrap();
        f.cancel(101);
        runner.join().unwrap()
    });
    // M0: the child left the group (as real provider tool processes do) and is stopped
    // through its recorded identity, not left running behind an unconfirmed stop.
    assert!(
        matches!(
            r.outcome,
            LaunchOutcome::TreeAbsent {
                stop_requested: true,
                ..
            }
        ),
        "{r:?}"
    );
    let escaped = r.descendants.first().expect("escaped child recorded");
    // It leads its own group, so the signal goes to that group.
    let target = format!("group:{}", escaped.pid);
    assert!(
        r.signals
            .iter()
            .any(|s| s.target == target && s.outcome == SignalOutcome::Sent),
        "{:?}",
        r.signals
    );
    assert_eq!(
        inspect_recorded(&inspector(), std::slice::from_ref(escaped))[0].1,
        IdentityCheck::Absent
    );
    // Stopped by signal, not by the fixture's release barrier.
    assert!(!f.barriers().join("linger-released").exists());
}

#[test]
fn t09_execution_deadline_requests_stop_without_fabricating_success() {
    let f = F::new(1);
    let mut spec = f.spec(0, CLAUDE_STREAM_JSON, "wait-release");
    spec.timeout_millis = 100;
    let r = f.run(&spec, BUDGETS, LaunchHooks::default());
    assert_eq!(
        r.outcome,
        LaunchOutcome::TreeAbsent {
            stop_requested: true,
            forced: false
        }
    );
    assert_ne!(r.terminal, Some(TerminalStatus::Success));
    assert!(
        f.observations(101)
            .iter()
            .any(|(_, detail)| detail.contains("execution deadline exceeded"))
    );
}

#[test]
fn t09_process_memory_limits_fail_before_any_spawn_or_spool() {
    let f = F::new(1);
    let mut spec = f.spec(0, CLAUDE_STREAM_JSON, "claude-ok");
    spec.max_output_bytes = MAX_OUTPUT_BYTES + 1;
    assert!(matches!(
        ProcessLaunch::from_spec(&spec, vec![], vec![]),
        Err(LaunchRejection::Spec(ProviderError::Limit))
    ));
    spec.max_output_bytes = MAX_OUTPUT_BYTES;
    spec.max_frame_bytes = MAX_FRAME_BYTES as u64 + 1;
    assert!(matches!(
        ProcessLaunch::from_spec(&spec, vec![], vec![]),
        Err(LaunchRejection::Spec(ProviderError::Limit))
    ));
    let mut launch = f.launch(&f.spec(0, CLAUDE_STREAM_JSON, "claude-ok"));
    launch.limits.max_frame_bytes = usize::MAX;
    assert!(matches!(
        Supervised::spawn(&launch, SpoolFaults::default()),
        Err(SpawnFailure::BeforeChild(_))
    ));
    assert!(!launch.spool.exists());
}

#[test]
fn t09_stale_or_forged_launch_authority_is_rejected_before_issue() {
    let f = F::new(1);
    let good = f.spec(0, CLAUDE_STREAM_JSON, "claude-ok");
    let ids = SequenceIds::new(99000, 1);
    let clock = RealClock(Instant::now());
    let ctx = LaunchContext {
        engine: &f.e,
        ids: &ids,
        clock: &clock,
        poll: Duration::from_millis(10),
    };
    let plan = StopPlan::new(&caps(Support::Supported), BUDGETS);
    for which in 0..16 {
        let mut bad = good.clone();
        match which {
            0 => bad.engine_epoch = OpaqueId(99),
            1 => bad.dispatch_generation = 2,
            2 => {
                bad.fingerprint.provider = ProviderKind::Codex;
                bad.fingerprint.parser = CODEX_EXEC_JSON.into();
            }
            3 => bad.bindings.feature_id = FeatureId(OpaqueId(99)),
            4 => bad.bindings.plan_id = PlanId(OpaqueId(99)),
            5 => bad.bindings.plan_revision = Revision(2),
            6 => bad.bindings.step_id = StepId(OpaqueId(99)),
            7 => bad.bindings.step_revision = Revision(2),
            8 => bad.bindings.repository_id = RepositoryId(OpaqueId(99)),
            9 => bad.bindings.worktree_id = WorktreeId(OpaqueId(99)),
            10 => bad.source.common_dir = "/not/the/owned/repo.git".into(),
            11 => bad.source.base_commit = "forged".into(),
            12 => bad.source.candidate_source_vector = "forged".into(),
            13 => bad.prompt_digest = "forged".into(),
            14 => bad.approval_ids[0] = OpaqueId(99),
            15 => bad.fingerprint.provider = ProviderKind::Codex,
            _ => unreachable!(),
        }
        match ProcessLaunch::from_spec(&bad, vec![f.barriers().display().to_string()], vec![]) {
            Ok(launch) => {
                let r = run_launch(&ctx, &bad, &launch, &plan, LaunchHooks::default()).unwrap();
                assert!(
                    matches!(r.outcome, LaunchOutcome::Refused(_)),
                    "case {which}: {r:?}"
                );
                assert!(r.root.is_none());
                assert!(!launch.spool.exists());
            }
            Err(_) if which == 15 => {}
            Err(e) => panic!("unexpected structural rejection {which}: {e:?}"),
        }
        assert!(f.kinds(101).is_empty());
    }
    // No rejected variant consumed the one allowed issue boundary.
    let report = f.run(&good, BUDGETS, LaunchHooks::default());
    assert!(matches!(report.outcome, LaunchOutcome::TreeAbsent { .. }));
}

#[test]
fn t09_small_stderr_and_eventless_stdout_keep_final_durable_checkpoints() {
    let f = F::new(1);
    let r = f.run(
        &f.spec(0, CLAUDE_STREAM_JSON, "quiet-raw"),
        BUDGETS,
        LaunchHooks::default(),
    );
    assert_eq!(
        r.outcome,
        LaunchOutcome::TreeAbsent {
            stop_requested: false,
            forced: false
        }
    );
    assert!(r.terminal.is_none() && !r.protocol_failed);
    assert!(f.events(101).is_empty());
    assert_eq!(assert_raw_coverage(&f, 101, 0), b" \n\n");
    assert_eq!(
        fs::read(f.paths[0].1.join("generation-1/stderr.raw")).unwrap(),
        b"diagnostic-only\n"
    );
}

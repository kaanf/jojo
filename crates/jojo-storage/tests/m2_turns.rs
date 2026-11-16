//! M2: lead agent turns at the storage layer. A turn is accepted in one transaction, run
//! read-only through the production adapters and the real supervised launch, and sealed;
//! a reply becomes questions and PRD versions authored by the model, never an approval.
//! The "provider" is a private copy of `jojo-codex-fixture`, which speaks both dialects.
//! No installed CLI is named, located or executed; no credential, network or inference.
mod common;
use common::attempts::{BUDGETS, RealClock, SERIAL, caps, git, human, os, ps, until};
use common::{TempDir, start};
use jojo_domain::claude::{CLAUDE_VARIABLES, CLAUDE_VERSION};
use jojo_domain::codex::{CODEX_HOME_VARIABLE, CODEX_VERSION};
use jojo_domain::identity::*;
use jojo_domain::planner::{TurnKind, TurnSpec};
use jojo_domain::provider::{AuthMode, ProviderKind, Support};
use jojo_storage::*;
use jojo_test_support::SequenceIds;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const FIXTURE: &str = env!("CARGO_BIN_EXE_jojo-codex-fixture");
const FEATURE: FeatureId = FeatureId(OpaqueId(900));
const BOTH: [ProviderKind; 2] = [ProviderKind::ClaudeCode, ProviderKind::Codex];

fn chmod(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

fn private(path: &Path) {
    fs::create_dir(path).unwrap();
    chmod(path, 0o700);
}

/// One engine over a temporary data root, one registered sample repository and a private
/// fixture executable behind both production adapters.
struct T {
    _serial: std::sync::MutexGuard<'static, ()>,
    _dir: TempDir,
    base: PathBuf,
    repository: LocalRepository,
    head: String,
    engine: Option<Engine>,
    commands: AtomicU64,
    namespace: AtomicU64,
}

impl T {
    fn new() -> Self {
        let serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let dir = TempDir::new("m2");
        let base = fs::canonicalize(dir.path()).unwrap();
        for name in ["disposable", "home", "codex-home", "data"] {
            private(&base.join(name));
        }
        let path = base.join("disposable/sample");
        fs::create_dir_all(path.join("src")).unwrap();
        git(&path, &["init", "--template=", "--initial-branch=main"]);
        fs::write(path.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
        git(&path, &["add", "--all"]);
        git(&path, &["commit", "-m", "sample"]);
        fs::create_dir(base.join("bin")).unwrap();
        fs::copy(FIXTURE, base.join("bin/agent")).unwrap();
        chmod(&base.join("bin/agent"), 0o755);
        let repository = LocalRepository::inspect(&path).unwrap();
        let t = Self {
            _serial: serial,
            _dir: dir,
            head: git(&path, &["rev-parse", "HEAD"]),
            engine: Some(start(&base.join("data/jojo.sqlite3"))),
            base,
            repository,
            commands: AtomicU64::new(1),
            namespace: AtomicU64::new(50),
        };
        let repository = t.repository.clone();
        t.cmd(move |tx| {
            tx.set_admission(Admission::Open)?;
            tx.register_local_repository(&repository)?;
            Ok("ready".into())
        })
        .unwrap();
        t
    }

    fn e(&self) -> &Engine {
        self.engine.as_ref().unwrap()
    }

    /// The helper ends and another starts on the same data root: a new engine epoch whose
    /// admission stays closed until a reviewed recovery inspection.
    fn restart(&mut self) {
        drop(self.engine.take());
        self.engine = Some(start(&self.base.join("data/jojo.sqlite3")));
    }

    /// The reviewed start after a restart, as the Recovery Center sends it.
    fn recover(&self) {
        let inspection = inspect_recovery(self.e(), &os(), &|_| None).unwrap();
        assert!(
            inspection.subjects().is_empty(),
            "a turn is never a recovery subject"
        );
        self.cmd(move |tx| {
            tx.start_after_recovery(&inspection, human("M2 operator"))?;
            Ok("started".into())
        })
        .unwrap();
    }

    fn cmd<F>(&self, f: F) -> Result<String, String>
    where
        F: FnOnce(&mut Tx<'_>) -> Result<String, TxError> + Send + 'static,
    {
        let id = (7u128 << 64) | u128::from(self.commands.fetch_add(1, Ordering::SeqCst));
        let request = CommandRequest::new(CommandId(OpaqueId(id)), self.e().epoch(), "Test", "");
        match self.e().execute(request, f) {
            Ok(r) => Ok(r.response),
            Err(CommandError::Rejected(TxError::Rejected(why))) => Err(why),
            Err(e) => Err(format!("{e:?}")),
        }
    }

    fn read<R>(&self, f: impl FnOnce(&ReadView<'_>) -> Result<R, StorageError>) -> R {
        self.e().reader().unwrap().read(f).unwrap()
    }

    fn request(&self, turn: u128, provider: ProviderKind, message: &str) -> TurnRequest {
        TurnRequest {
            turn: OpaqueId(turn),
            feature: FEATURE,
            kind: TurnKind::Discover,
            repository: self.repository.id(),
            provider,
            message: message.into(),
        }
    }

    fn accept(&self, request: &TurnRequest) -> Result<String, String> {
        let (request, base) = (request.clone(), repository_head(&self.repository).unwrap());
        self.cmd(move |tx| tx.accept_turn(&request, &base))
    }

    fn executable(&self) -> PathBuf {
        self.base.join("bin/agent")
    }

    fn digest(&self) -> String {
        sha256_hex(&[&fs::read(self.executable()).unwrap()])
    }

    fn claude(&self) -> ClaudeAdapter {
        ClaudeAdapter::new(ClaudeAdapterConfig {
            executable: self.executable(),
            version: CLAUDE_VERSION.into(),
            sha256: self.digest(),
            home: self.base.join("home"),
            user: "tester".into(),
            auth: AuthMode::NativeSubscription,
        })
        .unwrap()
    }

    fn codex(&self) -> CodexAdapter {
        CodexAdapter::new(CodexAdapterConfig {
            executable: self.executable(),
            version: CODEX_VERSION.into(),
            sha256: self.digest(),
            home: self.base.join("codex-home"),
            auth: AuthMode::NativeSubscription,
        })
        .unwrap()
    }

    /// The frozen request and the OS request of an accepted turn, through the adapter.
    fn prepare(&self, turn: u128, provider: ProviderKind) -> (TurnSpec, ProcessLaunch) {
        let (fingerprint, variables) = match provider {
            ProviderKind::ClaudeCode => (
                self.claude().turn_fingerprint(),
                CLAUDE_VARIABLES.map(String::from).to_vec(),
            ),
            ProviderKind::Codex => (
                self.codex().turn_fingerprint(),
                vec![CODEX_HOME_VARIABLE.to_owned()],
            ),
        };
        let spec = turn_spec(
            self.e(),
            OpaqueId(turn),
            &self.repository,
            &fingerprint,
            variables,
        )
        .unwrap();
        let launch = match provider {
            ProviderKind::ClaudeCode => self.claude().prepare_turn(&spec).unwrap(),
            ProviderKind::Codex => self.codex().prepare_turn(&spec).unwrap(),
        };
        (spec, launch)
    }

    /// Runs an accepted turn to its sealed outcome on the real supervised launch.
    fn finish(
        &self,
        spec: &TurnSpec,
        launch: &ProcessLaunch,
        during: impl FnOnce() + Send,
    ) -> (TurnRun, TurnOutcome) {
        let ids = SequenceIds::new(self.namespace.fetch_add(1, Ordering::SeqCst), 1);
        let clock = RealClock(Instant::now());
        let ctx = LaunchContext {
            engine: self.e(),
            ids: &ids,
            clock: &clock,
            poll: Duration::from_millis(10),
        };
        let plan = StopPlan::new(&caps(Support::Supported, Support::Supported), BUDGETS);
        let hooks = LaunchHooks {
            inspector: Some(ps()),
            ..LaunchHooks::default()
        };
        let run = std::thread::scope(|s| {
            let runner = s.spawn(|| run_turn(&ctx, spec, launch, &plan, hooks));
            during();
            runner.join().unwrap()
        });
        let outcome = settle_turn(&ctx, spec, &run).unwrap();
        (run, outcome)
    }

    /// Accepts and runs one turn; returns its stored record.
    fn turn(&self, turn: u128, provider: ProviderKind, message: &str) -> TurnRecord {
        self.accept(&self.request(turn, provider, message)).unwrap();
        let (spec, launch) = self.prepare(turn, provider);
        let (_, outcome) = self.finish(&spec, &launch, || {});
        let record = self.read(|v| v.turn(OpaqueId(turn))).unwrap();
        assert_eq!(record.result.as_ref().unwrap().outcome, outcome);
        record
    }

    fn questions(&self) -> Vec<(String, String, String, String)> {
        let mut reader = self.e().reader().unwrap();
        reader
            .read(|v| {
                let mut found = Vec::new();
                for a in v.aggregates()? {
                    if a.key.kind == "question" {
                        let q = v.question(QuestionId(a.key.id))?.unwrap();
                        found.push((
                            q.created_event,
                            (
                                q.prompt,
                                format!("{:?}", q.disposition),
                                format!("{:?}", q.provenance.author),
                                q.provenance.note,
                            ),
                        ));
                    }
                }
                found.sort();
                Ok(found.into_iter().map(|(_, q)| q).collect())
            })
            .unwrap()
    }

    fn question_id(&self, prompt: &str) -> (QuestionId, Revision) {
        self.read(|v| {
            for a in v.aggregates()? {
                if a.key.kind == "question" {
                    let q = v.question(QuestionId(a.key.id))?.unwrap();
                    if q.prompt == prompt {
                        return Ok(Some((q.id, q.revision)));
                    }
                }
            }
            Ok(None)
        })
        .unwrap()
    }

    fn repository_untouched(&self) {
        let path = self.repository.path();
        assert_eq!(git(path, &["rev-parse", "HEAD"]), self.head);
        assert_eq!(
            git(path, &["status", "--porcelain", "--untracked-files=all"]),
            ""
        );
        assert_eq!(
            git(path, &["for-each-ref", "--format=%(refname)"]),
            "refs/heads/main"
        );
    }

    /// What the run wrote to standard output so far; empty before it was spawned.
    fn stdout(&self, spec: &TurnSpec) -> String {
        fs::read_to_string(spec.raw_output_dir.join("generation-1/stdout.raw")).unwrap_or_default()
    }
}

const ASKED: &str = "Should the tool read from arguments, standard input, or both?";
const OBJECTIVE: &str = "slugify should be usable from the shell.\nfixture: lead";

#[test]
fn m2_discovery_rounds_turn_replies_into_model_questions_and_prd_drafts_never_approvals() {
    let t = T::new();
    // Round 1: nothing is known, so the lead asks. Nothing is drafted yet.
    let first = t.turn(1, ProviderKind::ClaudeCode, OBJECTIVE);
    let result = first.result.clone().unwrap();
    assert_eq!(
        (result.outcome, result.detail.as_str(), result.prd),
        (TurnOutcome::Replied, "", None)
    );
    assert_eq!(
        result.summary,
        "The repository has a slugify stub and its tests."
    );
    assert_eq!(
        (first.ordinal, first.kind, first.base.as_str()),
        (1, TurnKind::Discover, t.head.as_str())
    );
    let note = format!("lead turn {:032x}", 1);
    assert_eq!(
        t.questions(),
        [
            (
                ASKED.to_owned(),
                "Blocking".to_owned(),
                "Model".to_owned(),
                note.clone()
            ),
            (
                "Is packaging in scope?".to_owned(),
                "Deferred".to_owned(),
                "Model".to_owned(),
                note
            ),
        ]
    );
    let prompt = t.read(|v| v.turn_prompt(OpaqueId(1))).unwrap();
    assert!(prompt.contains(OBJECTIVE) && prompt.contains("# Questions and answers so far\nNone."));
    assert_eq!(sha256_hex(&[prompt.as_bytes()]), first.prompt_digest);

    // The human answers; round 2 carries the answer and yields the first draft.
    let (question, revision) = t.question_id(ASKED);
    t.cmd(move |tx| {
        let human = Provenance::new(Author::Human, "typed in Feature Chat");
        tx.answer_question(question, revision, "Arguments only.", &human)?;
        Ok("answered".into())
    })
    .unwrap();
    let second = t.turn(2, ProviderKind::Codex, "");
    let prompt = t.read(|v| v.turn_prompt(OpaqueId(2))).unwrap();
    assert!(prompt.contains(&format!("Q1 (blocking): {ASKED}\nAnswer: Arguments only.")));
    assert!(prompt.contains("Q2 (deferred): Is packaging in scope?\nAnswer: (not answered)"));
    assert!(!prompt.contains("Note from the product owner"));
    let (prd, revision) = second.result.clone().unwrap().prd.unwrap();
    assert_eq!(revision, Revision(1));
    let version = t.read(|v| v.artifact_version(prd, None)).unwrap();
    assert_eq!(
        (version.feature, version.provenance.author),
        (FEATURE, Author::Model)
    );
    assert_eq!(version.provenance.note, format!("lead turn {:032x}", 2));
    assert!(version.rendered.contains("Slugify from the shell"));
    // A draft is a draft: complete and answered, so it may be offered for review, and
    // nothing has approved it.
    let readiness = t.read(|v| v.review_readiness(prd)).unwrap();
    assert!(readiness.is_ready(), "{:?}", readiness.blockers);
    assert_eq!(t.read(|v| v.current_artifact_approval(prd)), None);
    assert_eq!(t.read(|v| v.human_decisions(prd)), vec![]);
    assert_eq!(t.questions().len(), 2, "no question was asked twice");

    // Round 3 revises the same document; round 4 says the same again and adds nothing.
    let third = t.turn(3, ProviderKind::ClaudeCode, "Tighten the title.");
    let prompt = t.read(|v| v.turn_prompt(OpaqueId(3))).unwrap();
    assert!(prompt.contains("# Note from the product owner for this round\nTighten the title."));
    assert!(prompt.contains(&version.content));
    assert_eq!(third.result.clone().unwrap().prd, Some((prd, Revision(2))));
    let fourth = t.turn(4, ProviderKind::Codex, "");
    assert_eq!(
        fourth
            .result
            .clone()
            .map(|r| (r.outcome, r.prd, r.summary.is_empty())),
        Some((TurnOutcome::Replied, None, false))
    );
    let versions = t.read(|v| v.artifact_versions(prd));
    assert_eq!(versions.len(), 2);
    assert!(
        versions[1]
            .rendered
            .contains("Slugify from the shell, revised")
    );
    assert!(
        versions
            .iter()
            .all(|v| v.provenance.author == Author::Model)
    );
    assert_eq!(t.read(|v| v.current_artifact_approval(prd)), None);
    let ordinals: Vec<u64> = t
        .read(|v| v.feature_turns(FEATURE))
        .iter()
        .map(|r| r.ordinal)
        .collect();
    assert_eq!(ordinals, [1, 2, 3, 4]);
    assert_eq!(t.read(|v| v.unfinished_turn()), None);
    t.repository_untouched();
}

#[test]
fn m2_what_crosses_the_os_boundary_is_the_read_only_vector_and_the_providers_own_variables() {
    let t = T::new();
    for (index, provider) in BOTH.into_iter().enumerate() {
        let turn = 10 + index as u128;
        t.accept(&t.request(turn, provider, "see what arrives\nfixture: report"))
            .unwrap();
        let (spec, launch) = t.prepare(turn, provider);
        assert_eq!(launch.cwd, t.repository.path());
        assert_eq!(launch.stdin, spec.prompt);
        assert!(spec.raw_output_dir.starts_with(t.base.join("data/turns")));
        let (run, outcome) = t.finish(&spec, &launch, || {});
        assert_eq!(run.conclusion, TurnConclusion::TreeAbsent);
        // What the child saw is not a reply, so the turn failed and stored nothing.
        assert_eq!(outcome, TurnOutcome::Failed);
        let record = t.read(|v| v.turn(OpaqueId(turn))).unwrap();
        let detail = record.result.unwrap().detail;
        assert!(
            detail.starts_with("the last message is not a reply"),
            "{detail}"
        );
        let seen = t.stdout(&spec);
        let repository = t.repository.path().display().to_string();
        let home = t.base.display().to_string();
        match provider {
            ProviderKind::ClaudeCode => {
                assert!(
                    seen.contains(r#"\"--tools\", \"Read,Glob,Grep\""#),
                    "{seen}"
                );
                assert!(
                    seen.contains(r#"\"--permission-mode\", \"manual\""#),
                    "{seen}"
                );
                assert!(
                    seen.contains(&format!(
                        r#"variables=[\"HOME={home}/home\", \"USER=tester\"]"#
                    )),
                    "{seen}"
                );
            }
            ProviderKind::Codex => {
                assert!(seen.contains(r#"\"--sandbox\", \"read-only\""#), "{seen}");
                assert!(
                    seen.contains(&format!(r#"\"-C\", \"{repository}\"]"#)),
                    "{seen}"
                );
                assert!(
                    seen.contains(&format!(r#"variables=[\"CODEX_HOME={home}/codex-home\"]"#)),
                    "{seen}"
                );
            }
        }
        for word in ["Write", "Edit", "acceptEdits", "workspace-write"] {
            assert!(!seen.contains(word), "{word}: {seen}");
        }
        // The frozen request cannot be bent after the fact.
        let mut other = launch.clone();
        other.args.push("--add-dir".into());
        let (run, _) = (
            {
                let ids = SequenceIds::new(99, 1);
                let clock = RealClock(Instant::now());
                let ctx = LaunchContext {
                    engine: t.e(),
                    ids: &ids,
                    clock: &clock,
                    poll: Duration::from_millis(10),
                };
                let plan = StopPlan::new(&caps(Support::Supported, Support::Supported), BUDGETS);
                run_turn(&ctx, &spec, &other, &plan, LaunchHooks::default())
            },
            (),
        );
        assert_eq!(
            run.conclusion,
            TurnConclusion::NotSpawned("process request differs from the frozen turn".into())
        );
    }
    assert_eq!(t.questions(), vec![]);
    t.repository_untouched();
}

#[test]
fn m2_a_turn_that_changes_a_path_or_answers_in_prose_fails_and_stores_nothing() {
    let t = T::new();
    let mut turn = 20;
    for provider in BOTH {
        for (scenario, expected) in [
            (
                "lead-write",
                "protocol failure: tool outside the approved envelope",
            ),
            ("lead-prose", "the last message is not a reply: Json("),
        ] {
            turn += 1;
            let record = t.turn(turn, provider, &format!("try it\nfixture: {scenario}"));
            let result = record.result.unwrap();
            assert_eq!(
                result.outcome,
                TurnOutcome::Failed,
                "{provider:?} {scenario}"
            );
            assert!(result.detail.starts_with(expected), "{}", result.detail);
            assert_eq!((result.summary.as_str(), result.prd), ("", None));
        }
    }
    assert_eq!(t.questions(), vec![]);
    assert_eq!(
        t.read(|v| v.aggregates())
            .iter()
            .filter(|a| a.key.kind == "artifact")
            .count(),
        0
    );
    // A failed turn is finished: the next one is accepted and works.
    let record = t.turn(30, ProviderKind::ClaudeCode, "fixture: lead");
    assert_eq!(record.result.unwrap().outcome, TurnOutcome::Replied);
    assert_eq!((record.ordinal, t.questions().len()), (5, 2));
    t.repository_untouched();
}

#[test]
fn m2_a_running_turn_is_stopped_by_the_human_or_the_deadline_and_leaves_no_process() {
    let t = T::new();
    for (index, provider) in BOTH.into_iter().enumerate() {
        let turn = 40 + index as u128;
        t.accept(&t.request(turn, provider, "wait\nfixture: lead-hang 120"))
            .unwrap();
        let (spec, launch) = t.prepare(turn, provider);
        let (run, outcome) = t.finish(&spec, &launch, || {
            // The run exists before it is stopped: its spool has the first records.
            until(|| !t.stdout(&spec).is_empty());
            t.cmd(move |tx| {
                tx.request_turn_stop(OpaqueId(turn))?;
                tx.request_turn_stop(OpaqueId(turn))?;
                Ok("stop".into())
            })
            .unwrap();
        });
        assert_eq!(run.conclusion, TurnConclusion::TreeAbsent, "{provider:?}");
        assert!(run.cancelled && run.stop.as_deref() == Some("stop requested"));
        assert_eq!(outcome, TurnOutcome::Cancelled);
        let record = t.read(|v| v.turn(OpaqueId(turn))).unwrap();
        assert!(record.stop_requested);
        assert_eq!(record.result.unwrap().detail, "stopped by the human");
        assert_eq!(
            t.cmd(move |tx| tx.request_turn_stop(OpaqueId(turn)).map(|()| "stop".into())),
            Err("TurnFinished".into())
        );
    }
    t.accept(&t.request(50, ProviderKind::ClaudeCode, "wait\nfixture: lead-hang 120"))
        .unwrap();
    let (mut spec, _) = t.prepare_again(50);
    spec.timeout_millis = 300;
    let launch = t.claude().prepare_turn(&spec).unwrap();
    let (run, outcome) = t.finish(&spec, &launch, || {});
    assert_eq!(run.conclusion, TurnConclusion::TreeAbsent);
    assert!(!run.cancelled);
    assert_eq!(outcome, TurnOutcome::Failed);
    let record = t.read(|v| v.turn(OpaqueId(50))).unwrap();
    assert_eq!(record.result.unwrap().detail, "deadline exceeded");
    assert_eq!(
        t.cmd(|tx| tx.request_turn_stop(OpaqueId(51)).map(|()| "stop".into())),
        Err("UnknownTurn".into())
    );
    assert_eq!(t.questions(), vec![]);
    t.repository_untouched();
}

impl T {
    /// `prepare` for the Claude Code adapter, kept apart so a test can adjust the spec.
    fn prepare_again(&self, turn: u128) -> (TurnSpec, ProcessLaunch) {
        self.prepare(turn, ProviderKind::ClaudeCode)
    }
}

#[test]
fn m2_acceptance_holds_admission_capacity_and_the_shape_of_a_feature() {
    let t = T::new();
    let refused = |request: &TurnRequest| t.accept(request).unwrap_err();
    let good = t.request(60, ProviderKind::ClaudeCode, OBJECTIVE);
    // A feature begins with a discovery turn that states an objective.
    assert_eq!(
        refused(&TurnRequest {
            kind: TurnKind::Plan,
            ..good.clone()
        }),
        "TurnRefused: a feature begins with a discovery turn"
    );
    for empty in ["", "  \n", "?!"] {
        assert_eq!(
            refused(&t.request(60, ProviderKind::ClaudeCode, empty)),
            "TurnRefused: the first turn needs an objective"
        );
    }
    assert_eq!(
        refused(&t.request(60, ProviderKind::Codex, &"x".repeat(16 * 1024 + 1))),
        "TurnRefused: the message is too long or contains NUL"
    );
    assert_eq!(
        refused(&t.request(60, ProviderKind::Codex, "a\0b")),
        "TurnRefused: the message is too long or contains NUL"
    );
    assert_eq!(
        refused(&TurnRequest {
            repository: RepositoryId(OpaqueId(1)),
            ..good.clone()
        }),
        "TurnRefused: the repository is not registered"
    );
    t.cmd(|tx| {
        tx.set_admission(Admission::Closed)
            .map(|()| "closed".into())
    })
    .unwrap();
    assert_eq!(refused(&good), "AdmissionClosed");
    t.cmd(|tx| tx.set_admission(Admission::Open).map(|()| "open".into()))
        .unwrap();
    assert_eq!(t.read(|v| v.feature_turns(FEATURE)), vec![]);

    // One run at a time: while a turn has no result, nothing else is accepted.
    assert_eq!(
        t.accept(&good).unwrap(),
        format!("accepted turn={:032x}", 60)
    );
    assert_eq!(t.read(|v| v.unfinished_turn()), Some(OpaqueId(60)));
    let other = TurnRequest {
        feature: FeatureId(OpaqueId(901)),
        ..t.request(61, ProviderKind::Codex, OBJECTIVE)
    };
    for request in [&other, &t.request(61, ProviderKind::Codex, "")] {
        assert_eq!(
            refused(request),
            "TurnRefused: another lead turn is running"
        );
    }
    let (spec, launch) = t.prepare(60, ProviderKind::ClaudeCode);
    assert_eq!(t.finish(&spec, &launch, || {}).1, TurnOutcome::Replied);
    // A turn is sealed once.
    assert_eq!(
        t.cmd(|tx| tx.finish_turn(OpaqueId(60), TurnOutcome::Failed, "again", None, &[])),
        Err("TurnFinished".into())
    );
    // A second repository cannot join the feature; a plan needs approved requirements.
    let elsewhere = t.base.join("disposable/other");
    fs::create_dir_all(&elsewhere).unwrap();
    git(
        &elsewhere,
        &["init", "--template=", "--initial-branch=main"],
    );
    fs::write(elsewhere.join("a.txt"), "a\n").unwrap();
    git(&elsewhere, &["add", "--all"]);
    git(&elsewhere, &["commit", "-m", "other"]);
    let second = LocalRepository::inspect(&elsewhere).unwrap();
    let registered = second.clone();
    t.cmd(move |tx| {
        tx.register_local_repository(&registered)
            .map(|_| "ok".into())
    })
    .unwrap();
    assert_eq!(
        refused(&TurnRequest {
            repository: second.id(),
            ..t.request(62, ProviderKind::Codex, "")
        }),
        "TurnRefused: the feature belongs to another repository"
    );
    assert_eq!(
        refused(&TurnRequest {
            kind: TurnKind::Plan,
            ..t.request(62, ProviderKind::Codex, "")
        }),
        "TurnRefused: a plan turn needs the approved requirements document"
    );
    // The other feature can now begin, in its own order.
    t.accept(&other).unwrap();
    let record = t.read(|v| v.turn(OpaqueId(61))).unwrap();
    assert_eq!((record.ordinal, record.feature.0), (1, OpaqueId(901)));
}

#[test]
fn m2_a_turn_left_by_a_dead_helper_is_closed_as_interrupted_and_holds_nothing() {
    let mut t = T::new();
    t.accept(&t.request(70, ProviderKind::Codex, OBJECTIVE))
        .unwrap();
    // The turn of the running epoch is never closed from under its runner.
    assert_eq!(
        t.cmd(|tx| tx.close_interrupted_turns().map(|n| n.to_string())),
        Ok("0".into())
    );
    t.restart();
    assert_eq!(t.read(|v| v.unfinished_turn()), Some(OpaqueId(70)));
    assert_eq!(
        t.accept(&t.request(71, ProviderKind::Codex, "")),
        Err("AdmissionClosed".into())
    );
    t.recover();
    assert_eq!(
        t.accept(&t.request(71, ProviderKind::Codex, "")),
        Err("TurnRefused: another lead turn is running".into())
    );
    assert_eq!(
        t.cmd(|tx| tx.close_interrupted_turns().map(|n| n.to_string())),
        Ok("1".into())
    );
    let record = t.read(|v| v.turn(OpaqueId(70))).unwrap();
    let result = record.result.unwrap();
    assert_eq!(result.outcome, TurnOutcome::Interrupted);
    assert_eq!(
        result.detail,
        "the helper that ran this turn ended before the turn did"
    );
    assert_eq!(
        t.cmd(|tx| tx.close_interrupted_turns().map(|n| n.to_string())),
        Ok("0".into())
    );
    // Nothing was held: no claim, no worktree, no recovery subject; the feature goes on.
    assert_eq!(t.read(|v| v.live_claims()), vec![]);
    let record = t.turn(71, ProviderKind::ClaudeCode, "");
    assert_eq!(
        (record.ordinal, record.result.unwrap().outcome),
        (2, TurnOutcome::Replied)
    );
    assert_eq!(t.questions().len(), 2);
    t.repository_untouched();
}

#[test]
fn m2_a_turn_request_is_refused_before_spawn_unless_it_is_exactly_the_turn_contract() {
    let t = T::new();
    t.accept(&t.request(80, ProviderKind::ClaudeCode, OBJECTIVE))
        .unwrap();
    let (spec, launch) = t.prepare(80, ProviderKind::ClaudeCode);
    // The developer fingerprint, envelope and vector are not a turn's.
    let mut developer = spec.clone();
    developer.fingerprint = t.claude().fingerprint().clone();
    assert!(matches!(
        t.claude().prepare_turn(&developer),
        Err(ClaudeRejection::Fingerprint)
    ));
    assert!(matches!(
        t.codex().prepare_turn(&spec),
        Err(CodexRejection::Fingerprint)
    ));
    let mut args = launch.args.clone();
    args[9] = "Read,Edit,Write,Glob,Grep".into();
    assert!(matches!(
        ProcessLaunch::for_turn(&spec, args, launch.environment.clone()),
        Err(LaunchRejection::Argument(_))
    ));
    assert!(matches!(
        ProcessLaunch::for_turn(
            &spec,
            jojo_domain::claude::print_argv(),
            launch.environment.clone()
        ),
        Err(LaunchRejection::Argument(_))
    ));
    // The home may hold the repository, never the other way round, and never extra names.
    let inside = t.repository.path().display().to_string();
    for environment in [
        vec![("HOME".into(), inside), ("USER".into(), "tester".into())],
        vec![("HOME".into(), t.base.join("home").display().to_string())],
        vec![
            ("HOME".into(), t.base.join("home").display().to_string()),
            ("USER".into(), "tester".into()),
            ("PATH".into(), "/usr/bin".into()),
        ],
    ] {
        assert!(matches!(
            ProcessLaunch::for_turn(&spec, launch.args.clone(), environment),
            Err(LaunchRejection::Environment(_))
        ));
    }
    // Other executable bytes than the approved ones are refused at the OS boundary.
    let mut other = spec.clone();
    other.fingerprint.configuration = jojo_domain::claude::configuration(&"0".repeat(64)).unwrap();
    assert!(matches!(
        ProcessLaunch::for_turn(&other, launch.args.clone(), launch.environment.clone()),
        Err(LaunchRejection::Executable(_))
    ));
    // A Plan spec or a developer parser cannot pass the turn rules either.
    let mut parser = spec.clone();
    parser.fingerprint.parser = jojo_domain::claude::CLAUDE_PRINT_STREAM_JSON.into();
    assert!(matches!(
        ProcessLaunch::for_turn(&parser, launch.args.clone(), launch.environment.clone()),
        Err(LaunchRejection::Spec(_))
    ));
    assert_eq!(
        ProcessLaunch::for_turn(&spec, launch.args.clone(), launch.environment.clone()).as_ref(),
        Ok(&launch)
    );
}

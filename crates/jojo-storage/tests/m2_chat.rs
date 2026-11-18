//! M2: Feature Chat driven by protocol 1.2 frames only. Every state change is a frame
//! handled by the production `Service`; the tests read the database only to assert. The
//! "provider" is a private copy of `jojo-codex-fixture` behind the production adapters.
//! No installed CLI is named, located or executed; no credential, network or inference.
mod common;
use common::attempts::{SERIAL, git, ps, until};
use common::{TempDir, start};
use jojo_domain::claude::CLAUDE_VERSION;
use jojo_domain::codex::CODEX_VERSION;
use jojo_domain::identity::*;
use jojo_domain::ipc::*;
use jojo_domain::json::Json;
use jojo_domain::provider::{AuthMode, ProviderKind};
use jojo_helper::service::*;
use jojo_storage::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;

const FIXTURE: &str = env!("CARGO_BIN_EXE_jojo-codex-fixture");
const FEATURE: u128 = 900;

fn chmod(path: &std::path::Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}

struct H {
    _serial: std::sync::MutexGuard<'static, ()>,
    dir: TempDir,
    base: PathBuf,
    repository: PathBuf,
    service: Option<Service>,
    next: u128,
}

impl H {
    fn new() -> Self {
        let serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let dir = TempDir::new("m2-chat");
        let base = fs::canonicalize(dir.path()).unwrap();
        for name in ["disposable", "home", "codex-home"] {
            fs::create_dir(base.join(name)).unwrap();
            chmod(&base.join(name), 0o700);
        }
        let repository = base.join("disposable/sample");
        fs::create_dir_all(repository.join("src")).unwrap();
        git(
            &repository,
            &["init", "--template=", "--initial-branch=main"],
        );
        fs::write(repository.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
        git(&repository, &["add", "--all"]);
        git(&repository, &["commit", "-m", "sample"]);
        fs::create_dir(base.join("bin")).unwrap();
        fs::copy(FIXTURE, base.join("bin/agent")).unwrap();
        chmod(&base.join("bin/agent"), 0o755);
        // M5: the fake GitHub CLI, found on the PATH the remote's discovery is given.
        fs::copy(FIXTURE, base.join("bin/gh")).unwrap();
        chmod(&base.join("bin/gh"), 0o755);
        let mut h = Self {
            _serial: serial,
            dir,
            base,
            repository,
            service: None,
            next: 10,
        };
        h.open();
        h
    }

    fn digest(&self) -> String {
        sha256_hex(&[&fs::read(self.base.join("bin/agent")).unwrap()])
    }

    /// A helper (re)starts on the same data root with both adapters configured.
    fn open(&mut self) {
        drop(self.service.take());
        let engine = Arc::new(start(&self.dir.db()));
        let config = ServiceConfig {
            slice: Some(SliceConfig {
                disposable_root: self.base.join("disposable"),
                worktree_root: self.base.join("owned"),
                codex: Some(CodexAdapterConfig {
                    executable: self.base.join("bin/agent"),
                    version: CODEX_VERSION.into(),
                    sha256: self.digest(),
                    home: self.base.join("codex-home"),
                    auth: AuthMode::NativeSubscription,
                }),
            }),
            inspector: Some(ps()),
            launch_hooks: None,
        };
        let mut service = Service::with_config(engine, config).unwrap();
        service.claude = Some(ClaudeAdapterConfig {
            executable: self.base.join("bin/agent"),
            version: CLAUDE_VERSION.into(),
            sha256: self.digest(),
            home: self.base.join("home"),
            user: "tester".into(),
            auth: AuthMode::NativeSubscription,
        });
        service.remote = Some(RemoteConfig::new(
            Discovery::with_search_path(self.base.join("bin").display().to_string()),
            self.base.join("home"),
        ));
        self.service = Some(service);
        let hello = self.at(2, "Hello", object([]));
        assert_eq!(
            get(&hello.payload, "compatible").unwrap(),
            &Json::Bool(true)
        );
    }

    fn s(&mut self) -> &mut Service {
        self.service.as_mut().unwrap()
    }

    fn frame(&mut self, request: u128, minor: u64, kind: &str, payload: Json) -> Envelope {
        let e = Envelope {
            major: 1,
            minor,
            session: OpaqueId(5),
            epoch: self.s().engine.epoch().0,
            request: OpaqueId(request),
            revision: None,
            kind: kind.into(),
            payload,
        };
        self.s().request(&e)
    }

    fn at(&mut self, minor: u64, kind: &str, payload: Json) -> Envelope {
        self.next += 1;
        self.frame(self.next, minor, kind, payload)
    }

    fn ok(&mut self, kind: &str, payload: Json) -> Json {
        let r = self.at(2, kind, payload);
        assert_eq!(r.kind, "Response", "{kind}: {:?}", r.payload);
        r.payload
    }

    fn refused(&mut self, kind: &str, payload: Json) -> String {
        let r = self.at(2, kind, payload);
        assert_eq!(r.kind, "Error", "{kind}: {:?}", r.payload);
        string(get(&r.payload, "code").unwrap()).unwrap().to_owned()
    }

    fn read<T>(&mut self, f: impl FnOnce(&ReadView<'_>) -> Result<T, StorageError>) -> T {
        self.s().engine.reader().unwrap().read(f).unwrap()
    }

    /// Registers the sample repository and opens admission after a reviewed inspection.
    fn ready(&mut self) -> String {
        let path = self.repository.display().to_string();
        let r = self.at(1, "RegisterRepository", object([("path", text(path))]));
        let repository = string(get(&r.payload, "response").unwrap())
            .unwrap()
            .to_owned();
        let inspected = self.ok("RecoveryInspect", object([]));
        let digest = string(get(&inspected, "digest").unwrap())
            .unwrap()
            .to_owned();
        self.ok("RecoveryStart", object([("digest", text(digest))]));
        repository
    }

    fn turn_payload(&self, repository: &str, provider: &str, message: &str) -> Json {
        object([
            ("feature", identity(OpaqueId(FEATURE))),
            ("kind", text("Discover")),
            ("repository", text(repository)),
            ("provider", text(provider)),
            ("message", text(message)),
        ])
    }

    /// Sends one turn under the command identity `turn` and waits for its result.
    fn turn(&mut self, turn: u128, repository: &str, provider: &str, message: &str) -> Json {
        let payload = self.turn_payload(repository, provider, message);
        let r = self.frame(turn, 2, "ChatTurn", payload);
        assert_eq!(r.kind, "Response", "{:?}", r.payload);
        assert_eq!(
            string(get(&r.payload, "dispatch").unwrap()).unwrap(),
            "launched"
        );
        assert_eq!(get(&r.payload, "turn").unwrap(), &identity(OpaqueId(turn)));
        until(|| self.s().workers.reap());
        assert!(!self.s().workers.failed(), "turn worker failed");
        self.chat(0)
    }

    fn spawned(&mut self) -> usize {
        let mut reader = self.s().engine.reader().unwrap();
        reader
            .events_after(EventSeq(0), 10_000)
            .unwrap()
            .iter()
            .filter(|e| e.kind == "LeadTurnSpawned")
            .count()
    }

    /// Approves exactly the review bundle the helper returns for `subject`.
    fn approve(&mut self, subject: &Json) -> Json {
        let bundle = self.ok("ReviewBundle", object([("subject", subject.clone())]));
        let Json::Object(mut fields) = bundle else {
            panic!("no review bundle for {subject:?}")
        };
        let revision = fields
            .iter()
            .position(|(k, _)| k == "revision")
            .map(|i| fields.remove(i).1)
            .unwrap();
        fields.push(("action".into(), text("Approve")));
        self.next += 1;
        let e = Envelope {
            major: 1,
            minor: 2,
            session: OpaqueId(5),
            epoch: self.s().engine.epoch().0,
            request: OpaqueId(self.next),
            revision: Some(uint(&revision).unwrap()),
            kind: "HumanDecision".into(),
            payload: Json::Object(fields),
        };
        let r = self.s().request(&e);
        assert_eq!(r.kind, "Response", "{:?}", r.payload);
        r.payload
    }

    fn approved(&mut self, subject: &Json) -> bool {
        let subject = ArtifactId(id(subject).unwrap());
        self.read(|v| v.current_artifact_approval(subject))
            .is_some()
    }

    /// Discovery to an approved requirements document: ask, answer, draft, approve.
    fn approved_requirements(&mut self, repository: &str) -> Json {
        let chat = self.turn(
            4001,
            repository,
            "Codex",
            "slugify should be usable from the shell.\nfixture: lead",
        );
        for question in items(field(&items(field(&chat, &["turns"]))[0], &["questions"])).to_vec() {
            self.ok(
                "AnswerQuestion",
                object([
                    ("question", field(&question, &["question"]).clone()),
                    ("revision", field(&question, &["revision"]).clone()),
                    ("action", text("Answer")),
                    ("answer", text("As simple as possible.")),
                ]),
            );
        }
        let chat = self.turn(4002, repository, "ClaudeCode", "");
        let document = field(&chat, &["prd", "document"]).clone();
        self.approve(&document);
        assert!(self.approved(&document));
        document
    }

    fn plan_payload(&self, repository: &str, provider: &str, message: &str) -> Json {
        object([
            ("feature", identity(OpaqueId(FEATURE))),
            ("kind", text("Plan")),
            ("repository", text(repository)),
            ("provider", text(provider)),
            ("message", text(message)),
        ])
    }

    fn chat(&mut self, after: u64) -> Json {
        self.ok(
            "FeatureChat",
            object([
                ("feature", identity(OpaqueId(FEATURE))),
                ("after", number(after)),
            ]),
        )
    }

    fn chat_text(&mut self, subject: Json, part: &str) -> Json {
        self.ok(
            "ChatText",
            object([
                ("subject", subject),
                ("part", text(part)),
                ("offset", number(0)),
            ]),
        )
    }
}

fn field<'a>(value: &'a Json, path: &[&str]) -> &'a Json {
    path.iter().fold(value, |v, key| get(v, key).unwrap())
}

fn items(value: &Json) -> &[Json] {
    match value {
        Json::List(items) => items,
        other => panic!("not a list: {other:?}"),
    }
}

fn str_of<'a>(value: &'a Json, path: &[&str]) -> &'a str {
    string(field(value, path)).unwrap()
}

#[test]
fn m2_feature_chat_over_the_protocol_asks_answers_drafts_and_leaves_approval_to_the_human() {
    let mut h = H::new();
    let repository = h.ready();
    // Round 1: the lead reads the repository and asks.
    let objective = "slugify should be usable from the shell.\nfixture: lead";
    let chat = h.turn(1001, &repository, "Codex", objective);
    let turns = items(field(&chat, &["turns"]));
    assert_eq!(turns.len(), 1);
    let first = &turns[0];
    assert_eq!(str_of(first, &["state"]), "Replied");
    assert_eq!(str_of(first, &["kind"]), "Discover");
    assert_eq!(str_of(first, &["provider"]), "Codex");
    assert_eq!(str_of(first, &["message"]), objective);
    assert_eq!(field(first, &["message_complete"]), &Json::Bool(true));
    assert_eq!(field(first, &["prd"]), &Json::Null);
    assert_eq!(
        str_of(first, &["summary"]),
        "The repository has a slugify stub and its tests."
    );
    let questions = items(field(first, &["questions"])).to_vec();
    assert_eq!(questions.len(), 2);
    assert_eq!(field(&questions[0], &["blocking"]), &Json::Bool(true));
    assert_eq!(field(&questions[1], &["blocking"]), &Json::Bool(false));
    assert_eq!(field(&questions[0], &["answer"]), &Json::Null);
    assert_eq!(field(&chat, &["running"]), &Json::Null);
    assert_eq!(field(&chat, &["prd"]), &Json::Null);
    assert_eq!(field(&chat, &["next"]), &Json::Null);
    // The same command again repeats nothing.
    let payload = h.turn_payload(&repository, "Codex", objective);
    let replay = h.frame(1001, 2, "ChatTurn", payload);
    assert_eq!(
        string(get(&replay.payload, "dispatch").unwrap()).unwrap(),
        "not repeated"
    );
    assert_eq!(items(field(&h.chat(0), &["turns"])).len(), 1);
    let reply = h.chat_text(identity(OpaqueId(1001)), "Reply");
    assert!(str_of(&reply, &["text"]).contains("jojo-lead-discovery-v1"));
    assert_eq!(field(&reply, &["complete"]), &Json::Bool(true));
    let prompt = h.chat_text(identity(OpaqueId(1001)), "Prompt");
    assert!(str_of(&prompt, &["text"]).contains("You may only read"));

    // The human answers at the revision shown, and sets the other question aside.
    let question = field(&questions[0], &["question"]).clone();
    let revision = field(&questions[0], &["revision"]).clone();
    let answer = |q: &Json, revision: Json, action: &str, body: Json| {
        object([
            ("question", q.clone()),
            ("revision", revision),
            ("action", text(action)),
            ("answer", body),
        ])
    };
    let stale = uint(&revision).unwrap() + 1;
    assert!(
        h.refused(
            "AnswerQuestion",
            answer(&question, number(stale), "Answer", text("Arguments only."))
        )
        .contains("RevisionConflict")
    );
    h.ok(
        "AnswerQuestion",
        answer(&question, revision, "Answer", text("Arguments only.")),
    );
    let other = field(&questions[1], &["question"]).clone();
    let other_revision = field(&questions[1], &["revision"]).clone();
    h.ok(
        "AnswerQuestion",
        answer(&other, other_revision, "Defer", Json::Null),
    );
    let chat = h.chat(0);
    let asked = items(field(&items(field(&chat, &["turns"]))[0], &["questions"])).to_vec();
    assert_eq!(str_of(&asked[0], &["answer"]), "Arguments only.");
    assert_eq!(str_of(&asked[0], &["answered_by"]), "HUMAN");
    assert_eq!(field(&asked[1], &["blocking"]), &Json::Bool(false));
    let answered = h.chat_text(question.clone(), "Answer");
    assert_eq!(str_of(&answered, &["text"]), "Arguments only.");

    // Round 2 on the other provider: the answers yield a draft nobody approved.
    let chat = h.turn(1002, &repository, "ClaudeCode", "");
    let second = &items(field(&chat, &["turns"]))[1];
    assert_eq!(str_of(second, &["state"]), "Replied");
    assert_eq!(str_of(second, &["provider"]), "ClaudeCode");
    assert_eq!(field(second, &["prd", "revision"]), &number(1));
    let prd = field(&chat, &["prd"]).clone();
    assert_eq!(field(&prd, &["revision"]), &number(1));
    assert_eq!(str_of(&prd, &["author"]), "MODEL");
    assert_eq!(field(&prd, &["ready"]), &Json::Bool(true));
    assert_eq!(field(&prd, &["approved"]), &Json::Bool(false));
    let document = field(&prd, &["document"]).clone();
    let rendered = h.chat_text(document.clone(), "Prd");
    assert!(str_of(&rendered, &["text"]).contains("Slugify from the shell"));
    let subject = ArtifactId(id(&document).unwrap());
    assert_eq!(h.read(|v| v.current_artifact_approval(subject)), None);

    // Approval stays with the human, through the unchanged Approval Inbox command.
    let bundle = h.ok("ReviewBundle", object([("subject", document.clone())]));
    let Json::Object(mut fields) = bundle else {
        panic!("bundle")
    };
    let revision = fields
        .iter()
        .position(|(k, _)| k == "revision")
        .map(|i| fields.remove(i).1)
        .unwrap();
    fields.push(("action".into(), text("Approve")));
    h.next += 1;
    let e = Envelope {
        major: 1,
        minor: 2,
        session: OpaqueId(5),
        epoch: h.s().engine.epoch().0,
        request: OpaqueId(h.next),
        revision: Some(uint(&revision).unwrap()),
        kind: "HumanDecision".into(),
        payload: Json::Object(fields),
    };
    assert_eq!(h.s().request(&e).kind, "Response");
    assert_eq!(field(&h.chat(0), &["prd", "approved"]), &Json::Bool(true));

    // A later round that revises the draft makes that approval stale at once.
    let chat = h.turn(1003, &repository, "Codex", "Tighten the title.");
    assert_eq!(field(&chat, &["prd", "revision"]), &number(2));
    assert_eq!(field(&chat, &["prd", "approved"]), &Json::Bool(false));
    assert_eq!(h.read(|v| v.current_artifact_approval(subject)), None);
    // The index finds the way back to this chat: the repository and the feature.
    let index = h.ok("ChatIndex", object([]));
    let repositories = items(field(&index, &["repositories"])).to_vec();
    assert_eq!(repositories.len(), 1);
    assert_eq!(str_of(&repositories[0], &["repository"]), repository);
    assert!(str_of(&repositories[0], &["path"]).ends_with("disposable/sample"));
    let features = items(field(&index, &["features"])).to_vec();
    assert_eq!(features.len(), 1);
    assert_eq!(
        field(&features[0], &["feature"]),
        &identity(OpaqueId(FEATURE))
    );
    assert_eq!(str_of(&features[0], &["objective"]), objective);
    assert_eq!(field(&features[0], &["turns"]), &number(3));
    assert_eq!(field(&features[0], &["running"]), &Json::Bool(false));
    assert_eq!(field(&index, &["features_complete"]), &Json::Bool(true));
    // Pages: the turns after an ordinal.
    let rest = items(field(&h.chat(2), &["turns"])).to_vec();
    assert_eq!(rest.len(), 1);
    assert_eq!(field(&rest[0], &["ordinal"]), &number(3));
    assert_eq!(
        git(
            &h.repository,
            &["status", "--porcelain", "--untracked-files=all"]
        ),
        ""
    );
}

#[test]
fn m2_a_running_turn_holds_capacity_and_is_stopped_by_cancel_or_close() {
    let mut h = H::new();
    let repository = h.ready();
    let hang = "wait for me\nfixture: lead-hang 120";
    let payload = h.turn_payload(&repository, "ClaudeCode", hang);
    let r = h.frame(2001, 2, "ChatTurn", payload);
    assert_eq!(r.kind, "Response", "{:?}", r.payload);
    let running = field(&h.chat(0), &["running"]).clone();
    assert_eq!(field(&running, &["turn"]), &identity(OpaqueId(2001)));
    assert_eq!(field(&running, &["stop_requested"]), &Json::Bool(false));
    assert_eq!(
        str_of(&items(field(&h.chat(0), &["turns"]))[0], &["state"]),
        "Running"
    );
    // One turn at a time; with a capacity of one (no project record) no Start either.
    let another = h.turn_payload(&repository, "Codex", "");
    assert_eq!(
        h.refused("ChatTurn", another),
        "TurnRefused: another lead turn is running"
    );
    let start = object([
        ("prd", identity(OpaqueId(1))),
        ("adr", identity(OpaqueId(2))),
        ("plan", identity(OpaqueId(3))),
        ("validation", identity(OpaqueId(4))),
        ("step", identity(OpaqueId(5))),
    ]);
    assert_eq!(
        h.refused("Start", start),
        "CapacityFull: 0 attempts and 1 turns use all 1 units; a Start waits"
    );
    until(|| h.spawned() == 1);
    h.ok("CancelTurn", object([("turn", identity(OpaqueId(2001)))]));
    until(|| h.s().workers.reap());
    let row = items(field(&h.chat(0), &["turns"]))[0].clone();
    assert_eq!(str_of(&row, &["state"]), "Cancelled");
    assert_eq!(str_of(&row, &["detail"]), "stopped by the human");
    assert_eq!(
        h.refused("CancelTurn", object([("turn", identity(OpaqueId(2001)))])),
        "TurnFinished"
    );

    // Closing the helper stops a running turn and seals it.
    let payload = h.turn_payload(&repository, "Codex", "fixture: lead-hang 120");
    let r = h.frame(2002, 2, "ChatTurn", payload);
    assert_eq!(r.kind, "Response", "{:?}", r.payload);
    until(|| h.spawned() == 2);
    assert_eq!(h.s().finish(false).unwrap(), None);
    until(|| h.s().workers.reap());
    let row = items(field(&h.chat(0), &["turns"]))[1].clone();
    assert_eq!(str_of(&row, &["state"]), "Failed");
    assert_eq!(str_of(&row, &["detail"]), "the helper is closing");
    assert_eq!(h.read(|v| v.unfinished_turn()), None);
}

#[test]
fn m2_a_turn_left_by_a_dead_helper_is_closed_when_the_next_helper_starts() {
    let mut h = H::new();
    let repository = h.ready();
    // The earlier helper accepted a turn and died before running or sealing it.
    let base = git(&h.repository, &["rev-parse", "HEAD"]);
    let request = TurnRequest {
        turn: OpaqueId(3001),
        feature: FeatureId(OpaqueId(FEATURE)),
        kind: jojo_domain::planner::TurnKind::Discover,
        repository: repository_id(&repository).unwrap(),
        provider: jojo_domain::provider::ProviderKind::Codex,
        message: "fixture: lead".into(),
    };
    let engine = h.s().engine.clone();
    engine
        .execute(
            CommandRequest::new(CommandId(OpaqueId(3001)), engine.epoch(), "ChatTurn", ""),
            move |tx| tx.accept_turn(&request, &base),
        )
        .unwrap();
    drop(engine);
    h.open();
    let row = items(field(&h.chat(0), &["turns"]))[0].clone();
    assert_eq!(str_of(&row, &["state"]), "Interrupted");
    assert_eq!(field(&h.chat(0), &["running"]), &Json::Null);
    // The feature goes on after the reviewed start.
    let inspected = h.ok("RecoveryInspect", object([]));
    assert_eq!(field(&inspected, &["subjects"]), &number(0));
    let digest = str_of(&inspected, &["digest"]).to_owned();
    h.ok("RecoveryStart", object([("digest", text(digest))]));
    let chat = h.turn(3002, &repository, "Codex", "fixture: lead");
    let row = &items(field(&chat, &["turns"]))[1];
    assert_eq!(field(row, &["ordinal"]), &number(2));
    assert_eq!(str_of(row, &["state"]), "Replied");
}

#[test]
fn m2_a_plan_turn_drafts_a_multi_step_plan_that_the_human_approves_document_by_document() {
    let mut h = H::new();
    let repository = h.ready();
    let prd = h.approved_requirements(&repository);
    // The plan turn reads the approved revision and drafts the ADR at once.
    let payload = h.plan_payload(&repository, "Codex", "Keep the steps small.");
    let r = h.frame(4003, 2, "ChatTurn", payload);
    assert_eq!(r.kind, "Response", "{:?}", r.payload);
    until(|| h.s().workers.reap());
    let prompt = h.chat_text(identity(OpaqueId(4003)), "Prompt");
    assert!(str_of(&prompt, &["text"]).contains("# Product requirements (approved revision 1)"));
    let chat = h.chat(0);
    let row = items(field(&chat, &["turns"])).last().unwrap().clone();
    assert_eq!(str_of(&row, &["kind"]), "Plan");
    assert_eq!(str_of(&row, &["state"]), "Replied", "{row:?}");
    let plan = field(&row, &["plan"]).clone();
    assert_eq!(field(&plan, &["drafted"]), &Json::Bool(false));
    let keys: Vec<&str> = items(field(&plan, &["steps"]))
        .iter()
        .map(|s| str_of(s, &["key"]))
        .collect();
    assert_eq!(keys, ["core", "docs", "check"]);
    let asked = items(field(&row, &["questions"])).to_vec();
    assert_eq!(asked.len(), 1);
    assert_eq!(str_of(&asked[0], &["step"]), "docs");
    assert_eq!(field(&asked[0], &["blocking"]), &Json::Bool(true));
    // A question about one step leaves the requirements approval alone.
    assert!(h.approved(&prd));
    let (adr, plan_id, validation) = (
        field(&plan, &["adr"]).clone(),
        field(&plan, &["plan"]).clone(),
        field(&plan, &["validation"]).clone(),
    );
    assert!(!h.approved(&adr));
    assert_eq!(
        h.ok("ReviewBundle", object([("subject", plan_id.clone())])),
        Json::Null
    );

    // Approving the ADR turns the draft into its validation plan and execution plan,
    // neither approved.
    let decided = h.approve(&adr);
    assert!(get(&decided, "lead_plan").is_err(), "{decided:?}");
    let chat = h.chat(0);
    let row = items(field(&chat, &["turns"])).last().unwrap().clone();
    assert_eq!(field(&row, &["plan", "drafted"]), &Json::Bool(true));
    assert!(!h.approved(&plan_id) && !h.approved(&validation));
    let plan_subject = ArtifactId(id(&plan_id).unwrap());
    let stored = h
        .read(|v| {
            let revision = v.review_bundle(plan_subject)?.unwrap().revision;
            v.execution_plan(plan_subject, revision)
        })
        .unwrap();
    let steps: Vec<OpaqueId> = items(field(&row, &["plan", "steps"]))
        .iter()
        .map(|s| id(field(s, &["step"])).unwrap())
        .collect();
    assert_eq!(
        stored
            .plan
            .execution
            .nodes
            .iter()
            .map(|n| n.id)
            .collect::<Vec<_>>(),
        steps
    );
    let mut edges: Vec<(OpaqueId, OpaqueId)> = stored
        .plan
        .execution
        .edges
        .iter()
        .map(|e| (e.prerequisite.id, e.dependent.id))
        .collect();
    edges.sort();
    let mut expected = vec![(steps[0], steps[2]), (steps[1], steps[2])];
    expected.sort();
    assert_eq!(edges, expected);
    for edge in &stored.plan.execution.edges {
        let jojo_domain::dependency::Predicate::Evidence { requirement, .. } = &edge.predicate
        else {
            panic!("a step edge is an evidence predicate");
        };
        assert_eq!(
            requirement.kind,
            jojo_domain::dependency::EvidenceKind::ValidationPassed
        );
    }
    let providers: Vec<_> = stored.plan.resources.iter().map(|r| r.provider).collect();
    assert_eq!(
        providers,
        [
            jojo_domain::provider::ProviderKind::Codex,
            jojo_domain::provider::ProviderKind::ClaudeCode,
            jojo_domain::provider::ProviderKind::ClaudeCode
        ]
    );

    // Each document is approved on its own, then the plan is what the core runs from.
    h.approve(&validation);
    h.approve(&plan_id);
    assert!(h.approved(&prd) && h.approved(&adr));
    assert!(h.approved(&validation) && h.approved(&plan_id));
    let approved = h.ok("ApprovedPlan", object([("after", Json::Null)]));
    assert_eq!(items(field(&approved, &["steps"])).len(), 3);
    assert_eq!(items(field(&approved, &["edges"])).len(), 2);

    // The blocking question about "docs" holds that step's Start, and only that.
    let start = |step: &OpaqueId| {
        object([
            ("prd", prd.clone()),
            ("adr", adr.clone()),
            ("plan", plan_id.clone()),
            ("validation", validation.clone()),
            ("step", identity(*step)),
        ])
    };
    assert_eq!(
        h.refused("Start", start(&steps[1])),
        "StepQuestionOpen: a blocking question about this step is unanswered"
    );
    h.ok(
        "AnswerQuestion",
        object([
            ("question", field(&asked[0], &["question"]).clone()),
            ("revision", field(&asked[0], &["revision"]).clone()),
            ("action", text("Answer")),
            ("answer", text("zsh and bash.")),
        ]),
    );
    let chat = h.chat(0);
    let row = items(field(&chat, &["turns"])).last().unwrap().clone();
    let answered = items(field(&row, &["questions"]))[0].clone();
    assert_eq!(str_of(&answered, &["answer"]), "zsh and bash.");
    let full = h.chat_text(field(&answered, &["question"]).clone(), "Answer");
    assert_eq!(str_of(&full, &["text"]), "zsh and bash.");
    // Answering a step question changed no approval.
    assert!(h.approved(&prd) && h.approved(&plan_id));
    let r = h.at(2, "Start", start(&steps[1]));
    assert_eq!(r.kind, "Response", "{:?}", r.payload);
    assert_eq!(str_of(&r.payload, &["dispatch"]), "launched");
    until(|| h.s().workers.reap());
    let attempt = id(field(&r.payload, &["attempt"])).unwrap();
    let result = h
        .read(|v| v.attempt_result(attempt))
        .expect("sealed result");
    assert_eq!(
        result.outcome,
        jojo_domain::attempt::AttemptOutcome::Succeeded,
        "{result:?}"
    );
}

#[test]
fn m2_a_plan_turn_needs_approved_requirements_and_a_feature_question_stales_them() {
    let mut h = H::new();
    let repository = h.ready();
    let early = h.plan_payload(&repository, "Codex", "");
    let first = h.turn_payload(&repository, "Codex", "objective\nfixture: lead");
    let r = h.frame(4009, 2, "ChatTurn", first);
    assert_eq!(r.kind, "Response", "{:?}", r.payload);
    until(|| h.s().workers.reap());
    assert_eq!(
        h.refused("ChatTurn", early),
        "TurnRefused: a plan turn needs the approved requirements document"
    );
    // The feature already began, so its requirements come from the next turns.
    let chat = h.chat(0);
    for question in items(field(&items(field(&chat, &["turns"]))[0], &["questions"])).to_vec() {
        h.ok(
            "AnswerQuestion",
            object([
                ("question", field(&question, &["question"]).clone()),
                ("revision", field(&question, &["revision"]).clone()),
                ("action", text("Answer")),
                ("answer", text("As simple as possible.")),
            ]),
        );
    }
    let chat = h.turn(4011, &repository, "ClaudeCode", "");
    let prd = field(&chat, &["prd", "document"]).clone();
    h.approve(&prd);
    let payload = h.plan_payload(&repository, "ClaudeCode", "fixture: lead-plan-asks");
    let r = h.frame(4010, 2, "ChatTurn", payload);
    assert_eq!(r.kind, "Response", "{:?}", r.payload);
    until(|| h.s().workers.reap());
    let chat = h.chat(0);
    let row = items(field(&chat, &["turns"])).last().unwrap().clone();
    assert_eq!(str_of(&row, &["state"]), "Replied");
    assert_eq!(field(&row, &["plan"]), &Json::Null);
    let asked = items(field(&row, &["questions"])).to_vec();
    assert_eq!(asked.len(), 1);
    assert_eq!(field(&asked[0], &["step"]), &Json::Null);
    assert_eq!(
        str_of(&asked[0], &["prompt"]),
        "Must the tool also run on Windows?"
    );
    // A new question about the feature is part of what the approval bound.
    assert!(!h.approved(&prd));
    assert_eq!(field(&chat, &["prd", "approved"]), &Json::Bool(false));
    assert_eq!(field(&chat, &["prd", "ready"]), &Json::Bool(false));
    let again = h.plan_payload(&repository, "Codex", "");
    assert_eq!(
        h.refused("ChatTurn", again),
        "TurnRefused: a plan turn needs the approved requirements document"
    );
}

// ---- M3 (protocol 1.3): project records ----

impl H {
    fn ok3(&mut self, kind: &str, payload: Json) -> Json {
        let r = self.at(3, kind, payload);
        assert_eq!(r.kind, "Response", "{kind}: {:?}", r.payload);
        r.payload
    }

    fn refused3(&mut self, kind: &str, payload: Json) -> String {
        let r = self.at(3, kind, payload);
        assert_eq!(r.kind, "Error", "{kind}: {:?}", r.payload);
        string(get(&r.payload, "code").unwrap()).unwrap().to_owned()
    }

    /// The sample project: backend steps go to Claude Code, docs to Codex.
    fn project_payload(&self, project: u128, revision: u64, repository: &str) -> Json {
        let rule = |kind: &str, provider: &str| {
            object([("kind", text(kind)), ("provider", text(provider))])
        };
        object([
            ("project", identity(OpaqueId(project))),
            ("revision", number(revision)),
            ("name", text("Sample")),
            ("repositories", Json::List(vec![text(repository)])),
            ("instructions", text("Never touch the lock file.")),
            ("test_command", Json::List(vec![text("/usr/bin/true")])),
            ("ci_command", Json::List(vec![])),
            (
                "kinds",
                Json::List(vec![rule("backend", "ClaudeCode"), rule("docs", "Codex")]),
            ),
            ("default_provider", text("ClaudeCode")),
            ("capacity", number(2)),
            ("repair_limit", number(2)),
            ("review", Json::Bool(true)),
        ])
    }

    /// A plan turn under the project, then the ADR approved: the plan is drafted.
    fn drafted_plan(&mut self, repository: &str, turn: u128, note: &str) -> Json {
        let payload = self.plan_payload(repository, "Codex", note);
        let r = self.frame(turn, 2, "ChatTurn", payload);
        assert_eq!(r.kind, "Response", "{:?}", r.payload);
        until(|| self.s().workers.reap());
        let chat = self.chat(0);
        items(field(&chat, &["turns"])).last().unwrap().clone()
    }
}

#[test]
fn m3_a_project_record_sets_capacity_and_decides_the_providers_of_a_lead_plan() {
    let mut h = H::new();
    let repository = h.ready();
    let missing = h.project_payload(3001, 0, &format!("{:032x}", 77));
    assert_eq!(
        h.refused3("SaveProject", missing),
        "InvalidProject: a repository is not registered"
    );
    // The 1.2 vocabulary has no project request.
    let early = h.project_payload(3001, 0, &repository);
    assert_eq!(h.refused("SaveProject", early), "UnknownType");
    let saved = h.ok3("SaveProject", h.project_payload(3001, 0, &repository));
    assert_eq!(str_of(&saved, &["response"]), "project revision=1");
    assert_eq!(
        h.refused3("SaveProject", h.project_payload(3001, 0, &repository)),
        "StaleProject: the project changed; read it again"
    );
    assert_eq!(
        h.refused3("SaveProject", h.project_payload(3002, 0, &repository)),
        "InvalidProject: a repository belongs to another project"
    );
    // D4: two in all, not split by provider, no limit per repository.
    let cfg = h.read(|v| v.resource_config());
    for key in ["global", "provider:claude", "provider:codex"] {
        assert_eq!(cfg.capacities.get(key), Some(&2), "{key}");
    }
    assert_eq!(cfg.default_repository, 2);
    // Saving the same capacity again writes no new configuration.
    h.ok3("SaveProject", h.project_payload(3001, 1, &repository));
    assert_eq!(h.read(|v| v.resource_config()).revision, cfg.revision);
    let listed = h.ok3("Projects", object([]));
    let projects = items(field(&listed, &["projects"])).to_vec();
    assert_eq!(projects.len(), 1);
    assert_eq!(field(&projects[0], &["revision"]), &number(2));
    assert_eq!(str_of(&projects[0], &["name"]), "Sample");
    assert_eq!(field(&listed, &["capacity"]), &number(2));

    // A plan turn is told about the project and keeps the revision it was accepted under.
    h.approved_requirements(&repository);
    let row = h.drafted_plan(&repository, 4003, "fixture: lead-kinds");
    assert_eq!(str_of(&row, &["state"]), "Replied", "{row:?}");
    let prompt = h.chat_text(identity(OpaqueId(4003)), "Prompt");
    let prompt = str_of(&prompt, &["text"]);
    assert!(prompt.contains("# Project \"Sample\""), "{prompt}");
    assert!(prompt.contains("- backend: ClaudeCode"));
    assert!(prompt.contains("Never touch the lock file."));
    let plan = field(&row, &["plan"]).clone();
    let (adr, plan_id) = (
        field(&plan, &["adr"]).clone(),
        field(&plan, &["plan"]).clone(),
    );
    let adr_id = ArtifactId(id(&adr).unwrap());
    let adr_text = h.read(|v| v.review_document_content(adr_id)).unwrap();
    assert!(adr_text.contains("## Project instructions"), "{adr_text}");
    assert!(adr_text.contains("revision 2.\n\nNever touch the lock file."));
    h.approve(&adr);
    let plan_subject = ArtifactId(id(&plan_id).unwrap());
    let stored = h
        .read(|v| {
            let revision = v.review_bundle(plan_subject)?.unwrap().revision;
            v.execution_plan(plan_subject, revision)
        })
        .unwrap();
    use jojo_domain::provider::ProviderKind::{ClaudeCode, Codex};
    // The kind's rule wins over the lead's choice; no kind and no choice is the default.
    let providers: Vec<_> = stored.plan.resources.iter().map(|r| r.provider).collect();
    assert_eq!(providers, [ClaudeCode, Codex, ClaudeCode]);
}

#[test]
fn m3_a_plan_with_a_task_kind_the_project_does_not_have_drafts_nothing() {
    let mut h = H::new();
    let repository = h.ready();
    h.ok3("SaveProject", h.project_payload(3001, 0, &repository));
    h.approved_requirements(&repository);
    let row = h.drafted_plan(&repository, 4003, "fixture: lead-kinds-unknown");
    assert_eq!(str_of(&row, &["state"]), "Failed", "{row:?}");
    assert!(
        str_of(&row, &["detail"]).contains("\"frontend\" is not one of the project's"),
        "{row:?}"
    );
    assert_eq!(field(&row, &["plan"]), &Json::Null);
}

impl H {
    /// The sample project's plan, every document approved. Returns the requirements, the
    /// four subjects and the three step identities (api, usage, join).
    fn approved_kinds_plan(&mut self, repository: &str) -> (Json, Json, Json, Json, Vec<OpaqueId>) {
        h_save_project(self, repository);
        let prd = self.approved_requirements(repository);
        let row = self.drafted_plan(repository, 4003, "fixture: lead-kinds");
        let plan = field(&row, &["plan"]).clone();
        let (adr, plan_id, validation) = (
            field(&plan, &["adr"]).clone(),
            field(&plan, &["plan"]).clone(),
            field(&plan, &["validation"]).clone(),
        );
        self.approve(&adr);
        self.approve(&validation);
        self.approve(&plan_id);
        let steps = items(field(&plan, &["steps"]))
            .iter()
            .map(|s| id(field(s, &["step"])).unwrap())
            .collect();
        (prd, adr, plan_id, validation, steps)
    }

    fn event_seq(&mut self, kind: &str, attempt: OpaqueId) -> u64 {
        let prefix = format!("{:032x}", attempt.0);
        let mut reader = self.s().engine.reader().unwrap();
        reader
            .events_after(EventSeq(0), 10_000)
            .unwrap()
            .iter()
            .find(|e| e.kind == kind && e.payload.starts_with(&prefix))
            .unwrap_or_else(|| panic!("no {kind} for {prefix}"))
            .seq
            .0
    }
}

fn h_save_project(h: &mut H, repository: &str) {
    let payload = h.project_payload(3001, 0, repository);
    h.ok3("SaveProject", payload);
}

#[test]
fn m3_two_steps_run_at_once_and_a_lead_turn_takes_one_unit_of_capacity() {
    let mut h = H::new();
    let repository = h.ready();
    let (prd, adr, plan, validation, steps) = h.approved_kinds_plan(&repository);
    let start = |step: OpaqueId| {
        object([
            ("prd", prd.clone()),
            ("adr", adr.clone()),
            ("plan", plan.clone()),
            ("validation", validation.clone()),
            ("step", identity(step)),
        ])
    };
    // One attempt and the lead turn fill the capacity of two.
    let api = h.at(2, "Start", start(steps[0]));
    assert_eq!(
        str_of(&api.payload, &["dispatch"]),
        "launched",
        "{:?}",
        api.payload
    );
    let turn = h.turn_payload(&repository, "Codex", "fixture: lead-hang 30");
    let r = h.frame(2101, 2, "ChatTurn", turn);
    assert_eq!(r.kind, "Response", "{:?}", r.payload);
    assert_eq!(
        h.refused("Start", start(steps[1])),
        "CapacityFull: 1 attempts and 1 turns use all 2 units; a Start waits"
    );
    h.ok("CancelTurn", object([("turn", identity(OpaqueId(2101)))]));
    until(|| h.s().workers.running(Work::Turn) == 0);
    // Two attempts, on two providers, at the same time; the turn now waits.
    let usage = h.at(2, "Start", start(steps[1]));
    assert_eq!(
        str_of(&usage.payload, &["dispatch"]),
        "launched",
        "{:?}",
        usage.payload
    );
    let again = h.turn_payload(&repository, "Codex", "fixture: lead");
    let refused = h.refused("ChatTurn", again);
    assert!(refused.starts_with("CapacityFull: "), "{refused}");
    until(|| h.s().workers.reap());
    assert!(!h.s().workers.failed());
    let (api, usage) = (
        id(field(&api.payload, &["attempt"])).unwrap(),
        id(field(&usage.payload, &["attempt"])).unwrap(),
    );
    for attempt in [api, usage] {
        let result = h
            .read(|v| v.attempt_result(attempt))
            .expect("sealed result");
        assert_eq!(
            result.outcome,
            jojo_domain::attempt::AttemptOutcome::Succeeded,
            "{result:?}"
        );
    }
    // The second attempt was issued before the first one settled.
    assert!(h.event_seq("ProcessLaunchIssued", usage) < h.event_seq("AttemptSettling", api));
}

// ---- M3: the plan runner ----

impl H {
    fn plan_run(&mut self, plan: &Json) -> Json {
        self.ok3("PlanRun", object([("plan", plan.clone())]))
    }

    /// The state of each step of the run, in plan order.
    fn step_states(&mut self, plan: &Json) -> Vec<String> {
        let run = self.plan_run(plan);
        items(field(&run, &["steps"]))
            .iter()
            .map(|s| str_of(s, &["state"]).to_owned())
            .collect()
    }

    /// Runner passes until `want` holds for the run.
    fn run_until(&mut self, plan: &Json, want: impl Fn(&[String]) -> bool) -> Vec<String> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            self.s().tick().unwrap();
            let states = self.step_states(plan);
            if want(&states) {
                return states;
            }
            if std::time::Instant::now() >= deadline {
                let run = self.plan_run(plan);
                panic!("run stuck at {states:?}: {run:?}");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn step_field(&mut self, plan: &Json, index: usize, name: &str) -> Json {
        let run = self.plan_run(plan);
        field(&items(field(&run, &["steps"]))[index], &[name]).clone()
    }

    fn control(&mut self, plan: &Json, action: &str) -> String {
        let r = self.at(
            3,
            "ControlPlan",
            object([("plan", plan.clone()), ("action", text(action))]),
        );
        match r.kind.as_str() {
            "Response" => str_of(&r.payload, &["response"]).to_owned(),
            _ => str_of(&r.payload, &["code"]).to_owned(),
        }
    }
}

#[test]
fn m3_a_started_plan_runs_its_ready_steps_validates_them_and_waits_for_the_human() {
    let mut h = H::new();
    let repository = h.ready();
    let (_, _, plan, _, _) = h.approved_kinds_plan(&repository);
    // Approval alone runs nothing.
    assert_eq!(h.plan_run(&plan), Json::Null);
    h.s().tick().unwrap();
    assert_eq!(h.spawned(), 3, "only the three lead turns ran");
    let started = h.ok3("RunPlan", object([("plan", plan.clone())]));
    assert_eq!(str_of(&started, &["response"]), "plan run revision=1");
    assert!(
        h.refused3("RunPlan", object([("plan", plan.clone())]))
            .starts_with("RunRefused: this plan revision was already started")
    );
    let run = h.plan_run(&plan);
    assert_eq!(str_of(&run, &["state"]), "RUNNING");
    assert_eq!(
        str_of(&run, &["branch"]),
        format!("jojo/plan/{}", str_of(&plan, &[]))
    );
    assert_eq!(h.step_states(&plan), ["Ready", "Ready", "Waiting"]);
    assert_eq!(
        h.step_field(&plan, 2, "detail"),
        text("waits for api, usage")
    );
    assert_eq!(h.step_field(&plan, 0, "provider"), text("ClaudeCode"));
    assert_eq!(h.step_field(&plan, 1, "kind"), text("docs"));
    // One pass starts both independent steps, on two providers, at once.
    h.s().tick().unwrap();
    assert_eq!(h.s().workers.running(Work::Attempt), 2);
    assert_eq!(h.step_states(&plan), ["Running", "Running", "Waiting"]);
    // Each is validated by the host as soon as it settles; acceptance is the human's.
    let states = h.run_until(&plan, |s| s[0] == "Review" && s[1] == "Review");
    assert_eq!(states[2], "Waiting");
    let api = id(&h.step_field(&plan, 0, "attempt")).unwrap();
    let usage = id(&h.step_field(&plan, 1, "attempt")).unwrap();
    assert!(h.event_seq("ProcessLaunchIssued", usage) < h.event_seq("AttemptSettling", api));
    for attempt in [api, usage] {
        assert!(matches!(
            h.read(|v| v.validation_state(attempt)),
            ValidationState::Passed(_)
        ));
        assert!(h.read(|v| v.candidate_acceptance(attempt)).is_none());
    }
    // Pause and resume change only admission; Cancel is final.
    assert_eq!(h.control(&plan, "Pause"), "plan run PAUSED");
    assert_eq!(str_of(&h.plan_run(&plan), &["state"]), "PAUSED");
    assert_eq!(h.control(&plan, "Resume"), "plan run RUNNING");
    assert_eq!(h.control(&plan, "Cancel"), "plan run CANCELLED");
    assert_eq!(h.step_states(&plan), ["Review", "Review", "Cancelled"]);
    assert_eq!(
        h.control(&plan, "Resume"),
        "PlanRunCancelled: the run was cancelled"
    );
}

// D3: while one provider's executable digest is computed in the background, the runner
// leaves that step ready (it is not refused) and starts the other provider's step. Every
// turn and attempt records the executable it ran; a provider whose executable differs
// from its previous run keeps the change, which the Executables query reports.
#[test]
fn d3_a_step_waits_for_its_executable_digest_and_every_run_records_its_executable() {
    let mut h = H::new();
    let repository = h.ready();
    let (_, _, plan, _, _) = h.approved_kinds_plan(&repository);
    // Claude Code now names a fresh copy, whose digest nothing has computed yet.
    let copy = h.base.join("bin/claude");
    fs::copy(h.base.join("bin/agent"), &copy).unwrap();
    chmod(&copy, 0o755);
    h.s().claude.as_mut().unwrap().executable = copy.clone();
    h.s().background_checks = true;
    let before = h.read(|v| v.executable_record(ProviderKind::ClaudeCode)).0;
    h.ok3("RunPlan", object([("plan", plan.clone())]));
    h.s().tick().unwrap();
    assert_eq!(h.step_states(&plan), ["Ready", "Running", "Waiting"]);
    assert_eq!(h.step_field(&plan, 0, "provider"), text("ClaudeCode"));
    let states = h.run_until(&plan, |s| s[0] == "Review" && s[1] == "Review");
    assert_eq!(states[2], "Waiting");
    assert_eq!(begin_check(&copy), ExecutableCheck::Known);
    // The plan turn ran the shared fixture; each attempt ran its provider's executable.
    let agent = h.base.join("bin/agent").display().to_string();
    let turn = h.read(|v| v.run_executable(OpaqueId(4003))).unwrap();
    assert_eq!(turn.path, agent);
    let api = id(&h.step_field(&plan, 0, "attempt")).unwrap();
    let usage = id(&h.step_field(&plan, 1, "attempt")).unwrap();
    let claude = h.read(|v| v.run_executable(api)).unwrap();
    assert_eq!(
        (claude.provider, claude.path.as_str()),
        (
            ProviderKind::ClaudeCode,
            copy.display().to_string().as_str()
        )
    );
    assert_eq!(
        (claude.version.as_str(), claude.profiled),
        (CLAUDE_VERSION, true)
    );
    assert_eq!(claude.sha256, h.digest());
    let codex = h.read(|v| v.run_executable(usage)).unwrap();
    assert_eq!(
        (codex.provider, codex.path.as_str()),
        (ProviderKind::Codex, agent.as_str())
    );
    // Claude Code ran the shared fixture in a lead turn (4002), so its executable changed
    // between two runs: the change is recorded, not refused.
    let changed = h.read(|v| v.executable_record(ProviderKind::ClaudeCode));
    assert_eq!(changed.0.as_ref(), Some(&claude));
    let previous = before.expect("the lead turn recorded Claude Code's executable");
    assert_eq!(previous.path, agent);
    let change = changed.1.clone().expect("a recorded change");
    assert_eq!((&change.previous, &change.current), (&previous, &claude));
    let r = h.at(4, "Executables", object([]));
    assert_eq!(r.kind, "Response", "{:?}", r.payload);
    let rows = items(field(&r.payload, &["providers"]));
    assert_eq!(str_of(&rows[0], &["provider"]), "ClaudeCode");
    assert_eq!(str_of(&rows[0], &["source"]), "pinned");
    assert_eq!(str_of(&rows[0], &["state"]), "ready");
    assert_eq!(
        str_of(&rows[0], &["last_run", "path"]),
        copy.display().to_string()
    );
    assert_eq!(
        field(&rows[0], &["last_run", "profiled"]),
        &Json::Bool(true)
    );
    assert_eq!(str_of(&rows[0], &["change", "previous", "path"]), agent);
    assert_eq!(field(&rows[0], &["change", "event"]), &number(change.event));
    assert_eq!(str_of(&rows[1], &["last_run", "path"]), agent);
}

#[test]
fn m3_a_rejected_step_stops_only_what_depends_on_it_and_a_restart_pauses_the_run() {
    let mut h = H::new();
    let repository = h.ready();
    let (_, _, plan, _, _) = h.approved_kinds_plan(&repository);
    h.ok3("RunPlan", object([("plan", plan.clone())]));
    h.run_until(&plan, |s| s[0] == "Review" && s[1] == "Review");
    let decide = |h: &mut H, index: usize| {
        let attempt = h.step_field(&plan, index, "attempt");
        let candidate = h.step_field(&plan, index, "candidate");
        let r = h.at(
            2,
            "CandidateDecision",
            object([
                ("attempt", attempt),
                ("candidate", candidate),
                ("receipt", Json::Null),
                ("action", text("Reject")),
            ]),
        );
        assert_eq!(r.kind, "Response", "{:?}", r.payload);
    };
    // M4 (D2): the human's rejection is never repaired. The runner holds the step for the
    // human, with the reason; the other goes on.
    decide(&mut h, 0);
    assert_eq!(h.step_states(&plan), ["Failed", "Review", "Waiting"]);
    h.s().tick().unwrap();
    assert_eq!(h.step_states(&plan), ["NeedsHuman", "Review", "Waiting"]);
    assert_eq!(
        h.step_field(&plan, 0, "detail"),
        text("the human rejected the candidate")
    );
    assert_eq!(h.step_field(&plan, 0, "repairs"), number(0));
    assert_eq!(str_of(&h.plan_run(&plan), &["state"]), "RUNNING");
    // A helper that starts again pauses the run.
    h.open();
    let run = h.plan_run(&plan);
    assert_eq!(str_of(&run, &["control"]), "PAUSED");
    h.ready_again();
    assert_eq!(h.control(&plan, "Resume"), "plan run RUNNING");
    // With both independent steps stopped, nothing more can happen without the human.
    decide(&mut h, 1);
    h.s().tick().unwrap();
    assert_eq!(str_of(&h.plan_run(&plan), &["state"]), "HALTED");
    h.s().tick().unwrap();
    assert_eq!(h.s().workers.running(Work::Attempt), 0);
}

impl H {
    /// After a restart: admission opens again after a reviewed inspection.
    fn ready_again(&mut self) {
        let inspected = self.ok("RecoveryInspect", object([]));
        let digest = str_of(&inspected, &["digest"]).to_owned();
        self.ok("RecoveryStart", object([("digest", text(digest))]));
    }
}

// ---- M3: integration onto the plan branch ----

impl H {
    fn accept_step(&mut self, plan: &Json, index: usize) {
        let attempt = self.step_field(plan, index, "attempt");
        let candidate = self.step_field(plan, index, "candidate");
        let receipt = self.step_field(plan, index, "receipt");
        let r = self.at(
            2,
            "CandidateDecision",
            object([
                ("attempt", attempt),
                ("candidate", candidate),
                ("receipt", receipt),
                ("action", text("Accept")),
            ]),
        );
        assert_eq!(r.kind, "Response", "{:?}", r.payload);
    }

    fn git_out(&self, args: &[&str]) -> String {
        git(&self.repository, args).trim().to_owned()
    }
}

#[test]
fn m3_accepted_steps_go_onto_the_plan_branch_and_the_dependent_starts_from_it() {
    let mut h = H::new();
    let repository = h.ready();
    let (_, _, plan, _, _) = h.approved_kinds_plan(&repository);
    let developer_head = h.git_out(&["rev-parse", "HEAD"]);
    h.ok3("RunPlan", object([("plan", plan.clone())]));
    h.run_until(&plan, |s| s[0] == "Review" && s[1] == "Review");
    let branch = str_of(&h.plan_run(&plan), &["branch"]).to_owned();
    let base = str_of(&h.plan_run(&plan), &["base"]).to_owned();
    h.accept_step(&plan, 1);
    h.accept_step(&plan, 0);
    // Integrated in the order accepted; then the dependent starts from the branch.
    let states = h.run_until(&plan, |s| s[2] != "Waiting" && s[2] != "Ready");
    assert_eq!(&states[..2], ["Integrated", "Integrated"]);
    let first = h.step_field(&plan, 1, "commit");
    let second = h.step_field(&plan, 0, "commit");
    assert_eq!(
        h.git_out(&["rev-parse", &format!("{}^", str_of(&first, &[]))]),
        base
    );
    assert_eq!(
        h.git_out(&["rev-parse", &format!("{}^", str_of(&second, &[]))]),
        str_of(&first, &[])
    );
    let join = id(&h.step_field(&plan, 2, "attempt")).unwrap();
    assert_eq!(
        h.read(|v| v.worktree(join)).unwrap().base,
        str_of(&second, &[])
    );
    h.run_until(&plan, |s| s[2] == "Review");
    h.accept_step(&plan, 2);
    h.run_until(&plan, |s| s.iter().all(|s| s == "Integrated"));
    let run = h.plan_run(&plan);
    assert_eq!(str_of(&run, &["state"]), "DONE");
    let head = str_of(&run, &["head"]).to_owned();
    assert_eq!(
        h.git_out(&["rev-parse", &format!("refs/heads/{branch}")]),
        head
    );
    assert_eq!(
        h.git_out(&["log", "--format=%an <%ae>", &format!("{base}..{head}")]),
        "Jojo <jojo@localhost>\nJojo <jojo@localhost>\nJojo <jojo@localhost>"
    );
    let files = h.git_out(&["ls-tree", "--name-only", "-r", &head, "src"]);
    assert_eq!(
        files,
        "src/api.txt\nsrc/join.txt\nsrc/lib.rs\nsrc/usage.txt"
    );
    // The developer's checkout is untouched: same HEAD, nothing changed.
    assert_eq!(h.git_out(&["rev-parse", "HEAD"]), developer_head);
    assert_eq!(h.git_out(&["status", "--porcelain"]), "");
}

#[test]
fn m3_a_plan_branch_moved_by_someone_else_is_a_conflict_and_nothing_is_merged() {
    let mut h = H::new();
    let repository = h.ready();
    let (_, _, plan, _, _) = h.approved_kinds_plan(&repository);
    h.ok3("RunPlan", object([("plan", plan.clone())]));
    h.run_until(&plan, |s| s[0] == "Review" && s[1] == "Review");
    let branch = str_of(&h.plan_run(&plan), &["branch"]).to_owned();
    h.accept_step(&plan, 0);
    h.run_until(&plan, |s| s[0] == "Integrated");
    let integrated = h.git_out(&["rev-parse", &format!("refs/heads/{branch}")]);
    // Someone moves the plan branch outside Jojo.
    let tree = h.git_out(&["rev-parse", &format!("{integrated}^{{tree}}")]);
    let foreign = h.git_out(&["commit-tree", &tree, "-p", &integrated, "-m", "outside"]);
    h.git_out(&["update-ref", &format!("refs/heads/{branch}"), &foreign]);
    h.accept_step(&plan, 1);
    let states = h.run_until(&plan, |s| s[1] != "Accepted");
    assert_eq!(states, ["Integrated", "Conflict", "Waiting"]);
    let detail = h.step_field(&plan, 1, "detail");
    assert!(
        str_of(&detail, &[]).contains("the plan branch moved"),
        "{detail:?}"
    );
    assert_eq!(str_of(&h.plan_run(&plan), &["state"]), "HALTED");
    assert_eq!(
        h.git_out(&["rev-parse", &format!("refs/heads/{branch}")]),
        foreign
    );
}

// ---- M4: repair and review ----

impl H {
    /// A one-step plan the fixture lead drafts for `scenario`, every document approved,
    /// under a project whose repair limit and reviewer are given.
    fn approved_one_step(
        &mut self,
        repository: &str,
        scenario: &str,
        limit: u64,
        review: bool,
    ) -> Json {
        let Json::Object(mut fields) = self.project_payload(3001, 0, repository) else {
            unreachable!()
        };
        fields.truncate(fields.len() - 2);
        fields.push(("repair_limit".into(), number(limit)));
        fields.push(("review".into(), Json::Bool(review)));
        self.ok3("SaveProject", Json::Object(fields));
        self.approved_requirements(repository);
        let row = self.drafted_plan(repository, 4003, &format!("fixture: {scenario}"));
        let plan = field(&row, &["plan"]).clone();
        for part in ["adr", "validation", "plan"] {
            let subject = field(&plan, &[part]).clone();
            self.approve(&subject);
        }
        field(&plan, &["plan"]).clone()
    }

    fn step_attempt(&mut self, plan: &Json) -> OpaqueId {
        id(&self.step_field(plan, 0, "attempt")).unwrap()
    }
}

#[test]
fn m4_a_failed_validation_is_repaired_with_its_output_then_reviewed_by_the_other_provider() {
    let mut h = H::new();
    let repository = h.ready();
    let plan = h.approved_one_step(&repository, "lead-repair", 2, true);
    h.ok3("RunPlan", object([("plan", plan.clone())]));
    // The first attempt's check fails; a repair is decided, then started.
    h.run_until(&plan, |s| s[0] == "Repairing");
    let states = h.run_until(&plan, |s| s[0] == "Review");
    assert_eq!(states, ["Review"]);
    assert_eq!(h.step_field(&plan, 0, "repairs"), number(1));
    let repair = h.step_field(&plan, 0, "repair");
    assert_eq!(field(&repair, &["ordinal"]), &number(1));
    assert_eq!(field(&repair, &["limit"]), &number(2));
    // The repair attempt was told what failed, from the host's own records.
    let attempt = h.step_attempt(&plan);
    let record = h.read(|v| v.repair_of(attempt)).expect("a repair record");
    assert!(
        record.section.starts_with("\n# Repair 1 of 2\n"),
        "{}",
        record.section
    );
    assert!(
        record.section.contains("## Host validation failed\n"),
        "{}",
        record.section
    );
    assert!(record.section.contains("src/fix.txt"), "{}", record.section);
    assert!(
        record.failure.starts_with("host validation failed"),
        "{}",
        record.failure
    );
    assert!(matches!(
        h.read(|v| v.validation_state(record.failed)),
        ValidationState::Failed(_)
    ));
    // The step was Codex's; Claude Code reviewed it and approved. Acceptance stays human.
    let review = h.step_field(&plan, 0, "review");
    assert_eq!(str_of(&review, &["provider"]), "ClaudeCode");
    assert_eq!(str_of(&review, &["state"]), "APPROVED");
    assert_eq!(
        h.step_field(&plan, 0, "detail"),
        text("approved by ClaudeCode")
    );
    assert!(h.read(|v| v.candidate_acceptance(attempt)).is_none());
    let reviewer = id(field(&review, &["review"])).unwrap();
    let ran = h
        .read(|v| v.run_executable(reviewer))
        .expect("the review's executable");
    assert_eq!(ran.provider, ProviderKind::ClaudeCode);
    // Protocol 1.5: the repairs query lists both attempts, newest first.
    assert_eq!(h.refused3("Repairs", object([])), "UnknownType");
    let r = h.at(5, "Repairs", object([]));
    assert_eq!(r.kind, "Response", "{:?}", r.payload);
    let rows = items(field(&r.payload, &["attempts"])).to_vec();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(field(&rows[0], &["attempt"]), &identity(attempt));
    assert_eq!(str_of(&rows[0], &["state"]), "Review");
    assert_eq!(field(&rows[0], &["repair", "ordinal"]), &number(1));
    assert_eq!(str_of(&rows[0], &["review", "state"]), "APPROVED");
    assert_eq!(field(&rows[1], &["attempt"]), &identity(record.failed));
    assert_eq!(field(&rows[1], &["state"]), &Json::Null);
    assert_eq!(field(&rows[1], &["repair"]), &Json::Null);
    assert_eq!(field(&r.payload, &["complete"]), &Json::Bool(true));
    h.accept_step(&plan, 0);
    h.run_until(&plan, |s| s[0] == "Integrated");
    let run = h.plan_run(&plan);
    assert_eq!(str_of(&run, &["state"]), "DONE");
    let head = str_of(&run, &["head"]).to_owned();
    assert_eq!(
        h.git_out(&["show", &format!("{head}:src/fix.txt")]),
        "repaired by fixture"
    );
}

#[test]
fn m4_a_step_that_keeps_failing_is_held_for_the_human_after_two_repairs() {
    let mut h = H::new();
    let repository = h.ready();
    let plan = h.approved_one_step(&repository, "lead-never", 2, true);
    h.ok3("RunPlan", object([("plan", plan.clone())]));
    let states = h.run_until(&plan, |s| s[0] == "NeedsHuman");
    assert_eq!(states, ["NeedsHuman"]);
    assert_eq!(
        h.step_field(&plan, 0, "detail"),
        text("host validation failed; 2 of 2 repairs used")
    );
    assert_eq!(h.step_field(&plan, 0, "repairs"), number(2));
    assert_eq!(str_of(&h.plan_run(&plan), &["state"]), "HALTED");
    // Three attempts in all: the first and two repairs; nothing starts again.
    let last = h.step_attempt(&plan);
    let second = h.read(|v| v.repair_of(last)).unwrap();
    assert_eq!((second.ordinal, second.limit), (2, 2));
    let first = h.read(|v| v.repair_of(second.failed)).unwrap();
    assert_eq!(first.ordinal, 1);
    assert!(h.read(|v| v.repair_of(first.failed)).is_none());
    h.s().tick().unwrap();
    assert_eq!(h.s().workers.running(Work::Attempt), 0);
    assert_eq!(h.step_states(&plan), ["NeedsHuman"]);
}

#[test]
fn m4_a_rejecting_review_sends_its_notes_to_the_repair() {
    let mut h = H::new();
    let repository = h.ready();
    let plan = h.approved_one_step(&repository, "lead-review", 2, true);
    h.ok3("RunPlan", object([("plan", plan.clone())]));
    h.run_until(&plan, |s| s[0] == "Review");
    // Claude Code did the step; Codex rejected the first candidate and approved the repair.
    assert_eq!(h.step_field(&plan, 0, "repairs"), number(1));
    let attempt = h.step_attempt(&plan);
    let record = h.read(|v| v.repair_of(attempt)).unwrap();
    assert!(
        record
            .section
            .contains("The reviewer rejected it; the notes are your fix list"),
        "{}",
        record.section
    );
    assert!(
        record.section.contains("write repaired by fixture"),
        "{}",
        record.section
    );
    let review = h.step_field(&plan, 0, "review");
    assert_eq!(str_of(&review, &["provider"]), "Codex");
    assert_eq!(str_of(&review, &["state"]), "APPROVED");
}

#[test]
fn m4_with_repair_and_review_switched_off_a_failure_is_held_at_once_and_nothing_reviews() {
    let mut h = H::new();
    let repository = h.ready();
    let plan = h.approved_one_step(&repository, "lead-never", 0, false);
    h.ok3("RunPlan", object([("plan", plan.clone())]));
    h.run_until(&plan, |s| s[0] == "NeedsHuman");
    assert_eq!(
        h.step_field(&plan, 0, "detail"),
        text("host validation failed; automatic repair is off for this project")
    );
    assert_eq!(h.step_field(&plan, 0, "repairs"), number(0));
    let run = h.plan_run(&plan);
    assert_eq!(field(&run, &["repair_limit"]), &number(0));
    assert_eq!(field(&run, &["review_enabled"]), &Json::Bool(false));
}

#[test]
fn m4_without_the_reviewer_a_passed_step_goes_straight_to_the_human() {
    let mut h = H::new();
    let repository = h.ready();
    let plan = h.approved_one_step(&repository, "lead-review", 2, false);
    h.ok3("RunPlan", object([("plan", plan.clone())]));
    assert_eq!(h.run_until(&plan, |s| s[0] == "Review"), ["Review"]);
    assert_eq!(h.step_field(&plan, 0, "review"), Json::Null);
    assert_eq!(h.step_field(&plan, 0, "repairs"), number(0));
}

#[test]
fn m4_a_review_that_cannot_start_holds_the_step_for_the_human() {
    let mut h = H::new();
    let repository = h.ready();
    let plan = h.approved_one_step(&repository, "lead-repair", 2, true);
    // The step is Codex's, so Claude Code would review it; it is not configured now.
    h.s().claude = None;
    h.ok3("RunPlan", object([("plan", plan.clone())]));
    h.run_until(&plan, |s| s[0] == "NeedsHuman");
    let detail = h.step_field(&plan, 0, "detail");
    assert!(
        str_of(&detail, &[]).starts_with("the review could not start: NoProductionAdapter"),
        "{detail:?}"
    );
    assert_eq!(h.step_field(&plan, 0, "review"), Json::Null);
    assert!(h.read(|v| v.unfinished_review()).is_none());
}

// ---- Protocol 1.6: user-facing data ----

impl H {
    fn ok6(&mut self, kind: &str, payload: Json) -> Json {
        let r = self.at(6, kind, payload);
        assert_eq!(r.kind, "Response", "{kind}: {:?}", r.payload);
        r.payload
    }

    /// Discovery to a drafted requirements document, not approved.
    fn approved_requirements_draft(&mut self, repository: &str) -> Json {
        let chat = self.chat(0);
        for question in items(field(&items(field(&chat, &["turns"]))[0], &["questions"])).to_vec() {
            self.ok(
                "AnswerQuestion",
                object([
                    ("question", field(&question, &["question"]).clone()),
                    ("revision", field(&question, &["revision"]).clone()),
                    ("action", text("Answer")),
                    ("answer", text("As simple as possible.")),
                ]),
            );
        }
        let chat = self.turn(4002, repository, "ClaudeCode", "");
        field(&chat, &["prd", "document"]).clone()
    }

    fn documents(&mut self) -> Json {
        self.ok6("Documents", object([("before", Json::Null)]))
    }

    fn attempts(&mut self) -> Vec<Json> {
        let page = self.ok6("Attempts", object([("before", Json::Null)]));
        assert_eq!(field(&page, &["next"]), &Json::Null);
        items(field(&page, &["attempts"])).to_vec()
    }

    /// The approval state of each listed document of `kind`.
    fn approvals(&mut self, kind: &str) -> Vec<String> {
        let documents = self.documents();
        items(field(&documents, &["documents"]))
            .iter()
            .filter(|d| str_of(d, &["kind"]) == kind)
            .map(|d| str_of(d, &["approval"]).to_owned())
            .collect()
    }

    /// Every event, read at `minor` in pages of eight.
    fn all_events(&mut self, minor: u64) -> Vec<Json> {
        let mut out: Vec<Json> = Vec::new();
        loop {
            let after = out.last().map_or(0, |e| uint(field(e, &["seq"])).unwrap());
            let r = self.at(
                minor,
                "Events",
                object([("after", number(after)), ("limit", number(8))]),
            );
            assert_eq!(r.kind, "Response", "{:?}", r.payload);
            let page = items(&r.payload).to_vec();
            if page.is_empty() {
                return out;
            }
            out.extend(page);
        }
    }
}

#[test]
fn s2_documents_carry_names_and_approval_states_and_count_what_waits() {
    let mut h = H::new();
    let repository = h.ready();
    h_save_project(&mut h, &repository);
    // Before a requirements document exists, a feature is named by its first message.
    h.turn(
        4001,
        &repository,
        "Codex",
        "slugify should be usable from the shell.\nfixture: lead",
    );
    let index = h.ok6("ChatIndex", object([]));
    let feature = items(field(&index, &["features"]))[0].clone();
    assert_eq!(
        str_of(&feature, &["name"]),
        "slugify should be usable from the shell."
    );
    // A 1.2 frame is answered as before: no name.
    let old = h.ok("ChatIndex", object([]));
    assert!(get(&items(field(&old, &["features"]))[0], "name").is_err());
    assert_eq!(field(&h.documents(), &["pending"]), &number(0));

    // The drafted requirements document waits for the human and names the feature.
    let prd = h.approved_requirements_draft(&repository);
    let documents = h.documents();
    let rows = items(field(&documents, &["documents"])).to_vec();
    assert_eq!(rows.len(), 1, "{documents:?}");
    let title = str_of(&rows[0], &["title"]).to_owned();
    assert!(!title.is_empty());
    assert_eq!(field(&rows[0], &["document"]), &prd);
    assert_eq!(str_of(&rows[0], &["kind"]), "Prd");
    assert_eq!(str_of(&rows[0], &["approval"]), "Pending");
    assert_eq!(str_of(&rows[0], &["author"]), "Model");
    assert_eq!(str_of(&rows[0], &["feature_name"]), title);
    assert_eq!(field(&rows[0], &["stale"]), &Json::Bool(false));
    assert!(uint(field(&rows[0], &["created_at_unix_ms"])).unwrap() > 0);
    assert_eq!(field(&rows[0], &["decided_at_unix_ms"]), &Json::Null);
    assert_eq!(field(&documents, &["pending"]), &number(1));
    let index = h.ok6("ChatIndex", object([]));
    assert_eq!(
        str_of(&items(field(&index, &["features"]))[0], &["name"]),
        title
    );

    h.approve(&prd);
    assert_eq!(h.approvals("Prd"), ["Approved"]);
    let documents = h.documents();
    assert_eq!(field(&documents, &["pending"]), &number(0));
    let row = &items(field(&documents, &["documents"]))[0];
    assert!(uint(field(row, &["decided_at_unix_ms"])).unwrap() > 0);

    // The ADR waits first; the two plans exist only once it is approved, then both wait.
    let row = h.drafted_plan(&repository, 4003, "fixture: lead-kinds");
    let plan = field(&row, &["plan"]).clone();
    assert_eq!(h.approvals("Adr"), ["Pending"]);
    assert_eq!(field(&h.documents(), &["pending"]), &number(1));
    h.approve(&field(&plan, &["adr"]).clone());
    assert_eq!(h.approvals("ValidationPlan"), ["Pending"]);
    assert_eq!(h.approvals("ExecutionPlan"), ["Pending"]);
    let documents = h.documents();
    assert_eq!(field(&documents, &["pending"]), &number(2));
    for d in items(field(&documents, &["documents"])) {
        assert_eq!(str_of(d, &["feature_name"]), title, "{d:?}");
    }
    // Newest first; a document's own title only for requirements.
    let kinds: Vec<_> = items(field(&documents, &["documents"]))
        .iter()
        .map(|d| str_of(d, &["kind"]).to_owned())
        .collect();
    assert_eq!(kinds.last().unwrap(), "Prd");
    assert_eq!(kinds.len(), 4);
}

#[test]
fn s2_attempts_report_live_state_names_and_times_and_events_their_actor() {
    let mut h = H::new();
    let repository = h.ready();
    let (_, _, plan, _, _) = h.approved_kinds_plan(&repository);
    let title = {
        let documents = h.documents();
        let rows = items(field(&documents, &["documents"])).to_vec();
        str_of(rows.last().unwrap(), &["title"]).to_owned()
    };
    h.ok3("RunPlan", object([("plan", plan.clone())]));
    h.s().tick().unwrap();
    // Both steps wait two seconds in the fixture: each is seen running.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let running = loop {
        let rows = h.attempts();
        if rows.len() == 2 && rows.iter().all(|r| str_of(r, &["state"]) == "Running") {
            break rows;
        }
        assert!(std::time::Instant::now() < deadline, "{rows:?}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let mut keys: Vec<_> = running
        .iter()
        .map(|r| {
            assert_eq!(str_of(r, &["feature_name"]), title);
            assert_eq!(field(r, &["plan"]), &plan);
            assert!(!str_of(r, &["title"]).is_empty());
            assert!(uint(field(r, &["started_at_unix_ms"])).unwrap() > 0);
            assert_eq!(field(r, &["ended_at_unix_ms"]), &Json::Null);
            (
                str_of(r, &["key"]).to_owned(),
                str_of(r, &["provider"]).to_owned(),
            )
        })
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            ("api".to_owned(), "ClaudeCode".to_owned()),
            ("usage".to_owned(), "Codex".to_owned())
        ]
    );
    // Settled and validated: succeeded, and both candidates wait for the human.
    h.run_until(&plan, |s| s[0] == "Review" && s[1] == "Review");
    for r in h.attempts() {
        assert_eq!(str_of(&r, &["state"]), "Succeeded");
        let started = uint(field(&r, &["started_at_unix_ms"])).unwrap();
        assert!(uint(field(&r, &["ended_at_unix_ms"])).unwrap() >= started);
    }
    let documents = h.documents();
    let candidates = items(field(&documents, &["candidates"])).to_vec();
    assert_eq!(candidates.len(), 2, "{documents:?}");
    assert_eq!(field(&documents, &["pending"]), &number(2));
    for c in &candidates {
        assert_eq!(str_of(c, &["feature_name"]), title);
        assert!(uint(field(c, &["validated_at_unix_ms"])).unwrap() > 0);
    }

    // Who caused each event: the core, the human through the app, each provider.
    let events = h.all_events(6);
    let actor = |kind: &str| -> Vec<String> {
        let mut found: Vec<String> = events
            .iter()
            .filter(|e| str_of(e, &["type"]) == kind)
            .map(|e| str_of(e, &["actor"]).to_owned())
            .collect();
        found.dedup();
        found
    };
    assert_eq!(actor("EngineStarted"), ["Core"]);
    assert_eq!(actor("PlanRunStarted"), ["Human"]);
    assert_eq!(actor("HumanDecision"), ["Human"]);
    assert_eq!(actor("SchedulerReserved"), ["Core"]);
    let mut providers = actor("ProviderOutput");
    providers.sort();
    providers.dedup();
    assert_eq!(providers, ["ClaudeCode", "Codex"]);
    let mut leads = actor("LeadTurnFinished");
    leads.sort();
    leads.dedup();
    assert_eq!(leads, ["ClaudeCode", "Codex"]);
    assert!(
        events
            .iter()
            .all(|e| uint(field(e, &["at_unix_ms"])).unwrap() > 0)
    );
    // A 1.1 frame is answered as before: no time, no actor.
    let old = h.all_events(1);
    assert_eq!(old.len(), events.len());
    assert!(get(&old[0], "actor").is_err() && get(&old[0], "at_unix_ms").is_err());
}

// ---- Protocol 1.7: the Dashboard's data ----

impl H {
    fn ok7(&mut self, kind: &str, payload: Json) -> Json {
        let r = self.at(7, kind, payload);
        assert_eq!(r.kind, "Response", "{kind}: {:?}", r.payload);
        r.payload
    }

    fn features(&mut self) -> Vec<Json> {
        let page = self.ok7("Features", object([("before", Json::Null)]));
        assert_eq!(field(&page, &["next"]), &Json::Null);
        items(field(&page, &["features"])).to_vec()
    }

    /// The state and the word of one stage of the only feature.
    fn stage(&mut self, name: &str) -> (String, String) {
        let features = self.features();
        assert_eq!(features.len(), 1, "{features:?}");
        let stage = field(&features[0], &["stages", name]);
        let label = match field(stage, &["label"]) {
            Json::Null => String::new(),
            l => str_of(l, &[]).to_owned(),
        };
        (str_of(stage, &["state"]).to_owned(), label)
    }
}

#[test]
fn s3_a_feature_moves_through_its_stages_and_events_and_attempts_say_what_happened() {
    let mut h = H::new();
    let repository = h.ready();
    h_save_project(&mut h, &repository);
    assert!(h.features().is_empty());
    let r = h.at(6, "Features", object([("before", Json::Null)]));
    assert_eq!(field(&r.payload, &["code"]), &text("UnknownType"));

    h.turn(
        4001,
        &repository,
        "Codex",
        "slugify should be usable from the shell.\nfixture: lead",
    );
    let feature = h.features()[0].clone();
    assert_eq!(field(&feature, &["number"]), &number(1));
    assert_eq!(
        str_of(&feature, &["name"]),
        "slugify should be usable from the shell."
    );
    assert_eq!(str_of(&feature, &["project_name"]), "Sample");
    assert!(!str_of(&feature, &["repository_name"]).is_empty());
    assert_ne!(h.stage("discovery").0, "Off");
    assert_eq!(h.stage("specification").0, "Off");
    assert_eq!(h.stage("execution").0, "Off");

    // The drafted requirements document waits for the human.
    let prd = h.approved_requirements_draft(&repository);
    assert_eq!(h.stage("discovery").0, "Done");
    assert_eq!(
        h.stage("specification"),
        ("NeedsYou".into(), "PRD rev 1".into())
    );
    assert_eq!(field(&h.features()[0], &["awaiting"]), &number(1));
    h.approve(&prd);
    assert_eq!(h.stage("specification").0, "Done");

    // The ADR waits first, then the two plans; approved, planning is done.
    let row = h.drafted_plan(&repository, 4003, "fixture: lead-kinds");
    let plan = field(&row, &["plan"]).clone();
    assert_eq!(h.stage("planning"), ("NeedsYou".into(), "ADR rev 1".into()));
    h.approve(&field(&plan, &["adr"]).clone());
    h.approve(&field(&plan, &["validation"]).clone());
    let plan_id = field(&plan, &["plan"]).clone();
    h.approve(&plan_id);
    assert_eq!(h.stage("planning").0, "Done");
    assert_eq!(field(&h.features()[0], &["awaiting"]), &number(0));

    // Two steps run at once; the third waits for them and blocks nothing.
    h.ok3("RunPlan", object([("plan", plan_id.clone())]));
    h.s().tick().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let (state, label) = h.stage("execution");
        if state == "Running" && label == "2 running" {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{state} {label}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(field(&h.features()[0], &["running"]), &number(2));

    h.run_until(&plan_id, |s| s[0] == "Review" && s[1] == "Review");
    assert_eq!(
        h.stage("execution"),
        ("NeedsYou".into(), "2 to review".into())
    );
    assert_eq!(
        h.stage("verification"),
        ("NeedsYou".into(), "2 to accept".into())
    );
    let feature = h.features()[0].clone();
    assert_eq!(field(&feature, &["awaiting"]), &number(2));
    assert_eq!(field(&feature, &["blocked"]), &number(0));
    assert_eq!(field(&feature, &["done"]), &Json::Bool(false));

    // At 1.7 an attempt carries its place, repository, validation and newest record.
    let attempts = items(field(
        &h.ok7("Attempts", object([("before", Json::Null)])),
        &["attempts"],
    ))
    .to_vec();
    assert_eq!(attempts.len(), 2);
    for a in &attempts {
        assert_eq!(field(a, &["ordinal"]), &number(1));
        assert_eq!(str_of(a, &["project_name"]), "Sample");
        assert!(!str_of(a, &["repository_name"]).is_empty());
        assert_eq!(str_of(a, &["validation", "state"]), "Passed");
        assert!(uint(field(a, &["validation", "at_unix_ms"])).unwrap() > 0);
        assert_eq!(field(a, &["validation", "decision"]), &Json::Null);
        assert!(!str_of(a, &["activity", "text"]).is_empty());
    }
    // A 1.6 frame is answered as before.
    assert!(get(&h.attempts()[0], "ordinal").is_err());

    h.accept_step(&plan_id, 0);
    assert_eq!(
        h.stage("verification"),
        ("NeedsYou".into(), "1 to accept".into())
    );
    let accepted = items(field(
        &h.ok7("Attempts", object([("before", Json::Null)])),
        &["attempts"],
    ))
    .iter()
    .filter(|a| field(a, &["validation", "decision"]) == &text("Accepted"))
    .count();
    assert_eq!(accepted, 1);

    // Every event has a sentence at 1.7, with the names a person knows.
    let events = h.all_events(7);
    let said = |kind: &str| -> Vec<String> {
        events
            .iter()
            .filter(|e| str_of(e, &["type"]) == kind)
            .map(|e| str_of(e, &["summary"]).to_owned())
            .collect()
    };
    assert!(events.iter().all(|e| !str_of(e, &["summary"]).is_empty()));
    assert_eq!(said("EngineStarted")[0], "Session 1 started");
    assert!(
        said("HumanDecision")
            .iter()
            .any(|s| s.ends_with("rev 1 approved")),
        "{:?}",
        said("HumanDecision")
    );
    let title = str_of(&h.features()[0], &["name"]).to_owned();
    assert_eq!(
        said("PlanRunStarted"),
        [format!("Plan run of {title} started")]
    );
    let mut launches = said("ProcessLaunchIssued");
    launches.sort();
    assert_eq!(
        launches,
        [
            "api · attempt 1 started on Claude Code",
            "usage · attempt 1 started on Codex"
        ]
    );
    assert!(
        said("ValidationEnded")
            .iter()
            .all(|s| s.ends_with("passed"))
    );
    assert!(said("CandidateAccepted")[0].ends_with("attempt 1 accepted"));
    // A 1.6 frame is answered as before: no sentence.
    assert!(get(&h.all_events(6)[0], "summary").is_err());
}

#[test]
fn s3_an_open_blocking_question_blocks_the_feature_and_names_it() {
    let mut h = H::new();
    let repository = h.ready();
    h_save_project(&mut h, &repository);
    h.turn(
        4001,
        &repository,
        "Codex",
        "slugify should be usable from the shell.\nfixture: lead",
    );
    let chat = h.chat(0);
    let questions = items(field(&items(field(&chat, &["turns"]))[0], &["questions"])).to_vec();
    let question = questions[0].clone();
    h.ok(
        "AnswerQuestion",
        object([
            ("question", field(&question, &["question"]).clone()),
            ("revision", field(&question, &["revision"]).clone()),
            ("action", text("Block")),
            ("answer", Json::Null),
        ]),
    );
    let feature = h.features()[0].clone();
    assert_eq!(
        str_of(&feature, &["stages", "discovery", "state"]),
        "NeedsYou"
    );
    assert!(str_of(&feature, &["stages", "discovery", "label"]).ends_with("open"));
    let blockers = items(field(&feature, &["blockers"]));
    assert!(!blockers.is_empty());
    assert_eq!(
        field(&feature, &["blocked"]),
        &number(blockers.len() as u64)
    );
    assert_eq!(str_of(&blockers[0], &["kind"]), "Question");
    assert!(!str_of(&blockers[0], &["reason"]).is_empty());
}

// ---- Protocol 1.8: turn times and assumptions in the feature chat ----

#[test]
fn s4_a_chat_at_1_8_dates_its_turns_and_lists_the_drafts_assumptions() {
    let mut h = H::new();
    let repository = h.ready();
    h_save_project(&mut h, &repository);
    h.turn(
        4001,
        &repository,
        "Codex",
        "slugify should be usable from the shell.\nfixture: lead",
    );
    h.approved_requirements_draft(&repository);
    let feature = field(&h.chat(0), &["feature"]).clone();
    let page = |h: &mut H, minor: u64| {
        let r = h.at(
            minor,
            "FeatureChat",
            object([("feature", feature.clone()), ("after", number(0))]),
        );
        assert_eq!(r.kind, "Response", "{:?}", r.payload);
        r.payload
    };
    let dated = page(&mut h, 8);
    let turns = items(field(&dated, &["turns"])).to_vec();
    assert_eq!(turns.len(), 2);
    let mut last_end = 0;
    for t in &turns {
        let started = uint(field(t, &["started_at_unix_ms"])).unwrap();
        let ended = uint(field(t, &["ended_at_unix_ms"])).unwrap();
        assert!(started > 0 && ended >= started && started >= last_end);
        last_end = ended;
    }
    let assumptions = items(field(&dated, &["assumptions"])).to_vec();
    assert!(!assumptions.is_empty(), "{dated:?}");
    for a in &assumptions {
        assert!(!str_of(a, &["id"]).is_empty() && !str_of(a, &["statement"]).is_empty());
        assert_eq!(str_of(a, &["state"]), "Current");
        assert_eq!(field(a, &["revision"]), &number(1));
    }
    // A 1.7 frame is answered as before.
    let old = page(&mut h, 7);
    assert!(get(&old, "assumptions").is_err());
    assert!(get(&items(field(&old, &["turns"]))[0], "started_at_unix_ms").is_err());
}

impl H {
    fn ok9(&mut self, kind: &str, payload: Json) -> Json {
        let r = self.at(9, kind, payload);
        assert_eq!(r.kind, "Response", "{kind}: {:?}", r.payload);
        r.payload
    }

    fn document(&mut self, subject: &Json) -> Json {
        self.ok9(
            "Document",
            object([
                ("document", subject.clone()),
                ("revision", Json::Null),
                ("offset", number(0)),
            ]),
        )
    }
}

#[test]
fn s4c_documents_plans_and_attempts_report_their_details_at_1_9() {
    let mut h = H::new();
    let repository = h.ready();
    h_save_project(&mut h, &repository);
    h.turn(
        4101,
        &repository,
        "Codex",
        "slugify should be usable from the shell.\nfixture: lead",
    );
    let prd = h.approved_requirements_draft(&repository);
    let r = h.at(
        8,
        "Document",
        object([
            ("document", prd.clone()),
            ("revision", Json::Null),
            ("offset", number(0)),
        ]),
    );
    assert_eq!(field(&r.payload, &["code"]), &text("UnknownType"));

    // A requirements document: its revisions, sections counted, criteria and text.
    let doc = h.document(&prd);
    assert_eq!(str_of(&doc, &["kind"]), "Prd");
    assert_eq!(str_of(&doc, &["approval"]), "Pending");
    assert_eq!(items(field(&doc, &["revisions"])).len(), 1);
    assert_eq!(
        field(&items(field(&doc, &["revisions"]))[0], &["decision"]),
        &Json::Null
    );
    assert!(uint(field(&doc, &["counts", "acceptance_criteria"])).unwrap() > 0);
    assert!(!items(field(&doc, &["criteria"])).is_empty());
    assert!(!str_of(&doc, &["text"]).is_empty());
    assert_eq!(field(&doc, &["complete"]), &Json::Bool(true));
    assert!(items(field(&doc, &["history"])).is_empty());
    h.approve(&prd);
    let doc = h.document(&prd);
    assert_eq!(str_of(&doc, &["approval"]), "Approved");
    let history = items(field(&doc, &["history"])).to_vec();
    assert_eq!(history.len(), 1);
    assert_eq!(str_of(&history[0], &["decision"]), "Approved");
    assert_eq!(field(&history[0], &["applies"]), &Json::Bool(true));
    assert_eq!(
        str_of(&items(field(&doc, &["revisions"]))[0], &["decision"]),
        "Approved"
    );

    // The plan drafted on it: the ADR names its requirements document upstream.
    let row = h.drafted_plan(&repository, 4103, "fixture: lead-kinds");
    let plan = field(&row, &["plan"]).clone();
    let adr = field(&plan, &["adr"]).clone();
    let doc = h.document(&adr);
    assert_eq!(str_of(&doc, &["kind"]), "Adr");
    assert_eq!(field(&doc, &["counts"]), &Json::Null);
    assert!(str_of(&doc, &["text"]).contains("Three small files."));
    assert_eq!(
        str_of(&items(field(&doc, &["upstream"]))[0], &["kind"]),
        "Prd"
    );
    assert!(!items(field(&h.document(&prd), &["downstream"])).is_empty());
    h.approve(&adr);
    h.approve(&field(&plan, &["validation"]).clone());
    let plan_id = field(&plan, &["plan"]).clone();

    // Before its approval every step waits for it; afterwards the two first steps could
    // start and the third waits for them.
    let detail = h.ok9("PlanDetail", object([("plan", plan_id.clone())]));
    let steps = items(field(&detail, &["steps"])).to_vec();
    assert_eq!(steps.len(), 3);
    assert!(
        steps
            .iter()
            .all(|s| field(s, &["eligible"]) == &Json::Bool(false))
    );
    h.approve(&plan_id);
    let detail = h.ok9("PlanDetail", object([("plan", plan_id.clone())]));
    assert_eq!(str_of(&detail, &["approval"]), "Approved");
    assert_eq!(field(&detail, &["run"]), &Json::Null);
    assert_eq!(field(&detail, &["checks", "cycle"]), &Json::Bool(false));
    assert!(items(field(&detail, &["checks", "conflicts"])).is_empty());
    let steps = items(field(&detail, &["steps"])).to_vec();
    let keys: Vec<&str> = steps.iter().map(|s| str_of(s, &["key"])).collect();
    assert_eq!(keys, ["api", "usage", "join"]);
    let waves: Vec<Json> = steps.iter().map(|s| field(s, &["wave"]).clone()).collect();
    assert_eq!(waves, [number(1), number(1), number(2)]);
    assert_eq!(str_of(&steps[0], &["kind"]), "backend");
    assert_eq!(
        items(field(&steps[0], &["write_scope"]))[0],
        text("src/api.txt")
    );
    assert_eq!(items(field(&steps[0], &["tests"]))[0], text("api"));
    assert_eq!(field(&steps[0], &["eligible"]), &Json::Bool(true));
    assert_eq!(field(&steps[2], &["eligible"]), &Json::Bool(false));
    assert_eq!(
        str_of(&items(field(&steps[2], &["blockers"]))[0], &["kind"]),
        "Dependency"
    );

    // The newest decisions over all documents: the plan's approval applies.
    let decisions = items(field(
        &h.ok9("Decisions", object([("before", Json::Null)])),
        &["decisions"],
    ))
    .to_vec();
    assert_eq!(decisions.len(), 4);
    assert_eq!(str_of(&decisions[0], &["kind"]), "ExecutionPlan");
    assert_eq!(field(&decisions[0], &["applies"]), &Json::Bool(true));

    // A run: the steps carry the run's states, and an attempt its details.
    h.ok3("RunPlan", object([("plan", plan_id.clone())]));
    h.run_until(&plan_id, |s| s[0] == "Review" && s[1] == "Review");
    let detail = h.ok9("PlanDetail", object([("plan", plan_id.clone())]));
    assert_eq!(str_of(&detail, &["run"]), "RUNNING");
    let first = items(field(&detail, &["steps"]))[0].clone();
    assert_eq!(str_of(&first, &["state"]), "Review");
    let attempts = items(field(&first, &["attempts"])).to_vec();
    assert_eq!(attempts.len(), 1);
    assert_eq!(str_of(&attempts[0], &["validation"]), "Passed");
    let attempt = field(&attempts[0], &["attempt"]).clone();
    let a = h.ok9("AttemptDetail", object([("attempt", attempt)]));
    assert_eq!(str_of(&a, &["key"]), "api");
    assert_eq!(field(&a, &["ordinal"]), &number(1));
    assert!(uint(field(&a, &["process", "pid"])).unwrap() > 0);
    assert!(!str_of(&a, &["worktree", "base"]).is_empty());
    assert!(!str_of(&a, &["candidate_head"]).is_empty());
    assert_eq!(items(field(&a, &["write_scope"]))[0], text("src/api.txt"));
    assert!(!items(field(&a, &["events"])).is_empty());
    assert!(
        items(field(&a, &["events"]))
            .iter()
            .all(|e| !str_of(e, &["summary"]).is_empty())
    );
    let runs = items(field(&a, &["validations"])).to_vec();
    assert_eq!(runs.len(), 1);
    assert_eq!(field(&runs[0], &["ordinal"]), &number(1));
    assert_eq!(field(&runs[0], &["passed"]), &Json::Bool(true));
    assert!(!str_of(&runs[0], &["validator"]).is_empty());
    let r = h.at(
        9,
        "AttemptDetail",
        object([("attempt", text("00000000000000000000000000000099"))]),
    );
    assert_eq!(r.payload, Json::Null);
}

// ---- M5: push and draft pull request of a finished plan ----

impl H {
    /// A bare repository as `origin`, holding `main`.
    fn origin(&mut self) -> PathBuf {
        let bare = self.base.join("remote.git");
        git(
            &self.base,
            &[
                "init",
                "--bare",
                "--template=",
                "--initial-branch=main",
                "remote.git",
            ],
        );
        let url = format!("file://{}", bare.display());
        git(&self.repository, &["remote", "add", "origin", &url]);
        git(&self.repository, &["push", &url, "main:refs/heads/main"]);
        bare
    }

    /// A one-step plan, run, accepted and on its plan branch.
    fn finished_plan(&mut self) -> Json {
        let repository = self.ready();
        let plan = self.approved_one_step(&repository, "lead-review", 2, false);
        self.ok3("RunPlan", object([("plan", plan.clone())]));
        self.run_until(&plan, |s| s[0] == "Review");
        self.accept_step(&plan, 0);
        self.run_until(&plan, |s| s[0] == "Integrated");
        assert_eq!(str_of(&self.plan_run(&plan), &["state"]), "DONE");
        plan
    }

    fn at10(&mut self, kind: &str, payload: Json) -> Envelope {
        self.at(10, kind, payload)
    }

    fn ok10(&mut self, kind: &str, payload: Json) -> Json {
        let r = self.at10(kind, payload);
        assert_eq!(r.kind, "Response", "{kind}: {:?}", r.payload);
        r.payload
    }

    fn refused10(&mut self, kind: &str, payload: Json) -> String {
        let r = self.at10(kind, payload);
        assert_eq!(r.kind, "Error", "{kind}: {:?}", r.payload);
        str_of(&r.payload, &["code"]).to_owned()
    }

    fn external(&mut self, plan: &Json) -> Json {
        self.ok10("ExternalActions", object([("plan", plan.clone())]))
    }

    /// The newest action of the plan.
    fn newest_action(&mut self, plan: &Json) -> Json {
        items(field(&self.external(plan), &["actions"]))[0].clone()
    }

    fn request_external(&mut self, plan: &Json, kind: &str) -> Envelope {
        self.at10(
            "RequestExternal",
            object([("plan", plan.clone()), ("kind", text(kind))]),
        )
    }

    fn decision(action: &Json, decision: &str) -> Json {
        object([
            ("action", field(action, &["action"]).clone()),
            ("digest", field(action, &["digest"]).clone()),
            ("decision", text(decision)),
        ])
    }

    /// Requests and authorizes the next action of `kind`, and waits for its worker.
    fn perform(&mut self, plan: &Json, kind: &str) -> Json {
        let r = self.request_external(plan, kind);
        assert_eq!(r.kind, "Response", "{:?}", r.payload);
        let action = self.newest_action(plan);
        assert_eq!(str_of(&action, &["state"]), "Requested");
        let r = self.ok10("AuthorizeExternal", Self::decision(&action, "Authorize"));
        assert_eq!(str_of(&r, &["dispatch"]), "started", "{r:?}");
        self.settle();
        self.newest_action(plan)
    }

    fn settle(&mut self) {
        until(|| self.s().workers.running(Work::External) == 0);
        assert!(!self.s().workers.failed(), "external worker failed");
    }

    /// The commit a branch of the bare remote names, or "" when it has no such branch.
    fn remote_head(bare: &std::path::Path, branch: &str) -> String {
        git(
            bare,
            &[
                "for-each-ref",
                "--format=%(objectname)",
                &format!("refs/heads/{branch}"),
            ],
        )
        .trim()
        .to_owned()
    }

    fn fake_gh(&self) -> PathBuf {
        let dir = self.base.join("home/fake-gh");
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn gh_calls(&self) -> usize {
        fs::read_to_string(self.fake_gh().join("calls")).map_or(0, |c| c.lines().count())
    }

    fn events_of(&mut self, kind: &str) -> usize {
        let mut reader = self.s().engine.reader().unwrap();
        reader
            .events_after(EventSeq(0), 10_000)
            .unwrap()
            .iter()
            .filter(|e| e.kind == kind)
            .count()
    }

    fn auth(&mut self, plan: &Json, tool: &str) -> (String, String) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let answer = self.external(plan);
            let state = str_of(&answer, &["auth", tool, "state"]).to_owned();
            if state != "Checking" || std::time::Instant::now() >= deadline {
                return (state, str_of(&answer, &["auth", tool, "detail"]).to_owned());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

#[test]
fn m5_a_finished_plan_is_pushed_then_opened_as_a_draft_each_after_its_own_authorization() {
    let mut h = H::new();
    let bare = h.origin();
    let plan = h.finished_plan();
    let run = h.plan_run(&plan);
    let (branch, head) = (
        str_of(&run, &["branch"]).to_owned(),
        str_of(&run, &["head"]).to_owned(),
    );
    // Protocol 1.10 only.
    assert_eq!(
        h.at(9, "ExternalActions", object([("plan", plan.clone())]))
            .payload,
        object([("code", text("UnknownType"))])
    );
    let answer = h.external(&plan);
    let runs = items(field(&answer, &["runs"])).to_vec();
    assert_eq!(runs.len(), 1);
    assert_eq!(field(&runs[0], &["can_push"]), &Json::Bool(true));
    assert_eq!(
        field(&runs[0], &["can_open_pull_request"]),
        &Json::Bool(false)
    );
    assert_eq!(
        str_of(&runs[0], &["pull_request_refusal"]),
        "the plan branch is not pushed yet"
    );
    assert!(items(field(&answer, &["actions"])).is_empty());
    assert_eq!(h.auth(&plan, "gh").0, "Ready");
    // A request performs nothing and waits in the Approval Inbox.
    let r = h.request_external(&plan, "Push");
    assert_eq!(r.kind, "Response", "{:?}", r.payload);
    let key = str_of(&r.payload, &["response"]).to_owned();
    assert_eq!(
        key,
        format!("external:push:{}:{head}:1", str_of(&plan, &[]))
    );
    assert!(
        h.refused10(
            "RequestExternal",
            object([("plan", plan.clone()), ("kind", text("Push"))])
        )
        .starts_with("ExternalRefused: a push of this plan waits")
    );
    assert_eq!(
        h.refused10(
            "RequestExternal",
            object([("plan", plan.clone()), ("kind", text("PullRequest"))])
        ),
        "ExternalRefused: the plan branch is not pushed yet"
    );
    let documents = h.ok10("Documents", object([("before", Json::Null)]));
    let waiting = items(field(&documents, &["external"])).to_vec();
    assert_eq!(waiting.len(), 1);
    assert_eq!(str_of(&waiting[0], &["action"]), key);
    assert_eq!(str_of(&waiting[0], &["kind"]), "Push");
    let older = h
        .at(9, "Documents", object([("before", Json::Null)]))
        .payload;
    assert!(
        get(&older, "external").is_err(),
        "a 1.9 answer is unchanged"
    );
    assert_eq!(
        uint(field(&documents, &["pending"])).unwrap(),
        uint(field(&older, &["pending"])).unwrap() + 1
    );
    let action = h.newest_action(&plan);
    assert_eq!(
        str_of(&action, &["url"]),
        format!("file://{}", bare.display())
    );
    assert_eq!(str_of(&action, &["branch"]), branch);
    assert_eq!(str_of(&action, &["commit"]), head);
    assert_eq!(
        H::remote_head(&bare, &branch),
        "",
        "requesting pushed nothing"
    );
    // The decision binds the exact payload.
    let mut stale = H::decision(&action, "Authorize");
    if let Json::Object(fields) = &mut stale {
        fields[1].1 = text("0".repeat(64));
    }
    assert!(
        h.refused10("AuthorizeExternal", stale)
            .starts_with("StaleExternal: ")
    );
    let decision = H::decision(&action, "Authorize");
    let frame = h.next + 1;
    let r = h.ok10("AuthorizeExternal", decision.clone());
    assert_eq!(str_of(&r, &["response"]), "Push authorized; issued once");
    assert_eq!(str_of(&r, &["dispatch"]), "started");
    h.settle();
    let pushed = h.newest_action(&plan);
    assert_eq!(str_of(&pushed, &["state"]), "Committed");
    assert_eq!(str_of(&pushed, &["outcome"]), "Committed");
    assert_eq!(str_of(&pushed, &["decision"]), "Authorized");
    let observations = items(field(&pushed, &["observations"])).to_vec();
    assert_eq!(observations.len(), 1);
    assert_eq!(str_of(&observations[0], &["observer"]), "git:push");
    assert_eq!(H::remote_head(&bare, &branch), head);
    // The same frame again answers from its receipt and sends nothing.
    let again = h.frame(frame, 10, "AuthorizeExternal", decision);
    assert_eq!(str_of(&again.payload, &["dispatch"]), "not repeated");
    h.settle();
    assert_eq!(
        items(field(&h.newest_action(&plan), &["observations"])).len(),
        1
    );
    assert_eq!(
        h.refused10(
            "RequestExternal",
            object([("plan", plan.clone()), ("kind", text("Push"))])
        ),
        "ExternalRefused: the plan branch is pushed"
    );
    // The draft pull request is its own request and its own authorization.
    let r = h.request_external(&plan, "PullRequest");
    assert_eq!(r.kind, "Response", "{:?}", r.payload);
    let request = h.newest_action(&plan);
    assert_eq!(str_of(&request, &["kind"]), "PullRequest");
    assert_eq!(str_of(&request, &["base"]), "main");
    assert_eq!(str_of(&request, &["push"]), key);
    assert!(!str_of(&request, &["title"]).is_empty());
    let body = str_of(&request, &["body"]).to_owned();
    assert!(body.contains("## Steps"), "{body}");
    assert!(body.contains(&head[..12]), "{body}");
    assert!(
        body.contains("after a separate human authorization"),
        "{body}"
    );
    assert_eq!(h.gh_calls(), 0);
    let r = h.ok10("AuthorizeExternal", H::decision(&request, "Authorize"));
    assert_eq!(str_of(&r, &["dispatch"]), "started");
    h.settle();
    let opened = h.newest_action(&plan);
    assert_eq!(str_of(&opened, &["state"]), "Committed");
    assert_eq!(field(&opened, &["number"]), &number(1));
    assert_eq!(
        str_of(&opened, &["pull_request_url"]),
        "https://github.example/fake/repo/pull/1"
    );
    assert_eq!(
        field(&opened, &["body"]),
        &Json::Null,
        "only a request lists its body"
    );
    assert_eq!(h.gh_calls(), 1);
    assert_eq!(
        fs::read_to_string(h.fake_gh().join("body-1")).unwrap(),
        body
    );
    let runs = items(field(&h.external(&plan), &["runs"])).to_vec();
    assert_eq!(
        field(&runs[0], &["can_open_pull_request"]),
        &Json::Bool(false)
    );
    assert_eq!(
        str_of(&runs[0], &["pull_request_refusal"]),
        "the draft pull request is open"
    );
    for (kind, n) in [
        ("ExternalActionRequested", 2),
        ("ExternalActionAuthorized", 2),
        ("ExternalActionIssued", 2),
        ("ExternalActionObserved", 2),
    ] {
        assert_eq!(h.events_of(kind), n, "{kind}");
    }
}

#[test]
fn m5_a_push_the_remote_refuses_is_rejected_and_a_new_request_may_be_declined() {
    let mut h = H::new();
    let bare = h.origin();
    let plan = h.finished_plan();
    let branch = str_of(&h.plan_run(&plan), &["branch"]).to_owned();
    // Someone else's branch of that name on the remote, not under the plan branch: never
    // forced, so the push is refused.
    let tree = git(&h.repository, &["rev-parse", "main^{tree}"])
        .trim()
        .to_owned();
    let main = git(
        &h.repository,
        &["commit-tree", &tree, "-p", "main", "-m", "someone else"],
    )
    .trim()
    .to_owned();
    let url = format!("file://{}", bare.display());
    git(
        &h.repository,
        &["push", &url, &format!("{main}:refs/heads/{branch}")],
    );
    let refused = h.perform(&plan, "Push");
    assert_eq!(str_of(&refused, &["state"]), "Rejected");
    assert!(
        str_of(&refused, &["detail"]).starts_with("the remote refused the push"),
        "{refused:?}"
    );
    assert_eq!(H::remote_head(&bare, &branch), main);
    assert_eq!(
        h.refused10(
            "InspectExternal",
            object([("action", field(&refused, &["action"]).clone())])
        ),
        "ExternalSettled: the outcome is known"
    );
    // A new request is a new decision; declining it sends nothing.
    let r = h.request_external(&plan, "Push");
    assert!(
        str_of(&r.payload, &["response"]).ends_with(":2"),
        "{:?}",
        r.payload
    );
    let request = h.newest_action(&plan);
    let r = h.ok10("AuthorizeExternal", H::decision(&request, "Decline"));
    assert_eq!(str_of(&r, &["response"]), "Push declined");
    assert_eq!(get(&r, "dispatch").ok(), None);
    assert_eq!(str_of(&h.newest_action(&plan), &["state"]), "Declined");
    assert!(
        h.refused10("AuthorizeExternal", H::decision(&request, "Authorize"))
            .starts_with("ExternalDecided: ")
    );
    let documents = h.ok10("Documents", object([("before", Json::Null)]));
    assert!(items(field(&documents, &["external"])).is_empty());
    assert_eq!(H::remote_head(&bare, &branch), main);
}

#[test]
fn m5_a_lost_outcome_is_held_across_a_restart_never_sent_again_and_resolved_by_the_human() {
    let mut h = H::new();
    let bare = h.origin();
    let plan = h.finished_plan();
    let (branch, head) = {
        let run = h.plan_run(&plan);
        (
            str_of(&run, &["branch"]).to_owned(),
            str_of(&run, &["head"]).to_owned(),
        )
    };
    h.s().external_fault = Some(ExternalFault::LoseOutcome);
    let lost = h.perform(&plan, "Push");
    assert_eq!(str_of(&lost, &["state"]), "Unknown");
    let observations = items(field(&lost, &["observations"])).to_vec();
    assert_eq!(
        str_of(&observations[0], &["observer"]),
        "fake-remote:lost-outcome"
    );
    assert_eq!(str_of(&observations[0], &["status"]), "Unknown");
    // The push did reach the remote. Remove it, so that any second push would show.
    assert_eq!(H::remote_head(&bare, &branch), head);
    git(
        &bare,
        &["update-ref", "-d", &format!("refs/heads/{branch}")],
    );
    // A helper that starts again keeps the outcome unknown and sends nothing.
    h.open();
    h.ready_again();
    for _ in 0..5 {
        h.s().tick().unwrap();
    }
    h.settle();
    assert_eq!(str_of(&h.newest_action(&plan), &["state"]), "Unknown");
    assert_eq!(
        H::remote_head(&bare, &branch),
        "",
        "an issued push is never sent again"
    );
    assert!(
        h.refused10(
            "RequestExternal",
            object([("plan", plan.clone()), ("kind", text("Push"))])
        )
        .starts_with("ExternalRefused: a push of this plan waits")
    );
    let key = field(&lost, &["action"]).clone();
    assert_eq!(
        h.read(|v| v.unresolved_external()),
        vec![str_of(&key, &[]).to_owned()]
    );
    // The human inspects first, then decides.
    assert!(
        h.refused10("ResolveExternal", object([("action", key.clone())]))
            .starts_with("InspectFirst: ")
    );
    let r = h.ok10("InspectExternal", object([("action", key.clone())]));
    assert_eq!(str_of(&r, &["inspection"]), "started");
    h.settle();
    let inspected = h.newest_action(&plan);
    assert_eq!(str_of(&inspected, &["state"]), "Unknown");
    assert_eq!(
        str_of(&inspected, &["detail"]),
        format!("the remote has no branch {branch}")
    );
    h.ok10("ResolveExternal", object([("action", key.clone())]));
    let resolved = h.newest_action(&plan);
    assert_eq!(str_of(&resolved, &["state"]), "Rejected");
    let last = items(field(&resolved, &["observations"]))
        .last()
        .unwrap()
        .clone();
    assert_eq!(str_of(&last, &["observer"]), "human:not-performed");
    assert!(h.read(|v| v.unresolved_external()).is_empty());
    // Only now may it be asked for again, as a new action.
    h.s().external_fault = None;
    let pushed = h.perform(&plan, "Push");
    assert!(str_of(&pushed, &["action"]).ends_with(":2"));
    assert_eq!(str_of(&pushed, &["state"]), "Committed");
    assert_eq!(H::remote_head(&bare, &branch), head);
}

#[test]
fn m5_an_action_issued_by_a_helper_that_stopped_is_unknown_until_inspected() {
    let mut h = H::new();
    let bare = h.origin();
    let plan = h.finished_plan();
    let branch = str_of(&h.plan_run(&plan), &["branch"]).to_owned();
    h.s().external_fault = Some(ExternalFault::StopBeforeDispatch);
    let issued = h.perform(&plan, "Push");
    assert_eq!(str_of(&issued, &["state"]), "Unknown");
    assert!(items(field(&issued, &["observations"])).is_empty());
    assert_eq!(H::remote_head(&bare, &branch), "");
    h.open();
    h.ready_again();
    h.s().tick().unwrap();
    h.settle();
    let key = field(&issued, &["action"]).clone();
    assert_eq!(str_of(&h.newest_action(&plan), &["state"]), "Unknown");
    assert_eq!(H::remote_head(&bare, &branch), "");
    h.ok10("InspectExternal", object([("action", key.clone())]));
    h.settle();
    h.ok10("ResolveExternal", object([("action", key)]));
    assert_eq!(str_of(&h.newest_action(&plan), &["state"]), "Rejected");
    assert_eq!(H::remote_head(&bare, &branch), "");
}

#[test]
fn m5_a_pull_request_whose_answer_is_lost_is_found_by_inspection_and_not_opened_twice() {
    let mut h = H::new();
    h.origin();
    let plan = h.finished_plan();
    assert_eq!(str_of(&h.perform(&plan, "Push"), &["state"]), "Committed");
    fs::write(h.fake_gh().join("mode"), "lost").unwrap();
    let lost = h.perform(&plan, "PullRequest");
    assert_eq!(str_of(&lost, &["state"]), "Unknown");
    assert_eq!(
        str_of(&items(field(&lost, &["observations"]))[0], &["observer"]),
        "gh:pr-create"
    );
    assert_eq!(h.gh_calls(), 1);
    fs::write(h.fake_gh().join("mode"), "").unwrap();
    h.ok10(
        "InspectExternal",
        object([("action", field(&lost, &["action"]).clone())]),
    );
    h.settle();
    let found = h.newest_action(&plan);
    assert_eq!(str_of(&found, &["state"]), "Committed");
    assert_eq!(field(&found, &["number"]), &number(1));
    assert_eq!(str_of(&found, &["detail"]), "pull request #1 found (draft)");
    assert_eq!(h.gh_calls(), 1, "gh pr create ran once");
}

#[test]
fn m5_without_gh_sign_in_the_reason_is_shown_and_a_refused_pull_request_is_rejected() {
    let mut h = H::new();
    h.origin();
    let plan = h.finished_plan();
    fs::write(h.fake_gh().join("mode"), "signed-out").unwrap();
    let (state, detail) = h.auth(&plan, "gh");
    assert_eq!(state, "Missing");
    assert!(detail.contains("gh auth login"), "{detail}");
    assert_eq!(str_of(&h.perform(&plan, "Push"), &["state"]), "Committed");
    let refused = h.perform(&plan, "PullRequest");
    assert_eq!(str_of(&refused, &["state"]), "Rejected");
    assert!(
        str_of(&refused, &["detail"]).contains("gh auth login"),
        "{refused:?}"
    );
    // A helper whose PATH has no gh says so, and requests nothing.
    let home = h.base.join("home");
    h.s().remote = Some(RemoteConfig::new(
        Discovery::with_search_path(h.base.join("disposable").display().to_string()),
        home,
    ));
    let (state, detail) = h.auth(&plan, "gh");
    assert_eq!(state, "Missing");
    assert!(
        detail.contains("`gh` is not on the login shell's PATH"),
        "{detail}"
    );
    assert!(
        h.refused10(
            "RequestExternal",
            object([("plan", plan.clone()), ("kind", text("PullRequest"))])
        )
        .starts_with("GhMissing: ")
    );
}

#[test]
fn m5_an_ssh_or_missing_origin_is_refused_before_anything_is_recorded() {
    let mut h = H::new();
    let plan = h.finished_plan();
    assert_eq!(
        h.refused10(
            "RequestExternal",
            object([("plan", plan.clone()), ("kind", text("Push"))])
        ),
        "ExternalRefused: the repository has no remote named origin"
    );
    git(
        &h.repository,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:someone/sample.git",
        ],
    );
    assert!(
        h.refused10(
            "RequestExternal",
            object([("plan", plan.clone()), ("kind", text("Push"))])
        )
        .starts_with("ExternalRefused: origin is git@github.com:someone/sample.git")
    );
    assert!(items(field(&h.external(&plan), &["actions"])).is_empty());
    assert_eq!(h.events_of("ExternalActionRequested"), 0);
}

// ---- Step 6 (protocol 1.11): project detail and events with a category and a project ----

impl H {
    fn project_detail(&mut self) -> Json {
        let r = self.at(
            11,
            "ProjectDetail",
            object([("project", identity(OpaqueId(3001)))]),
        );
        assert_eq!(r.kind, "Response", "{:?}", r.payload);
        r.payload
    }
}

fn texts_of(v: &Json) -> Vec<String> {
    items(v)
        .iter()
        .map(|t| string(t).unwrap().to_owned())
        .collect()
}

#[test]
fn s6_project_detail_reads_branch_head_worktrees_origin_and_what_is_missing() {
    let mut h = H::new();
    let _plan = h.finished_plan();
    let d = h.project_detail();
    assert_eq!(str_of(&d, &["name"]), "Sample");
    let repos = items(field(&d, &["repositories"])).to_vec();
    assert_eq!(repos.len(), 1);
    let repo = &repos[0];
    assert_eq!(str_of(repo, &["branch"]), "main");
    assert_eq!(str_of(repo, &["head"]), h.git_out(&["rev-parse", "HEAD"]));
    // The plan's one attempt owned a worktree; it is sealed, so none is active.
    assert!(uint(field(repo, &["worktrees"])).unwrap() >= 1);
    assert_eq!(field(repo, &["active_worktrees"]), &number(0));
    // Without origin nothing can be pushed: the repository is not healthy and the project
    // says what it lacks. The record has a test command and no CI command.
    assert_eq!(field(repo, &["origin"]), &Json::Null);
    assert_eq!(field(repo, &["healthy"]), &Json::Bool(false));
    let missing = |d: &Json| -> Vec<String> {
        items(field(d, &["missing"]))
            .iter()
            .map(|m| str_of(m, &["text"]).to_owned())
            .collect()
    };
    let name = str_of(repo, &["name"]).to_owned();
    assert_eq!(
        missing(&d),
        [
            "No CI command is recorded".to_owned(),
            format!("{name}: No origin remote: push and pull request are not possible"),
        ]
    );
    // A file origin is healthy.
    h.origin();
    let d = h.project_detail();
    let repo = items(field(&d, &["repositories"]))[0].clone();
    assert_eq!(str_of(&repo, &["origin_kind"]), "File");
    assert_eq!(field(&repo, &["healthy"]), &Json::Bool(true));
    assert_eq!(texts_of(field(&repo, &["issues"])), Vec::<String>::new());
    assert_eq!(missing(&d), ["No CI command is recorded"]);
    // A user name and password in the URL never leave the core.
    git(
        &h.repository,
        &[
            "remote",
            "set-url",
            "origin",
            "https://someone:secret@github.com/someone/sample.git",
        ],
    );
    let d = h.project_detail();
    let repo = items(field(&d, &["repositories"]))[0].clone();
    assert_eq!(
        str_of(&repo, &["origin"]),
        "https://github.com/someone/sample.git"
    );
    assert_eq!(str_of(&repo, &["origin_kind"]), "Https");
    assert!(!encode(&d).contains("secret"));
    // An SSH origin is refused by M5, and said so here.
    git(
        &h.repository,
        &[
            "remote",
            "set-url",
            "origin",
            "git@github.com:someone/sample.git",
        ],
    );
    let d = h.project_detail();
    let repo = items(field(&d, &["repositories"]))[0].clone();
    assert_eq!(str_of(&repo, &["origin_kind"]), "Ssh");
    assert_eq!(
        texts_of(field(&repo, &["issues"])),
        ["origin is an SSH URL: Jojo pushes over HTTPS only"]
    );
    // An unknown project is null; a frame before 1.11 does not know the query.
    let r = h.at(
        11,
        "ProjectDetail",
        object([("project", identity(OpaqueId(3999)))]),
    );
    assert_eq!(r.payload, Json::Null);
    let r = h.at(
        10,
        "ProjectDetail",
        object([("project", identity(OpaqueId(3001)))]),
    );
    assert_eq!(str_of(&r.payload, &["code"]), "UnknownType");
}

#[test]
fn s6_events_at_1_11_carry_a_category_and_the_project_they_concern() {
    let mut h = H::new();
    let _plan = h.finished_plan();
    let events = h.all_events(11);
    for e in &events {
        assert!(!str_of(e, &["category"]).is_empty(), "{e:?}");
        assert!(matches!(field(e, &["project"]), Json::Null | Json::Text(_)));
    }
    let first = |kind: &str| {
        events
            .iter()
            .find(|e| str_of(e, &["type"]) == kind)
            .unwrap_or_else(|| panic!("no {kind}"))
            .clone()
    };
    let started = first("EngineStarted");
    assert_eq!(str_of(&started, &["category"]), "Session");
    assert_eq!(field(&started, &["project"]), &Json::Null);
    for (kind, category) in [
        ("HumanDecision", "Approval"),
        ("PlanStepIntegrated", "Git"),
        ("CandidateAccepted", "Decision"),
        ("ProjectSaved", "Setup"),
    ] {
        let e = first(kind);
        assert_eq!(str_of(&e, &["category"]), category, "{kind}");
        assert_eq!(str_of(&e, &["project_name"]), "Sample", "{kind}");
        assert_eq!(field(&e, &["project"]), &identity(OpaqueId(3001)), "{kind}");
    }
    // Before 1.11 an event has neither.
    for e in h.all_events(10) {
        let Json::Object(fields) = e else {
            panic!("an event is an object")
        };
        assert!(
            !fields
                .iter()
                .any(|(k, _)| k == "category" || k == "project")
        );
    }
}

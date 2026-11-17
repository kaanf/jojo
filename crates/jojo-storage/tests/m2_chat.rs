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
use jojo_domain::provider::AuthMode;
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
        "CapacityFull: 0 attempts and 1 lead turn use all 1 units; a Start waits"
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
        "CapacityFull: 1 attempts and 1 lead turn use all 2 units; a Start waits"
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
            assert!(
                std::time::Instant::now() < deadline,
                "run stuck at {states:?}"
            );
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
// leaves that step ready (it is not refused) and starts the other provider's step.
#[test]
fn d3_a_step_waits_for_its_executable_digest_and_the_other_provider_starts() {
    let mut h = H::new();
    let repository = h.ready();
    let (_, _, plan, _, _) = h.approved_kinds_plan(&repository);
    // Claude Code now names a fresh copy, whose digest nothing has computed yet.
    let copy = h.base.join("bin/claude");
    fs::copy(h.base.join("bin/agent"), &copy).unwrap();
    chmod(&copy, 0o755);
    h.s().claude.as_mut().unwrap().executable = copy.clone();
    h.s().background_checks = true;
    h.ok3("RunPlan", object([("plan", plan.clone())]));
    h.s().tick().unwrap();
    assert_eq!(h.step_states(&plan), ["Ready", "Running", "Waiting"]);
    assert_eq!(h.step_field(&plan, 0, "provider"), text("ClaudeCode"));
    let states = h.run_until(&plan, |s| s[0] == "Review" && s[1] == "Review");
    assert_eq!(states[2], "Waiting");
    assert_eq!(begin_check(&copy), ExecutableCheck::Known);
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
    // D2 until M4: no retry. The rejected step waits for the human; the other goes on.
    decide(&mut h, 0);
    assert_eq!(h.step_states(&plan), ["Failed", "Review", "Waiting"]);
    assert_eq!(
        h.step_field(&plan, 0, "detail"),
        text("the human rejected the candidate")
    );
    assert_eq!(str_of(&h.plan_run(&plan), &["state"]), "RUNNING");
    // A helper that starts again pauses the run.
    h.open();
    let run = h.plan_run(&plan);
    assert_eq!(str_of(&run, &["control"]), "PAUSED");
    h.ready_again();
    assert_eq!(h.control(&plan, "Resume"), "plan run RUNNING");
    // With both independent steps stopped, nothing more can happen without the human.
    decide(&mut h, 1);
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

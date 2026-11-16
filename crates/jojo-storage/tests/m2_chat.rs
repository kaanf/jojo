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
    // Nothing else runs beside it: neither another turn nor a Start.
    let another = h.turn_payload(&repository, "Codex", "");
    assert_eq!(
        h.refused("ChatTurn", another),
        "CapacityOne: a run is active"
    );
    let start = object([
        ("prd", identity(OpaqueId(1))),
        ("adr", identity(OpaqueId(2))),
        ("plan", identity(OpaqueId(3))),
        ("validation", identity(OpaqueId(4))),
        ("step", identity(OpaqueId(5))),
    ]);
    assert!(h.refused("Start", start).starts_with("CapacityOne"));
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

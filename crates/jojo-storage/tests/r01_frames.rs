//! R-01: the protocol's answers, frame by frame, pinned before the request path was given
//! typed messages. Every frame here goes through the production `Service`; the expected
//! answers were recorded from the tree as it was before R-01 and must not change. Frames
//! with more than one fault pin which fault is reported, that is the order of the checks.
//! Nothing is launched: no provider is configured and no attempt exists.
mod common;
use common::attempts::{SERIAL, ps};
use common::{TempDir, start};
use jojo_domain::identity::OpaqueId;
use jojo_domain::ipc::*;
use jojo_domain::json::Json;
use jojo_domain::validation::{ScopePath, StepValidation, ValidationPlan};
use jojo_helper::service::*;
use jojo_test_support::{COMPLETE_PRD_JSON, dependency::node};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

struct H {
    _serial: std::sync::MutexGuard<'static, ()>,
    _dir: TempDir,
    root: String,
    service: Service,
    next: u128,
    seen: Vec<String>,
}

impl H {
    fn new() -> Self {
        let serial = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
        let dir = TempDir::new("r01");
        let base = fs::canonicalize(dir.path()).unwrap();
        fs::create_dir(base.join("disposable")).unwrap();
        fs::set_permissions(base.join("disposable"), fs::Permissions::from_mode(0o700)).unwrap();
        let config = ServiceConfig {
            slice: Some(SliceConfig {
                disposable_root: base.join("disposable"),
                worktree_root: base.join("owned"),
                codex: None,
            }),
            inspector: Some(ps()),
            launch_hooks: None,
        };
        let service = Service::with_config(Arc::new(start(&dir.db())), config).unwrap();
        Self {
            _serial: serial,
            root: base.display().to_string(),
            _dir: dir,
            service,
            next: 10,
            seen: Vec::new(),
        }
    }

    fn envelope(&mut self, minor: u64, kind: &str, payload: Json) -> Envelope {
        self.next += 1;
        Envelope {
            major: 1,
            minor,
            session: OpaqueId(5),
            epoch: self.service.engine.epoch().0,
            request: OpaqueId(self.next),
            revision: None,
            kind: kind.into(),
            payload,
        }
    }

    /// Sends the frame and records its answer under `label`.
    fn frame(&mut self, label: &str, e: Envelope) -> Envelope {
        let reply = self.service.request(&e);
        let body = encode(&reply.payload).replace(&self.root, "<root>");
        self.seen.push(format!(
            "{label} => {}.{} {} {body}",
            reply.major, reply.minor, reply.kind
        ));
        reply
    }

    fn at(&mut self, label: &str, minor: u64, kind: &str, payload: Json) -> Envelope {
        let e = self.envelope(minor, kind, payload);
        self.frame(label, e)
    }

    fn send(&mut self, label: &str, kind: &str, payload: Json) -> Envelope {
        self.at(label, 1, kind, payload)
    }
}

fn hex(n: u128) -> Json {
    identity(OpaqueId(n))
}

fn sha(c: char) -> Json {
    text(c.to_string().repeat(64))
}

fn decision(overrides: &[(&'static str, Json)]) -> Json {
    let mut fields = vec![
        ("subject", hex(701)),
        ("hash", text("h")),
        ("content_hash", text("c")),
        ("displayed_hash", text("d")),
        ("context_hash", text("x")),
        ("upstream", Json::List(vec![])),
        ("prior_decision", Json::Null),
        ("action", text("Approve")),
    ];
    for (key, value) in overrides {
        fields.iter_mut().find(|(k, _)| k == key).unwrap().1 = value.clone();
    }
    object(fields)
}

fn bundle(overrides: &[(&'static str, Json)]) -> Json {
    let validation = ValidationPlan {
        steps: vec![StepValidation {
            step: node(1),
            write_scope: vec![ScopePath::parse("src").unwrap()],
            baseline: vec![ScopePath::parse("tests").unwrap()],
            tests: vec!["unit::check".into()],
            command: vec![],
        }],
    };
    let mut fields = vec![
        ("feature", hex(700)),
        ("prd_id", hex(701)),
        ("adr_id", hex(702)),
        ("plan_id", hex(703)),
        ("validation_id", hex(704)),
        ("prd", text(COMPLETE_PRD_JSON)),
        ("adr", text("Keep the change inside src/.")),
        ("step", hex(1)),
        ("repository", hex(9)),
        ("base", text("0".repeat(40))),
        ("provider", text("Codex")),
        ("validation", text(validation.canonical())),
    ];
    for (key, value) in overrides {
        fields.iter_mut().find(|(k, _)| k == key).unwrap().1 = value.clone();
    }
    object(fields)
}

fn start_payload(overrides: &[(&'static str, Json)]) -> Json {
    let mut fields = vec![
        ("prd", hex(701)),
        ("adr", hex(702)),
        ("plan", hex(703)),
        ("validation", hex(704)),
        ("step", hex(1)),
    ];
    for (key, value) in overrides {
        fields.iter_mut().find(|(k, _)| k == key).unwrap().1 = value.clone();
    }
    object(fields)
}

fn record(h: &mut H) {
    let n = Json::Int;
    let bad = || text("x");

    // Before and around the handshake.
    h.send("snapshot before hello", "Snapshot", object([]));
    h.at("hello 1.0", 0, "Hello", object([]));
    // A frame newer than the helper (1.2 until protocol 1.2 existed, 1.3 since M2).
    h.at("hello 1.3", 3, "Hello", object([("junk", n(1))]));
    h.send(
        "close command while incompatible",
        "CancelClose",
        object([]),
    );
    h.send(
        "decision while incompatible",
        "HumanDecision",
        decision(&[]),
    );
    h.send("slice command while incompatible", "Start", object([]));
    h.send("hello 1.1", "Hello", object([]));
    let mut other = h.envelope(1, "Hello", object([]));
    other.session = OpaqueId(6);
    h.frame("hello from another session", other);
    let mut stale = h.envelope(1, "CloseStatus", object([]));
    stale.epoch += 1;
    h.frame("wrong epoch", stale);
    h.send("unknown type", "Nope", object([]));
    for kind in [
        "RecoveryInspect",
        "Start",
        "RegisterRepository",
        "Candidate",
        "ValidationReport",
    ] {
        h.at(&format!("1.0 frame names {kind}"), 0, kind, object([]));
    }

    // Protocol 1.0 queries: fields are read in order and extra fields are ignored.
    h.send("snapshot empty", "Snapshot", object([]));
    h.send(
        "snapshot two faults",
        "Snapshot",
        object([("section", n(1)), ("after", n(1))]),
    );
    h.send(
        "snapshot after, cursor, limit",
        "Snapshot",
        object([
            ("section", text("aggregates")),
            ("after", n(1)),
            ("cursor", bad()),
            ("limit", n(-1)),
        ]),
    );
    h.send(
        "snapshot cursor, limit",
        "Snapshot",
        object([
            ("section", text("aggregates")),
            ("after", text("")),
            ("cursor", bad()),
            ("limit", bad()),
        ]),
    );
    h.send(
        "snapshot limit range",
        "Snapshot",
        object([
            ("section", text("aggregates")),
            ("after", text("")),
            ("cursor", Json::Null),
            ("limit", n(5_000_000_000)),
        ]),
    );
    h.send(
        "snapshot unknown section",
        "Snapshot",
        object([
            ("section", text("nope")),
            ("after", text("")),
            ("cursor", Json::Null),
            ("limit", n(8)),
        ]),
    );
    h.send(
        "snapshot limit zero",
        "Snapshot",
        object([
            ("section", text("aggregates")),
            ("after", text("")),
            ("cursor", Json::Null),
            ("limit", n(0)),
        ]),
    );
    h.send(
        "snapshot with an extra field",
        "Snapshot",
        object([
            ("section", text("aggregates")),
            ("after", text("zzzz")),
            ("cursor", Json::Null),
            ("limit", n(8)),
            ("extra", n(1)),
        ]),
    );
    h.send(
        "events two faults",
        "Events",
        object([("after", bad()), ("limit", bad())]),
    );
    h.send(
        "events limit range",
        "Events",
        object([("after", n(0)), ("limit", n(5_000_000_000))]),
    );
    h.send(
        "events negative",
        "Events",
        object([("after", n(-1)), ("limit", n(1))]),
    );
    h.send(
        "events far ahead",
        "Events",
        object([("after", n(100_000)), ("limit", n(1))]),
    );
    h.send("event detail seq", "EventDetail", object([("seq", bad())]));
    h.send(
        "event detail offset",
        "EventDetail",
        object([("seq", n(1))]),
    );
    h.send(
        "log two faults",
        "Log",
        object([("attempt", text("zz")), ("sequence", bad())]),
    );
    h.send(
        "log sequence, offset",
        "Log",
        object([("attempt", hex(9)), ("sequence", bad()), ("offset", bad())]),
    );
    h.send(
        "log missing offset",
        "Log",
        object([("attempt", hex(9)), ("sequence", n(1))]),
    );
    h.send(
        "log unknown attempt",
        "Log",
        object([("attempt", hex(9)), ("sequence", n(1)), ("offset", n(0))]),
    );
    h.send("receipt empty", "Receipt", object([]));
    h.send("receipt bad", "Receipt", object([("command", bad())]));
    h.send(
        "receipt uppercase identity",
        "Receipt",
        object([("command", text("A".repeat(32)))]),
    );
    h.send("receipt unknown", "Receipt", object([("command", hex(9))]));
    h.send("shutdown receipt empty", "ShutdownReceipt", object([]));
    h.send(
        "shutdown receipt bad",
        "ShutdownReceipt",
        object([("ticket", n(5))]),
    );
    h.send(
        "shutdown receipt unknown",
        "ShutdownReceipt",
        object([("ticket", hex(9))]),
    );
    h.send(
        "recovery two faults",
        "Recovery",
        object([("offset", n(-1)), ("limit", n(0))]),
    );
    h.send(
        "recovery limit type",
        "Recovery",
        object([("offset", n(0)), ("limit", bad())]),
    );
    h.send(
        "recovery limit zero",
        "Recovery",
        object([("offset", n(0)), ("limit", n(0))]),
    );
    h.send(
        "recovery limit nine",
        "Recovery",
        object([("offset", n(0)), ("limit", n(9))]),
    );
    h.send(
        "recovery page",
        "Recovery",
        object([("offset", n(0)), ("limit", n(8)), ("extra", n(1))]),
    );
    h.at(
        "recovery page 1.0",
        0,
        "Recovery",
        object([("offset", n(0)), ("limit", n(8))]),
    );
    h.send("close status", "CloseStatus", object([("junk", n(1))]));
    h.send("review bundle empty", "ReviewBundle", object([]));
    h.send(
        "review bundle bad",
        "ReviewBundle",
        object([("subject", bad())]),
    );
    h.send(
        "review bundle unknown",
        "ReviewBundle",
        object([("subject", hex(9))]),
    );

    // HumanDecision: eight fields by count, then the fields in order.
    h.send("decision empty", "HumanDecision", object([]));
    h.send(
        "decision eight unknown fields",
        "HumanDecision",
        object([
            ("a", n(1)),
            ("b", n(1)),
            ("c", n(1)),
            ("d", n(1)),
            ("e", n(1)),
            ("f", n(1)),
            ("g", n(1)),
            ("h", n(1)),
        ]),
    );
    let mut reserved = h.envelope(1, "HumanDecision", decision(&[("subject", bad())]));
    reserved.request = OpaqueId(1 << 127 | 77);
    h.frame("decision reserved identity", reserved);
    h.send(
        "decision subject before revision",
        "HumanDecision",
        decision(&[("subject", bad())]),
    );
    h.send(
        "decision revision before hash",
        "HumanDecision",
        decision(&[("hash", n(1))]),
    );
    for (label, overrides) in [
        (
            "decision hash before the rest",
            vec![("hash", n(1)), ("content_hash", n(1)), ("action", n(1))],
        ),
        (
            "decision content hash",
            vec![("content_hash", n(1)), ("displayed_hash", n(1))],
        ),
        (
            "decision displayed hash",
            vec![("displayed_hash", n(1)), ("context_hash", n(1))],
        ),
        (
            "decision context hash",
            vec![("context_hash", n(1)), ("upstream", bad())],
        ),
        (
            "decision upstream type",
            vec![("upstream", bad()), ("prior_decision", bad())],
        ),
        (
            "decision upstream item",
            vec![("upstream", Json::List(vec![bad()])), ("action", n(1))],
        ),
        (
            "decision upstream length",
            vec![("upstream", Json::List(vec![n(1); 33]))],
        ),
        (
            "decision prior",
            vec![("prior_decision", bad()), ("action", text("Nope"))],
        ),
        ("decision action type", vec![("action", n(1))]),
        ("decision action name", vec![("action", text("Nope"))]),
        ("decision on nothing", vec![]),
        (
            "decision revoke on nothing",
            vec![("action", text("Revoke"))],
        ),
    ] {
        let mut e = h.envelope(1, "HumanDecision", decision(&overrides));
        e.revision = Some(1);
        h.frame(label, e);
    }

    // Close commands. PrepareClose itself is not sent: this helper stays open.
    h.send("cancel close empty", "CancelClose", object([]));
    h.send(
        "cancel close bad",
        "CancelClose",
        object([("ticket", bad())]),
    );
    h.send(
        "cancel close unknown",
        "CancelClose",
        object([("ticket", hex(9)), ("extra", n(1))]),
    );
    h.send("confirm close empty", "ConfirmClose", object([]));
    h.send(
        "confirm close unknown",
        "ConfirmClose",
        object([("ticket", hex(9))]),
    );
    h.send("cancel attempt empty", "CancelAttempt", object([]));
    h.send(
        "cancel attempt bad",
        "CancelAttempt",
        object([("attempt", n(1))]),
    );
    h.send(
        "cancel attempt unknown",
        "CancelAttempt",
        object([("attempt", hex(9))]),
    );
    let mut reserved = h.envelope(1, "CancelClose", object([]));
    reserved.request = OpaqueId(1 << 127 | 78);
    h.frame("close reserved identity", reserved);

    // Protocol 1.1 commands: exact field sets, then the fields in order.
    let mut reserved = h.envelope(1, "Start", object([]));
    reserved.request = OpaqueId(1 << 127 | 79);
    h.frame("slice reserved identity", reserved);
    h.send("register empty", "RegisterRepository", object([]));
    h.send(
        "register extra field",
        "RegisterRepository",
        object([("path", text("/tmp")), ("executable", text("/bin/sh"))]),
    );
    h.send(
        "register path type",
        "RegisterRepository",
        object([("path", n(1))]),
    );
    h.send(
        "register path with NUL",
        "RegisterRepository",
        object([("path", text("a\0b"))]),
    );
    h.send(
        "register long path",
        "RegisterRepository",
        object([("path", text("a".repeat(4097)))]),
    );
    h.send(
        "register outside",
        "RegisterRepository",
        object([("path", text("/tmp"))]),
    );
    h.send("import empty", "ImportBundle", object([]));
    for (label, overrides) in [
        (
            "import identities first",
            vec![("prd_id", bad()), ("adr_id", bad()), ("prd", text("{"))],
        ),
        ("import adr id", vec![("adr_id", bad()), ("plan_id", bad())]),
        (
            "import plan id",
            vec![("plan_id", bad()), ("validation_id", bad())],
        ),
        (
            "import validation id before prd",
            vec![("validation_id", bad()), ("prd", n(1))],
        ),
        (
            "import prd type before validation",
            vec![("prd", n(1)), ("validation", n(1))],
        ),
        (
            "import prd content before validation",
            vec![("prd", text("{")), ("validation", n(1)), ("feature", bad())],
        ),
        (
            "import validation type before provider",
            vec![("validation", n(1)), ("provider", n(1))],
        ),
        (
            "import validation content before provider",
            vec![("validation", text("{")), ("provider", text("Nope"))],
        ),
        (
            "import provider type before feature",
            vec![("provider", n(1)), ("feature", bad())],
        ),
        (
            "import provider name before feature",
            vec![("provider", text("Nope")), ("feature", bad())],
        ),
        (
            "import feature before adr",
            vec![("feature", bad()), ("adr", n(1))],
        ),
        (
            "import adr before step",
            vec![("adr", n(1)), ("step", bad())],
        ),
        (
            "import step before repository",
            vec![("step", bad()), ("repository", bad())],
        ),
        (
            "import repository before base",
            vec![("repository", bad()), ("base", n(1))],
        ),
        ("import base type", vec![("base", n(1))]),
        ("import base before bundle", vec![("base", text("zz"))]),
        ("import empty adr", vec![("adr", text(""))]),
        ("import claude code", vec![("provider", text("ClaudeCode"))]),
        ("import unregistered repository", vec![]),
    ] {
        h.send(label, "ImportBundle", bundle(&overrides));
    }
    h.send("start empty", "Start", object([]));
    h.send(
        "start prd before adr",
        "Start",
        start_payload(&[("prd", bad()), ("adr", bad())]),
    );
    h.send(
        "start adr before plan",
        "Start",
        start_payload(&[("adr", bad()), ("plan", bad())]),
    );
    h.send(
        "start plan before validation",
        "Start",
        start_payload(&[("plan", bad()), ("validation", bad())]),
    );
    h.send(
        "start validation before step",
        "Start",
        start_payload(&[("validation", bad()), ("step", n(1))]),
    );
    h.send("start step", "Start", start_payload(&[("step", n(1))]));
    h.send("start while closed", "Start", start_payload(&[]));
    let resolve = |digest: Json, attempt: Json, choice: Json| {
        object([("digest", digest), ("attempt", attempt), ("choice", choice)])
    };
    h.send("resolve empty", "RecoveryResolve", object([]));
    h.send(
        "resolve digest type first",
        "RecoveryResolve",
        resolve(n(1), bad(), n(1)),
    );
    h.send(
        "resolve stale before attempt",
        "RecoveryResolve",
        resolve(text("stale"), bad(), text("Nope")),
    );
    h.send("recovery start empty", "RecoveryStart", object([]));
    h.send(
        "recovery start type",
        "RecoveryStart",
        object([("digest", n(1))]),
    );
    h.send(
        "recovery start stale",
        "RecoveryStart",
        object([("digest", text("stale"))]),
    );
    h.at(
        "inspect 1.1",
        1,
        "RecoveryInspect",
        object([("junk", n(1))]),
    );
    let digest = {
        let r = h.service.request(&Envelope {
            major: 1,
            minor: 1,
            session: OpaqueId(5),
            epoch: h.service.engine.epoch().0,
            request: OpaqueId(9_000),
            revision: None,
            kind: "RecoveryInspect".into(),
            payload: object([]),
        });
        string(get(&r.payload, "digest").unwrap())
            .unwrap()
            .to_owned()
    };
    h.send(
        "resolve attempt before choice",
        "RecoveryResolve",
        resolve(text(&digest), bad(), n(1)),
    );
    h.send(
        "resolve choice type",
        "RecoveryResolve",
        resolve(text(&digest), hex(9), n(1)),
    );
    h.send(
        "resolve choice name",
        "RecoveryResolve",
        resolve(text(&digest), hex(9), text("Nope")),
    );
    for choice in [
        "ValidateOutput",
        "ContinueWithHistory",
        "ContinueFreshContext",
        "Hold",
    ] {
        h.send(
            &format!("resolve {choice} for no subject"),
            "RecoveryResolve",
            resolve(text(&digest), hex(9), text(choice)),
        );
    }
    let validate =
        |attempt: Json, candidate: Json| object([("attempt", attempt), ("candidate", candidate)]);
    h.send("validate empty", "Validate", object([]));
    h.send(
        "validate attempt before candidate",
        "Validate",
        validate(bad(), bad()),
    );
    h.send("validate candidate", "Validate", validate(hex(9), bad()));
    h.send(
        "validate uppercase digest",
        "Validate",
        validate(hex(9), sha('A')),
    );
    h.send("validate unknown", "Validate", validate(hex(9), sha('a')));
    let decide = |attempt: Json, candidate: Json, receipt: Json, action: Json| {
        object([
            ("attempt", attempt),
            ("candidate", candidate),
            ("receipt", receipt),
            ("action", action),
        ])
    };
    h.send("candidate decision empty", "CandidateDecision", object([]));
    h.send(
        "candidate decision attempt first",
        "CandidateDecision",
        decide(bad(), bad(), bad(), n(1)),
    );
    h.send(
        "candidate decision candidate before action",
        "CandidateDecision",
        decide(hex(9), bad(), bad(), n(1)),
    );
    h.send(
        "candidate decision action type",
        "CandidateDecision",
        decide(hex(9), sha('a'), bad(), n(1)),
    );
    h.send(
        "candidate decision accept without receipt",
        "CandidateDecision",
        decide(hex(9), sha('a'), Json::Null, text("Accept")),
    );
    h.send(
        "candidate decision accept with text receipt",
        "CandidateDecision",
        decide(hex(9), sha('a'), bad(), text("Accept")),
    );
    h.send(
        "candidate decision reject with receipt",
        "CandidateDecision",
        decide(hex(9), sha('a'), n(1), text("Reject")),
    );
    h.send(
        "candidate decision unknown action",
        "CandidateDecision",
        decide(hex(9), sha('a'), Json::Null, text("Nope")),
    );
    h.send(
        "candidate decision accept unknown",
        "CandidateDecision",
        decide(hex(9), sha('a'), n(1), text("Accept")),
    );
    h.send(
        "candidate decision reject unknown",
        "CandidateDecision",
        decide(hex(9), sha('a'), Json::Null, text("Reject")),
    );

    // Protocol 1.1 review queries.
    h.send("candidate empty", "Candidate", object([]));
    h.send("candidate bad", "Candidate", object([("attempt", bad())]));
    h.send(
        "candidate extra field",
        "Candidate",
        object([("attempt", hex(9)), ("extra", n(1))]),
    );
    h.send(
        "candidate unknown",
        "Candidate",
        object([("attempt", hex(9))]),
    );
    let changes = |attempt: Json, candidate: Json, offset: Json, limit: Json| {
        object([
            ("attempt", attempt),
            ("candidate", candidate),
            ("offset", offset),
            ("limit", limit),
        ])
    };
    h.send("changes empty", "CandidateChanges", object([]));
    h.send(
        "changes attempt first",
        "CandidateChanges",
        changes(bad(), bad(), n(-1), n(0)),
    );
    h.send(
        "changes candidate before offset",
        "CandidateChanges",
        changes(hex(9), bad(), n(-1), n(0)),
    );
    h.send(
        "changes offset before limit",
        "CandidateChanges",
        changes(hex(9), sha('a'), n(-1), n(0)),
    );
    h.send(
        "changes limit type",
        "CandidateChanges",
        changes(hex(9), sha('a'), n(0), bad()),
    );
    h.send(
        "changes limit zero",
        "CandidateChanges",
        changes(hex(9), sha('a'), n(0), n(0)),
    );
    h.send(
        "changes limit nine",
        "CandidateChanges",
        changes(hex(9), sha('a'), n(0), n(9)),
    );
    h.send(
        "changes unknown",
        "CandidateChanges",
        changes(hex(9), sha('a'), n(0), n(8)),
    );
    let diff = |attempt: Json, candidate: Json, offset: Json| {
        object([
            ("attempt", attempt),
            ("candidate", candidate),
            ("offset", offset),
        ])
    };
    h.send("diff empty", "CandidateDiff", object([]));
    h.send(
        "diff candidate before offset",
        "CandidateDiff",
        diff(hex(9), bad(), bad()),
    );
    h.send(
        "diff offset",
        "CandidateDiff",
        diff(hex(9), sha('a'), bad()),
    );
    h.send(
        "diff unknown",
        "CandidateDiff",
        diff(hex(9), sha('a'), n(0)),
    );
    let report = |attempt: Json, receipt: Json, offset: Json| {
        object([
            ("attempt", attempt),
            ("receipt", receipt),
            ("offset", offset),
        ])
    };
    h.send("report empty", "ValidationReport", object([]));
    h.send(
        "report attempt first",
        "ValidationReport",
        report(bad(), bad(), bad()),
    );
    h.send(
        "report receipt before offset",
        "ValidationReport",
        report(hex(9), bad(), bad()),
    );
    h.send(
        "report offset",
        "ValidationReport",
        report(hex(9), n(1), bad()),
    );
    h.send(
        "report unknown",
        "ValidationReport",
        report(hex(9), n(1), n(0)),
    );

    // A committed command is answered from its receipt; the same identity with another
    // payload is refused before any field is read.
    let open = h.envelope(1, "RecoveryStart", object([("digest", text(&digest))]));
    h.frame("reviewed start", open.clone());
    h.frame("reviewed start repeated", open.clone());
    let mut altered = open.clone();
    altered.payload = object([("digest", n(1))]);
    h.frame("reviewed start identity with another payload", altered);
    let mut renamed = open.clone();
    renamed.kind = "Start".into();
    renamed.payload = start_payload(&[("prd", bad())]);
    h.frame("reviewed start identity as another command", renamed);
    h.send(
        "receipt of the reviewed start",
        "Receipt",
        object([("command", identity(open.request))]),
    );
    h.send("start with nothing imported", "Start", start_payload(&[]));
    h.send(
        "events page",
        "Events",
        object([("after", n(0)), ("limit", n(8))]),
    );
    h.send(
        "snapshot page",
        "Snapshot",
        object([
            ("section", text("aggregates")),
            ("after", text("")),
            ("cursor", Json::Null),
            ("limit", n(8)),
        ]),
    );

    // ApprovedPlan (added after R-01, recorded last so that the request identities of
    // the frames above stay as they were): exactly one field, an identity or null.
    h.at(
        "1.0 frame names ApprovedPlan",
        0,
        "ApprovedPlan",
        object([]),
    );
    h.send("approved plan empty", "ApprovedPlan", object([]));
    h.send(
        "approved plan bad",
        "ApprovedPlan",
        object([("after", bad())]),
    );
    h.send(
        "approved plan extra field",
        "ApprovedPlan",
        object([("after", Json::Null), ("plan", hex(9))]),
    );
    h.send(
        "approved plan none",
        "ApprovedPlan",
        object([("after", Json::Null)]),
    );
    h.send(
        "approved plan none after",
        "ApprovedPlan",
        object([("after", hex(9))]),
    );

    // Feature Chat (protocol 1.2, M2), recorded last for the same reason. No provider is
    // configured, so no turn can be launched; every refusal before that is pinned.
    let at2 = |h: &mut H, label: &str, kind: &str, payload: Json| {
        h.at(label, 2, kind, payload);
    };
    h.send(
        "1.1 frame names FeatureChat",
        "FeatureChat",
        object([("feature", hex(7)), ("after", n(0))]),
    );
    h.send(
        "1.1 frame names CancelTurn",
        "CancelTurn",
        object([("turn", hex(7))]),
    );
    at2(
        h,
        "feature chat empty",
        "FeatureChat",
        object([("feature", hex(7)), ("after", n(0))]),
    );
    at2(
        h,
        "feature chat extra field",
        "FeatureChat",
        object([("feature", hex(7)), ("after", n(0)), ("limit", n(8))]),
    );
    at2(
        h,
        "feature chat bad after",
        "FeatureChat",
        object([("feature", hex(7)), ("after", bad())]),
    );
    at2(
        h,
        "chat text none",
        "ChatText",
        object([
            ("subject", hex(7)),
            ("part", text("Reply")),
            ("offset", n(0)),
        ]),
    );
    at2(
        h,
        "chat text unknown part",
        "ChatText",
        object([
            ("subject", hex(7)),
            ("part", text("Diff")),
            ("offset", n(0)),
        ]),
    );
    let turn = |provider: &str, kind: &str| {
        object([
            ("feature", hex(7)),
            ("kind", text(kind)),
            ("repository", hex(8)),
            ("provider", text(provider)),
            ("message", text("make it so")),
        ])
    };
    let Json::Object(mut extra) = turn("Codex", "Discover") else {
        unreachable!()
    };
    extra.push(("executable".into(), text("/bin/sh")));
    at2(h, "chat turn extra field", "ChatTurn", Json::Object(extra));
    at2(
        h,
        "chat turn unknown provider",
        "ChatTurn",
        turn("Gemini", "Discover"),
    );
    at2(
        h,
        "chat turn unknown kind",
        "ChatTurn",
        turn("Codex", "Build"),
    );
    at2(
        h,
        "chat turn codex not configured",
        "ChatTurn",
        turn("Codex", "Discover"),
    );
    at2(
        h,
        "chat turn claude not configured",
        "ChatTurn",
        turn("ClaudeCode", "Discover"),
    );
    let answer = |action: &str, body: Json| {
        object([
            ("question", hex(7)),
            ("revision", n(1)),
            ("action", text(action)),
            ("answer", body),
        ])
    };
    at2(
        h,
        "answer unknown question",
        "AnswerQuestion",
        answer("Answer", text("yes")),
    );
    at2(
        h,
        "answer defer with text",
        "AnswerQuestion",
        answer("Defer", text("yes")),
    );
    at2(
        h,
        "answer without text",
        "AnswerQuestion",
        answer("Answer", Json::Null),
    );
    at2(
        h,
        "answer unknown action",
        "AnswerQuestion",
        answer("Approve", Json::Null),
    );
    at2(
        h,
        "cancel unknown turn",
        "CancelTurn",
        object([("turn", hex(7))]),
    );
    at2(h, "chat index empty", "ChatIndex", object([]));
    at2(
        h,
        "chat index extra field",
        "ChatIndex",
        object([("limit", n(8))]),
    );
}

#[test]
fn r01_every_recorded_frame_is_answered_exactly_as_before_typed_messages() {
    let mut h = H::new();
    record(&mut h);
    let seen = h.seen.join("\n");
    assert!(
        seen == EXPECTED.trim(),
        "protocol answers changed:\n{seen}\n"
    );
}

const EXPECTED: &str = r#"
snapshot before hello => 1.1 Error {"code":"HandshakeRequired"}
hello 1.0 => 1.0 Response {"admission":"closed until reviewed recovery start","commands":"PrepareClose CancelClose ConfirmClose CancelAttempt HumanDecision","compatible":true,"epoch":1,"max_frame":65536,"max_page":8,"queries":"Snapshot Events EventDetail Log Receipt CloseStatus ReviewBundle ShutdownReceipt Recovery","slice":false}
hello 1.3 => 1.2 Response {"admission":"closed until reviewed recovery start","commands":"PrepareClose CancelClose ConfirmClose CancelAttempt HumanDecision RegisterRepository ImportBundle Start RecoveryResolve RecoveryStart Validate CandidateDecision ChatTurn AnswerQuestion CancelTurn","compatible":false,"epoch":1,"max_frame":65536,"max_page":8,"queries":"Snapshot Events EventDetail Log Receipt CloseStatus ReviewBundle ShutdownReceipt Recovery RecoveryInspect Candidate CandidateChanges CandidateDiff ValidationReport ApprovedPlan FeatureChat ChatText ChatIndex","slice":true}
close command while incompatible => 1.1 Error {"code":"ProtocolMismatch: diagnostics only"}
decision while incompatible => 1.1 Error {"code":"ProtocolMismatch: diagnostics only"}
slice command while incompatible => 1.1 Error {"code":"ProtocolMismatch: diagnostics only"}
hello 1.1 => 1.1 Response {"admission":"closed until reviewed recovery start","commands":"PrepareClose CancelClose ConfirmClose CancelAttempt HumanDecision RegisterRepository ImportBundle Start RecoveryResolve RecoveryStart Validate CandidateDecision","compatible":true,"epoch":1,"max_frame":65536,"max_page":8,"queries":"Snapshot Events EventDetail Log Receipt CloseStatus ReviewBundle ShutdownReceipt Recovery RecoveryInspect Candidate CandidateChanges CandidateDiff ValidationReport ApprovedPlan","slice":true}
hello from another session => 1.1 Error {"code":"SessionMismatch"}
wrong epoch => 1.1 Error {"code":"EpochMismatch"}
unknown type => 1.1 Error {"code":"UnknownType"}
1.0 frame names RecoveryInspect => 1.0 Error {"code":"UnknownType"}
1.0 frame names Start => 1.0 Error {"code":"UnknownType"}
1.0 frame names RegisterRepository => 1.0 Error {"code":"UnknownType"}
1.0 frame names Candidate => 1.0 Error {"code":"UnknownType"}
1.0 frame names ValidationReport => 1.0 Error {"code":"UnknownType"}
snapshot empty => 1.1 Error {"code":"missing section"}
snapshot two faults => 1.1 Error {"code":"expected text"}
snapshot after, cursor, limit => 1.1 Error {"code":"expected text"}
snapshot cursor, limit => 1.1 Error {"code":"expected integer"}
snapshot limit range => 1.1 Error {"code":"Limit"}
snapshot unknown section => 1.1 Error {"code":"Corrupt(\"unknown snapshot section\")"}
snapshot limit zero => 1.1 Error {"code":"Corrupt(\"invalid page\")"}
snapshot with an extra field => 1.1 Response {"admission":"Closed","as_of_event_seq":1,"epoch":1,"next":null,"rows":[],"section":"aggregates","shutdown":"Running"}
events two faults => 1.1 Error {"code":"expected integer"}
events limit range => 1.1 Error {"code":"Limit"}
events negative => 1.1 Error {"code":"expected nonnegative integer"}
events far ahead => 1.1 Response []
event detail seq => 1.1 Error {"code":"expected integer"}
event detail offset => 1.1 Error {"code":"missing offset"}
log two faults => 1.1 Error {"code":"expected 32 lowercase hex identity"}
log sequence, offset => 1.1 Error {"code":"expected integer"}
log missing offset => 1.1 Error {"code":"missing offset"}
log unknown attempt => 1.1 Response null
receipt empty => 1.1 Error {"code":"missing command"}
receipt bad => 1.1 Error {"code":"expected 32 lowercase hex identity"}
receipt uppercase identity => 1.1 Error {"code":"expected 32 lowercase hex identity"}
receipt unknown => 1.1 Response null
shutdown receipt empty => 1.1 Error {"code":"missing ticket"}
shutdown receipt bad => 1.1 Error {"code":"expected text"}
shutdown receipt unknown => 1.1 Response null
recovery two faults => 1.1 Error {"code":"expected nonnegative integer"}
recovery limit type => 1.1 Error {"code":"expected integer"}
recovery limit zero => 1.1 Error {"code":"Limit"}
recovery limit nine => 1.1 Error {"code":"Limit"}
recovery page => 1.1 Response {"digest":"95eb9cc2f28783674ee4858759130e25fc6ee54822d07726fb3868924bdb21ba","rows":[]}
recovery page 1.0 => 1.0 Response {"digest":"95eb9cc2f28783674ee4858759130e25fc6ee54822d07726fb3868924bdb21ba","rows":[]}
close status => 1.1 Response {"active_count":0,"receipt":null,"state":"Open","ticket":null}
review bundle empty => 1.1 Error {"code":"missing subject"}
review bundle bad => 1.1 Error {"code":"expected 32 lowercase hex identity"}
review bundle unknown => 1.1 Response null
decision empty => 1.1 Error {"code":"InvalidDecisionPayload"}
decision eight unknown fields => 1.1 Error {"code":"missing subject"}
decision reserved identity => 1.1 Error {"code":"ReservedIdentity"}
decision subject before revision => 1.1 Error {"code":"expected 32 lowercase hex identity"}
decision revision before hash => 1.1 Error {"code":"RevisionRequired"}
decision hash before the rest => 1.1 Error {"code":"expected text"}
decision content hash => 1.1 Error {"code":"expected text"}
decision displayed hash => 1.1 Error {"code":"expected text"}
decision context hash => 1.1 Error {"code":"expected text"}
decision upstream type => 1.1 Error {"code":"InvalidUpstream"}
decision upstream item => 1.1 Error {"code":"expected integer"}
decision upstream length => 1.1 Error {"code":"InvalidUpstream"}
decision prior => 1.1 Error {"code":"expected integer"}
decision action type => 1.1 Error {"code":"expected text"}
decision action name => 1.1 Error {"code":"UnknownDecision"}
decision on nothing => 1.1 Error {"code":"Rejected(Rejected(\"stale review bundle\"))"}
decision revoke on nothing => 1.1 Error {"code":"Rejected(Rejected(\"stale review bundle\"))"}
cancel close empty => 1.1 Error {"code":"missing ticket"}
cancel close bad => 1.1 Error {"code":"expected 32 lowercase hex identity"}
cancel close unknown => 1.1 Error {"code":"Rejected(Rejected(\"unknown close ticket\"))"}
confirm close empty => 1.1 Error {"code":"missing ticket"}
confirm close unknown => 1.1 Error {"code":"Rejected(Rejected(\"unknown close ticket\"))"}
cancel attempt empty => 1.1 Error {"code":"missing attempt"}
cancel attempt bad => 1.1 Error {"code":"expected text"}
cancel attempt unknown => 1.1 Error {"code":"Rejected(Storage(Sqlite(SqliteError { code: 787, message: \"FOREIGN KEY constraint failed\" })))"}
close reserved identity => 1.1 Error {"code":"ReservedIdentity"}
slice reserved identity => 1.1 Error {"code":"ReservedIdentity"}
register empty => 1.1 Error {"code":"InvalidPayload: unexpected or missing fields"}
register extra field => 1.1 Error {"code":"InvalidPayload: unexpected or missing fields"}
register path type => 1.1 Error {"code":"expected text"}
register path with NUL => 1.1 Error {"code":"InvalidRepositoryPath"}
register long path => 1.1 Error {"code":"InvalidRepositoryPath"}
register outside => 1.1 Error {"code":"RepositoryOutsideDisposableRoot"}
import empty => 1.1 Error {"code":"InvalidPayload: unexpected or missing fields"}
import identities first => 1.1 Error {"code":"expected 32 lowercase hex identity"}
import adr id => 1.1 Error {"code":"expected 32 lowercase hex identity"}
import plan id => 1.1 Error {"code":"expected 32 lowercase hex identity"}
import validation id before prd => 1.1 Error {"code":"expected 32 lowercase hex identity"}
import prd type before validation => 1.1 Error {"code":"expected text"}
import prd content before validation => 1.1 Error {"code":"InvalidPrd: Json(Syntax { offset: 0, message: \"invalid JSON\" })"}
import validation type before provider => 1.1 Error {"code":"expected text"}
import validation content before provider => 1.1 Error {"code":"InvalidValidationPlan: Syntax { offset: 0, message: \"invalid JSON\" }"}
import provider type before feature => 1.1 Error {"code":"expected text"}
import provider name before feature => 1.1 Error {"code":"UnknownProvider"}
import feature before adr => 1.1 Error {"code":"expected 32 lowercase hex identity"}
import adr before step => 1.1 Error {"code":"expected text"}
import step before repository => 1.1 Error {"code":"expected 32 lowercase hex identity"}
import repository before base => 1.1 Error {"code":"expected 32 lowercase hex identity"}
import base type => 1.1 Error {"code":"expected text"}
import base before bundle => 1.1 Error {"code":"InvalidBundle: exact base commit required"}
import empty adr => 1.1 Error {"code":"InvalidBundle: ADR text must be non-empty and bounded"}
import claude code => 1.1 Error {"code":"bundle repository is not registered"}
import unregistered repository => 1.1 Error {"code":"bundle repository is not registered"}
start empty => 1.1 Error {"code":"InvalidPayload: unexpected or missing fields"}
start prd before adr => 1.1 Error {"code":"expected 32 lowercase hex identity"}
start adr before plan => 1.1 Error {"code":"expected 32 lowercase hex identity"}
start plan before validation => 1.1 Error {"code":"expected 32 lowercase hex identity"}
start validation before step => 1.1 Error {"code":"expected 32 lowercase hex identity"}
start step => 1.1 Error {"code":"expected text"}
start while closed => 1.1 Error {"code":"AdmissionClosed"}
resolve empty => 1.1 Error {"code":"InvalidPayload: unexpected or missing fields"}
resolve digest type first => 1.1 Error {"code":"expected text"}
resolve stale before attempt => 1.1 Error {"code":"StaleInspection: inspect again"}
recovery start empty => 1.1 Error {"code":"InvalidPayload: unexpected or missing fields"}
recovery start type => 1.1 Error {"code":"expected text"}
recovery start stale => 1.1 Error {"code":"StaleInspection: inspect again"}
inspect 1.1 => 1.1 Response {"digest":"95eb9cc2f28783674ee4858759130e25fc6ee54822d07726fb3868924bdb21ba","other_quarantined":0,"subjects":0}
resolve attempt before choice => 1.1 Error {"code":"expected 32 lowercase hex identity"}
resolve choice type => 1.1 Error {"code":"expected text"}
resolve choice name => 1.1 Error {"code":"UnknownChoice"}
resolve ValidateOutput for no subject => 1.1 Error {"code":"attempt is not a recovery subject of this inspection"}
resolve ContinueWithHistory for no subject => 1.1 Error {"code":"attempt is not a recovery subject of this inspection"}
resolve ContinueFreshContext for no subject => 1.1 Error {"code":"attempt is not a recovery subject of this inspection"}
resolve Hold for no subject => 1.1 Error {"code":"attempt is not a recovery subject of this inspection"}
validate empty => 1.1 Error {"code":"InvalidPayload: unexpected or missing fields"}
validate attempt before candidate => 1.1 Error {"code":"expected 32 lowercase hex identity"}
validate candidate => 1.1 Error {"code":"expected a SHA-256 digest"}
validate uppercase digest => 1.1 Error {"code":"expected a SHA-256 digest"}
validate unknown => 1.1 Error {"code":"validation needs a sealed attempt result"}
candidate decision empty => 1.1 Error {"code":"InvalidPayload: unexpected or missing fields"}
candidate decision attempt first => 1.1 Error {"code":"expected 32 lowercase hex identity"}
candidate decision candidate before action => 1.1 Error {"code":"expected a SHA-256 digest"}
candidate decision action type => 1.1 Error {"code":"expected text"}
candidate decision accept without receipt => 1.1 Error {"code":"ReceiptRequired"}
candidate decision accept with text receipt => 1.1 Error {"code":"ReceiptRequired"}
candidate decision reject with receipt => 1.1 Error {"code":"UnknownDecision"}
candidate decision unknown action => 1.1 Error {"code":"UnknownDecision"}
candidate decision accept unknown => 1.1 Error {"code":"CandidateChanged: candidate is not durably recorded"}
candidate decision reject unknown => 1.1 Error {"code":"StaleCandidate: this is not the sealed candidate"}
candidate empty => 1.1 Error {"code":"InvalidPayload: unexpected or missing fields"}
candidate bad => 1.1 Error {"code":"expected 32 lowercase hex identity"}
candidate extra field => 1.1 Error {"code":"InvalidPayload: unexpected or missing fields"}
candidate unknown => 1.1 Response {"attempt":"00000000000000000000000000000009","sealed":false}
changes empty => 1.1 Error {"code":"InvalidPayload: unexpected or missing fields"}
changes attempt first => 1.1 Error {"code":"expected 32 lowercase hex identity"}
changes candidate before offset => 1.1 Error {"code":"expected a SHA-256 digest"}
changes offset before limit => 1.1 Error {"code":"expected nonnegative integer"}
changes limit type => 1.1 Error {"code":"expected integer"}
changes limit zero => 1.1 Error {"code":"Limit"}
changes limit nine => 1.1 Error {"code":"Limit"}
changes unknown => 1.1 Error {"code":"CandidateUnavailable: candidate is not durably recorded"}
diff empty => 1.1 Error {"code":"InvalidPayload: unexpected or missing fields"}
diff candidate before offset => 1.1 Error {"code":"expected a SHA-256 digest"}
diff offset => 1.1 Error {"code":"expected integer"}
diff unknown => 1.1 Error {"code":"CandidateUnavailable: worktree missing"}
report empty => 1.1 Error {"code":"InvalidPayload: unexpected or missing fields"}
report attempt first => 1.1 Error {"code":"expected 32 lowercase hex identity"}
report receipt before offset => 1.1 Error {"code":"expected integer"}
report offset => 1.1 Error {"code":"expected integer"}
report unknown => 1.1 Response null
reviewed start => 1.1 Response {"acknowledged":true,"command":"000000000000000000000000000000ab","epoch":1,"event_seq":2,"operation_completed":false,"replayed":false,"response":"admission open after reviewed recovery"}
reviewed start repeated => 1.1 Response {"acknowledged":true,"command":"000000000000000000000000000000ab","epoch":1,"event_seq":2,"operation_completed":false,"replayed":true,"response":"admission open after reviewed recovery"}
reviewed start identity with another payload => 1.1 Error {"code":"CommandIdConflict(CommandId(OpaqueId(171)))"}
reviewed start identity as another command => 1.1 Error {"code":"CommandIdConflict(CommandId(OpaqueId(171)))"}
receipt of the reviewed start => 1.1 Response {"acknowledged":true,"command":"000000000000000000000000000000ab","epoch":1,"event_seq":2,"operation_completed":false,"replayed":true,"response":"admission open after reviewed recovery"}
start with nothing imported => 1.1 Error {"code":"StartRefused: missing or stale approval: PRD"}
events page => 1.1 Response [{"aggregate_id":null,"aggregate_kind":null,"detail_complete":true,"epoch":1,"payload":"previous_epoch=0;previous_shutdown=NONE;quarantined_claims=0","revision":null,"seq":1,"type":"EngineStarted"},{"aggregate_id":null,"aggregate_kind":null,"detail_complete":true,"epoch":1,"payload":"95eb9cc2f28783674ee4858759130e25fc6ee54822d07726fb3868924bdb21ba","revision":null,"seq":2,"type":"RecoveryStarted"}]
snapshot page => 1.1 Response {"admission":"Open","as_of_event_seq":2,"epoch":1,"next":null,"rows":[],"section":"aggregates","shutdown":"Running"}
1.0 frame names ApprovedPlan => 1.0 Error {"code":"UnknownType"}
approved plan empty => 1.1 Error {"code":"InvalidPayload: unexpected or missing fields"}
approved plan bad => 1.1 Error {"code":"expected 32 lowercase hex identity"}
approved plan extra field => 1.1 Error {"code":"InvalidPayload: unexpected or missing fields"}
approved plan none => 1.1 Response null
approved plan none after => 1.1 Response null
1.1 frame names FeatureChat => 1.1 Error {"code":"UnknownType"}
1.1 frame names CancelTurn => 1.1 Error {"code":"UnknownType"}
feature chat empty => 1.2 Response {"feature":"00000000000000000000000000000007","next":null,"prd":null,"running":null,"turns":[]}
feature chat extra field => 1.2 Error {"code":"InvalidPayload: unexpected or missing fields"}
feature chat bad after => 1.2 Error {"code":"expected integer"}
chat text none => 1.2 Response null
chat text unknown part => 1.2 Error {"code":"UnknownPart"}
chat turn extra field => 1.2 Error {"code":"InvalidPayload: unexpected or missing fields"}
chat turn unknown provider => 1.2 Error {"code":"UnknownProvider"}
chat turn unknown kind => 1.2 Error {"code":"UnknownTurnKind"}
chat turn codex not configured => 1.2 Error {"code":"NoProductionAdapter: Codex is not configured"}
chat turn claude not configured => 1.2 Error {"code":"NoProductionAdapter: ClaudeCode"}
answer unknown question => 1.2 Error {"code":"UnknownQuestion: not asked in a Feature Chat"}
answer defer with text => 1.2 Error {"code":"InvalidAnswer: an answer is text exactly when the action is Answer"}
answer without text => 1.2 Error {"code":"InvalidAnswer: an answer is text exactly when the action is Answer"}
answer unknown action => 1.2 Error {"code":"UnknownAction"}
cancel unknown turn => 1.2 Error {"code":"UnknownTurn"}
chat index empty => 1.2 Response {"features":[],"features_complete":true,"repositories":[],"repositories_complete":true}
chat index extra field => 1.2 Error {"code":"InvalidPayload: unexpected or missing fields"}
"#;

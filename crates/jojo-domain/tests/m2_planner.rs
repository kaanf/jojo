//! M2 pure lead agent contract: the read-only argument vectors, the turn request, the two
//! lead agent stream profiles, the prompts and the strict reading of a reply. `PROBE_CLAUDE`
//! and `PROBE_CODEX` are the unmodified standard output of `claude` 2.1.311 and
//! `codex-cli` 0.169.0, each run once with exactly the golden discovery vector in a
//! generated sample repository on 2026-11-16 (inlined, because this crate's tests may not
//! read from disk; kept under `~/JojoDisposable/m2-probes/20261116T161559Z`). Everything
//! else is synthetic. No process, I/O, provider binary, credential or network here.
use jojo_domain::claude::{self, CLAUDE_VARIABLES, CLAUDE_VERSION};
use jojo_domain::codex::{self, CODEX_HOME_VARIABLE, CODEX_VERSION};
use jojo_domain::identity::OpaqueId;
use jojo_domain::json;
use jojo_domain::planner::*;
use jojo_domain::provider::*;
use jojo_domain::stream::{
    CLAUDE_DISCOVERY_STREAM_JSON, CLAUDE_PRINT_STREAM_JSON, CODEX_DISCOVERY_JSONL,
    CODEX_EXEC_JSONL, LineFramer, StreamNormalizer,
};
use std::path::Path;

const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const REPOSITORY: &str = "/selected/home/disposable/sample";
const OUTPUT: &str = "/selected/home/data/turns/turn-1";
const COMMIT: &str = "45147111b537d1965241c4d2b40e32ad9b740ea9";

fn spec(provider: ProviderKind) -> TurnSpec {
    let (version, configuration, variables) = match provider {
        ProviderKind::ClaudeCode => (
            CLAUDE_VERSION,
            claude::configuration(DIGEST).unwrap(),
            CLAUDE_VARIABLES.map(String::from).to_vec(),
        ),
        ProviderKind::Codex => (
            CODEX_VERSION,
            codex::configuration(DIGEST).unwrap(),
            vec![CODEX_HOME_VARIABLE.to_owned()],
        ),
    };
    TurnSpec {
        turn: OpaqueId(1),
        kind: TurnKind::Discover,
        fingerprint: ProviderFingerprint {
            provider,
            executable: "/synthetic/fixture".into(),
            version: version.into(),
            schema: PROVIDER_SCHEMA,
            parser: turn_parser(provider).into(),
            configuration,
        },
        prompt: b"synthetic objective".to_vec(),
        prompt_digest: "prompt-digest".into(),
        repository: REPOSITORY.into(),
        base_commit: COMMIT.into(),
        raw_output_dir: OUTPUT.into(),
        environment_allowlist: variables,
        timeout_millis: 1000,
        max_output_bytes: 4096,
        max_frame_bytes: 1024,
    }
}

fn changed(provider: ProviderKind, edit: impl FnOnce(&mut TurnSpec)) -> TurnSpec {
    let mut s = spec(provider);
    edit(&mut s);
    s
}

#[test]
fn m2_golden_discovery_vectors_are_byte_exact_and_read_only() {
    let argv = claude::discovery_argv();
    assert_eq!(
        argv.join("\0").as_bytes(),
        b"-p\0--output-format\0stream-json\0--verbose\0--input-format\0text\0--restricted\0--strict-mcp-config\0--tools\0Read,Glob,Grep\0--permission-mode\0manual\0--permission-prompts\0none\0--no-session-persistence\0--disable-slash-commands\0--no-chrome"
    );
    assert_eq!(claude::check_discovery_argv(&argv), Ok(()));
    let mut listed: Vec<&str> = argv[9].split(',').collect();
    listed.sort_unstable();
    assert_eq!(listed, claude::CLAUDE_DISCOVERY_TOOLS);
    assert_eq!(argv[11], claude::CLAUDE_DISCOVERY_PERMISSION_MODE);
    // The developer vector differs in exactly the tool list and the permission mode.
    let developer = claude::print_argv();
    let differing: Vec<usize> = (0..argv.len())
        .filter(|&i| argv[i] != developer[i])
        .collect();
    assert_eq!((argv.len(), differing), (developer.len(), vec![9, 11]));
    assert!(claude::check_print_argv(&argv).is_err());
    assert!(claude::check_discovery_argv(&developer).is_err());

    let repository = Path::new(REPOSITORY);
    let argv = codex::discovery_argv(repository).unwrap();
    assert_eq!(
        argv.join("\0"),
        format!(
            "exec\0--json\0--color\0never\0--sandbox\0read-only\0--ephemeral\0--ignore-user-config\0--ignore-rules\0-C\0{REPOSITORY}"
        )
    );
    assert_eq!(codex::check_discovery_argv(&argv, repository), Ok(()));
    assert!(codex::check_exec_argv(&argv, repository).is_err());
    let developer = codex::exec_argv(repository).unwrap();
    assert!(codex::check_discovery_argv(&developer, repository).is_err());
    assert!(codex::check_discovery_argv(&argv, Path::new("/elsewhere")).is_err());
    assert!(codex::discovery_argv(Path::new("relative")).is_err());
    assert!(codex::discovery_argv(Path::new("/a/../b")).is_err());

    for provider in [ProviderKind::ClaudeCode, ProviderKind::Codex] {
        let s = spec(provider);
        let golden = turn_argv(&s).unwrap();
        let prohibited: &[&str] = match provider {
            ProviderKind::ClaudeCode => &claude::PROHIBITED_ARGUMENTS,
            ProviderKind::Codex => &codex::PROHIBITED_ARGUMENTS,
        };
        for word in prohibited {
            assert!(!golden.iter().any(|a| a == word), "{word}");
        }
        for write in ["Edit", "Write", "acceptEdits", "workspace-write"] {
            assert!(
                !golden.iter().any(|a| a.split(',').any(|t| t == write)),
                "{write}"
            );
        }
        assert_eq!(s.role(), Role::Planner);
    }
}

#[test]
fn m2_turn_request_is_one_bounded_run_of_the_pinned_profile() {
    for provider in [ProviderKind::ClaudeCode, ProviderKind::Codex] {
        assert_eq!(check_turn(&spec(provider)), Ok(()));
        let invalid: Vec<TurnSpec> = vec![
            changed(provider, |s| s.prompt.clear()),
            changed(provider, |s| s.prompt = vec![b'x'; MAX_PROMPT_BYTES + 1]),
            changed(provider, |s| s.prompt_digest = " ".into()),
            changed(provider, |s| s.base_commit = "main".into()),
            changed(provider, |s| s.base_commit = "g".repeat(40)),
            changed(provider, |s| s.timeout_millis = 0),
            changed(provider, |s| s.max_frame_bytes = 0),
            changed(provider, |s| s.max_frame_bytes = s.max_output_bytes + 1),
            changed(provider, |s| s.repository = "relative/repository".into()),
            changed(provider, |s| s.repository = "/selected/../home".into()),
            changed(provider, |s| s.raw_output_dir = "output".into()),
            changed(provider, |s| s.fingerprint.executable = "claude".into()),
            changed(provider, |s| s.fingerprint.schema += 1),
            // The streams of a run are never kept inside the repository it reads.
            changed(provider, |s| {
                s.raw_output_dir = format!("{REPOSITORY}/.out").into()
            }),
            changed(provider, |s| {
                s.repository = format!("{OUTPUT}/repository").into()
            }),
            changed(provider, |s| s.environment_allowlist.push("PATH".into())),
            changed(provider, |s| s.environment_allowlist.clear()),
        ];
        for (index, s) in invalid.iter().enumerate() {
            assert_eq!(
                check_turn(s),
                Err(ProviderError::InvalidSpec),
                "{provider:?} {index}"
            );
        }
        let developer = match provider {
            ProviderKind::ClaudeCode => CLAUDE_PRINT_STREAM_JSON,
            ProviderKind::Codex => CODEX_EXEC_JSONL,
        };
        let mismatched: Vec<TurnSpec> = vec![
            changed(provider, |s| s.fingerprint.parser = developer.into()),
            changed(provider, |s| {
                s.fingerprint.version = "another release".into()
            }),
            changed(provider, |s| {
                s.fingerprint.configuration = "unpinned".into()
            }),
            changed(provider, |s| {
                s.fingerprint.provider = match provider {
                    ProviderKind::ClaudeCode => ProviderKind::Codex,
                    ProviderKind::Codex => ProviderKind::ClaudeCode,
                }
            }),
        ];
        for (index, s) in mismatched.iter().enumerate() {
            assert_eq!(
                check_turn(s),
                Err(ProviderError::FingerprintMismatch),
                "{provider:?} {index}"
            );
        }
    }
    assert_eq!(TurnKind::named("Discover"), Some(TurnKind::Discover));
    assert_eq!(TurnKind::named(TurnKind::Plan.name()), Some(TurnKind::Plan));
    assert_eq!(TurnKind::named("discover"), None);
}

#[test]
fn m2_turn_environment_is_exactly_the_providers_own_variables() {
    let pair = |home: &str, user: &str| {
        vec![
            ("HOME".to_owned(), home.to_owned()),
            ("USER".to_owned(), user.to_owned()),
        ]
    };
    let one = |home: &str| vec![(CODEX_HOME_VARIABLE.to_owned(), home.to_owned())];
    let claude = spec(ProviderKind::ClaudeCode);
    // The repository usually lies under the home; the home never lies inside the run.
    assert_eq!(
        check_turn_environment(&claude, &pair("/selected/home", "kaan")),
        Ok(Path::new("/selected/home"))
    );
    for (home, user) in [
        (REPOSITORY, "kaan"),
        ("/selected/home/disposable/sample/inner", "kaan"),
        (OUTPUT, "kaan"),
        ("relative", "kaan"),
        ("/", "kaan"),
        ("/selected/home/", "kaan"),
        ("/selected/home", ""),
        ("/selected/home", "ka an"),
        ("/selected/home", "k/aan"),
    ] {
        assert!(
            check_turn_environment(&claude, &pair(home, user)).is_err(),
            "{home} {user}"
        );
    }
    assert!(check_turn_environment(&claude, &one("/selected/home")).is_err());
    let mut swapped = pair("/selected/home", "kaan");
    swapped.swap(0, 1);
    assert!(check_turn_environment(&claude, &swapped).is_err());

    let codex = spec(ProviderKind::Codex);
    assert_eq!(
        check_turn_environment(&codex, &one("/selected/codex-home")),
        Ok(Path::new("/selected/codex-home"))
    );
    for home in [
        "/selected/home",
        REPOSITORY,
        OUTPUT,
        "/selected/home/data",
        "codex-home",
        "/",
    ] {
        assert!(
            check_turn_environment(&codex, &one(home)).is_err(),
            "{home}"
        );
    }
    assert!(check_turn_environment(&codex, &pair("/selected/codex-home", "kaan")).is_err());
    assert!(check_turn_environment(&codex, &[]).is_err());
}

fn payloads(parser: &str, stream: &str, chunk: usize) -> (Vec<EventPayload>, StreamNormalizer) {
    let mut framer = LineFramer::new(64 * 1024);
    let mut normalizer = StreamNormalizer::for_parser(parser).unwrap();
    let mut out = Vec::new();
    for piece in stream.as_bytes().chunks(chunk) {
        for frame in framer.push(piece) {
            out.extend(normalizer.frame(&frame));
        }
    }
    if let Some(frame) = framer.finish() {
        out.extend(normalizer.frame(&frame));
    }
    (out, normalizer)
}

fn last_text(events: &[EventPayload]) -> Option<&str> {
    events.iter().rev().find_map(|e| match e {
        EventPayload::TextDelta(text) => Some(text.as_str()),
        _ => None,
    })
}

fn violations(events: &[EventPayload]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|e| match e {
            EventPayload::ProtocolViolation(why) => Some(why.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn m2_real_read_only_streams_normalize_to_one_successful_turn_with_a_json_reply() {
    for (parser, provider, stream, tools) in [
        (
            CLAUDE_DISCOVERY_STREAM_JSON,
            ProviderKind::ClaudeCode,
            PROBE_CLAUDE,
            3,
        ),
        (CODEX_DISCOVERY_JSONL, ProviderKind::Codex, PROBE_CODEX, 2),
    ] {
        for chunk in [1, 7, 4096, usize::MAX] {
            let (events, normalizer) = payloads(parser, stream, chunk);
            assert_eq!(normalizer.provider(), provider);
            assert!(!normalizer.protocol_failed(), "{parser} {chunk}");
            assert_eq!(
                normalizer.terminal(),
                Some(TerminalStatus::Success),
                "{parser}"
            );
            assert_eq!(violations(&events), Vec::<&str>::new());
            let started = events
                .iter()
                .filter(|e| matches!(e, EventPayload::ToolStarted { .. }))
                .count();
            let finished = events
                .iter()
                .filter(|e| matches!(e, EventPayload::ToolFinished { .. }))
                .count();
            assert_eq!((started, finished), (tools, tools), "{parser}");
            // The reply is the last message: one JSON object and nothing around it.
            let reply = last_text(&events).unwrap();
            let json::Json::Object(members) = json::parse(reply.as_bytes()).unwrap() else {
                panic!("{parser}: the reply is not an object");
            };
            let keys: Vec<&str> = members.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(keys, ["wrote_notes", "files_read", "questions", "summary"]);
            assert_eq!(members[0].1, json::Json::Bool(false));
            // It is not a reply of either schema, so it is no reply at all.
            assert!(parse_reply(TurnKind::Discover, reply).is_err());
        }
    }
    // Neither stream passes as a developer run, nor a developer profile as a turn.
    let (_, n) = payloads(CLAUDE_PRINT_STREAM_JSON, PROBE_CLAUDE, usize::MAX);
    assert!(n.protocol_failed());
    assert_ne!(n.terminal(), Some(TerminalStatus::Success));
}

const INIT: &str = r#"{"type":"system","subtype":"init","session_id":"s1","tools":["Glob","Grep","Read"],"mcp_servers":[],"permissionMode":"default","apiKeySource":"none"}"#;
const TEXT: &str = r#"{"type":"assistant","session_id":"s1","parent_tool_use_id":null,"message":{"content":[{"type":"text","text":"{}"}]}}"#;
const RESULT: &str = r#"{"type":"result","subtype":"success","is_error":false,"terminal_reason":"completed","session_id":"s1","usage":{"input_tokens":1,"output_tokens":2}}"#;

fn run(parser: &str, records: &[&str]) -> (Vec<EventPayload>, StreamNormalizer) {
    payloads(parser, &format!("{}\n", records.join("\n")), usize::MAX)
}

#[test]
fn m2_claude_turn_is_held_to_the_read_only_envelope() {
    let (events, n) = run(CLAUDE_DISCOVERY_STREAM_JSON, &[INIT, TEXT, RESULT]);
    assert_eq!(n.terminal(), Some(TerminalStatus::Success));
    assert_eq!(last_text(&events), Some("{}"));
    let outside = "init outside the approved envelope";
    for (from, to) in [
        // The developer envelope is not a turn.
        (
            r#"["Glob","Grep","Read"]"#,
            r#"["Edit","Glob","Grep","Read","Write"]"#,
        ),
        (
            r#"["Glob","Grep","Read"]"#,
            r#"["Glob","Grep","Read","Write"]"#,
        ),
        (r#"["Glob","Grep","Read"]"#, r#"["Glob","Read"]"#),
        (
            r#"["Glob","Grep","Read"]"#,
            r#"["Bash","Glob","Grep","Read"]"#,
        ),
        (
            r#""permissionMode":"default""#,
            r#""permissionMode":"acceptEdits""#,
        ),
        (
            r#""permissionMode":"default""#,
            r#""permissionMode":"manual""#,
        ),
        (
            r#""permissionMode":"default""#,
            r#""permissionMode":"bypassPermissions""#,
        ),
        (r#""mcp_servers":[]"#, r#""mcp_servers":[{"name":"x"}]"#),
        (r#""apiKeySource":"none""#, r#""apiKeySource":"user""#),
    ] {
        let init = INIT.replace(from, to);
        assert_ne!(init, INIT);
        let (events, n) = run(CLAUDE_DISCOVERY_STREAM_JSON, &[&init, TEXT, RESULT]);
        assert!(n.protocol_failed(), "{to}");
        assert_ne!(n.terminal(), Some(TerminalStatus::Success), "{to}");
        assert_eq!(violations(&events)[0], outside, "{to}");
    }
    // The turn's own first record is refused by the developer profile.
    let (_, n) = run(CLAUDE_PRINT_STREAM_JSON, &[INIT, TEXT, RESULT]);
    assert!(n.protocol_failed());
    for name in ["Write", "Edit", "Bash", "WebFetch", "Agent"] {
        let tool = format!(
            r#"{{"type":"assistant","session_id":"s1","parent_tool_use_id":null,"message":{{"content":[{{"type":"tool_use","id":"t1","name":"{name}","input":{{}}}}]}}}}"#
        );
        let (events, n) = run(CLAUDE_DISCOVERY_STREAM_JSON, &[INIT, &tool, RESULT]);
        assert!(n.protocol_failed(), "{name}");
        assert_ne!(n.terminal(), Some(TerminalStatus::Success), "{name}");
        assert_eq!(violations(&events)[0], "tool outside the approved envelope");
    }
    for name in ["Read", "Glob", "Grep"] {
        let tool = format!(
            r#"{{"type":"assistant","session_id":"s1","parent_tool_use_id":null,"message":{{"content":[{{"type":"tool_use","id":"t1","name":"{name}","input":{{}}}}]}}}}"#
        );
        let (_, n) = run(CLAUDE_DISCOVERY_STREAM_JSON, &[INIT, &tool, TEXT, RESULT]);
        assert_eq!(n.terminal(), Some(TerminalStatus::Success), "{name}");
    }
    // Everything the developer profile refuses stays refused.
    for extra in [
        r#"{"type":"system","subtype":"compact_boundary","session_id":"s1"}"#,
        r#"{"type":"assistant","session_id":"s2","parent_tool_use_id":null,"message":{"content":[]}}"#,
        r#"{"type":"assistant","session_id":"s1","parent_tool_use_id":"t9","message":{"content":[]}}"#,
        r#"{"type":"stream_event","session_id":"s1"}"#,
    ] {
        let (_, n) = run(CLAUDE_DISCOVERY_STREAM_JSON, &[INIT, extra, TEXT, RESULT]);
        assert!(n.protocol_failed(), "{extra}");
        assert_ne!(n.terminal(), Some(TerminalStatus::Success), "{extra}");
    }
    let (_, n) = run(CLAUDE_DISCOVERY_STREAM_JSON, &[INIT, TEXT, RESULT, TEXT]);
    assert!(n.protocol_failed());
}

#[test]
fn m2_codex_turn_refuses_a_changed_path_and_keeps_every_other_rule() {
    let thread = r#"{"type":"thread.started","thread_id":"t"}"#;
    let turn = r#"{"type":"turn.started"}"#;
    let done = r#"{"type":"turn.completed","usage":{"input_tokens":5,"cached_input_tokens":0,"output_tokens":6}}"#;
    let message =
        r#"{"type":"item.completed","item":{"id":"i1","type":"agent_message","text":"{}"}}"#;
    let read = r#"{"type":"item.completed","item":{"id":"i2","type":"command_execution","status":"completed"}}"#;
    let (events, n) = run(CODEX_DISCOVERY_JSONL, &[thread, turn, read, message, done]);
    assert_eq!(n.terminal(), Some(TerminalStatus::Success));
    assert_eq!(last_text(&events), Some("{}"));
    for phase in ["started", "updated", "completed"] {
        let change =
            format!(r#"{{"type":"item.{phase}","item":{{"id":"i3","type":"file_change"}}}}"#);
        let (events, n) = run(
            CODEX_DISCOVERY_JSONL,
            &[thread, turn, &change, message, done],
        );
        assert!(n.protocol_failed(), "{phase}");
        assert_ne!(n.terminal(), Some(TerminalStatus::Success), "{phase}");
        assert_eq!(violations(&events)[0], "tool outside the approved envelope");
        // The developer profile accepts the same record: that is the whole difference.
        let (_, n) = run(CODEX_EXEC_JSONL, &[thread, turn, &change, message, done]);
        assert_eq!(n.terminal(), Some(TerminalStatus::Success), "{phase}");
    }
    for extra in [
        r#"{"type":"item.completed","item":{"id":"i4","type":"web_search"}}"#,
        r#"{"type":"item.completed","item":{"id":"i4","type":"mcp_tool_call"}}"#,
        r#"{"type":"item.completed","item":{"id":"i4","type":"unheard_of"}}"#,
        r#"{"type":"unheard.of"}"#,
        turn,
    ] {
        let (_, n) = run(CODEX_DISCOVERY_JSONL, &[thread, turn, extra, message, done]);
        assert!(n.protocol_failed(), "{extra}");
        assert_ne!(n.terminal(), Some(TerminalStatus::Success), "{extra}");
    }
}

const PRD: &str = r#"{"schema_version":1,"title":"Slugify from the shell","goal":"A title can be turned into a slug without writing code.","non_goals":["Packaging"],"actors":[{"id":"A1","name":"Author"}],"use_cases":[{"id":"UC1","title":"Slug one title","actor_ids":["A1"],"preconditions":["A shell is open in the repository"],"main_flow":["The author passes a title","The slug is printed"],"alternate_flows":[{"id":"UC1-A1","title":"Several words","steps":["The words are joined by hyphens"]}],"error_flows":[{"id":"UC1-E1","title":"Nothing to slug","steps":["An error is printed"]}],"postconditions":["Nothing on disk changed"],"acceptance_ids":["AC1"]}],"acceptance_criteria":[{"id":"AC1","statement":"A title given as arguments prints its slug and exits with status 0."}],"nfrs":[],"assumptions":[{"id":"AS1","statement":"Arguments are joined with one space."}],"blockers":[]}"#;

fn discovery(questions: &str, prd: &str) -> String {
    format!(
        r#"{{"schema":"jojo-lead-discovery-v1","summary":"The repository has one stub.\nNothing calls it yet.","questions":{questions},"prd":{prd}}}"#
    )
}

const ASK: &str = r#"[{"prompt":"From arguments, standard input, or both?","blocking":true},{"prompt":"Is packaging in scope?","blocking":false}]"#;

fn field(result: Result<LeadReply, LeadError>) -> (String, &'static str) {
    match result {
        Err(LeadError::Field { path, problem }) => (path, problem),
        other => panic!("not a field error: {other:?}"),
    }
}

#[test]
fn m2_discovery_reply_is_read_strictly() {
    let LeadReply::Discovery(d) = parse_reply(TurnKind::Discover, &discovery(ASK, PRD)).unwrap()
    else {
        panic!("not a discovery");
    };
    assert_eq!(
        d.summary,
        "The repository has one stub.\nNothing calls it yet."
    );
    assert_eq!(d.questions.len(), 2);
    assert_eq!(
        d.questions[0],
        LeadQuestion {
            prompt: "From arguments, standard input, or both?".into(),
            blocking: true,
            step: None
        }
    );
    assert!(!d.questions[1].blocking);
    let prd = d.prd.unwrap();
    assert_eq!(prd.title, "Slugify from the shell");
    assert_eq!(prd.canonical_json(), PRD);
    assert_eq!(prd.validate(), vec![]);

    // Questions without a draft, and a draft without questions, are both replies.
    let LeadReply::Discovery(d) = parse_reply(TurnKind::Discover, &discovery(ASK, "null")).unwrap()
    else {
        panic!("not a discovery");
    };
    assert!(d.prd.is_none());
    assert!(parse_reply(TurnKind::Discover, &discovery("[]", PRD)).is_ok());
    // One surrounding code fence is tolerated; nothing else around the object is.
    let plain = discovery(ASK, "null");
    for wrapped in [
        format!("```json\n{plain}\n```"),
        format!("```\n{plain}\n```\n"),
        format!("\n  {plain}\n"),
    ] {
        assert!(
            parse_reply(TurnKind::Discover, &wrapped).is_ok(),
            "{wrapped}"
        );
    }
    for wrapped in [
        format!("Here is the reply:\n{plain}"),
        format!("{plain}\nLet me know."),
        format!("```json\n{plain}\n```\n```json\n{plain}\n```"),
        format!("{plain}{plain}"),
        String::new(),
        "```json\n```".into(),
    ] {
        assert!(
            matches!(
                parse_reply(TurnKind::Discover, &wrapped),
                Err(LeadError::Json(_))
            ),
            "{wrapped}"
        );
    }
    // A reply of the other schema is not a reply of this turn.
    assert_eq!(
        field(parse_reply(TurnKind::Plan, &plain)),
        ("schema".into(), "not the schema of this turn")
    );
    let cases: Vec<(String, &str, &str)> = vec![
        (
            discovery("[]", "null"),
            "questions",
            "neither a question nor a draft",
        ),
        (
            plain.replace(
                r#""summary":"The repository has one stub.\nNothing calls it yet.","#,
                "",
            ),
            "summary",
            "missing",
        ),
        (
            plain.replace(r#""prd":null"#, r#""prd":null,"approved":true"#),
            "approved",
            "unknown member",
        ),
        (
            plain.replace(r#""blocking":true"#, r#""blocking":"yes""#),
            "questions[0].blocking",
            "not a boolean",
        ),
        (
            plain.replace(r#""blocking":false"#, r#""blocking":false,"step":null"#),
            "questions[1].step",
            "unknown member",
        ),
        (
            plain.replace(
                "Is packaging in scope?",
                "From arguments, standard input, or both?",
            ),
            "questions[1].prompt",
            "repeated question",
        ),
        (
            plain.replace("Is packaging in scope?", " ?? "),
            "questions[1].prompt",
            "empty",
        ),
        (
            plain.replace(
                "Is packaging in scope?",
                &"q".repeat(MAX_QUESTION_BYTES + 1),
            ),
            "questions[1].prompt",
            "too long",
        ),
        (
            plain.replace("one stub.", "one stub.\\u0007"),
            "summary",
            "control character",
        ),
        (
            discovery(
                &format!("[{}]", [r#"{"prompt":"q","blocking":true}"#; 6].join(",")),
                "null",
            ),
            "questions",
            "too many entries",
        ),
        (
            discovery(r#"{"prompt":"q","blocking":true}"#, "null"),
            "questions",
            "not an array",
        ),
        ("[]".into(), "", "not an object"),
    ];
    for (reply, path, problem) in cases {
        assert_eq!(
            field(parse_reply(TurnKind::Discover, &reply)),
            (path.to_owned(), problem),
            "{reply}"
        );
    }
    // The draft is held to the PRD schema itself.
    for broken in [
        PRD.replace(r#""schema_version":1"#, r#""schema_version":2"#),
        PRD.replace(r#""nfrs":[],"#, ""),
        PRD.replace(r#""blockers":[]"#, r#""blockers":[],"approved":true"#),
        r#""a requirements document""#.to_owned(),
    ] {
        assert!(
            matches!(
                parse_reply(TurnKind::Discover, &discovery(ASK, &broken)),
                Err(LeadError::Prd(_))
            ),
            "{broken}"
        );
    }
}

const STEPS: &str = r#"[{"key":"cli","title":"Add the entry point","provider":"Codex","depends_on":[],"write_scope":["slugify.py"],"baseline":["test_slugify.py"],"tests":["unittest"],"command":["/usr/bin/python3","-m","unittest","-q"]},{"key":"docs","title":"Describe the usage","provider":"ClaudeCode","depends_on":[],"write_scope":["README.md"],"baseline":[],"tests":["readme"],"command":["/usr/bin/grep","-q","slugify","README.md"]},{"key":"check","title":"Test the entry point","provider":"ClaudeCode","depends_on":["cli","docs"],"write_scope":["tests/cli"],"baseline":["slugify.py"],"tests":["cli"],"command":["/usr/bin/python3","-m","unittest","discover","-s","tests/cli"]}]"#;

fn plan(steps: &str, questions: &str) -> String {
    format!(
        r##"{{"schema":"jojo-lead-plan-v1","summary":"Three steps.","adr":"# Decision\nOne module, one entry point.","steps":{steps},"questions":{questions}}}"##
    )
}

#[test]
fn m2_plan_reply_is_a_small_graph_with_scopes_and_a_check_per_step() {
    let asked = r#"[{"prompt":"Which shells must the usage text cover?","blocking":true,"step":"docs"},{"prompt":"Is a man page wanted?","blocking":false,"step":null}]"#;
    let LeadReply::Plan(p) = parse_reply(TurnKind::Plan, &plan(STEPS, asked)).unwrap() else {
        panic!("not a plan");
    };
    assert_eq!(p.adr, "# Decision\nOne module, one entry point.");
    let keys: Vec<&str> = p.steps.iter().map(|s| s.key.as_str()).collect();
    assert_eq!(keys, ["cli", "docs", "check"]);
    assert_eq!(p.steps[0].provider, ProviderKind::Codex);
    assert_eq!(p.steps[1].provider, ProviderKind::ClaudeCode);
    assert_eq!(p.steps[2].depends_on, ["cli", "docs"]);
    assert_eq!(p.steps[2].write_scope[0].as_str(), "tests/cli");
    assert_eq!(
        p.steps[0].command,
        ["/usr/bin/python3", "-m", "unittest", "-q"]
    );
    assert_eq!(p.questions[0].step.as_deref(), Some("docs"));
    assert_eq!(p.questions[1].step, None);
    // The draft yields the validation plan the core approves, one entry per identity.
    let ids = [OpaqueId(7), OpaqueId(8), OpaqueId(9)];
    let validation = p.validation(&ids).unwrap();
    assert_eq!(validation.steps.len(), 3);
    assert_eq!(validation.steps[2].step.id, OpaqueId(9));
    assert_eq!(validation.steps[0].baseline[0].as_str(), "test_slugify.py");
    assert!(p.validation(&ids[..2]).is_err());
    assert!(
        p.validation(&[OpaqueId(7), OpaqueId(7), OpaqueId(9)])
            .is_err()
    );

    let one = |from: &str, to: &str| {
        let changed = STEPS.replace(from, to);
        assert_ne!(changed, STEPS, "{from}");
        plan(&changed, "[]")
    };
    let cases: Vec<(String, &str, &str)> = vec![
        (plan("[]", "[]"), "steps", "needs one to six steps"),
        (
            plan(&format!("[{}]", [r#"{"key":"a"}"#; 7].join(",")), "[]"),
            "steps",
            "needs one to six steps",
        ),
        (
            one(r#""key":"docs""#, r#""key":"cli""#),
            "steps[1].key",
            "repeated key",
        ),
        (
            one(r#""key":"docs""#, r#""key":"do cs""#),
            "steps[1].key",
            "malformed key",
        ),
        // A step may depend only on steps listed before it, so the graph has no cycle.
        (
            one(
                r#""key":"cli","title":"Add the entry point","provider":"Codex","depends_on":[]"#,
                r#""key":"cli","title":"Add the entry point","provider":"Codex","depends_on":["check"]"#,
            ),
            "steps[0].depends_on[0]",
            "not the key of an earlier step",
        ),
        (
            one(
                r#""depends_on":["cli","docs"]"#,
                r#""depends_on":["check"]"#,
            ),
            "steps[2].depends_on[0]",
            "not the key of an earlier step",
        ),
        (
            one(
                r#""depends_on":["cli","docs"]"#,
                r#""depends_on":["cli","cli"]"#,
            ),
            "steps[2].depends_on",
            "repeated key",
        ),
        (
            one(r#""provider":"Codex""#, r#""provider":"Gemini""#),
            "steps[0].provider",
            "unknown provider",
        ),
        (
            one(
                r#""write_scope":["slugify.py"]"#,
                r#""write_scope":["../slugify.py"]"#,
            ),
            "steps[0].write_scope[0]",
            "not a normal repository path",
        ),
        (
            one(
                r#""write_scope":["README.md"]"#,
                r#""write_scope":[".git/config"]"#,
            ),
            "steps[1].write_scope[0]",
            "not a normal repository path",
        ),
        (
            one(
                r#""baseline":["test_slugify.py"]"#,
                r#""baseline":["/etc/hosts"]"#,
            ),
            "steps[0].baseline[0]",
            "not a normal repository path",
        ),
        (
            one(r#""tests":["unittest"]"#, r#""tests":["unittest","lint"]"#),
            "steps[0].tests",
            "too many entries",
        ),
        (
            one(
                r#""command":["/usr/bin/grep","-q","slugify","README.md"]"#,
                r#""command":[]"#,
            ),
            "steps[1].command",
            "empty",
        ),
        (
            one(r#""title":"Describe the usage","#, ""),
            "steps[1].title",
            "missing",
        ),
        (
            one(r#""baseline":[],"#, r#""baseline":[],"approved":true,"#),
            "steps[1].approved",
            "unknown member",
        ),
        (
            plan(
                STEPS,
                r#"[{"prompt":"q","blocking":true,"step":"nowhere"}]"#,
            ),
            "questions[0].step",
            "not the key of a step",
        ),
        (
            plan(STEPS, r#"[{"prompt":"q","blocking":true}]"#),
            "questions[0].step",
            "missing",
        ),
        (
            plan(STEPS, "[]").replace(
                r##""adr":"# Decision\nOne module, one entry point.","##,
                r#""adr":"","#,
            ),
            "adr",
            "empty",
        ),
    ];
    for (reply, path, problem) in cases {
        assert_eq!(
            field(parse_reply(TurnKind::Plan, &reply)),
            (path.to_owned(), problem),
            "{reply}"
        );
    }
    // What the approved validation plan refuses, a draft refuses before it is stored.
    for (from, to) in [
        (r#""write_scope":["slugify.py"]"#, r#""write_scope":[]"#),
        (
            r#""baseline":["test_slugify.py"]"#,
            r#""baseline":["slugify.py"]"#,
        ),
        (
            r#""write_scope":["tests/cli"]"#,
            r#""write_scope":["tests","tests/cli"]"#,
        ),
        (r#""tests":["readme"]"#, r#""tests":[]"#),
        (
            r#""command":["/usr/bin/python3","-m","unittest","-q"]"#,
            r#""command":["python3","-m","unittest"]"#,
        ),
        (
            r#""command":["/usr/bin/python3","-m","unittest","-q"]"#,
            r#""command":["/usr/bin/../bin/sh"]"#,
        ),
    ] {
        assert!(
            matches!(
                parse_reply(TurnKind::Plan, &one(from, to)),
                Err(LeadError::Plan(_))
            ),
            "{to}"
        );
    }
}

#[test]
fn m2_prompts_carry_only_the_humans_words_and_stored_content() {
    let list = [
        Exchange {
            prompt: "From arguments, standard input, or both?".into(),
            blocking: true,
            answer: Some("Arguments only.".into()),
        },
        Exchange {
            prompt: "Is packaging in scope?".into(),
            blocking: false,
            answer: None,
        },
    ];
    let objective = "slugify should also be usable from the shell.";
    let first = discover_prompt(objective, &[], None, None);
    for part in [
        objective,
        "You may only read",
        "# Questions and answers so far\nNone.",
        "# Current requirements draft\nNone yet.",
        DISCOVERY_SCHEMA,
        "\"schema_version\":1",
        "At most 5 new questions",
        "\"non_goals\"",
    ] {
        assert!(first.contains(part), "{part}");
    }
    assert!(!first.contains("Note from the product owner"));
    assert!(!first.contains(PLAN_SCHEMA));
    let later = discover_prompt(objective, &list, Some(PRD), Some("Keep it small."));
    for part in [
        "Q1 (blocking): From arguments, standard input, or both?\nAnswer: Arguments only.",
        "Q2 (deferred): Is packaging in scope?\nAnswer: (not answered)",
        "# Note from the product owner for this round\nKeep it small.",
        PRD,
        "Revise it from the answers above",
    ] {
        assert!(later.contains(part), "{part}");
    }
    let planning = plan_prompt(objective, &list, 3, "# Slugify from the shell\n", None);
    for part in [
        objective,
        "Answer: Arguments only.",
        "# Product requirements (approved revision 3)\n# Slugify from the shell\n",
        PLAN_SCHEMA,
        "between 1 and 6 steps",
        "independently mergeable",
        "\"depends_on\"",
        "least confident",
    ] {
        assert!(planning.contains(part), "{part}");
    }
    assert!(!planning.contains(DISCOVERY_SCHEMA));
    // Same inputs, same bytes; and the fixed text leaves room for the largest inputs.
    assert_eq!(
        later,
        discover_prompt(objective, &list, Some(PRD), Some("Keep it small."))
    );
    let fixed = first.len().max(planning.len());
    assert!(
        fixed + 2 * MAX_HUMAN_TEXT_BYTES < MAX_PROMPT_BYTES,
        "{fixed}"
    );
    // The example a prompt shows is itself a reply the reader accepts once filled in.
    let example = r#"{"schema":"jojo-lead-plan-v1","summary":"s","adr":"a","steps":[{"key":"s1","title":"t","provider":"Codex","depends_on":[],"write_scope":["a"],"baseline":["b"],"tests":["c"],"command":["/usr/bin/true","x"]}],"questions":[{"prompt":"q","blocking":true,"step":null}]}"#;
    assert!(parse_reply(TurnKind::Plan, example).is_ok());
}

const PROBE_CLAUDE: &str = r####"{"type":"system","subtype":"init","cwd":"/Users/kaan/JojoDisposable/m2-probes/20261116T161559Z/claude-a/disposable/slugify-sample","session_id":"059e1cac-c6f9-4ee5-9c01-e8d54eee8a1b","tools":["Glob","Grep","Read"],"mcp_servers":[],"model":"claude-opus-5-5","permissionMode":"default","slash_commands":[],"apiKeySource":"none","claude_code_version":"2.1.311","output_style":"default","agents":["claude","Explore","general-purpose","Plan","statusline-setup","web-fetch"],"skills":[],"plugins":[{"name":"cc-plugin-agents-md","path":"builtin","source":"cc-plugin-agents-md@builtin"},{"name":"cc-plugin-telemetry","path":"builtin","source":"cc-plugin-telemetry@builtin"},{"name":"cc-plugin-plugin-authoring","path":"builtin","source":"cc-plugin-plugin-authoring@builtin"}],"capabilities":["interrupt_receipt_v1","interrupt_cancel_queued_v1","interrupt_send_now_v1","interrupt_requeue_taken_v1","msg_lifecycle_v1","request_marker_lists_v1","lifecycle_wakeup_source_v1","sdk_mcp_tools_list_changed","sdk_mcp_manifests","mcp_read_resource_v1","mcp_tool_ui_meta_v1","ui_surface_v1","mcp_call_result_not_for_model_v1"],"analytics_disabled":false,"product_feedback_disabled":false,"uuid":"b66f5984-ecfb-42dc-bdd1-15377d58af1d","original_cwd":"/Users/kaan/JojoDisposable/m2-probes/20261116T161559Z/claude-a/disposable/slugify-sample","additional_directories":[],"fast_mode_state":"off","fast_mode_disabled_reason":"sdk_opt_in_required","per_turn_effort_active":true,"view_mode":"default"}
{"type":"assistant","message":{"model":"claude-opus-5-5","id":"msg_011Ch6uXTPizEkCVEZTbj48R","type":"message","role":"assistant","content":[{"type":"thinking","thinking":"","signature":"CAQSqAUKEAgTGAI4AUIIdGhpbmtpbmcSDDFQ8MnJeO8BAWL4MxoMYD2D9sMRrODBik7NIjA6QEya70VEkxhXO8S0As4bjd3Nv0XXQmk41awh+mza16hE7exsQocU9TkWWObNJJ0qxQRatsClWom1fEJQB8Ef9+7cXYgQRHhiTY67OUOUeqVNxeMzEcYiltIQ7LEiCJIKSUmBTUE9x8l4pEUSlX+QU3EWNWjr3d200fHtt53dmrrlw1XMYhakQUuzENovxR+HaVJtkfBz3wOZFIHdQRILPLj0vBFp0BuKk4HnHWt60fjvoqhBzA7FNwU1i+fEeABaLpTfC7rPf2DRl46IFwQT03+YSYsCE4M9kLuaSTghUnD3zB6EuSyh80nq9FG1wVF+wru3PfC0OvFTd4lyALy84yVA9roFYTWUQfRo1tFjftoZY1iRvrD4qygtX6Hqnp2mNg5PNLLVponu/UDLjWwt5m+HJZB1t4OpfX3cPcBbhn9J3nwzUvh8QYQFFMIVDJGM6Oouo0m+wNddY5clR5LbQSgmeemmYa+Mf+cbzyFBsj57BiwZaSLwsUtpGKaI7artFf40drgb4Z67aOyc5HwxNMVVsnAN8V7Ucyi99bDYLyCfj1jJfzY9crv16n3RuYHabGbS15eKOkM+0PvIu6tTse9D9fiRIuFKZIZdq76KsMbIIm31pIAIEfePbGsbKdK/rEzpTwtVqDueT92pcAVWWl31EzbgnXKMAp9QjG+QySRAyr8XJ0Eqfo9fAU2gYqloV2pHKiXRfce7Kim0WAc+1uJXm11Pe6QpZ4v2VKfdhWiHYmWX6L4xykrYzz7RqDGRtQb/6mcHPnf/GKQduzaEu7KyX9bkvN0vF1p4m1rPcWD+wl5h7KT0mb4raUgIqsFQjcuXyIf15xgB"}],"container":null,"stop_reason":null,"stop_sequence":null,"stop_details":null,"usage":{"input_tokens":2,"cache_creation_input_tokens":1890,"cache_read_input_tokens":3223,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":1890},"output_tokens":8,"service_tier":"standard","inference_geo":"global","speed":"standard"},"input_transformations":[],"diagnostics":null,"context_management":null},"parent_tool_use_id":null,"session_id":"059e1cac-c6f9-4ee5-9c01-e8d54eee8a1b","uuid":"51703d6a-e618-4694-b3b0-acaee9f6c35d","timestamp":"2026-11-16T16:16:08.329Z","request_id":"req_011Ch6uXTEJmfnzTfE69edmE","thinking_duration_ms":847,"thinking_display":"updates"}
{"type":"assistant","message":{"model":"claude-opus-5-5","id":"msg_011Ch6uXTPizEkCVEZTbj48R","type":"message","role":"assistant","content":[{"type":"tool_use","id":"toolu_01Xa48zfGbPS7g6HhEtkptjo","name":"Glob","input":{"pattern":"**/*"},"caller":{"type":"direct"}}],"container":null,"stop_reason":null,"stop_sequence":null,"stop_details":null,"usage":{"input_tokens":2,"cache_creation_input_tokens":1890,"cache_read_input_tokens":3223,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":1890},"output_tokens":8,"service_tier":"standard","inference_geo":"global","speed":"standard"},"input_transformations":[],"diagnostics":null,"context_management":null},"parent_tool_use_id":null,"session_id":"059e1cac-c6f9-4ee5-9c01-e8d54eee8a1b","uuid":"6ad3af8b-3335-459f-a68b-3687cc9ea52e","timestamp":"2026-11-16T16:16:08.334Z","request_id":"req_011Ch6uXTEJmfnzTfE69edmE","wire_tool_inputs":{"toolu_01Xa48zfGbPS7g6HhEtkptjo":{"pattern":"**/*"}}}
{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1794852000,"rateLimitType":"five_hour","overageStatus":"rejected","overageDisabledReason":"out_of_credits","isUsingOverage":false,"unifiedWindows":{"five_hour":{"utilization":0.47,"resetsAt":1794852000,"status":"allowed"},"seven_day":{"utilization":0.56,"resetsAt":1795082400,"status":"allowed"}}},"uuid":"48e1c9f3-7c44-43f6-9d8b-98a76e31e468","session_id":"059e1cac-c6f9-4ee5-9c01-e8d54eee8a1b"}
{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_01Xa48zfGbPS7g6HhEtkptjo","type":"tool_result","content":"slugify.py\ntest_slugify.py\n.git/config\n.git/HEAD\n.git/objects/ac/e972a9dfa47d2a8ca33d17553c278b3ecb8c5e\n.git/objects/6c/b69f1264b24d3e8a22398114f7b6ad05f1eaa6\n.git/objects/42/b4f95e746d3a46928822b0bd5d158354afb376\n.git/COMMIT_EDITMSG\n.git/objects/45/147111b537d1965241c4d2b40e32ad9b740ea9\n.git/refs/heads/main\n.git/logs/HEAD\n.git/logs/refs/heads/main\n.git/index"}]},"parent_tool_use_id":null,"session_id":"059e1cac-c6f9-4ee5-9c01-e8d54eee8a1b","uuid":"27528268-e0a1-4959-9895-745c4977ef77","timestamp":"2026-11-16T16:16:08.370Z","tool_use_result":{"filenames":["slugify.py","test_slugify.py",".git/config",".git/HEAD",".git/objects/ac/e972a9dfa47d2a8ca33d17553c278b3ecb8c5e",".git/objects/6c/b69f1264b24d3e8a22398114f7b6ad05f1eaa6",".git/objects/42/b4f95e746d3a46928822b0bd5d158354afb376",".git/COMMIT_EDITMSG",".git/objects/45/147111b537d1965241c4d2b40e32ad9b740ea9",".git/refs/heads/main",".git/logs/HEAD",".git/logs/refs/heads/main",".git/index"],"durationMs":17,"numFiles":13,"truncated":false,"totalMatches":13,"countIsComplete":true},"tool_result_meta":[{"id":"toolu_01Xa48zfGbPS7g6HhEtkptjo","permission_decision":{"decision":"accept","source":"config","reason_type":"mode"}}]}
{"type":"assistant","message":{"model":"claude-opus-5-5","id":"msg_011Ch6uXaqwyTbUuArkWpp76","type":"message","role":"assistant","content":[{"type":"tool_use","id":"toolu_01JasuFuzm91YKtABXPoaSnZ","name":"Read","input":{"file_path":"/Users/kaan/JojoDisposable/m2-probes/20261116T161559Z/claude-a/disposable/slugify-sample/slugify.py"},"caller":{"type":"direct"}}],"container":null,"stop_reason":null,"stop_sequence":null,"stop_details":null,"usage":{"input_tokens":2,"cache_creation_input_tokens":303,"cache_read_input_tokens":5113,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":303},"output_tokens":16,"service_tier":"standard","inference_geo":"global","speed":"standard"},"input_transformations":[],"diagnostics":null,"context_management":null},"parent_tool_use_id":null,"session_id":"059e1cac-c6f9-4ee5-9c01-e8d54eee8a1b","uuid":"bf42096c-368f-4122-8a61-3ebca595c862","timestamp":"2026-11-16T16:16:09.846Z","request_id":"req_011Ch6uXaizgCFzyqScz8rif","wire_tool_inputs":{"toolu_01JasuFuzm91YKtABXPoaSnZ":{"file_path":"/Users/kaan/JojoDisposable/m2-probes/20261116T161559Z/claude-a/disposable/slugify-sample/slugify.py"}}}
{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_01JasuFuzm91YKtABXPoaSnZ","type":"tool_result","content":"1\t\"\"\"Turn a title into a URL slug.\"\"\"\n2\t\n3\t\n4\tdef slugify(text):\n5\t    \"\"\"Return a lower-case ASCII slug for text: words joined by single hyphens.\"\"\"\n6\t    raise NotImplementedError(\"slugify() is not implemented yet\")\n7\t"}]},"parent_tool_use_id":null,"session_id":"059e1cac-c6f9-4ee5-9c01-e8d54eee8a1b","uuid":"1428017a-4b17-4c5f-a9c7-d15840ecbae6","timestamp":"2026-11-16T16:16:09.868Z","tool_use_result":{"type":"text","file":{"filePath":"/Users/kaan/JojoDisposable/m2-probes/20261116T161559Z/claude-a/disposable/slugify-sample/slugify.py","content":"\"\"\"Turn a title into a URL slug.\"\"\"\n\n\ndef slugify(text):\n    \"\"\"Return a lower-case ASCII slug for text: words joined by single hyphens.\"\"\"\n    raise NotImplementedError(\"slugify() is not implemented yet\")\n","numLines":7,"startLine":1,"totalLines":7}},"tool_result_meta":[{"id":"toolu_01JasuFuzm91YKtABXPoaSnZ","permission_decision":{"decision":"accept","source":"config","reason_type":"mode"}}]}
{"type":"assistant","message":{"model":"claude-opus-5-5","id":"msg_011Ch6uXaqwyTbUuArkWpp76","type":"message","role":"assistant","content":[{"type":"tool_use","id":"toolu_01ETeAZy4UPZbwkbqgZT4Smr","name":"Read","input":{"file_path":"/Users/kaan/JojoDisposable/m2-probes/20261116T161559Z/claude-a/disposable/slugify-sample/test_slugify.py"},"caller":{"type":"direct"}}],"container":null,"stop_reason":null,"stop_sequence":null,"stop_details":null,"usage":{"input_tokens":2,"cache_creation_input_tokens":303,"cache_read_input_tokens":5113,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":303},"output_tokens":16,"service_tier":"standard","inference_geo":"global","speed":"standard"},"input_transformations":[],"diagnostics":null,"context_management":null},"parent_tool_use_id":null,"session_id":"059e1cac-c6f9-4ee5-9c01-e8d54eee8a1b","uuid":"df6e6021-09f7-4958-ba0e-4aee05d754e6","timestamp":"2026-11-16T16:16:10.378Z","request_id":"req_011Ch6uXaizgCFzyqScz8rif","wire_tool_inputs":{"toolu_01ETeAZy4UPZbwkbqgZT4Smr":{"file_path":"/Users/kaan/JojoDisposable/m2-probes/20261116T161559Z/claude-a/disposable/slugify-sample/test_slugify.py"}}}
{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_01ETeAZy4UPZbwkbqgZT4Smr","type":"tool_result","content":"1\t\"\"\"Acceptance tests for slugify(). Baseline: the task must not change this file.\"\"\"\n2\timport unittest\n3\t\n4\tfrom slugify import slugify\n5\t\n6\t\n7\tclass SlugifyTest(unittest.TestCase):\n8\t    def test_lower_cases_and_joins_words_with_hyphens(self):\n9\t        self.assertEqual(slugify(\"Hello World\"), \"hello-world\")\n10\t\n11\t    def test_collapses_punctuation_and_whitespace_into_one_hyphen(self):\n12\t        self.assertEqual(slugify(\"Rock & Roll:  the   sequel!\"), \"rock-roll-the-sequel\")\n13\t\n14\t    def test_strips_leading_and_trailing_separators(self):\n15\t        self.assertEqual(slugify(\"  --Already-Slugged--  \"), \"already-slugged\")\n16\t\n17\t    def test_keeps_digits(self):\n18\t        self.assertEqual(slugify(\"Version 2.0 (beta 3)\"), \"version-2-0-beta-3\")\n19\t\n20\t    def test_removes_accents(self):\n21\t        self.assertEqual(slugify(\"Crème Brûlée à la carte\"), \"creme-brulee-a-la-carte\")\n22\t\n23\t    def test_drops_characters_without_an_ascii_form(self):\n24\t        self.assertEqual(slugify(\"日本語 text\"), \"text\")\n25\t\n26\t    def test_empty_and_separator_only_input_gives_an_empty_slug(self):\n27\t        self.assertEqual(slugify(\"\"), \"\")\n28\t        self.assertEqual(slugify(\" -_- \"), \"\")\n29\t\n30\t    def test_rejects_values_that_are_not_strings(self):\n31\t        with self.assertRaises(TypeError):\n32\t            slugify(None)\n33\t        with self.assertRaises(TypeError):\n34\t            slugify(42)\n35\t\n36\t\n37\tif __name__ == \"__main__\":\n38\t    unittest.main()\n39\t"}]},"parent_tool_use_id":null,"session_id":"059e1cac-c6f9-4ee5-9c01-e8d54eee8a1b","uuid":"00210d70-107b-4a37-96b6-6f84e867f2f2","timestamp":"2026-11-16T16:16:10.407Z","tool_use_result":{"type":"text","file":{"filePath":"/Users/kaan/JojoDisposable/m2-probes/20261116T161559Z/claude-a/disposable/slugify-sample/test_slugify.py","content":"\"\"\"Acceptance tests for slugify(). Baseline: the task must not change this file.\"\"\"\nimport unittest\n\nfrom slugify import slugify\n\n\nclass SlugifyTest(unittest.TestCase):\n    def test_lower_cases_and_joins_words_with_hyphens(self):\n        self.assertEqual(slugify(\"Hello World\"), \"hello-world\")\n\n    def test_collapses_punctuation_and_whitespace_into_one_hyphen(self):\n        self.assertEqual(slugify(\"Rock & Roll:  the   sequel!\"), \"rock-roll-the-sequel\")\n\n    def test_strips_leading_and_trailing_separators(self):\n        self.assertEqual(slugify(\"  --Already-Slugged--  \"), \"already-slugged\")\n\n    def test_keeps_digits(self):\n        self.assertEqual(slugify(\"Version 2.0 (beta 3)\"), \"version-2-0-beta-3\")\n\n    def test_removes_accents(self):\n        self.assertEqual(slugify(\"Crème Brûlée à la carte\"), \"creme-brulee-a-la-carte\")\n\n    def test_drops_characters_without_an_ascii_form(self):\n        self.assertEqual(slugify(\"日本語 text\"), \"text\")\n\n    def test_empty_and_separator_only_input_gives_an_empty_slug(self):\n        self.assertEqual(slugify(\"\"), \"\")\n        self.assertEqual(slugify(\" -_- \"), \"\")\n\n    def test_rejects_values_that_are_not_strings(self):\n        with self.assertRaises(TypeError):\n            slugify(None)\n        with self.assertRaises(TypeError):\n            slugify(42)\n\n\nif __name__ == \"__main__\":\n    unittest.main()\n","numLines":39,"startLine":1,"totalLines":39}},"tool_result_meta":[{"id":"toolu_01ETeAZy4UPZbwkbqgZT4Smr","permission_decision":{"decision":"accept","source":"config","reason_type":"mode"}}]}
{"type":"assistant","message":{"model":"claude-opus-5-5","id":"msg_011Ch6uXjU7bfoFRscdgux14","type":"message","role":"assistant","content":[{"type":"text","text":"{\"wrote_notes\": false, \"files_read\": [\"slugify.py\", \"test_slugify.py\"], \"questions\": [{\"prompt\": \"How should the command-line tool get its input: from arguments (e.g. `slugify \\\"Hello World\\\"`, with several arguments joined into one title or slugified one by one), from stdin one line at a time, or both?\", \"blocking\": true}, {\"prompt\": \"How should it be run (`python slugify.py ...`, `python -m slugify ...`, or an installed `slugify` command, which would need packaging), and what should happen with errors, empty slugs, and exit codes?\", \"blocking\": true}, {\"prompt\": \"The slugify() function itself is still a stub that raises NotImplementedError. Should implementing it to pass the existing acceptance tests be part of this work, or is that tracked separately?\", \"blocking\": false}], \"summary\": \"The repository holds a stub Python slugify() function in slugify.py that raises NotImplementedError, plus unittest acceptance tests in test_slugify.py that define the expected behavior (lowercase, hyphen-joined, accents removed, non-ASCII dropped, TypeError for non-strings).\"}"}],"container":null,"stop_reason":null,"stop_sequence":null,"stop_details":null,"usage":{"input_tokens":2,"cache_creation_input_tokens":1001,"cache_read_input_tokens":5416,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":1001},"output_tokens":384,"service_tier":"standard","inference_geo":"global","speed":"standard"},"input_transformations":[],"diagnostics":null,"context_management":null},"parent_tool_use_id":null,"session_id":"059e1cac-c6f9-4ee5-9c01-e8d54eee8a1b","uuid":"bb97d9f8-d255-4693-8b75-3de74805cc4b","timestamp":"2026-11-16T16:16:14.558Z","request_id":"req_011Ch6uXjHh6w5ydMNrpjNFj"}
{"duration_api_ms":7862,"stop_reason":"end_turn","session_id":"059e1cac-c6f9-4ee5-9c01-e8d54eee8a1b","total_cost_usd":0.0414064,"usage":{"input_tokens":6,"cache_creation_input_tokens":3194,"cache_read_input_tokens":13752,"output_tokens":654,"output_tokens_details":{"thinking_tokens":21},"server_tool_use":{"web_search_requests":0,"web_fetch_requests":0},"service_tier":"standard","cache_creation":{"ephemeral_1h_input_tokens":3194,"ephemeral_5m_input_tokens":0},"inference_geo":"global","iterations":[{"input_tokens":2,"output_tokens":384,"cache_read_input_tokens":5416,"cache_creation_input_tokens":1001,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":1001},"type":"message"}],"speed":"standard","fallback_credit":null},"modelUsage":{"claude-opus-5-5":{"inputTokens":6,"outputTokens":654,"cacheReadInputTokens":13752,"cacheCreationInputTokens":3194,"webSearchRequests":0,"costUSD":0.0414064,"contextWindow":1000000,"maxOutputTokens":128000,"thinkingTokens":21,"cacheCreationEphemeral5mInputTokens":0,"cacheCreationEphemeral1hInputTokens":3194,"canonicalModel":"claude-opus-5-5","provider":"firstParty","costBasis":"list"}},"permission_denials":[],"terminal_reason":"completed","fast_mode_state":"off","fast_mode_disabled_reason":"sdk_opt_in_required","subagent_stats":{"spawned":0,"requested":{"background":0,"foreground":0,"unset":0},"started_in_background":0,"max_depth":0,"spawned_by_subagents":0,"completed":0,"failed":0,"killed":{"parent":0,"user":0,"system":0},"refused":{"depth_limit":0,"concurrency_limit":0,"budget":0},"by_type":{}},"safety_stops":0,"is_error":false,"num_turns":4,"subtype":"success","api_error_status":null,"result":"{\"wrote_notes\": false, \"files_read\": [\"slugify.py\", \"test_slugify.py\"], \"questions\": [{\"prompt\": \"How should the command-line tool get its input: from arguments (e.g. `slugify \\\"Hello World\\\"`, with several arguments joined into one title or slugified one by one), from stdin one line at a time, or both?\", \"blocking\": true}, {\"prompt\": \"How should it be run (`python slugify.py ...`, `python -m slugify ...`, or an installed `slugify` command, which would need packaging), and what should happen with errors, empty slugs, and exit codes?\", \"blocking\": true}, {\"prompt\": \"The slugify() function itself is still a stub that raises NotImplementedError. Should implementing it to pass the existing acceptance tests be part of this work, or is that tracked separately?\", \"blocking\": false}], \"summary\": \"The repository holds a stub Python slugify() function in slugify.py that raises NotImplementedError, plus unittest acceptance tests in test_slugify.py that define the expected behavior (lowercase, hyphen-joined, accents removed, non-ASCII dropped, TypeError for non-strings).\"}","ttft_ms":1711,"type":"result","duration_ms":7955,"uuid":"b5d59afb-0df2-48e2-abc1-3c8650c393c9","ttft_stream_ms":863,"time_to_request_ms":43,"first_content_frame_ms":863,"queued_turn_count":0,"result_index":0,"model_usage_since_last_result":{"claude-opus-5-5":{"inputTokens":6,"outputTokens":654,"cacheReadInputTokens":13752,"cacheCreationInputTokens":3194,"webSearchRequests":0,"costUSD":0.0414064,"contextWindow":1000000,"maxOutputTokens":128000,"thinkingTokens":21,"cacheCreationEphemeral5mInputTokens":0,"cacheCreationEphemeral1hInputTokens":3194,"canonicalModel":"claude-opus-5-5","provider":"firstParty","costBasis":"list"}}}
"####;

const PROBE_CODEX: &str = r####"{"type":"thread.started","thread_id":"01a1e525-502a-7a20-bbdf-539e94e803b7"}
{"type":"turn.started"}
{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"I’ll check whether the notes file can be created, then list and read the repository for the command-line discovery.\n"}}
{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"/bin/zsh -lc \"rg --files --hidden -g '\"'!.git'\"'\"","aggregated_output":"","exit_code":null,"status":"in_progress"}}
{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"/bin/zsh -lc \"rg --files --hidden -g '\"'!.git'\"'\"","aggregated_output":"slugify.py\ntest_slugify.py\n","exit_code":0,"status":"completed"}}
{"type":"item.started","item":{"id":"item_2","type":"command_execution","command":"/bin/zsh -lc 'cat slugify.py test_slugify.py'","aggregated_output":"","exit_code":null,"status":"in_progress"}}
{"type":"item.completed","item":{"id":"item_2","type":"command_execution","command":"/bin/zsh -lc 'cat slugify.py test_slugify.py'","aggregated_output":"\"\"\"Turn a title into a URL slug.\"\"\"\n\n\ndef slugify(text):\n    \"\"\"Return a lower-case ASCII slug for text: words joined by single hyphens.\"\"\"\n    raise NotImplementedError(\"slugify() is not implemented yet\")\n\"\"\"Acceptance tests for slugify(). Baseline: the task must not change this file.\"\"\"\nimport unittest\n\nfrom slugify import slugify\n\n\nclass SlugifyTest(unittest.TestCase):\n    def test_lower_cases_and_joins_words_with_hyphens(self):\n        self.assertEqual(slugify(\"Hello World\"), \"hello-world\")\n\n    def test_collapses_punctuation_and_whitespace_into_one_hyphen(self):\n        self.assertEqual(slugify(\"Rock & Roll:  the   sequel!\"), \"rock-roll-the-sequel\")\n\n    def test_strips_leading_and_trailing_separators(self):\n        self.assertEqual(slugify(\"  --Already-Slugged--  \"), \"already-slugged\")\n\n    def test_keeps_digits(self):\n        self.assertEqual(slugify(\"Version 2.0 (beta 3)\"), \"version-2-0-beta-3\")\n\n    def test_removes_accents(self):\n        self.assertEqual(slugify(\"Crème Brûlée à la carte\"), \"creme-brulee-a-la-carte\")\n\n    def test_drops_characters_without_an_ascii_form(self):\n        self.assertEqual(slugify(\"日本語 text\"), \"text\")\n\n    def test_empty_and_separator_only_input_gives_an_empty_slug(self):\n        self.assertEqual(slugify(\"\"), \"\")\n        self.assertEqual(slugify(\" -_- \"), \"\")\n\n    def test_rejects_values_that_are_not_strings(self):\n        with self.assertRaises(TypeError):\n            slugify(None)\n        with self.assertRaises(TypeError):\n            slugify(42)\n\n\nif __name__ == \"__main__\":\n    unittest.main()\n","exit_code":0,"status":"completed"}}
{"type":"item.completed","item":{"id":"item_3","type":"agent_message","text":"{\"wrote_notes\":false,\"files_read\":[\"slugify.py\",\"test_slugify.py\"],\"questions\":[{\"prompt\":\"How should users invoke the CLI: python slugify.py, an installed slugify command, or both?\",\"blocking\":true},{\"prompt\":\"Should the CLI accept text through command-line arguments, standard input, or both, and should it process one title or multiple lines per invocation?\",\"blocking\":true},{\"prompt\":\"What output and exit status should the CLI produce for missing input, invalid arguments, and input that produces an empty slug?\",\"blocking\":true}],\"summary\":\"The repository contains an unimplemented Python slugify function and unittest acceptance tests defining its expected conversion and type-validation behavior.\"}"}}
{"type":"turn.completed","usage":{"input_tokens":71832,"cached_input_tokens":59392,"cache_write_input_tokens":0,"output_tokens":276,"reasoning_output_tokens":0}}
"####;

//! Follow-ups (protocol 1.14), the pure parts: choices of a lead question, the argument
//! vectors that keep and continue a chat turn's provider session, the continued and the
//! repaired prompt, and the advisory review of a document. No process, I/O, provider
//! binary, credential or network here.
use jojo_domain::claude::{self, CLAUDE_VARIABLES, CLAUDE_VERSION};
use jojo_domain::codex::{self, CODEX_HOME_VARIABLE, CODEX_VERSION};
use jojo_domain::identity::OpaqueId;
use jojo_domain::planner::*;
use jojo_domain::provider::*;
use jojo_domain::review::{DocumentReviewInput, REVIEW_SCHEMA, document_review_prompt};
use std::path::Path;

const REPOSITORY: &str = "/selected/home/disposable/sample";
const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const SESSION: &str = "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";

fn spec(provider: ProviderKind, kind: TurnKind, resume: Option<&str>) -> TurnSpec {
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
        kind,
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
        base_commit: "45147111b537d1965241c4d2b40e32ad9b740ea9".into(),
        raw_output_dir: "/selected/home/data/turns/turn-1".into(),
        environment_allowlist: variables,
        timeout_millis: 1000,
        max_output_bytes: 4096,
        max_frame_bytes: 1024,
        resume: resume.map(str::to_owned),
    }
}

fn discovery(questions: &str) -> String {
    format!(
        r#"{{"schema":"{DISCOVERY_SCHEMA}","summary":"Two choices.","questions":{questions},"prd":null}}"#
    )
}

#[test]
fn f1_a_question_offers_none_or_two_to_four_distinct_choices() {
    let reply = discovery(
        r#"[{"prompt":"Where from?","blocking":true,"choices":["Arguments","Standard input"]},
            {"prompt":"Packaging?","blocking":false,"choices":[]},
            {"prompt":"Windows?","blocking":false}]"#,
    );
    let Ok(LeadReply::Discovery(d)) = parse_reply(TurnKind::Discover, &reply) else {
        panic!("not a discovery reply");
    };
    assert_eq!(d.questions[0].choices, ["Arguments", "Standard input"]);
    assert!(d.questions[1].choices.is_empty());
    // A reply written before choices still reads.
    assert!(d.questions[2].choices.is_empty());
    for (choices, problem) in [
        (r#"["Only"]"#, "one choice is no choice"),
        (r#"["A","A"]"#, "repeated choice"),
        (r#"["A","B","C","D","E"]"#, "too many entries"),
        (r#"["A",""]"#, "empty"),
        (r#""A""#, "not an array"),
    ] {
        let reply = discovery(&format!(
            r#"[{{"prompt":"Where from?","blocking":true,"choices":{choices}}}]"#
        ));
        match parse_reply(TurnKind::Discover, &reply) {
            Err(LeadError::Field { problem: p, .. }) => assert_eq!(p, problem, "{choices}"),
            other => panic!("{choices}: {other:?}"),
        }
    }
    let long = "x".repeat(MAX_CHOICE_BYTES + 1);
    let reply = discovery(&format!(
        r#"[{{"prompt":"Where from?","blocking":true,"choices":["A","{long}"]}}]"#
    ));
    assert!(parse_reply(TurnKind::Discover, &reply).is_err());
    // Both prompts show the member and say when to fill it.
    for prompt in [
        discover_prompt("objective", &[], None, None, None),
        plan_prompt("objective", &[], 1, "requirements", None, None),
    ] {
        assert!(prompt.contains(r#""choices":[]"#), "{prompt}");
        assert!(prompt.contains("give 2 to 4"), "{prompt}");
    }
}

#[test]
fn f1_a_chat_turn_keeps_its_session_and_the_next_one_resumes_it_exactly() {
    let fresh = turn_argv(&spec(ProviderKind::ClaudeCode, TurnKind::Discover, None)).unwrap();
    let mut expected: Vec<String> = claude::discovery_argv();
    expected.retain(|a| a != "--no-session-persistence");
    assert_eq!(fresh, expected);
    let resumed = turn_argv(&spec(
        ProviderKind::ClaudeCode,
        TurnKind::Plan,
        Some(SESSION),
    ))
    .unwrap();
    assert_eq!(resumed[..fresh.len()], fresh[..]);
    assert_eq!(resumed[fresh.len()..], ["--resume", SESSION]);
    // Review and brief turns stay unsaved, exactly as before.
    for kind in [TurnKind::Review, TurnKind::Brief] {
        assert_eq!(
            turn_argv(&spec(ProviderKind::ClaudeCode, kind, None)).unwrap(),
            claude::discovery_argv()
        );
        assert_eq!(
            turn_argv(&spec(ProviderKind::Codex, kind, None)).unwrap(),
            codex::discovery_argv(Path::new(REPOSITORY)).unwrap()
        );
    }
    let fresh = turn_argv(&spec(ProviderKind::Codex, TurnKind::Discover, None)).unwrap();
    assert_eq!(
        fresh.join("\0"),
        format!(
            "exec\0--json\0--color\0never\0--sandbox\0read-only\0--ignore-user-config\0--ignore-rules\0-C\0{REPOSITORY}"
        )
    );
    let resumed = turn_argv(&spec(
        ProviderKind::Codex,
        TurnKind::Discover,
        Some(SESSION),
    ))
    .unwrap();
    assert_eq!(resumed[fresh.len()..], ["resume", SESSION, "-"]);
    // The read-only sandbox and the options stay; nothing prohibited but the one resume.
    for word in codex::PROHIBITED_ARGUMENTS
        .iter()
        .filter(|w| **w != "resume")
    {
        assert!(!resumed.iter().any(|a| a == word), "{word}");
    }
    assert!(resumed.iter().any(|a| a == "read-only"));
    // A session is a plain identity, and only a chat turn continues one.
    assert_eq!(
        check_turn(&spec(ProviderKind::Codex, TurnKind::Plan, Some(SESSION))),
        Ok(())
    );
    for (kind, session) in [
        (TurnKind::Discover, "--resume"),
        (TurnKind::Discover, "a b"),
        (TurnKind::Discover, ""),
        (TurnKind::Discover, &*"a".repeat(65)),
        (TurnKind::Review, SESSION),
        (TurnKind::Brief, SESSION),
    ] {
        assert!(
            check_turn(&spec(ProviderKind::Codex, kind, Some(session))).is_err(),
            "{kind:?} {session:?}"
        );
    }
    assert!(plain_session("fixture-thread"));
}

#[test]
fn f1_a_continued_prompt_asks_to_read_again_only_what_is_needed_and_keeps_the_state() {
    let list = [Exchange {
        prompt: "Where from?".into(),
        blocking: true,
        answer: Some("Arguments".into()),
    }];
    let fresh = discover_prompt(
        "Slugify from the shell.",
        &list,
        None,
        Some("A note."),
        None,
    );
    let continued = continued_prompt(&fresh);
    assert!(continued.starts_with("You are the lead engineer of this repository and you continue"));
    assert!(!continued.contains("Before anything else, skim the README"));
    let body = &fresh[fresh
        .find("\n# Objective from the product owner\n")
        .unwrap()..];
    assert!(continued.ends_with(body));
}

#[test]
fn f1_a_repaired_turn_is_told_why_the_failed_one_failed_before_its_instructions() {
    let fresh = discover_prompt("Slugify from the shell.", &[], None, None, None);
    let repaired = with_repair_section(
        &fresh,
        "the last message is not a reply",
        Some("Mind the JSON."),
    );
    let section = repaired
        .find("# Your previous attempt at this turn failed\nthe last message is not a reply\n")
        .unwrap();
    assert!(section < repaired.find("\n# What to do\n").unwrap());
    assert!(repaired.contains("about the failure:\nMind the JSON.\n"));
    // An empty note adds nothing.
    let quiet = with_repair_section(&fresh, "not spawned", Some("  "));
    assert!(!quiet.contains("about the failure"));
}

#[test]
fn f1_a_document_review_asks_for_the_step_review_verdict_and_cuts_a_long_document() {
    let input = DocumentReviewInput {
        kind: "execution plan",
        revision: 3,
        body: "# Plan\nTwo steps.",
        requirements: Some((2, "# Requirements\nOne goal.")),
    };
    let prompt = document_review_prompt(&input, MAX_PROMPT_BYTES);
    assert!(prompt.contains(REVIEW_SCHEMA));
    assert!(prompt.contains("# Product requirements (approved revision 2)\n# Requirements"));
    assert!(prompt.ends_with("# The execution plan (revision 3)\n# Plan\nTwo steps.\n"));
    let body = "é".repeat(4000);
    let cut = document_review_prompt(
        &DocumentReviewInput {
            body: &body,
            requirements: None,
            ..input
        },
        4096,
    );
    assert!(cut.len() <= 4096);
    assert!(cut.contains("fit here; judge what you can see"));
}

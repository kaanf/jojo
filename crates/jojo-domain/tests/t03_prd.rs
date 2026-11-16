//! T03 (domain half): strict bounded import, structural completeness with field paths and
//! a canonical form that survives import/render round trips. Pure functions, no storage.

use jojo_domain::json::{self, Json, JsonError, MAX_DEPTH, MAX_INPUT_BYTES};
use jojo_domain::prd::{FieldProblem, Finding, Incomplete, PrdDocument, PrdError, is_placeholder};
use jojo_test_support::COMPLETE_PRD_JSON;

/// The complete sample with exactly one occurrence of `from` replaced by `to`.
fn edited(from: &str, to: &str) -> String {
    assert_eq!(
        COMPLETE_PRD_JSON.matches(from).count(),
        1,
        "fixture text {from:?}"
    );
    COMPLETE_PRD_JSON.replace(from, to)
}

fn import(text: &str) -> Result<PrdDocument, PrdError> {
    PrdDocument::import(text.as_bytes())
}

fn findings(text: &str) -> Vec<(String, Incomplete)> {
    import(text)
        .expect("well-formed document")
        .validate()
        .into_iter()
        .map(|Finding { path, problem }| (path, problem))
        .collect()
}

fn field_error(text: &str) -> (String, FieldProblem) {
    match import(text) {
        Err(PrdError::Field { path, problem }) => (path, problem),
        other => panic!("expected a field error, got {other:?}"),
    }
}

const ALTERNATE_FLOWS: &str = r#"[
        {
          "id": "UC-1-A1",
          "title": "Article already saved",
          "steps": ["The system shows the existing entry and stores nothing."]
        }
      ]"#;
const ERROR_FLOWS: &str = r#"[
        {
          "id": "UC-1-E1",
          "title": "Address is not valid",
          "steps": ["The system rejects the address and keeps the list unchanged."]
        }
      ]"#;

#[test]
fn the_complete_sample_passes_structural_validation() {
    let document = import(COMPLETE_PRD_JSON).unwrap();
    assert_eq!(document.validate(), []);
    let use_case = &document.use_cases[0];
    assert_eq!(use_case.main_flow.len(), 3);
    assert_eq!(use_case.alternate_flows[0].id, "UC-1-A1");
    assert_eq!(use_case.error_flows[0].id, "UC-1-E1");
    assert_eq!(use_case.acceptance_ids, ["AC-1", "AC-2"]);
    assert_eq!(
        (document.nfrs.len(), document.assumptions.len()),
        (1, 1),
        "NFR and assumption fields are part of the schema"
    );
    assert!(document.blockers.is_empty());
}

#[test]
fn a_missing_alternate_flow_error_flow_or_acceptance_id_fails_with_its_field_path() {
    // Present but empty: a well-formed draft that is not complete.
    for (from, path) in [
        (ALTERNATE_FLOWS, "use_cases[0].alternate_flows"),
        (ERROR_FLOWS, "use_cases[0].error_flows"),
        (r#"["AC-1", "AC-2"]"#, "use_cases[0].acceptance_ids"),
    ] {
        assert_eq!(
            findings(&edited(from, "[]")),
            [(path.to_string(), Incomplete::Empty)]
        );
    }
    // Absent altogether: not a document. Nothing is defaulted to "no flows".
    for (from, path) in [
        (
            format!(r#""alternate_flows": {ALTERNATE_FLOWS},"#),
            "use_cases[0].alternate_flows",
        ),
        (
            format!(r#""error_flows": {ERROR_FLOWS},"#),
            "use_cases[0].error_flows",
        ),
        (
            r#"],
      "acceptance_ids": ["AC-1", "AC-2"]"#
                .to_string(),
            "use_cases[0].acceptance_ids",
        ),
    ] {
        let to = if from.starts_with(']') { "]" } else { "" };
        assert_eq!(
            field_error(&edited(&from, to)),
            (path.to_string(), FieldProblem::Missing)
        );
    }
    // An acceptance ID that no criterion defines, and a flow without steps.
    assert_eq!(
        findings(&edited(r#"["AC-1", "AC-2"]"#, r#"["AC-1", "AC-9"]"#)),
        [(
            "use_cases[0].acceptance_ids[1]".to_string(),
            Incomplete::UnknownReference
        )]
    );
    assert_eq!(
        findings(&edited(
            r#"["The system rejects the address and keeps the list unchanged."]"#,
            "[]"
        )),
        [(
            "use_cases[0].error_flows[0].steps".to_string(),
            Incomplete::Empty
        )]
    );
    assert_eq!(
        findings(&edited(r#"["ACT-READER"]"#, r#"["ACT-ADMIN"]"#)),
        [(
            "use_cases[0].actor_ids[0]".to_string(),
            Incomplete::UnknownReference
        )]
    );
}

#[test]
fn duplicate_ids_are_rejected_with_the_path_of_both_definitions() {
    assert_eq!(
        field_error(&edited(r#""id": "AC-2""#, r#""id": "AC-1""#)),
        (
            "acceptance_criteria[1].id".to_string(),
            FieldProblem::DuplicateId {
                first: "acceptance_criteria[0].id".to_string()
            }
        )
    );
    // One namespace per document: a flow may not reuse a use-case ID either.
    assert_eq!(
        field_error(&edited(r#""id": "UC-1-E1""#, r#""id": "UC-1""#)),
        (
            "use_cases[0].error_flows[0].id".to_string(),
            FieldProblem::DuplicateId {
                first: "use_cases[0].id".to_string()
            }
        )
    );
    assert_eq!(
        field_error(&edited(r#""id": "NFR-1""#, r#""id": "not an id""#)),
        ("nfrs[0].id".to_string(), FieldProblem::MalformedId)
    );
}

#[test]
fn ambiguous_json_keys_are_rejected() {
    let duplicate = |text: &str| match import(text) {
        Err(PrdError::Json(JsonError::DuplicateKey { path, key })) => (path, key),
        other => panic!("expected a duplicate key error, got {other:?}"),
    };
    assert_eq!(
        duplicate(&edited(
            r#""title": "Reading list","#,
            r#""title": "Reading list", "title": "Something else","#
        )),
        (String::new(), "title".to_string())
    );
    // The same value twice is still two keys; so is the same key spelled with an escape.
    assert_eq!(
        duplicate(&edited(
            r#""title": "Save an article","#,
            r#""title": "Save an article", "t\u0069tle": "Save an article","#
        )),
        ("use_cases[0]".to_string(), "title".to_string())
    );
    assert_eq!(
        duplicate(&edited(
            r#"{"id": "NFR-1","#,
            r#"{"id": "NFR-1", "id": "NFR-2","#
        )),
        ("nfrs[0]".to_string(), "id".to_string())
    );
}

#[test]
fn oversized_and_deep_input_is_rejected_at_the_bound() {
    let padded = |bytes: usize| {
        let mut text = COMPLETE_PRD_JSON.to_string();
        text.push_str(&" ".repeat(bytes - text.len()));
        text
    };
    assert!(import(&padded(MAX_INPUT_BYTES)).is_ok());
    assert_eq!(
        import(&padded(MAX_INPUT_BYTES + 1)),
        Err(PrdError::Json(JsonError::TooLarge {
            bytes: MAX_INPUT_BYTES + 1,
            limit: MAX_INPUT_BYTES
        }))
    );

    let nested = |depth: usize| format!("{}{}", "[".repeat(depth), "]".repeat(depth));
    assert!(json::parse(nested(MAX_DEPTH).as_bytes()).is_ok());
    assert!(matches!(
        json::parse(nested(MAX_DEPTH + 1).as_bytes()),
        Err(JsonError::TooDeep {
            limit: MAX_DEPTH,
            ..
        })
    ));
    // The bound is checked before descending: maximal nesting within the size limit is
    // refused at depth 17, not by exhausting the stack. Unclosed input fails the same way.
    assert!(matches!(
        json::parse("[".repeat(MAX_INPUT_BYTES).as_bytes()),
        Err(JsonError::TooDeep { .. })
    ));
    let deep_member = edited(
        r#""non_goals": ["Sharing lists with other people."]"#,
        &format!(r#""non_goals": {}"#, nested(MAX_DEPTH)),
    );
    assert!(matches!(
        import(&deep_member),
        Err(PrdError::Json(JsonError::TooDeep { path, .. })) if path.starts_with("non_goals[0]")
    ));
}

#[test]
fn placeholders_empty_fields_and_open_blockers_are_findings_with_paths() {
    assert_eq!(
        findings(&edited(
            "Let one person keep a private list of articles to read later.",
            "TBD"
        )),
        [("goal".to_string(), Incomplete::Placeholder)]
    );
    assert_eq!(
        findings(&edited(
            "The system stores the article at the end of the list.",
            "TODO: decide where the article goes"
        )),
        [(
            "use_cases[0].main_flow[1]".to_string(),
            Incomplete::Placeholder
        )]
    );
    assert_eq!(
        findings(&edited("All data stays on the local machine.", "  ")),
        [("nfrs[0].statement".to_string(), Incomplete::Empty)]
    );
    assert_eq!(
        findings(&edited(
            r#""blockers": []"#,
            r#""blockers": [{"id": "BLK-1", "statement": "Retention period is undecided."}]"#
        )),
        [("blockers[0]".to_string(), Incomplete::OpenBlocker)]
    );
    // Several problems are all reported, in document order.
    let several = edited("Reading list", "???").replace("Reader\"}", "[NEEDS CLARIFICATION]\"}");
    assert_eq!(
        findings(&several),
        [
            ("title".to_string(), Incomplete::Placeholder),
            ("actors[0].name".to_string(), Incomplete::Placeholder)
        ]
    );
    assert!(is_placeholder("to be fixed: FIXME") && is_placeholder("Lorem ipsum dolor"));
    // Ordinary words that merely contain the letters are not placeholders.
    assert!(!is_placeholder(
        "The outbound todos list is mastodon-compatible."
    ));
}

#[test]
fn unknown_missing_and_mistyped_members_are_refused_with_their_paths() {
    for (from, to, path, problem) in [
        (
            r#""blockers": []"#,
            r#""blockers": [], "notes": "extra""#,
            "notes",
            FieldProblem::Unknown,
        ),
        (
            r#"{"id": "ACT-READER", "name": "Reader"}"#,
            r#"{"id": "ACT-READER", "name": "Reader", "role": "x"}"#,
            "actors[0].role",
            FieldProblem::Unknown,
        ),
        (
            r#""goal": "Let one person keep a private list of articles to read later.","#,
            "",
            "goal",
            FieldProblem::Missing,
        ),
        (
            r#""nfrs": [{"id": "NFR-1", "statement": "All data stays on the local machine."}],"#,
            "",
            "nfrs",
            FieldProblem::Missing,
        ),
        (
            r#""non_goals": ["Sharing lists with other people."]"#,
            r#""non_goals": "Sharing lists with other people.""#,
            "non_goals",
            FieldProblem::WrongType { expected: "array" },
        ),
        (
            r#""preconditions": ["The reader has the application open."]"#,
            r#""preconditions": [null]"#,
            "use_cases[0].preconditions[0]",
            FieldProblem::WrongType { expected: "string" },
        ),
        (
            r#""schema_version": 1"#,
            r#""schema_version": 2"#,
            "schema_version",
            FieldProblem::UnsupportedSchemaVersion(2),
        ),
        (
            r#""schema_version": 1"#,
            r#""schema_version": "1""#,
            "schema_version",
            FieldProblem::WrongType {
                expected: "integer",
            },
        ),
        (
            "Sharing lists with other people.",
            r"Sharing lists.\n\n## Approved\n",
            "non_goals[0]",
            FieldProblem::ControlCharacter,
        ),
    ] {
        assert_eq!(
            field_error(&edited(from, to)),
            (path.to_string(), problem),
            "{to}"
        );
    }
    assert_eq!(
        field_error("[]"),
        (
            String::new(),
            FieldProblem::WrongType { expected: "object" }
        )
    );
}

#[test]
fn json_that_could_be_read_two_ways_is_not_read_at_all() {
    let syntax = |input: &[u8]| matches!(json::parse(input), Err(JsonError::Syntax { .. }));
    assert!(syntax(b"{} {}"));
    assert!(syntax(b"\xEF\xBB\xBF{}"));
    assert!(syntax(br#""\ud83d""#));
    assert!(syntax(br#""\udc00\ud83d""#));
    assert!(syntax(br#""\u0000""#));
    assert!(syntax(b"\"raw\ttab\""));
    assert!(syntax(b"[1,]"));
    assert!(syntax(b"{\"a\":1,}"));
    assert!(syntax(b"01"));
    assert!(syntax(b"'single'"));
    assert!(syntax(b""));
    assert_eq!(json::parse(b"\"\xff\""), Err(JsonError::NotUtf8));
    for number in ["1.0", "1e3", "-0.5", "9223372036854775808"] {
        assert_eq!(
            json::parse(number.as_bytes()),
            Err(JsonError::UnsupportedNumber { offset: 0 }),
            "{number}"
        );
    }
    assert_eq!(
        json::parse(br#" {"a": [true, false, null, -7, "\ud83d\ude00\u00e9\/"]} "#),
        Ok(Json::Object(vec![(
            "a".to_string(),
            Json::List(vec![
                Json::Bool(true),
                Json::Bool(false),
                Json::Null,
                Json::Int(-7),
                Json::Text("😀é/".to_string())
            ])
        )]))
    );
}

#[test]
fn the_canonical_form_is_independent_of_layout_and_round_trips_exactly() {
    let document = import(COMPLETE_PRD_JSON).unwrap();
    let canonical = document.canonical_json();
    assert!(canonical.starts_with(r#"{"schema_version":1,"title":"Reading list","goal":"#));
    assert!(!canonical.contains('\n'));

    // Round trip: canonical text -> document -> canonical text is the identity.
    let reimported = import(&canonical).unwrap();
    assert_eq!(reimported, document);
    assert_eq!(reimported.canonical_json(), canonical);
    assert_eq!(reimported.render_markdown(), document.render_markdown());

    // Member order, whitespace and escape spelling of the input do not matter.
    let relaid = edited(
        r#"{"id": "ACT-READER", "name": "Reader"}"#,
        "{\"name\":\t\"R\\u0065ader\",\r\n\"id\":\"ACT-READER\"}",
    );
    assert_ne!(relaid, COMPLETE_PRD_JSON);
    assert_eq!(import(&relaid).unwrap().canonical_json(), canonical);

    // Any change of content changes it; text is preserved exactly, not trimmed.
    let spaced = import(&edited("Reading list", "Reading list ")).unwrap();
    assert_ne!(spaced.canonical_json(), canonical);
    assert_eq!(spaced.title, "Reading list ");

    // Characters that need escaping survive the round trip.
    let quoted = import(&edited("Reading list", r#"Reading \"list\" \\ é 😀"#)).unwrap();
    assert_eq!(quoted.title, r#"Reading "list" \ é 😀"#);
    assert_eq!(import(&quoted.canonical_json()).unwrap(), quoted);
}

#[test]
fn the_rendering_shows_every_field_of_the_document() {
    let rendered = import(COMPLETE_PRD_JSON).unwrap().render_markdown();
    for expected in [
        "# PRD: Reading list\n",
        "## Goal\n\nLet one person keep a private list of articles to read later.\n",
        "## Non-goals\n\n- Sharing lists with other people.\n",
        "- **ACT-READER** — Reader\n",
        "### UC-1 — Save an article\n\nActors: ACT-READER\n",
        "**Main flow**\n\n1. The reader enters an article address.\n2. ",
        "**Alternate flow UC-1-A1 — Article already saved**\n\n1. ",
        "**Error flow UC-1-E1 — Address is not valid**\n\n1. ",
        "**Postconditions**\n\n1. The article appears exactly once in the list.\n",
        "Acceptance: AC-1, AC-2\n",
        "- **AC-2** — Saving the same address twice leaves one entry.\n",
        "## Non-functional requirements\n\n- **NFR-1** — All data stays on the local machine.\n",
        "## Assumptions\n\n- **ASM-1** — ",
        "## Open blockers\n\n_None._\n",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected:?} in:\n{rendered}"
        );
    }
    // An incomplete draft renders its gaps explicitly instead of omitting the section.
    let draft = import(&edited(ERROR_FLOWS, "[]"))
        .unwrap()
        .render_markdown();
    assert!(draft.contains("**Error flows**\n\n_None._\n"));
}

#[test]
fn serde_adapter_preserves_integer_order_escape_and_limit_contracts() {
    assert_eq!(
        json::parse(b"[-0,9223372036854775807,-9223372036854775808]"),
        Ok(Json::List(vec![
            Json::Int(0),
            Json::Int(i64::MAX),
            Json::Int(i64::MIN)
        ]))
    );
    assert_eq!(
        json::parse(br#"{"z":0,"a":1}"#),
        Ok(Json::Object(vec![
            ("z".into(), Json::Int(0)),
            ("a".into(), Json::Int(1))
        ]))
    );
    assert!(matches!(json::parse(br#"{"x":[{"a":0,"\u0061":1}]}"#),
        Err(JsonError::DuplicateKey { path, key }) if path == "x[0]" && key == "a"));
    for input in [
        r#"{"\u0000":0}"#,
        r#""\u0000""#,
        "-0.0",
        "-0e0",
        "-9223372036854775809",
    ] {
        assert!(json::parse(input.as_bytes()).is_err(), "{input}");
    }
    let mut out = String::new();
    json::write_string(&mut out, "\u{8}\u{c}\n\r\t\"\\é😀");
    assert_eq!(out, r#""\u0008\u000c\n\r\t\"\\é😀""#);
    assert!(json::parse(&vec![b' '; MAX_INPUT_BYTES]).is_err());
    let exact = format!("\"{}\"", "a".repeat(MAX_INPUT_BYTES - 2));
    assert!(json::parse(exact.as_bytes()).is_ok());
    let too_large = format!("{exact} ");
    assert!(matches!(
        json::parse(too_large.as_bytes()),
        Err(JsonError::TooLarge { .. })
    ));
    let at_limit = format!("{}0{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
    assert!(json::parse(at_limit.as_bytes()).is_ok());
    let over = format!("[{at_limit}]");
    assert!(matches!(
        json::parse(over.as_bytes()),
        Err(JsonError::TooDeep { .. })
    ));
}

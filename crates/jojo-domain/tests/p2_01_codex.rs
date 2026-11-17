//! P2-01 pure Codex contract: argument allowlist, launch-envelope amendment and the
//! production stream profile. Synthetic records written for these tests; no process, I/O,
//! provider binary, credential or network.
use jojo_domain::codex::*;
use jojo_domain::stream::{CLAUDE_STREAM_JSON, CODEX_EXEC_JSON, LineFramer, StreamNormalizer};
use jojo_domain::{attempt::AttemptBindings, identity::*, provider::*};
use std::path::Path;

const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const WORKTREE: &str = "/owned/worktrees/attempt-1";
const HOME: &str = "/selected/codex-home";

fn spec() -> LaunchSpec {
    LaunchSpec {
        attempt: AttemptId(OpaqueId(1)),
        bindings: AttemptBindings {
            feature_id: FeatureId(OpaqueId(1)),
            plan_id: PlanId(OpaqueId(1)),
            plan_revision: Revision(1),
            step_id: StepId(OpaqueId(1)),
            step_revision: Revision(1),
            repository_id: RepositoryId(OpaqueId(1)),
            worktree_id: WorktreeId(OpaqueId(1)),
        },
        engine_epoch: OpaqueId(7),
        dispatch_generation: 1,
        fingerprint: ProviderFingerprint {
            provider: ProviderKind::Codex,
            executable: "/synthetic/fixture".into(),
            version: CODEX_VERSION.into(),
            schema: PROVIDER_SCHEMA,
            parser: CODEX_EXEC_JSONL.into(),
            configuration: configuration(DIGEST).unwrap(),
        },
        source: SourceBinding {
            common_dir: "/owned/repo.git".into(),
            base_commit: "commit".into(),
            candidate_source_vector: "vector-digest".into(),
            instruction_digest: "instructions-digest".into(),
        },
        prompt_digest: "prompt-digest".into(),
        prompt: b"synthetic task".to_vec(),
        continuation: None,
        role: Role::Developer,
        permissions: PermissionEnvelope {
            approved_digest: "permission-digest".into(),
            tools: vec![],
            filesystem_roots: vec![WORKTREE.into()],
            network: false,
            hooks: false,
            background: false,
        },
        cwd: WORKTREE.into(),
        environment_allowlist: vec![CODEX_HOME_VARIABLE.into()],
        config_references: vec![],
        timeout_millis: 1000,
        max_output_bytes: 4096,
        max_frame_bytes: 1024,
        raw_output_dir: "/owned/output".into(),
        approval_ids: [OpaqueId(10), OpaqueId(11), OpaqueId(12)],
        required_outputs: vec!["candidate".into()],
        validation_plan: "validation-digest".into(),
    }
}

fn changed(edit: impl FnOnce(&mut LaunchSpec)) -> LaunchSpec {
    let mut s = spec();
    edit(&mut s);
    s
}

#[test]
fn p2_01_golden_argv_is_byte_exact() {
    let argv = exec_argv(Path::new(WORKTREE)).unwrap();
    assert_eq!(
        argv,
        [
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
            "/owned/worktrees/attempt-1",
        ]
    );
    assert_eq!(
        argv.join("\0").as_bytes(),
        b"exec\0--json\0--color\0never\0--sandbox\0workspace-write\0--ephemeral\0--ignore-user-config\0--ignore-rules\0-C\0/owned/worktrees/attempt-1"
    );
    assert_eq!(check_exec_argv(&argv, Path::new(WORKTREE)), Ok(()));
    assert_eq!(check_spec(&spec()), Ok(()));
}

#[test]
fn p2_01_prohibited_arguments_are_unrepresentable_and_refused_everywhere() {
    let worktree = Path::new(WORKTREE);
    let golden = exec_argv(worktree).unwrap();
    for prohibited in PROHIBITED_ARGUMENTS {
        assert!(
            !golden.iter().any(|a| a == prohibited),
            "{prohibited} is in the golden vector"
        );
        // Inserted at any position, or replacing any element, the vector is refused.
        for at in 0..=golden.len() {
            let mut inserted = golden.clone();
            inserted.insert(at, prohibited.to_owned());
            assert!(
                check_exec_argv(&inserted, worktree).is_err(),
                "{prohibited}@{at}"
            );
        }
        for at in 0..golden.len() {
            let mut replaced = golden.clone();
            replaced[at] = prohibited.to_owned();
            assert!(
                check_exec_argv(&replaced, worktree).is_err(),
                "{prohibited}={at}"
            );
        }
        // The only free input is the worktree, and an option is not an absolute path.
        assert!(exec_argv(Path::new(prohibited)).is_err());
    }
    for extra in [
        "-",
        "resume",
        "--sandbox=danger-full-access",
        "-cmodel=x",
        "prompt",
    ] {
        let mut longer = golden.clone();
        longer.push(extra.into());
        assert!(check_exec_argv(&longer, worktree).is_err(), "{extra}");
    }
    assert!(check_exec_argv(&golden[..golden.len() - 1], worktree).is_err());
    assert!(check_exec_argv(&golden, Path::new("/owned/worktrees/attempt-2")).is_err());
    assert!(check_exec_argv(&[], worktree).is_err());
    for bad in [
        "relative/worktree",
        "/owned/../etc",
        "/owned/./worktree",
        "/owned//worktree",
        "/owned/worktree/",
        "",
        "-C",
    ] {
        assert_eq!(
            exec_argv(Path::new(bad)),
            Err(ProviderError::InvalidSpec),
            "{bad:?}"
        );
    }
}

#[test]
fn p2_01_envelope_amendment_is_scoped_to_the_production_codex_profile() {
    assert_eq!(spec().validate(), Ok(()));
    // The original rule is unchanged for Claude and for the synthetic Codex fixture.
    for (provider, parser) in [
        (ProviderKind::ClaudeCode, CLAUDE_STREAM_JSON),
        (ProviderKind::Codex, CODEX_EXEC_JSON),
        (ProviderKind::ClaudeCode, CODEX_EXEC_JSONL),
        (ProviderKind::Codex, CLAUDE_STREAM_JSON),
        (ProviderKind::Codex, "synthetic-parser-v1"),
    ] {
        let s = changed(|s| {
            s.fingerprint.provider = provider;
            s.fingerprint.parser = parser.into();
        });
        assert_eq!(
            s.validate(),
            Err(ProviderError::InvalidSpec),
            "{provider:?} {parser}"
        );
        let original = changed(|s| {
            s.fingerprint.provider = provider;
            s.fingerprint.parser = parser.into();
            s.environment_allowlist = vec!["LANG".into(), "LC_ALL".into(), "TZ".into()];
        });
        assert_eq!(original.validate(), Ok(()));
    }
    // Exactly one new name. No credential, routing, home or path variable.
    for name in [
        "HOME",
        "PATH",
        "OPENAI_API_KEY",
        "OPENAI_BASE_URL",
        "OPENAI_ORGANIZATION",
        "CODEX_API_KEY",
        "CODEX_ACCESS_TOKEN",
        "CODEX_SANDBOX",
        "codex_home",
        "CODEX_HOME ",
        "CODEX_HOME=/x",
        "ANTHROPIC_API_KEY",
        "HTTPS_PROXY",
        "NODE_OPTIONS",
        "DYLD_INSERT_LIBRARIES",
        "",
    ] {
        let s = changed(|s| s.environment_allowlist.push(name.into()));
        assert_eq!(s.validate(), Err(ProviderError::InvalidSpec), "{name:?}");
    }
    // Agent tool network, hooks and background permissions stay denied for every profile.
    for deny in [
        (|s: &mut LaunchSpec| s.permissions.network = true) as fn(&mut LaunchSpec),
        |s| s.permissions.hooks = true,
        |s| s.permissions.background = true,
    ] {
        assert_eq!(changed(deny).validate(), Err(ProviderError::InvalidSpec));
    }
}

#[test]
fn p2_01_profile_rejects_everything_one_exec_run_cannot_be_held_to() {
    use ProviderError::*;
    let continuation = Continuation {
        predecessor: AttemptId(OpaqueId(9)),
        exact: SessionRef {
            provider: ProviderKind::Codex,
            session: "thread".into(),
            turn: "turn-1".into(),
        },
        previous_cwd: WORKTREE.into(),
        reviewed_plan: "plan".into(),
    };
    let cases: Vec<(&str, LaunchSpec, ProviderError)> = vec![
        (
            "continuation",
            changed(|s| s.continuation = Some(continuation)),
            UnsupportedContinuation,
        ),
        (
            "validator role",
            changed(|s| s.role = Role::Validator),
            UnsupportedPermission,
        ),
        (
            "tool allowlist",
            changed(|s| s.permissions.tools = vec!["shell".into()]),
            UnsupportedPermission,
        ),
        (
            "second root",
            changed(|s| s.permissions.filesystem_roots.push("/owned/other".into())),
            UnsupportedPermission,
        ),
        (
            "root is not the worktree",
            changed(|s| s.permissions.filesystem_roots = vec!["/owned".into()]),
            UnsupportedPermission,
        ),
        (
            "version text of two lines",
            changed(|s| s.fingerprint.version = "codex-cli\n0.170.0".into()),
            FingerprintMismatch,
        ),
        (
            "synthetic parser",
            changed(|s| {
                s.fingerprint.parser = CODEX_EXEC_JSON.into();
                s.environment_allowlist.clear();
            }),
            FingerprintMismatch,
        ),
        (
            "claude",
            changed(|s| {
                s.fingerprint.provider = ProviderKind::ClaudeCode;
                s.fingerprint.parser = CLAUDE_STREAM_JSON.into();
                s.environment_allowlist.clear();
            }),
            FingerprintMismatch,
        ),
        (
            "no digest",
            changed(|s| s.fingerprint.configuration = "config-digest".into()),
            FingerprintMismatch,
        ),
        (
            "config reference",
            changed(|s| s.config_references = vec!["profile".into()]),
            InvalidSpec,
        ),
        (
            "no home variable",
            changed(|s| s.environment_allowlist.clear()),
            InvalidSpec,
        ),
        (
            "extra variable",
            changed(|s| s.environment_allowlist.push("LANG".into())),
            InvalidSpec,
        ),
        (
            "agent network",
            changed(|s| s.permissions.network = true),
            InvalidSpec,
        ),
        (
            "relative worktree",
            changed(|s| {
                s.cwd = "worktree".into();
                s.permissions.filesystem_roots = vec!["worktree".into()];
            }),
            InvalidSpec,
        ),
    ];
    for (name, case, expected) in cases {
        assert_eq!(check_spec(&case), Err(expected), "{name}");
    }
    // D3: another release is recorded with the run, not refused.
    let release = changed(|s| s.fingerprint.version = "codex-cli 0.170.0".into());
    assert_eq!(check_spec(&release), Ok(()));
    for bad in ["", "sha256=", &DIGEST[1..], &DIGEST.to_uppercase(), "zz"] {
        assert_eq!(configuration(bad), None, "{bad:?}");
    }
    let text = configuration(DIGEST).unwrap();
    assert_eq!(text, format!("jojo.codex.exec/1;sha256={DIGEST}"));
    assert_eq!(configured_digest(&text), Some(DIGEST));
    assert_eq!(configured_digest(&format!("{text};extra=1")), None);
}

#[test]
fn p2_01_child_environment_is_exactly_one_selected_home_outside_the_run() {
    let s = spec();
    let pair = |name: &str, value: &str| (name.to_owned(), value.to_owned());
    assert_eq!(
        check_environment(&s, &[pair(CODEX_HOME_VARIABLE, HOME)]),
        Ok(Path::new(HOME))
    );
    let refused: Vec<Vec<(String, String)>> = vec![
        vec![],
        vec![pair(CODEX_HOME_VARIABLE, HOME), pair("LANG", "C")],
        vec![
            pair(CODEX_HOME_VARIABLE, HOME),
            pair(CODEX_HOME_VARIABLE, HOME),
        ],
        vec![pair("HOME", HOME)],
        vec![pair("OPENAI_API_KEY", "not-a-real-key")],
        vec![pair(CODEX_HOME_VARIABLE, "relative/home")],
        vec![pair(CODEX_HOME_VARIABLE, "/selected/../etc")],
        vec![pair(CODEX_HOME_VARIABLE, "")],
        vec![pair(CODEX_HOME_VARIABLE, "/")],
        // Inside the worktree the agent could rewrite it; around it, it would be writable.
        vec![pair(CODEX_HOME_VARIABLE, "/owned/worktrees/attempt-1/home")],
        vec![pair(CODEX_HOME_VARIABLE, "/owned/worktrees")],
        vec![pair(CODEX_HOME_VARIABLE, "/owned/output/home")],
        vec![pair(CODEX_HOME_VARIABLE, "/owned/repo.git")],
    ];
    for environment in refused {
        assert_eq!(
            check_environment(&s, &environment),
            Err(ProviderError::InvalidSpec),
            "{environment:?}"
        );
    }
}

#[test]
fn p2_01_every_unverified_capability_is_recorded_unknown() {
    let c = unverified_capabilities();
    assert!(c.evidence.contains(CODEX_VERSION) && c.evidence.contains("never executed"));
    for (name, support) in [
        ("structured_events", c.structured_events),
        ("explicit_sessions", c.explicit_sessions),
        ("exact_continuation", c.exact_continuation),
        ("incremental_input", c.incremental_input),
        ("question_ids", c.question_ids),
        ("permission_response", c.permission_response),
        ("turn_interrupt", c.turn_interrupt),
        ("process_tree", c.process_tree),
        ("continuation_cwd_change", c.continuation_cwd_change),
        ("auth_status", c.auth_status),
        ("workspace_policy", c.workspace_policy),
    ] {
        assert_eq!(support, Support::Unknown, "{name}");
    }
}

// ---- production stream profile ---------------------------------------------------------

const THREAD: &str = r#"{"type":"thread.started","thread_id":"th-1"}"#;
const TURN: &str = r#"{"type":"turn.started"}"#;
const DONE: &str = r#"{"type":"turn.completed","usage":{"input_tokens":5,"cached_input_tokens":1,"output_tokens":6}}"#;
const MESSAGE: &str =
    r#"{"type":"item.completed","item":{"id":"i-2","type":"agent_message","text":"héllo 🙂"}}"#;

fn run(chunks: &[&[u8]]) -> (Vec<EventPayload>, StreamNormalizer) {
    let mut framer = LineFramer::new(1024);
    let mut n = StreamNormalizer::for_parser(CODEX_EXEC_JSONL).unwrap();
    let mut frames = Vec::new();
    for c in chunks {
        frames.extend(framer.push(c));
    }
    frames.extend(framer.finish());
    let out = frames.iter().flat_map(|f| n.frame(f)).collect();
    (out, n)
}

fn lines(records: &[&str]) -> (Vec<EventPayload>, StreamNormalizer) {
    let text: String = records.iter().map(|r| format!("{r}\n")).collect();
    run(&[text.as_bytes()])
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

/// The stream latched a protocol failure and never normalized a successful terminal.
fn fails_closed(records: &[&str], why: &str) {
    let (events, n) = lines(records);
    assert!(n.protocol_failed(), "{records:?}");
    assert_ne!(n.terminal(), Some(TerminalStatus::Success), "{records:?}");
    assert!(violations(&events).contains(&why), "{why}: {events:?}");
    assert!(!events.iter().any(|e| matches!(
        e,
        EventPayload::ProviderTerminal {
            status: TerminalStatus::Success,
            ..
        }
    )));
}

#[test]
fn p2_01_documented_stream_normalizes_to_one_successful_turn() {
    let (events, n) = lines(&[
        THREAD,
        TURN,
        r#"{"type":"item.started","item":{"id":"i-1","type":"command_execution","command":"true"}}"#,
        r#"{"type":"item.updated","item":{"id":"i-1","type":"command_execution"}}"#,
        r#"{"type":"item.completed","item":{"id":"i-1","type":"command_execution","exit_code":0}}"#,
        r#"{"type":"item.completed","item":{"id":"i-3","type":"reasoning","text":"…"}}"#,
        r#"{"type":"item.started","item":{"id":"i-4","type":"file_change"}}"#,
        r#"{"type":"item.completed","item":{"id":"i-4","type":"file_change"}}"#,
        MESSAGE,
        "",
        DONE,
    ]);
    use EventPayload::*;
    assert_eq!(
        events,
        vec![
            SessionIdentified(SessionRef {
                provider: ProviderKind::Codex,
                session: "th-1".into(),
                turn: "turn-1".into()
            }),
            TurnStarted {
                turn: "turn-1".into()
            },
            ToolStarted { id: "i-1".into() },
            Informational("item.updated:command_execution".into()),
            ToolFinished { id: "i-1".into() },
            Informational("item.completed:reasoning".into()),
            ToolStarted { id: "i-4".into() },
            ToolFinished { id: "i-4".into() },
            TextDelta("héllo 🙂".into()),
            UsageObserved { units: 11 },
            ProviderTerminal {
                turn: "turn-1".into(),
                status: TerminalStatus::Success,
                raw_status: "turn.completed".into()
            },
        ]
    );
    assert_eq!(n.provider(), ProviderKind::Codex);
    assert!(!n.protocol_failed());
    assert_eq!(n.terminal(), Some(TerminalStatus::Success));
}

/// The stdout of the one live `codex exec --json` run (P2-05, codex-cli 0.169.0), with the
/// thread id and the operator's paths replaced. Everything else is as Codex wrote it.
const LIVE_0_169_0: &str = concat!(
    r##"{"type":"thread.started","thread_id":"00000000-0000-7000-8000-000000000001"}"##,
    "\n",
    r##"{"type":"turn.started"}"##,
    "\n",
    r##"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"I’ll inspect the implementation and tests, update only `slugify.py`, and run the required test command."}}"##,
    "\n",
    r##"{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"/bin/zsh -lc \"pwd; rg --files -g 'AGENTS.md' -g 'slugify.py' -g 'test_slugify.py'; git status --short; cat slugify.py; cat test_slugify.py\"","aggregated_output":"","exit_code":null,"status":"in_progress"}}"##,
    "\n",
    r##"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"/bin/zsh -lc \"pwd; rg --files -g 'AGENTS.md' -g 'slugify.py' -g 'test_slugify.py'; git status --short; cat slugify.py; cat test_slugify.py\"","aggregated_output":"/owned/worktrees/attempt-1\nslugify.py\ntest_slugify.py\n\"\"\"Turn a title into a URL slug.\"\"\"\n\n\ndef slugify(text):\n    \"\"\"Return a lower-case ASCII slug for text: words joined by single hyphens.\"\"\"\n    raise NotImplementedError(\"slugify() is not implemented yet\")\n\"\"\"Acceptance tests for slugify(). Baseline: the task must not change this file.\"\"\"\nimport unittest\n\nfrom slugify import slugify\n\n\nclass SlugifyTest(unittest.TestCase):\n    def test_lower_cases_and_joins_words_with_hyphens(self):\n        self.assertEqual(slugify(\"Hello World\"), \"hello-world\")\n\n    def test_collapses_punctuation_and_whitespace_into_one_hyphen(self):\n        self.assertEqual(slugify(\"Rock & Roll:  the   sequel!\"), \"rock-roll-the-sequel\")\n\n    def test_strips_leading_and_trailing_separators(self):\n        self.assertEqual(slugify(\"  --Already-Slugged--  \"), \"already-slugged\")\n\n    def test_keeps_digits(self):\n        self.assertEqual(slugify(\"Version 2.0 (beta 3)\"), \"version-2-0-beta-3\")\n\n    def test_removes_accents(self):\n        self.assertEqual(slugify(\"Crème Brûlée à la carte\"), \"creme-brulee-a-la-carte\")\n\n    def test_drops_characters_without_an_ascii_form(self):\n        self.assertEqual(slugify(\"日本語 text\"), \"text\")\n\n    def test_empty_and_separator_only_input_gives_an_empty_slug(self):\n        self.assertEqual(slugify(\"\"), \"\")\n        self.assertEqual(slugify(\" -_- \"), \"\")\n\n    def test_rejects_values_that_are_not_strings(self):\n        with self.assertRaises(TypeError):\n            slugify(None)\n        with self.assertRaises(TypeError):\n            slugify(42)\n\n\nif __name__ == \"__main__\":\n    unittest.main()\n","exit_code":0,"status":"completed"}}"##,
    "\n",
    r##"{"type":"item.started","item":{"id":"item_2","type":"file_change","changes":[{"path":"/owned/worktrees/attempt-1/slugify.py","kind":"update"}],"status":"in_progress"}}"##,
    "\n",
    r##"{"type":"item.completed","item":{"id":"item_2","type":"file_change","changes":[{"path":"/owned/worktrees/attempt-1/slugify.py","kind":"update"}],"status":"completed"}}"##,
    "\n",
    r##"{"type":"item.started","item":{"id":"item_3","type":"command_execution","command":"/bin/zsh -lc 'PYTHONDONTWRITEBYTECODE=1 /usr/bin/python3 -m unittest -q; git diff --exit-code -- test_slugify.py; git diff --stat; git diff -- slugify.py'","aggregated_output":"","exit_code":null,"status":"in_progress"}}"##,
    "\n",
    r##"{"type":"item.completed","item":{"id":"item_3","type":"command_execution","command":"/bin/zsh -lc 'PYTHONDONTWRITEBYTECODE=1 /usr/bin/python3 -m unittest -q; git diff --exit-code -- test_slugify.py; git diff --stat; git diff -- slugify.py'","aggregated_output":"python3: warning: confstr() failed with code 5: couldn't get path of DARWIN_USER_TEMP_DIR; using /tmp instead\n2026-11-15 15:34:07.800 xcodebuild[49796:20806737]  DVTFilePathFSEvents: Failed to start fs event stream.\n2026-11-15 15:34:08.055 xcodebuild[49796:20806735] [MT] DVTDeveloperPaths: Failed to get length of DARWIN_USER_CACHE_DIR from confstr(3), error = Error Domain=NSPOSIXErrorDomain Code=5 \"Input/output error\". Using NSCachesDirectory instead.\npython3: warning: confstr() failed with code 5: couldn't get path of DARWIN_USER_TEMP_DIR; using /tmp instead\n----------------------------------------------------------------------\nRan 8 tests in 0.001s\n\nOK\n slugify.py | 9 ++++++++-\n 1 file changed, 8 insertions(+), 1 deletion(-)\ndiff --git a/slugify.py b/slugify.py\nindex ace972a..9e0eb04 100644\n--- a/slugify.py\n+++ b/slugify.py\n@@ -1,6 +1,13 @@\n \"\"\"Turn a title into a URL slug.\"\"\"\n \n+import re\n+import unicodedata\n+\n \n def slugify(text):\n     \"\"\"Return a lower-case ASCII slug for text: words joined by single hyphens.\"\"\"\n-    raise NotImplementedError(\"slugify() is not implemented yet\")\n+    if not isinstance(text, str):\n+        raise TypeError(\"text must be a string\")\n+\n+    ascii_text = unicodedata.normalize(\"NFKD\", text).encode(\"ascii\", \"ignore\").decode(\"ascii\")\n+    return re.sub(r\"[^a-z0-9]+\", \"-\", ascii_text.lower()).strip(\"-\")\n","exit_code":0,"status":"completed"}}"##,
    "\n",
    r##"{"type":"item.completed","item":{"id":"item_4","type":"agent_message","text":"Implemented `slugify()` in `slugify.py` using only the standard library, including normalization and `TypeError` validation.\n\nAll 8 tests passed with `/usr/bin/python3 -m unittest -q`. `test_slugify.py` is unchanged; only `slugify.py` was modified."}}"##,
    "\n",
    r##"{"type":"turn.completed","usage":{"input_tokens":75415,"cached_input_tokens":61696,"cache_write_input_tokens":0,"output_tokens":461,"reasoning_output_tokens":0}}"##,
    "\n",
);

#[test]
fn m0_real_0_169_0_stream_normalizes_to_one_successful_turn() {
    let mut framer = LineFramer::new(64 * 1024);
    let mut n = StreamNormalizer::for_parser(CODEX_EXEC_JSONL).unwrap();
    let mut frames = framer.push(LIVE_0_169_0.as_bytes());
    frames.extend(framer.finish());
    let events: Vec<EventPayload> = frames.iter().flat_map(|f| n.frame(f)).collect();
    use EventPayload::*;
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| match e {
            SessionIdentified(_) => "session",
            TurnStarted { .. } => "turn",
            TextDelta(_) => "text",
            ToolStarted { .. } => "tool-start",
            ToolFinished { .. } => "tool-end",
            UsageObserved { .. } => "usage",
            ProviderTerminal { .. } => "terminal",
            _ => "other",
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "session",
            "turn",
            "text",
            "tool-start",
            "tool-end",
            "tool-start",
            "tool-end",
            "tool-start",
            "tool-end",
            "text",
            "usage",
            "terminal"
        ],
        "{events:?}"
    );
    assert_eq!(
        events[0],
        SessionIdentified(SessionRef {
            provider: ProviderKind::Codex,
            session: "00000000-0000-7000-8000-000000000001".into(),
            turn: "turn-1".into()
        })
    );
    // input_tokens + output_tokens, as in the documented-shape test above.
    assert!(events.contains(&UsageObserved {
        units: 75_415 + 461
    }));
    assert!(violations(&events).is_empty());
    assert!(!n.protocol_failed());
    assert_eq!(n.terminal(), Some(TerminalStatus::Success));
}

#[test]
fn p2_01_split_records_are_identical_at_every_byte_boundary() {
    let text = format!("{THREAD}\n{TURN}\n{MESSAGE}\n{DONE}\n");
    let bytes = text.as_bytes();
    let (whole, _) = run(&[bytes]);
    assert_eq!(whole.len(), 5);
    for cut in 1..bytes.len() {
        let (split, n) = run(&[&bytes[..cut], &bytes[cut..]]);
        assert_eq!(split, whole, "cut at {cut}");
        assert_eq!(n.terminal(), Some(TerminalStatus::Success));
    }
    let single: Vec<&[u8]> = bytes.chunks(1).collect();
    assert_eq!(run(&single).0, whole);
}

#[test]
fn p2_01_malformed_oversized_and_unterminated_records_fail_closed() {
    fails_closed(
        &[THREAD, TURN, "this is not json", DONE],
        "record is not JSON",
    );
    fails_closed(
        &[THREAD, TURN, r#"["turn.completed"]"#, DONE],
        "record has no string type",
    );
    fails_closed(
        &[THREAD, TURN, r#"{"type":7}"#, DONE],
        "record has no string type",
    );
    fails_closed(
        &[THREAD, TURN, "{\"type\":\"turn.completed\"", DONE],
        "record is not JSON",
    );
    let long = format!(
        r#"{{"type":"item.completed","item":{{"id":"i","type":"agent_message","text":"{}"}}}}"#,
        "x".repeat(2000)
    );
    fails_closed(&[THREAD, TURN, &long, DONE], "record exceeds frame limit");
    let deep = format!(
        r#"{{"type":"error","message":{}1{}}}"#,
        "[".repeat(40),
        "]".repeat(40)
    );
    fails_closed(&[THREAD, TURN, &deep, DONE], "record nesting exceeds limit");
    // No trailing newline on the terminal record: the record may be incomplete.
    let text = format!("{THREAD}\n{TURN}\n{DONE}");
    let (events, n) = run(&[text.as_bytes()]);
    assert!(n.protocol_failed() && n.terminal().is_none());
    assert_eq!(violations(&events), ["unterminated final record"]);
    let (events, n) = run(&[b"\xff\xfe\n"]);
    assert!(n.protocol_failed());
    assert_eq!(violations(&events), ["record is not UTF-8"]);
    // The spool cap was reached: whatever follows cannot be a complete stream.
    let mut n = StreamNormalizer::for_parser(CODEX_EXEC_JSONL).unwrap();
    n.output_truncated();
    let mut framer = LineFramer::new(1024);
    let text = format!("{THREAD}\n{TURN}\n{DONE}\n");
    let events: Vec<_> = framer
        .push(text.as_bytes())
        .iter()
        .flat_map(|f| n.frame(f))
        .collect();
    assert!(n.terminal().is_none());
    assert_eq!(
        violations(&events),
        ["terminal success after protocol failure"]
    );
}

#[test]
fn p2_01_unknown_or_unmodelled_records_never_end_in_success() {
    for unknown in [
        r#"{"type":"session.configured"}"#,
        r#"{"type":"turn.interrupted"}"#,
        r#"{"type":"result","subtype":"success","is_error":false}"#,
        r#"{"type":"system","subtype":"init","session_id":"s"}"#,
        r#"{"type":""}"#,
    ] {
        fails_closed(&[THREAD, TURN, unknown, DONE], "unknown record type");
    }
    fails_closed(
        &[
            THREAD,
            TURN,
            r#"{"type":"item.completed","item":{"id":"i","type":"collab_tool_call"}}"#,
            DONE,
        ],
        "unknown item type",
    );
    for prohibited in ["web_search", "mcp_tool_call"] {
        let item =
            format!(r#"{{"type":"item.started","item":{{"id":"i","type":"{prohibited}"}}}}"#);
        fails_closed(
            &[THREAD, TURN, &item, DONE],
            "tool outside the approved envelope",
        );
    }
    let long_id = format!(
        r#"{{"type":"thread.started","thread_id":"{}"}}"#,
        "t".repeat(257)
    );
    let cases: [(&[&str], &str); 13] = [
        (&[TURN, DONE], "turn before thread.started"),
        (&[THREAD, DONE], "turn end without turn.started"),
        (&[THREAD, THREAD, TURN, DONE], "duplicate thread.started"),
        (
            &[r#"{"type":"thread.started"}"#, TURN, DONE],
            "thread.started without a usable thread_id",
        ),
        (
            &[&long_id, TURN, DONE],
            "thread.started without a usable thread_id",
        ),
        (&[THREAD, MESSAGE, TURN, DONE], "item outside an open turn"),
        (
            &[THREAD, TURN, r#"{"type":"item.completed"}"#, DONE],
            "item event without item",
        ),
        (
            &[
                THREAD,
                TURN,
                r#"{"type":"item.completed","item":{"type":"agent_message","text":"x"}}"#,
                DONE,
            ],
            "item without a usable id",
        ),
        (
            &[
                THREAD,
                TURN,
                r#"{"type":"item.completed","item":{"id":"i"}}"#,
                DONE,
            ],
            "item without type",
        ),
        (
            &[
                THREAD,
                TURN,
                r#"{"type":"item.completed","item":{"id":"i","type":"agent_message"}}"#,
                DONE,
            ],
            "agent_message without text",
        ),
        (
            &[
                THREAD,
                TURN,
                r#"{"type":"turn.failed","error":{"message":"x"}}"#,
                TURN,
                DONE,
            ],
            "record after terminal record",
        ),
        (
            &[THREAD, TURN, TURN, DONE],
            "more than one turn in one exec",
        ),
        (
            &[THREAD, TURN, r#"{"type":"item.started","item":[]}"#, DONE],
            "item event without item",
        ),
    ];
    for (records, why) in cases {
        fails_closed(records, why);
    }
    // After a successful terminal record, anything else latches the failure.
    for extra in [DONE, TURN, MESSAGE, r#"{"type":"error","message":"late"}"#] {
        let (events, n) = lines(&[THREAD, TURN, DONE, extra]);
        assert!(n.protocol_failed(), "{extra}");
        assert_eq!(violations(&events), ["record after terminal record"]);
    }
    // The synthetic profile is unchanged: it still preserves unknown records.
    let mut synthetic = StreamNormalizer::for_parser(CODEX_EXEC_JSON).unwrap();
    let mut framer = LineFramer::new(1024);
    let events: Vec<_> = framer
        .push(b"{\"type\":\"session.configured\"}\n")
        .iter()
        .flat_map(|f| synthetic.frame(f))
        .collect();
    assert_eq!(
        events,
        [EventPayload::Informational("session.configured".into())]
    );
    assert!(!synthetic.protocol_failed());
    assert_ne!(CODEX_EXEC_JSONL, CODEX_EXEC_JSON);
}

#[test]
fn p2_01_failure_error_and_unmodelled_completion_are_not_success() {
    use EventPayload::{ProviderError, ProviderTerminal};
    let (events, n) = lines(&[
        THREAD,
        TURN,
        r#"{"type":"turn.failed","error":{"message":"synthetic failure"}}"#,
    ]);
    assert_eq!(n.terminal(), Some(TerminalStatus::Error));
    assert!(!n.protocol_failed());
    assert_eq!(
        events[2..],
        [
            ProviderError("synthetic failure".into()),
            ProviderTerminal {
                turn: "turn-1".into(),
                status: TerminalStatus::Error,
                raw_status: "turn.failed".into()
            }
        ]
    );
    // Authentication failure text as an error record, then a completion: not success.
    for error in [
        r#"{"type":"error","message":"401 Unauthorized (synthetic)"}"#,
        r#"{"type":"item.completed","item":{"id":"e","type":"error","message":"denied"}}"#,
    ] {
        let (events, n) = lines(&[THREAD, TURN, error, DONE]);
        assert_eq!(n.terminal(), Some(TerminalStatus::Unknown), "{error}");
        assert!(events.iter().any(|e| matches!(e, ProviderError(_))));
    }
    // A completion that is not exactly the documented shape is recorded as unknown.
    for completion in [
        r#"{"type":"turn.completed"}"#,
        r#"{"type":"turn.completed","usage":null}"#,
        r#"{"type":"turn.completed","usage":{"input_tokens":1}}"#,
        r#"{"type":"turn.completed","usage":{"input_tokens":"1","output_tokens":1}}"#,
        r#"{"type":"turn.completed","usage":{"input_tokens":-1,"output_tokens":1}}"#,
        r#"{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1},"status":"failed"}"#,
        r#"{"type":"turn.completed","usage":{"input_tokens":1,"output_tokens":1},"error":null}"#,
    ] {
        let (events, n) = lines(&[THREAD, TURN, completion]);
        assert_eq!(n.terminal(), Some(TerminalStatus::Unknown), "{completion}");
        assert!(matches!(
            events.last(),
            Some(ProviderTerminal {
                status: TerminalStatus::Unknown,
                ..
            })
        ));
    }
    // Exit status is not part of the stream: without a terminal record there is none.
    let (events, n) = lines(&[THREAD, TURN, MESSAGE]);
    assert_eq!(events.len(), 3);
    assert!(n.terminal().is_none() && !n.protocol_failed());
    let (events, n) = lines(&[]);
    assert!(events.is_empty() && n.terminal().is_none());
}

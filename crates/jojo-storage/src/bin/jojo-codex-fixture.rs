//! Test-only stand-in for a Codex executable (P2-01). Never invoked by production code.
//!
//! It is launched with the exact argument vector and environment the Codex adapter
//! produces, reads the scenario name from stdin (where the adapter puts the prompt) and
//! prints records shaped like the documented `codex exec --json` stream. It is not Codex:
//! no provider binary, credential, network or inference is involved, it starts no
//! process, and every scenario ends on its own.
//!
//! P2-02: a production Start composes the prompt from approved documents, so the scenario
//! may also be given as a `fixture: <scenario>` line inside the prompt. `develop <path>`
//! writes that one relative file in the working directory (the owned worktree), as a
//! developer would; it is the only file operation here.
//!
//! P2-04: the rehearsal scenarios act on the generated sample repository (`slugify.py`,
//! `test_slugify.py`). `implement-slugify` writes a working implementation and then claims
//! that the tests pass, which nothing may believe; `-stray` and `-edit-tests` additionally
//! write outside the approved scope; `work-then-hang <seconds>` writes the implementation
//! and then stays alive without output, for at most that long, so that it can be stopped.
use std::io::{Read, Write};
use std::time::Duration;

const THREAD: &str = r#"{"type":"thread.started","thread_id":"fixture-thread"}"#;
const TURN: &str = r#"{"type":"turn.started"}"#;
const DONE: &str = r#"{"type":"turn.completed","usage":{"input_tokens":5,"cached_input_tokens":0,"output_tokens":6}}"#;

fn out(bytes: &[u8]) {
    let mut o = std::io::stdout().lock();
    let _ = o.write_all(bytes);
    let _ = o.flush();
}

fn line(text: &str) {
    out(format!("{text}\n").as_bytes());
}

fn message(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        r#"{{"type":"item.completed","item":{{"id":"item-9","type":"agent_message","text":"{escaped}"}}}}"#
    )
}

/// What the rehearsal's stand-in developer writes to `slugify.py`.
const SLUGIFY: &str = r#""""Turn a title into a URL slug."""
import re
import unicodedata


def slugify(text):
    """Return a lower-case ASCII slug for text: words joined by single hyphens."""
    if not isinstance(text, str):
        raise TypeError("slugify expects a string")
    plain = unicodedata.normalize("NFKD", text).encode("ascii", "ignore").decode("ascii")
    return re.sub(r"[^a-z0-9]+", "-", plain.lower()).strip("-")
"#;
/// A test file that can no longer fail: what an agent must never be allowed to do.
const WEAKENED_TESTS: &str = "import unittest\n\n\nclass SlugifyTest(unittest.TestCase):\n    \
    def test_nothing(self):\n        self.assertTrue(True)\n";
/// P2-04.1: an implementation whose tests never finish, so that host validation is
/// still running when the rehearsal kills the helper.
const SLUGIFY_HANGS: &str = "import time\n\n\ndef slugify(text):\n    time.sleep(600)\n";
const CLAIM: &str = "Implemented slugify(). All tests passed (says the agent).";

/// Writes the given files in the working directory, or fails the turn.
fn develop(files: &[(&str, &str)]) {
    for (path, content) in files {
        if std::fs::write(path, content).is_err() {
            line(r#"{"type":"turn.failed","error":{"message":"fixture could not write"}}"#);
            std::process::exit(1);
        }
    }
}

const CLAUDE_SESSION: &str = "fixture-session";

/// One Claude Code stream-json record of the fixture session. `rest` is the record's
/// other members, without braces.
fn claude(kind: &str, rest: &str) {
    line(&format!(
        r#"{{"type":"{kind}",{rest},"session_id":"{CLAUDE_SESSION}"}}"#
    ));
}

fn claude_init(tools: &str) {
    claude(
        "system",
        &format!(
            r#""subtype":"init","tools":[{tools}],"mcp_servers":[],"permissionMode":"acceptEdits","apiKeySource":"none","claude_code_version":"2.1.311""#
        ),
    );
}

fn claude_text(text: &str) {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    claude(
        "assistant",
        &format!(
            r#""message":{{"role":"assistant","content":[{{"type":"text","text":"{escaped}"}}]}},"parent_tool_use_id":null"#
        ),
    );
}

fn claude_tool(id: &str, name: &str) {
    claude(
        "assistant",
        &format!(
            r#""message":{{"role":"assistant","content":[{{"type":"tool_use","id":"{id}","name":"{name}","input":{{}}}}]}},"parent_tool_use_id":null"#
        ),
    );
    claude(
        "user",
        &format!(
            r#""message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"{id}","content":"ok"}}]}},"parent_tool_use_id":null"#
        ),
    );
}

fn claude_result(is_error: bool, reason: &str) {
    claude(
        "result",
        &format!(
            r#""subtype":"success","is_error":{is_error},"terminal_reason":"{reason}","usage":{{"input_tokens":5,"output_tokens":6}},"permission_denials":[]"#
        ),
    );
}

const CLAUDE_FILE_TOOLS: &str = r#""Edit","Glob","Grep","Read","Write""#;

/// M1: the same stand-in speaking the Claude Code `--print` stream-json dialect, when it
/// is launched with the argument vector the Claude Code adapter produces. It is not
/// Claude Code: no provider binary, credential, network or inference is involved.
fn claude_main(args: &[String], scenario: &str) {
    if let Some(path) = scenario.strip_prefix("develop ") {
        claude_init(CLAUDE_FILE_TOOLS);
        if path.starts_with('/') || path.split('/').any(|part| part == "..") {
            eprintln!("fixture writes only inside its working directory");
            std::process::exit(65);
        }
        if std::fs::write(path, b"written by fixture\n").is_err() {
            claude_result(true, "api_error");
            std::process::exit(1);
        }
        claude_tool("toolu_fixture_1", "Write");
        claude_text("developed");
        claude_result(false, "completed");
        return;
    }
    match scenario {
        "ok" => {
            claude_init(CLAUDE_FILE_TOOLS);
            claude(
                "system",
                r#""subtype":"thinking_tokens","estimated_tokens":50"#,
            );
            claude(
                "rate_limit_event",
                r#""rate_limit_info":{"status":"allowed"}"#,
            );
            claude_tool("toolu_fixture_1", "Read");
            claude_text("ok");
            claude_result(false, "completed");
        }
        // What actually crossed the OS boundary, as seen from inside the child.
        "report" => {
            let mut variables: Vec<String> = std::env::vars()
                .map(|(name, value)| format!("{name}={value}"))
                .collect();
            variables.sort();
            claude_init(CLAUDE_FILE_TOOLS);
            claude_text(&format!("argv={args:?} variables={variables:?}"));
            claude_result(false, "completed");
        }
        // The shape the CLI printed when it was not signed in: a "success" that is an error.
        "auth-error" => {
            claude_init(CLAUDE_FILE_TOOLS);
            claude(
                "assistant",
                r#""message":{"role":"assistant","content":[{"type":"text","text":"Not logged in (synthetic fixture text)"}]},"parent_tool_use_id":null,"error":"authentication_failed""#,
            );
            claude_result(true, "api_error");
            std::process::exit(1);
        }
        "unknown-kind" => {
            claude_init(CLAUDE_FILE_TOOLS);
            claude("system", r#""subtype":"compact_boundary""#);
            claude_result(false, "completed");
        }
        // A tool that was not approved, used or merely offered.
        "tool-not-approved" => {
            claude_init(CLAUDE_FILE_TOOLS);
            claude_tool("toolu_fixture_1", "Bash");
            claude_result(false, "completed");
        }
        "init-not-approved" => {
            claude_init(r#""Bash","Edit","Glob","Grep","Read","Write""#);
            claude_text("ok");
            claude_result(false, "completed");
        }
        "exit-zero-no-terminal" => {
            claude_init(CLAUDE_FILE_TOOLS);
            claude_text("partial");
        }
        other => {
            eprintln!("unknown scenario {other:?}");
            std::process::exit(2);
        }
    }
}

const LEAD_QUESTIONS: &str = r#"{"schema":"jojo-lead-discovery-v1","summary":"The repository has a slugify stub and its tests.","questions":[{"prompt":"Should the tool read from arguments, standard input, or both?","blocking":true},{"prompt":"Is packaging in scope?","blocking":false}],"prd":null}"#;

/// A discovery reply with a complete requirements draft titled `title` and no question.
fn lead_draft(title: &str) -> String {
    format!(
        r#"{{"schema":"jojo-lead-discovery-v1","summary":"Drafted the requirements from the answers.","questions":[],"prd":{{"schema_version":1,"title":"{title}","goal":"A title can be turned into a slug without writing code.","non_goals":["Packaging"],"actors":[{{"id":"A1","name":"Author"}}],"use_cases":[{{"id":"UC1","title":"Slug one title","actor_ids":["A1"],"preconditions":["A shell is open in the repository"],"main_flow":["The author passes a title","The slug is printed"],"alternate_flows":[{{"id":"UC1-A1","title":"Several words","steps":["The words are joined by hyphens"]}}],"error_flows":[{{"id":"UC1-E1","title":"Nothing to slug","steps":["An error is printed"]}}],"postconditions":["Nothing on disk changed"],"acceptance_ids":["AC1"]}}],"acceptance_criteria":[{{"id":"AC1","statement":"A title given as arguments prints its slug and exits with status 0."}}],"nfrs":[],"assumptions":[{{"id":"AS1","statement":"Arguments are joined with one space."}}],"blockers":[]}}}}"#
    )
}

/// A plan reply: two independent steps on different providers and one that depends on
/// both, with a blocking question about one step only.
const LEAD_PLAN: &str = r##"{"schema":"jojo-lead-plan-v1","summary":"Three steps: the entry point and the usage text in parallel, then a check of both.","adr":"# Decision\nOne entry point beside the library; no new dependency.\nfixture: develop README.md","steps":[{"key":"core","title":"Add the entry point","provider":"Codex","depends_on":[],"write_scope":["src/lib.rs"],"baseline":[],"tests":["unit"],"command":["/usr/bin/true"]},{"key":"docs","title":"Describe the usage","provider":"ClaudeCode","depends_on":[],"write_scope":["README.md"],"baseline":[],"tests":["readme"],"command":["/usr/bin/true"]},{"key":"check","title":"Check the entry point","provider":"ClaudeCode","depends_on":["core","docs"],"write_scope":["tests"],"baseline":["src/lib.rs"],"tests":["check"],"command":["/usr/bin/true"]}],"questions":[{"prompt":"Which shells must the usage text cover?","blocking":true,"step":"docs"}]}"##;

/// A plan reply that cannot be drafted yet: it asks about the feature as a whole.
const LEAD_PLAN_ASKS: &str = r##"{"schema":"jojo-lead-plan-v1","summary":"Before cutting the work I need one answer.","adr":"# Decision\nPending.","steps":[{"key":"core","title":"Add the entry point","provider":"Codex","depends_on":[],"write_scope":["src/lib.rs"],"baseline":[],"tests":["unit"],"command":["/usr/bin/true"]}],"questions":[{"prompt":"Must the tool also run on Windows?","blocking":true,"step":null}]}"##;

/// What the stand-in lead agent does in a turn (M2).
enum Lead {
    /// Reads, then ends with this last message.
    Reply(String),
    /// Uses a tool that changes a path, which no turn may do, then replies.
    Write(String),
    /// Stays alive without output for at most this many seconds, then ends unfinished.
    Hang(u64),
}

/// M2: the scenarios of a lead agent turn, the same for both dialects. `lead` answers by
/// what the prompt already holds: questions first, then a draft, then a revised draft.
fn lead(scenario: &str, prompt: &str, args: &[String]) -> Lead {
    if let Some(seconds) = scenario.strip_prefix("lead-hang ") {
        return Lead::Hang(seconds.parse::<u64>().unwrap_or(0).min(600));
    }
    let planning = prompt.contains("jojo-lead-plan-v1");
    match scenario {
        "lead" if planning => Lead::Reply(LEAD_PLAN.into()),
        "lead-plan-asks" => Lead::Reply(LEAD_PLAN_ASKS.into()),
        "lead" if prompt.contains("# Questions and answers so far\nNone.") => {
            Lead::Reply(LEAD_QUESTIONS.into())
        }
        "lead" if prompt.contains("# Current requirements draft\nNone yet.") => {
            Lead::Reply(lead_draft("Slugify from the shell"))
        }
        "lead" => Lead::Reply(lead_draft("Slugify from the shell, revised")),
        "lead-prose" => Lead::Reply("I looked around and could not decide.".into()),
        "lead-write" => Lead::Write(LEAD_QUESTIONS.into()),
        // What actually crossed the OS boundary, as seen from inside the child.
        "report" => {
            let mut variables: Vec<String> = std::env::vars()
                .map(|(name, value)| format!("{name}={value}"))
                .collect();
            variables.sort();
            Lead::Reply(format!("argv={args:?} variables={variables:?}"))
        }
        other => {
            eprintln!("unknown lead scenario {other:?}");
            std::process::exit(2);
        }
    }
}

fn hang(seconds: u64) {
    // Bounded: the scenario ends on its own even if nothing ever stops it.
    for _ in 0..seconds * 10 {
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn claude_lead(act: Lead) {
    claude(
        "system",
        r#""subtype":"init","tools":["Glob","Grep","Read"],"mcp_servers":[],"permissionMode":"default","apiKeySource":"none","claude_code_version":"2.1.311""#,
    );
    claude_tool("toolu_fixture_1", "Read");
    match act {
        Lead::Reply(text) => claude_text(&text),
        Lead::Write(text) => {
            claude_tool("toolu_fixture_2", "Write");
            claude_text(&text);
        }
        Lead::Hang(seconds) => return hang(seconds),
    }
    claude_result(false, "completed");
}

fn codex_lead(act: Lead) {
    line(THREAD);
    line(TURN);
    line(
        r#"{"type":"item.completed","item":{"id":"item-1","type":"command_execution","exit_code":0}}"#,
    );
    match act {
        Lead::Reply(text) => line(&message(&text)),
        Lead::Write(text) => {
            line(r#"{"type":"item.completed","item":{"id":"item-2","type":"file_change"}}"#);
            line(&message(&text));
        }
        Lead::Hang(seconds) => return hang(seconds),
    }
    line(DONE);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut prompt = String::new();
    let _ = std::io::stdin().read_to_string(&mut prompt);
    let scenario = prompt
        .lines()
        .find_map(|l| l.trim().strip_prefix("fixture: "))
        .unwrap_or(prompt.trim());
    // M2: a lead agent prompt carries the objective of the first turn and the note of this
    // one, so the scenario of a turn is the last directive, not the first.
    let latest = prompt
        .lines()
        .filter_map(|l| l.trim().strip_prefix("fixture: "))
        .next_back()
        .unwrap_or(prompt.trim());
    if args.first().map(String::as_str) == Some("-p") {
        // M2: the read-only vector of a lead agent turn names the three read tools.
        if args.iter().any(|a| a == "Read,Glob,Grep") {
            return claude_lead(lead(latest, &prompt, &args));
        }
        return claude_main(&args, scenario);
    }
    if args.first().map(String::as_str) != Some("exec") {
        eprintln!("fixture accepts only the exec subcommand");
        std::process::exit(64);
    }
    if args.iter().any(|a| a == "read-only") {
        return codex_lead(lead(latest, &prompt, &args));
    }
    if let Some(path) = scenario.strip_prefix("develop ") {
        line(THREAD);
        line(TURN);
        if path.starts_with('/') || path.split('/').any(|part| part == "..") {
            eprintln!("fixture writes only inside its working directory");
            std::process::exit(65);
        }
        if std::fs::write(path, b"written by fixture\n").is_err() {
            line(r#"{"type":"turn.failed","error":{"message":"fixture could not write"}}"#);
            std::process::exit(1);
        }
        line(&message("developed"));
        line(DONE);
        return;
    }
    if let Some(seconds) = scenario.strip_prefix("work-then-hang ") {
        let Ok(seconds) = seconds.parse::<u64>() else {
            eprintln!("work-then-hang needs a number of seconds");
            std::process::exit(2);
        };
        line(THREAD);
        line(TURN);
        develop(&[("slugify.py", SLUGIFY)]);
        // Bounded: the scenario ends on its own even if nothing ever stops it.
        for _ in 0..seconds.min(600) * 10 {
            std::thread::sleep(Duration::from_millis(100));
        }
        line(&message("finished late"));
        line(DONE);
        return;
    }
    match scenario {
        "implement-slugify-hang-tests" => {
            line(THREAD);
            line(TURN);
            develop(&[("slugify.py", SLUGIFY_HANGS)]);
            line(&message(CLAIM));
            line(DONE);
        }
        "implement-slugify" | "implement-slugify-stray" | "implement-slugify-edit-tests" => {
            line(THREAD);
            line(TURN);
            develop(&[("slugify.py", SLUGIFY)]);
            if scenario.ends_with("-stray") {
                develop(&[("NOTES.md", "scratch notes left behind by the agent\n")]);
            }
            if scenario.ends_with("-edit-tests") {
                develop(&[("test_slugify.py", WEAKENED_TESTS)]);
            }
            line(&message(CLAIM));
            line(DONE);
        }
        "ok" => {
            line(THREAD);
            line(TURN);
            line(
                r#"{"type":"item.started","item":{"id":"item-1","type":"command_execution","command":"true"}}"#,
            );
            line(
                r#"{"type":"item.completed","item":{"id":"item-1","type":"command_execution","exit_code":0}}"#,
            );
            line(
                r#"{"type":"item.completed","item":{"id":"item-2","type":"reasoning","text":"…"}}"#,
            );
            line(&message("ok"));
            line(DONE);
        }
        // What actually crossed the OS boundary, as seen from inside the child.
        "report" => {
            let mut variables: Vec<String> = std::env::vars()
                .map(|(name, value)| format!("{name}={value}"))
                .collect();
            variables.sort();
            line(THREAD);
            line(TURN);
            line(&message(&format!("argv={args:?} variables={variables:?}")));
            line(DONE);
        }
        "split" => {
            // Cut inside a 4-byte UTF-8 sequence, inside JSON tokens and across newlines.
            let text = format!("{THREAD}\n{TURN}\n{}\n{DONE}\n", message("a🙂b"));
            let bytes = text.as_bytes();
            let emoji = text.find('🙂').expect("emoji");
            let cuts = [
                1,
                7,
                THREAD.len(),
                THREAD.len() + 1,
                emoji + 1,
                emoji + 3,
                bytes.len() - 2,
                bytes.len() - 1,
            ];
            let mut last = 0;
            for cut in cuts {
                out(&bytes[last..cut]);
                std::thread::sleep(Duration::from_millis(2));
                last = cut;
            }
            out(&bytes[last..]);
        }
        "malformed" => {
            line(THREAD);
            line(TURN);
            line("this is not json");
            line(DONE);
        }
        "oversized" => {
            line(THREAD);
            line(TURN);
            out(&vec![b'x'; 256 * 1024]);
            out(b"\n");
            line(DONE);
        }
        "turn-failed" => {
            line(THREAD);
            line(TURN);
            line(r#"{"type":"turn.failed","error":{"message":"synthetic failure"}}"#);
            std::process::exit(1);
        }
        "auth-error" => {
            eprintln!("ERROR: not signed in (synthetic fixture text)");
            line(THREAD);
            line(TURN);
            line(r#"{"type":"error","message":"401 Unauthorized (synthetic)"}"#);
            line(r#"{"type":"turn.failed","error":{"message":"401 Unauthorized (synthetic)"}}"#);
            std::process::exit(1);
        }
        "auth-error-text" => {
            line("Not signed in. Sign in with the provider CLI first. (synthetic fixture text)");
            std::process::exit(1);
        }
        "exit-zero-no-terminal" => {
            line(THREAD);
            line(TURN);
            line(&message("partial"));
        }
        "unknown-kind" => {
            line(THREAD);
            line(TURN);
            line(r#"{"type":"session.configured","detail":"not a modelled record"}"#);
            line(DONE);
        }
        "error-then-completed" => {
            line(THREAD);
            line(TURN);
            line(r#"{"type":"error","message":"stream disconnected (synthetic)"}"#);
            line(DONE);
        }
        "stderr-flood" => {
            line(THREAD);
            line(TURN);
            let chunk = vec![b'e'; 64 * 1024];
            let mut diagnostics = std::io::stderr().lock();
            for _ in 0..64 {
                if diagnostics.write_all(&chunk).is_err() {
                    std::process::exit(3);
                }
            }
            line(DONE);
        }
        other => {
            eprintln!("unknown scenario {other:?}");
            std::process::exit(2);
        }
    }
}

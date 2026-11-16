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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("exec") {
        eprintln!("fixture accepts only the exec subcommand");
        std::process::exit(64);
    }
    let mut prompt = String::new();
    let _ = std::io::stdin().read_to_string(&mut prompt);
    let scenario = prompt
        .lines()
        .find_map(|l| l.trim().strip_prefix("fixture: "))
        .unwrap_or(prompt.trim());
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

//! Step 7 (New Project): the project brief. Pure: the prompt and the strict reading of
//! the lead's reply.
//!
//! When a project is set up, the lead reads one registered repository, read-only, and
//! writes what a later session has to know about it: the architecture, the commands that
//! build and test it, its conventions and what to watch out for, each row with the files
//! it came from. A brief is a proposal. It is approved by the human like any document, and
//! only an approved brief is carried into the later discovery and plan turns of that
//! repository. The test command it proposes is never adopted on its own.
//!
//! Prompt content is adapted, with attribution, from two pinned sources
//! (`docs/roadmap/SOURCE_FIRST_REUSE_MAP.md`, section 0): Kilbex/Vigla (Apache-2.0)
//! `mission_supervisor_run/prompts.rs` (read the codebase before anything else; name what
//! exists) and addyosmani/factory (MIT, Addy Osmani) `factory-spec/SKILL.md` (state what you
//! could not establish instead of guessing).
use crate::ipc::{encode, object, text as json_text};
use crate::json::{self, Json, element_path};
use crate::planner::{LeadError, Members, REPLY_RULES, bad, schema, text, unfenced};

pub const BRIEF_SCHEMA: &str = "jojo-project-brief-v1";
/// Most rows one brief may have.
pub const MAX_BRIEF_ROWS: usize = 8;
const MAX_ROW_KEY_BYTES: usize = 32;
const MAX_ROW_TEXT_BYTES: usize = 1024;
const MAX_SOURCES: usize = 6;
const MAX_SOURCE_BYTES: usize = 256;
const MAX_SUMMARY_BYTES: usize = 2 * 1024;

/// One line of a brief: a heading, what was found and the files it was found in. A
/// `warning` row is something a later session should be careful about.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BriefRow {
    pub key: String,
    pub text: String,
    pub sources: Vec<String>,
    pub warning: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Brief {
    pub summary: String,
    pub rows: Vec<BriefRow>,
    /// The argument vector the lead believes runs the repository's tests, with an absolute
    /// program path; a proposal only.
    pub test_command: Option<Vec<String>>,
}

/// The prompt of a brief turn. `note` is what the human said they bring; `project` is
/// the project section every turn gets.
pub fn brief_prompt(repository: &str, note: Option<&str>, project: Option<&str>) -> String {
    let mut out = format!(
        "You are the lead engineer joining the repository \"{repository}\". Before anyone \
         describes a feature, write the project brief a later session will be given. You \
         may only read: list and read what you need in the current directory, change \
         nothing, run nothing that has side effects, and do not try to work around a \
         missing tool.\n\n\
         Read the README, any agent instruction document, every package or build manifest, \
         the CI configuration, a shallow view of the directory tree and the entry points \
         you find there.\n"
    );
    if let Some(note) = note {
        out.push_str("\n# What the product owner says they bring\n");
        out.push_str(note);
        out.push('\n');
    }
    if let Some(project) = project {
        out.push_str(project);
    }
    out.push_str(&format!(
        "\n# What to do\n\
         Write between 1 and {MAX_BRIEF_ROWS} rows. Use these headings when they apply, in \
         this order: \"Architecture\" (modules, entry points and data stores), \"Commands\" \
         (how the repository is built, tested and linted), \"Conventions\" (naming, \
         layering, error handling, how changes are made), and \"Watch out\" (what is \
         missing or risky, such as absent tests). Each row states only what you saw, in \
         one or two sentences, and lists in \"sources\" the repository-relative files it \
         came from (at most {MAX_SOURCES}). Mark \"warning\" true for a risk or a gap. If you \
         could not establish something, say so in a \"Watch out\" row instead of guessing.\n\
         In \"test_command\" give the argument vector that runs the repository's tests from \
         its root, with an absolute program path, or null when you found none.\n\
         In \"summary\", tell the product owner in two or three sentences what this \
         repository is.\n\n\
         # Reply\n{REPLY_RULES}\n\
         {{\"schema\":\"{BRIEF_SCHEMA}\",\"summary\":\"...\",\
         \"rows\":[{{\"key\":\"Architecture\",\"text\":\"...\",\"sources\":[\"...\"],\
         \"warning\":false}}],\"test_command\":null}}\n"
    ));
    out
}

fn row(value: &Json, path: &str) -> Result<BriefRow, LeadError> {
    let mut members = Members::of(value, path)?;
    let key = members.text("key", MAX_ROW_KEY_BYTES, false)?;
    let text_value = members.text("text", MAX_ROW_TEXT_BYTES, true)?;
    let (value, sources_path) = members.take("sources")?;
    let Json::List(items) = value else {
        return Err(bad(sources_path, "not an array"));
    };
    if items.len() > MAX_SOURCES {
        return Err(bad(sources_path, "too many entries"));
    }
    let sources = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let path = element_path(&sources_path, index);
            let source = text(item, &path, MAX_SOURCE_BYTES, false)?;
            if source.starts_with('/') || source.split('/').any(|part| part == "..") {
                Err(bad(path, "not a repository-relative path"))
            } else {
                Ok(source)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let warning = match members.take("warning")? {
        (Json::Bool(warning), _) => *warning,
        (_, path) => return Err(bad(path, "not a boolean")),
    };
    members.finish()?;
    Ok(BriefRow {
        key,
        text: text_value,
        sources,
        warning,
    })
}

pub(crate) fn brief(root: &Json) -> Result<Brief, LeadError> {
    let mut members = Members::of(root, "")?;
    schema(&mut members, BRIEF_SCHEMA)?;
    let summary = members.text("summary", MAX_SUMMARY_BYTES, true)?;
    let (value, path) = members.take("rows")?;
    let Json::List(items) = value else {
        return Err(bad(path, "not an array"));
    };
    if items.is_empty() || items.len() > MAX_BRIEF_ROWS {
        return Err(bad(path, "needs one to eight rows"));
    }
    let rows = items
        .iter()
        .enumerate()
        .map(|(index, item)| row(item, &element_path(&path, index)))
        .collect::<Result<Vec<_>, _>>()?;
    let test_command = match members.take("test_command")? {
        (Json::Null, _) => None,
        (Json::List(items), path) => {
            if items.is_empty() || items.len() > crate::validation::MAX_COMMAND_ARGS {
                return Err(bad(path, "not a command"));
            }
            let command = items
                .iter()
                .map(|item| match item {
                    Json::Text(argument) if argument.len() <= 4096 && !argument.contains('\0') => {
                        Ok(argument.clone())
                    }
                    _ => Err(bad(&path, "not a bounded string")),
                })
                .collect::<Result<Vec<_>, _>>()?;
            if !command[0].starts_with('/') {
                return Err(bad(path, "the program path is not absolute"));
            }
            Some(command)
        }
        (_, path) => return Err(bad(path, "not an array or null")),
    };
    members.finish()?;
    Ok(Brief {
        summary,
        rows,
        test_command,
    })
}

/// Reads the last message of a brief turn; one surrounding code fence is tolerated.
pub fn parse_brief(reply: &str) -> Result<Brief, LeadError> {
    let root = json::parse(unfenced(reply).as_bytes()).map_err(LeadError::Json)?;
    brief(&root)
}

impl Brief {
    /// The canonical stored form: the reply's members in a fixed order.
    pub fn canonical_json(&self) -> String {
        encode(&self.json())
    }

    pub fn json(&self) -> Json {
        let texts = |list: &[String]| Json::List(list.iter().map(json_text).collect());
        object([
            ("schema", json_text(BRIEF_SCHEMA)),
            ("summary", json_text(&self.summary)),
            (
                "rows",
                Json::List(
                    self.rows
                        .iter()
                        .map(|row| {
                            object([
                                ("key", json_text(&row.key)),
                                ("text", json_text(&row.text)),
                                ("sources", texts(&row.sources)),
                                ("warning", Json::Bool(row.warning)),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "test_command",
                self.test_command.as_deref().map_or(Json::Null, texts),
            ),
        ])
    }

    /// The Markdown shown for review and carried into later prompts.
    pub fn render_markdown(&self, repository: &str) -> String {
        let mut out = format!("# Project brief: {repository}\n\n{}\n", self.summary);
        for row in &self.rows {
            out.push_str(&format!("\n## {}\n{}\n", row.key, row.text));
            if row.warning {
                out.push_str("Marked as a risk.\n");
            }
            if !row.sources.is_empty() {
                let list: Vec<String> = row.sources.iter().map(|s| format!("`{s}`")).collect();
                out.push_str(&format!("Sources: {}\n", list.join(", ")));
            }
        }
        if let Some(command) = &self.test_command {
            out.push_str(&format!(
                "\n## Proposed test command\n`{}`\n",
                encode(&Json::List(command.iter().map(json_text).collect()))
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"{"schema":"jojo-project-brief-v1","summary":"A sample service.",
        "rows":[{"key":"Architecture","text":"One binary in src/main.rs.","sources":["src/main.rs"],"warning":false},
                {"key":"Watch out","text":"No tests were found.","sources":[],"warning":true}],
        "test_command":["/usr/bin/make","test"]}"#;

    #[test]
    fn a_brief_reads_and_renders() {
        let brief = parse_brief(GOOD).unwrap();
        assert_eq!(brief.rows.len(), 2);
        assert!(brief.rows[1].warning);
        assert_eq!(
            brief.test_command.as_deref(),
            Some(&["/usr/bin/make".to_owned(), "test".to_owned()][..])
        );
        assert_eq!(parse_brief(&brief.canonical_json()).unwrap(), brief);
        let text = brief.render_markdown("sample");
        assert!(text.starts_with("# Project brief: sample\n"));
        assert!(text.contains("## Watch out\nNo tests were found.\nMarked as a risk.\n"));
    }

    #[test]
    fn a_brief_is_refused_when_it_strays() {
        for (from, to) in [
            ("jojo-project-brief-v1", "jojo-lead-discovery-v1"),
            ("\"src/main.rs\"", "\"/etc/passwd\""),
            ("\"src/main.rs\"", "\"../x\""),
            ("\"/usr/bin/make\"", "\"make\""),
            ("\"warning\":false", "\"warning\":\"no\""),
            ("\"test_command\"", "\"command\""),
        ] {
            assert!(parse_brief(&GOOD.replace(from, to)).is_err(), "{to}");
        }
        assert!(parse_brief(&GOOD.replace(
            r#""rows":[{"key":"Architecture","text":"One binary in src/main.rs.","sources":["src/main.rs"],"warning":false},
                {"key":"Watch out","text":"No tests were found.","sources":[],"warning":true}]"#,
            r#""rows":[]"#
        ))
        .is_err());
    }
}

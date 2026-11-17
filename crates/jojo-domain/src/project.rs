//! M3: the project record. A project owns registered repositories, the commands that test
//! them, instructions given to every agent session, the default provider per task kind
//! and the parallel capacity (D4). No I/O or mutation authority.
//!
//! M4 adds the repair limit and the cross-provider reviewer (D2). A record saved before
//! M4 has neither; it reads as two repairs with the reviewer on.
//!
//! A record is saved whole, as a new revision; revisions are immutable. What a plan was
//! drafted under is pinned by revision, so a later edit never changes an approved plan.
use crate::identity::RepositoryId;
use crate::ipc::{get, id, number, object, string, text, uint};
use crate::json::{self, Json};
use crate::provider::ProviderKind;
use crate::repair::MAX_REPAIRS;

pub const MAX_PROJECTS: usize = 8;
pub const MAX_PROJECT_REPOSITORIES: usize = 4;
pub const MAX_KINDS: usize = 8;
pub const MAX_NAME_BYTES: usize = 80;
pub const MAX_INSTRUCTIONS_BYTES: usize = 2048;
pub const MAX_KIND_BYTES: usize = 32;
pub const MAX_COMMAND_ARGS: usize = 16;
pub const MAX_ARGUMENT_BYTES: usize = 512;
/// D4 (decided 2026-11-16): two attempts in all, not split by provider.
pub const DEFAULT_CAPACITY: u32 = 2;
pub const MAX_CAPACITY: u32 = 8;

/// The default provider of one task kind.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KindRule {
    pub kind: String,
    pub provider: ProviderKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectRecord {
    pub name: String,
    pub repositories: Vec<RepositoryId>,
    /// Given to every lead turn and, through the ADR it is appended to, every attempt.
    pub instructions: String,
    /// Offered to the lead as the usual check; the approved validation plan decides.
    pub test_command: Vec<String>,
    /// Stored and shown only; Jojo runs no CI.
    pub ci_command: Vec<String>,
    pub kinds: Vec<KindRule>,
    pub default_provider: ProviderKind,
    /// Parallel attempts in all. A lead turn takes one unit of it as well.
    pub capacity: u32,
    /// M4 (D2): automatic repairs of a failed plan step, 0 to two.
    pub repair_limit: u8,
    /// M4 (D2): a settled plan step is reviewed by the other provider.
    pub review: bool,
}

pub fn provider_name(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::Codex => "Codex",
        ProviderKind::ClaudeCode => "ClaudeCode",
    }
}

pub fn provider_named(name: &str) -> Option<ProviderKind> {
    match name {
        "Codex" => Some(ProviderKind::Codex),
        "ClaudeCode" => Some(ProviderKind::ClaudeCode),
        _ => None,
    }
}

/// A task kind: short, lower case, letters, digits and '-'.
pub fn valid_kind(kind: &str) -> bool {
    !kind.is_empty()
        && kind.len() <= MAX_KIND_BYTES
        && kind
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn words(value: &Json, what: &str) -> Result<Vec<String>, String> {
    let Json::List(items) = value else {
        return Err(format!("InvalidProject: {what} is not a list"));
    };
    items
        .iter()
        .map(|v| string(v).map(str::to_owned))
        .collect::<Result<_, _>>()
        .map_err(|_| format!("InvalidProject: {what} holds a non-text entry"))
}

fn command_json(argv: &[String]) -> Json {
    Json::List(argv.iter().map(|a| text(a.clone())).collect())
}

impl ProjectRecord {
    pub fn validate(&self) -> Result<(), String> {
        let bad = |why: &str| Err(format!("InvalidProject: {why}"));
        if !self.name.chars().any(char::is_alphanumeric)
            || self.name.len() > MAX_NAME_BYTES
            || self.name.chars().any(char::is_control)
        {
            return bad("the name is empty, too long or has a control character");
        }
        if self.repositories.is_empty() || self.repositories.len() > MAX_PROJECT_REPOSITORIES {
            return bad("one to four repositories");
        }
        if (1..self.repositories.len())
            .any(|i| self.repositories[..i].contains(&self.repositories[i]))
        {
            return bad("a repository is listed twice");
        }
        if self.instructions.len() > MAX_INSTRUCTIONS_BYTES || self.instructions.contains('\0') {
            return bad("the instructions are too long or contain NUL");
        }
        for (what, argv) in [
            ("test command", &self.test_command),
            ("CI command", &self.ci_command),
        ] {
            if argv.len() > MAX_COMMAND_ARGS
                || argv
                    .iter()
                    .any(|a| a.len() > MAX_ARGUMENT_BYTES || a.contains('\0'))
                || argv.first().is_some_and(|p| !p.starts_with('/'))
            {
                return Err(format!(
                    "InvalidProject: the {what} is too long, has NUL or its program is not an absolute path"
                ));
            }
        }
        if self.kinds.len() > MAX_KINDS {
            return bad("at most eight task kinds");
        }
        for (i, rule) in self.kinds.iter().enumerate() {
            if !valid_kind(&rule.kind) {
                return bad("a task kind is not lower case letters, digits and '-'");
            }
            if self.kinds[..i].iter().any(|r| r.kind == rule.kind) {
                return bad("a task kind is listed twice");
            }
        }
        if self.capacity == 0 || self.capacity > MAX_CAPACITY {
            return bad("capacity is one to eight");
        }
        if self.repair_limit > MAX_REPAIRS {
            return bad("the repair limit is zero to two");
        }
        Ok(())
    }

    /// The fields in a fixed order, as stored and as answered.
    pub fn to_json(&self) -> Json {
        object([
            ("name", text(self.name.clone())),
            (
                "repositories",
                Json::List(
                    self.repositories
                        .iter()
                        .map(|r| text(format!("{:032x}", r.0.0)))
                        .collect(),
                ),
            ),
            ("instructions", text(self.instructions.clone())),
            ("test_command", command_json(&self.test_command)),
            ("ci_command", command_json(&self.ci_command)),
            (
                "kinds",
                Json::List(
                    self.kinds
                        .iter()
                        .map(|r| {
                            object([
                                ("kind", text(r.kind.clone())),
                                ("provider", text(provider_name(r.provider))),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "default_provider",
                text(provider_name(self.default_provider)),
            ),
            ("capacity", number(self.capacity.into())),
            ("repair_limit", number(self.repair_limit.into())),
            ("review", Json::Bool(self.review)),
        ])
    }

    /// Reads the record's fields from `p`; other members are the caller's to check.
    pub fn from_json(p: &Json) -> Result<Self, String> {
        let name = string(get(p, "name")?)?.to_owned();
        let Json::List(repos) = get(p, "repositories")? else {
            return Err("InvalidProject: repositories is not a list".into());
        };
        let repositories = repos
            .iter()
            .map(|r| id(r).map(RepositoryId))
            .collect::<Result<_, _>>()?;
        let instructions = string(get(p, "instructions")?)?.to_owned();
        let test_command = words(get(p, "test_command")?, "test command")?;
        let ci_command = words(get(p, "ci_command")?, "CI command")?;
        let Json::List(rules) = get(p, "kinds")? else {
            return Err("InvalidProject: kinds is not a list".into());
        };
        let kinds = rules
            .iter()
            .map(|r| {
                Ok(KindRule {
                    kind: string(get(r, "kind")?)?.to_owned(),
                    provider: provider_named(string(get(r, "provider")?)?)
                        .ok_or("UnknownProvider")?,
                })
            })
            .collect::<Result<_, String>>()?;
        let default_provider =
            provider_named(string(get(p, "default_provider")?)?).ok_or("UnknownProvider")?;
        let capacity = u32::try_from(uint(get(p, "capacity")?)?)
            .map_err(|_| "InvalidProject: capacity is one to eight")?;
        // M4: absent in a record saved before M4.
        let repair_limit = match get(p, "repair_limit").ok() {
            None => MAX_REPAIRS,
            Some(v) => u8::try_from(uint(v)?)
                .map_err(|_| "InvalidProject: the repair limit is zero to two")?,
        };
        let review = match get(p, "review").ok() {
            None => true,
            Some(Json::Bool(b)) => *b,
            Some(_) => return Err("InvalidProject: review is not a boolean".into()),
        };
        let record = Self {
            name,
            repositories,
            instructions,
            test_command,
            ci_command,
            kinds,
            default_provider,
            capacity,
            repair_limit,
            review,
        };
        record.validate()?;
        Ok(record)
    }

    pub fn canonical(&self) -> String {
        crate::ipc::encode(&self.to_json())
    }

    /// The canonical text of a record saved before M4: without the two M4 fields, which
    /// then hold their defaults. `None` when they do not.
    fn legacy_canonical(&self) -> Option<Vec<u8>> {
        if self.repair_limit != MAX_REPAIRS || !self.review {
            return None;
        }
        let Json::Object(mut fields) = self.to_json() else {
            return None;
        };
        fields.truncate(fields.len() - 2);
        Some(crate::ipc::encode(&Json::Object(fields)).into_bytes())
    }

    pub fn import(bytes: &[u8]) -> Result<Self, String> {
        let value = json::parse(bytes).map_err(|e| format!("{e:?}"))?;
        let record = Self::from_json(&value)?;
        if record.canonical().as_bytes() != bytes
            && record.legacy_canonical().as_deref() != Some(bytes)
        {
            return Err("project record is not canonical".into());
        }
        Ok(record)
    }

    /// The provider of a step: the project's rule for the step's kind first, then the
    /// provider the lead named, then the project's default. A kind the project does not
    /// list is refused rather than guessed.
    pub fn provider(
        &self,
        kind: Option<&str>,
        lead: Option<ProviderKind>,
    ) -> Result<ProviderKind, String> {
        if let Some(kind) = kind {
            let Some(rule) = self.kinds.iter().find(|r| r.kind == kind) else {
                return Err(format!(
                    "the task kind {kind:?} is not one of the project's"
                ));
            };
            return Ok(rule.provider);
        }
        Ok(lead.unwrap_or(self.default_provider))
    }

    /// What every lead turn is told about the project.
    pub fn prompt_section(&self) -> String {
        let mut out = format!("\n# Project \"{}\"\n", self.name);
        if self.kinds.is_empty() {
            out.push_str("Task kinds: none defined; give null for \"kind\".\n");
        } else {
            out.push_str("Task kinds and the provider each one goes to:\n");
            for rule in &self.kinds {
                out.push_str(&format!(
                    "- {}: {}\n",
                    rule.kind,
                    provider_name(rule.provider)
                ));
            }
        }
        out.push_str(&format!(
            "Default provider: {}\n",
            provider_name(self.default_provider)
        ));
        if !self.test_command.is_empty() {
            out.push_str(&format!(
                "Usual test command: {}\n",
                crate::ipc::encode(&command_json(&self.test_command))
            ));
        }
        if !self.instructions.trim().is_empty() {
            out.push_str("Instructions for every agent session:\n");
            out.push_str(&self.instructions);
            out.push('\n');
        }
        out
    }

    /// The section the helper appends to a lead's ADR draft, so that the instructions are
    /// approved with it and reach every attempt through the ADR.
    pub fn adr_section(&self, project: &str, revision: u64) -> String {
        let mut out = format!(
            "\n\n## Project instructions\n\nProject \"{}\" ({project}), revision {revision}.\n",
            self.name
        );
        if self.instructions.trim().is_empty() {
            out.push_str("\nNone.\n");
        } else {
            out.push('\n');
            out.push_str(&self.instructions);
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::OpaqueId;

    fn sample() -> ProjectRecord {
        ProjectRecord {
            name: "Crew".into(),
            repositories: vec![RepositoryId(OpaqueId(7))],
            instructions: "Keep functions short.".into(),
            test_command: vec!["/usr/bin/make".into(), "test".into()],
            ci_command: vec![],
            kinds: vec![
                KindRule {
                    kind: "backend".into(),
                    provider: ProviderKind::ClaudeCode,
                },
                KindRule {
                    kind: "mobile".into(),
                    provider: ProviderKind::Codex,
                },
            ],
            default_provider: ProviderKind::ClaudeCode,
            capacity: DEFAULT_CAPACITY,
            repair_limit: MAX_REPAIRS,
            review: true,
        }
    }

    #[test]
    fn canonical_round_trip() {
        let record = sample();
        record.validate().unwrap();
        let body = record.canonical();
        assert_eq!(ProjectRecord::import(body.as_bytes()).unwrap(), record);
        let spaced = body.replacen(',', ", ", 1);
        assert!(ProjectRecord::import(spaced.as_bytes()).is_err());
    }

    #[test]
    fn m4_record_saved_before_m4_reads_as_two_repairs_with_the_reviewer() {
        let record = sample();
        let Json::Object(mut fields) = record.to_json() else {
            unreachable!()
        };
        fields.truncate(fields.len() - 2);
        let old = crate::ipc::encode(&Json::Object(fields));
        assert_eq!(ProjectRecord::import(old.as_bytes()).unwrap(), record);
        let off = ProjectRecord {
            repair_limit: 0,
            review: false,
            ..record
        };
        assert_eq!(
            ProjectRecord::import(off.canonical().as_bytes()).unwrap(),
            off
        );
        let body = off.canonical();
        assert!(body.contains(r#""repair_limit":0"#) && body.contains(r#""review":false"#));
    }

    #[test]
    fn kind_rule_wins_then_lead_then_default() {
        let record = sample();
        let codex = Some(ProviderKind::Codex);
        assert_eq!(
            record.provider(Some("backend"), codex),
            Ok(ProviderKind::ClaudeCode)
        );
        assert_eq!(record.provider(None, codex), Ok(ProviderKind::Codex));
        assert_eq!(record.provider(None, None), Ok(ProviderKind::ClaudeCode));
        assert!(record.provider(Some("docs"), codex).is_err());
    }

    #[test]
    fn invalid_records_are_refused() {
        let cases: [fn(&mut ProjectRecord); 8] = [
            |r| r.name = " ".into(),
            |r| r.repositories.clear(),
            |r| r.repositories.push(r.repositories[0]),
            |r| r.kinds[1].kind = "Backend".into(),
            |r| r.kinds[1].kind = "backend".into(),
            |r| r.capacity = 0,
            |r| r.test_command = vec!["make".into()],
            |r| r.repair_limit = 3,
        ];
        for change in cases {
            let mut record = sample();
            change(&mut record);
            assert!(record.validate().is_err(), "{record:?}");
        }
    }
}

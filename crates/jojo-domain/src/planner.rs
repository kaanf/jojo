//! Lead agent contract (M2, Feature Chat). Pure: no I/O, no launch, no ambient state.
//!
//! A turn is one fresh, read-only provider run in a registered repository. It is not an
//! attempt: there is no approved plan, worktree or candidate behind it, so it has its own
//! frozen request ([`TurnSpec`]) instead of a `LaunchSpec`. Three things are fixed here:
//!
//! * the envelope: [`turn_argv`] is the one argument vector per provider, and
//!   [`check_turn`]/[`check_turn_environment`] are the rules a request must satisfy
//!   before it may cross the OS boundary;
//! * the prompt: [`discover_prompt`] and [`plan_prompt`] are composed only from what the
//!   human typed and what the core stores (questions, answers, the PRD);
//! * the reply: [`parse_reply`] reads the last message of a turn as one strict JSON
//!   object. Anything else is no reply at all; nothing is repaired or guessed.
//!
//! A reply never approves anything. Its PRD, ADR and plan become unapproved drafts and
//! its questions become clarification questions, all authored by the model.
//!
//! Prompt content is adapted, with attribution, from three pinned sources
//! (`docs/roadmap/SOURCE_FIRST_REUSE_MAP.md`, section 0): github/spec-kit (MIT, GitHub,
//! Inc.) `templates/commands/specify.md` and `clarify.md` for when to ask instead of
//! assuming and how many questions; addyosmani/factory (MIT, Addy Osmani)
//! `factory-spec/SKILL.md` for "what we are deliberately not doing" and the least
//! confident decisions; Kilbex/Vigla (Apache-2.0) `mission_supervisor_run/prompts.rs` for
//! reading the codebase before decomposing into a few independently mergeable tasks.
use crate::claude::{CLAUDE_DISCOVERY_STREAM_JSON, CLAUDE_VARIABLES};
use crate::codex::{CODEX_DISCOVERY_JSONL, CODEX_HOME_VARIABLE};
use crate::dependency::NodeRef;
use crate::identity::{OpaqueId, Revision};
use crate::json::{self, Json, JsonError, element_path, member_path};
use crate::prd::{PrdDocument, PrdError};
use crate::provider::{
    MAX_PROMPT_BYTES, PROVIDER_SCHEMA, ProviderError, ProviderFingerprint, ProviderKind, Role,
};
use crate::validation::{ScopePath, StepValidation, ValidationPlan};
use std::path::{Component, Path, PathBuf};

/// `schema` member of a discovery reply.
pub const DISCOVERY_SCHEMA: &str = "jojo-lead-discovery-v1";
/// `schema` member of a plan reply.
pub const PLAN_SCHEMA: &str = "jojo-lead-plan-v1";
/// Most questions one reply may ask (spec-kit's clarification limit per session).
pub const MAX_QUESTIONS: usize = 5;
/// Most steps one draft plan may have (Vigla's decomposition limit).
pub const MAX_STEPS: usize = 6;
pub const MAX_SUMMARY_BYTES: usize = 4 * 1024;
pub const MAX_QUESTION_BYTES: usize = 2 * 1024;
pub const MAX_TITLE_BYTES: usize = 256;
/// The bound the storage layer puts on an ADR text.
pub const MAX_ADR_BYTES: usize = 32 * 1024;
/// Longest human text a prompt carries: the objective, an answer or a note.
pub const MAX_HUMAN_TEXT_BYTES: usize = 16 * 1024;
const MAX_KEY_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TurnKind {
    /// Reads the repository, asks questions and drafts or revises the PRD.
    Discover,
    /// With an approved PRD: drafts the ADR, the steps and their validation.
    Plan,
    /// M4: reads a settled step's candidate and judges it (`review.rs`). Not a chat turn:
    /// it is started by the plan runner, never requested by name.
    Review,
    /// Step 7: reads a registered repository and writes its project brief (`brief.rs`).
    /// Not a chat turn: it belongs to a repository, not to a feature.
    Brief,
}

impl TurnKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Discover => "Discover",
            Self::Plan => "Plan",
            Self::Review => "Review",
            Self::Brief => "Brief",
        }
    }

    /// A chat turn's kind by name. `Review` is not one.
    pub fn named(name: &str) -> Option<Self> {
        [Self::Discover, Self::Plan]
            .into_iter()
            .find(|kind| kind.name() == name)
    }
}

/// The parser fingerprint of a lead agent turn on `provider`.
pub fn turn_parser(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::ClaudeCode => CLAUDE_DISCOVERY_STREAM_JSON,
        ProviderKind::Codex => CODEX_DISCOVERY_JSONL,
    }
}

/// The frozen request of one turn. Every field is explicit; nothing is discovered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TurnSpec {
    pub turn: OpaqueId,
    pub kind: TurnKind,
    /// The adapter's fingerprint with [`turn_parser`] as its parser.
    pub fingerprint: ProviderFingerprint,
    pub prompt: Vec<u8>,
    pub prompt_digest: String,
    /// The registered repository; the working directory of the run. Read, never written.
    pub repository: PathBuf,
    /// The commit the repository's HEAD named when the turn was accepted.
    pub base_commit: String,
    /// Where the raw streams of the run are kept. Outside the repository.
    pub raw_output_dir: PathBuf,
    /// Names only, never inherited secret values.
    pub environment_allowlist: Vec<String>,
    pub timeout_millis: u64,
    pub max_output_bytes: u64,
    pub max_frame_bytes: u64,
}

/// Absolute and already in normal form: no `.`/`..`, doubled or trailing separator.
fn plain_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
        && path.components().collect::<PathBuf>().as_os_str() == path.as_os_str()
}

impl TurnSpec {
    /// A chat or brief turn is the planner and a review turn the reviewer; no other role can be
    /// expressed. Both are read-only.
    pub fn role(&self) -> Role {
        match self.kind {
            TurnKind::Discover | TurnKind::Plan | TurnKind::Brief => Role::Planner,
            TurnKind::Review => Role::Reviewer,
        }
    }

    /// Structural boundary, like `LaunchSpec::validate`: not admission and not approval.
    pub fn validate(&self) -> Result<(), ProviderError> {
        let commit = &self.base_commit;
        if self.fingerprint.schema != PROVIDER_SCHEMA
            || self.prompt.is_empty()
            || self.prompt.len() > MAX_PROMPT_BYTES
            || self.prompt_digest.trim().is_empty()
            || !matches!(commit.len(), 40 | 64)
            || !commit.bytes().all(|b| b.is_ascii_hexdigit())
            || self.timeout_millis == 0
            || self.max_frame_bytes == 0
            || self.max_frame_bytes > self.max_output_bytes
            || ![
                &self.repository,
                &self.raw_output_dir,
                &self.fingerprint.executable,
            ]
            .into_iter()
            .all(|p| plain_absolute(p))
            || self.raw_output_dir.starts_with(&self.repository)
            || self.repository.starts_with(&self.raw_output_dir)
        {
            return Err(ProviderError::InvalidSpec);
        }
        Ok(())
    }
}

/// The rules of the two lead agent profiles on top of [`TurnSpec::validate`]: a plain
/// version text (D3), the turn parser and exactly the provider's own variables.
pub fn check_turn(spec: &TurnSpec) -> Result<(), ProviderError> {
    spec.validate()?;
    let fingerprint = &spec.fingerprint;
    let (digest, variables): (_, &[&str]) = match fingerprint.provider {
        ProviderKind::ClaudeCode => (
            crate::claude::configured_digest(&fingerprint.configuration),
            &CLAUDE_VARIABLES,
        ),
        ProviderKind::Codex => (
            crate::codex::configured_digest(&fingerprint.configuration),
            &[CODEX_HOME_VARIABLE],
        ),
    };
    if fingerprint.parser != turn_parser(fingerprint.provider)
        || !crate::provider::plain_version(&fingerprint.version)
        || digest.is_none()
    {
        return Err(ProviderError::FingerprintMismatch);
    }
    if spec.environment_allowlist != variables {
        return Err(ProviderError::InvalidSpec);
    }
    Ok(())
}

/// The one argument vector of a turn on the spec's provider.
pub fn turn_argv(spec: &TurnSpec) -> Result<Vec<String>, ProviderError> {
    match spec.fingerprint.provider {
        ProviderKind::ClaudeCode => Ok(crate::claude::discovery_argv()),
        ProviderKind::Codex => crate::codex::discovery_argv(&spec.repository),
    }
}

/// The child receives exactly the provider's own variables. Returns the home they name.
/// As for an attempt: the Claude Code home may hold the repository but may not lie inside
/// it or the output directory; the Codex home may not overlap either in any direction.
pub fn check_turn_environment<'a>(
    spec: &TurnSpec,
    environment: &'a [(String, String)],
) -> Result<&'a Path, ProviderError> {
    let names: Vec<&str> = environment.iter().map(|(name, _)| name.as_str()).collect();
    if names != spec.environment_allowlist {
        return Err(ProviderError::InvalidSpec);
    }
    let others = [&spec.repository, &spec.raw_output_dir];
    let (home, separate) = match (spec.fingerprint.provider, environment) {
        (ProviderKind::ClaudeCode, [(_, home), (_, user)]) => {
            let plain_user = !user.is_empty()
                && user.len() <= 64
                && user
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
            let home = Path::new(home);
            (
                home,
                plain_user && !others.iter().any(|other| home.starts_with(other)),
            )
        }
        (ProviderKind::Codex, [(_, home)]) => {
            let home = Path::new(home);
            (
                home,
                !others
                    .iter()
                    .any(|other| home.starts_with(other) || other.starts_with(home)),
            )
        }
        _ => return Err(ProviderError::InvalidSpec),
    };
    if !separate || !plain_absolute(home) || home.parent().is_none() {
        return Err(ProviderError::InvalidSpec);
    }
    Ok(home)
}

/// One stored clarification question as a prompt carries it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Exchange {
    pub prompt: String,
    pub blocking: bool,
    /// The newest human answer, if there is one.
    pub answer: Option<String>,
}

fn exchanges(out: &mut String, list: &[Exchange]) {
    out.push_str("\n# Questions and answers so far\n");
    if list.is_empty() {
        out.push_str("None.\n");
    }
    for (index, exchange) in list.iter().enumerate() {
        out.push_str(&format!(
            "\nQ{} ({}): {}\n",
            index + 1,
            if exchange.blocking {
                "blocking"
            } else {
                "deferred"
            },
            exchange.prompt
        ));
        match &exchange.answer {
            Some(answer) => out.push_str(&format!("Answer: {answer}\n")),
            None => out.push_str("Answer: (not answered)\n"),
        }
    }
}

pub(crate) const REPLY_RULES: &str = "Your last message must be exactly one JSON object and nothing else: \
no prose before or after it and no code fence. Use only integers, strings, booleans, null, \
arrays and objects; every key shown must be present and no other key may appear.";

const PRD_SHAPE: &str = r#"{"schema_version":1,"title":"...","goal":"...","non_goals":["..."],
 "actors":[{"id":"A1","name":"..."}],
 "use_cases":[{"id":"UC1","title":"...","actor_ids":["A1"],"preconditions":["..."],
   "main_flow":["..."],"alternate_flows":[{"id":"UC1-A1","title":"...","steps":["..."]}],
   "error_flows":[{"id":"UC1-E1","title":"...","steps":["..."]}],"postconditions":["..."],
   "acceptance_ids":["AC1"]}],
 "acceptance_criteria":[{"id":"AC1","statement":"..."}],
 "nfrs":[{"id":"NFR1","statement":"..."}],
 "assumptions":[{"id":"AS1","statement":"..."}],
 "blockers":[{"id":"B1","statement":"..."}]}"#;

/// The prompt of a discovery turn. `draft` is the canonical JSON of the newest PRD
/// version, when one exists; `note` is what the human typed for this turn after the
/// first one.
pub fn discover_prompt(
    objective: &str,
    list: &[Exchange],
    draft: Option<&str>,
    note: Option<&str>,
    project: Option<&str>,
) -> String {
    let mut out = String::from(
        "You are the lead engineer of this repository, in discovery for one feature. You \
         may only read: list and read what you need in the current directory, change \
         nothing, and do not try to work around a missing tool.\n\n\
         Before anything else, skim the README, any agent instruction document, the \
         top-level package manifest and a shallow view of the directory tree, so that \
         what you write names things that exist here.\n\n# Objective from the product owner\n",
    );
    out.push_str(objective);
    out.push('\n');
    exchanges(&mut out, list);
    if let Some(note) = note {
        out.push_str("\n# Note from the product owner for this round\n");
        out.push_str(note);
        out.push('\n');
    }
    match draft {
        Some(draft) => {
            out.push_str(
                "\n# Current requirements draft\nRevise it from the answers above; keep the \
                 identifiers of statements that did not change.\n",
            );
            out.push_str(draft);
            out.push('\n');
        }
        None => out.push_str("\n# Current requirements draft\nNone yet.\n"),
    }
    if let Some(project) = project {
        out.push_str(project);
    }
    out.push_str(&format!(
        "\n# What to do\n\
         1. Decide what is still unclear. Make an informed guess wherever a reasonable \
         default exists and record it as an assumption. Ask only when the choice changes \
         scope or user experience, several readings with different consequences exist, and \
         no reasonable default exists. Priority: scope, then security and privacy, then \
         user experience, then technical detail. Never repeat a question that is listed \
         above. At most {MAX_QUESTIONS} new questions; each must be answerable in a short \
         phrase or by choosing among a few options you name. Mark a question blocking when \
         the requirements cannot be reviewed without its answer.\n\
         2. Write the requirements draft as soon as you can state the goal. It describes \
         what users can do and how that is verified, never an implementation: no file, \
         function or library names. Say what is deliberately not being done in \
         \"non_goals\". Every acceptance criterion must be testable and unambiguous. List \
         something you could not resolve as a blocker, with the question that resolves it. \
         If you cannot yet state the goal, give null for \"prd\" and ask. The draft can be \
         offered for review only when it has at least one actor, one use case and one \
         acceptance criterion; every use case names its actors and its acceptance criteria \
         and has at least one precondition, main-flow step, alternate flow, error flow and \
         postcondition; no text is a placeholder such as TODO or TBD; and no blocker is \
         left.\n\
         3. In \"summary\", tell the product owner in a few sentences what you found in \
         the repository and what changed in the draft this round.\n\n\
         # Reply\n{REPLY_RULES}\n\
         {{\"schema\":\"{DISCOVERY_SCHEMA}\",\"summary\":\"...\",\
         \"questions\":[{{\"prompt\":\"...\",\"blocking\":true}}],\"prd\":null}}\n\
         where \"prd\" is null or an object of exactly this shape (a list you have \
         nothing for is written as []; identifiers are short, unique across the whole \
         document and made of letters, digits, '-', '_' and '.'):\n{PRD_SHAPE}\n"
    ));
    out
}

/// The prompt of a plan turn. `prd` is the rendered text of the approved PRD revision.
pub fn plan_prompt(
    objective: &str,
    list: &[Exchange],
    prd_revision: u64,
    prd: &str,
    note: Option<&str>,
    project: Option<&str>,
) -> String {
    let mut out = String::from(
        "You are the lead engineer of this repository, planning one feature whose \
         requirements are approved. You may only read: list and read what you need in the \
         current directory, change nothing, and do not try to work around a missing \
         tool.\n\nBefore decomposing, skim the README, any agent instruction document, the \
         top-level package manifest and a shallow view of the directory tree, and read the \
         code the feature touches.\n\n# Objective from the product owner\n",
    );
    out.push_str(objective);
    out.push('\n');
    exchanges(&mut out, list);
    if let Some(note) = note {
        out.push_str("\n# Note from the product owner for this round\n");
        out.push_str(note);
        out.push('\n');
    }
    out.push_str(&format!(
        "\n# Product requirements (approved revision {prd_revision})\n{prd}\n"
    ));
    if let Some(project) = project {
        out.push_str(project);
    }
    out.push_str(&format!(
        "\n# What to do\n\
         1. Write the architecture decision as Markdown in \"adr\": which existing modules \
         the feature touches, the new data shapes and the call flow in order, what could \
         break elsewhere, the alternatives you rejected, and the three decisions you are \
         least confident about.\n\
         2. Decompose the work into between 1 and {MAX_STEPS} steps. Each step is done by \
         one coding agent in its own copy of the repository and must be independently \
         mergeable and independently testable. Prefer a first step that proves the shape \
         end to end. Reference real paths of this repository.\n\
         3. For each step give: a short unique \"key\"; a \"title\"; its task \"kind\", \
         one of the project's task kinds or null (a kind decides the provider); the \
         \"provider\" that should do it when the kind does not, \"Codex\", \
         \"ClaudeCode\" or null for the project's default; \"depends_on\", the keys of \
         earlier steps whose result it needs (a step may only depend on steps listed \
         before it; steps without a common dependency run in parallel, so they must not \
         write the same paths); \"write_scope\", the repository-relative paths or \
         directories it may change, never empty; \"baseline\", paths that must stay \
         byte-identical, such as the tests that judge it; \"tests\", exactly one short \
         name for its check; and \"command\", the argument vector that Jojo runs in the \
         repository root after the step, with an absolute program path, where exit \
         status 0 means the step passed.\n\
         4. Ask a question only when a step cannot be planned without the answer. Give \
         the key of the one step it blocks in \"step\", or null when it blocks the whole \
         plan. At most {MAX_QUESTIONS} questions, none that is listed above.\n\
         5. In \"summary\", tell the product owner in a few sentences how the plan is cut \
         and why.\n\n\
         # Reply\n{REPLY_RULES}\n\
         {{\"schema\":\"{PLAN_SCHEMA}\",\"summary\":\"...\",\"adr\":\"...\",\
         \"steps\":[{{\"key\":\"s1\",\"title\":\"...\",\"kind\":null,\"provider\":\"Codex\",\
         \"depends_on\":[],\"write_scope\":[\"...\"],\"baseline\":[\"...\"],\
         \"tests\":[\"...\"],\"command\":[\"/usr/bin/...\",\"...\"]}}],\
         \"questions\":[{{\"prompt\":\"...\",\"blocking\":true,\"step\":null}}]}}\n"
    ));
    out
}

/// A question the lead asks. `step` is the key of the one draft step it blocks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LeadQuestion {
    pub prompt: String,
    pub blocking: bool,
    pub step: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Discovery {
    pub summary: String,
    pub questions: Vec<LeadQuestion>,
    pub prd: Option<PrdDocument>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DraftStep {
    pub key: String,
    pub title: String,
    /// M3: one of the project's task kinds; a reply without the member has none.
    pub kind: Option<String>,
    /// The provider the lead named, if any. The step's provider is resolved against the
    /// project record ([`PlanDraft::providers`]).
    pub provider: Option<ProviderKind>,
    /// Keys of steps listed before this one.
    pub depends_on: Vec<String>,
    pub write_scope: Vec<ScopePath>,
    pub baseline: Vec<ScopePath>,
    pub tests: Vec<String>,
    /// The host validation argument vector; never empty in a draft.
    pub command: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanDraft {
    pub summary: String,
    pub adr: String,
    pub steps: Vec<DraftStep>,
    pub questions: Vec<LeadQuestion>,
}

fn value_of<'a>(value: &'a Json, key: &str) -> Option<&'a Json> {
    match value {
        Json::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

impl PlanDraft {
    /// M3: the provider of every step, in order, under the project the plan is drafted
    /// in. Without a project a step must name its provider and no kind.
    pub fn providers(
        &self,
        project: Option<&crate::project::ProjectRecord>,
    ) -> Result<Vec<ProviderKind>, String> {
        self.steps
            .iter()
            .map(|step| match project {
                Some(project) => project
                    .provider(step.kind.as_deref(), step.provider)
                    .map_err(|why| format!("step {}: {why}", step.key)),
                None if step.kind.is_some() => Err(format!(
                    "step {}: a task kind needs a project record",
                    step.key
                )),
                None => step.provider.ok_or_else(|| {
                    format!("step {}: no provider and no project default", step.key)
                }),
            })
            .collect()
    }
    /// The validation plan of the draft, with `ids[i]` as the identity of step `i`.
    pub fn validation(&self, ids: &[OpaqueId]) -> Result<ValidationPlan, String> {
        if ids.len() != self.steps.len() {
            return Err("one identity per draft step is required".into());
        }
        let plan = ValidationPlan {
            steps: self
                .steps
                .iter()
                .zip(ids)
                .map(|(step, id)| StepValidation {
                    step: NodeRef {
                        id: *id,
                        revision: Revision(1),
                    },
                    write_scope: step.write_scope.clone(),
                    baseline: step.baseline.clone(),
                    tests: step.tests.clone(),
                    command: step.command.clone(),
                })
                .collect(),
        };
        plan.validate()?;
        Ok(plan)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LeadReply {
    Discovery(Discovery),
    Plan(PlanDraft),
    Review(crate::review::Verdict),
    Brief(crate::brief::Brief),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LeadError {
    /// The last message is not one strict JSON value.
    Json(JsonError),
    /// A member is missing, unknown, of the wrong type or outside its bound.
    Field { path: String, problem: &'static str },
    /// The `prd` member is not a PRD document.
    Prd(PrdError),
    /// The steps do not make a validation plan.
    Plan(String),
}

pub(crate) fn bad(path: impl Into<String>, problem: &'static str) -> LeadError {
    LeadError::Field {
        path: path.into(),
        problem,
    }
}

/// One JSON object being decoded: every member must be consumed exactly once.
pub(crate) struct Members<'a> {
    path: &'a str,
    members: &'a [(String, Json)],
    taken: Vec<&'static str>,
}

impl<'a> Members<'a> {
    pub(crate) fn of(value: &'a Json, path: &'a str) -> Result<Self, LeadError> {
        match value {
            Json::Object(members) => Ok(Self {
                path,
                members,
                taken: Vec::new(),
            }),
            _ => Err(bad(path, "not an object")),
        }
    }

    pub(crate) fn take(&mut self, key: &'static str) -> Result<(&'a Json, String), LeadError> {
        let path = member_path(self.path, key);
        self.taken.push(key);
        match self.members.iter().find(|(name, _)| name == key) {
            Some((_, value)) => Ok((value, path)),
            None => Err(bad(path, "missing")),
        }
    }

    /// Non-empty text of at most `limit` bytes. Line breaks are kept when `lines`.
    pub(crate) fn text(
        &mut self,
        key: &'static str,
        limit: usize,
        lines: bool,
    ) -> Result<String, LeadError> {
        let (value, path) = self.take(key)?;
        text(value, &path, limit, lines)
    }

    fn list<T>(
        &mut self,
        key: &'static str,
        limit: usize,
        mut item: impl FnMut(&'a Json, &str) -> Result<T, LeadError>,
    ) -> Result<Vec<T>, LeadError> {
        let (value, path) = self.take(key)?;
        let Json::List(items) = value else {
            return Err(bad(path, "not an array"));
        };
        if items.len() > limit {
            return Err(bad(path, "too many entries"));
        }
        items
            .iter()
            .enumerate()
            .map(|(index, value)| item(value, &element_path(&path, index)))
            .collect()
    }

    pub(crate) fn finish(self) -> Result<(), LeadError> {
        match self
            .members
            .iter()
            .find(|(name, _)| !self.taken.contains(&name.as_str()))
        {
            Some((name, _)) => Err(bad(member_path(self.path, name), "unknown member")),
            None => Ok(()),
        }
    }
}

pub(crate) fn text(
    value: &Json,
    path: &str,
    limit: usize,
    lines: bool,
) -> Result<String, LeadError> {
    let Json::Text(text) = value else {
        return Err(bad(path, "not a string"));
    };
    let control = |c: char| c.is_control() && !(lines && matches!(c, '\n' | '\t'));
    if !text.chars().any(char::is_alphanumeric) {
        Err(bad(path, "empty"))
    } else if text.len() > limit {
        Err(bad(path, "too long"))
    } else if text.chars().any(control) {
        Err(bad(path, "control character"))
    } else {
        Ok(text.clone())
    }
}

fn key(value: &Json, path: &str) -> Result<String, LeadError> {
    let key = text(value, path, MAX_KEY_BYTES, false)?;
    if key
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        Ok(key)
    } else {
        Err(bad(path, "malformed key"))
    }
}

fn question(value: &Json, path: &str, with_step: bool) -> Result<LeadQuestion, LeadError> {
    let mut members = Members::of(value, path)?;
    let prompt = members.text("prompt", MAX_QUESTION_BYTES, true)?;
    let blocking = match members.take("blocking")? {
        (Json::Bool(blocking), _) => *blocking,
        (_, path) => return Err(bad(path, "not a boolean")),
    };
    let step = if with_step {
        match members.take("step")? {
            (Json::Null, _) => None,
            (value, path) => Some(key(value, &path)?),
        }
    } else {
        None
    };
    members.finish()?;
    Ok(LeadQuestion {
        prompt,
        blocking,
        step,
    })
}

fn questions(members: &mut Members<'_>, with_step: bool) -> Result<Vec<LeadQuestion>, LeadError> {
    let list = members.list("questions", MAX_QUESTIONS, |v, p| question(v, p, with_step))?;
    for (index, question) in list.iter().enumerate() {
        if list[..index].iter().any(|q| q.prompt == question.prompt) {
            return Err(bad(
                format!("questions[{index}].prompt"),
                "repeated question",
            ));
        }
    }
    Ok(list)
}

pub(crate) fn schema(members: &mut Members<'_>, expected: &str) -> Result<(), LeadError> {
    match members.take("schema")? {
        (Json::Text(schema), _) if schema == expected => Ok(()),
        (_, path) => Err(bad(path, "not the schema of this turn")),
    }
}

fn discovery(root: &Json) -> Result<Discovery, LeadError> {
    let mut members = Members::of(root, "")?;
    schema(&mut members, DISCOVERY_SCHEMA)?;
    let summary = members.text("summary", MAX_SUMMARY_BYTES, true)?;
    let questions = questions(&mut members, false)?;
    let prd = match members.take("prd")? {
        (Json::Null, _) => None,
        (value, _) => Some(PrdDocument::from_json(value).map_err(LeadError::Prd)?),
    };
    members.finish()?;
    if prd.is_none() && questions.is_empty() {
        return Err(bad("questions", "neither a question nor a draft"));
    }
    Ok(Discovery {
        summary,
        questions,
        prd,
    })
}

fn step(value: &Json, path: &str, earlier: &[DraftStep]) -> Result<DraftStep, LeadError> {
    let has_kind = value_of(value, "kind").is_some();
    let mut members = Members::of(value, path)?;
    let (value, key_path) = members.take("key")?;
    let step_key = key(value, &key_path)?;
    if earlier.iter().any(|s| s.key == step_key) {
        return Err(bad(key_path, "repeated key"));
    }
    let title = members.text("title", MAX_TITLE_BYTES, false)?;
    // M3: the member is optional so that a reply written before task kinds still reads.
    let kind = if has_kind {
        match members.take("kind")? {
            (Json::Null, _) => None,
            (Json::Text(kind), _) if crate::project::valid_kind(kind) => Some(kind.clone()),
            (_, path) => return Err(bad(path, "malformed task kind")),
        }
    } else {
        None
    };
    let provider = match members.take("provider")? {
        (Json::Text(name), _) if name == "Codex" => Some(ProviderKind::Codex),
        (Json::Text(name), _) if name == "ClaudeCode" => Some(ProviderKind::ClaudeCode),
        (Json::Null, _) => None,
        (_, path) => return Err(bad(path, "unknown provider")),
    };
    let depends_on = members.list("depends_on", MAX_STEPS, |v, p| {
        let wanted = key(v, p)?;
        if earlier.iter().any(|s| s.key == wanted) {
            Ok(wanted)
        } else {
            Err(bad(p, "not the key of an earlier step"))
        }
    })?;
    if (1..depends_on.len()).any(|i| depends_on[..i].contains(&depends_on[i])) {
        return Err(bad(member_path(path, "depends_on"), "repeated key"));
    }
    let scope = |v: &Json, p: &str| {
        let entry = text(v, p, 4096, false)?;
        ScopePath::parse(&entry).map_err(|_| bad(p, "not a normal repository path"))
    };
    let write_scope = members.list("write_scope", 64, scope)?;
    let baseline = members.list("baseline", 64, scope)?;
    let tests = members.list("tests", 1, |v, p| text(v, p, MAX_TITLE_BYTES, false))?;
    let command = members.list(
        "command",
        crate::validation::MAX_COMMAND_ARGS,
        |v, p| match v {
            Json::Text(argument) if argument.len() <= 4096 => Ok(argument.clone()),
            _ => Err(bad(p, "not a bounded string")),
        },
    )?;
    if command.is_empty() {
        return Err(bad(member_path(path, "command"), "empty"));
    }
    members.finish()?;
    Ok(DraftStep {
        key: step_key,
        title,
        kind,
        provider,
        depends_on,
        write_scope,
        baseline,
        tests,
        command,
    })
}

fn plan(root: &Json) -> Result<PlanDraft, LeadError> {
    let mut members = Members::of(root, "")?;
    schema(&mut members, PLAN_SCHEMA)?;
    let summary = members.text("summary", MAX_SUMMARY_BYTES, true)?;
    let adr = members.text("adr", MAX_ADR_BYTES, true)?;
    let mut steps: Vec<DraftStep> = Vec::new();
    let (value, path) = members.take("steps")?;
    let Json::List(items) = value else {
        return Err(bad(path, "not an array"));
    };
    if items.is_empty() || items.len() > MAX_STEPS {
        return Err(bad(path, "needs one to six steps"));
    }
    for (index, item) in items.iter().enumerate() {
        let parsed = step(item, &element_path(&path, index), &steps)?;
        steps.push(parsed);
    }
    let questions = questions(&mut members, true)?;
    members.finish()?;
    if let Some(index) = questions.iter().position(|q| {
        q.step
            .as_ref()
            .is_some_and(|key| !steps.iter().any(|s| &s.key == key))
    }) {
        return Err(bad(
            format!("questions[{index}].step"),
            "not the key of a step",
        ));
    }
    let draft = PlanDraft {
        summary,
        adr,
        steps,
        questions,
    };
    // The same rules the approved validation plan is held to, before anything is stored.
    let ids: Vec<OpaqueId> = (1..=draft.steps.len() as u128).map(OpaqueId).collect();
    draft.validation(&ids).map_err(LeadError::Plan)?;
    Ok(draft)
}

/// The reply without one surrounding Markdown code fence. Nothing else is removed.
pub(crate) fn unfenced(reply: &str) -> &str {
    let trimmed = reply.trim();
    let inner = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|rest| rest.strip_suffix("```"));
    match inner {
        Some(inner) if !inner.contains("```") => inner.trim(),
        _ => trimmed,
    }
}

/// Reads the last message of a turn. The whole message must be one object of the turn's
/// schema; one surrounding code fence is tolerated, nothing else is.
pub fn parse_reply(kind: TurnKind, reply: &str) -> Result<LeadReply, LeadError> {
    let root = json::parse(unfenced(reply).as_bytes()).map_err(LeadError::Json)?;
    match kind {
        TurnKind::Discover => discovery(&root).map(LeadReply::Discovery),
        TurnKind::Plan => plan(&root).map(LeadReply::Plan),
        TurnKind::Review => crate::review::verdict(&root).map(LeadReply::Review),
        TurnKind::Brief => crate::brief::brief(&root).map(LeadReply::Brief),
    }
}

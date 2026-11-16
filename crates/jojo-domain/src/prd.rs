//! Typed PRD/use-case content: strict import, canonical form, review rendering and
//! structural completeness.
//!
//! Three separate questions, deliberately not merged:
//!
//! * [`PrdDocument::import`] — is the input unambiguous, bounded and of the right shape?
//!   Failure means there is no document at all.
//! * [`PrdDocument::validate`] — is a well-formed document structurally complete enough to
//!   put in front of a human? Findings name the field path.
//! * Human review — is the product intent right? No validator here answers that; an empty
//!   finding list is a precondition for review, never a substitute for it.
//!
//! The canonical form is a pure function of the typed document, and the document is a pure
//! function of the canonical form, so digests of either are stable across import/render.

use crate::json::{self, Json, JsonError, element_path, member_path};

/// Version of the typed PRD schema this code reads and writes.
pub const PRD_SCHEMA_VERSION: u32 = 1;
const MAX_ID_BYTES: usize = 64;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrdDocument {
    pub title: String,
    pub goal: String,
    pub non_goals: Vec<String>,
    pub actors: Vec<Actor>,
    pub use_cases: Vec<UseCase>,
    pub acceptance_criteria: Vec<Statement>,
    pub nfrs: Vec<Statement>,
    pub assumptions: Vec<Statement>,
    /// Unresolved blocking items. Resolving one means authoring a new version without it.
    pub blockers: Vec<Statement>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Actor {
    pub id: String,
    pub name: String,
}

/// An identified statement: acceptance criterion, NFR, assumption or blocker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Statement {
    pub id: String,
    pub statement: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UseCase {
    pub id: String,
    pub title: String,
    pub actor_ids: Vec<String>,
    pub preconditions: Vec<String>,
    pub main_flow: Vec<String>,
    pub alternate_flows: Vec<Flow>,
    pub error_flows: Vec<Flow>,
    pub postconditions: Vec<String>,
    pub acceptance_ids: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Flow {
    pub id: String,
    pub title: String,
    pub steps: Vec<String>,
}

/// Why input is not a PRD document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrdError {
    Json(JsonError),
    Field { path: String, problem: FieldProblem },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FieldProblem {
    Missing,
    /// Unknown members are refused rather than silently dropped from the canonical form.
    Unknown,
    WrongType {
        expected: &'static str,
    },
    UnsupportedSchemaVersion(i64),
    /// IDs are 1–64 characters of ASCII letters, digits, `-`, `_` or `.`.
    MalformedId,
    /// IDs share one namespace per document; `first` is where the ID was first defined.
    DuplicateId {
        first: String,
    },
    /// Text is single-line so the rendering cannot fake document structure.
    ControlCharacter,
}

/// A structural reason the document is not ready for human review.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Finding {
    pub path: String,
    pub problem: Incomplete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Incomplete {
    /// Required text with no letters or digits, or a required list with no entries.
    Empty,
    /// Text that marks unfinished work (`TODO`, `TBD`, `???`, …). A heuristic, not proof.
    Placeholder,
    /// A reference to an actor or acceptance ID the document does not define.
    UnknownReference,
    /// The document still lists an unresolved blocker.
    OpenBlocker,
}

impl From<JsonError> for PrdError {
    fn from(error: JsonError) -> Self {
        Self::Json(error)
    }
}

fn field(path: impl Into<String>, problem: FieldProblem) -> PrdError {
    PrdError::Field {
        path: path.into(),
        problem,
    }
}

/// One JSON object being decoded: every member must be consumed exactly once.
struct Members<'a> {
    path: &'a str,
    members: &'a [(String, Json)],
    taken: Vec<&'static str>,
}

impl<'a> Members<'a> {
    fn of(value: &'a Json, path: &'a str) -> Result<Self, PrdError> {
        match value {
            Json::Object(members) => Ok(Self {
                path,
                members,
                taken: Vec::new(),
            }),
            _ => Err(field(path, FieldProblem::WrongType { expected: "object" })),
        }
    }

    fn take(&mut self, key: &'static str) -> Result<(&'a Json, String), PrdError> {
        let path = member_path(self.path, key);
        self.taken.push(key);
        match self.members.iter().find(|(name, _)| name == key) {
            Some((_, value)) => Ok((value, path)),
            None => Err(field(path, FieldProblem::Missing)),
        }
    }

    fn text(&mut self, key: &'static str) -> Result<String, PrdError> {
        let (value, path) = self.take(key)?;
        text(value, &path)
    }

    fn id(&mut self, ids: &mut Ids) -> Result<String, PrdError> {
        let (value, path) = self.take("id")?;
        let id = reference(value, &path)?;
        if let Some((_, first)) = ids.iter().find(|(known, _)| *known == id) {
            return Err(field(
                path,
                FieldProblem::DuplicateId {
                    first: first.clone(),
                },
            ));
        }
        ids.push((id.clone(), path));
        Ok(id)
    }

    fn list<T>(
        &mut self,
        key: &'static str,
        mut item: impl FnMut(&'a Json, &str) -> Result<T, PrdError>,
    ) -> Result<Vec<T>, PrdError> {
        let (value, path) = self.take(key)?;
        let Json::List(items) = value else {
            return Err(field(path, FieldProblem::WrongType { expected: "array" }));
        };
        items
            .iter()
            .enumerate()
            .map(|(index, value)| item(value, &element_path(&path, index)))
            .collect()
    }

    fn finish(self) -> Result<(), PrdError> {
        match self
            .members
            .iter()
            .find(|(name, _)| !self.taken.contains(&name.as_str()))
        {
            Some((name, _)) => Err(field(member_path(self.path, name), FieldProblem::Unknown)),
            None => Ok(()),
        }
    }
}

/// Defined IDs with the path of their definition.
type Ids = Vec<(String, String)>;

fn text(value: &Json, path: &str) -> Result<String, PrdError> {
    match value {
        Json::Text(text) if text.chars().any(char::is_control) => {
            Err(field(path, FieldProblem::ControlCharacter))
        }
        Json::Text(text) => Ok(text.clone()),
        _ => Err(field(path, FieldProblem::WrongType { expected: "string" })),
    }
}

fn reference(value: &Json, path: &str) -> Result<String, PrdError> {
    let id = text(value, path)?;
    let well_formed = !id.is_empty()
        && id.len() <= MAX_ID_BYTES
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if well_formed {
        Ok(id)
    } else {
        Err(field(path, FieldProblem::MalformedId))
    }
}

fn statement(value: &Json, path: &str, ids: &mut Ids) -> Result<Statement, PrdError> {
    let mut members = Members::of(value, path)?;
    let parsed = Statement {
        id: members.id(ids)?,
        statement: members.text("statement")?,
    };
    members.finish()?;
    Ok(parsed)
}

fn flow(value: &Json, path: &str, ids: &mut Ids) -> Result<Flow, PrdError> {
    let mut members = Members::of(value, path)?;
    let parsed = Flow {
        id: members.id(ids)?,
        title: members.text("title")?,
        steps: members.list("steps", text)?,
    };
    members.finish()?;
    Ok(parsed)
}

fn use_case(value: &Json, path: &str, ids: &mut Ids) -> Result<UseCase, PrdError> {
    let mut members = Members::of(value, path)?;
    let parsed = UseCase {
        id: members.id(ids)?,
        title: members.text("title")?,
        actor_ids: members.list("actor_ids", reference)?,
        preconditions: members.list("preconditions", text)?,
        main_flow: members.list("main_flow", text)?,
        alternate_flows: members.list("alternate_flows", |v, p| flow(v, p, ids))?,
        error_flows: members.list("error_flows", |v, p| flow(v, p, ids))?,
        postconditions: members.list("postconditions", text)?,
        acceptance_ids: members.list("acceptance_ids", reference)?,
    };
    members.finish()?;
    Ok(parsed)
}

impl PrdDocument {
    /// Reads a document from bounded JSON. Every member of the schema must be present
    /// (an empty list is written as `[]`); nothing is defaulted, trimmed or ignored.
    pub fn import(input: &[u8]) -> Result<Self, PrdError> {
        Self::from_json(&json::parse(input)?)
    }

    /// The same strict reading of a document that is already parsed, for content that
    /// arrives as a member of a larger object (M2: the lead agent's reply).
    pub fn from_json(root: &Json) -> Result<Self, PrdError> {
        let mut members = Members::of(root, "")?;
        match members.take("schema_version")? {
            (Json::Int(version), _) if *version == i64::from(PRD_SCHEMA_VERSION) => {}
            (Json::Int(version), path) => {
                return Err(field(
                    path,
                    FieldProblem::UnsupportedSchemaVersion(*version),
                ));
            }
            (_, path) => {
                return Err(field(
                    path,
                    FieldProblem::WrongType {
                        expected: "integer",
                    },
                ));
            }
        }
        let mut ids = Ids::new();
        let document = Self {
            title: members.text("title")?,
            goal: members.text("goal")?,
            non_goals: members.list("non_goals", text)?,
            actors: members.list("actors", |value, path| {
                let mut members = Members::of(value, path)?;
                let actor = Actor {
                    id: members.id(&mut ids)?,
                    name: members.text("name")?,
                };
                members.finish()?;
                Ok(actor)
            })?,
            use_cases: members.list("use_cases", |v, p| use_case(v, p, &mut ids))?,
            acceptance_criteria: members
                .list("acceptance_criteria", |v, p| statement(v, p, &mut ids))?,
            nfrs: members.list("nfrs", |v, p| statement(v, p, &mut ids))?,
            assumptions: members.list("assumptions", |v, p| statement(v, p, &mut ids))?,
            blockers: members.list("blockers", |v, p| statement(v, p, &mut ids))?,
        };
        members.finish()?;
        Ok(document)
    }

    /// The one serialization that is hashed: fixed member order, no insignificant
    /// whitespace, one escaping. `import(canonical_json())` returns an equal document.
    pub fn canonical_json(&self) -> String {
        let mut out = format!("{{\"schema_version\":{PRD_SCHEMA_VERSION}");
        write_text(&mut out, "title", &self.title);
        write_text(&mut out, "goal", &self.goal);
        write_texts(&mut out, "non_goals", &self.non_goals);
        write_list(&mut out, "actors", &self.actors, |out, actor| {
            write_first(out, "id", &actor.id);
            write_text(out, "name", &actor.name);
        });
        write_list(&mut out, "use_cases", &self.use_cases, |out, use_case| {
            write_first(out, "id", &use_case.id);
            write_text(out, "title", &use_case.title);
            write_texts(out, "actor_ids", &use_case.actor_ids);
            write_texts(out, "preconditions", &use_case.preconditions);
            write_texts(out, "main_flow", &use_case.main_flow);
            write_list(
                out,
                "alternate_flows",
                &use_case.alternate_flows,
                write_flow,
            );
            write_list(out, "error_flows", &use_case.error_flows, write_flow);
            write_texts(out, "postconditions", &use_case.postconditions);
            write_texts(out, "acceptance_ids", &use_case.acceptance_ids);
        });
        for (key, statements) in [
            ("acceptance_criteria", &self.acceptance_criteria),
            ("nfrs", &self.nfrs),
            ("assumptions", &self.assumptions),
            ("blockers", &self.blockers),
        ] {
            write_list(&mut out, key, statements, |out, statement| {
                write_first(out, "id", &statement.id);
                write_text(out, "statement", &statement.statement);
            });
        }
        out.push('}');
        out
    }

    /// The Markdown a reviewer is shown. Deterministic; every field of the document
    /// appears, including empty sections, so nothing reviewed is implicit.
    pub fn render_markdown(&self) -> String {
        let mut out = format!(
            "# PRD: {}\n\nSchema version: {PRD_SCHEMA_VERSION}\n\n## Goal\n\n{}\n",
            self.title, self.goal
        );
        out.push_str("\n## Non-goals\n\n");
        bullets(&mut out, self.non_goals.iter().cloned());
        out.push_str("\n## Actors\n\n");
        bullets(
            &mut out,
            self.actors.iter().map(|a| labelled(&a.id, &a.name)),
        );
        out.push_str("\n## Use cases\n");
        if self.use_cases.is_empty() {
            out.push_str("\n_None._\n");
        }
        for use_case in &self.use_cases {
            out.push_str(&format!("\n### {} — {}\n", use_case.id, use_case.title));
            out.push_str(&format!("\nActors: {}\n", references(&use_case.actor_ids)));
            numbered(&mut out, "Preconditions", &use_case.preconditions);
            numbered(&mut out, "Main flow", &use_case.main_flow);
            for (kind, flows) in [
                ("Alternate flow", &use_case.alternate_flows),
                ("Error flow", &use_case.error_flows),
            ] {
                if flows.is_empty() {
                    out.push_str(&format!("\n**{kind}s**\n\n_None._\n"));
                }
                for flow in flows {
                    let heading = format!("{kind} {} — {}", flow.id, flow.title);
                    numbered(&mut out, &heading, &flow.steps);
                }
            }
            numbered(&mut out, "Postconditions", &use_case.postconditions);
            out.push_str(&format!(
                "\nAcceptance: {}\n",
                references(&use_case.acceptance_ids)
            ));
        }
        for (heading, statements) in [
            ("Acceptance criteria", &self.acceptance_criteria),
            ("Non-functional requirements", &self.nfrs),
            ("Assumptions", &self.assumptions),
            ("Open blockers", &self.blockers),
        ] {
            out.push_str(&format!("\n## {heading}\n\n"));
            bullets(
                &mut out,
                statements.iter().map(|s| labelled(&s.id, &s.statement)),
            );
        }
        out
    }

    /// Structural completeness findings in document order; empty means the document may be
    /// offered for human review (subject to unanswered questions, which are not part of
    /// the document). This checks structure only and cannot discover product ambiguity.
    pub fn validate(&self) -> Vec<Finding> {
        let mut check = Check(Vec::new());
        check.text("title", &self.title);
        check.text("goal", &self.goal);
        check.texts("non_goals", &self.non_goals, false);
        check.required("actors", self.actors.len());
        for (index, actor) in self.actors.iter().enumerate() {
            check.text(&format!("actors[{index}].name"), &actor.name);
        }
        check.required("use_cases", self.use_cases.len());
        for (index, use_case) in self.use_cases.iter().enumerate() {
            let at = |key: &str| format!("use_cases[{index}].{key}");
            check.text(&at("title"), &use_case.title);
            let actors = self.actors.iter().map(|a| a.id.as_str()).collect();
            check.references(&at("actor_ids"), &use_case.actor_ids, actors);
            check.texts(&at("preconditions"), &use_case.preconditions, true);
            check.texts(&at("main_flow"), &use_case.main_flow, true);
            check.flows(&at("alternate_flows"), &use_case.alternate_flows);
            check.flows(&at("error_flows"), &use_case.error_flows);
            check.texts(&at("postconditions"), &use_case.postconditions, true);
            let criteria = self
                .acceptance_criteria
                .iter()
                .map(|c| c.id.as_str())
                .collect();
            check.references(&at("acceptance_ids"), &use_case.acceptance_ids, criteria);
        }
        check.required("acceptance_criteria", self.acceptance_criteria.len());
        for (key, statements) in [
            ("acceptance_criteria", &self.acceptance_criteria),
            ("nfrs", &self.nfrs),
            ("assumptions", &self.assumptions),
        ] {
            for (index, statement) in statements.iter().enumerate() {
                check.text(&format!("{key}[{index}].statement"), &statement.statement);
            }
        }
        for index in 0..self.blockers.len() {
            check.add(format!("blockers[{index}]"), Incomplete::OpenBlocker);
        }
        check.0
    }
}

fn write_first(out: &mut String, key: &str, text: &str) {
    out.push('{');
    json::write_string(out, key);
    out.push(':');
    json::write_string(out, text);
}

fn write_text(out: &mut String, key: &str, text: &str) {
    out.push(',');
    json::write_string(out, key);
    out.push(':');
    json::write_string(out, text);
}

fn write_list<T>(out: &mut String, key: &str, items: &[T], mut item: impl FnMut(&mut String, &T)) {
    out.push(',');
    json::write_string(out, key);
    out.push_str(":[");
    for (index, value) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let start = out.len();
        item(out, value);
        // An object item is opened by `write_first` and closed here.
        if out.as_bytes().get(start) == Some(&b'{') {
            out.push('}');
        }
    }
    out.push(']');
}

fn write_texts(out: &mut String, key: &str, texts: &[String]) {
    write_list(out, key, texts, |out, text| json::write_string(out, text));
}

fn write_flow(out: &mut String, flow: &Flow) {
    write_first(out, "id", &flow.id);
    write_text(out, "title", &flow.title);
    write_texts(out, "steps", &flow.steps);
}

fn labelled(id: &str, text: &str) -> String {
    format!("**{id}** — {text}")
}

fn references(ids: &[String]) -> String {
    if ids.is_empty() {
        "_None._".to_string()
    } else {
        ids.join(", ")
    }
}

fn bullets(out: &mut String, lines: impl Iterator<Item = String>) {
    let before = out.len();
    for line in lines {
        out.push_str(&format!("- {line}\n"));
    }
    if out.len() == before {
        out.push_str("_None._\n");
    }
}

fn numbered(out: &mut String, heading: &str, lines: &[String]) {
    out.push_str(&format!("\n**{heading}**\n\n"));
    for (index, line) in lines.iter().enumerate() {
        out.push_str(&format!("{}. {line}\n", index + 1));
    }
    if lines.is_empty() {
        out.push_str("_None._\n");
    }
}

/// True when `text` marks unfinished work. Deliberately small and explainable.
pub fn is_placeholder(text: &str) -> bool {
    let upper = text.to_uppercase();
    ["???", "NEEDS CLARIFICATION", "LOREM IPSUM"]
        .iter()
        .any(|marker| upper.contains(marker))
        || upper
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| matches!(word, "TODO" | "TBD" | "TBC" | "FIXME" | "PLACEHOLDER"))
}

struct Check(Vec<Finding>);

impl Check {
    fn add(&mut self, path: String, problem: Incomplete) {
        self.0.push(Finding { path, problem });
    }

    fn required(&mut self, path: &str, entries: usize) {
        if entries == 0 {
            self.add(path.to_string(), Incomplete::Empty);
        }
    }

    fn text(&mut self, path: &str, text: &str) {
        if is_placeholder(text) {
            self.add(path.to_string(), Incomplete::Placeholder);
        } else if !text.chars().any(char::is_alphanumeric) {
            self.add(path.to_string(), Incomplete::Empty);
        }
    }

    fn texts(&mut self, path: &str, texts: &[String], required: bool) {
        if required {
            self.required(path, texts.len());
        }
        for (index, text) in texts.iter().enumerate() {
            self.text(&element_path(path, index), text);
        }
    }

    fn references(&mut self, path: &str, ids: &[String], defined: Vec<&str>) {
        self.required(path, ids.len());
        for (index, id) in ids.iter().enumerate() {
            if !defined.contains(&id.as_str()) {
                self.add(element_path(path, index), Incomplete::UnknownReference);
            }
        }
    }

    fn flows(&mut self, path: &str, flows: &[Flow]) {
        self.required(path, flows.len());
        for (index, flow) in flows.iter().enumerate() {
            let at = element_path(path, index);
            self.text(&format!("{at}.title"), &flow.title);
            self.texts(&format!("{at}.steps"), &flow.steps, true);
        }
    }
}

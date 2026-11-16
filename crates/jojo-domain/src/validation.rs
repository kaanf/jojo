//! Approved validation plan, changed-path scope and test-inventory checks (P1-10).
//!
//! The plan is human-reviewed content bound by digest to `ExecutionPlan::validation_digest`;
//! a validator or provider cannot widen it. Changed paths come from repository evidence
//! (Git raw diff of the frozen base against the preserved candidate) collected by storage,
//! never from an agent's list of files it says it touched.
//!
//! The post-change path gate reproduces one idea from Vigla's `acl` module (pinned in
//! `research/REPOSITORY_PINS.json`): component-wise prefix matching of every changed path
//! against a declared allow-list. Unlike that design there is no unconstrained mode, an
//! empty or overlapping declaration is refused, deletions/renames/mode changes count, and
//! unsupported entries (submodules, unknown statuses, non-UTF-8 names) fail closed.
use std::collections::BTreeSet;

use crate::dependency::NodeRef;
use crate::identity::{OpaqueId, Revision};
use crate::json::{self, Json};

pub const VALIDATION_PLAN_SCHEMA: &str = "jojo-validation-plan-v1";
/// P2-03: the same plan plus, per step, the exact validation command (an argument vector).
/// A plan without any command keeps the v1 encoding byte for byte.
pub const VALIDATION_PLAN_SCHEMA_V2: &str = "jojo-validation-plan-v2";
pub const MAX_COMMAND_ARGS: usize = 64;
/// The only step output kind Phase 1 can produce durably: the preserved candidate packet.
pub const CANDIDATE_OUTPUT: &str = "candidate";
const MAX_ENTRIES: usize = 1024;

/// Normalized repository-relative path: `/`-separated normal components, UTF-8, no `.git`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ScopePath(String);

impl ScopePath {
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.is_empty() || text.len() > 4096 || text.contains('\0') {
            return Err(format!("invalid repository path {text:?}"));
        }
        for c in text.split('/') {
            if c.is_empty() || c == "." || c == ".." || c.eq_ignore_ascii_case(".git") {
                return Err(format!("non-normal repository path {text:?}"));
            }
        }
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Component-wise prefix: `src` contains `src` and `src/a`, never `src2/a`.
    pub fn contains(&self, path: &str) -> bool {
        path == self.0
            || path
                .strip_prefix(self.0.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    }

    fn overlaps(&self, other: &Self) -> bool {
        self.contains(&other.0) || other.contains(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepValidation {
    pub step: NodeRef,
    /// Paths the step may change. Never empty; there is no unconstrained mode.
    pub write_scope: Vec<ScopePath>,
    /// Paths that must be byte-identical to the approved base (tests, fixtures).
    pub baseline: Vec<ScopePath>,
    /// Exact expected test inventory.
    pub tests: Vec<String>,
    /// P2-03: the argument vector the host validation runner executes, or empty when the
    /// step has no host command. These are human-approved bytes: the digest covers them.
    /// A step with a command has exactly one inventory entry, which names its verdict.
    pub command: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidationPlan {
    pub steps: Vec<StepValidation>,
}

fn disjoint(list: &[ScopePath], what: &str) -> Result<(), String> {
    for (i, a) in list.iter().enumerate() {
        if list[i + 1..].iter().any(|b| a.overlaps(b)) {
            return Err(format!("duplicate or nested {what} entry {:?}", a.0));
        }
    }
    Ok(())
}

impl ValidationPlan {
    pub fn validate(&self) -> Result<(), String> {
        if self.steps.is_empty() || self.steps.len() > MAX_ENTRIES {
            return Err("validation plan needs 1..=1024 steps".into());
        }
        let mut steps = BTreeSet::new();
        for s in &self.steps {
            if s.step.revision.0 == 0 || !steps.insert(s.step) {
                return Err("invalid or duplicate validation step".into());
            }
            if s.write_scope.is_empty() {
                return Err("empty write scope; unconstrained steps are not supported".into());
            }
            if s.write_scope.len() + s.baseline.len() > MAX_ENTRIES {
                return Err("scope too large".into());
            }
            disjoint(&s.write_scope, "write scope")?;
            disjoint(&s.baseline, "baseline")?;
            if let Some(p) = s
                .write_scope
                .iter()
                .find(|w| s.baseline.iter().any(|b| w.overlaps(b)))
            {
                return Err(format!(
                    "ambiguous ownership: {:?} is both writable and baseline",
                    p.0
                ));
            }
            if s.tests.is_empty()
                || s.tests.len() > MAX_ENTRIES
                || s.tests
                    .iter()
                    .any(|t| t.trim() != t || t.is_empty() || t.contains('\0'))
                || s.tests.iter().collect::<BTreeSet<_>>().len() != s.tests.len()
            {
                return Err("approved test inventory must be non-empty, exact and unique".into());
            }
            if let Some(executable) = s.command.first() {
                let normal = executable.starts_with('/')
                    && executable[1..]
                        .split('/')
                        .all(|c| !c.is_empty() && c != "." && c != "..");
                if !normal
                    || s.command.len() > MAX_COMMAND_ARGS
                    || s.command.iter().any(|a| a.len() > 4096 || a.contains('\0'))
                {
                    return Err("validation command must be a bounded absolute argv".into());
                }
                if s.tests.len() != 1 {
                    return Err("a validation command reports exactly one inventory entry".into());
                }
            }
        }
        Ok(())
    }

    pub fn step(&self, step: NodeRef) -> Option<&StepValidation> {
        self.steps.iter().find(|s| s.step == step)
    }

    /// Single canonical encoding; this exact text is what the approved digest covers.
    pub fn canonical(&self) -> String {
        let mut steps = self.steps.clone();
        steps.sort_by_key(|s| s.step);
        let v2 = steps.iter().any(|s| !s.command.is_empty());
        let mut out = String::from("[");
        json::write_string(
            &mut out,
            if v2 {
                VALIDATION_PLAN_SCHEMA_V2
            } else {
                VALIDATION_PLAN_SCHEMA
            },
        );
        out.push_str(",[");
        for (i, s) in steps.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('[');
            json::write_string(&mut out, &format!("{:032x}", s.step.id.0));
            out.push(',');
            json::write_string(&mut out, &s.step.revision.0.to_string());
            for list in [
                sorted(s.write_scope.iter().map(|p| p.0.clone())),
                sorted(s.baseline.iter().map(|p| p.0.clone())),
                sorted(s.tests.iter().cloned()),
            ] {
                out.push_str(",[");
                for (j, item) in list.iter().enumerate() {
                    if j > 0 {
                        out.push(',');
                    }
                    json::write_string(&mut out, item);
                }
                out.push(']');
            }
            if v2 {
                // Argument order is meaning: never sorted.
                out.push_str(",[");
                for (j, item) in s.command.iter().enumerate() {
                    if j > 0 {
                        out.push(',');
                    }
                    json::write_string(&mut out, item);
                }
                out.push(']');
            }
            out.push(']');
        }
        out.push_str("]]");
        out
    }

    /// Strict import: the bytes must already be the canonical encoding of a valid plan.
    pub fn import(bytes: &[u8]) -> Result<Self, String> {
        let j = json::parse(bytes).map_err(|e| format!("{e:?}"))?;
        let mut top = list(j)?.into_iter();
        let v2 = match text(top.next())?.as_str() {
            VALIDATION_PLAN_SCHEMA => false,
            VALIDATION_PLAN_SCHEMA_V2 => true,
            _ => return Err("unknown validation plan schema".into()),
        };
        let mut steps = Vec::new();
        for s in list(top.next().ok_or("missing steps")?)? {
            let mut f = list(s)?.into_iter();
            let id = u128::from_str_radix(&text(f.next())?, 16).map_err(|_| "invalid step id")?;
            let revision: u64 = text(f.next())?.parse().map_err(|_| "invalid revision")?;
            let mut paths = || -> Result<Vec<ScopePath>, String> {
                list(f.next().ok_or("missing scope")?)?
                    .into_iter()
                    .map(|p| ScopePath::parse(&text(Some(p))?))
                    .collect()
            };
            let write_scope = paths()?;
            let baseline = paths()?;
            let tests = list(f.next().ok_or("missing tests")?)?
                .into_iter()
                .map(|t| text(Some(t)))
                .collect::<Result<_, _>>()?;
            let command = if v2 {
                list(f.next().ok_or("missing command")?)?
                    .into_iter()
                    .map(|a| text(Some(a)))
                    .collect::<Result<_, _>>()?
            } else {
                Vec::new()
            };
            if f.next().is_some() {
                return Err("unknown trailing step fields".into());
            }
            steps.push(StepValidation {
                step: NodeRef {
                    id: OpaqueId(id),
                    revision: Revision(revision),
                },
                write_scope,
                baseline,
                tests,
                command,
            });
        }
        if top.next().is_some() {
            return Err("unknown trailing plan fields".into());
        }
        let plan = Self { steps };
        plan.validate()?;
        if plan.canonical().as_bytes() != bytes {
            return Err("validation plan is not in canonical form".into());
        }
        Ok(plan)
    }
}

fn sorted(items: impl Iterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = items.collect();
    v.sort();
    v
}

fn list(j: Json) -> Result<Vec<Json>, String> {
    match j {
        Json::List(v) => Ok(v),
        _ => Err("expected array".into()),
    }
}

fn text(j: Option<Json>) -> Result<String, String> {
    match j {
        Some(Json::Text(s)) => Ok(s),
        _ => Err("expected string".into()),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeKind {
    Added,
    Deleted,
    Modified,
    TypeChanged,
}

/// One repository change between the approved base and the candidate.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangedPath {
    pub path: String,
    pub kind: ChangeKind,
    pub old_mode: u32,
    pub new_mode: u32,
}

impl ChangedPath {
    pub fn canonical(&self) -> String {
        format!(
            "{:?} {:06o} {:06o} {}",
            self.kind, self.old_mode, self.new_mode, self.path
        )
    }
}

const SUPPORTED_MODES: [u32; 4] = [0o100644, 0o100755, 0o120000, 0];

fn mode(text: &[u8]) -> Result<u32, String> {
    let s = std::str::from_utf8(text).map_err(|_| "non-UTF-8 mode")?;
    let m = u32::from_str_radix(s, 8).map_err(|_| format!("invalid mode {s:?}"))?;
    if m == 0o160000 {
        return Err("submodule change is unsupported".into());
    }
    if !SUPPORTED_MODES.contains(&m) {
        return Err(format!("unsupported file mode {s}"));
    }
    Ok(m)
}

/// Parses `git diff --raw -z --no-renames` output. Any unknown status, unsupported mode,
/// non-normal or non-UTF-8 path is an error: the caller must not treat it as in scope.
pub fn parse_raw_diff(bytes: &[u8]) -> Result<Vec<ChangedPath>, String> {
    let mut fields = bytes.split(|b| *b == 0);
    let mut out = Vec::new();
    loop {
        let header = match fields.next() {
            None | Some([]) => break,
            Some(h) => h,
        };
        let header = header
            .strip_prefix(b":")
            .ok_or("malformed raw diff header")?;
        let parts: Vec<&[u8]> = header.split(|b| *b == b' ').collect();
        let [old, new, _, _, status] = parts.as_slice() else {
            return Err("malformed raw diff header".into());
        };
        let kind = match *status {
            b"A" => ChangeKind::Added,
            b"D" => ChangeKind::Deleted,
            b"M" => ChangeKind::Modified,
            b"T" => ChangeKind::TypeChanged,
            other => {
                return Err(format!(
                    "unsupported change status {:?}",
                    String::from_utf8_lossy(other)
                ));
            }
        };
        let path = fields.next().ok_or("raw diff path missing")?;
        let path = std::str::from_utf8(path).map_err(|_| "non-UTF-8 changed path")?;
        ScopePath::parse(path)?;
        let (old_mode, new_mode) = (mode(old)?, mode(new)?);
        if (kind == ChangeKind::Added) != (old_mode == 0)
            || (kind == ChangeKind::Deleted) != (new_mode == 0)
        {
            return Err(format!("inconsistent modes for {path:?}"));
        }
        out.push(ChangedPath {
            path: path.to_owned(),
            kind,
            old_mode,
            new_mode,
        });
    }
    Ok(out)
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScopeVerdict {
    pub allowed: Vec<String>,
    pub denied: Vec<String>,
    pub baseline_changed: Vec<String>,
}

impl ScopeVerdict {
    pub fn passed(&self) -> bool {
        self.denied.is_empty() && self.baseline_changed.is_empty()
    }
}

pub fn check_scope(changes: &[ChangedPath], step: &StepValidation) -> ScopeVerdict {
    let mut v = ScopeVerdict::default();
    for c in changes {
        if step.baseline.iter().any(|b| b.contains(&c.path)) {
            v.baseline_changed.push(c.path.clone());
        }
        if step.write_scope.iter().any(|w| w.contains(&c.path)) {
            v.allowed.push(c.path.clone());
        } else {
            v.denied.push(c.path.clone());
        }
    }
    v
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TestStatus {
    Passed,
    Failed,
    Skipped,
}

/// Every expected test executed exactly once and passed; nothing else ran.
pub fn check_inventory(expected: &[String], observed: &[(String, TestStatus)]) -> Vec<String> {
    let mut failures = Vec::new();
    if observed.is_empty() {
        failures.push("no tests executed".to_owned());
    }
    let mut seen = BTreeSet::new();
    for (id, status) in observed {
        if !seen.insert(id.as_str()) {
            failures.push(format!("duplicate test result {id:?}"));
        }
        if !expected.contains(id) {
            failures.push(format!("unexpected test {id:?}"));
        } else if *status != TestStatus::Passed {
            failures.push(format!("test {id:?} {status:?}"));
        }
    }
    for id in expected {
        if !seen.contains(id.as_str()) {
            failures.push(format!("missing test {id:?}"));
        }
    }
    failures
}

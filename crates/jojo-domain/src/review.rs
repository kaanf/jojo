//! M4: the cross-provider review of a settled plan step. Pure: the prompt and the strict
//! reading of the reviewer's reply.
//!
//! When a step's host validation passed, the provider that did not do the step reads the
//! change once, read-only, before the human decides. The reviewer gets the same inputs as
//! the developer (the approved requirements, architecture decision, step and scope) and the
//! candidate's diff, but nothing of the developer's conversation, and judges it. A
//! rejection's notes become the next repair's fix list; an approval decides nothing, the
//! human still accepts or rejects.
//!
//! Prompt content is adapted, with attribution, from two pinned sources
//! (`docs/roadmap/SOURCE_FIRST_REUSE_MAP.md`, section 0): cgeene/commandcenter (MIT, Caleb
//! Geene) `src/prompts/reviewer.ts` at `85f3995` (`buildReviewerPrompt`: an adversarial
//! reviewer, the same inputs as the worker and none of its conversation, the five checks,
//! a passed verify command that may not prove the task, actionable reject notes, approve
//! only after trying to reject); addyosmani/factory (MIT, Addy Osmani)
//! `template/.claude/agents/factory-critic.md` at `8af1165` (do not manufacture
//! objections; the single strongest objection).
use crate::json::Json;
use crate::planner::{LeadError, Members, REPLY_RULES, bad, schema};

pub const REVIEW_SCHEMA: &str = "jojo-step-review-v1";
/// Longest notes a verdict may carry; they become a repair's fix list.
pub const MAX_NOTES_BYTES: usize = 8 * 1024;
const MAX_OBJECTION_BYTES: usize = 1024;
/// Bytes kept free in the prompt after the diff.
const PROMPT_MARGIN: usize = 256;

/// What the review prompt is made of; all of it is read from the core's records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReviewInput<'a> {
    pub key: &'a str,
    pub title: &'a str,
    pub prd_revision: u64,
    pub prd: &'a str,
    pub adr: &'a str,
    pub write_scope: &'a [String],
    pub baseline: &'a [String],
    /// The approved validation command, which passed.
    pub command: &'a [String],
    /// The candidate's unified diff against the commit the step started from.
    pub diff: &'a str,
    /// Repairs before this candidate (0 for a first attempt).
    pub repairs: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Verdict {
    pub approve: bool,
    pub notes: String,
    pub objection: Option<String>,
}

/// The prompt of a review turn. The diff is cut to what fits in `limit` bytes in all, and
/// the reviewer is told so.
pub fn review_prompt(input: &ReviewInput<'_>, limit: usize) -> String {
    let list = |paths: &[String]| -> String {
        if paths.is_empty() {
            "(none)\n".into()
        } else {
            paths.iter().map(|p| format!("- {p}\n")).collect()
        }
    };
    let mut head = format!(
        "You are an adversarial code reviewer. A coding agent of another provider claims \
         that the step below is done, and the host already ran its approved check, which \
         passed. Your job is to find reasons it is NOT done. Approving broken, incomplete \
         or off-spec work is your failure mode: be skeptical and reject when in doubt.\n\n\
         You may only read. The current directory is the repository before this change; \
         the change itself is the diff at the end. Change nothing, and do not try to work \
         around a missing tool. You review; you do not fix.\n\n\
         # The step the agent was given ({key})\n{title}\n\n\
         # Product requirements (approved revision {revision})\n{prd}\n\n\
         # Architecture decision (approved)\n{adr}\n\n\
         # Scope\nThe agent could modify only:\n{scope}Paths that had to stay unchanged:\n\
         {baseline}\n# The check that passed\n`{command}` exited 0. Check whether it actually \
         proves the step is done: agents sometimes weaken tests or verify the wrong thing.\n",
        key = input.key,
        title = input.title,
        revision = input.prd_revision,
        prd = input.prd,
        adr = input.adr,
        scope = list(input.write_scope),
        baseline = list(input.baseline),
        command = input.command.join(" "),
    );
    if input.repairs > 0 {
        head.push_str(&format!(
            "This candidate is repair {} of the step; judge it as it stands.\n",
            input.repairs
        ));
    }
    let tail = format!(
        "\n# What to check\n\
         1. Does the diff do what the step and the requirements ask: every requirement, \
         not just the headline?\n\
         2. Correctness: bugs, unhandled edge cases, broken behavior for existing callers.\n\
         3. Cheating: weakened or deleted tests, hard-coded expected values, stubbed \
         functionality, a TODO in place of an implementation.\n\
         4. Scope: unrelated or destructive changes.\n\
         5. Assumptions nobody stated that later work would build on.\n\
         Comment wording and style are not findings unless a comment is wrong. Do not \
         manufacture objections: if the change is fine, say so briefly.\n\n\
         # Reply\n{REPLY_RULES}\n\
         {{\"schema\":\"{REVIEW_SCHEMA}\",\"verdict\":\"approve\",\"notes\":\"...\",\
         \"objection\":null}}\n\
         where \"verdict\" is \"approve\" or \"reject\". For \"reject\", \"notes\" are \
         specific and actionable (file, problem, what acceptance requires): the agent gets \
         them verbatim as its fix list. For \"approve\", \"notes\" say in a few sentences \
         what you checked. \"objection\" is the single strongest argument against the \
         change in one sentence, or null.\n\n# The diff\n"
    );
    let room = limit.saturating_sub(head.len() + tail.len() + PROMPT_MARGIN);
    let mut out = head;
    out.push_str(&tail);
    if input.diff.len() <= room {
        out.push_str("```diff\n");
        out.push_str(input.diff);
        out.push_str("\n```\n");
    } else {
        let mut end = room;
        while !input.diff.is_char_boundary(end) {
            end -= 1;
        }
        out.push_str(&format!(
            "(Only the first {end} of {} bytes fit here; read the changed files' current \
             versions in the directory for context and judge what you can see.)\n```diff\n",
            input.diff.len()
        ));
        out.push_str(&input.diff[..end]);
        out.push_str("\n```\n");
    }
    out
}

pub(crate) fn verdict(root: &Json) -> Result<Verdict, LeadError> {
    let mut members = Members::of(root, "")?;
    schema(&mut members, REVIEW_SCHEMA)?;
    let approve = match members.take("verdict")? {
        (Json::Text(v), _) if v == "approve" => true,
        (Json::Text(v), _) if v == "reject" => false,
        (_, path) => return Err(bad(path, "not \"approve\" or \"reject\"")),
    };
    let notes = members.text("notes", MAX_NOTES_BYTES, true)?;
    let objection = match members.take("objection")? {
        (Json::Null, _) => None,
        (value, path) => Some(crate::planner::text(
            value,
            &path,
            MAX_OBJECTION_BYTES,
            false,
        )?),
    };
    members.finish()?;
    Ok(Verdict {
        approve,
        notes,
        objection,
    })
}

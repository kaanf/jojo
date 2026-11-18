//! M4: bounded repair of a plan step. Pure: storage supplies what ended the step's newest
//! attempt and how many repairs it already had; this decides whether the runner starts one
//! more attempt or the step waits for the human.
//!
//! One match arm per kind of failure, as in the recovery policy of Kilbex/Vigla
//! (Apache-2.0) `crates/orchestrator/src/recovery/policy.rs` at `bbd19ae`: adding a kind
//! breaks the match and forces whoever adds it to say how it is repaired. The bound and the
//! hold at the cap follow cgeene/commandcenter (MIT) `src/daemon/review.ts`
//! (`reviewMaxCycles`, a blocked task with the round history) at `85f3995`.
//!
//! D2 (decided 2026-11-15): at most two repairs, then a hold the human sees. Something the
//! agent's own work caused is repaired; something the human, the helper, the host or the
//! outside world caused is not. An external action (push, pull request) is never retried.

/// D2: at most two repairs of one step.
pub const MAX_REPAIRS: u8 = 2;
/// Bytes of failure output a repair prompt carries: the end of it.
pub const MAX_FAILURE_BYTES: usize = 16 * 1024;
/// Changed paths a repair prompt lists.
pub const MAX_LISTED_PATHS: usize = 64;

/// Why the newest attempt of a step did not lead to a candidate the human can accept.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Failure {
    /// The agent's run ended in failure (exit, protocol, timeout).
    AttemptFailed { reason: String },
    /// The agent's run succeeded and changed nothing.
    NoChange,
    /// The host validation ran and failed; a write outside the scope fails it too.
    ValidationFailed {
        reasons: Vec<String>,
        output: String,
    },
    /// The reviewer of the other provider rejected the candidate.
    ReviewRejected { notes: String },
    /// The human or a plan cancel stopped the attempt.
    Cancelled,
    /// The helper ended under the attempt.
    Interrupted,
    /// The runner could not start the step.
    Refused { detail: String },
    /// The accepted candidate does not apply to the plan branch.
    Conflict { detail: String },
    /// The human rejected the candidate.
    HumanRejected,
    /// The host validation ended without a verdict.
    ValidationUnknown { detail: String },
    /// The review turn ended without a verdict.
    ReviewUnknown { detail: String },
    /// The recovery review holds the attempt.
    RecoveryHold,
    /// An action outside the machine (push, pull request).
    ExternalAction { detail: String },
}

/// Set per project (D2: "to move toward the design, set the limit to 0").
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RepairPolicy {
    /// 0 to [`MAX_REPAIRS`].
    pub limit: u8,
}

impl Default for RepairPolicy {
    fn default() -> Self {
        Self { limit: MAX_REPAIRS }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepairDecision {
    /// Start repair `ordinal` of `limit` with the failure in its prompt.
    Repair { ordinal: u8, limit: u8 },
    /// The step stops and waits for the human, who sees why.
    Hold { reason: String },
}

/// What the runner does after `failure`, when the step already had `repairs` repairs.
pub fn decide(failure: &Failure, repairs: u8, policy: RepairPolicy) -> RepairDecision {
    let limit = policy.limit.min(MAX_REPAIRS);
    let hold = |reason: String| RepairDecision::Hold { reason };
    let repair = |what: &str| {
        if repairs < limit {
            RepairDecision::Repair {
                ordinal: repairs + 1,
                limit,
            }
        } else if limit == 0 {
            hold(format!("{what}; automatic repair is off for this project"))
        } else {
            hold(format!("{what}; {repairs} of {limit} repairs used"))
        }
    };
    match failure {
        Failure::AttemptFailed { .. } => repair("the attempt failed"),
        Failure::NoChange => repair("the attempt changed nothing"),
        Failure::ValidationFailed { .. } => repair("host validation failed"),
        Failure::ReviewRejected { .. } => repair("the reviewer rejected the candidate"),
        Failure::Cancelled => hold("the attempt was cancelled".into()),
        Failure::Interrupted => hold("the helper ended under the attempt".into()),
        Failure::Refused { detail } => hold(format!("the runner could not start it: {detail}")),
        Failure::Conflict { detail } => hold(format!("integration conflict: {detail}")),
        Failure::HumanRejected => hold("the human rejected the candidate".into()),
        Failure::ValidationUnknown { detail } => {
            hold(format!("validation ended without a verdict: {detail}"))
        }
        Failure::ReviewUnknown { detail } => {
            hold(format!("the review ended without a verdict: {detail}"))
        }
        Failure::RecoveryHold => hold("the recovery review holds the attempt".into()),
        Failure::ExternalAction { detail } => hold(format!(
            "an external action is never retried automatically: {detail}"
        )),
    }
}

/// The end of `text`, at most `limit` bytes, cut at a character boundary.
pub fn tail(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut start = text.len() - limit;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// The section a repair attempt's prompt carries after the approved documents. Built only
/// from what the core recorded: the failure, the host's output and the earlier paths.
pub fn repair_section(ordinal: u8, limit: u8, failure: &Failure, changed: &[String]) -> String {
    let mut out = format!(
        "\n# Repair {ordinal} of {limit}\nAn earlier attempt at this step did not pass. You \
         start again from the same commit; nothing of that attempt is in this directory. \
         Do the step again and fix what is reported below. Stay inside the scope.\n"
    );
    failure_report(&mut out, failure, changed);
    out
}

/// M6: what a retry the human decided tells its attempt. A repair reports the failure as a
/// repair does; a restart reports nothing of the earlier attempt. The human's note follows
/// either. A restart without a note is told nothing more than a first attempt.
pub fn retry_section(failure: Option<&Failure>, changed: &[String], note: &str) -> String {
    let note = note.trim();
    let mut out = match failure {
        Some(failure) => {
            let mut out = String::from(
                "\n# Repair on the product owner's word\nAn earlier attempt at this step did \
                 not pass and the product owner asked for one more try. You start again from \
                 the plan branch as it is now; nothing of that attempt is in this directory. \
                 Do the step again and fix what is reported below. Stay inside the scope.\n",
            );
            failure_report(&mut out, failure, changed);
            out
        }
        None if note.is_empty() => return String::new(),
        None => String::from(
            "\n# Restart on the product owner's word\nAn earlier try at this step was set \
             aside. Do the step from the start. Stay inside the scope.\n",
        ),
    };
    if !note.is_empty() {
        out.push_str("\n## Note from the product owner\n");
        out.push_str(note);
        out.push('\n');
    }
    out
}

fn failure_report(out: &mut String, failure: &Failure, changed: &[String]) {
    let output = |out: &mut String, title: &str, text: &str| {
        let shown = tail(text, MAX_FAILURE_BYTES);
        out.push_str(&format!("\n## {title}"));
        if shown.len() < text.len() {
            out.push_str(&format!(" (the last {} bytes)", shown.len()));
        }
        out.push_str("\n```text\n");
        out.push_str(shown.trim_end());
        out.push_str("\n```\n");
    };
    match failure {
        Failure::AttemptFailed { reason } => output(out, "Why the attempt failed", reason),
        Failure::NoChange => {
            out.push_str("\n## What went wrong\nThe attempt ended without changing any file.\n")
        }
        Failure::ValidationFailed {
            reasons,
            output: text,
        } => {
            out.push_str("\n## Host validation failed\n");
            for reason in reasons {
                out.push_str(&format!("- {reason}\n"));
            }
            if !text.trim().is_empty() {
                output(out, "Output of the validation command", text);
            }
        }
        Failure::ReviewRejected { notes } => output(
            out,
            "The reviewer rejected it; the notes are your fix list",
            notes,
        ),
        other => output(out, "What went wrong", &format!("{other:?}")),
    }
    if !changed.is_empty() {
        out.push_str("\n## Paths the earlier attempt changed\n");
        for path in changed.iter().take(MAX_LISTED_PATHS) {
            out.push_str(&format!("- {path}\n"));
        }
        if changed.len() > MAX_LISTED_PATHS {
            out.push_str(&format!(
                "- … and {} more\n",
                changed.len() - MAX_LISTED_PATHS
            ));
        }
    }
}

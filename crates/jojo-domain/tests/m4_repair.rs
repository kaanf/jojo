//! M4 pure repair policy and review contract. The exhaustiveness check follows
//! Kilbex/Vigla (Apache-2.0) `crates/orchestrator/tests/recovery_unit.rs` at `bbd19ae`:
//! every kind of failure is driven through to a decision. The review scenarios follow
//! cgeene/commandcenter (MIT) `test/reviewloop.test.ts` at `85f3995` (the cap blocks).
//! Everything here is synthetic. No process, I/O, provider binary or network.
use jojo_domain::planner::{LeadError, LeadReply, TurnKind, parse_reply};
use jojo_domain::provider::MAX_PROMPT_BYTES;
use jojo_domain::repair::*;
use jojo_domain::review::*;

fn repairable() -> Vec<Failure> {
    vec![
        Failure::AttemptFailed {
            reason: "exit 1".into(),
        },
        Failure::NoChange,
        Failure::ValidationFailed {
            reasons: vec!["test failed".into()],
            output: "assertion failed".into(),
        },
        Failure::ReviewRejected {
            notes: "src/a.rs: the edge case is not handled".into(),
        },
    ]
}

fn held() -> Vec<Failure> {
    vec![
        Failure::Cancelled,
        Failure::Interrupted,
        Failure::Refused {
            detail: "StartRefused".into(),
        },
        Failure::Conflict {
            detail: "patch does not apply".into(),
        },
        Failure::HumanRejected,
        Failure::ValidationUnknown {
            detail: "shutdown".into(),
        },
        Failure::ReviewUnknown {
            detail: "no reply".into(),
        },
        Failure::RecoveryHold,
        Failure::ExternalAction {
            detail: "push".into(),
        },
    ]
}

#[test]
fn m4_agent_failures_are_repaired_twice_then_held() {
    let policy = RepairPolicy::default();
    assert_eq!(policy.limit, MAX_REPAIRS);
    for failure in repairable() {
        assert_eq!(
            decide(&failure, 0, policy),
            RepairDecision::Repair {
                ordinal: 1,
                limit: 2
            }
        );
        assert_eq!(
            decide(&failure, 1, policy),
            RepairDecision::Repair {
                ordinal: 2,
                limit: 2
            }
        );
        let RepairDecision::Hold { reason } = decide(&failure, 2, policy) else {
            panic!("a third repair for {failure:?}");
        };
        assert!(reason.ends_with("2 of 2 repairs used"), "{reason}");
    }
}

#[test]
fn m4_failures_the_agent_did_not_cause_are_never_repaired() {
    for failure in held() {
        for repairs in 0..=MAX_REPAIRS {
            assert!(
                matches!(
                    decide(&failure, repairs, RepairPolicy::default()),
                    RepairDecision::Hold { .. }
                ),
                "{failure:?}"
            );
        }
    }
    let RepairDecision::Hold { reason } = decide(
        &Failure::ExternalAction {
            detail: "push".into(),
        },
        0,
        RepairPolicy::default(),
    ) else {
        unreachable!()
    };
    assert!(reason.contains("never retried automatically"));
}

#[test]
fn m4_the_limit_is_a_setting_and_never_above_two() {
    let off = RepairPolicy { limit: 0 };
    for failure in repairable() {
        let RepairDecision::Hold { reason } = decide(&failure, 0, off) else {
            panic!("repaired with the limit at 0");
        };
        assert!(reason.contains("automatic repair is off"));
    }
    let one = RepairPolicy { limit: 1 };
    assert!(matches!(
        decide(&Failure::NoChange, 1, one),
        RepairDecision::Hold { .. }
    ));
    let wide = RepairPolicy { limit: 9 };
    assert!(matches!(
        decide(&Failure::NoChange, 2, wide),
        RepairDecision::Hold { .. }
    ));
}

#[test]
fn m4_repair_section_carries_the_end_of_the_failure_output_and_the_paths() {
    let long = format!("{}THE-END", "x".repeat(MAX_FAILURE_BYTES * 2));
    let failure = Failure::ValidationFailed {
        reasons: vec!["validation command exited 1".into()],
        output: long,
    };
    let paths: Vec<String> = (0..70).map(|i| format!("src/f{i}.rs")).collect();
    let section = repair_section(1, 2, &failure, &paths);
    assert!(section.starts_with("\n# Repair 1 of 2\n"));
    assert!(section.contains("- validation command exited 1\n"));
    assert!(section.contains(&format!("(the last {MAX_FAILURE_BYTES} bytes)")));
    assert!(section.contains("THE-END\n```"));
    assert!(section.len() < MAX_FAILURE_BYTES + 4096);
    assert!(section.contains("- src/f63.rs\n") && !section.contains("- src/f64.rs\n"));
    assert!(section.contains("- … and 6 more\n"));
    let notes = repair_section(
        2,
        2,
        &Failure::ReviewRejected {
            notes: "src/a.rs: handle zero".into(),
        },
        &[],
    );
    assert!(notes.contains("the notes are your fix list"));
    assert!(notes.contains("src/a.rs: handle zero"));
    assert!(!notes.contains("Paths the earlier attempt changed"));
    assert_eq!(tail("aéb", 2), "b");
}

fn input<'a>(diff: &'a str, scope: &'a [String], command: &'a [String]) -> ReviewInput<'a> {
    ReviewInput {
        key: "s1",
        title: "Add the greeting",
        prd_revision: 2,
        prd: "The tool greets.",
        adr: "Touch src/greet.rs only.",
        write_scope: scope,
        baseline: &[],
        command,
        diff,
        repairs: 1,
    }
}

#[test]
fn m4_review_prompt_has_the_developer_inputs_and_fits_the_diff() {
    let scope = vec!["src/greet.rs".to_owned()];
    let command = vec!["/usr/bin/make".to_owned(), "test".to_owned()];
    let prompt = review_prompt(&input("+fn greet() {}", &scope, &command), MAX_PROMPT_BYTES);
    for wanted in [
        "adversarial code reviewer",
        "# The step the agent was given (s1)\nAdd the greeting",
        "approved revision 2",
        "Touch src/greet.rs only.",
        "- src/greet.rs\n",
        "`/usr/bin/make test` exited 0",
        "repair 1 of the step",
        "Do not manufacture objections",
        REVIEW_SCHEMA,
        "```diff\n+fn greet() {}\n```",
    ] {
        assert!(prompt.contains(wanted), "{wanted}");
    }
    let big = "+".repeat(MAX_PROMPT_BYTES * 2);
    let cut = review_prompt(&input(&big, &scope, &command), MAX_PROMPT_BYTES);
    assert!(cut.len() <= MAX_PROMPT_BYTES);
    assert!(cut.contains("fit here"));
}

#[test]
fn m4_review_reply_is_one_strict_verdict() {
    let ok = format!(
        r#"{{"schema":"{REVIEW_SCHEMA}","verdict":"reject","notes":"src/a.rs: zero is not handled","objection":"The test does not cover zero."}}"#
    );
    let Ok(LeadReply::Review(v)) = parse_reply(TurnKind::Review, &ok) else {
        panic!("not a verdict");
    };
    assert!(!v.approve);
    assert_eq!(
        v.objection.as_deref(),
        Some("The test does not cover zero.")
    );
    let approve = format!(
        "```json\n{{\"schema\":\"{REVIEW_SCHEMA}\",\"verdict\":\"approve\",\"notes\":\"Read the diff and the test.\",\"objection\":null}}\n```"
    );
    let Ok(LeadReply::Review(v)) = parse_reply(TurnKind::Review, &approve) else {
        panic!("not a verdict");
    };
    assert!(v.approve && v.objection.is_none());
    for bad in [
        format!(r#"{{"schema":"{REVIEW_SCHEMA}","verdict":"maybe","notes":"x","objection":null}}"#),
        format!(
            r#"{{"schema":"{REVIEW_SCHEMA}","verdict":"approve","notes":"","objection":null}}"#
        ),
        format!(
            r#"{{"schema":"{REVIEW_SCHEMA}","verdict":"approve","notes":"ok","objection":null,"extra":1}}"#
        ),
        format!(r#"{{"schema":"{REVIEW_SCHEMA}","verdict":"approve","notes":"ok"}}"#),
        r#"{"schema":"jojo-lead-plan-v1","verdict":"approve","notes":"ok","objection":null}"#
            .into(),
        "Looks good to me.".into(),
    ] {
        assert!(
            matches!(
                parse_reply(TurnKind::Review, &bad),
                Err(LeadError::Field { .. } | LeadError::Json(_))
            ),
            "{bad}"
        );
    }
}

#[test]
fn m4_review_is_not_a_chat_turn_kind() {
    assert_eq!(TurnKind::named("Review"), None);
    assert_eq!(TurnKind::Review.name(), "Review");
}

#[test]
fn m6_a_retry_tells_a_repair_its_failure_and_either_retry_the_humans_note() {
    let failure = Failure::NoChange;
    let repair = retry_section(
        Some(&failure),
        &["src/a.txt".into()],
        "  Use the old name.  ",
    );
    assert!(
        repair.starts_with("\n# Repair on the product owner's word\n"),
        "{repair}"
    );
    assert!(repair.contains("## What went wrong\nThe attempt ended without changing any file.\n"));
    assert!(repair.contains("## Paths the earlier attempt changed\n- src/a.txt\n"));
    assert!(
        repair.ends_with("\n## Note from the product owner\nUse the old name.\n"),
        "{repair}"
    );
    // A restart is told nothing of the earlier attempt; without a note, nothing at all.
    assert_eq!(retry_section(None, &["src/a.txt".into()], " \n"), "");
    let restart = retry_section(None, &[], "Start from the README.");
    assert!(
        restart.starts_with("\n# Restart on the product owner's word\n"),
        "{restart}"
    );
    assert!(!restart.contains("src/a.txt"));
    assert!(restart.ends_with("## Note from the product owner\nStart from the README.\n"));
}

//! P2-03: the approved validation plan may name the host validation command. The command
//! is part of the canonical bytes, so it is covered by the digest a human approves.
use jojo_domain::dependency::NodeRef;
use jojo_domain::identity::{OpaqueId, Revision};
use jojo_domain::validation::{
    ScopePath, StepValidation, VALIDATION_PLAN_SCHEMA, VALIDATION_PLAN_SCHEMA_V2, ValidationPlan,
};

fn plan(tests: &[&str], command: &[&str]) -> ValidationPlan {
    ValidationPlan {
        steps: vec![StepValidation {
            step: NodeRef {
                id: OpaqueId(1),
                revision: Revision(1),
            },
            write_scope: vec![ScopePath::parse("slugify.py").unwrap()],
            baseline: vec![ScopePath::parse("test_slugify.py").unwrap()],
            tests: tests.iter().map(|t| t.to_string()).collect(),
            command: command.iter().map(|a| a.to_string()).collect(),
        }],
    }
}

const STEP: &str =
    r#""00000000000000000000000000000001","1",["slugify.py"],["test_slugify.py"],["unittest"]"#;

#[test]
fn p2_03_plan_without_a_command_keeps_the_v1_bytes() {
    let p = plan(&["unittest"], &[]);
    let expected = format!(r#"["{VALIDATION_PLAN_SCHEMA}",[[{STEP}]]]"#);
    assert_eq!(p.canonical(), expected);
    assert_eq!(ValidationPlan::import(expected.as_bytes()).unwrap(), p);
    // A v1 plan cannot smuggle a sixth field, and a v2 plan cannot omit it.
    let sixth = format!(r#"["{VALIDATION_PLAN_SCHEMA}",[[{STEP},["/bin/sh"]]]]"#);
    assert!(ValidationPlan::import(sixth.as_bytes()).is_err());
    let missing = format!(r#"["{VALIDATION_PLAN_SCHEMA_V2}",[[{STEP}]]]"#);
    assert!(ValidationPlan::import(missing.as_bytes()).is_err());
    // v2 with an empty command is not canonical: it is the v1 plan.
    let empty = format!(r#"["{VALIDATION_PLAN_SCHEMA_V2}",[[{STEP},[]]]]"#);
    assert!(ValidationPlan::import(empty.as_bytes()).is_err());
}

#[test]
fn p2_03_command_is_exact_ordered_and_part_of_the_approved_bytes() {
    let command = ["/usr/bin/python3", "-m", "unittest", "-q"];
    let p = plan(&["unittest"], &command);
    p.validate().unwrap();
    let expected = format!(
        r#"["{VALIDATION_PLAN_SCHEMA_V2}",[[{STEP},["/usr/bin/python3","-m","unittest","-q"]]]]"#
    );
    assert_eq!(p.canonical(), expected);
    let imported = ValidationPlan::import(expected.as_bytes()).unwrap();
    assert_eq!(imported, p);
    assert_eq!(imported.steps[0].command, command);
    // Any change to the command changes the bytes a human approved.
    let reordered = plan(&["unittest"], &["/usr/bin/python3", "-q", "unittest", "-m"]);
    assert_ne!(reordered.canonical(), expected);
    assert_ne!(plan(&["unittest"], &[]).canonical(), expected);
}

#[test]
fn p2_03_unsafe_or_ambiguous_commands_are_refused() {
    for command in [
        vec!["python3", "-m", "unittest"],
        vec!["./run.sh"],
        vec!["/usr/bin/../bin/sh"],
        vec!["/usr//bin/true"],
        vec!["/"],
        vec!["/bin/echo", "a\0b"],
    ] {
        assert!(
            plan(&["unittest"], &command).validate().is_err(),
            "{command:?}"
        );
    }
    let many: Vec<&str> = std::iter::once("/bin/echo").chain([""; 64]).collect();
    assert!(plan(&["unittest"], &many).validate().is_err());
    // One command yields one verdict: exactly one inventory entry names it.
    assert!(plan(&["a", "b"], &["/usr/bin/true"]).validate().is_err());
    plan(&["a", "b"], &[]).validate().unwrap();
}

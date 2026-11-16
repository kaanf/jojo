//! T10 pure contracts: ordered outcome classification, approved validation plans,
//! changed-path scope and exact test inventories. No storage, process or Git.
use jojo_domain::attempt::AttemptOutcome;
use jojo_domain::dependency::NodeRef;
use jojo_domain::identity::{OpaqueId, Revision};
use jojo_domain::outcome::*;
use jojo_domain::provider::TerminalStatus;
use jojo_domain::validation::*;

fn natural() -> SettlementFacts {
    SettlementFacts {
        conclusion: ProcessConclusion::TreeAbsent,
        stop: None,
        stop_cause_unknown: false,
        terminals: vec![TerminalStatus::Success],
        exit: Some(ExitStatus {
            code: Some(0),
            signal: None,
        }),
        protocol_failed: false,
        coverage: Coverage::Complete,
        candidate: CandidateEvidence::Recorded("c".repeat(64)),
        unresolved_effects: vec![],
        abandoned_effects: vec![],
        missing_outputs: vec![],
    }
}

fn outcome(f: &SettlementFacts) -> (AttemptOutcome, &'static str) {
    match classify(f) {
        Classification::Final {
            outcome, reason, ..
        } => (outcome, reason),
        other => panic!("expected final outcome, got {other:?}"),
    }
}

fn recovery(f: &SettlementFacts) -> Vec<String> {
    match classify(f) {
        Classification::RecoveryRequired(r) => r,
        other => panic!("expected recovery, got {other:?}"),
    }
}

#[test]
fn t10_success_needs_terminal_exit_zero_complete_coverage_and_candidate() {
    assert_eq!(
        outcome(&natural()),
        (AttemptOutcome::Succeeded, "terminal_success")
    );
    let mut f = natural();
    f.terminals.clear();
    assert_eq!(
        outcome(&f),
        (AttemptOutcome::Failed, "protocol_incomplete"),
        "exit zero without a terminal record is never success"
    );
    let mut f = natural();
    f.exit = Some(ExitStatus {
        code: Some(3),
        signal: None,
    });
    assert_eq!(outcome(&f), (AttemptOutcome::Failed, "exit_mismatch"));
    let mut f = natural();
    f.exit = Some(ExitStatus {
        code: None,
        signal: Some(9),
    });
    assert_eq!(outcome(&f).1, "exit_mismatch");
    for (status, reason) in [
        (TerminalStatus::Error, "provider_error"),
        (TerminalStatus::Interrupted, "provider_interrupted"),
        (TerminalStatus::Unknown, "provider_status_unknown"),
    ] {
        let mut f = natural();
        f.terminals = vec![status];
        assert_eq!(outcome(&f), (AttemptOutcome::Failed, reason));
    }
    let mut f = natural();
    f.terminals = vec![TerminalStatus::Success, TerminalStatus::Success];
    assert_eq!(outcome(&f).1, "protocol_violation");
    let mut f = natural();
    f.protocol_failed = true;
    assert_eq!(outcome(&f).1, "protocol_violation");
    let mut f = natural();
    f.coverage = Coverage::Truncated("STDERR discarded 9 bytes".into());
    assert_eq!(outcome(&f), (AttemptOutcome::Failed, "output_truncated"));
    let mut f = natural();
    f.missing_outputs = vec!["verified-build".into()];
    assert_eq!(outcome(&f).1, "required_output_unavailable");
}

#[test]
fn t10_unproven_absence_lost_coverage_and_unknown_effects_never_finalize() {
    for c in [
        ProcessConclusion::Unconcluded,
        ProcessConclusion::SpawnUncertain,
        ProcessConclusion::StopUnconfirmed,
    ] {
        let mut f = natural();
        f.conclusion = c;
        assert!(!recovery(&f).is_empty(), "{c:?}");
    }
    let mut f = natural();
    f.coverage = Coverage::Lost("stdout spool shorter than checkpoint".into());
    f.terminals.clear();
    assert!(recovery(&f)[0].contains("coverage lost"));
    let mut f = natural();
    f.stop = Some(StopCause::TaskCancel);
    f.coverage = Coverage::Lost("missing blob".into());
    assert!(
        recovery(&f)[0].contains("coverage lost"),
        "lost evidence outranks every stop cause"
    );
    let mut f = natural();
    f.unresolved_effects = vec!["attempt-effect:push".into()];
    assert!(recovery(&f)[0].contains("attempt-effect:push"));
    let mut f = natural();
    f.exit = None;
    assert!(recovery(&f)[0].contains("wait status"));
    let mut f = natural();
    f.candidate = CandidateEvidence::Unavailable("ignored files".into());
    assert!(recovery(&f)[0].contains("candidate"));
    let mut f = natural();
    f.stop_cause_unknown = true;
    assert!(recovery(&f)[0].contains("ordering"));
}

#[test]
fn t10_stop_committed_before_finalization_outranks_a_later_terminal_success() {
    let cases = [
        (
            StopCause::TaskCancel,
            AttemptOutcome::Cancelled,
            "task_cancelled",
        ),
        (
            StopCause::ApprovalInvalidated,
            AttemptOutcome::Interrupted,
            "approval_invalidated",
        ),
        (StopCause::Shutdown, AttemptOutcome::Interrupted, "shutdown"),
        (
            StopCause::Supervisor(SupervisorStop::Protocol),
            AttemptOutcome::Failed,
            "protocol_violation",
        ),
        (
            StopCause::Supervisor(SupervisorStop::Deadline),
            AttemptOutcome::Failed,
            "execution_deadline",
        ),
        (
            StopCause::Supervisor(SupervisorStop::Descendants),
            AttemptOutcome::Failed,
            "descendants_outlived_root",
        ),
        (
            StopCause::Supervisor(SupervisorStop::Other),
            AttemptOutcome::Interrupted,
            "supervisor_stop",
        ),
    ];
    for (cause, expected, reason) in cases {
        let mut f = natural();
        f.stop = Some(cause);
        let Classification::Final {
            outcome,
            reason: r,
            uncertainties,
        } = classify(&f)
        else {
            panic!("{cause:?}")
        };
        assert_eq!((outcome, r), (expected, reason));
        assert!(
            uncertainties.iter().any(|u| u.contains("Success")),
            "terminal success is retained as evidence, not as the outcome"
        );
    }
    // Shutdown with no terminal record: INTERRUPTED, never success or protocol failure.
    let mut f = natural();
    f.stop = Some(StopCause::Shutdown);
    f.terminals.clear();
    f.exit = Some(ExitStatus {
        code: None,
        signal: Some(2),
    });
    assert_eq!(outcome(&f), (AttemptOutcome::Interrupted, "shutdown"));
    let mut f = natural();
    f.stop = Some(StopCause::Supervisor(SupervisorStop::EvidenceLoss));
    assert!(!recovery(&f).is_empty());
}

#[test]
fn t10_not_spawned_launches_follow_stop_cause_without_candidate() {
    let mut f = natural();
    f.conclusion = ProcessConclusion::NotSpawned;
    f.terminals.clear();
    f.exit = None;
    f.candidate = CandidateEvidence::NotApplicable;
    assert_eq!(outcome(&f), (AttemptOutcome::Failed, "launch_failure"));
    f.stop = Some(StopCause::TaskCancel);
    assert_eq!(outcome(&f).0, AttemptOutcome::Cancelled);
    f.stop = Some(StopCause::Shutdown);
    assert_eq!(outcome(&f).0, AttemptOutcome::Interrupted);
    let mut f = natural();
    f.unresolved_effects = vec!["attempt-effect:x".into()];
    f.stop = Some(StopCause::Shutdown);
    let Classification::Final { uncertainties, .. } = classify(&f) else {
        panic!()
    };
    assert!(
        uncertainties.iter().any(|u| u.contains("outcome unknown")),
        "an interrupted result records unresolved effect uncertainty"
    );
}

fn node(n: u128) -> NodeRef {
    NodeRef {
        id: OpaqueId(n),
        revision: Revision(1),
    }
}

fn step(scope: &[&str], baseline: &[&str], tests: &[&str]) -> StepValidation {
    StepValidation {
        step: node(1),
        write_scope: scope.iter().map(|s| ScopePath::parse(s).unwrap()).collect(),
        baseline: baseline
            .iter()
            .map(|s| ScopePath::parse(s).unwrap())
            .collect(),
        tests: tests.iter().map(|s| s.to_string()).collect(),
        command: vec![],
    }
}

#[test]
fn t10_scope_paths_are_normalized_component_prefixes_without_unconstrained_mode() {
    for bad in [
        "",
        "/abs",
        "a//b",
        "./a",
        "a/./b",
        "../x",
        "a/../b",
        "a/",
        ".git",
        "src/.GIT/x",
        "a\0b",
    ] {
        assert!(ScopePath::parse(bad).is_err(), "{bad:?}");
    }
    let src = ScopePath::parse("src").unwrap();
    assert!(src.contains("src") && src.contains("src/a/b.rs"));
    assert!(!src.contains("src2/a.rs") && !src.contains("srcx") && !src.contains("SRC/a"));
    let s = step(&["src"], &["tests"], &["t"]);
    let plan = ValidationPlan { steps: vec![s] };
    assert!(plan.validate().is_ok());
    for (scope, baseline, tests) in [
        (&[][..], &[][..], &["t"][..]),
        (&["src"], &["src/fixtures"], &["t"]),
        (&["src"], &["src"], &["t"]),
        (&["src", "src/a"], &[], &["t"]),
        (&["src"], &[], &[]),
        (&["src"], &[], &["t", "t"]),
        (&["src"], &[], &[" t"]),
    ] {
        let plan = ValidationPlan {
            steps: vec![step(scope, baseline, tests)],
        };
        assert!(plan.validate().is_err(), "{scope:?} {baseline:?} {tests:?}");
    }
}

#[test]
fn t10_validation_plan_import_is_canonical_strict_and_digest_stable() {
    let plan = ValidationPlan {
        steps: vec![step(&["src", "docs"], &["tests"], &["b", "a"])],
    };
    let canonical = plan.canonical();
    let imported = ValidationPlan::import(canonical.as_bytes()).unwrap();
    assert_eq!(imported.canonical(), canonical);
    assert!(canonical.contains(r#"["docs","src"]"#) && canonical.contains(r#"["a","b"]"#));
    let spaced = canonical.replace(",[", ", [");
    assert!(ValidationPlan::import(spaced.as_bytes()).is_err());
    let reordered = canonical.replace(r#"["a","b"]"#, r#"["b","a"]"#);
    assert!(ValidationPlan::import(reordered.as_bytes()).is_err());
    let schema = canonical.replace("v1", "v2");
    assert!(ValidationPlan::import(schema.as_bytes()).is_err());
    let extra = canonical.replace(r#"["a","b"]]"#, r#"["a","b"],"x"]"#);
    assert!(ValidationPlan::import(extra.as_bytes()).is_err());
    let traversal = canonical.replace(r#""docs""#, r#""../docs""#);
    assert!(ValidationPlan::import(traversal.as_bytes()).is_err());
    assert!(ValidationPlan::import(br#"{"a":1,"a":2}"#).is_err());
}

fn raw(entries: &[(&str, &str, &str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (old, new, status, path) in entries {
        out.extend_from_slice(
            format!(
                ":{old} {new} {} {} {status}\0{path}\0",
                "a".repeat(40),
                "0".repeat(40)
            )
            .as_bytes(),
        );
    }
    out
}

#[test]
fn t10_raw_diff_counts_every_change_and_rejects_unsupported_entries() {
    let changes = parse_raw_diff(&raw(&[
        ("000000", "100644", "A", "src/new.rs"),
        ("100644", "000000", "D", "docs/old.md"),
        ("100644", "100755", "M", "src/run.sh"),
        ("100644", "120000", "T", "src/link"),
    ]))
    .unwrap();
    assert_eq!(changes.len(), 4);
    assert_eq!(changes[1].kind, ChangeKind::Deleted);
    assert_eq!(changes[2].new_mode, 0o100755);
    assert!(parse_raw_diff(b"").unwrap().is_empty());
    for bad in [
        raw(&[("160000", "160000", "M", "vendor/sub")]),
        raw(&[("100644", "100644", "U", "src/conflict.rs")]),
        raw(&[("100644", "100644", "R100", "src/a")]),
        raw(&[("100644", "100644", "M", "../escape")]),
        raw(&[("100644", "100644", "M", ".git/config")]),
        raw(&[("100644", "100644", "A", "src/a")]),
        raw(&[("100644", "040000", "M", "src/a")]),
        b":100644 100644 x y M\0".to_vec(),
        b"garbage\0src/a\0".to_vec(),
        [&raw(&[("100644", "100644", "M", "src/")])[..]].concat(),
        b":100644 100644 a b M\0\xff\xfe\0".to_vec(),
    ] {
        assert!(
            parse_raw_diff(&bad).is_err(),
            "{:?}",
            String::from_utf8_lossy(&bad)
        );
    }
}

#[test]
fn t10_scope_check_denies_outside_paths_and_any_baseline_change() {
    let s = step(&["src"], &["tests"], &["t"]);
    let change = |path: &str| ChangedPath {
        path: path.into(),
        kind: ChangeKind::Modified,
        old_mode: 0o100644,
        new_mode: 0o100644,
    };
    let v = check_scope(
        &[
            change("src/a.rs"),
            change("src2/b.rs"),
            change("tests/check.rs"),
            change("README.md"),
        ],
        &s,
    );
    assert_eq!(v.allowed, vec!["src/a.rs"]);
    assert_eq!(v.denied, vec!["src2/b.rs", "tests/check.rs", "README.md"]);
    assert_eq!(v.baseline_changed, vec!["tests/check.rs"]);
    assert!(!v.passed());
    assert!(check_scope(&[change("src/x")], &s).passed());
    assert!(
        check_scope(&[], &s).passed(),
        "no change stays within any scope"
    );
}

#[test]
fn t10_zero_missing_extra_duplicate_or_failed_tests_fail_the_inventory() {
    let expected = vec!["unit::a".to_string(), "unit::b".to_string()];
    let ok = |id: &str| (id.to_string(), TestStatus::Passed);
    assert!(check_inventory(&expected, &[ok("unit::a"), ok("unit::b")]).is_empty());
    let zero = check_inventory(&expected, &[]);
    assert!(zero.iter().any(|f| f == "no tests executed"));
    assert!(
        check_inventory(&expected, &[ok("unit::a")])
            .iter()
            .any(|f| f.contains("missing test"))
    );
    assert!(
        check_inventory(&expected, &[ok("unit::a"), ok("unit::b"), ok("unit::c")])
            .iter()
            .any(|f| f.contains("unexpected test"))
    );
    assert!(
        check_inventory(&expected, &[ok("unit::a"), ok("unit::a"), ok("unit::b")])
            .iter()
            .any(|f| f.contains("duplicate"))
    );
    for status in [TestStatus::Failed, TestStatus::Skipped] {
        assert!(
            !check_inventory(&expected, &[ok("unit::a"), ("unit::b".into(), status)]).is_empty()
        );
    }
}

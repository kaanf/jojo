use jojo_domain::dependency::*;
use jojo_domain::identity::{ArtifactId, ClockEpochId, OpaqueId, RepositoryId, Revision};
use jojo_test_support::dependency::*;
fn merged() -> Predicate {
    predicate("merge", EvidenceKind::PrMerged)
}
#[test]
fn cycle_paths_are_closed_explainable_and_deterministic_for_both_graphs() {
    for kind in [GraphKind::Artifact, GraphKind::Execution] {
        let mut g = Graph {
            kind,
            nodes: vec![node(3), node(1), node(2)],
            edges: vec![
                edge(3, 3, 1, merged()),
                edge(1, 1, 2, merged()),
                edge(2, 2, 3, merged()),
            ],
        };
        let expected = GraphError::Cycle(vec![node(1), node(2), node(3), node(1)]);
        assert_eq!(g.order(), Err(expected.clone()));
        g.nodes.reverse();
        g.edges.reverse();
        assert_eq!(g.order(), Err(expected));
    }
}
#[test]
fn self_duplicate_edges_nodes_and_missing_references_are_rejected() {
    let mut g = plan(&[1, 2], vec![edge(1, 1, 1, merged())]).execution;
    assert_eq!(g.order(), Err(GraphError::SelfEdge(node(1))));
    g.edges = vec![edge(1, 1, 2, merged()), edge(2, 1, 2, merged())];
    assert_eq!(g.order(), Err(GraphError::DuplicateEdge(OpaqueId(2))));
    g.edges = vec![edge(1, 1, 2, merged()), edge(1, 2, 1, merged())];
    assert_eq!(g.order(), Err(GraphError::DuplicateEdge(OpaqueId(1))));
    g.edges = vec![edge(1, 1, 3, merged())];
    assert_eq!(g.order(), Err(GraphError::MissingReference(node(3))));
    g.edges[0].dependent = NodeRef {
        id: OpaqueId(2),
        revision: Revision(2),
    };
    assert!(matches!(g.order(), Err(GraphError::MissingReference(_))));
    g.edges.clear();
    g.nodes.push(node(1));
    assert_eq!(g.order(), Err(GraphError::DuplicateNode(OpaqueId(1))));
}
#[test]
fn diamond_order_is_stable_and_includes_isolated_nodes() {
    let mut p = plan(
        &[5, 4, 3, 2, 1],
        vec![
            edge(4, 3, 4, merged()),
            edge(2, 1, 3, merged()),
            edge(3, 2, 4, merged()),
            edge(1, 1, 2, merged()),
        ],
    );
    assert_eq!(
        p.execution.order().unwrap(),
        vec![node(1), node(2), node(3), node(4), node(5)]
    );
    let canonical = p.canonical();
    p.execution.nodes.reverse();
    p.execution.edges.reverse();
    p.inputs.reverse();
    assert_eq!(p.canonical(), canonical);
    assert_eq!(
        ExecutionPlan::import(canonical.as_bytes())
            .unwrap()
            .canonical(),
        canonical
    );
    assert_eq!(
        evaluate_step(&p, node(5), &Facts::default(), now()).truth,
        Truth::Satisfied
    );
}
#[test]
fn opened_pr_and_provider_prose_cannot_satisfy_any_evidence_category() {
    for kind in [
        EvidenceKind::PrMerged,
        EvidenceKind::CodeVerified,
        EvidenceKind::BuildAvailable,
        EvidenceKind::DeployedHealthy,
        EvidenceKind::ValidationPassed,
    ] {
        let p = predicate("fact", kind);
        let mut o = observation("fact", kind);
        o.status = EvidenceStatus::PullRequestOpen;
        let mut f = Facts::default();
        f.observations.insert(o.id.clone(), o);
        let result = evaluate(&p, &f, now());
        assert_eq!(result.truth, Truth::Unsatisfied);
        assert!(result.reason.contains("open PR"));
    }
}
#[test]
fn failed_ci_lookup_is_unknown_and_and_combination_blocks() {
    let p = plan(
        &[1, 2, 3],
        vec![
            edge(1, 1, 3, predicate("ci", EvidenceKind::CodeVerified)),
            edge(2, 2, 3, merged()),
        ],
    );
    let mut f = Facts::default();
    let mut ci = observation("ci", EvidenceKind::CodeVerified);
    ci.status = EvidenceStatus::LookupFailed;
    f.observations.insert(ci.id.clone(), ci);
    f.observations
        .insert("merge".into(), observation("merge", EvidenceKind::PrMerged));
    let result = evaluate_step(&p, node(3), &f, now());
    assert_eq!(result.truth, Truth::Unknown);
    assert_eq!(result.reasons.len(), 2);
    assert!(
        result
            .reasons
            .iter()
            .any(|r| r.reason.contains("lookup failed"))
    );
}
#[test]
fn deployment_expiry_wrong_commit_and_wrong_environment_block() {
    let p = predicate("deploy", EvidenceKind::DeployedHealthy);
    for change in 0..5 {
        let mut o = observation("deploy", EvidenceKind::DeployedHealthy);
        match change {
            0 => o.expires_at_unix_ms = now().unix_millis,
            1 => o.binding.sources = vec![source('f')],
            2 => o.binding.environment = Some("production".into()),
            3 => o.binding.artifact_digest = Some("f".repeat(64)),
            _ => o.status = EvidenceStatus::Invalidated,
        }
        let mut f = Facts::default();
        f.observations.insert(o.id.clone(), o);
        let result = evaluate(&p, &f, now());
        assert_ne!(result.truth, Truth::Satisfied);
        assert_eq!(result.evidence_id.as_deref(), Some("deploy"));
        assert!(result.expires_at_unix_ms.is_some());
    }
}
#[test]
fn matching_receipt_unlocks_only_the_exact_dependent() {
    let mut other = requirement("deploy", EvidenceKind::DeployedHealthy);
    other.binding.environment = Some("other-env".into());
    let p = plan(
        &[1, 2, 3, 4],
        vec![
            edge(1, 1, 2, predicate("deploy", EvidenceKind::DeployedHealthy)),
            edge(
                2,
                3,
                4,
                Predicate::Evidence {
                    schema: 1,
                    requirement: other.into(),
                },
            ),
        ],
    );
    let mut f = Facts::default();
    f.observations.insert(
        "deploy".into(),
        observation("deploy", EvidenceKind::DeployedHealthy),
    );
    assert_eq!(
        evaluate_step(&p, node(2), &f, now()).truth,
        Truth::Satisfied
    );
    assert_eq!(
        evaluate_step(&p, node(4), &f, now()).truth,
        Truth::Unsatisfied
    );
}
fn selected_plan() -> (ExecutionPlan, Facts) {
    let selected = OutputSelection {
        output_id: "selected-output".into(),
        producer: node(1),
        digest: "a".repeat(64),
        sources: vec![source('a')],
    };
    let mut req = requirement("output", EvidenceKind::OutputAvailable);
    req.binding.output = Some(selected.clone());
    let mut p = plan(
        &[1, 2],
        vec![edge(
            1,
            1,
            2,
            Predicate::Evidence {
                schema: 1,
                requirement: req.clone().into(),
            },
        )],
    );
    p.inputs[1].selected_outputs = vec![selected];
    p.inputs[1].base_sources.clear();
    let mut o = observation("output", EvidenceKind::OutputAvailable);
    o.binding = req.binding;
    let mut f = Facts::default();
    f.observations.insert(o.id.clone(), o);
    (p, f)
}
#[test]
fn predecessor_branch_tip_mismatch_and_missing_source_prevent_materialization() {
    let (p, f) = selected_plan();
    let mut wrong_environment = target(vec![source('a')]);
    wrong_environment.environment = Some("wrong-environment".into());
    assert!(materialize(&p, node(2), &f, now(), &wrong_environment).is_err());
    assert_eq!(
        materialize(&p, node(2), &f, now(), &target(vec![source('a')])).unwrap(),
        target(vec![source('a')])
    );
    assert!(materialize(&p, node(2), &f, now(), &target(vec![source('b')])).is_err());
    assert!(materialize(&p, node(2), &f, now(), &target(vec![])).is_err());
    let mut wrong = source('a');
    wrong.repository = RepositoryId(OpaqueId(99));
    assert!(materialize(&p, node(2), &f, now(), &target(vec![wrong])).is_err());
    let mut stale = f.clone();
    stale.observations.get_mut("output").unwrap().status = EvidenceStatus::Invalidated;
    assert!(materialize(&p, node(2), &stale, now(), &target(vec![source('a')])).is_err());
}
#[test]
fn dirty_snapshot_materialization_binds_base_commit_and_snapshot_digest() {
    let (mut p, mut f) = selected_plan();
    let snapshot = Source {
        repository: source('a').repository,
        revision: SourceRevision::Snapshot {
            base_commit: "a".repeat(40),
            digest: "b".repeat(64),
        },
    };
    p.inputs[1].selected_outputs[0].sources = vec![snapshot.clone()];
    if let Predicate::Evidence { requirement, .. } = &mut p.execution.edges[0].predicate {
        requirement.binding.sources = vec![snapshot.clone()];
        requirement.binding.output = p.inputs[1].selected_outputs.first().cloned();
        f.observations.get_mut("output").unwrap().binding = requirement.binding.clone()
    }
    assert_eq!(
        materialize(&p, node(2), &f, now(), &target(vec![snapshot.clone()])).unwrap(),
        target(vec![snapshot])
    );
    assert!(materialize(&p, node(2), &f, now(), &target(vec![source('a')])).is_err());
}
#[test]
fn missing_stale_and_revoked_artifact_bindings_never_satisfy() {
    let a = ApprovalBinding {
        artifact: ArtifactId(OpaqueId(1)),
        revision: Revision(1),
        canonical_hash: "a".repeat(64),
        bundle_hash: "b".repeat(64),
        decision: 4,
    };
    let p = Predicate::ArtifactApproved {
        schema: 1,
        binding: a.clone(),
    };
    let mut f = Facts::default();
    assert_eq!(evaluate(&p, &f, now()).truth, Truth::Unsatisfied);
    f.approvals.insert(a.artifact, a.clone());
    assert_eq!(evaluate(&p, &f, now()).truth, Truth::Satisfied);
    f.approvals.get_mut(&a.artifact).unwrap().decision = 5;
    assert_eq!(evaluate(&p, &f, now()).truth, Truth::Unsatisfied);
}
#[test]
fn all_closed_predicate_kinds_require_their_exact_independent_selectors() {
    for kind in [
        EvidenceKind::OutputAvailable,
        EvidenceKind::CodeVerified,
        EvidenceKind::PrMerged,
        EvidenceKind::BuildAvailable,
        EvidenceKind::DeployedHealthy,
        EvidenceKind::ContractCompatible,
        EvidenceKind::ValidationPassed,
    ] {
        let mut req = requirement("e", kind);
        if kind == EvidenceKind::OutputAvailable {
            req.binding.output = Some(OutputSelection {
                output_id: "o".into(),
                producer: node(1),
                digest: "f".repeat(64),
                sources: req.binding.sources.clone(),
            });
        }
        if kind == EvidenceKind::ContractCompatible {
            let mut other = source('b');
            other.repository = RepositoryId(OpaqueId(21));
            req.binding.sources.push(other);
            req.binding.contract = Some("d".repeat(64));
        }
        let p = Predicate::Evidence {
            schema: 1,
            requirement: req.clone().into(),
        };
        assert!(p.validate().is_ok());
        let mut o = observation("e", kind);
        o.binding = req.binding;
        let mut facts = Facts::default();
        facts.observations.insert("e".into(), o);
        assert_eq!(evaluate(&p, &facts, now()).truth, Truth::Satisfied);
        facts.observations.get_mut("e").unwrap().binding.sources[0] = source('c');
        assert_ne!(evaluate(&p, &facts, now()).truth, Truth::Satisfied);
    }
}
#[test]
fn future_clock_reboot_monotonic_age_and_unknown_versions_are_fail_closed() {
    let p = predicate("merge", EvidenceKind::PrMerged);
    let mut f = Facts::default();
    f.observations
        .insert("merge".into(), observation("merge", EvidenceKind::PrMerged));
    let mut reboot = now();
    reboot.epoch = ClockEpochId(OpaqueId(2));
    assert_eq!(evaluate(&p, &f, reboot).truth, Truth::Unknown);
    let mut aged = now();
    aged.monotonic_nanos += 600_000_000;
    assert_eq!(evaluate(&p, &f, aged).truth, Truth::Unknown);
    let mut future = now();
    future.unix_millis -= 1;
    assert_eq!(evaluate(&p, &f, future).truth, Truth::Unknown);
    let Predicate::Evidence { requirement, .. } = p else {
        unreachable!()
    };
    assert_eq!(
        evaluate(
            &Predicate::Evidence {
                schema: 99,
                requirement
            },
            &f,
            now()
        )
        .truth,
        Truth::Unknown
    );
}
#[test]
fn transitive_invalidation_blocks_descendants_but_not_an_independent_branch() {
    let p = plan(
        &[1, 2, 3, 4],
        vec![
            edge(1, 1, 2, predicate("deploy", EvidenceKind::DeployedHealthy)),
            edge(2, 2, 3, merged()),
        ],
    );
    let mut f = Facts::default();
    let mut o = observation("deploy", EvidenceKind::DeployedHealthy);
    o.status = EvidenceStatus::Invalidated;
    f.observations.insert(o.id.clone(), o);
    f.observations
        .insert("merge".into(), observation("merge", EvidenceKind::PrMerged));
    assert_eq!(
        evaluate_step(&p, node(3), &f, now()).truth,
        Truth::Unsatisfied
    );
    assert_eq!(
        evaluate_step(&p, node(4), &f, now()).truth,
        Truth::Satisfied
    );
}
#[test]
fn strict_plan_codec_rejects_unknown_predicates_and_ambiguous_json() {
    let p = plan(&[1, 2], vec![edge(1, 1, 2, merged())]);
    assert!(
        ExecutionPlan::import(p.canonical().replace("PrMerged", "PrOpened").as_bytes()).is_err()
    );
    assert!(ExecutionPlan::import(br#"{"schema":1,"schema":1}"#).is_err());
    let mut p = p;
    p.execution.edges[0].predicate = Predicate::Evidence {
        schema: 1,
        requirement: requirement("bad", EvidenceKind::OutputAvailable).into(),
    };
    assert!(p.validate().is_err());
}

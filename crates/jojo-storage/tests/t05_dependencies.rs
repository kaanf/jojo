//! T05 integration: authoritative facts, plans, exact inputs and durable stop intents.
mod common;
use common::{TempDir, request};
use jojo_domain::dependency::*;
use jojo_domain::identity::{ArtifactId, FeatureId, OpaqueId, Revision};
use jojo_domain::time::Clock;
use jojo_storage::*;
use jojo_test_support::dependency::*;
use jojo_test_support::{COMPLETE_PRD_JSON, FakeClock};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
const PRD: ArtifactId = ArtifactId(OpaqueId(101));
const ADR: ArtifactId = ArtifactId(OpaqueId(102));
const PLAN: ArtifactId = ArtifactId(OpaqueId(103));
const RUN: OpaqueId = OpaqueId(500);
struct Fixture {
    dir: TempDir,
    engine: Engine,
    clock: Arc<FakeClock>,
    commands: AtomicU64,
    plan: ExecutionPlan,
    output: Observation,
}
impl Fixture {
    fn command(&self) -> u128 {
        self.commands.fetch_add(1, Ordering::Relaxed).into()
    }
    fn decide(&self, id: ArtifactId, action: DecisionAction) {
        let b = self
            .engine
            .reader()
            .unwrap()
            .read(|v| v.review_bundle(id))
            .unwrap()
            .unwrap();
        self.engine
            .human_decide(
                CommandId(OpaqueId(self.command())),
                HumanOrigin::FakeHumanFixture {
                    actor: "T05 human".into(),
                },
                b,
                action,
            )
            .unwrap();
    }
    fn record(&self, o: Observation) -> Result<Receipt, CommandError> {
        let expected = (o.revision.0 > 1).then(|| Revision(o.revision.0 - 1));
        self.engine.execute(
            request(&self.engine, self.command(), "observation", &o.canonical()),
            move |tx| {
                tx.record_dependency_observation(expected, &o)?;
                Ok("ok".into())
            },
        )
    }
    fn put(&self, p: ExecutionPlan, expected: Option<Revision>) -> Result<Receipt, CommandError> {
        self.engine.execute(
            request(&self.engine, self.command(), "plan", &p.canonical()),
            move |tx| {
                tx.put_execution_plan(PLAN, expected, PRD, ADR, &p, "T05 graph review display")?;
                Ok("ok".into())
            },
        )
    }
    fn eval(&self, step: u128, trigger: EvaluationTrigger) -> StepEvaluation {
        self.engine
            .reader()
            .unwrap()
            .read(|v| {
                v.evaluate_dependencies(PLAN, Revision(1), node(step), trigger, self.clock.now())
            })
            .unwrap()
    }
    fn reserve(
        &self,
        id: OpaqueId,
        step: u128,
        base: Vec<Source>,
    ) -> Result<Receipt, CommandError> {
        self.engine.execute(
            request(
                &self.engine,
                self.command(),
                "reserve-step",
                &format!("{id:?}|{step}|{base:?}"),
            ),
            move |tx| {
                tx.reserve_dependency_attempt(id, PRD, ADR, PLAN, node(step), &target(base))?;
                Ok("ok".into())
            },
        )
    }
    fn running(&self, id: OpaqueId) {
        self.engine
            .execute(
                request(&self.engine, self.command(), "running", &format!("{id:?}")),
                move |tx| {
                    tx.start_approved_attempt(id)?;
                    tx.mark_approval_attempt_running(id)?;
                    Ok("ok".into())
                },
            )
            .unwrap();
    }
    fn stopped(&self, id: OpaqueId) -> bool {
        self.engine
            .reader()
            .unwrap()
            .read(|v| v.approval_stop_requested(id))
            .unwrap()
    }
    fn reconcile(&self, trigger: EvaluationTrigger) {
        self.engine
            .execute(
                request(
                    &self.engine,
                    self.command(),
                    "reconcile",
                    &format!("{trigger:?}"),
                ),
                move |tx| {
                    tx.reconcile_dependencies(trigger)?;
                    Ok("ok".into())
                },
            )
            .unwrap();
    }
}
fn fixture() -> Fixture {
    let dir = TempDir::new("t05");
    let clock = Arc::new(FakeClock::new(now()));
    let engine = Engine::start(EngineConfig::new(dir.db(), clock.clone())).unwrap();
    let p = PreparedPrd::import(COMPLETE_PRD_JSON.as_bytes()).unwrap();
    engine
        .execute(request(&engine, 1, "prd", p.content()), move |tx| {
            tx.create_artifact(
                PRD,
                FeatureId(OpaqueId(1)),
                &p,
                &Provenance::new(Author::Human, "original fixture"),
            )?;
            Ok("ok".into())
        })
        .unwrap();
    let mut f = Fixture {
        dir,
        engine,
        clock,
        commands: AtomicU64::new(10),
        plan: plan(&[], vec![]),
        output: observation("output", EvidenceKind::OutputAvailable),
    };
    f.decide(PRD, DecisionAction::Approve);
    f.engine
        .execute(
            request(&f.engine, f.command(), "adr", "ADR fixture"),
            |tx| {
                tx.put_review_document(ADR, None, PRD, None, "ADR fixture", "ADR review")?;
                Ok("ok".into())
            },
        )
        .unwrap();
    f.decide(ADR, DecisionAction::Approve);
    let selected = OutputSelection {
        output_id: "predecessor-selected".into(),
        producer: node(1),
        digest: "a".repeat(64),
        sources: vec![source('a')],
    };
    let mut req = requirement("output", EvidenceKind::OutputAvailable);
    req.binding.output = Some(selected.clone());
    f.output.binding = req.binding.clone();
    f.plan = plan(
        &[1, 2, 3],
        vec![
            edge(
                11,
                1,
                2,
                Predicate::Evidence {
                    schema: 1,
                    requirement: req.into(),
                },
            ),
            edge(12, 1, 2, predicate("deploy", EvidenceKind::DeployedHealthy)),
        ],
    );
    f.plan.inputs[1].selected_outputs = vec![selected];
    f.plan.inputs[1].base_sources.clear();
    for id in [PRD, ADR] {
        let a = f
            .engine
            .reader()
            .unwrap()
            .read(|v| v.current_artifact_approval(id))
            .unwrap()
            .unwrap();
        f.plan.artifacts.nodes.push(NodeRef {
            id: id.0,
            revision: a.revision,
        });
        f.plan.approvals.push(a);
    }
    f.plan.artifacts.edges.push(Edge {
        id: OpaqueId(13),
        prerequisite: f.plan.artifacts.nodes[0],
        dependent: f.plan.artifacts.nodes[1],
        predicate: Predicate::ArtifactApproved {
            schema: 1,
            binding: f.plan.approvals[0].clone(),
        },
    });
    f.put(f.plan.clone(), None).unwrap();
    f.decide(PLAN, DecisionAction::Approve);
    f.record(f.output.clone()).unwrap();
    f.record(observation("deploy", EvidenceKind::DeployedHealthy))
        .unwrap();
    f
}
#[test]
fn immutable_graph_plan_and_selected_sources_survive_restart() {
    let f = fixture();
    assert_eq!(
        f.eval(2, EvaluationTrigger::PreLaunch).truth,
        Truth::Satisfied
    );
    f.reserve(RUN, 2, vec![source('a')]).unwrap();
    let before = f
        .engine
        .reader()
        .unwrap()
        .read(|v| v.execution_plan(PLAN, Revision(1)))
        .unwrap()
        .unwrap();
    let db = f.dir.db();
    drop(f.engine);
    let e = Engine::start(EngineConfig::new(db, f.clock)).unwrap();
    let after = e
        .reader()
        .unwrap()
        .read(|v| v.execution_plan(PLAN, Revision(1)))
        .unwrap()
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(
        e.reader()
            .unwrap()
            .read(|v| v.attempt_source_vector(RUN))
            .unwrap(),
        Some(target(vec![source('a')]))
    );
    assert_eq!(
        e.reader()
            .unwrap()
            .read(|v| v.evaluate_dependencies(
                PLAN,
                Revision(1),
                node(2),
                EvaluationTrigger::StartupRecovery,
                now()
            ))
            .unwrap()
            .truth,
        Truth::Satisfied
    );
}
#[test]
fn all_evaluator_entry_points_return_identical_decisions_and_reasons() {
    let f = fixture();
    let mut expired = now();
    expired.unix_millis += 500;
    expired.monotonic_nanos += 500_000_000;
    f.clock.set(expired);
    let unknown = f.eval(2, EvaluationTrigger::Event);
    assert_eq!(unknown.truth, Truth::Unknown);
    for trigger in [
        EvaluationTrigger::Event,
        EvaluationTrigger::Reconciliation,
        EvaluationTrigger::StartupRecovery,
        EvaluationTrigger::PreLaunch,
    ] {
        assert_eq!(f.eval(2, trigger), unknown);
        let expected = unknown.clone();
        f.engine
            .execute(
                request(
                    &f.engine,
                    f.command(),
                    "evaluate-unknown",
                    &format!("{trigger:?}"),
                ),
                move |tx| {
                    assert_eq!(
                        tx.evaluate_dependencies(PLAN, Revision(1), node(2), trigger)?,
                        expected
                    );
                    Ok("unknown blocks".into())
                },
            )
            .unwrap();
    }
    f.clock.set(now());
    for status in [
        EvidenceStatus::Verified,
        EvidenceStatus::LookupFailed,
        EvidenceStatus::Invalidated,
    ] {
        if status != EvidenceStatus::Verified {
            let rev = if status == EvidenceStatus::LookupFailed {
                2
            } else {
                3
            };
            let mut o = observation("deploy", EvidenceKind::DeployedHealthy);
            o.revision = Revision(rev);
            o.status = status;
            f.record(o).unwrap();
        }
        let expected = f.eval(2, EvaluationTrigger::Event);
        for trigger in [
            EvaluationTrigger::Event,
            EvaluationTrigger::Reconciliation,
            EvaluationTrigger::StartupRecovery,
            EvaluationTrigger::PreLaunch,
        ] {
            assert_eq!(f.eval(2, trigger), expected);
            let p = PLAN;
            let now = f.clock.now();
            let copy = expected.clone();
            f.engine
                .execute(
                    request(
                        &f.engine,
                        f.command(),
                        "evaluate",
                        &format!("{trigger:?}|{status:?}"),
                    ),
                    move |tx| {
                        let actual = tx.evaluate_dependencies(p, Revision(1), node(2), trigger)?;
                        assert_eq!(actual, copy);
                        assert_eq!(now, jojo_test_support::dependency::now());
                        Ok("ok".into())
                    },
                )
                .unwrap();
        }
    }
}
#[test]
fn invalidated_receipt_stops_only_affected_running_work_and_preserves_result() {
    let f = fixture();
    f.reserve(RUN, 2, vec![source('a')]).unwrap();
    f.running(RUN);
    let other = OpaqueId(501);
    f.reserve(other, 3, vec![source('a')]).unwrap();
    f.running(other);
    f.engine
        .execute(request(&f.engine, f.command(), "seal", ""), |tx| {
            tx.seal_approval_result(RUN, "immutable historical candidate")?;
            Ok("ok".into())
        })
        .unwrap();
    let mut invalid = observation("deploy", EvidenceKind::DeployedHealthy);
    invalid.revision = Revision(2);
    invalid.status = EvidenceStatus::Invalidated;
    f.record(invalid).unwrap();
    assert!(f.stopped(RUN));
    assert!(!f.stopped(other));
    assert_ne!(f.eval(2, EvaluationTrigger::Event).truth, Truth::Satisfied);
    assert_eq!(f.eval(3, EvaluationTrigger::Event).truth, Truth::Satisfied);
    assert_eq!(
        f.engine
            .reader()
            .unwrap()
            .read(|v| v.approval_result(RUN))
            .unwrap()
            .as_deref(),
        Some("immutable historical candidate")
    );
    assert!(
        !f.engine
            .reader()
            .unwrap()
            .read(|v| v.approval_result_applicable(RUN))
            .unwrap()
    );
}
#[test]
fn invalidated_active_receipt_stop_survives_restart_without_replacement() {
    let f = fixture();
    f.reserve(RUN, 2, vec![source('a')]).unwrap();
    f.running(RUN);
    let mut bad = observation("deploy", EvidenceKind::DeployedHealthy);
    bad.revision = Revision(2);
    bad.status = EvidenceStatus::Invalidated;
    f.record(bad).unwrap();
    assert!(f.stopped(RUN));
    let db = f.dir.db();
    drop(f.engine);
    let e = Engine::start(EngineConfig::new(db, f.clock)).unwrap();
    assert!(
        e.reader()
            .unwrap()
            .read(|v| v.approval_stop_requested(RUN))
            .unwrap()
    );
    assert!(
        e.execute(request(&e, 1000, "restart-denied", ""), |tx| {
            tx.start_approved_attempt(RUN)?;
            Ok("".into())
        })
        .is_err()
    );
}
#[test]
fn expiry_reconciliation_and_prelaunch_check_block_without_silent_restart() {
    let f = fixture();
    f.reserve(RUN, 2, vec![source('a')]).unwrap();
    let mut later = now();
    later.unix_millis += 500;
    later.monotonic_nanos += 500_000_000;
    f.clock.set(later);
    assert_eq!(
        f.eval(2, EvaluationTrigger::PreLaunch).truth,
        Truth::Unknown
    );
    f.reconcile(EvaluationTrigger::PreLaunch);
    assert!(f.stopped(RUN));
    assert!(
        f.engine
            .execute(request(&f.engine, f.command(), "start", ""), |tx| {
                tx.start_approved_attempt(RUN)?;
                Ok("".into())
            })
            .is_err()
    );
    assert!(f.reserve(OpaqueId(502), 2, vec![source('a')]).is_err());
}
#[test]
fn opened_pr_failed_lookup_and_mismatched_deployment_never_reserve() {
    for case in 0..6 {
        let f = fixture();
        let mut p = f.plan.clone();
        let mut o = observation("case", EvidenceKind::DeployedHealthy);
        let mut req = requirement("case", EvidenceKind::DeployedHealthy);
        match case {
            0 => o.status = EvidenceStatus::PullRequestOpen,
            1 => o.status = EvidenceStatus::LookupFailed,
            2 => o.binding.sources = vec![source('f')],
            3 => o.binding.environment = Some("production".into()),
            4 => o.binding.artifact_digest = Some("f".repeat(64)),
            _ => {
                req.evidence_id = "missing".into();
            }
        }
        p.execution.edges[1].predicate = Predicate::Evidence {
            schema: 1,
            requirement: req.into(),
        };
        f.put(p, Some(Revision(1))).unwrap();
        f.decide(PLAN, DecisionAction::Approve);
        f.record(o).unwrap();
        let result = f
            .engine
            .reader()
            .unwrap()
            .read(|v| {
                v.evaluate_dependencies(PLAN, Revision(2), node(2), EvaluationTrigger::Event, now())
            })
            .unwrap();
        assert_ne!(result.truth, Truth::Satisfied);
        if case == 1 || case == 5 {
            assert_eq!(result.truth, Truth::Unknown);
        }
        assert!(f.reserve(RUN, 2, vec![source('a')]).is_err());
    }
}
#[test]
fn source_materialization_is_exact_and_old_approval_path_cannot_bypass_it() {
    let f = fixture();
    assert!(f.reserve(RUN, 2, vec![source('b')]).is_err());
    assert!(!f.stopped(RUN));
    assert!(
        f.engine
            .execute(
                request(&f.engine, f.command(), "legacy-reserve", ""),
                |tx| {
                    tx.reserve_approved_attempt(RUN, PRD, ADR, PLAN)?;
                    Ok("".into())
                }
            )
            .is_err()
    );
    let resolved = f
        .engine
        .reader()
        .unwrap()
        .read(|v| {
            v.materialize_sources(
                PLAN,
                Revision(1),
                node(2),
                now(),
                &target(vec![source('a')]),
            )
        })
        .unwrap()
        .unwrap();
    assert_eq!(resolved, target(vec![source('a')]));
    f.reserve(RUN, 2, resolved.sources).unwrap();
}
#[test]
fn missing_revoked_or_changed_artifact_approvals_block_and_stop() {
    for revoke in [true, false] {
        let f = fixture();
        f.reserve(RUN, 2, vec![source('a')]).unwrap();
        f.running(RUN);
        if revoke {
            f.decide(PRD, DecisionAction::Revoke);
        } else {
            let p = PreparedPrd::import(
                COMPLETE_PRD_JSON
                    .replace(
                        "Let one person keep a private list of articles to read later.",
                        "Keep private articles to read later.",
                    )
                    .as_bytes(),
            )
            .unwrap();
            f.engine
                .execute(
                    request(&f.engine, f.command(), "edit", p.content()),
                    move |tx| {
                        tx.revise_artifact(
                            PRD,
                            Revision(1),
                            &p,
                            &Provenance::new(Author::Human, "changed requirements"),
                        )?;
                        Ok("".into())
                    },
                )
                .unwrap();
        }
        assert!(f.stopped(RUN));
        assert_ne!(f.eval(2, EvaluationTrigger::Event).truth, Truth::Satisfied);
        assert!(f.reserve(OpaqueId(503), 2, vec![source('a')]).is_err());
    }
}
#[test]
fn graph_and_binding_rejections_leave_no_partial_plan_or_events() {
    let f = fixture();
    let before = f.engine.reader().unwrap().snapshot().unwrap();
    for case in 0..5 {
        let mut p = f.plan.clone();
        match case {
            0 => p
                .execution
                .edges
                .push(edge(99, 2, 1, predicate("cycle", EvidenceKind::PrMerged))),
            1 => p.execution.nodes.push(node(1)),
            2 => p.execution.edges.push(p.execution.edges[0].clone()),
            3 => p.execution.edges[0].prerequisite = node(99),
            _ => p.approvals[0].decision += 1,
        };
        assert!(f.put(p, Some(Revision(1))).is_err());
        assert_eq!(f.engine.reader().unwrap().snapshot().unwrap(), before);
    }
    assert!(
        f.engine
            .reader()
            .unwrap()
            .read(|v| v.execution_plan(PLAN, Revision(2)))
            .unwrap()
            .is_none()
    );
}
#[test]
fn plan_revision_changes_require_new_human_review_and_preserve_old_graphs() {
    let f = fixture();
    f.reserve(RUN, 2, vec![source('a')]).unwrap();
    let old = f
        .engine
        .reader()
        .unwrap()
        .read(|v| v.execution_plan(PLAN, Revision(1)))
        .unwrap()
        .unwrap();
    let mut p = f.plan.clone();
    p.permission_digest = "f".repeat(64);
    f.put(p, Some(Revision(1))).unwrap();
    assert!(f.stopped(RUN));
    assert_ne!(f.eval(2, EvaluationTrigger::Event).truth, Truth::Satisfied);
    f.decide(PLAN, DecisionAction::Approve);
    assert!(f.stopped(RUN));
    assert_eq!(
        old,
        f.engine
            .reader()
            .unwrap()
            .read(|v| v.execution_plan(PLAN, Revision(1)))
            .unwrap()
            .unwrap()
    );
}
#[test]
fn fact_cas_and_transaction_failure_preserve_history_and_readiness() {
    let f = fixture();
    f.reserve(RUN, 2, vec![source('a')]).unwrap();
    let before = f.engine.reader().unwrap().snapshot().unwrap();
    let mut invalid = observation("deploy", EvidenceKind::DeployedHealthy);
    invalid.revision = Revision(2);
    invalid.status = EvidenceStatus::Invalidated;
    let fail = invalid.clone();
    assert!(
        f.engine
            .execute(
                request(
                    &f.engine,
                    f.command(),
                    "rollback-observation",
                    &fail.canonical()
                ),
                move |tx| {
                    tx.record_dependency_observation(Some(Revision(1)), &fail)?;
                    Err(TxError::Rejected("fail after stop intent".into()))
                }
            )
            .is_err()
    );
    assert_eq!(f.engine.reader().unwrap().snapshot().unwrap(), before);
    assert!(!f.stopped(RUN));
    assert_eq!(f.eval(2, EvaluationTrigger::Event).truth, Truth::Satisfied);
    f.record(invalid.clone()).unwrap();
    assert_eq!(
        f.engine
            .reader()
            .unwrap()
            .read(|v| v.dependency_observation("deploy", Revision(1)))
            .unwrap(),
        Some(observation("deploy", EvidenceKind::DeployedHealthy))
    );
    assert!(f.record(invalid).is_err());
    assert!(f.stopped(RUN));
}
#[test]
fn schema_three_upgrades_with_original_history_and_receipts_intact() {
    let dir = TempDir::new("t05-upgrade");
    let mut cfg = EngineConfig::new(dir.db(), Arc::new(FakeClock::new(now())));
    cfg.migrations = MigrationPlan::new(MigrationPlan::production().steps()[..3].to_vec()).unwrap();
    let e = Engine::start(cfg).unwrap();
    common::create(&e, 1, &common::key("counter", 1));
    e.shutdown().unwrap();
    let e = common::start(&dir.db());
    let report = &e.startup_report().migration;
    assert_eq!((report.from_version, report.to_version), (3, 13));
    assert!(report.backup.as_ref().unwrap().exists());
    assert!(common::create(&e, 1, &common::key("counter", 1)).replayed);
}

#[test]
fn prelaunch_source_drift_commits_stop_intent_and_cannot_be_reversed() {
    let f = fixture();
    f.reserve(RUN, 2, vec![source('a')]).unwrap();
    f.engine
        .execute(
            request(&f.engine, f.command(), "prelaunch-source", ""),
            |tx| {
                let result = tx.revalidate_dependency_attempt(
                    RUN,
                    &target(vec![source('b')]),
                    EvaluationTrigger::PreLaunch,
                )?;
                assert_ne!(result.truth, Truth::Satisfied);
                Ok("blocked".into())
            },
        )
        .unwrap();
    assert!(f.stopped(RUN));
    f.engine
        .execute(
            request(&f.engine, f.command(), "prelaunch-again", ""),
            |tx| {
                let result = tx.revalidate_dependency_attempt(
                    RUN,
                    &target(vec![source('a')]),
                    EvaluationTrigger::PreLaunch,
                )?;
                assert_ne!(result.truth, Truth::Satisfied);
                Ok("still blocked".into())
            },
        )
        .unwrap();
}

#[test]
fn transitive_receipt_invalidation_stops_active_descendants_not_independent_steps() {
    let f = fixture();
    let mut p = f.plan.clone();
    p.execution.nodes.push(node(4));
    p.inputs.push(StepInputs {
        step: node(4),
        environment: Some("test-env".into()),
        selected_outputs: vec![],
        base_sources: vec![source('a')],
    });
    p.execution
        .edges
        .push(edge(21, 2, 4, predicate("merge", EvidenceKind::PrMerged)));
    f.put(p, Some(Revision(1))).unwrap();
    f.decide(PLAN, DecisionAction::Approve);
    f.record(observation("merge", EvidenceKind::PrMerged))
        .unwrap();
    f.reserve(RUN, 4, vec![source('a')]).unwrap();
    f.running(RUN);
    let independent = OpaqueId(501);
    f.reserve(independent, 3, vec![source('a')]).unwrap();
    f.running(independent);
    let mut bad = observation("deploy", EvidenceKind::DeployedHealthy);
    bad.revision = Revision(2);
    bad.status = EvidenceStatus::Invalidated;
    f.record(bad).unwrap();
    assert!(f.stopped(RUN));
    assert!(!f.stopped(independent));
}

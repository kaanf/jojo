//! Deterministic T06: no process execution, network, credentials or timing sleeps.
mod common;
use common::{TempDir, request};
use jojo_domain::dependency::*;
use jojo_domain::identity::{ArtifactId, FeatureId, OpaqueId, RepositoryId, Revision};
use jojo_domain::provider::ProviderKind;
use jojo_domain::scheduler::*;
use jojo_domain::time::Clock;
use jojo_storage::*;
use jojo_test_support::dependency::*;
use jojo_test_support::{COMPLETE_PRD_JSON, FakeClock};
use std::collections::BTreeMap;
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicU64, Ordering},
};
const PRD: ArtifactId = ArtifactId(OpaqueId(101));
const ADR: ArtifactId = ArtifactId(OpaqueId(102));
const PLAN: ArtifactId = ArtifactId(OpaqueId(103));
#[derive(Clone)]
struct Job {
    repo: u128,
    provider: ProviderKind,
    mode: ResourceMode,
    priority: u64,
    extra: Vec<ResourceNeed>,
}
fn job(repo: u128, provider: ProviderKind) -> Job {
    Job {
        repo,
        provider,
        mode: ResourceMode::Shared,
        priority: 0,
        extra: vec![],
    }
}
fn claude(repo: u128) -> Job {
    job(repo, ProviderKind::ClaudeCode)
}
fn codex(repo: u128) -> Job {
    job(repo, ProviderKind::Codex)
}
fn need(units: u64) -> ResourceNeed {
    ResourceNeed {
        key: "named:pool".into(),
        units,
        mode: ResourceMode::Shared,
    }
}
struct Fixture {
    dir: TempDir,
    engine: Engine,
    clock: Arc<FakeClock>,
    commands: AtomicU64,
    plan: ExecutionPlan,
}
impl Fixture {
    fn id(&self) -> u128 {
        self.commands.fetch_add(1, Ordering::SeqCst).into()
    }
    fn execute<F>(&self, f: F) -> Result<Receipt, CommandError>
    where
        F: FnOnce(&mut Tx<'_>) -> Result<String, TxError> + Send + 'static,
    {
        self.engine
            .execute(request(&self.engine, self.id(), "T06", "controlled"), f)
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
                CommandId(OpaqueId(self.id())),
                HumanOrigin::FakeHumanFixture {
                    actor: "T06 human".into(),
                },
                b,
                action,
            )
            .unwrap();
    }
    fn queue(&self, step: u128) {
        let sources = SourceVector {
            sources: self.plan.inputs[(step - 1) as usize].base_sources.clone(),
            environment: Some("test-env".into()),
        };
        self.execute(move |tx| {
            tx.queue_step(PRD, ADR, PLAN, Revision(1), node(step), &sources)?;
            Ok("queued".into())
        })
        .unwrap();
    }
    fn config(&self, global: u32, provider: u32, repo: u32, pool: u32) -> u64 {
        let expected = self
            .engine
            .reader()
            .unwrap()
            .read(|v| v.resource_config())
            .unwrap()
            .revision;
        let caps = BTreeMap::from([
            ("global".into(), global),
            ("provider:claude".into(), provider),
            ("provider:codex".into(), provider),
            ("named:pool".into(), pool),
        ]);
        self.execute(move |tx| Ok(tx.configure_resources(expected, repo, caps)?.to_string()))
            .unwrap()
            .response
            .parse()
            .unwrap()
    }
    fn scan(&self) -> Vec<u128> {
        let rev = self
            .engine
            .reader()
            .unwrap()
            .read(|v| v.resource_config())
            .unwrap()
            .revision;
        let base = self.id() * 100;
        let ids: Vec<_> = (base..base + 50).map(OpaqueId).collect();
        let r = self
            .execute(move |tx| {
                Ok(tx
                    .schedule(rev, &ids)?
                    .iter()
                    .filter(|d| d.attempt.is_some())
                    .map(|d| d.step.id.0.to_string())
                    .collect::<Vec<_>>()
                    .join(","))
            })
            .unwrap();
        if r.response.is_empty() {
            vec![]
        } else {
            r.response.split(',').map(|s| s.parse().unwrap()).collect()
        }
    }
    fn snapshot(&self) -> Snapshot {
        self.engine.reader().unwrap().snapshot().unwrap()
    }
    fn queue_status(&self) -> Vec<QueueStatus> {
        self.engine
            .reader()
            .unwrap()
            .read(|v| v.scheduler_queue())
            .unwrap()
    }
    fn attempt(&self, step: u128) -> OpaqueId {
        let events = self
            .engine
            .reader()
            .unwrap()
            .events_after(EventSeq(0), 10000)
            .unwrap();
        let event = events
            .iter()
            .rev()
            .find(|e| {
                e.kind == "SchedulerReserved"
                    && e.payload.contains(&format!("id: OpaqueId({step})"))
            })
            .unwrap();
        let first = event.payload.split('|').next().unwrap();
        OpaqueId(
            first
                .trim_start_matches("OpaqueId(")
                .trim_end_matches(')')
                .parse()
                .unwrap(),
        )
    }
    fn retire(&self, step: u128) {
        let a = self.attempt(step);
        self.execute(move |tx| {
            tx.retire_unissued_reservation(a)?;
            Ok("retired".into())
        })
        .unwrap();
    }
    fn control(&self, step: u128, intent: &str, cooldown: i64) {
        let expected = self
            .queue_status()
            .into_iter()
            .find(|q| q.step == node(step))
            .unwrap()
            .control_revision;
        let intent = intent.to_owned();
        self.execute(move |tx| {
            tx.control_scheduled_step(node(step), expected, &intent, cooldown)?;
            Ok("controlled".into())
        })
        .unwrap();
    }
    fn issued(&self, step: u128) {
        let a = self.attempt(step);
        self.execute(move |tx| {
            tx.advance_effect(
                &format!("scheduler-launch:{:032x}", a.0),
                EffectState::IssuedUnknown,
            )?;
            Ok("unknown-live".into())
        })
        .unwrap();
    }
    fn advance(&self, nanos: u64) {
        let mut t = self.clock.now();
        t.monotonic_nanos += nanos;
        t.unix_millis += (nanos / 1_000_000) as i64;
        self.clock.set(t);
    }
    fn restart(&mut self) {
        let placeholder = TempDir::new("placeholder");
        let replacement =
            Engine::start(EngineConfig::new(placeholder.db(), self.clock.clone())).unwrap();
        let old = std::mem::replace(&mut self.engine, replacement);
        drop(old);
        let next = Engine::start(EngineConfig::new(self.dir.db(), self.clock.clone())).unwrap();
        self.engine = next;
    }
    fn open(&self) {
        if self.engine.epoch().0 > 1 {
            // A reopened engine opens only through a reviewed P1-11 recovery start.
            let inspection =
                inspect_recovery(&self.engine, &OsProcessInspector::default(), &|_| None).unwrap();
            self.execute(move |tx| {
                tx.start_after_recovery(
                    &inspection,
                    HumanOrigin::FakeHumanFixture {
                        actor: "T06".into(),
                    },
                )?;
                Ok("open after controlled recovery inspection".into())
            })
            .unwrap();
            return;
        }
        self.execute(|tx| {
            tx.set_admission(Admission::Open)?;
            Ok("open".into())
        })
        .unwrap();
    }
}
fn fixture(jobs: Vec<Job>, cap: u32) -> Fixture {
    let dir = TempDir::new("t06");
    let clock = Arc::new(FakeClock::new(now()));
    let engine = Engine::start(EngineConfig::new(dir.db(), clock.clone())).unwrap();
    let mut f = Fixture {
        dir,
        engine,
        clock,
        commands: AtomicU64::new(10),
        plan: plan(&[], vec![]),
    };
    let p = PreparedPrd::import(COMPLETE_PRD_JSON.as_bytes()).unwrap();
    f.execute(move |tx| {
        tx.create_artifact(
            PRD,
            FeatureId(OpaqueId(1)),
            &p,
            &Provenance::new(Author::Human, "original T06 fixture"),
        )?;
        Ok("prd".into())
    })
    .unwrap();
    f.decide(PRD, DecisionAction::Approve);
    f.execute(|tx| {
        tx.put_review_document(ADR, None, PRD, None, "T06 ADR", "reviewed ADR")?;
        Ok("adr".into())
    })
    .unwrap();
    f.decide(ADR, DecisionAction::Approve);
    f.plan = plan(&(1..=jobs.len() as u128).collect::<Vec<_>>(), vec![]);
    f.plan.schema = 2;
    for (i, j) in jobs.into_iter().enumerate() {
        f.plan.inputs[i].base_sources[0].repository = RepositoryId(OpaqueId(j.repo));
        f.plan.resources.push(StepResources {
            step: node(i as u128 + 1),
            repository: RepositoryId(OpaqueId(j.repo)),
            provider: j.provider,
            priority: j.priority,
            additional: j.extra,
            repository_mode: j.mode,
        });
    }
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
    let p = f.plan.clone();
    f.execute(move |tx| {
        tx.put_execution_plan(PLAN, None, PRD, ADR, &p, "T06 reviewed resources")?;
        Ok("plan".into())
    })
    .unwrap();
    f.decide(PLAN, DecisionAction::Approve);
    f.config(cap, cap, cap, 4);
    f.open();
    f
}
#[test]
fn t06_global_cap_one_retains_original_ready_order() {
    let f = fixture(vec![claude(1), codex(2)], 1);
    f.queue(1);
    f.queue(2);
    let before = f.queue_status();
    assert_eq!(f.scan(), vec![1]);
    assert!(f.scan().is_empty());
    assert_eq!(f.snapshot().live_claims.len(), 3);
    assert_eq!(f.queue_status()[1].ready_sequence, before[1].ready_sequence);
    f.retire(1);
    assert_eq!(f.scan(), vec![2]);
}
#[test]
fn t06_cap_two_mixed_providers_independent_repositories_overlap() {
    let f = fixture(vec![claude(1), codex(2)], 2);
    f.queue(1);
    f.queue(2);
    assert_eq!(f.scan(), vec![1, 2]);
    let s = f.snapshot();
    assert_eq!(s.live_claims.len(), 6);
    assert_eq!(s.effects.len(), 2);
    assert!(s.effects.iter().all(|e| e.state == EffectState::NotIssued));
    assert!(
        s.live_claims
            .iter()
            .any(|c| c.resource_key == "provider:claude")
    );
    assert!(
        s.live_claims
            .iter()
            .any(|c| c.resource_key == "provider:codex")
    );
}
#[test]
fn t06_same_repository_parallel_reservations_have_distinct_owners() {
    let f = fixture(vec![claude(1), codex(1)], 2);
    f.queue(1);
    f.queue(2);
    assert_eq!(f.scan(), vec![1, 2]);
    let held: Vec<_> = f
        .snapshot()
        .live_claims
        .into_iter()
        .filter(|c| c.resource_key.starts_with("repository:"))
        .collect();
    assert_eq!(held.len(), 2);
    assert_ne!(held[0].holder, held[1].holder);
}
#[test]
fn t06_concurrent_competition_for_same_step_has_one_winner() {
    let f = fixture(vec![claude(1)], 2);
    f.queue(1);
    let barrier = Barrier::new(2);
    let results = std::thread::scope(|s| {
        let a = s.spawn(|| {
            barrier.wait();
            f.scan()
        });
        let b = s.spawn(|| {
            barrier.wait();
            f.scan()
        });
        vec![a.join().unwrap(), b.join().unwrap()]
    });
    assert_eq!(results.iter().map(Vec::len).sum::<usize>(), 1);
    assert_eq!(f.snapshot().effects.len(), 1);
    assert_eq!(f.snapshot().live_claims.len(), 3);
}
#[test]
fn t06_atomic_all_or_nothing_when_named_resource_is_unavailable() {
    let mut j = claude(1);
    j.extra = vec![need(5)];
    let f = fixture(vec![j, codex(2)], 2);
    f.queue(1);
    f.queue(2);
    assert_eq!(f.scan(), vec![2]);
    assert_eq!(f.snapshot().live_claims.len(), 3);
    assert!(
        f.snapshot()
            .live_claims
            .iter()
            .all(|c| c.resource_key != "named:pool")
    );
}
#[test]
fn t06_saturated_provider_and_repository_allow_unrelated_work() {
    let f = fixture(vec![claude(1), claude(2), codex(1), codex(3)], 4);
    f.config(4, 1, 1, 4);
    f.queue(1);
    assert_eq!(f.scan(), vec![1]);
    for i in 2..=4 {
        f.queue(i);
    }
    assert_eq!(f.scan(), vec![4]);
}
#[test]
fn t06_exclusive_waiter_drains_shared_holders_and_unrelated_repo_continues() {
    let mut exclusive = claude(1);
    exclusive.mode = ResourceMode::Exclusive;
    let f = fixture(vec![claude(1), exclusive, codex(1), codex(2)], 4);
    f.queue(1);
    assert_eq!(f.scan(), vec![1]);
    for i in 2..=4 {
        f.queue(i);
    }
    assert_eq!(f.scan(), vec![4]);
    assert!(!f.queue_status()[1].barriers.is_empty());
    f.retire(1);
    assert_eq!(f.scan(), vec![2]);
    assert!(f.scan().is_empty());
    f.retire(2);
    assert_eq!(f.scan(), vec![3]);
}
#[test]
fn t06_three_actual_skips_create_contested_drain_barrier() {
    let mut large = claude(1);
    large.extra = vec![need(4)];
    let mut small = codex(2);
    small.extra = vec![need(1)];
    let f = fixture(
        vec![
            small.clone(),
            large,
            small.clone(),
            small.clone(),
            small.clone(),
            small,
            codex(3),
        ],
        10,
    );
    f.queue(1);
    assert_eq!(f.scan(), vec![1]);
    f.queue(2);
    for i in 3..=5 {
        f.queue(i);
        assert_eq!(f.scan(), vec![i]);
        assert_eq!(f.queue_status()[1].skips, i as u64 - 2);
    }
    let q = f.queue_status();
    assert_eq!(q[1].barriers, vec!["named:pool"]);
    for _ in 0..4 {
        assert!(f.scan().is_empty());
    }
    assert_eq!(f.queue_status()[1].skips, 3);
    f.queue(6);
    f.queue(7);
    assert_eq!(f.scan(), vec![7]);
    for i in [1, 3, 4, 5] {
        f.retire(i);
    }
    assert_eq!(f.scan(), vec![2]);
    f.retire(2);
    assert_eq!(f.scan(), vec![6]);
}
#[test]
fn t06_capacity_shrink_retains_incumbents_and_increase_admits() {
    let f = fixture(vec![claude(1), codex(2), codex(3)], 2);
    f.queue(1);
    f.queue(2);
    assert_eq!(f.scan(), vec![1, 2]);
    let before = f.snapshot().live_claims;
    f.config(1, 2, 2, 4);
    f.queue(3);
    assert!(f.scan().is_empty());
    assert_eq!(f.snapshot().live_claims, before);
    f.config(3, 2, 2, 4);
    assert_eq!(f.scan(), vec![3]);
}
#[test]
fn t06_stale_configuration_and_duplicate_commands_cannot_readmit() {
    let f = fixture(vec![claude(1)], 1);
    f.queue(1);
    let before = f.snapshot();
    assert!(
        f.execute(|tx| {
            tx.schedule(1, &[OpaqueId(900)])?;
            Ok("stale".into())
        })
        .is_err()
    );
    assert_eq!(f.snapshot(), before);
    let id = f.id();
    let req = request(&f.engine, id, "schedule", "config2");
    let original = f
        .engine
        .execute(req.clone(), |tx| {
            Ok(format!("{:?}", tx.schedule(2, &[OpaqueId(900)])?))
        })
        .unwrap();
    let replay = f
        .engine
        .execute(req, |_| panic!("receipt must replay"))
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(original.response, replay.response);
    assert!(f.scan().is_empty());
}
#[test]
fn t06_unknown_live_claims_survive_heartbeat_expiry_and_restart() {
    let mut f = fixture(vec![claude(1), codex(2)], 1);
    f.queue(1);
    f.queue(2);
    assert_eq!(f.scan(), vec![1]);
    f.issued(1);
    let a = f.attempt(1);
    let expiry = f.clock.now().unix_millis + 10;
    f.execute(move |tx| {
        tx.record_scheduler_heartbeat(a, expiry)?;
        Ok("heartbeat hint".into())
    })
    .unwrap();
    let before = f.snapshot().live_claims;
    f.advance(9_000_000_000_000);
    assert!(f.scan().is_empty());
    assert_eq!(f.snapshot().live_claims, before);
    f.restart();
    assert_eq!(f.snapshot().session.admission, Admission::Closed);
    assert!(
        f.snapshot()
            .live_claims
            .iter()
            .all(|c| c.state == ClaimState::Quarantined)
    );
    assert!(f.scan().is_empty());
    f.open();
    assert!(f.scan().is_empty());
    let a = f.attempt(1);
    assert!(
        f.execute(move |tx| {
            tx.retire_unissued_reservation(a)?;
            Ok("unproven release".into())
        })
        .is_err()
    );
}
#[test]
fn t06_fair_order_cursor_and_age_survive_restart() {
    let mut f = fixture(vec![claude(1), codex(2), claude(1)], 1);
    f.queue(1);
    f.queue(2);
    f.queue(3);
    assert_eq!(f.scan(), vec![1]);
    f.retire(1);
    let before = f.queue_status();
    f.advance(600_000_000_000);
    f.execute(|tx| {
        tx.set_admission(Admission::Closed)?;
        Ok("closed".into())
    })
    .unwrap();
    assert!(f.scan().is_empty());
    let aged = f.queue_status();
    assert!(aged[1].eligible_nanos >= 600_000_000_000);
    f.restart();
    assert_eq!(f.queue_status(), aged);
    f.open();
    assert_eq!(f.scan(), vec![2]);
    assert_eq!(f.queue_status()[2].ready_sequence, before[2].ready_sequence);
}
#[test]
fn t06_cancel_before_cooldown_prevents_reservation_and_is_irreversible() {
    let f = fixture(vec![claude(1)], 1);
    f.queue(1);
    f.control(1, "RUN", i64::MAX);
    f.control(1, "CANCEL", i64::MAX);
    assert!(f.scan().is_empty());
    assert_eq!(f.queue_status()[0].intent, "CANCEL");
    assert!(
        f.execute(|tx| {
            tx.control_scheduled_step(node(1), Revision(1), "RUN", 0)?;
            Ok("revive".into())
        })
        .is_err()
    );
}
#[test]
fn t06_revocation_before_cooldown_retires_unissued_and_retains_unknown() {
    let f = fixture(vec![claude(1), codex(2)], 2);
    f.queue(1);
    f.queue(2);
    assert_eq!(f.scan(), vec![1, 2]);
    f.issued(2);
    f.control(1, "RUN", i64::MAX);
    f.control(2, "RUN", i64::MAX);
    f.decide(PLAN, DecisionAction::Revoke);
    let s = f.snapshot();
    assert_eq!(s.live_claims.len(), 3);
    let a = f.attempt(1);
    assert!(
        f.engine
            .reader()
            .unwrap()
            .read(|v| v.approval_result(a))
            .unwrap()
            .unwrap()
            .starts_with("NOT_STARTED")
    );
    assert!(f.scan().is_empty());
}
#[test]
fn t06_dependency_invalidation_blocks_admission() {
    let f = fixture(vec![claude(20), codex(20)], 2);
    let mut p = f.plan.clone();
    p.execution.edges.push(edge(
        55,
        1,
        2,
        predicate("build", EvidenceKind::BuildAvailable),
    ));
    let o = observation("build", EvidenceKind::BuildAvailable);
    f.execute(move |tx| {
        tx.record_dependency_observation(None, &o)?;
        Ok("fact".into())
    })
    .unwrap();
    f.execute(move |tx| {
        tx.put_execution_plan(
            PLAN,
            Some(Revision(1)),
            PRD,
            ADR,
            &p,
            "new reviewed dependency",
        )?;
        Ok("plan2".into())
    })
    .unwrap();
    f.decide(PLAN, DecisionAction::Approve);
    let sources = target(vec![source('a')]);
    f.execute(move |tx| {
        tx.queue_step(PRD, ADR, PLAN, Revision(2), node(2), &sources)?;
        Ok("ready".into())
    })
    .unwrap();
    let mut o = observation("build", EvidenceKind::BuildAvailable);
    o.revision = Revision(2);
    o.status = EvidenceStatus::Invalidated;
    f.execute(move |tx| {
        tx.record_dependency_observation(Some(Revision(1)), &o)?;
        Ok("invalid".into())
    })
    .unwrap();
    assert!(f.scan().is_empty());
    assert!(f.snapshot().live_claims.is_empty());
}
#[test]
fn t06_priority_aging_and_ties_use_original_sequence() {
    let mut high = claude(1);
    high.priority = 2;
    let f = fixture(vec![claude(1), high], 1);
    f.queue(1);
    f.advance(1_800_000_000_000);
    f.queue(2);
    assert_eq!(f.scan(), vec![1]);
}
#[test]
fn t06_plan_resource_changes_require_review_and_schema_one_is_preserved() {
    let f = fixture(vec![claude(1)], 1);
    let bytes = f.plan.canonical();
    assert_eq!(ExecutionPlan::import(bytes.as_bytes()).unwrap(), f.plan);
    let old = plan(&[1], vec![]);
    assert_eq!(
        ExecutionPlan::import(old.canonical().as_bytes()).unwrap(),
        old
    );
    let mut invalid = f.plan.clone();
    invalid.resources[0].repository = RepositoryId(OpaqueId(88));
    assert!(invalid.validate().is_err());
    let mut invalid = f.plan.clone();
    invalid.resources[0].additional.push(ResourceNeed {
        key: "global".into(),
        units: 1,
        mode: ResourceMode::Shared,
    });
    assert!(invalid.validate().is_err());
    assert!(
        f.execute(|tx| {
            tx.reserve_dependency_attempt(
                OpaqueId(900),
                PRD,
                ADR,
                PLAN,
                node(1),
                &target(vec![source('a')]),
            )?;
            Ok("bypass".into())
        })
        .is_err()
    );
}
#[test]
fn t06_failed_admission_rolls_back_claims_inputs_events_and_cursor() {
    let f = fixture(vec![claude(1), codex(2)], 2);
    f.queue(1);
    f.queue(2);
    let before = f.snapshot();
    let queue = f.queue_status();
    assert!(
        f.execute(|tx| {
            tx.schedule(2, &[OpaqueId(900), OpaqueId(900)])?;
            Ok("duplicate attempt causes rollback".into())
        })
        .is_err()
    );
    assert_eq!(f.snapshot(), before);
    assert_eq!(f.queue_status(), queue);
    assert_eq!(f.scan(), vec![1, 2]);
}
#[test]
fn t06_cancel_reserved_releases_only_proven_unissued_claims() {
    let f = fixture(vec![claude(1), codex(2)], 2);
    f.queue(1);
    f.queue(2);
    assert_eq!(f.scan(), vec![1, 2]);
    f.issued(2);
    f.control(1, "CANCEL", i64::MAX);
    f.control(2, "CANCEL", i64::MAX);
    assert_eq!(f.snapshot().live_claims.len(), 3);
    let retired = f.attempt(1);
    assert!(
        f.execute(move |tx| {
            tx.advance_effect(
                &format!("scheduler-launch:{:032x}", retired.0),
                EffectState::IssuedUnknown,
            )?;
            Ok("revoked launch".into())
        })
        .is_err()
    );
    let a = f.attempt(2);
    assert!(
        f.engine
            .reader()
            .unwrap()
            .read(|v| v.approval_stop_requested(a))
            .unwrap()
    );
}

#[test]
fn t06_shipped_defaults_are_one_and_zero_is_a_durable_pause() {
    let dir = TempDir::new("default");
    let engine =
        Engine::start(EngineConfig::new(dir.db(), Arc::new(FakeClock::new(now())))).unwrap();
    let config = engine
        .reader()
        .unwrap()
        .read(|v| v.resource_config())
        .unwrap();
    assert_eq!(config.revision, 1);
    assert_eq!(config.default_repository, 1);
    assert_eq!(config.capacities["global"], 1);
    assert_eq!(config.capacities["provider:claude"], 1);
    assert_eq!(config.capacities["provider:codex"], 1);
    let f = fixture(vec![claude(1)], 1);
    f.queue(1);
    f.config(0, 1, 1, 4);
    assert!(f.scan().is_empty());
    f.config(1, 1, 1, 4);
    assert_eq!(f.scan(), vec![1]);
}
#[test]
fn t06_exclusive_waiter_blocks_even_an_older_queued_shared_job() {
    let mut exclusive = codex(1);
    exclusive.mode = ResourceMode::Exclusive;
    let f = fixture(vec![claude(1), claude(1), exclusive], 3);
    f.queue(1);
    assert_eq!(f.scan(), vec![1]);
    f.queue(2);
    f.queue(3);
    assert!(f.scan().is_empty());
    f.retire(1);
    assert_eq!(f.scan(), vec![3]);
    f.retire(3);
    assert_eq!(f.scan(), vec![2]);
}
#[test]
fn t06_hold_time_and_uncertain_clock_jump_do_not_fabricate_priority_age() {
    let f = fixture(vec![claude(1)], 1);
    f.queue(1);
    f.control(1, "HOLD", 0);
    f.advance(1_800_000_000_000);
    f.control(1, "RUN", 0);
    f.execute(|tx| {
        tx.set_admission(Admission::Closed)?;
        Ok("hold gate".into())
    })
    .unwrap();
    assert!(f.scan().is_empty());
    assert_eq!(f.queue_status()[0].eligible_nanos, 0);
    let mut jumped = f.clock.now();
    jumped.epoch = jojo_domain::identity::ClockEpochId(OpaqueId(55));
    jumped.monotonic_nanos += 9_000_000_000_000;
    jumped.unix_millis += 9_000_000;
    f.clock.set(jumped);
    assert!(f.scan().is_empty());
    assert_eq!(f.queue_status()[0].eligible_nanos, 0);
}
#[test]
fn t06_stale_control_cas_cannot_override_a_newer_hold() {
    let f = fixture(vec![claude(1)], 1);
    f.queue(1);
    f.control(1, "HOLD", 0);
    let before = f.snapshot();
    assert!(
        f.execute(|tx| {
            tx.control_scheduled_step(node(1), Revision(1), "RUN", 0)?;
            Ok("stale".into())
        })
        .is_err()
    );
    assert_eq!(f.snapshot(), before);
    assert!(f.scan().is_empty());
}
#[test]
fn t06_bad_source_is_rejected_before_it_can_block_unrelated_work() {
    let f = fixture(vec![claude(20), codex(2)], 2);
    assert!(
        f.execute(|tx| {
            tx.queue_step(
                PRD,
                ADR,
                PLAN,
                Revision(1),
                node(1),
                &target(vec![source('b')]),
            )?;
            Ok("wrong commit".into())
        })
        .is_err()
    );
    assert!(f.queue_status().is_empty());
    f.queue(2);
    assert_eq!(f.scan(), vec![2]);
}
#[test]
fn t06_barrier_skips_and_claims_persist_through_unclean_restart() {
    let mut large = claude(1);
    large.extra = vec![need(4)];
    let mut small = codex(2);
    small.extra = vec![need(1)];
    let mut f = fixture(
        vec![
            small.clone(),
            large,
            small.clone(),
            small.clone(),
            small,
            codex(3),
        ],
        10,
    );
    f.queue(1);
    f.scan();
    f.queue(2);
    for i in 3..=5 {
        f.queue(i);
        assert_eq!(f.scan(), vec![i]);
    }
    let before = f.queue_status();
    f.restart();
    assert_eq!(f.queue_status(), before);
    assert_eq!(before[1].skips, 3);
    assert_eq!(before[1].barriers, vec!["named:pool"]);
    f.queue(6);
    f.open();
    assert_eq!(f.scan(), vec![6]);
    assert_eq!(f.queue_status()[1].barriers, vec!["named:pool"]);
}
#[test]
fn t06_legacy_start_cannot_bypass_future_dispatch_validation() {
    let f = fixture(vec![claude(1)], 1);
    f.queue(1);
    f.scan();
    let a = f.attempt(1);
    let before = f.snapshot();
    assert!(
        f.execute(move |tx| {
            tx.start_approved_attempt(a)?;
            Ok("bypass dispatcher".into())
        })
        .is_err()
    );
    assert_eq!(f.snapshot(), before);
}

#[test]
fn t06_conflicting_barriers_service_the_oldest_without_deadlock() {
    let mut small = codex(3);
    small.extra = vec![need(1)];
    let mut older = claude(2);
    older.extra = vec![need(4)];
    let mut younger = claude(1);
    younger.extra = vec![need(4)];
    let f = fixture(
        vec![
            small.clone(),
            older,
            younger,
            small.clone(),
            small.clone(),
            small,
        ],
        10,
    );
    f.queue(1);
    f.scan();
    f.queue(2);
    f.queue(3);
    for i in 4..=6 {
        f.queue(i);
        assert_eq!(f.scan(), vec![i]);
    }
    assert_eq!(f.queue_status()[1].skips, 3);
    assert_eq!(f.queue_status()[2].skips, 3);
    for i in [1, 4, 5, 6] {
        f.retire(i);
    }
    assert_eq!(f.scan(), vec![2]);
    f.retire(2);
    assert_eq!(f.scan(), vec![3]);
}

#[test]
fn t06_provider_cooldown_is_local_durable_and_cancellation_still_wins() {
    let mut f = fixture(vec![claude(1), codex(2), claude(3)], 3);
    for i in 1..=3 {
        f.queue(i);
    }
    let until = f.clock.now().unix_millis + 100;
    f.execute(move |tx| {
        tx.set_provider_cooldown(ProviderKind::ClaudeCode, 0, until)?;
        Ok("Claude cooldown".into())
    })
    .unwrap();
    assert_eq!(f.scan(), vec![2]);
    f.control(1, "CANCEL", i64::MAX);
    f.retire(2);
    f.restart();
    f.open();
    assert!(f.scan().is_empty());
    f.advance(100_000_000);
    assert_eq!(f.scan(), vec![3]);
}

#[test]
fn t06_schema_four_upgrade_preserves_claims_and_command_receipts() {
    let dir = TempDir::new("schema4");
    let clock = Arc::new(FakeClock::new(now()));
    let mut cfg = EngineConfig::new(dir.db(), clock.clone());
    cfg.migrations = MigrationPlan::new(MigrationPlan::production().steps()[..4].to_vec()).unwrap();
    let e = Engine::start(cfg).unwrap();
    let holder = AggregateKey::new("legacy", OpaqueId(700));
    let req = request(&e, 1, "legacy reservation", "global");
    e.execute(req.clone(), move |tx| {
        tx.create_aggregate(&holder, "RESERVED")?;
        tx.acquire_claim("global", ClaimMode::Shared, &holder)?;
        Ok("legacy receipt".into())
    })
    .unwrap();
    drop(e);
    let e = Engine::start(EngineConfig::new(dir.db(), clock)).unwrap();
    let report = &e.startup_report().migration;
    assert_eq!((report.from_version, report.to_version), (4, 15));
    assert!(report.backup.as_ref().unwrap().exists());
    let s = e.reader().unwrap().snapshot().unwrap();
    assert_eq!(s.live_claims.len(), 1);
    assert_eq!(s.live_claims[0].state, ClaimState::Quarantined);
    assert_eq!(s.session.admission, Admission::Closed);
    assert_eq!(
        e.reader()
            .unwrap()
            .receipt(req.id)
            .unwrap()
            .unwrap()
            .response,
        "legacy receipt"
    );
}

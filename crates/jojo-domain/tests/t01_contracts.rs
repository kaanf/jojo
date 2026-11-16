use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Barrier;

use jojo_domain::attempt::{AttemptBindings, AttemptDescriptor};
use jojo_domain::identity::{
    ClockEpochId, FeatureId, IdGenerationError, IdGenerator, OpaqueId, PlanId, RepositoryId,
    Revision, StepId, WorktreeId,
};
use jojo_domain::paths::{LocalRoots, RootError};
use jojo_domain::policy::ConcurrencyLimits;
use jojo_domain::provider::ProviderKind;
use jojo_domain::time::ClockReading;
use jojo_test_support::{FakeClock, SequenceIds};

fn reading(wall: i64, ticks: u64) -> ClockReading {
    ClockReading {
        unix_millis: wall,
        monotonic_nanos: ticks,
        epoch: ClockEpochId(OpaqueId(99)),
    }
}

fn bindings(index: u128) -> AttemptBindings {
    AttemptBindings {
        feature_id: FeatureId(OpaqueId(10)),
        plan_id: PlanId(OpaqueId(20)),
        plan_revision: Revision(1),
        step_id: StepId(OpaqueId(100 + index)),
        step_revision: Revision(1),
        repository_id: RepositoryId(OpaqueId(200 + index)),
        worktree_id: WorktreeId(OpaqueId(300 + index)),
    }
}

#[test]
fn injected_readings_and_ids_are_reproducible_without_ambient_time() {
    let replay = || {
        let clock = FakeClock::new(reading(1000, 10));
        let ids = SequenceIds::new(7, 1);
        let a = AttemptDescriptor::capture(bindings(0), ProviderKind::ClaudeCode, &clock, &ids)
            .unwrap();
        // A wall-clock correction must not get replaced with an ambient clock read.
        clock.set(reading(900, 20));
        let b = AttemptDescriptor::capture(bindings(1), ProviderKind::Codex, &clock, &ids).unwrap();
        (a, b)
    };
    let first = replay();
    assert_eq!(first, replay());
    assert_ne!(first.0.id(), first.1.id());
    assert_eq!(first.0.created_at(), reading(1000, 10));
    assert_eq!(first.1.created_at(), reading(900, 20));
}

#[test]
fn independent_attempts_can_be_described_concurrently_for_both_providers() {
    let clock = FakeClock::new(reading(1000, 10));
    let ids = SequenceIds::new(8, 1);
    let gate = Barrier::new(8);
    let attempts = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let clock = &clock;
                let ids = &ids;
                let gate = &gate;
                scope.spawn(move || {
                    gate.wait();
                    let provider = if i % 2 == 0 {
                        ProviderKind::ClaudeCode
                    } else {
                        ProviderKind::Codex
                    };
                    AttemptDescriptor::capture(bindings(i), provider, clock, ids).unwrap()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        attempts
            .iter()
            .map(|a| a.id())
            .collect::<BTreeSet<_>>()
            .len(),
        8
    );
    assert_eq!(
        attempts
            .iter()
            .map(|a| a.bindings().worktree_id)
            .collect::<BTreeSet<_>>()
            .len(),
        8
    );
    assert_eq!(
        attempts
            .iter()
            .filter(|a| a.provider() == ProviderKind::ClaudeCode)
            .count(),
        4
    );
    assert_eq!(
        attempts
            .iter()
            .filter(|a| a.provider() == ProviderKind::Codex)
            .count(),
        4
    );
    assert!(attempts.iter().all(|a| a.created_at() == reading(1000, 10)));
}

#[test]
fn fixture_id_exhaustion_is_explicit_instead_of_wrapping_into_a_collision() {
    let ids = SequenceIds::new(9, u64::MAX - 1);
    assert!(ids.next_id().is_ok());
    assert_eq!(ids.next_id(), Err(IdGenerationError::Exhausted));
    assert_eq!(ids.next_id(), Err(IdGenerationError::Exhausted));
}

#[test]
fn capacity_one_is_a_default_and_can_be_raised_or_paused_as_policy_data() {
    let mut limits = ConcurrencyLimits::default();
    assert_eq!(limits.global_agent_slots, 1);
    limits.global_agent_slots = 8;
    limits.default_repository_slots = 2;
    limits.provider_slots.insert(ProviderKind::ClaudeCode, 4);
    limits.provider_slots.insert(ProviderKind::Codex, 4);
    assert_eq!(
        limits.provider_slots.values().sum::<u32>(),
        limits.global_agent_slots
    );
    limits.provider_slots.insert(ProviderKind::Codex, 0);
    assert_eq!(limits.provider_slots[&ProviderKind::ClaudeCode], 4);
}

#[test]
fn roots_are_explicit_separate_and_support_a_credential_free_test_home() {
    let roots = LocalRoots::for_macos_home(Path::new("/isolated-test-home")).unwrap();
    assert_eq!(
        roots.data(),
        Path::new("/isolated-test-home/Library/Application Support/Jojo")
    );
    assert_eq!(
        roots.worktrees(),
        Path::new("/isolated-test-home/Library/Application Support/JojoWorktrees")
    );
    let custom = LocalRoots::new(
        PathBuf::from("/fixture/data"),
        PathBuf::from("/fixture/worktrees"),
    )
    .unwrap();
    assert_eq!(custom.data(), Path::new("/fixture/data"));
    assert_eq!(
        LocalRoots::new("relative".into(), "/fixture/worktrees".into()),
        Err(RootError::NotAbsolute)
    );
    assert_eq!(
        LocalRoots::new("/".into(), "/fixture/worktrees".into()),
        Err(RootError::FilesystemRoot)
    );
    assert_eq!(
        LocalRoots::new("/fixture/data/../data".into(), "/fixture/worktrees".into()),
        Err(RootError::Traversal)
    );
    assert_eq!(
        LocalRoots::new("/fixture/data".into(), "/fixture/data/worktrees".into()),
        Err(RootError::OverlappingRoots)
    );
    assert_eq!(
        LocalRoots::new("/fixture/data".into(), "/fixture/data".into()),
        Err(RootError::OverlappingRoots)
    );
}

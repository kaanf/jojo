use crate::identity::{
    AttemptId, FeatureId, IdGenerationError, IdGenerator, PlanId, RepositoryId, Revision, StepId,
    WorktreeId,
};
use crate::provider::ProviderKind;
use crate::time::{Clock, ClockReading};

/// Explicit bindings for one future invocation. Ownership must be validated by later tasks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttemptBindings {
    pub feature_id: FeatureId,
    pub plan_id: PlanId,
    pub plan_revision: Revision,
    pub step_id: StepId,
    pub step_revision: Revision,
    pub repository_id: RepositoryId,
    pub worktree_id: WorktreeId,
}

/// Immutable metadata, not an admitted/running attempt or a persisted execution result.
/// Multiple descriptors coexist without a singleton current-agent/current-task field.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttemptDescriptor {
    id: AttemptId,
    bindings: AttemptBindings,
    provider: ProviderKind,
    created_at: ClockReading,
}

impl AttemptDescriptor {
    pub fn capture(
        bindings: AttemptBindings,
        provider: ProviderKind,
        clock: &dyn Clock,
        ids: &dyn IdGenerator,
    ) -> Result<Self, IdGenerationError> {
        Ok(Self {
            id: AttemptId(ids.next_id()?),
            bindings,
            provider,
            created_at: clock.now(),
        })
    }

    pub fn id(&self) -> AttemptId {
        self.id
    }

    pub fn bindings(&self) -> &AttemptBindings {
        &self.bindings
    }

    pub fn provider(&self) -> ProviderKind {
        self.provider
    }

    pub fn created_at(&self) -> ClockReading {
        self.created_at
    }
}

/// Normative state vocabulary only. No transitions, admission, settlement or recovery engine.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttemptPhase {
    Reserved,
    Starting,
    Running,
    AwaitingInput,
    StopRequested,
    Settling,
    RecoveryRequired,
    Finished,
}

/// Outcome vocabulary is separate from lifecycle; this enum does not certify an outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttemptOutcome {
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
    NotStarted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlIntent {
    Run,
    Hold,
    Cancel,
}

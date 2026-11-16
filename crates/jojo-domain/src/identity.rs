//! Opaque, typed identities. Production generation and durability are later tasks.

/// Caller-supplied identity bits; intentionally no timestamp, PID or provider-session meaning.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OpaqueId(pub u128);

/// An injected generator must provide unique identities across its intended scope.
/// No process-global generator or production randomness is installed by this foundation.
pub trait IdGenerator: Send + Sync {
    fn next_id(&self) -> Result<OpaqueId, IdGenerationError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdGenerationError {
    Exhausted,
}

macro_rules! typed_ids {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
            pub struct $name(pub OpaqueId);
        )+
    };
}

typed_ids!(
    AttemptId,
    ArtifactId,
    QuestionId,
    FeatureId,
    PlanId,
    StepId,
    RepositoryId,
    WorktreeId,
    ClockEpochId
);

/// Revision vocabulary only: no CAS, increment, storage or approval semantics yet.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Revision(pub u64);

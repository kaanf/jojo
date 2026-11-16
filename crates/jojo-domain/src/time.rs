use crate::identity::ClockEpochId;

/// Clock samples retain a scope: monotonic ticks from different epochs are not comparable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockReading {
    pub unix_millis: i64,
    pub monotonic_nanos: u64,
    pub epoch: ClockEpochId,
}

/// No ambient time reads. Implementations define their epoch and synchronization behavior.
pub trait Clock: Send + Sync {
    fn now(&self) -> ClockReading;
}

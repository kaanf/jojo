use std::collections::BTreeMap;
use std::time::Duration;

use crate::provider::ProviderKind;

/// Configurable policy data, not a semaphore or scheduler. Zero means paused for that class.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConcurrencyLimits {
    pub global_agent_slots: u32,
    pub provider_slots: BTreeMap<ProviderKind, u32>,
    pub default_repository_slots: u32,
}

impl Default for ConcurrencyLimits {
    fn default() -> Self {
        Self {
            global_agent_slots: 1,
            provider_slots: BTreeMap::from([
                (ProviderKind::ClaudeCode, 1),
                (ProviderKind::Codex, 1),
            ]),
            default_repository_slots: 1,
        }
    }
}

/// Accepted escalation defaults, represented as data only; no signal is sent by this crate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StopBudgets {
    pub graceful_interrupt: Duration,
    pub terminate: Duration,
    pub kill_verification: Duration,
}

impl Default for StopBudgets {
    fn default() -> Self {
        Self {
            graceful_interrupt: Duration::from_secs(10),
            terminate: Duration::from_secs(20),
            kill_verification: Duration::from_secs(5),
        }
    }
}

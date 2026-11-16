//! Reviewed admission envelopes and deterministic resource conflict policy.
use crate::dependency::{ExecutionPlan, NodeRef};
use crate::identity::RepositoryId;
use crate::provider::ProviderKind;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceMode {
    Shared,
    Exclusive,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceNeed {
    pub key: String,
    pub units: u64,
    pub mode: ResourceMode,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepResources {
    pub step: NodeRef,
    pub repository: RepositoryId,
    pub provider: ProviderKind,
    pub priority: u64,
    /// Additional named resources; mandatory invocation/repository resources are derived.
    pub additional: Vec<ResourceNeed>,
    pub repository_mode: ResourceMode,
}
pub fn repository_key(repo: RepositoryId) -> String {
    format!("repository:{:032x}", repo.0.0)
}
pub fn provider_key(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::ClaudeCode => "provider:claude",
        ProviderKind::Codex => "provider:codex",
    }
}
impl StepResources {
    pub fn needs(&self) -> Vec<ResourceNeed> {
        let mut result = vec![
            ResourceNeed {
                key: "global".into(),
                units: 1,
                mode: ResourceMode::Shared,
            },
            ResourceNeed {
                key: provider_key(self.provider).into(),
                units: 1,
                mode: ResourceMode::Shared,
            },
            ResourceNeed {
                key: repository_key(self.repository),
                units: 1,
                mode: self.repository_mode,
            },
        ];
        result.extend(self.additional.clone());
        result.sort_by(|a, b| a.key.cmp(&b.key));
        result
    }
    pub fn validate(&self, plan: &ExecutionPlan) -> Result<(), String> {
        if self.priority > 3 || !plan.execution.nodes.contains(&self.step) {
            return Err("invalid resource step/priority".into());
        }
        let input = plan
            .inputs
            .iter()
            .find(|i| i.step == self.step)
            .ok_or("missing step input")?;
        let repos: BTreeSet<_> = input
            .base_sources
            .iter()
            .chain(input.selected_outputs.iter().flat_map(|o| o.sources.iter()))
            .map(|s| s.repository)
            .collect();
        // Multi-repository writers need a separate reviewed execution design.
        if repos != BTreeSet::from([self.repository]) {
            return Err("resource repository must equal reviewed source repository".into());
        }
        let mut seen = BTreeSet::new();
        for n in self.needs() {
            if n.units == 0
                || n.units > u32::MAX.into()
                || n.key.len() > 200
                || n.key.trim().is_empty()
                || n.key.contains('\0')
                || !seen.insert(n.key)
            {
                return Err("invalid or duplicate resource need".into());
            }
        }
        if self.additional.iter().any(|n| !n.key.starts_with("named:")) {
            return Err("additional resource requires named: namespace".into());
        }
        Ok(())
    }
}
pub fn conflicts(a: &[ResourceNeed], b: &[ResourceNeed]) -> bool {
    a.iter().any(|x| b.iter().any(|y| x.key == y.key))
}
/// Only same-clock monotonic elapsed time ages priority; reboot/clock jumps retain order.
pub fn effective_priority(base: u64, waiting_nanos: u64) -> u64 {
    (base + waiting_nanos / 600_000_000_000).min(3)
}

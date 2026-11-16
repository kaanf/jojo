//! Deterministic dependency contracts. No I/O, scheduling or provider authority.
use crate::identity::{ArtifactId, OpaqueId, RepositoryId, Revision};
use crate::time::ClockReading;
use std::collections::{BTreeMap, BTreeSet};

pub const DEPENDENCY_SCHEMA: u64 = 1;
pub const MAX_NODES: usize = 512;
pub const MAX_EDGES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct NodeRef {
    pub id: OpaqueId,
    pub revision: Revision,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphKind {
    Artifact,
    Execution,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Edge {
    pub id: OpaqueId,
    pub prerequisite: NodeRef,
    pub dependent: NodeRef,
    pub predicate: Predicate,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Graph {
    pub kind: GraphKind,
    pub nodes: Vec<NodeRef>,
    pub edges: Vec<Edge>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphError {
    Limit,
    InvalidRevision(NodeRef),
    DuplicateNode(OpaqueId),
    DuplicateEdge(OpaqueId),
    MissingReference(NodeRef),
    SelfEdge(NodeRef),
    Cycle(Vec<NodeRef>),
    InvalidPredicate(String),
}
impl Graph {
    /// Stable Kahn order, with an iterative DFS only when a cycle needs explaining.
    pub fn order(&self) -> Result<Vec<NodeRef>, GraphError> {
        if self.nodes.len() > MAX_NODES || self.edges.len() > MAX_EDGES {
            return Err(GraphError::Limit);
        }
        let mut nodes = BTreeMap::new();
        for n in &self.nodes {
            if n.revision.0 == 0 {
                return Err(GraphError::InvalidRevision(*n));
            }
            if nodes.insert(n.id, *n).is_some() {
                return Err(GraphError::DuplicateNode(n.id));
            }
        }
        let mut ids = BTreeSet::new();
        let mut signatures = BTreeSet::new();
        let mut incoming: BTreeMap<_, usize> = self.nodes.iter().map(|n| (*n, 0)).collect();
        let mut next: BTreeMap<NodeRef, Vec<NodeRef>> = BTreeMap::new();
        let mut edges: Vec<_> = self.edges.iter().collect();
        edges.sort_by_key(|e| e.id);
        for e in edges {
            for n in [e.prerequisite, e.dependent] {
                if nodes.get(&n.id) != Some(&n) {
                    return Err(GraphError::MissingReference(n));
                }
            }
            if e.prerequisite == e.dependent {
                return Err(GraphError::SelfEdge(e.dependent));
            }
            e.predicate
                .validate()
                .map_err(GraphError::InvalidPredicate)?;
            let signature = (e.prerequisite, e.dependent, e.predicate.canonical());
            if !ids.insert(e.id) || !signatures.insert(signature) {
                return Err(GraphError::DuplicateEdge(e.id));
            }
            *incoming.get_mut(&e.dependent).expect("known endpoint") += 1;
            next.entry(e.prerequisite).or_default().push(e.dependent);
        }
        for children in next.values_mut() {
            children.sort();
        }
        let mut ready: BTreeSet<_> = incoming
            .iter()
            .filter(|(_, n)| **n == 0)
            .map(|(n, _)| *n)
            .collect();
        let mut ordered = Vec::new();
        while let Some(n) = ready.pop_first() {
            ordered.push(n);
            for child in next.get(&n).into_iter().flatten() {
                let degree = incoming.get_mut(child).expect("known endpoint");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(*child);
                }
            }
        }
        if ordered.len() != self.nodes.len() {
            let mut done = BTreeSet::new();
            for root in incoming.keys() {
                let mut path = Vec::new();
                let mut stack = vec![(*root, false)];
                while let Some((node, exit)) = stack.pop() {
                    if exit {
                        path.pop();
                        done.insert(node);
                        continue;
                    }
                    if let Some(at) = path.iter().position(|n| *n == node) {
                        let mut cycle = path[at..].to_vec();
                        cycle.push(node);
                        return Err(GraphError::Cycle(cycle));
                    }
                    if done.contains(&node) {
                        continue;
                    }
                    path.push(node);
                    stack.push((node, true));
                    for child in next.get(&node).into_iter().flatten().rev() {
                        stack.push((*child, false));
                    }
                }
            }
        }
        Ok(ordered)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum SourceRevision {
    Commit(String),
    Snapshot { base_commit: String, digest: String },
}
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Source {
    pub repository: RepositoryId,
    pub revision: SourceRevision,
}
pub fn is_digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}
fn is_commit(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64)
        && s.bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}
pub fn validate_sources(sources: &[Source]) -> Result<(), String> {
    if sources.is_empty() {
        return Err("source vector is missing".into());
    }
    let mut repos = BTreeSet::new();
    for s in sources {
        if !repos.insert(s.repository) {
            return Err("duplicate repository in source vector".into());
        }
        let valid = match &s.revision {
            SourceRevision::Commit(c) => is_commit(c),
            SourceRevision::Snapshot {
                base_commit,
                digest,
            } => is_commit(base_commit) && is_digest(digest),
        };
        if !valid {
            return Err("invalid commit or snapshot identity".into());
        }
    }
    Ok(())
}
/// Approved/observed source materialization target; no implicit environment switch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceVector {
    pub sources: Vec<Source>,
    pub environment: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalBinding {
    pub artifact: ArtifactId,
    pub revision: Revision,
    pub canonical_hash: String,
    pub bundle_hash: String,
    pub decision: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceKind {
    OutputAvailable,
    CodeVerified,
    PrMerged,
    BuildAvailable,
    DeployedHealthy,
    ContractCompatible,
    ValidationPassed,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputSelection {
    pub output_id: String,
    pub producer: NodeRef,
    pub digest: String,
    pub sources: Vec<Source>,
}
/// Exact selectors, never a branch name, URL or "latest". Optional fields become
/// mandatory for the corresponding predicate category in `Predicate::validate`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceBinding {
    pub sources: Vec<Source>,
    pub environment: Option<String>,
    pub artifact_digest: Option<String>,
    pub contract: Option<String>,
    pub validation_plan: Option<String>,
    pub test_inventory: Vec<String>,
    pub output: Option<OutputSelection>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceRequirement {
    pub kind: EvidenceKind,
    pub evidence_id: String,
    pub observation_revision: Revision,
    pub binding: EvidenceBinding,
    pub max_age_ms: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Predicate {
    ArtifactApproved {
        schema: u64,
        binding: ApprovalBinding,
    },
    Evidence {
        schema: u64,
        requirement: Box<EvidenceRequirement>,
    },
}
impl Predicate {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::ArtifactApproved { schema, binding } => {
                if *schema != DEPENDENCY_SCHEMA
                    || binding.revision.0 == 0
                    || binding.decision == 0
                    || !is_digest(&binding.canonical_hash)
                    || !is_digest(&binding.bundle_hash)
                {
                    return Err("invalid artifact approval selector/schema".into());
                }
            }
            Self::Evidence {
                schema,
                requirement: r,
            } => {
                if *schema != DEPENDENCY_SCHEMA
                    || r.evidence_id.trim().is_empty()
                    || r.observation_revision.0 == 0
                    || r.max_age_ms == 0
                {
                    return Err("invalid evidence selector/schema/freshness".into());
                }
                validate_sources(&r.binding.sources)?;
                let b = &r.binding;
                if b.environment.as_ref().is_some_and(|s| s.trim().is_empty())
                    || b.artifact_digest.as_ref().is_some_and(|s| !is_digest(s))
                {
                    return Err("invalid environment/artifact identity".into());
                }
                match r.kind {
                    EvidenceKind::OutputAvailable => {
                        let o = b.output.as_ref().ok_or("selected output required")?;
                        if o.output_id.is_empty()
                            || o.producer.revision.0 == 0
                            || !is_digest(&o.digest)
                            || !same_sources(&o.sources, &b.sources)
                        {
                            return Err("invalid selected output".into());
                        }
                    }
                    EvidenceKind::CodeVerified | EvidenceKind::ValidationPassed => {
                        if b.validation_plan.as_ref().is_none_or(|s| !is_digest(s))
                            || b.test_inventory.is_empty()
                            || b.test_inventory.iter().any(|s| s.trim().is_empty())
                            || b.test_inventory.iter().collect::<BTreeSet<_>>().len()
                                != b.test_inventory.len()
                        {
                            return Err(
                                "approved validation plan and complete test inventory required"
                                    .into(),
                            );
                        }
                    }
                    EvidenceKind::BuildAvailable => {
                        if b.artifact_digest.is_none() {
                            return Err("build artifact required".into());
                        }
                    }
                    EvidenceKind::DeployedHealthy => {
                        if b.environment.is_none() || b.artifact_digest.is_none() {
                            return Err("deployment environment/artifact required".into());
                        }
                    }
                    EvidenceKind::ContractCompatible => {
                        if b.contract.as_ref().is_none_or(|s| !is_digest(s)) || b.sources.len() < 2
                        {
                            return Err(
                                "both contract sides and approved contract digest required".into(),
                            );
                        }
                    }
                    EvidenceKind::PrMerged => {}
                }
            }
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceStatus {
    Verified,
    Failed,
    Unknown,
    LookupFailed,
    PullRequestOpen,
    Invalidated,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Observation {
    pub schema: u64,
    pub id: String,
    pub revision: Revision,
    pub kind: EvidenceKind,
    pub binding: EvidenceBinding,
    pub status: EvidenceStatus,
    pub observer: String,
    pub observed: ClockReading,
    pub expires_at_unix_ms: i64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Truth {
    Satisfied,
    Unsatisfied,
    Unknown,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Evaluation {
    pub truth: Truth,
    pub reason: String,
    pub evidence_id: Option<String>,
    pub observation_revision: Option<Revision>,
    pub sources: Vec<Source>,
    pub observed_sources: Vec<Source>,
    pub artifact: Option<ApprovalBinding>,
    pub expires_at_unix_ms: Option<i64>,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Facts {
    pub observations: BTreeMap<String, Observation>,
    pub approvals: BTreeMap<ArtifactId, ApprovalBinding>,
}
fn same_sources(a: &[Source], b: &[Source]) -> bool {
    let mut a = a.to_vec();
    let mut b = b.to_vec();
    a.sort();
    b.sort();
    a == b
}
fn same_binding(a: &EvidenceBinding, b: &EvidenceBinding) -> bool {
    let mut a = a.clone();
    let mut b = b.clone();
    a.sources.sort();
    b.sources.sort();
    a.test_inventory.sort();
    b.test_inventory.sort();
    if let Some(o) = a.output.as_mut() {
        o.sources.sort()
    };
    if let Some(o) = b.output.as_mut() {
        o.sources.sort()
    };
    a == b
}
/// The sole predicate implementation. Every integration entry point supplies the
/// same immutable plan, facts and injected clock sample to this function.
pub fn evaluate(predicate: &Predicate, facts: &Facts, now: ClockReading) -> Evaluation {
    let mut out = Evaluation {
        truth: Truth::Unknown,
        reason: String::new(),
        evidence_id: None,
        observation_revision: None,
        sources: vec![],
        observed_sources: vec![],
        artifact: None,
        expires_at_unix_ms: None,
    };
    if let Err(reason) = predicate.validate() {
        out.reason = reason;
        return out;
    }
    match predicate {
        Predicate::ArtifactApproved { binding, .. } => {
            out.artifact = Some(binding.clone());
            out.evidence_id = Some(format!("human-decision:{}", binding.decision));
            out.observation_revision = Some(binding.revision);
            if facts.approvals.get(&binding.artifact) == Some(binding) {
                out.truth = Truth::Satisfied;
                out.reason = "exact applicable human decision".into()
            } else {
                out.truth = Truth::Unsatisfied;
                out.reason = "missing, revoked or stale artifact approval".into()
            }
        }
        Predicate::Evidence { requirement: r, .. } => {
            out.evidence_id = Some(r.evidence_id.clone());
            out.sources = r.binding.sources.clone();
            let Some(o) = facts.observations.get(&r.evidence_id) else {
                out.reason = "missing observation".into();
                return out;
            };
            out.observation_revision = Some(o.revision);
            out.observed_sources = o.binding.sources.clone();
            let max_age = i64::try_from(r.max_age_ms).unwrap_or(i64::MAX);
            let effective_expiry = o
                .expires_at_unix_ms
                .min(o.observed.unix_millis.saturating_add(max_age));
            out.expires_at_unix_ms = Some(effective_expiry);
            if o.schema != DEPENDENCY_SCHEMA || o.observer.trim().is_empty() {
                out.reason = "unknown observation schema or observer".into();
                return out;
            }
            if o.revision != r.observation_revision
                || o.kind != r.kind
                || !same_binding(&o.binding, &r.binding)
            {
                out.truth = Truth::Unsatisfied;
                out.reason =
                    "observation revision/source/environment/contract selector mismatch".into();
                return out;
            }
            if now.epoch != o.observed.epoch
                || now.monotonic_nanos < o.observed.monotonic_nanos
                || now.unix_millis < o.observed.unix_millis
            {
                out.reason = "clock scope changed or observation is in the future".into();
                return out;
            }
            let age = now.unix_millis.abs_diff(o.observed.unix_millis);
            let monotonic_age = (now.monotonic_nanos - o.observed.monotonic_nanos) / 1_000_000;
            let monotonic_ttl = o.expires_at_unix_ms.saturating_sub(o.observed.unix_millis);
            if now.unix_millis >= effective_expiry
                || age >= r.max_age_ms
                || monotonic_age >= r.max_age_ms
                || monotonic_ttl <= 0
                || monotonic_age >= monotonic_ttl as u64
            {
                out.reason = "expired or stale observation; reobserve".into();
                return out;
            }
            match o.status {
                EvidenceStatus::Verified => {
                    out.truth = Truth::Satisfied;
                    out.reason = "exact independent evidence is current".into()
                }
                EvidenceStatus::Failed => {
                    out.truth = Truth::Unsatisfied;
                    out.reason = "independent check failed".into()
                }
                EvidenceStatus::Invalidated => {
                    out.truth = Truth::Unsatisfied;
                    out.reason = "evidence invalidated".into()
                }
                EvidenceStatus::PullRequestOpen => {
                    out.truth = Truth::Unsatisfied;
                    out.reason = "open PR is informational, never completion evidence".into()
                }
                EvidenceStatus::Unknown | EvidenceStatus::LookupFailed => {
                    out.reason = "observation unknown or lookup failed".into()
                }
            }
        }
    }
    out
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepInputs {
    pub step: NodeRef,
    pub environment: Option<String>,
    pub selected_outputs: Vec<OutputSelection>,
    pub base_sources: Vec<Source>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionPlan {
    pub schema: u64,
    pub artifacts: Graph,
    pub execution: Graph,
    pub approvals: Vec<ApprovalBinding>,
    pub inputs: Vec<StepInputs>,
    pub permission_digest: String,
    pub validation_digest: String,
    pub required_outputs: Vec<String>,
    pub resources: Vec<crate::scheduler::StepResources>,
}
impl ExecutionPlan {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.schema, 1 | 2)
            || self.artifacts.kind != GraphKind::Artifact
            || self.execution.kind != GraphKind::Execution
            || !is_digest(&self.permission_digest)
            || !is_digest(&self.validation_digest)
            || self.required_outputs.is_empty()
        {
            return Err("invalid plan schema, graphs or execution envelope".into());
        }
        if self.schema == 1 && !self.resources.is_empty() {
            return Err("schema 1 has no resource envelope".into());
        }
        if self.schema == 2 {
            let mut seen = BTreeSet::new();
            for resource in &self.resources {
                resource.validate(self)?;
                if !seen.insert(resource.step) {
                    return Err("duplicate resource envelope".into());
                }
            }
            if seen != self.execution.nodes.iter().copied().collect() {
                return Err("every step needs resources".into());
            }
        }
        self.artifacts
            .order()
            .map_err(|e| format!("artifact graph: {e:?}"))?;
        self.execution
            .order()
            .map_err(|e| format!("execution graph: {e:?}"))?;
        if self.required_outputs.iter().any(|s| s.trim().is_empty())
            || self.required_outputs.iter().collect::<BTreeSet<_>>().len()
                != self.required_outputs.len()
        {
            return Err("invalid required output inventory".into());
        }
        let mut seen = BTreeSet::new();
        for a in &self.approvals {
            Predicate::ArtifactApproved {
                schema: 1,
                binding: a.clone(),
            }
            .validate()?;
            if !seen.insert(a.artifact) {
                return Err("duplicate approval binding".into());
            }
        }
        let mut steps = BTreeSet::new();
        for input in &self.inputs {
            if !self.execution.nodes.contains(&input.step) || !steps.insert(input.step) {
                return Err("missing or duplicate step inputs".into());
            }
            if input
                .environment
                .as_ref()
                .is_some_and(|s| s.trim().is_empty())
                || (input.base_sources.is_empty() && input.selected_outputs.is_empty())
            {
                return Err("explicit step source/environment identity required".into());
            }
            if !input.base_sources.is_empty() {
                validate_sources(&input.base_sources)?;
            }
            let mut repositories: BTreeMap<_, _> = input
                .base_sources
                .iter()
                .map(|s| (s.repository, s.revision.clone()))
                .collect();
            let mut outputs = BTreeSet::new();
            for e in self
                .execution
                .edges
                .iter()
                .filter(|e| e.dependent == input.step)
            {
                if let Predicate::Evidence { requirement: r, .. } = &e.predicate
                    && r.kind == EvidenceKind::OutputAvailable
                    && r.binding
                        .output
                        .as_ref()
                        .is_none_or(|o| !input.selected_outputs.contains(o))
                {
                    return Err("output edge has no exact source selection".into());
                }
            }
            for o in &input.selected_outputs {
                validate_sources(&o.sources)?;
                if !is_digest(&o.digest) || !outputs.insert(o.output_id.clone()) {
                    return Err("invalid or duplicate selected output".into());
                }
                let linked=self.execution.edges.iter().any(|e|e.prerequisite==o.producer&&e.dependent==input.step&&matches!(&e.predicate,Predicate::Evidence{requirement:r,..} if r.kind==EvidenceKind::OutputAvailable && r.binding.output.as_ref()==Some(o)));
                if !linked {
                    return Err(
                        "selected output requires exact predecessor OUTPUT_AVAILABLE edge".into(),
                    );
                }
                for s in &o.sources {
                    if repositories
                        .insert(s.repository, s.revision.clone())
                        .is_some_and(|old| old != s.revision)
                    {
                        return Err(
                            "conflicting predecessor sources require explicit integration output"
                                .into(),
                        );
                    }
                }
            }
            for e in self
                .execution
                .edges
                .iter()
                .filter(|e| e.dependent == input.step)
            {
                if let Predicate::Evidence { requirement: r, .. } = &e.predicate {
                    for source in &r.binding.sources {
                        if repositories.get(&source.repository) != Some(&source.revision) {
                            return Err("dependent source vector does not include exact predecessor evidence source".into());
                        }
                    }
                }
            }
        }
        if steps.len() != self.execution.nodes.len() {
            return Err("every step, including isolated nodes, requires explicit inputs".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StepEvaluation {
    pub truth: Truth,
    pub reasons: Vec<Evaluation>,
}
pub fn evaluate_step(
    plan: &ExecutionPlan,
    step: NodeRef,
    facts: &Facts,
    now: ClockReading,
) -> StepEvaluation {
    let mut reasons = Vec::new();
    if let Err(reason) = plan.validate() {
        return StepEvaluation {
            truth: Truth::Unknown,
            reasons: vec![Evaluation {
                truth: Truth::Unknown,
                reason,
                evidence_id: None,
                observation_revision: None,
                sources: vec![],
                observed_sources: vec![],
                artifact: None,
                expires_at_unix_ms: None,
            }],
        };
    }
    if !plan.execution.nodes.contains(&step) {
        return StepEvaluation {
            truth: Truth::Unknown,
            reasons: vec![Evaluation {
                truth: Truth::Unknown,
                reason: "step revision missing".into(),
                evidence_id: None,
                observation_revision: None,
                sources: vec![],
                observed_sources: vec![],
                artifact: None,
                expires_at_unix_ms: None,
            }],
        };
    }
    for a in &plan.approvals {
        reasons.push(evaluate(
            &Predicate::ArtifactApproved {
                schema: 1,
                binding: a.clone(),
            },
            facts,
            now,
        ));
    }
    // Artifact graph constraints are global plan prerequisites; execution edges are
    // evaluated for this dependent and its prerequisites. No OR or inferred success.
    let mut affected = BTreeSet::from([step]);
    loop {
        let before = affected.len();
        for e in &plan.execution.edges {
            if affected.contains(&e.dependent) {
                affected.insert(e.prerequisite);
            }
        }
        if affected.len() == before {
            break;
        }
    }
    let mut edges: Vec<_> = plan
        .artifacts
        .edges
        .iter()
        .chain(
            plan.execution
                .edges
                .iter()
                .filter(|e| affected.contains(&e.dependent)),
        )
        .collect();
    edges.sort_by_key(|e| e.id);
    for e in edges {
        reasons.push(evaluate(&e.predicate, facts, now));
    }
    let truth = if reasons.iter().any(|r| r.truth == Truth::Unsatisfied) {
        Truth::Unsatisfied
    } else if reasons.iter().any(|r| r.truth == Truth::Unknown) {
        Truth::Unknown
    } else {
        Truth::Satisfied
    };
    StepEvaluation { truth, reasons }
}
/// Resolve a declarative source vector, never copy/reset files. Observed bases are
/// supplied by the future local worktree inspector. Branch tips are not substitutes.
pub fn materialize(
    plan: &ExecutionPlan,
    step: NodeRef,
    facts: &Facts,
    now: ClockReading,
    observed_bases: &SourceVector,
) -> Result<SourceVector, String> {
    if evaluate_step(plan, step, facts, now).truth != Truth::Satisfied {
        return Err("dependency or approval blocks materialization".into());
    }
    validate_sources(&observed_bases.sources)?;
    let input = plan
        .inputs
        .iter()
        .find(|i| i.step == step)
        .ok_or("step inputs missing")?;
    if observed_bases.environment != input.environment {
        return Err("materialization environment differs from approved step".into());
    }
    let mut selected: BTreeMap<_, _> = input
        .base_sources
        .iter()
        .map(|s| (s.repository, s.clone()))
        .collect();
    for o in &input.selected_outputs {
        for s in &o.sources {
            selected.insert(s.repository, s.clone());
        }
    }
    if selected.is_empty() {
        return Err("explicit source selection required; no branch-tip fallback".into());
    }
    let selected: Vec<_> = selected.into_values().collect();
    if !same_sources(&selected, &observed_bases.sources) {
        return Err("predecessor source and materialized base differ".into());
    }
    Ok(SourceVector {
        sources: selected,
        environment: input.environment.clone(),
    })
}

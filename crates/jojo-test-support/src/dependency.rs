//! Original synthetic offline dependency fixtures; no upstream material reused.
use jojo_domain::dependency::*;
use jojo_domain::identity::{ClockEpochId, OpaqueId, RepositoryId, Revision};
use jojo_domain::time::ClockReading;
pub fn node(id: u128) -> NodeRef {
    NodeRef {
        id: OpaqueId(id),
        revision: Revision(1),
    }
}
pub fn source(letter: char) -> Source {
    Source {
        repository: RepositoryId(OpaqueId(20)),
        revision: SourceRevision::Commit(letter.to_string().repeat(40)),
    }
}
pub fn now() -> ClockReading {
    ClockReading {
        unix_millis: 1_700_000_000_000,
        monotonic_nanos: 1_000_000_000,
        epoch: ClockEpochId(OpaqueId(1)),
    }
}
pub fn binding() -> EvidenceBinding {
    EvidenceBinding {
        sources: vec![source('a')],
        environment: Some("test-env".into()),
        artifact_digest: Some("b".repeat(64)),
        contract: None,
        validation_plan: Some("c".repeat(64)),
        test_inventory: vec!["independent-contract-test".into()],
        output: None,
    }
}
pub fn requirement(id: &str, kind: EvidenceKind) -> EvidenceRequirement {
    EvidenceRequirement {
        kind,
        evidence_id: id.into(),
        observation_revision: Revision(1),
        binding: binding(),
        max_age_ms: 1_000,
    }
}
pub fn predicate(id: &str, kind: EvidenceKind) -> Predicate {
    Predicate::Evidence {
        schema: 1,
        requirement: requirement(id, kind).into(),
    }
}
pub fn observation(id: &str, kind: EvidenceKind) -> Observation {
    Observation {
        schema: 1,
        id: id.into(),
        revision: Revision(1),
        kind,
        binding: binding(),
        status: EvidenceStatus::Verified,
        observer: "independent fake observer".into(),
        observed: now(),
        expires_at_unix_ms: now().unix_millis + 500,
    }
}
pub fn edge(id: u128, from: u128, to: u128, predicate: Predicate) -> Edge {
    Edge {
        id: OpaqueId(id),
        prerequisite: node(from),
        dependent: node(to),
        predicate,
    }
}
pub fn plan(nodes: &[u128], edges: Vec<Edge>) -> ExecutionPlan {
    ExecutionPlan {
        schema: 1,
        resources: vec![],
        artifacts: Graph {
            kind: GraphKind::Artifact,
            nodes: vec![],
            edges: vec![],
        },
        execution: Graph {
            kind: GraphKind::Execution,
            nodes: nodes.iter().map(|i| node(*i)).collect(),
            edges,
        },
        approvals: vec![],
        inputs: nodes
            .iter()
            .map(|i| StepInputs {
                step: node(*i),
                environment: Some("test-env".into()),
                selected_outputs: vec![],
                base_sources: vec![source('a')],
            })
            .collect(),
        permission_digest: "d".repeat(64),
        validation_digest: "e".repeat(64),
        required_outputs: vec!["verified-build".into()],
    }
}

pub fn target(sources: Vec<Source>) -> SourceVector {
    SourceVector {
        sources,
        environment: Some("test-env".into()),
    }
}

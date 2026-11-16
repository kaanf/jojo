//! Versioned, strict positional JSON codec. Canonical arrays avoid object-key order
//! ambiguity; integer identities are decimal/hex strings without lossy JSON numbers.
use crate::dependency::*;
use crate::identity::{ArtifactId, ClockEpochId, OpaqueId, RepositoryId, Revision};
use crate::json::{self, Json};
use crate::provider::ProviderKind;
use crate::scheduler::*;
use crate::time::ClockReading;

trait Wire: Sized {
    fn encode(&self) -> Json;
    fn decode(j: Json) -> Result<Self, String>;
}
fn array(j: Json) -> Result<std::vec::IntoIter<Json>, String> {
    if let Json::List(v) = j {
        Ok(v.into_iter())
    } else {
        Err("expected array".into())
    }
}
fn next<T: Wire>(i: &mut std::vec::IntoIter<Json>) -> Result<T, String> {
    T::decode(i.next().ok_or("missing field")?)
}
fn end(i: &mut std::vec::IntoIter<Json>) -> Result<(), String> {
    if i.next().is_some() {
        Err("unknown trailing fields".into())
    } else {
        Ok(())
    }
}
impl Wire for String {
    fn encode(&self) -> Json {
        Json::Text(self.clone())
    }
    fn decode(j: Json) -> Result<Self, String> {
        if let Json::Text(s) = j {
            Ok(s)
        } else {
            Err("expected string".into())
        }
    }
}
impl Wire for u64 {
    fn encode(&self) -> Json {
        Json::Text(self.to_string())
    }
    fn decode(j: Json) -> Result<Self, String> {
        String::decode(j)?
            .parse()
            .map_err(|_| "invalid integer".into())
    }
}
impl Wire for i64 {
    fn encode(&self) -> Json {
        Json::Text(self.to_string())
    }
    fn decode(j: Json) -> Result<Self, String> {
        String::decode(j)?
            .parse()
            .map_err(|_| "invalid signed integer".into())
    }
}
impl<T: Wire> Wire for Vec<T> {
    fn encode(&self) -> Json {
        Json::List(self.iter().map(Wire::encode).collect())
    }
    fn decode(j: Json) -> Result<Self, String> {
        array(j)?.map(T::decode).collect()
    }
}
impl<T: Wire> Wire for Box<T> {
    fn encode(&self) -> Json {
        (**self).encode()
    }
    fn decode(j: Json) -> Result<Self, String> {
        Ok(Box::new(T::decode(j)?))
    }
}
impl<T: Wire> Wire for Option<T> {
    fn encode(&self) -> Json {
        self.as_ref().map_or(Json::Null, Wire::encode)
    }
    fn decode(j: Json) -> Result<Self, String> {
        if j == Json::Null {
            Ok(None)
        } else {
            Ok(Some(T::decode(j)?))
        }
    }
}
impl Wire for OpaqueId {
    fn encode(&self) -> Json {
        Json::Text(format!("{:032x}", self.0))
    }
    fn decode(j: Json) -> Result<Self, String> {
        let s = String::decode(j)?;
        if s.len() != 32 {
            return Err("invalid opaque ID".into());
        }
        u128::from_str_radix(&s, 16)
            .map(Self)
            .map_err(|_| "invalid opaque ID".into())
    }
}
macro_rules! wrapped {
    ($t:ident,$inner:ty) => {
        impl Wire for $t {
            fn encode(&self) -> Json {
                self.0.encode()
            }
            fn decode(j: Json) -> Result<Self, String> {
                Ok(Self(<$inner>::decode(j)?))
            }
        }
    };
}
wrapped!(Revision, u64);
wrapped!(ArtifactId, OpaqueId);
wrapped!(RepositoryId, OpaqueId);
wrapped!(ClockEpochId, OpaqueId);
macro_rules! record {($t:ident {$($f:ident),+})=>{impl Wire for $t{fn encode(&self)->Json{Json::List(vec![$(self.$f.encode()),+])}fn decode(j:Json)->Result<Self,String>{let mut i=array(j)?;let value=Self{$($f:next(&mut i)?),+};end(&mut i)?;Ok(value)}}};}
macro_rules! choices {($t:ident {$($v:ident),+})=>{impl Wire for $t{fn encode(&self)->Json{Json::Text(match self{$(Self::$v=>stringify!($v)),+}.into())}fn decode(j:Json)->Result<Self,String>{match String::decode(j)?.as_str(){$(stringify!($v)=>Ok(Self::$v)),+, _=>Err(concat!("unknown ",stringify!($t)).into())}}}};}
choices!(GraphKind {
    Artifact,
    Execution
});
choices!(EvidenceKind {
    OutputAvailable,
    CodeVerified,
    PrMerged,
    BuildAvailable,
    DeployedHealthy,
    ContractCompatible,
    ValidationPassed
});
choices!(EvidenceStatus {
    Verified,
    Failed,
    Unknown,
    LookupFailed,
    PullRequestOpen,
    Invalidated
});
record!(NodeRef { id, revision });
record!(Graph { kind, nodes, edges });
record!(Edge {
    id,
    prerequisite,
    dependent,
    predicate
});
record!(Source {
    repository,
    revision
});
record!(ApprovalBinding {
    artifact,
    revision,
    canonical_hash,
    bundle_hash,
    decision
});
record!(OutputSelection {
    output_id,
    producer,
    digest,
    sources
});
record!(EvidenceBinding {
    sources,
    environment,
    artifact_digest,
    contract,
    validation_plan,
    test_inventory,
    output
});
record!(EvidenceRequirement {
    kind,
    evidence_id,
    observation_revision,
    binding,
    max_age_ms
});
record!(ClockReading {
    unix_millis,
    monotonic_nanos,
    epoch
});
record!(Observation {
    schema,
    id,
    revision,
    kind,
    binding,
    status,
    observer,
    observed,
    expires_at_unix_ms
});
record!(StepInputs {
    step,
    environment,
    selected_outputs,
    base_sources
});
choices!(ProviderKind { ClaudeCode, Codex });
choices!(ResourceMode { Shared, Exclusive });
record!(ResourceNeed { key, units, mode });
record!(StepResources {
    step,
    repository,
    provider,
    priority,
    additional,
    repository_mode
});
impl Wire for ExecutionPlan {
    fn encode(&self) -> Json {
        let mut v = vec![
            self.schema.encode(),
            self.artifacts.encode(),
            self.execution.encode(),
            self.approvals.encode(),
            self.inputs.encode(),
            self.permission_digest.encode(),
            self.validation_digest.encode(),
            self.required_outputs.encode(),
        ];
        if self.schema == 2 {
            v.push(self.resources.encode());
        }
        Json::List(v)
    }
    fn decode(j: Json) -> Result<Self, String> {
        let mut i = array(j)?;
        let schema = next(&mut i)?;
        let p = Self {
            schema,
            artifacts: next(&mut i)?,
            execution: next(&mut i)?,
            approvals: next(&mut i)?,
            inputs: next(&mut i)?,
            permission_digest: next(&mut i)?,
            validation_digest: next(&mut i)?,
            required_outputs: next(&mut i)?,
            resources: if schema == 2 { next(&mut i)? } else { vec![] },
        };
        end(&mut i)?;
        Ok(p)
    }
}
impl Wire for SourceRevision {
    fn encode(&self) -> Json {
        Json::List(match self {
            Self::Commit(c) => vec!["commit".to_string().encode(), c.encode()],
            Self::Snapshot {
                base_commit,
                digest,
            } => vec![
                "snapshot".to_string().encode(),
                base_commit.encode(),
                digest.encode(),
            ],
        })
    }
    fn decode(j: Json) -> Result<Self, String> {
        let mut i = array(j)?;
        let tag: String = next(&mut i)?;
        let value = match tag.as_str() {
            "commit" => Self::Commit(next(&mut i)?),
            "snapshot" => Self::Snapshot {
                base_commit: next(&mut i)?,
                digest: next(&mut i)?,
            },
            _ => return Err("unknown source kind".into()),
        };
        end(&mut i)?;
        Ok(value)
    }
}
impl Wire for Predicate {
    fn encode(&self) -> Json {
        Json::List(match self {
            Self::ArtifactApproved { schema, binding } => vec![
                "ArtifactApproved".to_string().encode(),
                schema.encode(),
                binding.encode(),
            ],
            Self::Evidence {
                schema,
                requirement,
            } => vec![
                "Evidence".to_string().encode(),
                schema.encode(),
                requirement.encode(),
            ],
        })
    }
    fn decode(j: Json) -> Result<Self, String> {
        let mut i = array(j)?;
        let tag: String = next(&mut i)?;
        let schema = next(&mut i)?;
        let value = match tag.as_str() {
            "ArtifactApproved" => Self::ArtifactApproved {
                schema,
                binding: next(&mut i)?,
            },
            "Evidence" => Self::Evidence {
                schema,
                requirement: next(&mut i)?,
            },
            _ => return Err("unknown predicate kind".into()),
        };
        end(&mut i)?;
        Ok(value)
    }
}
fn render(j: &Json, out: &mut String) {
    match j {
        Json::Text(s) => json::write_string(out, s),
        Json::Null => out.push_str("null"),
        Json::List(v) => {
            out.push('[');
            for (n, j) in v.iter().enumerate() {
                if n > 0 {
                    out.push(',')
                }
                render(j, out);
            }
            out.push(']')
        }
        _ => unreachable!("codec emits arrays, strings and null only"),
    }
}
fn canonical<T: Wire>(v: &T) -> String {
    let mut out = String::new();
    render(&v.encode(), &mut out);
    out
}
fn binding(b: &mut EvidenceBinding) {
    b.sources.sort();
    b.test_inventory.sort();
    if let Some(o) = &mut b.output {
        o.sources.sort()
    }
}
fn predicate(p: &mut Predicate) {
    if let Predicate::Evidence { requirement, .. } = p {
        binding(&mut requirement.binding)
    }
}
fn graph(g: &mut Graph) {
    g.nodes.sort();
    g.edges.sort_by_key(|e| e.id);
    for e in &mut g.edges {
        predicate(&mut e.predicate)
    }
}
impl Predicate {
    pub fn canonical(&self) -> String {
        let mut p = self.clone();
        predicate(&mut p);
        canonical(&p)
    }
}
impl ExecutionPlan {
    pub fn canonical(&self) -> String {
        let mut p = self.clone();
        graph(&mut p.artifacts);
        graph(&mut p.execution);
        p.approvals.sort_by_key(|a| a.artifact);
        p.inputs.sort_by_key(|i| i.step);
        for i in &mut p.inputs {
            i.base_sources.sort();
            i.selected_outputs
                .sort_by(|a, b| a.output_id.cmp(&b.output_id));
            for o in &mut i.selected_outputs {
                o.sources.sort()
            }
        }
        p.required_outputs.sort();
        canonical(&p)
    }
    pub fn import(bytes: &[u8]) -> Result<Self, String> {
        let p = Self::decode(json::parse(bytes).map_err(|e| format!("{e:?}"))?)?;
        p.validate()?;
        Ok(p)
    }
}
impl Observation {
    pub fn canonical(&self) -> String {
        let mut o = self.clone();
        binding(&mut o.binding);
        canonical(&o)
    }
    pub fn import(bytes: &[u8]) -> Result<Self, String> {
        Self::decode(json::parse(bytes).map_err(|e| format!("{e:?}"))?)
    }
}

record!(SourceVector {
    sources,
    environment
});
impl SourceVector {
    pub fn canonical(&self) -> String {
        let mut v = self.clone();
        v.sources.sort();
        canonical(&v)
    }
    pub fn import(bytes: &[u8]) -> Result<Self, String> {
        let v = Self::decode(json::parse(bytes).map_err(|e| format!("{e:?}"))?)?;
        validate_sources(&v.sources)?;
        if v.environment.as_ref().is_some_and(|s| s.trim().is_empty()) {
            return Err("empty environment".into());
        }
        Ok(v)
    }
}

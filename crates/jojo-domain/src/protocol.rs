//! R-01 typed protocol messages. One definition of every request type and of every
//! payload the helper reads. No I/O or mutation authority.
//!
//! The helper names a request once, through [`RequestKind`], and reads its payload once,
//! through the payload type of that request. Each `parse` reads the fields in the order
//! the helper always read them and reports the first fault with the same text, so a frame
//! is accepted or refused exactly as before these types existed.
//!
//! The table in [`SPECS`] is also what the macOS client's field lists are checked against
//! (`scripts/tests/test_protocol_fields.py`); keep one request per `spec(...)` entry.
use crate::identity::{ArtifactId, FeatureId, OpaqueId, QuestionId, RepositoryId, Revision};
use crate::ipc::{MAX_PAGE, get, id, string, text, uint};
use crate::json::Json;
use crate::planner::TurnKind;
use crate::prd::PrdDocument;
use crate::provider::ProviderKind;
use crate::recovery::{Assessment, RecoveryChoice};
use crate::validation::ValidationPlan;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestKind {
    Hello,
    PrepareClose,
    CancelClose,
    ConfirmClose,
    CancelAttempt,
    HumanDecision,
    RegisterRepository,
    ImportBundle,
    Start,
    RecoveryResolve,
    RecoveryStart,
    Validate,
    CandidateDecision,
    Snapshot,
    Events,
    EventDetail,
    Log,
    Receipt,
    CloseStatus,
    ReviewBundle,
    ShutdownReceipt,
    Recovery,
    RecoveryInspect,
    Candidate,
    CandidateChanges,
    CandidateDiff,
    ValidationReport,
    ApprovedPlan,
    ChatTurn,
    AnswerQuestion,
    CancelTurn,
    FeatureChat,
    ChatText,
    ChatIndex,
    SaveProject,
    Projects,
    RunPlan,
    ControlPlan,
    PlanRun,
    Executables,
    Repairs,
    Attempts,
    Documents,
    Features,
    Document,
    Decisions,
    PlanDetail,
    AttemptDetail,
    ExternalActions,
    RequestExternal,
    AuthorizeExternal,
    InspectExternal,
    ResolveExternal,
    RecoveryDetail,
    ProjectDetail,
    BriefTurn,
    Workspace,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Class {
    Handshake,
    /// Each commits under the client's command identity.
    Mutation,
    Query,
}

/// How strictly the payload's field set is checked before any field is read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shape {
    /// The listed fields are read; anything else in the payload is ignored.
    Open,
    /// Exactly as many fields as listed; their names are checked as they are read.
    Count,
    /// Exactly the listed fields: a payload carrying anything else is refused outright.
    Exact,
}

#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub kind: RequestKind,
    pub name: &'static str,
    pub class: Class,
    /// The protocol minor that introduced the request. An older frame naming it is
    /// answered as an unknown type.
    pub since: u64,
    /// Read from the control queue, which a full bulk queue cannot block.
    pub control: bool,
    pub shape: Shape,
    pub fields: &'static [&'static str],
}

const fn spec(
    kind: RequestKind,
    name: &'static str,
    class: Class,
    since: u64,
    control: bool,
    shape: Shape,
    fields: &'static [&'static str],
) -> Spec {
    Spec {
        kind,
        name,
        class,
        since,
        control,
        shape,
        fields,
    }
}

use Class::{Handshake, Mutation, Query};
use RequestKind as K;
use Shape::{Count, Exact, Open};

/// Every request, in `RequestKind` order. Within a class this is also the order in which
/// Hello lists the names.
#[rustfmt::skip]
pub const SPECS: [Spec; 57] = [
    spec(K::Hello, "Hello", Handshake, 0, true, Open, &[]),
    spec(K::PrepareClose, "PrepareClose", Mutation, 0, true, Open, &[]),
    spec(K::CancelClose, "CancelClose", Mutation, 0, true, Open, &["ticket"]),
    spec(K::ConfirmClose, "ConfirmClose", Mutation, 0, true, Open, &["ticket"]),
    spec(K::CancelAttempt, "CancelAttempt", Mutation, 0, true, Open, &["attempt"]),
    spec(K::HumanDecision, "HumanDecision", Mutation, 0, true, Count, &["subject", "hash", "content_hash", "displayed_hash", "context_hash", "upstream", "prior_decision", "action"]),
    spec(K::RegisterRepository, "RegisterRepository", Mutation, 1, false, Exact, &["path"]),
    spec(K::ImportBundle, "ImportBundle", Mutation, 1, false, Exact, &["feature", "prd_id", "adr_id", "plan_id", "validation_id", "prd", "adr", "step", "repository", "base", "provider", "validation"]),
    spec(K::Start, "Start", Mutation, 1, true, Exact, &["prd", "adr", "plan", "validation", "step"]),
    spec(K::RecoveryResolve, "RecoveryResolve", Mutation, 1, true, Exact, &["digest", "attempt", "choice"]),
    spec(K::RecoveryStart, "RecoveryStart", Mutation, 1, true, Exact, &["digest"]),
    spec(K::Validate, "Validate", Mutation, 1, true, Exact, &["attempt", "candidate"]),
    spec(K::CandidateDecision, "CandidateDecision", Mutation, 1, true, Exact, &["attempt", "candidate", "receipt", "action"]),
    spec(K::Snapshot, "Snapshot", Query, 0, false, Open, &["section", "after", "cursor", "limit"]),
    spec(K::Events, "Events", Query, 0, false, Open, &["after", "limit"]),
    spec(K::EventDetail, "EventDetail", Query, 0, false, Open, &["seq", "offset"]),
    spec(K::Log, "Log", Query, 0, false, Open, &["attempt", "sequence", "offset"]),
    spec(K::Receipt, "Receipt", Query, 0, false, Open, &["command"]),
    spec(K::CloseStatus, "CloseStatus", Query, 0, false, Open, &[]),
    spec(K::ReviewBundle, "ReviewBundle", Query, 0, false, Open, &["subject"]),
    spec(K::ShutdownReceipt, "ShutdownReceipt", Query, 0, false, Open, &["ticket"]),
    spec(K::Recovery, "Recovery", Query, 0, false, Open, &["offset", "limit"]),
    spec(K::RecoveryInspect, "RecoveryInspect", Query, 1, false, Open, &[]),
    spec(K::Candidate, "Candidate", Query, 1, false, Exact, &["attempt"]),
    spec(K::CandidateChanges, "CandidateChanges", Query, 1, false, Exact, &["attempt", "candidate", "offset", "limit"]),
    spec(K::CandidateDiff, "CandidateDiff", Query, 1, false, Exact, &["attempt", "candidate", "offset"]),
    spec(K::ValidationReport, "ValidationReport", Query, 1, false, Exact, &["attempt", "receipt", "offset"]),
    spec(K::ApprovedPlan, "ApprovedPlan", Query, 1, false, Exact, &["after"]),
    spec(K::ChatTurn, "ChatTurn", Mutation, 2, false, Exact, &["feature", "kind", "repository", "provider", "message"]),
    spec(K::AnswerQuestion, "AnswerQuestion", Mutation, 2, false, Exact, &["question", "revision", "action", "answer"]),
    spec(K::CancelTurn, "CancelTurn", Mutation, 2, true, Exact, &["turn"]),
    spec(K::FeatureChat, "FeatureChat", Query, 2, false, Exact, &["feature", "after"]),
    spec(K::ChatText, "ChatText", Query, 2, false, Exact, &["subject", "part", "offset"]),
    spec(K::ChatIndex, "ChatIndex", Query, 2, false, Exact, &[]),
    spec(K::SaveProject, "SaveProject", Mutation, 3, false, Exact, &["project", "revision", "name", "repositories", "instructions", "test_command", "ci_command", "kinds", "default_provider", "capacity", "repair_limit", "review"]),
    spec(K::Projects, "Projects", Query, 3, false, Exact, &[]),
    spec(K::RunPlan, "RunPlan", Mutation, 3, false, Exact, &["plan"]),
    spec(K::ControlPlan, "ControlPlan", Mutation, 3, true, Exact, &["plan", "action"]),
    spec(K::PlanRun, "PlanRun", Query, 3, false, Exact, &["plan"]),
    spec(K::Executables, "Executables", Query, 4, false, Exact, &[]),
    spec(K::Repairs, "Repairs", Query, 5, false, Exact, &[]),
    spec(K::Attempts, "Attempts", Query, 6, false, Exact, &["before"]),
    spec(K::Documents, "Documents", Query, 6, false, Exact, &["before"]),
    spec(K::Features, "Features", Query, 7, false, Exact, &["before"]),
    spec(K::Document, "Document", Query, 9, false, Exact, &["document", "revision", "offset"]),
    spec(K::Decisions, "Decisions", Query, 9, false, Exact, &["before"]),
    spec(K::PlanDetail, "PlanDetail", Query, 9, false, Exact, &["plan"]),
    spec(K::AttemptDetail, "AttemptDetail", Query, 9, false, Exact, &["attempt"]),
    spec(K::ExternalActions, "ExternalActions", Query, 10, false, Exact, &["plan"]),
    spec(K::RequestExternal, "RequestExternal", Mutation, 10, false, Exact, &["plan", "kind"]),
    spec(K::AuthorizeExternal, "AuthorizeExternal", Mutation, 10, false, Exact, &["action", "digest", "decision"]),
    spec(K::InspectExternal, "InspectExternal", Mutation, 10, false, Exact, &["action"]),
    spec(K::ResolveExternal, "ResolveExternal", Mutation, 10, false, Exact, &["action"]),
    spec(K::RecoveryDetail, "RecoveryDetail", Query, 11, false, Exact, &[]),
    spec(K::ProjectDetail, "ProjectDetail", Query, 11, false, Exact, &["project"]),
    spec(K::BriefTurn, "BriefTurn", Mutation, 12, false, Exact, &["repository", "provider", "message"]),
    spec(K::Workspace, "Workspace", Query, 12, false, Exact, &[]),
];

impl RequestKind {
    pub fn spec(self) -> &'static Spec {
        &SPECS[self as usize]
    }
    pub fn name(self) -> &'static str {
        self.spec().name
    }
    /// The request a frame's type names, whatever minor the frame claims.
    pub fn named(name: &str) -> Option<Self> {
        SPECS.iter().find(|s| s.name == name).map(|s| s.kind)
    }
    /// The request a frame of protocol minor `minor` names. A request newer than the
    /// frame does not exist for it.
    pub fn of_frame(name: &str, minor: u64) -> Option<Self> {
        Self::named(name).filter(|k| k.spec().since <= minor)
    }
    pub fn fields(self) -> &'static [&'static str] {
        self.spec().fields
    }
    /// The field-set check of this request's [`Shape`]. It reads no value.
    pub fn shaped(self, payload: &Json) -> bool {
        let Json::Object(given) = payload else {
            return false;
        };
        let fields = self.fields();
        match self.spec().shape {
            Open => true,
            Count => given.len() == fields.len(),
            Exact => {
                given.len() == fields.len()
                    && fields
                        .iter()
                        .all(|k| given.iter().any(|(name, _)| name == k))
            }
        }
    }
}

/// The names Hello lists for a frame of protocol minor `minor`, separated by one space.
pub fn names(class: Class, minor: u64) -> String {
    SPECS
        .iter()
        .filter(|s| s.class == class && s.since <= minor)
        .map(|s| s.name)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A lowercase SHA-256 digest as protocol text.
pub fn digest(value: &Json) -> Result<String, String> {
    let s = string(value)?;
    if s.len() != 64
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("expected a SHA-256 digest".into());
    }
    Ok(s.to_owned())
}

fn optional_uint(value: &Json) -> Result<Option<u64>, String> {
    match value {
        Json::Null => Ok(None),
        v => uint(v).map(Some),
    }
}

/// The `attempt` every review query names first.
pub fn attempt(p: &Json) -> Result<OpaqueId, String> {
    id(get(p, "attempt")?)
}

/// The inspection a recovery command was reviewed against must be the current one.
pub fn reviewed_inspection(p: &Json, current: &str) -> Result<(), String> {
    if string(get(p, "digest")?)? != current {
        return Err("StaleInspection: inspect again".into());
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotQuery {
    pub section: String,
    pub after: String,
    pub cursor: Option<u64>,
    pub limit: u32,
}
impl SnapshotQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            section: string(get(p, "section")?)?.to_owned(),
            after: string(get(p, "after")?)?.to_owned(),
            cursor: optional_uint(get(p, "cursor")?)?,
            limit: u32::try_from(uint(get(p, "limit")?)?).map_err(|_| "Limit")?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventsQuery {
    pub after: u64,
    pub limit: u32,
}
impl EventsQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            after: uint(get(p, "after")?)?,
            limit: u32::try_from(uint(get(p, "limit")?)?).map_err(|_| "Limit")?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventDetailQuery {
    pub seq: u64,
    pub offset: u64,
}
impl EventDetailQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            seq: uint(get(p, "seq")?)?,
            offset: uint(get(p, "offset")?)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogQuery {
    pub attempt: OpaqueId,
    pub sequence: u64,
    pub offset: u64,
}
impl LogQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            attempt: attempt(p)?,
            sequence: uint(get(p, "sequence")?)?,
            offset: uint(get(p, "offset")?)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReceiptQuery {
    pub command: OpaqueId,
}
impl ReceiptQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            command: id(get(p, "command")?)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShutdownReceiptQuery {
    pub ticket: OpaqueId,
}
impl ShutdownReceiptQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            ticket: id(get(p, "ticket")?)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryQuery {
    pub offset: usize,
    /// 1 to [`MAX_PAGE`].
    pub limit: usize,
}
impl RecoveryQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        let offset = usize::try_from(uint(get(p, "offset")?)?).map_err(|_| "Offset")?;
        let limit = page_limit(get(p, "limit")?)?;
        Ok(Self { offset, limit })
    }
}

fn page_limit(value: &Json) -> Result<usize, String> {
    let limit = uint(value)?;
    if limit == 0 || limit > u64::from(MAX_PAGE) {
        return Err("Limit".into());
    }
    Ok(limit as usize)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReviewBundleQuery {
    pub subject: ArtifactId,
}
impl ReviewBundleQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            subject: ArtifactId(id(get(p, "subject")?)?),
        })
    }
}

/// The approved plans are answered one at a time, in identity order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApprovedPlanQuery {
    /// The plan the previous answer held; `None` asks for the first.
    pub after: Option<ArtifactId>,
}
impl ApprovedPlanQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            after: match get(p, "after")? {
                Json::Null => None,
                v => Some(ArtifactId(id(v)?)),
            },
        })
    }
}

/// Protocol 1.6: one page of `Attempts` or `Documents`, newest first; 1.7: of `Features`;
/// 1.9: of `Decisions`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OverviewQuery {
    /// The `next` of the previous answer; `None` asks for the newest rows.
    pub before: Option<u64>,
}
impl OverviewQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            before: optional_uint(get(p, "before")?)?,
        })
    }
}

/// Protocol 1.9: one revision of a document (`None`: its newest) and its text from a
/// character offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DocumentQuery {
    pub document: ArtifactId,
    pub revision: Option<u64>,
    pub offset: usize,
}
impl DocumentQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            document: ArtifactId(id(get(p, "document")?)?),
            revision: optional_uint(get(p, "revision")?)?,
            offset: usize::try_from(uint(get(p, "offset")?)?).map_err(|_| "Offset")?,
        })
    }
}

/// Protocol 1.9: the steps of one execution plan with what the Execution Plan screen shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlanDetailQuery {
    pub plan: ArtifactId,
}
impl PlanDetailQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            plan: ArtifactId(id(get(p, "plan")?)?),
        })
    }
}

/// Protocol 1.9: what the core records about one attempt beyond `Attempts`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AttemptDetailQuery {
    pub attempt: OpaqueId,
}
impl AttemptDetailQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            attempt: id(get(p, "attempt")?)?,
        })
    }
}

/// Protocol 1.10 (M5): the external actions of one plan (`None`: of every plan).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExternalActionsQuery {
    pub plan: Option<ArtifactId>,
}
impl ExternalActionsQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            plan: match get(p, "plan")? {
                Json::Null => None,
                v => Some(ArtifactId(id(v)?)),
            },
        })
    }
}

/// Protocol 1.11: what the core derives about one project's repositories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectDetailQuery {
    pub project: OpaqueId,
}
impl ProjectDetailQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            project: id(get(p, "project")?)?,
        })
    }
}

/// Protocol 1.12 (step 7): the project brief of one registered repository, written by a
/// read-only lead turn on `provider`. `message` is what the human says they bring.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BriefTurnCommand {
    pub repository: RepositoryId,
    pub provider: ProviderKind,
    pub message: String,
}
impl BriefTurnCommand {
    pub fn parse(p: &Json) -> Result<Self, String> {
        let repository = RepositoryId(id(get(p, "repository")?)?);
        let provider = match string(get(p, "provider")?)? {
            "Codex" => ProviderKind::Codex,
            "ClaudeCode" => ProviderKind::ClaudeCode,
            _ => return Err("UnknownProvider".into()),
        };
        Ok(Self {
            repository,
            provider,
            message: string(get(p, "message")?)?.to_owned(),
        })
    }
}

/// What an external action does: push the plan branch, or open its draft pull request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalKindName {
    Push,
    PullRequest,
}

/// Protocol 1.10 (M5): the human asks for a push or a draft pull request of a finished
/// plan. Asking performs nothing; the request waits for its own decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestExternalCommand {
    pub plan: ArtifactId,
    pub kind: ExternalKindName,
}
impl RequestExternalCommand {
    pub fn parse(p: &Json) -> Result<Self, String> {
        let plan = ArtifactId(id(get(p, "plan")?)?);
        let kind = match string(get(p, "kind")?)? {
            "Push" => ExternalKindName::Push,
            "PullRequest" => ExternalKindName::PullRequest,
            _ => return Err("UnknownKind".into()),
        };
        Ok(Self { plan, kind })
    }
}

/// The operation key of an external action as protocol text.
pub fn external_action(value: &Json) -> Result<String, String> {
    let s = string(value)?;
    if !s.starts_with("external:")
        || s.len() > 200
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase() || b == b':')
    {
        return Err("expected an external action".into());
    }
    Ok(s.to_owned())
}

/// Protocol 1.10 (M5): the human's decision on the exact payload of one request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizeExternalCommand {
    pub action: String,
    pub digest: String,
    pub authorize: bool,
}
impl AuthorizeExternalCommand {
    pub fn parse(p: &Json) -> Result<Self, String> {
        let action = external_action(get(p, "action")?)?;
        let digest = digest(get(p, "digest")?)?;
        let authorize = match string(get(p, "decision")?)? {
            "Authorize" => true,
            "Decline" => false,
            _ => return Err("UnknownAction".into()),
        };
        Ok(Self {
            action,
            digest,
            authorize,
        })
    }
}

/// Protocol 1.10 (M5): `InspectExternal` and `ResolveExternal` name one action.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalCommand {
    pub action: String,
}
impl ExternalCommand {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            action: external_action(get(p, "action")?)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecisionVerb {
    Approve,
    Reject,
    Revoke,
}

/// The exact review bundle the human decided on, and the decision. The revision is the
/// envelope's; it is required once the subject has been read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionCommand {
    pub subject: ArtifactId,
    pub revision: Revision,
    pub hash: String,
    pub content_hash: String,
    pub displayed_hash: String,
    pub context_hash: String,
    pub upstream: Vec<u64>,
    pub prior_decision: Option<u64>,
    pub action: DecisionVerb,
}
impl DecisionCommand {
    pub const MAX_UPSTREAM: usize = 32;

    pub fn parse(p: &Json, revision: Option<u64>) -> Result<Self, String> {
        Ok(Self {
            subject: ArtifactId(id(get(p, "subject")?)?),
            revision: Revision(revision.ok_or("RevisionRequired")?),
            hash: string(get(p, "hash")?)?.into(),
            content_hash: string(get(p, "content_hash")?)?.into(),
            displayed_hash: string(get(p, "displayed_hash")?)?.into(),
            context_hash: string(get(p, "context_hash")?)?.into(),
            upstream: match get(p, "upstream")? {
                Json::List(v) if v.len() <= Self::MAX_UPSTREAM => {
                    v.iter().map(uint).collect::<Result<_, _>>()?
                }
                _ => return Err("InvalidUpstream".into()),
            },
            prior_decision: optional_uint(get(p, "prior_decision")?)?,
            action: match string(get(p, "action")?)? {
                "Approve" => DecisionVerb::Approve,
                "Reject" => DecisionVerb::Reject,
                "Revoke" => DecisionVerb::Revoke,
                _ => return Err("UnknownDecision".into()),
            },
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CloseCommand {
    Prepare,
    Cancel { ticket: OpaqueId },
    Confirm { ticket: OpaqueId },
    CancelAttempt { attempt: OpaqueId },
}
impl CloseCommand {
    /// `kind` is one of the four close commands.
    pub fn parse(kind: RequestKind, p: &Json) -> Result<Self, String> {
        Ok(match kind {
            K::PrepareClose => Self::Prepare,
            K::CancelClose => Self::Cancel {
                ticket: id(get(p, "ticket")?)?,
            },
            K::ConfirmClose => Self::Confirm {
                ticket: id(get(p, "ticket")?)?,
            },
            _ => Self::CancelAttempt {
                attempt: attempt(p)?,
            },
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisterRepositoryCommand {
    pub path: String,
}
impl RegisterRepositoryCommand {
    pub const MAX_PATH_BYTES: usize = 4096;

    pub fn parse(p: &Json) -> Result<Self, String> {
        let path = string(get(p, "path")?)?;
        if path.len() > Self::MAX_PATH_BYTES || path.contains('\0') {
            return Err("InvalidRepositoryPath".into());
        }
        Ok(Self { path: path.into() })
    }
}

/// A feature bundle as the client sent it: identities, strictly imported PRD and
/// validation plan, and the texts the storage layer bounds and checks further.
#[derive(Clone, Debug)]
pub struct ImportBundleCommand {
    pub feature: FeatureId,
    pub prd_id: ArtifactId,
    pub adr_id: ArtifactId,
    pub plan_id: ArtifactId,
    pub validation_id: ArtifactId,
    pub prd: PrdDocument,
    pub adr: String,
    pub step: OpaqueId,
    pub repository: RepositoryId,
    pub base: String,
    pub provider: ProviderKind,
    pub validation: ValidationPlan,
}
impl ImportBundleCommand {
    pub fn parse(p: &Json) -> Result<Self, String> {
        let subject = |key: &str| get(p, key).and_then(id).map(ArtifactId);
        let prd_id = subject("prd_id")?;
        let adr_id = subject("adr_id")?;
        let plan_id = subject("plan_id")?;
        let validation_id = subject("validation_id")?;
        let prd = PrdDocument::import(string(get(p, "prd")?)?.as_bytes())
            .map_err(|e| format!("InvalidPrd: {e:?}"))?;
        let validation = ValidationPlan::import(string(get(p, "validation")?)?.as_bytes())
            .map_err(|e| format!("InvalidValidationPlan: {e}"))?;
        let provider = match string(get(p, "provider")?)? {
            "Codex" => ProviderKind::Codex,
            "ClaudeCode" => ProviderKind::ClaudeCode,
            _ => return Err("UnknownProvider".into()),
        };
        Ok(Self {
            feature: FeatureId(id(get(p, "feature")?)?),
            prd_id,
            adr_id,
            plan_id,
            validation_id,
            prd,
            adr: string(get(p, "adr")?)?.to_owned(),
            step: id(get(p, "step")?)?,
            repository: RepositoryId(id(get(p, "repository")?)?),
            base: string(get(p, "base")?)?.to_owned(),
            provider,
            validation,
        })
    }
}

/// M2 (protocol 1.2): one lead agent turn of a Feature Chat. The turn's identity is the
/// command identity. Like a Start, nothing here names an executable, argument or variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatTurnCommand {
    pub feature: FeatureId,
    pub kind: TurnKind,
    pub repository: RepositoryId,
    pub provider: ProviderKind,
    /// The objective of the first turn of a feature; a note, possibly empty, after it.
    pub message: String,
}
impl ChatTurnCommand {
    pub fn parse(p: &Json) -> Result<Self, String> {
        let feature = FeatureId(id(get(p, "feature")?)?);
        let kind = TurnKind::named(string(get(p, "kind")?)?).ok_or("UnknownTurnKind")?;
        let repository = RepositoryId(id(get(p, "repository")?)?);
        let provider = match string(get(p, "provider")?)? {
            "Codex" => ProviderKind::Codex,
            "ClaudeCode" => ProviderKind::ClaudeCode,
            _ => return Err("UnknownProvider".into()),
        };
        Ok(Self {
            feature,
            kind,
            repository,
            provider,
            message: string(get(p, "message")?)?.to_owned(),
        })
    }
}

/// M3 (protocol 1.3): the whole project record, saved as the revision after `revision`
/// (0 for a new project).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveProjectCommand {
    pub project: OpaqueId,
    pub revision: u64,
    pub record: crate::project::ProjectRecord,
}
impl SaveProjectCommand {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            project: id(get(p, "project")?)?,
            revision: uint(get(p, "revision")?)?,
            record: crate::project::ProjectRecord::from_json(p)?,
        })
    }
}

/// M3 (protocol 1.3): the human starts, or reads the run of, one approved lead plan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlanCommand {
    pub plan: ArtifactId,
}
impl PlanCommand {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            plan: ArtifactId(id(get(p, "plan")?)?),
        })
    }
}

/// M3 (protocol 1.3): Pause, Resume or Cancel the run of one plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlPlanCommand {
    pub plan: ArtifactId,
    pub action: String,
}
impl ControlPlanCommand {
    pub fn parse(p: &Json) -> Result<Self, String> {
        let plan = ArtifactId(id(get(p, "plan")?)?);
        let action = string(get(p, "action")?)?;
        if !matches!(action, "Pause" | "Resume" | "Cancel") {
            return Err("UnknownAction".into());
        }
        Ok(Self {
            plan,
            action: action.to_owned(),
        })
    }
}

/// What the human does with one question the lead asked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuestionAction {
    /// A new answer, which supersedes an earlier one.
    Answer(String),
    /// Set the question aside: it no longer blocks review.
    Defer,
    /// Make a deferred question blocking again.
    Block,
}

/// M2 (protocol 1.2): the human's answer to, or disposition of, one question at the
/// revision the human was shown.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnswerQuestionCommand {
    pub question: QuestionId,
    pub revision: Revision,
    pub action: QuestionAction,
}
impl AnswerQuestionCommand {
    pub fn parse(p: &Json) -> Result<Self, String> {
        let question = QuestionId(id(get(p, "question")?)?);
        let revision = Revision(uint(get(p, "revision")?)?);
        let answer = get(p, "answer")?;
        let action = match (string(get(p, "action")?)?, answer) {
            ("Answer", Json::Text(body)) => QuestionAction::Answer(body.clone()),
            ("Defer", Json::Null) => QuestionAction::Defer,
            ("Block", Json::Null) => QuestionAction::Block,
            ("Answer" | "Defer" | "Block", _) => {
                return Err(
                    "InvalidAnswer: an answer is text exactly when the action is Answer".into(),
                );
            }
            _ => return Err("UnknownAction".into()),
        };
        Ok(Self {
            question,
            revision,
            action,
        })
    }
}

/// M2 (protocol 1.2): the human's stop of a running turn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CancelTurnCommand {
    pub turn: OpaqueId,
}
impl CancelTurnCommand {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            turn: id(get(p, "turn")?)?,
        })
    }
}

/// M2 (protocol 1.2): one page of a feature's chat, the turns after `after` (an ordinal).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FeatureChatQuery {
    pub feature: FeatureId,
    pub after: u64,
}
impl FeatureChatQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            feature: FeatureId(id(get(p, "feature")?)?),
            after: uint(get(p, "after")?)?,
        })
    }
}

/// The long texts of a chat that a page carries only in part.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChatPart {
    /// What the human typed for a turn (subject: the turn).
    Message,
    /// The lead's last message, the raw reply (subject: the turn).
    Reply,
    /// The prompt the turn was run with (subject: the turn).
    Prompt,
    /// The newest answer to a question (subject: the question).
    Answer,
    /// The newest version of a requirements document, rendered (subject: the document).
    Prd,
}
impl ChatPart {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "Message" => Self::Message,
            "Reply" => Self::Reply,
            "Prompt" => Self::Prompt,
            "Answer" => Self::Answer,
            "Prd" => Self::Prd,
            _ => return None,
        })
    }
}

/// M2 (protocol 1.2): one chunk of a long chat text, from a character offset.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChatTextQuery {
    pub subject: OpaqueId,
    pub part: ChatPart,
    pub offset: u64,
}
impl ChatTextQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            subject: id(get(p, "subject")?)?,
            part: ChatPart::named(string(get(p, "part")?)?).ok_or("UnknownPart")?,
            offset: uint(get(p, "offset")?)?,
        })
    }
}

/// The five identities a Start names. Nothing else can be sent: no executable, argument
/// or environment value exists in this type or in the command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StartCommand {
    pub prd: ArtifactId,
    pub adr: ArtifactId,
    pub plan: ArtifactId,
    pub validation: ArtifactId,
    pub step: OpaqueId,
}
impl StartCommand {
    pub fn parse(p: &Json) -> Result<Self, String> {
        let subject = |key: &str| get(p, key).and_then(id).map(ArtifactId);
        Ok(Self {
            prd: subject("prd")?,
            adr: subject("adr")?,
            plan: subject("plan")?,
            validation: subject("validation")?,
            step: id(get(p, "step")?)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryResolveCommand {
    pub attempt: OpaqueId,
    pub choice: RecoveryChoice,
}
impl RecoveryResolveCommand {
    /// `inspected` is the digest of the helper's current inspection. A command reviewed
    /// against another one is stale whatever else it says.
    pub fn parse(p: &Json, inspected: &str) -> Result<Self, String> {
        reviewed_inspection(p, inspected)?;
        Ok(Self {
            attempt: attempt(p)?,
            choice: RecoveryChoice::from_wire(string(get(p, "choice")?)?).ok_or("UnknownChoice")?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidateCommand {
    pub attempt: OpaqueId,
    pub candidate: String,
}
impl ValidateCommand {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            attempt: attempt(p)?,
            candidate: digest(get(p, "candidate")?)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateVerdict {
    /// Bound to the validation receipt the human reviewed.
    Accept {
        receipt: u64,
    },
    Reject,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateDecisionCommand {
    pub attempt: OpaqueId,
    pub candidate: String,
    pub verdict: CandidateVerdict,
}
impl CandidateDecisionCommand {
    pub fn parse(p: &Json) -> Result<Self, String> {
        let attempt = attempt(p)?;
        let candidate = digest(get(p, "candidate")?)?;
        let verdict = match (string(get(p, "action")?)?, get(p, "receipt")?) {
            ("Accept", receipt) => CandidateVerdict::Accept {
                receipt: uint(receipt).map_err(|_| "ReceiptRequired")?,
            },
            ("Reject", Json::Null) => CandidateVerdict::Reject,
            _ => return Err("UnknownDecision".into()),
        };
        Ok(Self {
            attempt,
            candidate,
            verdict,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateChangesQuery {
    pub attempt: OpaqueId,
    pub candidate: String,
    pub offset: usize,
    /// 1 to [`MAX_PAGE`].
    pub limit: usize,
}
impl CandidateChangesQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            attempt: attempt(p)?,
            candidate: digest(get(p, "candidate")?)?,
            offset: usize::try_from(uint(get(p, "offset")?)?).map_err(|_| "Offset")?,
            limit: page_limit(get(p, "limit")?)?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateDiffQuery {
    pub attempt: OpaqueId,
    pub candidate: String,
    pub offset: u64,
}
impl CandidateDiffQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            attempt: attempt(p)?,
            candidate: digest(get(p, "candidate")?)?,
            offset: uint(get(p, "offset")?)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidationReportQuery {
    pub attempt: OpaqueId,
    pub receipt: u64,
    pub offset: u64,
}
impl ValidationReportQuery {
    pub fn parse(p: &Json) -> Result<Self, String> {
        Ok(Self {
            attempt: attempt(p)?,
            receipt: uint(get(p, "receipt")?)?,
            offset: uint(get(p, "offset")?)?,
        })
    }
}

/// Bounds of the structured assessment in a Recovery row. The row's `assessment` text is
/// unchanged and unbounded as before; these fields are additional.
pub const MAX_ROW_BLOCKERS: usize = 8;
pub const MAX_BLOCKER_CHARS: usize = 256;

/// The recovery assessment as structured fields of a Recovery row (protocol 1.1): what
/// blocks, whether every process is proven gone, and the choices the evidence offers, by
/// the names RecoveryResolve accepts.
pub fn assessment_fields(a: &Assessment) -> [(&'static str, Json); 4] {
    let complete = a.blockers.len() <= MAX_ROW_BLOCKERS
        && a.blockers
            .iter()
            .all(|b| b.chars().count() <= MAX_BLOCKER_CHARS);
    [
        (
            "blockers",
            Json::List(
                a.blockers
                    .iter()
                    .take(MAX_ROW_BLOCKERS)
                    .map(|b| text(b.chars().take(MAX_BLOCKER_CHARS).collect::<String>()))
                    .collect(),
            ),
        ),
        ("blockers_complete", Json::Bool(complete)),
        ("process_absent", Json::Bool(a.process_absent)),
        (
            "choices",
            Json::List(a.choices.iter().map(|c| text(c.wire())).collect()),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{encode, number, object};

    #[test]
    fn r01_table_is_in_kind_order_with_unique_names_and_fields() {
        for (i, s) in SPECS.iter().enumerate() {
            assert_eq!(s.kind as usize, i, "{}", s.name);
            assert_eq!(format!("{:?}", s.kind), s.name);
            assert_eq!(RequestKind::named(s.name), Some(s.kind));
            assert!(s.since <= crate::ipc::MINOR as u64);
            let mut fields = s.fields.to_vec();
            fields.sort_unstable();
            fields.dedup();
            assert_eq!(fields.len(), s.fields.len(), "{}", s.name);
        }
        assert_eq!(RequestKind::named("hello"), None);
        assert_eq!(RequestKind::named(""), None);
    }

    #[test]
    fn r01_hello_lists_and_minor_gating_are_the_1_0_and_1_1_vocabularies() {
        assert_eq!(
            names(Mutation, 0),
            "PrepareClose CancelClose ConfirmClose CancelAttempt HumanDecision"
        );
        assert_eq!(
            names(Query, 0),
            "Snapshot Events EventDetail Log Receipt CloseStatus ReviewBundle ShutdownReceipt \
             Recovery"
        );
        let v11_commands = "PrepareClose CancelClose ConfirmClose CancelAttempt HumanDecision \
             RegisterRepository ImportBundle Start RecoveryResolve RecoveryStart Validate \
             CandidateDecision";
        let v11_queries = "Snapshot Events EventDetail Log Receipt CloseStatus ReviewBundle \
             ShutdownReceipt Recovery RecoveryInspect Candidate CandidateChanges CandidateDiff \
             ValidationReport ApprovedPlan";
        assert_eq!(names(Mutation, 1), v11_commands);
        assert_eq!(names(Query, 1), v11_queries);
        // M2 (1.2): Feature Chat, listed after everything a 1.1 client knows.
        let v12_commands = format!("{v11_commands} ChatTurn AnswerQuestion CancelTurn");
        let v12_queries = format!("{v11_queries} FeatureChat ChatText ChatIndex");
        assert_eq!(names(Mutation, 2), v12_commands);
        assert_eq!(names(Query, 2), v12_queries);
        // M3 (1.3): project records and the plan runner, after everything 1.2 knows.
        // D3 (1.4): the executables query, after everything 1.3 knows.
        // M4 (1.5): the repairs query, after everything 1.4 knows.
        // 1.6: the attempts and documents queries, after everything 1.5 knows.
        // 1.7: the features query, after everything 1.6 knows.
        // 1.9: the document, decision, plan and attempt detail queries (1.8 added none).
        // 1.10 (M5): the external actions query and its four commands.
        // 1.11: the recovery detail and project detail queries.
        // 1.12 (step 7): the brief turn and the workspace query.
        for minor in [3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13] {
            let v110 = if minor >= 10 {
                " RequestExternal AuthorizeExternal InspectExternal ResolveExternal"
            } else {
                ""
            };
            let v112 = if minor >= 12 { " BriefTurn" } else { "" };
            assert_eq!(
                names(Mutation, minor),
                format!("{v12_commands} SaveProject RunPlan ControlPlan{v110}{v112}")
            );
            let v14 = if minor >= 4 { " Executables" } else { "" };
            let v15 = if minor >= 5 { " Repairs" } else { "" };
            let v16 = if minor >= 6 {
                " Attempts Documents"
            } else {
                ""
            };
            let v17 = if minor >= 7 { " Features" } else { "" };
            let v19 = if minor >= 9 {
                " Document Decisions PlanDetail AttemptDetail"
            } else {
                ""
            };
            let v110q = if minor >= 10 { " ExternalActions" } else { "" };
            let v111 = if minor >= 11 {
                " RecoveryDetail ProjectDetail"
            } else {
                ""
            };
            let v112q = if minor >= 12 { " Workspace" } else { "" };
            assert_eq!(
                names(Query, minor),
                format!(
                    "{v12_queries} Projects PlanRun{v14}{v15}{v16}{v17}{v19}{v110q}{v111}{v112q}"
                )
            );
        }
        assert_eq!(RequestKind::of_frame("BriefTurn", 11), None);
        assert_eq!(RequestKind::of_frame("Workspace", 12), Some(K::Workspace));
        assert_eq!(RequestKind::of_frame("Executables", 3), None);
        assert_eq!(RequestKind::of_frame("Repairs", 4), None);
        assert_eq!(RequestKind::of_frame("Attempts", 5), None);
        assert_eq!(RequestKind::of_frame("Documents", 6), Some(K::Documents));
        assert_eq!(RequestKind::of_frame("Features", 6), None);
        assert_eq!(RequestKind::of_frame("Features", 7), Some(K::Features));
        assert_eq!(RequestKind::of_frame("Document", 8), None);
        assert_eq!(
            RequestKind::of_frame("AttemptDetail", 9),
            Some(K::AttemptDetail)
        );
        assert_eq!(RequestKind::of_frame("ExternalActions", 9), None);
        assert_eq!(RequestKind::of_frame("RecoveryDetail", 10), None);
        assert_eq!(
            RequestKind::of_frame("ProjectDetail", 11),
            Some(K::ProjectDetail)
        );
        assert_eq!(
            RequestKind::of_frame("AuthorizeExternal", 10),
            Some(K::AuthorizeExternal)
        );
        assert_eq!(RequestKind::of_frame("SaveProject", 2), None);
        assert_eq!(RequestKind::of_frame("ChatTurn", 1), None);
        assert_eq!(RequestKind::of_frame("ChatTurn", 2), Some(K::ChatTurn));
        assert_eq!(names(Handshake, 0), "Hello");
        assert_eq!(RequestKind::of_frame("Start", 0), None);
        assert_eq!(RequestKind::of_frame("Start", 1), Some(K::Start));
        assert_eq!(RequestKind::of_frame("Snapshot", 0), Some(K::Snapshot));
        let control: Vec<_> = SPECS.iter().filter(|s| s.control).map(|s| s.name).collect();
        assert_eq!(
            control.join(" "),
            "Hello PrepareClose CancelClose ConfirmClose CancelAttempt HumanDecision Start \
             RecoveryResolve RecoveryStart Validate CandidateDecision CancelTurn ControlPlan"
        );
    }

    #[test]
    fn r01_shapes_are_open_counted_or_exact() {
        let one = |k: &'static str| object([(k, number(1))]);
        assert!(K::Snapshot.shaped(&one("anything")));
        assert!(!K::Snapshot.shaped(&Json::Null));
        assert!(K::RecoveryStart.shaped(&one("digest")));
        assert!(!K::RecoveryStart.shaped(&one("other")));
        assert!(!K::RecoveryStart.shaped(&object([])));
        assert!(
            !K::RecoveryStart.shaped(&object([("digest", number(1)), ("executable", number(1))]))
        );
        let eight = object(["a", "b", "c", "d", "e", "f", "g", "h"].map(|k| (k, number(1))));
        assert!(K::HumanDecision.shaped(&eight));
        assert!(!K::HumanDecision.shaped(&one("subject")));
    }

    #[test]
    fn r01_payloads_parse_into_typed_values() {
        let hex = |n: u128| crate::ipc::identity(OpaqueId(n));
        let p = object([
            ("attempt", hex(7)),
            ("candidate", text("a".repeat(64))),
            ("receipt", number(3)),
            ("action", text("Accept")),
        ]);
        assert_eq!(
            CandidateDecisionCommand::parse(&p).unwrap(),
            CandidateDecisionCommand {
                attempt: OpaqueId(7),
                candidate: "a".repeat(64),
                verdict: CandidateVerdict::Accept { receipt: 3 },
            }
        );
        let p = object([
            ("digest", text("d")),
            ("attempt", hex(7)),
            ("choice", text("Hold")),
        ]);
        assert_eq!(
            RecoveryResolveCommand::parse(&p, "d").unwrap().choice,
            RecoveryChoice::Hold
        );
        assert_eq!(
            RecoveryResolveCommand::parse(&p, "e").unwrap_err(),
            "StaleInspection: inspect again"
        );
        let p = object([("offset", number(0)), ("limit", number(8))]);
        assert_eq!(RecoveryQuery::parse(&p).unwrap().limit, 8);
        assert_eq!(
            ApprovedPlanQuery::parse(&object([("after", Json::Null)])).unwrap(),
            ApprovedPlanQuery { after: None }
        );
        assert_eq!(
            ApprovedPlanQuery::parse(&object([("after", hex(4))])).unwrap(),
            ApprovedPlanQuery {
                after: Some(ArtifactId(OpaqueId(4)))
            }
        );
        assert!(ApprovedPlanQuery::parse(&object([("after", number(4))])).is_err());
        assert_eq!(
            CloseCommand::parse(K::PrepareClose, &object([])).unwrap(),
            CloseCommand::Prepare
        );
        assert_eq!(
            CloseCommand::parse(K::ConfirmClose, &object([("ticket", hex(4))])).unwrap(),
            CloseCommand::Confirm {
                ticket: OpaqueId(4)
            }
        );
    }

    #[test]
    fn r01_assessment_is_sent_as_bounded_structured_fields() {
        use RecoveryChoice::*;
        let a = Assessment {
            blockers: vec![],
            process_absent: true,
            choices: vec![
                ValidateOutput,
                ContinueWithHistory,
                ContinueFreshContext,
                Hold,
            ],
        };
        assert_eq!(
            encode(&object(assessment_fields(&a))),
            r#"{"blockers":[],"blockers_complete":true,"choices":["ValidateOutput","ContinueWithHistory","ContinueFreshContext","Hold"],"process_absent":true}"#
        );
        for choice in a.choices {
            assert_eq!(RecoveryChoice::from_wire(choice.wire()), Some(choice));
            assert_eq!(format!("{choice:?}"), choice.wire());
        }
        assert_eq!(RecoveryChoice::from_wire("HOLD"), None);
        let a = Assessment {
            blockers: (0..9).map(|i| format!("{i}{}", "é".repeat(300))).collect(),
            process_absent: false,
            choices: vec![Hold],
        };
        let fields = assessment_fields(&a);
        let Json::List(blockers) = &fields[0].1 else {
            panic!("blockers is a list")
        };
        assert_eq!(blockers.len(), MAX_ROW_BLOCKERS);
        assert!(
            blockers
                .iter()
                .all(|b| matches!(b, Json::Text(s) if s.chars().count() == MAX_BLOCKER_CHARS))
        );
        assert_eq!(fields[1].1, Json::Bool(false));
        assert_eq!(fields[2].1, Json::Bool(false));
    }
}

import Foundation

/// R-01: every request this client sends, by its protocol name, with the payload fields
/// the helper reads for it. The helper's own definition is the table in
/// `crates/jojo-domain/src/protocol.rs`; `scripts/tests/test_protocol_fields.py` checks the
/// two against each other, so keep one request per `case ...: return [...]` line below.
public enum Request: String, CaseIterable, Sendable {
    case Hello
    case PrepareClose, CancelClose, ConfirmClose, CancelAttempt, HumanDecision
    case Start, RecoveryResolve, RecoveryStart, Validate, CandidateDecision
    case Snapshot, Events, EventDetail, Log, Receipt, CloseStatus, ReviewBundle, Recovery
    case RecoveryInspect, Candidate, CandidateChanges, CandidateDiff, ValidationReport
    case ApprovedPlan
    case ChatTurn, AnswerQuestion, CancelTurn, FeatureChat, ChatText, ChatIndex
    case SaveProject, Projects, RunPlan, ControlPlan, PlanRun
    case Executables
    case Repairs
    case Attempts, Documents
    case Features
    case Document, Decisions, PlanDetail, AttemptDetail
    case ExternalActions, RequestExternal, AuthorizeExternal, InspectExternal, ResolveExternal
    case RecoveryDetail, ProjectDetail

    public var fields: [String] {
        switch self {
        case .Hello: return []
        case .PrepareClose: return []
        case .CancelClose: return ["ticket"]
        case .ConfirmClose: return ["ticket"]
        case .CancelAttempt: return ["attempt"]
        case .HumanDecision: return ["subject", "hash", "content_hash", "displayed_hash", "context_hash", "upstream", "prior_decision", "action"]
        case .Start: return ["prd", "adr", "plan", "validation", "step"]
        case .RecoveryResolve: return ["digest", "attempt", "choice"]
        case .RecoveryStart: return ["digest"]
        case .Validate: return ["attempt", "candidate"]
        case .CandidateDecision: return ["attempt", "candidate", "receipt", "action"]
        case .Snapshot: return ["section", "after", "cursor", "limit"]
        case .Events: return ["after", "limit"]
        case .EventDetail: return ["seq", "offset"]
        case .Log: return ["attempt", "sequence", "offset"]
        case .Receipt: return ["command"]
        case .CloseStatus: return []
        case .ReviewBundle: return ["subject"]
        case .Recovery: return ["offset", "limit"]
        case .RecoveryInspect: return []
        case .Candidate: return ["attempt"]
        case .CandidateChanges: return ["attempt", "candidate", "offset", "limit"]
        case .CandidateDiff: return ["attempt", "candidate", "offset"]
        case .ValidationReport: return ["attempt", "receipt", "offset"]
        case .ApprovedPlan: return ["after"]
        case .ChatTurn: return ["feature", "kind", "repository", "provider", "message"]
        case .AnswerQuestion: return ["question", "revision", "action", "answer"]
        case .CancelTurn: return ["turn"]
        case .FeatureChat: return ["feature", "after"]
        case .ChatText: return ["subject", "part", "offset"]
        case .ChatIndex: return []
        case .SaveProject: return ["project", "revision", "name", "repositories", "instructions", "test_command", "ci_command", "kinds", "default_provider", "capacity", "repair_limit", "review"]
        case .Projects: return []
        case .RunPlan: return ["plan"]
        case .ControlPlan: return ["plan", "action"]
        case .PlanRun: return ["plan"]
        case .Executables: return []
        case .Repairs: return []
        case .Attempts: return ["before"]
        case .Documents: return ["before"]
        case .Features: return ["before"]
        case .Document: return ["document", "revision", "offset"]
        case .Decisions: return ["before"]
        case .PlanDetail: return ["plan"]
        case .AttemptDetail: return ["attempt"]
        case .ExternalActions: return ["plan"]
        case .RequestExternal: return ["plan", "kind"]
        case .AuthorizeExternal: return ["action", "digest", "decision"]
        case .InspectExternal: return ["action"]
        case .ResolveExternal: return ["action"]
        case .RecoveryDetail: return []
        case .ProjectDetail: return ["project"]
        }
    }

    /// The protocol minor a frame of this request carries. M2's Feature Chat requests exist
    /// only from 1.2; every other request keeps the 1.1 frames it always had, so a helper
    /// that does not know 1.2 answers those as before and the chat ones as unknown. M3's
    /// project and plan-runner requests exist only from 1.3, and D3's executables query
    /// only from 1.4, and M4's repairs query only from 1.5, in the same way. The attempts
    /// and documents queries exist only from 1.6; at 1.6 the chat index also names each
    /// feature (Activity & Audit reads events at `Request.attributedEvents` for the same
    /// reason). The features query exists only from 1.7, and at 1.7 an attempt carries
    /// its ordinal, repository, validation and newest activity, so attempts are read there.
    /// At 1.8 a feature chat dates its turns and lists the assumptions, so it is read there.
    /// The document, decisions, plan detail and attempt detail queries exist only from 1.9.
    /// M5's external actions query and its four commands exist only from 1.10, and at 1.10
    /// the documents answer lists the push and pull request requests that wait, so the
    /// documents are read there (after every snapshot, for the sidebar counter). The
    /// recovery detail and project detail queries exist only from 1.11, and at 1.11 an
    /// event carries its category and project (`Request.attributedEvents`).
    public var minor: Int64 {
        let chat: [Request] = [.ChatTurn, .AnswerQuestion, .CancelTurn, .FeatureChat, .ChatText]
        let runner: [Request] = [.SaveProject, .Projects, .RunPlan, .ControlPlan, .PlanRun]
        let external: [Request] = [.ExternalActions, .RequestExternal, .AuthorizeExternal, .InspectExternal,
                                   .ResolveExternal, .Documents]
        if [.RecoveryDetail, .ProjectDetail].contains(self) { return 11 }
        if external.contains(self) { return 10 }
        if [.Document, .Decisions, .PlanDetail, .AttemptDetail].contains(self) { return 9 }
        if self == .FeatureChat { return 8 }
        if [.Features, .Attempts].contains(self) { return 7 }
        if self == .ChatIndex { return 6 }
        if self == .Repairs { return 5 }
        if self == .Executables { return 4 }
        if runner.contains(self) { return 3 }
        return chat.contains(self) ? 2 : Envelope.minor
    }

    /// The minor at which an event carries its time and actor (protocol 1.6), a sentence
    /// that says what it recorded (1.7), and its category and project (1.11).
    public static let attributedEvents: Int64 = 11

    /// A payload carries exactly this request's fields: nothing missing, nothing else.
    public func accepts(_ payload: [String: JSON]) -> Bool {
        Set(payload.keys) == Set(fields)
    }
}

/// The helper's recovery assessment as the structured fields of a protocol 1.1 Recovery
/// row. The GUI reads these; it never parses the row's `assessment` text.
public struct RecoveryAssessment: Equatable, Sendable {
    public let blockers: [String]
    /// False when the helper truncated the list or one of its entries.
    public let blockersComplete: Bool
    /// Every process that could belong to the attempt is proven gone.
    public let processAbsent: Bool
    /// What the helper's evidence offers for this subject.
    public let choices: [RecoveryChoice]

    /// Nil unless the row carries all four fields in their protocol form. A row from an
    /// older helper has none of them.
    public init?(row: JSON) {
        guard let blockerItems = row["blockers"]?.items, let complete = row["blockers_complete"]?.boolean,
              let absent = row["process_absent"]?.boolean, let choiceItems = row["choices"]?.items else { return nil }
        let blockers = blockerItems.compactMap(\.string)
        let choices = choiceItems.compactMap { $0.string.flatMap(RecoveryChoice.init(rawValue:)) }
        guard blockers.count == blockerItems.count, choices.count == choiceItems.count else { return nil }
        self.blockers = blockers
        self.blockersComplete = complete
        self.processAbsent = absent
        self.choices = choices
    }

    /// The offered choices as one line, for example `[ValidateOutput, Hold]`.
    public var choicesText: String {
        "[" + choices.map(\.rawValue).joined(separator: ", ") + "]"
    }
}

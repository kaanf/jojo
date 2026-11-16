import Foundation

/// R-01: every request this client sends, by its protocol name, with the payload fields
/// the helper reads for it. The helper's own definition is the table in
/// `crates/jojo-domain/src/protocol.rs`; `scripts/tests/test_protocol_fields.py` checks the
/// two against each other, so keep one request per `case ...: return [...]` line below.
public enum Request: String, CaseIterable, Sendable {
    case Hello
    case PrepareClose, CancelClose, ConfirmClose, CancelAttempt, HumanDecision
    case Start, RecoveryResolve, RecoveryStart, Validate, CandidateDecision
    case Snapshot, Events, Log, Receipt, CloseStatus, ReviewBundle, Recovery
    case RecoveryInspect, Candidate, CandidateChanges, CandidateDiff, ValidationReport
    case ApprovedPlan
    case ChatTurn, AnswerQuestion, CancelTurn, FeatureChat, ChatText, ChatIndex

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
        }
    }

    /// The protocol minor a frame of this request carries. M2's Feature Chat requests exist
    /// only from 1.2; every other request keeps the 1.1 frames it always had, so a helper
    /// that does not know 1.2 answers those as before and the chat ones as unknown.
    public var minor: Int64 {
        let chat: [Request] = [.ChatTurn, .AnswerQuestion, .CancelTurn, .FeatureChat, .ChatText, .ChatIndex]
        return chat.contains(self) ? 2 : Envelope.minor
    }

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

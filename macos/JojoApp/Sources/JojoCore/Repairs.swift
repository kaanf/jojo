import Foundation

// M4 (protocol 1.5): repairs, holds and reviews of plan steps, as the helper reports them.
// Nothing here is inferred: every field is the helper's answer.

/// The other provider's review of one attempt's candidate.
public struct StepReview: Equatable, Sendable {
    public let provider: String
    /// RUNNING until it ends, then APPROVED, REJECTED, FAILED, CANCELLED or INTERRUPTED.
    public let state: String
    public let notes: String
    public let objection: String?
    public let detail: String

    public init?(_ p: JSON) {
        guard let provider = p["provider"]?.string, let state = p["state"]?.string, let notes = p["notes"]?.string,
              let detail = p["detail"]?.string else { return nil }
        self.init(provider: provider, state: state, notes: notes, objection: p["objection"]?.string, detail: detail)
    }

    public init(provider: String, state: String, notes: String = "", objection: String? = nil, detail: String = "") {
        (self.provider, self.state, self.notes, self.objection, self.detail) = (provider, state, notes, objection, detail)
    }

    /// One line: who reviewed and what they decided.
    public var verdict: String {
        let who = provider == "ClaudeCode" ? "Claude Code" : provider
        switch state {
        case "RUNNING": return "\(who) is reviewing"
        case "APPROVED": return "Approved by \(who)"
        case "REJECTED": return "Rejected by \(who)"
        default: return "No verdict from \(who) (\(state.lowercased()))"
        }
    }
}

/// One attempt of a plan step with its repair, hold and review (`Repairs`).
public struct AttemptRepair: Equatable, Sendable, Identifiable {
    public let id: String
    public let plan: String
    public let step: String
    public let key: String
    public let provider: String
    /// The step's state when this is the step's newest attempt, else nil.
    public let state: String?
    /// "repair ordinal of limit" when the attempt is a repair.
    public let ordinal: Int64?
    public let limit: Int64?
    public let failure: String?
    /// Why the step waits for the human, when this attempt was held.
    public let hold: String?
    public let review: StepReview?

    public init?(_ p: JSON) {
        guard let id = p["attempt"]?.string, let plan = p["plan"]?.string, let step = p["step"]?.string,
              let key = p["key"]?.string, let provider = p["provider"]?.string else { return nil }
        let repair = p["repair"]
        self.init(id: id, plan: plan, step: step, key: key, provider: provider, state: p["state"]?.string,
                  ordinal: repair?["ordinal"]?.integer, limit: repair?["limit"]?.integer, failure: repair?["failure"]?.string,
                  hold: p["hold"]?.string, review: p["review"].flatMap(StepReview.init))
    }

    public init(id: String, plan: String, step: String, key: String, provider: String, state: String? = nil,
                ordinal: Int64? = nil, limit: Int64? = nil, failure: String? = nil, hold: String? = nil,
                review: StepReview? = nil) {
        (self.id, self.plan, self.step, self.key, self.provider, self.state) = (id, plan, step, key, provider, state)
        (self.ordinal, self.limit, self.failure, self.hold, self.review) = (ordinal, limit, failure, hold, review)
    }

    /// "repair 1 of 2", or "first attempt".
    public var counter: String {
        guard let ordinal, let limit else { return "first attempt" }
        return "repair \(ordinal) of \(limit)"
    }
}

/// The answer of `Repairs`: attempts of plan runs, newest run first.
public struct RepairList: Equatable, Sendable {
    public let attempts: [AttemptRepair]
    /// False when the helper listed only the first ones.
    public let complete: Bool

    public init?(_ p: [String: JSON]) {
        guard let items = p["attempts"]?.items, let complete = p["complete"]?.boolean else { return nil }
        let attempts = items.compactMap(AttemptRepair.init)
        guard attempts.count == items.count else { return nil }
        (self.attempts, self.complete) = (attempts, complete)
    }

    public init(attempts: [AttemptRepair], complete: Bool = true) {
        (self.attempts, self.complete) = (attempts, complete)
    }

    public subscript(attempt: String) -> AttemptRepair? { attempts.first { $0.id == attempt } }
}

extension WorkspaceModel {
    /// M4: reads the repairs, holds and reviews of plan steps. Asked only while Agent Runs
    /// is shown, and again with each new snapshot.
    public func loadRepairs() {
        guard repairsLoad != .loading, snapshotSeq != nil, repairsShown else { return }
        repairsLoad = .loading
        onChange?()
        client.query(.Repairs, [:]) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                if let list = RepairList(p) {
                    (self.repairs, self.repairsLoad) = (list, .loaded)
                } else {
                    (self.repairs, self.repairsLoad) = (nil, .failed("malformed repairs"))
                }
            case .error(let reason), .unanswered(let reason):
                (self.repairs, self.repairsLoad) = (nil, .failed(reason))
            default:
                (self.repairs, self.repairsLoad) = (nil, .failed("unexpected answer"))
            }
            self.onChange?()
        }
    }
}

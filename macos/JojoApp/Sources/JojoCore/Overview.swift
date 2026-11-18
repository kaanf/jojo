import Foundation

// Protocol 1.6: user-facing data the core derives from what it records — names of
// features, tasks and documents, the live state of an attempt, what waits for the human.
// Nothing here is inferred: every field is the helper's answer.

/// A time the helper reports, in milliseconds since 1970.
public func helperDate(_ ms: Int64?) -> Date? {
    ms.map { Date(timeIntervalSince1970: TimeInterval($0) / 1000) }
}

/// The name a provider is shown by.
public func providerName(_ wire: String) -> String {
    wire == "ClaudeCode" ? "Claude Code" : wire
}

/// One attempt as `Attempts` reports it.
public struct AttemptSummary: Equatable, Sendable, Identifiable {
    public let id: String
    public let order: Int64
    public let step: String
    /// The step's key and title from its lead plan; nil for an imported plan.
    public let key: String?
    public let title: String?
    public let plan: String?
    public let feature: String?
    public let featureName: String?
    /// ClaudeCode or Codex, once a process was issued.
    public let provider: String?
    /// Reserved, Preparing, Launching, Running, Stopping, Settling, Unsupervised, Held,
    /// Succeeded, Failed, Cancelled or Interrupted.
    public let state: String
    public let started: Date?
    public let ended: Date?
    /// Protocol 1.7; nil from an older helper. The attempt's place among its step's
    /// attempts, from 1.
    public let ordinal: Int64?
    public let repositoryName: String?
    public let projectName: String?
    /// Its candidate's newest host validation; nil without a candidate.
    public let validation: AttemptValidation?
    /// What its newest provider record says the agent did, and when.
    public let activity: AttemptActivity?

    public init?(_ p: JSON) {
        guard let id = p["attempt"]?.string, let order = p["order"]?.integer, let step = p["step"]?.string,
              let state = p["state"]?.string else { return nil }
        self.init(id: id, order: order, step: step, key: p["key"]?.string, title: p["title"]?.string,
                  plan: p["plan"]?.string, feature: p["feature"]?.string, featureName: p["feature_name"]?.string,
                  provider: p["provider"]?.string, state: state, started: helperDate(p["started_at_unix_ms"]?.integer),
                  ended: helperDate(p["ended_at_unix_ms"]?.integer), ordinal: p["ordinal"]?.integer,
                  repositoryName: p["repository_name"]?.string, projectName: p["project_name"]?.string,
                  validation: p["validation"].flatMap(AttemptValidation.init),
                  activity: p["activity"].flatMap(AttemptActivity.init))
    }

    public init(id: String, order: Int64 = 0, step: String = "", key: String? = nil, title: String? = nil,
                plan: String? = nil, feature: String? = nil, featureName: String? = nil, provider: String? = nil,
                state: String, started: Date? = nil, ended: Date? = nil, ordinal: Int64? = nil,
                repositoryName: String? = nil, projectName: String? = nil, validation: AttemptValidation? = nil,
                activity: AttemptActivity? = nil) {
        (self.id, self.order, self.step, self.key, self.title, self.plan) = (id, order, step, key, title, plan)
        (self.feature, self.featureName, self.provider, self.state) = (feature, featureName, provider, state)
        (self.started, self.ended, self.ordinal) = (started, ended, ordinal)
        (self.repositoryName, self.projectName, self.validation, self.activity) =
            (repositoryName, projectName, validation, activity)
    }

    /// Unsealed and supervised by this helper.
    public var live: Bool {
        ["Reserved", "Preparing", "Launching", "Running", "Stopping", "Settling"].contains(state)
    }
}

/// Protocol 1.7: an attempt's candidate's newest host validation and the human's decision.
public struct AttemptValidation: Equatable, Sendable {
    /// NotRun, Unknown, Passed or Failed.
    public let state: String
    public let at: Date?
    /// Accepted or Rejected; nil while undecided.
    public let decision: String?

    public init?(_ p: JSON) {
        guard let state = p["state"]?.string else { return nil }
        self.init(state: state, at: helperDate(p["at_unix_ms"]?.integer), decision: p["decision"]?.string)
    }

    public init(state: String, at: Date? = nil, decision: String? = nil) {
        (self.state, self.at, self.decision) = (state, at, decision)
    }
}

/// Protocol 1.7: what an attempt's newest provider record says, in a few words.
public struct AttemptActivity: Equatable, Sendable {
    public let text: String
    public let at: Date?

    public init?(_ p: JSON) {
        guard let text = p["text"]?.string else { return nil }
        self.init(text: text, at: helperDate(p["at_unix_ms"]?.integer))
    }

    public init(text: String, at: Date? = nil) {
        (self.text, self.at) = (text, at)
    }
}

/// The answer of `Attempts`: newest first.
public struct AttemptList: Equatable, Sendable {
    public let attempts: [AttemptSummary]
    /// More attempts exist than were read.
    public let more: Bool

    public init?(_ p: [String: JSON]) {
        guard let items = p["attempts"]?.items else { return nil }
        let attempts = items.compactMap(AttemptSummary.init)
        guard attempts.count == items.count else { return nil }
        (self.attempts, self.more) = (attempts, p["next"]?.integer != nil)
    }

    public init(attempts: [AttemptSummary], more: Bool = false) {
        (self.attempts, self.more) = (attempts, more)
    }

    public subscript(attempt: String) -> AttemptSummary? { attempts.first { $0.id == attempt } }
}

/// One document as `Documents` reports it.
public struct DocumentSummary: Equatable, Sendable, Identifiable {
    public let id: String
    /// Prd, Adr, ValidationPlan or ExecutionPlan.
    public let kind: String
    public let revision: Int64
    /// A requirements document's own title; nil for the others.
    public let title: String?
    public let feature: String?
    public let featureName: String?
    /// Human or Model, for a requirements document.
    public let author: String?
    /// Pending, Approved, Rejected, Revoked, Waiting (an upstream document is not
    /// approved) or NotReady (a requirements document with findings).
    public let approval: String
    /// Pending again after a decision on an earlier revision.
    public let stale: Bool
    public let created: Date?
    public let decided: Date?

    public init?(_ p: JSON) {
        guard let id = p["document"]?.string, let kind = p["kind"]?.string, let revision = p["revision"]?.integer,
              let approval = p["approval"]?.string else { return nil }
        self.init(id: id, kind: kind, revision: revision, title: p["title"]?.string, feature: p["feature"]?.string,
                  featureName: p["feature_name"]?.string, author: p["author"]?.string, approval: approval,
                  stale: p["stale"]?.boolean ?? false, created: helperDate(p["created_at_unix_ms"]?.integer),
                  decided: helperDate(p["decided_at_unix_ms"]?.integer))
    }

    public init(id: String, kind: String, revision: Int64 = 1, title: String? = nil, feature: String? = nil,
                featureName: String? = nil, author: String? = nil, approval: String, stale: Bool = false,
                created: Date? = nil, decided: Date? = nil) {
        (self.id, self.kind, self.revision, self.title, self.feature) = (id, kind, revision, title, feature)
        (self.featureName, self.author, self.approval, self.stale) = (featureName, author, approval, stale)
        (self.created, self.decided) = (created, decided)
    }

    public var kindName: String {
        switch kind {
        case "Prd": return "Requirements"
        case "Adr": return "Architecture decision"
        case "ValidationPlan": return "Validation plan"
        case "ExecutionPlan": return "Execution plan"
        case "Brief": return "Project brief"
        default: return kind
        }
    }

    /// The document's own title, else its kind under its feature's name.
    public var name: String {
        if let title { return title }
        guard let featureName else { return kindName }
        return "\(featureName) · \(kindName)"
    }
}

/// A sealed candidate whose host validation passed and that the human has not decided.
public struct PendingCandidate: Equatable, Sendable, Identifiable {
    public let id: String
    public let key: String?
    public let title: String?
    public let featureName: String?
    public let receipt: Int64
    public let validated: Date?

    public init?(_ p: JSON) {
        guard let id = p["attempt"]?.string, let receipt = p["receipt"]?.integer else { return nil }
        self.init(id: id, key: p["key"]?.string, title: p["title"]?.string, featureName: p["feature_name"]?.string,
                  receipt: receipt, validated: helperDate(p["validated_at_unix_ms"]?.integer))
    }

    public init(id: String, key: String? = nil, title: String? = nil, featureName: String? = nil, receipt: Int64 = 0,
                validated: Date? = nil) {
        (self.id, self.key, self.title, self.featureName, self.receipt, self.validated) =
            (id, key, title, featureName, receipt, validated)
    }
}

/// The answer of `Documents`: documents newest first, the candidates that wait, the push
/// and pull request requests that wait (1.10), and how many things wait for the human.
public struct DocumentList: Equatable, Sendable {
    public let documents: [DocumentSummary]
    public let candidates: [PendingCandidate]
    public let external: [PendingExternal]
    public let pending: Int64
    public let more: Bool

    public init?(_ p: [String: JSON]) {
        guard let items = p["documents"]?.items, let waiting = p["candidates"]?.items,
              let pending = p["pending"]?.integer else { return nil }
        let documents = items.compactMap(DocumentSummary.init)
        let candidates = waiting.compactMap(PendingCandidate.init)
        let requests = p["external"]?.items ?? []
        let external = requests.compactMap(PendingExternal.init)
        guard documents.count == items.count, candidates.count == waiting.count,
              external.count == requests.count else { return nil }
        (self.documents, self.candidates, self.external, self.pending) = (documents, candidates, external, pending)
        more = p["next"]?.integer != nil
    }

    public init(documents: [DocumentSummary], candidates: [PendingCandidate] = [],
                external: [PendingExternal] = [], pending: Int64? = nil, more: Bool = false) {
        (self.documents, self.candidates, self.external, self.more) = (documents, candidates, external, more)
        self.pending = pending ?? Int64(documents.filter { $0.approval == "Pending" }.count + candidates.count
                                        + external.count)
    }

    public subscript(document: String) -> DocumentSummary? { documents.first { $0.id == document } }
    public var waiting: [DocumentSummary] { documents.filter { $0.approval == "Pending" } }
}

extension WorkspaceModel {
    /// Protocol 1.6: the newest attempts with their names and live state (1.7: and their
    /// details). Read with the repairs, only while Agent Runs or the Dashboard is shown,
    /// and again with each new snapshot.
    public func loadAttempts() {
        guard attemptsLoad != .loading, snapshotSeq != nil, repairsShown || dashboardShown || followingStep else { return }
        attemptsLoad = .loading
        onChange?()
        client.query(.Attempts, ["before": .null]) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                if let list = AttemptList(p) {
                    (self.attempts, self.attemptsLoad) = (list, .loaded)
                } else {
                    (self.attempts, self.attemptsLoad) = (nil, .failed("malformed attempts"))
                }
            case .error(let reason), .unanswered(let reason):
                (self.attempts, self.attemptsLoad) = (nil, .failed(reason))
            default:
                (self.attempts, self.attemptsLoad) = (nil, .failed("unexpected answer"))
            }
            self.onChange?()
        }
    }

    /// Protocol 1.6: the newest documents, the candidates that wait and the number of
    /// things that wait for the human. Read with every snapshot, so the sidebar counter
    /// follows the journal on every screen.
    public func loadDocuments() {
        guard documentsLoad != .loading else { return }
        documentsLoad = .loading
        client.query(.Documents, ["before": .null]) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                if let list = DocumentList(p) {
                    (self.documents, self.documentsLoad) = (list, .loaded)
                } else {
                    (self.documents, self.documentsLoad) = (nil, .failed("malformed documents"))
                }
            case .error(let reason), .unanswered(let reason):
                (self.documents, self.documentsLoad) = (nil, .failed(reason))
            default:
                (self.documents, self.documentsLoad) = (nil, .failed("unexpected answer"))
            }
            self.onChange?()
        }
    }
}

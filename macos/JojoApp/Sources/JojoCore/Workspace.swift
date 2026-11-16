import Foundation

/// U02: closing the last primary window quits (through the coordinator); auxiliary
/// windows close independently. Minimizing or hiding is not closure.
public enum WindowRole: String, Sendable {
    case primary
    case auxiliary
}

public enum WindowCloseDecision: Equatable, Sendable {
    case close
    case routeToQuit
}

public enum WindowPolicy {
    public static func onClose(_ role: WindowRole) -> WindowCloseDecision {
        role == .primary ? .routeToQuit : .close
    }
}

/// The exact P1-04 review DTO as returned by the helper. A decision binds all of it.
public struct ReviewBundle: Equatable, Sendable {
    public let subject: String
    public let revision: Int64
    public let hash: String
    public let contentHash: String
    public let displayedHash: String
    public let contextHash: String
    public let upstream: [Int64]
    public let priorDecision: Int64?

    public init?(_ p: [String: JSON]) {
        guard let subject = p["subject"]?.string, Identity.valid(subject),
              let revision = p["revision"]?.integer,
              let hash = p["hash"]?.string, let contentHash = p["content_hash"]?.string,
              let displayedHash = p["displayed_hash"]?.string, let contextHash = p["context_hash"]?.string,
              let upstreamItems = p["upstream"]?.items, let prior = p["prior_decision"] else { return nil }
        let upstream = upstreamItems.compactMap(\.integer)
        guard upstream.count == upstreamItems.count, prior.isNull || prior.integer != nil else { return nil }
        self.subject = subject
        self.revision = revision
        self.hash = hash
        self.contentHash = contentHash
        self.displayedHash = displayedHash
        self.contextHash = contextHash
        self.upstream = upstream
        self.priorDecision = prior.integer
    }

    func decision(_ action: DecisionAction) -> [String: JSON] {
        [
            "subject": .text(subject), "hash": .text(hash), "content_hash": .text(contentHash),
            "displayed_hash": .text(displayedHash), "context_hash": .text(contextHash),
            "upstream": .list(upstream.map(JSON.int)), "prior_decision": priorDecision.map(JSON.int) ?? .null,
            "action": .text(action.rawValue),
        ]
    }
}

public enum DecisionAction: String, CaseIterable, Sendable {
    case Approve, Reject, Revoke
}

public struct SnapshotRow: Equatable, Sendable, Identifiable {
    public let key: String
    public let value: String
    public let detailComplete: Bool
    public var id: String { key }
    public var kind: String { String(key.split(separator: ":", maxSplits: 1).first ?? "") }
    public var subject: String { String(key.split(separator: ":", maxSplits: 1).last ?? "") }
}

public struct RecoveryRow: Equatable, Sendable, Identifiable {
    public let attempt: String
    public let assessment: String
    /// Worktree path, durable offsets and process findings (evidence locations).
    public let findings: String
    public let detailComplete: Bool
    /// The assessment as structured fields (protocol 1.1); nil from an older helper.
    public let structured: RecoveryAssessment?
    public var id: String { attempt }

    public init(attempt: String, assessment: String, findings: String, detailComplete: Bool,
                structured: RecoveryAssessment? = nil) {
        (self.attempt, self.assessment, self.findings) = (attempt, assessment, findings)
        (self.detailComplete, self.structured) = (detailComplete, structured)
    }
}

/// A reviewed recovery choice as protocol 1.1 names it. Which of them is permitted for a
/// subject is decided by the helper from OS and repository evidence, never by the GUI.
public enum RecoveryChoice: String, CaseIterable, Sendable {
    case ValidateOutput, ContinueWithHistory, ContinueFreshContext, Hold

    public var label: String { recoveryChoices[Self.allCases.firstIndex(of: self)!] }
}

/// The five identities a Start names. Nothing else can be sent: no executable, argument
/// or environment value exists in this type or in the command.
public struct StartSubjects: Equatable, Sendable {
    public var prd = "", adr = "", plan = "", validation = "", step = ""
    public init() {}
    public init(prd: String, adr: String, plan: String, validation: String, step: String) {
        (self.prd, self.adr, self.plan, self.validation, self.step) = (prd, adr, plan, validation, step)
    }
    var payload: [String: JSON]? {
        let all = [prd, adr, plan, validation, step]
        guard all.allSatisfy(Identity.valid) else { return nil }
        return ["prd": .text(prd), "adr": .text(adr), "plan": .text(plan), "validation": .text(validation),
                "step": .text(step)]
    }
}

/// Reviewed recovery choices (SHUTDOWN_RECOVERY), in `RecoveryChoice` order. Sent only by
/// an explicit click, bound to the digest of the inspection on screen.
public let recoveryChoices = [
    "Validate existing output (no new inference)",
    "Continue with supported history (new attempt and worktree)",
    "Continue in fresh context (new attempt and worktree)",
    "Keep on hold / cancel task (state, worktree and evidence retained)",
]

/// Read-mostly projection of helper state. It never owns or writes the database; every
/// mutation is a helper command whose acknowledgement is not completion.
@MainActor
public final class WorkspaceModel {
    public private(set) var rows: [SnapshotRow] = []
    public private(set) var snapshotSeq: Int64?
    public private(set) var admission = ""
    public private(set) var recovery: [RecoveryRow] = []
    public private(set) var recoveryDigest = ""
    public private(set) var review: ReviewBundle?
    public private(set) var reviewStale = false
    public private(set) var status = ""
    /// A Start or recovery command is unanswered; the same control cannot send again.
    public private(set) var commandPending = false
    /// P2-03 candidate review. Every field is the helper's answer; nothing is inferred here.
    public private(set) var candidate: CandidateReview?
    public private(set) var changedFiles: [ChangedFile] = []
    /// Write-scope verdict of the changed paths, with the reason when it is unknown.
    public private(set) var scope = ValidationVerdict.notRun
    public private(set) var scopeDetail = ""
    public private(set) var diff = ""
    public private(set) var agentLog: [String] = []
    public private(set) var validationReport = ""
    private var candidateLoad = 0
    public var onChange: (() -> Void)?

    private let client: HelperClient
    private var loading = false
    static let maxPages = 32

    public init(client: HelperClient) {
        self.client = client
    }

    public func refresh() {
        guard !loading else { return }
        loading = true
        loadSnapshot(after: "", cursor: nil, collected: [], attempt: 0)
        loadRecovery(offset: 0, collected: [])
        if let shown = candidate?.attempt { loadCandidate(shown) }
    }

    /// Snapshot pages share one committed cursor; SnapshotChanged discards and reloads.
    private func loadSnapshot(after: String, cursor: Int64?, collected: [SnapshotRow], attempt: Int) {
        client.query(.Snapshot, ["section": .text("aggregates"), "after": .text(after),
                                  "cursor": cursor.map(JSON.int) ?? .null, "limit": .int(8)]) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                let page = (p["rows"]?.items ?? []).compactMap { row -> SnapshotRow? in
                    guard let k = row["key"]?.string, let v = row["value"]?.string else { return nil }
                    return SnapshotRow(key: k, value: v, detailComplete: row["detail_complete"] == .bool(true))
                }
                let all = collected + page
                let seq = p["as_of_event_seq"]?.integer
                if let next = p["next"]?.string, page.count == 8, all.count < Self.maxPages * 8 {
                    return self.loadSnapshot(after: next, cursor: seq, collected: all, attempt: attempt)
                }
                self.rows = all
                self.snapshotSeq = seq
                self.admission = p["admission"]?.string ?? ""
                self.loading = false
            case .error(let code) where code.contains("SnapshotChanged") && attempt < 3:
                return self.loadSnapshot(after: "", cursor: nil, collected: [], attempt: attempt + 1)
            default:
                self.status = "Snapshot unavailable: \(reply)"
                self.loading = false
            }
            self.onChange?()
        }
    }

    private func loadRecovery(offset: Int, collected: [RecoveryRow]) {
        client.query(.Recovery, ["offset": .int(Int64(offset)), "limit": .int(8)]) { [weak self] reply in
            guard let self, case .response(let p) = reply else { return }
            let page = (p["rows"]?.items ?? []).compactMap { r -> RecoveryRow? in
                guard let a = r["attempt"]?.string, let s = r["assessment"]?.string, let f = r["findings"]?.string else { return nil }
                return RecoveryRow(attempt: a, assessment: s, findings: f, detailComplete: r["detail_complete"] == .bool(true),
                                   structured: RecoveryAssessment(row: r))
            }
            if page.count == 8, collected.count < 256 {
                return self.loadRecovery(offset: offset + 8, collected: collected + page)
            }
            self.recovery = collected + page
            self.recoveryDigest = p["digest"]?.string ?? ""
            self.onChange?()
        }
    }

    /// Committed events after the snapshot cursor; any new event reloads the projection.
    public func pollEvents() {
        guard let seq = snapshotSeq, !loading else { return }
        client.query(.Events, ["after": .int(seq), "limit": .int(1)]) { [weak self] reply in
            if case .list(let rows) = reply, !rows.isEmpty { self?.refresh() }
        }
    }

    public func loadReview(_ subject: String) {
        guard Identity.valid(subject) else {
            status = "Review subject must be a 32-character lowercase hex identity"
            onChange?()
            return
        }
        client.query(.ReviewBundle, ["subject": .text(subject)]) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                self.review = ReviewBundle(p)
                self.reviewStale = false
                self.status = self.review == nil ? "Malformed review bundle" : "Review loaded at revision \(self.review!.revision)"
            case .null:
                self.review = nil
                self.status = "No reviewable artifact \(subject)"
            default:
                self.status = "Review unavailable: \(reply)"
            }
            self.onChange?()
        }
    }

    /// Sends exactly the displayed bundle. A stale rejection disables decisions until reload;
    /// an unanswered decision stays outstanding for receipt reconciliation, never resent.
    public func decide(_ action: DecisionAction) {
        guard let bundle = review, !reviewStale else { return }
        reviewStale = true
        status = "\(action.rawValue) sent; awaiting acknowledgement"
        onChange?()
        client.command(.HumanDecision, bundle.decision(action), revision: bundle.revision) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                self.status = "\(action.rawValue) committed at event \(p["event_seq"]?.integer ?? 0)\(p["replayed"] == .bool(true) ? " (replayed receipt)" : ""); reload to review again"
            case .error(let code):
                self.status = "Decision rejected (\(code)); the review may be stale. Reload before deciding; nothing was resent."
            case .null, .list, .unanswered:
                self.status = "Decision outcome unknown; it will be reconciled by receipt, not resent"
            }
            self.onChange?()
        }
    }

    /// A fresh helper inspection; resolution and the reviewed start bind its digest.
    public func inspectRecovery() {
        client.query(.RecoveryInspect) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                self.status = "Inspection refreshed: \(p["subjects"]?.integer ?? 0) subject(s) need a decision"
                self.loadRecovery(offset: 0, collected: [])
            case .error(let code):
                self.status = "Inspection unavailable (\(code))"
            default:
                self.status = "Inspection unavailable: \(reply)"
            }
            self.onChange?()
        }
    }

    /// One explicit choice for one inspected subject. The helper refuses any choice the
    /// evidence does not offer; a refusal changes nothing and is never retried here.
    public func resolveRecovery(_ attempt: String, _ choice: RecoveryChoice) {
        guard Identity.valid(attempt), !recoveryDigest.isEmpty else { return }
        slice(.RecoveryResolve, ["digest": .text(recoveryDigest), "attempt": .text(attempt),
                                  "choice": .text(choice.rawValue)],
              sent: "Recovery choice \(choice.rawValue) sent for \(attempt)") { p in
            "Recovery choice \(choice.rawValue) committed for \(attempt): \(p["response"]?.string ?? "")"
        } refused: { "Recovery choice refused (\($0)); nothing changed. Inspect again before deciding." }
    }

    /// The human's start after reviewing the inspection on screen: opens admission only.
    public func startAfterRecovery() {
        guard !recoveryDigest.isEmpty else { return }
        slice(.RecoveryStart, ["digest": .text(recoveryDigest)], sent: "Reviewed start sent") { _ in
            "Admission open after reviewed recovery; nothing was started"
        } refused: { "Reviewed start refused (\($0)); admission is unchanged" }
    }

    /// One human Start for one approved step. Acknowledgement is not completion.
    public func start(_ subjects: StartSubjects) {
        guard let payload = subjects.payload else {
            status = "Start needs five 32-character lowercase hex identities"
            onChange?()
            return
        }
        slice(.Start, payload, sent: "Start sent; awaiting acknowledgement") { p in
            "Start accepted for attempt \(p["attempt"]?.string ?? "?") (\(p["dispatch"]?.string ?? "?")); follow the run in Runs"
        } refused: { "Start refused (\($0)); nothing was started" }
    }

    private func slice(_ type: Request, _ payload: [String: JSON], sent: String,
                       accepted: @escaping ([String: JSON]) -> String, refused: @escaping (String) -> String) {
        guard !commandPending else {
            status = "A command is still unanswered; it will be reconciled by receipt, not sent again"
            onChange?()
            return
        }
        commandPending = true
        status = sent
        onChange?()
        client.command(type, payload) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                self.commandPending = false
                self.status = accepted(p)
                self.refresh()
                if type == .RecoveryResolve || type == .RecoveryStart { self.inspectRecoveryQuietly() }
            case .error(let code):
                self.commandPending = false
                self.status = refused(code)
            case .null, .list, .unanswered:
                // Stays pending: an unknown outcome is reconciled by receipt, never resent.
                self.status = "\(type.rawValue) outcome unknown; it will be reconciled by receipt, not resent"
            }
            self.onChange?()
        }
    }

    private func inspectRecoveryQuietly() {
        client.query(.RecoveryInspect) { [weak self] reply in
            if case .response = reply { self?.loadRecovery(offset: 0, collected: []) }
        }
    }

    public func cancelAttempt(_ attempt: String) {
        guard Identity.valid(attempt) else { return }
        client.command(.CancelAttempt, ["attempt": .text(attempt)]) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response:
                self.status = "Cancel committed for \(attempt); completion requires a sealed result"
                self.refresh()
            case .error(let code):
                self.status = "Cancel rejected: \(code)"
            case .null, .list, .unanswered:
                self.status = "Cancel outcome unknown; it will be reconciled by receipt"
            }
            self.onChange?()
        }
    }
}

/// P2-03: the four validation states, shown distinctly. Only `pass` is a success, and it
/// exists only as the helper's recorded host validation: provider output is never read
/// to produce it, and any state this GUI does not recognize is `unknown`.
public enum ValidationVerdict: String, Sendable {
    case pass = "PASS", fail = "FAIL", unknown = "UNKNOWN", notRun = "NOT_RUN"

    public init(helper: String?) {
        self = helper.flatMap(ValidationVerdict.init(rawValue:)) ?? .unknown
    }

    public var label: String { self == .notRun ? "NOT RUN" : rawValue }
}

public struct ChangedFile: Equatable, Sendable, Identifiable {
    public let path: String
    public let kind: String
    /// ALLOWED, OUT_OF_SCOPE, BASELINE or UNKNOWN, as decided by the helper.
    public let scope: String
    public var id: String { path }
}

public enum CandidateAction: String, Sendable {
    case Accept, Reject
}

/// The helper's review projection of one attempt (`Candidate` query).
public struct CandidateReview: Equatable, Sendable {
    public let attempt: String
    public let sealed: Bool
    public let outcome: String
    public let reason: String
    /// What the provider said about itself. Displayed as such; never a validation state.
    public let providerTerminal: String
    public let candidate: String?
    public let logRecords: Int64
    public let validation: ValidationVerdict
    public let validationDetail: String
    public let receipt: Int64?
    public let validatedCandidate: String?
    public let reasons: [String]
    public let decision: String

    public init?(_ p: [String: JSON]) {
        guard let attempt = p["attempt"]?.string, Identity.valid(attempt), let sealed = p["sealed"]?.boolean else { return nil }
        let v = p["validation"]
        self.attempt = attempt
        self.sealed = sealed
        outcome = p["outcome"]?.string ?? ""
        reason = p["reason"]?.string ?? ""
        providerTerminal = p["provider_terminal"]?.string ?? ""
        candidate = p["candidate"]?.string
        logRecords = p["log_records"]?.integer ?? 0
        // No sealed result: nothing was validated. Sealed but unreadable: unknown.
        validation = sealed ? ValidationVerdict(helper: v?["state"]?.string) : .notRun
        validationDetail = v?["detail"]?.string ?? ""
        receipt = v?["receipt"]?.integer
        validatedCandidate = v?["candidate"]?.string
        reasons = (v?["reasons"]?.items ?? []).compactMap(\.string)
        decision = p["decision"]?.string ?? "UNKNOWN"
    }

    /// Accept is offered only for a passing host validation of exactly this candidate.
    /// The helper re-checks all of it and approval freshness; this only gates the button.
    public var canAccept: Bool {
        sealed && validation == .pass && receipt != nil && candidate != nil && validatedCandidate == candidate
            && decision == "NONE"
    }
}

extension WorkspaceModel {
    /// Loads the review projection, then the repository-derived detail it points at.
    public func loadCandidate(_ attempt: String) {
        guard Identity.valid(attempt) else {
            status = "Attempt must be a 32-character lowercase hex identity"
            onChange?()
            return
        }
        candidateLoad += 1
        let load = candidateLoad
        client.query(.Candidate, ["attempt": .text(attempt)]) { [weak self] reply in
            guard let self, load == self.candidateLoad else { return }
            guard case .response(let p) = reply, let review = CandidateReview(p) else {
                self.candidate = nil
                self.status = "Candidate unavailable: \(reply)"
                self.onChange?()
                return
            }
            let changed = self.candidate?.candidate != review.candidate
            self.candidate = review
            if changed { (self.changedFiles, self.diff, self.agentLog, self.validationReport) = ([], "", [], "") }
            self.scope = .notRun
            self.scopeDetail = review.candidate == nil ? "no sealed candidate" : ""
            self.onChange?()
            if let digest = review.candidate {
                self.loadChanges(load, attempt, digest, offset: 0, collected: [])
                self.loadText(.CandidateDiff, ["attempt": .text(attempt), "candidate": .text(digest)], load, offset: 0, collected: "") {
                    self.diff = $0
                }
            }
            if let receipt = review.receipt {
                self.loadText(.ValidationReport, ["attempt": .text(attempt), "receipt": .int(receipt)], load, offset: 0, collected: "") {
                    self.validationReport = $0
                }
            } else {
                self.validationReport = ""
            }
            self.loadLog(load, attempt, sequence: 1, last: min(review.logRecords, 64), collected: [])
        }
    }

    private func loadChanges(_ load: Int, _ attempt: String, _ digest: String, offset: Int64, collected: [ChangedFile]) {
        client.query(.CandidateChanges, ["attempt": .text(attempt), "candidate": .text(digest), "offset": .int(offset),
                                          "limit": .int(8)]) { [weak self] reply in
            guard let self, load == self.candidateLoad else { return }
            switch reply {
            case .response(let p):
                let page = (p["rows"]?.items ?? []).compactMap { r -> ChangedFile? in
                    guard let path = r["path"]?.string, let kind = r["kind"]?.string else { return nil }
                    return ChangedFile(path: path, kind: kind, scope: r["scope"]?.string ?? "UNKNOWN")
                }
                let all = collected + page
                if page.count == 8, all.count < 256 {
                    return self.loadChanges(load, attempt, digest, offset: offset + 8, collected: all)
                }
                self.changedFiles = all
                self.scope = ValidationVerdict(helper: p["scope"]?.string)
                self.scopeDetail = ""
            case .error(let code):
                // The candidate could not be inspected (for example it was tampered with).
                self.changedFiles = []
                self.scope = .unknown
                self.scopeDetail = code
            default:
                self.scope = .unknown
                self.scopeDetail = "changed paths unavailable"
            }
            self.onChange?()
        }
    }

    private func loadText(_ type: Request, _ payload: [String: JSON], _ load: Int, offset: Int64, collected: String,
                          _ done: @escaping (String) -> Void) {
        var page = payload
        page["offset"] = .int(offset)
        client.query(type, page) { [weak self] reply in
            guard let self, load == self.candidateLoad else { return }
            guard case .response(let p) = reply, let text = p["text"]?.string, let next = p["next"]?.integer,
                  let total = p["total"]?.integer else {
                if case .error(let code) = reply { done("(unavailable: \(code))") } else { done(collected) }
                self.onChange?()
                return
            }
            let all = collected + text
            if next < total, next > offset, next < 256 * 1024 {
                return self.loadText(type, payload, load, offset: next, collected: all, done)
            }
            done(all + (next < total || p["complete"] == .bool(false) ? "\n… (truncated)" : ""))
            self.onChange?()
        }
    }

    private func loadLog(_ load: Int, _ attempt: String, sequence: Int64, last: Int64, collected: [String]) {
        guard sequence <= last else {
            agentLog = collected
            onChange?()
            return
        }
        client.query(.Log, ["attempt": .text(attempt), "sequence": .int(sequence), "offset": .int(0)]) { [weak self] reply in
            guard let self, load == self.candidateLoad else { return }
            var all = collected
            if case .response(let p) = reply, let text = p["text"]?.string {
                all.append("\(p["type"]?.string ?? "?"): \(text)")
            }
            self.loadLog(load, attempt, sequence: sequence + 1, last: last, collected: all)
        }
    }

    /// Asks the helper to validate the displayed sealed candidate. The command names the
    /// attempt and the digest only: what runs is the approved plan's command.
    public func validateCandidate() {
        guard let c = candidate, let digest = c.candidate else {
            status = "Nothing to validate: load an attempt with a sealed candidate"
            onChange?()
            return
        }
        slice(.Validate, ["attempt": .text(c.attempt), "candidate": .text(digest)],
              sent: "Validation requested; awaiting acknowledgement") { p in
            "Validation \(p["validation"]?.string ?? "requested") for \(c.attempt); the result is UNKNOWN until the helper records it"
        } refused: { "Validation refused (\($0)); nothing was run" }
    }

    /// The human's decision on the displayed candidate, bound to its digest and receipt.
    public func decideCandidate(_ action: CandidateAction) {
        guard let c = candidate, let digest = c.candidate, c.decision == "NONE" else {
            status = "No undecided sealed candidate is loaded"
            onChange?()
            return
        }
        if action == .Accept, !c.canAccept {
            status = "Accept is unavailable: validation is \(c.validation.label). Only a passing host validation of this exact candidate can be accepted."
            onChange?()
            return
        }
        let receipt: JSON = action == .Accept ? .int(c.receipt ?? 0) : .null
        slice(.CandidateDecision, ["attempt": .text(c.attempt), "candidate": .text(digest), "receipt": receipt,
                                    "action": .text(action.rawValue)],
              sent: "\(action.rawValue) sent; awaiting acknowledgement") { p in
            "Candidate \(action == .Accept ? "accepted" : "rejected") (\(p["response"]?.string ?? "")); nothing was merged or pushed"
        } refused: { "\(action.rawValue) refused (\($0)); the candidate is unchanged" }
    }
}

import Foundation

// Protocol 1.9: the details of the rebuilt screens — one document with its revisions,
// text and decisions; the newest decisions; one plan step by step; one attempt beyond
// `Attempts`. Every field is the helper's answer; nothing here is inferred.

/// One revision of a document as `Document` lists it.
public struct DocumentRevision: Equatable, Sendable, Identifiable {
    public let revision: Int64
    public let created: Date?
    public let digest: String
    /// Human or Model for a requirements document; nil for the others.
    public let author: String?
    /// The latest decision recorded on this revision: Approved, Rejected or Revoked.
    public let decision: String?
    public let decided: Date?
    /// A newer revision exists.
    public let superseded: Bool
    public var id: Int64 { revision }

    public init?(_ p: JSON) {
        guard let revision = p["revision"]?.integer, let digest = p["digest"]?.string,
              let superseded = p["superseded"]?.boolean else { return nil }
        self.init(revision: revision, created: helperDate(p["created_at_unix_ms"]?.integer), digest: digest,
                  author: p["author"]?.string, decision: p["decision"]?.string,
                  decided: helperDate(p["decided_at_unix_ms"]?.integer), superseded: superseded)
    }

    public init(revision: Int64, created: Date? = nil, digest: String = "", author: String? = nil,
                decision: String? = nil, decided: Date? = nil, superseded: Bool = false) {
        (self.revision, self.created, self.digest, self.author) = (revision, created, digest, author)
        (self.decision, self.decided, self.superseded) = (decision, decided, superseded)
    }
}

/// One decision on a document: on which revision, which, when, and whether it binds the
/// document's current content.
public struct DocumentDecision: Equatable, Sendable {
    public let revision: Int64
    /// Approved, Rejected or Revoked.
    public let decision: String
    public let at: Date?
    public let digest: String
    public let applies: Bool

    public init?(_ p: JSON) {
        guard let revision = p["revision"]?.integer, let decision = p["decision"]?.string,
              let applies = p["applies"]?.boolean else { return nil }
        self.init(revision: revision, decision: decision, at: helperDate(p["at_unix_ms"]?.integer),
                  digest: p["digest"]?.string ?? "", applies: applies)
    }

    public init(revision: Int64, decision: String, at: Date? = nil, digest: String = "", applies: Bool = false) {
        (self.revision, self.decision, self.at, self.digest, self.applies) = (revision, decision, at, digest, applies)
    }
}

/// A document another one was drafted on, or that was drafted on it.
public struct DocumentLink: Equatable, Sendable, Identifiable {
    public let id: String
    public let kind: String
    public let revision: Int64?
    public let title: String?
    public let approval: String

    public init?(_ p: JSON) {
        guard let id = p["document"]?.string, let kind = p["kind"]?.string,
              let approval = p["approval"]?.string else { return nil }
        self.init(id: id, kind: kind, revision: p["revision"]?.integer, title: p["title"]?.string, approval: approval)
    }

    public init(id: String, kind: String, revision: Int64? = nil, title: String? = nil, approval: String = "Pending") {
        (self.id, self.kind, self.revision, self.title, self.approval) = (id, kind, revision, title, approval)
    }
}

/// A requirements document's acceptance criterion and the use cases that name it.
public struct DocumentCriterion: Equatable, Sendable, Identifiable {
    public let id: String
    public let statement: String
    public let useCases: [String]

    public init?(_ p: JSON) {
        guard let id = p["id"]?.string, let statement = p["statement"]?.string else { return nil }
        self.init(id: id, statement: statement, useCases: (p["use_cases"]?.items ?? []).compactMap(\.string))
    }

    public init(id: String, statement: String, useCases: [String] = []) {
        (self.id, self.statement, self.useCases) = (id, statement, useCases)
    }
}

/// A structural reason a requirements document is not ready for review.
public struct DocumentFinding: Equatable, Sendable {
    public let path: String
    /// Empty, Placeholder, UnknownReference or OpenBlocker.
    public let problem: String

    public init(path: String, problem: String) { (self.path, self.problem) = (path, problem) }
}

/// The answer of `Document`, with its text read to the end.
public struct DocumentDetail: Equatable, Sendable {
    public let id: String
    public let kind: String
    public let title: String?
    public let feature: String?
    public let featureName: String?
    public let revision: Int64
    public let newest: Int64
    public let approval: String
    public let stale: Bool
    public let revisions: [DocumentRevision]
    public let history: [DocumentDecision]
    public let upstream: [DocumentLink]
    public let downstream: [DocumentLink]
    /// For a requirements document: how many entries each section has.
    public let counts: [String: Int64]?
    public let findings: [DocumentFinding]?
    public let criteria: [DocumentCriterion]?
    public var text: String
    /// The whole text was read.
    public var complete: Bool

    public init?(_ p: [String: JSON]) {
        guard let id = p["document"]?.string, let kind = p["kind"]?.string, let revision = p["revision"]?.integer,
              let newest = p["newest"]?.integer, let approval = p["approval"]?.string,
              let revisions = p["revisions"]?.items, let history = p["history"]?.items,
              let text = p["text"]?.string, let complete = p["complete"]?.boolean else { return nil }
        var counts: [String: Int64]?
        if case .object(let fields)? = p["counts"] {
            counts = fields.compactMapValues(\.integer)
        }
        self.init(id: id, kind: kind, title: p["title"]?.string, feature: p["feature"]?.string,
                  featureName: p["feature_name"]?.string, revision: revision, newest: newest, approval: approval,
                  stale: p["stale"]?.boolean ?? false, revisions: revisions.compactMap(DocumentRevision.init),
                  history: history.compactMap(DocumentDecision.init),
                  upstream: (p["upstream"]?.items ?? []).compactMap(DocumentLink.init),
                  downstream: (p["downstream"]?.items ?? []).compactMap(DocumentLink.init), counts: counts,
                  findings: p["findings"]?.items.map { $0.compactMap { f in
                      f["path"]?.string.flatMap { path in f["problem"]?.string.map { DocumentFinding(path: path, problem: $0) } }
                  } },
                  criteria: p["criteria"]?.items.map { $0.compactMap(DocumentCriterion.init) }, text: text,
                  complete: complete)
    }

    public init(id: String, kind: String, title: String? = nil, feature: String? = nil, featureName: String? = nil,
                revision: Int64 = 1, newest: Int64? = nil, approval: String = "Pending", stale: Bool = false,
                revisions: [DocumentRevision] = [], history: [DocumentDecision] = [], upstream: [DocumentLink] = [],
                downstream: [DocumentLink] = [], counts: [String: Int64]? = nil, findings: [DocumentFinding]? = nil,
                criteria: [DocumentCriterion]? = nil, text: String = "", complete: Bool = true) {
        (self.id, self.kind, self.title, self.feature, self.featureName) = (id, kind, title, feature, featureName)
        (self.revision, self.newest, self.approval, self.stale) = (revision, newest ?? revision, approval, stale)
        (self.revisions, self.history, self.upstream, self.downstream) = (revisions, history, upstream, downstream)
        (self.counts, self.findings, self.criteria, self.text, self.complete) = (counts, findings, criteria, text, complete)
    }

    /// The revision before the described one, if the core lists it.
    public var previous: DocumentRevision? {
        revisions.filter { $0.revision < revision }.max { $0.revision < $1.revision }
    }
}

/// One decision of `Decisions`.
public struct DecisionSummary: Equatable, Sendable, Identifiable {
    public let order: Int64
    public let document: String
    public let kind: String
    public let title: String?
    public let featureName: String?
    public let revision: Int64
    public let decision: String
    public let applies: Bool
    public let at: Date?
    public var id: Int64 { order }

    public init?(_ p: JSON) {
        guard let order = p["order"]?.integer, let document = p["document"]?.string, let kind = p["kind"]?.string,
              let revision = p["revision"]?.integer, let decision = p["decision"]?.string,
              let applies = p["applies"]?.boolean else { return nil }
        self.init(order: order, document: document, kind: kind, title: p["title"]?.string,
                  featureName: p["feature_name"]?.string, revision: revision, decision: decision, applies: applies,
                  at: helperDate(p["at_unix_ms"]?.integer))
    }

    public init(order: Int64, document: String, kind: String, title: String? = nil, featureName: String? = nil,
                revision: Int64, decision: String, applies: Bool = true, at: Date? = nil) {
        (self.order, self.document, self.kind, self.title, self.featureName) = (order, document, kind, title, featureName)
        (self.revision, self.decision, self.applies, self.at) = (revision, decision, applies, at)
    }

    /// The document's name as the Approval Inbox shows it.
    public var name: String {
        DocumentSummary(id: document, kind: kind, revision: revision, title: title, featureName: featureName, approval: decision).name
    }
}

/// One attempt of a plan step as `PlanDetail` lists it.
public struct StepAttempt: Equatable, Sendable, Identifiable {
    public let id: String
    public let ordinal: Int64
    /// Succeeded, Failed, Cancelled or Interrupted; nil while live.
    public let outcome: String?
    public let started: Date?
    public let ended: Date?
    /// NotRun, Unknown, Passed or Failed; nil without a candidate.
    public let validation: String?
    public let accepted: Bool

    public init?(_ p: JSON) {
        guard let id = p["attempt"]?.string, let ordinal = p["ordinal"]?.integer else { return nil }
        self.init(id: id, ordinal: ordinal, outcome: p["outcome"]?.string,
                  started: helperDate(p["started_at_unix_ms"]?.integer), ended: helperDate(p["ended_at_unix_ms"]?.integer),
                  validation: p["validation"]?.string, accepted: p["accepted"]?.boolean ?? false)
    }

    public init(id: String, ordinal: Int64, outcome: String? = nil, started: Date? = nil, ended: Date? = nil,
                validation: String? = nil, accepted: Bool = false) {
        (self.id, self.ordinal, self.outcome, self.started, self.ended) = (id, ordinal, outcome, started, ended)
        (self.validation, self.accepted) = (validation, accepted)
    }
}

/// What blocks a step: Approval, Question, Dependency or Step, and why.
public struct StepBlocker: Equatable, Sendable {
    public let kind: String
    public let reason: String

    public init(kind: String, reason: String) { (self.kind, self.reason) = (kind, reason) }
}

/// One step of `PlanDetail`.
public struct PlanStepDetail: Equatable, Sendable, Identifiable {
    public let id: String
    public let key: String?
    public let title: String?
    public let kind: String?
    public let provider: String?
    public let dependsOn: [String]
    public let wave: Int64?
    public let writeScope: [String]
    public let baseline: [String]
    public let tests: [String]
    public let command: [String]
    public let state: String
    public let detail: String
    public let eligible: Bool
    public let attempts: [StepAttempt]
    public let blockers: [StepBlocker]

    public init?(_ p: JSON) {
        guard let id = p["step"]?.string, let state = p["state"]?.string else { return nil }
        let strings = { (name: String) in (p[name]?.items ?? []).compactMap(\.string) }
        self.init(id: id, key: p["key"]?.string, title: p["title"]?.string, kind: p["kind"]?.string,
                  provider: p["provider"]?.string, dependsOn: strings("depends_on"), wave: p["wave"]?.integer,
                  writeScope: strings("write_scope"), baseline: strings("baseline"), tests: strings("tests"),
                  command: strings("command"), state: state, detail: p["detail"]?.string ?? "",
                  eligible: p["eligible"]?.boolean ?? false,
                  attempts: (p["attempts"]?.items ?? []).compactMap(StepAttempt.init),
                  blockers: (p["blockers"]?.items ?? []).compactMap { b in
                      b["kind"]?.string.flatMap { k in b["reason"]?.string.map { StepBlocker(kind: k, reason: $0) } }
                  })
    }

    public init(id: String, key: String? = nil, title: String? = nil, kind: String? = nil, provider: String? = nil,
                dependsOn: [String] = [], wave: Int64? = 1, writeScope: [String] = [], baseline: [String] = [],
                tests: [String] = [], command: [String] = [], state: String = "Waiting", detail: String = "",
                eligible: Bool = false, attempts: [StepAttempt] = [], blockers: [StepBlocker] = []) {
        (self.id, self.key, self.title, self.kind, self.provider) = (id, key, title, kind, provider)
        (self.dependsOn, self.wave, self.writeScope, self.baseline) = (dependsOn, wave, writeScope, baseline)
        (self.tests, self.command, self.state, self.detail) = (tests, command, state, detail)
        (self.eligible, self.attempts, self.blockers) = (eligible, attempts, blockers)
    }

    /// The step's key, else its place.
    public var label: String { key ?? "Step" }
}

/// Two steps whose write scopes overlap.
public struct PlanConflict: Equatable, Sendable {
    public let steps: [String]
    public let paths: [String]

    public init(steps: [String], paths: [String]) { (self.steps, self.paths) = (steps, paths) }
}

/// The answer of `PlanDetail`.
public struct PlanDetail: Equatable, Sendable {
    public let plan: String
    public let revision: Int64
    public let approval: String
    public let featureName: String?
    /// The newest run's state, nil without a run.
    public let run: String?
    public let steps: [PlanStepDetail]
    public let cycle: Bool
    public let missing: [String]
    public let conflicts: [PlanConflict]

    public init?(_ p: [String: JSON]) {
        guard let plan = p["plan"]?.string, let revision = p["revision"]?.integer,
              let approval = p["approval"]?.string, let steps = p["steps"]?.items else { return nil }
        let checks = p["checks"]
        self.init(plan: plan, revision: revision, approval: approval, featureName: p["feature_name"]?.string,
                  run: p["run"]?.string, steps: steps.compactMap(PlanStepDetail.init),
                  cycle: checks?["cycle"]?.boolean ?? false,
                  missing: (checks?["missing"]?.items ?? []).compactMap(\.string),
                  conflicts: (checks?["conflicts"]?.items ?? []).map {
                      PlanConflict(steps: ($0["steps"]?.items ?? []).compactMap(\.string),
                                   paths: ($0["paths"]?.items ?? []).compactMap(\.string))
                  })
    }

    public init(plan: String, revision: Int64 = 1, approval: String = "Approved", featureName: String? = nil,
                run: String? = nil, steps: [PlanStepDetail] = [], cycle: Bool = false, missing: [String] = [],
                conflicts: [PlanConflict] = []) {
        (self.plan, self.revision, self.approval, self.featureName, self.run) = (plan, revision, approval, featureName, run)
        (self.steps, self.cycle, self.missing, self.conflicts) = (steps, cycle, missing, conflicts)
    }

    public var eligible: [PlanStepDetail] { steps.filter(\.eligible) }
}

/// One event about an attempt, with the core's sentence.
public struct AttemptEvent: Equatable, Sendable, Identifiable {
    public let seq: Int64
    public let at: Date?
    public let kind: String
    public let summary: String
    public var id: Int64 { seq }

    public init(seq: Int64, at: Date? = nil, kind: String = "", summary: String) {
        (self.seq, self.at, self.kind, self.summary) = (seq, at, kind, summary)
    }
}

/// One host validation run of an attempt ("V-n").
public struct ValidationRunDetail: Equatable, Sendable, Identifiable {
    public let ordinal: Int64
    public let receipt: Int64
    public let at: Date?
    public let candidate: String
    public let base: String
    public let validator: String
    public let passed: Bool
    public let reasons: [String]
    public let report: String
    public let approvals: String
    public var id: Int64 { receipt }

    public init?(_ p: JSON) {
        guard let ordinal = p["ordinal"]?.integer, let receipt = p["receipt"]?.integer,
              let passed = p["passed"]?.boolean else { return nil }
        self.init(ordinal: ordinal, receipt: receipt, at: helperDate(p["at_unix_ms"]?.integer),
                  candidate: p["candidate"]?.string ?? "", base: p["base"]?.string ?? "",
                  validator: p["validator"]?.string ?? "", passed: passed,
                  reasons: (p["reasons"]?.items ?? []).compactMap(\.string), report: p["report"]?.string ?? "",
                  approvals: p["approvals"]?.string ?? "")
    }

    public init(ordinal: Int64, receipt: Int64, at: Date? = nil, candidate: String = "", base: String = "",
                validator: String = "", passed: Bool, reasons: [String] = [], report: String = "", approvals: String = "") {
        (self.ordinal, self.receipt, self.at, self.candidate, self.base) = (ordinal, receipt, at, candidate, base)
        (self.validator, self.passed, self.reasons, self.report, self.approvals) =
            (validator, passed, reasons, report, approvals)
    }
}

/// The answer of `AttemptDetail`.
public struct AttemptDetail: Equatable, Sendable {
    public let id: String
    public let key: String?
    public let title: String?
    public let plan: String?
    public let featureName: String?
    public let ordinal: Int64?
    public let repositoryName: String?
    public let projectName: String?
    public let provider: String?
    public let session: String?
    public let pid: Int64?
    public let pgid: Int64?
    public let heartbeat: Date?
    public let worktree: String?
    public let base: String?
    public let snapshot: String?
    public let candidateHead: String?
    public let continues: String?
    public let continuedBy: String?
    /// Resource, mode and state of each claim.
    public let claims: [(resource: String, mode: String, state: String)]
    public let writeScope: [String]
    public let baseline: [String]
    public let tests: [String]
    public let command: [String]
    public let events: [AttemptEvent]
    public let validations: [ValidationRunDetail]
    public let executableVersion: String?
    public let evidenceRecords: Int64

    public init?(_ p: [String: JSON]) {
        guard let id = p["attempt"]?.string else { return nil }
        let strings = { (name: String) in (p[name]?.items ?? []).compactMap(\.string) }
        self.init(id: id, key: p["key"]?.string, title: p["title"]?.string, plan: p["plan"]?.string,
                  featureName: p["feature_name"]?.string, ordinal: p["ordinal"]?.integer,
                  repositoryName: p["repository_name"]?.string, projectName: p["project_name"]?.string,
                  provider: p["provider"]?.string, session: p["session"]?.string,
                  pid: p["process"]?["pid"]?.integer, pgid: p["process"]?["pgid"]?.integer,
                  heartbeat: helperDate(p["heartbeat_at_unix_ms"]?.integer),
                  worktree: p["worktree"]?["root"]?.string, base: p["worktree"]?["base"]?.string,
                  snapshot: p["worktree"]?["snapshot"]?.string, candidateHead: p["candidate_head"]?.string,
                  continues: p["continues"]?.string, continuedBy: p["continued_by"]?.string,
                  claims: (p["claims"]?.items ?? []).compactMap { c in
                      guard let r = c["resource"]?.string, let m = c["mode"]?.string, let s = c["state"]?.string
                      else { return nil }
                      return (r, m, s)
                  },
                  writeScope: strings("write_scope"), baseline: strings("baseline"), tests: strings("tests"),
                  command: strings("command"),
                  events: (p["events"]?.items ?? []).compactMap { e in
                      guard let seq = e["seq"]?.integer, let summary = e["summary"]?.string else { return nil }
                      return AttemptEvent(seq: seq, at: helperDate(e["at_unix_ms"]?.integer),
                                          kind: e["kind"]?.string ?? "", summary: summary)
                  },
                  validations: (p["validations"]?.items ?? []).compactMap(ValidationRunDetail.init),
                  executableVersion: p["executable"]?["version"]?.string,
                  evidenceRecords: p["evidence_records"]?.integer ?? 0)
    }

    public init(id: String, key: String? = nil, title: String? = nil, plan: String? = nil, featureName: String? = nil,
                ordinal: Int64? = nil, repositoryName: String? = nil, projectName: String? = nil,
                provider: String? = nil, session: String? = nil, pid: Int64? = nil, pgid: Int64? = nil,
                heartbeat: Date? = nil, worktree: String? = nil, base: String? = nil, snapshot: String? = nil,
                candidateHead: String? = nil, continues: String? = nil, continuedBy: String? = nil,
                claims: [(resource: String, mode: String, state: String)] = [], writeScope: [String] = [],
                baseline: [String] = [], tests: [String] = [], command: [String] = [], events: [AttemptEvent] = [],
                validations: [ValidationRunDetail] = [], executableVersion: String? = nil, evidenceRecords: Int64 = 0) {
        (self.id, self.key, self.title, self.plan, self.featureName) = (id, key, title, plan, featureName)
        (self.ordinal, self.repositoryName, self.projectName) = (ordinal, repositoryName, projectName)
        (self.provider, self.session, self.pid, self.pgid, self.heartbeat) = (provider, session, pid, pgid, heartbeat)
        (self.worktree, self.base, self.snapshot, self.candidateHead) = (worktree, base, snapshot, candidateHead)
        (self.continues, self.continuedBy, self.claims) = (continues, continuedBy, claims)
        (self.writeScope, self.baseline, self.tests, self.command) = (writeScope, baseline, tests, command)
        (self.events, self.validations) = (events, validations)
        (self.executableVersion, self.evidenceRecords) = (executableVersion, evidenceRecords)
    }

    public static func == (a: AttemptDetail, b: AttemptDetail) -> Bool {
        a.id == b.id && a.events == b.events && a.validations == b.validations && a.pid == b.pid
            && a.candidateHead == b.candidateHead && a.heartbeat == b.heartbeat
            && a.claims.map(\.resource) == b.claims.map(\.resource) && a.claims.map(\.state) == b.claims.map(\.state)
    }
}

/// The most characters of a document's text the app reads (eight pages).
public let documentTextLimit = 8 * 8192

extension WorkspaceModel {
    /// Protocol 1.9: one document (`revision` nil: its newest) with its text read to the
    /// end, page by page. Read by Specifications and the Approval Inbox while shown.
    public func loadDocument(_ id: String, revision: Int64? = nil) {
        let key = "\(id)#\(revision.map(String.init) ?? "")"
        guard documentKey != key || documentLoad != .loading else { return }
        documentKey = key
        documentLoad = .loading
        onChange?()
        readDocument(id, revision: revision, key: key, offset: 0, partial: nil, comparing: false)
    }

    /// Protocol 1.9: another revision of a document, for Compare on Specifications.
    public func loadComparison(_ id: String, revision: Int64) {
        let key = "\(id)#\(revision)"
        guard comparisonKey != key || comparisonLoad != .loading else { return }
        comparisonKey = key
        comparisonLoad = .loading
        onChange?()
        readDocument(id, revision: revision, key: key, offset: 0, partial: nil, comparing: true)
    }

    private func settle(_ detail: DocumentDetail?, _ load: PlanLoad, comparing: Bool) {
        if comparing {
            (comparison, comparisonLoad) = (detail, load)
        } else {
            (document, documentLoad) = (detail, load)
        }
    }

    private func readDocument(_ id: String, revision: Int64?, key: String, offset: Int64, partial: DocumentDetail?,
                              comparing: Bool) {
        client.query(.Document, ["document": .text(id), "revision": revision.map(JSON.int) ?? .null,
                                 "offset": .int(offset)]) { [weak self] reply in
            guard let self, (comparing ? self.comparisonKey : self.documentKey) == key else { return }
            switch reply {
            case .response(let p):
                guard var page = DocumentDetail(p), let next = p["next"]?.integer else {
                    self.settle(nil, .failed("malformed document"), comparing: comparing)
                    break
                }
                if let partial { page.text = partial.text + page.text }
                if !page.complete && page.text.count < documentTextLimit && next > offset {
                    self.readDocument(id, revision: revision, key: key, offset: next, partial: page, comparing: comparing)
                    return
                }
                self.settle(page, .loaded, comparing: comparing)
            case .null:
                self.settle(nil, .failed("This core has no such document."), comparing: comparing)
            case .error(let reason), .unanswered(let reason):
                self.settle(nil, .failed(reason), comparing: comparing)
            default:
                self.settle(nil, .failed("unexpected answer"), comparing: comparing)
            }
            self.onChange?()
        }
    }

    /// Protocol 1.9: the newest decisions on documents.
    public func loadDecisions() {
        guard decisionsLoad != .loading else { return }
        decisionsLoad = .loading
        client.query(.Decisions, ["before": .null]) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                if let items = p["decisions"]?.items {
                    (self.decisions, self.decisionsLoad) = (items.compactMap(DecisionSummary.init), .loaded)
                } else {
                    (self.decisions, self.decisionsLoad) = ([], .failed("malformed decisions"))
                }
            case .error(let reason), .unanswered(let reason):
                (self.decisions, self.decisionsLoad) = ([], .failed(reason))
            default:
                (self.decisions, self.decisionsLoad) = ([], .failed("unexpected answer"))
            }
            self.onChange?()
        }
    }

    /// Protocol 1.9: one plan step by step. Read by Execution Plan while shown.
    public func loadPlanDetail(_ plan: String) {
        guard planDetailLoads[plan] != .loading else { return }
        planDetailLoads[plan] = .loading
        client.query(.PlanDetail, ["plan": .text(plan)]) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                if let detail = PlanDetail(p) {
                    self.planDetails[plan] = detail
                    self.planDetailLoads[plan] = .loaded
                } else {
                    self.planDetailLoads[plan] = .failed("malformed plan detail")
                }
            case .null:
                self.planDetails[plan] = nil
                self.planDetailLoads[plan] = .failed("This core has no such plan.")
            case .error(let reason), .unanswered(let reason):
                self.planDetailLoads[plan] = .failed(reason)
            default:
                self.planDetailLoads[plan] = .failed("unexpected answer")
            }
            self.onChange?()
        }
    }

    /// Protocol 1.9: one attempt beyond `Attempts`. Read by Agent Runs, Source Control and
    /// Verification & Evidence while shown.
    public func loadAttemptDetail(_ attempt: String) {
        guard attemptDetailKey != attempt || attemptDetailLoad != .loading else { return }
        attemptDetailKey = attempt
        attemptDetailLoad = .loading
        onChange?()
        client.query(.AttemptDetail, ["attempt": .text(attempt)]) { [weak self] reply in
            guard let self, self.attemptDetailKey == attempt else { return }
            switch reply {
            case .response(let p):
                if let detail = AttemptDetail(p) {
                    (self.attemptDetail, self.attemptDetailLoad) = (detail, .loaded)
                } else {
                    (self.attemptDetail, self.attemptDetailLoad) = (nil, .failed("malformed attempt detail"))
                }
            case .null:
                (self.attemptDetail, self.attemptDetailLoad) = (nil, .failed("This core has no such attempt."))
            case .error(let reason), .unanswered(let reason):
                (self.attemptDetail, self.attemptDetailLoad) = (nil, .failed(reason))
            default:
                (self.attemptDetail, self.attemptDetailLoad) = (nil, .failed("unexpected answer"))
            }
            self.onChange?()
        }
    }
}

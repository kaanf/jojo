import Foundation

/// M5 (protocol 1.10): the push of a finished plan branch and the draft pull request of a
/// pushed one. Each is requested, authorized by the human on its exact payload, performed
/// once by the helper and resolved only by what was observed. Nothing here sends anything
/// twice: an unanswered command is reconciled by its receipt.

/// What an external action does.
public enum ExternalKind: String, Sendable {
    case Push, PullRequest

    public var title: String { self == .Push ? "Push" : "Draft pull request" }
}

/// Where one external action stands, as the helper reports it.
public enum ExternalState: String, Sendable {
    /// Waits for the human's decision.
    case Requested
    case Declined
    /// Authorized and running in the helper now.
    case InFlight
    /// Issued, and its outcome was not observed: held for the human, never sent again.
    case Unknown
    case Committed
    case Rejected
}

/// One observation of an action: what the program said, or what an inspection saw.
public struct ExternalObservation: Equatable, Sendable {
    public let observer: String
    public let status: String
    public let detail: String
    public let at: Date?

    public init?(_ p: JSON) {
        guard let observer = p["observer"]?.string, let status = p["status"]?.string,
              let detail = p["detail"]?.string else { return nil }
        self.init(observer: observer, status: status, detail: detail, at: helperDate(p["at_unix_ms"]?.integer))
    }

    public init(observer: String, status: String, detail: String, at: Date? = nil) {
        (self.observer, self.status, self.detail, self.at) = (observer, status, detail, at)
    }
}

public struct ExternalAction: Equatable, Sendable, Identifiable {
    public let id: String
    public let kind: ExternalKind
    public let plan: String
    public let sequence: Int64
    public let featureName: String?
    public let remote: String
    public let url: String
    public let branch: String
    public let commit: String
    public let base: String?
    public let title: String?
    /// The body of a pull request, while it waits for the decision.
    public let body: String?
    public let digest: String
    public let state: ExternalState
    public let requested: Date?
    public let decided: Date?
    public let detail: String
    public let number: Int64?
    public let pullRequestURL: String?
    public let observations: [ExternalObservation]

    public init?(_ p: JSON) {
        guard let id = p["action"]?.string, let kind = p["kind"]?.string.flatMap(ExternalKind.init(rawValue:)),
              let plan = p["plan"]?.string, let url = p["url"]?.string, let branch = p["branch"]?.string,
              let commit = p["commit"]?.string, let digest = p["digest"]?.string,
              let state = p["state"]?.string.flatMap(ExternalState.init(rawValue:)),
              let items = p["observations"]?.items else { return nil }
        let observations = items.compactMap(ExternalObservation.init)
        guard observations.count == items.count else { return nil }
        self.init(id: id, kind: kind, plan: plan, sequence: p["sequence"]?.integer ?? 1,
                  featureName: p["feature_name"]?.string, remote: p["remote"]?.string ?? "origin", url: url,
                  branch: branch, commit: commit, base: p["base"]?.string, title: p["title"]?.string,
                  body: p["body"]?.string, digest: digest, state: state,
                  requested: helperDate(p["requested_at_unix_ms"]?.integer),
                  decided: helperDate(p["decided_at_unix_ms"]?.integer), detail: p["detail"]?.string ?? "",
                  number: p["number"]?.integer, pullRequestURL: p["pull_request_url"]?.string,
                  observations: observations)
    }

    public init(id: String, kind: ExternalKind, plan: String, sequence: Int64 = 1, featureName: String? = nil,
                remote: String = "origin", url: String, branch: String, commit: String, base: String? = nil,
                title: String? = nil, body: String? = nil, digest: String = String(repeating: "a", count: 64),
                state: ExternalState, requested: Date? = nil, decided: Date? = nil, detail: String = "",
                number: Int64? = nil, pullRequestURL: String? = nil, observations: [ExternalObservation] = []) {
        (self.id, self.kind, self.plan, self.sequence, self.featureName) = (id, kind, plan, sequence, featureName)
        (self.remote, self.url, self.branch, self.commit, self.base) = (remote, url, branch, commit, base)
        (self.title, self.body, self.digest, self.state) = (title, body, digest, state)
        (self.requested, self.decided, self.detail) = (requested, decided, detail)
        (self.number, self.pullRequestURL, self.observations) = (number, pullRequestURL, observations)
    }

    /// The full reference the push writes.
    public var reference: String { "refs/heads/\(branch)" }
}

/// One plan run as far as pushing goes: what may be requested now, and why not.
public struct ExternalRun: Equatable, Sendable, Identifiable {
    public let id: String
    public let revision: Int64
    public let feature: String?
    public let featureName: String?
    public let branch: String
    public let base: String
    public let head: String
    public let state: String
    public let push: String?
    public let pullRequest: String?
    public let pushRefusal: String?
    public let pullRequestRefusal: String?

    public init?(_ p: JSON) {
        guard let id = p["plan"]?.string, let branch = p["branch"]?.string, let head = p["head"]?.string,
              let state = p["state"]?.string else { return nil }
        self.init(id: id, revision: p["revision"]?.integer ?? 1, feature: p["feature"]?.string,
                  featureName: p["feature_name"]?.string,
                  branch: branch, base: p["base"]?.string ?? "", head: head, state: state, push: p["push"]?.string,
                  pullRequest: p["pull_request"]?.string,
                  pushRefusal: p["can_push"] == .bool(true) ? nil : (p["push_refusal"]?.string ?? "not available"),
                  pullRequestRefusal: p["can_open_pull_request"] == .bool(true)
                      ? nil : (p["pull_request_refusal"]?.string ?? "not available"))
    }

    public init(id: String, revision: Int64 = 1, feature: String? = nil, featureName: String? = nil, branch: String,
                base: String = "", head: String, state: String = "DONE", push: String? = nil, pullRequest: String? = nil,
                pushRefusal: String? = nil, pullRequestRefusal: String? = "the plan branch is not pushed yet") {
        (self.id, self.revision, self.feature, self.featureName) = (id, revision, feature, featureName)
        (self.branch, self.base) = (branch, base)
        (self.head, self.state, self.push, self.pullRequest) = (head, state, push, pullRequest)
        (self.pushRefusal, self.pullRequestRefusal) = (pushRefusal, pullRequestRefusal)
    }

    public var canPush: Bool { pushRefusal == nil }
    public var canOpenPullRequest: Bool { pullRequestRefusal == nil }
}

/// How git and gh are signed in, as the helper last read them.
public struct ExternalSignIn: Equatable, Sendable {
    /// `Ready`, `Missing`, `Checking` or `NotConfigured`.
    public let state: String
    public let detail: String

    public init(state: String, detail: String) { (self.state, self.detail) = (state, detail) }
    init(_ p: JSON?) {
        self.init(state: p?["state"]?.string ?? "Checking", detail: p?["detail"]?.string ?? "")
    }

    public var ready: Bool { state == "Ready" }
}

/// The answer of `ExternalActions`.
public struct ExternalList: Equatable, Sendable {
    public let runs: [ExternalRun]
    public let actions: [ExternalAction]
    public let git: ExternalSignIn
    public let gh: ExternalSignIn

    public init?(_ p: [String: JSON]) {
        guard let runItems = p["runs"]?.items, let actionItems = p["actions"]?.items else { return nil }
        let runs = runItems.compactMap(ExternalRun.init)
        let actions = actionItems.compactMap(ExternalAction.init)
        guard runs.count == runItems.count, actions.count == actionItems.count else { return nil }
        self.init(runs: runs, actions: actions, git: ExternalSignIn(p["auth"]?["git"]),
                  gh: ExternalSignIn(p["auth"]?["gh"]))
    }

    public init(runs: [ExternalRun] = [], actions: [ExternalAction] = [],
                git: ExternalSignIn = ExternalSignIn(state: "Ready", detail: "credential helper: osxkeychain"),
                gh: ExternalSignIn = ExternalSignIn(state: "Ready", detail: "Logged in to github.com")) {
        (self.runs, self.actions, self.git, self.gh) = (runs, actions, git, gh)
    }

    public subscript(action: String) -> ExternalAction? { actions.first { $0.id == action } }
    /// The newest action of a kind for a plan.
    public func newest(_ kind: ExternalKind, plan: String) -> ExternalAction? {
        actions.first { $0.plan == plan && $0.kind == kind }
    }
    /// Issued actions whose outcome is not known: the Recovery Center holds them.
    public var unresolved: [ExternalAction] { actions.filter { $0.state == .Unknown || $0.state == .InFlight } }
}

/// A push or pull request request that waits for the human (`Documents` at 1.10).
public struct PendingExternal: Equatable, Sendable, Identifiable {
    public let id: String
    public let kind: ExternalKind
    public let plan: String
    public let featureName: String?
    public let branch: String
    public let commit: String
    public let base: String?
    public let title: String?
    public let requested: Date?

    public init?(_ p: JSON) {
        guard let id = p["action"]?.string, let kind = p["kind"]?.string.flatMap(ExternalKind.init(rawValue:)),
              let plan = p["plan"]?.string, let branch = p["branch"]?.string,
              let commit = p["commit"]?.string else { return nil }
        self.init(id: id, kind: kind, plan: plan, featureName: p["feature_name"]?.string, branch: branch,
                  commit: commit, base: p["base"]?.string, title: p["title"]?.string,
                  requested: helperDate(p["requested_at_unix_ms"]?.integer))
    }

    public init(id: String, kind: ExternalKind, plan: String, featureName: String? = nil, branch: String,
                commit: String, base: String? = nil, title: String? = nil, requested: Date? = nil) {
        (self.id, self.kind, self.plan, self.featureName, self.branch) = (id, kind, plan, featureName, branch)
        (self.commit, self.base, self.title, self.requested) = (commit, base, title, requested)
    }
}

extension WorkspaceModel {
    /// Protocol 1.10: the runs and external actions of every plan, with the sign-in state
    /// of git and gh. Read while Source Control, the Approval Inbox or the Recovery Center
    /// is shown, and again with each new snapshot.
    public func loadExternalActions() {
        guard externalLoad != .loading else { return }
        externalLoad = .loading
        client.query(.ExternalActions, ["plan": .null]) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                if let list = ExternalList(p) {
                    (self.external, self.externalLoad) = (list, .loaded)
                } else {
                    (self.external, self.externalLoad) = (nil, .failed("malformed external actions"))
                }
            case .error(let reason), .unanswered(let reason):
                (self.external, self.externalLoad) = (nil, .failed(reason))
            default:
                (self.external, self.externalLoad) = (nil, .failed("unexpected answer"))
            }
            self.onChange?()
        }
    }

    /// Asks for the push or the draft pull request of a finished plan. Performs nothing;
    /// the request waits in the Approval Inbox.
    public func requestExternal(plan: String, kind: ExternalKind) {
        guard Identity.valid(plan) else { return }
        slice(.RequestExternal, ["plan": .text(plan), "kind": .text(kind.rawValue)],
              sent: "\(kind.title) request sent") { [weak self] _ in
            self?.loadExternalActions()
            return "\(kind.title) requested; it waits for its own authorization in the Approval Inbox"
        } refused: { "\(kind.title) request refused (\($0)); nothing was recorded" }
    }

    /// The human's decision on the exact payload shown. Authorizing issues the action once.
    public func decideExternal(_ action: ExternalAction, authorize: Bool) {
        let verb = authorize ? "Authorize" : "Decline"
        slice(.AuthorizeExternal, ["action": .text(action.id), "digest": .text(action.digest),
                                   "decision": .text(verb)],
              sent: "\(verb) sent for \(action.kind.title.lowercased())") { [weak self] p in
            self?.loadExternalActions()
            let dispatch = p["dispatch"]?.string.map { " (\($0))" } ?? ""
            return "\(p["response"]?.string ?? verb)\(dispatch)"
        } refused: { "\(verb) refused (\($0)); nothing was sent" }
    }

    /// Reads the remote again for an action whose outcome is unknown. Sends nothing.
    public func inspectExternal(_ action: String) {
        slice(.InspectExternal, ["action": .text(action)], sent: "Inspection of the remote sent") { [weak self] _ in
            self?.loadExternalActions()
            return "Inspecting the remote; what it shows is recorded"
        } refused: { "Inspection refused (\($0))" }
    }

    /// Marks an inspected action with an unknown outcome as not performed.
    public func resolveExternal(_ action: String) {
        slice(.ResolveExternal, ["action": .text(action)], sent: "Not performed sent") { [weak self] _ in
            self?.loadExternalActions()
            return "Marked not performed; a new request may follow"
        } refused: { "Marking not performed refused (\($0))" }
    }
}

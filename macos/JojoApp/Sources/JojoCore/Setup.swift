import Foundation

// Step 7 (Onboarding and New Project): what the app keeps about this Mac's setup, the
// workspace folder as the core lists it (protocol 1.12 `Workspace`), a repository's project
// brief (`ProjectDetail` at 1.12) and the commands that register a repository and ask for
// its brief (`RegisterRepository`, 1.1; `BriefTurn`, 1.12).

/// What Onboarding decided, kept in this Mac's app settings. When the app is started
/// without helper options, it starts the helper from this; without it, Onboarding runs
/// first. The core knows nothing of it but the options it is started with.
public struct JojoSetup: Codable, Equatable, Sendable {
    /// The workspace folder: the helper's disposable root. Repositories must be inside it.
    public var workspace: String
    /// The agents Jojo may run; an agent that is off is not offered in the app.
    public var claudeCode: Bool
    public var codex: Bool
    /// The provider new projects default to (`ClaudeCode` or `Codex`).
    public var defaultProvider: String

    public init(workspace: String, claudeCode: Bool = true, codex: Bool = true, defaultProvider: String = "ClaudeCode") {
        (self.workspace, self.claudeCode, self.codex, self.defaultProvider) = (workspace, claudeCode, codex, defaultProvider)
    }

    public static let key = "JojoSetup"

    public static var stored: JojoSetup? {
        guard let data = UserDefaults.standard.data(forKey: key) else { return nil }
        return try? JSONDecoder().decode(JojoSetup.self, from: data)
    }

    public func save() {
        if let data = try? JSONEncoder().encode(self) { UserDefaults.standard.set(data, forKey: Self.key) }
    }

    /// The default workspace folder: `~/JojoWorkspace`.
    public static func defaultWorkspace(home: String) -> String { (home as NSString).appendingPathComponent("JojoWorkspace") }

    /// Whether `provider` may be run.
    public func allows(_ provider: String) -> Bool { provider == "Codex" ? codex : claudeCode }

    /// The helper options this setup stands for, the same ones `scripts/live_m3_plan.py`
    /// passes: the workspace as the disposable root and each allowed agent on its native
    /// subscription, found through the login shell's PATH (D3).
    public func helperOptions(home: String, user: String, shell: String?) -> [String] {
        var options = ["--disposable-root", workspace]
        if claudeCode {
            options += ["--claude-home", home, "--claude-user", user, "--claude-auth", "native-subscription"]
        }
        if codex {
            options += ["--codex-home", (home as NSString).appendingPathComponent(".codex"), "--codex-auth", "native-subscription"]
        }
        if let shell, shell.hasPrefix("/") { options += ["--login-shell", shell] }
        return options
    }
}

/// The answer of `Workspace` (protocol 1.12): the Git repositories one or two levels below
/// the workspace folder, each with what its manifests say and its identity when registered.
public struct WorkspaceListing: Equatable, Sendable {
    public struct Entry: Equatable, Sendable, Identifiable {
        public let path: String
        public let name: String
        public let stack: [String]
        /// The registered repository's identity; nil when it is not registered.
        public let repository: String?
        public var id: String { path }

        public init(path: String, name: String, stack: [String] = [], repository: String? = nil) {
            (self.path, self.name, self.stack, self.repository) = (path, name, stack, repository)
        }
    }

    public let root: String?
    /// Why the folder cannot be used (not private, missing); nil when it can.
    public let problem: String?
    public let entries: [Entry]

    public init?(_ p: [String: JSON]) {
        guard let items = p["repositories"]?.items else { return nil }
        root = p["root"]?.string
        problem = p["problem"]?.string
        entries = items.compactMap { item in
            guard let path = item["path"]?.string else { return nil }
            return Entry(path: path, name: item["name"]?.string ?? (path as NSString).lastPathComponent,
                         stack: (item["stack"]?.items ?? []).compactMap(\.string), repository: item["repository"]?.string)
        }
    }

    public init(root: String?, problem: String? = nil, entries: [Entry]) {
        (self.root, self.problem, self.entries) = (root, problem, entries)
    }
}

/// A repository's project brief at its newest revision (`ProjectDetail` at 1.12).
public struct ProjectBrief: Equatable, Sendable {
    public struct Row: Equatable, Sendable, Identifiable {
        public let key: String
        public let text: String
        public let sources: [String]
        public let warning: Bool
        public var id: String { key + text }

        public init(key: String, text: String, sources: [String] = [], warning: Bool = false) {
            (self.key, self.text, self.sources, self.warning) = (key, text, sources, warning)
        }
    }

    public let document: String
    public let revision: Int64
    /// `Pending`, `Approved`, `Rejected`, `Revoked` or `Waiting`, as the documents answer.
    public let approval: String
    public let summary: String
    public let rows: [Row]
    public let testCommand: [String]?
    public let created: Int64?

    init?(_ p: JSON) {
        guard let document = p["document"]?.string, let revision = p["revision"]?.integer else { return nil }
        self.init(document: document, revision: revision, approval: p["approval"]?.string ?? "Pending",
                  summary: p["summary"]?.string ?? "",
                  rows: (p["rows"]?.items ?? []).compactMap { r in
                      guard let key = r["key"]?.string, let text = r["text"]?.string else { return nil }
                      return Row(key: key, text: text, sources: (r["sources"]?.items ?? []).compactMap(\.string),
                                 warning: r["warning"]?.boolean ?? false)
                  },
                  testCommand: p["test_command"]?.items?.compactMap(\.string), created: p["created_at_unix_ms"]?.integer)
    }

    public init(document: String, revision: Int64, approval: String, summary: String, rows: [Row],
                testCommand: [String]? = nil, created: Int64? = nil) {
        (self.document, self.revision, self.approval, self.summary) = (document, revision, approval, summary)
        (self.rows, self.testCommand, self.created) = (rows, testCommand, created)
    }

    public var approved: Bool { approval == "Approved" }
}

/// A repository's newest brief turn (`ProjectDetail` at 1.12).
public struct BriefTurnState: Equatable, Sendable {
    public let turn: String
    public let provider: String
    /// `Running`, `Stopping`, `Replied`, `Failed`, `Cancelled` or `Interrupted`.
    public let state: String
    public let detail: String?
    public let started: Int64?
    public let ended: Int64?

    init?(_ p: JSON) {
        guard let turn = p["turn"]?.string, let state = p["state"]?.string else { return nil }
        self.init(turn: turn, provider: p["provider"]?.string ?? "", state: state, detail: p["detail"]?.string,
                  started: p["started_at_unix_ms"]?.integer, ended: p["ended_at_unix_ms"]?.integer)
    }

    public init(turn: String, provider: String, state: String, detail: String? = nil, started: Int64? = nil,
                ended: Int64? = nil) {
        (self.turn, self.provider, self.state, self.detail, self.started, self.ended) = (turn, provider, state, detail, started, ended)
    }

    public var running: Bool { state == "Running" || state == "Stopping" }
}

extension WorkspaceModel {
    /// The Git repositories in the workspace folder (protocol 1.12). Read while New Project
    /// is shown.
    public func loadWorkspaceListing() {
        guard workspaceListingLoad != .loading, snapshotSeq != nil else { return }
        workspaceListingLoad = .loading
        onChange?()
        client.query(.Workspace, [:]) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                if let listing = WorkspaceListing(p) {
                    (self.workspaceListing, self.workspaceListingLoad) = (listing, .loaded)
                } else {
                    (self.workspaceListing, self.workspaceListingLoad) = (nil, .failed("malformed workspace listing"))
                }
            case .error(let reason), .unanswered(let reason):
                (self.workspaceListing, self.workspaceListingLoad) = (nil, .failed(reason))
            default:
                (self.workspaceListing, self.workspaceListingLoad) = (nil, .failed("unexpected answer"))
            }
            self.onChange?()
        }
    }

    /// Registers the repository at `path` (protocol 1.1). The core accepts only a
    /// repository inside the workspace folder; `done` gets its identity, or nil.
    public func registerRepository(_ path: String, done: @escaping (String?) -> Void = { _ in }) {
        slice(.RegisterRepository, ["path": .text(path)], sent: "Registering \((path as NSString).lastPathComponent)…") { [weak self] p in
            let id = p["response"]?.string
            done(id)
            self?.loadWorkspaceListing()
            return "Repository \((path as NSString).lastPathComponent) registered"
        } refused: { code in
            done(nil)
            return code.contains("RepositoryOutsideDisposableRoot")
                ? "The repository is outside the workspace folder; the core registers only repositories inside it"
                : "Registration refused (\(code))"
        }
    }

    /// Approves the newest revision of `subject` exactly as the core binds it now: reads its
    /// review bundle and sends that bundle's decision (New Project's "Approve Brief"). A
    /// stale or refused decision approves nothing; `done` says whether it was committed.
    public func approveDocument(_ subject: String, done: @escaping (Bool) -> Void = { _ in }) {
        guard Identity.valid(subject) else { return done(false) }
        client.query(.ReviewBundle, ["subject": .text(subject)]) { [weak self] reply in
            guard let self, case .response(let p) = reply, let bundle = ReviewBundle(p) else {
                self?.status = "The brief could not be read for approval; nothing was decided"
                self?.onChange?()
                return done(false)
            }
            self.client.command(.HumanDecision, bundle.decision(.Approve), revision: bundle.revision) { [weak self] reply in
                guard let self else { return }
                switch reply {
                case .response:
                    self.status = "Approved revision \(bundle.revision)"
                    done(true)
                case .error(let code):
                    self.status = "Approval refused (\(code)); nothing was resent"
                    done(false)
                case .null, .list, .unanswered:
                    self.status = "Approval outcome unknown; it will be reconciled by receipt, not resent"
                    done(false)
                }
                self.refresh()
                self.onChange?()
            }
        }
    }

    /// Asks the lead for `repository`'s project brief (protocol 1.12): one read-only turn
    /// on `provider`. `message` says what the human brings.
    public func requestBrief(repository: String, provider: String, message: String) {
        guard Identity.valid(repository) else { return }
        briefOutcome = .sent
        slice(.BriefTurn, ["repository": .text(repository), "provider": .text(provider), "message": .text(message)],
              sent: "Brief turn sent; awaiting acknowledgement") { [weak self] p in
            self?.briefOutcome = .accepted(attempt: p["turn"]?.string ?? "", dispatch: p["dispatch"]?.string ?? "")
            if let project = self?.projectDetailKey, !project.isEmpty { self?.loadProjectDetail(project) }
            return p["dispatch"]?.string == "launched" ? "The lead is reading the repository" : "Brief turn accepted"
        } refused: { [weak self] code in
            self?.briefOutcome = .refused(code)
            return "Brief turn refused (\(code))"
        }
    }
}

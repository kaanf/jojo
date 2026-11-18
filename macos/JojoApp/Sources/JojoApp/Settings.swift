import JojoCore
import SwiftUI

// Settings & Providers (docs/design/screens/SETTINGS.md): what the app knows about its
// settings today. The helper reports no provider configuration and no setting, and the app
// has no request that writes one, so the screen is read-only: it shows the options the app
// was started with, the helper's Hello answer and the connection, and says what is missing.

/// The sections of the board, and "Core", which is not on it.
enum SettingsSection: String, CaseIterable, Identifiable, Sendable {
    case claude, codex, core, discovery, defaults, git, storage, retention, security, shortcuts, appearance

    var id: String { rawValue }

    /// The two providers, then the rest.
    static let groups: [[SettingsSection]] = [[.claude, .codex],
                                              [.core, .discovery, .defaults, .git, .storage, .retention, .security, .shortcuts, .appearance]]

    var title: String {
        switch self {
        case .claude: return "Claude Code"
        case .codex: return "Codex"
        case .core: return "Core"
        case .discovery: return "Executable discovery"
        case .defaults: return "Defaults & concurrency"
        case .git: return "Git & GitHub"
        case .storage: return "Workspace storage"
        case .retention: return "Evidence retention"
        case .security: return "Security & approvals"
        case .shortcuts: return "Keyboard shortcuts"
        case .appearance: return "Appearance"
        }
    }

    var symbol: String {
        switch self {
        case .claude: return "sparkle"
        case .codex: return "chevron.left.forwardslash.chevron.right"
        case .core: return "cpu"
        case .discovery: return "magnifyingglass"
        case .defaults: return "slider.horizontal.3"
        case .git: return "arrow.triangle.branch"
        case .storage: return "internaldrive"
        case .retention: return "archivebox"
        case .security: return "lock.shield"
        case .shortcuts: return "keyboard"
        case .appearance: return "paintbrush"
        }
    }

    /// Solid fills of the board; the same in both appearances.
    var color: RGBA {
        switch self {
        case .claude: return RGBA(0xC2643A)
        case .codex: return RGBA(0x3C3C44)
        case .discovery: return RGBA(0x4F5BD5)
        case .defaults: return RGBA(0x2F6FD0)
        case .git, .retention: return RGBA(0x2E7D5B)
        case .core, .storage, .shortcuts: return RGBA(0x5A5A64)
        case .security: return RGBA(0xB5402F)
        case .appearance: return RGBA(0x7B4FD6)
        }
    }
}

/// The options of one provider the app passed to the helper. The provider's home directory
/// and account name are options too; they are not read here and never shown.
struct ProviderLaunch: Equatable, Sendable {
    var executable: String?
    var version: String?
    var digest: String?
    var auth: String?

    var passed: Bool { executable != nil || version != nil || digest != nil || auth != nil }
}

/// What the app was started with, as far as this screen shows it.
struct SettingsLaunch: Equatable, Sendable {
    var helper: String?
    var dataRoot: String?
    var worktreeRoot: String?
    var disposableRoot: String?
    var claude = ProviderLaunch()
    var codex = ProviderLaunch()
    /// D3: the login shell the core asks for PATH, as the app passed it.
    var loginShell: String?

    init(helper: String? = nil, dataRoot: String? = nil, worktreeRoot: String? = nil, disposableRoot: String? = nil,
         claude: ProviderLaunch = ProviderLaunch(), codex: ProviderLaunch = ProviderLaunch(), loginShell: String? = nil) {
        self.helper = helper
        self.dataRoot = dataRoot
        self.worktreeRoot = worktreeRoot
        self.disposableRoot = disposableRoot
        self.claude = claude
        self.codex = codex
        self.loginShell = loginShell
    }

    init(_ options: LaunchOptions) {
        // `slice` is name, value, name, value, as the helper gets it.
        var passed: [String: String] = [:]
        for i in stride(from: 0, to: options.slice.count - 1, by: 2) { passed[options.slice[i]] = options.slice[i + 1] }
        let provider = { (name: String) in
            ProviderLaunch(executable: passed["--\(name)-executable"], version: passed["--\(name)-version"],
                           digest: passed["--\(name)-sha256"], auth: passed["--\(name)-auth"])
        }
        self.init(helper: options.helper?.path, dataRoot: options.dataRoot, worktreeRoot: options.worktreeRoot,
                  disposableRoot: passed["--disposable-root"], claude: provider("claude"), codex: provider("codex"),
                  loginShell: passed["--login-shell"])
    }
}

/// One line of a settings card as a plain value.
struct SettingsLine: Equatable, Identifiable {
    enum Trailing: Equatable {
        case none
        case note(String)
        case evidence(EvidenceState, String)
    }

    var label: String
    var value = ""
    var form = SettingForm.plain
    /// Request names in place of the value, drawn as tags.
    var names: [String] = []
    /// A shortcut in place of the value, drawn as a key cap.
    var keys: String?
    var trailing = Trailing.none

    var id: String { label }

    static func missing(_ label: String, _ value: String = SettingsState.notReported) -> SettingsLine {
        SettingsLine(label: label, value: value, form: .missing)
    }

    /// A path or identifier the app passed, or the statement that it passed none.
    static func passed(_ label: String, _ value: String?, otherwise: String = "Not passed") -> SettingsLine {
        value.map { SettingsLine(label: label, value: $0, form: .identifier) } ?? .missing(label, otherwise)
    }
}

struct SettingsGroup: Equatable, Identifiable {
    var title: String
    var lines: [SettingsLine]

    var id: String { title }
}

/// What one section shows.
struct SettingsPage: Equatable {
    struct Empty: Equatable {
        var title: String
        var message: String
    }

    struct Warning: Equatable {
        var title: String
        var message: String
    }

    var empty: Empty?
    /// D3: a provider's executable changed between two runs.
    var warning: Warning?
    var groups: [SettingsGroup] = []
    /// A sentence in place of groups that have no source right now.
    var notice: String?
    var footnote: String?
}

/// What the screen shows: the connection, the Hello answer, the launch options and the
/// chosen section, which lives in the screen.
struct SettingsState: Equatable {
    var connection: HelperConnection
    var admission: String
    var pid: Int32 = 0
    var hello: HelperHello?
    var launch = SettingsLaunch()
    /// D3: the `Executables` answer, while the core is connected.
    var executables: ExecutableList?
    var executablesLoad = PlanLoad.idle
    var section = SettingsSection.claude
    /// M5 (1.10): how git and gh are signed in, as the core last read them.
    var external: ExternalList?
    /// The core's parallel capacity as the newest project record says (D4).
    var capacity: Int64?
    /// Settings › Appearance, the one setting of the app (step 6).
    var appearance = AppAppearance.light

    static let notReported = "Not reported"
    static let notBuilt = "Not built"
    /// The menu commands of `JojoApp.swift`; a new shortcut is added in both places.
    static let shortcuts = [("Quit Jojo", "⌘Q"), ("Show Jojo", "⌘0"), ("Show Inspector", "⌥⌘I")]

    var link: RecoveryCenterState.Link {
        switch connection {
        case .ready: return .connected
        case .notStarted, .handshaking: return .starting
        case .incompatible: return .incompatible
        case .launchFailed, .channelLost, .ended: return .lost
        }
    }

    /// The words of the sidebar's status well, from the same value.
    var status: ShellStatus { ShellStatus(connection: connection, admission: admission, pid: pid) }

    /// The Hello answer counts only while the helper that gave it is connected.
    var reported: HelperHello? { link == .connected || link == .incompatible ? hello : nil }

    var page: SettingsPage { page(section) }

    func page(_ section: SettingsSection) -> SettingsPage {
        switch section {
        case .claude: return provider("Claude Code", key: "ClaudeCode", flag: "claude", launch.claude)
        case .codex: return provider("Codex", key: "Codex", flag: "codex", launch.codex)
        case .core: return core
        case .discovery:
            let shell = launch.loginShell ?? "/bin/zsh, the core's default"
            return SettingsPage(
                groups: [SettingsGroup(title: "Executable discovery", lines: [
                    SettingsLine(label: "Search", value: "The PATH of the login shell (\(shell)), looked up by the core. No other directory is searched."),
                    SettingsLine(label: "Re-check", value: "At core start and before each attempt and lead turn. Version and digest are learned again when the file changed."),
                    SettingsLine(label: "Changed binary", value: "Warned, not refused (D3): the run records the new version and digest. A pinned executable is refused when it differs."),
                    SettingsLine(label: "Custom paths", value: "--claude-executable and --codex-executable at launch. With a version and a digest they pin the executable."),
                ])],
                footnote: "There is no switch and no rescan: every run checks again. The Claude Code and Codex sections show what the core found.")
        case .defaults:
            return SettingsPage(
                groups: [
                    SettingsGroup(title: "Default providers", lines: [
                        .missing("Provider per task", "Set per project: a project record maps task kinds to a provider. Shown on Projects."),
                        SettingsLine(label: "Validator", value: "The core's host validation of a candidate, then a read-only review by the other provider (M4). The review approves nothing."),
                        .missing("Repair", "Set per project (M4): automatic, at most two tries, then a hold for you; 0 switches it off. External actions are never retried. Shown on Projects."),
                    ]),
                    SettingsGroup(title: "Concurrency", lines: [
                        capacity.map { SettingsLine(label: "Parallel attempts", value: "\($0) across all projects, set by the project record saved last (D4). Edited on Projects.") }
                            ?? .missing("Parallel attempts", "One number for the whole core, set in a project record (D4). No record is read yet."),
                        .missing("Per repository", "No limit (D4)."),
                        .missing("Per provider", "Not split by provider (D4)."),
                    ]),
                ])
        case .git:
            return SettingsPage(
                groups: [
                    SettingsGroup(title: "Git", lines: [
                        .passed("Worktree root", launch.worktreeRoot, otherwise: "Not passed. The core uses its default, which it does not report."),
                        SettingsLine(label: "Branch prefix", value: "jojo/plan/ — a finished plan's branch is pushed under this name"),
                        .missing("Sign commits"),
                        signIn("Credential helper", external?.git),
                    ]),
                    SettingsGroup(title: "GitHub", lines: [
                        signIn("Account", external?.gh, prefix: "Via the gh CLI"),
                        SettingsLine(label: "Push and pull request", value: "Each needs its own authorization in the Approval Inbox, on its exact payload (M5)."),
                        SettingsLine(label: "Combined approval", value: "None. There is no approve-all.", trailing: .note("Off")),
                    ]),
                ])
        case .storage:
            return SettingsPage(
                groups: [SettingsGroup(title: "Workspace storage", lines: [
                    .passed("App data", launch.dataRoot, otherwise: "Not passed. The core uses its default, which it does not report."),
                    .passed("Disposable root", launch.disposableRoot),
                    .missing("Disk space", Self.notBuilt),
                    .missing("Backups", Self.notBuilt),
                    .missing("Restore", Self.notBuilt),
                ])])
        case .retention:
            return SettingsPage(
                groups: [SettingsGroup(title: "Evidence retention", lines: [
                    .missing("Sealed manifests", "Not built. There is no request that seals evidence."),
                    .missing("Raw logs"),
                    .missing("Sensitive values"),
                ])])
        case .security:
            return SettingsPage(
                groups: [
                    SettingsGroup(title: "Approvals", lines: [
                        SettingsLine(label: "Approval binding", value: "A decision names the document's revision and digests; the core refuses a stale one.",
                                     trailing: .note("Always on")),
                        SettingsLine(label: "Approval kinds",
                                     value: "Specification, plan, push and pull request, each decided on its own. There is no approve-all."),
                        .missing("Scope expansions", Self.notBuilt),
                        .missing("Baseline changes", Self.notBuilt),
                    ]),
                    SettingsGroup(title: "Scanning", lines: [.missing("OSV-Scanner", Self.notBuilt)]),
                ])
        case .shortcuts:
            return SettingsPage(
                groups: [SettingsGroup(title: "Keyboard shortcuts", lines: Self.shortcuts.map { SettingsLine(label: $0.0, keys: $0.1) })],
                footnote: "The command palette and the other shortcuts of the design are not built.")
        case .appearance:
            return SettingsPage(
                groups: [SettingsGroup(title: "Appearance", lines: [
                    SettingsLine(label: "Density", value: "One density"),
                    SettingsLine(label: "Reduce motion", value: "Follows the system"),
                ])],
                footnote: "Theme is kept by this Mac's app settings, not by the core. Light is the default.")
        }
    }

    /// git's or gh's sign-in as the core last read it (M5), with its pill.
    private func signIn(_ label: String, _ s: ExternalSignIn?, prefix: String = "") -> SettingsLine {
        guard reported != nil, let s else {
            return .missing(label, reported == nil ? "Unknown: the core is not connected." : "Not read yet")
        }
        let pill = ExternalWords.signIn(s)
        let words = [prefix, s.detail].filter { !$0.isEmpty }.joined(separator: " · ")
        return SettingsLine(label: label, value: words.isEmpty ? pill.0 : words,
                            trailing: .evidence(s.ready ? .passed : .unknown, pill.0))
    }

    /// Whether the helper says it holds a slice configuration: the one thing it reports
    /// about the options it was given.
    var slice: SettingsLine {
        guard let reported else { return .missing("Slice configuration", "Unknown: the core is not connected.") }
        switch reported.slice {
        case true?: return SettingsLine(label: "Slice configuration", value: "Present. The core does not say which providers it holds.")
        case false?: return SettingsLine(label: "Slice configuration", value: "Absent")
        case nil: return .missing("Slice configuration")
        }
    }

    private func provider(_ name: String, key: String, flag: String, _ options: ProviderLaunch) -> SettingsPage {
        let confirmed = SettingsGroup(title: "Reported by the core", lines: [slice])
        guard options.passed else {
            return SettingsPage(
                empty: .init(title: "No \(name) options were passed at launch",
                             message: "Jojo was started without them, so the core does not run \(name)."),
                groups: [confirmed])
        }
        let executable = reported == nil ? nil : executables?[key]
        let warning = executable?.change.map { change in
            SettingsPage.Warning(
                title: "\(name) changed since an earlier run",
                message: "\(change.previous.version) at \(change.previous.path) was followed by \(change.current.version) at \(change.current.path) (event \(change.event)). The run was not refused.")
        }
        return SettingsPage(
            warning: warning,
            groups: [
                found(executable, flag: flag),
                SettingsGroup(title: "Passed to the core at launch", lines: [
                    .passed("Path", options.executable, otherwise: "Not passed. The core finds it on the login shell's PATH."),
                    options.version.map { SettingsLine(label: "Version", value: $0) } ?? .missing("Version", "Not passed"),
                    .passed("SHA-256 digest", options.digest),
                ]),
                SettingsGroup(title: "Authentication", lines: [
                    options.auth.map { SettingsLine(label: "Sign-in", value: "Attested at launch as \($0). The sign-in itself is not checked.") }
                        ?? .missing("Sign-in", "Not passed"),
                    SettingsLine(label: "Credentials",
                                 value: "Handled by the provider's CLI. The only sign-in kind the core accepts is native-subscription; there is no API key option."),
                ]),
                confirmed,
                SettingsGroup(title: "Capabilities", lines: [
                    SettingsLine(label: "Structured event stream", value: "Used: the core reads the CLI's JSON events through a stream profile"),
                    SettingsLine(label: "Headless mode", value: "Used: every run is non-interactive"),
                    .missing("Session resume", "Not built in Jojo: every turn is a fresh run"),
                    .missing("Token reporting"),
                ]),
                SettingsGroup(title: "Usage", lines: [.missing("Tokens"), .missing("Plan limits")]),
            ],
            footnote: "The core finds the executable, checks it again before each run and records its version and digest with every run. It reports no capability, sign-in state, token count or limit of a provider.")
    }

    /// D3: the executable as the core knows it now, and the one of its last run.
    private func found(_ executable: ProviderExecutable?, flag: String) -> SettingsGroup {
        let title = "Found by the core"
        guard let executable else {
            let why: String
            switch (reported, executablesLoad) {
            case (nil, _): why = "Unknown: the core is not connected."
            case (_, .failed(let reason)): why = "Not read: \(reason)"
            default: why = "Being read…"
            }
            return SettingsGroup(title: title, lines: [.missing("Executable", why)])
        }
        var lines: [SettingsLine] = []
        switch executable.source {
        case .discovered:
            lines.append(SettingsLine(label: "Found", value: executable.entry.map { "On the login shell's PATH as \($0)" } ?? "On the login shell's PATH"))
        case .path:
            lines.append(SettingsLine(label: "Found", value: "Given by --\(flag)-executable; version and digest are learned from the file"))
        case .pinned:
            lines.append(SettingsLine(label: "Found", value: "Pinned at launch; refused when its version or digest differs"))
        case .none:
            lines.append(.missing("Found", "Not configured in the core"))
        }
        switch executable.state {
        case .checking: lines.append(.missing("State", "Being found and checked. A Start waits for it."))
        case .refused: lines.append(SettingsLine(label: "State", value: executable.detail, trailing: .evidence(.failed, "Refused")))
        case .ready, .unconfigured: break
        }
        if let path = executable.path { lines.append(SettingsLine(label: "Path", value: path, form: .identifier)) }
        if let version = executable.version {
            lines.append(SettingsLine(label: "Version", value: version,
                                      trailing: executable.profiled == true ? .evidence(.passed, "Compatible") : .note("Not the profiled release")))
        }
        if let digest = executable.sha256 { lines.append(SettingsLine(label: "SHA-256 digest", value: digest, form: .identifier)) }
        lines.append(executable.lastRun.map {
            SettingsLine(label: "Last run", value: "\($0.version) · \($0.sha256.prefix(12))… · recorded at event \($0.event)")
        } ?? .missing("Last run", "No run recorded yet"))
        return SettingsGroup(title: title, lines: lines)
    }

    private var core: SettingsPage {
        let status = status
        let connection = SettingsGroup(title: "Connection", lines: [
            SettingsLine(label: "Core", value: status.core),
            status.session.isEmpty ? .missing("Session", "None") : SettingsLine(label: "Session", value: status.session),
            SettingsLine(label: "Admission", value: status.admission, form: status.tone == nil ? .missing : .plain),
            .passed("Helper executable", launch.helper, otherwise: "None configured"),
        ])
        guard let reported else {
            return SettingsPage(groups: [connection],
                                notice: link == .starting ? "Waiting for the core's answer…" : "Nothing is reported while the core is not connected.")
        }
        let names = { (label: String, names: [String]) in
            names.isEmpty ? SettingsLine.missing(label) : SettingsLine(label: label, names: names)
        }
        return SettingsPage(
            groups: [
                connection,
                SettingsGroup(title: "Protocol", lines: [
                    link == .incompatible
                        ? SettingsLine(label: "Compatibility", value: "The helper speaks another version; diagnostics only.",
                                       trailing: .evidence(.failed, "Mismatch"))
                        : SettingsLine(label: "Compatibility", value: "The helper accepts this app's protocol version.",
                                       trailing: .evidence(.passed, "Compatible")),
                    reported.maxFrame.map { SettingsLine(label: "Largest frame", value: "\($0) bytes") } ?? .missing("Largest frame"),
                    reported.maxPage.map { SettingsLine(label: "Largest page", value: "\($0) rows") } ?? .missing("Largest page"),
                    slice,
                ]),
                SettingsGroup(title: "Requests the core offers", lines: [
                    names("Commands", reported.commands),
                    names("Queries", reported.queries),
                ]),
            ],
            footnote: "None of them changes a setting: the core's values are read-only here. Appearance is this app's own setting.")
    }
}

struct SettingsScreen: View {
    @ObservedObject var model: AppModel
    /// The executables, sign-in states and project records are read only while this
    /// destination is the shown one.
    let visible: Bool
    @State private var section = SettingsSection.claude

    var body: some View {
        let workspace = model.workspace
        SettingsView(
            state: SettingsState(
                connection: model.client.connection, admission: workspace.admission,
                pid: model.client.processIdentifier, hello: model.client.hello,
                launch: SettingsLaunch(model.options), executables: workspace.executables,
                executablesLoad: workspace.executablesLoad, section: section, external: workspace.external,
                capacity: workspace.projects?.capacity, appearance: model.appearance),
            select: { section = $0 },
            setAppearance: { model.setAppearance($0) },
            runSetup: { model.showingSetup = true })
            .onChange(of: visible) {
                workspace.executablesShown = visible
                if visible { load() }
            }
            .onChange(of: workspace.snapshotSeq) { if visible { load() } }
    }

    private func load() {
        let workspace = model.workspace
        workspace.loadExecutables()
        workspace.loadExternalActions()
        workspace.loadProjects()
    }
}

struct SettingsView: View {
    @Environment(\.theme) private var theme
    let state: SettingsState
    var select: (SettingsSection) -> Void = { _ in }
    var setAppearance: (AppAppearance) -> Void = { _ in }
    var runSetup: () -> Void = {}

    var body: some View {
        HStack(alignment: .top, spacing: 14) {
            Scrolling { sections }
                .frame(width: 230)
                .frame(maxHeight: .infinity, alignment: .top)
                .glassPanel(radius: 18)
            Scrolling {
                VStack(alignment: .leading, spacing: 18) {
                    banners
                    HStack(spacing: 10) {
                        SectionTile(symbol: state.section.symbol, color: state.section.color, size: 28)
                        Text(state.section.title).font(.system(size: 20, weight: .bold)).foregroundStyle(theme.textStrong)
                            .accessibilityAddTraits(.isHeader)
                    }
                    if state.section == .appearance { theme_ }
                    if state.section == .storage { setup }
                    page(state.page)
                }
                .frame(maxWidth: 760, alignment: .topLeading)
                .padding(EdgeInsets(top: 6, leading: 8, bottom: 24, trailing: 12))
                .frame(maxWidth: .infinity, alignment: .topLeading)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .padding(EdgeInsets(top: 6, leading: 10, bottom: 10, trailing: 2))
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    /// The one writable setting: the theme of this app.
    private var theme_: some View {
        LabeledSection(title: "Theme") {
            HStack(spacing: 12) {
                Text("Theme").textStyle(.body).foregroundStyle(theme.text)
                Spacer(minLength: 0)
                SegmentedCapsule(selection: Binding(get: { state.appearance }, set: { setAppearance($0) }), label: "Theme")
            }
            .padding(EdgeInsets(top: 9, leading: 12, bottom: 9, trailing: 9))
            .cardElevation(radius: 12)
        }
    }

    /// Step 7: Onboarding again. A changed workspace folder or agent takes effect when
    /// Jojo starts next.
    private var setup: some View {
        LabeledSection(title: "Setup") {
            HStack(spacing: 12) {
                VStack(alignment: .leading, spacing: 1) {
                    Text("Workspace folder and agents").textStyle(.body).foregroundStyle(theme.text)
                    Text("Kept by Onboarding on this Mac. A change takes effect when Jojo starts next.")
                        .textStyle(.secondary).foregroundStyle(theme.text2)
                }
                Spacer(minLength: 0)
                Button("Run Setup Again", action: runSetup).buttonStyle(.jojo(.secondary, compact: true))
            }
            .padding(EdgeInsets(top: 9, leading: 12, bottom: 9, trailing: 9))
            .cardElevation(radius: 12)
        }
    }

    private var sections: some View {
        VStack(alignment: .leading, spacing: 2) {
            ForEach(Array(SettingsSection.groups.enumerated()), id: \.offset) { index, group in
                GroupLabel(index == 0 ? "Providers" : "Jojo")
                    .padding(EdgeInsets(top: index == 0 ? 0 : 12, leading: 6, bottom: 2, trailing: 6))
                ForEach(group) { section in
                    SelectableRow(selected: section == state.section, action: { select(section) }) {
                        SectionTile(symbol: section.symbol, color: section.color, size: 20)
                        Text(section.title).textStyle(.body).foregroundStyle(theme.text).lineLimit(1)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
            }
        }
        .padding(10)
    }

    @ViewBuilder
    private var banners: some View {
        switch state.link {
        case .lost:
            Banner(.danger, title: "Jojo core isn't responding",
                   message: "What the core reported is not shown. Launch options are shown as the app passed them.")
        case .incompatible:
            Banner(.danger, title: "Jojo core speaks another protocol version",
                   message: "Diagnostics only. The values below are what it answered.")
        case .connected, .starting:
            EmptyView()
        }
    }

    @ViewBuilder
    private func page(_ page: SettingsPage) -> some View {
        if let warning = page.warning { Banner(.warning, title: warning.title, message: warning.message) }
        if let empty = page.empty {
            StateBlock(symbol: state.section.symbol, title: empty.title, message: empty.message).frame(height: 180)
        }
        ForEach(page.groups) { group in
            LabeledSection(title: group.title) {
                VStack(spacing: 0) {
                    ForEach(group.lines) { line in self.line(line, last: line.id == group.lines.last?.id) }
                }
                .cardElevation(radius: 12)
            }
        }
        if let notice = page.notice { NoticeWell(notice) }
        if let footnote = page.footnote { Footnote(footnote) }
    }

    private func line(_ line: SettingsLine, last: Bool) -> some View {
        SettingLine(line.label, last: last) {
            if let keys = line.keys {
                KeyCap(keys)
            } else if !line.names.isEmpty {
                WrappingRow(spacing: 6, lineSpacing: 6) {
                    ForEach(line.names, id: \.self) { KindTag($0) }
                }
            } else {
                SettingText(value: line.value, form: line.form)
            }
        } trailing: {
            switch line.trailing {
            case .none: EmptyView()
            case .note(let note): Text(note).textStyle(.meta).foregroundStyle(theme.text3)
            case .evidence(let evidence, let label): EvidencePill(evidence, label: label)
            }
        }
    }
}

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

    init(helper: String? = nil, dataRoot: String? = nil, worktreeRoot: String? = nil, disposableRoot: String? = nil,
         claude: ProviderLaunch = ProviderLaunch(), codex: ProviderLaunch = ProviderLaunch()) {
        self.helper = helper
        self.dataRoot = dataRoot
        self.worktreeRoot = worktreeRoot
        self.disposableRoot = disposableRoot
        self.claude = claude
        self.codex = codex
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
                  disposableRoot: passed["--disposable-root"], claude: provider("claude"), codex: provider("codex"))
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

    var empty: Empty?
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
    var section = SettingsSection.claude

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
        case .claude: return provider("Claude Code", launch.claude)
        case .codex: return provider("Codex", launch.codex)
        case .core: return core
        case .discovery:
            return SettingsPage(
                groups: [SettingsGroup(title: "Executable discovery", lines: [
                    .missing("Search", "Not built. Each executable is a launch option."),
                    .missing("Re-check", "Not built. Each executable is a launch option."),
                    SettingsLine(label: "Changed binary", value: "Refused, not warned: the core runs only the executable whose digest was passed."),
                ])],
                footnote: "Decided and not built: Jojo finds the executables itself, records version and digest per run, and warns when a binary changed.")
        case .defaults:
            return SettingsPage(
                groups: [
                    SettingsGroup(title: "Default providers", lines: [
                        .missing("Provider per task", "Set per project: a project record maps task kinds to a provider. Shown on Projects."),
                        SettingsLine(label: "Validator", value: "The core's host validation of a candidate. No reviewing agent yet."),
                        .missing("Repair", "Decided, not built: automatic, at most two tries, then a human hold."),
                    ]),
                    SettingsGroup(title: "Concurrency", lines: [
                        .missing("Parallel attempts", "Decided (D4): one number for the whole core, set in a project record. Shown on Projects."),
                        .missing("Per repository", "No limit (D4)."),
                        .missing("Per provider", "Not split by provider (D4)."),
                    ]),
                ])
        case .git:
            return SettingsPage(
                groups: [
                    SettingsGroup(title: "Git", lines: [
                        .passed("Worktree root", launch.worktreeRoot, otherwise: "Not passed. The core uses its default, which it does not report."),
                        .missing("Branch prefix"),
                        .missing("Sign commits"),
                    ]),
                    SettingsGroup(title: "GitHub", lines: [
                        .missing("Account", Self.notBuilt),
                        .missing("Push and pull request", "Decided, not built: each is its own approval kind."),
                        SettingsLine(label: "Combined approval", value: "None. There is no approve-all."),
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
                                     value: "Specification and plan. Push and pull request are decided and not built. There is no approve-all."),
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
                    SettingsLine(label: "Theme", value: "Follows the system"),
                    SettingsLine(label: "Density", value: "One density"),
                    .missing("Reduce motion", "No setting"),
                ])],
                footnote: "The app has no switch for these yet.")
        }
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

    private func provider(_ name: String, _ options: ProviderLaunch) -> SettingsPage {
        let confirmed = SettingsGroup(title: "Reported by the core", lines: [slice])
        guard options.passed else {
            return SettingsPage(
                empty: .init(title: "No \(name) options were passed at launch",
                             message: "Jojo was started without them. The core reports no provider configuration, so nothing else is known here."),
                groups: [confirmed])
        }
        return SettingsPage(
            groups: [
                SettingsGroup(title: "Passed to the core at launch", lines: [
                    .passed("Path", options.executable),
                    options.version.map { SettingsLine(label: "Version", value: $0) } ?? .missing("Version", "Not passed"),
                    .passed("SHA-256 digest", options.digest),
                    .missing("Discovery", "Not built. The path is a launch option."),
                ]),
                SettingsGroup(title: "Authentication", lines: [
                    options.auth.map { SettingsLine(label: "Sign-in", value: "Attested at launch as \($0). The sign-in itself is not checked.") }
                        ?? .missing("Sign-in", "Not passed"),
                    SettingsLine(label: "Credentials",
                                 value: "Handled by the provider's CLI. The only sign-in kind the core accepts is native-subscription; there is no API key option."),
                ]),
                confirmed,
            ],
            footnote: "The core does not report these values back, and it reports no capability, sign-in state, token count or limit of a provider. It hashes the executable itself and refuses one whose digest differs.")
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
            footnote: "None of them changes a setting, so every value on this screen is read-only.")
    }
}

struct SettingsScreen: View {
    @ObservedObject var model: AppModel
    @State private var section = SettingsSection.claude

    var body: some View {
        SettingsView(
            state: SettingsState(
                connection: model.client.connection, admission: model.workspace.admission,
                pid: model.client.processIdentifier, hello: model.client.hello,
                launch: SettingsLaunch(model.options), section: section),
            select: { section = $0 })
    }
}

struct SettingsView: View {
    @Environment(\.theme) private var theme
    let state: SettingsState
    var select: (SettingsSection) -> Void = { _ in }

    var body: some View {
        HStack(spacing: 0) {
            Scrolling { sections }
                .frame(maxHeight: .infinity, alignment: .top)
                .frame(width: 220)
                .background(theme.sidebar)
                .overlay(alignment: .trailing) { Rectangle().fill(theme.sidebarEdge).frame(width: 1) }
            Scrolling {
                VStack(alignment: .leading, spacing: 18) {
                    banners
                    HStack(spacing: 10) {
                        SectionTile(symbol: state.section.symbol, color: state.section.color, size: 28)
                        Text(state.section.title).font(.system(size: 18, weight: .bold)).foregroundStyle(theme.textStrong)
                            .accessibilityAddTraits(.isHeader)
                    }
                    page(state.page)
                }
                .frame(maxWidth: 760, alignment: .topLeading)
                .padding(EdgeInsets(top: 18, leading: 22, bottom: 24, trailing: 22))
                .frame(maxWidth: .infinity, alignment: .topLeading)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    private var sections: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(SettingsSection.groups, id: \.self) { group in
                VStack(spacing: 1) {
                    ForEach(group) { section in
                        Button { select(section) } label: {
                            SectionItem(title: section.title, symbol: section.symbol, color: section.color,
                                        selected: section == state.section)
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
        .padding(.vertical, 12)
        .padding(.horizontal, 8)
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
        if let empty = page.empty {
            StateBlock(symbol: state.section.symbol, title: empty.title, message: empty.message).frame(height: 180)
        }
        ForEach(page.groups) { group in
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading(group.title)
                Card(.panelList) {
                    VStack(spacing: 0) {
                        ForEach(group.lines) { line in self.line(line, last: line.id == group.lines.last?.id) }
                    }
                }
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

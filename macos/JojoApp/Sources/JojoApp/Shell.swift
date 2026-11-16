import AppKit
import JojoCore
import SwiftUI

// The app shell of docs/design/COMPONENTS.md, section 4: sidebar, toolbar and content area.

/// The sidebar destinations, in canvas order.
enum Screen: String, CaseIterable, Identifiable, Sendable {
    case dashboard, projects, approvalInbox, recoveryCenter
    case featureChat, specifications, executionPlan, agentRuns, sourceControl, verification
    case activity, settings

    enum Group: Sendable {
        case workspace, feature, system
    }

    var id: String { rawValue }

    var title: String {
        switch self {
        case .dashboard: return "Dashboard"
        case .projects: return "Projects"
        case .approvalInbox: return "Approval Inbox"
        case .recoveryCenter: return "Recovery Center"
        case .featureChat: return "Feature Chat"
        case .specifications: return "Specifications"
        case .executionPlan: return "Execution Plan"
        case .agentRuns: return "Agent Runs"
        case .sourceControl: return "Source Control"
        case .verification: return "Verification & Evidence"
        case .activity: return "Activity & Audit"
        case .settings: return "Settings & Providers"
        }
    }

    /// SF Symbol standing in for the canvas line icon.
    var symbol: String {
        switch self {
        case .dashboard: return "square.grid.2x2"
        case .projects: return "folder"
        case .approvalInbox: return "tray"
        case .recoveryCenter: return "cross.case"
        case .featureChat: return "bubble.left.and.bubble.right"
        case .specifications: return "doc.text"
        case .executionPlan: return "list.bullet.indent"
        case .agentRuns: return "play.circle"
        case .sourceControl: return "arrow.triangle.branch"
        case .verification: return "checkmark.shield"
        case .activity: return "clock.arrow.circlepath"
        case .settings: return "gearshape"
        }
    }

    var group: Group {
        switch self {
        case .dashboard, .projects, .approvalInbox, .recoveryCenter: return .workspace
        case .featureChat, .specifications, .executionPlan, .agentRuns, .sourceControl, .verification: return .feature
        case .activity, .settings: return .system
        }
    }

    /// Ancestors shown before the screen's own name in the toolbar breadcrumb.
    var ancestors: [String] {
        group == .system ? ["Jojo", "System"] : ["Jojo"]
    }

    static func screens(in group: Group) -> [Screen] {
        allCases.filter { $0.group == group }
    }
}

/// What the sidebar status well says, from what the helper reports today. Admission is
/// stated only while the helper is connected; otherwise it is unknown, never open or closed.
struct ShellStatus: Equatable, Sendable {
    /// `nil` is unknown: drawn as an empty ring, not as a status color.
    var tone: Tone?
    var admission: String
    var session: String
    var core: String

    init(connection: HelperConnection, admission: String, pid: Int32) {
        switch connection {
        case .ready(let epoch):
            session = "epoch \(epoch)"
            core = "Core connected · pid \(pid)"
        case .incompatible(let epoch):
            session = "epoch \(epoch)"
            core = "Core protocol mismatch; diagnostics only"
        case .notStarted:
            session = ""
            core = "Core not started"
        case .handshaking:
            session = ""
            core = "Connecting to core…"
        case .launchFailed(let reason):
            session = ""
            core = "Core launch failed: \(reason)"
        case .channelLost(let reason):
            session = ""
            core = "Core channel lost; state unknown, not assumed stopped (\(reason))"
        case .ended(let status):
            session = ""
            core = "Core exited with status \(status); state unknown until reopen inspection"
        }
        guard case .ready = connection, !admission.isEmpty else {
            tone = nil
            self.admission = "Admission unknown"
            return
        }
        tone = admission == "Open" ? .ok : .danger
        self.admission = admission == "Open" ? "Admission open" : "Admission closed"
    }
}

struct AppShell<Content: View>: View {
    @Environment(\.theme) private var theme
    @ObservedObject var model: AppModel
    @Binding var screen: Screen
    let showInspector: () -> Void
    @ViewBuilder var content: Content

    var body: some View {
        HStack(spacing: 0) {
            Sidebar(screen: $screen, status: status, counters: counters)
            VStack(spacing: 0) {
                ShellToolbar(screen: screen, pending: model.workspace.commandPending,
                             refresh: { model.workspace.refresh() }, showInspector: showInspector)
                content
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                    .background(theme.content)
            }
            // The toolbar strip starts at the top of the window, beside the traffic lights.
            .ignoresSafeArea(.container, edges: .top)
        }
        .foregroundStyle(theme.text)
    }

    private var status: ShellStatus {
        ShellStatus(connection: model.client.connection, admission: model.workspace.admission,
                    pid: model.client.processIdentifier)
    }

    /// Work on hold needs the human; attempts are a plain count.
    private var counters: [Screen: SidebarCounter] {
        [.recoveryCenter: SidebarCounter(count: model.workspace.recovery.count, tone: .warn),
         .agentRuns: SidebarCounter(count: AgentRunsState.attempts(in: model.workspace.rows).count, tone: .neutral)]
    }
}

struct SidebarCounter: Equatable {
    var count: Int
    var tone: Tone
}

struct Sidebar: View {
    @Environment(\.theme) private var theme
    @Binding var screen: Screen
    let status: ShellStatus
    let counters: [Screen: SidebarCounter]

    var body: some View {
        VStack(alignment: .leading, spacing: 1) {
            SearchField().padding(.bottom, 8)
            items(.workspace)
            FeatureSwitcher().padding(.top, 14).padding(.bottom, 4)
            items(.feature)
            Text("System")
                .textStyle(.sidebarSection)
                .foregroundStyle(theme.text3)
                .padding(EdgeInsets(top: 14, leading: 9, bottom: 4, trailing: 9))
            items(.system)
            Spacer(minLength: 12)
            StatusWell(status: status)
        }
        .padding(.vertical, 12)
        .padding(.horizontal, 10)
        .frame(width: 236)
        .background(SidebarBackground().ignoresSafeArea())
        .overlay(alignment: .trailing) {
            Rectangle().fill(theme.sidebarEdge).frame(width: 1).ignoresSafeArea()
        }
    }

    private func items(_ group: Screen.Group) -> some View {
        ForEach(Screen.screens(in: group)) { item in
            SidebarItem(screen: item, selected: item == screen, counter: counters[item]) { screen = item }
        }
    }
}

/// The system sidebar material; the `sidebar` token, opaque, when transparency is reduced.
private struct SidebarBackground: View {
    @Environment(\.theme) private var theme
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.opaqueSidebar) private var opaque

    var body: some View {
        if reduceTransparency || opaque {
            theme.sidebar.opacity(1)
        } else {
            SidebarMaterial()
        }
    }
}

private struct SidebarMaterial: NSViewRepresentable {
    func makeNSView(context: Context) -> NSVisualEffectView {
        let view = NSVisualEffectView()
        view.material = .sidebar
        view.blendingMode = .behindWindow
        view.state = .followsWindowActiveState
        return view
    }

    func updateNSView(_ view: NSVisualEffectView, context: Context) {}
}

private struct OpaqueSidebarKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    /// Draws the sidebar with the opaque `sidebar` token instead of the system material
    /// (off-screen rendering cannot draw the material).
    var opaqueSidebar: Bool {
        get { self[OpaqueSidebarKey.self] }
        set { self[OpaqueSidebarKey.self] = newValue }
    }
}

/// Opens the command palette, which is not built yet: disabled, and says why.
private struct SearchField: View {
    @Environment(\.theme) private var theme

    var body: some View {
        HStack(spacing: 7) {
            Image(systemName: "magnifyingglass").font(.system(size: 13))
            Text("Search or run").textStyle(.control).lineLimit(1)
            Spacer(minLength: 4)
            KeyCap("⌘K")
        }
        .foregroundStyle(theme.text3)
        .padding(.leading, 8)
        .padding(.trailing, 5)
        .frame(height: 28)
        .background(theme.fill, in: RoundedRectangle(cornerRadius: 7))
        .opacity(0.42)
        .help("The command palette is not available yet.")
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Search or run. Not available yet.")
    }
}

private struct SidebarItem: View {
    @Environment(\.theme) private var theme
    @State private var hovered = false
    let screen: Screen
    let selected: Bool
    let counter: SidebarCounter?
    let action: () -> Void

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 6)
        Button(action: action) {
            HStack(spacing: 9) {
                Image(systemName: screen.symbol)
                    .font(.system(size: 15))
                    .frame(width: 18)
                    .foregroundStyle(selected ? theme.onAccent : theme.icon)
                Text(screen.title)
                    .textStyle(.nav)
                    .lineLimit(1)
                    .foregroundStyle(selected ? theme.onAccent : theme.navText)
                Spacer(minLength: 4)
                if let counter, counter.count > 0 {
                    CounterBadge(counter.count, tone: counter.tone, onAccent: selected)
                }
            }
            .padding(.leading, 9)
            .padding(.trailing, 8)
            .frame(height: 28)
            .background(selected ? theme.accent : hovered ? theme.hover : .clear, in: shape)
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// Project and feature of the screens below it. Neither exists in the helper yet, so the
/// switcher says so instead of showing an example.
private struct FeatureSwitcher: View {
    @Environment(\.theme) private var theme

    var body: some View {
        HStack(spacing: 8) {
            Text("–")
                .font(.system(size: 12, weight: .bold))
                .foregroundStyle(.white)
                .frame(width: 24, height: 24)
                .background(theme.tileNeutral, in: RoundedRectangle(cornerRadius: 6))
            VStack(alignment: .leading, spacing: 0) {
                Text("No project").font(.system(size: 11)).foregroundStyle(theme.text3)
                Text("No feature selected").textStyle(.controlStrong).foregroundStyle(theme.text)
            }
            .lineLimit(1)
            Spacer(minLength: 0)
        }
        .padding(.vertical, 7)
        .padding(.horizontal, 8)
        .background(theme.fill, in: RoundedRectangle(cornerRadius: 8))
        .help("Projects and features are not available yet.")
    }
}

private struct StatusWell: View {
    @Environment(\.theme) private var theme
    let status: ShellStatus

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 7) {
                if let tone = status.tone {
                    Circle().fill(theme.foreground(tone)).frame(width: 7, height: 7)
                } else {
                    Circle().strokeBorder(theme.text3, lineWidth: 1.5).frame(width: 7, height: 7)
                }
                Text(status.admission).font(.system(size: 12)).foregroundStyle(theme.text).lineLimit(1)
                Spacer(minLength: 4)
                Text(status.session).textStyle(.mono).foregroundStyle(theme.text3).lineLimit(1)
            }
            Text(status.core)
                .font(.system(size: 12))
                .foregroundStyle(theme.text2)
                .lineLimit(3)
                .fixedSize(horizontal: false, vertical: true)
                .help(status.core)
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(theme.fill2, in: RoundedRectangle(cornerRadius: 9))
        .accessibilityElement(children: .combine)
    }
}

struct ShellToolbar: View {
    @Environment(\.theme) private var theme
    let screen: Screen
    /// A command was sent and the core has not confirmed it: "Requested", never success.
    let pending: Bool
    let refresh: () -> Void
    let showInspector: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            VStack(alignment: .leading, spacing: 1) {
                Text(screen.title).textStyle(.toolbarTitle).foregroundStyle(theme.textStrong).lineLimit(1)
                HStack(spacing: 5) {
                    ForEach(screen.ancestors, id: \.self) { name in
                        Text(name).foregroundStyle(theme.text3)
                        Text("›").foregroundStyle(theme.text4)
                    }
                    Text(screen.title).foregroundStyle(theme.text2)
                }
                .textStyle(.meta)
                .lineLimit(1)
            }
            Spacer(minLength: 12)
            if pending {
                OperationalPill(.waiting, label: "Requested")
                    .help("Sent to core · waiting for confirmation")
            }
            Button(action: refresh) { Label("Refresh", systemImage: "arrow.clockwise") }
                .buttonStyle(IconButtonStyle())
                .help("Refresh")
            Button(action: showInspector) { Label("Show Inspector", systemImage: "sidebar.right") }
                .buttonStyle(IconButtonStyle())
                .help("Show Inspector")
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 20)
        .frame(minHeight: 52)
        .background(theme.toolbar)
        .overlay(alignment: .bottom) {
            Rectangle().fill(theme.toolbarEdge).frame(height: 1)
        }
    }
}

/// The Empty form of docs/design/STATES.md for a destination whose screen is not built yet.
struct NotBuiltYet: View {
    @Environment(\.theme) private var theme
    let screen: Screen

    var body: some View {
        VStack(spacing: 8) {
            Image(systemName: screen.symbol).font(.system(size: 24)).foregroundStyle(theme.text3)
            Text("\(screen.title) is not built yet").textStyle(.bodyStrong).foregroundStyle(theme.textStrong)
            Text("Runs, review, recovery, start and candidate review are under Agent Runs, Approval Inbox, Recovery Center, Execution Plan and Verification & Evidence.")
                .textStyle(.body)
                .foregroundStyle(theme.text2)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 320)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

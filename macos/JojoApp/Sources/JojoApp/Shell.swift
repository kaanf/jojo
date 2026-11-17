import AppKit
import JojoCore
import SwiftUI

// The app shell of docs/design/COMPONENTS.md, section 4: the window material, the sidebar as
// a glass panel, and the content column that starts with the screen's header row.

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

    /// The place named in the header's `sub` line before the screen's own name. Feature
    /// screens name the feature, which the core does not report yet.
    var place: String {
        switch group {
        case .workspace: return "Workspace"
        case .feature: return "No feature selected"
        case .system: return "System"
        }
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


/// The window (padding 8): the sidebar panel, then the content column, which has no surface
/// of its own and starts with the header row.
struct AppShell<Content: View>: View {
    @Environment(\.theme) private var theme
    @ObservedObject var model: AppModel
    @Binding var screen: Screen
    let showInspector: () -> Void
    /// The shown screen's own header controls, before the shell's icon buttons, and its one
    /// primary action after them.
    var controls: AnyView?
    var primary: AnyView?
    /// The shown screen's own title and sub line, when they are not the destination's
    /// (New Feature, a feature's chat).
    var heading: (title: String, subtitle: String)?
    @ViewBuilder var content: Content

    var body: some View {
        HStack(spacing: 10) {
            Sidebar(screen: $screen, status: status, counters: counters)
            VStack(spacing: 0) {
                Header(screen: screen, pending: model.workspace.commandPending,
                       refresh: { model.workspace.refresh() }, showInspector: showInspector,
                       controls: controls, primary: primary, heading: heading)
                    .padding(EdgeInsets(top: 10, leading: 4, bottom: 6, trailing: 10))
                content
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            }
        }
        .padding(8)
        // The sidebar and the header start at the top of the window, beside the traffic lights.
        .ignoresSafeArea(.container, edges: .top)
        .background(WindowMaterial().ignoresSafeArea())
        .foregroundStyle(theme.text)
    }

    private var status: ShellStatus {
        ShellStatus(connection: model.client.connection, admission: model.workspace.admission,
                    pid: model.client.processIdentifier)
    }

    /// Work on hold needs the human; attempts are a plain count; Source Control counts the
    /// loaded attempt's changed paths outside the write scope.
    private var counters: [Screen: SidebarCounter] {
        [.recoveryCenter: SidebarCounter(count: model.workspace.recovery.count, tone: .warn),
         .agentRuns: SidebarCounter(count: AgentRunsState.attempts(in: model.workspace.rows).count, tone: .neutral),
         .sourceControl: SidebarCounter(count: SourceControlState.violations(in: model.workspace.changedFiles).count, tone: .warn),
         .approvalInbox: SidebarCounter(count: Int(model.workspace.documents?.pending ?? 0), tone: .review)]
    }
}

struct SidebarCounter: Equatable {
    var count: Int
    var tone: Tone
}

/// Glass panel, 248 wide, padding 14 10 12, groups 14 apart.
struct Sidebar: View {
    @Environment(\.theme) private var theme
    @Binding var screen: Screen
    let status: ShellStatus
    let counters: [Screen: SidebarCounter]

    var body: some View {
        VStack(alignment: .leading, spacing: 1) {
            // The system traffic lights sit here.
            Spacer().frame(height: 32)
            SearchField().padding(.bottom, 14)
            items(.workspace)
            FeatureHeading().padding(EdgeInsets(top: 14, leading: 9, bottom: 4, trailing: 9))
            items(.feature)
            heading("System")
            items(.system)
            Spacer(minLength: 12)
            StatusCard(status: status)
        }
        .padding(EdgeInsets(top: 14, leading: 10, bottom: 12, trailing: 10))
        .frame(width: 248)
        .frame(maxHeight: .infinity)
        .glassPanel()
    }

    private func heading(_ title: String) -> some View {
        Text(title)
            .textStyle(.sidebarSection)
            .foregroundStyle(theme.text3)
            .padding(EdgeInsets(top: 14, leading: 9, bottom: 4, trailing: 9))
    }

    private func items(_ group: Screen.Group) -> some View {
        ForEach(Screen.screens(in: group)) { item in
            SidebarItem(screen: item, selected: item == screen, counter: counters[item]) { screen = item }
        }
    }
}

/// Opens the command palette, which is not built yet: disabled, and says why.
private struct SearchField: View {
    @Environment(\.theme) private var theme

    var body: some View {
        HStack(spacing: 7) {
            Image(systemName: "magnifyingglass").font(.system(size: 12, weight: .medium))
            Text("Search or run").textStyle(.body).lineLimit(1)
            Spacer(minLength: 4)
            KeyCap("⌘K")
        }
        .foregroundStyle(theme.text3)
        .padding(.horizontal, 11)
        .frame(height: 30)
        .glassCapsule()
        .opacity(0.6)
        .help("The command palette is not available yet.")
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Search or run. Not available yet.")
    }
}

/// Nav item: 30 high, radius 9, an `accent` glyph, the label and a trailing counter. Chosen:
/// `accentTint` and a semibold label; under the pointer: `grayBg`.
private struct SidebarItem: View {
    @Environment(\.theme) private var theme
    @State private var hovered = false
    let screen: Screen
    let selected: Bool
    let counter: SidebarCounter?
    let action: () -> Void

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 9)
        Button(action: action) {
            HStack(spacing: 9) {
                Image(systemName: screen.symbol)
                    .font(.system(size: 14))
                    .frame(width: 18)
                    .foregroundStyle(theme.accent)
                Text(screen.title)
                    .font(.system(size: 13, weight: selected ? .semibold : .regular))
                    .lineLimit(1)
                    .foregroundStyle(theme.text)
                Spacer(minLength: 4)
                if let counter, counter.count > 0 {
                    CounterBadge(counter.count, tone: counter.tone)
                }
            }
            .padding(.horizontal, 9)
            .frame(height: 30)
            .background(selected ? theme.accentTint : hovered ? theme.grayBg : .clear, in: shape)
            .contentShape(shape)
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// The heading of the feature group: the project badge and "Project · Feature". Neither
/// exists in the helper yet, so it says so instead of showing an example.
private struct FeatureHeading: View {
    @Environment(\.theme) private var theme

    var body: some View {
        HStack(spacing: 7) {
            Text("–")
                .font(.system(size: 10, weight: .bold))
                .foregroundStyle(.white)
                .frame(width: 16, height: 16)
                .background(Palette.gray.color, in: RoundedRectangle(cornerRadius: 5))
            Text("No project · No feature selected")
                .textStyle(.sidebarSection)
                .foregroundStyle(theme.text3)
                .lineLimit(1)
        }
        .help("Projects and features are not available yet.")
    }
}

/// The card at the bottom of the sidebar (radius 12, padding 10): the admission dot and
/// sentence with the session, then the core's connection.
private struct StatusCard: View {
    @Environment(\.theme) private var theme
    let status: ShellStatus

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 7) {
                if let tone = status.tone {
                    Circle().fill(theme.foreground(tone)).frame(width: 8, height: 8)
                } else {
                    Circle().strokeBorder(theme.text3, lineWidth: 1.5).frame(width: 8, height: 8)
                }
                Text(status.admission).font(.system(size: 12, weight: .semibold)).lineLimit(1)
                Spacer(minLength: 4)
                Text(status.session).textStyle(.secondary).foregroundStyle(theme.text2).lineLimit(1)
            }
            Text(status.core)
                .textStyle(.secondary)
                .foregroundStyle(theme.text2)
                .lineLimit(3)
                .fixedSize(horizontal: false, vertical: true)
                .help(status.core)
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .cardElevation(radius: 12)
        .accessibilityElement(children: .combine)
    }
}

/// The header row each screen starts with (min height 44): the title and its `sub` line,
/// then the trailing actions: "Requested" while a command awaits the core, and a capsule
/// group of icon buttons.
struct Header: View {
    @Environment(\.theme) private var theme
    let screen: Screen
    /// A command was sent and the core has not confirmed it: "Requested", never success.
    let pending: Bool
    let refresh: () -> Void
    let showInspector: () -> Void
    var controls: AnyView?
    var primary: AnyView?
    var heading: (title: String, subtitle: String)?
    var now = Date()

    var body: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(heading?.title ?? screen.title)
                    .textStyle(screen == .dashboard ? .dashboardTitle : .pageTitle)
                    .lineLimit(1)
                // The Dashboard names the day ("Workspace · Monday, 16 November"); the others the screen.
                Text(heading?.subtitle ?? "\(screen.place) · \(screen == .dashboard ? now.formatted(.dateTime.weekday(.wide).day().month(.wide)) : screen.title)")
                    .textStyle(.secondary)
                    .foregroundStyle(theme.text2)
                    .lineLimit(1)
            }
            Spacer(minLength: 12)
            if pending {
                OperationalPill(.requested, standalone: true)
                    .help("Sent to core · waiting for confirmation")
            }
            if let controls { controls }
            CapsuleGroup {
                Button(action: refresh) { Label("Refresh", systemImage: "arrow.clockwise") }
                    .buttonStyle(IconButtonStyle())
                    .help("Refresh")
                Button(action: showInspector) { Label("Show Inspector", systemImage: "sidebar.right") }
                    .buttonStyle(IconButtonStyle())
                    .help("Show Inspector")
            }
            if let primary { primary }
        }
        .frame(minHeight: 44)
    }
}

/// The trailing inspector: a glass panel 280 to 320 wide, radius 18. The screen gives its
/// content, which brings its own padding.
struct InspectorPanel<Content: View>: View {
    var width: CGFloat = 300
    @ViewBuilder let content: () -> Content

    var body: some View {
        content()
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            .clipShape(RoundedRectangle(cornerRadius: 18))
            .glassPanel()
            .frame(width: width)
    }
}

/// The Empty form of docs/design/STATES.md for a destination whose screen is not built yet.
struct NotBuiltYet: View {
    @Environment(\.theme) private var theme
    let screen: Screen

    var body: some View {
        VStack(spacing: 8) {
            Image(systemName: screen.symbol).font(.system(size: 24)).foregroundStyle(theme.text3)
            Text("\(screen.title) is not built yet").textStyle(.bodyStrong).foregroundStyle(theme.text)
            Text("Runs, review, recovery, start and candidate review are under Agent Runs, Approval Inbox, Recovery Center, Execution Plan and Verification & Evidence.")
                .textStyle(.body)
                .foregroundStyle(theme.text2)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 320)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

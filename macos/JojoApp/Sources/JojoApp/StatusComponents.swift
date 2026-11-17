import SwiftUI

// The status model of docs/design/COMPONENTS.md, sections 1 to 3. Three families that share
// colors and never share a shape: operational state (capsule, dot glyph), evidence (square,
// symbol glyph) and document status (uppercase tag). Unknown is never success or failure.

/// What is happening now.
enum OperationalState: CaseIterable, Sendable {
    case idle, running, waiting, blocked, awaitingApproval, paused, recoveryRequired, failed, completed
    /// M4 (D2): the core starts a repair attempt of a failed plan step on its own.
    case repairing

    var label: String {
        switch self {
        case .idle: return "Idle"
        case .running: return "Running"
        case .waiting: return "Waiting"
        case .blocked: return "Blocked"
        case .awaitingApproval: return "Awaiting approval"
        case .paused: return "Paused"
        case .recoveryRequired: return "Recovery required"
        case .failed: return "Failed"
        case .completed: return "Completed"
        case .repairing: return "Repairing"
        }
    }

    var tone: Tone {
        switch self {
        case .idle, .waiting, .paused: return .neutral
        case .running, .repairing: return .info
        case .blocked, .awaitingApproval: return .warn
        case .recoveryRequired, .failed: return .danger
        case .completed: return .ok
        }
    }

    enum Glyph: Equatable {
        case dot, haloDot, ring, ringDot, symbol(String, CGFloat)
    }

    var glyph: Glyph {
        switch self {
        case .idle, .awaitingApproval, .failed, .completed: return .dot
        case .running: return .haloDot
        case .waiting: return .ring
        case .blocked: return .symbol("exclamationmark.triangle.fill", 9)
        case .paused: return .symbol("pause.fill", 8)
        case .recoveryRequired: return .ringDot
        case .repairing: return .symbol("arrow.triangle.2.circlepath", 8)
        }
    }
}

/// Capsule pill. `label` replaces the state's own name where the design reuses a form
/// ("Held", "Requested", "Temporarily unavailable").
struct OperationalPill: View {
    @Environment(\.theme) private var theme
    let state: OperationalState
    var label: String?
    /// Standing alone (legend, state notice): 11.5 pt and 20 high instead of 11 pt and 18.
    var standalone = false

    init(_ state: OperationalState, label: String? = nil, standalone: Bool = false) {
        self.state = state
        self.label = label
        self.standalone = standalone
    }

    var body: some View {
        let color = theme.foreground(state.tone)
        HStack(spacing: 6) {
            glyph(color)
            Text(label ?? state.label).textStyle(standalone ? .pillStandalone : .pill).lineLimit(1)
        }
        .foregroundStyle(color)
        .padding(.leading, 7)
        .padding(.trailing, 8)
        .frame(height: standalone ? 20 : 18)
        .background(theme.tint(state.tone), in: Capsule())
        .fixedSize()
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(label ?? state.label)
    }

    @ViewBuilder private func glyph(_ color: Color) -> some View {
        switch state.glyph {
        case .dot:
            Circle().fill(color).frame(width: 7, height: 7)
        case .haloDot:
            Circle().fill(color).frame(width: 7, height: 7)
                .padding(3).background(theme.tint(state.tone), in: Circle()).padding(-3)
        case .ring:
            Circle().strokeBorder(color, lineWidth: 1.5).frame(width: 7, height: 7)
        case .ringDot:
            Circle().strokeBorder(color, lineWidth: 1.5).frame(width: 9, height: 9)
                .overlay(Circle().fill(color).frame(width: 3, height: 3))
        case .symbol(let name, let size):
            Image(systemName: name).font(.system(size: size, weight: .semibold))
        }
    }
}

/// What is proven.
enum EvidenceState: CaseIterable, Sendable {
    case passed, sealed, failed, incomplete, needsApproval, notRun, skipped, invalidated, unknown

    var label: String {
        switch self {
        case .passed: return "Passed"
        case .sealed: return "Sealed"
        case .failed: return "Failed"
        case .incomplete: return "Incomplete"
        case .needsApproval: return "Needs approval"
        case .notRun: return "Not run"
        case .skipped: return "Skipped"
        case .invalidated: return "Invalidated"
        case .unknown: return "Unknown"
        }
    }

    var tone: Tone {
        switch self {
        case .passed, .sealed: return .ok
        case .failed: return .danger
        case .incomplete, .needsApproval: return .warn
        case .notRun, .skipped, .invalidated, .unknown: return .neutral
        }
    }

    /// SF Symbol; `nil` for Unknown, which draws a bold question mark.
    var symbol: String? {
        switch self {
        case .passed: return "checkmark"
        case .sealed: return "lock.fill"
        case .failed: return "xmark"
        case .incomplete: return "circle.lefthalf.filled"
        case .needsApproval: return "hand.raised.fill"
        case .notRun: return "minus"
        case .skipped: return "forward.fill"
        case .invalidated: return "slash.circle"
        case .unknown: return nil
        }
    }
}

/// Square pill (radius 4). Unknown has no fill: a dashed outline and a question mark.
struct EvidencePill: View {
    @Environment(\.theme) private var theme
    let state: EvidenceState
    var label: String?

    init(_ state: EvidenceState, label: String? = nil) {
        self.state = state
        self.label = label
    }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 4)
        HStack(spacing: 5) {
            if let symbol = state.symbol {
                Image(systemName: symbol).font(.system(size: 10, weight: .semibold))
            } else {
                Text("?").font(.system(size: 10, weight: .bold))
            }
            Text(label ?? state.label).textStyle(.pillStandalone).strikethrough(state == .invalidated).lineLimit(1)
        }
        .foregroundStyle(theme.foreground(state.tone))
        .padding(.leading, 6)
        .padding(.trailing, 7)
        .frame(height: 18)
        .background(state == .unknown ? Color.clear : theme.tint(state.tone), in: shape)
        .overlay {
            if state == .unknown {
                shape.strokeBorder(theme.dashedOutline, style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
            }
        }
        .fixedSize()
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(label ?? state.label)
    }
}

/// What is binding.
enum DocumentStatus: CaseIterable, Sendable {
    case proposed, approved, superseded, invalidated, stale

    var label: String {
        switch self {
        case .proposed: return "Proposed"
        case .approved: return "Approved"
        case .superseded: return "Superseded"
        case .invalidated: return "Invalidated"
        case .stale: return "Stale"
        }
    }

    var tone: Tone {
        switch self {
        case .proposed: return .info
        case .approved: return .ok
        case .superseded, .invalidated: return .neutral
        case .stale: return .warn
        }
    }

    /// Proposed and Stale are outlined; the others are filled.
    var outlined: Bool { self == .proposed || self == .stale }
}

/// Uppercase tag (radius 4). A revision reference ("PRD rev 4") uses the Approved form.
struct DocumentTag: View {
    @Environment(\.theme) private var theme
    let status: DocumentStatus
    var label: String?

    init(_ status: DocumentStatus, label: String? = nil) {
        self.status = status
        self.label = label
    }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 4)
        Text(label ?? status.label)
            .textStyle(.tag)
            .strikethrough(status == .invalidated)
            .lineLimit(1)
            .foregroundStyle(theme.foreground(status.tone))
            .padding(.horizontal, 8)
            .frame(height: 19)
            .background(status.outlined ? Color.clear : theme.tint(status.tone), in: shape)
            .overlay {
                if status.outlined {
                    shape.strokeBorder(status == .proposed ? theme.infoRing : theme.warnRing, lineWidth: 1)
                }
            }
            .fixedSize()
    }
}

/// Counter badge. The text color carries the meaning: warn needs the human, info is
/// running, danger is failures, neutral is a plain count.
struct CounterBadge: View {
    @Environment(\.theme) private var theme
    let count: Int
    var tone = Tone.neutral
    /// On a selected sidebar item the badge sits on `accent`.
    var onAccent = false

    init(_ count: Int, tone: Tone = .neutral, onAccent: Bool = false) {
        self.count = count
        self.tone = tone
        self.onAccent = onAccent
    }

    var body: some View {
        Text("\(count)")
            .textStyle(.counter)
            .monospacedDigit()
            .foregroundStyle(onAccent ? theme.onAccent : tone == .neutral ? theme.text3 : theme.foreground(tone))
            .padding(.horizontal, 6)
            .frame(height: 17)
            .background(onAccent ? Color.white.opacity(0.2) : theme.fill, in: Capsule())
            .fixedSize()
    }
}

/// Key cap ("⌘K").
struct KeyCap: View {
    @Environment(\.theme) private var theme
    let keys: String

    init(_ keys: String) {
        self.keys = keys
    }

    var body: some View {
        Text(keys)
            .textStyle(.kbd)
            .foregroundStyle(theme.text3)
            .padding(.horizontal, 5)
            .frame(height: 18)
            .background(theme.fill, in: RoundedRectangle(cornerRadius: 5))
            .shadow(color: theme.kbdShadow, radius: 0, y: 1)
    }
}

/// The four button kinds. One primary action per area; destructive actions always confirm.
struct JojoButtonStyle: ButtonStyle {
    enum Kind: CaseIterable, Sendable {
        case primary, secondary, ghost, destructive
    }

    var kind = Kind.secondary
    /// Inspector and empty-state size: 26 high instead of 28.
    var compact = false

    func makeBody(configuration: Configuration) -> some View {
        Face(configuration: configuration, kind: kind, compact: compact)
    }

    private struct Face: View {
        @Environment(\.theme) private var theme
        @Environment(\.isEnabled) private var enabled
        @State private var hovered = false
        let configuration: Configuration
        let kind: Kind
        let compact: Bool

        var body: some View {
            let shape = RoundedRectangle(cornerRadius: 6)
            configuration.label
                .labelStyle(ButtonLabelStyle())
                .textStyle(.control)
                .lineLimit(1)
                .foregroundStyle(foreground)
                .padding(.horizontal, compact ? 10 : 11)
                .frame(height: compact ? 26 : 28)
                .background(background, in: shape)
                .overlay { if let border { shape.strokeBorder(border, lineWidth: 1) } }
                .contentShape(shape)
                // Pressed is not drawn on the canvas; a custom style has no system pressed state.
                .opacity(enabled ? (configuration.isPressed ? 0.8 : 1) : 0.42)
                .onHover { hovered = $0 }
        }

        private var foreground: Color {
            switch kind {
            case .primary: return theme.onAccent
            case .secondary: return theme.text
            case .ghost: return theme.text2
            case .destructive: return theme.danger
            }
        }

        private var background: Color {
            switch kind {
            case .primary: return theme.accent
            case .secondary: return theme.fill
            case .ghost: return hovered && enabled ? theme.fill : .clear
            case .destructive: return theme.dangerTint
            }
        }

        private var border: Color? {
            switch kind {
            case .primary: return theme.primaryBorder
            case .secondary: return theme.hairline
            case .ghost: return nil
            case .destructive: return theme.dangerRing
            }
        }
    }
}

/// A 14 pt leading glyph, 6 from the title.
private struct ButtonLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 6) {
            configuration.icon.font(.system(size: 14))
            configuration.title
        }
    }
}

extension ButtonStyle where Self == JojoButtonStyle {
    static func jojo(_ kind: JojoButtonStyle.Kind, compact: Bool = false) -> JojoButtonStyle {
        JojoButtonStyle(kind: kind, compact: compact)
    }
}

/// A keyboard shortcut shown inside a button ("⌘↵").
struct ShortcutHint: View {
    let keys: String

    init(_ keys: String) {
        self.keys = keys
    }

    var body: some View {
        Text(keys).font(.system(size: 11)).opacity(0.72)
    }
}

/// A disabled control always says why.
struct DisabledReason: View {
    @Environment(\.theme) private var theme
    let text: String

    init(_ text: String) {
        self.text = text
    }

    var body: some View {
        HStack(spacing: 5) {
            Image(systemName: "info.circle").font(.system(size: 12))
            Text(text).textStyle(.meta)
        }
        .foregroundStyle(theme.text3)
    }
}

/// Toolbar icon button: 28 × 28, 16 pt glyph, `fill` under the pointer.
struct IconButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        Face(configuration: configuration)
    }

    private struct Face: View {
        @Environment(\.theme) private var theme
        @Environment(\.isEnabled) private var enabled
        @State private var hovered = false
        let configuration: Configuration

        var body: some View {
            let shape = RoundedRectangle(cornerRadius: 6)
            configuration.label
                .labelStyle(.iconOnly)
                .font(.system(size: 16))
                .foregroundStyle(theme.text2)
                .frame(width: 28, height: 28)
                .background(hovered && enabled ? theme.fill : .clear, in: shape)
                .contentShape(shape)
                .opacity(enabled ? (configuration.isPressed ? 0.8 : 1) : 0.42)
                .onHover { hovered = $0 }
        }
    }
}

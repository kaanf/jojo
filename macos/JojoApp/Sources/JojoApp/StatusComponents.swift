import SwiftUI

// The status model of docs/design/COMPONENTS.md, sections 1 to 3. Three families that share
// colors and never share a shape: operational state (capsule with a dot), evidence (rounded
// square with a symbol) and document status (uppercase outline tag). Unknown is never
// success or failure.

/// What is happening now.
enum OperationalState: CaseIterable, Sendable {
    case idle, running, waiting, blocked, awaitingApproval, paused, recoveryRequired, failed, completed
    /// Sent to the core, not yet confirmed.
    case requested
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
        case .requested: return "Requested"
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
        case .requested: return .review
        }
    }

    /// The dot of the standing pill. States that share a hue differ by glyph as well as by
    /// their word, so color is never the only signal.
    enum Glyph: Equatable {
        case dot, haloDot, ring, ringDot, symbol(String, CGFloat)
    }

    var glyph: Glyph {
        switch self {
        case .idle, .awaitingApproval, .failed, .completed, .requested: return .dot
        case .running: return .haloDot
        case .waiting: return .ring
        case .blocked: return .symbol("exclamationmark.triangle.fill", 8)
        case .paused: return .symbol("pause.fill", 7)
        case .recoveryRequired: return .ringDot
        case .repairing: return .symbol("arrow.triangle.2.circlepath", 8)
        }
    }
}

/// Capsule pill in the hue's text color on its tint. Standing alone (header, inspector,
/// legend) it is 22 high with its glyph; in rows and cards it is 20 high without one.
/// `label` replaces the state's own name where the design reuses a form ("Held").
struct OperationalPill: View {
    @Environment(\.theme) private var theme
    let state: OperationalState
    var label: String?
    var standalone = false

    init(_ state: OperationalState, label: String? = nil, standalone: Bool = false) {
        self.state = state
        self.label = label
        self.standalone = standalone
    }

    var body: some View {
        let color = theme.foreground(state.tone)
        HStack(spacing: 6) {
            if standalone { glyph(color) }
            Text(label ?? state.label).textStyle(.pill).lineLimit(1)
        }
        .foregroundStyle(color)
        .padding(.horizontal, standalone ? 10 : 8)
        .frame(height: standalone ? 22 : 20)
        .background(theme.tint(state.tone), in: Capsule())
        .fixedSize()
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(label ?? state.label)
    }

    @ViewBuilder private func glyph(_ color: Color) -> some View {
        switch state.glyph {
        case .dot:
            Circle().fill(color).frame(width: 6, height: 6)
        case .haloDot:
            Circle().fill(color).frame(width: 6, height: 6)
                .padding(3).background(theme.accentTint, in: Circle()).padding(-3)
        case .ring:
            Circle().strokeBorder(color, lineWidth: 1.5).frame(width: 7, height: 7)
        case .ringDot:
            Circle().strokeBorder(color, lineWidth: 1.5).frame(width: 9, height: 9)
                .overlay(Circle().fill(color).frame(width: 3, height: 3))
        case .symbol(let name, let size):
            Image(systemName: name).font(.system(size: size, weight: .bold))
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
        case .incomplete: return .warn
        case .needsApproval: return .review
        case .notRun, .skipped, .invalidated, .unknown: return .neutral
        }
    }

    /// The text symbol of COMPONENTS.md, section 2.3.
    var symbol: String {
        switch self {
        case .passed: return "✓"
        case .sealed: return "◆"
        case .failed: return "✕"
        case .incomplete: return "◐"
        case .needsApproval: return "!"
        case .notRun: return "○"
        case .skipped: return "↷"
        case .invalidated: return "⊘"
        case .unknown: return "?"
        }
    }
}

/// Rounded square (radius 6), 22 high: the symbol, then the label, in the hue on its tint.
struct EvidencePill: View {
    @Environment(\.theme) private var theme
    let state: EvidenceState
    var label: String?

    init(_ state: EvidenceState, label: String? = nil) {
        self.state = state
        self.label = label
    }

    var body: some View {
        HStack(spacing: 5) {
            Text(state.symbol).font(.system(size: 11, weight: .bold))
            Text(label ?? state.label).textStyle(.pill).strikethrough(state == .invalidated).lineLimit(1)
        }
        .foregroundStyle(theme.foreground(state.tone))
        .padding(.horizontal, 8)
        .frame(height: 22)
        .background(theme.tint(state.tone), in: RoundedRectangle(cornerRadius: 6))
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
        case .superseded: return .neutral
        case .invalidated: return .danger
        case .stale: return .warn
        }
    }
}

/// Uppercase tag: 18 high, radius 4, a 1 pt ring in the text color and no fill. A revision
/// reference ("PRD rev 4") uses the Approved form.
struct DocumentTag: View {
    @Environment(\.theme) private var theme
    let tone: Tone
    let label: String
    var struck = false

    init(_ status: DocumentStatus, label: String? = nil) {
        tone = status.tone
        self.label = label ?? status.label
        struck = status == .invalidated
    }

    /// The other uppercase tags of the same shape ("Eligible", "Not available yet").
    init(_ label: String, tone: Tone) {
        self.tone = tone
        self.label = label
    }

    var body: some View {
        let color = theme.foreground(tone)
        Text(label)
            .textStyle(.tag)
            .strikethrough(struck)
            .lineLimit(1)
            .foregroundStyle(color)
            .padding(.horizontal, 6)
            .frame(height: 18)
            .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(color, lineWidth: 1))
            .fixedSize()
    }
}

/// A count beside a nav item or a group heading. A plain count is `text2` at 12; a count
/// that needs the human is a pill 18 high in its hue (blue for open questions and proposed
/// documents, orange for what waits on the human, red for violations).
struct CounterBadge: View {
    @Environment(\.theme) private var theme
    let count: Int
    var tone = Tone.neutral

    init(_ count: Int, tone: Tone = .neutral) {
        self.count = count
        self.tone = tone
    }

    var body: some View {
        if tone == .neutral {
            Text("\(count)").textStyle(.secondary).monospacedDigit().foregroundStyle(theme.text2).fixedSize()
        } else {
            Text("\(count)")
                .textStyle(.pill)
                .monospacedDigit()
                .foregroundStyle(theme.foreground(tone))
                .padding(.horizontal, 6)
                .frame(minWidth: 18, minHeight: 18)
                .background(theme.tint(tone), in: Capsule())
                .fixedSize()
        }
    }
}

/// A shortcut hint ("⌘K"), 11 in `text3`.
struct KeyCap: View {
    @Environment(\.theme) private var theme
    let keys: String

    init(_ keys: String) {
        self.keys = keys
    }

    var body: some View {
        Text(keys).textStyle(.kbd).foregroundStyle(theme.text3)
    }
}

/// The button kinds of COMPONENTS.md, section 3. One primary action per area; destructive
/// actions always confirm.
struct JojoButtonStyle: ButtonStyle {
    enum Kind: CaseIterable, Sendable {
        case primary, secondary, ghost, destructive
    }

    var kind = Kind.secondary
    /// Small: 28 high, text 12 (inspector actions, empty states, row actions).
    var compact = false

    func makeBody(configuration: Configuration) -> some View {
        Face(configuration: configuration, kind: kind, compact: compact)
    }

    private struct Face: View {
        @Environment(\.theme) private var theme
        @Environment(\.isEnabled) private var enabled
        @Environment(\.shadowLevel) private var level
        let configuration: Configuration
        let kind: Kind
        let compact: Bool

        var body: some View {
            let label = configuration.label
                .labelStyle(ButtonLabelStyle())
                .font(.system(size: compact ? 12 : 13, weight: .medium))
                .lineLimit(1)
                .foregroundStyle(foreground)
                .padding(.horizontal, 14)
                .frame(height: compact ? 28 : 32)
                .contentShape(Capsule())
                // Pressed is not drawn on the canvas; a custom style has no system pressed state.
                .opacity(configuration.isPressed ? 0.8 : 1)
            if !enabled {
                label.background(theme.grayBg, in: Capsule())
            } else {
                switch kind {
                case .primary:
                    label
                        .background(theme.accent, in: Capsule())
                        .overlay { if level != .none { Capsule().strokeBorder(topLight, lineWidth: 1) } }
                        .shadow(color: level == .full ? RGBA(0, 113, 227, 0.35).color : .clear, radius: 5, y: 2)
                case .secondary:
                    label.glassCapsule()
                case .ghost, .destructive:
                    label
                }
            }
        }

        /// The primary button's inner top highlight.
        private var topLight: LinearGradient {
            LinearGradient(stops: [.init(color: .white.opacity(level == .full ? 0.35 : 0.3), location: 0),
                                   .init(color: .clear, location: 0.1)],
                           startPoint: .top, endPoint: .bottom)
        }

        private var foreground: Color {
            guard enabled else { return theme.text3 }
            switch kind {
            case .primary: return theme.onAccent
            case .secondary: return theme.text
            case .ghost: return theme.accent
            case .destructive: return theme.red
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

/// A keyboard shortcut shown inside a button ("⌘↵"): 12 at 75%.
struct ShortcutHint: View {
    let keys: String

    init(_ keys: String) {
        self.keys = keys
    }

    var body: some View {
        Text(keys).font(.system(size: 12)).opacity(0.75)
    }
}

/// A disabled control always says why, in `secondary` style beside it.
struct DisabledReason: View {
    @Environment(\.theme) private var theme
    let text: String

    init(_ text: String) {
        self.text = text
    }

    var body: some View {
        HStack(spacing: 5) {
            Image(systemName: "info.circle").font(.system(size: 12))
            Text(text).textStyle(.secondary)
        }
        .foregroundStyle(theme.text3)
    }
}

/// A glass capsule group (padding 2) of icon buttons or related-view text buttons.
struct CapsuleGroup<Content: View>: View {
    @ViewBuilder let content: () -> Content

    var body: some View {
        HStack(spacing: 0) { content() }
            .padding(2)
            .glassCapsule()
    }
}

/// Icon button: a 32 × 32 capsule inside a `CapsuleGroup`, `grayBg` under the pointer.
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
            configuration.label
                .labelStyle(.iconOnly)
                .font(.system(size: 15))
                .foregroundStyle(enabled ? theme.text : theme.text3)
                .frame(width: 32, height: 32)
                .background(hovered && enabled ? theme.grayBg : .clear, in: Capsule())
                .contentShape(Capsule())
                .opacity(configuration.isPressed ? 0.8 : 1)
                .onHover { hovered = $0 }
        }
    }
}

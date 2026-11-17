import JojoCore
import SwiftUI

// The surfaces the feature screens of step 4 share (COMPONENTS.md, section 28):
// Specifications, Approval Inbox, Execution Plan, Agent Runs, Source Control and
// Verification & Evidence. Liquid Glass tokens only; the first-system surfaces of
// sections 14 to 24 are not used on these screens.

/// The feature a feature screen is about: the one Feature Chat holds, as `Features` (1.7)
/// reports it. Nil fields when no feature is held or the core does not report features.
struct FeatureScope: Equatable {
    var feature: String?
    var summary: FeatureSummary?

    init(feature: String? = nil, summary: FeatureSummary? = nil) {
        (self.feature, self.summary) = (feature, summary)
    }

    @MainActor init(workspace: WorkspaceModel) {
        let feature = workspace.chatFeature
        self.init(feature: feature, summary: workspace.features?.features.first { $0.id == feature })
    }

    /// "FT-3 Slugify from the shell".
    var title: String? {
        guard let summary else { return nil }
        return [summary.code, summary.name].compactMap { $0 }.joined(separator: " ")
    }

    /// The sub line of a feature screen's header: "Sample › FT-3 Slugify › Specifications".
    func place(_ last: String?) -> String {
        let parts = [summary?.projectName ?? (summary == nil ? "Workspace" : nil), title, last].compactMap { $0 }
        return parts.joined(separator: " › ")
    }

    /// Whether something of `feature` belongs to the held feature; everything does when none
    /// is held.
    func holds(_ feature: String?) -> Bool {
        guard let held = self.feature else { return true }
        return feature == held
    }
}

/// The lifecycle card of COMPONENTS.md, section 2.1: the core's five stages (1.7) with
/// the feature's counts after it. Also on Feature Chat.
struct LifecycleCard: View {
    let summary: FeatureSummary?

    var body: some View {
        HStack(spacing: 14) {
            LifecycleStepper(stages: summary?.stages)
            if let s = summary {
                HStack(spacing: 6) {
                    if s.running > 0 { OperationalPill("\(s.running) running", tone: .info) }
                    if s.blocked > 0 { OperationalPill("\(s.blocked) blocked", tone: .danger) }
                    if s.awaiting > 0 { OperationalPill("\(s.awaiting) awaiting approval", tone: .warn) }
                }
            }
            Spacer(minLength: 0)
        }
        .padding(EdgeInsets(top: 10, leading: 14, bottom: 10, trailing: 14))
        .cardElevation(radius: 14)
    }
}

/// One text button of a header capsule group, with why it is disabled.
struct HeaderButton: Identifiable {
    let title: String
    var disabled: String? = nil
    var action: () -> Void = {}
    var id: String { title }
}

/// A screen's header capsule group of 12 text buttons (the boards' "Review Plan",
/// "Approval History", "Inspect CI"); a disabled one says why in its help text.
struct HeaderButtons: View {
    let buttons: [HeaderButton]

    var body: some View {
        CapsuleGroup {
            ForEach(buttons) { b in
                Button(b.title, action: b.action).buttonStyle(CapsuleTextButtonStyle())
                    .disabled(b.disabled != nil)
                    .help(b.disabled ?? b.title)
            }
        }
    }
}

/// The trailing inspector of a feature screen: a glass panel (radius 18, padding 16) whose
/// sections stand 18 apart.
struct GlassInspector<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 18) { content }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .topLeading)
            .glassPanel(radius: 18)
    }
}

/// A section of an inspector or a column: a `label` heading, an optional trailing link,
/// then its content 6 to 10 below.
struct LabeledSection<Content: View>: View {
    let title: String
    var link: (title: String, action: () -> Void)? = nil
    var spacing: CGFloat = 8
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: spacing) {
            HStack {
                GroupLabel(title)
                Spacer(minLength: 6)
                if let link { TextLink(link.title, action: link.action).font(.system(size: 12, weight: .medium)) }
            }
            content
        }
    }
}

/// One row of a definition card: a key (`text2`, a fixed width) and its value, plain or
/// mono, with an optional pill.
struct Definition: Identifiable {
    let key: String
    let value: String
    var mono = false
    var pill: (String, Tone)? = nil
    var id: String { key + value }
}

/// The boards' `dl.card`: rows padding 9 12, a 0.5 `sep` between them, radius 12.
struct DefinitionCard: View {
    @Environment(\.theme) private var theme
    let rows: [Definition]
    var keyWidth: CGFloat = 96

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(rows.enumerated()), id: \.offset) { index, row in
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Text(row.key).textStyle(.secondary).foregroundStyle(theme.text2)
                        .frame(width: keyWidth, alignment: .leading)
                    VStack(alignment: .leading, spacing: 4) {
                        if let pill = row.pill { OperationalPill(pill.0, tone: pill.1) }
                        if !row.value.isEmpty {
                            Text(row.value).textStyle(row.mono ? .mono : .secondary).foregroundStyle(theme.text)
                                .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .padding(EdgeInsets(top: 9, leading: 12, bottom: 9, trailing: 12))
                .overlay(alignment: .top) { if index > 0 { Rectangle().fill(theme.sep).frame(height: 0.5) } }
            }
        }
        .cardElevation(radius: 12)
    }
}

/// The facts strip at the top of a detail (Agent Runs, Source Control): label over a 13
/// semibold value, cells at least 120 wide, on a card (radius 14, padding 12 14).
struct FactStrip: View {
    @Environment(\.theme) private var theme
    let facts: [(label: String, value: String, mono: Bool)]

    var body: some View {
        FlowRow(spacing: 12) {
            ForEach(Array(facts.enumerated()), id: \.offset) { _, fact in
                VStack(alignment: .leading, spacing: 2) {
                    Text(fact.label).textStyle(.label).foregroundStyle(theme.text3)
                    Text(fact.value)
                        .font(.system(size: 13, weight: .semibold, design: fact.mono ? .monospaced : .default))
                        .lineLimit(1).truncationMode(.middle).textSelection(.enabled)
                }
                .frame(minWidth: 110, alignment: .leading)
            }
        }
        .padding(EdgeInsets(top: 12, leading: 14, bottom: 12, trailing: 14))
        .frame(maxWidth: .infinity, alignment: .leading)
        .cardElevation(radius: 14)
    }
}

/// Lays its children out left to right and wraps them.
struct FlowRow: Layout {
    var spacing: CGFloat = 8

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        var (x, y, line, widest) = (CGFloat(0), CGFloat(0), CGFloat(0), CGFloat(0))
        for view in subviews {
            let size = view.sizeThatFits(.unspecified)
            if x > 0 && x + size.width > width { (x, y, line) = (0, y + line + spacing, 0) }
            x += size.width + spacing
            line = max(line, size.height)
            widest = max(widest, x - spacing)
        }
        return CGSize(width: proposal.width ?? widest, height: y + line)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var (x, y, line) = (bounds.minX, bounds.minY, CGFloat(0))
        for view in subviews {
            let size = view.sizeThatFits(.unspecified)
            if x > bounds.minX && x + size.width > bounds.maxX { (x, y, line) = (bounds.minX, y + line + spacing, 0) }
            view.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
            x += size.width + spacing
            line = max(line, size.height)
        }
    }
}

/// A 26 × 26 tile (radius 7) with two or three letters or a glyph on a fixed color: the
/// documents' "PRD", "ADR", "PLAN".
struct LetterTile: View {
    let text: String
    let color: RGBA
    var size: CGFloat = 26

    var body: some View {
        Text(text)
            .font(.system(size: text.count > 3 ? 8 : 10, weight: .bold))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(color.color, in: RoundedRectangle(cornerRadius: size * 0.27))
    }
}

/// A selectable list row of a leading column (radius 10, padding 8 10): chosen, `accentTint`;
/// under the pointer, `cardHover`.
struct SelectableRow<Content: View>: View {
    @Environment(\.theme) private var theme
    let selected: Bool
    let action: () -> Void
    @ViewBuilder var content: Content
    @State private var hovered = false

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) { content }
                .padding(EdgeInsets(top: 8, leading: 10, bottom: 8, trailing: 10))
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(selected ? theme.accentTint : hovered ? theme.cardHover : .clear,
                            in: RoundedRectangle(cornerRadius: 10))
                .contentShape(RoundedRectangle(cornerRadius: 10))
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// A tinted notice (radius 14, padding 12 14) with a title, a sentence and trailing
/// buttons: the boards' revision, scope and stale banners.
struct TintedNotice<Actions: View>: View {
    @Environment(\.theme) private var theme
    let tone: Tone
    let title: String
    let message: String
    @ViewBuilder var actions: Actions

    var body: some View {
        HStack(alignment: .center, spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title).textStyle(.bodyStrong)
                Text(message).textStyle(.secondary).fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            actions
        }
        .padding(EdgeInsets(top: 12, leading: 14, bottom: 12, trailing: 14))
        .background(theme.tint(tone), in: RoundedRectangle(cornerRadius: 14))
    }
}

/// The pill of a document's approval state as the Liquid Glass boards draw it.
func approvalPill(_ approval: String, stale: Bool = false, superseded: Bool = false) -> (String, Tone) {
    switch approval {
    case "Pending": return stale ? ("Stale", .warn) : ("Proposed", .info)
    case "Approved": return superseded ? ("Superseded", .neutral) : ("Approved", .ok)
    case "Rejected": return ("Rejected", .danger)
    case "Revoked": return ("Revoked", .danger)
    case "NotReady": return ("Has findings", .warn)
    default: return ("Waiting", .neutral)
    }
}

/// The tile letters and color of a document kind.
func documentTile(_ kind: String) -> (String, RGBA) {
    switch kind {
    case "Prd": return ("PRD", Palette.blue)
    case "Adr": return ("ADR", Palette.indigo)
    case "ExecutionPlan": return ("PLAN", Palette.blue)
    case "ValidationPlan": return ("VAL", Palette.teal)
    default: return ("DOC", Palette.gray)
    }
}

/// "ADR", "PRD", "Plan", "Validation plan": a document kind in a heading.
func documentCode(_ kind: String) -> String {
    switch kind {
    case "Prd": return "PRD"
    case "Adr": return "ADR"
    case "ExecutionPlan": return "Plan"
    case "ValidationPlan": return "Validation plan"
    default: return "Document"
    }
}

/// The sentence for something the core does not report, said where the board has a value.
struct NotReported: View {
    @Environment(\.theme) private var theme
    let text: String

    init(_ text: String = "Not reported") { self.text = text }

    var body: some View {
        Text(text).textStyle(.secondary).foregroundStyle(theme.text3).fixedSize(horizontal: false, vertical: true)
    }
}

/// Three columns from `wide`, two from `medium`, one below: a leading column, the main
/// one and the inspector, as the feature boards lay them out.
struct FeatureColumns<Leading: View, Main: View, Trailing: View>: View {
    var leadingWidth: CGFloat = 250
    var trailingWidth: CGFloat = 320
    var wide: CGFloat = 1000
    var medium: CGFloat = 720
    @ViewBuilder var leading: Leading
    @ViewBuilder var main: Main
    @ViewBuilder var trailing: Trailing

    var body: some View {
        GeometryReader { geometry in
            let width = geometry.size.width
            Scrolling {
                Group {
                    if width >= wide {
                        HStack(alignment: .top, spacing: 14) {
                            leading.frame(width: leadingWidth)
                            main.frame(maxWidth: .infinity)
                            trailing.frame(width: trailingWidth)
                        }
                    } else if width >= medium {
                        HStack(alignment: .top, spacing: 14) {
                            VStack(alignment: .leading, spacing: 14) { leading; trailing }.frame(width: leadingWidth + 20)
                            main.frame(maxWidth: .infinity)
                        }
                    } else {
                        VStack(alignment: .leading, spacing: 14) { leading; main; trailing }
                    }
                }
                .padding(EdgeInsets(top: 4, leading: 10, bottom: 14, trailing: 12))
            }
        }
    }
}

/// A clock time or a day, as the boards write them: "09:21" today, "Nov 13 10:02" before.
func boardTime(_ date: Date?, now: Date = Date()) -> String? {
    guard let date else { return nil }
    if Calendar.current.isDate(date, inSameDayAs: now) {
        return date.formatted(.dateTime.hour(.twoDigits(amPM: .omitted)).minute(.twoDigits))
    }
    return date.formatted(.dateTime.month(.abbreviated).day().hour(.twoDigits(amPM: .omitted)).minute(.twoDigits))
}

/// "12m 08s", "1h 04m": a duration as Agent Runs writes it.
func elapsedText(from start: Date?, to end: Date?) -> String? {
    guard let start, let end, end >= start else { return nil }
    let s = Int(end.timeIntervalSince(start))
    if s >= 3600 { return String(format: "%dh %02dm", s / 3600, (s % 3600) / 60) }
    return String(format: "%dm %02ds", s / 60, s % 60)
}

/// Where the core connection stands, in the words of the Recovery Center's banners.
func coreLink(_ connection: HelperConnection) -> RecoveryCenterState.Link {
    switch connection {
    case .ready: return .connected
    case .notStarted, .handshaking: return .starting
    case .incompatible: return .incompatible
    case .launchFailed, .channelLost, .ended: return .lost
    }
}

/// The banners every feature screen starts with: a lost or incompatible core, and a
/// query this core does not list.
struct CoreBanners: View {
    let connection: HelperConnection
    var missing: (query: String, what: String)? = nil

    var body: some View {
        switch coreLink(connection) {
        case .lost:
            Banner(.warning, title: "Jojo core isn't responding", message: "Showing the last known state, read-only.")
        case .incompatible:
            Banner(.danger, title: "The core speaks another protocol version", message: "Diagnostics only. Nothing can be sent.")
        case .starting, .connected:
            EmptyView()
        }
        if let missing {
            Banner(.warning, title: "This core does not report \(missing.what)",
                   message: "It does not list the \(missing.query) query (protocol 1.9). Start a newer helper.")
        }
    }
}

/// The main column and the inspector beside it from `wide`, below it under that.
struct InspectorColumns<Main: View, Trailing: View>: View {
    var trailingWidth: CGFloat = 320
    var wide: CGFloat = 900
    @ViewBuilder var main: Main
    @ViewBuilder var trailing: Trailing

    var body: some View {
        GeometryReader { geometry in
            Scrolling {
                Group {
                    if geometry.size.width >= wide {
                        HStack(alignment: .top, spacing: 14) {
                            main.frame(maxWidth: .infinity)
                            trailing.frame(width: trailingWidth)
                        }
                    } else {
                        VStack(alignment: .leading, spacing: 14) { main; trailing }
                    }
                }
                .padding(EdgeInsets(top: 4, leading: 10, bottom: 14, trailing: 12))
            }
        }
    }
}

import SwiftUI

// Shared surfaces of docs/design/COMPONENTS.md: banner (section 11), card and list row
// (section 5), and the state forms of docs/design/STATES.md that screens repeat.

private struct OffscreenKey: EnvironmentKey {
    static let defaultValue = false
}

extension EnvironmentValues {
    /// Set by the off-screen snapshot: a scroll view does not draw there, so content is laid
    /// out in place instead.
    var offscreen: Bool {
        get { self[OffscreenKey.self] }
        set { self[OffscreenKey.self] = newValue }
    }
}

/// A vertical scroll view on screen; plain content in an off-screen snapshot.
struct Scrolling<Content: View>: View {
    @Environment(\.offscreen) private var offscreen
    @ViewBuilder let content: () -> Content

    var body: some View {
        if offscreen {
            Color.clear.overlay(alignment: .top) { content() }.clipped()
        } else {
            ScrollView { content() }
        }
    }
}

/// Full-width notice above data that cannot be acted on as usual. One sentence, at most one
/// compact button.
struct Banner<Action: View>: View {
    enum Kind: CaseIterable, Sendable {
        case warning, danger

        var tone: Tone { self == .warning ? .warn : .danger }
        var symbol: String { self == .warning ? "exclamationmark.triangle.fill" : "exclamationmark.octagon.fill" }
    }

    @Environment(\.theme) private var theme
    let kind: Kind
    let title: String
    let message: String
    @ViewBuilder let action: () -> Action

    init(_ kind: Kind, title: String, message: String, @ViewBuilder action: @escaping () -> Action) {
        self.kind = kind
        self.title = title
        self.message = message
        self.action = action
    }

    var body: some View {
        let tone = theme.foreground(kind.tone)
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: kind.symbol).font(.system(size: 15)).foregroundStyle(tone)
                .accessibilityLabel(kind == .warning ? "Warning" : "Problem")
            VStack(alignment: .leading, spacing: 2) {
                Text(title).textStyle(.bodyStrong).foregroundStyle(theme.text)
                Text(message).textStyle(.body).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            action()
        }
        .padding(.vertical, 10)
        .padding(.horizontal, 14)
        .background(tone.opacity(0.08), in: RoundedRectangle(cornerRadius: 9))
        .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(tone.opacity(0.28), lineWidth: 1))
    }
}

extension Banner where Action == EmptyView {
    init(_ kind: Kind, title: String, message: String) {
        self.init(kind, title: title, message: message) { EmptyView() }
    }
}

/// Radius and padding of a card.
struct CardShape: Equatable, Sendable {
    var radius: CGFloat
    var vertical: CGFloat
    var horizontal: CGFloat

    static let page = CardShape(radius: 12, vertical: 14, horizontal: 16)
    static let panel = CardShape(radius: 10, vertical: 12, horizontal: 14)
    static let list = CardShape(radius: 12, vertical: 0, horizontal: 0)
    static let panelList = CardShape(radius: 10, vertical: 0, horizontal: 0)
}

/// Card: `surface`, card elevation, content clipped to the radius. `Card()` is the page card
/// (radius 12, padding 14 by 16); `.panel` is the smaller card used inside a screen column;
/// `.list` has no padding, for rows.
struct Card<Content: View>: View {
    var shape = CardShape.page
    @ViewBuilder let content: () -> Content

    init(_ shape: CardShape = .page, @ViewBuilder content: @escaping () -> Content) {
        self.shape = shape
        self.content = content
    }

    var body: some View {
        content()
            .padding(.vertical, shape.vertical)
            .padding(.horizontal, shape.horizontal)
            .frame(maxWidth: .infinity, alignment: .leading)
            .clipShape(RoundedRectangle(cornerRadius: shape.radius))
            .cardElevation(radius: shape.radius)
    }
}

/// Rows of a list card, separated by a hairline; the last row has none.
struct Rows<Item: Identifiable, Row: View>: View {
    @Environment(\.theme) private var theme
    let items: [Item]
    @ViewBuilder let row: (Item) -> Row

    init(_ items: [Item], @ViewBuilder row: @escaping (Item) -> Row) {
        self.items = items
        self.row = row
    }

    var body: some View {
        VStack(spacing: 0) {
            ForEach(items) { item in
                row(item)
                if item.id != items.last?.id {
                    Rectangle().fill(theme.hairline).frame(height: 1)
                }
            }
        }
    }
}

/// Group header of a list: uppercase label on `fill2`, optional counter, hairline below.
struct GroupHeader: View {
    @Environment(\.theme) private var theme
    let title: String
    var count: Int?

    init(_ title: String, count: Int? = nil) {
        self.title = title
        self.count = count
    }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Text(title).textStyle(.groupHeader).foregroundStyle(theme.text3)
                if let count { CounterBadge(count) }
                Spacer(minLength: 0)
            }
            .padding(EdgeInsets(top: 9, leading: 16, bottom: 7, trailing: 16))
            .background(theme.fill2)
            Rectangle().fill(theme.hairline).frame(height: 1)
        }
    }
}

/// 26 × 26 leading tile of a row: fixed color by kind, white glyph.
struct LeadingTile: View {
    @Environment(\.theme) private var theme
    let symbol: String
    var tone: Tone = .neutral

    var body: some View {
        RoundedRectangle(cornerRadius: 7)
            .fill(tone == .neutral ? theme.tileNeutral : theme.foreground(tone))
            .frame(width: 26, height: 26)
            .overlay(Image(systemName: symbol).font(.system(size: 13, weight: .medium)).foregroundStyle(theme.onAccent))
            .accessibilityHidden(true)
    }
}

/// List row: leading slot, title with meta, a second line, one trailing status component and
/// an optional chevron. Each text line is cut with an ellipsis unless `wraps` is set.
struct ListRow<Leading: View, Trailing: View>: View {
    @Environment(\.theme) private var theme
    @State private var hovered = false
    let title: String
    var meta: String?
    var detail: String?
    var chevron = false
    /// The second line may wrap (a finding's sentence) and the row aligns to the top.
    var wraps = false
    var inset: CGFloat = 16
    @ViewBuilder let leading: () -> Leading
    @ViewBuilder let trailing: () -> Trailing

    init(_ title: String, meta: String? = nil, detail: String? = nil, chevron: Bool = false, wraps: Bool = false,
         inset: CGFloat = 16, @ViewBuilder leading: @escaping () -> Leading,
         @ViewBuilder trailing: @escaping () -> Trailing) {
        self.title = title
        self.meta = meta
        self.detail = detail
        self.chevron = chevron
        self.wraps = wraps
        self.inset = inset
        self.leading = leading
        self.trailing = trailing
    }

    var body: some View {
        HStack(alignment: wraps ? .top : .center, spacing: 12) {
            leading()
            VStack(alignment: .leading, spacing: wraps ? 3 : 1) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(title).textStyle(.bodyStrong).foregroundStyle(theme.textStrong).lineLimit(1)
                    if let meta { Text(meta).textStyle(.meta).foregroundStyle(theme.text3).lineLimit(1) }
                }
                if let detail {
                    Text(detail).textStyle(wraps ? .body : .secondary).foregroundStyle(theme.text2)
                        .lineLimit(wraps ? nil : 1)
                        .fixedSize(horizontal: false, vertical: wraps)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            trailing()
            if chevron {
                Image(systemName: "chevron.right").font(.system(size: 12, weight: .semibold)).foregroundStyle(theme.text4)
                    .accessibilityHidden(true)
            }
        }
        .padding(.vertical, 11)
        .padding(.horizontal, inset)
        .background(hovered ? theme.hover : .clear)
        .onHover { hovered = $0 }
    }
}

extension ListRow where Leading == EmptyView, Trailing == EmptyView {
    init(_ title: String, meta: String? = nil, detail: String? = nil, chevron: Bool = false) {
        self.init(title, meta: meta, detail: detail, chevron: chevron, leading: { EmptyView() }, trailing: { EmptyView() })
    }
}

/// Section heading inside a screen column: 12 semibold `text2`.
struct SectionHeading<Trailing: View>: View {
    @Environment(\.theme) private var theme
    let title: String
    @ViewBuilder let trailing: () -> Trailing

    init(_ title: String, @ViewBuilder trailing: @escaping () -> Trailing) {
        self.title = title
        self.trailing = trailing
    }

    var body: some View {
        HStack(spacing: 12) {
            Text(title).font(.system(size: 12, weight: .semibold)).tracking(0.12).foregroundStyle(theme.text2)
                .accessibilityAddTraits(.isHeader)
            Spacer(minLength: 0)
            trailing()
        }
        .frame(minHeight: 22)
        .padding(.horizontal, 2)
    }
}

extension SectionHeading where Trailing == EmptyView {
    init(_ title: String) {
        self.init(title) { EmptyView() }
    }
}

/// A small note under a card: info glyph and one sentence in `text3`.
struct Footnote: View {
    @Environment(\.theme) private var theme
    let text: String

    init(_ text: String) {
        self.text = text
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Image(systemName: "info.circle").font(.system(size: 12)).accessibilityHidden(true)
            Text(text).textStyle(.meta).lineSpacing(2).fixedSize(horizontal: false, vertical: true)
        }
        .foregroundStyle(theme.text3)
    }
}

/// The centered block of STATES.md (Empty, Error, Permission denied): glyph, title, one
/// sentence, buttons.
struct StateBlock<Actions: View>: View {
    @Environment(\.theme) private var theme
    let symbol: String
    let title: String
    let message: String
    @ViewBuilder let actions: () -> Actions

    init(symbol: String, title: String, message: String, @ViewBuilder actions: @escaping () -> Actions) {
        self.symbol = symbol
        self.title = title
        self.message = message
        self.actions = actions
    }

    var body: some View {
        VStack(spacing: 8) {
            Image(systemName: symbol).font(.system(size: 24)).foregroundStyle(theme.text3).accessibilityHidden(true)
            Text(title).textStyle(.bodyStrong).foregroundStyle(theme.textStrong)
            Text(message).textStyle(.body).foregroundStyle(theme.text2).multilineTextAlignment(.center)
                .frame(maxWidth: 320)
            HStack(spacing: 8) { actions() }.padding(.top, 4)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

extension StateBlock where Actions == EmptyView {
    init(symbol: String, title: String, message: String) {
        self.init(symbol: symbol, title: title, message: message) { EmptyView() }
    }
}

/// The Loading form of STATES.md: what loads, then three skeleton bars.
struct LoadingBlock: View {
    @Environment(\.theme) private var theme
    let text: String

    init(_ text: String) {
        self.text = text
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(text).textStyle(.body).foregroundStyle(theme.text2)
            ForEach([0.7, 0.9, 0.55], id: \.self) { width in
                GeometryReader { geometry in
                    RoundedRectangle(cornerRadius: 5).fill(theme.fill).frame(width: geometry.size.width * width)
                }
                .frame(height: 10)
            }
        }
        .frame(maxWidth: 420, alignment: .leading)
        .accessibilityElement(children: .combine)
    }
}

/// A drawn checkbox: `accent` with a white check when on.
struct CheckBox: View {
    @Environment(\.theme) private var theme
    let isOn: Bool

    var body: some View {
        RoundedRectangle(cornerRadius: 3.5)
            .fill(isOn ? theme.accent : .clear)
            .overlay(RoundedRectangle(cornerRadius: 3.5).strokeBorder(isOn ? theme.primaryBorder : theme.text4, lineWidth: 1))
            .overlay(Image(systemName: "checkmark").font(.system(size: 9, weight: .bold))
                .foregroundStyle(theme.onAccent).opacity(isOn ? 1 : 0))
            .frame(width: 14, height: 14)
    }
}

// MARK: Attempt surfaces (COMPONENTS.md, section 15)

/// One fact of a summary strip: a label in `text3` over its value.
struct LabeledField<Content: View>: View {
    @Environment(\.theme) private var theme
    let label: String
    @ViewBuilder let content: () -> Content

    init(_ label: String, @ViewBuilder content: @escaping () -> Content) {
        self.label = label
        self.content = content
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(label).textStyle(.meta).foregroundStyle(theme.text3)
            content()
        }
    }
}

/// A row that continues on further lines when it runs out of width.
struct WrappingRow: Layout {
    var spacing: CGFloat = 28
    var lineSpacing: CGFloat = 14

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        arrange(width: proposal.width ?? .infinity, subviews).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let origins = arrange(width: bounds.width, subviews).origins
        for (subview, origin) in zip(subviews, origins) {
            subview.place(at: CGPoint(x: bounds.minX + origin.x, y: bounds.minY + origin.y),
                          proposal: ProposedViewSize(width: bounds.width, height: nil))
        }
    }

    private func arrange(width: CGFloat, _ subviews: Subviews) -> (origins: [CGPoint], size: CGSize) {
        var origins: [CGPoint] = []
        var x: CGFloat = 0, y: CGFloat = 0, lineHeight: CGFloat = 0, widest: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(ProposedViewSize(width: width, height: nil))
            if x > 0, x + size.width > width {
                (x, y, lineHeight) = (0, y + lineHeight + lineSpacing, 0)
            }
            origins.append(CGPoint(x: x, y: y))
            lineHeight = max(lineHeight, size.height)
            widest = max(widest, x + size.width)
            x += size.width + spacing
        }
        return (origins, CGSize(width: widest, height: y + lineHeight))
    }
}

struct TimelineEntry: Identifiable, Equatable, Sendable {
    let text: String
    /// `neutral` is a record that is neither done nor lost: an empty ring.
    var tone = Tone.ok
    var time: String?
    var id: String { text }
}

/// Entries, oldest first, joined by a connector; the last entry has none.
struct Timeline: View {
    @Environment(\.theme) private var theme
    let entries: [TimelineEntry]

    init(_ entries: [TimelineEntry]) {
        self.entries = entries
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(entries.enumerated()), id: \.element.id) { index, entry in
                HStack(alignment: .top, spacing: 12) {
                    dot(entry.tone).padding(.top, 4)
                    Text(entry.text).textStyle(.body).foregroundStyle(theme.text)
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    if let time = entry.time {
                        Text(time).textStyle(.mono).foregroundStyle(theme.text3)
                    }
                }
                .padding(.vertical, 5)
                .background(alignment: .topLeading) {
                    if index < entries.count - 1 {
                        Rectangle().fill(theme.hairline).frame(width: 1)
                            .padding(EdgeInsets(top: 18, leading: 4, bottom: -6, trailing: 0))
                    }
                }
            }
        }
    }

    @ViewBuilder
    private func dot(_ tone: Tone) -> some View {
        if tone == .neutral {
            Circle().strokeBorder(theme.text4, lineWidth: 1.5).frame(width: 9, height: 9)
        } else {
            Circle().fill(theme.foreground(tone)).frame(width: 9, height: 9)
        }
    }
}

struct ConsoleLine: Identifiable, Equatable, Sendable {
    enum Kind: Equatable, Sendable {
        case plain, command, status(Tone)
    }

    let id: Int
    let text: String
    var kind = Kind.plain
}

/// Terminal output: monospaced, selectable, wrapping lines on the console background.
struct Console: View {
    @Environment(\.theme) private var theme
    let lines: [ConsoleLine]

    init(_ lines: [ConsoleLine]) {
        self.lines = lines
    }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 10)
        VStack(alignment: .leading, spacing: 0) {
            ForEach(lines) { line in
                Text(line.text).font(.system(size: 12, design: .monospaced)).foregroundStyle(color(line.kind))
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, minHeight: 21, alignment: .leading)
            }
        }
        .textSelection(.enabled)
        .padding(.vertical, 12)
        .padding(.horizontal, 14)
        .background(Console.background(dark: theme.isDark).color, in: shape)
        .overlay(shape.strokeBorder(theme.cardRing, lineWidth: 1))
    }

    /// The canvas has the dark value only.
    static func background(dark: Bool) -> RGBA {
        RGBA(dark ? 0x111113 : 0xF1F1F4)
    }

    private func color(_ kind: ConsoleLine.Kind) -> Color {
        switch kind {
        case .plain: return theme.text2
        case .command: return theme.text
        case .status(let tone): return theme.foreground(tone)
        }
    }
}

/// One titled block of an inspector, separated from the next by a hairline.
struct InspectorSection<Content: View>: View {
    @Environment(\.theme) private var theme
    let title: String
    var last = false
    @ViewBuilder let content: () -> Content

    init(_ title: String, last: Bool = false, @ViewBuilder content: @escaping () -> Content) {
        self.title = title
        self.last = last
        self.content = content
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title).font(.system(size: 11.5, weight: .semibold)).foregroundStyle(theme.text3)
                .accessibilityAddTraits(.isHeader)
            content()
        }
        .padding(.vertical, 12)
        .padding(.horizontal, 16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .overlay(alignment: .bottom) { if !last { Rectangle().fill(theme.hairline).frame(height: 1) } }
    }
}

/// The two-column facts of an inspector section: `Fact` rows.
struct FactGrid<Content: View>: View {
    @ViewBuilder let content: () -> Content

    var body: some View {
        Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 10, verticalSpacing: 7) { content() }
    }
}

struct Fact<Content: View>: View {
    @Environment(\.theme) private var theme
    let label: String
    @ViewBuilder let content: () -> Content

    init(_ label: String, @ViewBuilder content: @escaping () -> Content) {
        self.label = label
        self.content = content
    }

    var body: some View {
        GridRow {
            Text(label).font(.system(size: 12.5)).foregroundStyle(theme.text3).frame(width: 104, alignment: .leading)
            content().foregroundStyle(theme.text).frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

extension Fact where Content == Text {
    /// A plain value at 12.5 in `text`.
    init(_ label: String, _ value: String) {
        self.init(label) { Text(value).font(.system(size: 12.5)) }
    }
}

/// A quiet well for the one sentence that says what an action does not mean.
struct NoticeWell: View {
    @Environment(\.theme) private var theme
    let text: String

    init(_ text: String) {
        self.text = text
    }

    var body: some View {
        Text(text).textStyle(.meta).foregroundStyle(theme.text2)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.vertical, 8)
            .padding(.horizontal, 10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(theme.fill2, in: RoundedRectangle(cornerRadius: 8))
    }
}

// MARK: Verification surfaces (docs/design/COMPONENTS.md, sections 10 and 16)

/// A one-line text field. In the off-screen snapshot, where a system field does not draw,
/// its text or prompt is drawn in place.
struct InputField: View {
    @Environment(\.theme) private var theme
    @Environment(\.offscreen) private var offscreen
    let prompt: String
    @Binding var text: String
    var mono = false
    var submit: () -> Void = {}

    var body: some View {
        Group {
            if offscreen {
                Text(text.isEmpty ? prompt : text).foregroundStyle(text.isEmpty ? theme.text3 : theme.text).lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
            } else {
                TextField("", text: $text, prompt: Text(prompt).foregroundColor(theme.text3))
                    .textFieldStyle(.plain).foregroundStyle(theme.text).onSubmit(submit)
            }
        }
        .font(.system(size: 12, design: mono ? .monospaced : .default))
        .padding(.horizontal, 8)
        .frame(height: 26)
        .background(theme.fill, in: RoundedRectangle(cornerRadius: 6))
        .accessibilityLabel(prompt)
    }
}

/// One verification result: name, evidence pill, an optional count, the command or the
/// core's detail over a note, and one trailing control.
struct ResultRow<Trailing: View>: View {
    @Environment(\.theme) private var theme
    let name: String
    var scope: String?
    let state: EvidenceState
    var label: String?
    var count: String?
    var command: String?
    var note: String?
    @ViewBuilder let trailing: () -> Trailing

    init(_ name: String, scope: String? = nil, state: EvidenceState, label: String? = nil, count: String? = nil,
         command: String? = nil, note: String? = nil, @ViewBuilder trailing: @escaping () -> Trailing) {
        self.name = name
        self.scope = scope
        self.state = state
        self.label = label
        self.count = count
        self.command = command
        self.note = note
        self.trailing = trailing
    }

    var body: some View {
        HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 0) {
                Text(name).textStyle(.bodyStrong).foregroundStyle(theme.textStrong)
                if let scope { Text(scope).textStyle(.meta).foregroundStyle(theme.text3) }
            }
            .frame(width: 150, alignment: .leading)
            EvidencePill(state, label: label).frame(width: 130, alignment: .leading)
            if let count { Text(count).textStyle(.mono).foregroundStyle(theme.text).frame(width: 80, alignment: .leading) }
            VStack(alignment: .leading, spacing: 2) {
                if let command { Text(command).textStyle(.mono).foregroundStyle(theme.text) }
                if let note { Text(note).textStyle(.meta).foregroundStyle(theme.text3) }
            }
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            trailing()
        }
        .textSelection(.enabled)
        .padding(.vertical, 10)
        .padding(.horizontal, 14)
    }
}

extension ResultRow where Trailing == EmptyView {
    init(_ name: String, scope: String? = nil, state: EvidenceState, label: String? = nil, count: String? = nil,
         command: String? = nil, note: String? = nil) {
        self.init(name, scope: scope, state: state, label: label, count: count, command: command, note: note) { EmptyView() }
    }
}

/// One record of evidence: its name, a short note such as a digest, and an evidence pill.
struct EvidenceRow: View {
    @Environment(\.theme) private var theme
    let name: String
    var note: String?
    let state: EvidenceState
    var label: String?
    /// The name is an identifier (a path).
    var mono = false

    init(_ name: String, note: String? = nil, state: EvidenceState, label: String? = nil, mono: Bool = false) {
        self.name = name
        self.note = note
        self.state = state
        self.label = label
        self.mono = mono
    }

    var body: some View {
        HStack(spacing: 10) {
            Text(name).textStyle(mono ? .mono : .body).foregroundStyle(theme.text)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
            if let note { Text(note).textStyle(.mono).foregroundStyle(theme.text3).lineLimit(1) }
            EvidencePill(state, label: label)
        }
        .textSelection(.enabled)
        .padding(.vertical, 9)
        .padding(.horizontal, 14)
    }
}

/// One line of a unified diff, numbered from its hunk header.
struct DiffLine: Identifiable, Equatable, Sendable {
    enum Kind: Equatable, Sendable {
        /// A file or hunk header, or any line outside a hunk: no number and no sign.
        case context, added, removed, header
    }

    let id: Int
    let kind: Kind
    /// Removed lines carry the old file's number, added and context lines the new one's.
    var number: Int?
    let text: String

    static func parse(_ diff: String) -> [DiffLine] {
        var lines: [DiffLine] = []
        // Lines the current hunk still has in the old and in the new file.
        var (old, new, oldLeft, newLeft) = (0, 0, 0, 0)
        var rows = diff.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        if rows.last == "" { rows.removeLast() }
        for row in rows {
            let id = lines.count
            if oldLeft > 0 || newLeft > 0, let sign = row.first, "+- ".contains(sign) {
                let text = String(row.dropFirst())
                switch sign {
                case "+":
                    lines.append(DiffLine(id: id, kind: .added, number: new, text: text))
                    (new, newLeft) = (new + 1, newLeft - 1)
                case "-":
                    lines.append(DiffLine(id: id, kind: .removed, number: old, text: text))
                    (old, oldLeft) = (old + 1, oldLeft - 1)
                default:
                    lines.append(DiffLine(id: id, kind: .context, number: new, text: text))
                    (old, new, oldLeft, newLeft) = (old + 1, new + 1, oldLeft - 1, newLeft - 1)
                }
                continue
            }
            if row.hasPrefix("@@"), let hunk = hunk(row) {
                (old, oldLeft, new, newLeft) = hunk
            } else if !row.hasPrefix("\\") {
                // Anything else outside the counted lines ends the hunk.
                (oldLeft, newLeft) = (0, 0)
            }
            lines.append(DiffLine(id: id, kind: .header, number: nil, text: row))
        }
        return lines
    }

    /// "@@ -12,7 +12,9 @@": start and length in the old and in the new file; a length left out is 1.
    private static func hunk(_ row: String) -> (Int, Int, Int, Int)? {
        let parts = row.split(separator: " ")
        guard parts.count >= 3, parts[1].hasPrefix("-"), parts[2].hasPrefix("+") else { return nil }
        func range(_ part: Substring) -> (Int, Int)? {
            let numbers = part.dropFirst().split(separator: ",").map { Int($0) }
            guard let first = numbers.first, let start = first else { return nil }
            guard numbers.count > 1 else { return (start, 1) }
            return numbers[1].map { (start, $0) }
        }
        guard let before = range(parts[1]), let after = range(parts[2]) else { return nil }
        return (before.0, before.1, after.0, after.1)
    }
}

/// A unified diff: line number, sign and code, with added and removed lines tinted.
struct DiffView: View {
    @Environment(\.theme) private var theme
    let lines: [DiffLine]

    init(_ lines: [DiffLine]) {
        self.lines = lines
    }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 10)
        VStack(alignment: .leading, spacing: 0) {
            ForEach(lines) { line in
                HStack(alignment: .firstTextBaseline, spacing: 0) {
                    Text(line.number.map(String.init) ?? "").foregroundStyle(theme.text3).frame(width: 34, alignment: .trailing)
                    Text(sign(line.kind)).foregroundStyle(signColor(line.kind)).frame(width: 16)
                    Text(line.text).foregroundStyle(line.kind == .header ? theme.text3 : theme.text)
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .padding(.trailing, 14)
                .frame(minHeight: 22)
                .background(background(line.kind))
            }
        }
        .font(.system(size: 12, design: .monospaced))
        .textSelection(.enabled)
        .padding(.vertical, 6)
        .background(DiffView.background(dark: theme.isDark).color)
        .clipShape(shape)
        .overlay(shape.strokeBorder(theme.cardRing, lineWidth: 1))
    }

    /// The canvas has the dark value only.
    static func background(dark: Bool) -> RGBA {
        RGBA(dark ? 0x17171A : 0xF1F1F4)
    }

    private func sign(_ kind: DiffLine.Kind) -> String {
        switch kind {
        case .added: return "+"
        case .removed: return "-"
        case .context, .header: return ""
        }
    }

    private func signColor(_ kind: DiffLine.Kind) -> Color {
        kind == .added ? theme.ok : theme.danger
    }

    private func background(_ kind: DiffLine.Kind) -> Color {
        switch kind {
        case .added: return theme.ok.opacity(0.09)
        case .removed: return theme.danger.opacity(0.10)
        case .context, .header: return .clear
        }
    }
}

/// A command was sent and the core has not confirmed it (STATES.md, "Requested").
struct RequestedNotice: View {
    @Environment(\.theme) private var theme
    var text = "Sent to core · waiting for confirmation"

    var body: some View {
        HStack(spacing: 8) {
            OperationalPill(.waiting, label: "Requested")
            Text(text).textStyle(.meta).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(theme.fill2, in: RoundedRectangle(cornerRadius: 8))
    }
}

// MARK: Source control surfaces (COMPONENTS.md, section 17)

/// The kind of change of a path as one letter on its status color.
struct ChangeBadge: View {
    @Environment(\.theme) private var theme
    let letter: String
    let tone: Tone
    /// The kind in words, for the tooltip and for VoiceOver.
    let name: String

    init(_ letter: String, tone: Tone, name: String) {
        self.letter = letter
        self.tone = tone
        self.name = name
    }

    var body: some View {
        Text(letter).font(.system(size: 10.5, weight: .semibold))
            .foregroundStyle(theme.foreground(tone))
            .frame(width: 15, height: 15)
            .background(theme.foreground(tone).opacity(0.16), in: RoundedRectangle(cornerRadius: 4))
            .help(name)
            .accessibilityLabel(name)
    }
}

/// One changed path: badge, file name and line counts over its directory and, when the
/// path is not plainly in scope, an evidence pill.
struct ChangedFileRow: View {
    @Environment(\.theme) private var theme
    let name: String
    let directory: String
    let badge: ChangeBadge
    var counts: String?
    var scope: (state: EvidenceState, label: String)?
    /// Outside the approved scope: tinted, with a ring.
    var flagged = false
    var selected = false

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 7)
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 8) {
                badge
                Text(name).font(.system(size: 12.5)).foregroundStyle(theme.text).lineLimit(1).truncationMode(.middle)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if let counts { Text(counts).textStyle(.mono).foregroundStyle(theme.text3).lineLimit(1) }
            }
            HStack(spacing: 6) {
                Text(directory).font(.system(size: 10.5, design: .monospaced)).foregroundStyle(theme.text3)
                    .lineLimit(1).truncationMode(.head)
                if let scope { EvidencePill(scope.state, label: scope.label) }
            }
            .padding(.leading, 23)
        }
        .padding(.vertical, 6)
        .padding(.horizontal, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(selected ? theme.fill : flagged ? theme.danger.opacity(0.08) : Color.clear, in: shape)
        .overlay { if flagged { shape.strokeBorder(theme.dangerRing, lineWidth: 1) } }
        .contentShape(shape)
    }
}

// MARK: Approval surfaces (COMPONENTS.md, section 18)

/// The small icon tile of a request: the approval color while a decision is awaited,
/// neutral otherwise. Solid fills stay the same in both appearances.
struct RequestTile: View {
    @Environment(\.theme) private var theme
    let symbol: String
    var awaiting = false
    var size: CGFloat = 24

    static let approval = RGBA(0xB5701F)

    var body: some View {
        RoundedRectangle(cornerRadius: 6)
            .fill(awaiting ? Self.approval.color : theme.tileNeutral)
            .frame(width: size, height: size)
            .overlay(Image(systemName: symbol).font(.system(size: size / 2, weight: .medium)).foregroundStyle(theme.onAccent))
            .accessibilityHidden(true)
    }
}

/// One request of the Approval Inbox list: tile, the kind with an optional age, the object,
/// and a third line with an optional document tag. The row is a plain view; the screen
/// wraps it in a button.
struct RequestRow: View {
    @Environment(\.theme) private var theme
    let kind: String
    var age: String?
    let title: String
    /// The object is an identity and is drawn monospaced.
    var mono = false
    var detail: String?
    var status: DocumentStatus?
    let symbol: String
    var awaiting = false
    var selected = false

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 8)
        HStack(alignment: .top, spacing: 10) {
            RequestTile(symbol: symbol, awaiting: awaiting)
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(kind).font(.system(size: 11)).foregroundStyle(theme.text3).lineLimit(1)
                    Spacer(minLength: 0)
                    if let age { Text(age).font(.system(size: 10.5, design: .monospaced)).foregroundStyle(theme.text3) }
                }
                Text(title).font(.system(size: 12.5, weight: .semibold, design: mono ? .monospaced : .default))
                    .foregroundStyle(theme.text).lineLimit(1).truncationMode(.middle)
                HStack(spacing: 6) {
                    if let detail { Text(detail).textStyle(.meta).foregroundStyle(theme.text3).lineLimit(1) }
                    if let status { DocumentTag(status) }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.vertical, 9)
        .padding(.horizontal, 10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(selected ? theme.accent.opacity(0.16) : .clear, in: shape)
        .overlay(shape.strokeBorder(selected ? theme.info.opacity(0.4) : .clear, lineWidth: 1))
        .contentShape(shape)
    }
}

/// One fact of a request: a label 130 wide and a value that wraps, with a hairline below
/// unless it is the last line of its card.
struct FactLine<Content: View>: View {
    @Environment(\.theme) private var theme
    let label: String
    var last = false
    @ViewBuilder let content: () -> Content

    init(_ label: String, last: Bool = false, @ViewBuilder content: @escaping () -> Content) {
        self.label = label
        self.last = last
        self.content = content
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(label).font(.system(size: 12.5)).foregroundStyle(theme.text3).frame(width: 130, alignment: .leading)
            content().font(.system(size: 12.5)).foregroundStyle(theme.text)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.vertical, 11)
        .padding(.horizontal, 14)
        .overlay(alignment: .bottom) { if !last { Rectangle().fill(theme.hairline).frame(height: 1) } }
    }
}

extension FactLine where Content == Text {
    /// A plain sentence.
    init(_ label: String, _ value: String, last: Bool = false) {
        self.init(label, last: last) { Text(value) }
    }
}

/// A name and its trailing value, a line of a policy card; a hairline below unless last.
struct ValueLine: View {
    @Environment(\.theme) private var theme
    let name: String
    let value: String
    var last = false

    init(_ name: String, _ value: String, last: Bool = false) {
        self.name = name
        self.value = value
        self.last = last
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(name).textStyle(.body).foregroundStyle(theme.text).frame(maxWidth: .infinity, alignment: .leading)
            Text(value).textStyle(.body).foregroundStyle(theme.text2).multilineTextAlignment(.trailing)
                .fixedSize(horizontal: false, vertical: true)
                .layoutPriority(1)
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 14)
        .overlay(alignment: .bottom) { if !last { Rectangle().fill(theme.hairline).frame(height: 1) } }
    }
}

// MARK: Document surfaces (COMPONENTS.md, section 19)

/// One document of the Specifications list in two lines: the document with an optional
/// document tag, and a note. An earlier revision is indented under its document; a row
/// that cannot be opened is `quiet`. The row is a plain view; the screen wraps it in a button.
struct DocumentRow: View {
    @Environment(\.theme) private var theme
    let name: String
    /// The document is an identity and is drawn monospaced.
    var mono = false
    var detail: String?
    var status: DocumentStatus?
    var indented = false
    var quiet = false
    var selected = false

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 7)
        VStack(alignment: .leading, spacing: quiet ? 2 : 3) {
            HStack(spacing: 8) {
                Text(name).font(.system(size: quiet ? 12 : 12.5, weight: quiet ? .regular : .semibold, design: mono ? .monospaced : .default))
                    .foregroundStyle(quiet ? theme.text2 : theme.text).lineLimit(1).truncationMode(.middle)
                Spacer(minLength: 0)
                if let status { DocumentTag(status) }
            }
            if let detail { Text(detail).textStyle(.meta).foregroundStyle(theme.text3).lineLimit(1) }
        }
        .padding(.vertical, quiet ? 6 : 7)
        .padding(.leading, indented ? 24 : 10)
        .padding(.trailing, 10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(selected ? theme.accent.opacity(0.16) : .clear, in: shape)
        .overlay(shape.strokeBorder(selected ? theme.info.opacity(0.4) : .clear, lineWidth: 1))
        .contentShape(shape)
    }
}

/// One section of a document: a heading at 12 semibold `text3` over its content.
struct DocumentSection<Content: View>: View {
    @Environment(\.theme) private var theme
    let title: String
    @ViewBuilder let content: () -> Content

    init(_ title: String, @ViewBuilder content: @escaping () -> Content) {
        self.title = title
        self.content = content
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.system(size: 12, weight: .semibold)).foregroundStyle(theme.text3)
                .accessibilityAddTraits(.isHeader)
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// One line of a traceability card: an identifier, what it is, and an optional trailing
/// reference to what implements it; a hairline below unless last.
struct TraceLine: View {
    @Environment(\.theme) private var theme
    let id: String
    let text: String
    var target: String?
    var last = false

    init(_ id: String, _ text: String, target: String? = nil, last: Bool = false) {
        self.id = id
        self.text = text
        self.target = target
        self.last = last
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(id).font(.system(size: 11.5, design: .monospaced)).foregroundStyle(theme.info)
            Text(text).textStyle(.body).foregroundStyle(theme.text)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
            if let target { Text(target).textStyle(.body).foregroundStyle(theme.info) }
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 14)
        .overlay(alignment: .bottom) { if !last { Rectangle().fill(theme.hairline).frame(height: 1) } }
    }
}

// MARK: Audit surfaces (COMPONENTS.md, section 20)

/// The search box above a list: a magnifier and a one-line field on `surface` with a ring.
/// In the off-screen snapshot its text or prompt is drawn in place.
struct ListSearchField: View {
    @Environment(\.theme) private var theme
    @Environment(\.offscreen) private var offscreen
    let prompt: String
    @Binding var text: String

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 8)
        HStack(spacing: 8) {
            Image(systemName: "magnifyingglass").font(.system(size: 12, weight: .medium)).foregroundStyle(theme.text3)
                .accessibilityHidden(true)
            Group {
                if offscreen {
                    Text(text.isEmpty ? prompt : text).foregroundStyle(text.isEmpty ? theme.text3 : theme.text).lineLimit(1)
                        .frame(maxWidth: .infinity, alignment: .leading)
                } else {
                    TextField("", text: $text, prompt: Text(prompt).foregroundColor(theme.text3))
                        .textFieldStyle(.plain).foregroundStyle(theme.text)
                }
            }
            .font(.system(size: 13))
        }
        .padding(.horizontal, 10)
        .frame(height: 32)
        .background(theme.surface, in: shape)
        .overlay(shape.strokeBorder(theme.ringEmpty, lineWidth: 1))
        .accessibilityLabel(prompt)
    }
}

/// A filter that is on or off, as a capsule: on is `info` on `accent` at 16%, off is quiet.
/// The chip is a plain view; the screen wraps it in a button.
struct FilterChip: View {
    @Environment(\.theme) private var theme
    let title: String
    var count: Int?
    let isOn: Bool

    var body: some View {
        HStack(spacing: 5) {
            Text(title).font(.system(size: 12)).lineLimit(1)
            if let count { Text("\(count)").font(.system(size: 11, design: .monospaced)).opacity(0.75) }
        }
        .foregroundStyle(isOn ? theme.info : theme.text3)
        .padding(.horizontal, 10)
        .frame(height: 24)
        .background(isOn ? theme.accent.opacity(0.16) : theme.fill2, in: Capsule())
        .overlay(Capsule().strokeBorder(isOn ? .clear : theme.hairline, lineWidth: 1))
        .contentShape(Capsule())
        .accessibilityAddTraits(isOn ? .isSelected : [])
    }
}

/// The type of a record as a small tag: 11 semibold in the tone's color on a quiet fill.
struct KindTag: View {
    @Environment(\.theme) private var theme
    let text: String
    var tone = Tone.neutral

    init(_ text: String, tone: Tone = .neutral) {
        self.text = text
        self.tone = tone
    }

    var body: some View {
        Text(text).font(.system(size: 11, weight: .semibold)).foregroundStyle(theme.foreground(tone))
            .lineLimit(1).truncationMode(.middle)
            .padding(.horizontal, 6)
            .frame(height: 18)
            .background(theme.hover, in: RoundedRectangle(cornerRadius: 4))
    }
}

/// The columns of a record table. The event column takes the width that is left.
enum RecordColumns {
    static let type: CGFloat = 150
    static let subject: CGFloat = 92
    static let record: CGFloat = 64
    static let gap: CGFloat = 10
}

/// The column captions of a record table, with a hairline below.
struct RecordHeader: View {
    @Environment(\.theme) private var theme
    var type = "Type"
    var event = "Event"
    var subject = "Subject"
    var record = "Record"

    var body: some View {
        HStack(spacing: RecordColumns.gap) {
            Text(type).frame(width: RecordColumns.type, alignment: .leading)
            Text(event).frame(maxWidth: .infinity, alignment: .leading)
            Text(subject).frame(width: RecordColumns.subject, alignment: .leading)
            Text(record).frame(width: RecordColumns.record, alignment: .leading)
        }
        .font(.system(size: 11, weight: .semibold)).foregroundStyle(theme.text3).lineLimit(1)
        .padding(.vertical, 8)
        .padding(.horizontal, 14)
        .overlay(alignment: .bottom) { Rectangle().fill(theme.ringEmpty).frame(height: 1) }
    }
}

/// The caption of a group of records inside the table ("Today" on the canvas).
struct RecordGroup: View {
    @Environment(\.theme) private var theme
    let title: String

    init(_ title: String) {
        self.title = title
    }

    var body: some View {
        Text(title).font(.system(size: 11, weight: .semibold)).foregroundStyle(theme.text3)
            .padding(EdgeInsets(top: 10, leading: 14, bottom: 6, trailing: 14))
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityAddTraits(.isHeader)
    }
}

/// One record of the table in one line: its type tag, what happened, the subject and the
/// record's identifier; a hairline below unless last. The row is a plain view; the screen
/// wraps it in a button.
struct RecordRow: View {
    @Environment(\.theme) private var theme
    let type: String
    var tone = Tone.neutral
    let text: String
    /// The text is the record's raw payload and is drawn monospaced.
    var mono = false
    var subject: String?
    let record: String
    var selected = false
    var last = false

    var body: some View {
        HStack(spacing: RecordColumns.gap) {
            HStack(spacing: 0) { KindTag(type, tone: tone) }.frame(width: RecordColumns.type, alignment: .leading)
            Text(text).font(.system(size: mono ? 12 : 13, design: mono ? .monospaced : .default)).foregroundStyle(theme.text)
                .lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
            Text(subject ?? "—").font(.system(size: 11.5, design: .monospaced))
                .foregroundStyle(subject == nil ? theme.text4 : theme.text2)
                .lineLimit(1).truncationMode(.middle).frame(width: RecordColumns.subject, alignment: .leading)
            Text(record).font(.system(size: 11, design: .monospaced)).foregroundStyle(theme.text3)
                .lineLimit(1).frame(width: RecordColumns.record, alignment: .leading)
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 14)
        .background(selected ? theme.accent.opacity(0.16) : .clear)
        .overlay(alignment: .bottom) { if !last, !selected { Rectangle().fill(theme.hairline).frame(height: 1) } }
        .overlay { if selected { Rectangle().strokeBorder(theme.info.opacity(0.4), lineWidth: 1) } }
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

// MARK: Settings surfaces (COMPONENTS.md, section 21)

/// The tile of a settings section: a solid color that stays the same in both appearances,
/// with a white glyph. 20 × 20 in the section list, 28 × 28 in the title of the section.
struct SectionTile: View {
    @Environment(\.theme) private var theme
    let symbol: String
    let color: RGBA
    var size: CGFloat = 20

    var body: some View {
        RoundedRectangle(cornerRadius: 6)
            .fill(color.color)
            .frame(width: size, height: size)
            .overlay(Image(systemName: symbol).font(.system(size: size * 0.55, weight: .medium)).foregroundStyle(theme.onAccent))
            .accessibilityHidden(true)
    }
}

/// One item of a section list: tile and name. The row is a plain view; the screen wraps it
/// in a button.
struct SectionItem: View {
    @Environment(\.theme) private var theme
    let title: String
    let symbol: String
    let color: RGBA
    var selected = false

    var body: some View {
        HStack(spacing: 9) {
            SectionTile(symbol: symbol, color: color)
            Text(title).textStyle(.body).foregroundStyle(theme.text).lineLimit(1)
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 8)
        .frame(height: 30)
        .background(selected ? theme.accent.opacity(0.16) : .clear, in: RoundedRectangle(cornerRadius: 7))
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// How the value of a setting reads: a sentence, a path or identifier, or the statement
/// that there is no value ("Not reported").
enum SettingForm: Equatable, Sendable {
    case plain, identifier, missing
}

/// The value of a setting as read-only text in one of the three forms.
struct SettingText: View {
    @Environment(\.theme) private var theme
    let value: String
    var form = SettingForm.plain

    var body: some View {
        switch form {
        case .plain: Text(value).textStyle(.body).foregroundStyle(theme.text2)
        case .identifier: Text(value).font(.system(size: 11.5, design: .monospaced)).tracking(-0.115).foregroundStyle(theme.text)
        case .missing: Text(value).textStyle(.body).foregroundStyle(theme.text3)
        }
    }
}

/// One setting of a settings card: a label, its value and an optional trailing part (a pill
/// or a note); a hairline below unless last. Nothing in it can be changed.
struct SettingLine<Value: View, Trailing: View>: View {
    @Environment(\.theme) private var theme
    let label: String
    var last = false
    @ViewBuilder let value: () -> Value
    @ViewBuilder let trailing: () -> Trailing

    init(_ label: String, last: Bool = false, @ViewBuilder value: @escaping () -> Value, @ViewBuilder trailing: @escaping () -> Trailing) {
        self.label = label
        self.last = last
        self.value = value
        self.trailing = trailing
    }

    var body: some View {
        HStack(alignment: .center, spacing: 10) {
            Text(label).textStyle(.body).foregroundStyle(theme.text)
                .fixedSize(horizontal: false, vertical: true)
                .frame(width: 170, alignment: .leading)
            value()
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
            trailing()
        }
        .padding(.vertical, 10)
        .padding(.horizontal, 14)
        .overlay(alignment: .bottom) { if !last { Rectangle().fill(theme.hairline).frame(height: 1) } }
        .accessibilityElement(children: .combine)
    }
}

extension SettingLine where Value == SettingText {
    init(_ label: String, _ value: String, form: SettingForm = .plain, last: Bool = false, @ViewBuilder trailing: @escaping () -> Trailing) {
        self.init(label, last: last, value: { SettingText(value: value, form: form) }, trailing: trailing)
    }
}

extension SettingLine where Value == SettingText, Trailing == EmptyView {
    init(_ label: String, _ value: String, form: SettingForm = .plain, last: Bool = false) {
        self.init(label, value, form: form, last: last) { EmptyView() }
    }
}

// MARK: Plan surfaces (design wave 1, Execution Plan; COMPONENTS.md, section 22)

/// The task node of COMPONENTS.md, section 6, without a status: the core reports no state
/// of a step, so the node has no pill, no state outline and no "Eligible" tag. Three lines:
/// an identifier, a title, a detail. The chosen node has a 2 pt accent ring.
struct TaskNode: View {
    static let size = CGSize(width: 196, height: 84)

    @Environment(\.theme) private var theme
    let identifier: String
    let title: String
    let detail: String
    var selected = false

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(identifier).font(.system(size: 11.5, weight: .medium, design: .monospaced)).foregroundStyle(theme.text2).lineLimit(1)
            Text(title).font(.system(size: 12.5, weight: .semibold)).foregroundStyle(theme.textStrong).lineLimit(1)
            Text(detail).textStyle(.meta).foregroundStyle(theme.text3).lineLimit(1).truncationMode(.middle)
        }
        .padding(.vertical, 9)
        .padding(.horizontal, 11)
        .frame(width: Self.size.width, height: Self.size.height, alignment: .topLeading)
        .background(theme.surface, in: RoundedRectangle(cornerRadius: 10))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(selected ? theme.accent : theme.liftRing, lineWidth: selected ? 2 : 1))
        .contentShape(RoundedRectangle(cornerRadius: 10))
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
}

/// An edge of the plan graph from the trailing side of one node to the leading side of
/// another, with an arrowhead. Solid or dashed; one color, because whether a dependency is
/// met is not reported.
struct GraphEdge: View {
    @Environment(\.theme) private var theme
    let from: CGPoint
    let to: CGPoint
    var dashed = false

    var body: some View {
        let reach = max(24, abs(to.x - from.x) / 2)
        ZStack {
            Path { path in
                path.move(to: from)
                path.addCurve(to: CGPoint(x: to.x - 5, y: to.y), control1: CGPoint(x: from.x + reach, y: from.y),
                              control2: CGPoint(x: to.x - 5 - reach, y: to.y))
            }
            .stroke(theme.dashedOutline, style: StrokeStyle(lineWidth: 1.5, lineCap: .round, dash: dashed ? [4, 4] : []))
            Path { path in
                path.move(to: to)
                path.addLine(to: CGPoint(x: to.x - 6, y: to.y - 3.5))
                path.addLine(to: CGPoint(x: to.x - 6, y: to.y + 3.5))
                path.closeSubpath()
            }
            .fill(theme.text3)
        }
        .accessibilityHidden(true)
    }
}

/// A lane of the plan graph: a faint panel that carries a name beside a neutral square.
/// The board colors the square per repository; no color of a repository is reported.
struct LaneBand: View {
    static let header: CGFloat = 24

    @Environment(\.theme) private var theme
    let name: String
    var mono = false

    var body: some View {
        RoundedRectangle(cornerRadius: 8).fill(theme.fill2)
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(theme.hairline, lineWidth: 1))
            .overlay(alignment: .topLeading) {
                HStack(spacing: 6) {
                    RoundedRectangle(cornerRadius: 2).fill(theme.tileNeutral).frame(width: 8, height: 8)
                    Text(name).font(.system(size: 11.5, weight: .semibold, design: mono ? .monospaced : .default))
                        .foregroundStyle(theme.text2).lineLimit(1)
                }
                .frame(height: Self.header)
                .padding(.horizontal, 10)
            }
    }
}

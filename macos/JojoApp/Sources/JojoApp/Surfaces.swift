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

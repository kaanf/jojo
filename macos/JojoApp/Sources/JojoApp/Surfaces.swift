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

/// A drawn checkbox (COMPONENTS.md, section 5.6): 18 × 18, radius 5, a 1.5 `text3` ring;
/// on, `accent` with a white check.
struct CheckBox: View {
    @Environment(\.theme) private var theme
    let isOn: Bool

    var body: some View {
        RoundedRectangle(cornerRadius: 5)
            .fill(isOn ? theme.accent : .clear)
            .overlay { if !isOn { RoundedRectangle(cornerRadius: 5).strokeBorder(theme.text3, lineWidth: 1.5) } }
            .overlay(Image(systemName: "checkmark").font(.system(size: 11, weight: .bold))
                .foregroundStyle(theme.onAccent).opacity(isOn ? 1 : 0))
            .frame(width: 18, height: 18)
    }
}

// MARK: Attempt surfaces (COMPONENTS.md, section 15)

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

struct ConsoleLine: Identifiable, Equatable, Sendable {
    enum Kind: Equatable, Sendable {
        case plain, command, status(Tone)
    }

    let id: Int
    let text: String
    var kind = Kind.plain
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

/// A command was sent and the core has not confirmed it (STATES.md, "Requested").
struct RequestedNotice: View {
    @Environment(\.theme) private var theme
    var text = "Sent to core · waiting for confirmation"

    var body: some View {
        HStack(spacing: 8) {
            OperationalPill(.requested)
            Text(text).textStyle(.meta).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(theme.fill2, in: RoundedRectangle(cornerRadius: 8))
    }
}

// MARK: Source control surfaces (COMPONENTS.md, section 17)

// MARK: Approval surfaces (COMPONENTS.md, section 18)

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

// MARK: Document surfaces (COMPONENTS.md, section 19)

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
    /// Protocol 1.6: the time and actor columns, when the core reports them.
    static let time: CGFloat = 58
    static let actor: CGFloat = 76
}

/// The column captions of a record table, with a hairline below.
struct RecordHeader: View {
    @Environment(\.theme) private var theme
    var type = "Type"
    var event = "Event"
    var subject = "Subject"
    var record = "Record"
    /// Draw the time and actor columns.
    var attributed = false

    var body: some View {
        HStack(spacing: RecordColumns.gap) {
            if attributed {
                Text("Time").frame(width: RecordColumns.time, alignment: .leading)
                Text("Actor").frame(width: RecordColumns.actor, alignment: .leading)
            }
            Text(type).frame(width: RecordColumns.type, alignment: .leading)
            Text(event).frame(maxWidth: .infinity, alignment: .leading)
            Text(subject).frame(width: RecordColumns.subject, alignment: .leading)
            // With the time and actor columns the record identifier is in the record panel only.
            if !attributed { Text(record).frame(width: RecordColumns.record, alignment: .leading) }
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
    /// Protocol 1.6: when the record was made and who caused it; both columns are drawn
    /// when either is given.
    var time: String? = nil
    var actor: String? = nil

    var body: some View {
        HStack(spacing: RecordColumns.gap) {
            if time != nil || actor != nil {
                Text(time ?? "—").font(.system(size: 11.5, design: .monospaced)).foregroundStyle(theme.text2)
                    .lineLimit(1).frame(width: RecordColumns.time, alignment: .leading)
                Text(actor ?? "—").font(.system(size: 12)).foregroundStyle(theme.text2)
                    .lineLimit(1).frame(width: RecordColumns.actor, alignment: .leading)
            }
            HStack(spacing: 0) { KindTag(type, tone: tone) }.frame(width: RecordColumns.type, alignment: .leading)
            Text(text).font(.system(size: mono ? 12 : 13, design: mono ? .monospaced : .default)).foregroundStyle(theme.text)
                .lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
            Text(subject ?? "—").font(.system(size: 11.5, design: .monospaced))
                .foregroundStyle(subject == nil ? theme.text4 : theme.text2)
                .lineLimit(1).truncationMode(.middle).frame(width: RecordColumns.subject, alignment: .leading)
            if time == nil, actor == nil {
                Text(record).font(.system(size: 11, design: .monospaced)).foregroundStyle(theme.text3)
                    .lineLimit(1).frame(width: RecordColumns.record, alignment: .leading)
            }
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

// MARK: Chat surfaces (COMPONENTS.md, section 23)

/// A multi-line text field: what the human types into Feature Chat and New Feature. It grows
/// from `lines` up to eight lines. In the off-screen snapshot, where a system field does not
/// draw, its text or prompt is drawn in place.
struct TextArea: View {
    @Environment(\.theme) private var theme
    @Environment(\.offscreen) private var offscreen
    let prompt: String
    @Binding var text: String
    var lines = 3

    var body: some View {
        Group {
            if offscreen {
                Text(text.isEmpty ? prompt : text).foregroundStyle(text.isEmpty ? theme.text3 : theme.text)
                    .lineLimit(lines...8)
                    .frame(maxWidth: .infinity, minHeight: CGFloat(lines) * 17, alignment: .topLeading)
            } else {
                TextField("", text: $text, prompt: Text(prompt).foregroundColor(theme.text3), axis: .vertical)
                    .textFieldStyle(.plain).foregroundStyle(theme.text).lineLimit(lines...8)
            }
        }
        .font(.system(size: 13))
        .padding(.vertical, 8)
        .padding(.horizontal, 10)
        .background(theme.fill, in: RoundedRectangle(cornerRadius: 8))
        .accessibilityLabel(prompt)
    }
}

/// One message of a chat transcript: who speaks, a line of facts beside it, then the text.
/// The human's messages stand on `fill2`; the lead's on the card.
struct ChatMessage<Content: View>: View {
    @Environment(\.theme) private var theme
    let author: String
    var meta: String?
    var human = false
    @ViewBuilder let content: () -> Content

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Text(String(author.prefix(1))).font(.system(size: 11, weight: .bold))
                .foregroundStyle(human ? theme.text2 : theme.onAccent)
                .frame(width: 22, height: 22)
                .background(human ? theme.fill : theme.accent, in: Circle())
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 6) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(author).textStyle(.bodyStrong).foregroundStyle(theme.textStrong)
                    if let meta { Text(meta).textStyle(.meta).foregroundStyle(theme.text3).lineLimit(1) }
                }
                content()
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(.vertical, 10)
        .padding(.horizontal, 12)
        .background(human ? theme.fill2 : Color.clear, in: RoundedRectangle(cornerRadius: 10))
    }
}

/// How long ago a time the core reported was: "now", "12m", "3h", "2d".
func shortAge(_ date: Date?, now: Date) -> String? {
    guard let date else { return nil }
    let seconds = max(0, now.timeIntervalSince(date))
    switch seconds {
    case ..<60: return "now"
    case ..<3600: return "\(Int(seconds / 60))m"
    case ..<86400: return "\(Int(seconds / 3600))h"
    default: return "\(Int(seconds / 86400))d"
    }
}

/// A time the core reported, in the viewer's time zone: "14:05:09", with the date when it
/// is not today ("17 Nov 14:05").
func clockTime(_ date: Date?, now: Date, seconds: Bool = true) -> String? {
    guard let date else { return nil }
    let calendar = Calendar.current
    let format = calendar.isDate(date, inSameDayAs: now) ? (seconds ? "HH:mm:ss" : "HH:mm") : "d MMM HH:mm"
    let formatter = DateFormatter()
    formatter.locale = Locale(identifier: "en_GB")
    formatter.dateFormat = format
    return formatter.string(from: date)
}

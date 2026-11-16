import JojoCore
import SwiftUI

// Activity & Audit (docs/design/screens/ACTIVITY.md): the events the core committed, as the
// `Events` answer gives them. The workspace reads them when this screen asks. The helper
// reports no time, no actor and no hash for an event, so none is shown and no chain is verified.

/// What the screen shows: the events the workspace holds, and the search, the type filters
/// and the chosen event, which live in the screen.
struct ActivityState: Equatable {
    var connection: HelperConnection
    var admission: String
    /// The snapshot rows; an event whose subject is a listed document can be opened.
    var rows: [SnapshotRow] = []
    /// The event the snapshot is at: the end of the journal as far as the workspace knows.
    var head: Int64?
    /// Oldest first, as the workspace holds them.
    var events: [JournalEvent]
    var load: JournalLoad
    var query = ""
    /// Event types that are switched off.
    var hidden: Set<String> = []
    var selected: Int64?

    var link: RecoveryCenterState.Link {
        switch connection {
        case .ready: return .connected
        case .notStarted, .handshaking: return .starting
        case .incompatible: return .incompatible
        case .launchFailed, .channelLost, .ended: return .lost
        }
    }

    var admissionClosed: Bool { link == .connected && !admission.isEmpty && admission != "Open" }

    /// One type among the loaded events, with how many of them have it.
    struct TypeCount: Identifiable, Equatable {
        let type: String
        let count: Int
        var id: String { type }
    }

    /// The types of the loaded events, by name. The core reports no category above them.
    var types: [TypeCount] {
        Dictionary(grouping: events, by: \.type).map { TypeCount(type: $0.key, count: $0.value.count) }
            .sorted { $0.type < $1.type }
    }

    private var needle: String { query.trimmingCharacters(in: .whitespaces).lowercased() }

    var filtered: Bool { !needle.isEmpty || !hidden.isDisjoint(with: events.map(\.type)) }

    /// Search reads what the core reported about a loaded event, nothing else.
    static func matches(_ event: JournalEvent, _ needle: String) -> Bool {
        needle.isEmpty || [event.type, event.payload, event.aggregateKind ?? "", event.aggregateID ?? "", record(event)]
            .contains { $0.lowercased().contains(needle) }
    }

    /// The loaded events that pass the search and the type filters, newest first.
    var shown: [JournalEvent] {
        events.reversed().filter { !hidden.contains($0.type) && Self.matches($0, needle) }
    }

    /// The shown events of one engine epoch. The core reports no time to group by.
    struct Group: Identifiable, Equatable {
        let epoch: Int64
        var events: [JournalEvent]
        var id: Int64 { events.first?.seq ?? epoch }
        var title: String { "Epoch \(epoch)" }
    }

    var groups: [Group] {
        shown.reduce(into: []) { groups, event in
            if groups.last?.epoch == event.epoch {
                groups[groups.count - 1].events.append(event)
            } else {
                groups.append(Group(epoch: event.epoch, events: [event]))
            }
        }
    }

    /// The chosen event while it is shown, otherwise the newest shown one.
    var chosen: JournalEvent? {
        shown.first { $0.seq == selected } ?? shown.first
    }

    static func record(_ event: JournalEvent) -> String { "event \(event.seq)" }

    /// The subject of an event in the table: the first twelve characters of its aggregate.
    static func subject(_ event: JournalEvent) -> String? {
        event.aggregateID.map(ApprovalInboxState.short)
    }

    /// The loaded range and how much of it passes the filters.
    var range: String? {
        guard let first = events.first, let last = events.last else { return nil }
        let loaded = first.seq == last.seq ? "Event \(first.seq) is loaded" : "Events \(first.seq) to \(last.seq) are loaded, newest first"
        return filtered ? "\(loaded); \(shown.count) of \(events.count) shown." : "\(loaded)."
    }

    /// Events before the oldest loaded one exist and the core can be asked for them.
    var canLoadOlder: Bool {
        (link == .connected || link == .incompatible) && load != .loading && (events.first?.seq ?? 0) > 1
    }

    /// The subject is a document the snapshot lists, so Specifications can load it.
    func isDocument(_ event: JournalEvent) -> Bool {
        guard let subject = event.aggregateID else { return false }
        return ApprovalInboxState.documents(in: rows).contains { $0.id == subject }
    }
}

struct ActivityActions {
    var search: (String) -> Void = { _ in }
    var toggle: (String) -> Void = { _ in }
    var clearFilters: () -> Void = {}
    var select: (Int64) -> Void = { _ in }
    var reload: () -> Void = {}
    var loadOlder: () -> Void = {}
    /// Loads a document's review and opens Specifications.
    var openDocument: (String) -> Void = { _ in }
    var open: (Screen) -> Void = { _ in }
}

struct ActivityScreen: View {
    @ObservedObject var model: AppModel
    /// The journal is read only while this destination is the shown one.
    let visible: Bool
    let open: (Screen) -> Void
    @State private var query = ""
    @State private var hidden: Set<String> = []
    @State private var selected: Int64?

    var body: some View {
        let workspace = model.workspace
        ActivityView(
            state: ActivityState(
                connection: model.client.connection, admission: workspace.admission, rows: workspace.rows,
                head: workspace.snapshotSeq, events: workspace.events, load: workspace.eventsLoad,
                query: query, hidden: hidden, selected: selected),
            actions: ActivityActions(
                search: { query = $0 },
                toggle: { hidden.formSymmetricDifference([$0]) },
                clearFilters: { (query, hidden) = ("", []) },
                select: { selected = $0 },
                reload: { workspace.loadEvents() },
                loadOlder: { workspace.loadOlderEvents() },
                openDocument: { workspace.loadReview($0); open(.specifications) },
                open: open))
            .onChange(of: visible) { if visible { workspace.loadEvents() } }
            .onChange(of: workspace.snapshotSeq) { if visible { workspace.loadEvents() } }
    }
}

struct ActivityView: View {
    @Environment(\.theme) private var theme
    let state: ActivityState
    var actions = ActivityActions()

    var body: some View {
        GeometryReader { geometry in
            // The table keeps at least 460 beside the record panel.
            let wide = geometry.size.width >= 780
            HStack(spacing: 0) {
                Scrolling {
                    VStack(alignment: .leading, spacing: 12) {
                        banners
                        if state.events.isEmpty {
                            placeholder.frame(minHeight: 260)
                        } else {
                            ListSearchField(prompt: "Search the loaded events: type, payload, subject, event number",
                                        text: Binding(get: { state.query }, set: { actions.search($0) }))
                            chips
                            if state.shown.isEmpty { unmatched.frame(minHeight: 200) } else { table }
                            footer
                            if !wide { Card(.panelList) { record } }
                        }
                    }
                    .padding(EdgeInsets(top: 14, leading: 18, bottom: 20, trailing: 18))
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                if wide {
                    Scrolling { record }
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                        .background(theme.sidebar)
                        .overlay(alignment: .leading) { Rectangle().fill(theme.sidebarEdge).frame(width: 1) }
                        .frame(width: 300)
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    @ViewBuilder
    private var banners: some View {
        switch state.link {
        case .lost:
            Banner(.danger, title: "Jojo core isn't responding",
                   message: "Showing the events read before, read-only. No more can be read.")
        case .incompatible:
            Banner(.danger, title: "Jojo core speaks another protocol version",
                   message: "Read-only. Events can still be read.")
        case .connected, .starting:
            EmptyView()
        }
        if state.admissionClosed {
            Banner(.danger, title: "Execution admission is closed",
                   message: "No new attempt starts until you review the inspection. Events can still be read.") {
                Button("Open Recovery Center") { actions.open(.recoveryCenter) }.buttonStyle(.jojo(.secondary, compact: true))
            }
        }
        if case .failed(let reason) = state.load, state.link != .lost {
            Banner(.warning, title: "The event journal could not be read",
                   message: state.events.isEmpty ? "The core answered: \(reason)." : "The core answered: \(reason). The events read before stay.") {
                Button("Try Again", action: actions.reload).buttonStyle(.jojo(.secondary, compact: true))
            }
        }
    }

    /// No event is held: say why.
    @ViewBuilder
    private var placeholder: some View {
        switch state.load {
        case .loading:
            LoadingBlock("Reading the event journal…")
        case .loaded:
            StateBlock(symbol: "clock.arrow.circlepath", title: "No events yet",
                       message: "The core has committed no event after the one this read started from.")
        case .failed:
            StateBlock(symbol: "clock.arrow.circlepath", title: "No events loaded",
                       message: "The journal was not read. Nothing is shown in its place.")
        case .idle:
            if state.link == .lost || state.link == .incompatible {
                StateBlock(symbol: "clock.arrow.circlepath", title: "No events loaded",
                           message: "The journal was not read before the core stopped answering as expected.")
            } else {
                LoadingBlock("Waiting for the core's snapshot…")
            }
        }
    }

    private var unmatched: some View {
        StateBlock(symbol: "magnifyingglass", title: "No loaded event matches",
                   message: "Search and the type filters cover the \(state.events.count) loaded events only, not the whole journal.") {
            Button("Clear Filters", action: actions.clearFilters).buttonStyle(.jojo(.secondary, compact: true))
        }
    }

    private var chips: some View {
        WrappingRow(spacing: 6, lineSpacing: 6) {
            ForEach(state.types) { item in
                let isOn = !state.hidden.contains(item.type)
                Button { actions.toggle(item.type) } label: {
                    FilterChip(title: item.type, count: item.count, isOn: isOn)
                }
                .buttonStyle(.plain)
                .help(isOn ? "Hide \(item.type) events" : "Show \(item.type) events")
            }
        }
    }

    private var table: some View {
        Card(.panelList) {
            VStack(spacing: 0) {
                RecordHeader()
                let last = state.shown.last?.seq
                ForEach(state.groups) { group in
                    RecordGroup(group.title)
                    ForEach(group.events) { event in
                        Button { actions.select(event.seq) } label: {
                            RecordRow(type: event.type, text: event.payload.isEmpty ? "(empty payload)" : event.payload, mono: true,
                                      subject: ActivityState.subject(event), record: ActivityState.record(event),
                                      selected: event.seq == state.chosen?.seq, last: event.seq == last)
                        }
                        .buttonStyle(.plain)
                    }
                }
            }
        }
    }

    private var footer: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                if let range = state.range { Text(range).textStyle(.meta).foregroundStyle(theme.text3) }
                Spacer(minLength: 0)
                if state.load == .loading, !state.events.isEmpty {
                    Text("Reading…").textStyle(.meta).foregroundStyle(theme.text3)
                }
                if (state.events.first?.seq ?? 0) > 1 {
                    Button("Load Older", action: actions.loadOlder).buttonStyle(.jojo(.secondary, compact: true))
                        .disabled(!state.canLoadOlder)
                } else {
                    Text("The journal starts here.").textStyle(.meta).foregroundStyle(theme.text3)
                }
            }
            Footnote("Events are append-only in the core. It reports no hash for them, so the app verifies no chain. Not reported: the time and the actor of an event; events are grouped by the engine epoch that committed them. There is no export and no filter by project or date.")
        }
    }

    // MARK: Record panel

    @ViewBuilder
    private var record: some View {
        if let event = state.chosen {
            VStack(alignment: .leading, spacing: 0) {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Audit record · \(ActivityState.record(event))").font(.system(size: 11, weight: .medium))
                        .foregroundStyle(theme.text3)
                    Text(event.type).textStyle(.inspectorTitle).foregroundStyle(theme.textStrong)
                        .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
                    HStack(spacing: 6) {
                        KindTag("Epoch \(event.epoch)")
                        if let kind = event.aggregateKind { KindTag(kind) }
                    }
                }
                .padding(EdgeInsets(top: 14, leading: 16, bottom: 12, trailing: 16))
                .frame(maxWidth: .infinity, alignment: .leading)
                .overlay(alignment: .bottom) { Rectangle().fill(theme.hairline).frame(height: 1) }
                InspectorSection("Record") {
                    FactGrid {
                        Fact("Record ID") { identifier(ActivityState.record(event)) }
                        Fact("Time") { missing("Not reported") }
                        Fact("Actor") { missing("Not reported") }
                        Fact("Epoch", "\(event.epoch)")
                        Fact("Subject") {
                            if let id = event.aggregateID {
                                identifier("\(event.aggregateKind ?? "aggregate") \(id)")
                            } else {
                                missing("This event names none")
                            }
                        }
                        Fact("Revision") {
                            if let revision = event.revision { Text("\(revision)").font(.system(size: 12.5)) } else { missing("None") }
                        }
                    }
                }
                InspectorSection("Payload") {
                    Text(event.payload.isEmpty ? "(empty)" : event.payload).font(.system(size: 11.5, design: .monospaced))
                        .foregroundStyle(theme.text).textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.vertical, 8)
                        .padding(.horizontal, 10)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(theme.fill2, in: RoundedRectangle(cornerRadius: 8))
                    if !event.detailComplete {
                        Footnote("The first 512 characters. The app has no request for the rest of a payload yet.")
                    }
                }
                InspectorSection("Integrity") {
                    FactGrid {
                        Fact("Record hash") { missing("Not reported") }
                        Fact("Previous") { missing("Not reported") }
                        Fact("Chain") { EvidencePill(.notRun, label: "Not verified") }
                    }
                }
                InspectorSection("Go to", last: true) {
                    if let id = event.aggregateID, state.isDocument(event) {
                        Button("Open in Specifications") { actions.openDocument(id) }.buttonStyle(.jojo(.secondary, compact: true))
                            .help("Loads this document's review and opens Specifications")
                    } else {
                        missing(event.aggregateID == nil ? "This event names no subject." : "No screen opens this subject yet.")
                    }
                }
            }
        } else {
            VStack(alignment: .leading, spacing: 0) {
                InspectorSection("Record", last: true) { missing("No event is shown.") }
            }
        }
    }

    private func identifier(_ value: String) -> some View {
        Text(value).font(.system(size: 11.5, design: .monospaced)).textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
    }

    private func missing(_ text: String) -> some View {
        Text(text).font(.system(size: 12.5)).foregroundStyle(theme.text3)
    }
}

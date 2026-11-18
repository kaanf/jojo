import JojoCore
import SwiftUI

// Activity & Audit: docs/design/screens/ACTIVITY.md, rebuilt from `ActivityMac.dc.html`
// (step 6). The journal the core reports, newest first: time, actor, category, the core's
// sentence, subject and record; a project, a category and a range filter the loaded
// events. The core stores no hash, so no chain is verified and the screen says so.

/// The board's range control: the last day, the last week, everything loaded.
enum ActivityRange: String, CaseIterable, Identifiable {
    case day = "24h", week = "7d", all = "All"
    var id: String { rawValue }

    var seconds: TimeInterval? {
        switch self {
        case .day: return 86_400
        case .week: return 7 * 86_400
        case .all: return nil
        }
    }
}

/// Everything the screen reads, as plain values.
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
    /// The category shown (nil: every category).
    var category: String?
    /// The project shown, by identity (nil: every project).
    var project: String?
    var range = ActivityRange.all
    var selected: Int64?
    /// The day the groups are named from ("Today") and the range is measured from.
    var now = Date()
    /// The whole payload of the chosen event, when it was read (`EventDetail`).
    var payload: EventPayload?
    var payloadLoad = PlanLoad.idle

    /// The core reported times and actors (protocol 1.6).
    var attributed: Bool { events.contains { $0.at != nil } }
    /// The core reported categories and projects (protocol 1.11).
    var categorized: Bool { events.contains { $0.category != nil } }

    /// Who caused an event, as shown.
    static func actor(_ event: JournalEvent) -> String? {
        switch event.actor {
        case nil: return nil
        case "Core": return "Jojo core"
        case "Human": return "You"
        case let actor?: return providerName(actor)
        }
    }

    var link: RecoveryCenterState.Link { coreLink(connection) }

    var admissionClosed: Bool { link == .connected && !admission.isEmpty && admission != "Open" }

    /// The categories in the board's order, then the rest the core reports.
    static let categoryOrder = ["Decision", "Approval", "Attempt", "Recovery", "Git", "Validation", "Error", "Evidence",
                                "Chat", "Document", "Plan", "Setup", "Session", "Core"]

    /// The categories among the loaded events, in the board's order.
    var categories: [String] {
        let present = Set(events.compactMap(\.category))
        return Self.categoryOrder.filter(present.contains) + present.subtracting(Self.categoryOrder).sorted()
    }

    /// The projects the loaded events concern, by name.
    var projects: [(id: String, name: String)] {
        var seen: [String: String] = [:]
        for e in events { if let id = e.project { seen[id] = e.projectName ?? String(id.prefix(8)) } }
        return seen.map { (id: $0.key, name: $0.value) }.sorted { $0.name < $1.name }
    }

    var projectName: String? { project.flatMap { p in projects.first { $0.id == p }?.name } }

    private var needle: String { query.trimmingCharacters(in: .whitespaces).lowercased() }

    var filtered: Bool { !needle.isEmpty || category != nil || project != nil || range != .all }

    /// Search reads what the core reported about a loaded event, nothing else.
    static func matches(_ event: JournalEvent, _ needle: String) -> Bool {
        needle.isEmpty || [event.type, event.payload, event.summary ?? "", event.category ?? "", event.projectName ?? "",
                           event.aggregateKind ?? "", event.aggregateID ?? "", record(event), actor(event) ?? ""]
            .contains { $0.lowercased().contains(needle) }
    }

    /// The loaded events that pass the search and the filters, newest first. An event
    /// without a time is outside every range but "All".
    var shown: [JournalEvent] {
        events.reversed().filter { e in
            (category == nil || e.category == category) && (project == nil || e.project == project)
                && (range.seconds.map { span in e.at.map { now.timeIntervalSince($0) <= span } ?? false } ?? true)
                && Self.matches(e, needle)
        }
    }

    /// The shown events of one day, or of one engine epoch from a core that reports no time.
    struct Group: Identifiable, Equatable {
        let title: String
        var events: [JournalEvent]
        var id: Int64 { events.first?.seq ?? 0 }
    }

    /// "Today", "Yesterday" or the date of an event.
    func day(_ event: JournalEvent) -> String {
        guard let at = event.at else { return "Epoch \(event.epoch)" }
        let calendar = Calendar.current
        if calendar.isDate(at, inSameDayAs: now) { return "Today" }
        if let yesterday = calendar.date(byAdding: .day, value: -1, to: now), calendar.isDate(at, inSameDayAs: yesterday) {
            return "Yesterday"
        }
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_GB")
        formatter.dateFormat = "EEEE d MMMM"
        return formatter.string(from: at)
    }

    var groups: [Group] {
        shown.reduce(into: []) { groups, event in
            let title = day(event)
            if groups.last?.title == title {
                groups[groups.count - 1].events.append(event)
            } else {
                groups.append(Group(title: title, events: [event]))
            }
        }
    }

    /// The chosen event while it is shown, otherwise the newest shown one.
    var chosen: JournalEvent? {
        shown.first { $0.seq == selected } ?? shown.first
    }

    static func record(_ event: JournalEvent) -> String { "event \(event.seq)" }

    /// "17 Nov 09:24:11": the time of a record to the second.
    static func recordTime(_ date: Date) -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_GB")
        formatter.dateFormat = "d MMM HH:mm:ss"
        return formatter.string(from: date)
    }

    /// The subject of an event in the table: the first twelve characters of its aggregate.
    static func subject(_ event: JournalEvent) -> String? {
        event.aggregateID.map(ApprovalInboxState.short)
    }

    /// The sentence of an event, or its type and payload from a core older than 1.7.
    static func sentence(_ event: JournalEvent) -> String {
        event.summary ?? (event.payload.isEmpty ? event.type : "\(event.type): \(event.payload)")
    }

    /// The color of a category tag.
    static func tone(_ category: String?) -> Tone {
        switch category {
        case "Approval", "Decision": return .review
        case "Error": return .danger
        case "Recovery": return .warn
        case "Validation", "Evidence": return .ok
        case "Git", "Attempt", "Plan", "Chat", "Document": return .info
        default: return .neutral
        }
    }

    /// The loaded range and how much of it passes the filters.
    var rangeLine: String? {
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

    /// The whole payload of the chosen event when it was read, else the excerpt.
    func payloadText(_ event: JournalEvent) -> String {
        if let payload, payload.seq == event.seq { return payload.text }
        return event.payload
    }

    /// The shown events as Markdown, newest first: what Export writes. The record says
    /// that no hash chain was verified.
    var markdown: String {
        var lines = ["# Jojo activity and audit", ""]
        var filters = [rangeLine ?? "No events are loaded."]
        if let category { filters.append("Category: \(category).") }
        if let projectName { filters.append("Project: \(projectName).") }
        if !needle.isEmpty { filters.append("Search: \"\(query)\".") }
        lines += [filters.joined(separator: " "), "",
                  "Records are append-only. The core stores no record hash, so no hash chain was verified.", "",
                  "| Time | Actor | Category | Event | Subject | Record |", "|---|---|---|---|---|---|"]
        let cell = { (s: String) in s.replacingOccurrences(of: "|", with: "\\|").replacingOccurrences(of: "\n", with: " ") }
        for e in shown {
            let time = e.at.map { $0.formatted(.iso8601) } ?? "Epoch \(e.epoch)"
            lines.append("| \(time) | \(cell(Self.actor(e) ?? "Not reported")) | \(e.category ?? "Not reported") | "
                         + "\(cell(Self.sentence(e))) | \(cell(Self.subject(e) ?? "")) | \(Self.record(e)) |")
        }
        return lines.joined(separator: "\n") + "\n"
    }
}

struct ActivityActions {
    var search: (String) -> Void = { _ in }
    var category: (String?) -> Void = { _ in }
    var project: (String?) -> Void = { _ in }
    var range: (ActivityRange) -> Void = { _ in }
    var clearFilters: () -> Void = {}
    var select: (Int64) -> Void = { _ in }
    var reload: () -> Void = {}
    var loadOlder: () -> Void = {}
    /// Reads the rest of a long payload (`EventDetail`).
    var loadPayload: (Int64) -> Void = { _ in }
    /// Loads a document's review and opens Specifications.
    var openDocument: (String) -> Void = { _ in }
    var open: (Screen) -> Void = { _ in }
}

/// The filters of Activity & Audit, kept by the shell so that Export writes what is shown.
struct ActivityFilters: Equatable {
    var query = ""
    var category: String?
    var project: String?
    var range = ActivityRange.all
    var selected: Int64?
}

struct ActivityScreen: View {
    @ObservedObject var model: AppModel
    /// The journal is read only while this destination is the shown one.
    let visible: Bool
    @Binding var filters: ActivityFilters
    /// Export writes the shown events as Markdown through the save panel.
    @Binding var exporting: Bool
    let open: (Screen) -> Void

    var state: ActivityState {
        let workspace = model.workspace
        return ActivityState(
            connection: model.client.connection, admission: workspace.admission, rows: workspace.rows,
            head: workspace.snapshotSeq, events: workspace.events, load: workspace.eventsLoad,
            query: filters.query, category: filters.category, project: filters.project, range: filters.range,
            selected: filters.selected, payload: workspace.eventPayload, payloadLoad: workspace.eventPayloadLoad)
    }

    var body: some View {
        let workspace = model.workspace
        ActivityView(
            state: state,
            actions: ActivityActions(
                search: { filters.query = $0 },
                category: { filters.category = $0 },
                project: { filters.project = $0 },
                range: { filters.range = $0 },
                clearFilters: { filters = ActivityFilters(selected: filters.selected) },
                select: { filters.selected = $0 },
                reload: { workspace.loadEvents() },
                loadOlder: { workspace.loadOlderEvents() },
                loadPayload: { workspace.loadEventPayload($0) },
                openDocument: { workspace.loadReview($0); open(.specifications) },
                open: open))
            .onChange(of: visible) { if visible { workspace.loadEvents() } }
            .onChange(of: workspace.snapshotSeq) { if visible { workspace.loadEvents() } }
            .fileExporter(isPresented: $exporting, document: TranscriptDocument(text: state.markdown),
                          contentType: TranscriptDocument.readableContentTypes[0],
                          defaultFilename: "Jojo activity") { _ in }
    }
}

/// A capsule that opens a menu of choices; off screen, where a menu does not draw, its
/// title stands in for it.
struct MenuCapsule: View {
    @Environment(\.theme) private var theme
    @Environment(\.offscreen) private var offscreen
    let title: String
    let choices: [(id: String?, title: String)]
    let choose: (String?) -> Void
    var label = ""

    var body: some View {
        Group {
            if offscreen {
                HStack(spacing: 4) {
                    Text(title).font(.system(size: 12, weight: .medium))
                    Image(systemName: "chevron.down").font(.system(size: 9, weight: .semibold)).foregroundStyle(theme.text2)
                }
            } else {
                Menu {
                    ForEach(Array(choices.enumerated()), id: \.offset) { _, c in Button(c.title) { choose(c.id) } }
                } label: {
                    Text(title).font(.system(size: 12, weight: .medium))
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
            }
        }
        .padding(.horizontal, 12)
        .frame(height: 30)
        .background(theme.glassStrong, in: Capsule())
        .accessibilityLabel(label.isEmpty ? title : label)
    }
}

struct ActivityView: View {
    @Environment(\.theme) private var theme
    let state: ActivityState
    var actions = ActivityActions()

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            VStack(alignment: .leading, spacing: 10) { banners }.padding(.horizontal, 10)
            if state.events.isEmpty {
                placeholder.frame(maxWidth: .infinity, minHeight: 260).padding(.horizontal, 10)
                Spacer(minLength: 0)
            } else {
                InspectorColumns(trailingWidth: 320, wide: 900) {
                    VStack(alignment: .leading, spacing: 12) {
                        filterBar
                        if state.shown.isEmpty { unmatched.frame(minHeight: 200) } else { table }
                        footer
                    }
                } trailing: {
                    GlassInspector { record }
                }
            }
        }
        .padding(.top, 6)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    @ViewBuilder
    private var banners: some View {
        CoreBanners(connection: state.connection)
        if state.admissionClosed {
            Banner(.warning, title: "Execution admission is closed",
                   message: "The journal is still readable. Review recovery to start a new session.") {
                Button("Open Recovery Center") { actions.open(.recoveryCenter) }.buttonStyle(.jojo(.secondary, compact: true))
            }
        }
        if case .failed(let reason) = state.load {
            Banner(.warning, title: "The events could not be read", message: reason) {
                Button("Try Again") { actions.reload() }.buttonStyle(.jojo(.secondary, compact: true))
                    .disabled(state.link != .connected)
            }
        }
        if !state.events.isEmpty, !state.categorized {
            Banner(.warning, title: "This core reports no category or project",
                   message: "It answers events below protocol 1.11. The type stands in for the category; the project filter is empty.")
        }
    }

    @ViewBuilder
    private var placeholder: some View {
        if state.load == .loading || (state.load == .idle && state.link == .connected) {
            LoadingBlock("Reading the event journal…")
        } else if state.link != .connected {
            StateBlock(symbol: "clock.arrow.circlepath", title: "No events are loaded",
                       message: "The journal is read from the core while it is connected.")
        } else {
            StateBlock(symbol: "clock.arrow.circlepath", title: "No events yet",
                       message: "The core has recorded nothing yet.")
        }
    }

    private var unmatched: some View {
        StateBlock(symbol: "line.3.horizontal.decrease.circle", title: "No loaded event matches",
                   message: "The search and filters cover the loaded events only.") {
            Button("Clear Filters") { actions.clearFilters() }.buttonStyle(.jojo(.secondary, compact: true))
        }
    }

    /// The board's filter row: search, type, project and range.
    private var filterBar: some View {
        FlowRow(spacing: 8) {
            ListSearchField(prompt: "Search the loaded events",
                            text: Binding(get: { state.query }, set: { actions.search($0) }))
                .frame(width: 240)
            MenuCapsule(title: state.category ?? "All types",
                        choices: [(nil, "All types")] + state.categories.map { (Optional($0), $0) },
                        choose: actions.category, label: "Filter by category")
            MenuCapsule(title: state.projectName ?? "All projects",
                        choices: [(nil, "All projects")] + state.projects.map { (Optional($0.id), $0.name) },
                        choose: actions.project, label: "Filter by project")
            SegmentedCapsule(selection: Binding(get: { state.range }, set: { actions.range($0) }), label: "Range")
            if state.filtered {
                Button("Clear") { actions.clearFilters() }.buttonStyle(.jojo(.ghost, compact: true))
            }
        }
    }

    private static let columns: [(String, CGFloat?)] = [("Time", 46), ("Actor", 84), ("Type", 86), ("Event", nil),
                                                        ("Subject", 96), ("Record", 74)]

    private var table: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 10) {
                ForEach(Self.columns, id: \.0) { name, width in
                    Text(name).textStyle(.label).foregroundStyle(theme.text3)
                        .frame(width: width, alignment: .leading)
                        .frame(maxWidth: width == nil ? .infinity : nil, alignment: .leading)
                }
            }
            .padding(EdgeInsets(top: 8, leading: 12, bottom: 8, trailing: 12))
            ForEach(state.groups) { group in
                Text(group.title).textStyle(.label).foregroundStyle(theme.text2)
                    .padding(EdgeInsets(top: 10, leading: 12, bottom: 4, trailing: 12))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .overlay(alignment: .top) { Rectangle().fill(theme.sep).frame(height: 0.5) }
                ForEach(group.events) { event in row(event) }
            }
        }
        .cardElevation(radius: 14)
    }

    private func row(_ event: JournalEvent) -> some View {
        let chosen = event.seq == state.chosen?.seq
        return Button { actions.select(event.seq) } label: {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text(boardTime(event.at, now: state.now).map { String($0.suffix(5)) } ?? "–")
                    .textStyle(.mono).foregroundStyle(theme.text2).frame(width: 46, alignment: .leading)
                Text(ActivityState.actor(event) ?? "–").textStyle(.secondary).foregroundStyle(theme.text)
                    .lineLimit(1).frame(width: 84, alignment: .leading)
                Group {
                    if let category = event.category {
                        DocumentTag(category.uppercased(), tone: ActivityState.tone(category))
                    } else {
                        Text(event.type).textStyle(.secondary).foregroundStyle(theme.text3).lineLimit(1)
                    }
                }
                .frame(width: 86, alignment: .leading)
                Text(ActivityState.sentence(event)).textStyle(.secondary).foregroundStyle(theme.text)
                    .lineLimit(2).frame(maxWidth: .infinity, alignment: .leading)
                Text(ActivityState.subject(event) ?? "–").textStyle(.mono).foregroundStyle(theme.text2)
                    .lineLimit(1).truncationMode(.middle).frame(width: 96, alignment: .leading)
                Text(ActivityState.record(event)).textStyle(.mono).foregroundStyle(theme.text3)
                    .lineLimit(1).frame(width: 74, alignment: .leading)
            }
            .padding(EdgeInsets(top: 7, leading: 12, bottom: 7, trailing: 12))
            .background(chosen ? theme.accentTint : .clear)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(chosen ? .isSelected : [])
    }

    private var footer: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text("Records are append-only. The core stores no record hash, so no chain is verified. "
                 + (state.rangeLine ?? ""))
                .textStyle(.secondary).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
            Button(state.load == .loading ? "Loading…" : "Load Older") { actions.loadOlder() }
                .buttonStyle(.jojo(.secondary, compact: true))
                .disabled(!state.canLoadOlder)
                .help(state.canLoadOlder ? "Reads the 64 events before the oldest loaded one." : "No older events to read.")
        }
        .padding(.horizontal, 4)
    }

    // MARK: Record

    @ViewBuilder
    private var record: some View {
        if let event = state.chosen {
            VStack(alignment: .leading, spacing: 6) {
                Text("Audit record · \(event.category ?? event.type)").textStyle(.label).foregroundStyle(theme.text3)
                Text(ActivityState.sentence(event)).font(.system(size: 17, weight: .bold))
                    .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
                HStack(spacing: 6) {
                    if let category = event.category { DocumentTag(category.uppercased(), tone: ActivityState.tone(category)) }
                    Text(event.type).textStyle(.mono).foregroundStyle(theme.text2)
                }
            }
            LabeledSection(title: "Record") {
                DefinitionCard(rows: [
                    Definition(key: "Record ID", value: ActivityState.record(event), mono: true),
                    Definition(key: "Time", value: event.at.map(ActivityState.recordTime) ?? "Not reported"),
                    Definition(key: "Actor", value: ActivityState.actor(event) ?? "Not reported"),
                    Definition(key: "Project", value: event.projectName ?? (state.categorized ? "None" : "Not reported")),
                    Definition(key: "Subject", value: [event.aggregateKind, event.aggregateID].compactMap { $0 }.joined(separator: " ")
                               .nonEmpty ?? "None", mono: true),
                    Definition(key: "Session", value: "Epoch \(event.epoch)"),
                    Definition(key: "Cause", value: "Not reported"),
                ], keyWidth: 84)
            }
            LabeledSection(title: "Integrity") {
                DefinitionCard(rows: [
                    Definition(key: "Record hash", value: "Not reported"),
                    Definition(key: "Previous", value: "Not reported"),
                    Definition(key: "Chain", value: "", pill: ("Not verified", .warn)),
                ], keyWidth: 84)
                NotReported("The core stores no record hash, and the app verifies no chain.")
            }
            payloadSection(event)
            LabeledSection(title: "Go to") {
                FlowRow(spacing: 8) {
                    if let subject = event.aggregateID, state.isDocument(event) {
                        Button("Open Document") { actions.openDocument(subject) }.buttonStyle(.jojo(.secondary, compact: true))
                    }
                    if event.category == "Approval" || event.category == "Decision" {
                        Button("Approval Inbox") { actions.open(.approvalInbox) }.buttonStyle(.jojo(.secondary, compact: true))
                    }
                    if event.category == "Recovery" {
                        Button("Recovery Center") { actions.open(.recoveryCenter) }.buttonStyle(.jojo(.secondary, compact: true))
                    }
                    Button("Affected Tasks") {}.buttonStyle(.jojo(.secondary, compact: true)).disabled(true)
                        .help("Not reported: the core does not say which tasks an event affects.")
                }
            }
        } else {
            NotReported("Choose an event to see its record.")
        }
    }

    private func payloadSection(_ event: JournalEvent) -> some View {
        let full = state.payload?.seq == event.seq
        return LabeledSection(title: "Payload") {
            Text(state.payloadText(event).isEmpty ? "(empty payload)" : state.payloadText(event))
                .textStyle(.mono).foregroundStyle(theme.text).textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
                .lineLimit(full ? nil : 12)
                .padding(EdgeInsets(top: 9, leading: 12, bottom: 9, trailing: 12))
                .frame(maxWidth: .infinity, alignment: .leading)
                .cardElevation(radius: 12)
            if !event.detailComplete {
                if full, let payload = state.payload {
                    NotReported(payload.complete ? "The whole payload, \(payload.total) characters."
                                                 : "The first \(payload.text.count) of \(payload.total) characters.")
                } else {
                    HStack(spacing: 8) {
                        Button(state.payloadLoad == .loading ? "Reading…" : "Load Full Payload") { actions.loadPayload(event.seq) }
                            .buttonStyle(.jojo(.secondary, compact: true))
                            .disabled(state.link != .connected || state.payloadLoad == .loading)
                        NotReported("The first 512 characters are shown.")
                    }
                    if case .failed(let reason) = state.payloadLoad { NotReported(reason) }
                }
            }
        }
    }
}

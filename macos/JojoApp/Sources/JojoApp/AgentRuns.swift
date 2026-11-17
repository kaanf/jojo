import JojoCore
import SwiftUI

// Agent Runs (docs/design/screens/AGENT_RUNS.md), from `AgentRunsMac.dc.html`: the held
// feature's attempts grouped by task, one attempt's facts, timeline, terminal output,
// evidence and claims, and in the inspector its session, supervision, workspace,
// continuation, repair and review, claims and actions. Protocol 1.6 and 1.7 (`Attempts`),
// 1.5 (`Repairs`) and 1.9 (`AttemptDetail`); the log is `Log` through `loadCandidate`.

/// Timeline, Logs, Evidence or Claims: the detail's segmented control.
enum AttemptTab: String, CaseIterable, Identifiable, Sendable {
    case timeline = "Timeline", logs = "Logs", evidence = "Evidence", claims = "Claims"
    var id: String { rawValue }
}

/// What the screen shows. Every value is the helper's answer.
struct AgentRunsState: Equatable {
    var connection: HelperConnection
    var admission: String
    var scope = FeatureScope()
    /// Protocol 1.6 (1.7): the newest attempts; nil from an older helper.
    var summaries: AttemptList?
    /// The snapshot's attempt identities, for an older helper.
    var snapshot: [String] = []
    var recovery: [RecoveryRow] = []
    /// M4: repairs, holds and reviews of plan steps.
    var repairs: RepairList?
    /// The chosen attempt.
    var selected: String?
    /// Protocol 1.9: the chosen attempt beyond `Attempts`.
    var detail: AttemptDetail?
    var detailLoad = PlanLoad.idle
    /// The loaded result and its log, when they are the chosen attempt's.
    var result: CandidateReview?
    var log: [String] = []
    var commandPending = false
    var tab = AttemptTab.timeline
    var search = ""
    var reportsDetail = true
    var now = Date()

    static let logLimit: Int64 = 64
    static let attemptKinds: Set<String> = ["scheduler_attempt", "attempt"]

    /// The snapshot's attempt identities, in its order.
    static func attempts(in rows: [SnapshotRow]) -> [String] {
        var seen = Set<String>()
        return rows.filter { attemptKinds.contains($0.kind) && seen.insert($0.subject).inserted }.map(\.subject)
    }

    /// The sidebar counter: live attempts when the core reports them, else the snapshot's.
    static func counter(_ summaries: AttemptList?, rows: [SnapshotRow]) -> Int {
        guard let summaries else { return attempts(in: rows).count }
        return summaries.attempts.filter(\.live).count
    }

    var link: RecoveryCenterState.Link { coreLink(connection) }

    /// The held feature's attempts, newest first.
    var listed: [AttemptSummary] {
        guard let summaries else { return snapshot.map { AttemptSummary(id: $0, state: "Not reported") } }
        return summaries.attempts.filter { scope.holds($0.feature) }
    }

    /// The attempts grouped by task, the newest task first, each newest attempt first.
    var groups: [(task: String, attempts: [AttemptSummary])] {
        var order: [String] = []
        var groups: [String: [AttemptSummary]] = [:]
        for a in listed {
            let task = Self.task(a)
            if groups[task] == nil { order.append(task) }
            groups[task, default: []].append(a)
        }
        return order.map { ($0, groups[$0] ?? []) }
    }

    /// "api · Invite endpoint": a task's key and title.
    static func task(_ a: AttemptSummary) -> String {
        [a.key, a.title].compactMap { $0 }.joined(separator: " · ").nonEmpty ?? "A step"
    }

    var chosen: AttemptSummary? { listed.first { $0.id == selected } ?? listed.first }

    func held(_ id: String) -> RecoveryRow? { recovery.first { $0.attempt == id } }

    /// The attempt's pill: a recovery hold, the step's M4 state, else its lifecycle state.
    func pill(_ a: AttemptSummary) -> (String, Tone) {
        if let row = held(a.id) { return row.structured?.processAbsent == true ? ("Confirmed dead", .warn) : ("Unknown", .warn) }
        switch repairs?[a.id]?.state {
        case "Repairing": return ("Repairing", .info)
        case "Reviewing": return ("Reviewing", .info)
        case "NeedsHuman": return ("Needs human", .danger)
        default: break
        }
        switch a.state {
        case "Running", "Launching", "Preparing", "Settling", "Reserved": return (a.state, .info)
        case "Stopping": return ("Stopping", .warn)
        case "Held", "Unsupervised": return (a.state, .warn)
        case "Interrupted": return ("Interrupted", .neutral)
        case "Succeeded": return ("Completed", .ok)
        case "Failed": return ("Failed", .danger)
        case "Cancelled": return ("Cancelled", .neutral)
        // A snapshot attempt of an older helper: nothing about it was read.
        case "Not reported": return ("Not loaded", .neutral)
        default: return (a.state, .neutral)
        }
    }

    /// The second line of a list row: provider and duration, or what settled it.
    func line(_ a: AttemptSummary) -> String {
        if held(a.id) != nil { return "Held in Recovery Center" }
        if let repair = repairs?[a.id] { return [a.featureName, repair.key, repair.counter].compactMap { $0 }.joined(separator: " · ") }
        if a.live { return [a.provider.map(providerName), elapsedText(from: a.started, to: now)].compactMap { $0 }.joined(separator: " · ") }
        let validation = a.validation.map { v in v.state == "Passed" ? "validated" : v.state == "Failed" ? "validation failed" : "not validated" }
        return [a.provider.map(providerName), validation].compactMap { $0 }.joined(separator: " · ").nonEmpty ?? a.state
    }

    /// "Running · supervised", "Exited · confirmed": the process fact.
    func process(_ a: AttemptSummary) -> String {
        if held(a.id) != nil { return "Held · unsupervised" }
        switch a.state {
        case "Running": return "Running · supervised"
        case "Launching", "Preparing", "Reserved": return "\(a.state) · supervised"
        case "Unsupervised": return "Unsupervised"
        case "Stopping", "Settling": return "\(a.state)"
        default: return a.ended == nil ? a.state : "Exited · sealed"
        }
    }

    func cancelBlocked(_ a: AttemptSummary) -> String? {
        if link != .connected { return "Unavailable: the core is not connected." }
        if held(a.id) != nil { return "On hold: resolve it in Recovery Center first." }
        if !a.live { return "The result is already sealed." }
        if commandPending { return "Unavailable while a command awaits the core." }
        return nil
    }

    /// The loaded log as lines, filtered by the search; says so when there is none.
    var console: [ConsoleLine] {
        guard let result, result.attempt == chosen?.id else { return [ConsoleLine(id: 0, text: "Reading the log…")] }
        if result.logRecords == 0 { return [ConsoleLine(id: 0, text: "No log records.")] }
        let needle = search.trimmingCharacters(in: .whitespaces).lowercased()
        var lines = log.enumerated().filter { needle.isEmpty || $0.element.lowercased().contains(needle) }
            .map { ConsoleLine(id: $0.offset, text: $0.element) }
        if result.logRecords > Self.logLimit {
            lines.append(ConsoleLine(id: log.count, text: "… (\(result.logRecords - Self.logLimit) later records are not loaded)",
                                     kind: .status(.warn)))
        }
        return lines
    }

    /// The tone of a timeline event's dot.
    static func tone(_ e: AttemptEvent) -> Tone {
        let kind = e.kind
        if kind.contains("Failed") || kind.contains("Refused") || kind.contains("Lost") || kind.contains("Rejected") { return .danger }
        if kind.contains("Hold") || kind.contains("Unknown") || kind.contains("Stop") { return .warn }
        return .ok
    }

    /// Where the attempt came from: a recovery continuation, a repair, or a fresh start.
    var provenance: String {
        guard let detail else { return "Not reported" }
        if let from = detail.continues { return "Fresh continuation of attempt \(ApprovalInboxState.short(from)) after a recovery. History not replayed." }
        if let repair = chosen.flatMap({ repairs?[$0.id] }), let ordinal = repair.ordinal {
            return "Repair \(ordinal) of \(repair.limit ?? 2): a new attempt whose prompt carries the failure."
        }
        if let by = detail.continuedBy { return "Continued by attempt \(ApprovalInboxState.short(by))." }
        return "A fresh attempt."
    }
}

struct AgentRunsActions {
    var select: (String) -> Void = { _ in }
    var tab: (AttemptTab) -> Void = { _ in }
    var search: (String) -> Void = { _ in }
    var cancel: (String) -> Void = { _ in }
    var open: (Screen) -> Void = { _ in }
    var evidence: (String) -> Void = { _ in }
}

/// The Agent Runs destination, bound to the app model.
struct AgentRunsScreen: View {
    @ObservedObject var model: AppModel
    /// Attempts, repairs and the chosen attempt are read only while this destination is shown.
    let visible: Bool
    @Binding var tab: AttemptTab
    let open: (Screen) -> Void
    @State private var selected: String?
    @State private var search = ""

    var body: some View {
        AgentRunsView(state: state, actions: actions)
            .onAppear { if visible { read() } }
            .onChange(of: visible) {
                model.workspace.repairsShown = visible
                if visible { read() }
            }
            .onChange(of: model.workspace.snapshotSeq) { if visible { read() } }
            .onChange(of: selected) { if visible { read() } }
            .onChange(of: model.workspace.focusAttempt) {
                if let focus = model.workspace.focusAttempt { selected = focus; model.workspace.focusAttempt = nil }
            }
            .onChange(of: model.workspace.attempts) { if visible { read() } }
    }

    private var state: AgentRunsState {
        let workspace = model.workspace
        let chosen = selected ?? workspace.focusAttempt
        return AgentRunsState(
            connection: model.client.connection, admission: workspace.admission, scope: FeatureScope(workspace: workspace),
            summaries: workspace.attempts, snapshot: AgentRunsState.attempts(in: workspace.rows), recovery: workspace.recovery,
            repairs: workspace.repairs, selected: chosen, detail: workspace.attemptDetail, detailLoad: workspace.attemptDetailLoad,
            result: workspace.candidate, log: workspace.agentLog, commandPending: workspace.commandPending, tab: tab,
            search: search, reportsDetail: model.client.hello.map { $0.queries.contains("AttemptDetail") } ?? true)
    }

    /// The attempts and repairs; the chosen attempt's detail and, when it changes, its result
    /// and log.
    private func read() {
        let workspace = model.workspace
        workspace.repairsShown = true
        workspace.loadFeatures()
        workspace.loadRepairs()
        workspace.loadAttempts()
        guard let chosen = state.chosen, Identity.valid(chosen.id) else { return }
        workspace.loadAttemptDetail(chosen.id)
        if workspace.candidate?.attempt != chosen.id || chosen.live { workspace.loadCandidate(chosen.id) }
    }

    private var actions: AgentRunsActions {
        let workspace = model.workspace
        return AgentRunsActions(
            select: { selected = $0 },
            tab: { tab = $0 },
            search: { search = $0 },
            cancel: { workspace.cancelAttempt($0) },
            open: open,
            evidence: { attempt in
                workspace.loadCandidate(attempt)
                open(.verification)
            })
    }
}

struct AgentRunsView: View {
    @Environment(\.theme) private var theme
    let state: AgentRunsState
    var actions = AgentRunsActions()
    @State private var cancelling: AttemptSummary?

    static let cancelSentence =
        "Asks the core to stop the attempt and record it as cancelled. It counts as cancelled only after the core seals a result."

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 14) {
                CoreBanners(connection: state.connection, missing: state.reportsDetail ? nil : ("AttemptDetail", "attempt details"))
                LifecycleCard(summary: state.scope.summary)
            }
            .padding(.horizontal, 10)
            FeatureColumns(leadingWidth: 240, trailingWidth: 310, wide: 1040, medium: 760) {
                list
            } main: {
                detail
            } trailing: {
                inspector
            }
        }
        .padding(.top, 6)
        .confirmationDialog(cancelling.map { "Cancel \(AgentRunsState.task($0))?" } ?? "",
                            isPresented: Binding(get: { cancelling != nil }, set: { if !$0 { cancelling = nil } }),
                            titleVisibility: .visible) {
            if let cancelling {
                Button("Cancel Attempt", role: .destructive) { actions.cancel(cancelling.id) }
                Button("Keep Attempt", role: .cancel) {}
            }
        } message: {
            Text(Self.cancelSentence)
        }
    }

    // MARK: List

    private var list: some View {
        VStack(alignment: .leading, spacing: 2) {
            GroupLabel(state.scope.summary.map { "Attempts · \($0.code)" } ?? "Attempts").padding(.horizontal, 6)
            if state.listed.isEmpty {
                NotReported(state.summaries == nil && state.snapshot.isEmpty ? "Reading…" : "No attempts yet.").padding(10)
            }
            ForEach(state.groups, id: \.task) { group in
                Text(group.task).textStyle(.label).foregroundStyle(theme.text3).lineLimit(1)
                    .padding(EdgeInsets(top: 8, leading: 10, bottom: 2, trailing: 10))
                ForEach(group.attempts) { a in
                    SelectableRow(selected: a.id == state.chosen?.id, action: { actions.select(a.id) }) {
                        VStack(alignment: .leading, spacing: 2) {
                            HStack(spacing: 6) {
                                Text("Attempt \(a.ordinal ?? 1)").textStyle(.bodyStrong).frame(maxWidth: .infinity, alignment: .leading)
                                let pill = state.pill(a)
                                OperationalPill(pill.0, tone: pill.1)
                            }
                            Text(state.line(a)).textStyle(.secondary).foregroundStyle(theme.text2).lineLimit(1)
                        }
                    }
                }
            }
        }
    }

    // MARK: Detail

    @ViewBuilder
    private var detail: some View {
        if let a = state.chosen {
            VStack(alignment: .leading, spacing: 16) {
                HStack(alignment: .top, spacing: 12) {
                    VStack(alignment: .leading, spacing: 4) {
                        Text("\(a.key ?? "Step") · Attempt \(a.ordinal ?? 1)").textStyle(.secondary).foregroundStyle(theme.text2)
                        Text(a.title ?? "A step").font(.system(size: 22, weight: .bold)).fixedSize(horizontal: false, vertical: true)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    let pill = state.pill(a)
                    OperationalPill(pill.0, tone: pill.1)
                }
                SegmentedCapsule(selection: Binding(get: { state.tab }, set: { actions.tab($0) }), label: "Attempt view")
                FactStrip(facts: [
                    ("Attempt", ApprovalInboxState.short(a.id), true),
                    ("process", state.process(a), false),
                    ("Started", boardTime(a.started, now: state.now) ?? "Not started", false),
                    ("Duration", elapsedText(from: a.started, to: a.ended ?? (a.live ? state.now : nil)) ?? "—", false),
                    ("Base commit", state.detail?.base.map { String($0.prefix(7)) } ?? "Not reported", true),
                ])
                switch state.tab {
                case .timeline:
                    timeline
                    terminal(lines: 8)
                case .logs: terminal(lines: nil)
                case .evidence: evidence(a)
                case .claims: claims
                }
            }
            .padding(EdgeInsets(top: 22, leading: 24, bottom: 22, trailing: 24))
            .frame(maxWidth: .infinity, alignment: .topLeading)
            .cardElevation(radius: 18)
        } else {
            Placeholder(symbol: "play.rectangle", tile: Palette.blue, title: "No attempts yet",
                        message: "An attempt appears here when a plan step starts.") {
                Button("Open Execution Plan") { actions.open(.executionPlan) }.buttonStyle(.jojo(.secondary, compact: true))
            }
            .frame(maxWidth: .infinity)
            .padding(24)
            .cardElevation(radius: 18)
        }
    }

    @ViewBuilder
    private var timeline: some View {
        LabeledSection(title: "Event timeline") {
            if let events = state.detail?.events, !events.isEmpty {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(events.enumerated()), id: \.offset) { index, e in
                        HStack(spacing: 10) {
                            Circle().fill(index == events.count - 1 && state.chosen?.live == true ? theme.accent
                                          : theme.foreground(AgentRunsState.tone(e))).frame(width: 8, height: 8)
                            Text(e.summary).font(.system(size: 13)).frame(maxWidth: .infinity, alignment: .leading)
                            Text(e.at.map { $0.formatted(.dateTime.hour(.twoDigits(amPM: .omitted)).minute(.twoDigits).second(.twoDigits)) } ?? "")
                                .textStyle(.mono).foregroundStyle(theme.text2)
                        }
                        .padding(.vertical, 7)
                        .overlay(alignment: .top) { if index > 0 { Rectangle().fill(theme.sep).frame(height: 0.5) } }
                    }
                }
                .padding(.horizontal, 14).padding(.vertical, 4)
                .cardElevation(radius: 14)
            } else {
                NotReported(state.detailLoad == .loading ? "Reading…" : "No events recorded about this attempt.")
            }
        }
    }

    private func terminal(lines: Int?) -> some View {
        let all = state.console
        let shown = lines.map { Array(all.suffix($0)) } ?? all
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                GroupLabel("Terminal output")
                Spacer()
                LineField(prompt: "Search output", size: 12, text: Binding(get: { state.search }, set: { actions.search($0) }))
                    .frame(width: 160, height: 26)
                    .padding(.horizontal, 10)
                    .background(theme.glassStrong, in: Capsule())
            }
            VStack(alignment: .leading, spacing: 2) {
                ForEach(shown) { line in
                    Text(line.text).font(.system(size: 12, design: .monospaced)).foregroundStyle(RGBA(0xD6D9E4).color)
                        .frame(maxWidth: .infinity, alignment: .leading).textSelection(.enabled)
                }
            }
            .padding(EdgeInsets(top: 14, leading: 16, bottom: 14, trailing: 16))
            .background(RGBA(0x15161B).color, in: RoundedRectangle(cornerRadius: 14))
        }
    }

    @ViewBuilder
    private func evidence(_ a: AttemptSummary) -> some View {
        LabeledSection(title: "Host validation runs", link: ("View Evidence", { actions.evidence(a.id) })) {
            if let runs = state.detail?.validations, !runs.isEmpty {
                DefinitionCard(rows: runs.map { r in
                    Definition(key: "V-\(r.ordinal)", value: boardTime(r.at, now: state.now) ?? "",
                               pill: r.passed ? ("Passed", .ok) : ("Failed", .danger))
                }, keyWidth: 60)
            } else {
                NotReported("No host validation of this attempt yet.")
            }
            Text("Evidence records: \(state.detail?.evidenceRecords ?? 0)").textStyle(.secondary).foregroundStyle(theme.text2)
        }
    }

    @ViewBuilder
    private var claims: some View {
        LabeledSection(title: "Resource claims") {
            if let claims = state.detail?.claims, !claims.isEmpty {
                DefinitionCard(rows: claims.map { Definition(key: $0.mode.lowercased(), value: $0.resource, mono: true,
                                                             pill: ($0.state.capitalized, $0.state == "ACTIVE" ? .info : .neutral)) },
                               keyWidth: 70)
            } else {
                NotReported("No claims recorded for this attempt.")
            }
        }
    }

    // MARK: Inspector

    private var inspector: some View {
        GlassInspector {
            if let a = state.chosen {
                let d = state.detail
                VStack(alignment: .leading, spacing: 4) {
                    Text("Agent attempt · \(a.key ?? "step")").textStyle(.secondary).foregroundStyle(theme.text2)
                    Text(a.id).font(.system(size: 13, weight: .bold, design: .monospaced)).lineLimit(1).truncationMode(.middle)
                        .textSelection(.enabled)
                }
                LabeledSection(title: "Provider session") {
                    DefinitionCard(rows: [
                        Definition(key: "Provider", value: a.provider.map(providerName) ?? "Not launched"),
                        Definition(key: "Session", value: d?.session ?? "Not reported", mono: d?.session != nil),
                        Definition(key: "Resumable", value: "No: each attempt is a fresh run"),
                        Definition(key: "Executable", value: d?.executableVersion ?? "Not reported"),
                    ], keyWidth: 86)
                }
                LabeledSection(title: "process supervision") {
                    DefinitionCard(rows: [
                        Definition(key: "State", value: state.process(a)),
                        Definition(key: "PID", value: d?.pid.map(String.init) ?? "Not reported", mono: d?.pid != nil),
                        Definition(key: "process group", value: d?.pgid.map(String.init) ?? "Not reported", mono: d?.pgid != nil),
                        Definition(key: "Liveness", value: d?.heartbeat.map { "Heartbeat \(boardTime($0, now: state.now) ?? "")" } ?? "Not reported"),
                    ], keyWidth: 86)
                }
                LabeledSection(title: "Workspace") {
                    DefinitionCard(rows: [
                        Definition(key: "Worktree", value: d?.worktree ?? "Not reported", mono: d?.worktree != nil),
                        Definition(key: "Repository", value: a.repositoryName ?? "Not reported"),
                    ], keyWidth: 86)
                }
                LabeledSection(title: "Continuation") {
                    DefinitionCard(rows: [Definition(key: "Provenance", value: state.provenance)], keyWidth: 86)
                }
                if let repair = state.repairs?[a.id] {
                    LabeledSection(title: "Repair and review") {
                        DefinitionCard(rows: [
                            Definition(key: "Repairs", value: repair.counter),
                            Definition(key: "Review", value: repair.review?.verdict ?? "None"),
                            Definition(key: "Hold", value: repair.hold ?? "None"),
                        ], keyWidth: 86)
                    }
                }
                LabeledSection(title: "Checkpoints") { NotReported("The core records stream offsets, not checkpoints of the work.") }
                LabeledSection(title: "Actions") {
                    VStack(alignment: .leading, spacing: 8) {
                        VStack(alignment: .leading, spacing: 6) {
                            Button("Request Safe Stop") {}.buttonStyle(.jojo(.secondary, compact: true)).disabled(true)
                                .help("Not available yet: the core has no safe stop.")
                            Button("Cancel Attempt…") { cancelling = a }.buttonStyle(.jojo(.ghost, compact: true))
                                .disabled(state.cancelBlocked(a) != nil).help(state.cancelBlocked(a) ?? Self.cancelSentence)
                        }
                        Text("Safe stop is not available yet. Cancel stops the whole process group and keeps the worktree.")
                            .font(.system(size: 11)).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
                    }
                    .padding(EdgeInsets(top: 10, leading: 12, bottom: 10, trailing: 12))
                    .cardElevation(radius: 12)
                    Text("A stopped process is not a successful attempt. Exit is recorded only when the OS confirms it.")
                        .font(.system(size: 11)).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
                }
            } else {
                NotReported("Choose an attempt.")
            }
        }
    }
}

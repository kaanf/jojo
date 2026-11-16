import JojoCore
import SwiftUI

// Agent Runs: docs/design/screens/AGENT_RUNS.md. The screen draws only what the helper
// reports; a part without a source says so.

/// One attempt as the snapshot reports it: its identity and the core's record texts.
struct AttemptRecord: Identifiable, Equatable {
    let id: String
    var record: String
    var recordComplete = true
    /// The worktree record of the same identity, when the snapshot has one.
    var worktree: String?
}

/// Everything the screen reads, as plain values.
struct AgentRunsState: Equatable {
    var connection: HelperConnection
    var admission: String
    /// nil until a snapshot has arrived.
    var attempts: [AttemptRecord]?
    var recovery: [RecoveryRow]
    /// The one result the workspace has loaded; it belongs to the attempt it names.
    var result: CandidateReview?
    var log: [String]
    var commandPending: Bool

    /// Log records the workspace reads for a loaded result.
    static let logLimit: Int64 = 64
    static let attemptKinds: Set<String> = ["scheduler_attempt", "attempt"]

    /// The snapshot's attempt rows, in its order, each with its worktree record.
    static func attempts(in rows: [SnapshotRow]) -> [AttemptRecord] {
        let worktrees = Dictionary(rows.filter { $0.kind == "worktree_admin" }.map { ($0.subject, $0.value) },
                                   uniquingKeysWith: { first, _ in first })
        var seen = Set<String>()
        return rows.filter { attemptKinds.contains($0.kind) && seen.insert($0.subject).inserted }.map {
            AttemptRecord(id: $0.subject, record: $0.value, recordComplete: $0.detailComplete, worktree: worktrees[$0.subject])
        }
    }

    var link: RecoveryCenterState.Link {
        switch connection {
        case .ready: return .connected
        case .notStarted, .handshaking: return .starting
        case .incompatible: return .incompatible
        case .launchFailed, .channelLost, .ended: return .lost
        }
    }

    /// Admission is stated only while the helper is connected and has reported it.
    var admissionClosed: Bool { link == .connected && !admission.isEmpty && admission != "Open" }

    var loading: Bool { attempts == nil && (link == .connected || link == .starting) }

    var runs: [AttemptRun] {
        (attempts ?? []).map { record in
            AttemptRun(record: record, held: recovery.first { $0.attempt == record.id },
                       result: result?.attempt == record.id ? result : nil)
        }
    }

    /// Why "Cancel Attempt" cannot be sent, or nil when it can.
    func cancelBlocked(_ run: AttemptRun) -> String? {
        if link != .connected { return "Unavailable: the core is not connected." }
        if run.held != nil { return "On hold: resolve it in Recovery Center first." }
        if run.result?.sealed == true { return "The result is already sealed." }
        if commandPending { return "Unavailable while a command awaits the core." }
        return nil
    }

    /// The loaded log of an attempt as console lines; says so when there is none.
    func console(_ run: AttemptRun) -> [ConsoleLine] {
        guard let result = run.result else { return [ConsoleLine(id: 0, text: "Not loaded.")] }
        if result.logRecords == 0 { return [ConsoleLine(id: 0, text: "No log records.")] }
        if log.isEmpty { return [ConsoleLine(id: 0, text: "Reading the log records…")] }
        var lines = log.enumerated().map { ConsoleLine(id: $0.offset, text: $0.element) }
        if result.logRecords > Self.logLimit {
            lines.append(ConsoleLine(id: lines.count, text: "… (\(result.logRecords - Self.logLimit) later records are not loaded)",
                                     kind: .status(.warn)))
        }
        return lines
    }
}

/// One attempt with what else is known about it: a recovery hold, a loaded result.
struct AttemptRun: Identifiable, Equatable {
    let record: AttemptRecord
    let held: RecoveryRow?
    let result: CandidateReview?

    var id: String { record.id }
    /// The first eight characters of the identity: the list title, under "Attempts".
    var short: String { String(record.id.prefix(8)) }
    var title: String { "Attempt \(short)" }
    var processGone: Bool { held?.structured?.processAbsent == true }
    var recordText: String { record.record + (record.recordComplete ? "" : " … (partial)") }

    /// The core reports no lifecycle state per attempt. "Running" is never derived: an
    /// attempt without a sealed result may be running, starting or stopped.
    var pill: (state: OperationalState, label: String) {
        if held != nil { return (.recoveryRequired, processGone ? "Confirmed dead" : "Unknown") }
        guard let result else { return (.idle, "Not loaded") }
        guard result.sealed else { return (.idle, "Not sealed") }
        switch result.outcome {
        case "Succeeded": return (.completed, "Succeeded")
        case "Failed": return (.failed, "Failed")
        case "Interrupted": return (.recoveryRequired, "Interrupted")
        default: return (.idle, result.outcome.isEmpty ? "Sealed" : result.outcome)
        }
    }

    /// Second line of the list item.
    var line: String {
        if held != nil { return "Held in Recovery Center" }
        guard let result else { return "Scheduler record: \(record.record)" }
        guard result.sealed else { return "No sealed result yet" }
        return result.reason.isEmpty ? "No reason recorded" : result.reason
    }

    /// The records the app holds for the attempt, in lifecycle order. They carry no times.
    var facts: [TimelineEntry] {
        var entries = [TimelineEntry(text: "Scheduler record: \(recordText)")]
        if let worktree = record.worktree { entries.append(TimelineEntry(text: "Worktree record: \(worktree)")) }
        if held != nil {
            entries.append(TimelineEntry(text: "Held for recovery: " + (processGone ? "every process proven gone" : "process state unknown"),
                                         tone: .danger))
        }
        guard let result else { return entries }
        guard result.sealed else { return entries + [TimelineEntry(text: "No sealed result yet", tone: .neutral)] }
        let tone: Tone = result.outcome == "Succeeded" ? .ok : ["Failed", "Interrupted"].contains(result.outcome) ? .danger : .neutral
        entries.append(TimelineEntry(text: "Result sealed: \(result.outcome)" + (result.reason.isEmpty ? "" : " · \(result.reason)"), tone: tone))
        if result.validation != .notRun {
            entries.append(TimelineEntry(text: "Validation: \(result.validation.label)",
                                         tone: result.validation == .pass ? .ok : result.validation == .fail ? .danger : .neutral))
        }
        if result.decision != "NONE" { entries.append(TimelineEntry(text: "Decision: \(result.decision)", tone: .neutral)) }
        return entries
    }

    var recoveryStatus: String {
        guard held != nil else { return "None reported by the last inspection." }
        return "Held in Recovery Center · " + (processGone ? "every process proven gone" : "process state unknown")
    }
}

struct AgentRunsActions {
    var load: (String) -> Void = { _ in }
    var cancel: (String) -> Void = { _ in }
    var open: (Screen) -> Void = { _ in }
}

/// The Agent Runs destination, bound to the app model.
struct AgentRunsScreen: View {
    @ObservedObject var model: AppModel
    let open: (Screen) -> Void

    var body: some View {
        let workspace = model.workspace
        AgentRunsView(
            state: AgentRunsState(connection: model.client.connection, admission: workspace.admission,
                                  attempts: workspace.snapshotSeq == nil && workspace.rows.isEmpty
                                      ? nil : AgentRunsState.attempts(in: workspace.rows),
                                  recovery: workspace.recovery, result: workspace.candidate, log: workspace.agentLog,
                                  commandPending: workspace.commandPending),
            actions: AgentRunsActions(load: { workspace.loadCandidate($0) }, cancel: { workspace.cancelAttempt($0) }, open: open))
    }
}

struct AgentRunsView: View {
    @Environment(\.theme) private var theme
    let state: AgentRunsState
    var actions = AgentRunsActions()
    @State private var selected: String?
    @State private var cancelling: AttemptRun?

    init(state: AgentRunsState, actions: AgentRunsActions = AgentRunsActions(), selected: String? = nil) {
        self.state = state
        self.actions = actions
        _selected = State(initialValue: selected)
    }

    var body: some View {
        let runs = state.runs
        Group {
            if let current = runs.first(where: { $0.id == selected }) ?? runs.first {
                columns(runs, current)
            } else {
                VStack(alignment: .leading, spacing: 16) {
                    banners
                    placeholder.frame(minHeight: 260)
                }
                .padding(EdgeInsets(top: 14, leading: 18, bottom: 18, trailing: 18))
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .confirmationDialog(cancelling.map { "Cancel \($0.title)?" } ?? "",
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

    static let cancelSentence =
        "Asks the core to stop the attempt and record it as cancelled. It counts as cancelled only after the core seals a result."

    private func columns(_ runs: [AttemptRun], _ current: AttemptRun) -> some View {
        GeometryReader { geometry in
            // Three columns only while the detail keeps at least 440.
            let wide = geometry.size.width >= 960
            HStack(spacing: 0) {
                panel(edge: .trailing) { list(runs, current) }.frame(width: 240)
                Scrolling {
                    VStack(alignment: .leading, spacing: 16) {
                        banners
                        detail(current)
                        if !wide { Card(.panelList) { inspector(current) } }
                    }
                    .padding(EdgeInsets(top: 14, leading: 18, bottom: 18, trailing: 18))
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                if wide {
                    panel(edge: .leading) { Scrolling { inspector(current) } }.frame(width: 280)
                }
            }
        }
    }

    private func panel(edge: Alignment, @ViewBuilder _ content: () -> some View) -> some View {
        content()
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            .background(theme.sidebar)
            .overlay(alignment: edge) { Rectangle().fill(theme.sidebarEdge).frame(width: 1) }
    }

    @ViewBuilder
    private var banners: some View {
        switch state.link {
        case .lost:
            Banner(.danger, title: "Jojo core isn't responding",
                   message: "Showing the last known state, read-only. No actions can be requested.")
        case .incompatible:
            Banner(.danger, title: "Jojo core speaks another protocol version",
                   message: "Read-only. No actions can be requested.")
        case .connected, .starting:
            EmptyView()
        }
        if state.admissionClosed {
            Banner(.danger, title: "Execution admission is closed",
                   message: "No new attempt starts until you review the inspection.") {
                Button("Open Recovery Center") { actions.open(.recoveryCenter) }.buttonStyle(.jojo(.secondary, compact: true))
            }
        }
    }

    @ViewBuilder
    private var placeholder: some View {
        if state.loading {
            LoadingBlock("Loading attempts…")
        } else if state.attempts == nil {
            StateBlock(symbol: "play.circle", title: "No snapshot was loaded",
                       message: "The core stopped answering before it reported its attempts.")
        } else {
            StateBlock(symbol: "play.circle", title: "No attempts yet",
                       message: "An attempt appears here when an approved plan step is started.") {
                Button("Open Execution Plan") { actions.open(.executionPlan) }.buttonStyle(.jojo(.secondary, compact: true))
            }
        }
    }

    // MARK: List

    private func list(_ runs: [AttemptRun], _ current: AttemptRun) -> some View {
        Scrolling {
            VStack(alignment: .leading, spacing: 0) {
                SectionHeading("Attempts · \(runs.count)")
                    .padding(EdgeInsets(top: 14, leading: 16, bottom: 4, trailing: 14))
                VStack(spacing: 2) {
                    ForEach(runs) { run in item(run, selected: run.id == current.id) }
                }
                Footnote("Tasks, features and lifecycle stages are not reported yet. Attempts are listed by identity.")
                    .padding(EdgeInsets(top: 12, leading: 18, bottom: 14, trailing: 16))
            }
        }
    }

    private func item(_ run: AttemptRun, selected isSelected: Bool) -> some View {
        Button { selected = run.id } label: {
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 8) {
                    Text(run.short).textStyle(.controlStrong).foregroundStyle(theme.text).lineLimit(1)
                    Spacer(minLength: 0)
                    OperationalPill(run.pill.state, label: run.pill.label)
                }
                Text(run.line).textStyle(.meta).foregroundStyle(theme.text3).lineLimit(1)
            }
            .padding(.vertical, 8)
            .padding(.horizontal, 10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(isSelected ? theme.accent.opacity(0.16) : .clear, in: RoundedRectangle(cornerRadius: 8))
            .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(isSelected ? theme.info.opacity(0.4) : .clear, lineWidth: 1))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .padding(.horizontal, 8)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }

    // MARK: Detail

    @ViewBuilder
    private func detail(_ run: AttemptRun) -> some View {
        Text(run.title).font(.system(size: 17, weight: .semibold)).tracking(-0.17).foregroundStyle(theme.textStrong)
        VStack(alignment: .leading, spacing: 8) {
            Card(.panel) {
                WrappingRow(spacing: 28, lineSpacing: 14) {
                    LabeledField("Attempt") { identifier(run.id) }
                    LabeledField("State") { OperationalPill(run.pill.state, label: run.pill.label) }
                    LabeledField("Scheduler record") { value(run.recordText) }
                    LabeledField("Worktree record") { value(run.record.worktree ?? "None reported", known: run.record.worktree != nil) }
                }
            }
            Footnote("Start time, duration and base commit are not reported yet.")
        }
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Recorded facts")
            Card(CardShape(radius: 10, vertical: 8, horizontal: 14)) { Timeline(run.facts) }
            Footnote("Events and their times are not reported per attempt yet. These are the core's records in lifecycle order.")
        }
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Terminal output") {
                if let result = run.result, result.logRecords > 0 {
                    Text("\(state.log.count) of \(result.logRecords) records").textStyle(.meta).foregroundStyle(theme.text3)
                }
            }
            Console(state.console(run))
        }
    }

    private func identifier(_ text: String) -> some View {
        Text(text).textStyle(.mono).foregroundStyle(theme.text).textSelection(.enabled)
    }

    private func value(_ text: String, known: Bool = true) -> some View {
        Text(text).textStyle(.control).foregroundStyle(known ? theme.text : theme.text3)
    }

    // MARK: Inspector

    private func inspector(_ run: AttemptRun) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 6) {
                Text("Agent attempt").font(.system(size: 11, weight: .medium)).foregroundStyle(theme.text3)
                Text(run.title).textStyle(.inspectorTitle).foregroundStyle(theme.textStrong)
                OperationalPill(run.pill.state, label: run.pill.label, standalone: true)
            }
            .padding(EdgeInsets(top: 14, leading: 16, bottom: 12, trailing: 16))
            .frame(maxWidth: .infinity, alignment: .leading)
            .overlay(alignment: .bottom) { Rectangle().fill(theme.hairline).frame(height: 1) }
            InspectorSection("Supervision") {
                FactGrid {
                    Fact("State") { OperationalPill(run.pill.state, label: run.pill.label) }
                    Fact("Scheduler record", run.recordText)
                    Fact("Worktree record", run.record.worktree ?? "None reported")
                }
            }
            InspectorSection("Execution evidence") { evidence(run) }
            InspectorSection("Recovery status") {
                Text(run.recoveryStatus).textStyle(.control).foregroundStyle(theme.text2)
                    .fixedSize(horizontal: false, vertical: true)
                if run.held != nil {
                    Button("Inspect Recovery") { actions.open(.recoveryCenter) }.buttonStyle(.jojo(.secondary, compact: true))
                }
            }
            InspectorSection("Actions") { stop(run) }
            InspectorSection("Not reported yet", last: true) {
                Text("Provider and session, PID and process group, liveness, start and stop times, worktree path, branch, base commit, continuation, resource claims and checkpoints.")
                    .textStyle(.meta).foregroundStyle(theme.text3).lineSpacing(2)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    @ViewBuilder
    private func evidence(_ run: AttemptRun) -> some View {
        if let result = run.result {
            FactGrid {
                Fact("Result") { resultPill(result) }
                Fact("Provider said", result.providerTerminal.isEmpty ? "Nothing" : result.providerTerminal)
                Fact("Log", "\(result.logRecords) records")
                Fact("Candidate") {
                    if let digest = result.candidate { identifier(String(digest.prefix(12))) } else { Text("None").font(.system(size: 12.5)) }
                }
                Fact("Validation") { validationPill(result.validation) }
                Fact("Decision", result.decision)
            }
            HStack(spacing: 6) {
                Button("View Evidence") { actions.open(.verification) }.buttonStyle(.jojo(.secondary, compact: true))
                    .help("Opens Verification & Evidence, which shows this loaded attempt.")
                Button("Reload") { actions.load(run.id) }.buttonStyle(.jojo(.ghost, compact: true))
                    .disabled(state.link != .connected)
            }
        } else {
            Text("The result of this attempt is not loaded. One result is loaded at a time; Verification & Evidence shows the same one.")
                .textStyle(.meta).foregroundStyle(theme.text2).lineSpacing(2)
                .fixedSize(horizontal: false, vertical: true)
            Button { actions.load(run.id) } label: { Label("Load Result", systemImage: "arrow.down.circle") }
                .buttonStyle(.jojo(.secondary, compact: true))
                .disabled(state.link != .connected)
                .help("Reads the sealed result, the validation and the first log records of this attempt.")
        }
    }

    @ViewBuilder
    private func resultPill(_ result: CandidateReview) -> some View {
        if !result.sealed {
            EvidencePill(.notRun, label: "Not sealed")
        } else {
            switch result.outcome {
            case "Succeeded": EvidencePill(.passed, label: "Succeeded")
            case "Failed": EvidencePill(.failed, label: "Failed")
            case "Interrupted": EvidencePill(.incomplete, label: "Interrupted")
            default: EvidencePill(.notRun, label: result.outcome.isEmpty ? "Sealed" : result.outcome)
            }
        }
    }

    @ViewBuilder
    private func validationPill(_ verdict: ValidationVerdict) -> some View {
        switch verdict {
        case .pass: EvidencePill(.passed)
        case .fail: EvidencePill(.failed)
        case .notRun: EvidencePill(.notRun)
        case .unknown: EvidencePill(.unknown)
        }
    }

    @ViewBuilder
    private func stop(_ run: AttemptRun) -> some View {
        let blocked = state.cancelBlocked(run)
        VStack(alignment: .leading, spacing: 10) {
            VStack(alignment: .leading, spacing: 4) {
                Button {} label: { Label("Request Safe Stop", systemImage: "stop.circle") }
                    .buttonStyle(.jojo(.secondary))
                    .disabled(true)
                DisabledReason("The core has no safe-stop command yet.")
            }
            VStack(alignment: .leading, spacing: 4) {
                Button { cancelling = run } label: { Label("Cancel Attempt…", systemImage: "xmark.circle") }
                    .buttonStyle(.jojo(.destructive))
                    .disabled(blocked != nil)
                    .help(blocked ?? Self.cancelSentence)
                if let blocked { DisabledReason(blocked) } else { Footnote(Self.cancelSentence) }
            }
            if blocked == nil {
                NoticeWell("A stopped process is not a successful attempt. Exit is recorded only when the OS confirms it.")
            }
        }
    }
}

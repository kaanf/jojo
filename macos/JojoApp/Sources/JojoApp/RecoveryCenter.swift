import JojoCore
import SwiftUI

// Recovery Center and launch review: docs/design/screens/RECOVERY_CENTER.md. The screen
// draws only what the helper reports; a part without a source says so.

/// Everything the screen reads, as plain values.
struct RecoveryCenterState: Equatable {
    enum Link: Equatable, Sendable {
        case connected, starting, lost, incompatible
    }

    enum Admission: Equatable, Sendable {
        case open, closed, unknown
    }

    var connection: HelperConnection
    var admission: String
    var rows: [RecoveryRow]
    var digest: String
    var commandPending: Bool

    var link: Link {
        switch connection {
        case .ready: return .connected
        case .notStarted, .handshaking: return .starting
        case .incompatible: return .incompatible
        case .launchFailed, .channelLost, .ended: return .lost
        }
    }

    /// Admission is stated only while the helper is connected and has reported it.
    var admissionState: Admission {
        guard link == .connected, !admission.isEmpty else { return .unknown }
        return admission == "Open" ? .open : .closed
    }

    var epoch: Int64? {
        if case .ready(let epoch) = connection { return epoch }
        return nil
    }

    var items: [HeldItem] { rows.map(HeldItem.init) }

    /// No inspection has arrived yet on a connection that can still deliver one.
    var loading: Bool { digest.isEmpty && (link == .connected || link == .starting) }

    /// A recovery command can be sent: connected, an inspection on screen, nothing pending.
    var canAct: Bool { link == .connected && !digest.isEmpty && !commandPending }

    /// The inspection digest as shown: its first twelve characters.
    var digestShown: String { digest.isEmpty ? "None" : String(digest.prefix(12)) }

    /// Why "Start New Session" cannot be sent, or nil when it can.
    func startBlocked(reviewed: Set<String>) -> String? {
        if link != .connected { return "Unavailable: the core is not connected." }
        if digest.isEmpty { return "Unavailable until an inspection is loaded." }
        if admissionState == .open { return "Admission is already open." }
        if !rows.allSatisfy({ reviewed.contains($0.attempt) }) { return "Unavailable until every finding is marked reviewed." }
        if commandPending { return "Unavailable while a command awaits the core." }
        return nil
    }
}

/// One held subject. Today every subject is a prior attempt.
struct HeldItem: Identifiable, Equatable {
    let row: RecoveryRow

    var id: String { row.attempt }
    var title: String { "Attempt \(row.attempt.prefix(8))" }

    /// True only when the helper states that every process is proven gone.
    var processGone: Bool { row.structured?.processAbsent == true }
    var pillLabel: String { processGone ? "Confirmed dead" : "Unknown" }

    var blockerLine: String {
        guard let structured = row.structured else { return "Blockers not reported" }
        switch structured.blockers.count {
        case 0: return "No blockers"
        case 1: return "1 blocker"
        default: return "\(structured.blockers.count) blockers"
        }
    }

    /// A helper that reports no choices still decides; the control stays available.
    func offers(_ choice: RecoveryChoice) -> Bool {
        row.structured.map { $0.choices.contains(choice) } ?? true
    }

    /// The first offered choice other than Hold is the primary action.
    var primary: RecoveryChoice? {
        RecoveryChoice.allCases.first { $0 != .Hold && offers($0) }
    }

    func sentence(_ choice: RecoveryChoice) -> String {
        if offers(choice) { return choice.sentence }
        return processGone
            ? "Unavailable: the core's inspection does not offer this choice."
            : "Unavailable while process state is unknown. Needs OS evidence that the process exited."
    }

    var footnote: String {
        if row.structured == nil {
            return "This core does not report which choices it offers. It decides when you choose."
        }
        return primary == nil
            ? "Only Hold is available. Confirming in the UI never substitutes for OS evidence that a process has exited."
            : "Offered by the core's inspection. The core checks eligibility again when you choose."
    }

    var findingTitle: String {
        "\(title): " + (processGone ? "every process proven gone" : "process state unknown")
    }

    var findingSentence: String {
        row.structured?.blockers.first ?? row.assessment
    }
}

extension RecoveryChoice {
    var buttonTitle: String {
        switch self {
        case .ValidateOutput: return "Validate Output"
        case .ContinueWithHistory: return "Continue with History"
        case .ContinueFreshContext: return "Continue Fresh"
        case .Hold: return "Hold"
        }
    }

    var sentence: String {
        switch self {
        case .ValidateOutput: return "Checks the existing output against the task's verification requirements. No new inference."
        case .ContinueWithHistory: return "Starts a new attempt in a new worktree that resumes the provider session with its history."
        case .ContinueFreshContext: return "Starts a new attempt in a new worktree with a new session."
        case .Hold: return "Keeps everything as is. State, worktree and evidence are retained."
        }
    }

    /// Hold changes nothing and is sent directly; the others ask once.
    var confirms: Bool { self != .Hold }
}

struct RecoveryActions {
    var inspect: () -> Void = {}
    var resolve: (String, RecoveryChoice) -> Void = { _, _ in }
    var start: () -> Void = {}
}

/// The Recovery Center destination, bound to the app model.
struct RecoveryCenterScreen: View {
    @ObservedObject var model: AppModel

    var body: some View {
        RecoveryCenterView(
            state: RecoveryCenterState(connection: model.client.connection, admission: model.workspace.admission,
                                       rows: model.workspace.recovery, digest: model.workspace.recoveryDigest,
                                       commandPending: model.workspace.commandPending),
            actions: RecoveryActions(inspect: { model.workspace.inspectRecovery() },
                                     resolve: { model.workspace.resolveRecovery($0, $1) },
                                     start: { model.workspace.startAfterRecovery() }))
    }
}

struct RecoveryCenterView: View {
    private struct Confirmation: Equatable {
        let item: HeldItem
        let choice: RecoveryChoice
    }

    @Environment(\.theme) private var theme
    let state: RecoveryCenterState
    var actions = RecoveryActions()
    @State private var reviewing: Bool
    @State private var selected: String?
    /// Findings marked reviewed, and the inspection they were marked on.
    @State private var marks: (digest: String, attempts: Set<String>) = ("", [])
    @State private var confirmation: Confirmation?

    init(state: RecoveryCenterState, actions: RecoveryActions = RecoveryActions(), reviewing: Bool = false) {
        self.state = state
        self.actions = actions
        _reviewing = State(initialValue: reviewing)
    }

    /// A new inspection clears the marks: they were made on other facts.
    private var reviewed: Set<String> { marks.digest == state.digest ? marks.attempts : [] }

    private var current: HeldItem? {
        state.items.first { $0.id == selected } ?? state.items.first
    }

    var body: some View {
        Group {
            if reviewing, state.admissionState == .closed { review } else { center }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .confirmationDialog(confirmation.map { "\($0.choice.buttonTitle): \($0.item.title)?" } ?? "",
                            isPresented: Binding(get: { confirmation != nil }, set: { if !$0 { confirmation = nil } }),
                            titleVisibility: .visible) {
            if let confirmation {
                Button(confirmation.choice.buttonTitle) { actions.resolve(confirmation.item.id, confirmation.choice) }
                Button("Cancel", role: .cancel) {}
            }
        } message: {
            Text((confirmation?.choice.sentence ?? "") + " The core checks eligibility again.")
        }
    }

    // MARK: Center

    private var center: some View {
        GeometryReader { geometry in
            // Three columns only while the detail keeps at least 420.
            let wide = geometry.size.width >= 920
            HStack(spacing: 0) {
                panel(edge: .trailing) { heldList }.frame(width: 240)
                Scrolling {
                    VStack(alignment: .leading, spacing: 16) {
                        banners(openReview: true)
                        detail
                        if !wide { aside }
                    }
                    .padding(EdgeInsets(top: 18, leading: 22, bottom: 24, trailing: 22))
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                if wide {
                    panel(edge: .leading) { Scrolling { aside.padding(14) } }.frame(width: 260)
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
    private func banners(openReview: Bool) -> some View {
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
        if state.admissionState == .closed {
            Banner(.danger, title: "Execution admission is closed",
                   message: openReview
                       ? "Review the inspection to start a new session."
                       : "No new work starts and nothing continues until you review this inspection and start a new session.") {
                if openReview {
                    Button("Open Review") { reviewing = true }.buttonStyle(.jojo(.secondary, compact: true))
                }
            }
        }
    }

    private var heldList: some View {
        Scrolling {
            VStack(alignment: .leading, spacing: 0) {
                SectionHeading("Held · \(state.rows.count)")
                    .padding(EdgeInsets(top: 12, leading: 16, bottom: 6, trailing: 12))
                VStack(spacing: 2) {
                    ForEach(state.items) { item in heldRow(item) }
                }
                SectionHeading("Settled today").padding(EdgeInsets(top: 14, leading: 16, bottom: 6, trailing: 16))
                Text("Not reported yet.").textStyle(.meta).foregroundStyle(theme.text3)
                    .padding(EdgeInsets(top: 0, leading: 18, bottom: 14, trailing: 16))
            }
        }
    }

    private func heldRow(_ item: HeldItem) -> some View {
        let isSelected = item.id == current?.id
        return Button { selected = item.id } label: {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text("Prior attempt").font(.system(size: 11)).foregroundStyle(theme.text3)
                    Spacer(minLength: 0)
                    OperationalPill(.recoveryRequired, label: item.pillLabel)
                }
                Text(item.title).textStyle(.controlStrong).foregroundStyle(theme.text)
                Text(item.blockerLine).textStyle(.meta).foregroundStyle(theme.text3)
            }
            .padding(.vertical, 9)
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

    @ViewBuilder
    private var detail: some View {
        if let item = current {
            header(item)
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Inspected facts · read-only")
                Card(.panel) { facts(item) }
                Footnote("Worktree, snapshot, evidence, session, claims and the process group are not reported as separate facts yet. See Findings.")
            }
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Recovery actions")
                Card(.panelList) {
                    Rows(RecoveryChoice.allCases.map { ActionRow(choice: $0) }) { action(item, $0.choice) }
                }
            }
            Footnote(item.footnote)
        } else if state.loading {
            LoadingBlock("Loading the recovery inspection…")
        } else if state.digest.isEmpty {
            StateBlock(symbol: "cross.case", title: "No inspection was loaded",
                       message: "The core stopped answering before it reported a recovery inspection.")
                .frame(minHeight: 260)
        } else {
            StateBlock(symbol: "checkmark.circle", title: "Nothing is on hold",
                       message: "The last inspection found no interrupted or unknown work.") {
                reinspect(help: "Read-only. Collects fresh facts for every held item.")
            }
            .frame(minHeight: 260)
        }
    }

    private func reinspect(help: String) -> some View {
        Button { actions.inspect() } label: { Label("Reinspect", systemImage: "arrow.clockwise") }
            .buttonStyle(.jojo(.secondary))
            .disabled(state.link != .connected)
            .help(help)
    }

    private func header(_ item: HeldItem) -> some View {
        HStack(alignment: .top, spacing: 10) {
            VStack(alignment: .leading, spacing: 5) {
                Text("Prior attempt").textStyle(.secondary).foregroundStyle(theme.text2)
                Text(item.title).font(.system(size: 20, weight: .bold)).tracking(-0.4).foregroundStyle(theme.textStrong)
                HStack(spacing: 8) {
                    OperationalPill(.recoveryRequired, label: item.pillLabel, standalone: true)
                    Text("Inspection age not reported").textStyle(.meta).foregroundStyle(theme.text3)
                }
            }
            Spacer(minLength: 0)
            reinspect(help: "Read-only. Collects fresh facts for every held item.")
        }
    }

    private func facts(_ item: HeldItem) -> some View {
        Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 10, verticalSpacing: 7) {
            fact("Attempt") { Text(item.row.attempt).textStyle(.mono).foregroundStyle(theme.text).textSelection(.enabled) }
            fact("OS evidence") {
                if item.processGone {
                    EvidencePill(.passed, label: "Every process proven gone")
                } else {
                    EvidencePill(.unknown, label: item.row.structured == nil ? "Not reported" : "Not proven gone")
                }
            }
            fact("Blockers") {
                VStack(alignment: .leading, spacing: 3) {
                    if let structured = item.row.structured {
                        if structured.blockers.isEmpty { value("None reported") }
                        ForEach(structured.blockers, id: \.self) { value($0) }
                        if !structured.blockersComplete {
                            Text("The core shortened this list.").textStyle(.meta).foregroundStyle(theme.text3)
                        }
                    } else {
                        Text("Not reported by this core.").textStyle(.control).foregroundStyle(theme.text3)
                    }
                }
            }
            fact("Assessment") { value(item.row.assessment) }
            fact("Findings") {
                Text(item.row.findings + (item.row.detailComplete ? "" : " … (partial)"))
                    .textStyle(.mono).foregroundStyle(theme.text).textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private func fact(_ label: String, @ViewBuilder _ content: () -> some View) -> some View {
        GridRow {
            Text(label).font(.system(size: 12.5)).foregroundStyle(theme.text3).frame(width: 130, alignment: .leading)
            content().frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    private func value(_ text: String) -> some View {
        Text(text).font(.system(size: 12.5)).foregroundStyle(theme.text).textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
    }

    private struct ActionRow: Identifiable {
        let choice: RecoveryChoice
        var id: String { choice.rawValue }
    }

    private func action(_ item: HeldItem, _ choice: RecoveryChoice) -> some View {
        let offered = item.offers(choice)
        return HStack(alignment: .top, spacing: 12) {
            Button(choice.buttonTitle + (offered && choice.confirms ? "…" : "")) {
                if choice.confirms {
                    confirmation = Confirmation(item: item, choice: choice)
                } else {
                    actions.resolve(item.id, choice)
                }
            }
            .buttonStyle(.jojo(choice == item.primary ? .primary : .secondary))
            .disabled(!offered || !state.canAct)
            .help(item.sentence(choice))
            .frame(width: 180, alignment: .leading)
            Text(item.sentence(choice)).textStyle(.body).foregroundStyle(offered ? theme.text2 : theme.text3)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.top, 5)
        }
        .padding(.vertical, 10)
        .padding(.horizontal, 14)
    }

    // MARK: Admission and inspection

    private var aside: some View {
        VStack(alignment: .leading, spacing: 16) {
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Execution admission")
                Card(.panel) { admission }
            }
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Last inspection")
                Card(.panelList) { Rows(counts) { count($0) } }
                Footnote("Attempts examined, worktree, snapshot, external effect and approval counts are not reported yet.")
            }
            Footnote("Recovery inspection never changes anything. Only the actions you choose do, and the core checks eligibility again.")
        }
    }

    private var admission: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                switch state.admissionState {
                case .open: OperationalPill(.completed, label: "Open", standalone: true)
                case .closed: OperationalPill(.recoveryRequired, label: "Closed", standalone: true)
                case .unknown: EvidencePill(.unknown)
                }
                if let epoch = state.epoch {
                    Text("epoch \(epoch)").textStyle(.mono).foregroundStyle(theme.text2)
                }
            }
            Text(admissionSentence).textStyle(.body).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
            switch state.admissionState {
            case .closed:
                Button("Open Review") { reviewing = true }.buttonStyle(.jojo(.primary, compact: true))
            case .open:
                Button {} label: { Label("Pause Admission", systemImage: "pause.fill") }
                    .buttonStyle(.jojo(.secondary, compact: true))
                    .disabled(true)
                    .help("The core has no pause command yet.")
                DisabledReason("The core has no pause command yet.")
            case .unknown:
                EmptyView()
            }
        }
    }

    private var admissionSentence: String {
        switch state.admissionState {
        case .open: return "Held items stay held. Who started the session, and when, is not reported yet."
        case .closed: return "No new work starts until you review the inspection and start a new session."
        case .unknown:
            return state.link == .connected ? "The core has not reported admission yet." : "Not known while the core is not connected."
        }
    }

    private struct Count: Identifiable {
        let label: String
        let value: String
        var attention = false
        var id: String { label }
    }

    private var counts: [Count] {
        let items = state.items
        let reported = items.filter { $0.row.structured != nil }
        let blocked = reported.filter { !($0.row.structured?.blockers.isEmpty ?? true) }.count
        return [
            Count(label: "Held attempts", value: "\(items.count)"),
            Count(label: "Processes proven gone", value: "\(items.filter(\.processGone).count) / \(items.count)"),
            Count(label: "With blockers", value: "\(blocked)", attention: blocked > 0),
            Count(label: "Offering more than Hold", value: "\(reported.filter { $0.primary != nil }.count)"),
            Count(label: "Inspection", value: state.digestShown),
        ]
    }

    private func count(_ count: Count) -> some View {
        HStack(spacing: 10) {
            Text(count.label).textStyle(.body).foregroundStyle(theme.text)
            Spacer(minLength: 0)
            Text(count.value).textStyle(.mono).foregroundStyle(count.attention ? theme.warn : theme.text)
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 14)
    }

    // MARK: Launch review

    private var review: some View {
        Scrolling {
            VStack(alignment: .leading, spacing: 16) {
                Button { reviewing = false } label: { Label("Recovery Center", systemImage: "chevron.left") }
                    .buttonStyle(.jojo(.ghost, compact: true))
                banners(openReview: false)
                Card(.panel) { strip }
                VStack(alignment: .leading, spacing: 8) {
                    SectionHeading("Review each finding · \(reviewed.intersection(state.rows.map(\.attempt)).count) of \(state.rows.count)")
                    if state.items.isEmpty {
                        Card(.panel) {
                            Text(state.loading ? "Loading the recovery inspection…" : "The inspection found nothing on hold.")
                                .textStyle(.body).foregroundStyle(theme.text2)
                        }
                    } else {
                        Card(.panelList) { Rows(state.items) { finding($0) } }
                    }
                }
                startBar
            }
            .padding(EdgeInsets(top: 18, leading: 22, bottom: 26, trailing: 22))
            .frame(maxWidth: 980, alignment: .topLeading)
            .frame(maxWidth: .infinity)
        }
    }

    private var strip: some View {
        HStack(alignment: .top, spacing: 30) {
            field("Inspection") { Text(state.digestShown).textStyle(.mono).foregroundStyle(theme.text) }
            field("Findings") { Text("\(state.rows.count)").textStyle(.control).foregroundStyle(theme.text) }
            field("Collected") { Text("Not reported yet").textStyle(.control).foregroundStyle(theme.text3) }
            field("Freshness") { Text("Not reported yet").textStyle(.control).foregroundStyle(theme.text3) }
            Spacer(minLength: 0)
            reinspect(help: "Collect a new read-only inspection")
        }
    }

    private func field(_ label: String, @ViewBuilder _ content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(label).textStyle(.meta).foregroundStyle(theme.text3)
            content()
        }
    }

    private func finding(_ item: HeldItem) -> some View {
        let isOn = reviewed.contains(item.id)
        return Button {
            var attempts = reviewed
            if isOn { attempts.remove(item.id) } else { attempts.insert(item.id) }
            marks = (state.digest, attempts)
        } label: {
            ListRow(item.findingTitle, detail: item.findingSentence, wraps: true, inset: 14) {
                CheckBox(isOn: isOn).padding(.top, 2)
            } trailing: {
                OperationalPill(.recoveryRequired, label: item.pillLabel, standalone: true)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Reviewed: \(item.findingTitle)")
        .accessibilityValue(isOn ? "checked" : "not checked")
    }

    private var startBar: some View {
        let blocked = state.startBlocked(reviewed: reviewed)
        return HStack(alignment: .center, spacing: 12) {
            Text("Starting a session reopens admission for new work. It does not continue, retry or settle any held item. Live and unknown processes stay held.")
                .textStyle(.body).foregroundStyle(theme.text2)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: 560, alignment: .leading)
            Spacer(minLength: 0)
            VStack(alignment: .trailing, spacing: 6) {
                Button { actions.start() } label: { Label("Start New Session", systemImage: "play.fill") }
                    .buttonStyle(.jojo(.primary))
                    .disabled(blocked != nil)
                    .help(blocked ?? "Asks the core to open admission.")
                if let blocked { DisabledReason(blocked) }
            }
        }
        .padding(.vertical, 12)
        .padding(.horizontal, 14)
        .background(theme.fill2, in: RoundedRectangle(cornerRadius: 10))
    }
}

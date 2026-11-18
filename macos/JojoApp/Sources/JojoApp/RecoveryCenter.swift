import JojoCore
import SwiftUI

// Recovery Center and launch review: docs/design/screens/RECOVERY_CENTER.md, rebuilt from
// `RecoveryCenterMac.dc.html` (step 6). Held items are prior attempts, external actions
// whose outcome is unknown (M5: never sent again until the human inspects the remote and
// decides) and quarantined claims; each fact comes from the core's inspection (protocol
// 1.11 `RecoveryDetail`). A part without a source says so.

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
    /// M5 (1.10): issued external actions whose outcome is not known.
    var external: [ExternalAction] = []
    /// 1.11: the inspection's counts and facts; nil before it is read.
    var detail: RecoveryDetail? = nil
    /// False when the core does not list `RecoveryDetail` (older than 1.11).
    var reportsDetail = true
    var now = Date()

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

    /// The detail answers for the inspection on screen only.
    var currentDetail: RecoveryDetail? { detail?.digest == digest ? detail : nil }

    var items: [HeldItem] { rows.map { HeldItem(row: $0, subject: currentDetail?.subject($0.attempt)) } }

    /// Every held item in the board's order: external side effects, prior attempts, then
    /// quarantined claims of holders that are not subjects.
    var held: [Held] {
        external.map { Held(kind: .external($0)) } + items.map { Held(kind: .attempt($0)) }
            + (currentDetail?.claims ?? []).map { Held(kind: .claim($0)) }
    }

    /// Resolutions of this session that moved an attempt on (a Hold keeps it held).
    var settled: [RecoveryDetail.Settled] { currentDetail?.settled.filter { $0.choice != "HOLD" } ?? [] }

    /// No inspection has arrived yet on a connection that can still deliver one.
    var loading: Bool { digest.isEmpty && (link == .connected || link == .starting) }

    /// A recovery command can be sent: connected, an inspection on screen, nothing pending.
    var canAct: Bool { link == .connected && !digest.isEmpty && !commandPending }

    /// The inspection digest as shown: its first twelve characters.
    var digestShown: String { digest.isEmpty ? "None" : String(digest.prefix(12)) }

    /// "inspected 09:04 · 38 min ago"; nil when the time is not reported.
    var inspectedLine: String? {
        guard let at = currentDetail?.inspectedAt, let time = boardTime(at, now: now) else { return nil }
        let minutes = max(0, Int(now.timeIntervalSince(at) / 60))
        return "inspected \(time) · " + (minutes == 0 ? "just now" : "\(minutes) min ago")
    }

    /// An inspection older than five minutes is stale: reinspect before acting.
    var fresh: Bool? {
        guard let at = currentDetail?.inspectedAt else { return nil }
        return now.timeIntervalSince(at) < 300
    }

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

/// One held item of the leading list.
struct Held: Identifiable, Equatable {
    enum Kind: Equatable {
        case external(ExternalAction)
        case attempt(HeldItem)
        case claim(RecoveryDetail.Claim)
    }

    let kind: Kind

    var id: String {
        switch kind {
        case .external(let a): return "external:" + a.id
        case .attempt(let item): return item.id
        case .claim(let c): return "claim:" + c.id
        }
    }

    var kindTitle: String {
        switch kind {
        case .external: return "External side effect"
        case .attempt: return "Prior attempt"
        case .claim: return "Quarantined claim"
        }
    }

    var title: String {
        switch kind {
        case .external(let a): return ExternalWords.title(a)
        case .attempt(let item): return item.title
        case .claim(let c): return c.resource
        }
    }

    var place: String {
        switch kind {
        case .external(let a): return [a.featureName, a.remote].compactMap { $0 }.joined(separator: " · ")
        case .attempt(let item): return item.place
        case .claim(let c): return c.holder.map { "Held by \($0)" } ?? ""
        }
    }

    var pill: (String, Tone) {
        switch kind {
        case .external: return ("Outcome unknown", .warn)
        case .attempt(let item): return item.pill
        case .claim: return ("Quarantined", .review)
        }
    }
}

/// One held prior attempt, with the facts of the 1.11 inspection when they are reported.
struct HeldItem: Identifiable, Equatable {
    let row: RecoveryRow
    var subject: RecoveryDetail.Subject? = nil

    var id: String { row.attempt }
    var title: String {
        guard let label = subject?.label else { return "Attempt \(row.attempt.prefix(8))" }
        return label.replacingOccurrences(of: "attempt", with: "Attempt")
    }

    /// "Sample · FT-3 name · Step title" from what the core names.
    var place: String {
        guard let s = subject else { return "Names not reported" }
        return [s.projectName, s.featureName, s.title].compactMap { $0 }.joined(separator: " · ")
    }

    /// True only when the helper states that every process is proven gone.
    var processGone: Bool { row.structured?.processAbsent == true }
    var pillLabel: String { processGone ? "Confirmed dead" : "Unknown" }

    /// A recorded process still alive under no supervision, as the inspection found it.
    var alive: Bool { subject?.processes.contains { $0.state == "Alive" } == true }

    var pill: (String, Tone) {
        if alive { return ("Live · unsupervised", .danger) }
        return processGone ? ("Confirmed dead", .neutral) : ("Unknown", .warn)
    }

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
        if alive { return "Unavailable: the process is alive but unsupervised. Actions open only after the OS reports its exit." }
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

    /// The board's "Inspected facts" rows from the 1.11 facts, one by one.
    var facts: [Definition] {
        var out = [Definition(key: "Attempt", value: row.attempt, mono: true)]
        guard let s = subject else {
            out.append(Definition(key: "OS evidence", value: processGone ? "Every process proven gone" : "Not proven gone"))
            out.append(Definition(key: "Findings", value: row.findings + (row.detailComplete ? "" : " … (partial)"), mono: true))
            return out
        }
        out.append(Definition(key: "Stage", value: HeldItem.stageWords(s.stage)))
        for p in s.processes {
            let (key, name) = HeldItem.processName(p.label)
            out.append(Definition(key: key, value: "\(name) · \(HeldItem.findingWords(p.state))" + (p.detail.isEmpty ? "" : " · \(p.detail)"),
                                  mono: true))
        }
        out.append(Definition(key: "OS evidence", value: processGone ? "No process with the recorded identity" : "Original process not confirmed gone"))
        out.append(Definition(key: "Worktree", value: HeldItem.worktreeWords(s.worktree)))
        out.append(Definition(key: "Snapshot", value: HeldItem.snapshotWords(s.snapshot.state, digest: s.snapshotDigest)))
        out.append(Definition(key: "Evidence", value: s.evidenceGaps.isEmpty ? "Logs complete to their last durable record" : s.evidenceGaps.joined(separator: "; ")))
        out.append(Definition(key: "Session", value: HeldItem.sessionWords(s.session)))
        if !s.claims.isEmpty {
            out.append(Definition(key: "Claims", value: s.claims.map { "\($0.resource) · \(($0.state ?? "").lowercased())" }.joined(separator: "\n"), mono: true))
        }
        if !s.staleApprovals.isEmpty {
            out.append(Definition(key: "Stale approvals", value: s.staleApprovals.joined(separator: "\n")))
        }
        if !s.unknownEffects.isEmpty {
            out.append(Definition(key: "Unknown effects", value: s.unknownEffects.joined(separator: "\n"), mono: true))
        }
        if !s.blockers.isEmpty {
            out.append(Definition(key: "Blockers", value: s.blockers.joined(separator: "\n")))
        }
        return out
    }

    /// The core labels a recorded identity "pid:N start:S" and its group "group:N".
    static func processName(_ label: String) -> (String, String) {
        if label.hasPrefix("group:") { return ("PGID", String(label.dropFirst(6))) }
        if label.hasPrefix("pid:") { return ("PID", label.dropFirst(4).split(separator: " ").first.map(String.init) ?? "") }
        return ("PID", label)
    }

    static func stageWords(_ stage: String) -> String {
        switch stage {
        case "NotIssued": return "Reserved; the launch was never issued"
        case "IssuedWithoutIdentity": return "Launch issued; no process identity recorded"
        case "Spawned": return "Running when supervision was lost"
        case "Concluded": return "Supervision concluded the process tree gone"
        case "Sealed": return "Result sealed"
        default: return stage
        }
    }

    static func findingWords(_ finding: String) -> String {
        switch finding {
        case "Absent": return "absent"
        case "Alive": return "alive · start time matches"
        case "Reused": return "reused by another program"
        default: return "unknown"
        }
    }

    static func worktreeWords(_ fact: RecoveryDetail.Fact) -> String {
        switch fact.state {
        case "Intact": return "Present · matches its record"
        case "NotCreated": return "Not created"
        case "Changed": return "Changed · \(fact.detail)"
        default: return fact.detail.isEmpty ? "Not readable" : fact.detail
        }
    }

    static func snapshotWords(_ state: String, digest: String?) -> String {
        switch state {
        case "Matches": return "sha256 verified" + (digest.map { " · \($0.prefix(12))" } ?? "")
        case "Mismatch": return "Changed after snapshot"
        default: return "No reviewed snapshot"
        }
    }

    static func sessionWords(_ fact: RecoveryDetail.Fact) -> String {
        switch fact.state {
        case "Recorded": return "\(fact.detail.isEmpty ? "Provider" : fact.detail) session recorded"
        case "NotRecorded": return "No session recorded"
        default: return fact.detail.isEmpty ? "Not resumable" : fact.detail
        }
    }

    /// "Continued fresh", "Continued with history", "Output validated".
    static func settledWords(_ choice: String) -> String {
        switch choice {
        case "VALIDATE_OUTPUT": return "Output validated"
        case "CONTINUE_WITH_HISTORY": return "Continued with history"
        case "CONTINUE_FRESH_CONTEXT": return "Continued fresh"
        default: return "Held"
        }
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
    /// M5: reads the remote again for an action; sends nothing.
    var inspectExternal: (String) -> Void = { _ in }
    /// M5: marks an inspected action as not performed.
    var resolveExternal: (String) -> Void = { _ in }
}

/// The Recovery Center destination, bound to the app model.
struct RecoveryCenterScreen: View {
    @ObservedObject var model: AppModel
    /// The external actions (1.10) and the inspection detail (1.11) are read only while
    /// this is the shown screen.
    var visible = false
    var reviewing: Binding<Bool>? = nil

    var state: RecoveryCenterState {
        let workspace = model.workspace
        return RecoveryCenterState(connection: model.client.connection, admission: workspace.admission,
                                   rows: workspace.recovery, digest: workspace.recoveryDigest,
                                   commandPending: workspace.commandPending,
                                   external: workspace.external?.unresolved ?? [],
                                   detail: workspace.recoveryDetail,
                                   reportsDetail: model.client.hello?.queries.contains("RecoveryDetail") ?? true)
    }

    var body: some View {
        RecoveryCenterView(
            state: state,
            actions: RecoveryActions(inspect: { model.workspace.inspectRecovery() },
                                     resolve: { model.workspace.resolveRecovery($0, $1) },
                                     start: { model.workspace.startAfterRecovery() },
                                     inspectExternal: { model.workspace.inspectExternal($0) },
                                     resolveExternal: { model.workspace.resolveExternal($0) }),
            review: reviewing)
            .onAppear { if visible { load() } }
            .onChange(of: visible) { if visible { load() } }
            .onChange(of: model.workspace.snapshotSeq) { if visible { load() } }
            .onChange(of: model.workspace.recoveryDigest) { if visible { model.workspace.loadRecoveryDetail() } }
    }

    private func load() {
        model.workspace.loadExternalActions()
        if state.reportsDetail { model.workspace.loadRecoveryDetail() }
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
    /// The launch review: the shell's binding when there is one, else this view's own.
    private let review: Binding<Bool>?
    @State private var ownReviewing: Bool
    @State private var selected: String?
    /// Findings marked reviewed, and the inspection they were marked on.
    @State private var marks: (digest: String, attempts: Set<String>) = ("", [])
    @State private var confirmation: Confirmation?
    /// M5: the external action "Mark Not Performed" asks about.
    @State private var notPerformed: ExternalAction?

    init(state: RecoveryCenterState, actions: RecoveryActions = RecoveryActions(), reviewing: Bool = false,
         review: Binding<Bool>? = nil) {
        self.state = state
        self.actions = actions
        self.review = review
        _ownReviewing = State(initialValue: reviewing)
    }

    private var reviewing: Bool {
        get { review?.wrappedValue ?? ownReviewing }
        nonmutating set { if let review { review.wrappedValue = newValue } else { ownReviewing = newValue } }
    }

    /// A new inspection clears the marks: they were made on other facts.
    private var reviewed: Set<String> { marks.digest == state.digest ? marks.attempts : [] }

    private var current: Held? {
        state.held.first { $0.id == selected } ?? state.held.first
    }

    var body: some View {
        Group {
            if reviewing, state.admissionState == .closed { launchReview } else { center }
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
        .alert("Mark this \(notPerformed?.kind == .Push ? "push" : "pull request") as not performed?",
               isPresented: Binding(get: { notPerformed != nil }, set: { if !$0 { notPerformed = nil } })) {
            if let action = notPerformed {
                Button("Mark Not Performed", role: .destructive) { actions.resolveExternal(action.id) }
                Button("Cancel", role: .cancel) {}
            }
        } message: {
            Text("Records that it did not happen, on the strength of your inspection of the remote. A new request can follow; it needs its own authorization.")
        }
    }

    // MARK: Center

    private var center: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 10) {
                CoreBanners(connection: state.connection,
                            missing: state.reportsDetail ? nil : ("RecoveryDetail", "the inspection's counts and facts"),
                            version: "1.11")
                admissionBanner(openReview: true)
            }
            .padding(.horizontal, 10)
            FeatureColumns(leadingWidth: 250, trailingWidth: 300, wide: 1060, medium: 780) {
                heldList
            } main: {
                detail
            } trailing: {
                inspector
            }
        }
        .padding(.top, 6)
    }

    @ViewBuilder
    private func admissionBanner(openReview: Bool) -> some View {
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
        VStack(alignment: .leading, spacing: 2) {
            GroupLabel("Held · \(state.held.count)").padding(.horizontal, 6)
            if state.held.isEmpty {
                NotReported(state.loading ? "Reading the inspection…" : "Nothing is on hold.").padding(10)
            }
            ForEach(state.held) { h in
                SelectableRow(selected: h.id == current?.id, action: { selected = h.id }) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text(h.kindTitle).textStyle(.label).foregroundStyle(theme.text3)
                        HStack(spacing: 6) {
                            Text(h.title).textStyle(.bodyStrong).lineLimit(1).truncationMode(.middle)
                                .frame(maxWidth: .infinity, alignment: .leading)
                            OperationalPill(h.pill.0, tone: h.pill.1)
                        }
                        if !h.place.isEmpty {
                            Text(h.place).textStyle(.secondary).foregroundStyle(theme.text2).lineLimit(1)
                        }
                    }
                }
            }
            GroupLabel("Settled this session").padding(EdgeInsets(top: 16, leading: 6, bottom: 2, trailing: 6))
            if state.currentDetail == nil {
                NotReported(state.reportsDetail ? "Reading…" : "Not reported by this core.").padding(10)
            } else if state.settled.isEmpty {
                NotReported("Nothing was settled in this session.").padding(10)
            }
            ForEach(state.settled) { s in
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(s.label.map { $0.replacingOccurrences(of: "attempt", with: "Attempt") } ?? "Attempt \(s.attempt.prefix(8))")
                            .textStyle(.bodyStrong).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                        if let time = boardTime(s.at, now: state.now) {
                            Text(time).textStyle(.secondary).foregroundStyle(theme.text3)
                        }
                    }
                    Text(HeldItem.settledWords(s.choice)).textStyle(.secondary).foregroundStyle(theme.text2)
                }
                .padding(EdgeInsets(top: 6, leading: 10, bottom: 6, trailing: 10))
            }
        }
    }

    // MARK: Detail

    @ViewBuilder
    private var detail: some View {
        if let held = current {
            VStack(alignment: .leading, spacing: 16) {
                header(held)
                freshness
                switch held.kind {
                case .attempt(let item): attemptDetail(item)
                case .external(let a): externalDetail(a)
                case .claim(let c): claimDetail(c)
                }
            }
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

    private func header(_ held: Held) -> some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text(([held.kindTitle] + (held.place.isEmpty ? [] : [held.place])).joined(separator: " · "))
                    .textStyle(.secondary).foregroundStyle(theme.text2).lineLimit(2)
                Text(held.title).font(.system(size: 22, weight: .bold)).fixedSize(horizontal: false, vertical: true)
                    .textSelection(.enabled)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            OperationalPill(held.pill.0, tone: held.pill.1)
        }
    }

    /// The board's freshness strip: Fresh or Stale, when it was inspected, Reinspect.
    private var freshness: some View {
        HStack(spacing: 10) {
            switch state.fresh {
            case .some(true): OperationalPill("Fresh", tone: .ok)
            case .some(false): OperationalPill("Stale", tone: .warn)
            case .none: OperationalPill("Not reported", tone: .neutral)
            }
            Text(state.inspectedLine.map { $0 + (state.fresh == false ? " · reinspect before acting" : "") }
                 ?? "The time of the inspection is not reported by this core.")
                .textStyle(.secondary).foregroundStyle(theme.text2).lineLimit(2)
                .frame(maxWidth: .infinity, alignment: .leading)
            reinspect(help: "Read-only. Collects fresh facts for every held item.")
        }
        .padding(EdgeInsets(top: 8, leading: 12, bottom: 8, trailing: 8))
        .cardElevation(radius: 12)
    }

    private func reinspect(help: String) -> some View {
        Button { actions.inspect() } label: { Label("Reinspect", systemImage: "arrow.clockwise") }
            .buttonStyle(.jojo(.secondary, compact: true))
            .disabled(state.link != .connected)
            .help(help)
    }

    @ViewBuilder
    private func attemptDetail(_ item: HeldItem) -> some View {
        LabeledSection(title: "Inspected facts · read-only") {
            DefinitionCard(rows: item.facts, keyWidth: 120)
            if item.subject == nil {
                NotReported(state.reportsDetail
                            ? "The worktree, snapshot, evidence, session and claims are read with the inspection's detail."
                            : "This core reports no separate facts for the worktree, snapshot, evidence, session and claims.")
            }
        }
        LabeledSection(title: "Recovery actions") {
            actionCard(RecoveryChoice.allCases.map { choice in AnyView(action(item, choice)) })
        }
        Footnote(item.footnote)
    }

    @ViewBuilder
    private func externalDetail(_ a: ExternalAction) -> some View {
        let push = a.kind == .Push
        LabeledSection(title: "Inspected facts · read-only") {
            DefinitionCard(rows: [
                Definition(key: "Operation", value: push ? "git push \(a.remote) \(a.reference)"
                                                         : "gh pr create --draft \(a.branch) → \(a.base ?? "default branch")", mono: true),
                Definition(key: "Idempotency key", value: a.id, mono: true),
                Definition(key: "Commit", value: a.commit, mono: true),
                Definition(key: "Remote", value: "\(a.remote) · \(a.url)", mono: true),
                Definition(key: "Last seen", value: a.observations.last.map { "\(ExternalWords.observer($0)): \($0.detail)" }
                           ?? "No exit recorded: the helper stopped before the result came back."),
            ], keyWidth: 120)
        }
        LabeledSection(title: "How this resolves") {
            Text("Check Remote reads \(push ? "the remote branch \(a.branch)" : "the remote's pull requests for \(a.branch)"). If it shows the \(push ? "commit" : "pull request"), the \(push ? "push" : "pull request") is recorded as performed with that evidence; if not, you can mark it not performed. A new \(push ? "push" : "pull request") always needs a new authorization.")
                .textStyle(.secondary).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
                .padding(EdgeInsets(top: 10, leading: 12, bottom: 10, trailing: 12))
                .frame(maxWidth: .infinity, alignment: .leading)
                .cardElevation(radius: 12)
        }
        LabeledSection(title: "External actions · \(state.external.count) with unknown outcome") {
            actionCard([
                AnyView(actionRow(title: "Check Remote", primary: true, enabled: externalActable(a),
                                  sentence: "Reads the remote and records what it shows. Sends nothing.") { actions.inspectExternal(a.id) }),
                AnyView(actionRow(title: "Mark Not Performed…", primary: false,
                                  enabled: externalActable(a) && a.observations.contains { $0.observer.hasPrefix("git:ls-remote") || $0.observer.hasPrefix("gh:pr-list") },
                                  sentence: "Available after the remote was checked. Records that it did not happen.") { notPerformed = a }),
            ])
        }
        Footnote("Jojo never sends it again by itself. Validate Output, Continue with History and Continue Fresh are unavailable while the external outcome is unknown.")
    }

    @ViewBuilder
    private func claimDetail(_ c: RecoveryDetail.Claim) -> some View {
        LabeledSection(title: "Inspected facts · read-only") {
            DefinitionCard(rows: [
                Definition(key: "Paths", value: c.resource, mono: true),
                Definition(key: "Held by", value: c.holder ?? "Not reported"),
                Definition(key: "Mode", value: c.mode.lowercased()),
                Definition(key: "Blocks", value: c.mode == "EXCLUSIVE" ? "Any task using these paths" : "Any task writing these paths"),
            ], keyWidth: 120)
        }
        Footnote("Only Hold is available: the claim's owner is unconfirmed. It resolves when its attempt is settled.")
    }

    private func externalActable(_ a: ExternalAction) -> Bool {
        state.link == .connected && !state.commandPending && a.state == .Unknown
    }

    /// The board's action list: a card of rows 0.5 `sep` apart.
    private func actionCard(_ rows: [AnyView]) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(rows.enumerated()), id: \.offset) { index, row in
                row.overlay(alignment: .top) { if index > 0 { Rectangle().fill(theme.sep).frame(height: 0.5) } }
            }
        }
        .cardElevation(radius: 12)
    }

    private func actionRow(title: String, primary: Bool, enabled: Bool, sentence: String,
                           _ perform: @escaping () -> Void) -> some View {
        HStack(alignment: .top, spacing: 12) {
            Button(title, action: perform)
                .buttonStyle(.jojo(primary ? .primary : .secondary, compact: true))
                .disabled(!enabled)
                .help(sentence)
                .frame(width: 180, alignment: .leading)
            Text(sentence).textStyle(.secondary).foregroundStyle(enabled ? theme.text2 : theme.text3)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.top, 3)
        }
        .padding(EdgeInsets(top: 9, leading: 12, bottom: 9, trailing: 12))
    }

    private func action(_ item: HeldItem, _ choice: RecoveryChoice) -> some View {
        let offered = item.offers(choice)
        return actionRow(title: choice.buttonTitle + (offered && choice.confirms ? "…" : ""),
                         primary: choice == item.primary, enabled: offered && state.canAct,
                         sentence: item.sentence(choice)) {
            if choice.confirms {
                confirmation = Confirmation(item: item, choice: choice)
            } else {
                actions.resolve(item.id, choice)
            }
        }
    }

    // MARK: Admission and inspection

    private var inspector: some View {
        GlassInspector {
            LabeledSection(title: "Execution admission") { admission }
            LabeledSection(title: "Last inspection") {
                if let d = state.currentDetail {
                    DefinitionCard(rows: counts(d), keyWidth: 150)
                } else {
                    NotReported(state.reportsDetail ? (state.digest.isEmpty ? "No inspection yet." : "Reading…")
                                                    : "Not reported by this core (protocol 1.11).")
                }
            }
            Footnote("Recovery inspection never changes anything. Only the actions you choose do, and the core checks eligibility again.")
        }
    }

    private func counts(_ d: RecoveryDetail) -> [Definition] {
        let c = d.counts
        return [
            Definition(key: "Attempts examined", value: "\(c.examined)"),
            Definition(key: "Worktrees intact", value: "\(c.worktreesIntact) / \(c.worktrees)"),
            Definition(key: "Snapshot mismatches", value: "\(c.snapshotMismatches)"),
            Definition(key: "Unknown external effects", value: "\(c.unknownEffects)"),
            Definition(key: "Stale approvals", value: "\(c.staleApprovals)"),
            Definition(key: "Evidence gaps", value: "\(c.evidenceGaps)"),
            Definition(key: "Quarantined claims", value: "\(c.claims)"),
            Definition(key: "Inspected", value: d.inspectedAt.flatMap { boardTime($0, now: state.now) } ?? "Not reported"),
            Definition(key: "Inspection", value: String(state.digestShown.prefix(8)), mono: true),
        ]
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
                    Text("Session \(epoch)").textStyle(.mono).foregroundStyle(theme.text2)
                }
            }
            Text(admissionSentence).textStyle(.secondary).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
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
        case .open: return "Held items stay held. Who started the session, and when, is not reported."
        case .closed: return "No new work starts until you review the inspection and start a new session."
        case .unknown:
            return state.link == .connected ? "The core has not reported admission yet." : "Not known while the core is not connected."
        }
    }

    // MARK: Launch review

    private var launchReview: some View {
        Scrolling {
            VStack(alignment: .leading, spacing: 16) {
                Button { reviewing = false } label: { Label("Recovery Center", systemImage: "chevron.left") }
                    .buttonStyle(.jojo(.ghost, compact: true))
                CoreBanners(connection: state.connection)
                admissionBanner(openReview: false)
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
            field("Collected") {
                Text(state.currentDetail?.inspectedAt.flatMap { boardTime($0, now: state.now) } ?? "Not reported")
                    .textStyle(.control).foregroundStyle(state.currentDetail == nil ? theme.text3 : theme.text)
            }
            field("Freshness") {
                switch state.fresh {
                case .some(true): OperationalPill("Fresh", tone: .ok)
                case .some(false): OperationalPill("Stale", tone: .warn)
                case .none: Text("Not reported").textStyle(.control).foregroundStyle(theme.text3)
                }
            }
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

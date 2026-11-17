import JojoCore
import SwiftUI

// Dashboard (docs/design/screens/DASHBOARD.md), the canvas board `DashboardMac.dc.html`: what
// needs the human, what runs, what is blocked, each feature's stages, the latest host
// validations and the recent history. Every value is the core's: the features and their
// stages (`Features`, protocol 1.7), the attempts (`Attempts`), what waits (`Documents`,
// `Recovery`), the executables (`Executables`) and the event sentences (`Events`). Read
// while the Dashboard is shown and with each new snapshot.

/// The header's time range: which validations and history entries are listed.
enum DashboardRange: String, CaseIterable, Identifiable, Sendable {
    case today = "Today", week = "Week", all = "All"

    var id: String { rawValue }

    func contains(_ date: Date?, now: Date, calendar: Calendar = .current) -> Bool {
        switch self {
        case .all: return true
        case .today: return date.map { calendar.isDate($0, inSameDayAs: now) } ?? false
        case .week: return date.map { now.timeIntervalSince($0) < 7 * 24 * 3600 } ?? false
        }
    }
}

/// The project table's filter.
enum FeatureFilter: String, CaseIterable, Identifiable, Sendable {
    case active = "Active", needsYou = "Needs you", done = "Done"

    var id: String { rawValue }

    func contains(_ f: FeatureSummary) -> Bool {
        switch self {
        case .active: return !f.done
        case .needsYou: return f.awaiting + f.blocked > 0
        case .done: return f.done
        }
    }
}

/// One row of "Needs you".
struct NeedItem: Identifiable, Equatable {
    enum Target: Equatable {
        case document(String), candidate(String), recovery
    }

    var id: String
    var symbol: String
    var tile: RGBA
    var title: String
    var detail: String
    var pill: String
    var tone: Tone
    var target: Target
}

/// One line of "Blocked".
struct BlockedItem: Identifiable, Equatable {
    var id: String
    var title: String
    var reason: String
    /// A question is answered in Feature Chat; a step is explained on Execution Plan.
    var question: Bool
    var feature: String
}

/// One line of "Recent history".
struct HistoryItem: Identifiable, Equatable {
    var id: Int64
    var text: String
    var at: Date?
    var tone: Tone?
}

/// What the Dashboard shows, as a plain value.
struct DashboardState: Equatable {
    var connection: HelperConnection
    var admission = ""
    var documents: DocumentList?
    var recovery: [RecoveryRow] = []
    var attempts: AttemptList?
    var attemptsLoad = PlanLoad.idle
    var features: FeatureList?
    var featuresLoad = PlanLoad.idle
    var executables: ExecutableList?
    var events: [JournalEvent] = []
    var eventsLoad = JournalLoad.idle
    var range = DashboardRange.today
    var now = Date()

    var connected: Bool {
        if case .ready = connection { return true }
        return false
    }

    var admissionClosed: Bool { connected && !admission.isEmpty && admission != "Open" }

    /// Nothing to show yet: no feature, nothing running, nothing waiting.
    var empty: Bool {
        featuresLoad == .loaded && (features?.features.isEmpty ?? true) && needs.isEmpty && running.isEmpty
    }

    /// "FT-n" of a feature the features answer lists.
    func code(_ feature: String?) -> String? {
        guard let feature else { return nil }
        return features?.features.first { $0.id == feature }?.code
    }

    // MARK: Needs you

    var approvals: [NeedItem] {
        let documents = (self.documents?.waiting ?? []).map { d in
            NeedItem(id: d.id, symbol: Self.symbol(d.kind), tile: Self.tile(d.kind),
                     title: "\(d.title ?? d.kindName) rev \(d.revision)",
                     detail: [d.kindName, code(d.feature), d.title == nil ? d.featureName : nil].compactMap { $0 }
                         .joined(separator: " · "),
                     pill: d.stale ? "Stale" : "Proposed", tone: d.stale ? .warn : .info, target: .document(d.id))
        }
        let candidates = (self.documents?.candidates ?? []).map { c in
            NeedItem(id: c.id, symbol: "checkmark.seal", tile: Palette.teal,
                     title: [c.key, c.title].compactMap { $0 }.joined(separator: " · ").nonEmpty ?? "Candidate",
                     detail: ["Candidate", c.featureName, "Host validation passed"].compactMap { $0 }.joined(separator: " · "),
                     pill: "Needed", tone: .warn, target: .candidate(c.id))
        }
        return documents + candidates
    }

    var recoveries: [NeedItem] {
        recovery.map { r in
            let held = !(r.structured?.blockers.isEmpty ?? true)
            let summary = attempts?[r.attempt]
            let name = summary.map { [$0.key, $0.title].compactMap { $0 }.joined(separator: " · ") }?.nonEmpty
            return NeedItem(id: r.attempt, symbol: held ? "pause.circle" : "arrow.uturn.backward.circle",
                            tile: held ? Palette.red : Palette.purple, title: name ?? "Attempt on hold",
                            detail: ["Recovery", summary?.featureName, r.structured?.blockers.first ?? "Eligible: choose how to continue"]
                                .compactMap { $0 }.joined(separator: " · "),
                            pill: held ? "Held" : "Review", tone: held ? .warn : .review, target: .recovery)
        }
    }

    var needs: [NeedItem] { approvals + recoveries }

    // MARK: Running now

    var running: [AttemptSummary] { (attempts?.attempts ?? []).filter(\.live) }

    var runningSummary: String {
        let parts = running.prefix(3).map { a in
            "\(a.key ?? "Attempt") on \(a.provider.map(providerName) ?? "a provider")"
        }
        return parts.isEmpty ? "Nothing is running" : parts.joined(separator: " · ")
    }

    /// "Crew · crew-api · Claude Code · Attempt 2".
    static func place(_ a: AttemptSummary) -> String {
        [a.projectName, a.repositoryName, a.provider.map(providerName), a.ordinal.map { "Attempt \($0)" }]
            .compactMap { $0 }.joined(separator: " · ")
    }

    /// "12m", "1h 5m", "<1m".
    static func elapsed(_ from: Date?, now: Date) -> String? {
        guard let from else { return nil }
        let minutes = Int(now.timeIntervalSince(from) / 60)
        if minutes < 1 { return "<1m" }
        if minutes < 60 { return "\(minutes)m" }
        return minutes % 60 == 0 ? "\(minutes / 60)h" : "\(minutes / 60)h \(minutes % 60)m"
    }

    // MARK: Blocked

    var blocked: [BlockedItem] {
        (features?.features ?? []).flatMap { f in
            f.blockers.map { b in
                let title = b.kind == "Question"
                    ? "\(f.code) · \(f.name ?? "Feature")"
                    : [b.key, b.title].compactMap { $0 }.joined(separator: " · ").nonEmpty ?? "\(f.code) · step"
                return BlockedItem(id: "\(f.id)-\(b.id)", title: title,
                                   reason: b.kind == "Question" ? "Feature-level question open: \(b.reason)" : b.reason,
                                   question: b.kind == "Question", feature: f.id)
            }
        }
    }

    var blockedCount: Int { Int((features?.features ?? []).reduce(0) { $0 + $1.blocked }) }

    /// "T7, T6 and FT-145".
    var blockedSummary: String {
        let names = blocked.map { $0.title.components(separatedBy: " · ").first ?? $0.title }
        switch names.count {
        case 0: return "Nothing is blocked"
        case 1: return names[0]
        default: return names.dropLast().prefix(3).joined(separator: ", ") + " and " + names.last!
        }
    }

    // MARK: Providers

    var providers: [ProviderExecutable] { executables?.providers ?? [] }
    var available: Int { providers.filter { $0.state == .ready }.count }

    var providersSummary: String {
        guard executables != nil else { return "Not read yet" }
        let missing = providers.filter { $0.state != .ready }.map { "\(providerName($0.provider)) \(Self.availability($0.state).lowercased())" }
        return missing.isEmpty ? "Usage limits are not reported" : missing.joined(separator: " · ")
    }

    func activeRuns(_ provider: String) -> Int { running.filter { $0.provider == provider }.count }

    static func availability(_ state: ProviderExecutable.State) -> String {
        switch state {
        case .ready: return "Available"
        case .checking: return "Checking"
        case .refused: return "Refused"
        case .unconfigured: return "Not found"
        }
    }

    static func availabilityTone(_ state: ProviderExecutable.State) -> Tone {
        switch state {
        case .ready: return .ok
        case .checking: return .neutral
        case .refused: return .warn
        case .unconfigured: return .danger
        }
    }

    // MARK: Verification and history

    var verifications: [AttemptSummary] {
        (attempts?.attempts ?? [])
            .filter { a in a.validation.map { range.contains($0.at ?? a.ended, now: now) } ?? false }
            .sorted { ($0.validation?.at ?? $0.ended ?? .distantPast) > ($1.validation?.at ?? $1.ended ?? .distantPast) }
    }

    static func evidence(_ v: AttemptValidation) -> EvidenceState {
        switch v.state {
        case "Passed": return .passed
        case "Failed": return .failed
        case "NotRun": return .notRun
        default: return .unknown
        }
    }

    static func validationWords(_ v: AttemptValidation) -> String {
        let verdict: String
        switch v.state {
        case "Passed": verdict = "host validation passed"
        case "Failed": verdict = "host validation failed"
        case "NotRun": verdict = "not validated"
        default: verdict = "validation without a verdict"
        }
        return [verdict, v.decision?.lowercased()].compactMap { $0 }.joined(separator: " · ")
    }

    /// Events that say something to a person, newest first, in the time range.
    var history: [HistoryItem] {
        events.reversed()
            .filter { Self.historyTone[$0.type] != nil && range.contains($0.at, now: now) }
            .prefix(6)
            .map { HistoryItem(id: $0.seq, text: $0.summary ?? $0.type, at: $0.at, tone: Self.historyTone[$0.type] ?? nil) }
    }

    /// The event types listed in "Recent history", with the dot each is drawn with; `nil`
    /// is the gray dot.
    static let historyTone: [String: Tone?] = [
        "EngineStarted": nil, "RecoveryStarted": .ok, "ProcessLaunchIssued": .info,
        "AttemptResultSealed": .ok, "AttemptRecoveryRequired": .danger, "ValidationEnded": .warn,
        "CandidateAccepted": .ok, "CandidateRejected": .danger, "HumanDecision": .ok,
        "ArtifactVersionCreated": .info, "ReviewDocumentCreated": .info, "LeadTurnAccepted": .info,
        "LeadTurnFinished": nil, "QuestionAsked": .warn, "QuestionAnswered": .ok, "PlanRunStarted": .info,
        "PlanRunControl": .warn, "PlanStepIntegrated": .ok, "PlanStepHeld": .danger, "PlanStepRefused": .danger,
        "PlanStepRepairDecided": .warn, "StepReviewFinished": .review, "RecoveryResolved": .ok, "ProjectSaved": nil,
    ]

    static func symbol(_ kind: String) -> String {
        switch kind {
        case "Adr": return "building.columns"
        case "ExecutionPlan": return "list.bullet.indent"
        case "ValidationPlan": return "checklist"
        default: return "doc.text"
        }
    }

    /// The tile colors of COMPONENTS.md, section 5.3, by document kind.
    static func tile(_ kind: String) -> RGBA {
        switch kind {
        case "Adr": return Palette.indigo
        case "ExecutionPlan", "ValidationPlan": return Palette.blue
        default: return Palette.orange
        }
    }

    static func loadFailure(_ reason: String) -> String {
        if reason.hasPrefix("UnknownType") { return "This core does not report features (protocol 1.7)." }
        if reason.hasPrefix("malformed") { return "The core's answer was not one this app can read." }
        return "The core did not answer the read."
    }
}

extension String {
    var nonEmpty: String? { isEmpty ? nil : self }
}

/// "1 approval", "2 approvals".
func counted(_ n: Int, _ noun: String) -> String {
    "\(n) \(noun)\(n == 1 ? "" : "s")"
}

/// What the Dashboard's links and rows do.
struct DashboardActions {
    var open: (Screen) -> Void = { _ in }
    var openDocument: (String) -> Void = { _ in }
    var openCandidate: (String) -> Void = { _ in }
    var answer: (String) -> Void = { _ in }
    /// Opens a feature's chat.
    var openFeature: (String) -> Void = { _ in }
    var newFeature: () -> Void = {}
}

/// The Dashboard destination, bound to the app model.
struct DashboardScreen: View {
    @ObservedObject var model: AppModel
    let visible: Bool
    let range: DashboardRange
    let open: (Screen) -> Void

    var body: some View {
        let workspace = model.workspace
        DashboardView(
            state: DashboardState(connection: model.client.connection, admission: workspace.admission,
                                  documents: workspace.documents, recovery: workspace.recovery,
                                  attempts: workspace.attempts, attemptsLoad: workspace.attemptsLoad,
                                  features: workspace.features, featuresLoad: workspace.featuresLoad,
                                  executables: workspace.executables, events: workspace.events,
                                  eventsLoad: workspace.eventsLoad, range: range),
            actions: DashboardActions(
                open: open,
                openDocument: { workspace.loadReview($0); open(.approvalInbox) },
                openCandidate: { workspace.loadCandidate($0); open(.verification) },
                answer: { workspace.openChat($0); open(.featureChat) },
                openFeature: { workspace.openChat($0); open(.featureChat) },
                newFeature: { workspace.openChat(nil); open(.featureChat) }))
            .onChange(of: visible) { read() }
            .onChange(of: workspace.snapshotSeq) { if visible { read() } }
            .onAppear { read() }
    }

    /// Reads what the Dashboard shows, only while it is the shown destination.
    private func read() {
        let workspace = model.workspace
        workspace.dashboardShown = visible
        guard visible else { return }
        workspace.loadFeatures()
        workspace.loadAttempts()
        workspace.loadExecutables()
        workspace.loadEvents()
    }
}

/// The header's primary action on the Dashboard: New Feature (⌘N, only while shown).
struct NewFeatureButton: View {
    let newFeature: () -> Void

    var body: some View {
        Button(action: newFeature) {
            HStack(spacing: 6) {
                Image(systemName: "plus").font(.system(size: 13, weight: .semibold))
                Text("New Feature")
                ShortcutHint("⌘N")
            }
        }
        .buttonStyle(.jojo(.primary))
        .keyboardShortcut("n", modifiers: .command)
    }
}

struct DashboardView: View {
    @Environment(\.theme) private var theme
    let state: DashboardState
    var actions = DashboardActions()
    @State private var filter = FeatureFilter.active

    var body: some View {
        GeometryReader { geometry in
            let width = geometry.size.width - 32
            Scrolling {
                VStack(alignment: .leading, spacing: 20) {
                    banners
                    if state.empty {
                        emptyState
                    } else {
                        summary(width)
                        main(width)
                        projects(width)
                        lower(width)
                    }
                }
                .padding(EdgeInsets(top: 6, leading: 14, bottom: 22, trailing: 18))
            }
        }
    }

    // MARK: States

    @ViewBuilder private var banners: some View {
        if !state.connected {
            Banner(.warning, title: "Jojo core isn't responding",
                   message: "Showing the last known state, read-only. Nothing can be requested until the core is connected again.")
        } else if state.admissionClosed {
            Banner(.danger, title: "Execution admission is closed",
                   message: "Review the recovery inspection to start a new session.") {
                Button("Open Review") { actions.open(.recoveryCenter) }.buttonStyle(.jojo(.primary, compact: true))
            }
        }
    }

    private var emptyState: some View {
        Placeholder(symbol: "sparkles", tile: Palette.blue, title: "No features yet",
                    message: "Describe what users should be able to do. Jojo reads the code and asks before planning.") {
            Button("New Feature", action: actions.newFeature).buttonStyle(.jojo(.primary, compact: true))
        }
    }

    // MARK: Summary

    private func summary(_ width: CGFloat) -> some View {
        // Four tiles share a row, else two, so none is left alone.
        let fit = Int((width + 12) / 202)
        let columns = fit >= 4 ? 4 : fit >= 2 ? 2 : 1
        return Grid(horizontalSpacing: 12, verticalSpacing: 12) {
            ForEach(0..<((4 + columns - 1) / columns), id: \.self) { row in
                GridRow {
                    ForEach(0..<columns, id: \.self) { column in
                        let index = row * columns + column
                        if index < 4 { kpi(index) } else { Color.clear.gridCellUnsizedAxes([.horizontal, .vertical]) }
                    }
                }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Summary")
    }

    @ViewBuilder private func kpi(_ index: Int) -> some View {
        switch index {
        case 0:
            KPITile(symbol: "tray.full", tile: Palette.orange, label: "Needs you", value: "\(state.needs.count)", unit: "items",
                    detail: "\(counted(state.approvals.count, "approval")) · \(counted(state.recoveries.count, "recovery review"))") { actions.open(.approvalInbox) }
        case 1:
            KPITile(symbol: "play.fill", tile: Palette.blue, label: "Running now", value: "\(state.running.count)", unit: "tasks",
                    detail: state.runningSummary) { actions.open(.agentRuns) }
        case 2:
            KPITile(symbol: "exclamationmark.octagon", tile: Palette.red, label: "Blocked", value: "\(state.blockedCount)", unit: "tasks",
                    detail: state.blockedSummary) { actions.open(.executionPlan) }
        default:
            KPITile(symbol: "cpu", tile: Palette.green, label: "Providers", value: "\(state.available)",
                    unit: "of \(max(state.providers.count, 2)) available", detail: state.providersSummary) { actions.open(.settings) }
        }
    }

    // MARK: Needs you, running, providers

    @ViewBuilder private func main(_ width: CGFloat) -> some View {
        if width >= 780 {
            HStack(alignment: .top, spacing: 20) {
                needsYou.frame(maxWidth: .infinity)
                VStack(alignment: .leading, spacing: 20) { runningNow; providers }
                    .frame(width: max(320, (width - 20) * 0.4))
            }
        } else {
            VStack(alignment: .leading, spacing: 20) { needsYou; runningNow; providers }
        }
    }

    private var needsYou: some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionHead("Needs you", count: state.needs.count, link: "Approval Inbox") { actions.open(.approvalInbox) }
            if state.needs.isEmpty {
                QuietCard("Nothing waits for you.")
            } else {
                if !state.approvals.isEmpty {
                    GroupLabel("Approvals · \(state.approvals.count)")
                    needRows(state.approvals)
                }
                if !state.recoveries.isEmpty {
                    GroupLabel("Recovery · \(state.recoveries.count)").padding(.top, 6)
                    needRows(state.recoveries)
                }
            }
            NoteLine("Scope expansion requests and external actions (push, pull request) are not available yet.")
        }
    }

    private func needRows(_ items: [NeedItem]) -> some View {
        VStack(spacing: 0) {
            ForEach(Array(items.enumerated()), id: \.element.id) { index, item in
                ItemRow(tile: item.tile, symbol: item.symbol, title: item.title, detail: item.detail, first: index == 0,
                        action: { act(item.target) }) {
                    OperationalPill(item.pill, tone: item.tone)
                }
            }
        }
        .cardElevation(radius: 14)
        .clipShape(RoundedRectangle(cornerRadius: 14))
    }

    private func act(_ target: NeedItem.Target) {
        switch target {
        case .document(let id): actions.openDocument(id)
        case .candidate(let id): actions.openCandidate(id)
        case .recovery: actions.open(.recoveryCenter)
        }
    }

    private var runningNow: some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionHead("Running now", count: state.running.count, link: "Agent Runs") { actions.open(.agentRuns) }
            if state.attempts == nil, state.attemptsLoad == .loading || state.attemptsLoad == .idle {
                SkeletonCard()
            } else if case .failed(let reason) = state.attemptsLoad, state.attempts == nil {
                QuietCard(DashboardState.loadFailure(reason))
            } else if state.running.isEmpty {
                QuietCard("Nothing is running.")
            } else {
                ForEach(state.running.prefix(4)) { a in RunCard(attempt: a, now: state.now) }
            }
        }
    }

    private var providers: some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionHead("Providers", link: "Settings") { actions.open(.settings) }
            if state.providers.isEmpty {
                QuietCard(state.executables == nil ? "Not read yet." : "No provider is configured.")
            } else {
                VStack(spacing: 0) {
                    ForEach(Array(state.providers.enumerated()), id: \.element.provider) { index, p in
                        let runs = state.activeRuns(p.provider)
                        ItemRow(tile: p.provider == "ClaudeCode" ? Palette.claudeCode : RGBA(0x3A3A3C),
                                text: p.provider == "ClaudeCode" ? "CC" : "CX", title: providerName(p.provider),
                                detail: ["\(runs) active \(runs == 1 ? "attempt" : "attempts")", p.version].compactMap { $0 }
                                    .joined(separator: " · "),
                                first: index == 0, action: nil) {
                            OperationalPill(DashboardState.availability(p.state), tone: DashboardState.availabilityTone(p.state))
                        }
                    }
                }
                .cardElevation(radius: 14)
                .clipShape(RoundedRectangle(cornerRadius: 14))
            }
            NoteLine("Sign-in state and usage limits are not reported.")
        }
    }

    // MARK: Active projects

    private func projects(_ width: CGFloat) -> some View {
        let all = state.features?.features ?? []
        let shown = all.filter(filter.contains)
        return VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .center, spacing: 10) {
                Text("Active projects").textStyle(.sectionTitle).accessibilityAddTraits(.isHeader)
                Text("\(all.filter { !$0.done }.count)").textStyle(.secondary).foregroundStyle(theme.text2)
                SegmentedCapsule(selection: $filter, label: "Project filter").padding(.leading, 8)
                Spacer(minLength: 8)
                TextLink("Projects") { actions.open(.projects) }
            }
            .padding(.horizontal, 4)
            if state.features == nil, state.featuresLoad == .loading || state.featuresLoad == .idle {
                SkeletonCard()
            } else if case .failed(let reason) = state.featuresLoad, state.features == nil {
                QuietCard(DashboardState.loadFailure(reason))
            } else if shown.isEmpty {
                QuietCard(filter == .done ? "No feature is done yet." : filter == .needsYou ? "No feature waits for you." : "No active feature.")
            } else {
                TableScrolling(width: width) {
                    PipelineTable(features: shown, width: max(width, PipelineTable.minimum), open: actions.openFeature)
                }
            }
        }
    }

    // MARK: Blocked, verification, history

    private func lower(_ width: CGFloat) -> some View {
        let columns = max(1, min(3, Int((width + 20) / 320)))
        return Grid(alignment: .topLeading, horizontalSpacing: 20, verticalSpacing: 20) {
            ForEach(0..<((3 + columns - 1) / columns), id: \.self) { row in
                GridRow(alignment: .top) {
                    ForEach(0..<columns, id: \.self) { column in
                        let index = row * columns + column
                        switch index {
                        case 0: blockedSection
                        case 1: verificationSection
                        case 2: historySection
                        default: Color.clear.gridCellUnsizedAxes([.horizontal, .vertical])
                        }
                    }
                }
            }
        }
    }

    private var blockedSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionHead("Blocked", count: state.blockedCount)
            if state.blocked.isEmpty {
                QuietCard(state.features == nil ? "Not read yet." : "Nothing is blocked.")
            } else {
                VStack(spacing: 0) {
                    ForEach(Array(state.blocked.prefix(6).enumerated()), id: \.element.id) { index, b in
                        BlockedRow(item: b, first: index == 0) {
                            if b.question { actions.answer(b.feature) } else { actions.open(.executionPlan) }
                        }
                    }
                }
                .cardElevation(radius: 14)
                .clipShape(RoundedRectangle(cornerRadius: 14))
            }
        }
        .frame(maxWidth: .infinity, alignment: .topLeading)
    }

    private var verificationSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionHead("Latest verification", link: "Evidence") { actions.open(.verification) }
            let rows = Array(state.verifications.prefix(4))
            if rows.isEmpty {
                QuietCard(state.attempts == nil ? "Not read yet." : "No host validation in this range.")
            } else {
                VStack(spacing: 0) {
                    ForEach(Array(rows.enumerated()), id: \.element.id) { index, a in
                        let v = a.validation!
                        let evidence = DashboardState.evidence(v)
                        ItemRow(tile: Palette.color(evidence.tone), symbol: evidence == .passed ? "checkmark" : evidence == .failed ? "xmark" : "questionmark",
                                title: [state.code(a.feature), a.key].compactMap { $0 }.joined(separator: " · ").nonEmpty ?? "Attempt",
                                detail: [a.ordinal.map { "attempt \($0)" }, DashboardState.validationWords(v)].compactMap { $0 }
                                    .joined(separator: " · "),
                                first: index == 0, action: nil) {
                            EvidencePill(evidence)
                        }
                    }
                }
                .cardElevation(radius: 14)
                .clipShape(RoundedRectangle(cornerRadius: 14))
            }
            NoteLine("Evidence is not sealed by the core yet; each row is the candidate's host validation.")
        }
        .frame(maxWidth: .infinity, alignment: .topLeading)
    }

    private var historySection: some View {
        VStack(alignment: .leading, spacing: 10) {
            SectionHead("Recent history", link: "Activity") { actions.open(.activity) }
            let items = state.history
            if items.isEmpty {
                if state.events.isEmpty, state.eventsLoad == .loading || state.eventsLoad == .idle {
                    SkeletonCard()
                } else {
                    QuietCard("Nothing happened in this range.")
                }
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(items.enumerated()), id: \.element.id) { index, item in
                        HistoryLine(item: item, first: index == 0)
                    }
                }
                .padding(.vertical, 6)
                .padding(.horizontal, 14)
                .cardElevation(radius: 14)
            }
        }
        .frame(maxWidth: .infinity, alignment: .topLeading)
    }
}

// MARK: - Components (COMPONENTS.md, sections 4.5 and 5)

extension Palette {
    /// The fixed tile color of a status hue.
    static func color(_ tone: Tone) -> RGBA {
        switch tone {
        case .neutral: return gray
        case .info: return blue
        case .warn: return orange
        case .ok: return green
        case .danger: return red
        case .review: return purple
        }
    }
}

/// Section heading (COMPONENTS.md, section 4.5): the title (`sectionTitle`), a count in `sub`
/// style, and a trailing text link that opens the related screen.
struct SectionHead: View {
    @Environment(\.theme) private var theme
    let title: String
    var count: Int?
    var link: String?
    var action: () -> Void = {}

    init(_ title: String, count: Int? = nil, link: String? = nil, action: @escaping () -> Void = {}) {
        (self.title, self.count, self.link, self.action) = (title, count, link, action)
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(title).textStyle(.sectionTitle).foregroundStyle(theme.text).accessibilityAddTraits(.isHeader)
            if let count { Text("\(count)").textStyle(.secondary).foregroundStyle(theme.text2) }
            Spacer(minLength: 8)
            if let link { TextLink(link, action: action) }
        }
        .padding(.horizontal, 4)
    }
}

/// A trailing text link: 13 medium `accent`.
struct TextLink: View {
    @Environment(\.theme) private var theme
    let title: String
    let action: () -> Void

    init(_ title: String, action: @escaping () -> Void) {
        (self.title, self.action) = (title, action)
    }

    var body: some View {
        Button(action: action) { Text(title).font(.system(size: 13, weight: .medium)).foregroundStyle(theme.accent) }
            .buttonStyle(.plain)
    }
}

/// A group label inside a section: 11 semibold `text3` in capitals ("APPROVALS · 4").
struct GroupLabel: View {
    @Environment(\.theme) private var theme
    let text: String

    init(_ text: String) { self.text = text }

    var body: some View {
        Text(text).textStyle(.label).foregroundStyle(theme.text3).padding(.horizontal, 4)
    }
}

/// A one-line note under a section, in `secondary` `text3`.
struct NoteLine: View {
    @Environment(\.theme) private var theme
    let text: String

    init(_ text: String) { self.text = text }

    var body: some View {
        Text(text).textStyle(.secondary).foregroundStyle(theme.text3).fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 4)
    }
}

/// A card that says there is nothing to list, in `secondary` `text2`.
struct QuietCard: View {
    @Environment(\.theme) private var theme
    let text: String

    init(_ text: String) { self.text = text }

    var body: some View {
        Text(text).textStyle(.secondary).foregroundStyle(theme.text2)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(14)
            .cardElevation(radius: 14)
    }
}

/// Skeleton rows that keep the layout while a read is outstanding (STATES.md, Loading).
struct SkeletonCard: View {
    @Environment(\.theme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach([1.0, 0.7], id: \.self) { fraction in
                GeometryReader { g in
                    RoundedRectangle(cornerRadius: 5).fill(theme.grayBg).frame(width: min(260, g.size.width) * fraction)
                }
                .frame(height: 10)
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .cardElevation(radius: 14)
        .accessibilityLabel("Loading")
    }
}

/// The placeholder of STATES.md, section 2: a 44 × 44 tile, the title (15 bold), one sentence
/// and the actions, centered on a card (radius 18).
struct Placeholder<Actions: View>: View {
    @Environment(\.theme) private var theme
    let symbol: String
    let tile: RGBA
    let title: String
    let message: String
    @ViewBuilder let actions: () -> Actions

    var body: some View {
        VStack(spacing: 10) {
            RoundedRectangle(cornerRadius: 13).fill(tile.color).frame(width: 44, height: 44)
                .overlay(Image(systemName: symbol).font(.system(size: 20, weight: .medium)).foregroundStyle(.white))
            Text(title).font(.system(size: 15, weight: .bold)).foregroundStyle(theme.text)
            Text(message).textStyle(.secondary).foregroundStyle(theme.text2).multilineTextAlignment(.center)
                .lineSpacing(3).frame(maxWidth: 280)
            HStack(spacing: 6) { actions() }
        }
        .padding(20)
        .frame(maxWidth: .infinity, minHeight: 220)
        .cardElevation(radius: 18)
    }
}

/// A KPI tile (COMPONENTS.md, section 5.2): a clickable card, radius 16, padding 14 16.
struct KPITile: View {
    @Environment(\.theme) private var theme
    @State private var hovered = false
    let symbol: String
    let tile: RGBA
    let label: String
    let value: String
    let unit: String
    let detail: String
    var action: () -> Void = {}

    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 8) {
                    RoundedRectangle(cornerRadius: 7).fill(tile.color).frame(width: 24, height: 24)
                        .overlay(Image(systemName: symbol).font(.system(size: 12, weight: .semibold)).foregroundStyle(.white))
                    Text(label).textStyle(.bodyStrong).foregroundStyle(theme.text)
                }
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    Text(value).textStyle(.kpiValue).monospacedDigit().foregroundStyle(theme.text)
                    Text(unit).textStyle(.secondary).foregroundStyle(theme.text2).lineLimit(1)
                }
                Text(detail).textStyle(.secondary).foregroundStyle(theme.text2).lineLimit(1)
            }
            .padding(EdgeInsets(top: 14, leading: 16, bottom: 14, trailing: 16))
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(hovered ? theme.cardHover : .clear, in: RoundedRectangle(cornerRadius: 16))
            .cardElevation(radius: 16)
            .contentShape(RoundedRectangle(cornerRadius: 16))
        }
        .buttonStyle(.plain)
        .onHover { hovered = $0 }
        .accessibilityElement(children: .combine)
    }
}

/// A list row (COMPONENTS.md, section 5.3): min height 52, padding 8 14, gap 12, a 28 × 28
/// tile, the title and a `sub` line, a status pill and, when it opens something, a chevron.
struct ItemRow<Trailing: View>: View {
    @Environment(\.theme) private var theme
    @State private var hovered = false
    let tile: RGBA
    var symbol: String?
    var text: String?
    let title: String
    let detail: String
    let first: Bool
    let action: (() -> Void)?
    @ViewBuilder let trailing: () -> Trailing

    init(tile: RGBA, symbol: String? = nil, text: String? = nil, title: String, detail: String, first: Bool,
         action: (() -> Void)?, @ViewBuilder trailing: @escaping () -> Trailing) {
        (self.tile, self.symbol, self.text, self.title, self.detail, self.first) = (tile, symbol, text, title, detail, first)
        (self.action, self.trailing) = (action, trailing)
    }

    var body: some View {
        let row = HStack(spacing: 12) {
            RoundedRectangle(cornerRadius: 8).fill(tile.color).frame(width: 28, height: 28)
                .overlay {
                    if let symbol { Image(systemName: symbol).font(.system(size: 13, weight: .semibold)).foregroundStyle(.white) }
                    if let text { Text(text).font(.system(size: 12, weight: .bold)).foregroundStyle(.white) }
                }
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 1) {
                Text(title).textStyle(.bodyStrong).foregroundStyle(theme.text).lineLimit(1)
                Text(detail).textStyle(.secondary).foregroundStyle(theme.text2).lineLimit(1)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            trailing()
            if action != nil {
                Image(systemName: "chevron.right").font(.system(size: 11, weight: .semibold)).foregroundStyle(theme.text3)
                    .accessibilityHidden(true)
            }
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 14)
        .frame(minHeight: 52)
        .background(hovered && action != nil ? theme.cardHover : .clear)
        .overlay(alignment: .top) { if !first { Rectangle().fill(theme.sep).frame(height: 0.5) } }
        .contentShape(Rectangle())
        if let action {
            Button(action: action) { row }.buttonStyle(.plain).onHover { hovered = $0 }
        } else {
            row.accessibilityElement(children: .combine)
        }
    }
}

/// A card of "Running now": the task key and title with its elapsed time, then where it runs
/// and what its newest provider record says. The board's progress bar is not drawn: the core
/// reports no progress.
struct RunCard: View {
    @Environment(\.theme) private var theme
    let attempt: AttemptSummary
    let now: Date

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                Text(attempt.key ?? "—").font(.system(size: 12, weight: .bold)).foregroundStyle(theme.text2)
                Text(attempt.title ?? attempt.featureName ?? "Attempt").textStyle(.bodyStrong).foregroundStyle(theme.text)
                    .lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                if let elapsed = DashboardState.elapsed(attempt.started, now: now) {
                    Text(elapsed).textStyle(.secondary).monospacedDigit().foregroundStyle(theme.text2)
                }
            }
            HStack(spacing: 8) {
                Text(DashboardState.place(attempt).nonEmpty ?? "Not reported").textStyle(.secondary).foregroundStyle(theme.text2)
                    .lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                OperationalPill(attempt.activity?.text ?? attempt.state, tone: attempt.state == "Running" ? .ok : .warn)
            }
        }
        .padding(14)
        .cardElevation(radius: 14)
    }
}

/// A row of "Blocked": a dot (red for a step, orange for a question), the title and why, and
/// the one action that moves it ("Why" or "Answer").
struct BlockedRow: View {
    @Environment(\.theme) private var theme
    let item: BlockedItem
    let first: Bool
    let action: () -> Void

    var body: some View {
        HStack(spacing: 12) {
            Circle().fill(item.question ? theme.orange : theme.red).frame(width: 8, height: 8)
            VStack(alignment: .leading, spacing: 1) {
                Text(item.title).textStyle(.bodyStrong).foregroundStyle(theme.text).lineLimit(1)
                Text(item.reason).textStyle(.secondary).foregroundStyle(theme.text2).lineLimit(1)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if item.question {
                Button("Answer", action: action).buttonStyle(.jojo(.primary, compact: true))
            } else {
                Button("Why", action: action).buttonStyle(.jojo(.secondary, compact: true))
            }
        }
        .padding(.vertical, 8)
        .padding(.horizontal, 14)
        .frame(minHeight: 52)
        .overlay(alignment: .top) { if !first { Rectangle().fill(theme.sep).frame(height: 0.5) } }
    }
}

/// A line of "Recent history": a dot, the core's sentence and the time.
struct HistoryLine: View {
    @Environment(\.theme) private var theme
    let item: HistoryItem
    let first: Bool

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Circle().fill(item.tone.map(theme.foreground) ?? theme.gray).frame(width: 8, height: 8)
                .alignmentGuide(.firstTextBaseline) { $0[.bottom] - 1 }
            Text(item.text).textStyle(.secondary).foregroundStyle(theme.text).lineLimit(2)
                .frame(maxWidth: .infinity, alignment: .leading)
            Text(item.at.map { $0.formatted(date: .omitted, time: .shortened) } ?? "—")
                .textStyle(.secondary).monospacedDigit().foregroundStyle(theme.text2)
        }
        .padding(.vertical, 7)
        .overlay(alignment: .top) { if !first { Rectangle().fill(theme.sep).frame(height: 0.5) } }
    }
}

/// A segmented control (COMPONENTS.md, section 5.6), drawn: a glass capsule group with padding
/// 2; segments 28 high, padding 0 13, text 12 medium; the chosen one a `cardHover` capsule.
struct SegmentedCapsule<Choice: RawRepresentable & CaseIterable & Identifiable & Hashable>: View
where Choice.RawValue == String, Choice.AllCases: RandomAccessCollection {
    @Environment(\.theme) private var theme
    @Binding var selection: Choice
    let label: String

    var body: some View {
        HStack(spacing: 0) {
            ForEach(Choice.allCases) { choice in
                Button { selection = choice } label: {
                    Text(choice.rawValue).font(.system(size: 12, weight: .medium)).foregroundStyle(theme.text)
                        .padding(.horizontal, 13)
                        .frame(height: 28)
                        .background {
                            if choice == selection {
                                Capsule().fill(theme.cardHover).shadow(color: .black.opacity(0.08), radius: 2, y: 1)
                            }
                        }
                        .contentShape(Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(choice == selection ? .isSelected : [])
            }
        }
        .padding(2)
        .glassCapsule()
        .fixedSize()
        .accessibilityElement(children: .contain)
        .accessibilityLabel(label)
    }
}

/// The Dashboard's project table (COMPONENTS.md, section 5.5): one row per feature, one column
/// per lifecycle stage; the feature column at least 200 and 2 parts, six stage columns at least
/// 86 and 1 part, gap 8, padding 12 16. The core reports five stages; Evidence is not reported.
struct PipelineTable: View {
    @Environment(\.theme) private var theme
    let features: [FeatureSummary]
    let width: CGFloat
    /// Opens a feature's chat.
    var open: (String) -> Void = { _ in }
    static let headers = ["Discovery", "Spec", "Planning", "Execution", "Verification", "Evidence"]

    /// Below this the table scrolls sideways, as on the board (min-width 820).
    static let minimum: CGFloat = 32 + 6 * 8 + 200 + 6 * 86

    /// The feature column's and a stage column's width for the table's width.
    private var columns: (feature: CGFloat, stage: CGFloat) {
        let inner = max(0, width - 32 - 6 * 8)
        let part = inner / 8
        let stage = max(86, part)
        return (max(200, inner - 6 * stage), stage)
    }

    var body: some View {
        let c = columns
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                Text("Feature").frame(width: c.feature, alignment: .leading)
                ForEach(Self.headers, id: \.self) { Text($0).frame(width: c.stage, alignment: .leading) }
            }
            .font(.system(size: 11, weight: .semibold))
            .foregroundStyle(theme.text3)
            .padding(.vertical, 10)
            .padding(.horizontal, 16)
            .overlay(alignment: .bottom) { Rectangle().fill(theme.sep).frame(height: 0.5) }
            ForEach(Array(features.enumerated()), id: \.element.id) { index, f in
                HStack(spacing: 8) {
                    VStack(alignment: .leading, spacing: 1) {
                        Text(f.name ?? "Unnamed feature").textStyle(.bodyStrong).foregroundStyle(theme.text).lineLimit(1)
                        Text([f.projectName, f.code].compactMap { $0 }.joined(separator: " · "))
                            .textStyle(.secondary).foregroundStyle(theme.text2).lineLimit(1)
                    }
                    .frame(width: c.feature, alignment: .leading)
                    ForEach(Array(f.stages.enumerated()), id: \.offset) { _, stage in
                        StageCell(stage: stage).frame(width: c.stage, alignment: .leading)
                    }
                    StageCell(stage: .off).frame(width: c.stage, alignment: .leading)
                        .help("Evidence is not reported: the core seals no evidence yet.")
                }
                .padding(.vertical, 12)
                .padding(.horizontal, 16)
                .overlay(alignment: .top) { if index > 0 { Rectangle().fill(theme.sep).frame(height: 0.5) } }
                .contentShape(Rectangle())
                .onTapGesture { open(f.id) }
                .accessibilityAddTraits(.isButton)
                .help("Open the chat of \(f.code)")
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .cardElevation(radius: 14)
        .clipShape(RoundedRectangle(cornerRadius: 14))
    }
}

/// Content of a fixed width that scrolls sideways when it is wider; clipped off screen.
struct TableScrolling<Content: View>: View {
    @Environment(\.offscreen) private var offscreen
    let width: CGFloat
    @ViewBuilder let content: () -> Content

    var body: some View {
        Group {
            if offscreen {
                content().frame(width: width, alignment: .leading).clipped()
            } else {
                ScrollView(.horizontal) { content() }.frame(width: width)
            }
        }
    }
}

/// A stage cell: an 8 pt dot and a word (12 `text2`, gap 6). Done is green, what waits on the
/// human orange, a running stage the running dot, failures red; a stage not reached is a
/// `dotOff` dot alone.
struct StageCell: View {
    @Environment(\.theme) private var theme
    let stage: FeatureStage

    var body: some View {
        HStack(spacing: 6) {
            dot
            if let label = stage.label { Text(label).textStyle(.secondary).foregroundStyle(theme.text2).lineLimit(1) }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(stage.label ?? "Not reached")
    }

    @ViewBuilder private var dot: some View {
        switch stage.state {
        case "Running":
            Circle().fill(theme.accent).frame(width: 8, height: 8)
                .padding(3).background(theme.accentTint, in: Circle()).padding(-3)
        case "Off":
            Circle().fill(theme.dotOff).frame(width: 8, height: 8)
        default:
            Circle().fill(color).frame(width: 8, height: 8)
        }
    }

    private var color: Color {
        switch stage.state {
        case "Done": return theme.green
        case "Current": return theme.accent
        case "NeedsYou": return theme.orange
        case "Blocked", "Failed": return theme.red
        default: return theme.gray
        }
    }
}

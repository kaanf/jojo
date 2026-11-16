import JojoCore
import SwiftUI

// Verification & Evidence (docs/design/screens/VERIFICATION.md): the one loaded attempt
// result of the workspace, its host validation, its evidence and the human decision.

/// What the screen shows. Every field is the helper's answer; nothing is inferred here.
struct VerificationState: Equatable {
    var connection: HelperConnection
    var admission: String
    /// The loaded result, shared with Agent Runs. Nil when nothing is loaded.
    var candidate: CandidateReview?
    var changedFiles: [ChangedFile]
    var scope: ValidationVerdict
    var scopeDetail: String
    var diff: String
    var log: [String]
    var report: String
    var commandPending: Bool

    /// Lines of a diff and of the report drawn before "Show All".
    static let lineLimit = 300

    var link: RecoveryCenterState.Link {
        switch connection {
        case .ready: return .connected
        case .notStarted, .handshaking: return .starting
        case .incompatible: return .incompatible
        case .launchFailed, .channelLost, .ended: return .lost
        }
    }

    var admissionClosed: Bool { link == .connected && !admission.isEmpty && admission != "Open" }

    static func evidence(_ verdict: ValidationVerdict) -> EvidenceState {
        switch verdict {
        case .pass: return .passed
        case .fail: return .failed
        case .notRun: return .notRun
        case .unknown: return .unknown
        }
    }

    /// The verdict header: PASS is the only state that is not "Not verified".
    static func headline(_ c: CandidateReview) -> (title: String, state: EvidenceState, label: String?) {
        if !c.sealed { return ("Not verified", .notRun, "No sealed result") }
        return (c.validation == .pass ? "Host validation passed" : "Not verified", evidence(c.validation), nil)
    }

    static func sentence(_ c: CandidateReview) -> String {
        let rule = "Unknown and not-run checks never count as passed. "
        guard let digest = c.candidate else { return rule + "This attempt has no sealed candidate." }
        return rule + "Results apply to candidate \(digest.prefix(12)) only."
    }

    /// The sealed outcome, in the forms Agent Runs uses.
    static func result(_ c: CandidateReview) -> (state: EvidenceState, label: String) {
        guard c.sealed else { return (.notRun, "Not sealed") }
        switch c.outcome {
        case "Succeeded": return (.passed, "Succeeded")
        case "Failed": return (.failed, "Failed")
        case "Interrupted": return (.incomplete, "Interrupted")
        default: return (.notRun, c.outcome.isEmpty ? "Sealed" : c.outcome)
        }
    }

    static func decision(_ c: CandidateReview) -> String {
        c.decision == "NONE" ? "None recorded" : c.decision
    }

    static func validationNote(_ c: CandidateReview) -> String? {
        if !c.sealed { return "No sealed result yet." }
        switch c.validation {
        case .notRun: return "Jojo has not validated this candidate. Tests the agent says it ran do not count."
        case .unknown: return "The core could not determine the result. Never shown as passed or failed."
        case .pass, .fail: return nil
        }
    }

    static func scope(_ file: ChangedFile) -> (state: EvidenceState, label: String) {
        switch file.scope {
        case "ALLOWED": return (.passed, "In scope")
        case "OUT_OF_SCOPE": return (.failed, "Out of scope")
        case "BASELINE": return (.failed, "Protected baseline")
        default: return (.unknown, "Unknown")
        }
    }

    /// The changed paths counted by the scope the core gave each; zero counts are left out.
    var scopeCounts: String? {
        let labels = changedFiles.map { Self.scope($0).label }
        let parts = ["In scope", "Out of scope", "Protected baseline", "Unknown"].compactMap { label -> String? in
            let count = labels.filter { $0 == label }.count
            return count == 0 ? nil : "\(count) \(label.lowercased())"
        }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }

    /// Said in place of the list while there are no changed paths to show.
    var noFiles: String {
        if candidate?.candidate == nil { return "No sealed candidate." }
        return scope == .notRun ? "Reading the changed paths…" : "No changed paths."
    }

    struct Record: Identifiable, Equatable {
        let name: String
        let note: String
        let state: EvidenceState
        let label: String?
        var id: String { name }
    }

    /// The records the app holds for the loaded attempt.
    var records: [Record] {
        guard let c = candidate else { return [] }
        let reportLines = report.isEmpty ? 0 : report.split(separator: "\n", omittingEmptySubsequences: false).count
        return [
            Record(name: "Sealed candidate", note: c.candidate.map(Self.shortened) ?? "none",
                   state: c.candidate == nil ? .notRun : .sealed, label: c.candidate == nil ? "Not sealed" : nil),
            Record(name: "Attempt log", note: "\(c.logRecords) records",
                   state: c.logRecords > 0 ? .passed : .notRun, label: c.logRecords > 0 ? "Recorded" : "None"),
            Record(name: "Validation receipt", note: c.receipt.map { "receipt \($0)" } ?? "none",
                   state: c.receipt == nil ? .notRun : .passed, label: c.receipt == nil ? "None" : "Recorded"),
            Record(name: "Validation report", note: reportLines == 0 ? "none" : "\(reportLines) lines",
                   state: reportLines == 0 ? .notRun : .passed, label: reportLines == 0 ? "Not loaded" : "Recorded"),
        ]
    }

    /// A digest shortened in the middle ("9f2c41ab…41ab").
    static func shortened(_ digest: String) -> String {
        digest.count > 16 ? "\(digest.prefix(8))…\(digest.suffix(4))" : digest
    }

    /// The loaded log as console lines; says so when there is none.
    var console: [ConsoleLine] {
        guard let c = candidate else { return [] }
        if c.logRecords == 0 { return [ConsoleLine(id: 0, text: "No log records.")] }
        if log.isEmpty { return [ConsoleLine(id: 0, text: "Reading the log records…")] }
        var lines = log.enumerated().map { ConsoleLine(id: $0.offset, text: $0.element) }
        if c.logRecords > AgentRunsState.logLimit {
            lines.append(ConsoleLine(id: lines.count, text: "… (\(c.logRecords - AgentRunsState.logLimit) later records are not loaded)",
                                     kind: .status(.warn)))
        }
        return lines
    }

    var reportLines: [ConsoleLine] {
        if report.isEmpty {
            return [ConsoleLine(id: 0, text: candidate?.receipt == nil ? "No validation recorded." : "No report text loaded.")]
        }
        return report.split(separator: "\n", omittingEmptySubsequences: false).enumerated()
            .map { ConsoleLine(id: $0.offset, text: String($0.element)) }
    }

    // MARK: Actions

    /// Why nothing can be sent for the loaded candidate at all; nil when it can.
    private var undecidable: String? {
        if link != .connected { return "Unavailable: the core is not connected." }
        guard let c = candidate, c.candidate != nil else { return "No sealed candidate." }
        if c.decision != "NONE" { return "Already decided: \(c.decision)." }
        return nil
    }

    private var pending: String? {
        commandPending ? "Unavailable while a command awaits the core." : nil
    }

    var validateBlocked: String? { undecidable ?? pending }
    var rejectBlocked: String? { undecidable ?? pending }

    /// Accept follows `CandidateReview.canAccept`; the rule itself is the core's.
    var acceptBlocked: String? {
        if let undecidable { return undecidable }
        if candidate?.canAccept != true {
            return "Unavailable: the core accepts only a passing host validation of exactly this candidate."
        }
        return pending
    }
}

struct VerificationActions {
    var load: (String) -> Void = { _ in }
    var validate: () -> Void = {}
    var decide: (CandidateAction) -> Void = { _ in }
    var open: (Screen) -> Void = { _ in }
}

struct VerificationScreen: View {
    @ObservedObject var model: AppModel
    let open: (Screen) -> Void

    var body: some View {
        let workspace = model.workspace
        VerificationView(
            state: VerificationState(connection: model.client.connection, admission: workspace.admission,
                                     candidate: workspace.candidate, changedFiles: workspace.changedFiles,
                                     scope: workspace.scope, scopeDetail: workspace.scopeDetail, diff: workspace.diff,
                                     log: workspace.agentLog, report: workspace.validationReport,
                                     commandPending: workspace.commandPending),
            actions: VerificationActions(load: { workspace.loadCandidate($0) }, validate: { workspace.validateCandidate() },
                                         decide: { workspace.decideCandidate($0) }, open: open))
    }
}

struct VerificationView: View {
    @Environment(\.theme) private var theme
    let state: VerificationState
    var actions = VerificationActions()
    @State private var identity: String
    @State private var rejecting = false
    /// The long texts drawn in full: "report".
    @State private var expanded: Set<String>

    init(state: VerificationState, actions: VerificationActions = VerificationActions(), expanded: Set<String> = []) {
        self.state = state
        self.actions = actions
        _identity = State(initialValue: state.candidate?.attempt ?? "")
        _expanded = State(initialValue: expanded)
    }

    var body: some View {
        GeometryReader { geometry in
            // Two columns only while the main column keeps at least 460.
            let wide = geometry.size.width >= 800
            HStack(spacing: 0) {
                Scrolling {
                    VStack(alignment: .leading, spacing: 16) {
                        loadBar
                        banners
                        if let c = state.candidate {
                            header(c)
                            summary(c)
                            validation(c)
                            evidence
                            output(c)
                            report
                            if !wide { decision(c) }
                        } else {
                            placeholder.frame(minHeight: 260)
                        }
                    }
                    .padding(EdgeInsets(top: 16, leading: 18, bottom: 22, trailing: 18))
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                if wide, let c = state.candidate {
                    Scrolling { decision(c).padding(14) }
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                        .background(theme.sidebar)
                        .overlay(alignment: .leading) { Rectangle().fill(theme.sidebarEdge).frame(width: 1) }
                        .frame(width: 320)
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        // The field names the loaded attempt, also when Agent Runs loaded it.
        .onChange(of: state.candidate?.attempt) { _, attempt in
            if let attempt { identity = attempt }
        }
        .confirmationDialog("Reject this candidate?", isPresented: $rejecting, titleVisibility: .visible) {
            Button("Reject Candidate", role: .destructive) { actions.decide(.Reject) }
            Button("Keep Undecided", role: .cancel) {}
        } message: {
            Text(Self.rejectSentence)
        }
    }

    static let rejectSentence = "Records your decision to reject this candidate. A recorded decision is not changed from this app."

    private var loadBar: some View {
        HStack(spacing: 8) {
            InputField(prompt: "Attempt identity (32 hex)", text: $identity, mono: true, submit: { actions.load(identity) })
                .frame(maxWidth: 340)
            Button("Load") { actions.load(identity) }.buttonStyle(.jojo(.secondary, compact: true))
                .disabled(state.link != .connected)
                .help("Loads the result of this attempt. One result is loaded at a time; Agent Runs shows the same one.")
            Spacer(minLength: 0)
        }
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

    private var placeholder: some View {
        StateBlock(symbol: "checkmark.shield", title: "No attempt loaded",
                   message: "Load a finished attempt by its identity, or choose one in Agent Runs and use Load Result.") {
            Button("Open Agent Runs") { actions.open(.agentRuns) }.buttonStyle(.jojo(.secondary, compact: true))
        }
    }

    // MARK: Main column

    private func header(_ c: CandidateReview) -> some View {
        let headline = VerificationState.headline(c)
        return VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 10) {
                Text(headline.title).font(.system(size: 20, weight: .bold)).tracking(-0.4).foregroundStyle(theme.textStrong)
                EvidencePill(headline.state, label: headline.label)
            }
            Text(VerificationState.sentence(c)).textStyle(.body).foregroundStyle(theme.text2)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func summary(_ c: CandidateReview) -> some View {
        let result = VerificationState.result(c)
        return VStack(alignment: .leading, spacing: 8) {
            Card(.panel) {
                WrappingRow {
                    LabeledField("Attempt") { identifier(c.attempt) }
                    LabeledField("Candidate") {
                        if let digest = c.candidate { identifier(digest) } else { value("None") }
                    }
                    LabeledField("Result") { EvidencePill(result.state, label: result.label) }
                    if !c.reason.isEmpty { LabeledField("Reason") { value(c.reason) } }
                    LabeledField("Provider said") { value(c.providerTerminal.isEmpty ? "Nothing" : c.providerTerminal) }
                    LabeledField("Decision") { value(VerificationState.decision(c)) }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            Footnote("What the provider said is its own statement, not a validation result. Not reported: the repository, branch and commit under test, whether the snapshot is clean, and how the validator is isolated.")
        }
        .textSelection(.enabled)
    }

    private func validation(_ c: CandidateReview) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Host validation") {
                if let why = state.validateBlocked { DisabledReason(why) }
                Button("View Changes") { actions.open(.sourceControl) }.buttonStyle(.jojo(.secondary, compact: true))
                    .help("The changed files and the diff are in Source Control.")
                Button("Run Validation") { actions.validate() }.buttonStyle(.jojo(.primary, compact: true))
                    .disabled(state.validateBlocked != nil)
                    .help(state.validateBlocked ?? "Asks the core to validate the sealed candidate on the host.")
            }
            Card(.panelList) {
                VStack(alignment: .leading, spacing: 0) {
                    ResultRow("Host validation", scope: c.receipt.map { "Receipt \($0)" } ?? "No receipt",
                              state: VerificationState.evidence(c.validation),
                              command: c.validationDetail.isEmpty ? nil : c.validationDetail,
                              note: VerificationState.validationNote(c))
                    Rectangle().fill(theme.hairline).frame(height: 1)
                    ResultRow("Write scope", scope: "\(state.changedFiles.count) changed paths",
                              state: VerificationState.evidence(state.scope),
                              command: state.scopeDetail.isEmpty ? nil : state.scopeDetail, note: state.scopeCounts)
                    if !c.reasons.isEmpty {
                        Rectangle().fill(theme.hairline).frame(height: 1)
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(Array(c.reasons.enumerated()), id: \.offset) { reason in
                                Text(reason.element).textStyle(.secondary).foregroundStyle(theme.text2)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                        }
                        .textSelection(.enabled)
                        .padding(.vertical, 10)
                        .padding(.horizontal, 14)
                    }
                }
            }
            Footnote("The core records one host validation per candidate. Suites, test counts, commands, durations and a log per suite are not reported.")
        }
    }

    private var evidence: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Evidence")
            Card(.panelList) {
                Rows(state.records) { record in
                    EvidenceRow(record.name, note: record.note, state: record.state, label: record.label)
                }
            }
            Footnote("Document revisions, hashes checked on read, tool and runner versions and deployment receipts are not reported.")
        }
    }

    private func output(_ c: CandidateReview) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Agent output") {
                Text("\(state.log.count) of \(c.logRecords) records").textStyle(.meta).foregroundStyle(theme.text3)
            }
            Console(state.console)
        }
    }

    private var report: some View {
        let lines = state.reportLines
        return VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Validation report") { showAll("report", lines.count) }
            Console(shown("report", lines))
        }
    }

    private func shown<Line>(_ part: String, _ lines: [Line]) -> [Line] {
        expanded.contains(part) ? lines : Array(lines.prefix(VerificationState.lineLimit))
    }

    @ViewBuilder
    private func showAll(_ part: String, _ count: Int) -> some View {
        if count > VerificationState.lineLimit, !expanded.contains(part) {
            Button("Show All \(count) Lines") { expanded.insert(part) }.buttonStyle(.jojo(.ghost, compact: true))
                .help("The first \(VerificationState.lineLimit) lines are drawn.")
        }
    }

    // MARK: Decision

    private func decision(_ c: CandidateReview) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Decision")
            Card(.panel) {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Accepting records your decision only. Nothing is merged, pushed or published.")
                        .textStyle(.body).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
                    FactGrid {
                        Fact("Validation") { EvidencePill(VerificationState.evidence(c.validation)) }
                        Fact("Write scope") { EvidencePill(VerificationState.evidence(state.scope)) }
                        Fact("Decision", VerificationState.decision(c))
                    }
                    if state.commandPending { RequestedNotice() }
                    action("Accept Candidate", .primary, state.acceptBlocked) { actions.decide(.Accept) }
                    action("Reject Candidate…", .destructive, state.rejectBlocked) { rejecting = true }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            Footnote("Sealing evidence, exporting a bundle and comparing validation runs are not available: the core has no such commands and reports only the latest validation of a candidate.")
        }
    }

    private func action(_ title: String, _ kind: JojoButtonStyle.Kind, _ blocked: String?,
                        _ run: @escaping () -> Void) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            Button(title, action: run).buttonStyle(.jojo(kind)).disabled(blocked != nil)
            if let blocked { DisabledReason(blocked).fixedSize(horizontal: false, vertical: true) }
        }
    }

    // MARK: Pieces

    private func identifier(_ text: String) -> some View {
        Text(text).textStyle(.mono).foregroundStyle(theme.text)
    }

    private func value(_ text: String) -> some View {
        Text(text).textStyle(.control).foregroundStyle(theme.text)
    }

    private func quiet(_ text: String) -> some View {
        Text(text).textStyle(.secondary).foregroundStyle(theme.text3)
            .padding(.vertical, 10)
            .padding(.horizontal, 14)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
}

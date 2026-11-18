import JojoCore
import SwiftUI
import UniformTypeIdentifiers

// Verification & Evidence (docs/design/screens/VERIFICATION.md), from
// `VerificationMac.dc.html`: the loaded candidate's verdict, what was tested, its host
// validation runs ("V-n") as suites, the evidence the core records, and in the aside the
// seal (not available), export, the decision and the history of runs. The loaded candidate
// is the one Source Control shows (`loadCandidate`); protocol 1.9 (`AttemptDetail`) adds
// the runs, the validator, the source under test and the executable.

/// What the screen shows. Every value is the helper's answer.
struct VerificationState: Equatable {
    var connection: HelperConnection
    var admission: String
    var scope = FeatureScope()
    var candidate: CandidateReview?
    var files: [ChangedFile] = []
    var verdict = ValidationVerdict.notRun
    var report = ""
    var commandPending = false
    /// Protocol 1.9: the loaded attempt beyond `Attempts`.
    var detail: AttemptDetail?
    /// The held feature's attempts that have a candidate, for the chooser.
    var candidates: [AttemptSummary] = []
    var reportsDetail = true
    var now = Date()

    var link: RecoveryCenterState.Link { coreLink(connection) }

    var runs: [ValidationRunDetail] { detail?.validations ?? [] }
    var current: ValidationRunDetail? { runs.last }

    /// "V-2 passed", "V-1 failed", "Not validated".
    var headline: (title: String, pill: (String, Tone), tile: RGBA) {
        guard let c = candidate else { return ("No candidate loaded", ("Not verified", .neutral), Palette.gray) }
        let run = current.map { "V-\($0.ordinal)" } ?? "Host validation"
        switch c.validation {
        case .pass: return ("\(run) passed", ("Host validation passed", .ok), Palette.green)
        case .fail: return ("\(run) failed", ("Not verified", .danger), Palette.red)
        case .unknown: return ("\(run) without a verdict", ("Not verified", .warn), Palette.orange)
        case .notRun: return ("Not validated yet", ("Not verified", .neutral), Palette.gray)
        }
    }

    /// Passed, Failed and Unknown over the runs; the board's other counts are not reported.
    var counts: [(String, Int, Int, Tone)] {
        let passed = runs.filter(\.passed).count
        let failed = runs.count - passed
        let unknown = candidate?.validation == .unknown ? 1 : 0
        return [("Passed", passed, 0, .ok), ("Failed", failed, 0, .danger), ("Unknown", unknown, 0, .neutral)]
    }

    /// One suite per run: the step's command and its test inventory.
    var suiteName: String { detail?.tests.first ?? detail?.key ?? "Host validation" }

    static func shortened(_ digest: String) -> String {
        digest.count > 10 ? "\(digest.prefix(4))…\(digest.suffix(4))" : digest
    }

    /// Why nothing can be sent for the loaded candidate at all; nil when it can.
    private var undecidable: String? {
        if link != .connected { return "Unavailable: the core is not connected." }
        guard let c = candidate, c.candidate != nil else { return "No sealed candidate." }
        if c.decision != "NONE" { return "Already decided: \(c.decision)." }
        return nil
    }

    private var pending: String? { commandPending ? "Unavailable while a command awaits the core." : nil }

    var validateBlocked: String? { undecidable ?? pending }
    var rejectBlocked: String? { undecidable ?? pending }

    /// Accept follows `CandidateReview.canAccept`; the rule itself is the core's.
    var acceptBlocked: String? {
        if let undecidable { return undecidable }
        if candidate?.canAccept != true { return "Unavailable: the core accepts only a passing host validation of exactly this candidate." }
        return pending
    }

    /// Protocol 1.14: why the other provider cannot be asked to review the loaded candidate.
    var reviewBlocked: String? {
        if candidate?.candidate == nil { return "No sealed candidate." }
        return Advisory.blocked(link: link, commandPending: commandPending, offer: detail?.advisory)
    }

    /// The requirements of a seal, met or not, from what the core reports.
    var sealChecks: [(String, Bool, String)] {
        let failed = runs.filter { !$0.passed }.count
        return [
            ("Host validation passed", candidate?.validation == .pass, candidate?.validation == .pass ? "" : candidate?.validation.label ?? "none"),
            ("No unknown results", candidate?.validation != .unknown, candidate?.validation == .unknown ? "1" : ""),
            ("No failed runs kept", failed == 0, failed == 0 ? "" : "\(failed) failed"),
            ("Candidate accepted", candidate?.decision == "ACCEPTED", candidate?.decision == "ACCEPTED" ? "" : "Not decided"),
            ("Deployment receipt present", false, "Not available yet"),
        ]
    }

    /// The Markdown of Export Evidence Bundle: the records as they are, marked unsealed.
    var bundle: String {
        guard let c = candidate else { return "" }
        var out = "# Evidence bundle (unsealed)\n\nExported by Jojo on \(now.formatted(.iso8601)). Exporting does not seal.\n\n"
        out += "- Attempt: \(c.attempt)\n- Candidate: \(c.candidate ?? "none")\n- Outcome: \(c.outcome) \(c.reason)\n"
        out += "- Host validation: \(c.validation.label) \(c.validationDetail)\n- Decision: \(c.decision)\n"
        if let d = detail {
            out += "- Task: \(d.key ?? "-") · attempt \(d.ordinal ?? 1)\n- Base: \(d.base ?? "-")\n- Head: \(d.candidateHead ?? "-")\n"
            out += "- Executable: \(d.executableVersion ?? "not reported")\n- Evidence records: \(d.evidenceRecords)\n"
            out += "- Write scope: \(d.writeScope.joined(separator: ", "))\n- Command: \(d.command.joined(separator: " "))\n\n## Runs\n\n"
            for r in d.validations {
                out += "- V-\(r.ordinal) · \(r.passed ? "passed" : "failed") · \(r.at.map { $0.formatted(.iso8601) } ?? "-") · receipt event \(r.receipt) · validator \(r.validator)\n"
                for reason in r.reasons { out += "  - \(reason)\n" }
            }
        }
        out += "\n## Changed paths\n\n" + files.map { "- \($0.kind) \($0.path) (\($0.scope))" }.joined(separator: "\n")
        if !report.isEmpty { out += "\n\n## Report\n\n```\n\(report)\n```\n" }
        return out
    }
}

struct VerificationActions {
    var choose: (String) -> Void = { _ in }
    var validate: () -> Void = {}
    var decide: (CandidateAction) -> Void = { _ in }
    var open: (Screen) -> Void = { _ in }
    var export: () -> Void = {}
    /// Protocol 1.14: the other provider's review of the loaded candidate.
    var requestReview: () -> Void = {}
}

struct VerificationScreen: View {
    @ObservedObject var model: AppModel
    let visible: Bool
    @Binding var exporting: Bool
    let open: (Screen) -> Void

    var body: some View {
        let workspace = model.workspace
        VerificationView(state: state, actions: VerificationActions(
            choose: { workspace.loadCandidate($0) },
            validate: { workspace.validateCandidate() },
            decide: { workspace.decideCandidate($0) },
            open: open,
            export: { exporting = true },
            requestReview: {
                guard state.reviewBlocked == nil, let c = workspace.candidate, let candidate = c.candidate else { return }
                workspace.reviewAttempt(c.attempt, candidate: candidate)
            }))
            .onAppear { if visible { read() } }
            .onChange(of: visible) { if visible { read() } }
            .onChange(of: workspace.snapshotSeq) { if visible { read() } }
            .onChange(of: workspace.candidate?.attempt) { if visible { read() } }
            .fileExporter(isPresented: $exporting, document: TranscriptDocument(text: state.bundle),
                          contentType: TranscriptDocument.readableContentTypes[0],
                          defaultFilename: "\(state.detail?.key ?? "candidate") evidence (unsealed)") { _ in }
    }

    var state: VerificationState {
        let workspace = model.workspace
        let scope = FeatureScope(workspace: workspace)
        let detail = workspace.attemptDetail?.id == workspace.candidate?.attempt ? workspace.attemptDetail : nil
        return VerificationState(
            connection: model.client.connection, admission: workspace.admission, scope: scope, candidate: workspace.candidate,
            files: workspace.changedFiles, verdict: workspace.scope, report: workspace.validationReport,
            commandPending: workspace.commandPending, detail: detail,
            candidates: (workspace.attempts?.attempts ?? []).filter { $0.validation != nil && scope.holds($0.feature) },
            reportsDetail: model.client.hello.map { $0.queries.contains("AttemptDetail") } ?? true)
    }

    private func read() {
        let workspace = model.workspace
        workspace.loadFeatures()
        workspace.loadAttempts()
        if workspace.candidate == nil, let newest = state.candidates.first { workspace.loadCandidate(newest.id) }
        if let attempt = workspace.candidate?.attempt { workspace.loadAttemptDetail(attempt) }
    }
}

struct VerificationView: View {
    @Environment(\.theme) private var theme
    let state: VerificationState
    var actions = VerificationActions()
    @State private var rejecting = false

    static let rejectSentence = "Records your decision to reject this candidate. A recorded decision is not changed from this app."

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 14) {
                CoreBanners(connection: state.connection, missing: state.reportsDetail ? nil : ("AttemptDetail", "validation runs"))
                LifecycleCard(summary: state.scope.summary)
            }
            .padding(.horizontal, 10)
            InspectorColumns(trailingWidth: 320, wide: 940) {
                main
            } trailing: {
                aside
            }
        }
        .padding(.top, 6)
        .confirmationDialog("Reject this candidate?", isPresented: $rejecting, titleVisibility: .visible) {
            Button("Reject Candidate", role: .destructive) { actions.decide(.Reject) }
            Button("Keep Undecided", role: .cancel) {}
        } message: {
            Text(Self.rejectSentence)
        }
    }

    // MARK: Main column

    @ViewBuilder
    private var main: some View {
        VStack(alignment: .leading, spacing: 16) {
            if state.candidates.count > 1 {
                ChoiceCapsule(options: state.candidates.prefix(6).map { a in (a.id, "\(a.key ?? "step") #\(a.ordinal ?? 1)") },
                              selection: state.candidate?.attempt, label: "Candidate") { actions.choose($0) }
            }
            if let c = state.candidate {
                verdict(c)
                candidateCard(c)
                suites(c)
                manifest(c)
            } else {
                Placeholder(symbol: "checkmark.seal", tile: Palette.green, title: "No candidate loaded",
                            message: "A candidate appears here when an attempt of a plan step seals one.") {
                    Button("Open Agent Runs") { actions.open(.agentRuns) }.buttonStyle(.jojo(.secondary, compact: true))
                }
                .frame(maxWidth: .infinity)
                .padding(24)
                .cardElevation(radius: 18)
            }
        }
    }

    private func verdict(_ c: CandidateReview) -> some View {
        let head = state.headline
        return FlowRow(spacing: 18) {
            HStack(alignment: .top, spacing: 14) {
                RoundedRectangle(cornerRadius: 12).fill(head.tile.color).frame(width: 44, height: 44)
                    .overlay(Image(systemName: c.validation == .pass ? "checkmark" : "exclamationmark").foregroundStyle(.white)
                        .font(.system(size: 18, weight: .bold)))
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 8) {
                        OperationalPill(head.pill.0, tone: head.pill.1)
                        Text(head.title).font(.system(size: 18, weight: .bold))
                    }
                    Text("Unknown and not-run checks never count as passed. Results apply to candidate \(ApprovalInboxState.short(c.candidate ?? c.attempt)) only.")
                        .textStyle(.secondary).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: 360, alignment: .leading)
                }
            }
            HStack(spacing: 6) {
                ForEach(state.counts, id: \.0) { count in
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(count.1)").font(.system(size: 20, weight: .bold)).foregroundStyle(theme.foreground(count.3))
                        Text(count.0).font(.system(size: 11)).foregroundStyle(theme.text2)
                    }
                    .padding(EdgeInsets(top: 8, leading: 12, bottom: 8, trailing: 12))
                    .frame(minWidth: 84, alignment: .leading)
                    .background(theme.cardHover, in: RoundedRectangle(cornerRadius: 12))
                }
            }
        }
        .padding(EdgeInsets(top: 18, leading: 20, bottom: 18, trailing: 20))
        .frame(maxWidth: .infinity, alignment: .leading)
        .cardElevation(radius: 18)
    }

    private func candidateCard(_ c: CandidateReview) -> some View {
        let d = state.detail
        return VStack(alignment: .leading, spacing: 8) {
            Text("Candidate").textStyle(.sectionTitle).padding(.horizontal, 4)
            DefinitionCard(rows: [
                Definition(key: "Candidate", value: c.candidate ?? "No sealed candidate", mono: true),
                Definition(key: "Source under test", value: d.map { "\($0.repositoryName ?? "repository") @ \(($0.candidateHead ?? "working tree").prefix(7)) on base \(($0.base ?? "").prefix(7))" } ?? "Not reported", mono: true),
                Definition(key: "Snapshot", value: d?.snapshot.map { "sha256 \(VerificationState.shortened($0))" } ?? "Not reported", mono: d?.snapshot != nil),
                Definition(key: "Validator", value: state.current.map { "\($0.validator) · on the host, not in the agent's session" } ?? "Not run yet"),
            ], keyWidth: 130)
        }
    }

    private func suites(_ c: CandidateReview) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Text("Suites\(state.current.map { " · V-\($0.ordinal)" } ?? "")").textStyle(.sectionTitle)
                Spacer()
                Button("Run Validation") { actions.validate() }.buttonStyle(.jojo(.primary, compact: true))
                    .disabled(state.validateBlocked != nil).help(state.validateBlocked ?? "Asks the core to validate the sealed candidate on the host.")
            }
            .padding(.horizontal, 4)
            VStack(alignment: .leading, spacing: 0) {
                row(["Suite", "Result", "Count", "Command and detail"], header: true)
                suiteRow(c)
                scopeRow
            }
            .cardElevation(radius: 14)
            NoteLine("The core records one host validation per run: the step's command and its test inventory. Durations, per-suite counts and other suites are not reported.")
        }
    }

    private func row(_ cells: [String], header: Bool) -> some View {
        HStack(spacing: 10) {
            ForEach(Array(cells.enumerated()), id: \.offset) { i, cell in
                Text(cell).font(.system(size: 11, weight: .semibold)).foregroundStyle(theme.text3)
                    .frame(width: i == 0 ? 150 : i == 1 ? 110 : i == 2 ? 70 : nil, alignment: .leading)
                    .frame(maxWidth: i == 3 ? .infinity : nil, alignment: .leading)
            }
        }
        .padding(EdgeInsets(top: 8, leading: 14, bottom: 8, trailing: 14))
    }

    private func suiteRow(_ c: CandidateReview) -> some View {
        let d = state.detail
        let pill: (String, Tone) = switch c.validation {
        case .pass: ("Passed", .ok)
        case .fail: ("Failed", .danger)
        case .unknown: ("Unknown", .neutral)
        case .notRun: ("Not run", .warn)
        }
        return HStack(alignment: .top, spacing: 10) {
            VStack(alignment: .leading, spacing: 1) {
                Text(state.suiteName).textStyle(.bodyStrong)
                Text(d?.repositoryName ?? "repository").textStyle(.secondary).foregroundStyle(theme.text2)
            }
            .frame(width: 150, alignment: .leading)
            OperationalPill(pill.0, tone: pill.1).frame(width: 110, alignment: .leading)
            Text(d.map { "\($0.tests.count) tests" } ?? "n/a").textStyle(.mono).frame(width: 70, alignment: .leading)
            VStack(alignment: .leading, spacing: 2) {
                Text(d?.command.joined(separator: " ").nonEmpty ?? "No host command").textStyle(.mono).foregroundStyle(theme.text2)
                Text(([c.validationDetail] + c.reasons).filter { !$0.isEmpty }.joined(separator: " · ").nonEmpty ?? "No detail recorded")
                    .font(.system(size: 12)).foregroundStyle(c.validation == .fail ? theme.red : theme.text)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(EdgeInsets(top: 10, leading: 14, bottom: 10, trailing: 14))
        .overlay(alignment: .top) { Rectangle().fill(theme.sep).frame(height: 0.5) }
    }

    private var scopeRow: some View {
        let outside = SourceControlState.violations(in: state.files).count
        let pill: (String, Tone) = switch state.verdict {
        case .pass: ("Passed", .ok)
        case .fail: ("Failed", .danger)
        case .unknown: ("Unknown", .neutral)
        case .notRun: ("Not run", .warn)
        }
        return HStack(alignment: .top, spacing: 10) {
            VStack(alignment: .leading, spacing: 1) {
                Text("Write scope").textStyle(.bodyStrong)
                Text("core check").textStyle(.secondary).foregroundStyle(theme.text2)
            }
            .frame(width: 150, alignment: .leading)
            OperationalPill(pill.0, tone: pill.1).frame(width: 110, alignment: .leading)
            Text("\(state.files.count - outside) / \(state.files.count)").textStyle(.mono).frame(width: 70, alignment: .leading)
            HStack {
                Text(outside == 0 ? "Every changed path is inside the approved scope." : "\(outside) \(outside == 1 ? "path" : "paths") outside the approved scope.")
                    .font(.system(size: 12)).frame(maxWidth: .infinity, alignment: .leading)
                Button("View Changes") { actions.open(.sourceControl) }.buttonStyle(.jojo(.secondary, compact: true))
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(EdgeInsets(top: 10, leading: 14, bottom: 10, trailing: 14))
        .overlay(alignment: .top) { Rectangle().fill(theme.sep).frame(height: 0.5) }
    }

    private func manifest(_ c: CandidateReview) -> some View {
        let d = state.detail
        return VStack(alignment: .leading, spacing: 8) {
            GroupLabel("Evidence manifest · \(ApprovalInboxState.short(c.candidate ?? c.attempt))")
            DefinitionCard(rows: [
                Definition(key: "Approvals bound", value: state.current.map { "sha256 \(VerificationState.shortened($0.approvals))" } ?? "Not validated yet",
                           mono: state.current != nil, pill: state.current == nil ? nil : ("Recorded", .ok)),
                Definition(key: "Attempt log", value: "\(c.logRecords) records", pill: ("Recorded", .ok)),
                Definition(key: "Validation reports", value: "\(state.runs.count) runs",
                           pill: c.validation == .pass ? ("Complete", .ok) : ("Incomplete", .warn)),
                Definition(key: "Tool and runner versions", value: d?.executableVersion ?? "Not reported",
                           pill: d?.executableVersion == nil ? nil : ("Recorded", .ok)),
                Definition(key: "Deployment receipt", value: "Not available yet", pill: ("Missing", .danger)),
            ], keyWidth: 160)
        }
    }

    // MARK: Aside

    private var aside: some View {
        GlassInspector {
            LabeledSection(title: "Decision") {
                if let c = state.candidate, c.decision != "NONE" {
                    DefinitionCard(rows: [Definition(key: "Recorded", value: "", pill: c.decision == "ACCEPTED" ? ("Accepted", .ok) : ("Rejected", .danger))],
                                   keyWidth: 80)
                } else {
                    HStack(spacing: 6) {
                        Button("Accept Candidate") { actions.decide(.Accept) }.buttonStyle(.jojo(.primary, compact: true))
                            .disabled(state.acceptBlocked != nil).help(state.acceptBlocked ?? "Records your acceptance of this exact candidate.")
                        Button("Reject Candidate…") { rejecting = true }.buttonStyle(.jojo(.ghost, compact: true))
                            .disabled(state.rejectBlocked != nil).help(state.rejectBlocked ?? Self.rejectSentence)
                    }
                    if let why = state.acceptBlocked, state.candidate != nil {
                        Text(why).font(.system(size: 11)).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
            if state.candidate != nil {
                AdvisorySection(offer: state.detail?.advisory, subject: "this candidate", blocked: state.reviewBlocked,
                                request: actions.requestReview)
            }
            LabeledSection(title: "Seal evidence") {
                Text("Sealing would make the manifest immutable for this candidate. The core has no seal yet.")
                    .font(.system(size: 12)).fixedSize(horizontal: false, vertical: true)
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(state.sealChecks, id: \.0) { check in
                        HStack(spacing: 8) {
                            Circle().fill(check.1 ? theme.green : theme.red).frame(width: 7, height: 7)
                            Text(check.0).textStyle(.secondary).frame(maxWidth: .infinity, alignment: .leading)
                            Text(check.2).textStyle(.secondary).foregroundStyle(theme.text2)
                        }
                        .padding(EdgeInsets(top: 7, leading: 12, bottom: 7, trailing: 12))
                    }
                }
                .cardElevation(radius: 12)
                Button("Seal Evidence") {}.buttonStyle(.jojo(.secondary)).frame(maxWidth: .infinity).disabled(true)
                    .help("Not available yet: the core seals no evidence.")
            }
            LabeledSection(title: "Export") {
                Text("Exports the records as they are, marked unsealed. Exporting does not seal.")
                    .font(.system(size: 12)).fixedSize(horizontal: false, vertical: true)
                Button("Export Evidence Bundle", action: actions.export).buttonStyle(.jojo(.secondary)).frame(maxWidth: .infinity)
                    .disabled(state.candidate == nil)
            }
            LabeledSection(title: "Verification history") {
                if state.runs.isEmpty {
                    NotReported("No host validation run yet.")
                } else {
                    DefinitionCard(rows: state.runs.reversed().map { r in
                        Definition(key: r.ordinal == state.current?.ordinal ? "V-\(r.ordinal) · current" : "V-\(r.ordinal)",
                                   value: "candidate \(ApprovalInboxState.short(r.candidate)) · \(boardTime(r.at, now: state.now) ?? "")",
                                   pill: r.passed ? ("Passed", .ok) : ("Failed", .danger))
                    }, keyWidth: 90)
                }
            }
        }
    }
}

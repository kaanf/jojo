import JojoCore
import SwiftUI

// Source Control (docs/design/screens/SOURCE_CONTROL.md), from `SourceControlMac.dc.html`:
// the held feature's candidates by attempt, one candidate's branch facts, changed files,
// status, scope banner and diff (unified or side by side), and in the inspector the scope
// validation, external actions (M5) and CI. The loaded candidate is the one Verification &
// Evidence shows (`loadCandidate`); protocol 1.9 (`AttemptDetail`) adds its head and the
// patterns of its write scope.

/// Unified or Side by side.
enum DiffLayout: String, CaseIterable, Identifiable, Sendable {
    case unified = "Unified", sideBySide = "Side by side"
    var id: String { rawValue }
}

/// One line of a file's diff with both line numbers.
struct DiffRow: Equatable, Identifiable {
    enum Kind: Equatable { case context, added, removed, hunk }
    let id: Int
    let kind: Kind
    let old: Int?
    let new: Int?
    let text: String
}

/// One changed file's part of the diff.
struct FileDiff: Equatable {
    let path: String
    let rows: [DiffRow]
    var added: Int { rows.filter { $0.kind == .added }.count }
    var removed: Int { rows.filter { $0.kind == .removed }.count }

    /// "+48 −6"; nil without a counted line.
    var counts: String? {
        let parts = [added > 0 ? "+\(added)" : nil, removed > 0 ? "−\(removed)" : nil].compactMap { $0 }
        return parts.isEmpty ? nil : parts.joined(separator: " ")
    }
}

/// What the screen shows. Every value is the helper's answer.
struct SourceControlState: Equatable {
    var connection: HelperConnection
    var scope = FeatureScope()
    /// The held feature's attempts that have a candidate (1.7), newest first.
    var candidates: [AttemptSummary] = []
    /// The loaded attempt's result, changed files, diff and the base they were compared with.
    var result: CandidateReview?
    var files: [ChangedFile] = []
    var diff = ""
    var base = ""
    var verdict = ValidationVerdict.notRun
    /// Protocol 1.9: the loaded attempt beyond `Attempts`.
    var detail: AttemptDetail?
    var layout = DiffLayout.unified
    /// The chosen file; nil shows the first.
    var chosenFile: String?
    var reportsDetail = true
    /// M5 (1.10): the plan runs and their push and pull request; the held feature's run.
    var external: ExternalList?
    var externalLoad = PlanLoad.idle
    var commandPending = false

    var link: RecoveryCenterState.Link { coreLink(connection) }

    /// The badge of a kind of change, from the core's word.
    static func change(_ kind: String) -> (letter: String, color: RGBA, name: String) {
        switch kind {
        case "Added": return ("A", Palette.green, "Added")
        case "Modified": return ("M", RGBA(0xFF9F0A), "Modified")
        case "Deleted": return ("D", Palette.red, "Deleted")
        case "TypeChanged": return ("T", RGBA(0xFF9F0A), "Type changed")
        default: return ("?", Palette.gray, kind)
        }
    }

    /// File name and directory of a path; "." for the root.
    static func split(_ path: String) -> (name: String, directory: String) {
        guard let slash = path.lastIndex(of: "/") else { return (path, ".") }
        return (String(path[path.index(after: slash)...]), String(path[..<slash]))
    }

    /// The scope the core gave a path, in words.
    static func scope(_ file: ChangedFile) -> (label: String, ok: Bool?) {
        switch file.scope {
        case "ALLOWED": return ("In scope", true)
        case "OUT_OF_SCOPE": return ("Out of scope", false)
        case "BASELINE": return ("Protected baseline", false)
        default: return ("Unknown", nil)
        }
    }

    /// The changed paths the core put outside the write scope or in the protected baseline.
    static func violations(in files: [ChangedFile]) -> [ChangedFile] {
        files.filter { scope($0).ok == false }
    }

    var violations: [ChangedFile] { Self.violations(in: files) }

    /// The diff split by file, with both line numbers read from the hunk headers.
    static func fileDiffs(_ diff: String, paths: [String]) -> [FileDiff] {
        var out: [FileDiff] = []
        var path: String?
        var rows: [DiffRow] = []
        var (old, new) = (0, 0)
        func flush() { if let path { out.append(FileDiff(path: path, rows: rows)) } }
        for line in diff.components(separatedBy: "\n") {
            if line.hasPrefix("diff --git ") {
                flush()
                path = paths.first { line == "diff --git a/\($0) b/\($0)" } ?? line.components(separatedBy: " b/").last
                rows = []
                continue
            }
            guard path != nil else { continue }
            if line.hasPrefix("@@") {
                let numbers = line.split(separator: " ").dropFirst().prefix(2).map { part in
                    Int(part.dropFirst().split(separator: ",").first ?? "") ?? 0
                }
                (old, new) = (numbers.first ?? 0, numbers.dropFirst().first ?? 0)
                rows.append(DiffRow(id: rows.count, kind: .hunk, old: nil, new: nil, text: line))
            } else if line.hasPrefix("+++") || line.hasPrefix("---") || line.hasPrefix("index ") || line.hasPrefix("new file")
                        || line.hasPrefix("deleted file") || line.hasPrefix("similarity") || line.hasPrefix("rename") {
                continue
            } else if line.hasPrefix("+") {
                rows.append(DiffRow(id: rows.count, kind: .added, old: nil, new: new, text: String(line.dropFirst())))
                new += 1
            } else if line.hasPrefix("-") {
                rows.append(DiffRow(id: rows.count, kind: .removed, old: old, new: nil, text: String(line.dropFirst())))
                old += 1
            } else if line.hasPrefix(" ") {
                rows.append(DiffRow(id: rows.count, kind: .context, old: old, new: new, text: String(line.dropFirst())))
                old += 1
                new += 1
            }
        }
        flush()
        return out
    }

    var diffs: [FileDiff] { Self.fileDiffs(diff, paths: files.map(\.path)) }

    var chosen: FileDiff? {
        let all = diffs
        return all.first { $0.path == chosenFile } ?? all.first
    }

    /// The loaded attempt's summary, when the core lists it.
    var attempt: AttemptSummary? { candidates.first { $0.id == result?.attempt } }

    /// "api · Attempt 1": where the candidate came from.
    var from: String {
        let key = detail?.key ?? attempt?.key ?? "Attempt"
        let ordinal = detail?.ordinal ?? attempt?.ordinal
        let running = attempt?.live == true ? " (running)" : ""
        return "\(key) · Attempt \(ordinal ?? 1)\(running)"
    }

    /// The header's last part: the task the candidate belongs to.
    static func placeTail(_ detail: AttemptDetail?) -> String? { detail?.key }

    /// The newest plan run of the held feature (any run when no feature is held).
    var run: ExternalRun? {
        external?.runs.first { scope.holds($0.feature) }
    }
    var push: ExternalAction? { run.flatMap { external?.newest(.Push, plan: $0.id) } }
    var pullRequest: ExternalAction? { run.flatMap { external?.newest(.PullRequest, plan: $0.id) } }

    /// Why a request cannot be sent now; nil when it can.
    func requestBlocked(_ kind: ExternalKind) -> String? {
        guard external != nil else { return externalLoad == .loading ? "Reading…" : "This core does not report push and pull requests." }
        guard let run else { return "The feature has no plan run yet. A plan branch is pushed when its run is done." }
        if link != .connected { return "Unavailable: the core is not connected." }
        if commandPending { return "Unavailable while a command awaits the core." }
        return kind == .Push ? run.pushRefusal : run.pullRequestRefusal
    }

    /// Local review, scope, push, pull request and location, as the board's Status card.
    var status: [Definition] {
        let violated = violations.count
        let scope: (String, Tone) = switch verdict {
        case .pass: ("In scope", .ok)
        case .fail: (violated == 1 ? "1 violation" : "\(violated) violations", .danger)
        case .unknown: ("Unknown", .neutral)
        case .notRun: ("Not checked", .neutral)
        }
        let decision = result?.decision ?? "NONE"
        let review: (String, Tone) = decision == "ACCEPTED" ? ("Accepted", .ok) : decision == "REJECTED" ? ("Rejected", .danger) : ("Not decided", .warn)
        return [
            Definition(key: "Decision", value: "", pill: review),
            Definition(key: "Scope", value: "", pill: scope),
            Definition(key: "Push", value: push.map { ExternalWords.commit($0.commit) } ?? "", pill: ExternalWords.pill(push)),
            Definition(key: "Pull request", value: "", pill: pullRequest == nil ? ("None", .neutral) : ExternalWords.pill(pullRequest)),
            Definition(key: "Location", value: push?.state == .Committed ? "Plan branch on \(push?.remote ?? "origin")"
                       : run == nil ? "Local candidate only" : "Local plan branch"),
        ]
    }

    /// The patterns of the approved scope, from the step's validation plan (1.9).
    var patterns: [String] { detail?.writeScope ?? [] }
}

struct SourceControlActions {
    var open: (Screen) -> Void = { _ in }
    var choose: (String) -> Void = { _ in }
    var chooseFile: (String) -> Void = { _ in }
    /// M5: asks for the push or the draft pull request of a plan; performs nothing.
    var request: (String, ExternalKind) -> Void = { _, _ in }
}

struct SourceControlScreen: View {
    @ObservedObject var model: AppModel
    let visible: Bool
    @Binding var layout: DiffLayout
    let open: (Screen) -> Void
    @State private var chosenFile: String?

    var body: some View {
        SourceControlView(state: state, actions: SourceControlActions(
            open: open,
            choose: { attempt in chosenFile = nil; model.workspace.loadCandidate(attempt) },
            chooseFile: { chosenFile = $0 },
            request: { model.workspace.requestExternal(plan: $0, kind: $1) }))
            .onAppear { if visible { read() } }
            .onChange(of: visible) { if visible { read() } }
            .onChange(of: model.workspace.snapshotSeq) { if visible { read() } }
            .onChange(of: model.workspace.candidate?.attempt) { chosenFile = nil; if visible { read() } }
    }

    private var state: SourceControlState {
        let workspace = model.workspace
        let scope = FeatureScope(workspace: workspace)
        let detail = workspace.attemptDetail?.id == workspace.candidate?.attempt ? workspace.attemptDetail : nil
        return SourceControlState(
            connection: model.client.connection, scope: scope,
            candidates: (workspace.attempts?.attempts ?? []).filter { $0.validation != nil && scope.holds($0.feature) },
            result: workspace.candidate, files: workspace.changedFiles, diff: workspace.diff, base: workspace.candidateBase,
            verdict: workspace.scope, detail: detail, layout: layout, chosenFile: chosenFile,
            reportsDetail: model.client.hello.map { $0.queries.contains("AttemptDetail") } ?? true,
            external: workspace.external, externalLoad: workspace.externalLoad,
            commandPending: workspace.commandPending)
    }

    /// The candidates; the newest is loaded when none is.
    private func read() {
        let workspace = model.workspace
        workspace.loadFeatures()
        workspace.loadAttempts()
        workspace.loadExternalActions()
        if workspace.candidate == nil, let newest = state.candidates.first { workspace.loadCandidate(newest.id) }
        if let attempt = workspace.candidate?.attempt { workspace.loadAttemptDetail(attempt) }
    }
}

struct SourceControlView: View {
    @Environment(\.theme) private var theme
    let state: SourceControlState
    var actions = SourceControlActions()

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 14) {
                CoreBanners(connection: state.connection, missing: state.reportsDetail ? nil : ("AttemptDetail", "candidate heads"))
                LifecycleCard(summary: state.scope.summary)
            }
            .padding(.horizontal, 10)
            InspectorColumns(trailingWidth: 320, wide: 900) {
                main
            } trailing: {
                inspector
            }
        }
        .padding(.top, 6)
    }

    // MARK: Main column

    @ViewBuilder
    private var main: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 10) {
                if !state.candidates.isEmpty {
                    ChoiceCapsule(options: state.candidates.prefix(6).map { a in
                        (a.id, "\(a.repositoryName ?? "repository") · \(a.key ?? "step") #\(a.ordinal ?? 1)")
                    }, selection: state.result?.attempt, label: "Repository") { actions.choose($0) }
                }
                Spacer(minLength: 0)
            }
            if let result = state.result {
                FactStrip(facts: [
                    ("Branch", "Not reported", false),
                    ("Base", state.detail?.base.map { String($0.prefix(7)) } ?? (state.base.isEmpty ? "Not reported" : String(state.base.prefix(7))), true),
                    ("Candidate", state.detail?.candidateHead.map { String($0.prefix(7)) } ?? result.candidate.map { String($0.prefix(7)) } ?? "None", true),
                    ("From", state.from, false),
                ])
                FlowRow(spacing: 14) {
                    files.frame(minWidth: 300, maxWidth: .infinity)
                    LabeledSection(title: "Status") { DefinitionCard(rows: state.status, keyWidth: 96) }
                        .frame(minWidth: 240, maxWidth: 320)
                }
                if !state.violations.isEmpty {
                    let first = state.violations[0]
                    TintedNotice(tone: .warn,
                                 title: state.violations.count == 1 ? "1 change is outside the approved scope"
                                     : "\(state.violations.count) changes are outside the approved scope",
                                 message: "\(first.path) was changed by \(state.from). Review it before accepting the candidate.") {
                        Button("View Scope Violations") { actions.chooseFile(first.path) }.buttonStyle(.jojo(.secondary, compact: true))
                        Button("Revert file") {}.buttonStyle(.jojo(.ghost, compact: true)).disabled(true)
                            .help("Not available yet: the core has no command to revert a path.")
                    }
                }
                diffCard
            } else {
                Placeholder(symbol: "arrow.triangle.branch", tile: Palette.blue, title: "No candidate loaded",
                            message: "A candidate appears here when an attempt of a plan step seals one.") {
                    Button("Open Agent Runs") { actions.open(.agentRuns) }.buttonStyle(.jojo(.secondary, compact: true))
                }
                .frame(maxWidth: .infinity)
                .padding(24)
                .cardElevation(radius: 18)
            }
        }
    }

    private var files: some View {
        LabeledSection(title: "Changed files · \(state.files.count)") {
            VStack(spacing: 0) {
                if state.files.isEmpty { NotReported("No changed paths.").padding(12) }
                ForEach(Array(state.files.enumerated()), id: \.offset) { index, file in
                    let change = SourceControlState.change(file.kind)
                    let split = SourceControlState.split(file.path)
                    let counts = state.diffs.first { $0.path == file.path }?.counts
                    Button { actions.chooseFile(file.path) } label: {
                        HStack(spacing: 10) {
                            Text(change.letter).font(.system(size: 12, weight: .bold, design: .monospaced))
                                .foregroundStyle(change.color.color).frame(width: 18)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(split.name).textStyle(.bodyStrong).lineLimit(1)
                                Text(split.directory).font(.system(size: 11, design: .monospaced)).foregroundStyle(theme.text3).lineLimit(1)
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            if let counts { Text(counts).font(.system(size: 11, design: .monospaced)).foregroundStyle(theme.text2) }
                            if SourceControlState.scope(file).ok == false {
                                OperationalPill(SourceControlState.scope(file).label, tone: .danger)
                            }
                        }
                        .padding(EdgeInsets(top: 8, leading: 12, bottom: 8, trailing: 12))
                        .background(state.chosen?.path == file.path ? theme.accentTint : .clear)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .overlay(alignment: .top) { if index > 0 { Rectangle().fill(theme.sep).frame(height: 0.5) } }
                }
            }
            .cardElevation(radius: 14)
        }
    }

    @ViewBuilder
    private var diffCard: some View {
        if let file = state.chosen {
            let head = state.detail?.candidateHead.map { String($0.prefix(7)) } ?? "candidate"
            let base = String((state.detail?.base ?? state.base).prefix(7))
            VStack(alignment: .leading, spacing: 0) {
                HStack(spacing: 10) {
                    Text(file.path).font(.system(size: 12, weight: .semibold, design: .monospaced)).lineLimit(1)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    Text("\(base) → \(head)").textStyle(.mono).foregroundStyle(theme.text2)
                }
                .padding(EdgeInsets(top: 8, leading: 12, bottom: 8, trailing: 12))
                Rectangle().fill(theme.sep).frame(height: 0.5)
                if state.layout == .unified { unified(file) } else { sideBySide(file) }
            }
            .cardElevation(radius: 14)
            .clipShape(RoundedRectangle(cornerRadius: 14))
        } else if !state.files.isEmpty {
            NotReported(state.diff.isEmpty ? "Reading the diff…" : "The diff names none of the changed paths.")
        }
    }

    private static let lineLimit = 400

    private func unified(_ file: FileDiff) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(file.rows.prefix(Self.lineLimit)) { row in
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    number(row.old).frame(width: 36, alignment: .trailing)
                    number(row.new).frame(width: 36, alignment: .trailing)
                    Text(row.kind == .added ? "+" : row.kind == .removed ? "-" : " ").frame(width: 12)
                    Text(row.text).frame(maxWidth: .infinity, alignment: .leading)
                }
                .font(.system(size: 12, design: .monospaced))
                .foregroundStyle(row.kind == .hunk ? theme.text3 : theme.text)
                .padding(EdgeInsets(top: 1, leading: 10, bottom: 1, trailing: 10))
                .background(tint(row.kind))
            }
            if file.rows.count > Self.lineLimit { NoteLine("\(file.rows.count - Self.lineLimit) more lines are not drawn.").padding(10) }
        }
        .padding(.vertical, 6)
        .textSelection(.enabled)
    }

    private func sideBySide(_ file: FileDiff) -> some View {
        let rows = Array(file.rows.prefix(Self.lineLimit))
        return HStack(alignment: .top, spacing: 0) {
            column(rows.filter { $0.kind != .added }, number: \.old)
            Rectangle().fill(theme.sep).frame(width: 0.5)
            column(rows.filter { $0.kind != .removed }, number: \.new)
        }
        .padding(.vertical, 6)
        .textSelection(.enabled)
    }

    private func column(_ rows: [DiffRow], number path: KeyPath<DiffRow, Int?>) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(rows) { row in
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    number(row[keyPath: path]).frame(width: 30, alignment: .trailing)
                    Text(row.text).frame(maxWidth: .infinity, alignment: .leading)
                }
                .font(.system(size: 12, design: .monospaced))
                .foregroundStyle(row.kind == .hunk ? theme.text3 : theme.text)
                .padding(EdgeInsets(top: 1, leading: 10, bottom: 1, trailing: 10))
                .background(tint(row.kind))
            }
        }
        .frame(maxWidth: .infinity, alignment: .topLeading)
    }

    private func number(_ n: Int?) -> some View {
        Text(n.map(String.init) ?? "").foregroundStyle(theme.text3)
    }

    private func tint(_ kind: DiffRow.Kind) -> Color {
        switch kind {
        case .added: return theme.greenBg
        case .removed: return theme.redBg
        default: return .clear
        }
    }

    // MARK: Inspector

    private var inspector: some View {
        GlassInspector {
            LabeledSection(title: "Scope validation") {
                let inside = state.files.filter { SourceControlState.scope($0).ok == true }.count
                DefinitionCard(rows: [
                    Definition(key: "Approved scope", value: state.patterns.isEmpty ? "Not reported"
                               : "\(state.patterns.count) \(state.patterns.count == 1 ? "path" : "paths")\n" + state.patterns.joined(separator: "\n"),
                               mono: !state.patterns.isEmpty),
                    Definition(key: "Inside scope", value: "\(inside) of \(state.files.count)"),
                    Definition(key: "Unexpected", value: "\(state.violations.count)"),
                ], keyWidth: 100)
            }
            LabeledSection(title: "External actions") {
                actionCard(.Push, title: state.run.map { "Push \(ExternalWords.branch($0.branch))" } ?? "Push the plan branch",
                           action: state.push)
                actionCard(.PullRequest, title: state.pullRequest?.title ?? "Draft pull request", action: state.pullRequest)
                Text("Each external action is requested here and authorized on its own in the Approval Inbox, for an exact ref and commit, recorded with an idempotency key and never retried.")
                    .font(.system(size: 11)).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
            }
            LabeledSection(title: "CI") {
                DefinitionCard(rows: [Definition(key: "Plan branch", value: "",
                                                 pill: state.push?.state == .Committed ? ("Pushed", .ok) : ("Not pushed", .neutral)),
                                      Definition(key: "Pipelines", value: "Not reported")], keyWidth: 100)
            }
        }
    }

    /// One external action: its state from the core, and what the human can do next here.
    private func actionCard(_ kind: ExternalKind, title: String, action: ExternalAction?) -> some View {
        let pill = ExternalWords.pill(action)
        let blocked = state.requestBlocked(kind)
        let request = kind == .Push ? "Request Push" : "Request Draft PR"
        return VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 6) {
                Text(title).textStyle(.bodyStrong).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                OperationalPill(pill.0, tone: pill.1)
            }
            switch action?.state {
            case .Requested?:
                Button("Authorize in Inbox") { actions.open(.approvalInbox) }.buttonStyle(.jojo(.secondary, compact: true))
                note("Waits for its own authorization in the Approval Inbox.")
            case .InFlight?:
                note(kind == .Push ? "Pushing once; the outcome is recorded when git answers." : "Opening once; the outcome is recorded when gh answers.")
            case .Unknown?:
                TextLink("Inspect in Recovery") { actions.open(.recoveryCenter) }.font(.system(size: 12))
                note("Issued once; its outcome was not observed. Jojo never sends it again.")
            case .Committed?:
                if kind == .Push, let a = action {
                    note("\(a.commit.prefix(12)) is on \(a.remote) as \(ExternalWords.branch(a.branch)).")
                } else if let url = action?.pullRequestURL {
                    Text(url).textStyle(.mono).foregroundStyle(theme.text2).textSelection(.enabled).lineLimit(2)
                }
            case .Rejected?, .Declined?, nil:
                if let detail = action?.detail.nonEmpty, action?.state == .Rejected { note(detail) }
                Button(request) {
                    if let plan = state.run?.id { actions.request(plan, kind) }
                }
                .buttonStyle(.jojo(.secondary, compact: true)).disabled(blocked != nil)
                .help(blocked ?? "Asks for it; nothing is sent until you authorize it in the Approval Inbox.")
                if let blocked { note(blocked.prefix(1).uppercased() + blocked.dropFirst() + ".") }
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .cardElevation(radius: 12)
    }

    private func note(_ text: String) -> some View {
        Text(text).textStyle(.secondary).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
    }
}

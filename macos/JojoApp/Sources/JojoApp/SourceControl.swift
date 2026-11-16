import JojoCore
import SwiftUI

// Source Control (docs/design/screens/SOURCE_CONTROL.md): the changed paths, the write scope
// and the diff of the one attempt result the workspace holds. It has no load of its own.

/// What the screen shows. Every field is the helper's answer; nothing is inferred here.
struct SourceControlState: Equatable {
    /// The loaded attempt result, the one Verification & Evidence shows.
    var loaded: VerificationState
    /// The base the changed paths were compared with; empty until they arrive.
    var base: String

    /// The badge of a kind of change, from the core's word.
    static func change(_ kind: String) -> (letter: String, tone: Tone, name: String) {
        switch kind {
        case "Added": return ("A", .ok, "Added")
        case "Modified": return ("M", .warn, "Modified")
        case "Deleted": return ("D", .danger, "Deleted")
        case "TypeChanged": return ("T", .warn, "Type changed")
        default: return ("?", .neutral, kind)
        }
    }

    /// File name and directory of a path; "." for the root.
    static func split(_ path: String) -> (name: String, directory: String) {
        guard let slash = path.lastIndex(of: "/") else { return (path, ".") }
        return (String(path[path.index(after: slash)...]), String(path[..<slash]))
    }

    static func short(_ identity: String) -> String {
        String(identity.prefix(12))
    }

    /// One file's part of the diff, from its "diff --git" line to the next.
    struct Section: Equatable {
        /// The changed path the header names; `nil` when it names none of them.
        let path: String?
        let lines: [DiffLine]

        var added: Int { lines.filter { $0.kind == .added }.count }
        var removed: Int { lines.filter { $0.kind == .removed }.count }

        /// "+48 −6"; `nil` when the section has no counted line.
        var counts: String? {
            let parts = [added > 0 ? "+\(added)" : nil, removed > 0 ? "−\(removed)" : nil].compactMap { $0 }
            return parts.isEmpty ? nil : parts.joined(separator: " ")
        }
    }

    static func sections(of lines: [DiffLine], paths: [String]) -> [Section] {
        let headers = Dictionary(paths.map { ("diff --git a/\($0) b/\($0)", $0) }, uniquingKeysWith: { first, _ in first })
        var sections: [Section] = []
        var (path, current): (String?, [DiffLine]) = (nil, [])
        for line in lines {
            if line.kind == .header, line.text.hasPrefix("diff --git ") {
                if !current.isEmpty { sections.append(Section(path: path, lines: current)) }
                (path, current) = (headers[line.text], [])
            }
            current.append(line)
        }
        if !current.isEmpty { sections.append(Section(path: path, lines: current)) }
        return sections
    }

    var sections: [Section] {
        Self.sections(of: DiffLine.parse(loaded.diff), paths: loaded.changedFiles.map(\.path))
    }

    /// The changed paths the core put outside the write scope or in the protected baseline.
    static func violations(in files: [ChangedFile]) -> [ChangedFile] {
        files.filter { VerificationState.scope($0).state == .failed }
    }

    var violations: [ChangedFile] { Self.violations(in: loaded.changedFiles) }

    /// The write scope in the status strip: the core's verdict, with the number of violations.
    var scopePill: (state: EvidenceState, label: String?) {
        switch loaded.scope {
        case .pass: return (.passed, "In scope")
        case .fail:
            let count = violations.count
            return (.failed, count == 0 ? nil : count == 1 ? "1 violation" : "\(count) violations")
        case .unknown: return (.unknown, nil)
        case .notRun: return (.notRun, nil)
        }
    }

    struct Notice: Equatable {
        /// Danger for a failed scope, warning for an unknown one.
        let danger: Bool
        let title: String
        let message: String
        /// The path "View Scope Violations" chooses.
        var first: String?
    }

    /// The banner under the status strip: a failed or an unknown write scope.
    var scopeNotice: Notice? {
        switch loaded.scope {
        case .fail:
            let paths = violations.map(\.path)
            let verdict = "The core's verdict on the write scope is Failed."
            guard let first = paths.first else {
                return Notice(danger: true, title: "The write scope check failed", message: "No listed path is marked. " + verdict)
            }
            let named = paths.prefix(3).joined(separator: ", ") + (paths.count > 3 ? " and \(paths.count - 3) more" : "")
            return Notice(danger: true,
                          title: paths.count == 1 ? "1 change is outside the approved scope"
                              : "\(paths.count) changes are outside the approved scope",
                          message: "\(named). \(verdict) There is no command to revert a path.", first: first)
        case .unknown where loaded.candidate?.candidate != nil:
            let reason = loaded.scopeDetail.isEmpty ? "" : " (\(loaded.scopeDetail))"
            return Notice(danger: false, title: "The write scope is unknown",
                          message: "The core gave no verdict\(reason). Unknown is never shown as in scope.")
        case .pass, .unknown, .notRun:
            return nil
        }
    }

    struct ScopeFact: Identifiable, Equatable {
        let name: String
        var note: String?
        let state: EvidenceState
        var label: String?
        var id: String { name }
    }

    /// The lines of the "Scope validation" card.
    var scopeFacts: [ScopeFact] {
        let files = loaded.changedFiles
        var facts = [ScopeFact(name: "Write scope", note: loaded.scopeDetail.isEmpty ? nil : loaded.scopeDetail,
                               state: VerificationState.evidence(loaded.scope))]
        guard !files.isEmpty else { return facts }
        func count(_ scope: String) -> Int { files.filter { VerificationState.scope($0).label == scope }.count }
        let (inside, outside, baseline, unknown) = (count("In scope"), count("Out of scope"), count("Protected baseline"), count("Unknown"))
        facts.append(ScopeFact(name: "Paths inside scope", state: inside > 0 ? .passed : .notRun, label: "\(inside) of \(files.count)"))
        facts.append(ScopeFact(name: "Outside the approved scope", state: outside > 0 ? .failed : .notRun,
                               label: outside > 0 ? "\(outside)" : "None"))
        if baseline > 0 { facts.append(ScopeFact(name: "Protected baseline", state: .failed, label: "\(baseline)")) }
        if unknown > 0 { facts.append(ScopeFact(name: "Scope unknown", state: .unknown, label: "\(unknown)")) }
        return facts
    }

    /// The two ends of the diff, shortened: "9c1e4b2a71f0 → c4c4c4c4c4c4".
    var ends: String {
        guard !base.isEmpty, let candidate = loaded.candidate?.candidate else { return "base → sealed candidate" }
        return "\(Self.short(base)) → \(Self.short(candidate))"
    }
}

struct SourceControlScreen: View {
    @ObservedObject var model: AppModel
    let open: (Screen) -> Void

    var body: some View {
        let workspace = model.workspace
        SourceControlView(
            state: SourceControlState(
                loaded: VerificationState(connection: model.client.connection, admission: workspace.admission,
                                          candidate: workspace.candidate, changedFiles: workspace.changedFiles,
                                          scope: workspace.scope, scopeDetail: workspace.scopeDetail, diff: workspace.diff,
                                          log: workspace.agentLog, report: workspace.validationReport,
                                          commandPending: workspace.commandPending),
                base: workspace.candidateBase),
            open: open)
    }
}

struct SourceControlView: View {
    @Environment(\.theme) private var theme
    let state: SourceControlState
    var open: (Screen) -> Void = { _ in }
    /// The changed path whose section of the diff is shown; `nil` shows the whole diff.
    @State private var selected: String?
    @State private var expanded: Bool

    init(state: SourceControlState, open: @escaping (Screen) -> Void = { _ in }, selected: String? = nil, expanded: Bool = false) {
        self.state = state
        self.open = open
        _selected = State(initialValue: selected)
        _expanded = State(initialValue: expanded)
    }

    var body: some View {
        GeometryReader { geometry in
            // The main column keeps at least 460 beside each panel.
            let columns = geometry.size.width >= 1020 ? 3 : geometry.size.width >= 710 ? 2 : 1
            let sections = state.sections
            HStack(spacing: 0) {
                if columns > 1, let c = state.loaded.candidate {
                    panel(edge: .trailing, width: 250) { leading(c, sections) }
                }
                Scrolling {
                    VStack(alignment: .leading, spacing: 12) {
                        banners
                        if let c = state.loaded.candidate {
                            if columns == 1 { Card(.panelList) { leading(c, sections) } }
                            strip(c)
                            scopeBanner
                            diff(sections)
                            if columns < 3 { trailing(c).padding(.top, 6) }
                        } else {
                            placeholder.frame(minHeight: 260)
                        }
                    }
                    .padding(EdgeInsets(top: 14, leading: 18, bottom: 18, trailing: 18))
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                if columns == 3, let c = state.loaded.candidate {
                    panel(edge: .leading, width: 310) { trailing(c).padding(14) }
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        // A choice belongs to the attempt it was made in.
        .onChange(of: state.loaded.candidate?.attempt) { _, _ in
            (selected, expanded) = (nil, false)
        }
    }

    private func panel(edge: Alignment, width: CGFloat, @ViewBuilder _ content: @escaping () -> some View) -> some View {
        Scrolling(content: content)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            .background(theme.sidebar)
            .overlay(alignment: edge) { Rectangle().fill(theme.sidebarEdge).frame(width: 1) }
            .frame(width: width)
    }

    @ViewBuilder
    private var banners: some View {
        switch state.loaded.link {
        case .lost:
            Banner(.danger, title: "Jojo core isn't responding",
                   message: "Showing the last known state, read-only. No actions can be requested.")
        case .incompatible:
            Banner(.danger, title: "Jojo core speaks another protocol version",
                   message: "Read-only. No actions can be requested.")
        case .connected, .starting:
            EmptyView()
        }
        if state.loaded.admissionClosed {
            Banner(.danger, title: "Execution admission is closed",
                   message: "No new attempt starts until you review the inspection.") {
                Button("Open Recovery Center") { open(.recoveryCenter) }.buttonStyle(.jojo(.secondary, compact: true))
            }
        }
    }

    private var placeholder: some View {
        StateBlock(symbol: "arrow.triangle.branch", title: "No attempt loaded",
                   message: "Source Control shows the changed paths, the write scope and the diff of the loaded attempt. Choose one in Agent Runs and use Load Result, or load it by its identity in Verification & Evidence.") {
            Button("Open Agent Runs") { open(.agentRuns) }.buttonStyle(.jojo(.secondary, compact: true))
            Button("Open Verification & Evidence") { open(.verification) }.buttonStyle(.jojo(.secondary, compact: true))
        }
    }

    // MARK: Leading panel

    private func leading(_ c: CandidateReview, _ sections: [SourceControlState.Section]) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 10) {
                LabeledField("Branch") { fact(nil, missing: "Not reported") }
                HStack(alignment: .top, spacing: 14) {
                    LabeledField("Base") { fact(state.base.isEmpty ? nil : state.base, missing: "Not loaded") }
                    LabeledField("Candidate") { fact(c.candidate, missing: "None") }
                }
                LabeledField("From") {
                    Button("Attempt \(SourceControlState.short(c.attempt))") { open(.agentRuns) }
                        .buttonStyle(.plain).font(.system(size: 12)).foregroundStyle(theme.icon)
                        .help("Attempt \(c.attempt). Opens Agent Runs.")
                }
                SectionHeading("Changed files · \(state.loaded.changedFiles.count)").padding(.top, 4)
            }
            .padding(EdgeInsets(top: 12, leading: 14, bottom: 8, trailing: 14))
            files(sections)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    /// An identity shortened to twelve characters, or the words that say why there is none.
    @ViewBuilder
    private func fact(_ identity: String?, missing: String) -> some View {
        if let identity {
            Text(SourceControlState.short(identity)).textStyle(.mono).foregroundStyle(theme.text).help(identity)
        } else {
            Text(missing).textStyle(.meta).foregroundStyle(theme.text3)
        }
    }

    @ViewBuilder
    private func files(_ sections: [SourceControlState.Section]) -> some View {
        if state.loaded.changedFiles.isEmpty {
            Text(state.loaded.noFiles).textStyle(.secondary).foregroundStyle(theme.text3)
                .padding(.horizontal, 14)
                .padding(.bottom, 12)
        } else {
            VStack(spacing: 2) {
                ForEach(state.loaded.changedFiles) { file in
                    Button { selected = selected == file.path ? nil : file.path } label: {
                        row(file, sections.first { $0.path == file.path })
                    }
                    .buttonStyle(.plain)
                    .help(file.path)
                }
            }
            .padding(.horizontal, 8)
            .padding(.bottom, 12)
        }
    }

    private func row(_ file: ChangedFile, _ section: SourceControlState.Section?) -> some View {
        let (name, directory) = SourceControlState.split(file.path)
        let change = SourceControlState.change(file.kind)
        let scope = VerificationState.scope(file)
        return ChangedFileRow(name: name, directory: directory,
                              badge: ChangeBadge(change.letter, tone: change.tone, name: change.name),
                              counts: section?.counts, scope: scope.state == .passed ? nil : scope,
                              flagged: scope.state == .failed, selected: selected == file.path)
    }

    // MARK: Main column

    private func strip(_ c: CandidateReview) -> some View {
        let scope = state.scopePill
        return Card(CardShape(radius: 10, vertical: 10, horizontal: 14)) {
            HStack(alignment: .top, spacing: 26) {
                WrappingRow(spacing: 26, lineSpacing: 10) {
                    LabeledField("Decision") {
                        Text(VerificationState.decision(c)).textStyle(.control).foregroundStyle(theme.text)
                    }
                    LabeledField("Write scope") { EvidencePill(scope.state, label: scope.label) }
                    LabeledField("Host validation") { EvidencePill(VerificationState.evidence(c.validation)) }
                }
                Spacer(minLength: 0)
                LabeledField("Location") {
                    HStack(spacing: 6) {
                        Image(systemName: "internaldrive").font(.system(size: 11)).foregroundStyle(theme.text3)
                        Text("Local candidate").textStyle(.control).foregroundStyle(theme.text)
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }

    @ViewBuilder
    private var scopeBanner: some View {
        if let notice = state.scopeNotice {
            if let first = notice.first {
                Banner(.danger, title: notice.title, message: notice.message) {
                    Button("View Scope Violations") { selected = first }.buttonStyle(.jojo(.secondary, compact: true))
                        .help("Shows the diff of the first path outside the scope.")
                }
            } else {
                Banner(notice.danger ? .danger : .warning, title: notice.title, message: notice.message)
            }
        }
    }

    private func diff(_ sections: [SourceControlState.Section]) -> some View {
        // A choice that is no longer among the changed paths shows the whole diff.
        let path = selected.flatMap { choice in state.loaded.changedFiles.contains { $0.path == choice } ? choice : nil }
        let lines = path.map { choice in sections.first { $0.path == choice }?.lines ?? [] } ?? sections.flatMap(\.lines)
        let limit = VerificationState.lineLimit
        return VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                if let path {
                    Text(path).font(.system(size: 12, design: .monospaced)).foregroundStyle(theme.text)
                        .lineLimit(1).truncationMode(.head).textSelection(.enabled)
                } else {
                    Text("All changed files").textStyle(.secondary).fontWeight(.semibold).foregroundStyle(theme.text2)
                }
                Spacer(minLength: 8)
                Text(state.ends).textStyle(.mono).foregroundStyle(theme.text3).lineLimit(1)
                    .help("From the base to the sealed candidate.")
                if path != nil {
                    Button("Show All Files") { selected = nil }.buttonStyle(.jojo(.ghost, compact: true))
                }
                if lines.count > limit, !expanded {
                    Button("Show All \(lines.count) Lines") { expanded = true }.buttonStyle(.jojo(.ghost, compact: true))
                        .help("The first \(limit) lines are drawn.")
                }
            }
            .frame(minHeight: 22)
            if lines.isEmpty {
                Card(.panelList) {
                    Text(path == nil ? "No diff loaded." : "The loaded diff has no section for this path.")
                        .textStyle(.secondary).foregroundStyle(theme.text3)
                        .padding(.vertical, 10)
                        .padding(.horizontal, 14)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            } else {
                DiffView(expanded ? lines : Array(lines.prefix(limit)))
            }
        }
    }

    // MARK: Trailing panel

    private func trailing(_ c: CandidateReview) -> some View {
        VStack(alignment: .leading, spacing: 18) {
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Scope validation")
                Card(.panelList) {
                    Rows(state.scopeFacts) { fact in
                        EvidenceRow(fact.name, note: fact.note, state: fact.state, label: fact.label)
                    }
                }
                Footnote("The patterns of the approved scope are not reported; the core gives one verdict for the candidate and a scope for each path. There is no command to revert a path or to widen the scope.")
            }
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Review")
                Card(.panel) {
                    VStack(alignment: .leading, spacing: 10) {
                        Text("Validation, accept and reject are on Verification & Evidence.")
                            .textStyle(.body).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
                        FactGrid {
                            Fact("Validation") { EvidencePill(VerificationState.evidence(c.validation)) }
                            Fact("Decision", VerificationState.decision(c))
                        }
                        Button("Open Verification & Evidence") { open(.verification) }.buttonStyle(.jojo(.secondary))
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("External actions")
                Card(.panel) {
                    Text("Jojo has not pushed or published this candidate. The core has no push or pull request command yet.")
                        .textStyle(.body).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                Footnote("Not reported: the branch and commit of the candidate, external actions and their outcomes, pull requests and CI results.")
            }
        }
    }
}

import JojoCore
import SwiftUI

// Specifications (docs/design/screens/SPECIFICATIONS.md), from `SpecificationMac.dc.html`:
// the held feature's documents, one revision's text in sections or compared with the one
// before it, and in the inspector what an approval covers, the decisions on the document,
// its findings and its acceptance criteria. Protocol 1.9 (`Document`). A decision is sent
// with the Approval Inbox's `decide` for the review both screens share.

/// Document or Compare, the header's segmented control.
enum SpecificationMode: String, CaseIterable, Identifiable, Sendable {
    case document = "Document", compare = "Compare"
    var id: String { rawValue }
}

/// One section of a document's rendered text: its heading and its lines.
struct TextSection: Equatable, Identifiable {
    let heading: String
    let lines: [String]
    var id: String { heading }
}

/// One line of a comparison: unchanged, removed (only in the earlier revision) or added.
struct ComparedLine: Equatable {
    enum Kind: Equatable { case same, removed, added }
    let kind: Kind
    let text: String
}

/// What the screen shows. Every value is the helper's answer.
struct SpecificationsState: Equatable {
    var connection: HelperConnection
    var admission: String
    var scope = FeatureScope()
    /// Protocol 1.6: the documents and what waits; nil from an older helper.
    var documents: DocumentList?
    /// The review the Approval Inbox shares; a decision is sent for it.
    var review: ReviewBundle?
    var reviewStale = false
    var decisionOutstanding = false
    /// Protocol 1.9: the chosen revision of the chosen document.
    var detail: DocumentDetail?
    var detailLoad = PlanLoad.idle
    /// The revision before it, for Compare.
    var comparison: DocumentDetail?
    var mode = SpecificationMode.document
    /// Whether the core lists `Document` in its Hello answer.
    var reportsDetail = true
    var now = Date()

    var link: RecoveryCenterState.Link { coreLink(connection) }

    /// The held feature's documents, newest first; every document when none is held.
    var listed: [DocumentSummary] {
        (documents?.documents ?? []).filter { scope.holds($0.feature) }
    }

    /// The pending documents of the held feature: the sidebar counter.
    static func proposed(_ list: DocumentList?, scope: FeatureScope) -> Int {
        (list?.waiting ?? []).filter { scope.holds($0.feature) }.count
    }

    var chosen: DocumentSummary? {
        guard let id = detail?.id ?? review?.subject else { return nil }
        return documents?[id]
    }

    /// "ADR · Slugify from the shell", the header's title.
    var title: String {
        guard let detail else { return "Specifications" }
        return "\(documentCode(detail.kind)) · \(detail.title ?? detail.featureName ?? ApprovalInboxState.short(detail.id))"
    }

    /// The chosen revision is the newest and its review is the one loaded and current.
    var decidable: Bool {
        guard let detail, let review else { return false }
        return review.subject == detail.id && review.revision == detail.newest && detail.revision == detail.newest
    }

    /// Why Approve Revision is disabled; nil when it can be sent.
    var approveBlocked: String? {
        guard let detail else { return "No document chosen." }
        if link != .connected { return "Unavailable: the core is not connected." }
        if detail.revision != detail.newest { return "Only the newest revision can be approved." }
        if detail.approval == "Approved" && !detail.stale { return "This revision is approved." }
        if detail.approval == "Waiting" { return "Waits for the documents it was drafted on." }
        if detail.approval == "NotReady" { return "The requirements have findings; the core does not accept an approval yet." }
        if !decidable { return "Loading the review of this revision." }
        if decisionOutstanding { return "Unavailable while a decision awaits the core." }
        if reviewStale { return "Unavailable: the review is stale. It is read again." }
        return nil
    }

    /// The document's text in sections: each Markdown heading starts one; text before the
    /// first heading is "Summary".
    static func sections(_ text: String) -> [TextSection] {
        var out: [TextSection] = []
        var heading = "Summary"
        var lines: [String] = []
        func flush() {
            let body = lines.drop { $0.trimmingCharacters(in: .whitespaces).isEmpty }
            if !body.isEmpty || heading != "Summary" { out.append(TextSection(heading: heading, lines: Array(body))) }
        }
        for line in text.components(separatedBy: "\n") {
            if line.hasPrefix("#") {
                flush()
                heading = line.drop { $0 == "#" }.trimmingCharacters(in: .whitespaces)
                lines = []
            } else {
                lines.append(line)
            }
        }
        flush()
        // A heading repeated in the text keeps its own place.
        var seen: [String: Int] = [:]
        return out.map { s in
            let n = seen[s.heading, default: 0]
            seen[s.heading] = n + 1
            return n == 0 ? s : TextSection(heading: "\(s.heading) (\(n + 1))", lines: s.lines)
        }
    }

    /// The lines of two texts in order, each unchanged, removed or added (longest common
    /// subsequence of non-empty lines).
    static func compare(_ old: [String], _ new: [String]) -> [ComparedLine] {
        let a = old.filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
        let b = new.filter { !$0.trimmingCharacters(in: .whitespaces).isEmpty }
        var table = Array(repeating: Array(repeating: 0, count: b.count + 1), count: a.count + 1)
        for i in stride(from: a.count - 1, through: 0, by: -1) {
            for j in stride(from: b.count - 1, through: 0, by: -1) {
                table[i][j] = a[i] == b[j] ? table[i + 1][j + 1] + 1 : max(table[i + 1][j], table[i][j + 1])
            }
        }
        var (i, j, out) = (0, 0, [ComparedLine]())
        while i < a.count || j < b.count {
            if i < a.count, j < b.count, a[i] == b[j] {
                out.append(ComparedLine(kind: .same, text: a[i])); i += 1; j += 1
            } else if j < b.count, i == a.count || table[i][j + 1] >= table[i + 1][j] {
                out.append(ComparedLine(kind: .added, text: b[j])); j += 1
            } else {
                out.append(ComparedLine(kind: .removed, text: a[i])); i += 1
            }
        }
        return out
    }

    /// Each section of either revision with its compared lines, and how many changed.
    var compared: [(heading: String, lines: [ComparedLine])] {
        guard let detail, let comparison else { return [] }
        let old = Self.sections(comparison.text)
        let new = Self.sections(detail.text)
        var headings = new.map(\.heading)
        for h in old.map(\.heading) where !headings.contains(h) { headings.append(h) }
        return headings.map { h in
            (h, Self.compare(old.first { $0.heading == h }?.lines ?? [], new.first { $0.heading == h }?.lines ?? []))
        }
    }

    var changedSections: Int { compared.filter { $0.lines.contains { $0.kind != .same } }.count }

    /// What an approval of this kind lets the core do, and what it never does.
    static func authorizes(_ detail: DocumentDetail) -> (yes: String, no: String) {
        let drafted = detail.downstream.map { "\(documentCode($0.kind)) rev \($0.revision ?? 1)" }
        let on = drafted.isEmpty ? "" : " (\(drafted.joined(separator: ", ")))"
        switch detail.kind {
        case "Prd":
            return ("Planning may build on this revision\(on).", "Start tasks, push, deploy, or carry over to a changed revision")
        case "Adr":
            return ("Plans drafted on it may use this decision\(on).", "Start tasks, push, deploy, or carry over to a changed revision")
        case "ExecutionPlan":
            return ("Its steps may be admitted and run.", "Push, deploy, or approve the candidates its steps produce")
        default:
            return ("Host validation of its steps' candidates checks against it.", "Accept a candidate, push or deploy")
        }
    }

    static func findingText(_ f: DocumentFinding) -> String {
        switch f.problem {
        case "Empty": return "Required text or list is empty."
        case "Placeholder": return "Marks unfinished work (TODO, TBD)."
        case "UnknownReference": return "Names an actor or criterion the document does not define."
        case "OpenBlocker": return "Still lists an unresolved blocker."
        default: return f.problem
        }
    }

    /// The prefilled note of Request Changes.
    var changeRequest: String {
        guard let detail else { return "" }
        return "Changes requested on \(documentCode(detail.kind)) rev \(detail.revision): "
    }
}

struct SpecificationsActions {
    var open: (Screen) -> Void = { _ in }
    /// Chooses a document (its newest revision) and loads its review.
    var choose: (String) -> Void = { _ in }
    var chooseRevision: (String, Int64) -> Void = { _, _ in }
    var approve: () -> Void = {}
    var requestChanges: () -> Void = {}
    var reload: () -> Void = {}
}

struct SpecificationsScreen: View {
    @ObservedObject var model: AppModel
    let visible: Bool
    @Binding var mode: SpecificationMode
    let open: (Screen) -> Void
    @State private var revision: Int64?

    var body: some View {
        SpecificationsView(state: state, actions: actions)
            .onAppear { if visible { read() } }
            .onChange(of: visible) { if visible { read() } }
            .onChange(of: model.workspace.snapshotSeq) { if visible { read() } }
            .onChange(of: model.workspace.review?.subject) { revision = nil; if visible { read() } }
            .onChange(of: mode) { if visible { read() } }
    }

    private var state: SpecificationsState {
        let workspace = model.workspace
        return SpecificationsState(
            connection: model.client.connection, admission: workspace.admission, scope: FeatureScope(workspace: workspace),
            documents: workspace.documents, review: workspace.review, reviewStale: workspace.reviewStale,
            decisionOutstanding: model.client.outstandingCommands.values.contains("HumanDecision"),
            detail: workspace.document, detailLoad: workspace.documentLoad, comparison: workspace.comparison, mode: mode,
            reportsDetail: model.client.hello.map { $0.queries.contains("Document") } ?? true)
    }

    /// The chosen document's revision and, for Compare, the one before it; a document is
    /// chosen when none is.
    private func read() {
        let workspace = model.workspace
        workspace.loadFeatures()
        let current = state
        if workspace.review == nil, let first = current.listed.first(where: { $0.approval == "Pending" }) ?? current.listed.first {
            workspace.loadReview(first.id)
            return
        }
        guard let subject = workspace.review?.subject else { return }
        if workspace.reviewStale && !current.decisionOutstanding { workspace.loadReview(subject) }
        workspace.loadDocument(subject, revision: revision)
        if mode == .compare, let detail = workspace.document, detail.id == subject, let previous = detail.previous {
            workspace.loadComparison(subject, revision: previous.revision)
        }
    }

    private var actions: SpecificationsActions {
        let workspace = model.workspace
        return SpecificationsActions(
            open: open,
            choose: { id in revision = nil; workspace.loadReview(id); workspace.loadDocument(id) },
            chooseRevision: { id, r in
                revision = r
                if workspace.review?.subject != id { workspace.loadReview(id) }
                workspace.loadDocument(id, revision: r)
                mode = .document
            },
            approve: { workspace.decide(.Approve) },
            requestChanges: {
                if let feature = workspace.document?.feature { workspace.openChat(feature) }
                workspace.chatDraft = state.changeRequest
                open(.featureChat)
            },
            reload: { read() })
    }
}

struct SpecificationsView: View {
    @Environment(\.theme) private var theme
    let state: SpecificationsState
    var actions = SpecificationsActions()

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            banners.padding(.horizontal, 10)
            LifecycleCard(summary: state.scope.summary).padding(.horizontal, 10)
            FeatureColumns(leadingWidth: 250, trailingWidth: 310, wide: 1040, medium: 760) {
                leading
            } main: {
                article
            } trailing: {
                inspector
            }
        }
        .padding(.top, 6)
    }

    // MARK: Banners

    private var banners: some View {
        CoreBanners(connection: state.connection,
                    missing: state.reportsDetail ? nil : ("Document", "document text"))
    }

    // MARK: Leading column

    private var leading: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 4) {
                GroupLabel(state.scope.summary.map { "\($0.code) documents" } ?? "Documents").padding(.horizontal, 6)
                if state.listed.isEmpty {
                    NotReported(state.documents == nil ? "This core does not list documents." : "No documents yet.")
                        .padding(.horizontal, 10)
                }
                ForEach(state.listed) { document in documentRows(document) }
            }
            inside
        }
    }

    @ViewBuilder
    private func documentRows(_ document: DocumentSummary) -> some View {
        let open = state.detail?.id == document.id ? state.detail : nil
        let tile = documentTile(document.kind)
        if let open {
            ForEach(open.revisions) { r in
                let pill = r.revision == open.newest
                    ? approvalPill(open.approval, stale: open.stale)
                    : r.decision.map { approvalPill($0 == "Approved" ? "Approved" : $0, superseded: true) } ?? ("Superseded", .neutral)
                SelectableRow(selected: r.revision == open.revision, action: { actions.chooseRevision(document.id, r.revision) }) {
                    LetterTile(text: tile.0, color: r.revision == open.newest ? tile.1 : Palette.gray)
                    rowText(name: documentCode(document.kind), pill: pill, sub: "\(document.title ?? document.featureName ?? document.kindName) · rev \(r.revision)")
                }
            }
        } else {
            SelectableRow(selected: false, action: { actions.choose(document.id) }) {
                LetterTile(text: tile.0, color: tile.1)
                rowText(name: documentCode(document.kind), pill: approvalPill(document.approval, stale: document.stale),
                        sub: "\(document.title ?? document.featureName ?? document.kindName) · rev \(document.revision)")
            }
        }
    }

    private func rowText(name: String, pill: (String, Tone), sub: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 6) {
                Text(name).textStyle(.bodyStrong).frame(maxWidth: .infinity, alignment: .leading)
                OperationalPill(pill.0, tone: pill.1)
            }
            Text(sub).textStyle(.secondary).foregroundStyle(theme.text2).lineLimit(1)
        }
    }

    /// "Inside PRD rev n": how many entries each section of the requirements has.
    @ViewBuilder
    private var inside: some View {
        if let detail = state.detail, let counts = detail.counts {
            VStack(alignment: .leading, spacing: 6) {
                GroupLabel("Inside PRD rev \(detail.revision)").padding(.horizontal, 6)
                DefinitionCard(rows: [
                    ("Acceptance criteria", "acceptance_criteria"), ("Use cases", "use_cases"),
                    ("Non-functional requirements", "nfrs"), ("Assumptions", "assumptions"), ("Open blockers", "blockers"),
                ].map { Definition(key: $0.0, value: "\(counts[$0.1] ?? 0)") } + [
                    Definition(key: "Technical constraints", value: "Not reported"),
                    Definition(key: "Risks", value: "Not reported"),
                ], keyWidth: 150)
            }
        }
    }

    // MARK: Article

    private var article: some View {
        VStack(alignment: .leading, spacing: 22) {
            switch (state.detail, state.detailLoad) {
            case (let detail?, _):
                if state.mode == .compare { comparison(detail) } else { document(detail) }
            case (nil, .failed(let reason)):
                Placeholder(symbol: "doc.questionmark", tile: Palette.red, title: "The document was not read", message: reason) {
                    Button("Read Again") { actions.reload() }.buttonStyle(.jojo(.primary, compact: true))
                }
            case (nil, .loading):
                SkeletonCard()
            case (nil, _):
                Placeholder(symbol: "doc.text", tile: Palette.blue, title: "No document chosen",
                            message: "Choose a document. Requirements, decisions and plans appear here once the lead drafts them.") {
                    Button("Open Feature Chat") { actions.open(.featureChat) }.buttonStyle(.jojo(.secondary, compact: true))
                }
            }
        }
        .padding(EdgeInsets(top: 28, leading: 32, bottom: 32, trailing: 32))
        .frame(maxWidth: .infinity, alignment: .topLeading)
        .cardElevation(radius: 18)
    }

    private func document(_ detail: DocumentDetail) -> some View {
        let newest = detail.revision == detail.newest
        let pill = newest ? approvalPill(detail.approval, stale: detail.stale)
            : approvalPill(detail.revisions.first { $0.revision == detail.revision }?.decision ?? "Superseded", superseded: true)
        let digest = detail.revisions.first { $0.revision == detail.revision }?.digest ?? ""
        return VStack(alignment: .leading, spacing: 22) {
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    OperationalPill(pill.0, tone: pill.1)
                    Text("\(documentCode(detail.kind)) · rev \(detail.revision) · sha256 \(Self.shortDigest(digest))")
                        .textStyle(.mono).foregroundStyle(theme.text2).textSelection(.enabled)
                }
                Text(detail.title ?? detail.featureName.map { "\($0) · \(documentCode(detail.kind))" } ?? documentCode(detail.kind))
                    .font(.system(size: 28, weight: .bold)).tracking(-0.5)
            }
            // The text's own title heads the article; an empty section is left out.
            ForEach(SpecificationsState.sections(detail.text).filter { !$0.lines.isEmpty }) { section in
                VStack(alignment: .leading, spacing: 6) {
                    GroupLabel(section.heading).padding(.horizontal, -4)
                    ForEach(Array(section.lines.enumerated()), id: \.offset) { _, line in
                        if !line.trimmingCharacters(in: .whitespaces).isEmpty { Inline(line) }
                    }
                }
            }
            if !detail.complete {
                NoteLine("The text is longer than this app reads (\(documentTextLimit) characters); the rest is not shown.")
            }
        }
    }

    private func comparison(_ detail: DocumentDetail) -> some View {
        VStack(alignment: .leading, spacing: 22) {
            if let previous = detail.previous, state.comparison?.revision == previous.revision {
                VStack(alignment: .leading, spacing: 6) {
                    Text("\(documentCode(detail.kind)) rev \(previous.revision) → rev \(detail.revision)")
                        .font(.system(size: 24, weight: .bold))
                    Text("\(state.changedSections) of \(state.compared.count) sections changed").textStyle(.secondary).foregroundStyle(theme.text2)
                }
                ForEach(Array(state.compared.enumerated()), id: \.offset) { _, section in
                    VStack(alignment: .leading, spacing: 6) {
                        GroupLabel(section.heading).padding(.horizontal, -4)
                        HStack(alignment: .top, spacing: 10) {
                            side("rev \(previous.revision)", section.lines.filter { $0.kind != .added })
                            side("rev \(detail.revision)", section.lines.filter { $0.kind != .removed })
                        }
                    }
                }
            } else if detail.previous == nil {
                Placeholder(symbol: "arrow.left.arrow.right", tile: Palette.gray, title: "Nothing to compare",
                            message: "Revision \(detail.revision) is the document's first.") { EmptyView() }
            } else {
                SkeletonCard()
            }
        }
    }

    private func side(_ title: String, _ lines: [ComparedLine]) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).textStyle(.secondary).foregroundStyle(theme.text2)
            ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                Text(line.text).font(.system(size: 13)).strikethrough(line.kind == .removed)
                    .foregroundStyle(line.kind == .same ? theme.text : line.kind == .added ? theme.green : theme.red)
                    .padding(.horizontal, line.kind == .same ? 0 : 3)
                    .background(line.kind == .same ? .clear : line.kind == .added ? theme.greenBg : theme.redBg,
                                in: RoundedRectangle(cornerRadius: 4))
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .topLeading)
        .background(theme.grayBg.opacity(0.5), in: RoundedRectangle(cornerRadius: 12))
    }

    static func shortDigest(_ digest: String) -> String {
        digest.count > 10 ? "\(digest.prefix(4))…\(digest.suffix(3))" : digest
    }

    // MARK: Inspector

    private var inspector: some View {
        GlassInspector {
            if let detail = state.detail {
                approvalScope(detail)
                history(detail)
                validation(detail)
                traceability(detail)
            } else {
                NotReported("Choose a document to see what an approval covers.")
            }
        }
    }

    private func approvalScope(_ detail: DocumentDetail) -> some View {
        let digest = detail.revisions.first { $0.revision == detail.revision }?.digest ?? ""
        let scope = SpecificationsState.authorizes(detail)
        return LabeledSection(title: "Approval scope", spacing: 10) {
            DefinitionCard(rows: [
                Definition(key: "Subject", value: "\(documentCode(detail.kind)) rev \(detail.revision)"),
                Definition(key: "Content hash", value: "sha256 \(Self.shortDigest(digest))", mono: true),
                Definition(key: "Authorizes", value: scope.yes),
                Definition(key: "Does not", value: scope.no),
            ], keyWidth: 84)
            Button(action: actions.approve) { Text("Approve Revision").frame(maxWidth: .infinity) }.buttonStyle(.jojo(.primary))
                .disabled(state.approveBlocked != nil).help(state.approveBlocked ?? "Records your approval of this exact revision.")
            Button(action: actions.requestChanges) { Text("Request Changes").frame(maxWidth: .infinity) }.buttonStyle(.jojo(.secondary))
                .disabled(detail.feature == nil).help("Opens Feature Chat with a note to the lead.")
            Text(state.decisionOutstanding ? "Sent to core · waiting for confirmation. Nothing is shown as approved until the core records it."
                 : "Any edit after approval invalidates it. Approval never carries over to new content.")
                .font(.system(size: 11)).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
        }
    }

    private func history(_ detail: DocumentDetail) -> some View {
        LabeledSection(title: "Approval history", link: ("View all", { actions.open(.approvalInbox) })) {
            if detail.history.isEmpty {
                NotReported("No decision recorded on this document.")
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(detail.history.enumerated()), id: \.offset) { index, d in
                        VStack(alignment: .leading, spacing: 4) {
                            HStack(spacing: 6) {
                                Text("rev \(d.revision)").textStyle(.bodyStrong).frame(maxWidth: .infinity, alignment: .leading)
                                let pill = approvalPill(d.decision)
                                OperationalPill(pill.0, tone: pill.1)
                                if !d.applies && d.decision == "Approved" { OperationalPill("Superseded", tone: .neutral) }
                            }
                            Text((["You", boardTime(d.at, now: state.now)].compactMap { $0 } + ["sha256 \(Self.shortDigest(d.digest))"])
                                .joined(separator: " · "))
                                .textStyle(.secondary).foregroundStyle(theme.text2)
                        }
                        .padding(EdgeInsets(top: 9, leading: 12, bottom: 9, trailing: 12))
                        .overlay(alignment: .top) { if index > 0 { Rectangle().fill(theme.sep).frame(height: 0.5) } }
                    }
                }
                .cardElevation(radius: 12)
            }
        }
    }

    @ViewBuilder
    private func validation(_ detail: DocumentDetail) -> some View {
        let findings = detail.findings ?? []
        LabeledSection(title: detail.findings == nil ? "Validation" : "Validation · \(findings.count) issues") {
            if detail.findings == nil {
                NotReported("The core checks only requirements documents; it reports no findings for this kind.")
            } else if findings.isEmpty {
                NotReported("No findings: the requirements are ready for review.")
            } else {
                DefinitionCard(rows: findings.map {
                    Definition(key: $0.path, value: SpecificationsState.findingText($0),
                               pill: ($0.problem == "OpenBlocker" ? "Blocker" : "Missing", $0.problem == "Placeholder" ? .warn : .danger))
                }, keyWidth: 110)
            }
        }
    }

    @ViewBuilder
    private func traceability(_ detail: DocumentDetail) -> some View {
        LabeledSection(title: "Traceability", link: ("Affected Tasks", { actions.open(.executionPlan) })) {
            if let criteria = detail.criteria {
                if criteria.isEmpty {
                    NotReported("No acceptance criteria.")
                } else {
                    KeyValueCard(rows: criteria.map { ($0.id, $0.statement, $0.useCases.isEmpty ? "—" : $0.useCases.joined(separator: ", ")) },
                                 plain: true)
                    NoteLine("Criteria with the use cases that name them. Which task implements a criterion is not reported.")
                }
            } else if detail.upstream.isEmpty {
                NotReported("Not reported for this document.")
            } else {
                KeyValueCard(rows: detail.upstream.map {
                    (documentCode($0.kind), $0.title ?? "Drafted on rev \($0.revision ?? 1)", approvalPill($0.approval).0)
                }, plain: true)
                NoteLine("The documents this revision was drafted on. Which task implements a criterion is not reported.")
            }
        }
    }
}

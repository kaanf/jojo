import JojoCore
import SwiftUI

// Specifications (docs/design/screens/SPECIFICATIONS.md): the documents the snapshot knows
// and what the core records about the loaded one. It has no load or query of its own, and
// no decision: the loaded document is the Approval Inbox's, and is decided there.

/// What the screen shows: the review the workspace holds and the snapshot's documents, as
/// the Approval Inbox reads them. The helper reports no document text.
struct SpecificationsState: Equatable {
    var inbox: ApprovalInboxState

    var review: ReviewBundle? { inbox.review }
    var documents: [ApprovalInboxState.Document] { inbox.documents }

    /// The core's record of the loaded document, when the snapshot lists it.
    var record: String? {
        guard let subject = review?.subject else { return nil }
        return documents.first { $0.id == subject }?.record
    }

    /// The second line of a row: the bundle's kind and revision for the loaded document,
    /// the kind of the snapshot row and the core's record for any other.
    func detail(_ document: ApprovalInboxState.Document) -> String {
        if let review, review.subject == document.id {
            return "\(ApprovalInboxState.kind(review).name) · rev \(review.revision)"
        }
        return "\(document.kind) · record \(ApprovalInboxState.short(document.record))"
    }

    static func contentDigest(_ bundle: ReviewBundle) -> String {
        "sha256 \(ApprovalInboxState.short(bundle.contentHash))"
    }

    /// One upstream approval the loaded revision binds: all the core reports as traceability.
    struct Trace: Identifiable, Equatable {
        let event: Int64
        var id: Int64 { event }
        var name: String { "event \(event)" }
    }

    static func traces(_ bundle: ReviewBundle) -> [Trace] {
        bundle.upstream.map(Trace.init(event:))
    }
}

struct SpecificationsActions {
    var load: (String) -> Void = { _ in }
    var open: (Screen) -> Void = { _ in }
}

struct SpecificationsScreen: View {
    @ObservedObject var model: AppModel
    let open: (Screen) -> Void

    var body: some View {
        let workspace = model.workspace
        SpecificationsView(
            state: SpecificationsState(inbox: ApprovalInboxState(
                connection: model.client.connection, admission: workspace.admission,
                rows: workspace.rows, review: workspace.review, reviewStale: workspace.reviewStale,
                decisionOutstanding: model.client.outstandingCommands.values.contains("HumanDecision"),
                candidate: workspace.candidate)),
            actions: SpecificationsActions(load: { workspace.loadReview($0) }, open: open))
    }
}

struct SpecificationsView: View {
    @Environment(\.theme) private var theme
    let state: SpecificationsState
    var actions = SpecificationsActions()

    var body: some View {
        GeometryReader { geometry in
            // The main column keeps at least 460 beside each panel.
            let columns = geometry.size.width >= 1030 ? 3 : geometry.size.width >= 730 ? 2 : 1
            HStack(spacing: 0) {
                if columns > 1 { panel(edge: .trailing, width: 270) { leading } }
                Scrolling {
                    VStack(alignment: .leading, spacing: 18) {
                        banners
                        if columns == 1 { Card(.panelList) { leading } }
                        if let bundle = state.review {
                            head(bundle)
                            sections(bundle)
                        } else {
                            placeholder.frame(minHeight: 260)
                        }
                        if columns < 3 { trailing.padding(.top, 6) }
                    }
                    .padding(EdgeInsets(top: 16, leading: 24, bottom: 26, trailing: 24))
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                if columns == 3 { panel(edge: .leading, width: 300) { trailing.padding(14) } }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
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
        switch state.inbox.link {
        case .lost:
            Banner(.danger, title: "Jojo core isn't responding",
                   message: "Showing the last known state, read-only. No document can be loaded.")
        case .incompatible:
            Banner(.danger, title: "Jojo core speaks another protocol version",
                   message: "Read-only. No document can be loaded.")
        case .connected, .starting:
            EmptyView()
        }
        if state.inbox.admissionClosed {
            Banner(.danger, title: "Execution admission is closed",
                   message: "No new attempt starts until you review the inspection. Documents can still be read.") {
                Button("Open Recovery Center") { actions.open(.recoveryCenter) }.buttonStyle(.jojo(.secondary, compact: true))
            }
        }
    }

    private var placeholder: some View {
        StateBlock(symbol: "doc.text", title: "No document loaded",
                   message: "Choose a document in the list. What the core records about its current revision is shown here; decisions are made in the Approval Inbox.")
    }

    // MARK: Leading panel

    private var leading: some View {
        let documents = state.documents
        return VStack(alignment: .leading, spacing: 0) {
            SectionHeading("Documents · \(documents.count)")
                .padding(EdgeInsets(top: 14, leading: 16, bottom: 6, trailing: 14))
            if documents.isEmpty {
                Text("The snapshot lists no documents.").textStyle(.secondary).foregroundStyle(theme.text3)
                    .padding(.horizontal, 18)
            } else {
                VStack(spacing: 2) {
                    ForEach(documents) { document in item(document) }
                }
            }
            VStack(alignment: .leading, spacing: 8) {
                Footnote("Choosing a document loads its review. It is the same document the Approval Inbox shows and decides.")
                Footnote("Not reported: the names and statuses of documents, their earlier revisions and the sections inside them. Documents are listed by identity.")
            }
            .padding(.vertical, 14)
            .padding(.horizontal, 16)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func item(_ document: ApprovalInboxState.Document) -> some View {
        let loaded = document.id == state.review?.subject
        return Button { actions.load(document.id) } label: {
            DocumentRow(name: ApprovalInboxState.short(document.id), mono: true, detail: state.detail(document),
                        status: loaded && state.inbox.stale ? .stale : nil, selected: loaded)
        }
        .buttonStyle(.plain)
        .padding(.horizontal, 8)
        .help("Document \(document.id). The core's record: \(document.record)")
        .accessibilityAddTraits(loaded ? .isSelected : [])
    }

    // MARK: Main column

    private func head(_ bundle: ReviewBundle) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            WrappingRow(spacing: 8, lineSpacing: 6) {
                Text(ApprovalInboxState.kind(bundle).name).textStyle(.secondary).foregroundStyle(theme.text2)
                ReviewMark(state: state.inbox, bundle: bundle)
                Text(SpecificationsState.contentDigest(bundle)).textStyle(.mono).foregroundStyle(theme.text3)
                    .help("Content digest \(bundle.contentHash)")
            }
            Text(ApprovalInboxState.title(bundle)).font(.system(size: 22, weight: .bold)).tracking(-0.44)
                .foregroundStyle(theme.textStrong).help("Document \(bundle.subject)")
        }
    }

    @ViewBuilder
    private func sections(_ bundle: ReviewBundle) -> some View {
        DocumentSection("Text") {
            NoticeWell("The text of this document is not reported. The core gives its digests, not its sections; a human approves exactly those digests.")
        }
        DocumentSection("Recorded by the core") {
            Card(.panelList) {
                VStack(spacing: 0) {
                    FactLine("Document") { digest(bundle.subject) }
                    FactLine("Revision", "\(bundle.revision)")
                    FactLine("Snapshot record") {
                        if let record = state.record {
                            digest(record)
                        } else {
                            Text("Not in the snapshot").foregroundStyle(theme.text3)
                        }
                    }
                    FactLine("Bundle") { digest(bundle.hash) }
                    FactLine("Content") { digest(bundle.contentHash) }
                    FactLine("Displayed") { digest(bundle.displayedHash) }
                    FactLine("Context", last: true) { digest(bundle.contextHash) }
                }
            }
        }
        DocumentSection("Compare revisions") {
            NoticeWell("Earlier revisions are not reported, so revision \(bundle.revision) cannot be compared with the one before it.")
        }
    }

    private func digest(_ value: String) -> some View {
        Text(value).font(.system(size: 11.5, design: .monospaced)).textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
    }

    // MARK: Trailing panel

    private var trailing: some View {
        VStack(alignment: .leading, spacing: 18) {
            block("Approval scope") { scope }
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Approval history") {
                    Button("View all") { actions.open(ApprovalInboxState.historyScreen) }
                        .buttonStyle(.jojo(.secondary, compact: true))
                }
                history
                Footnote(ApprovalInboxState.historyNote("View all")).padding(.horizontal, 2)
            }
            block("Validation") {
                NoticeWell("Not reported. The core sends no findings about a document, such as a conflict or a missing acceptance criterion.")
            }
            block("Traceability") { traceability }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func block(_ title: String, @ViewBuilder _ content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading(title)
            content()
        }
    }

    private var unloaded: some View {
        Text("No document is loaded.").textStyle(.secondary).foregroundStyle(theme.text3).padding(.horizontal, 2)
    }

    @ViewBuilder
    private var scope: some View {
        if let bundle = state.review {
            Card(.panel) {
                VStack(alignment: .leading, spacing: 10) {
                    FactGrid {
                        Fact("Subject", ApprovalInboxState.title(bundle))
                        Fact("Content hash") {
                            Text(SpecificationsState.contentDigest(bundle)).font(.system(size: 11.5, design: .monospaced))
                                .help("Content digest \(bundle.contentHash)")
                        }
                        Fact("Authorizes") {
                            Text("Not reported: the plans and tasks that may use this revision.").font(.system(size: 12.5))
                                .foregroundStyle(theme.text3).fixedSize(horizontal: false, vertical: true)
                        }
                        Fact("Does not") {
                            Text("Start tasks, push or deploy. Approving a document starts nothing.").font(.system(size: 12.5))
                                .fixedSize(horizontal: false, vertical: true)
                        }
                        Fact("Decision") { ReviewMark(state: state.inbox, bundle: bundle) }
                    }
                    Button("Open Approval Inbox") { actions.open(.approvalInbox) }
                        .buttonStyle(.jojo(.primary, compact: true))
                        .help("Opens this revision in the Approval Inbox, where it is approved, rejected or revoked")
                    Footnote("Any edit after approval invalidates it. Approval never carries over to new content.")
                }
            }
        } else {
            unloaded
        }
    }

    @ViewBuilder
    private var history: some View {
        if let bundle = state.review {
            Card(.panelList) {
                VStack(alignment: .leading, spacing: 2) {
                    if let event = bundle.priorDecision {
                        Text("event \(event)").font(.system(size: 11.5, design: .monospaced)).foregroundStyle(theme.text)
                        Text("The latest decision on this document. What was decided is not reported.")
                            .textStyle(.secondary).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
                    } else {
                        Text("No decision is recorded for this document.").textStyle(.secondary).foregroundStyle(theme.text2)
                    }
                }
                .padding(.vertical, 9)
                .padding(.horizontal, 14)
            }
            Footnote("Not reported: what was decided, by whom and when, and the decisions on earlier revisions.")
                .padding(.horizontal, 2)
        } else {
            unloaded
        }
    }

    @ViewBuilder
    private var traceability: some View {
        if let bundle = state.review {
            let traces = SpecificationsState.traces(bundle)
            Card(.panelList) {
                if traces.isEmpty {
                    Text("This revision binds no upstream approval.").textStyle(.secondary).foregroundStyle(theme.text2)
                        .padding(.vertical, 9)
                        .padding(.horizontal, 14)
                } else {
                    VStack(spacing: 0) {
                        ForEach(traces) { trace in
                            TraceLine(trace.name, "Bound upstream approval", last: trace == traces.last)
                        }
                    }
                }
            }
            Footnote("Not reported: requirements, acceptance criteria and the tasks that implement them.")
                .padding(.horizontal, 2)
        } else {
            unloaded
        }
    }
}

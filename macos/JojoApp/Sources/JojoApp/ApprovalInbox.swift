import JojoCore
import SwiftUI

// Approval Inbox (docs/design/screens/APPROVAL_INBOX.md): the one review the workspace holds,
// its decision, and the documents the snapshot knows. It has no load or query of its own.

/// What the screen shows. Every field is the helper's answer; nothing is inferred here
/// except the kind of a document, which the bundle shows (`kind`).
struct ApprovalInboxState: Equatable {
    var connection: HelperConnection
    var admission: String
    /// The snapshot rows; the documents among them are listed.
    var rows: [SnapshotRow]
    var review: ReviewBundle?
    /// A decision was sent or refused for the loaded review; it must be loaded again.
    var reviewStale: Bool
    /// A decision is on its way to the core and has no answer yet.
    var decisionOutstanding: Bool
    /// The loaded attempt result, read-only here; its decision is on Verification & Evidence.
    var candidate: CandidateReview?
    /// Protocol 1.6: documents with names and approval states, and the candidates that
    /// wait; nil from an older helper, which leaves the snapshot's list.
    var list: DocumentList? = nil
    /// The time ages are counted from.
    var now = Date()

    /// One document the snapshot knows. Its approval state is not reported.
    struct Document: Identifiable, Equatable {
        let id: String
        /// The kind of its snapshot row: `artifact` or `review_document`.
        let row: String
        let record: String

        var kind: String { row == "artifact" ? "Specification" : "Decision or plan" }
        var symbol: String { row == "artifact" ? "doc.text" : "doc.on.doc" }
    }

    static let documentKinds: Set<String> = ["artifact", "review_document"]

    /// The snapshot's document rows, in its order.
    static func documents(in rows: [SnapshotRow]) -> [Document] {
        var seen = Set<String>()
        return rows.filter { documentKinds.contains($0.kind) && seen.insert($0.subject).inserted }.map {
            Document(id: $0.subject, row: $0.kind, record: $0.value)
        }
    }

    var documents: [Document] { Self.documents(in: rows) }

    /// What the core reports about the loaded document.
    var summary: DocumentSummary? { review.flatMap { list?[$0.subject] } }

    /// The tag of a document's approval state.
    static func tag(_ d: DocumentSummary) -> (DocumentStatus, String?) {
        switch d.approval {
        case "Pending": return d.stale ? (.stale, nil) : (.proposed, "Needs approval")
        case "Approved": return (.approved, nil)
        case "Rejected": return (.invalidated, "Rejected")
        case "Revoked": return (.invalidated, "Revoked")
        case "NotReady": return (.superseded, "Has findings")
        default: return (.superseded, "Waiting")
        }
    }

    static func symbol(_ kind: String) -> String {
        switch kind {
        case "Prd": return "doc.text"
        case "Adr": return "building.columns"
        default: return "list.bullet.indent"
        }
    }

    struct Kind: Equatable {
        let name: String
        let approval: String
        let symbol: String
        let why: String
        /// "Approve Revision", or "Approve Plan".
        let approve: String
    }

    /// The helper sends no word for the kind. A specification binds no upstream approval, an
    /// architecture decision its PRD's, a plan its PRD's and its architecture decision's.
    static func kind(_ bundle: ReviewBundle) -> Kind {
        switch bundle.upstream.count {
        case 0:
            return Kind(name: "Specification", approval: "Spec approval", symbol: "doc.text",
                        why: "Requirements bind planning for the feature. An approval does not carry over to changed content.",
                        approve: "Approve Revision")
        case 1:
            return Kind(name: "Architecture decision", approval: "Spec approval", symbol: "building.columns",
                        why: "Architecture decisions bind every plan that builds on them. An approval does not carry over to changed content.",
                        approve: "Approve Revision")
        default:
            return Kind(name: "Execution plan", approval: "Plan approval", symbol: "list.bullet.indent",
                        why: "Only approved plans can admit tasks. Approving does not start anything.",
                        approve: "Approve Plan")
        }
    }

    static func short(_ identity: String) -> String {
        String(identity.prefix(12))
    }

    static func title(_ bundle: ReviewBundle, _ summary: DocumentSummary? = nil) -> String {
        guard let summary else { return "\(short(bundle.subject)) rev \(bundle.revision)" }
        return "\(summary.name) rev \(bundle.revision)"
    }

    static func upstream(_ bundle: ReviewBundle) -> String {
        switch bundle.upstream.count {
        case 0: return "None"
        case 1: return "event \(bundle.upstream[0])"
        default: return "events " + bundle.upstream.map(String.init).joined(separator: ", ")
        }
    }

    /// The bundle names the event of the latest decision; since 1.6 the core also says
    /// what it was when it applies to this revision.
    static func latestDecision(_ bundle: ReviewBundle, _ summary: DocumentSummary? = nil, now: Date = Date()) -> String {
        guard let event = bundle.priorDecision else { return "None recorded" }
        guard let summary else { return "event \(event) · the action is not reported" }
        let when = clockTime(summary.decided, now: now, seconds: false).map { " · \($0)" } ?? ""
        switch summary.approval {
        case "Approved", "Rejected", "Revoked": return "\(summary.approval) · event \(event)\(when)"
        default: return "event \(event) · on an earlier revision"
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

    var admissionClosed: Bool { link == .connected && !admission.isEmpty && admission != "Open" }

    /// The decision is on its way: shown as Requested, never as recorded.
    var requested: Bool { review != nil && reviewStale && decisionOutstanding }

    /// The stale banner: the review cannot be decided and nothing is on its way.
    var stale: Bool { review != nil && reviewStale && !decisionOutstanding }

    /// Why Approve, Reject and Revoke are disabled; `nil` when they can be sent. Which of them
    /// the core accepts for a document is the core's rule.
    var decideBlocked: String? {
        guard review != nil else { return "No document loaded." }
        if link != .connected { return "Unavailable: the core is not connected." }
        if requested { return "Unavailable while a decision awaits the core." }
        if reviewStale { return "Unavailable: this review is stale. Reload the document." }
        return nil
    }

    /// Where the canvas's "Approval History" link goes, here and on Specifications: the event
    /// journal as it is. Nothing filters it to a document, and the link reads nothing.
    static let historyScreen = Screen.activity

    /// What the link named `button` opens, said beside it: the journal is not a decision history.
    static func historyNote(_ button: String) -> String {
        "\"\(button)\" opens \(historyScreen.title), the core's event journal: every loaded event, not one document's decisions. A decision is an event of type HumanDecision."
    }

    static func confirmation(_ action: DecisionAction, _ bundle: ReviewBundle)
        -> (title: String, message: String, confirm: String, keep: String)? {
        let object = "revision \(bundle.revision) of document \(short(bundle.subject))"
        let kept = " A recorded decision is not removed from this app."
        switch action {
        case .Approve:
            return nil
        case .Reject:
            return ("Reject this revision?",
                    "Records your decision to reject \(object). Work that needs its approval does not start." + kept,
                    "Reject Revision", "Keep Undecided")
        case .Revoke:
            return ("Revoke the approval of this revision?",
                    "Records that \(object) is no longer approved. The core may stop attempts that depend on it." + kept,
                    "Revoke Approval", "Keep Approval")
        }
    }
}

/// Where the loaded review stands: a decision on its way, stale, or whether the bundle
/// names an earlier decision. What was decided is not reported. Also on Specifications.
struct ReviewMark: View {
    let state: ApprovalInboxState
    let bundle: ReviewBundle

    var body: some View {
        if state.requested {
            OperationalPill(.requested, standalone: true)
        } else if state.stale {
            DocumentTag(.stale)
        } else if bundle.priorDecision == nil {
            OperationalPill(.awaitingApproval, label: "No decision yet", standalone: true)
        } else {
            OperationalPill(.idle, label: "Decision recorded", standalone: true)
        }
    }
}

struct ApprovalInboxActions {
    var load: (String) -> Void = { _ in }
    var decide: (DecisionAction) -> Void = { _ in }
    var open: (Screen) -> Void = { _ in }
    /// Loads a waiting candidate and opens Verification & Evidence, where it is decided.
    var candidate: (String) -> Void = { _ in }
}

struct ApprovalInboxScreen: View {
    @ObservedObject var model: AppModel
    let open: (Screen) -> Void

    var body: some View {
        let workspace = model.workspace
        ApprovalInboxView(
            state: ApprovalInboxState(connection: model.client.connection, admission: workspace.admission,
                                      rows: workspace.rows, review: workspace.review, reviewStale: workspace.reviewStale,
                                      decisionOutstanding: model.client.outstandingCommands.values.contains("HumanDecision"),
                                      candidate: workspace.candidate, list: workspace.documents),
            actions: ApprovalInboxActions(load: { workspace.loadReview($0) }, decide: { workspace.decide($0) }, open: open,
                                          candidate: { attempt in
                                              workspace.loadCandidate(attempt)
                                              open(.verification)
                                          }))
    }
}

struct ApprovalInboxView: View {
    @Environment(\.theme) private var theme
    let state: ApprovalInboxState
    var actions = ApprovalInboxActions()
    @State private var identity: String
    /// The decision the confirmation dialog asks about: Reject or Revoke.
    @State private var confirming: DecisionAction?

    init(state: ApprovalInboxState, actions: ApprovalInboxActions = ApprovalInboxActions()) {
        self.state = state
        self.actions = actions
        _identity = State(initialValue: state.review?.subject ?? "")
    }

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
                            facts(bundle)
                            evidence
                            decision(bundle)
                        } else {
                            placeholder.frame(minHeight: 260)
                        }
                        if columns < 3 { trailing.padding(.top, 6) }
                    }
                    .padding(EdgeInsets(top: 18, leading: 22, bottom: 24, trailing: 22))
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                if columns == 3 { panel(edge: .leading, width: 300) { trailing.padding(14) } }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        // The field names the loaded document, also when a row loaded it.
        .onChange(of: state.review?.subject) { _, subject in
            if let subject { identity = subject }
        }
        .confirmationDialog(dialog?.title ?? "", isPresented: Binding(get: { dialog != nil }, set: { if !$0 { confirming = nil } }),
                            titleVisibility: .visible) {
            if let dialog, let action = confirming {
                Button(dialog.confirm, role: .destructive) { actions.decide(action) }
                Button(dialog.keep, role: .cancel) {}
            }
        } message: {
            Text(dialog?.message ?? "")
        }
    }

    private var dialog: (title: String, message: String, confirm: String, keep: String)? {
        guard let confirming, let bundle = state.review else { return nil }
        return ApprovalInboxState.confirmation(confirming, bundle)
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
                   message: "No new attempt starts until you review the inspection. Documents can still be decided.") {
                Button("Open Recovery Center") { actions.open(.recoveryCenter) }.buttonStyle(.jojo(.secondary, compact: true))
            }
        }
    }

    private var placeholder: some View {
        StateBlock(symbol: "tray", title: "No document loaded",
                   message: "Choose a document in the list, or load one by its identity. Its review bundle is shown here, and you approve, reject or revoke exactly that revision.")
    }

    // MARK: Leading panel

    @ViewBuilder
    private var leading: some View {
        if let list = state.list { reported(list) } else { snapshotList }
    }

    /// Protocol 1.6: what waits for the human first, then every other document.
    private func reported(_ list: DocumentList) -> some View {
        let others = list.documents.filter { $0.approval != "Pending" }
        return VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Waiting for you · \(list.pending)")
                HStack(spacing: 6) {
                    InputField(prompt: "Document identity (32 hex)", text: $identity, mono: true, submit: { actions.load(identity) })
                    Button("Load") { actions.load(identity) }.buttonStyle(.jojo(.secondary, compact: true))
                }
            }
            .padding(EdgeInsets(top: 12, leading: 16, bottom: 8, trailing: 12))
            if list.pending == 0 {
                Text("Nothing waits for your decision.").textStyle(.secondary).foregroundStyle(theme.text3)
                    .padding(.horizontal, 18).padding(.bottom, 6)
            }
            VStack(spacing: 2) {
                ForEach(list.waiting) { document in item(document) }
                ForEach(list.candidates) { candidate in item(candidate) }
            }
            if !others.isEmpty {
                SectionHeading("Documents · \(others.count)")
                    .padding(EdgeInsets(top: 14, leading: 16, bottom: 6, trailing: 12))
                VStack(spacing: 2) {
                    ForEach(others) { document in item(document) }
                }
            }
            VStack(alignment: .leading, spacing: 8) {
                Footnote("Approvals are type-specific. There is no approve-all.")
                if list.more { Footnote("Only the newest documents are listed; load an older one by its identity.") }
                Footnote("A candidate is accepted or rejected on Verification & Evidence.")
            }
            .padding(.vertical, 14)
            .padding(.horizontal, 16)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func item(_ document: DocumentSummary) -> some View {
        let loaded = document.id == state.review?.subject
        let (status, label) = ApprovalInboxState.tag(document)
        return Button { actions.load(document.id) } label: {
            RequestRow(kind: document.kindName, age: shortAge(document.created, now: state.now),
                       title: document.title ?? document.featureName ?? document.kindName, detail: "rev \(document.revision)", status: loaded && state.stale ? .stale : status,
                       statusLabel: loaded && state.stale ? nil : label, symbol: ApprovalInboxState.symbol(document.kind),
                       awaiting: document.approval == "Pending", selected: loaded)
        }
        .buttonStyle(.plain)
        .padding(.horizontal, 8)
        .help("Document \(document.id)")
        .accessibilityAddTraits(loaded ? .isSelected : [])
    }

    private func item(_ candidate: PendingCandidate) -> some View {
        Button { actions.candidate(candidate.id) } label: {
            RequestRow(kind: "Candidate", age: shortAge(candidate.validated, now: state.now),
                       title: candidate.title ?? candidate.key ?? ApprovalInboxState.short(candidate.id),
                       detail: candidate.featureName ?? "host validation passed", status: .proposed,
                       statusLabel: "Needs decision", symbol: "checkmark.seal", awaiting: true)
        }
        .buttonStyle(.plain)
        .padding(.horizontal, 8)
        .help("Attempt \(candidate.id): open it on Verification & Evidence")
    }

    /// An older helper: the snapshot's documents, by identity.
    private var snapshotList: some View {
        let documents = state.documents
        return VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Documents · \(documents.count)")
                HStack(spacing: 6) {
                    InputField(prompt: "Document identity (32 hex)", text: $identity, mono: true, submit: { actions.load(identity) })
                    Button("Load") { actions.load(identity) }.buttonStyle(.jojo(.secondary, compact: true))
                }
            }
            .padding(EdgeInsets(top: 12, leading: 16, bottom: 8, trailing: 12))
            if documents.isEmpty {
                Text("The snapshot lists no documents.").textStyle(.secondary).foregroundStyle(theme.text3)
                    .padding(.horizontal, 18)
            } else {
                VStack(spacing: 2) {
                    ForEach(documents) { document in item(document) }
                }
            }
            VStack(alignment: .leading, spacing: 8) {
                Footnote("Approvals are type-specific. There is no approve-all.")
                Footnote("The core does not report which documents wait for approval, nor their names, places or ages. Documents are listed by identity.")
            }
            .padding(.vertical, 14)
            .padding(.horizontal, 16)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private func item(_ document: ApprovalInboxState.Document) -> some View {
        let loaded = document.id == state.review?.subject
        return Button { actions.load(document.id) } label: {
            RequestRow(kind: document.kind, title: ApprovalInboxState.short(document.id), mono: true,
                       detail: "record \(ApprovalInboxState.short(document.record))",
                       status: loaded && state.stale ? .stale : nil, symbol: document.symbol, selected: loaded)
        }
        .buttonStyle(.plain)
        .padding(.horizontal, 8)
        .help("Document \(document.id). The core's record: \(document.record)")
        .accessibilityAddTraits(loaded ? .isSelected : [])
    }

    // MARK: Main column

    private func head(_ bundle: ReviewBundle) -> some View {
        let kind = ApprovalInboxState.kind(bundle)
        return VStack(alignment: .leading, spacing: 6) {
            WrappingRow(spacing: 8, lineSpacing: 6) {
                RequestTile(symbol: kind.symbol, awaiting: bundle.priorDecision == nil, size: 22)
                Text("\(state.summary?.kindName ?? kind.name) · \(kind.approval)").textStyle(.secondary).foregroundStyle(theme.text2)
                ReviewMark(state: state, bundle: bundle)
            }
            Text(ApprovalInboxState.title(bundle, state.summary)).font(.system(size: 20, weight: .bold)).tracking(-0.4)
                .foregroundStyle(theme.textStrong).help("Document \(bundle.subject)")
            Text("bundle \(ApprovalInboxState.short(bundle.hash))").textStyle(.mono).foregroundStyle(theme.text3)
                .help("Bundle digest \(bundle.hash)")
        }
    }

    private func facts(_ bundle: ReviewBundle) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Card(.panelList) {
                VStack(spacing: 0) {
                    FactLine("Requested action",
                             "Approve, reject or revoke exactly this revision. A decision binds every digest below; the core refuses it if any of them changed.")
                    FactLine("Why approval", ApprovalInboxState.kind(bundle).why)
                    if let summary = state.summary {
                        FactLine("Feature", summary.featureName ?? "Not reported")
                        FactLine("Requested", clockTime(summary.created, now: state.now) ?? "Not reported")
                        if let author = summary.author { FactLine("Written by", author == "Model" ? "The lead agent" : "A human") }
                    }
                    FactLine("Revision", "\(bundle.revision)")
                    FactLine("Content") { digest(bundle.contentHash) }
                    FactLine("Displayed") { digest(bundle.displayedHash) }
                    FactLine("Context") { digest(bundle.contextHash) }
                    FactLine("Upstream approvals", ApprovalInboxState.upstream(bundle))
                    FactLine("Latest decision", ApprovalInboxState.latestDecision(bundle, state.summary, now: state.now))
                    FactLine("Staleness", last: true) {
                        HStack(spacing: 8) {
                            if state.requested {
                                OperationalPill(.requested)
                                Text("a decision was sent · waiting for the core").foregroundStyle(theme.text3)
                            } else if state.stale {
                                DocumentTag(.stale)
                                Text("a decision was sent or refused · reload before deciding again").foregroundStyle(theme.text3)
                            } else {
                                EvidencePill(.passed, label: "Current")
                                Text("as loaded · the core checks again when you decide").foregroundStyle(theme.text3)
                            }
                        }
                    }
                }
            }
            Footnote(state.summary == nil
                     ? "Not reported: the document's name and text, its feature and repository, what it affects, its risks, and the time of the request."
                     : "Not reported: the document's text, its repository, what it affects and its risks.")
                .padding(.horizontal, 2)
        }
    }

    private func digest(_ value: String) -> some View {
        Text(value).font(.system(size: 11.5, design: .monospaced)).textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
    }

    private var evidence: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Supporting evidence")
            NoticeWell("No supporting evidence is reported. The core gives the digests of the document, not its text, a comparison with the earlier revision or its validation.")
        }
    }

    private func decision(_ bundle: ReviewBundle) -> some View {
        let blocked = state.decideBlocked
        return VStack(alignment: .leading, spacing: 10) {
            if state.stale {
                Banner(.warning, title: "This review is stale",
                       message: "A decision was sent or refused for this revision. Reload the document before deciding again; nothing is resent.") {
                    Button("Reload") { actions.load(bundle.subject) }.buttonStyle(.jojo(.secondary, compact: true))
                        .disabled(state.link != .connected)
                }
            }
            WrappingRow(spacing: 8, lineSpacing: 8) {
                Button(ApprovalInboxState.kind(bundle).approve) { actions.decide(.Approve) }.buttonStyle(.jojo(.primary))
                Button("Reject…") { confirming = .Reject }.buttonStyle(.jojo(.destructive))
                Button("Revoke Approval…") { confirming = .Revoke }.buttonStyle(.jojo(.destructive))
            }
            .disabled(blocked != nil)
            if state.requested {
                RequestedNotice(text: "Sent to core · waiting for confirmation. Nothing is shown as approved until the core records it.")
            } else if let blocked {
                DisabledReason(blocked).fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    // MARK: Trailing panel

    private var trailing: some View {
        VStack(alignment: .leading, spacing: 16) {
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Approval policy")
                Card(.panelList) {
                    VStack(spacing: 0) {
                        ValueLine("Approval kinds", "Spec, plan, push, PR")
                        ValueLine("Approve all", "None")
                        ValueLine("Approvals expire", "On content change")
                        ValueLine("Push", "Not available yet")
                        ValueLine("Pull request", "Not available yet", last: true)
                    }
                }
                Footnote("Push and pull request approvals belong to milestone M5. The core has no such command and reports no such request.")
                    .padding(.horizontal, 2)
            }
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Candidate decision")
                Card(.panel) {
                    VStack(alignment: .leading, spacing: 10) {
                        Text("Accept and reject of a candidate are on Verification & Evidence.")
                            .textStyle(.body).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
                        if let c = state.candidate {
                            FactGrid {
                                Fact("Attempt") { identifier(c.attempt) }
                                Fact("Candidate") {
                                    if let digest = c.candidate { identifier(digest) } else { missing("None") }
                                }
                                Fact("Validation") { EvidencePill(VerificationState.evidence(c.validation)) }
                                Fact("Decision", VerificationState.decision(c))
                            }
                        } else {
                            missing("No attempt result is loaded.")
                        }
                        Button("Open Verification & Evidence") { actions.open(.verification) }
                            .buttonStyle(.jojo(.secondary, compact: true))
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Recently decided") {
                    Button("Approval History") { actions.open(ApprovalInboxState.historyScreen) }
                        .buttonStyle(.jojo(.secondary, compact: true))
                }
                Group {
                    Footnote("Decisions already made are not listed: the core reports only the event of the latest decision on the loaded document.")
                    Footnote(ApprovalInboxState.historyNote("Approval History"))
                }
                .padding(.horizontal, 2)
            }
            Footnote("Approving sends a request. The core checks validity again before recording it.")
                .padding(.horizontal, 2)
        }
    }

    private func identifier(_ identity: String) -> some View {
        Text(ApprovalInboxState.short(identity)).textStyle(.mono).foregroundStyle(theme.text).help(identity)
    }

    private func missing(_ words: String) -> some View {
        Text(words).textStyle(.meta).foregroundStyle(theme.text3)
    }
}

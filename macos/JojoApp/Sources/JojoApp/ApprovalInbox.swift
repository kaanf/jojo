import JojoCore
import SwiftUI

// Approval Inbox (docs/design/screens/APPROVAL_INBOX.md), from `ApprovalInboxMac.dc.html`:
// what waits for the human (pending documents and candidates whose host validation
// passed), one request in detail, the approval policy and the newest decisions. A document
// is decided here with `decide` for the review the workspace holds; a candidate is accepted
// on Verification & Evidence. M5: a push or draft pull request request (protocol 1.10) is
// authorized or declined here, each on its own, after a confirmation that names the exact
// reference, commit and remote.

/// Pending or All, the list's segmented control.
enum InboxFilter: String, CaseIterable, Identifiable, Sendable {
    case pending = "Pending", all = "All"
    var id: String { rawValue }
}

/// The type menu of the list.
enum InboxType: String, CaseIterable, Identifiable, Sendable {
    case all = "All types", specs = "Specs", plans = "Plans", candidates = "Candidates", external = "External"
    var id: String { rawValue }
}

/// One request of the list: a document or a candidate.
struct InboxItem: Identifiable, Equatable {
    enum Subject: Equatable {
        case document(DocumentSummary)
        case candidate(PendingCandidate)
        case external(ExternalAction)
    }
    let subject: Subject

    var id: String {
        switch subject {
        case .document(let d): return d.id
        case .candidate(let c): return c.id
        case .external(let a): return a.id
        }
    }

    var kind: String {
        switch subject {
        case .document(let d): return d.kindName
        case .candidate: return "Candidate"
        case .external(let a): return a.kind == .Push ? "Push" : "Pull request"
        }
    }

    var dot: RGBA {
        switch subject {
        case .document(let d): return documentTile(d.kind).1
        case .candidate: return Palette.green
        case .external: return Palette.orange
        }
    }

    var title: String {
        switch subject {
        case .document(let d): return "\(d.title ?? documentCode(d.kind)) rev \(d.revision)"
        case .candidate(let c): return [c.key, c.title].compactMap { $0 }.joined(separator: " · ").nonEmpty ?? "Candidate"
        case .external(let a): return ExternalWords.title(a)
        }
    }

    var waiting: Bool {
        switch subject {
        case .document(let d): return d.approval == "Pending"
        case .candidate: return true
        case .external(let a): return a.state == .Requested
        }
    }

    var date: Date? {
        switch subject {
        case .document(let d): return d.created
        case .candidate(let c): return c.validated
        case .external(let a): return a.requested
        }
    }

    var featureName: String? {
        switch subject {
        case .document(let d): return d.featureName
        case .candidate(let c): return c.featureName
        case .external(let a): return a.featureName
        }
    }

    func matches(_ type: InboxType) -> Bool {
        switch (type, subject) {
        case (.all, _): return true
        case (.specs, .document(let d)): return ["Prd", "Adr"].contains(d.kind)
        case (.plans, .document(let d)): return ["ExecutionPlan", "ValidationPlan"].contains(d.kind)
        case (.candidates, .candidate): return true
        case (.external, .external): return true
        default: return false
        }
    }
}

/// What the screen shows. Every value is the helper's answer.
struct ApprovalInboxState: Equatable {
    var connection: HelperConnection
    var admission: String
    /// Protocol 1.6: the documents, the candidates that wait and how many things wait.
    var list: DocumentList?
    /// The review the workspace holds; a decision is sent for it.
    var review: ReviewBundle?
    var reviewStale = false
    var decisionOutstanding = false
    /// The chosen request.
    var selected: String?
    /// Protocol 1.9: the chosen document's revisions and decisions.
    var detail: DocumentDetail?
    /// Protocol 1.9: the newest decisions.
    var decisions: [DecisionSummary] = []
    var decisionsLoad = PlanLoad.idle
    /// The features (1.7), for a request's project and code.
    var features: FeatureList?
    /// M5 (1.10): push and pull request requests with their exact payload, and how git and
    /// gh are signed in.
    var external: ExternalList?
    var externalLoad = PlanLoad.idle
    var commandPending = false
    var filter = InboxFilter.pending
    var type = InboxType.all
    var reportsDetail = true
    var now = Date()

    var link: RecoveryCenterState.Link { coreLink(connection) }

    var items: [InboxItem] {
        let documents = (list?.documents ?? []).map { InboxItem(subject: .document($0)) }
        let candidates = (list?.candidates ?? []).map { InboxItem(subject: .candidate($0)) }
        let requests = (external?.actions ?? []).map { InboxItem(subject: .external($0)) }
        let all = (documents + candidates + requests).filter { $0.matches(type) }
        let shown = filter == .pending ? all.filter(\.waiting) : all
        return shown.sorted { ($0.date ?? .distantPast) > ($1.date ?? .distantPast) }
    }

    var chosen: InboxItem? {
        let all = items
        return all.first { $0.id == selected } ?? all.first
    }

    var pending: Int64 { list?.pending ?? 0 }

    /// "Sample · FT-3": the project and code of a request's feature.
    func place(_ item: InboxItem) -> String {
        let summary = features?.features.first { f in
            switch item.subject {
            case .document(let d): return f.id == d.feature
            case .candidate(let c): return f.name != nil && f.name == c.featureName
            case .external(let a): return f.name != nil && f.name == a.featureName
            }
        }
        return [summary?.projectName, summary?.code ?? item.featureName].compactMap { $0 }.joined(separator: " · ")
            .nonEmpty ?? "Workspace"
    }

    /// "18m", "1h", "1d": how long a request has waited.
    func age(_ item: InboxItem) -> String {
        if case .document(let d) = item.subject, d.stale { return "Stale" }
        guard let date = item.date else { return "" }
        let s = max(0, Int(now.timeIntervalSince(date)))
        if s < 3600 { return "\(max(1, s / 60))m" }
        if s < 86_400 { return "\(s / 3600)h" }
        return "\(s / 86_400)d"
    }

    /// The loaded review is the chosen document's, current and not on its way.
    var decidable: Bool {
        guard case .document(let d)? = chosen?.subject, let review else { return false }
        return review.subject == d.id && review.revision == d.revision
    }

    /// Why Approve, Reject and Revoke are disabled; nil when they can be sent. Which of them
    /// the core accepts for a document is the core's rule.
    var decideBlocked: String? {
        guard case .document(let d)? = chosen?.subject else { return "No document chosen." }
        if link != .connected { return "Unavailable: the core is not connected." }
        if decisionOutstanding { return "Unavailable while a decision awaits the core." }
        if d.approval == "Waiting" { return "Waits for the documents it was drafted on." }
        if d.approval == "NotReady" { return "The requirements have findings; the core does not accept an approval yet." }
        if !decidable || reviewStale { return "Loading the review of this revision." }
        return nil
    }

    /// The pill in the detail's head.
    var status: (String, Tone) {
        guard let chosen else { return ("", .neutral) }
        switch chosen.subject {
        case .candidate: return ("Awaiting your decision", .info)
        case .external(let a):
            if a.state == .Requested { return commandPending ? ("Requested", .review) : ("Awaiting authorization", .info) }
            return ExternalWords.pill(a)
        case .document(let d):
            if d.stale { return ("Stale", .warn) }
            if decisionOutstanding { return ("Requested", .review) }
            return d.approval == "Pending" ? ("Awaiting approval", .info) : approvalPill(d.approval)
        }
    }

    struct Kind: Equatable {
        let name: String
        let why: String
        /// "Approve Revision", or "Approve Plan".
        let approve: String
    }

    static func kind(_ kind: String) -> Kind {
        switch kind {
        case "Prd":
            return Kind(name: "Specification", why: "Requirements bind planning for the feature. An approval does not carry over to changed content.",
                        approve: "Approve Revision")
        case "Adr":
            return Kind(name: "Architecture decision", why: "Architecture decisions bind every plan that builds on them. An approval does not carry over to changed content.",
                        approve: "Approve Revision")
        case "ValidationPlan":
            return Kind(name: "Validation plan", why: "Host validation checks every candidate of the plan against it. Approving does not start anything.",
                        approve: "Approve Plan")
        default:
            return Kind(name: "Execution plan", why: "Only approved plans can admit tasks. Approving does not start anything.",
                        approve: "Approve Plan")
        }
    }

    /// "Approve PRD rev 2 of Slugify from the shell."
    static func action(_ d: DocumentSummary) -> String {
        let of = d.featureName.map { " of \($0)" } ?? ""
        switch d.kind {
        case "ExecutionPlan": return "Approve plan rev \(d.revision)\(of). Its steps may then be admitted; nothing starts by itself."
        case "ValidationPlan": return "Approve validation plan rev \(d.revision)\(of): the checks each step's candidate must pass."
        default: return "Approve this exact revision: \(documentCode(d.kind)) rev \(d.revision)\(of)."
        }
    }

    /// What a decision on this document affects, from what was drafted on it.
    func affected(_ d: DocumentSummary) -> String {
        guard let detail, detail.id == d.id else { return "" }
        if !detail.downstream.isEmpty {
            return detail.downstream.map { "\(documentCode($0.kind)) rev \($0.revision ?? 1)" }.joined(separator: " · ")
        }
        if !detail.upstream.isEmpty {
            return "Drafted on " + detail.upstream.map { "\(documentCode($0.kind)) rev \($0.revision ?? 1)" }.joined(separator: " · ")
        }
        return d.kind == "Prd" ? "Planning for \(d.featureName ?? "the feature")" : ""
    }

    func repository(_ item: InboxItem) -> String? {
        switch item.subject {
        case .document(let d): return features?.features.first { $0.id == d.feature }?.repositoryName
        case .candidate(let c): return features?.features.first { $0.name == c.featureName }?.repositoryName
        case .external(let a): return features?.features.first { $0.name == a.featureName }?.repositoryName
        }
    }

    static func short(_ identity: String) -> String {
        String(identity.prefix(12))
    }

    static let documentKinds: Set<String> = ["artifact", "review_document"]

    /// One document the snapshot knows (Activity & Audit's "Go to").
    struct SnapshotDocument: Identifiable, Equatable {
        let id: String
    }

    /// The snapshot's document rows, in its order.
    static func documents(in rows: [SnapshotRow]) -> [SnapshotDocument] {
        var seen = Set<String>()
        return rows.filter { documentKinds.contains($0.kind) && seen.insert($0.subject).inserted }.map {
            SnapshotDocument(id: $0.subject)
        }
    }

    static func confirmation(_ action: DecisionAction, _ d: DocumentSummary)
        -> (title: String, message: String, confirm: String, keep: String)? {
        let object = "\(documentCode(d.kind)) rev \(d.revision)\(d.featureName.map { " of \($0)" } ?? "")"
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

    /// "Approved", "Rejected", "Revoked", or "Invalidated" for an approval that no longer
    /// applies.
    static func decisionPill(_ d: DecisionSummary) -> (String, Tone) {
        if d.decision == "Approved" && !d.applies { return ("Invalidated", .danger) }
        return approvalPill(d.decision)
    }
}

struct ApprovalInboxActions {
    var select: (InboxItem) -> Void = { _ in }
    var decide: (DecisionAction) -> Void = { _ in }
    var open: (Screen) -> Void = { _ in }
    /// Opens Specifications comparing the chosen document's two newest revisions.
    var compare: (String) -> Void = { _ in }
    /// Opens Feature Chat with a note to the lead about the chosen document.
    var requestChanges: (DocumentSummary) -> Void = { _ in }
    /// Loads a candidate and opens Verification & Evidence, where it is decided.
    var candidate: (String) -> Void = { _ in }
    var filter: (InboxFilter) -> Void = { _ in }
    var type: (InboxType) -> Void = { _ in }
    /// M5: authorizes (true) or declines (false) one push or pull request request.
    var authorize: (ExternalAction, Bool) -> Void = { _, _ in }
}

struct ApprovalInboxScreen: View {
    @ObservedObject var model: AppModel
    let visible: Bool
    let open: (Screen) -> Void
    /// Opens Specifications in Compare.
    let compare: (String) -> Void
    @State private var selected: String?
    @State private var filter = InboxFilter.pending
    @State private var type = InboxType.all

    var body: some View {
        ApprovalInboxView(state: state, actions: actions)
            .onAppear { if visible { read() } }
            .onChange(of: visible) { if visible { read() } }
            .onChange(of: model.workspace.snapshotSeq) { if visible { read() } }
            .onChange(of: selected) { if visible { read() } }
            .onChange(of: model.workspace.documents) { if visible { read() } }
    }

    private var state: ApprovalInboxState {
        let workspace = model.workspace
        return ApprovalInboxState(
            connection: model.client.connection, admission: workspace.admission, list: workspace.documents,
            review: workspace.review, reviewStale: workspace.reviewStale,
            decisionOutstanding: model.client.outstandingCommands.values.contains("HumanDecision"),
            selected: selected ?? workspace.review?.subject, detail: workspace.document, decisions: workspace.decisions,
            decisionsLoad: workspace.decisionsLoad, features: workspace.features,
            external: workspace.external, externalLoad: workspace.externalLoad,
            commandPending: workspace.commandPending, filter: filter, type: type,
            reportsDetail: model.client.hello.map { $0.queries.contains("Decisions") } ?? true)
    }

    /// The decisions, the features and the chosen document's review and revisions.
    private func read() {
        let workspace = model.workspace
        workspace.loadDecisions()
        workspace.loadFeatures()
        workspace.loadExternalActions()
        guard case .document(let d)? = state.chosen?.subject else { return }
        if workspace.review?.subject != d.id || workspace.review?.revision != d.revision
            || (workspace.reviewStale && !state.decisionOutstanding) {
            workspace.loadReview(d.id)
        }
        workspace.loadDocument(d.id)
    }

    private var actions: ApprovalInboxActions {
        let workspace = model.workspace
        return ApprovalInboxActions(
            select: { selected = $0.id },
            decide: { workspace.decide($0) },
            open: open,
            compare: compare,
            requestChanges: { d in
                if let feature = d.feature { workspace.openChat(feature) }
                workspace.chatDraft = "Changes requested on \(documentCode(d.kind)) rev \(d.revision): "
                open(.featureChat)
            },
            candidate: { attempt in
                workspace.loadCandidate(attempt)
                open(.verification)
            },
            filter: { filter = $0 },
            type: { type = $0 },
            authorize: { workspace.decideExternal($0, authorize: $1) })
    }
}

struct ApprovalInboxView: View {
    @Environment(\.theme) private var theme
    @Environment(\.offscreen) private var offscreen
    let state: ApprovalInboxState
    var actions = ApprovalInboxActions()
    /// The decision the confirmation dialog asks about: Reject or Revoke.
    @State private var confirming: DecisionAction?
    /// M5: the push or pull request the authorization dialog asks about.
    @State private var authorizing: ExternalAction?

    var body: some View {
        GeometryReader { geometry in
            let beside = geometry.size.width >= 760
            Scrolling {
                VStack(alignment: .leading, spacing: 14) {
                    CoreBanners(connection: state.connection,
                                missing: state.reportsDetail ? nil : ("Decisions", "decisions"))
                    if beside {
                        HStack(alignment: .top, spacing: 14) {
                            leading.frame(width: min(340, max(280, geometry.size.width * 0.3)))
                            detail
                        }
                    } else {
                        leading
                        detail
                    }
                }
                .padding(EdgeInsets(top: 6, leading: 10, bottom: 14, trailing: 12))
            }
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
        guard let confirming, case .document(let d)? = state.chosen?.subject else { return nil }
        return ApprovalInboxState.confirmation(confirming, d)
    }

    // MARK: Leading column

    private var leading: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                ChoiceCapsule(options: [(InboxFilter.pending.rawValue, "Pending \(state.pending)"), (InboxFilter.all.rawValue, "All")],
                              selection: state.filter.rawValue, label: "Status") {
                    actions.filter(InboxFilter(rawValue: $0) ?? .pending)
                }
                Spacer(minLength: 0)
                // A menu does not draw off screen; its title stands in for it there.
                Group {
                    if offscreen {
                        Text(state.type.rawValue).font(.system(size: 12, weight: .medium))
                    } else {
                        Menu {
                            ForEach(InboxType.allCases) { t in Button(t.rawValue) { actions.type(t) } }
                        } label: {
                            Text(state.type.rawValue).font(.system(size: 12, weight: .medium))
                        }
                        .menuStyle(.borderlessButton)
                        .fixedSize()
                    }
                }
                .padding(.horizontal, 12)
                .frame(height: 32)
                .background(theme.glassStrong, in: Capsule())
                .accessibilityLabel("Filter by type")
            }
            list
            policy
            recent
        }
    }

    @ViewBuilder
    private var list: some View {
        VStack(alignment: .leading, spacing: 2) {
            if state.type == .external && state.external == nil {
                NotReported(state.externalLoad == .loading ? "Reading…"
                            : "This core does not report push and pull request requests.").padding(10)
            } else if state.list == nil {
                NotReported("This core does not list what waits.").padding(10)
            } else if state.items.isEmpty {
                QuietCard(state.filter == .pending ? "Nothing waits for you." : "No requests yet.")
            }
            ForEach(state.items) { item in
                SelectableRow(selected: item.id == state.chosen?.id, action: { actions.select(item) }) {
                    Circle().fill(item.dot.color).frame(width: 8, height: 8)
                    VStack(alignment: .leading, spacing: 2) {
                        HStack(spacing: 8) {
                            Text(item.kind).textStyle(.secondary).foregroundStyle(theme.text3).frame(maxWidth: .infinity, alignment: .leading)
                            Text(state.age(item)).textStyle(.secondary).foregroundStyle(theme.text3)
                        }
                        Text(item.title).textStyle(.bodyStrong).lineLimit(1)
                        Text(state.place(item)).textStyle(.secondary).foregroundStyle(theme.text2).lineLimit(1)
                    }
                }
            }
        }
    }

    /// The decisions in force, as the board's card states them.
    private var policy: some View {
        VStack(alignment: .leading, spacing: 10) {
            GroupLabel("Approval policy")
            HStack { Text("Push and PR").textStyle(.body); Spacer(); OperationalPill("Separate approvals", tone: .neutral) }
            HStack { Text("Approvals expire").textStyle(.body); Spacer(); OperationalPill("On content change", tone: .neutral) }
            HStack { Text("Approve all").textStyle(.body); Spacer(); OperationalPill("Never", tone: .neutral) }
        }
        .padding(12)
        .cardElevation(radius: 14)
    }

    private var recent: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                GroupLabel("Recently decided")
                Spacer()
                TextLink("History") { actions.open(.activity) }.font(.system(size: 12))
            }
            .padding(EdgeInsets(top: 10, leading: 12, bottom: 4, trailing: 12))
            if state.decisions.isEmpty {
                NotReported(state.decisionsLoad == .loading ? "Reading…" : "No decision recorded yet.").padding(12)
            }
            ForEach(state.decisions.prefix(6)) { d in
                HStack(spacing: 8) {
                    Text("\(documentCode(d.kind)) rev \(d.revision)\(d.featureName.map { " · \($0)" } ?? "")")
                        .textStyle(.body).lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
                    let pill = ApprovalInboxState.decisionPill(d)
                    OperationalPill(pill.0, tone: pill.1)
                    Text(d.at.map { $0.formatted(.dateTime.month(.abbreviated).day()) } ?? "")
                        .textStyle(.secondary).foregroundStyle(theme.text3).frame(width: 46, alignment: .trailing)
                }
                .padding(EdgeInsets(top: 8, leading: 12, bottom: 8, trailing: 12))
            }
        }
        .cardElevation(radius: 14)
    }

    // MARK: Detail

    @ViewBuilder
    private var detail: some View {
        VStack(alignment: .leading, spacing: 20) {
            switch state.chosen?.subject {
            case .document(let d)?: document(d)
            case .candidate(let c)?: candidate(c)
            case .external(let a)?: external(a)
            case nil:
                Placeholder(symbol: "tray", tile: Palette.purple, title: "Nothing waits for you",
                            message: "Requirements, decisions, plans and candidates appear here when they need your decision.") { EmptyView() }
            }
        }
        .padding(EdgeInsets(top: 24, leading: 28, bottom: 28, trailing: 28))
        .frame(maxWidth: .infinity, alignment: .topLeading)
        .cardElevation(radius: 18)
        .confirmationDialog(authorizing.map { ExternalWords.confirmation($0).title } ?? "",
                            isPresented: Binding(get: { authorizing != nil }, set: { if !$0 { authorizing = nil } }),
                            titleVisibility: .visible) {
            if let action = authorizing {
                Button(ExternalWords.confirmation(action).confirm) { actions.authorize(action, true) }
                Button("Cancel", role: .cancel) {}
            }
        } message: {
            Text(authorizing.map { ExternalWords.confirmation($0).message } ?? "")
        }
    }

    private func head(sub: String, title: String, mono: String) -> some View {
        HStack(alignment: .top, spacing: 12) {
            VStack(alignment: .leading, spacing: 6) {
                Text(sub).textStyle(.secondary).foregroundStyle(theme.text2)
                Text(title).font(.system(size: 26, weight: .bold)).tracking(-0.5)
                Text(mono).textStyle(.mono).foregroundStyle(theme.text2).textSelection(.enabled)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            OperationalPill(state.status.0, tone: state.status.1)
        }
    }

    private func document(_ d: DocumentSummary) -> some View {
        let kind = ApprovalInboxState.kind(d.kind)
        let item = InboxItem(subject: .document(d))
        let digest = state.detail?.id == d.id ? state.detail?.revisions.first { $0.revision == d.revision }?.digest : nil
        let previous = state.detail?.id == d.id ? state.detail?.previous : nil
        return VStack(alignment: .leading, spacing: 20) {
            head(sub: "\(kind.name) · \(state.place(item))", title: item.title,
                 mono: digest.map { "sha256 \(SpecificationsView.shortDigest($0))" } ?? "document \(ApprovalInboxState.short(d.id))")
            if d.stale {
                TintedNotice(tone: .warn, title: "This request is stale",
                             message: "An earlier revision was decided; rev \(d.revision) is the one to review now.") { EmptyView() }
            }
            VStack(alignment: .leading, spacing: 6) {
                GroupLabel("Requested action").padding(.horizontal, -4)
                Text(ApprovalInboxState.action(d)).font(.system(size: 16, weight: .medium)).fixedSize(horizontal: false, vertical: true)
            }
            DefinitionCard(rows: [
                Definition(key: "Why approval", value: kind.why),
                Definition(key: "Affected", value: state.affected(d), mono: true,
                           pill: state.repository(item).map { ($0, .neutral) }),
                Definition(key: "Risk and side effects", value: "Not reported"),
                Definition(key: "Staleness", value: d.stale ? "an earlier revision was decided" : "checked by core \(boardTime(state.now, now: state.now) ?? "")",
                           pill: d.stale ? ("Stale", .warn) : ("Current", .ok)),
            ], keyWidth: 130)
            VStack(alignment: .leading, spacing: 6) {
                GroupLabel("Supporting evidence").padding(.horizontal, -4)
                VStack(spacing: 0) {
                    if let previous {
                        evidenceRow("Diff rev \(previous.revision) → \(d.revision)") { actions.compare(d.id) }
                    }
                    if d.kind == "Prd" {
                        let findings = state.detail?.id == d.id ? state.detail?.findings?.count ?? 0 : 0
                        evidenceRow(findings == 0 ? "Validation: no findings" : "Validation: \(findings) findings") { actions.open(.specifications) }
                    }
                    evidenceRow("The document in Specifications") { actions.open(.specifications) }
                    evidenceRow("Questions and answers in Feature Chat") { actions.open(.featureChat) }
                }
                .cardElevation(radius: 14)
            }
            decisionBar(d, kind: kind, previous: previous)
        }
    }

    private func evidenceRow(_ title: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 10) {
                Image(systemName: "doc.text.magnifyingglass").foregroundStyle(theme.text2)
                Text(title).textStyle(.body).frame(maxWidth: .infinity, alignment: .leading)
                Image(systemName: "chevron.right").font(.system(size: 11, weight: .semibold)).foregroundStyle(theme.text3)
            }
            .padding(EdgeInsets(top: 10, leading: 14, bottom: 10, trailing: 14))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func decisionBar(_ d: DocumentSummary, kind: ApprovalInboxState.Kind, previous: DocumentRevision?) -> some View {
        let blocked = state.decideBlocked
        return FlowRow(spacing: 10) {
            if d.approval == "Approved" {
                Button("Revoke…") { confirming = .Revoke }.buttonStyle(.jojo(.secondary))
                    .disabled(blocked != nil).help(blocked ?? "Records that this revision is no longer approved.")
            } else {
                Button(kind.approve) { actions.decide(.Approve) }.buttonStyle(.jojo(.primary))
                    .disabled(blocked != nil).help(blocked ?? "Records your approval of this exact revision.")
                Button("Request Changes") { actions.requestChanges(d) }.buttonStyle(.jojo(.secondary))
                    .disabled(d.feature == nil)
                Button("Reject…") { confirming = .Reject }.buttonStyle(.jojo(.ghost))
                    .disabled(blocked != nil).help(blocked ?? "Records your decision to reject this revision.")
            }
            if previous != nil {
                TextLink("Compare Revisions") { actions.compare(d.id) }
            }
            Text(state.decisionOutstanding ? "Sent to core · waiting for confirmation. Nothing is shown as approved until the core records it."
                 : "Approving sends a request. The core checks validity again before recording it.")
                .font(.system(size: 11)).foregroundStyle(theme.text2).frame(maxWidth: 260, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassPanel(radius: 18)
    }

    private func candidate(_ c: PendingCandidate) -> some View {
        let item = InboxItem(subject: .candidate(c))
        return VStack(alignment: .leading, spacing: 20) {
            head(sub: "Candidate · \(state.place(item))", title: item.title,
                 mono: "attempt \(ApprovalInboxState.short(c.id)) · receipt event \(c.receipt)")
            VStack(alignment: .leading, spacing: 6) {
                GroupLabel("Requested action").padding(.horizontal, -4)
                Text("Accept this candidate so the plan can integrate it, or reject it.")
                    .font(.system(size: 16, weight: .medium)).fixedSize(horizontal: false, vertical: true)
            }
            DefinitionCard(rows: [
                Definition(key: "Why approval", value: "Host validation passed. Only a person accepts a candidate; Jojo never does."),
                Definition(key: "Affected", value: "", pill: state.repository(item).map { ($0, .neutral) } ?? ("Repository not reported", .neutral)),
                Definition(key: "Risk and side effects", value: "Not reported"),
                Definition(key: "Validated", value: boardTime(c.validated, now: state.now) ?? "Not reported", pill: ("Passed", .ok)),
            ], keyWidth: 130)
            FlowRow(spacing: 10) {
                Button("Open in Verification") { actions.candidate(c.id) }.buttonStyle(.jojo(.primary))
                Text("Accept and Reject are on Verification & Evidence, beside the validation they rest on.")
                    .font(.system(size: 11)).foregroundStyle(theme.text2).frame(maxWidth: 300, alignment: .leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .glassPanel(radius: 18)
        }
    }

    // MARK: Push and pull request (M5)

    private func external(_ a: ExternalAction) -> some View {
        let item = InboxItem(subject: .external(a))
        let signIn = a.kind == .Push ? state.external?.git : state.external?.gh
        return VStack(alignment: .leading, spacing: 20) {
            head(sub: "External action · \(state.place(item))", title: item.title, mono: "commit \(a.commit)")
            if a.state == .Unknown {
                TintedNotice(tone: .warn, title: "The outcome of this action is unknown",
                             message: "It was issued once and its result was not observed. Jojo never sends it again; inspect the remote in the Recovery Center.") {
                    Button("Inspect in Recovery") { actions.open(.recoveryCenter) }.buttonStyle(.jojo(.secondary))
                }
            }
            VStack(alignment: .leading, spacing: 6) {
                GroupLabel("Requested action").padding(.horizontal, -4)
                Text(ExternalWords.requested(a)).font(.system(size: 16, weight: .medium)).fixedSize(horizontal: false, vertical: true)
            }
            DefinitionCard(rows: [
                Definition(key: "Why approval", value: "It leaves this Mac. Push and pull request are separate approvals, and neither is retried automatically."),
                Definition(key: "Remote", value: "\(a.remote) · \(a.url)", mono: true,
                           pill: state.repository(item).map { ($0, .neutral) }),
                Definition(key: "Reference", value: a.kind == .Push ? a.reference : "\(a.branch) → \(a.base ?? "")", mono: true),
                Definition(key: "Commit", value: a.commit, mono: true),
                Definition(key: "Risk and side effects", value: ExternalWords.risk(a)),
                Definition(key: a.kind == .Push ? "git sign-in" : "gh sign-in", value: signIn?.detail ?? "Not reported",
                           pill: signIn.map { ExternalWords.signIn($0) }),
            ], keyWidth: 130)
            if a.kind == .PullRequest, let body = a.body {
                VStack(alignment: .leading, spacing: 6) {
                    GroupLabel("Title and body, exactly as they are sent").padding(.horizontal, -4)
                    VStack(alignment: .leading, spacing: 8) {
                        Text(a.title ?? "").textStyle(.bodyStrong)
                        Text(body).textStyle(.mono).foregroundStyle(theme.text2).textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .padding(14)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .cardElevation(radius: 14)
                }
            }
            if !a.observations.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    GroupLabel("What was observed").padding(.horizontal, -4)
                    VStack(spacing: 0) {
                        ForEach(Array(a.observations.enumerated()), id: \.offset) { _, o in
                            HStack(alignment: .firstTextBaseline, spacing: 10) {
                                Text(ExternalWords.observer(o)).textStyle(.bodyStrong).frame(width: 190, alignment: .leading)
                                Text(o.detail).textStyle(.body).foregroundStyle(theme.text2)
                                    .frame(maxWidth: .infinity, alignment: .leading).fixedSize(horizontal: false, vertical: true)
                                OperationalPill(o.status, tone: o.status == "Committed" ? .ok : o.status == "Rejected" ? .danger : .warn)
                            }
                            .padding(EdgeInsets(top: 9, leading: 14, bottom: 9, trailing: 14))
                        }
                    }
                    .cardElevation(radius: 14)
                }
            }
            externalBar(a)
        }
    }

    private func externalBar(_ a: ExternalAction) -> some View {
        let blocked = ExternalWords.blocked(a, list: state.external, connected: state.link == .connected,
                                            pending: state.commandPending)
        return FlowRow(spacing: 10) {
            if a.state == .Requested {
                Button(ExternalWords.authorize(a.kind)) { authorizing = a }.buttonStyle(.jojo(.primary))
                    .disabled(blocked != nil).help(blocked ?? "Asks you to confirm, then sends this exact request once.")
                Button("Decline") { actions.authorize(a, false) }.buttonStyle(.jojo(.secondary))
                    .disabled(state.link != .connected || state.commandPending)
                    .help("Records that this request is not performed. Nothing is sent.")
            } else if a.state == .Unknown || a.state == .InFlight {
                Button("Inspect in Recovery") { actions.open(.recoveryCenter) }.buttonStyle(.jojo(.primary))
            }
            TextLink("Source Control") { actions.open(.sourceControl) }
            Text(state.commandPending ? "Sent to core · waiting for confirmation. Nothing is shown as pushed until the core records it."
                 : "Authorizing sends one request. The core checks the branch again and performs it once; it is never retried.")
                .font(.system(size: 11)).foregroundStyle(theme.text2).frame(maxWidth: 280, alignment: .leading)
                .fixedSize(horizontal: false, vertical: true)
            if let blocked, a.state == .Requested {
                Text(blocked).font(.system(size: 11)).foregroundStyle(theme.text2)
                    .frame(maxWidth: 280, alignment: .leading).fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassPanel(radius: 18)
    }
}

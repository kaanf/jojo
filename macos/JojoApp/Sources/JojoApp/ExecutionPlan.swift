import JojoCore
import SwiftUI

// Execution Plan (docs/design/screens/EXECUTION_PLAN.md): the approved plan as the core
// stores it (steps, edges, revision), one human Start for one of its steps, and what the
// core answered. The plan is read with `ApprovedPlan` while the screen is the shown one.
// The core reports no name, text or state of a step, so none is drawn.

/// Where each step of a plan is drawn. Waves are derived from the edges (the longest chain
/// of prerequisites before a step); lanes are the repositories in the order the plan
/// names them. Nothing here is reported by the core as such.
struct PlanGraph: Equatable {
    struct Node: Equatable, Identifiable {
        let step: ApprovedPlan.Step
        let wave: Int
        let lane: Int
        let row: Int
        var id: String { step.id }
    }

    struct Lane: Equatable {
        /// nil for steps the plan binds to no repository.
        let repository: String?
        let rows: Int
    }

    static let column: CGFloat = 236
    static let heading: CGFloat = 22
    static let gap: CGFloat = 8
    static let rowGap: CGFloat = 12
    static let inset: CGFloat = 20

    let nodes: [Node]
    let lanes: [Lane]
    let waves: Int
    /// The edges between two steps of this plan.
    let edges: [ApprovedPlan.Edge]
    /// The edges do not order the steps. The core refuses such a plan; nothing is drawn.
    let cyclic: Bool

    init(_ plan: ApprovedPlan) {
        let ids = Set(plan.steps.map(\.id))
        let edges = plan.edges.filter { ids.contains($0.prerequisite) && ids.contains($0.dependent) }
        var depth = Dictionary(uniqueKeysWithValues: plan.steps.map { ($0.id, 0) })
        var changed = true
        var passes = 0
        while changed, passes <= plan.steps.count {
            (changed, passes) = (false, passes + 1)
            for edge in edges where depth[edge.dependent, default: 0] <= depth[edge.prerequisite, default: 0] {
                depth[edge.dependent] = depth[edge.prerequisite, default: 0] + 1
                changed = true
            }
        }
        var repositories: [String?] = []
        for step in plan.steps where !repositories.contains(step.repository) { repositories.append(step.repository) }
        var taken: [[Int]: Int] = [:]
        let cyclic = changed
        let nodes = plan.steps.map { step -> Node in
            let lane = repositories.firstIndex(of: step.repository) ?? 0
            let wave = cyclic ? 0 : depth[step.id, default: 0]
            let row = taken[[lane, wave], default: 0]
            taken[[lane, wave]] = row + 1
            return Node(step: step, wave: wave, lane: lane, row: row)
        }
        self.cyclic = cyclic
        self.edges = edges
        self.nodes = nodes
        self.lanes = repositories.enumerated().map { index, repository in
            Lane(repository: repository, rows: taken.filter { $0.key[0] == index }.values.max() ?? 1)
        }
        self.waves = (nodes.map(\.wave).max() ?? 0) + 1
    }

    func height(_ lane: Lane) -> CGFloat {
        LaneBand.header + CGFloat(lane.rows) * TaskNode.size.height + CGFloat(lane.rows - 1) * Self.rowGap + 12
    }

    func top(lane index: Int) -> CGFloat {
        lanes.prefix(index).reduce(Self.heading) { $0 + height($1) + Self.gap }
    }

    func origin(_ node: Node) -> CGPoint {
        CGPoint(x: CGFloat(node.wave) * Self.column + Self.inset,
                y: top(lane: node.lane) + LaneBand.header + CGFloat(node.row) * (TaskNode.size.height + Self.rowGap))
    }

    var size: CGSize {
        CGSize(width: CGFloat(waves) * Self.column, height: max(Self.heading, top(lane: lanes.count) - Self.gap))
    }

    /// Where an edge leaves its prerequisite and reaches its dependent.
    func ends(_ edge: ApprovedPlan.Edge) -> (from: CGPoint, to: CGPoint)? {
        guard let from = nodes.first(where: { $0.id == edge.prerequisite }), let to = nodes.first(where: { $0.id == edge.dependent }) else { return nil }
        let (a, b) = (origin(from), origin(to))
        return (CGPoint(x: a.x + TaskNode.size.width, y: a.y + TaskNode.size.height / 2), CGPoint(x: b.x, y: b.y + TaskNode.size.height / 2))
    }
}

/// What the screen shows. Every field is the helper's answer or what the human typed.
struct ExecutionPlanState: Equatable {
    var connection: HelperConnection
    var admission: String
    var pid: Int32 = 0
    /// Hello's `slice`: whether the helper holds a slice configuration; nil when it did not say.
    var slice: Bool?
    /// The five identities as typed.
    var subjects = StartSubjects()
    /// A Start or recovery command is unanswered.
    var commandPending = false
    var outcome: StartOutcome?
    /// The approved plans the workspace read, and where that read stands.
    var plans: [ApprovedPlan] = []
    var load = PlanLoad.idle
    /// More approved plans exist than were read.
    var truncated = false
    /// The plan the human chose; nil shows the first.
    var chosenPlan: String?
    /// The step the human chose.
    var selected: String?

    /// What the plan section shows.
    enum Phase: Equatable {
        case reading
        /// The core is not connected and nothing was read.
        case unknown
        case none
        case failed(String)
        case shown(ApprovedPlan)
    }

    var plan: ApprovedPlan? { plans.first { $0.plan == chosenPlan } ?? plans.first }
    var step: ApprovedPlan.Step? { plan?.steps.first { $0.id == selected } }

    var phase: Phase {
        if let plan { return .shown(plan) }
        switch load {
        case .failed(let reason): return .failed(reason)
        case .loaded: return .none
        case .idle, .loading: return link == .lost || link == .incompatible ? .unknown : .reading
        }
    }

    /// The first twelve characters of an identity, as the other screens shorten one.
    static func short(_ identity: String) -> String { String(identity.prefix(12)) }

    /// The core's word for a provider, in the words of Settings & Providers.
    static func provider(_ word: String?) -> String {
        switch word {
        case "ClaudeCode"?: return "Claude Code"
        case let word?: return word
        case nil: return "No provider bound"
        }
    }

    static func repository(_ identity: String?) -> String {
        identity.map { "Repository \(short($0))" } ?? "No repository bound"
    }

    /// A sentence for a read of the plan that failed. The reason itself is shown as sent.
    static func loadFailure(_ reason: String) -> String {
        if reason.hasPrefix("UnknownType") { return "This core has no request that returns the approved plan." }
        if reason.hasPrefix("PayloadTooLarge") { return "The plan is too large for one answer of the core. It is not shown." }
        if reason == "malformed plan" { return "The core's answer was not a plan this app can read. Nothing is shown." }
        return "The core did not answer the read. Nothing is shown."
    }

    /// What an edge requires, from the predicate the plan stores. Whether it is met is not reported.
    static func requirement(_ edge: ApprovedPlan.Edge) -> String {
        if edge.predicate == "ArtifactApproved", let artifact = edge.artifact {
            return "Approval of \(short(artifact))" + (edge.artifactRevision.map { " rev \($0)" } ?? "")
        }
        if edge.predicate == "Evidence" {
            return "Evidence: " + [edge.evidenceKind, edge.evidence].compactMap { $0 }.joined(separator: " · ")
        }
        return edge.predicate
    }

    /// The identities a Start of `step` names, when the shown plan has that step.
    func subjects(picking step: String) -> StartSubjects? {
        guard let plan, plan.steps.contains(where: { $0.id == step }) else { return nil }
        return plan.subjects(step: step)
    }

    /// The five identities a Start names, in the order of the request.
    enum Field: String, CaseIterable, Identifiable, Sendable {
        case prd = "PRD", adr = "ADR", plan = "Plan", validation = "Validation plan", step = "Step"

        var id: String { rawValue }
        var label: String { rawValue }
        var path: WritableKeyPath<StartSubjects, String> {
            switch self {
            case .prd: return \.prd
            case .adr: return \.adr
            case .plan: return \.plan
            case .validation: return \.validation
            case .step: return \.step
            }
        }
    }

    static let fields = Field.allCases

    static let notReported = "Not reported"

    var link: RecoveryCenterState.Link {
        switch connection {
        case .ready: return .connected
        case .notStarted, .handshaking: return .starting
        case .incompatible: return .incompatible
        case .launchFailed, .channelLost, .ended: return .lost
        }
    }

    /// The words of the sidebar's status well, from the same value.
    var status: ShellStatus { ShellStatus(connection: connection, admission: admission, pid: pid) }

    /// Admission is stated only while the helper is connected and has reported it.
    var admissionClosed: Bool { link == .connected && !admission.isEmpty && admission != "Open" }

    /// A typed value that is not an identity; an empty field is not marked.
    func invalid(_ field: Field) -> Bool {
        let value = subjects[keyPath: field.path]
        return !value.isEmpty && !Identity.valid(value)
    }

    /// Why "Start Step" cannot be sent, or nil when it can. The helper decides the rest.
    var startBlocked: String? {
        if link != .connected { return "Unavailable: the core is not connected." }
        if slice == false { return "The core has no slice configuration; it cannot start a step." }
        if admissionClosed { return "Admission is closed. The core refuses a Start until it is open." }
        if commandPending { return "Unavailable while a command awaits the core." }
        if Self.fields.contains(where: { subjects[keyPath: $0.path].isEmpty }) { return "Enter all five identities." }
        let wrong = Self.fields.filter(invalid).map(\.label)
        return wrong.isEmpty ? nil : "Not an identity: \(wrong.joined(separator: ", "))."
    }

    /// Whether the helper holds a slice configuration, in the words of Settings & Providers.
    var sliceLine: (value: String, form: SettingForm) {
        guard link == .connected else { return ("Unknown: the core is not connected.", .missing) }
        switch slice {
        case true?: return ("Present", .plain)
        case false?: return ("Absent. A Start would be refused.", .plain)
        case nil: return (Self.notReported, .missing)
        }
    }

    /// A sentence for the code of a refusal. The code itself is shown as the helper sent it.
    static func refusal(_ code: String) -> (title: String, message: String) {
        let refused = "The core refused the Start"
        let reasons: [(String, String, String)] = [
            ("StartFailed", "The Start did not launch", "The core committed the Start, but nothing was launched."),
            ("AdmissionClosed", refused, "Admission is closed. Nothing was started."),
            ("SliceNotConfigured", refused, "The core has no slice configuration. Nothing was started."),
            ("CapacityOne", refused, "Another attempt is active or on hold. Nothing was started."),
            ("CapacityFull", refused, "Every unit of parallel capacity is in use by attempts or a lead turn. Nothing was started."),
            ("RecoveryHold", refused, "Earlier work is on hold. Resolve it in Recovery Center. Nothing was started."),
            ("StartRefused: missing or stale approval", refused,
             "An approval is missing or no longer current. Review the document in the Approval Inbox. Nothing was started."),
            ("StartRefused", refused, "The core could not match the step to an approved plan. Nothing was started."),
            ("DuplicateStart", refused, "This step was already started. Nothing was started again."),
            ("NoProductionAdapter", refused, "The provider of this step is not configured. Nothing was started."),
            ("AdapterRejected", refused, "The core rejected the provider's executable. Nothing was started."),
        ]
        let known = reasons.first { code.hasPrefix($0.0) }
        return (known?.1 ?? refused, known?.2 ?? "Nothing was started. The core's reason is below.")
    }

    struct Missing: Identifiable, Equatable {
        let label: String
        let value: String
        var id: String { label }
    }

    /// What the board draws and the core does not report, and the two decisions that are
    /// not built. No example stands in for any of them.
    static let missing = [
        Missing(label: "Step names", value: "Not reported. A plan stores the identity of a step, not a name or a description."),
        Missing(label: "Acceptance criteria", value: notReported),
        Missing(label: "File scope", value: notReported),
        Missing(label: "Step state", value: "Not reported, so no node has a status. An attempt's record is on Agent Runs."),
        Missing(label: "Gates", value: "Not reported as nodes. The core stores a gate as the predicate of an edge; whether it is met is not reported."),
        Missing(label: "Lifecycle", value: "Not reported. The core has no lifecycle stage of a feature."),
        Missing(label: "Plan checks", value: "Not reported. The core validates a plan when it is stored."),
        Missing(label: "Plan revisions", value: "Only the approved revision is reported. A newer revision that is not approved makes the approval stale, and then no plan is reported."),
        Missing(label: "Execution history", value: "Not reported for a step. Attempts are on Agent Runs."),
        Missing(label: "Repair", value: "Decided, not built: automatic, at most two tries, then a human hold."),
        Missing(label: "Parallel capacity", value: "Not reported, and not decided."),
    ]
}

struct ExecutionPlanActions {
    var edit: (StartSubjects) -> Void = { _ in }
    var start: () -> Void = {}
    var open: (Screen) -> Void = { _ in }
    /// Reads the approved plans again.
    var reload: () -> Void = {}
    var choosePlan: (String) -> Void = { _ in }
    /// Chooses a step: it is inspected and its identities fill the Start fields.
    var pick: (String) -> Void = { _ in }
}

struct ExecutionPlanScreen: View {
    @ObservedObject var model: AppModel
    /// The plan is read only while this destination is the shown one.
    let visible: Bool
    let open: (Screen) -> Void
    @State private var subjects = StartSubjects()
    @State private var chosenPlan: String?
    @State private var selected: String?

    var body: some View {
        let workspace = model.workspace
        let state = ExecutionPlanState(
            connection: model.client.connection, admission: workspace.admission, pid: model.client.processIdentifier,
            slice: model.client.hello?.slice, subjects: subjects, commandPending: workspace.commandPending,
            outcome: workspace.startOutcome, plans: workspace.plans, load: workspace.plansLoad,
            truncated: workspace.plansTruncated, chosenPlan: chosenPlan, selected: selected)
        ExecutionPlanView(
            state: state,
            actions: ExecutionPlanActions(
                edit: { subjects = $0 }, start: { workspace.start(subjects) }, open: open,
                reload: { workspace.loadPlans() },
                choosePlan: { (chosenPlan, selected) = ($0, nil) },
                pick: { step in
                    guard let picked = state.subjects(picking: step) else { return }
                    (selected, subjects) = (step, picked)
                }))
            .onChange(of: visible) { if visible { workspace.loadPlans() } }
            .onChange(of: workspace.snapshotSeq) { if visible { workspace.loadPlans() } }
    }
}

/// The graph scrolls sideways; off screen it is clipped, as `Scrolling` is.
private struct SidewaysScrolling<Content: View>: View {
    @Environment(\.offscreen) private var offscreen
    @ViewBuilder let content: () -> Content

    var body: some View {
        if offscreen {
            Color.clear.overlay(alignment: .topLeading) { content() }.clipped()
        } else {
            ScrollView(.horizontal) { content() }
        }
    }
}

struct ExecutionPlanView: View {
    @Environment(\.theme) private var theme
    let state: ExecutionPlanState
    var actions = ExecutionPlanActions()

    var body: some View {
        GeometryReader { geometry in
            // The inspector stands beside the plan only while the plan keeps at least 520.
            let beside = geometry.size.width >= 820 && state.plan != nil
            HStack(spacing: 0) {
                Scrolling {
                    VStack(alignment: .leading, spacing: 18) {
                        banners.frame(maxWidth: 760, alignment: .leading)
                        plan(inspectorBelow: !beside)
                        Group {
                            step
                            outcome
                            reported
                            missing
                        }
                        .frame(maxWidth: 760, alignment: .topLeading)
                    }
                    .padding(EdgeInsets(top: 18, leading: 22, bottom: 24, trailing: 22))
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                if beside, let plan = state.plan {
                    Scrolling { inspector(plan) }
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                        .background(theme.sidebar)
                        .overlay(alignment: .leading) { Rectangle().fill(theme.sidebarEdge).frame(width: 1) }
                        .frame(width: 300)
                }
            }
        }
    }

    // MARK: Plan

    @ViewBuilder
    private func plan(inspectorBelow: Bool) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            SectionHeading("Approved plan")
            switch state.phase {
            case .reading:
                LoadingBlock("Reading the approved plan…").frame(maxWidth: 760, minHeight: 140)
            case .unknown:
                StateBlock(symbol: "list.bullet.indent", title: "No plan was read",
                           message: "The core stopped answering before the approved plan was read.")
                    .frame(maxWidth: 760, minHeight: 140)
            case .none:
                StateBlock(symbol: "list.bullet.indent", title: "No approved plan",
                           message: "The core holds no plan with a current approval. A plan arrives as an imported bundle and is approved in the Approval Inbox. A newer revision that is not approved makes the approval stale.") {
                    Button("Open Approval Inbox") { actions.open(.approvalInbox) }.buttonStyle(.jojo(.secondary, compact: true))
                }
                .frame(maxWidth: 760, minHeight: 160)
            case .failed(let reason):
                VStack(alignment: .leading, spacing: 8) {
                    Banner(.warning, title: "The approved plan could not be read", message: ExecutionPlanState.loadFailure(reason)) {
                        Button("Try Again") { actions.reload() }.buttonStyle(.jojo(.secondary, compact: true))
                    }
                    identifier(reason).foregroundStyle(theme.text3)
                }
                .frame(maxWidth: 760, alignment: .leading)
            case .shown(let plan):
                revision(plan)
                if state.plans.count > 1 { chooser(plan) }
                graph(plan)
                if inspectorBelow { Card(.panelList) { inspector(plan) }.frame(maxWidth: 760, alignment: .leading) }
            }
        }
    }

    /// The board's revision notice with the one revision the core reports.
    private func revision(_ plan: ApprovedPlan) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text("Plan rev \(plan.revision) is approved.").textStyle(.bodyStrong).foregroundStyle(theme.text)
            Text("A Start follows this revision. When a newer revision is stored, the approval is stale and the core reports no plan until that revision is approved.")
                .textStyle(.secondary).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
        }
        .padding(.vertical, 10)
        .padding(.horizontal, 12)
        .frame(maxWidth: 760, alignment: .leading)
        .background(theme.accent.opacity(0.08), in: RoundedRectangle(cornerRadius: 9))
        .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(theme.accent.opacity(0.28), lineWidth: 1))
    }

    private func chooser(_ shown: ApprovedPlan) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            WrappingRow(spacing: 6) {
                ForEach(state.plans) { plan in
                    Button { actions.choosePlan(plan.plan) } label: {
                        FilterChip(title: "Plan \(ExecutionPlanState.short(plan.plan))", count: plan.steps.count, isOn: plan.plan == shown.plan)
                    }
                    .buttonStyle(.plain)
                }
            }
            Footnote(state.truncated
                ? "More plans are approved than this screen reads. The number beside a plan is its steps."
                : "Every approved plan the core holds. The number beside a plan is its steps.")
        }
    }

    @ViewBuilder
    private func graph(_ plan: ApprovedPlan) -> some View {
        let graph = PlanGraph(plan)
        if graph.cyclic {
            NoticeWell("The edges of this plan do not order its steps, so no graph is drawn. The steps are listed below.")
        } else {
            VStack(alignment: .leading, spacing: 10) {
                SidewaysScrolling {
                    ZStack(alignment: .topLeading) {
                        ForEach(0..<graph.waves, id: \.self) { wave in
                            Text("Wave \(wave + 1)").font(.system(size: 11, weight: .semibold)).foregroundStyle(theme.text3)
                                .frame(height: PlanGraph.heading, alignment: .top)
                                .offset(x: CGFloat(wave) * PlanGraph.column + PlanGraph.inset)
                        }
                        ForEach(Array(graph.lanes.enumerated()), id: \.offset) { index, lane in
                            LaneBand(name: ExecutionPlanState.repository(lane.repository), mono: false)
                                .frame(width: graph.size.width, height: graph.height(lane))
                                .offset(y: graph.top(lane: index))
                        }
                        ForEach(graph.edges) { edge in
                            if let ends = graph.ends(edge) {
                                GraphEdge(from: ends.from, to: ends.to, dashed: edge.predicate == "ArtifactApproved")
                            }
                        }
                        ForEach(graph.nodes) { node in
                            Button { actions.pick(node.id) } label: {
                                TaskNode(identifier: ExecutionPlanState.short(node.id), title: ExecutionPlanState.provider(node.step.provider),
                                         detail: "Step rev \(node.step.revision)", selected: node.id == state.selected)
                            }
                            .buttonStyle(.plain)
                            .offset(x: graph.origin(node).x, y: graph.origin(node).y)
                        }
                    }
                    .frame(width: graph.size.width, height: graph.size.height, alignment: .topLeading)
                }
                .frame(height: graph.size.height)
                .padding(EdgeInsets(top: 12, leading: 8, bottom: 12, trailing: 8))
                .background(theme.fill2, in: RoundedRectangle(cornerRadius: 12))
                legend(graph)
            }
        }
    }

    private func legend(_ graph: PlanGraph) -> some View {
        WrappingRow(spacing: 14) {
            if graph.edges.isEmpty {
                Text(graph.nodes.count == 1 ? "This plan has one step and no dependency." : "No step of this plan depends on another.")
            } else {
                HStack(spacing: 6) {
                    GraphEdge(from: CGPoint(x: 0, y: 6), to: CGPoint(x: 26, y: 6)).frame(width: 26, height: 12)
                    Text("Evidence dependency")
                }
                HStack(spacing: 6) {
                    GraphEdge(from: CGPoint(x: 0, y: 6), to: CGPoint(x: 26, y: 6), dashed: true).frame(width: 26, height: 12)
                    Text("Approval dependency")
                }
                Text("Whether a dependency is met is not reported.")
            }
            Text("Waves are derived from the edges. Steps have no name or state in the core.")
        }
        .textStyle(.meta).foregroundStyle(theme.text3)
    }

    // MARK: Inspector

    @ViewBuilder
    private func inspector(_ plan: ApprovedPlan) -> some View {
        if let step = state.step {
            stepInspector(plan, step)
        } else {
            planInspector(plan)
        }
    }

    private func header(_ line: String, _ title: String, tag: String? = nil) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(line).font(.system(size: 11)).foregroundStyle(theme.text3).lineLimit(1).truncationMode(.middle)
            Text(title).textStyle(.inspectorTitle).foregroundStyle(theme.textStrong).lineLimit(1)
            if let tag { KindTag(tag) }
        }
        .padding(EdgeInsets(top: 14, leading: 16, bottom: 12, trailing: 16))
        .frame(maxWidth: .infinity, alignment: .leading)
        .overlay(alignment: .bottom) { Rectangle().fill(theme.hairline).frame(height: 1) }
    }

    /// An identity or digest on as many lines as it needs; the inspector is narrow.
    private func whole(_ text: String) -> some View {
        identifier(text).lineLimit(nil).fixedSize(horizontal: false, vertical: true)
    }

    private func quiet(_ text: String) -> some View {
        Text(text).font(.system(size: 12.5)).foregroundStyle(theme.text3).fixedSize(horizontal: false, vertical: true)
    }

    private func planInspector(_ plan: ApprovedPlan) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            header("Plan · rev \(plan.revision)", "Plan \(ExecutionPlanState.short(plan.plan))", tag: "Approved")
            InspectorSection("Plan") {
                FactGrid {
                    Fact("Identity") { whole(plan.plan) }
                    Fact("Revision", "\(plan.revision)")
                    Fact("Digest") { whole(VerificationState.shortened(plan.digest)) }
                    Fact("Approved by", "Decision event \(plan.decision)")
                    Fact("Steps", "\(plan.steps.count)")
                    Fact("Edges", "\(plan.edges.count)")
                }
            }
            InspectorSection("Documents") {
                FactGrid {
                    Fact("PRD") { whole(plan.prd) }
                    Fact("ADR") { whole(plan.adr) }
                    Fact("Validation") {
                        if let validation = plan.validation { whole(validation) } else { quiet("Not named by the core") }
                    }
                }
            }
            approvals(plan)
            InspectorSection("Step", last: true) { quiet("Choose a step in the graph or in the list to inspect it and to fill the Start.") }
        }
    }

    private func stepInspector(_ plan: ApprovedPlan, _ step: ApprovedPlan.Step) -> some View {
        let before = plan.edges.filter { $0.dependent == step.id }, after = plan.edges.filter { $0.prerequisite == step.id }
        return VStack(alignment: .leading, spacing: 0) {
            header("Step · \(ExecutionPlanState.repository(step.repository)) · Plan rev \(plan.revision)",
                   "Step \(ExecutionPlanState.short(step.id))", tag: ExecutionPlanState.provider(step.provider))
            InspectorSection("Assignment") {
                FactGrid {
                    Fact("Step") { whole(step.id) }
                    Fact("Revision", "\(step.revision)")
                    Fact("Repository") {
                        if let repository = step.repository { whole(repository) } else { quiet("None bound") }
                    }
                    Fact("Provider", ExecutionPlanState.provider(step.provider))
                    Fact("Repository use", step.repositoryMode ?? ExecutionPlanState.notReported)
                    Fact("Priority", step.priority.map { "\($0)" } ?? ExecutionPlanState.notReported)
                }
            }
            InspectorSection("Source") {
                if step.sources.isEmpty {
                    quiet("The plan binds no source to this step.")
                } else {
                    FactGrid {
                        ForEach(Array(step.sources.enumerated()), id: \.offset) { _, source in
                            Fact("Base commit") { whole(source.commit) }
                            if let snapshot = source.snapshot { Fact("Snapshot") { whole(VerificationState.shortened(snapshot)) } }
                        }
                        Fact("Environment", step.environment ?? "None")
                    }
                }
            }
            InspectorSection("Dependencies") {
                if before.isEmpty && after.isEmpty {
                    quiet("None")
                } else {
                    VStack(alignment: .leading, spacing: 8) {
                        ForEach(before) { edge in dependency("After step \(ExecutionPlanState.short(edge.prerequisite))", edge) }
                        ForEach(after) { edge in dependency("Before step \(ExecutionPlanState.short(edge.dependent))", edge) }
                        quiet("Whether a dependency is met is not reported.")
                    }
                }
            }
            approvals(plan)
            InspectorSection("Not reported", last: true) {
                quiet("Description, acceptance criteria, file scope, execution history and verification requirements of a step.")
            }
        }
    }

    private func dependency(_ title: String, _ edge: ApprovedPlan.Edge) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title).font(.system(size: 12.5, weight: .medium)).foregroundStyle(theme.text)
            Text(ExecutionPlanState.requirement(edge)).textStyle(.meta).foregroundStyle(theme.text3)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private func approvals(_ plan: ApprovedPlan) -> some View {
        InspectorSection("Required approvals") {
            if plan.approvals.isEmpty {
                quiet("The plan binds no approval.")
            } else {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(Array(plan.approvals.enumerated()), id: \.offset) { _, approval in
                        HStack(spacing: 8) {
                            identifier(ExecutionPlanState.short(approval.artifact))
                            Text("rev \(approval.revision) · decision event \(approval.decision)").textStyle(.meta).foregroundStyle(theme.text3)
                        }
                    }
                    quiet("Bound when the plan was stored. The core checks that each is current at a Start.")
                }
            }
        }
    }

    @ViewBuilder
    private var banners: some View {
        switch state.link {
        case .lost:
            Banner(.danger, title: "Jojo core isn't responding",
                   message: state.plan == nil ? "A Start cannot be sent. What was typed is kept."
                       : "A Start cannot be sent. What was typed is kept. The plan below was read before the core stopped answering.")
        case .incompatible:
            Banner(.danger, title: "Jojo core speaks another protocol version", message: "Diagnostics only. A Start cannot be sent.")
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

    private var step: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Step to start") {
                Button("Review Plan") { actions.open(.approvalInbox) }.buttonStyle(.jojo(.secondary, compact: true))
            }
            Text("Start runs one approved plan step in a Jojo-owned worktree. The core refuses it unless admission is open, every approval is current and no earlier work is on hold.")
                .textStyle(.body).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
            if let plan = state.plan {
                Card(.panelList) {
                    Rows(plan.steps) { step in
                        Button { actions.pick(step.id) } label: {
                            ListRow("Step \(ExecutionPlanState.short(step.id))", meta: ExecutionPlanState.provider(step.provider),
                                    detail: ExecutionPlanState.repository(step.repository), leading: { EmptyView() },
                                    trailing: { if step.id == state.selected { KindTag("Chosen", tone: .info) } })
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                }
                Footnote(plan.validation == nil
                    ? "Choosing a step fills its identities below. The core names no validation plan for this plan; type that one."
                    : "Choosing a step fills the five identities below. They can still be typed.")
            }
            Card(.panelList) {
                VStack(spacing: 0) {
                    ForEach(ExecutionPlanState.fields) { field in
                        FactLine(field.label, last: field == ExecutionPlanState.fields.last) {
                            VStack(alignment: .leading, spacing: 4) {
                                InputField(prompt: "identity (32 hex)", text: binding(field), mono: true)
                                if state.invalid(field) {
                                    Text("Not a 32-character lowercase hex identity").textStyle(.meta).foregroundStyle(theme.warn)
                                }
                            }
                        }
                    }
                }
            }
            HStack(spacing: 10) {
                Button { actions.start() } label: { Label("Start Step", systemImage: "play.fill") }
                    .buttonStyle(.jojo(.primary))
                    .disabled(state.startBlocked != nil)
                if let blocked = state.startBlocked { DisabledReason(blocked) }
            }
            .padding(.top, 4)
            Footnote("Starting asks the core to admit the step. It can still be refused, for example when an approval is stale.")
        }
    }

    private func binding(_ field: ExecutionPlanState.Field) -> Binding<String> {
        Binding(get: { state.subjects[keyPath: field.path] }, set: { value in
            var subjects = state.subjects
            subjects[keyPath: field.path] = value
            actions.edit(subjects)
        })
    }

    @ViewBuilder
    private var outcome: some View {
        switch state.outcome {
        case nil:
            EmptyView()
        case .sent?:
            RequestedNotice(text: "Start sent to core · waiting for confirmation")
        case .accepted(let attempt, let dispatch)?:
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Last Start") {
                    Button("Open Agent Runs") { actions.open(.agentRuns) }.buttonStyle(.jojo(.secondary, compact: true))
                }
                Card(.panelList) {
                    VStack(spacing: 0) {
                        FactLine("Result", "Accepted by the core")
                        FactLine("Attempt") { identifier(attempt) }
                        FactLine("Dispatch", dispatch, last: true)
                    }
                }
                NoticeWell("Accepted is not completed. The attempt's result is on Agent Runs.")
            }
        case .refused(let code)?:
            let refusal = ExecutionPlanState.refusal(code)
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Last Start")
                Banner(.warning, title: refusal.title, message: refusal.message)
                Card(.panelList) {
                    VStack(spacing: 0) {
                        FactLine("Result", "Refused")
                        FactLine("Reason", last: true) { identifier(code) }
                    }
                }
            }
        case .unknown?:
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Last Start")
                Banner(.warning, title: "Start outcome unknown", message: "It will be reconciled by receipt, not sent again.")
            }
        }
    }

    private func identifier(_ text: String) -> some View {
        Text(text).font(.system(size: 11.5, design: .monospaced)).tracking(-0.115).textSelection(.enabled)
    }

    private var reported: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Reported by the core")
            Card(.panelList) {
                VStack(spacing: 0) {
                    SettingLine("Core", state.status.core)
                    SettingLine("Admission", state.status.admission)
                    SettingLine("Slice configuration", state.sliceLine.value, form: state.sliceLine.form, last: true)
                }
            }
        }
    }

    private var missing: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Not reported")
            Card(.panelList) {
                VStack(spacing: 0) {
                    ForEach(ExecutionPlanState.missing) { line in
                        SettingLine(line.label, line.value, form: .missing, last: line == ExecutionPlanState.missing.last)
                    }
                }
            }
            Footnote("The graph above is drawn from the approved plan as the core stores it. No example stands in for what is not reported.")
        }
    }
}

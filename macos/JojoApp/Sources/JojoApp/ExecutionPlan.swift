import JojoCore
import SwiftUI

// Execution Plan (docs/design/screens/EXECUTION_PLAN.md), from `ExecutionPlanMac.dc.html`:
// the held feature's execution plan by waves, its gates, filters and plan checks, and in
// the inspector one step or gate. Protocol 1.9 (`PlanDetail`); the run is `PlanRun` (1.3),
// a step is started by hand with `Start` from the approved plan (`ApprovedPlan`).
// M6: the inspector follows a running step's log live (`Log`), decides a step that waits
// for the human's decision (`CandidateDecision`) and starts again a step that waits for the
// human (`RetryPlanStep`, 1.13).

/// The grid's filters, as the board's aside draws them.
struct PlanFilters: Equatable {
    var hiddenProviders: Set<String> = []
    var hiddenStatuses: Set<String> = []
    /// Nil shows every wave.
    var wave: Int64?
}

/// A node of the grid: a step or a gate.
enum PlanNode: Equatable, Hashable {
    case step(String)
    case approvalGate
    case verificationGate
}

/// What the screen shows. Every value is the helper's answer.
struct ExecutionPlanState: Equatable {
    var connection: HelperConnection
    var admission: String
    var slice: Bool?
    var scope = FeatureScope()
    /// The feature's execution plans (1.6), newest first.
    var plans: [DocumentSummary] = []
    var chosenPlan: String?
    /// Protocol 1.9: the chosen plan step by step.
    var detail: PlanDetail?
    var detailLoad = PlanLoad.idle
    /// The run of the plan (1.3), if it was started.
    var run: PlanRun?
    /// The approved plans the workspace read (1.1): a step is started from one of them.
    var approved: [ApprovedPlan] = []
    var commandPending = false
    var outcome: StartOutcome?
    /// The loaded review is this plan's newest revision and can be decided.
    var reviewCurrent = false
    var decisionOutstanding = false
    var filters = PlanFilters()
    var selected: PlanNode?
    var reportsDetail = true
    /// M6: the candidate the workspace loaded, the selected step's live log and the newest
    /// activity of its attempt.
    var candidate: CandidateReview?
    var log: StepLog?
    var activity: AttemptActivity?

    var link: RecoveryCenterState.Link { coreLink(connection) }
    var admissionClosed: Bool { link == .connected && !admission.isEmpty && admission != "Open" }

    var plan: DocumentSummary? { plans.first { $0.id == chosenPlan } ?? plans.first }

    /// Steps the filters let through.
    var shown: [PlanStepDetail] {
        (detail?.steps ?? []).filter { step in
            !filters.hiddenProviders.contains(step.provider ?? "Unassigned")
                && !filters.hiddenStatuses.contains(Self.group(step))
                && (filters.wave == nil || step.wave == filters.wave)
        }
    }

    var waves: [Int64] {
        Array(Set((detail?.steps ?? []).compactMap(\.wave))).sorted()
    }

    /// The board's status groups: Running, Blocked, Eligible, Completed, Waiting.
    static func group(_ step: PlanStepDetail) -> String {
        if step.eligible { return "Eligible" }
        switch step.state {
        case "Running", "Repairing", "Validating", "Reviewing", "Launching": return "Running"
        case "Blocked", "NeedsHuman", "Conflict", "Refused", "Failed", "Cancelled": return "Blocked"
        case "Accepted", "Integrated", "Review": return "Completed"
        default: return "Waiting"
        }
    }

    func count(status: String) -> Int { (detail?.steps ?? []).filter { Self.group($0) == status }.count }
    func count(provider: String) -> Int { (detail?.steps ?? []).filter { ($0.provider ?? "Unassigned") == provider }.count }

    var step: PlanStepDetail? {
        guard case .step(let id)? = selected else { return nil }
        return detail?.steps.first { $0.id == id }
    }

    /// The step's state as a pill: the core's words, "Needs human" spelled out.
    static func pill(_ step: PlanStepDetail) -> (String, Tone) {
        switch step.state {
        case "Integrated", "Accepted": return (step.state, .ok)
        case "Review": return ("Passed", .ok)
        case "Running", "Repairing", "Validating", "Reviewing": return (step.state, .info)
        case "Blocked", "NeedsHuman", "Conflict", "Refused", "Failed": return (step.state == "NeedsHuman" ? "Needs human" : step.state, .danger)
        case "Ready": return ("Idle", .neutral)
        default: return (step.state, .neutral)
        }
    }

    var approvedPlan: ApprovedPlan? { approved.first { $0.plan == plan?.id } }

    /// Why Start Task cannot be sent for a step; nil when it can.
    func startBlocked(_ step: PlanStepDetail) -> String? {
        if link != .connected { return "Unavailable: the core is not connected." }
        if slice == false { return "The core has no slice configuration; it cannot start a step." }
        if admissionClosed { return "Admission is closed. The core refuses a Start until it is open." }
        if commandPending { return "Unavailable while a command awaits the core." }
        if run != nil { return "The plan runs: the core starts its ready steps itself." }
        if !step.eligible { return "Not eligible: \(step.blockers.first?.reason ?? "it ran already.")" }
        if approvedPlan == nil { return "The approved plan was not read yet." }
        return nil
    }

    /// Why Start Eligible Tasks cannot be sent; nil when it can (RunPlan or Resume).
    var runBlocked: String? {
        if link != .connected { return "Unavailable: the core is not connected." }
        if slice == false { return "The core has no slice configuration; it cannot run a plan." }
        if admissionClosed { return "Admission is closed. The core starts nothing until it is open." }
        if commandPending { return "Unavailable while a command awaits the core." }
        guard let detail else { return "No plan read." }
        if detail.approval != "Approved" { return "Plan rev \(detail.revision) is not approved." }
        if let run {
            if run.state == "PAUSED" { return nil }
            return run.state == "RUNNING" ? "The run starts eligible steps itself." : "The run is \(run.state.lowercased())."
        }
        return detail.eligible.isEmpty ? "No step is eligible." : nil
    }

    var eligibleCount: Int {
        if let run, run.state == "PAUSED" { return run.steps.filter { $0.state == "Ready" }.count }
        return detail?.eligible.count ?? 0
    }

    /// Pause Admission pauses the run; Resume Admission resumes it.
    var pauseTitle: String { run?.state == "PAUSED" ? "Resume Admission" : "Pause Admission" }

    var pauseBlocked: String? {
        if link != .connected { return "Unavailable: the core is not connected." }
        guard let run else { return "The plan has no run to pause." }
        if !["RUNNING", "PAUSED"].contains(run.state) { return "The run is \(run.state.lowercased())." }
        if commandPending { return "Unavailable while a command awaits the core." }
        return nil
    }

    var approveBlocked: String? {
        guard let detail else { return "No plan read." }
        if link != .connected { return "Unavailable: the core is not connected." }
        if detail.approval == "Approved" { return "Plan rev \(detail.revision) is approved." }
        if detail.approval == "Waiting" { return "Waits for the requirements and the ADR it was drafted on." }
        if decisionOutstanding { return "Unavailable while a decision awaits the core." }
        if !reviewCurrent { return "Loading the review of this plan." }
        return nil
    }

    /// Whether the approval gate is drawn: the plan's newest revision is not approved.
    var approvalGate: Bool { detail.map { $0.approval != "Approved" } ?? false }

    /// The verification gate: how many steps passed host validation.
    var verified: (passed: Int, of: Int) {
        let steps = detail?.steps ?? []
        return (steps.filter { $0.attempts.last?.validation == "Passed" }.count, steps.count)
    }

    /// M6: a step whose newest attempt runs, is repaired, validated or reviewed: its log is
    /// followed live.
    static func live(_ step: PlanStepDetail) -> Bool {
        step.attempts.last != nil && ["Running", "Repairing", "Validating", "Reviewing"].contains(step.state)
    }

    /// M6: the candidate of a step that waits for the human's decision, once it is loaded.
    func decision(_ step: PlanStepDetail) -> CandidateReview? {
        guard step.state == "Review", let newest = step.attempts.last, candidate?.attempt == newest.id else { return nil }
        return candidate
    }

    /// Why Accept or Reject cannot be sent for a step; nil when it can.
    func decideBlocked(_ step: PlanStepDetail, accept: Bool) -> String? {
        if link != .connected { return "Unavailable: the core is not connected." }
        guard let c = decision(step), c.candidate != nil else { return "Loading the candidate." }
        if c.decision != "NONE" { return "Already decided: \(c.decision)." }
        if accept, !c.canAccept { return "Unavailable: the core accepts only a passing host validation of exactly this candidate." }
        if commandPending { return "Unavailable while a command awaits the core." }
        return nil
    }

    /// Why a retry cannot be sent for a step; nil when it can. What the step allows is the core's.
    func retryBlocked(_ step: PlanStepDetail) -> String? {
        if link != .connected { return "Unavailable: the core is not connected." }
        if slice == false { return "The core has no slice configuration; it cannot start a step." }
        if commandPending { return "Unavailable while a command awaits the core." }
        if step.retryActions.isEmpty { return "The core offers no way to start this step again." }
        return nil
    }

    /// What the human is told before a retry is sent.
    static func retryWords(_ action: String) -> (button: String, title: String, sentence: String) {
        action == "Repair"
            ? ("Repair Once More", "Repair this step once more?",
               "A new attempt is told what failed and your note. It starts when capacity allows. If it fails, the step waits for you again; it is not repaired on its own.")
            : ("Restart Step", "Restart this step?",
               "A new attempt starts fresh from the plan branch as it is now, with your note if you give one. It starts when capacity allows. If it fails, the step waits for you again.")
    }

    func title(_ key: String) -> String? { detail?.steps.first { $0.key == key }?.title }
    func state(_ key: String) -> PlanStepDetail? { detail?.steps.first { $0.key == key } }

    static func outcome(_ a: StepAttempt) -> String {
        let what = a.outcome ?? "Running"
        let validation = a.validation.map { " · validation \($0 == "NotRun" ? "not run" : $0.lowercased())" } ?? ""
        return "Attempt \(a.ordinal) · \(what)\(validation)\(a.accepted ? " · accepted" : "")"
    }
}

struct ExecutionPlanActions {
    var open: (Screen) -> Void = { _ in }
    var reload: () -> Void = {}
    var choosePlan: (String) -> Void = { _ in }
    var select: (PlanNode) -> Void = { _ in }
    var filters: (PlanFilters) -> Void = { _ in }
    var start: (PlanStepDetail) -> Void = { _ in }
    var inspect: (String) -> Void = { _ in }
    var evidence: (String) -> Void = { _ in }
    var compare: () -> Void = {}
    var review: () -> Void = {}
    var decide: (CandidateAction) -> Void = { _ in }
    var retry: (PlanStepDetail, String, String) -> Void = { _, _, _ in }
}

struct ExecutionPlanScreen: View {
    @ObservedObject var model: AppModel
    /// The plan is read only while this destination is the shown one.
    let visible: Bool
    @Binding var chosenPlan: String?
    let open: (Screen) -> Void
    let compare: (String) -> Void
    @State private var filters = PlanFilters()
    @State private var selected: PlanNode?

    var body: some View {
        ExecutionPlanView(state: state, actions: actions)
            .onAppear { if visible { read() } }
            .onChange(of: visible) { if visible { read() } }
            .onChange(of: model.workspace.snapshotSeq) { if visible { read() } }
            .onChange(of: chosenPlan) { selected = nil; if visible { read() } }
            .onChange(of: model.workspace.documents) { if visible { read() } }
            // M6: the selected step's log every two seconds while it runs and is shown.
            .task(id: liveAttempt) {
                guard let attempt = liveAttempt else {
                    model.workspace.followingStep = false
                    return
                }
                model.workspace.followingStep = true
                while !Task.isCancelled {
                    model.workspace.tailLog(attempt)
                    model.workspace.loadAttempts()
                    try? await Task.sleep(nanoseconds: 2_000_000_000)
                }
            }
            // M6: the candidate of a step that waits for the human's decision.
            .task(id: reviewAttempt) {
                if let attempt = reviewAttempt, model.workspace.candidate?.attempt != attempt {
                    model.workspace.loadCandidate(attempt)
                }
            }
    }

    private var selectedStep: PlanStepDetail? {
        guard visible else { return nil }
        return state.step
    }

    private var liveAttempt: String? {
        selectedStep.flatMap { ExecutionPlanState.live($0) ? $0.attempts.last?.id : nil }
    }

    private var reviewAttempt: String? {
        selectedStep.flatMap { $0.state == "Review" ? $0.attempts.last?.id : nil }
    }

    static func plans(_ workspace: WorkspaceModel, scope: FeatureScope) -> [DocumentSummary] {
        (workspace.documents?.documents ?? []).filter { $0.kind == "ExecutionPlan" && scope.holds($0.feature) }
    }

    var state: ExecutionPlanState {
        let workspace = model.workspace
        let scope = FeatureScope(workspace: workspace)
        let plans = Self.plans(workspace, scope: scope)
        let plan = plans.first { $0.id == chosenPlan } ?? plans.first
        let review = workspace.review
        let detail = plan.flatMap { workspace.planDetails[$0.id] }
        let newest: String? = {
            guard case .step(let id)? = selected else { return nil }
            return detail?.steps.first { $0.id == id }?.attempts.last?.id
        }()
        return ExecutionPlanState(
            connection: model.client.connection, admission: workspace.admission, slice: model.client.hello?.slice,
            scope: scope, plans: plans, chosenPlan: chosenPlan,
            detail: detail, detailLoad: plan.flatMap { workspace.planDetailLoads[$0.id] } ?? .idle,
            run: plan.flatMap { workspace.planRuns[$0.id] }, approved: workspace.plans, commandPending: workspace.commandPending,
            outcome: workspace.startOutcome,
            reviewCurrent: review != nil && review?.subject == plan?.id && review?.revision == plan?.revision && !workspace.reviewStale,
            decisionOutstanding: model.client.outstandingCommands.values.contains("HumanDecision"),
            filters: filters, selected: selected,
            reportsDetail: model.client.hello.map { $0.queries.contains("PlanDetail") } ?? true,
            candidate: workspace.candidate, log: newest.flatMap { workspace.stepLogs[$0] },
            activity: newest.flatMap { workspace.attempts?[$0]?.activity })
    }

    private func read() {
        let workspace = model.workspace
        workspace.loadFeatures()
        workspace.loadPlans()
        guard let plan = state.plan else { return }
        workspace.loadPlanDetail(plan.id)
        if plan.approval != "Approved", workspace.review?.subject != plan.id || workspace.reviewStale {
            workspace.loadReview(plan.id)
        }
    }

    private var actions: ExecutionPlanActions {
        let workspace = model.workspace
        return ExecutionPlanActions(
            open: open,
            reload: { read() },
            choosePlan: { chosenPlan = $0 },
            select: { selected = $0 },
            filters: { filters = $0 },
            start: { step in
                guard let approved = state.approvedPlan else { return }
                workspace.start(approved.subjects(step: step.id))
            },
            inspect: { attempt in
                workspace.focusAttempt = attempt
                open(.agentRuns)
            },
            evidence: { attempt in
                workspace.loadCandidate(attempt)
                open(.verification)
            },
            compare: { if let plan = state.plan { compare(plan.id) } },
            review: {
                if let plan = state.plan { workspace.loadReview(plan.id) }
                open(.approvalInbox)
            },
            decide: { workspace.decideCandidate($0) },
            retry: { step, action, note in
                guard let plan = state.plan else { return }
                workspace.retryPlanStep(plan.id, step: step.id, attempt: step.retryAttempt, action: action, note: note)
            })
    }
}

struct ExecutionPlanView: View {
    @Environment(\.theme) private var theme
    let state: ExecutionPlanState
    var actions = ExecutionPlanActions()
    /// M6: the note the human gives a retry, and the retry or rejection being confirmed.
    @State private var note = ""
    @State private var confirming: (step: PlanStepDetail, action: String)?
    @State private var rejecting = false

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 14) {
                CoreBanners(connection: state.connection, missing: state.reportsDetail ? nil : ("PlanDetail", "plan steps"))
                LifecycleCard(summary: state.scope.summary)
            }
            .padding(.horizontal, 10)
            FeatureColumns(leadingWidth: 190, trailingWidth: 320, wide: 1060, medium: 760) {
                filtersColumn
            } main: {
                main
            } trailing: {
                inspector
            }
        }
        .padding(.top, 6)
    }

    // MARK: Filters

    private var filtersColumn: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                GroupLabel("Filters")
                Spacer()
                TextLink("Reset") { actions.filters(PlanFilters()) }.font(.system(size: 12))
            }
            filterGroup("Repository", [(state.scope.summary?.repositoryName ?? "Repository", state.detail?.steps.count ?? 0, nil)])
            filterGroup("Provider", ["ClaudeCode", "Codex", "Unassigned"].map { (providerName($0), state.count(provider: $0), $0) },
                        hidden: state.filters.hiddenProviders) { key in
                var f = state.filters
                if f.hiddenProviders.contains(key) { f.hiddenProviders.remove(key) } else { f.hiddenProviders.insert(key) }
                actions.filters(f)
            }
            filterGroup("Status", ["Running", "Blocked", "Eligible", "Completed", "Waiting"].map { ($0, state.count(status: $0), $0) },
                        hidden: state.filters.hiddenStatuses) { key in
                var f = state.filters
                if f.hiddenStatuses.contains(key) { f.hiddenStatuses.remove(key) } else { f.hiddenStatuses.insert(key) }
                actions.filters(f)
            }
            if state.waves.count > 1 {
                VStack(alignment: .leading, spacing: 6) {
                    GroupLabel("Wave")
                    ChoiceCapsule(options: [("all", "All")] + state.waves.map { ("\($0)", "\($0)") },
                                  selection: state.filters.wave.map { "\($0)" } ?? "all", label: "Wave") { choice in
                        var f = state.filters
                        f.wave = Int64(choice)
                        actions.filters(f)
                    }
                }
            }
            checks
        }
        .padding(.horizontal, 4)
    }

    private func filterGroup(_ title: String, _ rows: [(String, Int, String?)], hidden: Set<String> = [],
                             toggle: @escaping (String) -> Void = { _ in }) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            GroupLabel(title)
            ForEach(rows, id: \.0) { row in
                Button { if let key = row.2 { toggle(key) } } label: {
                    HStack(spacing: 8) {
                        CheckBox(isOn: row.2.map { !hidden.contains($0) } ?? true)
                        Text(row.0).font(.system(size: 12)).frame(maxWidth: .infinity, alignment: .leading)
                        Text("\(row.1)").textStyle(.secondary).foregroundStyle(theme.text2)
                    }
                    .padding(.vertical, 3)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
    }

    @ViewBuilder
    private var checks: some View {
        if let detail = state.detail {
            VStack(alignment: .leading, spacing: 6) {
                GroupLabel("Plan checks")
                check(!detail.cycle, detail.cycle ? "A cycle among the steps" : "No cycles")
                check(detail.missing.isEmpty, detail.missing.isEmpty ? "No missing dependencies" : "\(detail.missing.count) missing dependencies")
                ForEach(Array(detail.conflicts.enumerated()), id: \.offset) { _, c in
                    check(false, "File conflict: \(c.steps.joined(separator: " and "))", warn: true)
                }
                if detail.conflicts.isEmpty { check(true, "No file conflicts") }
            }
        }
    }

    private func check(_ ok: Bool, _ text: String, warn: Bool = false) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Circle().fill(ok ? theme.green : warn ? theme.orange : theme.red).frame(width: 7, height: 7)
            Text(text).font(.system(size: 12)).fixedSize(horizontal: false, vertical: true)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    // MARK: Main column

    private var main: some View {
        VStack(alignment: .leading, spacing: 12) {
            if state.plans.count > 1 {
                ChoiceCapsule(options: state.plans.map { ($0.id, "Plan rev \($0.revision) · \(ApprovalInboxState.short($0.id))") },
                              selection: state.plan?.id, label: "Plan") { actions.choosePlan($0) }
            }
            revisionNotice
            runNotice
            switch (state.detail, state.detailLoad) {
            case (let detail?, _):
                grid(detail)
                legend
            case (nil, .failed(let reason)):
                Placeholder(symbol: "list.bullet.indent", tile: Palette.red, title: "The plan was not read", message: reason) {
                    Button("Read Again") { actions.reload() }.buttonStyle(.jojo(.primary, compact: true))
                }
            case (nil, _) where state.plan == nil:
                Placeholder(symbol: "list.bullet.indent", tile: Palette.blue, title: "No execution plan yet",
                            message: "The lead drafts a plan once the requirements are approved. Draft one in Feature Chat.") {
                    Button("Open Feature Chat") { actions.open(.featureChat) }.buttonStyle(.jojo(.secondary, compact: true))
                }
            case (nil, _):
                SkeletonCard()
            }
            if let outcome = state.outcome { startOutcome(outcome) }
        }
    }

    @ViewBuilder
    private var revisionNotice: some View {
        if let detail = state.detail, detail.approval != "Approved" {
            TintedNotice(tone: .info, title: "Plan rev \(detail.revision) is proposed.",
                         message: "Nothing runs from it until it is approved in the Approval Inbox.") {
                if detail.revision > 1 {
                    Button("Compare rev \(detail.revision - 1) → \(detail.revision)", action: actions.compare)
                        .buttonStyle(.jojo(.secondary, compact: true))
                }
                Button("Review Plan", action: actions.review).buttonStyle(.jojo(.primary, compact: true))
            }
        }
    }

    @ViewBuilder
    private var runNotice: some View {
        if let run = state.run {
            NoteLine("Run \(run.state.lowercased()) on \(run.branch) at \(ApprovalInboxState.short(run.head)). The plan branch is local; push belongs to M5.")
        }
    }

    private func grid(_ detail: PlanDetail) -> some View {
        let steps = state.shown
        let waves = state.waves.filter { w in state.filters.wave == nil || state.filters.wave == w }
        return TableScrolling(width: 560) {
            HStack(alignment: .top, spacing: 10) {
                Text(state.scope.summary?.repositoryName ?? "repository")
                    .font(.system(size: 12, weight: .bold, design: .monospaced)).foregroundStyle(theme.text2)
                    .frame(width: 80, alignment: .leading).padding(.top, 28)
                if state.approvalGate {
                    column("Gates", gate: true) {
                        gateNode(.approvalGate, title: "Approval gate", sub: "Plan rev \(detail.revision)",
                                 count: "0 of 1 approved")
                    }
                }
                ForEach(waves, id: \.self) { wave in
                    column("Wave \(wave)") {
                        ForEach(steps.filter { $0.wave == wave }) { step in node(step) }
                    }
                }
                if steps.contains(where: { $0.wave == nil }) {
                    column("On a cycle") { ForEach(steps.filter { $0.wave == nil }) { step in node(step) } }
                }
                column("Gates", gate: true) {
                    let v = state.verified
                    gateNode(.verificationGate, title: "Verification gate", sub: "Every step passed host validation",
                             count: v.passed == 0 ? "Not run" : "\(v.passed) of \(v.of)")
                }
            }
            .padding(14)
            .frame(minWidth: 560, alignment: .leading)
        }
        .cardElevation(radius: 16)
    }

    private func column(_ title: String, gate: Bool = false, @ViewBuilder _ content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            GroupLabel(title)
            VStack(alignment: .leading, spacing: 8) { content() }
                .padding(6)
                .background(gate ? theme.grayBg : .clear, in: RoundedRectangle(cornerRadius: 14))
        }
        .frame(minWidth: 150, maxWidth: 220, alignment: .topLeading)
    }

    private func node(_ step: PlanStepDetail) -> some View {
        let pill = ExecutionPlanState.pill(step)
        let selected = state.selected == .step(step.id)
        return Button { actions.select(.step(step.id)) } label: {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text(step.label).font(.system(size: 12, weight: .bold, design: .monospaced))
                        .frame(maxWidth: .infinity, alignment: .leading)
                    OperationalPill(pill.0, tone: pill.1)
                    if step.eligible { OperationalPill("Eligible", tone: .review) }
                }
                Text(step.title ?? "A step").textStyle(.bodyStrong).lineLimit(2).fixedSize(horizontal: false, vertical: true)
                Text(step.blockers.count > 1 ? "\(step.blockers.count) blockers" : step.provider.map(providerName) ?? "No provider bound")
                    .textStyle(.secondary).foregroundStyle(theme.text2)
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(theme.card, in: RoundedRectangle(cornerRadius: 12))
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(selected ? theme.accent : theme.sep, lineWidth: selected ? 2 : 0.5))
        }
        .buttonStyle(.plain)
    }

    private func gateNode(_ node: PlanNode, title: String, sub: String, count: String) -> some View {
        let selected = state.selected == node
        return Button { actions.select(node) } label: {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text("Gate").textStyle(.label).foregroundStyle(theme.text3).frame(maxWidth: .infinity, alignment: .leading)
                    Text(count).font(.system(size: 11)).foregroundStyle(theme.text2)
                }
                Text(title).textStyle(.bodyStrong)
                Text(sub).textStyle(.secondary).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
            }
            .padding(10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(selected ? theme.accent : theme.text3,
                                                                     style: StrokeStyle(lineWidth: selected ? 2 : 1.5, dash: selected ? [] : [5, 4])))
        }
        .buttonStyle(.plain)
    }

    private var legend: some View {
        FlowRow(spacing: 14) {
            legendItem(theme.green, "Dependency met")
            legendItem(theme.orange, "Dependency pending")
            HStack(spacing: 6) {
                RoundedRectangle(cornerRadius: 3).strokeBorder(theme.text3, style: StrokeStyle(lineWidth: 1.5, dash: [3, 2]))
                    .frame(width: 14, height: 10)
                Text("Gate: approval, verification; deployment gates are not available yet")
            }
        }
        .font(.system(size: 11)).foregroundStyle(theme.text2)
    }

    private func legendItem(_ color: Color, _ text: String) -> some View {
        HStack(spacing: 6) { Circle().fill(color).frame(width: 7, height: 7); Text(text) }
    }

    private func startOutcome(_ outcome: StartOutcome) -> some View {
        let text: String
        switch outcome {
        case .sent: text = "Start sent; awaiting the core."
        case .accepted(let attempt, _): text = "Start accepted for attempt \(ApprovalInboxState.short(attempt))."
        case .refused(let code): text = "The core refused the Start: \(code)."
        case .unknown: text = "The Start's outcome is unknown."
        }
        return NoteLine(text)
    }

    // MARK: Inspector

    private var inspector: some View {
        GlassInspector {
            switch state.selected {
            case .step?:
                if let step = state.step { stepInspector(step) } else { NotReported("The step is no longer in this plan.") }
            case .approvalGate?: approvalGateInspector
            case .verificationGate?: verificationGateInspector
            case nil:
                VStack(alignment: .leading, spacing: 6) {
                    Text("Choose a step or a gate").textStyle(.inspectorTitle)
                    NotReported("Its description, scope, dependencies, approvals, history and checks appear here.")
                }
            }
        }
    }

    private func stepInspector(_ step: PlanStepDetail) -> some View {
        let pill = ExecutionPlanState.pill(step)
        let detail = state.detail
        let newest = step.attempts.last
        return VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 6) {
                Text("Task · \(state.scope.summary?.repositoryName ?? "repository") · Plan rev \(detail?.revision ?? 1)")
                    .textStyle(.secondary).foregroundStyle(theme.text2)
                Text("\(step.label) · \(step.title ?? "A step")").textStyle(.panelTitle).fixedSize(horizontal: false, vertical: true)
                HStack(spacing: 6) {
                    OperationalPill(pill.0, tone: pill.1)
                    if step.eligible { OperationalPill("Eligible now", tone: .review) }
                }
            }
            if !step.blockers.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    GroupLabel("Why it's blocked · \(step.blockers.count)")
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(Array(step.blockers.enumerated()), id: \.offset) { _, b in
                            HStack(alignment: .firstTextBaseline, spacing: 8) {
                                Circle().fill(theme.red).frame(width: 7, height: 7)
                                Text(b.reason).textStyle(.secondary).fixedSize(horizontal: false, vertical: true)
                            }
                            .padding(EdgeInsets(top: 8, leading: 12, bottom: 8, trailing: 12))
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(theme.redBg, in: RoundedRectangle(cornerRadius: 12))
                }
            }
            if ExecutionPlanState.live(step) { liveLog }
            if step.state == "Review" { decisionSection(step) }
            if !step.retryActions.isEmpty { retrySection(step) }
            DefinitionCard(rows: [
                Definition(key: "Description", value: step.kind.map { "Kind: \($0). A description is not reported." } ?? "Not reported"),
                Definition(key: "Acceptance", value: "Not reported"),
                Definition(key: "Provider", value: step.provider.map(providerName) ?? "No provider bound"),
                Definition(key: "File scope", value: step.writeScope.joined(separator: "\n").nonEmpty ?? "None declared", mono: true),
                Definition(key: "Dependencies", value: step.dependsOn.isEmpty ? "None" : step.dependsOn.map { key in
                    "\(key) · \(state.title(key) ?? "") · \(state.state(key).map { ExecutionPlanState.pill($0).0 } ?? "")"
                }.joined(separator: "\n")),
                Definition(key: "Approvals", value: "Plan rev \(detail?.revision ?? 1)",
                           pill: approvalPill(detail?.approval ?? "Waiting")),
                Definition(key: "History", value: step.attempts.isEmpty ? "No attempts yet"
                           : step.attempts.reversed().map(ExecutionPlanState.outcome).joined(separator: "\n")),
                Definition(key: "Verification", value: ([step.command.joined(separator: " ")] + step.tests.map { "test: \($0)" })
                    .filter { !$0.isEmpty }.joined(separator: "\n").nonEmpty ?? "None declared", mono: true),
            ], keyWidth: 84)
            FlowRow(spacing: 6) {
                if let newest {
                    Button("Inspect Attempt") { actions.inspect(newest.id) }.buttonStyle(.jojo(.primary, compact: true))
                    Button("View Logs") { actions.inspect(newest.id) }.buttonStyle(.jojo(.secondary, compact: true))
                    if newest.validation != nil {
                        Button("View Evidence") { actions.evidence(newest.id) }.buttonStyle(.jojo(.secondary, compact: true))
                    }
                }
                if step.eligible || newest == nil {
                    Button("Start Task") { actions.start(step) }.buttonStyle(.jojo(.primary, compact: true))
                        .disabled(state.startBlocked(step) != nil).help(state.startBlocked(step) ?? "Starts this step now.")
                }
                if step.blockers.contains(where: { $0.kind == "Question" }) {
                    Button("Answer in Feature Chat") { actions.open(.featureChat) }.buttonStyle(.jojo(.primary, compact: true))
                }
            }
        }
    }

    /// M6: the newest records of the step's log, read every two seconds while it is shown.
    private var liveLog: some View {
        LabeledSection(title: "Live log") {
            VStack(alignment: .leading, spacing: 6) {
                if let activity = state.activity {
                    Text(activity.text).textStyle(.secondary).foregroundStyle(theme.text2)
                        .fixedSize(horizontal: false, vertical: true)
                }
                let lines = state.log?.lines ?? []
                if lines.isEmpty {
                    NotReported("No log records yet.")
                } else {
                    ScrollView {
                        VStack(alignment: .leading, spacing: 2) {
                            ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                                Text(line).font(.system(size: 11, design: .monospaced)).foregroundStyle(theme.text)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                            }
                        }
                        .padding(8)
                    }
                    .defaultScrollAnchor(.bottom)
                    .frame(maxHeight: 180)
                    .background(theme.fill, in: RoundedRectangle(cornerRadius: 8))
                }
            }
        }
    }

    /// M6: Accept or Reject the step's candidate, as on Verification & Evidence.
    private func decisionSection(_ step: PlanStepDetail) -> some View {
        let accept = state.decideBlocked(step, accept: true)
        let reject = state.decideBlocked(step, accept: false)
        return LabeledSection(title: "Your decision") {
            VStack(alignment: .leading, spacing: 8) {
                Text("The candidate passed host validation\(step.detail.isEmpty ? "" : " and was \(step.detail)"). Accepting puts it on the plan branch; nothing is pushed.")
                    .font(.system(size: 12)).fixedSize(horizontal: false, vertical: true)
                FlowRow(spacing: 6) {
                    Button("Accept Candidate") { actions.decide(.Accept) }.buttonStyle(.jojo(.primary, compact: true))
                        .disabled(accept != nil).help(accept ?? "Accepts this exact candidate.")
                    Button("Reject Candidate…") { rejecting = true }.buttonStyle(.jojo(.ghost, compact: true))
                        .disabled(reject != nil).help(reject ?? "Rejects it; the step then waits for you.")
                }
            }
        }
        .confirmationDialog("Reject this candidate?", isPresented: $rejecting, titleVisibility: .visible) {
            Button("Reject Candidate", role: .destructive) { actions.decide(.Reject) }
            Button("Keep Undecided", role: .cancel) {}
        } message: {
            Text("The step then waits for you: you can have it repaired or restarted here.")
        }
    }

    /// M6: start again a step that waits for the human, with an optional note.
    private func retrySection(_ step: PlanStepDetail) -> some View {
        let blocked = state.retryBlocked(step)
        return LabeledSection(title: "Start it again") {
            VStack(alignment: .leading, spacing: 8) {
                Text("Nothing more happens to this step without you. Its automatic repairs are used up or do not apply.")
                    .font(.system(size: 12)).fixedSize(horizontal: false, vertical: true)
                TextArea(prompt: "A note for the agent (optional)", text: $note, lines: 2)
                FlowRow(spacing: 6) {
                    ForEach(step.retryActions, id: \.self) { action in
                        Button(ExecutionPlanState.retryWords(action).button) { confirming = (step, action) }
                            .buttonStyle(.jojo(action == "Repair" ? .primary : .secondary, compact: true))
                            .disabled(blocked != nil).help(blocked ?? ExecutionPlanState.retryWords(action).sentence)
                    }
                }
            }
        }
        .confirmationDialog(confirming.map { ExecutionPlanState.retryWords($0.action).title } ?? "",
                            isPresented: Binding(get: { confirming != nil }, set: { if !$0 { confirming = nil } }),
                            titleVisibility: .visible) {
            if let confirming {
                Button(ExecutionPlanState.retryWords(confirming.action).button) {
                    actions.retry(confirming.step, confirming.action, note)
                    note = ""
                }
                Button("Keep Waiting", role: .cancel) {}
            }
        } message: {
            Text(confirming.map { ExecutionPlanState.retryWords($0.action).sentence } ?? "")
        }
    }

    private var approvalGateInspector: some View {
        let detail = state.detail
        return VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 6) {
                Text("Gate").textStyle(.secondary).foregroundStyle(theme.text2)
                Text("Approval gate").textStyle(.panelTitle)
                OperationalPill("0 of 1 approved", tone: .neutral)
            }
            LabeledSection(title: "Required evidence") {
                DefinitionCard(rows: [Definition(key: "Plan rev \(detail?.revision ?? 1) approval", value: "",
                                                 pill: approvalPill(detail?.approval ?? "Waiting"))], keyWidth: 160)
            }
            LabeledSection(title: "Unblocks") {
                Text((detail?.steps ?? []).map { "\($0.label) · \($0.title ?? "")" }.joined(separator: "\n")).textStyle(.body)
            }
            LabeledSection(title: "How it opens") {
                Text("Opens only when this exact revision is approved in the Approval Inbox, after the requirements and the ADR it was drafted on. An edit to it resets the approval.")
                    .font(.system(size: 12)).fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private var verificationGateInspector: some View {
        let steps = state.detail?.steps ?? []
        let v = state.verified
        return VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 6) {
                Text("Gate").textStyle(.secondary).foregroundStyle(theme.text2)
                Text("Verification gate").textStyle(.panelTitle)
                OperationalPill(v.passed == 0 ? "Not run" : "\(v.passed) of \(v.of) passed", tone: v.passed == v.of && v.of > 0 ? .ok : .neutral)
            }
            LabeledSection(title: "Required evidence") {
                DefinitionCard(rows: steps.map { step in
                    let validation = step.attempts.last?.validation
                    return Definition(key: "\(step.label) host validation", value: "",
                                      pill: validation == "Passed" ? ("Passed", .ok) : validation == "Failed" ? ("Failed", .danger)
                                          : (validation == nil ? "Not run" : "Unknown", .neutral))
                }, keyWidth: 160)
            }
            LabeledSection(title: "How it opens") {
                Text("Opens when every step's candidate passed host validation and was accepted. Deployment receipts are not available yet.")
                    .font(.system(size: 12)).fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

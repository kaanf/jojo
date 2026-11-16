import JojoCore
import SwiftUI

// Execution Plan (docs/design/screens/EXECUTION_PLAN.md): one human Start for one approved
// plan step, and what the core answered. The core reports no plan, so no graph is drawn.
// The screen has no load or query of its own.

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
        Missing(label: "Tasks and waves", value: "Not reported. The snapshot has no plan or step, and no request returns a plan."),
        Missing(label: "Dependencies", value: notReported),
        Missing(label: "Gates", value: notReported),
        Missing(label: "Lifecycle", value: "Not reported. The core has no lifecycle stage of a feature."),
        Missing(label: "Plan checks", value: notReported),
        Missing(label: "Plan revisions", value: "Not reported here. The Approval Inbox shows the revision of a loaded document."),
        Missing(label: "Step state", value: "Not reported. An attempt's record is on Agent Runs."),
        Missing(label: "Repair", value: "Decided, not built: automatic, at most two tries, then a human hold."),
        Missing(label: "Parallel capacity", value: "Not reported, and not decided."),
    ]
}

struct ExecutionPlanActions {
    var edit: (StartSubjects) -> Void = { _ in }
    var start: () -> Void = {}
    var open: (Screen) -> Void = { _ in }
}

struct ExecutionPlanScreen: View {
    @ObservedObject var model: AppModel
    let open: (Screen) -> Void
    @State private var subjects = StartSubjects()

    var body: some View {
        let workspace = model.workspace
        ExecutionPlanView(
            state: ExecutionPlanState(
                connection: model.client.connection, admission: workspace.admission, pid: model.client.processIdentifier,
                slice: model.client.hello?.slice, subjects: subjects, commandPending: workspace.commandPending,
                outcome: workspace.startOutcome),
            actions: ExecutionPlanActions(edit: { subjects = $0 }, start: { workspace.start(subjects) }, open: open))
    }
}

struct ExecutionPlanView: View {
    @Environment(\.theme) private var theme
    let state: ExecutionPlanState
    var actions = ExecutionPlanActions()

    var body: some View {
        Scrolling {
            VStack(alignment: .leading, spacing: 18) {
                banners
                step
                outcome
                reported
                missing
            }
            .frame(maxWidth: 760, alignment: .topLeading)
            .padding(EdgeInsets(top: 18, leading: 22, bottom: 24, trailing: 22))
            .frame(maxWidth: .infinity, alignment: .topLeading)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    @ViewBuilder
    private var banners: some View {
        switch state.link {
        case .lost:
            Banner(.danger, title: "Jojo core isn't responding", message: "A Start cannot be sent. What was typed is kept.")
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
            Footnote("The task graph, the gates and the lifecycle of the design need a request that returns the approved plan. None exists yet.")
        }
    }
}

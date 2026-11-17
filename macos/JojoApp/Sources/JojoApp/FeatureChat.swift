import JojoCore
import SwiftUI

// Feature Chat and New Feature (design wave 2, M2). docs/design/screens/FEATURE_CHAT.md.
// What the screen shows is what the protocol 1.2 chat queries returned and what the human
// typed. Nothing here approves: a draft is opened in the Approval Inbox.

/// What the screen shows. Every field is the helper's answer or what the human typed.
struct FeatureChatState: Equatable {
    var connection: HelperConnection
    var admission: String
    var pid: Int32 = 0
    /// Hello's query names; empty when the helper did not say.
    var queries: [String] = []
    var index: ChatIndex?
    var indexLoad = PlanLoad.idle
    /// The feature the screen holds; nil shows New Feature.
    var feature: String?
    /// The held feature's name, from the chat index (protocol 1.6).
    var featureName: String? { index?.features.first { $0.id == feature }?.name }
    var chat: FeatureChatPage?
    var chatLoad = PlanLoad.idle
    var prdText = ""
    var commandPending = false
    var outcome: StartOutcome?
    /// New Feature: the objective and the repository as chosen.
    var objective = ""
    var repository: String?
    var provider = "ClaudeCode"
    /// The note of the next turn.
    var message = ""
    /// Answers as typed, by question.
    var answers: [String: String] = [:]

    var link: RecoveryCenterState.Link {
        switch connection {
        case .ready: return .connected
        case .notStarted, .handshaking: return .starting
        case .incompatible: return .incompatible
        case .launchFailed, .channelLost, .ended: return .lost
        }
    }

    var status: ShellStatus { ShellStatus(connection: connection, admission: admission, pid: pid) }
    var admissionClosed: Bool { link == .connected && !admission.isEmpty && admission != "Open" }
    /// The helper said which queries it has, and Feature Chat is not among them.
    var unsupported: Bool { link == .connected && !queries.isEmpty && !queries.contains("FeatureChat") }

    var turns: [LeadTurn] { chat?.turns ?? [] }
    var running: LeadTurn? { turns.last { $0.running } }
    var questions: [LeadQuestion] { chat?.questions ?? [] }
    var prd: ChatDocument? { chat?.prd }
    var plan: LeadPlanDraft? { chat?.plan }

    /// "Q3": the question's place in the chat, oldest first.
    func number(_ question: LeadQuestion) -> String {
        "Q\((questions.firstIndex { $0.id == question.id } ?? 0) + 1)"
    }

    /// The four discovery states of STATES.md, section 3, from what the chat reports.
    enum Discovery: Equatable {
        case empty
        case investigating(provider: String, stopping: Bool)
        /// An unanswered blocking question about the whole feature.
        case blocked(LeadQuestion)
        case complete(ChatDocument)
        /// Turns ran; no draft can be offered for review yet.
        case continuing(ChatDocument?)
    }

    var discovery: Discovery {
        guard !turns.isEmpty else { return .empty }
        if let running { return .investigating(provider: running.provider, stopping: running.state == "Stopping") }
        if let blocked = questions.first(where: { $0.open && $0.step == nil }) { return .blocked(blocked) }
        if let prd, prd.ready { return .complete(prd) }
        return .continuing(prd)
    }

    /// Open questions about one plan step: each holds only that step's Start.
    var stepQuestions: [LeadQuestion] { questions.filter { $0.open && $0.step != nil } }

    static func provider(_ word: String) -> String { word == "ClaudeCode" ? "Claude Code" : word }
    static func short(_ identity: String) -> String { String(identity.prefix(12)) }
    static func kind(_ word: String) -> String { word == "Plan" ? "Planning" : "Discovery" }

    /// The operational form of a turn's state.
    static func operational(_ state: String) -> (OperationalState, String) {
        switch state {
        case "Running": return (.running, "Investigating")
        case "Stopping": return (.waiting, "Stopping")
        case "Replied": return (.completed, "Replied")
        case "Cancelled": return (.idle, "Stopped")
        case "Interrupted": return (.recoveryRequired, "Interrupted")
        default: return (.failed, "Failed")
        }
    }

    /// The repository a turn of the held feature names: the feature's own, or the chosen one.
    var turnRepository: String? {
        if let feature, let known = index?.features.first(where: { $0.id == feature }) { return known.repository }
        return feature == nil ? repository : nil
    }

    /// Why a turn of `kind` cannot be sent, or nil when it can. The helper decides the rest.
    func sendBlocked(_ kind: String) -> String? {
        if link != .connected { return "Unavailable: the core is not connected." }
        if unsupported { return "This core has no Feature Chat." }
        if admissionClosed { return "Admission is closed. The core refuses a turn until it is open." }
        if commandPending { return "Unavailable while a command awaits the core." }
        if running != nil { return "A turn is running. Stop it or wait for its result." }
        if feature == nil {
            if !objective.contains(where: { $0.isLetter || $0.isNumber }) { return "Describe what users should be able to do." }
            if repository == nil { return "Choose a registered repository." }
            return nil
        }
        if turnRepository == nil { return "The core did not report this feature's repository. Reload." }
        if kind == "Plan", prd?.approved != true {
            return "A plan needs the approved requirements. Approve the draft in the Approval Inbox."
        }
        return nil
    }

    /// Why an answer to `question` cannot be sent, or nil when it can.
    func answerBlocked(_ question: LeadQuestion) -> String? {
        if link != .connected { return "Unavailable: the core is not connected." }
        if commandPending { return "Unavailable while a command awaits the core." }
        let typed = answers[question.id] ?? ""
        return typed.contains(where: { $0.isLetter || $0.isNumber }) ? nil : "Type an answer."
    }

    /// A sentence for the code of a refused turn. The code itself is shown as sent.
    static func refusal(_ code: String) -> String {
        let reasons: [(String, String)] = [
            ("TurnRefused: a plan turn needs the approved requirements document",
             "A plan needs the approved requirements. Approve the draft in the Approval Inbox."),
            ("TurnRefused: the repository is not registered", "The repository is not registered with the core."),
            ("TurnRefused: the feature belongs to another repository", "This feature reads another repository."),
            ("TurnRefused: another lead turn is running", "Another lead turn is running. Nothing was run."),
            ("TurnRefused", "The core refused the turn."),
            ("AdmissionClosed", "Admission is closed. Nothing was run."),
            ("CapacityOne", "Another run is active. Nothing was run."),
            ("CapacityFull", "Every unit of parallel capacity is in use. Nothing was run."),
            ("NoProductionAdapter", "This provider is not configured in the core. Nothing was run."),
            ("AdapterRejected", "The core rejected the provider's executable. Nothing was run."),
            ("TurnFailed", "The core accepted the turn, but it could not be launched."),
            ("UnknownType", "This core has no Feature Chat."),
        ]
        return reasons.first { code.hasPrefix($0.0) }?.1 ?? "Nothing was run. The core's reason is below."
    }

    /// A sentence for a read of the chat that failed.
    static func loadFailure(_ reason: String) -> String {
        if reason.hasPrefix("UnknownType") { return "This core has no Feature Chat." }
        if reason == "malformed chat" || reason == "malformed index" {
            return "The core's answer was not a chat this app can read. Nothing is shown."
        }
        return "The core did not answer the read. Nothing is shown."
    }

    struct Missing: Identifiable, Equatable {
        let label: String
        let value: String
        var id: String { label }
    }

    /// What the boards draw and the core does not report. No example stands in for them.
    static let missing = [
        Missing(label: "Lifecycle and tasks", value: "Not reported. The core has no lifecycle stage of a feature and runs no plan yet (M3)."),
        Missing(label: "Assumptions", value: "Written into the requirements draft by the lead; not reported as records."),
        Missing(label: "Decisions in use", value: "Not reported."),
        Missing(label: "Discovery history", value: "The turns above and the draft's revisions; no other history is reported."),
        Missing(label: "Suggestions and spikes", value: "Not built."),
        Missing(label: "Export transcript", value: "Not built."),
        Missing(label: "Project, reference files, design references, constraints", value: "Not built. New Feature takes an objective, a repository and a provider."),
    ]
}

struct FeatureChatActions {
    var open: (Screen) -> Void = { _ in }
    var reload: () -> Void = {}
    /// Holds a feature; nil holds none (New Feature).
    var choose: (String?) -> Void = { _ in }
    var editObjective: (String) -> Void = { _ in }
    var chooseRepository: (String) -> Void = { _ in }
    var chooseProvider: (String) -> Void = { _ in }
    var editMessage: (String) -> Void = { _ in }
    var editAnswer: (String, String) -> Void = { _, _ in }
    var send: (String) -> Void = { _ in }
    var stop: () -> Void = {}
    var answer: (LeadQuestion, QuestionAnswer) -> Void = { _, _ in }
    /// Loads a document's review in the Approval Inbox and opens it.
    var review: (String) -> Void = { _ in }
}

struct FeatureChatScreen: View {
    @ObservedObject var model: AppModel
    /// The chat is read only while this destination is the shown one.
    let visible: Bool
    let open: (Screen) -> Void
    @State private var objective = ""
    @State private var repository: String?
    @State private var provider = "ClaudeCode"
    @State private var message = ""
    @State private var answers: [String: String] = [:]

    var body: some View {
        let workspace = model.workspace
        let state = FeatureChatState(
            connection: model.client.connection, admission: workspace.admission, pid: model.client.processIdentifier,
            queries: model.client.hello?.queries ?? [], index: workspace.chatIndex, indexLoad: workspace.chatIndexLoad,
            feature: workspace.chatFeature, chat: workspace.chat, chatLoad: workspace.chatLoad, prdText: workspace.prdText,
            commandPending: workspace.commandPending, outcome: workspace.turnOutcome, objective: objective,
            repository: repository, provider: provider, message: message, answers: answers)
        FeatureChatView(
            state: state,
            actions: FeatureChatActions(
                open: open,
                reload: { workspace.loadChatIndex(); workspace.loadChat() },
                choose: { workspace.openChat($0) },
                editObjective: { objective = $0 }, chooseRepository: { repository = $0 },
                chooseProvider: { provider = $0 }, editMessage: { message = $0 },
                editAnswer: { answers[$0] = $1 },
                send: { kind in
                    guard state.sendBlocked(kind) == nil else { return }
                    if let feature = state.feature, let repository = state.turnRepository {
                        workspace.sendTurn(feature: feature, kind: kind, repository: repository, provider: provider, message: message)
                        message = ""
                    } else if let repository {
                        workspace.sendTurn(feature: Identity.randomClientID(), kind: "Discover", repository: repository,
                                           provider: provider, message: objective)
                    }
                },
                stop: { if let turn = state.running?.id { workspace.cancelTurn(turn) } },
                answer: { question, action in
                    workspace.answer(question, action)
                    answers[question.id] = nil
                },
                review: { document in
                    workspace.loadReview(document)
                    open(.approvalInbox)
                }))
            .onAppear { if visible { workspace.loadChatIndex() } }
            .onChange(of: visible) { if visible { workspace.loadChatIndex(); workspace.loadChat() } }
            .onChange(of: workspace.snapshotSeq) { if visible { workspace.loadChatIndex(); workspace.loadChat() } }
            .onChange(of: workspace.chatFeature) { if workspace.chatFeature != nil { objective = "" } }
    }
}

struct FeatureChatView: View {
    @Environment(\.theme) private var theme
    let state: FeatureChatState
    var actions = FeatureChatActions()

    var body: some View {
        GeometryReader { geometry in
            let beside = geometry.size.width >= 900 && state.feature != nil
            HStack(spacing: 0) {
                Scrolling {
                    VStack(alignment: .leading, spacing: 18) {
                        Group {
                            banners
                            if state.feature == nil { newFeature } else { conversation }
                            if !beside, state.feature != nil { inspector }
                            missing
                        }
                        .frame(maxWidth: 760, alignment: .topLeading)
                    }
                    .padding(EdgeInsets(top: 18, leading: 22, bottom: 24, trailing: 22))
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                if beside {
                    Scrolling { inspector.padding(EdgeInsets(top: 18, leading: 16, bottom: 24, trailing: 16)) }
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
                        .background(theme.sidebar)
                        .overlay(alignment: .leading) { Rectangle().fill(theme.sidebarEdge).frame(width: 1) }
                        .frame(width: 320)
                }
            }
        }
    }

    // MARK: Banners

    @ViewBuilder
    private var banners: some View {
        switch state.link {
        case .lost:
            Banner(.danger, title: "Jojo core isn't responding",
                   message: "Showing the last known chat, read-only. No turn or answer can be sent.")
        case .incompatible:
            Banner(.danger, title: "The core speaks another protocol version",
                   message: "Diagnostics only. No turn or answer can be sent.")
        case .starting, .connected:
            EmptyView()
        }
        if state.unsupported {
            Banner(.warning, title: "This core has no Feature Chat",
                   message: "It does not list the chat queries in its Hello answer. Start a newer helper.")
        }
        if state.admissionClosed {
            Banner(.warning, title: "Execution admission is closed",
                   message: "The core refuses a turn until a reviewed recovery start opens admission.") {
                Button("Open Review") { actions.open(.recoveryCenter) }.buttonStyle(.jojo(.secondary, compact: true))
            }
        }
    }

    // MARK: New Feature

    private var newFeature: some View {
        VStack(alignment: .leading, spacing: 18) {
            Card {
                VStack(alignment: .leading, spacing: 14) {
                    VStack(alignment: .leading, spacing: 3) {
                        Text("Describe the feature").textStyle(.sectionTitle).foregroundStyle(theme.textStrong)
                        Text("Say what users should be able to do. Jojo investigates the code and asks before anything is planned.")
                            .textStyle(.body).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
                    }
                    LabeledField("What should it do?") {
                        TextArea(prompt: "Players should be able to…",
                                 text: Binding(get: { state.objective }, set: actions.editObjective), lines: 4)
                    }
                    Footnote("No need to name files, APIs or architecture. That's what discovery is for.")
                    LabeledField("Repository") { repositories }
                    LabeledField("Provider") { providers }
                    NoticeWell("What happens next: 1. Jojo reads the selected repository, read-only. 2. It asks clarifying questions in Feature Chat. 3. It drafts a PRD for your approval. Nothing is executed before you approve a plan.")
                    HStack(spacing: 10) {
                        Button { actions.send("Discover") } label: {
                            HStack(spacing: 6) { Label("Start Discovery", systemImage: "sparkle.magnifyingglass"); ShortcutHint("⌘↵") }
                        }
                        .buttonStyle(.jojo(.primary))
                        .keyboardShortcut(.return, modifiers: .command)
                        .disabled(state.sendBlocked("Discover") != nil)
                        if let reason = state.sendBlocked("Discover") { DisabledReason(reason) }
                    }
                    outcome
                }
            }
            earlierChats
        }
    }

    @ViewBuilder
    private var repositories: some View {
        switch (state.index, state.indexLoad) {
        case (let index?, _) where !index.repositories.isEmpty:
            WrappingRow(spacing: 8, lineSpacing: 8) {
                ForEach(index.repositories) { repository in
                    Button { actions.chooseRepository(repository.id) } label: {
                        FilterChip(title: URL(fileURLWithPath: repository.path).lastPathComponent, isOn: state.repository == repository.id)
                    }
                    .buttonStyle(.plain)
                    .help(repository.path)
                }
            }
        case (_?, _):
            NoticeWell("No repository is registered with the core. A repository is registered under the disposable root before a chat can read it.")
        case (nil, .failed(let reason)):
            NoticeWell(FeatureChatState.loadFailure(reason))
        case (nil, _):
            Text(state.link == .connected ? "Reading the registered repositories…" : "Not reported").textStyle(.meta)
                .foregroundStyle(theme.text3)
        }
    }

    private var providers: some View {
        HStack(spacing: 8) {
            ForEach(["ClaudeCode", "Codex"], id: \.self) { word in
                Button { actions.chooseProvider(word) } label: {
                    FilterChip(title: FeatureChatState.provider(word), isOn: state.provider == word)
                }
                .buttonStyle(.plain)
            }
        }
    }

    @ViewBuilder
    private var earlierChats: some View {
        if let features = state.index?.features, !features.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Earlier chats")
                Card(.list) {
                    Rows(features) { feature in
                        Button { actions.choose(feature.id) } label: {
                            ListRow(feature.name ?? (feature.objective + (feature.objectiveComplete ? "" : "…")),
                                    meta: "\(feature.turns) turn\(feature.turns == 1 ? "" : "s")",
                                    detail: "Feature \(FeatureChatState.short(feature.id)) · repository \(FeatureChatState.short(feature.repository))",
                                    chevron: true, leading: { LeadingTile(symbol: "bubble.left.and.bubble.right") },
                                    trailing: { if feature.running { OperationalPill(.running, label: "Investigating") } })
                        }
                        .buttonStyle(.plain)
                    }
                }
                if state.index?.featuresComplete == false { Footnote("Only the 32 newest chats are listed.") }
            }
        }
    }

    // MARK: Conversation

    @ViewBuilder
    private var conversation: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text(state.featureName ?? "Feature \(FeatureChatState.short(state.feature ?? ""))").textStyle(.sectionTitle)
                    .foregroundStyle(theme.textStrong)
                if let repository = state.turnRepository {
                    Text("Repository \(FeatureChatState.short(repository))").textStyle(.meta).foregroundStyle(theme.text3)
                }
                Spacer(minLength: 0)
                Button { actions.choose(nil) } label: { Label("New Feature", systemImage: "plus") }
                    .buttonStyle(.jojo(.secondary, compact: true))
            }
            discovery
            switch (state.chat, state.chatLoad) {
            case (let chat?, _):
                ForEach(chat.turns) { turn in exchange(turn) }
            case (nil, .failed(let reason)):
                StateBlock(symbol: "exclamationmark.bubble", title: "The chat was not read",
                           message: FeatureChatState.loadFailure(reason)) {
                    Button("Read Again") { actions.reload() }.buttonStyle(.jojo(.secondary, compact: true))
                }
                .frame(minHeight: 140)
                Text(reason).textStyle(.mono).foregroundStyle(theme.text3).textSelection(.enabled)
            case (nil, _):
                LoadingBlock(state.link == .connected ? "Reading the chat…" : "The core is not connected; nothing was read.")
                    .frame(minHeight: 100)
            }
            composer
            draft
        }
    }

    /// STATES.md, section 3, in the words of the board.
    @ViewBuilder
    private var discovery: some View {
        switch state.discovery {
        case .empty:
            StateBlock(symbol: "text.bubble", title: "Describe what users should be able to do",
                       message: "Jojo reads the code and asks before planning.")
                .frame(minHeight: 120)
        case .investigating(let provider, let stopping):
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    OperationalPill(stopping ? .waiting : .running, label: stopping ? "Stopping" : "Investigating", standalone: true)
                    Text("\(FeatureChatState.provider(provider)) is reading the repository").textStyle(.secondary)
                        .foregroundStyle(theme.text2)
                }
                Text("Read-only. No files are changed during discovery.").textStyle(.meta).foregroundStyle(theme.text3)
            }
        case .blocked(let question):
            Banner(.warning, title: "Whole feature blocked by \(state.number(question))",
                   message: "The requirements can't be reviewed until “\(question.prompt)” is answered.")
        case .complete(let prd):
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    EvidencePill(.passed, label: "Discovery complete")
                    DocumentTag(prd.approved ? .approved : .proposed, label: "PRD rev \(prd.revision)")
                    Spacer(minLength: 0)
                    Button("Open PRD") { actions.review(prd.document) }.buttonStyle(.jojo(.secondary, compact: true))
                }
                Text("\(state.questions.filter { $0.answeredBy == "HUMAN" }.count) questions answered. The draft is complete and can be reviewed; \(prd.approved ? "this revision is approved" : "nothing approved it").")
                    .textStyle(.secondary).foregroundStyle(theme.text2)
            }
        case .continuing(let prd):
            NoticeWell(prd.map { "PRD rev \($0.revision) is a draft that can't be reviewed yet: \($0.blockers) finding\($0.blockers == 1 ? "" : "s") remain. Answer the questions or send another round." }
                ?? "No requirements draft yet. Answer the questions and send another round.")
        }
        ForEach(state.stepQuestions) { question in
            Banner(.warning, title: "Feature continues · step \(question.step ?? "") is blocked by \(state.number(question))",
                   message: "Only that step waits for this answer. The rest of the plan is unaffected.")
        }
    }

    private func exchange(_ turn: LeadTurn) -> some View {
        let (operational, label) = FeatureChatState.operational(turn.state)
        let kind = FeatureChatState.kind(turn.kind)
        return VStack(alignment: .leading, spacing: 8) {
            ChatMessage(author: "You", meta: "\(kind) · turn \(turn.ordinal)", human: true) {
                Text(turn.message.isEmpty ? "Another round with the answers so far." : turn.message + (turn.messageComplete ? "" : "…"))
                    .textStyle(.body).foregroundStyle(turn.message.isEmpty ? theme.text3 : theme.text)
                    .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
            }
            ChatMessage(author: "Jojo · \(kind)", meta: "\(FeatureChatState.provider(turn.provider)) · base \(FeatureChatState.short(turn.base))") {
                OperationalPill(operational, label: label)
                if !turn.summary.isEmpty {
                    Text(turn.summary).textStyle(.body).foregroundStyle(theme.text)
                        .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
                }
                if !turn.detail.isEmpty {
                    Text(turn.detail).textStyle(.mono).foregroundStyle(theme.text2)
                        .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
                }
                if let prd = turn.prd {
                    HStack(spacing: 8) {
                        DocumentTag(.proposed, label: "PRD rev \(prd.revision)")
                        Text("drafted by the lead").textStyle(.meta).foregroundStyle(theme.text3)
                    }
                }
                if let plan = turn.plan {
                    HStack(spacing: 8) {
                        DocumentTag(.proposed, label: "Plan draft")
                        Text("\(plan.steps.count) steps · \(plan.drafted ? "plan and validation drafted" : "waits for the ADR approval")")
                            .textStyle(.meta).foregroundStyle(theme.text3)
                    }
                }
                ForEach(turn.questions) { question in asked(question) }
            }
        }
    }

    private func asked(_ question: LeadQuestion) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(spacing: 6) {
                KindTag(state.number(question))
                questionTag(question)
            }
            Text(question.prompt).textStyle(.body).foregroundStyle(theme.text).fixedSize(horizontal: false, vertical: true)
            if let answer = question.answer {
                Text("Answer: " + answer + (question.answerComplete ? "" : "…")).textStyle(.secondary).foregroundStyle(theme.text2)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(.top, 2)
    }

    @ViewBuilder
    private func questionTag(_ question: LeadQuestion) -> some View {
        if let step = question.step {
            KindTag("Blocks step \(step) only", tone: question.open ? .warn : .neutral)
        } else if question.blocking {
            KindTag(question.open ? "Blocking" : "Blocking · answered", tone: question.open ? .warn : .neutral)
        } else {
            KindTag("Non-blocking")
        }
    }

    private var composer: some View {
        Card(.panel) {
            VStack(alignment: .leading, spacing: 10) {
                TextArea(prompt: "A note for the next round (optional)",
                         text: Binding(get: { state.message }, set: actions.editMessage), lines: 2)
                HStack(spacing: 8) {
                    providers
                    Spacer(minLength: 0)
                    if let running = state.running {
                        Button("Stop Turn") { actions.stop() }.buttonStyle(.jojo(.secondary))
                            .disabled(running.state == "Stopping" || state.link != .connected || state.commandPending)
                    }
                    Button("Draft Plan") { actions.send("Plan") }.buttonStyle(.jojo(.secondary))
                        .disabled(state.sendBlocked("Plan") != nil)
                    Button { actions.send("Discover") } label: { HStack(spacing: 6) { Text("Send"); ShortcutHint("⌘↵") } }
                        .buttonStyle(.jojo(.primary))
                        .keyboardShortcut(.return, modifiers: .command)
                        .disabled(state.sendBlocked("Discover") != nil)
                }
                if let reason = state.sendBlocked("Discover") {
                    DisabledReason(reason)
                } else if let reason = state.sendBlocked("Plan") {
                    DisabledReason("Draft Plan: " + reason)
                }
                outcome
            }
        }
    }

    @ViewBuilder
    private var outcome: some View {
        switch state.outcome {
        case .sent?:
            RequestedNotice(text: "Turn sent. Shown as running only after the core records it.")
        case .accepted(let turn, let dispatch)?:
            Footnote("The core accepted turn \(FeatureChatState.short(turn)) (\(dispatch)).")
        case .refused(let code)?:
            VStack(alignment: .leading, spacing: 4) {
                Banner(.warning, title: "The core refused the turn", message: FeatureChatState.refusal(code))
                Text(code).textStyle(.mono).foregroundStyle(theme.text3).textSelection(.enabled)
            }
        case .unknown?:
            HStack(spacing: 8) {
                EvidencePill(.unknown, label: "Outcome unknown")
                Text("Never shown as done or failed. Never sent again.").textStyle(.meta).foregroundStyle(theme.text3)
            }
        case nil:
            EmptyView()
        }
    }

    /// The draft's text, as the core renders it.
    @ViewBuilder
    private var draft: some View {
        if let prd = state.prd {
            DocumentSection("Requirements draft · rev \(prd.revision)") {
                if state.prdText.isEmpty {
                    Text(state.link == .connected ? "Reading the draft…" : "Not read.").textStyle(.meta).foregroundStyle(theme.text3)
                } else {
                    Text(state.prdText).textStyle(.secondary).foregroundStyle(theme.text)
                        .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
                }
            }
        }
    }

    // MARK: Inspector

    private var inspector: some View {
        VStack(alignment: .leading, spacing: 18) {
            questionsPanel
            documentPanel
            planPanel
        }
    }

    private var questionsPanel: some View {
        // Open: no human answered it yet, blocking or not. Resolved: a human answered it.
        let open = state.questions.filter { $0.answeredBy != "HUMAN" }
        let resolved = state.questions.count - open.count
        return VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Open questions · \(open.count)") {
                if resolved > 0 { Text("Resolved \(resolved)").textStyle(.meta).foregroundStyle(theme.text3) }
            }
            if state.questions.isEmpty {
                Text("The lead asked nothing yet.").textStyle(.meta).foregroundStyle(theme.text3)
            }
            ForEach(state.questions.filter { $0.answeredBy != "HUMAN" }) { question in
                Card(.panel) {
                    VStack(alignment: .leading, spacing: 8) {
                        HStack(spacing: 6) {
                            KindTag(state.number(question))
                            questionTag(question)
                        }
                        Text(question.prompt).textStyle(.body).foregroundStyle(theme.text).fixedSize(horizontal: false, vertical: true)
                        InputField(prompt: "Your answer",
                                   text: Binding(get: { state.answers[question.id] ?? "" }, set: { actions.editAnswer(question.id, $0) }),
                                   submit: { if state.answerBlocked(question) == nil {
                                       actions.answer(question, .answer(state.answers[question.id] ?? ""))
                                   } })
                        HStack(spacing: 8) {
                            Button("Resolve Question") { actions.answer(question, .answer(state.answers[question.id] ?? "")) }
                                .buttonStyle(.jojo(.primary, compact: true))
                                .disabled(state.answerBlocked(question) != nil)
                            if question.blocking {
                                Button("Defer") { actions.answer(question, .deferQuestion) }.buttonStyle(.jojo(.ghost, compact: true))
                                    .disabled(state.link != .connected || state.commandPending)
                            } else {
                                Button("Make Blocking") { actions.answer(question, .block) }.buttonStyle(.jojo(.ghost, compact: true))
                                    .disabled(state.link != .connected || state.commandPending)
                            }
                        }
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var documentPanel: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Requirements")
            if let prd = state.prd {
                Card(.panel) {
                    VStack(alignment: .leading, spacing: 8) {
                        HStack(spacing: 6) {
                            DocumentTag(prd.approved ? .approved : .proposed, label: "PRD rev \(prd.revision)")
                            EvidencePill(prd.ready ? .passed : .incomplete,
                                         label: prd.ready ? "Ready for review" : "\(prd.blockers) finding\(prd.blockers == 1 ? "" : "s")")
                        }
                        ValueLine("Written by", prd.author == "MODEL" ? "The lead (model)" : "A human")
                        ValueLine("Document", FeatureChatState.short(prd.document), last: true)
                        Button("Open in Approval Inbox") { actions.review(prd.document) }.buttonStyle(.jojo(.secondary, compact: true))
                    }
                }
            } else {
                Text("No draft yet.").textStyle(.meta).foregroundStyle(theme.text3)
            }
        }
    }

    @ViewBuilder
    private var planPanel: some View {
        if let plan = state.plan {
            VStack(alignment: .leading, spacing: 8) {
                SectionHeading("Plan draft")
                Card(.panel) {
                    VStack(alignment: .leading, spacing: 8) {
                        ForEach(Array(plan.steps.enumerated()), id: \.element.id) { offset, step in
                            ValueLine(step.key, step.title, last: offset == plan.steps.count - 1)
                        }
                        Divider()
                        HStack(spacing: 8) {
                            Button("ADR") { actions.review(plan.adr) }.buttonStyle(.jojo(.secondary, compact: true))
                            Button("Validation") { actions.review(plan.validation) }.buttonStyle(.jojo(.secondary, compact: true))
                                .disabled(!plan.drafted)
                            Button("Plan") { actions.review(plan.plan) }.buttonStyle(.jojo(.secondary, compact: true))
                                .disabled(!plan.drafted)
                        }
                        Footnote(plan.drafted
                                 ? "Each document is approved on its own in the Approval Inbox."
                                 : "The plan and its validation are drafted when the ADR is approved.")
                    }
                }
            }
        }
    }

    // MARK: Missing

    private var missing: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Not reported by the core")
            Card(.panel) {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(Array(FeatureChatState.missing.enumerated()), id: \.element.id) { offset, item in
                        ValueLine(item.label, item.value, last: offset == FeatureChatState.missing.count - 1)
                    }
                }
            }
        }
    }
}

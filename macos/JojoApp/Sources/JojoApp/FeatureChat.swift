import JojoCore
import SwiftUI
import UniformTypeIdentifiers

// Feature Chat (docs/design/screens/FEATURE_CHAT.md), the canvas board `FeatureChatMac.dc.html`:
// a feature's lifecycle and counts (`Features`, protocol 1.7), its conversation with the lead
// (`FeatureChat`, dated at 1.8), the questions, the requirements document's assumptions
// (1.8), the decisions in use (`Documents`) and the discovery history. Nothing here approves:
// a draft is opened in the Approval Inbox. New Feature (no feature held) is NewFeature.swift.

/// What the screen shows. Every field is the helper's answer or what the human typed.
struct FeatureChatState: Equatable {
    var connection: HelperConnection
    var admission: String
    var pid: Int32 = 0
    /// Hello's query names; empty when the helper did not say.
    var queries: [String] = []
    var index: ChatIndex?
    var indexLoad = PlanLoad.idle
    /// The feature the screen holds.
    var feature: String?
    var chat: FeatureChatPage?
    var chatLoad = PlanLoad.idle
    var prdText = ""
    var commandPending = false
    var outcome: StartOutcome?
    /// Protocol 1.7: the held feature's code, stages and counts.
    var summary: FeatureSummary?
    /// Protocol 1.6: the documents, for the decisions in use.
    var documents: DocumentList?
    /// The projects, for the lead's provider.
    var projects: ProjectList?
    /// The note of the next turn, and the text files attached to it.
    var message = ""
    var attachments: [ReferenceFile] = []
    var unreadable: [String] = []
    /// Answers as typed, by question.
    var answers: [String: String] = [:]

    /// The held feature's name: the features answer's (1.7), else the chat index's (1.6).
    var featureName: String? {
        summary?.name ?? index?.features.first { $0.id == feature }?.name
    }

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
    /// Open: no human answered it yet. Resolved: a human answered it.
    var openQuestions: [LeadQuestion] { questions.filter { $0.answeredBy != "HUMAN" } }
    var resolvedQuestions: [LeadQuestion] { questions.filter { $0.answeredBy == "HUMAN" } }

    /// "Q3": the question's place in the chat, oldest first.
    func number(_ question: LeadQuestion) -> String {
        "Q\((questions.firstIndex { $0.id == question.id } ?? 0) + 1)"
    }

    /// The four discovery states of STATES.md, section 4, from what the chat reports.
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

    /// The documents of this feature that are approved: the decisions it builds on.
    var decisions: [DocumentSummary] {
        (documents?.documents ?? []).filter { $0.feature == feature && $0.approval == "Approved" }
    }

    /// The repository a turn of the held feature names: the feature's own.
    var turnRepository: String? {
        guard let feature else { return nil }
        return index?.features.first { $0.id == feature }?.repository
    }

    /// The lead's provider: the default of the project that holds the feature's repository,
    /// else the provider of its newest turn.
    var provider: String {
        if let repository = turnRepository,
           let project = projects?.projects.first(where: { $0.repositories.contains(repository) }) {
            return project.defaultProvider
        }
        return turns.last?.provider ?? "ClaudeCode"
    }

    /// The next turn's message: the note, then each attached file under its own heading.
    var composed: String {
        var parts = [message.trimmingCharacters(in: .whitespacesAndNewlines)].filter { !$0.isEmpty }
        for file in attachments {
            parts.append("## Reference file: \(file.name)\n\n\(file.text.trimmingCharacters(in: .whitespacesAndNewlines))")
        }
        return parts.joined(separator: "\n\n")
    }

    /// The plan can be drafted: the requirements are approved and no plan was drafted.
    var canDraftPlan: Bool { prd?.approved == true && plan == nil && running == nil }

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

    /// The header: "FT-3 Leaderboard API" and "Crew · Discovery rev 2".
    static func heading(feature: String, summary: FeatureSummary?, name: String?,
                        turns: [LeadTurn]) -> (title: String, subtitle: String) {
        let title = [summary?.code, summary?.name ?? name ?? "Feature \(short(feature))"].compactMap { $0 }
            .joined(separator: " ")
        let last = turns.last
        let round = last.map { turn in
            "\(kind(turn.kind)) rev \(turns.filter { $0.kind == turn.kind }.count)"
        }
        return (title, [summary?.projectName, round].compactMap { $0 }.joined(separator: " · ").nonEmpty ?? "Feature Chat")
    }

    /// "Discovery rev 2 · Nov 12": the turn's kind, its place among turns of that kind and
    /// the day it started.
    func separator(_ turn: LeadTurn) -> String {
        let n = turns.filter { $0.kind == turn.kind && $0.ordinal <= turn.ordinal }.count
        let day = turn.started.map { $0.formatted(.dateTime.month(.abbreviated).day()) }
        return (["\(Self.kind(turn.kind)) rev \(n)"] + [day].compactMap { $0 }).joined(separator: " · ")
    }

    /// Why a turn of `kind` cannot be sent, or nil when it can. The helper decides the rest.
    func sendBlocked(_ kind: String) -> String? {
        if link != .connected { return "Unavailable: the core is not connected." }
        if unsupported { return "This core has no Feature Chat." }
        if admissionClosed { return "Admission is closed. The core refuses a turn until it is open." }
        if commandPending { return "Unavailable while a command awaits the core." }
        if running != nil { return "A turn is running. Stop it or wait for its result." }
        if feature == nil { return "No feature is held. New Feature starts one." }
        if turnRepository == nil { return "The core did not report this feature's repository. Reload." }
        if kind == "Plan", prd?.approved != true {
            return "A plan needs the approved requirements. Approve the draft in the Approval Inbox."
        }
        if composed.utf8.count > NewFeatureState.maxMessageBytes {
            return "The note and attached files are \(composed.utf8.count / 1024) KiB; the core takes at most 16 KiB."
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

    /// The conversation as Markdown, for Export Transcript.
    var transcript: String {
        var lines = ["# \(featureName ?? "Feature \(Self.short(feature ?? ""))")", ""]
        for turn in turns {
            lines += ["## \(separator(turn))", "", "**You:** \(turn.message.isEmpty ? "(another round)" : turn.message)", ""]
            if !turn.summary.isEmpty { lines += ["**Jojo (\(Self.provider(turn.provider))):** \(turn.summary)", ""] }
            for q in turn.questions {
                lines += ["- \(number(q)): \(q.prompt)" + (q.answer.map { " — \($0)" } ?? ""), ""]
            }
        }
        return lines.joined(separator: "\n")
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
}

struct FeatureChatActions {
    var open: (Screen) -> Void = { _ in }
    var reload: () -> Void = {}
    /// Holds a feature; nil holds none (New Feature).
    var choose: (String?) -> Void = { _ in }
    var editMessage: (String) -> Void = { _ in }
    var attach: ([URL]) -> Void = { _ in }
    var detach: (String) -> Void = { _ in }
    var editAnswer: (String, String) -> Void = { _, _ in }
    var send: (String) -> Void = { _ in }
    var stop: () -> Void = {}
    var answer: (LeadQuestion, QuestionAnswer) -> Void = { _, _ in }
    /// Loads a document's review in the Approval Inbox and opens it.
    var review: (String) -> Void = { _ in }
}

/// The Markdown of Export Transcript, written to the file the human chooses.
struct TranscriptDocument: FileDocument {
    static let readableContentTypes = [UTType(filenameExtension: "md") ?? .plainText]
    var text: String

    init(text: String) { self.text = text }

    init(configuration: ReadConfiguration) throws {
        text = configuration.file.regularFileContents.flatMap { String(data: $0, encoding: .utf8) } ?? ""
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        FileWrapper(regularFileWithContents: Data(text.utf8))
    }
}

/// The chat's header actions: Create Spike (not available) and Export Transcript.
struct ChatHeaderActions: View {
    @Environment(\.theme) private var theme
    let export: () -> Void

    var body: some View {
        CapsuleGroup {
            Button("Create Spike") {}.buttonStyle(CapsuleTextButtonStyle()).disabled(true)
                .help("Not available yet: the core has no spikes.")
            Button("Export Transcript", action: export).buttonStyle(CapsuleTextButtonStyle())
        }
    }
}

/// A text button inside a header capsule group: 12, transparent, `grayBg` under the pointer.
struct CapsuleTextButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        Face(configuration: configuration)
    }

    private struct Face: View {
        @Environment(\.theme) private var theme
        @Environment(\.isEnabled) private var enabled
        @State private var hovered = false
        let configuration: Configuration

        var body: some View {
            configuration.label
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(enabled ? theme.text : theme.text3)
                .padding(.horizontal, 12)
                .frame(height: 32)
                .background(hovered && enabled ? theme.grayBg : .clear, in: Capsule())
                .contentShape(Capsule())
                .opacity(configuration.isPressed ? 0.8 : 1)
                .onHover { hovered = $0 }
        }
    }
}

struct FeatureChatScreen: View {
    @ObservedObject var model: AppModel
    /// The chat is read only while this destination is the shown one.
    let visible: Bool
    /// New Feature's project, shared with the header's place.
    @Binding var project: String?
    /// Export Transcript, asked from the header.
    @Binding var exporting: Bool
    let open: (Screen) -> Void
    @State private var objective = ""
    @State private var constraints = ""
    @State private var references: [ReferenceFile] = []
    @State private var unreadable: [String] = []
    @State private var repository: String?
    @State private var message = ""
    @State private var attachments: [ReferenceFile] = []
    @State private var unattached: [String] = []
    @State private var answers: [String: String] = [:]

    var body: some View {
        let workspace = model.workspace
        Group {
            if workspace.chatFeature == nil { newFeature } else { chat }
        }
        .onAppear { if visible { read() } }
        .onChange(of: visible) { read(); if visible { workspace.loadChat() } }
        .onChange(of: workspace.snapshotSeq) { if visible { read(); workspace.loadChat() } }
        .onChange(of: workspace.chatFeature) {
            read()
            if workspace.chatFeature != nil { (objective, constraints, references, unreadable) = ("", "", [], []) }
            (message, attachments, unattached) = ("", [], [])
        }
        .fileExporter(isPresented: $exporting, document: TranscriptDocument(text: chatState.transcript),
                      contentType: TranscriptDocument.readableContentTypes[0],
                      defaultFilename: "\(chatState.summary?.code ?? "Feature") transcript") { _ in }
    }

    /// The chat index and the projects; while a feature is held, its features row too.
    private func read() {
        let workspace = model.workspace
        workspace.chatShown = visible && workspace.chatFeature != nil
        guard visible else { return }
        workspace.loadChatIndex()
        workspace.loadProjects()
        workspace.loadFeatures()
    }

    private var newFeature: some View {
        let workspace = model.workspace
        let state = NewFeatureState(
            connection: model.client.connection, admission: workspace.admission, queries: model.client.hello?.queries ?? [],
            projects: workspace.projects, projectsLoad: workspace.projectsLoad, index: workspace.chatIndex,
            indexLoad: workspace.chatIndexLoad, commandPending: workspace.commandPending, outcome: workspace.turnOutcome,
            project: project, repository: repository, objective: objective, constraints: constraints,
            references: references, unreadable: unreadable)
        return NewFeatureView(
            state: state,
            actions: NewFeatureActions(
                chooseProject: { project = $0; repository = nil },
                chooseRepository: { repository = $0 },
                editObjective: { objective = $0 },
                editConstraints: { constraints = $0 },
                addReferences: { (references, unreadable) = Self.read($0, into: references, unreadable) },
                removeReference: { name in references.removeAll { $0.name == name } },
                start: {
                    guard state.sendBlocked == nil, let repository = state.turnRepository, let provider = state.provider else { return }
                    workspace.sendTurn(feature: Identity.randomClientID(), kind: "Discover", repository: repository,
                                       provider: provider, message: state.message)
                },
                cancel: { open(.dashboard) },
                open: open),
            shown: visible)
    }

    /// Reads each chosen file as UTF-8 text; a file that is not text is named and left out.
    static func read(_ urls: [URL], into files: [ReferenceFile], _ unreadable: [String]) -> ([ReferenceFile], [String]) {
        var (files, unreadable) = (files, unreadable)
        for url in urls {
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            let name = url.lastPathComponent
            if let data = try? Data(contentsOf: url), let text = String(data: data, encoding: .utf8) {
                files.removeAll { $0.name == name }
                files.append(ReferenceFile(name: name, text: text))
                unreadable.removeAll { $0 == name }
            } else if !unreadable.contains(name) {
                unreadable.append(name)
            }
        }
        return (files, unreadable)
    }

    private var chatState: FeatureChatState {
        let workspace = model.workspace
        return FeatureChatState(
            connection: model.client.connection, admission: workspace.admission, pid: model.client.processIdentifier,
            queries: model.client.hello?.queries ?? [], index: workspace.chatIndex, indexLoad: workspace.chatIndexLoad,
            feature: workspace.chatFeature, chat: workspace.chat, chatLoad: workspace.chatLoad, prdText: workspace.prdText,
            commandPending: workspace.commandPending, outcome: workspace.turnOutcome,
            summary: workspace.features?.features.first { $0.id == workspace.chatFeature },
            documents: workspace.documents, projects: workspace.projects, message: message,
            attachments: attachments, unreadable: unattached, answers: answers)
    }

    private var chat: some View {
        let workspace = model.workspace
        let state = chatState
        return FeatureChatView(
            state: state,
            actions: FeatureChatActions(
                open: open,
                reload: { workspace.loadChatIndex(); workspace.loadChat() },
                choose: { workspace.openChat($0) },
                editMessage: { message = $0 },
                attach: { (attachments, unattached) = Self.read($0, into: attachments, unattached) },
                detach: { name in attachments.removeAll { $0.name == name } },
                editAnswer: { answers[$0] = $1 },
                send: { kind in
                    guard state.sendBlocked(kind) == nil, let feature = state.feature, let repository = state.turnRepository else { return }
                    workspace.sendTurn(feature: feature, kind: kind, repository: repository, provider: state.provider,
                                       message: kind == "Plan" ? "" : state.composed)
                    if kind != "Plan" { (message, attachments, unattached) = ("", [], []) }
                },
                stop: { if let turn = state.running?.id { workspace.cancelTurn(turn) } },
                answer: { question, action in
                    workspace.answer(question, action)
                    answers[question.id] = nil
                },
                review: { document in
                    workspace.loadReview(document)
                    open(.approvalInbox)
                }),
            shown: visible)
    }
}

struct FeatureChatView: View {
    @Environment(\.theme) private var theme
    let state: FeatureChatState
    var actions = FeatureChatActions()
    /// Keyboard shortcuts act only while the screen is shown.
    var shown = true
    @State private var resolvedShown = false
    @State private var draftShown = false
    @State private var attaching = false
    @FocusState private var focused: String?

    var body: some View {
        GeometryReader { geometry in
            let beside = geometry.size.width >= 860
            Scrolling {
                VStack(alignment: .leading, spacing: 14) {
                    banners
                    lifecycle
                    if beside {
                        HStack(alignment: .top, spacing: 14) {
                            conversation.frame(maxWidth: .infinity)
                            inspector.frame(width: min(330, max(290, geometry.size.width * 0.28)))
                        }
                    } else {
                        conversation
                        inspector
                    }
                }
                .padding(EdgeInsets(top: 6, leading: 10, bottom: 14, trailing: 12))
            }
        }
        .fileImporter(isPresented: $attaching, allowedContentTypes: [.text], allowsMultipleSelection: true) { result in
            if case .success(let urls) = result { actions.attach(urls) }
        }
    }

    // MARK: Banners

    @ViewBuilder
    private var banners: some View {
        switch state.link {
        case .lost:
            Banner(.warning, title: "Jojo core isn't responding",
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
            Banner(.danger, title: "Execution admission is closed",
                   message: "The core refuses a turn until a reviewed recovery start opens admission.") {
                Button("Open Review") { actions.open(.recoveryCenter) }.buttonStyle(.jojo(.primary, compact: true))
            }
        }
    }

    // MARK: Lifecycle

    /// The lifecycle card of COMPONENTS.md, section 2.1, from the core's stages (1.7), with
    /// the feature's counts after it.
    private var lifecycle: some View {
        HStack(spacing: 14) {
            LifecycleStepper(stages: state.summary?.stages)
            if let s = state.summary {
                HStack(spacing: 6) {
                    if s.running > 0 { OperationalPill("\(s.running) running", tone: .info) }
                    if s.blocked > 0 { OperationalPill("\(s.blocked) blocked", tone: .danger) }
                    if s.awaiting > 0 { OperationalPill("\(s.awaiting) awaiting approval", tone: .warn) }
                }
            }
            Spacer(minLength: 0)
        }
        .padding(EdgeInsets(top: 10, leading: 14, bottom: 10, trailing: 14))
        .cardElevation(radius: 14)
    }

    // MARK: Conversation

    private var conversation: some View {
        VStack(alignment: .leading, spacing: 0) {
            featureBanner
            VStack(alignment: .leading, spacing: 12) {
                switch (state.chat, state.chatLoad) {
                case (let chat?, _):
                    if chat.turns.isEmpty {
                        Placeholder(symbol: "text.bubble", tile: Palette.blue, title: "Describe what users should be able to do",
                                    message: "Jojo reads the code and asks before planning.") { EmptyView() }
                    }
                    ForEach(chat.turns) { turn in exchange(turn) }
                    planReceipt
                    ForEach(state.openQuestions) { question in openQuestion(question) }
                case (nil, .failed(let reason)):
                    Placeholder(symbol: "exclamationmark.bubble", tile: Palette.red, title: "The chat was not read",
                                message: FeatureChatState.loadFailure(reason)) {
                        Button("Read Again") { actions.reload() }.buttonStyle(.jojo(.primary, compact: true))
                    }
                    Text(reason).textStyle(.mono).foregroundStyle(theme.text3).textSelection(.enabled)
                case (nil, _):
                    SkeletonCard()
                }
            }
            .padding(EdgeInsets(top: 20, leading: 20, bottom: 12, trailing: 20))
            composer
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .cardElevation(radius: 18)
        .clipShape(RoundedRectangle(cornerRadius: 18))
    }

    /// The inline banner of COMPONENTS.md, section 10: what blocks the feature or one step.
    @ViewBuilder
    private var featureBanner: some View {
        let blocking: LeadQuestion? = {
            if case .blocked(let q) = state.discovery { return q }
            return state.stepQuestions.first
        }()
        if let q = blocking {
            HStack(spacing: 12) {
                RoundedRectangle(cornerRadius: 8).fill(Palette.orange.color).frame(width: 26, height: 26)
                    .overlay(Image(systemName: "exclamationmark").font(.system(size: 13, weight: .bold)).foregroundStyle(.white))
                VStack(alignment: .leading, spacing: 1) {
                    Text(q.step.map { "Feature continues · \($0) is blocked by \(state.number(q))" }
                         ?? "Whole feature blocked by \(state.number(q))")
                        .textStyle(.bodyStrong).foregroundStyle(theme.text)
                    Text(q.step == nil ? "The requirements can't be reviewed until it is answered."
                         : "Only that step waits for this answer. The rest of the plan is unaffected.")
                        .textStyle(.secondary).foregroundStyle(theme.text2)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                Button("Answer \(state.number(q))") { focused = q.id }.buttonStyle(.jojo(.primary, compact: true))
            }
            .padding(EdgeInsets(top: 10, leading: 12, bottom: 10, trailing: 12))
            .glassPanel(radius: 16)
            .padding(EdgeInsets(top: 12, leading: 12, bottom: 0, trailing: 12))
        }
    }

    private func exchange(_ turn: LeadTurn) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(state.separator(turn)).font(.system(size: 11, weight: .semibold)).foregroundStyle(theme.text3)
                .frame(maxWidth: .infinity)
            HumanBubble(text: turn.message.isEmpty ? "Another round with the answers so far."
                        : turn.message + (turn.messageComplete ? "" : "…"), quiet: turn.message.isEmpty)
            if turn.running {
                LeadBubble {
                    HStack(spacing: 8) {
                        OperationalPill(turn.state == "Stopping" ? .waiting : .running,
                                        label: turn.state == "Stopping" ? "Stopping" : "Investigating", standalone: true)
                        Text("\(FeatureChatState.provider(turn.provider)) is reading the repository. Read-only.")
                            .textStyle(.secondary).foregroundStyle(theme.text2)
                    }
                    Button("Stop Turn") { actions.stop() }.buttonStyle(.jojo(.secondary, compact: true))
                        .disabled(turn.state == "Stopping" || state.link != .connected || state.commandPending)
                }
            } else {
                LeadBubble {
                    if !turn.summary.isEmpty { Inline(turn.summary) }
                    if turn.state != "Replied" {
                        let (pill, label) = FeatureChatState.operational(turn.state)
                        OperationalPill(pill, label: label)
                        if !turn.detail.isEmpty {
                            Text(turn.detail).textStyle(.mono).foregroundStyle(theme.text2).textSelection(.enabled)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
            }
            receipts(turn)
        }
    }

    /// The receipt lines after a lead's reply: what it asked and what it drafted.
    @ViewBuilder
    private func receipts(_ turn: LeadTurn) -> some View {
        if !turn.questions.isEmpty {
            let resolved = turn.questions.filter { $0.answeredBy == "HUMAN" }
            Receipt(tone: resolved.count == turn.questions.count ? .ok : .warn,
                    text: "Asked " + turn.questions.map(state.number).joined(separator: ", ")
                        + (resolved.isEmpty ? "" : " · \(resolved.map(state.number).joined(separator: ", ")) resolved by you"))
        }
        if let prd = turn.prd {
            let current = state.prd?.revision == prd.revision
            Receipt(tone: current && state.prd?.approved == true ? .ok : .info,
                    text: "PRD rev \(prd.revision) " + (current && state.prd?.approved == true ? "approved" : "drafted"),
                    link: draftShown && current ? "Hide PRD" : "Open PRD") {
                if current { draftShown.toggle() } else { actions.review(prd.document) }
            }
            if draftShown, current { draft }
        }
        if let plan = turn.plan {
            Receipt(tone: .info, text: plan.drafted ? "ADR, plan and validation drafted · \(plan.steps.count) steps"
                        : "ADR drafted · the plan follows its approval", link: "Review") { actions.review(plan.adr) }
        }
    }

    /// Once the requirements are approved and no plan was drafted: Draft Plan.
    @ViewBuilder
    private var planReceipt: some View {
        if state.canDraftPlan, let prd = state.prd {
            Receipt(tone: .info, text: "Next: a plan from PRD rev \(prd.revision)", link: "Draft Plan") { actions.send("Plan") }
            if let reason = state.sendBlocked("Plan") { DisabledReason(reason).padding(.leading, 34) }
        }
    }

    /// An open question in the conversation: its number, what it blocks, the prompt and a
    /// written answer. The board's choices are not built: the core reports none.
    private func openQuestion(_ question: LeadQuestion) -> some View {
        LeadBubble(highlighted: focused == question.id) {
            HStack(spacing: 8) {
                Text(state.number(question)).font(.system(size: 12, weight: .bold, design: .monospaced))
                questionPill(question)
            }
            Text(question.prompt).font(.system(size: 15, weight: .semibold)).foregroundStyle(theme.text)
                .fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 8) {
                LineField(prompt: "Write your answer", size: 13,
                          text: Binding(get: { state.answers[question.id] ?? "" }, set: { actions.editAnswer(question.id, $0) }),
                          submit: { if state.answerBlocked(question) == nil { actions.answer(question, .answer(state.answers[question.id] ?? "")) } })
                    .focused($focused, equals: question.id)
                    .padding(.horizontal, 12)
                    .frame(minHeight: 34)
                    .background(theme.cardHover, in: RoundedRectangle(cornerRadius: 9))
                    .overlay(RoundedRectangle(cornerRadius: 9).strokeBorder(theme.sep, lineWidth: 0.5))
                Button("Answer") { actions.answer(question, .answer(state.answers[question.id] ?? "")) }
                    .buttonStyle(.jojo(.primary, compact: true))
                    .disabled(state.answerBlocked(question) != nil)
            }
        }
    }

    @ViewBuilder
    private func questionPill(_ question: LeadQuestion) -> some View {
        if let step = question.step {
            OperationalPill("Blocks \(step) only", tone: .danger)
        } else if question.blocking {
            OperationalPill("Blocking", tone: .danger)
        } else {
            OperationalPill("Non-blocking", tone: .neutral)
        }
    }

    /// The composer capsule: attach a text file, the note, ⌘↵ and send.
    private var composer: some View {
        VStack(alignment: .leading, spacing: 6) {
            if !state.attachments.isEmpty || !state.unreadable.isEmpty {
                WrappingRow(spacing: 6, lineSpacing: 6) {
                    ForEach(state.attachments) { file in
                        HStack(spacing: 5) {
                            Image(systemName: "doc.plaintext")
                            Text(file.name).lineLimit(1)
                            Button { actions.detach(file.name) } label: { Image(systemName: "xmark.circle.fill") }
                                .buttonStyle(.plain).foregroundStyle(theme.text3)
                        }
                        .font(.system(size: 12)).padding(.horizontal, 10).frame(height: 26)
                        .background(theme.accentTint, in: Capsule())
                    }
                    ForEach(state.unreadable, id: \.self) { name in
                        Text("\(name) is not a text file; it was not attached.").textStyle(.secondary).foregroundStyle(theme.orange)
                    }
                }
                .padding(.horizontal, 8)
            }
            HStack(spacing: 8) {
                Button { attaching = true } label: { Label("Attach", systemImage: "paperclip") }
                    .buttonStyle(IconButtonStyle()).help("Attach a text file to the next message")
                    .disabled(state.link != .connected)
                LineField(prompt: "Message Jojo about \(state.summary?.code ?? "this feature")", size: 14,
                          text: Binding(get: { state.message }, set: actions.editMessage))
                Text("⌘↵").textStyle(.secondary).foregroundStyle(theme.text2)
                Button { actions.send("Discover") } label: {
                    Image(systemName: "arrow.up").font(.system(size: 14, weight: .semibold)).foregroundStyle(.white)
                        .frame(width: 34, height: 34)
                        .background(state.sendBlocked("Discover") == nil ? theme.accent : theme.grayBg, in: Circle())
                }
                .buttonStyle(.plain)
                .keyboardShortcut(shown ? KeyboardShortcut(.return, modifiers: .command) : nil)
                .disabled(state.sendBlocked("Discover") != nil)
                .accessibilityLabel("Send")
            }
            .padding(EdgeInsets(top: 4, leading: 8, bottom: 4, trailing: 4))
            .frame(minHeight: 44)
            .glassCapsule()
            HStack(spacing: 8) {
                Text("Lead: \(FeatureChatState.provider(state.provider)), the project's default").textStyle(.secondary)
                    .foregroundStyle(theme.text3)
                if let reason = state.sendBlocked("Discover"), state.running == nil || !reason.hasPrefix("A turn is running") {
                    DisabledReason(reason)
                }
            }
            .padding(.horizontal, 10)
            outcome.padding(.horizontal, 8)
        }
        .padding(EdgeInsets(top: 0, leading: 12, bottom: 12, trailing: 12))
    }

    @ViewBuilder
    private var outcome: some View {
        switch state.outcome {
        case .sent?:
            RequestedNotice(text: "Turn sent. Shown as running only after the core records it.")
        case .refused(let code)?:
            VStack(alignment: .leading, spacing: 4) {
                Banner(.warning, title: "The core refused the turn", message: FeatureChatState.refusal(code))
                Text(code).textStyle(.mono).foregroundStyle(theme.text3).textSelection(.enabled)
            }
        case .unknown?:
            HStack(spacing: 8) {
                EvidencePill(.unknown, label: "Outcome unknown")
                Text("Never shown as done or failed. Never sent again.").textStyle(.secondary).foregroundStyle(theme.text3)
            }
        case .accepted?, nil:
            EmptyView()
        }
    }

    /// The draft's text, as the core renders it, under its receipt.
    private var draft: some View {
        VStack(alignment: .leading, spacing: 6) {
            if state.prdText.isEmpty {
                Text(state.link == .connected ? "Reading the draft…" : "Not read.").textStyle(.secondary).foregroundStyle(theme.text3)
            } else {
                Text(state.prdText).textStyle(.secondary).foregroundStyle(theme.text)
                    .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
            }
            if let prd = state.prd {
                Button("Open in Approval Inbox") { actions.review(prd.document) }.buttonStyle(.jojo(.secondary, compact: true))
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .well(radius: 12)
        .padding(.leading, 34)
    }

    // MARK: Inspector

    private var inspector: some View {
        VStack(alignment: .leading, spacing: 18) {
            questionsSection
            assumptionsSection
            decisionsSection
            historySection
            NoteLine("Suggestions, spikes, a verified state of an assumption, choices for a question and what a question affects are not reported.")
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .topLeading)
        .glassPanel(radius: 18)
    }

    private var questionsSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            GroupLabel("Open questions · \(state.openQuestions.count)").padding(.horizontal, -4)
            ChoiceCapsule(options: [("open", "Open"), ("resolved", "Resolved \(state.resolvedQuestions.count)")],
                          selection: resolvedShown ? "resolved" : "open", label: "Question filter") { resolvedShown = $0 == "resolved" }
            if resolvedShown {
                if state.resolvedQuestions.isEmpty {
                    Text("Nothing resolved yet.").textStyle(.secondary).foregroundStyle(theme.text3)
                } else {
                    KeyValueCard(rows: state.resolvedQuestions.map { q in
                        (state.number(q), q.answer.map { $0 + (q.answerComplete ? "" : "…") } ?? q.prompt, "Done")
                    })
                }
            } else if state.openQuestions.isEmpty {
                Text(state.questions.isEmpty ? "The lead asked nothing yet." : "Every question is answered.")
                    .textStyle(.secondary).foregroundStyle(theme.text3)
            } else {
                ForEach(state.openQuestions) { q in
                    VStack(alignment: .leading, spacing: 6) {
                        HStack(spacing: 6) {
                            Text(state.number(q)).font(.system(size: 12, weight: .bold, design: .monospaced))
                            questionPill(q)
                        }
                        Text(q.prompt).textStyle(.bodyStrong).foregroundStyle(theme.text).fixedSize(horizontal: false, vertical: true)
                        if let step = q.step { Text("Affects \(step)").textStyle(.secondary).foregroundStyle(theme.text2) }
                        HStack(spacing: 6) {
                            Button("Resolve Question") { focused = q.id }
                                .buttonStyle(.jojo(q.blocking ? .primary : .secondary, compact: true))
                            if q.blocking {
                                Button("Defer") { actions.answer(q, .deferQuestion) }.buttonStyle(.jojo(.ghost, compact: true))
                                    .disabled(state.link != .connected || state.commandPending)
                            } else {
                                Button("Make Blocking") { actions.answer(q, .block) }.buttonStyle(.jojo(.ghost, compact: true))
                                    .disabled(state.link != .connected || state.commandPending)
                            }
                        }
                        .padding(.top, 4)
                    }
                    .padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .cardElevation(radius: 12)
                }
            }
        }
    }

    private var assumptionsSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            GroupLabel("Assumptions").padding(.horizontal, -4)
            if let assumptions = state.chat?.assumptions {
                if assumptions.isEmpty {
                    Text(state.prd == nil ? "No requirements draft yet." : "The draft states none.")
                        .textStyle(.secondary).foregroundStyle(theme.text3)
                } else {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(Array(assumptions.enumerated()), id: \.element.id) { index, a in
                            VStack(alignment: .leading, spacing: 4) {
                                HStack(spacing: 6) {
                                    Text(a.id).font(.system(size: 12, weight: .bold, design: .monospaced))
                                    if a.state == "Dropped" {
                                        OperationalPill("Dropped", tone: .danger)
                                    } else {
                                        OperationalPill("In PRD", tone: .neutral)
                                    }
                                }
                                Text(a.statement).textStyle(.secondary)
                                    .strikethrough(a.state == "Dropped")
                                    .foregroundStyle(a.state == "Dropped" ? theme.text2 : theme.text)
                                    .fixedSize(horizontal: false, vertical: true)
                                Text(a.state == "Dropped" ? "Dropped in PRD rev \(a.revision)" : "Since PRD rev \(a.revision)")
                                    .textStyle(.secondary).foregroundStyle(theme.text3)
                            }
                            .padding(EdgeInsets(top: 9, leading: 12, bottom: 9, trailing: 12))
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .overlay(alignment: .top) { if index > 0 { Rectangle().fill(theme.sep).frame(height: 0.5) } }
                        }
                    }
                    .cardElevation(radius: 12)
                }
            } else {
                Text("Not reported by this core (protocol 1.8).").textStyle(.secondary).foregroundStyle(theme.text3)
            }
        }
    }

    private var decisionsSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            GroupLabel("Decisions in use").padding(.horizontal, -4)
            if state.decisions.isEmpty {
                Text("Nothing of this feature is approved yet.").textStyle(.secondary).foregroundStyle(theme.text3)
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(state.decisions.enumerated()), id: \.element.id) { index, d in
                        Button { actions.review(d.id) } label: {
                            VStack(alignment: .leading, spacing: 4) {
                                HStack(spacing: 6) {
                                    Text(d.kindName).font(.system(size: 12, weight: .bold)).foregroundStyle(theme.text)
                                    OperationalPill("Approved", tone: .ok)
                                }
                                Text(d.title ?? "\(d.kindName) of this feature").textStyle(.secondary).foregroundStyle(theme.text)
                                Text("rev \(d.revision)" + (d.decided.map { " · " + $0.formatted(.dateTime.month(.abbreviated).day()) } ?? ""))
                                    .textStyle(.secondary).foregroundStyle(theme.text3)
                            }
                            .padding(EdgeInsets(top: 9, leading: 12, bottom: 9, trailing: 12))
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .overlay(alignment: .top) { if index > 0 { Rectangle().fill(theme.sep).frame(height: 0.5) } }
                    }
                }
                .cardElevation(radius: 12)
            }
        }
    }

    private var historySection: some View {
        VStack(alignment: .leading, spacing: 10) {
            GroupLabel("Discovery history").padding(.horizontal, -4)
            if state.turns.isEmpty {
                Text("No turn yet.").textStyle(.secondary).foregroundStyle(theme.text3)
            } else {
                KeyValueCard(rows: state.turns.reversed().map { turn in
                    let line = turn.summary.split(separator: "\n").first.map { $0.replacingOccurrences(of: "`", with: "") }
                        ?? FeatureChatState.operational(turn.state).1
                    return ("rev \(turn.ordinal)", line,
                            turn.started.map { $0.formatted(.dateTime.month(.abbreviated).day()) } ?? "—")
                }, plain: true)
            }
        }
    }
}

// MARK: - Components

/// The feature lifecycle stepper of COMPONENTS.md, section 2.1: six stages, dots 8 and bars
/// 2 high. The core reports five; Evidence is not reached.
struct LifecycleStepper: View {
    @Environment(\.theme) private var theme
    let stages: [FeatureStage]?
    static let names = ["Discovery", "Specification", "Planning", "Execution", "Verification", "Evidence"]

    var body: some View {
        let all = (stages ?? []) + Array(repeating: FeatureStage.off, count: max(0, 6 - (stages?.count ?? 0)))
        HStack(spacing: 8) {
            ForEach(Array(Self.names.enumerated()), id: \.offset) { index, name in
                let stage = all[index]
                HStack(spacing: 6) {
                    dot(stage)
                    Text(name).font(.system(size: 12, weight: current(stage) ? .semibold : .medium))
                        .foregroundStyle(current(stage) ? theme.text : theme.text2).lineLimit(1)
                }
                .help(stage.label ?? "Not reached")
                if index < Self.names.count - 1 {
                    Capsule().fill(stage.state == "Done" ? theme.green : theme.dotOff).frame(minWidth: 12, maxWidth: 40).frame(height: 2)
                }
            }
        }
        .fixedSize(horizontal: false, vertical: true)
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Lifecycle")
    }

    private func current(_ stage: FeatureStage) -> Bool { !["Done", "Off"].contains(stage.state) }

    @ViewBuilder private func dot(_ stage: FeatureStage) -> some View {
        switch stage.state {
        case "Done": Circle().fill(theme.green).frame(width: 8, height: 8)
        case "Off": Circle().fill(theme.dotOff).frame(width: 8, height: 8)
        case "Running", "Current":
            Circle().fill(theme.accent).frame(width: 8, height: 8).padding(3).background(theme.accentTint, in: Circle()).padding(-3)
        case "NeedsYou": Circle().fill(theme.orange).frame(width: 8, height: 8)
        case "Blocked", "Failed": Circle().fill(theme.red).frame(width: 8, height: 8)
        default: Circle().fill(theme.gray).frame(width: 8, height: 8)
        }
    }
}

/// A one-line text field without a frame; off screen, where a text field does not draw, its
/// text or prompt as plain text.
struct LineField: View {
    @Environment(\.theme) private var theme
    @Environment(\.offscreen) private var offscreen
    let prompt: String
    var size: CGFloat = 13
    @Binding var text: String
    var submit: () -> Void = {}

    var body: some View {
        if offscreen {
            Text(text.isEmpty ? prompt : text).font(.system(size: size)).foregroundStyle(text.isEmpty ? theme.text3 : theme.text)
                .lineLimit(1).frame(maxWidth: .infinity, alignment: .leading)
        } else {
            TextField(prompt, text: $text).textFieldStyle(.plain).font(.system(size: size)).onSubmit(submit)
        }
    }
}

/// The human's message: right-aligned, `accent` with white text, at most 78% wide.
struct HumanBubble: View {
    @Environment(\.theme) private var theme
    let text: String
    var quiet = false

    var body: some View {
        HStack {
            Spacer(minLength: 80)
            Text(text).font(.system(size: 14)).lineSpacing(3)
                .foregroundStyle(quiet ? theme.text2 : .white)
                .padding(EdgeInsets(top: 10, leading: 14, bottom: 10, trailing: 14))
                .background(quiet ? theme.grayBg : theme.accent, in: RoundedRectangle(cornerRadius: 16))
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
        }
    }
}

/// The lead's message: the "J" avatar (26, round, indigo) and a `cardHover` bubble.
struct LeadBubble<Content: View>: View {
    @Environment(\.theme) private var theme
    var highlighted = false
    @ViewBuilder let content: () -> Content

    var body: some View {
        HStack(alignment: .bottom, spacing: 8) {
            Circle().fill(Palette.indigo.color).frame(width: 26, height: 26)
                .overlay(Text("J").font(.system(size: 11, weight: .bold)).foregroundStyle(.white))
            VStack(alignment: .leading, spacing: 10) { content() }
                .padding(EdgeInsets(top: 10, leading: 14, bottom: 10, trailing: 14))
                .background(theme.cardHover, in: RoundedRectangle(cornerRadius: 16))
                .overlay(RoundedRectangle(cornerRadius: 16).strokeBorder(highlighted ? theme.accent : theme.sep,
                                                                       lineWidth: highlighted ? 2 : 0.5))
                .frame(maxWidth: 620, alignment: .leading)
            Spacer(minLength: 40)
        }
    }
}

/// Text with the core's inline code (`LobbyController.kt`) set in `mono`.
struct Inline: View {
    @Environment(\.theme) private var theme
    let text: String

    init(_ text: String) { self.text = text }

    var body: some View {
        Text(attributed).font(.system(size: 14)).lineSpacing(3).foregroundStyle(theme.text)
            .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
    }

    private var attributed: AttributedString {
        (try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
            ?? AttributedString(text)
    }
}

/// A receipt under a lead's reply: a dot, a sentence and an optional link (12, `text2`).
struct Receipt: View {
    @Environment(\.theme) private var theme
    let tone: Tone
    let text: String
    var link: String?
    var action: () -> Void = {}

    var body: some View {
        HStack(spacing: 8) {
            Circle().fill(theme.foreground(tone)).frame(width: 8, height: 8)
            Text(text).textStyle(.secondary).foregroundStyle(theme.text2)
            if let link {
                Button(action: action) { Text(link).font(.system(size: 12, weight: .semibold)).foregroundStyle(theme.accent) }
                    .buttonStyle(.plain)
            }
        }
        .padding(EdgeInsets(top: 6, leading: 10, bottom: 6, trailing: 12))
        .background(theme.grayBg, in: Capsule())
        .padding(.leading, 34)
    }
}

/// A key-value card of the inspector (COMPONENTS.md, section 4.4): rows padding 9 12, a key in
/// `mono`, the value, and a trailing pill ("Done") or a `sub` note.
struct KeyValueCard: View {
    @Environment(\.theme) private var theme
    let rows: [(key: String, value: String, trailing: String)]
    /// The trailing text is a `sub` note instead of a green pill.
    var plain = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(rows.enumerated()), id: \.offset) { index, row in
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Text(row.key).font(.system(size: 12, weight: plain ? .regular : .bold, design: .monospaced))
                        .foregroundStyle(plain ? theme.text2 : theme.text).frame(minWidth: 26, alignment: .leading)
                    Text(row.value).textStyle(.secondary).foregroundStyle(theme.text).lineLimit(2)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    if plain {
                        Text(row.trailing).textStyle(.secondary).foregroundStyle(theme.text2)
                    } else {
                        OperationalPill(row.trailing, tone: .ok)
                    }
                }
                .padding(EdgeInsets(top: 9, leading: 12, bottom: 9, trailing: 12))
                .overlay(alignment: .top) { if index > 0 { Rectangle().fill(theme.sep).frame(height: 0.5) } }
            }
        }
        .cardElevation(radius: 12)
    }
}

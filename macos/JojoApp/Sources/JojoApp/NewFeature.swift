import JojoCore
import SwiftUI
import UniformTypeIdentifiers

// New Feature (docs/design/screens/NEW_FEATURE.md), the canvas board `NewFeatureMac.dc.html`:
// Feature Chat holding no feature. The human names a project, describes the feature and may
// add constraints and text reference files; Start Discovery sends one `ChatTurn` (protocol
// 1.2) in the project's repository, on the project's default provider. Constraints and
// references travel in the turn's message under their own headings, so the core keeps them
// with the turn and carries them into every later prompt. Nothing new is stored.

/// A text file the human chose as a reference, read whole.
struct ReferenceFile: Identifiable, Equatable {
    var name: String
    var text: String
    var id: String { name }
}

/// What New Feature shows, as a plain value.
struct NewFeatureState: Equatable {
    /// The core's limit on a human's text (`MAX_HUMAN_TEXT_BYTES`).
    static let maxMessageBytes = 16 * 1024

    var connection: HelperConnection
    var admission = ""
    var queries: [String] = []
    var projects: ProjectList?
    var projectsLoad = PlanLoad.idle
    var index: ChatIndex?
    var indexLoad = PlanLoad.idle
    var commandPending = false
    var outcome: StartOutcome?
    /// The chosen project; nil is the first one listed.
    var project: String?
    /// The chosen repository; nil lets Jojo decide.
    var repository: String?
    var objective = ""
    var constraints = ""
    var references: [ReferenceFile] = []
    /// Files that were chosen and could not be read as text.
    var unreadable: [String] = []

    var connected: Bool {
        if case .ready = connection { return true }
        return false
    }

    var admissionClosed: Bool { connected && !admission.isEmpty && admission != "Open" }
    var unsupported: Bool { connected && !queries.isEmpty && !queries.contains("FeatureChat") }

    var chosenProject: ProjectRecord? {
        projects?.projects.first { $0.id == project } ?? projects?.projects.first
    }

    /// The registered repositories of the chosen project, in the project's order.
    var projectRepositories: [ChatIndex.Repository] {
        guard let chosenProject, let index else { return [] }
        return chosenProject.repositories.compactMap { id in index.repositories.first { $0.id == id } }
    }

    /// Why "Let Jojo decide" cannot choose, or nil when it can.
    var autoBlocked: String? {
        switch projectRepositories.count {
        case 1: return nil
        case 0: return "The project has no registered repository."
        default: return "The project has several repositories; a feature reads one until a plan can span several. Choose one."
        }
    }

    /// The repository the turn reads: the chosen one, or the project's only one.
    var turnRepository: String? {
        if let repository, projectRepositories.contains(where: { $0.id == repository }) { return repository }
        return autoBlocked == nil ? projectRepositories.first?.id : nil
    }

    /// The lead's provider: the project's default.
    var provider: String? { chosenProject?.defaultProvider }

    /// The turn's message: the description, then the constraints and each reference file
    /// under its own heading.
    var message: String {
        var parts = [objective.trimmingCharacters(in: .whitespacesAndNewlines)]
        let constraints = constraints.trimmingCharacters(in: .whitespacesAndNewlines)
        if !constraints.isEmpty { parts.append("## Technical constraints\n\n\(constraints)") }
        for file in references {
            parts.append("## Reference file: \(file.name)\n\n\(file.text.trimmingCharacters(in: .whitespacesAndNewlines))")
        }
        return parts.joined(separator: "\n\n")
    }

    var messageBytes: Int { message.utf8.count }

    /// Why Start Discovery cannot be sent, or nil when it can. The core decides the rest.
    var sendBlocked: String? {
        if !connected { return "Unavailable: the core is not connected." }
        if unsupported { return "This core has no Feature Chat." }
        if admissionClosed { return "Admission is closed. The core refuses a turn until it is open." }
        if commandPending { return "Unavailable while a command awaits the core." }
        if chosenProject == nil { return "Create a project first: the project names the repository and the provider." }
        if !objective.contains(where: { $0.isLetter || $0.isNumber }) { return "Describe what users should be able to do." }
        if turnRepository == nil { return autoBlocked ?? "Choose a repository." }
        if messageBytes > Self.maxMessageBytes {
            return "The description, constraints and references are \(messageBytes / 1024) KiB; the core takes at most 16 KiB."
        }
        return nil
    }

    /// The header's place: "Workspace · Crew".
    var place: String { ["Workspace", chosenProject?.name].compactMap { $0 }.joined(separator: " · ") }

    static func loadFailure(_ reason: String) -> String {
        if reason.hasPrefix("UnknownType") { return "This core has no project records." }
        return "The core did not answer the read of the projects."
    }
}

struct NewFeatureActions {
    var chooseProject: (String) -> Void = { _ in }
    /// nil lets Jojo decide.
    var chooseRepository: (String?) -> Void = { _ in }
    var editObjective: (String) -> Void = { _ in }
    var editConstraints: (String) -> Void = { _ in }
    var addReferences: ([URL]) -> Void = { _ in }
    var removeReference: (String) -> Void = { _ in }
    var start: () -> Void = {}
    var cancel: () -> Void = {}
    var open: (Screen) -> Void = { _ in }
}

struct NewFeatureView: View {
    @Environment(\.theme) private var theme
    let state: NewFeatureState
    var actions = NewFeatureActions()
    /// Keyboard shortcuts act only while the screen is shown.
    var shown = true
    @State private var choosing = false

    var body: some View {
        Scrolling {
            VStack(spacing: 16) {
                banners.frame(maxWidth: 780)
                form
            }
            .padding(EdgeInsets(top: 6, leading: 10, bottom: 22, trailing: 14))
            .frame(maxWidth: .infinity)
        }
        .fileImporter(isPresented: $choosing, allowedContentTypes: [.text], allowsMultipleSelection: true) { result in
            if case .success(let urls) = result { actions.addReferences(urls) }
        }
    }

    @ViewBuilder private var banners: some View {
        if !state.connected {
            Banner(.warning, title: "Jojo core isn't responding", message: "Nothing can be started until the core is connected again.")
        } else if state.unsupported {
            Banner(.warning, title: "This core has no Feature Chat",
                   message: "It does not list the chat queries in its Hello answer. Start a newer helper.")
        } else if state.admissionClosed {
            Banner(.danger, title: "Execution admission is closed",
                   message: "The core refuses a turn until a reviewed recovery start opens admission.") {
                Button("Open Review") { actions.open(.recoveryCenter) }.buttonStyle(.jojo(.primary, compact: true))
            }
        }
    }

    private var form: some View {
        VStack(alignment: .leading, spacing: 22) {
            VStack(alignment: .leading, spacing: 6) {
                Text("Describe the feature").font(.system(size: 26, weight: .bold)).tracking(26 * -0.025).foregroundStyle(theme.text)
                Text("Say what users should be able to do. Jojo investigates the code and asks before anything is planned.")
                    .font(.system(size: 14)).lineSpacing(4).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
            }
            projectRow
            VStack(alignment: .leading, spacing: 6) {
                FieldLabel("What should it do?")
                TextArea(prompt: "Players should be able to share a link that drops a friend straight into my lobby…",
                         text: Binding(get: { state.objective }, set: actions.editObjective), lines: 5)
                Text("No need to name files, APIs or architecture. That's what discovery is for.")
                    .textStyle(.secondary).foregroundStyle(theme.text2)
            }
            repositories
            // Two columns, as the board's grid at least 280 wide each: the window's minimum width
            // (900) still leaves the card room for both.
            HStack(alignment: .top, spacing: 12) { referenceFiles; designReferences }
            VStack(alignment: .leading, spacing: 6) {
                FieldLabel("Technical constraints", optional: true)
                TextArea(prompt: "For example: no new infrastructure",
                         text: Binding(get: { state.constraints }, set: actions.editConstraints), lines: 2)
            }
            nextSteps
            footer
        }
        .padding(EdgeInsets(top: 28, leading: 30, bottom: 28, trailing: 30))
        .frame(maxWidth: 780, alignment: .leading)
        .cardElevation(radius: 22)
    }

    private var referenceFiles: some View {
        VStack(alignment: .leading, spacing: 6) {
            FieldLabel("Reference files", optional: true)
            DropZone(symbol: "doc.text", title: state.references.isEmpty ? "Text files" : "\(state.references.count) added",
                     lines: ["Specs, tickets, notes as text. Added to the first message."]) {
                Button("Choose…") { choosing = true }.buttonStyle(.jojo(.secondary, compact: true))
                    .disabled(!state.connected)
            }
            ForEach(state.references) { file in
                HStack(spacing: 8) {
                    Image(systemName: "doc.plaintext").foregroundStyle(theme.accent)
                    Text(file.name).textStyle(.secondary).foregroundStyle(theme.text).lineLimit(1)
                    Text("\(file.text.utf8.count) bytes").textStyle(.secondary).foregroundStyle(theme.text3)
                    Spacer(minLength: 4)
                    Button { actions.removeReference(file.id) } label: { Image(systemName: "xmark.circle.fill") }
                        .buttonStyle(.plain).foregroundStyle(theme.text3).help("Remove \(file.name)")
                }
                .padding(.horizontal, 4)
            }
            ForEach(state.unreadable, id: \.self) { name in
                Text("\(name) is not a text file; it was not added.").textStyle(.secondary).foregroundStyle(theme.orange)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .frame(maxWidth: .infinity, alignment: .topLeading)
    }

    private var designReferences: some View {
        VStack(alignment: .leading, spacing: 6) {
            FieldLabel("Design references", optional: true)
            DropZone(symbol: "photo", title: "Images or exported frames",
                     lines: ["PNG, JPG, PDF: not available yet", "Figma link import: not available yet"]) {
                EmptyView()
            }
        }
        .frame(maxWidth: .infinity, alignment: .topLeading)
    }

    private var projectRow: some View {
        HStack(spacing: 12) {
            FieldLabel("Project").frame(width: 90, alignment: .leading)
            if let projects = state.projects?.projects, !projects.isEmpty {
                ChoiceCapsule(options: projects.map { ($0.id, $0.name) }, selection: state.chosenProject?.id,
                              label: "Project", choose: { actions.chooseProject($0) })
                if let provider = state.provider {
                    Text("Lead: \(providerName(provider)), the project's default").textStyle(.secondary).foregroundStyle(theme.text2)
                }
            } else if case .failed(let reason) = state.projectsLoad {
                Text(NewFeatureState.loadFailure(reason)).textStyle(.secondary).foregroundStyle(theme.text2)
            } else if state.projects != nil {
                Text("No project yet.").textStyle(.secondary).foregroundStyle(theme.text2)
                Button("Create Project") { actions.open(.projects) }.buttonStyle(.jojo(.secondary, compact: true))
            } else {
                Text(state.connected ? "Reading the projects…" : "Not read.").textStyle(.secondary).foregroundStyle(theme.text3)
            }
            Spacer(minLength: 0)
        }
    }

    private var repositories: some View {
        VStack(alignment: .leading, spacing: 8) {
            FieldLabel("Repositories", optional: true)
            WrappingRow(spacing: 6, lineSpacing: 6) {
                CheckChip(title: "Let Jojo decide", isOn: state.repository == nil) { actions.chooseRepository(nil) }
                ForEach(state.projectRepositories) { repository in
                    CheckChip(title: URL(fileURLWithPath: repository.path).lastPathComponent, isOn: state.repository == repository.id) {
                        actions.chooseRepository(state.repository == repository.id ? nil : repository.id)
                    }
                    .help(repository.path)
                }
            }
            if state.repository == nil, state.chosenProject != nil, let reason = state.autoBlocked {
                Text(reason).textStyle(.secondary).foregroundStyle(theme.orange).fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private var nextSteps: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("What happens next").textStyle(.bodyStrong).foregroundStyle(theme.text)
            ForEach(Array(["Jojo reads the selected repositories, read-only.",
                           "It asks clarifying questions in Feature Chat.",
                           "It drafts a PRD for your approval. Nothing is executed before you approve a plan."].enumerated()),
                    id: \.offset) { index, line in
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Text("\(index + 1)").font(.system(size: 13, weight: .bold, design: .monospaced)).foregroundStyle(theme.accent)
                    Text(line).font(.system(size: 13)).lineSpacing(3).foregroundStyle(theme.text)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .padding(EdgeInsets(top: 14, leading: 16, bottom: 14, trailing: 16))
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassPanel(radius: 16)
    }

    private var footer: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                Button(action: actions.cancel) {
                    HStack(spacing: 6) { Text("Cancel"); Text("esc").textStyle(.secondary).foregroundStyle(theme.text2) }
                        .padding(.horizontal, 2)
                }
                .buttonStyle(.jojo(.secondary))
                .keyboardShortcut(shown ? KeyboardShortcut.cancelAction : nil)
                Spacer(minLength: 8)
                if let reason = state.sendBlocked { DisabledReason(reason).lineLimit(2) }
                Button(action: actions.start) {
                    HStack(spacing: 6) { Text("Start Discovery"); ShortcutHint("⌘↵") }.padding(.horizontal, 6)
                }
                .buttonStyle(.jojo(.primary))
                .keyboardShortcut(shown ? KeyboardShortcut(.return, modifiers: .command) : nil)
                .disabled(state.sendBlocked != nil)
            }
            outcome
        }
        .padding(.top, 6)
    }

    @ViewBuilder private var outcome: some View {
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
}

// MARK: - Components

/// A form label (`label` style, `text3`), with "· optional" in normal case.
struct FieldLabel: View {
    @Environment(\.theme) private var theme
    let text: String
    var optional = false

    init(_ text: String, optional: Bool = false) {
        (self.text, self.optional) = (text, optional)
    }

    var body: some View {
        HStack(spacing: 4) {
            Text(text).textStyle(.label)
            if optional { Text("· optional").font(.system(size: 11)) }
        }
        .foregroundStyle(theme.text3)
    }
}

/// A capsule chip with a check box (30 high, padding 0 12, 12 medium); on, `accentTint`.
struct CheckChip: View {
    @Environment(\.theme) private var theme
    let title: String
    let isOn: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                RoundedRectangle(cornerRadius: 4)
                    .fill(isOn ? theme.accent : .clear)
                    .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(isOn ? .clear : theme.text3, lineWidth: 1.2))
                    .overlay { if isOn { Image(systemName: "checkmark").font(.system(size: 8, weight: .bold)).foregroundStyle(.white) } }
                    .frame(width: 14, height: 14)
                Text(title).font(.system(size: 12, weight: .medium)).foregroundStyle(theme.text).lineLimit(1)
            }
            .padding(.horizontal, 12)
            .frame(height: 30)
            .background(isOn ? theme.accentTint : theme.grayBg, in: Capsule())
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(isOn ? .isSelected : [])
    }
}

/// A drawn segmented control over values known only at run time (the projects).
struct ChoiceCapsule: View {
    @Environment(\.theme) private var theme
    let options: [(id: String, title: String)]
    let selection: String?
    let label: String
    let choose: (String) -> Void

    var body: some View {
        HStack(spacing: 0) {
            ForEach(options, id: \.id) { option in
                Button { choose(option.id) } label: {
                    Text(option.title).font(.system(size: 12, weight: .medium)).foregroundStyle(theme.text).lineLimit(1)
                        .padding(.horizontal, 13)
                        .frame(height: 28)
                        .background {
                            if option.id == selection {
                                Capsule().fill(theme.cardHover).shadow(color: .black.opacity(0.08), radius: 2, y: 1)
                            }
                        }
                        .contentShape(Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityAddTraits(option.id == selection ? .isSelected : [])
            }
        }
        .padding(2)
        .glassCapsule()
        .fixedSize()
        .accessibilityElement(children: .contain)
        .accessibilityLabel(label)
    }
}

/// A drop zone: radius 14, padding 14 16, a 1.5 dashed `dotOff` outline, a 36 × 36 tile on
/// `accentTint` with an `accent` glyph, a title and `sub` lines, and one trailing control.
struct DropZone<Trailing: View>: View {
    @Environment(\.theme) private var theme
    let symbol: String
    let title: String
    let lines: [String]
    @ViewBuilder let trailing: () -> Trailing

    var body: some View {
        HStack(spacing: 12) {
            RoundedRectangle(cornerRadius: 10).fill(theme.accentTint).frame(width: 36, height: 36)
                .overlay(Image(systemName: symbol).font(.system(size: 15, weight: .medium)).foregroundStyle(theme.accent))
            VStack(alignment: .leading, spacing: 2) {
                Text(title).textStyle(.bodyStrong).foregroundStyle(theme.text)
                ForEach(lines, id: \.self) { Text($0).textStyle(.secondary).foregroundStyle(theme.text2) }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            trailing()
        }
        .padding(EdgeInsets(top: 14, leading: 16, bottom: 14, trailing: 16))
        .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(theme.dotOff, style: StrokeStyle(lineWidth: 1.5, dash: [4, 3])))
    }
}

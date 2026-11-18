import JojoCore
import SwiftUI

// New Project (docs/design/screens/NEW_PROJECT.md), the canvas board `NewProjectMac.dc.html`:
// three steps that cover the window. Sources: a name and the Git repositories in the
// workspace folder (protocol 1.12 `Workspace`), or another folder inside it; designs are not
// read yet. Start Reading registers what is not registered (`RegisterRepository`), saves the
// project record (`SaveProject`) and asks the lead for each repository's project brief, one
// read-only turn at a time (`BriefTurn`, 1.12). Reading shows each brief turn as the core
// reports it (`ProjectDetail` at 1.12). Review shows each brief with its rows and sources;
// each is approved on its own (`ReviewBundle`, `HumanDecision`), and a proposed test command
// is saved to the project only when the human ticks it. Discovery continues in Feature Chat.

/// What New Project shows, as a plain value.
struct NewProjectState: Equatable {
    var step = 0
    var connection: HelperConnection = .notStarted
    var admission = ""
    var queries: [String] = []
    var haves: Set<Have> = [.code]
    var name = ""
    var listing: WorkspaceListing?
    var listingLoad = PlanLoad.idle
    /// The paths of the chosen repositories.
    var chosen: Set<String> = []
    /// The lead's provider: the setup's default.
    var provider = "ClaudeCode"
    /// The project this flow saved, and what the core reports about it.
    var project: String?
    var detail: ProjectDetail?
    var commandPending = false
    /// Why the last step of Start Reading stopped, if it did.
    var problem: String?
    /// Briefs whose proposed test command the human chose to adopt.
    var adoptTest: String?

    var connected: Bool {
        if case .ready = connection { return true }
        return false
    }

    var admissionClosed: Bool { connected && !admission.isEmpty && admission != "Open" }
    var unsupported: Bool { connected && !queries.isEmpty && !queries.contains("Workspace") }

    var entries: [WorkspaceListing.Entry] { listing?.entries ?? [] }
    var chosenEntries: [WorkspaceListing.Entry] { entries.filter { chosen.contains($0.path) } }

    /// The project's repositories as the core reports them, in the order they were chosen.
    var repositories: [ProjectDetail.Repository] { detail?.project == project ? detail?.repositories ?? [] : [] }

    var plan: StartingPlan { .plan(for: haves.union([.code])) }

    /// The name the project gets: what was typed, else the first chosen repository's.
    var projectName: String {
        let typed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return typed.isEmpty ? chosenEntries.first?.name ?? "" : typed
    }

    var startBlocked: String? {
        if !connected { return "Unavailable: the core is not connected." }
        if unsupported { return "This core cannot set up projects. Start a newer helper." }
        if admissionClosed { return "Admission is closed. The core refuses a turn until it is open." }
        if commandPending { return "Unavailable while a command awaits the core." }
        if listing?.problem != nil { return "The workspace folder cannot be used." }
        if chosen.isEmpty { return "Choose at least one repository." }
        if projectName.isEmpty { return "Name the project." }
        return nil
    }

    /// Each repository's reading: done when its newest brief turn replied.
    var readCount: Int { repositories.filter { $0.briefTurn?.state == "Replied" }.count }
    var reading: Bool { repositories.contains { $0.briefTurn?.running == true } }

    /// The briefs that exist, and how many of them are approved.
    var briefs: [(ProjectDetail.Repository, ProjectBrief)] { repositories.compactMap { r in r.brief.map { (r, $0) } } }
    var approvedCount: Int { briefs.filter { $0.1.approved }.count }

    var footNote: String {
        switch step {
        case 0: return "Starting at Discovery · read-only next"
        case 1: return "Read-only · you can leave this screen"
        default:
            return !briefs.isEmpty && approvedCount == briefs.count ? "All approved · Discovery can start" : "Unapproved briefs stay Proposed"
        }
    }

    /// "You said you have code and designs."
    var subtitle: String {
        let names = Have.allCases.filter(haves.contains).map { $0.title.lowercased() }
        let said = names.isEmpty ? "" : "You said you have \(ListFormatter.localizedString(byJoining: names)). "
        return said + "Confirm where they are. Everything Jojo derives starts as Proposed, nothing is binding until you approve it."
    }
}

struct NewProjectActions {
    var editName: (String) -> Void = { _ in }
    var toggle: (String) -> Void = { _ in }
    var chooseFolder: (URL) -> Void = { _ in }
    var start: () -> Void = {}
    var retry: (String) -> Void = { _ in }
    var approve: (String) -> Void = { _ in }
    var adopt: (String?) -> Void = { _ in }
    var go: (Int) -> Void = { _ in }
    var cancel: () -> Void = {}
    var background: () -> Void = {}
    var discovery: () -> Void = {}
    var toggleAppearance: () -> Void = {}
}

struct NewProjectView: View {
    @Environment(\.theme) private var theme
    let state: NewProjectState
    var actions = NewProjectActions()
    @State private var choosingFolder = false

    var body: some View {
        VStack(spacing: 0) {
            header
            VStack(alignment: .leading, spacing: 0) {
                Scrolling {
                    Group {
                        switch state.step {
                        case 0: sourcesStep
                        case 1: readingStep
                        default: reviewStep
                        }
                    }
                    .padding(.trailing, 8)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                footer
            }
            .padding(EdgeInsets(top: 26, leading: 28, bottom: 16, trailing: 28))
        }
        .padding(8)
        .ignoresSafeArea(.container, edges: .top)
        .background(WindowMaterial().ignoresSafeArea())
        .foregroundStyle(theme.text)
        .fileImporter(isPresented: $choosingFolder, allowedContentTypes: [.folder]) { result in
            if case .success(let url) = result { actions.chooseFolder(url) }
        }
    }

    private var header: some View {
        HStack(spacing: 14) {
            // The system traffic lights sit here.
            Spacer().frame(width: 64)
            Text("New Project").font(.system(size: 13, weight: .bold))
            HStack(spacing: 10) {
                ForEach(Array(["Sources", "Reading", "Review"].enumerated()), id: \.offset) { index, title in
                    HStack(spacing: 8) {
                        StepNumber(number: index + 1, current: index == state.step, done: index < state.step)
                        Text(title).font(.system(size: 13, weight: index == state.step ? .semibold : .medium))
                            .foregroundStyle(index == state.step ? theme.text : index < state.step ? theme.text2 : theme.text3)
                    }
                    if index < 2 { RoundedRectangle(cornerRadius: 1).fill(theme.dotOff).frame(width: 28, height: 2) }
                }
            }
            .frame(maxWidth: .infinity)
            Button(action: actions.toggleAppearance) {
                Image(systemName: theme.isDark ? "sun.max" : "moon").font(.system(size: 13, weight: .medium))
            }
            .buttonStyle(IconButtonStyle())
        }
        .padding(EdgeInsets(top: 10, leading: 14, bottom: 10, trailing: 14))
        .frame(height: 52)
        .glassPanel()
    }

    // MARK: Sources

    private var sourcesStep: some View {
        VStack(alignment: .leading, spacing: 22) {
            VStack(alignment: .leading, spacing: 6) {
                Text("Point Jojo at your sources").font(.system(size: 28, weight: .bold)).tracking(-0.84)
                Text(state.subtitle).font(.system(size: 14)).lineSpacing(4).foregroundStyle(theme.text2)
                    .frame(maxWidth: 640, alignment: .leading).fixedSize(horizontal: false, vertical: true)
            }
            banners
            HStack(alignment: .top, spacing: 22) {
                VStack(alignment: .leading, spacing: 20) {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Project name").textStyle(.label).foregroundStyle(theme.text3)
                        InputField(prompt: state.chosenEntries.first?.name ?? "Crew",
                                   text: Binding(get: { state.name }, set: actions.editName))
                            .frame(maxWidth: 360)
                    }
                    repositoriesSection
                    designsSection
                }
                .frame(maxWidth: .infinity, alignment: .topLeading)
                planAside.frame(width: 300)
            }
        }
    }

    @ViewBuilder private var banners: some View {
        if !state.connected {
            Banner(.warning, title: "Jojo core isn't responding", message: "Nothing can be set up until the core is connected again.")
        } else if state.unsupported {
            Banner(.warning, title: "This core cannot set up projects",
                   message: "It does not list the Workspace query in its Hello answer. Start a newer helper.")
        } else if let problem = state.listing?.problem {
            Banner(.warning, title: "The workspace folder cannot be used", message: problem)
        }
    }

    private var repositoriesSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline) {
                Text("Code · repositories in your workspace").textStyle(.label).foregroundStyle(theme.text3)
                Spacer()
                if let root = state.listing?.root {
                    Text("Found in \(OnboardingView.tilde(root))").textStyle(.secondary).foregroundStyle(theme.text2)
                }
            }
            VStack(spacing: 0) {
                if state.entries.isEmpty {
                    Text(emptyRepositories).textStyle(.secondary).foregroundStyle(theme.text2)
                        .padding(EdgeInsets(top: 14, leading: 14, bottom: 14, trailing: 14))
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .fixedSize(horizontal: false, vertical: true)
                }
                ForEach(Array(state.entries.enumerated()), id: \.element.id) { index, entry in
                    repositoryRow(entry).overlay(alignment: .top) { if index > 0 { Rectangle().fill(theme.sep).frame(height: 0.5) } }
                }
            }
            .cardElevation(radius: 14)
            HStack(spacing: 6) {
                Button("Choose Another Folder…") { choosingFolder = true }.buttonStyle(.jojo(.secondary, compact: true))
                    .disabled(!state.connected || state.commandPending)
                Button("Clone from GitHub…") {}.buttonStyle(.jojo(.secondary, compact: true)).disabled(true)
                    .help("Not available yet: clone the repository into the workspace folder yourself.")
            }
        }
    }

    private var emptyRepositories: String {
        switch state.listingLoad {
        case .loading, .idle: return "Reading the workspace folder…"
        case .failed(let reason): return "The core did not list the workspace folder (\(reason))."
        case .loaded:
            let root = state.listing?.root.map(OnboardingView.tilde) ?? "the workspace folder"
            return "No Git repository in \(root) yet. Clone or move one there, then come back."
        }
    }

    private func repositoryRow(_ entry: WorkspaceListing.Entry) -> some View {
        let on = state.chosen.contains(entry.path)
        return Button { actions.toggle(entry.path) } label: {
            HStack(spacing: 12) {
                CheckBox(isOn: on)
                RoundedRectangle(cornerRadius: 8).fill(Palette.gray.color).frame(width: 28, height: 28)
                    .overlay(Image(systemName: "folder").font(.system(size: 12, weight: .semibold)).foregroundStyle(.white))
                VStack(alignment: .leading, spacing: 0) {
                    Text(entry.name).font(.system(size: 13, weight: .semibold)).foregroundStyle(theme.text)
                    Text(entry.stack.isEmpty ? "No manifest found" : entry.stack.joined(separator: " · "))
                        .textStyle(.secondary).foregroundStyle(theme.text2)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                if entry.repository != nil { OperationalPill("Registered", tone: .neutral) }
                Text(OnboardingView.tilde(entry.path)).textStyle(.mono).foregroundStyle(theme.text3).lineLimit(1)
            }
            .padding(EdgeInsets(top: 10, leading: 14, bottom: 10, trailing: 14))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(on ? .isSelected : [])
    }

    private var designsSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Designs").textStyle(.label).foregroundStyle(theme.text3)
            ChoiceCapsule(options: [("figma", "Figma link"), ("files", "Screenshots"), ("canvas", "Design canvas")],
                          selection: nil, label: "Design source", choose: { _ in })
                .disabled(true)
            Text("Not available yet: Jojo can't read designs. Describe the screens in Feature Chat; anything it can't place becomes a question.")
                .font(.system(size: 12)).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
        }
    }

    private var planAside: some View {
        let plan = state.plan
        return VStack(alignment: .leading, spacing: 14) {
            Text("Jojo's starting plan").textStyle(.label).foregroundStyle(theme.text3)
            Text(plan.title).font(.system(size: 17, weight: .bold)).tracking(-0.17)
            Text(plan.why).font(.system(size: 13)).lineSpacing(3).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
            PipelinePills(start: plan.start)
            VStack(alignment: .leading, spacing: 6) {
                Text("Next screen, read-only").font(.system(size: 12, weight: .semibold))
                ForEach(Array(["Read each repository and write its Project brief", "Propose the command that runs its tests",
                               "Wait for your approval of each brief"].enumerated()), id: \.offset) { index, line in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("\(index + 1)").font(.system(size: 12, weight: .bold, design: .monospaced)).foregroundStyle(theme.accent)
                        Text(line).font(.system(size: 12)).lineSpacing(2).fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
            (Text("Runs on ") + Text(providerName(state.provider)).bold() + Text(" in read-only mode. No files are written, no commands with side effects."))
                .font(.system(size: 12)).lineSpacing(3)
                .padding(EdgeInsets(top: 10, leading: 12, bottom: 10, trailing: 12))
                .frame(maxWidth: .infinity, alignment: .leading)
                .cardElevation(radius: 12)
        }
        .padding(18)
        .frame(maxWidth: .infinity, alignment: .topLeading)
        .glassPanel(radius: 18)
    }

    // MARK: Reading

    private var readingStep: some View {
        VStack(alignment: .leading, spacing: 22) {
            VStack(spacing: 6) {
                RoundedRectangle(cornerRadius: 16).fill(theme.accentTint).frame(width: 56, height: 56)
                    .overlay(Image(systemName: "doc.text.magnifyingglass").font(.system(size: 22, weight: .medium)).foregroundStyle(theme.accent))
                    .padding(.bottom, 6)
                Text("Jojo is reading what you brought").font(.system(size: 28, weight: .bold)).tracking(-0.84)
                Text("Read-only, on \(providerName(state.provider)). Results arrive as proposals you review next. You don't need to watch this.")
                    .font(.system(size: 14)).lineSpacing(4).foregroundStyle(theme.text2).multilineTextAlignment(.center)
                    .frame(maxWidth: 540).fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity)
            VStack(alignment: .leading, spacing: 6) {
                HStack {
                    Text("\(state.readCount) of \(max(state.repositories.count, state.chosen.count)) read")
                    Spacer()
                    Text("Progress within a turn is not reported")
                }
                .font(.system(size: 12)).foregroundStyle(theme.text2)
                GeometryReader { geometry in
                    let total = max(state.repositories.count, state.chosen.count, 1)
                    Capsule().fill(theme.grayBg)
                        .overlay(alignment: .leading) {
                            Capsule().fill(theme.accent).frame(width: geometry.size.width * CGFloat(state.readCount) / CGFloat(total))
                        }
                }
                .frame(height: 6)
            }
            VStack(spacing: 0) {
                if state.repositories.isEmpty {
                    Text(state.problem ?? "Setting up the project…").textStyle(.secondary)
                        .foregroundStyle(state.problem == nil ? theme.text2 : theme.orange)
                        .padding(EdgeInsets(top: 12, leading: 16, bottom: 12, trailing: 16))
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                ForEach(Array(state.repositories.enumerated()), id: \.element.id) { index, repository in
                    taskRow(repository).overlay(alignment: .top) { if index > 0 { Rectangle().fill(theme.sep).frame(height: 0.5) } }
                }
            }
            .cardElevation(radius: 14)
            HStack(spacing: 12) {
                Image(systemName: "bell").font(.system(size: 14, weight: .medium)).foregroundStyle(theme.accent)
                Text("Leave this screen anytime. The briefs wait in the Approval Inbox as Proposed.")
                    .font(.system(size: 13)).lineSpacing(3).frame(maxWidth: .infinity, alignment: .leading)
                Button("Continue in Background", action: actions.background).buttonStyle(.jojo(.secondary, compact: true))
            }
            .padding(EdgeInsets(top: 12, leading: 14, bottom: 12, trailing: 14))
            .glassPanel(radius: 16)
        }
        .frame(maxWidth: 760)
        .frame(maxWidth: .infinity)
    }

    private func taskRow(_ repository: ProjectDetail.Repository) -> some View {
        let turn = repository.briefTurn
        let (word, color): (String, Color) = {
            switch turn?.state {
            case "Replied"?: return ("Done", theme.text2)
            case "Running"?, "Stopping"?: return ("Running", theme.accent)
            case "Failed"?, "Interrupted"?: return ("Failed", theme.orange)
            case "Cancelled"?: return ("Cancelled", theme.text2)
            default: return ("Waiting", theme.text3)
            }
        }()
        return HStack(spacing: 12) {
            Group {
                if turn?.running == true {
                    ProgressView().controlSize(.small)
                } else {
                    Circle().fill(turn?.state == "Replied" ? Palette.green.color : turn == nil ? theme.dotOff : Palette.orange.color)
                        .overlay {
                            Image(systemName: turn?.state == "Replied" ? "checkmark" : turn == nil ? "clock" : "exclamationmark")
                                .font(.system(size: 9, weight: .bold)).foregroundStyle(.white)
                        }
                }
            }
            .frame(width: 20, height: 20)
            VStack(alignment: .leading, spacing: 1) {
                Text("Reading \(repository.name)").font(.system(size: 13, weight: .semibold))
                Text(turn?.detail ?? "Architecture, commands, conventions and what to watch out for")
                    .textStyle(.secondary).foregroundStyle(theme.text2).lineLimit(2)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if turn.map({ ["Failed", "Interrupted", "Cancelled"].contains($0.state) }) == true {
                Button("Read Again") { actions.retry(repository.id) }.buttonStyle(.jojo(.secondary, compact: true))
                    .disabled(state.reading || state.commandPending)
            }
            Text(word).font(.system(size: 12, weight: word == "Running" ? .semibold : .regular)).foregroundStyle(color)
        }
        .padding(EdgeInsets(top: 12, leading: 16, bottom: 12, trailing: 16))
        .background(turn?.running == true ? theme.accentTint : .clear)
    }

    // MARK: Review

    private var reviewStep: some View {
        VStack(alignment: .leading, spacing: 20) {
            HStack(alignment: .bottom, spacing: 16) {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Review what Jojo found").font(.system(size: 28, weight: .bold)).tracking(-0.84)
                    Text("Approve each proposal on its own. Anything you leave stays Proposed in your Approval Inbox.")
                        .font(.system(size: 14)).lineSpacing(4).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                OperationalPill("\(state.approvedCount) of \(state.briefs.count) approved", tone: .info)
            }
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 380), spacing: 16, alignment: .top)], alignment: .leading, spacing: 16) {
                ForEach(state.briefs, id: \.1.document) { repository, brief in
                    briefCard(repository, brief)
                }
                designsCard
            }
            HStack(spacing: 12) {
                RoundedRectangle(cornerRadius: 8).fill(Palette.orange.color).frame(width: 30, height: 30)
                    .overlay(Image(systemName: "bubble.left.and.bubble.right").font(.system(size: 12, weight: .semibold)).foregroundStyle(.white))
                VStack(alignment: .leading, spacing: 1) {
                    Text("Discovery continues in Feature Chat").font(.system(size: 13, weight: .semibold))
                    Text("Describe the first feature there. Every approved brief goes into the lead's prompt.")
                        .textStyle(.secondary).foregroundStyle(theme.text2)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                Button("Answer in Feature Chat", action: actions.discovery).buttonStyle(.jojo(.secondary, compact: true))
            }
            .padding(EdgeInsets(top: 14, leading: 16, bottom: 14, trailing: 16))
            .glassPanel(radius: 16)
        }
    }

    private func briefCard(_ repository: ProjectDetail.Repository, _ brief: ProjectBrief) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .top, spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Project brief").font(.system(size: 15, weight: .bold))
                    Text("From \(repository.name) · read-only analysis · rev \(brief.revision)").textStyle(.secondary).foregroundStyle(theme.text2)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                OperationalPill(brief.approved ? "Approved" : "Proposed", tone: brief.approved ? .ok : .info)
            }
            .padding(EdgeInsets(top: 14, leading: 16, bottom: 14, trailing: 16))
            .overlay(alignment: .bottom) { Rectangle().fill(theme.sep).frame(height: 0.5) }
            ForEach(Array(brief.rows.enumerated()), id: \.offset) { index, row in
                HStack(alignment: .firstTextBaseline, spacing: 14) {
                    Text(row.key).font(.system(size: 13, weight: .semibold)).foregroundStyle(theme.text2).frame(width: 90, alignment: .leading)
                    VStack(alignment: .leading, spacing: 4) {
                        Text(row.text).font(.system(size: 13)).lineSpacing(3).fixedSize(horizontal: false, vertical: true)
                        if !row.sources.isEmpty || row.warning {
                            Text(row.sources.isEmpty ? "No source named" : row.sources.joined(separator: " · "))
                                .textStyle(.mono).foregroundStyle(row.warning ? theme.orange : theme.text3)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .padding(EdgeInsets(top: 10, leading: 16, bottom: 10, trailing: 16))
                .overlay(alignment: .top) { if index > 0 { Rectangle().fill(theme.sep).frame(height: 0.5) } }
            }
            if let command = brief.testCommand {
                Button { actions.adopt(state.adoptTest == brief.document ? nil : brief.document) } label: {
                    HStack(alignment: .top, spacing: 10) {
                        CheckBox(isOn: state.adoptTest == brief.document)
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Use as the project's test command").font(.system(size: 12, weight: .semibold)).foregroundStyle(theme.text)
                            Text(command.joined(separator: " ")).textStyle(.mono).foregroundStyle(theme.text2).lineLimit(2)
                        }
                        Spacer(minLength: 0)
                    }
                    .padding(EdgeInsets(top: 10, leading: 16, bottom: 10, trailing: 16))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .disabled(brief.approved)
                .overlay(alignment: .top) { Rectangle().fill(theme.sep).frame(height: 0.5) }
            }
            HStack(spacing: 8) {
                Text("Every line names the files it came from.").textStyle(.secondary).foregroundStyle(theme.text2)
                    .frame(maxWidth: .infinity, alignment: .leading)
                Button("Read Again") { actions.retry(repository.id) }.buttonStyle(.jojo(.secondary, compact: true))
                    .disabled(state.reading || state.commandPending)
                Button(brief.approved ? "Approved" : "Approve Brief") { actions.approve(brief.document) }
                    .buttonStyle(.jojo(brief.approved ? .secondary : .primary, compact: true))
                    .disabled(brief.approved || state.commandPending)
            }
            .padding(EdgeInsets(top: 12, leading: 16, bottom: 12, trailing: 16))
            .overlay(alignment: .top) { Rectangle().fill(theme.sep).frame(height: 0.5) }
        }
        .cardElevation(radius: 16)
    }

    private var designsCard: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .top) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("PRD draft from designs").font(.system(size: 15, weight: .bold))
                    Text("Requirements linked to screens").textStyle(.secondary).foregroundStyle(theme.text2)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                OperationalPill("Not available yet", tone: .neutral)
            }
            Text("Jojo can't read designs yet. The PRD is drafted in Feature Chat from what you describe.")
                .font(.system(size: 13)).lineSpacing(3).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
        }
        .padding(EdgeInsets(top: 14, leading: 16, bottom: 14, trailing: 16))
        .cardElevation(radius: 16)
    }

    // MARK: Footer

    private var footer: some View {
        HStack(spacing: 10) {
            Button("Cancel", action: actions.cancel).buttonStyle(.plain).font(.system(size: 13, weight: .medium))
                .foregroundStyle(theme.text2).padding(.horizontal, 12)
            Text(state.footNote).textStyle(.secondary).foregroundStyle(theme.text2).frame(maxWidth: .infinity)
            if state.step == 0, let reason = state.startBlocked { DisabledReason(reason).lineLimit(1) }
            if state.step > 0 {
                Button("Back") { actions.go(state.step - 1) }.buttonStyle(.jojo(.secondary))
            }
            switch state.step {
            case 0:
                Button("Start Reading", action: actions.start).buttonStyle(.jojo(.primary)).disabled(state.startBlocked != nil)
            case 1:
                Button("Review Results") { actions.go(2) }.buttonStyle(.jojo(.primary)).disabled(state.briefs.isEmpty)
            default:
                Button("Open Discovery", action: actions.discovery).buttonStyle(.jojo(.primary))
            }
        }
        .padding(.top, 18)
        .overlay(alignment: .top) { Rectangle().fill(theme.sep).frame(height: 0.5) }
    }
}

/// New Project wired to the app model. Start Reading runs as a chain of commands, each sent
/// only after the previous one was answered: registrations, the project record, then one
/// brief turn per repository, the next once the core reports the previous one ended.
struct NewProjectScreen: View {
    @ObservedObject var model: AppModel
    let haves: Set<Have>
    var close: () -> Void = {}
    var openDiscovery: (String?) -> Void = { _ in }
    @State private var step = 0
    @State private var name = ""
    @State private var chosen: Set<String> = []
    @State private var project: String?
    @State private var problem: String?
    @State private var adoptTest: String?
    /// Repositories whose brief this screen asked for, in order.
    @State private var asked: [String] = []

    private var provider: String {
        let setup = model.setup
        guard let setup else { return "ClaudeCode" }
        return setup.allows(setup.defaultProvider) ? setup.defaultProvider : (setup.claudeCode ? "ClaudeCode" : "Codex")
    }

    var state: NewProjectState {
        let workspace = model.workspace
        return NewProjectState(step: step, connection: model.client.connection, admission: workspace.admission,
                               queries: model.client.hello?.queries ?? [], haves: haves, name: name,
                               listing: workspace.workspaceListing, listingLoad: workspace.workspaceListingLoad,
                               chosen: chosen, provider: provider, project: project, detail: workspace.projectDetail,
                               commandPending: workspace.commandPending, problem: problem, adoptTest: adoptTest)
    }

    var body: some View {
        NewProjectView(state: state, actions: actions)
            .onAppear {
                model.workspace.projectsShown = true
                model.workspace.loadWorkspaceListing()
                model.workspace.loadProjects()
            }
            .onChange(of: model.client.connection) { model.workspace.loadWorkspaceListing() }
            .task(id: step) {
                // While the briefs are read, the project's repositories are read every two
                // seconds; the next brief is asked for when the previous turn ended.
                while step == 1, !Task.isCancelled {
                    if let project { model.workspace.loadProjectDetail(project) }
                    advance()
                    try? await Task.sleep(nanoseconds: 2_000_000_000)
                }
            }
    }

    private var actions: NewProjectActions {
        var a = NewProjectActions()
        a.editName = { name = $0 }
        a.toggle = { path in
            if chosen.contains(path) { chosen.remove(path) } else { chosen.insert(path) }
        }
        a.chooseFolder = { url in
            model.workspace.registerRepository(url.path) { id in
                if id != nil { chosen.insert(url.resolvingSymlinksInPath().path) }
            }
        }
        a.start = start
        a.retry = { repository in
            asked.removeAll { $0 == repository }
            model.workspace.requestBrief(repository: repository, provider: provider, message: message)
            asked.append(repository)
            step = 1
        }
        a.approve = { document in
            let adopt = adoptTest == document
            model.workspace.approveDocument(document) { done in
                guard done, adopt else { return }
                saveTestCommand(from: document)
            }
        }
        a.adopt = { adoptTest = $0 }
        a.go = { target in
            if target == 2, let project { model.workspace.loadProjectDetail(project) }
            step = max(0, min(2, target))
        }
        a.cancel = close
        a.background = close
        a.discovery = { openDiscovery(project) }
        a.toggleAppearance = { model.setAppearance(model.appearance == .dark ? .light : .dark) }
        return a
    }

    /// What the human brings, as the brief turn's message.
    private var message: String {
        let names = Have.allCases.filter(haves.contains).map { $0.title.lowercased() }
        return names.isEmpty ? "" : "The product owner brings: \(names.joined(separator: ", "))."
    }

    /// Registers what is not registered, then saves the project, then reads.
    private func start() {
        problem = nil
        let entries = state.chosenEntries
        var ids: [String] = []
        func register(_ index: Int) {
            guard index < entries.count else { return save(ids) }
            if let id = entries[index].repository {
                ids.append(id)
                return register(index + 1)
            }
            model.workspace.registerRepository(entries[index].path) { id in
                guard let id else {
                    problem = "\(entries[index].name) could not be registered; see the status line."
                    return
                }
                ids.append(id)
                register(index + 1)
            }
        }
        step = 1
        register(0)
    }

    private func save(_ repositories: [String]) {
        let id = ProjectsView.newIdentity()
        let record = ProjectRecord(id: id, revision: 0, name: state.projectName, repositories: repositories,
                                   defaultProvider: provider, capacity: model.workspace.projects?.capacity ?? 2)
        model.workspace.saveProject(record, asNew: true)
        project = id
        asked = []
    }

    /// The next repository whose brief was not asked for, once no brief turn runs.
    private func advance() {
        guard let project, !model.workspace.commandPending else { return }
        let detail = model.workspace.projectDetail
        guard detail?.project == project, let repositories = detail?.repositories else { return }
        if repositories.contains(where: { $0.briefTurn?.running == true }) { return }
        guard let next = repositories.first(where: { !asked.contains($0.id) }) else { return }
        asked.append(next.id)
        model.workspace.requestBrief(repository: next.id, provider: provider, message: message)
    }

    private func saveTestCommand(from document: String) {
        guard let project, let record = model.workspace.projects?.projects.first(where: { $0.id == project }),
              record.testCommand.isEmpty,
              let command = state.briefs.first(where: { $0.1.document == document })?.1.testCommand else { return }
        let next = ProjectRecord(id: record.id, revision: record.revision, name: record.name, repositories: record.repositories,
                                 instructions: record.instructions, testCommand: command, ciCommand: record.ciCommand,
                                 kinds: record.kinds, defaultProvider: record.defaultProvider, capacity: record.capacity,
                                 repairLimit: record.repairLimit, review: record.review)
        model.workspace.saveProject(next, asNew: false)
    }
}

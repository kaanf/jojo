import JojoCore
import SwiftUI

// Projects (docs/design/screens/PROJECTS.md), the M3 minimum of the canvas board: the
// project records the core holds and a form that saves one whole. The records are read
// with `Projects` while the screen is the shown one; repositories come from the chat
// index (the registered ones). Health, worktrees, features and missing configuration of
// the board are not reported by the core and are not drawn.

/// What the human typed for a project, before it is a record.
struct ProjectDraft: Equatable {
    var id = ""
    var revision: Int64 = 0
    var name = ""
    var repositories: [String] = []
    var instructions = ""
    var testCommand = ""
    var ciCommand = ""
    /// "kind=Provider" pairs separated by commas, for example "backend=ClaudeCode, docs=Codex".
    var kinds = ""
    var defaultProvider = "ClaudeCode"
    var capacity = "2"

    static let providers = ["ClaudeCode", "Codex"]

    init() {}

    init(_ record: ProjectRecord) {
        (id, revision, name, repositories) = (record.id, record.revision, record.name, record.repositories)
        (instructions, defaultProvider, capacity) = (record.instructions, record.defaultProvider, "\(record.capacity)")
        testCommand = record.testCommand.joined(separator: " ")
        ciCommand = record.ciCommand.joined(separator: " ")
        kinds = record.kinds.map { "\($0.kind)=\($0.provider)" }.joined(separator: ", ")
    }

    var isNew: Bool { revision == 0 }

    /// The parsed task kinds, or nil when one pair is not "kind=Provider".
    var kindRules: [ProjectRecord.KindRule]? {
        let pairs = kinds.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        let rules = pairs.compactMap { pair -> ProjectRecord.KindRule? in
            let parts = pair.split(separator: "=").map { $0.trimmingCharacters(in: .whitespaces) }
            guard parts.count == 2, Self.providers.contains(parts[1]), !parts[0].isEmpty else { return nil }
            return ProjectRecord.KindRule(kind: parts[0], provider: parts[1])
        }
        return rules.count == pairs.count ? rules : nil
    }

    /// Why the draft cannot be saved, or nil. The core checks the rest.
    var problem: String? {
        if !name.contains(where: \.isLetter) && !name.contains(where: \.isNumber) { return "Name the project." }
        if repositories.isEmpty { return "Choose at least one registered repository." }
        if kindRules == nil { return "Write task kinds as kind=ClaudeCode or kind=Codex, separated by commas." }
        guard let n = Int64(capacity), (1...8).contains(n) else { return "Capacity is a number from 1 to 8." }
        return nil
    }

    func record() -> ProjectRecord? {
        guard problem == nil, let rules = kindRules, let n = Int64(capacity) else { return nil }
        let words = { (s: String) in s.split(separator: " ").map(String.init) }
        return ProjectRecord(id: id, revision: revision, name: name, repositories: repositories, instructions: instructions,
                             testCommand: words(testCommand), ciCommand: words(ciCommand), kinds: rules,
                             defaultProvider: defaultProvider, capacity: n)
    }
}

struct ProjectsState: Equatable {
    var connection: HelperConnection
    var list: ProjectList?
    var load = PlanLoad.idle
    var repositories: [ChatIndex.Repository] = []
    var chosen: String?
    var draft: ProjectDraft?
    var commandPending = false

    var project: ProjectRecord? { list?.projects.first { $0.id == chosen } ?? list?.projects.first }

    var saveBlocked: String? {
        guard case .ready = connection else { return "Unavailable: the core is not connected." }
        if commandPending { return "Unavailable while a command awaits the core." }
        return draft?.problem
    }

    static func provider(_ word: String) -> String { word == "ClaudeCode" ? "Claude Code" : word }

    static func loadFailure(_ reason: String) -> String {
        if reason.hasPrefix("UnknownType") { return "This core has no project records." }
        if reason == "malformed projects" { return "The core's answer was not a list this app can read." }
        return "The core did not answer the read."
    }

    /// What the board draws and the core does not report.
    static let missing = [
        ("Repository health", "Not reported: branch, worktrees and issues of a repository."),
        ("Features", "Not reported per project. Feature Chat lists the features of a registered repository."),
        ("Missing configuration", "Not reported."),
        ("Open in Finder, Import Local Project", "Not built. A repository is registered from Feature Chat."),
    ]
}

struct ProjectsActions {
    var choose: (String) -> Void = { _ in }
    var edit: (ProjectDraft?) -> Void = { _ in }
    var save: (ProjectDraft) -> Void = { _ in }
    var reload: () -> Void = {}
}

struct ProjectsScreen: View {
    @ObservedObject var model: AppModel
    /// The records are read only while this destination is the shown one.
    let visible: Bool
    @State private var chosen: String?
    @State private var draft: ProjectDraft?

    var body: some View {
        let workspace = model.workspace
        let state = ProjectsState(connection: model.client.connection, list: workspace.projects, load: workspace.projectsLoad,
                                  repositories: workspace.chatIndex?.repositories ?? [], chosen: chosen, draft: draft,
                                  commandPending: workspace.commandPending)
        ProjectsView(state: state, actions: ProjectsActions(
            choose: { (chosen, draft) = ($0, nil) },
            edit: { draft = $0 },
            save: { typed in
                guard let record = typed.record() else { return }
                workspace.saveProject(record, asNew: typed.isNew)
                (chosen, draft) = (record.id, nil)
            },
            reload: { workspace.loadProjects() }))
            .onChange(of: visible) {
                if visible {
                    workspace.loadProjects()
                    workspace.loadChatIndex()
                }
            }
            .onChange(of: workspace.snapshotSeq) { if visible { workspace.loadProjects() } }
    }
}

struct ProjectsView: View {
    @Environment(\.theme) private var theme
    let state: ProjectsState
    var actions = ProjectsActions()

    var body: some View {
        Scrolling {
            VStack(alignment: .leading, spacing: 18) {
                if case .ready = state.connection {} else {
                    Banner(.danger, title: "Jojo core isn't responding", message: "Project records cannot be read or saved.")
                }
                list
                if let draft = state.draft {
                    form(draft)
                } else if let project = state.project {
                    detail(project)
                }
                missing
            }
            .frame(maxWidth: 760, alignment: .topLeading)
            .padding(EdgeInsets(top: 18, leading: 22, bottom: 24, trailing: 22))
            .frame(maxWidth: .infinity, alignment: .topLeading)
        }
    }

    @ViewBuilder
    private var list: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Projects") {
                Button("Create Project…") {
                    var fresh = ProjectDraft()
                    fresh.id = ProjectsView.newIdentity()
                    actions.edit(fresh)
                }
                .buttonStyle(.jojo(.secondary, compact: true))
                .disabled(state.draft != nil)
            }
            switch state.load {
            case .idle, .loading where state.list == nil:
                LoadingBlock("Reading the project records…").frame(minHeight: 100)
            case .failed(let reason):
                Banner(.warning, title: "The project records could not be read", message: ProjectsState.loadFailure(reason)) {
                    Button("Try Again") { actions.reload() }.buttonStyle(.jojo(.secondary, compact: true))
                }
            default:
                if let list = state.list, !list.projects.isEmpty {
                    Card(.panelList) {
                        Rows(list.projects) { project in
                            Button { actions.choose(project.id) } label: {
                                ListRow(project.name, meta: "Revision \(project.revision)",
                                        detail: "\(project.repositories.count) repositories · default \(ProjectsState.provider(project.defaultProvider))",
                                        leading: { EmptyView() },
                                        trailing: { if project.id == state.project?.id { KindTag("Shown", tone: .info) } })
                                    .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    Footnote("Parallel capacity of the core: \(list.capacity) attempts in all, not split by provider. A lead turn takes one of them (D4).")
                } else {
                    StateBlock(symbol: "folder", title: "No projects yet",
                               message: "A project names its registered repositories, the commands that test them, the instructions every agent session gets, the provider per task kind and the parallel capacity.") {
                        EmptyView()
                    }
                    .frame(minHeight: 140)
                }
            }
        }
    }

    private func detail(_ project: ProjectRecord) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading(project.name) {
                Button("Edit Project") { actions.edit(ProjectDraft(project)) }.buttonStyle(.jojo(.secondary, compact: true))
            }
            Card(.panelList) {
                VStack(spacing: 0) {
                    FactLine("Repositories") {
                        VStack(alignment: .leading, spacing: 3) {
                            ForEach(project.repositories, id: \.self) { repository in
                                identifier(path(repository) ?? repository)
                            }
                        }
                    }
                    FactLine("Test command") { command(project.testCommand) }
                    FactLine("CI command") { command(project.ciCommand) }
                    FactLine("Task kinds", project.kinds.isEmpty ? "None; the lead names the provider" :
                        project.kinds.map { "\($0.kind) → \(ProjectsState.provider($0.provider))" }.joined(separator: ", "))
                    FactLine("Default provider", ProjectsState.provider(project.defaultProvider))
                    FactLine("Parallel capacity", "\(project.capacity) in all", last: true)
                }
            }
            SectionHeading("Project instructions")
            Card(.panelList) {
                Text(project.instructions.isEmpty ? "None" : project.instructions)
                    .textStyle(.body).foregroundStyle(project.instructions.isEmpty ? theme.text3 : theme.text)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(14).frame(maxWidth: .infinity, alignment: .leading)
            }
            Footnote("Given to every lead turn of this project and appended to each ADR a lead drafts, so that it is approved and reaches every attempt. A saved change is a new revision; a plan keeps the revision it was drafted under.")
        }
    }

    private func form(_ draft: ProjectDraft) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading(draft.isNew ? "New project" : "Edit \(draft.name)")
            Card(.panelList) {
                VStack(spacing: 0) {
                    FactLine("Name") { InputField(prompt: "project name", text: field(\.name)) }
                    FactLine("Repositories") {
                        if state.repositories.isEmpty {
                            Text("No registered repository. Register one in Feature Chat.").foregroundStyle(theme.text3)
                        } else {
                            WrappingRow(spacing: 6) {
                                ForEach(state.repositories) { repository in
                                    Button { toggle(repository.id) } label: {
                                        FilterChip(title: repository.path, isOn: draft.repositories.contains(repository.id))
                                    }
                                    .buttonStyle(.plain)
                                }
                            }
                        }
                    }
                    FactLine("Test command") { InputField(prompt: "/usr/bin/make test", text: field(\.testCommand), mono: true) }
                    FactLine("CI command") { InputField(prompt: "stored and shown only", text: field(\.ciCommand), mono: true) }
                    FactLine("Task kinds") { InputField(prompt: "backend=ClaudeCode, docs=Codex", text: field(\.kinds), mono: true) }
                    FactLine("Default provider") {
                        HStack(spacing: 6) {
                            ForEach(ProjectDraft.providers, id: \.self) { provider in
                                Button { set(\.defaultProvider, provider) } label: {
                                    FilterChip(title: ProjectsState.provider(provider), isOn: draft.defaultProvider == provider)
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                    FactLine("Parallel capacity") { InputField(prompt: "2", text: field(\.capacity), mono: true) }
                    FactLine("Instructions", last: true) { InputField(prompt: "given to every agent session", text: field(\.instructions)) }
                }
            }
            HStack(spacing: 10) {
                Button { actions.save(draft) } label: { Label("Save Project", systemImage: "checkmark") }
                    .buttonStyle(.jojo(.primary))
                    .disabled(state.saveBlocked != nil)
                Button("Discard") { actions.edit(nil) }.buttonStyle(.jojo(.secondary))
                if let blocked = state.saveBlocked { DisabledReason(blocked) }
            }
            Footnote("Saving sends the whole record as its next revision. The capacity is one number for the whole core: the last project saved sets it.")
        }
    }

    private var missing: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeading("Not reported")
            Card(.panelList) {
                VStack(spacing: 0) {
                    ForEach(Array(ProjectsState.missing.enumerated()), id: \.offset) { index, line in
                        SettingLine(line.0, line.1, form: .missing, last: index == ProjectsState.missing.count - 1)
                    }
                }
            }
        }
    }

    private func path(_ repository: String) -> String? {
        state.repositories.first { $0.id == repository }?.path
    }

    @ViewBuilder
    private func command(_ argv: [String]) -> some View {
        if argv.isEmpty { Text("None").foregroundStyle(theme.text3) } else { identifier(argv.joined(separator: " ")) }
    }

    private func identifier(_ text: String) -> some View {
        Text(text).font(.system(size: 11.5, design: .monospaced)).textSelection(.enabled)
    }

    private func field(_ path: WritableKeyPath<ProjectDraft, String>) -> Binding<String> {
        Binding(get: { state.draft?[keyPath: path] ?? "" }, set: { set(path, $0) })
    }

    private func set(_ path: WritableKeyPath<ProjectDraft, String>, _ value: String) {
        guard var draft = state.draft else { return }
        draft[keyPath: path] = value
        actions.edit(draft)
    }

    private func toggle(_ repository: String) {
        guard var draft = state.draft else { return }
        if let index = draft.repositories.firstIndex(of: repository) {
            draft.repositories.remove(at: index)
        } else {
            draft.repositories.append(repository)
        }
        actions.edit(draft)
    }

    /// A fresh project identity: 32 lowercase hex characters.
    static func newIdentity() -> String {
        (0..<32).map { _ in String("0123456789abcdef".randomElement()!) }.joined()
    }
}

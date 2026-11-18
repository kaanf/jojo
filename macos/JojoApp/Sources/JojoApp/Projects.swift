import JojoCore
import SwiftUI

// Projects (docs/design/screens/PROJECTS.md), rebuilt from `ProjectsMac.dc.html` (step
// 6): the project records the core holds, each with its repositories as the core reads
// them now (protocol 1.11 `ProjectDetail`: branch, commit, worktrees, health, what is
// missing), its features (1.7) and a form that saves the whole record. Everything is read
// while the screen is the shown one.

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
    /// M4 (D2): automatic repairs of a failed plan step (0 to 2) and the reviewer.
    var repairLimit: Int64 = 2
    var review = true

    static let providers = ["ClaudeCode", "Codex"]

    init() {}

    init(_ record: ProjectRecord) {
        (id, revision, name, repositories) = (record.id, record.revision, record.name, record.repositories)
        (instructions, defaultProvider, capacity) = (record.instructions, record.defaultProvider, "\(record.capacity)")
        testCommand = record.testCommand.joined(separator: " ")
        ciCommand = record.ciCommand.joined(separator: " ")
        kinds = record.kinds.map { "\($0.kind)=\($0.provider)" }.joined(separator: ", ")
        (repairLimit, review) = (record.repairLimit, record.review)
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
                             defaultProvider: defaultProvider, capacity: n, repairLimit: repairLimit, review: review)
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
    /// 1.11: the shown project's repositories as the core reads them now.
    var detail: ProjectDetail?
    var detailLoad = PlanLoad.idle
    /// False when the core does not list `ProjectDetail` (older than 1.11).
    var reportsDetail = true
    /// The features the core reports (1.7), for the features of a project.
    var features: [FeatureSummary] = []
    var admission = ""
    /// gh's sign-in as the core last read it (M5): a missing configuration when not ready.
    var gh: ExternalSignIn?
    /// Prior attempts held in the Recovery Center, by project name (1.11).
    var held: [String: Int] = [:]

    var project: ProjectRecord? { list?.projects.first { $0.id == chosen } ?? list?.projects.first }

    /// The detail answers for the shown project only.
    var shownDetail: ProjectDetail? { detail?.project == project?.id ? detail : nil }

    func features(of project: ProjectRecord) -> [FeatureSummary] {
        features.filter { $0.project == project.id || ($0.project == nil && $0.projectName == project.name) }
    }

    /// "2 repos · 3 features".
    func line(_ project: ProjectRecord) -> String {
        let repos = project.repositories.count, count = features(of: project).count
        return "\(repos) \(repos == 1 ? "repo" : "repos") · \(count) \(count == 1 ? "feature" : "features")"
    }

    /// The board's operational pills of the shown project.
    func pills(_ project: ProjectRecord) -> [(String, Tone)] {
        let mine = features(of: project)
        var out: [(String, Tone)] = []
        if case .ready = connection, !admission.isEmpty {
            out.append(admission == "Open" ? ("Admission open", .ok) : ("Admission closed", .danger))
        }
        let running = mine.reduce(0) { $0 + $1.running }, blocked = mine.reduce(0) { $0 + $1.blocked }
        let awaiting = mine.reduce(0) { $0 + $1.awaiting }, held = held[project.name] ?? 0
        if running > 0 { out.append(("\(running) running", .info)) }
        if blocked > 0 { out.append(("\(blocked) blocked", .warn)) }
        if awaiting > 0 { out.append(("\(awaiting) awaiting approval", .review)) }
        if held > 0 { out.append(("\(held) held", .danger)) }
        return out
    }

    /// What the project lacks for the flow: the core's list, and gh's sign-in.
    var missingConfiguration: [String] {
        var out = shownDetail?.missing.map(\.text) ?? []
        if let gh, !gh.ready { out.append("GitHub CLI: \(gh.detail.isEmpty ? gh.state : gh.detail)") }
        return out
    }

    /// A feature's stage line: "Execution · 2 running · 3 blocked".
    static func stageLine(_ f: FeatureSummary) -> (String, String) {
        let index = f.stages.lastIndex { $0.state != "Off" } ?? 0
        let name = FeatureSummary.stageNames[index].capitalized
        var parts = [name]
        if f.running > 0 { parts.append("\(f.running) running") }
        if f.blocked > 0 { parts.append("\(f.blocked) blocked") }
        if f.awaiting > 0 { parts.append("\(f.awaiting) awaiting approval") }
        if f.done { parts.append("completed") }
        return (parts.joined(separator: " · "), f.done ? "Done" : name)
    }

    var saveBlocked: String? {
        guard case .ready = connection else { return "Unavailable: the core is not connected." }
        if commandPending { return "Unavailable while a command awaits the core." }
        return draft?.problem
    }

    static func provider(_ word: String) -> String { word == "ClaudeCode" ? "Claude Code" : word }
    /// M4 (D2): the repair limit in words.
    static func repairs(_ limit: Int64) -> String {
        switch limit {
        case 0: return "Off"
        case 1: return "Once"
        default: return "Up to \(limit) times"
        }
    }

    static func loadFailure(_ reason: String) -> String {
        if reason.hasPrefix("UnknownType") { return "This core has no project records." }
        if reason == "malformed projects" { return "The core's answer was not a list this app can read." }
        return "The core did not answer the read."
    }

    /// What the board draws and the core does not report.
    static let missing = [
        ("Import Local Project, Attach Repository", "Not built. A repository is registered from Feature Chat."),
        ("Instructions revisions", "Every save is a revision of the whole record; there is no separate instructions history."),
    ]
}

struct ProjectsActions {
    var choose: (String) -> Void = { _ in }
    var edit: (ProjectDraft?) -> Void = { _ in }
    var save: (ProjectDraft) -> Void = { _ in }
    var reload: () -> Void = {}
    var newFeature: (String) -> Void = { _ in }
    var openFeature: (String) -> Void = { _ in }
    /// Step 7: New Project, the board's flow (the form stays for editing a project).
    var newProject: () -> Void = {}
}

/// Opens a project's repositories in Finder: the one thing this screen does with a path.
@MainActor func openInFinder(_ paths: [String]) {
    let urls = paths.map { URL(fileURLWithPath: $0) }
    if !urls.isEmpty { NSWorkspace.shared.activateFileViewerSelecting(urls) }
}

struct ProjectsScreen: View {
    @ObservedObject var model: AppModel
    /// The records are read only while this destination is the shown one.
    let visible: Bool
    @Binding var chosen: String?
    @Binding var draft: ProjectDraft?
    var newFeature: (String) -> Void = { _ in }
    var openFeature: (String) -> Void = { _ in }
    var newProject: () -> Void = {}

    var state: ProjectsState {
        let workspace = model.workspace
        var held: [String: Int] = [:]
        for s in workspace.recoveryDetail?.subjects ?? [] { if let name = s.projectName { held[name, default: 0] += 1 } }
        return ProjectsState(connection: model.client.connection, list: workspace.projects, load: workspace.projectsLoad,
                             repositories: workspace.chatIndex?.repositories ?? [], chosen: chosen, draft: draft,
                             commandPending: workspace.commandPending, detail: workspace.projectDetail,
                             detailLoad: workspace.projectDetailLoad,
                             reportsDetail: model.client.hello.map { $0.queries.contains("ProjectDetail") } ?? true,
                             features: workspace.features?.features ?? [], admission: workspace.admission,
                             gh: workspace.external?.gh, held: held)
    }

    var body: some View {
        let workspace = model.workspace
        ProjectsView(state: state, actions: ProjectsActions(
            choose: { (chosen, draft) = ($0, nil) },
            edit: { draft = $0 },
            save: { typed in
                guard let record = typed.record() else { return }
                workspace.saveProject(record, asNew: typed.isNew)
                (chosen, draft) = (record.id, nil)
            },
            reload: { workspace.loadProjects() },
            newFeature: newFeature,
            openFeature: openFeature,
            newProject: newProject))
            .onChange(of: visible) {
                workspace.projectsShown = visible
                if visible { load() }
            }
            .onChange(of: workspace.snapshotSeq) { if visible { load() } }
            .onChange(of: state.project?.id) { if visible { loadDetail() } }
    }

    private func load() {
        let workspace = model.workspace
        workspace.loadProjects()
        workspace.loadChatIndex()
        workspace.loadFeatures()
        workspace.loadExternalActions()
        if model.client.hello?.queries.contains("RecoveryDetail") ?? false { workspace.loadRecoveryDetail() }
        loadDetail()
    }

    private func loadDetail() {
        guard state.reportsDetail, let project = state.project?.id else { return }
        model.workspace.loadProjectDetail(project)
    }
}

struct ProjectsView: View {
    @Environment(\.theme) private var theme
    let state: ProjectsState
    var actions = ProjectsActions()

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            VStack(alignment: .leading, spacing: 10) {
                CoreBanners(connection: state.connection,
                            missing: state.reportsDetail ? nil : ("ProjectDetail", "repository health"), version: "1.11")
            }
            .padding(.horizontal, 10)
            FeatureColumns(leadingWidth: 240, trailingWidth: 300, wide: 1060, medium: 760) {
                list
            } main: {
                VStack(alignment: .leading, spacing: 18) {
                    if let draft = state.draft {
                        form(draft)
                    } else if let project = state.project {
                        detail(project)
                    } else {
                        empty
                    }
                }
            } trailing: {
                if let project = state.project, state.draft == nil { configuration(project) }
            }
        }
        .padding(.top, 6)
    }

    @ViewBuilder
    private var empty: some View {
        switch state.load {
        case .idle, .loading where state.list == nil:
            LoadingBlock("Reading the project records…").frame(minHeight: 160)
        case .failed(let reason):
            Banner(.warning, title: "The project records could not be read", message: ProjectsState.loadFailure(reason)) {
                Button("Try Again") { actions.reload() }.buttonStyle(.jojo(.secondary, compact: true))
            }
        default:
            StateBlock(symbol: "folder", title: "No projects yet",
                       message: "A project names its registered repositories, the commands that test them, the instructions every agent session gets, the provider per task kind and the parallel capacity.") {
                Button("Create Project…") { actions.newProject() }.buttonStyle(.jojo(.primary, compact: true))
            }
            .frame(minHeight: 200)
        }
    }

    private func create() {
        var fresh = ProjectDraft()
        fresh.id = ProjectsView.newIdentity()
        actions.edit(fresh)
    }

    private var list: some View {
        VStack(alignment: .leading, spacing: 2) {
            GroupLabel("Projects · \(state.list?.projects.count ?? 0)").padding(.horizontal, 6)
            ForEach(state.list?.projects ?? []) { project in
                SelectableRow(selected: project.id == state.project?.id && state.draft == nil, action: { actions.choose(project.id) }) {
                    LetterBadge(project.name)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(project.name).textStyle(.bodyStrong).lineLimit(1)
                        Text(state.line(project)).textStyle(.secondary).foregroundStyle(theme.text2).lineLimit(1)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            VStack(alignment: .leading, spacing: 6) {
                Button("Create Project…") { actions.newProject() }
                    .buttonStyle(.jojo(.ghost, compact: true))
                    .disabled(state.draft != nil)
                Button("Import Local Project…") {}
                    .buttonStyle(.jojo(.ghost, compact: true))
                    .disabled(true)
                    .help("Not built: a repository is registered from Feature Chat.")
            }
            .padding(EdgeInsets(top: 10, leading: 6, bottom: 0, trailing: 6))
            if let list = state.list, !list.projects.isEmpty {
                Footnote("Parallel capacity of the core: \(list.capacity) attempts in all, not split by provider. A lead turn takes one of them (D4).")
                    .padding(EdgeInsets(top: 8, leading: 6, bottom: 0, trailing: 6))
            }
        }
    }

    private func detail(_ project: ProjectRecord) -> some View {
        VStack(alignment: .leading, spacing: 18) {
            HStack(alignment: .top, spacing: 12) {
                LetterBadge(project.name, size: 40)
                VStack(alignment: .leading, spacing: 4) {
                    Text(project.name).font(.system(size: 22, weight: .bold))
                    Text(project.repositories.compactMap(path).first.map { ($0 as NSString).abbreviatingWithTildeInPath }
                         ?? "Revision \(project.revision)")
                        .textStyle(.mono).foregroundStyle(theme.text2).lineLimit(1).truncationMode(.middle)
                    FlowRow(spacing: 6) {
                        ForEach(Array(state.pills(project).enumerated()), id: \.offset) { _, pill in
                            OperationalPill(pill.0, tone: pill.1)
                        }
                    }
                    .padding(.top, 4)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            repositories(project)
            missingConfiguration
            commands(project)
            features(project)
            instructions(project)
        }
    }

    private func repositories(_ project: ProjectRecord) -> some View {
        LabeledSection(title: "Repositories · \(project.repositories.count)") {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(project.repositories.enumerated()), id: \.offset) { index, id in
                    repositoryRow(id, detail: state.shownDetail?.repositories.first { $0.id == id })
                        .overlay(alignment: .top) { if index > 0 { Rectangle().fill(theme.sep).frame(height: 0.5) } }
                }
            }
            .cardElevation(radius: 12)
            if state.shownDetail == nil {
                NotReported(!state.reportsDetail ? "Branch, worktrees and health are not reported by this core (protocol 1.11)."
                            : state.detailLoad == .loading ? "Reading the repositories…"
                            : { if case .failed(let r) = state.detailLoad { return "Not read: \(r)" }; return "Reading the repositories…" }())
            }
        }
    }

    private func repositoryRow(_ id: String, detail: ProjectDetail.Repository?) -> some View {
        let path = detail?.path ?? path(id)
        let name = detail?.name.isEmpty == false ? detail!.name : (path.map { ($0 as NSString).lastPathComponent } ?? String(id.prefix(8)))
        let place = path.map { ($0 as NSString).abbreviatingWithTildeInPath } ?? id
        var line: [String] = []
        if let detail {
            line.append([detail.branch ?? "detached", detail.head.map { String($0.prefix(7)) }].compactMap { $0 }.joined(separator: " @ "))
            line.append("\(detail.worktrees) \(detail.worktrees == 1 ? "worktree" : "worktrees")"
                        + (detail.activeWorktrees > 0 ? " (\(detail.activeWorktrees) active)" : ""))
        }
        return HStack(alignment: .firstTextBaseline, spacing: 10) {
            VStack(alignment: .leading, spacing: 2) {
                Text(name).textStyle(.bodyStrong)
                if !line.isEmpty {
                    Text(line.joined(separator: " · ")).textStyle(.mono).foregroundStyle(theme.text)
                        .lineLimit(1).truncationMode(.middle)
                }
                Text(place).textStyle(.mono).foregroundStyle(theme.text2).lineLimit(1).truncationMode(.middle)
                if let detail, !detail.issues.isEmpty {
                    Text(detail.issues.joined(separator: " · ")).textStyle(.secondary).foregroundStyle(theme.text2)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if let detail {
                OperationalPill(detail.healthy ? "Healthy" : "\(detail.issues.count) \(detail.issues.count == 1 ? "issue" : "issues")",
                                tone: detail.healthy ? .ok : .warn)
            }
        }
        .padding(EdgeInsets(top: 9, leading: 12, bottom: 9, trailing: 12))
    }

    @ViewBuilder
    private var missingConfiguration: some View {
        let lines = state.missingConfiguration
        if state.shownDetail != nil || !lines.isEmpty {
            LabeledSection(title: "Missing configuration · \(lines.count)") {
                if lines.isEmpty {
                    NotReported("Nothing is missing for the flow.")
                } else {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(Array(lines.enumerated()), id: \.offset) { index, text in
                            HStack(spacing: 10) {
                                Image(systemName: "exclamationmark.triangle.fill").font(.system(size: 11)).foregroundStyle(theme.warn)
                                Text(text).textStyle(.secondary).foregroundStyle(theme.text).fixedSize(horizontal: false, vertical: true)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                if text.hasPrefix("No test command") || text.hasPrefix("No CI command"), let project = state.project {
                                    Button("Edit Command") { actions.edit(ProjectDraft(project)) }.buttonStyle(.jojo(.secondary, compact: true))
                                }
                            }
                            .padding(EdgeInsets(top: 8, leading: 12, bottom: 8, trailing: 10))
                            .overlay(alignment: .top) { if index > 0 { Rectangle().fill(theme.sep).frame(height: 0.5) } }
                        }
                    }
                    .background(theme.tint(.warn), in: RoundedRectangle(cornerRadius: 12))
                }
            }
        }
    }

    private func commands(_ project: ProjectRecord) -> some View {
        LabeledSection(title: "Test and CI commands") {
            DefinitionCard(rows: [
                Definition(key: "Test", value: project.testCommand.isEmpty ? "None" : project.testCommand.joined(separator: " "), mono: true),
                Definition(key: "CI", value: project.ciCommand.isEmpty ? "None" : project.ciCommand.joined(separator: " "), mono: true),
            ], keyWidth: 60)
            NotReported("The test command is offered to the lead; host validation runs the approved validation plan's command. The CI command is stored and shown only.")
        }
    }

    private func features(_ project: ProjectRecord) -> some View {
        let mine = state.features(of: project)
        return LabeledSection(title: "Features · \(mine.count)", link: ("New Feature", { actions.newFeature(project.id) })) {
            if mine.isEmpty {
                NotReported("No feature of this project yet.")
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(mine.enumerated()), id: \.offset) { index, f in
                        let stage = ProjectsState.stageLine(f)
                        Button { actions.openFeature(f.id) } label: {
                            HStack(spacing: 10) {
                                LetterTile(text: "FT", color: Palette.blue)
                                VStack(alignment: .leading, spacing: 1) {
                                    Text("\(f.code) \(f.name ?? "Unnamed feature")").textStyle(.bodyStrong).lineLimit(1)
                                    Text(stage.0).textStyle(.secondary).foregroundStyle(theme.text2).lineLimit(1)
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                                OperationalPill(stage.1, tone: f.done ? .ok : f.blocked > 0 ? .warn : .info)
                            }
                            .padding(EdgeInsets(top: 8, leading: 12, bottom: 8, trailing: 12))
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

    private func instructions(_ project: ProjectRecord) -> some View {
        LabeledSection(title: "Project instructions", link: ("Edit Instructions", { actions.edit(ProjectDraft(project)) })) {
            Text(project.instructions.isEmpty ? "None" : project.instructions)
                .textStyle(.body).foregroundStyle(project.instructions.isEmpty ? theme.text3 : theme.text)
                .fixedSize(horizontal: false, vertical: true)
                .padding(EdgeInsets(top: 10, leading: 12, bottom: 10, trailing: 12))
                .frame(maxWidth: .infinity, alignment: .leading)
                .cardElevation(radius: 12)
            Footnote("Given to every provider session for this project: every lead turn, and through the approved ADR every attempt. A saved change is a new revision; a plan keeps the revision it was drafted under.")
        }
    }

    private func configuration(_ project: ProjectRecord) -> some View {
        GlassInspector {
            LabeledSection(title: "Configuration", link: ("Edit", { actions.edit(ProjectDraft(project)) })) {
                DefinitionCard(rows: [
                    Definition(key: "Revision", value: "\(project.revision)"),
                    Definition(key: "Default provider", value: ProjectsState.provider(project.defaultProvider)),
                    Definition(key: "Task kinds", value: project.kinds.isEmpty ? "None; the lead names the provider" :
                        project.kinds.map { "\($0.kind) → \(ProjectsState.provider($0.provider))" }.joined(separator: "\n")),
                    Definition(key: "Parallel capacity", value: "\(project.capacity) in all"),
                    Definition(key: "Automatic repair", value: ProjectsState.repairs(project.repairLimit)),
                    Definition(key: "Review", value: project.review ? "On: the other provider reviews each step" : "Off"),
                ], keyWidth: 110)
            }
            missing
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
                    FactLine("Automatic repair") {
                        HStack(spacing: 6) {
                            ForEach([Int64(0), 1, 2], id: \.self) { limit in
                                Button { set(\.repairLimit, limit) } label: {
                                    FilterChip(title: ProjectsState.repairs(limit), isOn: draft.repairLimit == limit)
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                    FactLine("Cross-provider review") {
                        Button { set(\.review, !draft.review) } label: {
                            HStack(spacing: 6) {
                                CheckBox(isOn: draft.review)
                                Text("The other provider reviews each step that passed its check").foregroundStyle(theme.text)
                            }
                        }
                        .buttonStyle(.plain)
                    }
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
        LabeledSection(title: "Not built") {
            DefinitionCard(rows: ProjectsState.missing.map { Definition(key: $0.0, value: $0.1) }, keyWidth: 110)
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

    private func set<Value>(_ path: WritableKeyPath<ProjectDraft, Value>, _ value: Value) {
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

/// A project's tile: its first letter on the project gradient (the board's "C", "J").
struct LetterBadge: View {
    let name: String
    var size: CGFloat = 26

    init(_ name: String, size: CGFloat = 26) { (self.name, self.size) = (name, size) }

    var body: some View {
        Text(name.first.map { String($0).uppercased() } ?? "–")
            .font(.system(size: size * 0.46, weight: .bold))
            .foregroundStyle(.white)
            .frame(width: size, height: size)
            .background(Palette.gradient(Palette.projectBadge), in: RoundedRectangle(cornerRadius: size * 0.27))
    }
}

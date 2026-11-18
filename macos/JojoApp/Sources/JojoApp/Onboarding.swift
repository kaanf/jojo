import JojoCore
import SwiftUI

// Onboarding (docs/design/screens/ONBOARDING.md), the canvas board `OnboardingMac.dc.html`:
// three steps that cover the window before the app is used. Agents and tools: the workspace
// folder the helper is started with (the human's decision of 2026-11-18: Onboarding keeps it
// in this Mac's app settings), the coding agents the core found on the login shell's PATH
// (`Executables`), which ones Jojo may run and which one new projects default to, and the
// tools the core reports (git, gh). Autonomy: the one policy the core enforces, drawn as the
// "Balanced" preset with its rules; the other presets are not built. First project: what the
// human brings, and the starting plan that follows from it. Setup only reads the machine.

/// What the human says they bring (step 3), also New Project's subtitle.
enum Have: String, CaseIterable, Identifiable, Sendable {
    case idea, designs, documents, code
    var id: String { rawValue }

    var title: String {
        switch self {
        case .idea: return "An idea"
        case .designs: return "Designs"
        case .documents: return "Documents"
        case .code: return "Code"
        }
    }

    var detail: String {
        switch self {
        case .idea: return "Something you can describe, even roughly."
        case .designs: return "Figma, screenshots or a Claude design canvas."
        case .documents: return "A PRD, brief, notes or a Notion page."
        case .code: return "A project on this Mac or on GitHub."
        }
    }
}

/// Jojo's starting plan for what the human brings: where in the pipeline it begins, what
/// it does first and what the human decides. Only steps the core can take are named.
struct StartingPlan: Equatable {
    var title: String
    var why: String
    /// The pipeline stage it begins at (0 Discovery … 5 Evidence); nil before a choice.
    var start: Int?
    var first: [String]
    var you: [String]
    /// What the human brought that Jojo cannot read yet.
    var unread: String?

    static let stages = ["Discovery", "Spec", "Planning", "Execution", "Verification", "Evidence"]

    static func plan(for haves: Set<Have>) -> StartingPlan {
        let unread: String? = {
            let names = [Have.designs, .documents].filter(haves.contains).map { $0.title.lowercased() }
            guard !names.isEmpty else { return nil }
            return "Jojo can't read \(names.joined(separator: " or ")) yet. Describe what they show in Feature Chat."
        }()
        if haves.contains(.code) && haves.count > 1 {
            return StartingPlan(title: "New feature on existing code", why: "You have code and something to add to it. Jojo learns the codebase first, then you describe the feature.",
                                start: 0, first: ["Read the repository, read-only, and write a Project brief with sources",
                                                  "Propose the command that runs its tests",
                                                  "Open a question for every gap when you describe the feature"],
                                you: ["Approve the Project brief", "Describe the feature in Feature Chat"], unread: unread)
        }
        if haves.contains(.code) {
            return StartingPlan(title: "Learn the codebase", why: "Code, nothing else yet. Jojo learns the architecture, commands and conventions before you describe your first feature.",
                                start: 0, first: ["Map modules, entry points and data stores", "Detect the build and test commands",
                                                  "Write a Project brief with sources"],
                                you: ["Approve the Project brief", "Describe your first feature"], unread: nil)
        }
        if !haves.isEmpty {
            return StartingPlan(title: "Start from scratch", why: "Jojo starts with a short Discovery conversation to pin down users, scope and the first version. It needs a repository to read, even an empty one in the workspace folder.",
                                start: 0, first: ["Ask about users, scope and constraints", "Draft a PRD for the first version",
                                                  "Propose the steps once the PRD is approved"],
                                you: ["Answer the discovery questions", "Approve the PRD and the plan"], unread: unread)
        }
        return StartingPlan(title: "Pick what you have", why: "Pick at least one card. Most projects start from a mix.",
                            start: nil, first: [], you: [], unread: nil)
    }
}

/// What Onboarding shows, as a plain value.
struct OnboardingState: Equatable {
    var step = 0
    var maxStep = 0
    var connection: HelperConnection = .notStarted
    /// The helper was started; until then the workspace folder can still be chosen.
    var started = false
    var setup: JojoSetup
    var listing: WorkspaceListing?
    var executables: ExecutableList?
    var external: ExternalList?
    var haves: Set<Have> = [.code]

    var connected: Bool {
        if case .ready = connection { return true }
        return false
    }

    /// The two agents Jojo runs, in the board's order.
    static let providers = ["ClaudeCode", "Codex"]

    func found(_ provider: String) -> Bool {
        guard let e = executables?[provider] else { return false }
        return e.state == .ready || e.state == .checking
    }

    /// The agents that are found and turned on.
    var enabled: [String] { Self.providers.filter { found($0) && setup.allows($0) } }

    /// The default: the chosen one while it is on, else the first one that is.
    var defaultProvider: String {
        enabled.contains(setup.defaultProvider) ? setup.defaultProvider : enabled.first ?? setup.defaultProvider
    }

    /// Why Continue is not possible yet, or nil.
    var blocked: String? {
        guard step == 0 else { return nil }
        if !started { return "Choose the workspace folder first." }
        if !connected { return "Waiting for the Jojo core." }
        if executables == nil { return "Looking for coding agents…" }
        if enabled.isEmpty { return "Turn on at least one agent to continue." }
        return nil
    }

    /// "We found 2 coding agents on this Mac".
    var headline: String {
        guard executables != nil else { return "Let's find your coding agents" }
        let count = Self.providers.filter(found).count
        switch count {
        case 0: return "We found no coding agent on this Mac"
        case 1: return "We found 1 coding agent on this Mac"
        default: return "We found \(count) coding agents on this Mac"
        }
    }

    var plan: StartingPlan { .plan(for: haves) }

    /// "2 agents on · default Claude Code · Balanced · pushes always ask".
    var summary: String {
        let agents = enabled.count == 1 ? "1 agent on" : "\(enabled.count) agents on"
        return "\(agents) · default \(providerName(defaultProvider)) · Balanced · pushes always ask"
    }
}

struct OnboardingActions {
    var go: (Int) -> Void = { _ in }
    var useWorkspace: () -> Void = {}
    var chooseWorkspace: (URL) -> Void = { _ in }
    var toggle: (String) -> Void = { _ in }
    var makeDefault: (String) -> Void = { _ in }
    var rescan: () -> Void = {}
    var toggleHave: (Have) -> Void = { _ in }
    var toggleAppearance: () -> Void = {}
    var skip: () -> Void = {}
    var continueToProject: () -> Void = {}
}

struct OnboardingView: View {
    @Environment(\.theme) private var theme
    let state: OnboardingState
    var actions = OnboardingActions()
    @State private var choosingFolder = false

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            side
            VStack(alignment: .leading, spacing: 0) {
                HStack {
                    Spacer()
                    Button(action: actions.toggleAppearance) {
                        Image(systemName: theme.isDark ? "sun.max" : "moon").font(.system(size: 13, weight: .medium))
                    }
                    .buttonStyle(IconButtonStyle())
                    .help(theme.isDark ? "Switch to light appearance" : "Switch to dark appearance")
                }
                .padding(.top, 6)
                Scrolling {
                    Group {
                        switch state.step {
                        case 0: agentsStep
                        case 1: autonomyStep
                        default: projectStep
                        }
                    }
                    .padding(.trailing, 8)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                footer
            }
            .padding(EdgeInsets(top: 14, leading: 18, bottom: 16, trailing: 26))
        }
        .padding(8)
        .ignoresSafeArea(.container, edges: .top)
        .background(WindowMaterial().ignoresSafeArea())
        .foregroundStyle(theme.text)
        .fileImporter(isPresented: $choosingFolder, allowedContentTypes: [.folder]) { result in
            if case .success(let url) = result { actions.chooseWorkspace(url) }
        }
    }

    // MARK: Side

    private var side: some View {
        VStack(alignment: .leading, spacing: 20) {
            // The system traffic lights sit here.
            Spacer().frame(height: 22)
            VStack(alignment: .leading, spacing: 10) {
                RoundedRectangle(cornerRadius: 15)
                    .fill(LinearGradient(colors: Palette.appIcon.map(\.color), startPoint: .top, endPoint: .bottom))
                    .frame(width: 56, height: 56)
                    .overlay(Text("J").font(.system(size: 26, weight: .bold)).foregroundStyle(.white))
                VStack(alignment: .leading, spacing: 2) {
                    Text("Set up Jojo").font(.system(size: 17, weight: .bold)).tracking(-0.17)
                    Text("About two minutes").textStyle(.secondary).foregroundStyle(theme.text2)
                }
            }
            .padding(.horizontal, 8)
            VStack(spacing: 2) {
                ForEach(Array(Self.steps.enumerated()), id: \.offset) { index, step in
                    SetupStepItem(number: index + 1, title: step.0, detail: step.1, current: index == state.step,
                                  done: index < state.maxStep && index != state.step, locked: index > state.maxStep) {
                        actions.go(index)
                    }
                }
            }
            Spacer(minLength: 12)
            VStack(alignment: .leading, spacing: 10) {
                promise("desktopcomputer", "Runs on your Mac.", "Your agent CLIs, your sign-ins. Code isn't uploaded.")
                promise("square.on.square.dashed", "Isolated.", "Every task gets its own worktree. Your working copy is never touched.")
                promise("checkmark.shield", "Evidence first.", "Done means tests prove it. Pushes always wait for you.")
            }
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .cardElevation(radius: 14)
        }
        .padding(EdgeInsets(top: 14, leading: 12, bottom: 16, trailing: 12))
        .frame(width: 262)
        .frame(maxHeight: .infinity)
        .glassPanel()
    }

    static let steps = [("Agents and tools", "Found on this Mac"), ("Autonomy", "What runs without asking"),
                        ("First project", "What you are starting from")]

    private func promise(_ symbol: String, _ title: String, _ text: String) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: symbol).font(.system(size: 12, weight: .medium)).foregroundStyle(theme.accent).frame(width: 16)
            (Text(title).bold() + Text(" " + text)).font(.system(size: 12)).lineSpacing(3)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    // MARK: Step 1

    private var agentsStep: some View {
        VStack(alignment: .leading, spacing: 22) {
            VStack(alignment: .leading, spacing: 8) {
                Text("Welcome to Jojo").font(.system(size: 13, weight: .semibold)).foregroundStyle(theme.accent)
                Text(state.headline).font(.system(size: 30, weight: .bold)).tracking(-0.9)
                Text("Jojo turns a feature request into a spec, a plan and verified code using the agents you already have. Agents propose, you approve. Pick which ones Jojo may run and which one it uses first.")
                    .font(.system(size: 14)).lineSpacing(4).foregroundStyle(theme.text2).frame(maxWidth: 600, alignment: .leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
            workspaceSection
            agentsSection
            toolsSection
        }
    }

    private var workspaceSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Workspace folder").textStyle(.label).foregroundStyle(theme.text3)
            HStack(spacing: 12) {
                RoundedRectangle(cornerRadius: 9).fill(Palette.gray.color).frame(width: 34, height: 34)
                    .overlay(Image(systemName: "folder").font(.system(size: 14, weight: .semibold)).foregroundStyle(.white))
                VStack(alignment: .leading, spacing: 2) {
                    Text(Self.tilde(state.setup.workspace)).font(.system(size: 13, weight: .semibold, design: .monospaced))
                    Text(workspaceNote).textStyle(.secondary).foregroundStyle(workspaceProblem == nil ? theme.text2 : theme.orange)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                if state.started {
                    OperationalPill(workspaceProblem == nil ? "In use" : "Not usable", tone: workspaceProblem == nil ? .ok : .warn)
                } else {
                    Button("Choose…") { choosingFolder = true }.buttonStyle(.jojo(.secondary, compact: true))
                    Button("Use This Folder", action: actions.useWorkspace).buttonStyle(.jojo(.primary, compact: true))
                }
            }
            .padding(EdgeInsets(top: 12, leading: 14, bottom: 12, trailing: 14))
            .cardElevation(radius: 14)
        }
    }

    private var workspaceProblem: String? { state.started ? state.listing?.problem : nil }

    private var workspaceNote: String {
        if let problem = workspaceProblem {
            return "\(problem). Choose a private folder and start Jojo again."
        }
        if state.started {
            return "Jojo works only on repositories inside this folder. Another folder takes effect when Jojo starts again."
        }
        return "Jojo works only on repositories inside this private folder. It is created for you if it does not exist."
    }

    private var agentsSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline) {
                Text("Coding agents").textStyle(.label).foregroundStyle(theme.text3)
                Spacer()
                Button("Rescan", action: actions.rescan).buttonStyle(.plain).font(.system(size: 12))
                    .foregroundStyle(theme.accent).disabled(!state.connected)
            }
            VStack(spacing: 0) {
                ForEach(Array(OnboardingState.providers.enumerated()), id: \.offset) { index, provider in
                    agentRow(provider).overlay(alignment: .top) { if index > 0 { Rectangle().fill(theme.sep).frame(height: 0.5) } }
                }
                geminiRow.overlay(alignment: .top) { Rectangle().fill(theme.sep).frame(height: 0.5) }
            }
            .cardElevation(radius: 14)
            if let reason = state.blocked, state.started, state.executables != nil {
                Text(reason).font(.system(size: 12)).foregroundStyle(theme.red).padding(.horizontal, 4)
            }
            Text("Jojo uses the default for new projects. You can still pick per project.")
                .font(.system(size: 12)).foregroundStyle(theme.text2).padding(.horizontal, 4)
        }
    }

    private func agentRow(_ provider: String) -> some View {
        let e = state.executables?[provider]
        let found = state.found(provider)
        let on = state.setup.allows(provider)
        let (status, tone): (String, Tone) = {
            guard state.started else { return ("Not checked", .neutral) }
            switch e?.state {
            case .ready?: return (e?.profiled == false ? "Ready · newer" : "Ready", .ok)
            case .checking?: return ("Checking", .info)
            case .refused?: return ("Not found", .neutral)
            case .unconfigured?: return ("Turned off", .neutral)
            case nil: return (state.connected ? "Looking…" : "Not checked", .neutral)
            }
        }()
        let note: String = {
            guard let e else { return "Jojo looks on your login shell's PATH once the core is running." }
            switch e.state {
            case .ready, .checking:
                let version = e.version.map { "Version \($0)" } ?? "Version not read yet"
                return "\(version) · its own sign-in, checked when a turn runs"
            case .refused: return e.detail.isEmpty ? "Not found on PATH. Install it, then start Jojo again." : e.detail
            case .unconfigured: return "Not started for this session. Turn it on and start Jojo again."
            }
        }()
        return HStack(spacing: 12) {
            RoundedRectangle(cornerRadius: 9).fill((provider == "ClaudeCode" ? Palette.claudeCode : RGBA(0x3C3C44)).color)
                .frame(width: 34, height: 34)
                .overlay(Text(provider == "ClaudeCode" ? "CC" : "CX").font(.system(size: 12, weight: .bold)).foregroundStyle(.white))
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 8) {
                    Text(providerName(provider)).font(.system(size: 14, weight: .semibold))
                    OperationalPill(status, tone: tone)
                }
                Text(note).textStyle(.secondary).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
                Text(e?.path ?? e?.entry ?? "Not found on PATH").textStyle(.mono).foregroundStyle(theme.text3).lineLimit(1)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if found || !state.started {
                HStack(spacing: 10) {
                    DefaultRadio(isOn: on && state.defaultProvider == provider, enabled: on) { actions.makeDefault(provider) }
                    SwitchToggle(isOn: on, label: "Allow Jojo to run \(providerName(provider))") { actions.toggle(provider) }
                }
            }
        }
        .padding(EdgeInsets(top: 12, leading: 14, bottom: 12, trailing: 14))
        .frame(minHeight: 64)
    }

    private var geminiRow: some View {
        HStack(spacing: 12) {
            RoundedRectangle(cornerRadius: 9).fill(Palette.gray.color).frame(width: 34, height: 34)
                .overlay(Text("GM").font(.system(size: 12, weight: .bold)).foregroundStyle(.white))
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 8) {
                    Text("Gemini CLI").font(.system(size: 14, weight: .semibold))
                    OperationalPill("Not supported", tone: .neutral)
                }
                Text("Jojo runs Claude Code and Codex only.").textStyle(.secondary).foregroundStyle(theme.text2)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(EdgeInsets(top: 12, leading: 14, bottom: 12, trailing: 14))
        .frame(minHeight: 64)
    }

    private var toolsSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Tools Jojo relies on").textStyle(.label).foregroundStyle(theme.text3)
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 220), spacing: 8)], spacing: 8) {
                tool("git", "Required · worktrees and commits\(gitNote)", state: gitState)
                tool("GitHub CLI", ghNote, state: ghState)
                tool("Xcode command line tools", "Not reported by this core", state: nil)
                tool("JDK and Gradle wrapper", "Not reported by this core", state: nil)
                tool("Docker", "Optional · not reported by this core", state: nil)
            }
        }
    }

    private var gitNote: String {
        guard let git = state.external?.git else { return "" }
        return git.ready ? " · credential helper set" : " · no credential helper"
    }

    /// git itself is always there for the core (`/usr/bin/git`); its sign-in only matters
    /// for a push.
    private var gitState: Bool? { state.connected ? true : nil }

    private var ghState: Bool? { state.external.map { $0.gh.ready } }

    private var ghNote: String {
        guard let gh = state.external?.gh else { return "Only for pull requests you approve" }
        return gh.ready ? "Signed in · only for pull requests you approve" : "Not signed in · only needed for pull requests"
    }

    /// `state` true is found, false is missing, nil is not reported.
    private func tool(_ title: String, _ detail: String, state: Bool?) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Circle().fill(state == true ? theme.green : state == false ? theme.orange : theme.dotOff)
                .frame(width: 8, height: 8).padding(.top, 5)
            VStack(alignment: .leading, spacing: 1) {
                Text(title).font(.system(size: 13, weight: .semibold))
                Text(detail).textStyle(.secondary).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
        }
        .padding(EdgeInsets(top: 10, leading: 12, bottom: 10, trailing: 12))
        .frame(maxWidth: .infinity, alignment: .leading)
        .cardElevation(radius: 12)
    }

    // MARK: Step 2

    private var autonomyStep: some View {
        VStack(alignment: .leading, spacing: 22) {
            VStack(alignment: .leading, spacing: 8) {
                Text("How much should agents do on their own?").font(.system(size: 30, weight: .bold)).tracking(-0.9)
                Text("This is what the core enforces for every project today. Every action lands in the audit log either way.")
                    .font(.system(size: 14)).lineSpacing(4).foregroundStyle(theme.text2).frame(maxWidth: 600, alignment: .leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
            HStack(alignment: .top, spacing: 10) {
                PresetCard(name: "Careful", detail: "Agents ask before editing files or running commands. Not built: the core runs one policy.",
                           on: false, recommended: false, enabled: false)
                PresetCard(name: "Balanced", detail: "Agents work freely inside the approved task scope; anything outside it fails the step.",
                           on: true, recommended: true, enabled: true)
                PresetCard(name: "Autonomous", detail: "Agents may widen scope and install dependencies on their own. Not built: the core runs one policy.",
                           on: false, recommended: false, enabled: false)
            }
            VStack(alignment: .leading, spacing: 8) {
                Text("With Balanced, when an agent wants to").textStyle(.label).foregroundStyle(theme.text3)
                VStack(spacing: 0) {
                    ForEach(Array(Self.rules.enumerated()), id: \.offset) { index, rule in
                        HStack(spacing: 12) {
                            VStack(alignment: .leading, spacing: 1) {
                                Text(rule.0).font(.system(size: 13, weight: .semibold))
                                Text(rule.1).textStyle(.secondary).foregroundStyle(theme.text2)
                            }
                            .frame(maxWidth: .infinity, alignment: .leading)
                            OperationalPill(rule.2, tone: rule.3)
                        }
                        .padding(EdgeInsets(top: 10, leading: 14, bottom: 10, trailing: 14))
                        .overlay(alignment: .top) { if index > 0 { Rectangle().fill(theme.sep).frame(height: 0.5) } }
                    }
                }
                .cardElevation(radius: 14)
            }
            HStack(alignment: .top, spacing: 12) {
                Image(systemName: "exclamationmark.shield").font(.system(size: 15, weight: .semibold)).foregroundStyle(theme.red)
                (Text("Pushes and pull requests always ask.").bold()
                    + Text(" They leave your Mac and Jojo can't undo them, so no preset can make them automatic."))
                    .font(.system(size: 13)).lineSpacing(3).fixedSize(horizontal: false, vertical: true)
            }
            .padding(EdgeInsets(top: 12, leading: 14, bottom: 12, trailing: 14))
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(theme.redBg, in: RoundedRectangle(cornerRadius: 14))
        }
    }

    /// The core's policy, as `CLAUDE_TOOLS`, Codex's `workspace-write` sandbox, host
    /// validation and M5's authorization enforce it.
    static let rules: [(String, String, String, Tone)] = [
        ("Read files in the project", "Code, docs and git history", "Allow", .ok),
        ("Edit files inside the task scope", "In the task's own worktree; your working copy is untouched", "Allow", .ok),
        ("Run builds and tests", "Jojo runs the approved check after each step", "Jojo runs it", .info),
        ("Write outside the task scope", "There is no scope expansion request yet", "Fails the step", .warn),
        ("Install dependencies or use the network", "Agents get no network tool", "Not allowed", .danger),
        ("Push or open a pull request", "Leaves your Mac, can't be undone", "Always ask", .danger),
    ]

    // MARK: Step 3

    private var projectStep: some View {
        VStack(alignment: .leading, spacing: 20) {
            HStack(spacing: 10) {
                Circle().fill(Palette.green.color).frame(width: 22, height: 22)
                    .overlay(Image(systemName: "checkmark").font(.system(size: 10, weight: .bold)).foregroundStyle(.white))
                Text("Your Mac is set up").font(.system(size: 13, weight: .semibold))
                Text(state.summary).textStyle(.secondary).foregroundStyle(theme.text2)
                Spacer(minLength: 0)
            }
            .padding(EdgeInsets(top: 10, leading: 14, bottom: 10, trailing: 14))
            .cardElevation(radius: 14)
            VStack(alignment: .leading, spacing: 8) {
                Text("What are you starting from?").font(.system(size: 30, weight: .bold)).tracking(-0.9)
                Text("Bring whatever you have, in any mix. Jojo works out where in the pipeline to begin and what it can skip.")
                    .font(.system(size: 14)).lineSpacing(4).foregroundStyle(theme.text2).frame(maxWidth: 600, alignment: .leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
            DropZone(symbol: "tray.and.arrow.down", title: "Bring a project folder",
                     lines: ["A Git repository inside \(Self.tilde(state.setup.workspace)). Links, screenshots and PDFs: not available yet"]) {
                Button("Choose Folder…", action: actions.continueToProject).buttonStyle(.jojo(.secondary, compact: true))
                Button("Paste Link") {}.buttonStyle(.jojo(.secondary, compact: true)).disabled(true)
                    .help("Not available yet: Jojo reads repositories on this Mac only.")
            }
            VStack(alignment: .leading, spacing: 8) {
                Text("Or pick what you have").textStyle(.label).foregroundStyle(theme.text3)
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 170), spacing: 8)], spacing: 8) {
                    ForEach(Have.allCases) { have in
                        HaveCard(have: have, isOn: state.haves.contains(have)) { actions.toggleHave(have) }
                    }
                }
            }
            planPanel
        }
    }

    private var planPanel: some View {
        let plan = state.plan
        return VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text("Jojo's starting plan").textStyle(.label).foregroundStyle(theme.text3)
                Text(plan.title).font(.system(size: 15, weight: .bold))
            }
            Text(plan.why).font(.system(size: 13)).lineSpacing(3).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
            PipelinePills(start: plan.start)
            if !plan.first.isEmpty {
                HStack(alignment: .top, spacing: 12) {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("Jojo does first").font(.system(size: 12, weight: .semibold))
                        ForEach(Array(plan.first.enumerated()), id: \.offset) { index, line in
                            HStack(alignment: .firstTextBaseline, spacing: 8) {
                                Text("\(index + 1)").font(.system(size: 12, weight: .bold, design: .monospaced)).foregroundStyle(theme.accent)
                                Text(line).font(.system(size: 12)).lineSpacing(2).fixedSize(horizontal: false, vertical: true)
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                    VStack(alignment: .leading, spacing: 6) {
                        Text("You decide").font(.system(size: 12, weight: .semibold))
                        ForEach(plan.you, id: \.self) { line in
                            HStack(alignment: .top, spacing: 8) {
                                Circle().fill(theme.orange).frame(width: 8, height: 8).padding(.top, 4)
                                Text(line).font(.system(size: 12)).lineSpacing(2).fixedSize(horizontal: false, vertical: true)
                            }
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .topLeading)
                }
            }
            if let unread = plan.unread {
                Text(unread).textStyle(.secondary).foregroundStyle(theme.orange).fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(EdgeInsets(top: 16, leading: 18, bottom: 16, trailing: 18))
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassPanel(radius: 16)
    }

    // MARK: Footer

    private var footer: some View {
        HStack(spacing: 10) {
            Text("Step \(state.step + 1) of 3 · setup only reads your machine").textStyle(.secondary).foregroundStyle(theme.text2)
            Spacer(minLength: 8)
            if let reason = state.blocked, state.step == 0, !(state.started && state.executables != nil) {
                DisabledReason(reason).lineLimit(1)
            }
            if state.step > 0 {
                Button("Back") { actions.go(state.step - 1) }.buttonStyle(.jojo(.secondary))
            }
            if state.step < 2 {
                Button(state.step == 0 ? "Continue" : "Finish Machine Setup") { actions.go(state.step + 1) }
                    .buttonStyle(.jojo(.primary))
                    .disabled(state.blocked != nil)
            } else {
                Button("Skip for Now", action: actions.skip).buttonStyle(.plain).font(.system(size: 13, weight: .medium))
                    .foregroundStyle(theme.text2).padding(.horizontal, 10)
                Button("Continue to Project Setup", action: actions.continueToProject).buttonStyle(.jojo(.primary))
                    .disabled(state.plan.start == nil)
            }
        }
        .padding(.top, 18)
        .overlay(alignment: .top) { Rectangle().fill(theme.sep).frame(height: 0.5) }
    }

    /// `~/JojoWorkspace` for a path under the home folder.
    static func tilde(_ path: String) -> String {
        let home = NSHomeDirectory()
        return path.hasPrefix(home + "/") ? "~" + path.dropFirst(home.count) : path
    }
}

// MARK: - Components

/// A step of a setup flow in a side panel: a 22 numbered ring, a title and a sub line;
/// current on `accentTint`, done with a green filled number.
struct SetupStepItem: View {
    @Environment(\.theme) private var theme
    let number: Int
    let title: String
    let detail: String
    let current: Bool
    let done: Bool
    let locked: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 10) {
                StepNumber(number: number, current: current, done: done)
                VStack(alignment: .leading, spacing: 0) {
                    Text(title).font(.system(size: 13, weight: current ? .semibold : .regular))
                        .foregroundStyle(current ? theme.text : theme.text2)
                    Text(detail).font(.system(size: 11)).foregroundStyle(theme.text3)
                }
                Spacer(minLength: 0)
            }
            .padding(EdgeInsets(top: 8, leading: 10, bottom: 8, trailing: 10))
            .background(current ? theme.accentTint : .clear, in: RoundedRectangle(cornerRadius: 10))
            .contentShape(RoundedRectangle(cornerRadius: 10))
        }
        .buttonStyle(.plain)
        .disabled(locked)
        .accessibilityAddTraits(current ? .isSelected : [])
    }
}

/// The 22 round step number: a `dotOff` ring, `accent` when current, green with a check
/// when done.
struct StepNumber: View {
    @Environment(\.theme) private var theme
    let number: Int
    let current: Bool
    let done: Bool

    var body: some View {
        Circle()
            .fill(current ? theme.accent : done ? theme.green : .clear)
            .overlay { if !current && !done { Circle().strokeBorder(theme.dotOff, lineWidth: 1.5) } }
            .overlay {
                if done && !current {
                    Image(systemName: "checkmark").font(.system(size: 10, weight: .bold)).foregroundStyle(.white)
                } else {
                    Text("\(number)").font(.system(size: 11, weight: .bold)).foregroundStyle(current ? .white : theme.text2)
                }
            }
            .frame(width: 22, height: 22)
    }
}

/// A drawn switch, 38 × 22: `dotOff` off, green on, a white 18 knob.
struct SwitchToggle: View {
    @Environment(\.theme) private var theme
    let isOn: Bool
    let label: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Capsule().fill(isOn ? Palette.green.color : theme.dotOff)
                .frame(width: 38, height: 22)
                .overlay(alignment: isOn ? .trailing : .leading) {
                    Circle().fill(.white).frame(width: 18, height: 18).shadow(color: .black.opacity(0.3), radius: 1.5, y: 1).padding(2)
                }
        }
        .buttonStyle(.plain)
        .accessibilityLabel(label)
        .accessibilityValue(isOn ? "On" : "Off")
    }
}

/// "Default": a 14 ring that fills to an `accent` dot when chosen.
struct DefaultRadio: View {
    @Environment(\.theme) private var theme
    let isOn: Bool
    let enabled: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 6) {
                Circle().strokeBorder(isOn ? theme.accent : theme.text3, lineWidth: isOn ? 4.5 : 1.5).frame(width: 14, height: 14)
                Text("Default").font(.system(size: 12, weight: isOn ? .semibold : .regular))
                    .foregroundStyle(isOn ? theme.text : theme.text2)
            }
            .padding(.horizontal, 10)
            .frame(height: 26)
            .contentShape(Capsule())
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
        .accessibilityAddTraits(isOn ? .isSelected : [])
    }
}

/// An autonomy preset: radius 14, padding 14, `accentTint` and an `accent` outline when on.
struct PresetCard: View {
    @Environment(\.theme) private var theme
    let name: String
    let detail: String
    let on: Bool
    let recommended: Bool
    let enabled: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Text(name).font(.system(size: 15, weight: .bold)).frame(maxWidth: .infinity, alignment: .leading)
                if recommended { OperationalPill("Recommended", tone: .info) }
            }
            Text(detail).font(.system(size: 12)).lineSpacing(3).foregroundStyle(theme.text2).fixedSize(horizontal: false, vertical: true)
        }
        .padding(14)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(on ? theme.accentTint : theme.card, in: RoundedRectangle(cornerRadius: 14))
        .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(on ? theme.accent : theme.sep, lineWidth: on ? 1.5 : 0.5))
        .opacity(enabled ? 1 : 0.55)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(on ? .isSelected : [])
    }
}

/// What the human has: a check box, a title and a line; `accentTint` with an outline on.
struct HaveCard: View {
    @Environment(\.theme) private var theme
    let have: Have
    let isOn: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(alignment: .top, spacing: 10) {
                CheckBox(isOn: isOn).padding(.top, 1)
                VStack(alignment: .leading, spacing: 2) {
                    Text(have.title).font(.system(size: 13, weight: .semibold)).foregroundStyle(theme.text)
                    Text(have.detail).font(.system(size: 12)).lineSpacing(2).foregroundStyle(theme.text2)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 0)
            }
            .padding(12)
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(isOn ? theme.accentTint : theme.card, in: RoundedRectangle(cornerRadius: 14))
            .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(isOn ? theme.accent : theme.sep, lineWidth: isOn ? 1.5 : 0.5))
            .contentShape(RoundedRectangle(cornerRadius: 14))
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(isOn ? .isSelected : [])
    }
}

/// The six pipeline stages as pills: the start blue, the rest gray.
struct PipelinePills: View {
    let start: Int?

    var body: some View {
        WrappingRow(spacing: 6, lineSpacing: 6) {
            ForEach(Array(StartingPlan.stages.enumerated()), id: \.offset) { index, stage in
                OperationalPill(stage, tone: index == start ? .info : .neutral)
            }
        }
    }
}

/// Onboarding wired to the app model: the setup in force, the core's answers and the
/// commands. It reads `Executables`, `Workspace` and `ExternalActions` while shown.
struct OnboardingScreen: View {
    @ObservedObject var model: AppModel
    var openProject: (Set<Have>) -> Void = { _ in }
    @State private var step = 0
    @State private var maxStep = 0
    @State private var haves: Set<Have> = [.code]
    @State private var draft: JojoSetup?

    private var setup: JojoSetup {
        draft ?? model.setup ?? JojoSetup(workspace: JojoSetup.defaultWorkspace(home: NSHomeDirectory()))
    }

    var state: OnboardingState {
        let workspace = model.workspace
        return OnboardingState(step: step, maxStep: maxStep, connection: model.client.connection, started: model.started,
                               setup: setup, listing: workspace.workspaceListing, executables: workspace.executables,
                               external: workspace.external, haves: haves)
    }

    var body: some View {
        OnboardingView(state: state, actions: actions)
            .onAppear(perform: read)
            .onChange(of: model.client.connection) { read() }
    }

    private func read() {
        let workspace = model.workspace
        workspace.executablesShown = true
        guard model.started, case .ready = model.client.connection else { return }
        workspace.loadExecutables()
        workspace.loadWorkspaceListing()
        workspace.loadExternalActions()
    }

    private var actions: OnboardingActions {
        var a = OnboardingActions()
        a.go = { target in
            guard target <= maxStep + 1, target >= 0, target <= 2 else { return }
            if target > step, state.blocked != nil { return }
            step = target
            maxStep = max(maxStep, target)
            model.setup = setup
            setup.save()
        }
        a.useWorkspace = { model.useSetup(setup) }
        a.chooseWorkspace = { url in
            var next = setup
            next.workspace = url.path
            draft = next
        }
        a.toggle = { provider in
            var next = setup
            if provider == "Codex" { next.codex.toggle() } else { next.claudeCode.toggle() }
            draft = next
            if model.started { model.setup = next; next.save() }
        }
        a.makeDefault = { provider in
            var next = setup
            next.defaultProvider = provider
            draft = next
            if model.started { model.setup = next; next.save() }
        }
        a.rescan = { model.workspace.loadExecutables() }
        a.toggleHave = { have in
            if haves.contains(have) { haves.remove(have) } else { haves.insert(have) }
        }
        a.toggleAppearance = { model.setAppearance(model.appearance == .dark ? .light : .dark) }
        a.skip = {
            model.workspace.executablesShown = false
            model.finishSetup(finished)
        }
        a.continueToProject = {
            model.workspace.executablesShown = false
            model.finishSetup(finished)
            openProject(haves)
        }
        return a
    }

    /// The setup as it is kept: the default is one of the agents that are on.
    private var finished: JojoSetup {
        var done = setup
        done.defaultProvider = state.defaultProvider
        return done
    }
}

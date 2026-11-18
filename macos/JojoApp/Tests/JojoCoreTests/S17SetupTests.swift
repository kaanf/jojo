import AppKit
import SwiftUI
import XCTest
@testable import JojoApp
@testable import JojoCore

/// Step 7 rows: a workspace folder, two agents, a project with two repositories and briefs.
enum SetupRows {
    static let api = String(repeating: "a", count: 31) + "1"
    static let app = String(repeating: "a", count: 31) + "2"
    static let project = String(repeating: "c", count: 32)
    static let brief = String(repeating: "b", count: 32)
    static let setup = JojoSetup(workspace: "/Users/me/JojoWorkspace")

    static let executables = ExecutableList(providers: [
        ProviderExecutable(provider: "ClaudeCode", source: .discovered, state: .ready, entry: "/Users/me/.local/bin",
                           path: "/Users/me/.local/bin/claude", version: "2.1.3", profiled: true),
        ProviderExecutable(provider: "Codex", source: .discovered, state: .ready, entry: "/opt/homebrew/bin",
                           path: "/opt/homebrew/lib/codex/codex", version: "0.120.0", profiled: true),
    ])

    static let listing = WorkspaceListing(root: "/Users/me/JojoWorkspace", entries: [
        .init(path: "/Users/me/JojoWorkspace/crew-api", name: "crew-api", stack: ["Gradle"], repository: api),
        .init(path: "/Users/me/JojoWorkspace/crew-app", name: "crew-app", stack: ["Gradle", "Swift"]),
        .init(path: "/Users/me/JojoWorkspace/tools/jojo", name: "jojo", stack: ["Rust", "Swift", "Make"]),
    ])

    static let proposed = ProjectBrief(
        document: brief, revision: 1, approval: "Pending", summary: "A Spring Boot API with Flyway migrations.",
        rows: [.init(key: "Architecture", text: "Spring Boot API with Flyway migrations; thin controllers.",
                     sources: ["build.gradle.kts", "src/main/kotlin"]),
               .init(key: "Commands", text: "Build and tests run through the Gradle wrapper.", sources: ["gradlew"]),
               .init(key: "Watch out", text: "No contract tests between the app and the API.", warning: true)],
        testCommand: ["/Users/me/JojoWorkspace/crew-api/gradlew", "test"])

    static func detail(turn: String = "Replied", approved: Bool = false) -> ProjectDetail {
        let brief = ProjectBrief(document: proposed.document, revision: 1, approval: approved ? "Approved" : "Pending",
                                 summary: proposed.summary, rows: proposed.rows, testCommand: proposed.testCommand)
        return ProjectDetail(project: project, revision: 1, name: "Crew", repositories: [
            .init(id: api, name: "crew-api", path: "/Users/me/JojoWorkspace/crew-api", branch: "main",
                  brief: turn == "Replied" ? brief : nil,
                  briefTurn: BriefTurnState(turn: String(repeating: "d", count: 32), provider: "ClaudeCode", state: turn)),
            .init(id: app, name: "crew-app", path: "/Users/me/JojoWorkspace/crew-app", branch: "main",
                  briefTurn: turn == "Replied" ? BriefTurnState(turn: String(repeating: "e", count: 32), provider: "ClaudeCode",
                                                                state: "Running") : nil),
        ])
    }

    static func onboarding(step: Int = 0, started: Bool = true, executables: ExecutableList? = executables,
                           setup: JojoSetup = setup, haves: Set<Have> = [.code, .designs]) -> OnboardingState {
        OnboardingState(step: step, maxStep: step, connection: started ? .ready(epoch: 3) : .notStarted, started: started,
                        setup: setup, listing: listing, executables: executables, external: nil, haves: haves)
    }

    static func newProject(step: Int = 0, detail: ProjectDetail? = nil) -> NewProjectState {
        NewProjectState(step: step, connection: .ready(epoch: 3), admission: "Open", queries: ["Workspace"],
                        haves: [.code, .designs], name: "", listing: listing, listingLoad: .loaded,
                        chosen: ["/Users/me/JojoWorkspace/crew-api", "/Users/me/JojoWorkspace/crew-app"],
                        project: detail == nil ? nil : project, detail: detail)
    }
}

@MainActor
final class S17SetupTests: XCTestCase {
    func test_s17_setup_requests_go_at_1_12_with_exactly_the_helpers_fields() throws {
        XCTAssertEqual(Request.BriefTurn.minor, 12)
        XCTAssertEqual(Request.Workspace.minor, 12)
        XCTAssertEqual(Request.RegisterRepository.minor, 1)
        XCTAssertEqual(Request.BriefTurn.fields, ["repository", "provider", "message"])
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.refresh()
        h.fake.reply(h.fake.last("Snapshot")!, ["rows": .list([]), "as_of_event_seq": .int(30), "admission": .text("Open")])
        model.loadWorkspaceListing()
        let listing = try XCTUnwrap(h.fake.last("Workspace"))
        XCTAssertEqual(listing.minor, 12)
        h.fake.reply(listing, ["root": .text("/w"), "problem": .null, "repositories": .list([
            .object(["path": .text("/w/a"), "name": .text("a"), "stack": .list([.text("Rust")]), "repository": .null]),
        ])])
        XCTAssertEqual(model.workspaceListing?.entries.map(\.name), ["a"])
        XCTAssertEqual(model.workspaceListing?.entries.first?.stack, ["Rust"])
        model.requestBrief(repository: SetupRows.api, provider: "ClaudeCode", message: "The product owner brings: code.")
        let turn = try XCTUnwrap(h.fake.last("BriefTurn"))
        XCTAssertEqual(turn.minor, 12)
        XCTAssertEqual(turn.payload, ["repository": .text(SetupRows.api), "provider": .text("ClaudeCode"),
                                      "message": .text("The product owner brings: code.")])
    }

    func test_s17_a_brief_is_decoded_from_project_detail() {
        let row: JSON = .object([
            "repository": .text(SetupRows.api), "name": .text("crew-api"),
            "brief": .object([
                "brief": .object(["document": .text(SetupRows.brief), "revision": .int(2), "approval": .text("Approved"),
                                  "summary": .text("S"), "rows": .list([.object(["key": .text("Commands"), "text": .text("make test"),
                                                                               "sources": .list([.text("Makefile")]),
                                                                               "warning": .bool(false)])]),
                                  "test_command": .list([.text("/usr/bin/make"), .text("test")]),
                                  "created_at_unix_ms": .int(5)]),
                "turn": .object(["turn": .text(String(repeating: "d", count: 32)), "provider": .text("Codex"),
                                 "state": .text("Replied"), "detail": .null]),
            ]),
        ])
        let detail = ProjectDetail(["project": .text(SetupRows.project), "repositories": .list([row])])
        let repository = detail?.repositories.first
        XCTAssertEqual(repository?.brief?.revision, 2)
        XCTAssertTrue(repository?.brief?.approved ?? false)
        XCTAssertEqual(repository?.brief?.rows.first?.sources, ["Makefile"])
        XCTAssertEqual(repository?.brief?.testCommand, ["/usr/bin/make", "test"])
        XCTAssertEqual(repository?.briefTurn?.state, "Replied")
        XCTAssertFalse(repository?.briefTurn?.running ?? true)
    }

    func test_s17_the_setup_composes_the_helper_options_of_the_live_scripts() {
        var setup = SetupRows.setup
        XCTAssertEqual(setup.helperOptions(home: "/Users/me", user: "me", shell: "/bin/zsh"), [
            "--disposable-root", "/Users/me/JojoWorkspace",
            "--claude-home", "/Users/me", "--claude-user", "me", "--claude-auth", "native-subscription",
            "--codex-home", "/Users/me/.codex", "--codex-auth", "native-subscription", "--login-shell", "/bin/zsh",
        ])
        setup.codex = false
        XCTAssertFalse(setup.helperOptions(home: "/Users/me", user: "me", shell: nil).contains("--codex-home"))
        // A launch without helper options starts from the saved setup, else Onboarding runs.
        let saved = LaunchOptions.parse(["JojoApp"], setup: SetupRows.setup, environment: ["HOME": "/Users/me", "SHELL": "/bin/zsh"])
        XCTAssertFalse(saved.needsSetup)
        XCTAssertEqual(saved.slice.prefix(2), ["--disposable-root", "/Users/me/JojoWorkspace"])
        let first = LaunchOptions.parse(["JojoApp"], setup: nil, environment: ["HOME": "/Users/me"])
        XCTAssertTrue(first.needsSetup)
        XCTAssertEqual(first.slice, [])
        let scripted = LaunchOptions.parse(["JojoApp", "--ui-script", "/s.json"], setup: nil, environment: [:])
        XCTAssertFalse(scripted.needsSetup, "a scripted launch never shows Onboarding")
    }

    func test_s17_onboarding_continues_only_with_an_agent_on() {
        XCTAssertEqual(SetupRows.onboarding(started: false).blocked, "Choose the workspace folder first.")
        XCTAssertNil(SetupRows.onboarding().blocked)
        XCTAssertEqual(SetupRows.onboarding().headline, "We found 2 coding agents on this Mac")
        var off = SetupRows.setup
        off.claudeCode = false
        off.codex = false
        XCTAssertEqual(SetupRows.onboarding(setup: off).blocked, "Turn on at least one agent to continue.")
        off.codex = true
        XCTAssertEqual(SetupRows.onboarding(setup: off).defaultProvider, "Codex", "the default follows the agents that are on")
        XCTAssertEqual(StartingPlan.plan(for: [.code, .designs]).title, "New feature on existing code")
        XCTAssertNotNil(StartingPlan.plan(for: [.code, .designs]).unread)
        XCTAssertEqual(StartingPlan.plan(for: [.code]).title, "Learn the codebase")
        XCTAssertNil(StartingPlan.plan(for: []).start)
    }

    func test_s17_new_project_reads_and_counts_briefs() {
        XCTAssertEqual(SetupRows.newProject().startBlocked, nil)
        XCTAssertEqual(SetupRows.newProject().projectName, "crew-api")
        var none = SetupRows.newProject()
        none.chosen = []
        XCTAssertEqual(none.startBlocked, "Choose at least one repository.")
        let reading = SetupRows.newProject(step: 1, detail: SetupRows.detail())
        XCTAssertEqual(reading.readCount, 1)
        XCTAssertTrue(reading.reading)
        let review = SetupRows.newProject(step: 2, detail: SetupRows.detail(approved: true))
        XCTAssertEqual(review.approvedCount, 1)
        XCTAssertEqual(review.footNote, "All approved · Discovery can start")
    }

    /// Review aid: Onboarding and New Project from their Mac boards, in their states.
    func test_s17_setup_renders_in_both_appearances() throws {
        let size = CGSize(width: 1180, height: 940)
        try render(OnboardingView(state: SetupRows.onboarding(started: false, executables: nil)), size, "s17-onboarding-folder")
        try render(OnboardingView(state: SetupRows.onboarding()), size, "s17-onboarding-agents")
        try render(OnboardingView(state: SetupRows.onboarding(step: 1)), size, "s17-onboarding-autonomy")
        try render(OnboardingView(state: SetupRows.onboarding(step: 2)), size, "s17-onboarding-project")
        try render(NewProjectView(state: SetupRows.newProject()), size, "s17-new-project-sources")
        try render(NewProjectView(state: SetupRows.newProject(step: 1, detail: SetupRows.detail())), size, "s17-new-project-reading")
        try render(NewProjectView(state: SetupRows.newProject(step: 2, detail: SetupRows.detail())), CGSize(width: 1180, height: 1100),
                   "s17-new-project-review")
        try render(OnboardingView(state: SetupRows.onboarding()), CGSize(width: 900, height: 900), "s17-onboarding-narrow")
    }

    private func render(_ view: some View, _ size: CGSize, _ file: String) throws {
        guard let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"] else { return }
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            let content = Themed { view }
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: size.width, height: size.height)
            let renderer = ImageRenderer(content: content)
            renderer.scale = 2
            let image = try XCTUnwrap(renderer.nsImage)
            let bitmap = try XCTUnwrap(image.tiffRepresentation.flatMap(NSBitmapImageRep.init(data:)))
            let png = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
            try png.write(to: URL(fileURLWithPath: directory).appendingPathComponent("\(file)-\(name).png"))
        }
    }
}

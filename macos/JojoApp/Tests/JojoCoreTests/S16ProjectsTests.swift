import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

/// Step 6: the project "Crew" with two registered repositories, as `ProjectDetail`
/// (protocol 1.11) reads them, and three of its features.
enum ProjectRows {
    static let crew = DetailRows.id(3001), api = DetailRows.id(81), app = DetailRows.id(82)

    static let record = ProjectRecord(id: crew, revision: 4, name: "Crew", repositories: [api, app],
                                      instructions: "Kotlin only. Follow existing MVVM layering.\nNever edit Flyway migrations after merge.",
                                      testCommand: ["./gradlew", "test", "integrationTest"],
                                      kinds: [.init(kind: "backend", provider: "ClaudeCode"), .init(kind: "mobile", provider: "Codex")],
                                      defaultProvider: "ClaudeCode", capacity: 2)
    static let other = ProjectRecord(id: DetailRows.id(3002), revision: 1, name: "Jojo", repositories: [DetailRows.id(83)],
                                     instructions: "", testCommand: [], defaultProvider: "Codex", capacity: 2)
    static let repositories = [
        ChatIndex.Repository(id: api, path: "/Users/me/Code/crew/crew-api", pathComplete: true),
        ChatIndex.Repository(id: app, path: "/Users/me/Code/crew/crew-app", pathComplete: true),
    ]

    static func repository(_ id: String, name: String, branch: String?, head: String, worktrees: Int64, origin: String?,
                           kind: String?, issues: [String]) -> JSON {
        .object(["repository": .text(id), "name": .text(name), "path": .text("/Users/me/Code/crew/\(name)"),
                 "branch": branch.map(JSON.text) ?? .null, "head": .text(head), "origin": origin.map(JSON.text) ?? .null,
                 "origin_kind": kind.map(JSON.text) ?? .null, "worktrees": .int(worktrees), "active_worktrees": .int(worktrees > 1 ? 1 : 0),
                 "healthy": .bool(issues.isEmpty), "issues": .list(issues.map(JSON.text))])
    }

    static let detail = ProjectDetail([
        "project": .text(crew), "revision": .int(4), "name": .text("Crew"),
        "repositories": .list([
            repository(api, name: "crew-api", branch: "main", head: "9c1e4b2" + String(repeating: "0", count: 33), worktrees: 2,
                       origin: "https://github.com/kaanf/crew-api.git", kind: "Https", issues: []),
            repository(app, name: "crew-app", branch: "main", head: "41d07aa" + String(repeating: "0", count: 33), worktrees: 1,
                       origin: "git@github.com:kaanf/crew-app.git", kind: "Ssh", issues: ["origin is an SSH URL: Jojo pushes over HTTPS only"]),
        ]),
        "missing": .list([
            .object(["repository": .null, "text": .text("No CI command is recorded")]),
            .object(["repository": .text(app), "text": .text("crew-app: origin is an SSH URL: Jojo pushes over HTTPS only")]),
        ]),
    ])!

    static func feature(_ n: Int64, _ name: String, stage: Int, running: Int64 = 0, blocked: Int64 = 0, awaiting: Int64 = 0,
                        done: Bool = false, project: String = crew) -> FeatureSummary {
        let stages = (0..<5).map { i in FeatureStage(i < stage ? "Done" : i == stage ? "Current" : "Off") }
        return FeatureSummary(id: DetailRows.id(900 + Int(n)), number: n, name: name, projectName: project == crew ? "Crew" : "Jojo",
                              stages: stages, running: running, blocked: blocked, awaiting: awaiting, done: done, project: project)
    }

    static let features = [
        feature(142, "Lobby invite links", stage: 3, running: 2, blocked: 3),
        feature(145, "Leaderboard API", stage: 0, awaiting: 1),
        feature(139, "Push opt-in", stage: 4, done: true),
        feature(7, "Recovery inspector", stage: 3, project: DetailRows.id(3002)),
    ]

    static func state(detail: ProjectDetail? = detail, gh: ExternalSignIn? = ExternalSignIn(state: "Ready", detail: ""),
                      draft: ProjectDraft? = nil, reportsDetail: Bool = true, load: PlanLoad = .loaded,
                      connection: HelperConnection = .ready(epoch: 7)) -> ProjectsState {
        ProjectsState(connection: connection, list: ProjectList(projects: [record, other], capacity: 2), load: load,
                      repositories: repositories, chosen: crew, draft: draft, detail: detail, detailLoad: detail == nil ? .loading : .loaded,
                      reportsDetail: reportsDetail, features: features, admission: "Open", gh: gh, held: ["Crew": 1])
    }
}

@MainActor
final class S16ProjectsTests: XCTestCase {
    func test_s16_project_detail_is_read_at_1_12_for_the_shown_project_and_decoded() throws {
        // Step 7: at 1.12 each repository carries its brief.
        XCTAssertEqual(Request.ProjectDetail.minor, 12)
        XCTAssertEqual(Request.ProjectDetail.fields, ["project"])
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.refresh()
        h.fake.reply(h.fake.last("Snapshot")!, ["rows": .list([]), "as_of_event_seq": .int(30), "admission": .text("Open")])
        model.loadProjectDetail(ProjectRows.crew)
        let request = try XCTUnwrap(h.fake.last("ProjectDetail"))
        XCTAssertEqual(request.minor, 12)
        XCTAssertEqual(request.payload, ["project": .text(ProjectRows.crew)])
        h.fake.replyNull(request)
        XCTAssertEqual(model.projectDetailLoad, .failed("This core has no such project."))

        let detail = ProjectRows.detail
        XCTAssertEqual(detail.repositories.map(\.name), ["crew-api", "crew-app"])
        XCTAssertEqual(detail.repositories[0].branch, "main")
        XCTAssertEqual(detail.repositories[0].worktrees, 2)
        XCTAssertTrue(detail.repositories[0].healthy)
        XCTAssertEqual(detail.repositories[1].originKind, "Ssh")
        XCTAssertEqual(detail.missing.map(\.repository), [nil, ProjectRows.app])
    }

    func test_s16_a_project_lists_its_features_pills_and_what_it_lacks() {
        let state = ProjectsState.self
        let s = ProjectRows.state()
        XCTAssertEqual(s.features(of: ProjectRows.record).map(\.number), [142, 145, 139])
        XCTAssertEqual(s.line(ProjectRows.record), "2 repos · 3 features")
        XCTAssertEqual(s.line(ProjectRows.other), "1 repo · 1 feature")
        XCTAssertEqual(s.pills(ProjectRows.record).map(\.0), ["Admission open", "2 running", "3 blocked", "1 awaiting approval", "1 held"])
        XCTAssertEqual(state.stageLine(ProjectRows.features[0]).0, "Execution · 2 running · 3 blocked")
        XCTAssertEqual(state.stageLine(ProjectRows.features[1]).0, "Discovery · 1 awaiting approval")
        XCTAssertEqual(state.stageLine(ProjectRows.features[2]).1, "Done")
        XCTAssertEqual(s.missingConfiguration, ["No CI command is recorded", "crew-app: origin is an SSH URL: Jojo pushes over HTTPS only"])
        let signedOut = ProjectRows.state(gh: ExternalSignIn(state: "Missing", detail: "You are not logged into any GitHub hosts."))
        XCTAssertEqual(signedOut.missingConfiguration.last, "GitHub CLI: You are not logged into any GitHub hosts.")
        // A detail of another project is not shown for this one.
        var other = ProjectRows.state()
        other.chosen = ProjectRows.other.id
        XCTAssertNil(other.shownDetail)
        XCTAssertEqual(ProjectsState.missing.map(\.0).first, "Import Local Project, Attach Repository")
    }

    /// Review aid: Projects from its Mac board in its states.
    func test_s16_projects_renders_in_both_appearances() throws {
        let heading = ("Projects", "Workspace · Crew")
        let controls = AnyView(HeaderButtons(buttons: [HeaderButton(title: "Open in Finder"), HeaderButton(title: "Project Settings")]))
        let primary = AnyView(NewFeatureButton(newFeature: {}))
        let wide = CGSize(width: 1440, height: 1150), opening = CGSize(width: 1120, height: 1300)
        let cases: [(String, ProjectsState, CGSize)] = [
            ("s16-projects", ProjectRows.state(), wide),
            ("s16-projects-opening", ProjectRows.state(), opening),
            ("s16-projects-reading", ProjectRows.state(detail: nil), wide),
            ("s16-projects-older-core", ProjectRows.state(detail: nil, reportsDetail: false), wide),
            ("s16-projects-editing", ProjectRows.state(draft: ProjectDraft(ProjectRows.record)), wide),
            ("s16-projects-narrow", ProjectRows.state(), CGSize(width: 900, height: 2000)),
            ("s16-projects-disconnected", ProjectRows.state(connection: .channelLost("pipe closed")), wide),
        ]
        for (file, state, size) in cases {
            try DetailRows.render(ProjectsView(state: state), screen: .projects, heading: heading, controls: controls, primary: primary,
                                  size: size, file: file)
        }
    }
}

import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

/// Projects and registered repositories for New Feature, written for these tests.
enum NewFeatureRows {
    static let api = String(repeating: "a1", count: 16)
    static let app = String(repeating: "a2", count: 16)
    static let crew = ProjectRecord(id: String(repeating: "c1", count: 16), revision: 1, name: "Crew", repositories: [api, app],
                                    defaultProvider: "Codex", capacity: 2)
    static let jojo = ProjectRecord(id: String(repeating: "c2", count: 16), revision: 1, name: "Jojo", repositories: [api],
                                    defaultProvider: "ClaudeCode", capacity: 2)
    static let index = ChatIndex([
        "repositories": .list([.object(["repository": .text(api), "path": .text("/Users/someone/Code/crew-api"), "path_complete": .bool(true)]),
                               .object(["repository": .text(app), "path": .text("/Users/someone/Code/crew-app"), "path_complete": .bool(true)])]),
        "repositories_complete": .bool(true), "features": .list([]), "features_complete": .bool(true),
    ])

    static func state(projects: [ProjectRecord] = [crew, jojo], connection: HelperConnection = .ready(epoch: 1),
                      admission: String = "Open") -> NewFeatureState {
        NewFeatureState(connection: connection, admission: admission, queries: ["FeatureChat"],
                        projects: ProjectList(projects: projects, capacity: 2), projectsLoad: .loaded, index: index,
                        indexLoad: .loaded)
    }
}

@MainActor
final class S4NewFeatureTests: XCTestCase {
    func test_s4_the_project_names_the_repository_and_the_provider_and_jojo_decides_only_one() {
        var state = NewFeatureRows.state()
        XCTAssertEqual(state.chosenProject?.name, "Crew", "the first project is chosen")
        XCTAssertEqual(state.projectRepositories.map(\.id), [NewFeatureRows.api, NewFeatureRows.app])
        XCTAssertEqual(state.provider, "Codex", "the lead runs on the project's default provider")
        XCTAssertEqual(state.place, "Workspace · Crew")
        XCTAssertEqual(state.sendBlocked, "Describe what users should be able to do.")
        state.objective = "Players share a link that drops a friend into my lobby."
        XCTAssertNil(state.turnRepository, "with two repositories Jojo does not choose")
        XCTAssertEqual(state.sendBlocked, state.autoBlocked)
        state.repository = NewFeatureRows.app
        XCTAssertEqual(state.turnRepository, NewFeatureRows.app)
        XCTAssertNil(state.sendBlocked)

        // A project with one repository: Jojo decides it.
        state.project = NewFeatureRows.jojo.id
        state.repository = nil
        XCTAssertNil(state.autoBlocked)
        XCTAssertEqual(state.turnRepository, NewFeatureRows.api)
        XCTAssertEqual(state.provider, "ClaudeCode")
        // A repository of another project is not this turn's.
        state.repository = NewFeatureRows.app
        XCTAssertEqual(state.turnRepository, NewFeatureRows.api)
    }

    func test_s4_constraints_and_references_travel_in_the_message_within_the_core_limit() {
        var state = NewFeatureRows.state(projects: [NewFeatureRows.jojo])
        state.objective = "  Players share a link.\n"
        XCTAssertEqual(state.message, "Players share a link.")
        state.constraints = "No new infrastructure."
        state.references = [ReferenceFile(name: "ticket.md", text: "# FT-142\nInvite links expire.\n")]
        XCTAssertEqual(state.message, """
            Players share a link.

            ## Technical constraints

            No new infrastructure.

            ## Reference file: ticket.md

            # FT-142
            Invite links expire.
            """)
        XCTAssertNil(state.sendBlocked)
        state.references.append(ReferenceFile(name: "big.txt", text: String(repeating: "x", count: 17 * 1024)))
        XCTAssertEqual(state.sendBlocked, "The description, constraints and references are 17 KiB; the core takes at most 16 KiB.")
    }

    func test_s4_new_feature_says_why_it_cannot_start() {
        var none = NewFeatureRows.state(projects: [])
        none.objective = "Players share a link."
        XCTAssertEqual(none.sendBlocked, "Create a project first: the project names the repository and the provider.")
        XCTAssertEqual(NewFeatureRows.state(connection: .channelLost("gone")).sendBlocked, "Unavailable: the core is not connected.")
        XCTAssertEqual(NewFeatureRows.state(admission: "Closed").sendBlocked,
                       "Admission is closed. The core refuses a turn until it is open.")
        var old = NewFeatureRows.state()
        old.queries = ["Snapshot"]
        XCTAssertEqual(old.sendBlocked, "This core has no Feature Chat.")
        XCTAssertEqual(NewFeatureState.loadFailure("UnknownType"), "This core has no project records.")
    }

    /// Review aid: with JOJO_SNAPSHOT_DIR set, writes New Feature in its states as PNG files in
    /// both appearances. Without it, only checks that the views can be built.
    func test_s4_new_feature_renders_in_both_appearances() throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        var filled = NewFeatureRows.state()
        filled.objective = "Players should be able to share a link that drops a friend straight into my lobby."
        filled.repository = NewFeatureRows.api
        filled.constraints = "No new infrastructure."
        filled.references = [ReferenceFile(name: "FT-142-ticket.md", text: "# Invite links\nLinks expire after 7 days.")]
        filled.unreadable = ["mockup.png"]
        var refused = filled
        refused.outcome = .refused("CapacityOne: a run is active")
        var sent = filled
        sent.outcome = .sent
        var loading = NewFeatureRows.state()
        (loading.projects, loading.projectsLoad) = (nil, .loading)
        let cases: [(String, NewFeatureState, CGSize)] = [
            ("new-feature", NewFeatureRows.state(), CGSize(width: 1440, height: 1160)),
            ("new-feature-filled", filled, CGSize(width: 1440, height: 1240)),
            ("new-feature-narrow", filled, CGSize(width: 900, height: 1500)),
            ("new-feature-sent", sent, CGSize(width: 1440, height: 1240)),
            ("new-feature-refused", refused, CGSize(width: 1440, height: 1300)),
            ("new-feature-no-project", NewFeatureRows.state(projects: []), CGSize(width: 1440, height: 1160)),
            ("new-feature-loading", loading, CGSize(width: 1440, height: 1160)),
            ("new-feature-closed", NewFeatureRows.state(admission: "Closed"), CGSize(width: 1440, height: 1240)),
            ("new-feature-disconnected", NewFeatureRows.state(connection: .channelLost("pipe closed")), CGSize(width: 1440, height: 1240)),
        ]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            for (file, state, size) in cases {
                let shell = Themed {
                    AppShell(model: model, screen: .constant(.featureChat), showInspector: {},
                             heading: ("New Feature", state.place + " · New Feature")) {
                        NewFeatureView(state: state)
                    }
                }
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: size.width, height: size.height)
                guard let directory else { continue }
                try writeNewFeaturePNG(shell, to: directory, "\(file)-\(name).png")
            }
        }
    }
}

@MainActor private func writeNewFeaturePNG(_ view: some View, to directory: String, _ file: String) throws {
    let renderer = ImageRenderer(content: view)
    renderer.scale = 2
    let image = try XCTUnwrap(renderer.nsImage)
    let bitmap = try XCTUnwrap(image.tiffRepresentation.flatMap(NSBitmapImageRep.init(data:)))
    let png = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
    try png.write(to: URL(fileURLWithPath: directory).appendingPathComponent(file))
}

import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

/// A dated chat (protocol 1.8) with its feature's stages (1.7), written for these tests.
enum ChatV2Rows {
    static let t0 = OverviewRows.t0

    static func dated(_ turn: JSON, _ n: Int64) -> JSON {
        guard case .object(var row) = turn else { return turn }
        row["started_at_unix_ms"] = .int(t0 + n * 3_600_000)
        row["ended_at_unix_ms"] = .int(t0 + n * 3_600_000 + 90_000)
        return .object(row)
    }

    static let first = dated(ChatRows.turn(1, message: "Players should be able to share a link that drops a friend into my lobby.",
                                           summary: "I read `LobbyController.kt` and the lobby tests in crew-api.",
                                           questions: [ChatRows.question(1, prompt: "Should links expire?", blocking: true,
                                                                         answer: "After a week."),
                                                       ChatRows.question(2, prompt: "Track who invited whom?", blocking: false)]), 0)
    static let second = dated(ChatRows.turn(2, summary: "Drafted the requirements from the answers.", prd: 1), 1)

    static func page(prd: JSON, assumptions: [JSON]? = defaultAssumptions, turns: [JSON] = [first, second]) -> FeatureChatPage {
        var p = ChatRows.page(turns, prd: prd)
        if let assumptions { p["assumptions"] = .list(assumptions) }
        return FeatureChatPage(p)!
    }

    static let defaultAssumptions: [JSON] = [
        .object(["id": .text("A1"), "statement": .text("Deferred deep links work on both stores"), "state": .text("Current"),
                 "revision": .int(1)]),
        .object(["id": .text("A3"), "statement": .text("Lobby capacity is at most 8"), "state": .text("Dropped"),
                 "revision": .int(2)]),
    ]

    static let summary = FeatureSummary(id: ChatRows.feature, number: 3, name: "Lobby invite links", repositoryName: "crew-api",
                                        projectName: "Crew",
                                        stages: [FeatureStage("Done", "Done"), FeatureStage("NeedsYou", "PRD rev 1"), .off, .off, .off],
                                        running: 0, blocked: 0, awaiting: 1)

    static let crew = ProjectRecord(id: String(repeating: "c1", count: 16), revision: 1, name: "Crew",
                                    repositories: [ChatRows.repository], defaultProvider: "Codex", capacity: 2)

    static func state(prd: JSON = ChatRows.document(revision: 1, ready: true, approved: false), assumptions: [JSON]? = defaultAssumptions,
                      turns: [JSON] = [first, second], connection: HelperConnection = .ready(epoch: 7)) -> FeatureChatState {
        FeatureChatState(connection: connection, admission: "Open", queries: ["FeatureChat"], index: ChatIndex(ChatRows.index),
                         indexLoad: .loaded, feature: ChatRows.feature, chat: page(prd: prd, assumptions: assumptions, turns: turns),
                         chatLoad: .loaded,
                         prdText: "# Lobby invite links\n\n## Goal\nA friend joins a lobby from a link.",
                         summary: summary,
                         documents: DocumentList(documents: [DocumentSummary(id: ChatRows.prd, kind: "Prd", revision: 1,
                                                                            title: "Lobby invite links", feature: ChatRows.feature,
                                                                            approval: "Approved", decided: helperDate(t0))]),
                         projects: ProjectList(projects: [crew], capacity: 2))
    }
}

@MainActor
final class S5FeatureChatTests: XCTestCase {
    func test_s5_turns_and_assumptions_decode_at_1_8_and_an_older_core_leaves_them_out() throws {
        let state = ChatV2Rows.state()
        XCTAssertEqual(state.turns.first?.started, helperDate(ChatV2Rows.t0))
        XCTAssertEqual(state.turns.first?.ended, helperDate(ChatV2Rows.t0 + 90_000))
        XCTAssertEqual(state.chat?.assumptions?.map(\.state), ["Current", "Dropped"])
        let old = try XCTUnwrap(FeatureChatPage(ChatRows.page([ChatRows.turn(1)])))
        XCTAssertNil(old.assumptions)
        XCTAssertNil(old.turns[0].started)
        XCTAssertEqual(Request.FeatureChat.minor, 14)
    }

    func test_s5_the_chat_names_the_feature_dates_its_rounds_and_runs_on_the_project_provider() {
        var state = ChatV2Rows.state()
        let heading = FeatureChatState.heading(feature: ChatRows.feature, summary: state.summary, name: nil, turns: state.turns)
        XCTAssertEqual(heading.title, "FT-3 Lobby invite links")
        XCTAssertEqual(heading.subtitle, "Crew · Discovery rev 2")
        XCTAssertEqual(state.separator(state.turns[1]), "Discovery rev 2 · " + helperDate(ChatV2Rows.t0 + 3_600_000)!
            .formatted(.dateTime.month(.abbreviated).day()))
        XCTAssertEqual(state.provider, "Codex", "the project's default provider, not the last turn's")
        XCTAssertEqual(state.openQuestions.map(state.number), ["Q2"])
        XCTAssertEqual(state.resolvedQuestions.map(state.number), ["Q1"])
        XCTAssertEqual(state.decisions.map(\.kind), ["Prd"])
        XCTAssertFalse(state.canDraftPlan, "the requirements are not approved")

        // Attached files travel in the message; the core's limit is checked first.
        state.message = "Links should expire after a week."
        state.attachments = [ReferenceFile(name: "ticket.md", text: "# FT-3\nRevocable.\n")]
        XCTAssertEqual(state.composed, "Links should expire after a week.\n\n## Reference file: ticket.md\n\n# FT-3\nRevocable.")
        XCTAssertNil(state.sendBlocked("Discover"))
        state.attachments.append(ReferenceFile(name: "big.txt", text: String(repeating: "x", count: 17 * 1024)))
        XCTAssertEqual(state.sendBlocked("Discover"), "The note and attached files are 17 KiB; the core takes at most 16 KiB.")

        // Approved requirements without a plan: Draft Plan.
        let approved = ChatV2Rows.state(prd: ChatRows.document(revision: 1, ready: true, approved: true))
        XCTAssertTrue(approved.canDraftPlan)
        XCTAssertNil(approved.sendBlocked("Plan"))
        XCTAssertTrue(approved.transcript.hasPrefix("# Lobby invite links\n\n## Discovery rev 1"))
        XCTAssertTrue(approved.transcript.contains("- Q1: Should links expire? — After a week."))
    }

    /// Review aid: with JOJO_SNAPSHOT_DIR set, writes Feature Chat in its states as PNG files
    /// in both appearances. Without it, only checks that the views can be built.
    func test_s5_feature_chat_renders_in_both_appearances() throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        let blocking = ChatV2Rows.dated(ChatRows.turn(3, summary: "One question is still open. It only blocks the docs step.",
                                                      questions: [ChatRows.question(3, prompt: "Which shells must the usage cover?",
                                                                                    blocking: true, step: "docs")]), 2)
        let running = ChatV2Rows.dated(ChatRows.turn(3, state: "Running", message: "Links should expire after a week."), 2)
        var investigating = ChatV2Rows.state(turns: [ChatV2Rows.first, ChatV2Rows.second, running])
        investigating.chat = ChatV2Rows.page(prd: ChatRows.document(revision: 1, ready: true, approved: false),
                                            turns: [ChatV2Rows.first, ChatV2Rows.second, running])
        var refused = ChatV2Rows.state()
        refused.outcome = .refused("CapacityOne: a run is active")
        let cases: [(String, FeatureChatState, CGSize)] = [
            ("feature-chat", ChatV2Rows.state(), CGSize(width: 1440, height: 1260)),
            ("feature-chat-step-blocked", ChatV2Rows.state(turns: [ChatV2Rows.first, ChatV2Rows.second, blocking]),
             CGSize(width: 1440, height: 1400)),
            ("feature-chat-approved", ChatV2Rows.state(prd: ChatRows.document(revision: 1, ready: true, approved: true)),
             CGSize(width: 1440, height: 1260)),
            ("feature-chat-running", investigating, CGSize(width: 1440, height: 1260)),
            ("feature-chat-refused", refused, CGSize(width: 1440, height: 1300)),
            ("feature-chat-older-core", ChatV2Rows.state(assumptions: nil), CGSize(width: 1440, height: 1260)),
            ("feature-chat-narrow", ChatV2Rows.state(), CGSize(width: 900, height: 2000)),
            ("feature-chat-disconnected", ChatV2Rows.state(connection: .channelLost("pipe closed")), CGSize(width: 1440, height: 1300)),
        ]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            for (file, state, size) in cases {
                let heading = FeatureChatState.heading(feature: ChatRows.feature, summary: state.summary, name: nil, turns: state.turns)
                let shell = Themed {
                    AppShell(model: model, screen: .constant(.featureChat), showInspector: {},
                             controls: AnyView(ChatHeaderActions(export: {})), heading: heading) {
                        FeatureChatView(state: state)
                    }
                }
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: size.width, height: size.height)
                guard let directory else { continue }
                try writeChatPNG(shell, to: directory, "s5-\(file)-\(name).png")
            }
        }
    }
}

@MainActor private func writeChatPNG(_ view: some View, to directory: String, _ file: String) throws {
    let renderer = ImageRenderer(content: view)
    renderer.scale = 2
    let image = try XCTUnwrap(renderer.nsImage)
    let bitmap = try XCTUnwrap(image.tiffRepresentation.flatMap(NSBitmapImageRep.init(data:)))
    let png = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
    try png.write(to: URL(fileURLWithPath: directory).appendingPathComponent(file))
}

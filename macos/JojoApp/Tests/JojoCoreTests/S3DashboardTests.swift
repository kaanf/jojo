import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

/// Rows of the protocol 1.7 answers, written for these tests.
enum DashboardRows {
    static let t0 = OverviewRows.t0

    static func stage(_ state: String, _ label: String? = nil) -> JSON {
        .object(["state": .text(state), "label": label.map(JSON.text) ?? .null])
    }

    static func feature(_ n: Int, name: String, stages: [JSON], running: Int64 = 0, awaiting: Int64 = 0, done: Bool = false,
                        blockers: [JSON] = []) -> JSON {
        let names = FeatureSummary.stageNames
        return .object([
            "feature": .text(OverviewRows.id(60 + n)), "number": .int(Int64(n)), "name": .text(name),
            "repository": .text(OverviewRows.id(70)), "repository_name": .text("crew-api"),
            "project": .text(OverviewRows.id(71)), "project_name": .text("Crew"),
            "stages": .object(Dictionary(uniqueKeysWithValues: zip(names, stages))),
            "running": .int(running), "blocked": .int(Int64(blockers.count)), "awaiting": .int(awaiting), "done": .bool(done),
            "blockers": .list(blockers),
        ])
    }

    static let features: [String: JSON] = [
        "features": .list([
            feature(3, name: "Leaderboard API", stages: [stage("NeedsYou", "Q open"), stage("Off"), stage("Off"), stage("Off"), stage("Off")],
                    blockers: [.object(["kind": .text("Question"), "question": .text(OverviewRows.id(80)), "step": .null,
                                        "key": .null, "title": .null, "reason": .text("Rank by season or all-time?")])]),
            feature(2, name: "Lobby invite links",
                    stages: [stage("Done", "Done"), stage("Done", "Done"), stage("NeedsYou", "ADR rev 3"), stage("Running", "2 running"),
                             stage("Off")],
                    running: 2, awaiting: 1,
                    blockers: [.object(["kind": .text("Step"), "question": .null, "step": .text(OverviewRows.id(81)),
                                        "key": .text("T7"), "title": .text("Lobby join screen"),
                                        "reason": .text("A blocking question about this step is unanswered")])]),
            feature(1, name: "Push opt-in",
                    stages: [stage("Done", "Done"), stage("Done", "Done"), stage("Done", "Done"), stage("Done", "Done"), stage("Done", "Done")],
                    done: true),
        ]),
        "next": .null,
    ]

    static func attempt(_ n: Int, state: String, key: String, provider: String, validation: JSON = .null,
                        activity: String? = "Using a tool") -> JSON {
        guard case .object(var row) = OverviewRows.attempt(n, state: state, key: key, provider: provider) else { return .null }
        row["feature"] = .text(OverviewRows.id(62))
        row["ordinal"] = .int(2)
        row["repository"] = .text(OverviewRows.id(70))
        row["repository_name"] = .text("crew-api")
        row["project"] = .text(OverviewRows.id(71))
        row["project_name"] = .text("Crew")
        row["validation"] = validation
        row["activity"] = activity.map { .object(["text": .text($0), "at_unix_ms": .int(t0 + 30_000)]) } ?? .null
        return .object(row)
    }

    static let attempts: [String: JSON] = [
        "attempts": .list([
            attempt(4, state: "Running", key: "T4", provider: "ClaudeCode"),
            attempt(5, state: "Running", key: "T5", provider: "Codex", activity: nil),
            attempt(3, state: "Succeeded", key: "T2", provider: "Codex",
                    validation: .object(["state": .text("Failed"), "at_unix_ms": .int(t0 + 20_000), "decision": .null])),
            attempt(2, state: "Succeeded", key: "T1", provider: "ClaudeCode",
                    validation: .object(["state": .text("Passed"), "at_unix_ms": .int(t0 - 3 * 86_400_000), "decision": .text("Accepted")])),
        ]),
        "next": .null,
    ]

    static func event(_ seq: Int64, _ type: String, _ summary: String, at: Int64) -> JournalEvent {
        JournalEvent(.object(["seq": .int(seq), "epoch": .int(1), "type": .text(type), "payload": .text(""),
                              "detail_complete": .bool(true), "at_unix_ms": .int(at), "actor": .text("Core"),
                              "summary": .text(summary)]))!
    }

    static let events = [
        event(1, "EngineStarted", "Session 1 started", at: t0 - 86_400_000),
        event(2, "ProviderOutput", "Provider output · T4 · attempt 2", at: t0),
        event(3, "ProcessLaunchIssued", "T4 · attempt 2 started on Claude Code", at: t0 + 1_000),
        event(4, "ValidationEnded", "Host validation of T2 · attempt 2 failed", at: t0 + 20_000),
    ]

    static let executables = ExecutableList(providers: [
        ProviderExecutable(provider: "ClaudeCode", source: .discovered, state: .ready, version: "2.1.311"),
        ProviderExecutable(provider: "Codex", source: .none, state: .unconfigured),
    ])

    static func state(connection: HelperConnection = .ready(epoch: 1), admission: String = "Open",
                      range: DashboardRange = .today) -> DashboardState {
        DashboardState(connection: connection, admission: admission, documents: DocumentList(OverviewRows.documents),
                       recovery: [RecoveryRow(attempt: OverviewRows.id(4), assessment: "", findings: "", detailComplete: true,
                                              structured: RecoveryAssessment(row: .object([
                                                  "blockers": .list([.text("Hold only until the tree is reinspected")]),
                                                  "blockers_complete": .bool(true), "process_absent": .bool(false),
                                                  "choices": .list([.text("Hold")])])))],
                       attempts: AttemptList(attempts), attemptsLoad: .loaded,
                       features: FeatureList(features), featuresLoad: .loaded,
                       executables: executables, events: events, eventsLoad: .loaded, range: range,
                       now: helperDate(t0 + 12 * 60_000)!)
    }
}

@MainActor
final class S3DashboardTests: XCTestCase {
    private func ready() -> (Harness, WorkspaceModel) {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.refresh()
        h.fake.reply(h.fake.last("Snapshot")!, ["rows": .list([]), "as_of_event_seq": .int(30), "admission": .text("Open")])
        return (h, model)
    }

    func test_s3_the_features_query_is_sent_at_1_7_only_while_the_dashboard_is_shown() throws {
        XCTAssertEqual(Request.allCases.filter { $0.minor == 7 }, [.Attempts, .Features])
        XCTAssertEqual(Request.Features.fields, ["before"])
        XCTAssertEqual(Request.attributedEvents, 7)
        let (h, model) = ready()
        model.loadFeatures()
        model.loadAttempts()
        model.loadExecutables()
        XCTAssertEqual(h.fake.count("Features") + h.fake.count("Attempts") + h.fake.count("Executables"), 0,
                       "nothing of the Dashboard is read while another screen is shown")
        model.dashboardShown = true
        model.loadFeatures()
        model.loadAttempts()
        model.loadExecutables()
        XCTAssertEqual(h.fake.last("Features")?.minor, 7)
        XCTAssertEqual(h.fake.last("Features")?.payload, ["before": .null])
        XCTAssertEqual(h.fake.last("Attempts")?.minor, 7)
        XCTAssertEqual(h.fake.last("Executables")?.minor, 4)

        h.fake.reply(h.fake.last("Features")!, DashboardRows.features)
        let list = try XCTUnwrap(model.features)
        XCTAssertEqual(list.features.map(\.code), ["FT-3", "FT-2", "FT-1"])
        XCTAssertEqual(list.features[1].stages.map(\.state), ["Done", "Done", "NeedsYou", "Running", "Off"])
        XCTAssertEqual(list.features[1].stages[2].label, "ADR rev 3")
        XCTAssertEqual(list.features[0].blockers.first?.kind, "Question")
        XCTAssertTrue(list.features[2].done)

        // A core without the query answers UnknownType: the table says so.
        model.loadFeatures()
        h.fake.error(h.fake.last("Features")!, "UnknownType")
        XCTAssertNil(model.features)
        XCTAssertEqual(model.featuresLoad, .failed("UnknownType"))
        XCTAssertEqual(DashboardState.loadFailure("UnknownType"), "This core does not report features (protocol 1.7).")
    }

    func test_s3_attempt_details_and_event_sentences_decode_and_an_older_core_leaves_them_out() throws {
        let list = try XCTUnwrap(AttemptList(DashboardRows.attempts))
        let first = list.attempts[0]
        XCTAssertEqual(first.ordinal, 2)
        XCTAssertEqual(first.projectName, "Crew")
        XCTAssertEqual(first.repositoryName, "crew-api")
        XCTAssertEqual(first.activity?.text, "Using a tool")
        XCTAssertNil(first.validation)
        XCTAssertEqual(list.attempts[3].validation, AttemptValidation(state: "Passed", at: helperDate(OverviewRows.t0 - 3 * 86_400_000),
                                                                       decision: "Accepted"))
        XCTAssertEqual(DashboardRows.events[2].summary, "T4 · attempt 2 started on Claude Code")
        // A 1.6 row: no details, no sentence.
        let old = try XCTUnwrap(AttemptSummary(OverviewRows.attempt(1, state: "Running")))
        XCTAssertNil(old.ordinal)
        XCTAssertNil(old.activity)
        let oldEvent = try XCTUnwrap(JournalEvent(.object(["seq": .int(1), "epoch": .int(1), "type": .text("EngineStarted"),
                                                            "payload": .text(""), "detail_complete": .bool(true)])))
        XCTAssertNil(oldEvent.summary)
    }

    func test_s3_the_dashboard_derives_what_the_board_shows_from_the_answers() {
        let state = DashboardRows.state()
        XCTAssertEqual(state.approvals.map(\.title), ["Execution plan rev 1", "usage · Describe the usage"])
        XCTAssertEqual(state.approvals.map(\.pill), ["Proposed", "Needed"])
        XCTAssertEqual(state.approvals[0].detail, "Execution plan · Slugify from the shell")
        XCTAssertEqual(state.recoveries.map(\.pill), ["Held"])
        XCTAssertEqual(state.recoveries[0].title, "T4 · Add the entry point", "a held attempt is named by its task")
        XCTAssertEqual(state.needs.count, 3)
        XCTAssertEqual(state.running.map(\.key), ["T4", "T5"])
        XCTAssertEqual(state.runningSummary, "T4 on Claude Code · T5 on Codex")
        XCTAssertEqual(DashboardState.place(state.running[0]), "Crew · crew-api · Claude Code · Attempt 2")
        XCTAssertEqual(DashboardState.elapsed(helperDate(OverviewRows.t0), now: state.now), "12m")
        XCTAssertEqual(DashboardState.elapsed(helperDate(OverviewRows.t0), now: helperDate(OverviewRows.t0 + 65 * 60_000)!), "1h 5m")
        XCTAssertEqual(state.blockedCount, 2)
        XCTAssertEqual(state.blocked.map(\.title), ["FT-3 · Leaderboard API", "T7 · Lobby join screen"])
        XCTAssertEqual(state.blockedSummary, "FT-3 and T7")
        XCTAssertTrue(state.blocked[0].question)
        XCTAssertEqual(state.available, 1)
        XCTAssertEqual(state.providersSummary, "Codex not found")
        XCTAssertEqual(state.activeRuns("ClaudeCode"), 1)

        // Today: the validation of three days ago and yesterday's session are left out.
        XCTAssertEqual(state.verifications.map(\.key), ["T2"])
        XCTAssertEqual(state.history.map(\.text), ["Host validation of T2 · attempt 2 failed", "T4 · attempt 2 started on Claude Code"],
                       "newest first; provider output is not history")
        let all = DashboardRows.state(range: .all)
        XCTAssertEqual(all.verifications.map(\.key), ["T2", "T1"])
        XCTAssertEqual(all.history.count, 3)
        XCTAssertEqual(DashboardState.validationWords(all.verifications[1].validation!), "host validation passed · accepted")

        let features = state.features!.features
        XCTAssertEqual(features.filter(FeatureFilter.active.contains).map(\.code), ["FT-3", "FT-2"])
        XCTAssertEqual(features.filter(FeatureFilter.needsYou.contains).map(\.code), ["FT-3", "FT-2"])
        XCTAssertEqual(features.filter(FeatureFilter.done.contains).map(\.code), ["FT-1"])
        XCTAssertFalse(state.empty)
        XCTAssertFalse(state.admissionClosed)
        XCTAssertTrue(DashboardRows.state(admission: "ClosedUntilReviewedStart").admissionClosed)
    }

    func test_s3_an_empty_workspace_is_the_empty_state_and_the_dashboard_opens_first() {
        let empty = DashboardState(connection: .ready(epoch: 1), admission: "Open", documents: DocumentList(documents: []),
                                   attempts: AttemptList(attempts: []), attemptsLoad: .loaded,
                                   features: FeatureList(features: []), featuresLoad: .loaded)
        XCTAssertTrue(empty.empty)
        XCTAssertTrue(MainView.built.contains(.dashboard))
        XCTAssertEqual(DashboardRange.allCases.map(\.rawValue), ["Today", "Week", "All"])
    }

    /// Review aid: with JOJO_SNAPSHOT_DIR set, writes the Dashboard in its states as PNG files
    /// in both appearances. Without it, only checks that the views can be built.
    func test_s3_dashboard_renders_in_both_appearances() throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        var loading = DashboardState(connection: .ready(epoch: 1), admission: "Open")
        loading.featuresLoad = .loading
        loading.attemptsLoad = .loading
        loading.eventsLoad = .loading
        let empty = DashboardState(connection: .ready(epoch: 1), admission: "Open", documents: DocumentList(documents: []),
                                   attempts: AttemptList(attempts: []), attemptsLoad: .loaded,
                                   features: FeatureList(features: []), featuresLoad: .loaded)
        var unreadable = DashboardRows.state()
        (unreadable.features, unreadable.featuresLoad) = (nil, .failed("UnknownType"))
        let cases: [(String, DashboardState, CGSize)] = [
            ("dashboard", DashboardRows.state(), CGSize(width: 1440, height: 1240)),
            ("dashboard-narrow", DashboardRows.state(), CGSize(width: 1000, height: 1900)),
            ("dashboard-all", DashboardRows.state(range: .all), CGSize(width: 1440, height: 1240)),
            ("dashboard-empty", empty, CGSize(width: 1120, height: 720)),
            ("dashboard-loading", loading, CGSize(width: 1120, height: 1100)),
            ("dashboard-unreadable", unreadable, CGSize(width: 1440, height: 1240)),
            ("dashboard-closed", DashboardRows.state(admission: "ClosedUntilReviewedStart"), CGSize(width: 1440, height: 1300)),
            ("dashboard-disconnected", DashboardRows.state(connection: .channelLost("pipe closed")), CGSize(width: 1440, height: 1300)),
        ]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            for (file, state, size) in cases {
                let shell = Themed {
                    AppShell(model: model, screen: .constant(.dashboard), showInspector: {},
                             controls: AnyView(SegmentedCapsule(selection: .constant(state.range), label: "Time range")),
                             primary: AnyView(NewFeatureButton(newFeature: {}))) {
                        DashboardView(state: state)
                    }
                }
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: size.width, height: size.height)
                guard let directory else { continue }
                try writePNG(shell, to: directory, "\(file)-\(name).png")
            }
        }
    }
}

@MainActor private func writePNG(_ view: some View, to directory: String, _ file: String) throws {
    let renderer = ImageRenderer(content: view)
    renderer.scale = 2
    let image = try XCTUnwrap(renderer.nsImage)
    let bitmap = try XCTUnwrap(image.tiffRepresentation.flatMap(NSBitmapImageRep.init(data:)))
    let png = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
    try png.write(to: URL(fileURLWithPath: directory).appendingPathComponent(file))
}

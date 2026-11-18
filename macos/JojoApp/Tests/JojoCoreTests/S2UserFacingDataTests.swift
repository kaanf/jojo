import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

/// Rows of the protocol 1.6 answers, written for these tests.
enum OverviewRows {
    static let feature = String(repeating: "f0", count: 16)
    static func id(_ n: Int) -> String { String(repeating: String(format: "%02x", n), count: 16) }
    /// 2026-11-17 10:00:00 UTC.
    static let t0: Int64 = 1_794_909_600_000

    static func attempt(_ n: Int, state: String, title: String? = "Add the entry point", key: String? = "api",
                        provider: String? = "ClaudeCode", started: Int64? = t0, ended: Int64? = nil) -> JSON {
        .object(["order": .int(Int64(n)), "attempt": .text(id(n)), "step": .text(id(40 + n)), "key": key.map(JSON.text) ?? .null,
                 "title": title.map(JSON.text) ?? .null, "plan": .text(id(30)), "feature": .text(feature),
                 "feature_name": .text("Slugify from the shell"), "provider": provider.map(JSON.text) ?? .null,
                 "state": .text(state), "started_at_unix_ms": started.map(JSON.int) ?? .null,
                 "ended_at_unix_ms": ended.map(JSON.int) ?? .null])
    }

    static func document(_ n: Int, kind: String, approval: String, title: String? = nil, stale: Bool = false) -> JSON {
        .object(["document": .text(id(n)), "kind": .text(kind), "revision": .int(1), "title": title.map(JSON.text) ?? .null,
                 "feature": .text(feature), "feature_name": .text("Slugify from the shell"),
                 "author": kind == "Prd" ? .text("Model") : .null, "approval": .text(approval), "stale": .bool(stale),
                 "created_at_unix_ms": .int(t0), "decided_at_unix_ms": approval == "Approved" ? .int(t0 + 60_000) : .null,
                 "order": .int(Int64(100 + n))])
    }

    static let documents: [String: JSON] = [
        "documents": .list([document(3, kind: "ExecutionPlan", approval: "Pending"),
                            document(2, kind: "Adr", approval: "Approved"),
                            document(1, kind: "Prd", approval: "Approved", title: "Slugify from the shell")]),
        "next": .null,
        "candidates": .list([.object(["attempt": .text(id(9)), "key": .text("usage"), "title": .text("Describe the usage"),
                                      "feature": .text(feature), "feature_name": .text("Slugify from the shell"),
                                      "receipt": .int(91), "validated_at_unix_ms": .int(t0)])]),
        "pending": .int(2),
    ]
}

@MainActor
final class S2UserFacingDataTests: XCTestCase {
    private func ready() -> (Harness, WorkspaceModel) {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.refresh()
        h.fake.reply(h.fake.last("Snapshot")!, ["rows": .list([]), "as_of_event_seq": .int(30), "admission": .text("Open")])
        return (h, model)
    }

    func test_s2_the_new_queries_and_the_attributed_events_are_sent_at_1_6_and_nothing_else_moves() {
        // Since 1.7 attempts and attributed events are read at 7, where they carry more; since
        // 1.10 (M5) the documents at 10, where they list the external requests that wait.
        XCTAssertEqual(Request.allCases.filter { $0.minor == 6 }, [.ChatIndex])
        XCTAssertEqual(Request.Attempts.fields, ["before"])
        let (h, model) = ready()
        XCTAssertEqual(h.fake.last("Snapshot")?.minor, 1)
        XCTAssertEqual(h.fake.last("Documents")?.minor, 10, "every snapshot reads the documents for the sidebar counter")
        XCTAssertEqual(h.fake.last("Documents")?.payload, ["before": .null])
        model.pollEvents()
        XCTAssertEqual(h.fake.last("Events")?.minor, 1, "polling keeps its 1.1 frame")
        model.loadEvents()
        XCTAssertEqual(h.fake.last("Events")?.minor, 11, "Activity & Audit reads events with time, actor, sentence, category and project")
        XCTAssertEqual(h.fake.count("Attempts"), 0, "attempts are read only while Agent Runs is shown")
        model.repairsShown = true
        model.loadAttempts()
        XCTAssertEqual(h.fake.last("Attempts")?.minor, 7)
    }

    func test_s2_documents_decode_with_the_pending_count_and_an_older_core_leaves_no_counter() {
        let (h, model) = ready()
        h.fake.reply(h.fake.last("Documents")!, OverviewRows.documents)
        XCTAssertEqual(model.documentsLoad, .loaded)
        let list = try! XCTUnwrap(model.documents)
        XCTAssertEqual(list.pending, 2)
        XCTAssertEqual(list.waiting.map(\.kind), ["ExecutionPlan"])
        XCTAssertEqual(list.candidates.first?.title, "Describe the usage")
        XCTAssertEqual(list[OverviewRows.id(1)]?.name, "Slugify from the shell")
        XCTAssertEqual(list[OverviewRows.id(3)]?.name, "Slugify from the shell · Execution plan")
        XCTAssertEqual(list[OverviewRows.id(2)]?.decided, helperDate(OverviewRows.t0 + 60_000))

        // The next snapshot reads them again; a core without the query answers UnknownType.
        model.refresh()
        h.fake.reply(h.fake.last("Snapshot")!, ["rows": .list([]), "as_of_event_seq": .int(31), "admission": .text("Open")])
        h.fake.error(h.fake.last("Documents")!, "UnknownType")
        XCTAssertNil(model.documents)
        XCTAssertEqual(model.documentsLoad, .failed("UnknownType"))
    }

    func test_s2_activity_groups_by_day_and_names_the_actor() throws {
        let row: (Int64, Int64, String) -> JSON = { seq, at, actor in
            .object(["seq": .int(seq), "epoch": .int(2), "type": .text("PlanRunStarted"), "aggregate_kind": .null,
                     "aggregate_id": .null, "revision": .null, "payload": .text("x"), "detail_complete": .bool(true),
                     "at_unix_ms": .int(at), "actor": .text(actor)])
        }
        let day: Int64 = 86_400_000
        let events = try [row(1, OverviewRows.t0 - day, "Core"), row(2, OverviewRows.t0, "Human"), row(3, OverviewRows.t0, "Codex")]
            .map { try XCTUnwrap(JournalEvent($0)) }
        let state = ActivityState(connection: .ready(epoch: 1), admission: "Open", events: events, load: .loaded,
                                  now: helperDate(OverviewRows.t0 + 1000)!)
        XCTAssertTrue(state.attributed)
        XCTAssertEqual(state.groups.map(\.title), ["Today", "Yesterday"])
        XCTAssertEqual(state.shown.map { ActivityState.actor($0) }, ["Codex", "Human", "Jojo core"])
        XCTAssertEqual(ActivityState(connection: .ready(epoch: 1), admission: "Open", events: events, load: .loaded, query: "jojo core").shown.map(\.seq),
                       [1], "search covers the actor")
        let old = try XCTUnwrap(JournalEvent(.object(["seq": .int(4), "epoch": .int(3), "type": .text("T"), "payload": .text(""),
                                                      "detail_complete": .bool(true)])))
        XCTAssertNil(old.at)
        XCTAssertEqual(ActivityState(connection: .ready(epoch: 1), admission: "Open", events: [old], load: .loaded).groups.map(\.title), ["Epoch 3"])
    }

    /// Review aid: with JOJO_SNAPSHOT_DIR set, writes Activity & Audit with the 1.6 data in
    /// both appearances (Agent Runs and the Approval Inbox are rendered by their step 4 tests). The rows are test data.
    func test_s2_screens_with_reported_data_render_in_both_appearances() throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        let now = helperDate(OverviewRows.t0 + 90_000)!
        let events = [("Core", "EngineStarted"), ("Human", "PlanRunStarted"), ("ClaudeCode", "ProviderOutput")].enumerated()
            .compactMap { index, pair in
                JournalEvent(.object(["seq": .int(Int64(index + 1)), "epoch": .int(2), "type": .text(pair.1), "aggregate_kind": .null,
                                      "aggregate_id": .null, "revision": .null, "payload": .text("x"), "detail_complete": .bool(true),
                                      "at_unix_ms": .int(OverviewRows.t0 + Int64(index) * 1000), "actor": .text(pair.0)]))
            }
        let activity = ActivityState(connection: .ready(epoch: 1), admission: "Open", events: events, load: .loaded, now: now)
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            let screens: [(String, Screen, AnyView)] = [
                ("s2-activity", .activity, AnyView(ActivityView(state: activity))),
            ]
            for (file, screen, view) in screens {
                let shell = Themed {
                    AppShell(model: model, screen: .constant(screen), showInspector: {}) { view }
                }
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: 1120, height: 720)
                guard let directory else { continue }
                let renderer = ImageRenderer(content: shell)
                renderer.scale = 2
                let image = try XCTUnwrap(renderer.nsImage)
                let bitmap = try XCTUnwrap(image.tiffRepresentation.flatMap(NSBitmapImageRep.init(data:)))
                let png = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
                try png.write(to: URL(fileURLWithPath: directory).appendingPathComponent("\(file)-\(name).png"))
            }
        }
    }
}

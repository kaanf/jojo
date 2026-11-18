import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

/// Step 6: events as a 1.11 core reports them, with time, actor, sentence, category and
/// project, over two days.
enum ActivityRows {
    static let now: Int64 = 1_763_400_000_000
    static let sample = DetailRows.id(3001), crew = DetailRows.id(3002)

    static func event(_ seq: Int64, minutesAgo: Int64, actor: String, type: String, category: String, summary: String,
                      project: String? = nil, name: String? = nil, aggregate: String? = nil, payload: String = "",
                      complete: Bool = true) -> JournalEvent {
        JournalEvent(.object([
            "seq": .int(seq), "epoch": .int(7), "type": .text(type), "aggregate_kind": aggregate == nil ? .null : .text("artifact"),
            "aggregate_id": aggregate.map(JSON.text) ?? .null, "revision": .null, "payload": .text(payload),
            "detail_complete": .bool(complete), "at_unix_ms": .int(now - minutesAgo * 60_000), "actor": .text(actor),
            "summary": .text(summary), "category": .text(category), "project": project.map(JSON.text) ?? .null,
            "project_name": name.map(JSON.text) ?? .null,
        ]))!
    }

    static var events: [JournalEvent] {
        [
            event(80, minutesAgo: 26 * 60, actor: "Codex", type: "AttemptResultSealed", category: "Attempt",
                  summary: "T2 · attempt 2 succeeded", project: crew, name: "Crew"),
            event(81, minutesAgo: 25 * 60, actor: "Core", type: "ValidationEnded", category: "Validation",
                  summary: "Host validation of T2 · attempt 2 passed", project: crew, name: "Crew"),
            event(82, minutesAgo: 60, actor: "Core", type: "EngineStopped", category: "Error", summary: "Session stopped",
                  payload: "shutdown=UNCLEAN"),
            event(83, minutesAgo: 50, actor: "Human", type: "RecoveryStarted", category: "Recovery",
                  summary: "Admission reopened after the recovery review"),
            event(84, minutesAgo: 30, actor: "Human", type: "HumanDecision", category: "Approval",
                  summary: "Slugify from the shell rev 3 approved", project: sample, name: "Sample", aggregate: DetailRows.id(9)),
            event(85, minutesAgo: 20, actor: "Core", type: "PlanStepIntegrated", category: "Git",
                  summary: "T4 · Inspector layout integrated onto the plan branch", project: sample, name: "Sample",
                  payload: String(repeating: "x|y ", count: 200), complete: false),
            event(86, minutesAgo: 5, actor: "Human", type: "ExternalActionAuthorized", category: "Approval",
                  summary: "Push of jojo/plan/40 authorized", project: sample, name: "Sample"),
        ]
    }

    static func state(query: String = "", category: String? = nil, project: String? = nil, range: ActivityRange = .all,
                      selected: Int64? = nil, payload: EventPayload? = nil,
                      connection: HelperConnection = .ready(epoch: 7)) -> ActivityState {
        ActivityState(connection: connection, admission: "Open", head: 86, events: events, load: .loaded, query: query,
                      category: category, project: project, range: range, selected: selected, now: helperDate(now)!,
                      payload: payload, payloadLoad: payload == nil ? .idle : .loaded)
    }
}

@MainActor
final class S14ActivityTests: XCTestCase {
    func test_s14_events_carry_category_and_project_at_1_11_and_the_rest_of_a_payload_is_read_on_request() throws {
        XCTAssertEqual(Request.attributedEvents, 11)
        XCTAssertEqual(Request.EventDetail.minor, 1, "EventDetail exists since 1.0")
        XCTAssertEqual(Request.EventDetail.fields, ["seq", "offset"])
        let e = ActivityRows.events[4]
        XCTAssertEqual(e.category, "Approval")
        XCTAssertEqual(e.project, ActivityRows.sample)
        XCTAssertEqual(e.projectName, "Sample")
        XCTAssertNil(ActivityRows.events[2].project)

        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.refresh()
        h.fake.reply(h.fake.last("Snapshot")!, ["rows": .list([]), "as_of_event_seq": .int(86), "admission": .text("Open")])
        XCTAssertEqual(h.fake.count("EventDetail"), 0)
        model.loadEventPayload(85)
        let first = try XCTUnwrap(h.fake.last("EventDetail"))
        XCTAssertEqual(first.payload, ["seq": .int(85), "offset": .int(0)])
        h.fake.reply(first, ["seq": .int(85), "next": .int(2048), "total": .int(2100), "text": .text(String(repeating: "a", count: 2048))])
        let second = try XCTUnwrap(h.fake.last("EventDetail"))
        XCTAssertEqual(second.payload, ["seq": .int(85), "offset": .int(2048)], "the next page starts where the last ended")
        XCTAssertEqual(model.eventPayloadLoad, .loading)
        h.fake.reply(second, ["seq": .int(85), "next": .int(2100), "total": .int(2100), "text": .text(String(repeating: "b", count: 52))])
        XCTAssertEqual(model.eventPayloadLoad, .loaded)
        XCTAssertEqual(model.eventPayload?.text.count, 2100)
        XCTAssertEqual(model.eventPayload?.complete, true)
        model.loadEventPayload(99)
        h.fake.replyNull(try XCTUnwrap(h.fake.last("EventDetail")))
        XCTAssertEqual(model.eventPayloadLoad, .failed("This core has no such event."))
    }

    func test_s14_category_project_and_range_filter_the_loaded_events() {
        let all = ActivityRows.state()
        XCTAssertTrue(all.categorized)
        XCTAssertEqual(all.categories, ["Approval", "Attempt", "Recovery", "Git", "Validation", "Error"], "in the board's order")
        XCTAssertEqual(all.projects.map(\.name), ["Crew", "Sample"])
        XCTAssertEqual(all.shown.map(\.seq), Array((80...86).reversed()))
        XCTAssertEqual(all.groups.map(\.title), ["Today", "Yesterday"])

        XCTAssertEqual(ActivityRows.state(category: "Approval").shown.map(\.seq), [86, 84])
        XCTAssertEqual(ActivityRows.state(project: ActivityRows.crew).shown.map(\.seq), [81, 80])
        XCTAssertEqual(ActivityRows.state(project: ActivityRows.sample).projectName, "Sample")
        XCTAssertEqual(ActivityRows.state(range: .day).shown.map(\.seq), [86, 85, 84, 83, 82], "the last 24 hours")
        XCTAssertEqual(ActivityRows.state(range: .week).shown.count, 7)
        XCTAssertEqual(ActivityRows.state(query: "inspector").shown.map(\.seq), [85], "the sentence is searched")
        XCTAssertEqual(ActivityRows.state(query: "crew").shown.map(\.seq), [81, 80], "and the project")
        XCTAssertEqual(ActivityRows.state(query: "error").shown.map(\.seq), [82], "and the category")
        XCTAssertEqual(ActivityRows.state(category: "Git", selected: 80).chosen?.seq, 85, "a filtered-out event is not the chosen one")
        XCTAssertEqual(ActivityState.actor(ActivityRows.events[3]), "You")
        XCTAssertEqual(ActivityState.tone("Error"), .danger)
        XCTAssertEqual(ActivityState.tone("Approval"), .review)
    }

    func test_s14_export_writes_the_shown_events_as_markdown_and_says_no_chain_was_verified() {
        let state = ActivityRows.state(project: ActivityRows.sample)
        let lines = state.markdown.split(separator: "\n").map(String.init)
        XCTAssertEqual(lines.first, "# Jojo activity and audit")
        XCTAssertTrue(lines.contains { $0.contains("Project: Sample.") })
        XCTAssertTrue(state.markdown.contains("no hash chain was verified"))
        let rows = lines.filter { $0.hasPrefix("| ") && !$0.hasPrefix("| Time") }
        XCTAssertEqual(rows.count, 3)
        XCTAssertTrue(rows[0].contains("| You | Approval | Push of jojo/plan/40 authorized |"))
        XCTAssertTrue(rows.allSatisfy { $0.hasSuffix(" |") })
        // The payload is not exported; a "|" in a sentence would be escaped.
        XCTAssertFalse(state.markdown.contains("x|y"))

        let long = ActivityRows.events[5]
        XCTAssertEqual(ActivityRows.state().payloadText(long), long.payload, "the excerpt until the whole payload is read")
        let full = EventPayload(seq: 85, text: String(repeating: "z", count: 900), total: 900)
        XCTAssertEqual(ActivityRows.state(payload: full).payloadText(long).count, 900)
    }

    /// Review aid: Activity & Audit from its Mac board in its states.
    func test_s14_activity_renders_in_both_appearances() throws {
        let heading = ("Activity & Audit", "Workspace · append-only history")
        let controls = AnyView(HeaderButtons(buttons: [HeaderButton(title: "Export")]))
        let wide = CGSize(width: 1440, height: 1000), opening = CGSize(width: 1120, height: 800)
        let full = EventPayload(seq: 85, text: String(repeating: "x|y ", count: 300), total: 1200)
        let cases: [(String, ActivityState, CGSize)] = [
            ("s14-activity", ActivityRows.state(), wide),
            ("s14-activity-opening", ActivityRows.state(), opening),
            ("s14-activity-approval", ActivityRows.state(category: "Approval", selected: 84), wide),
            ("s14-activity-project-day", ActivityRows.state(project: ActivityRows.sample, range: .day, selected: 85), wide),
            ("s14-activity-payload", ActivityRows.state(selected: 85, payload: full), wide),
            ("s14-activity-unmatched", ActivityRows.state(query: "nothing like this"), wide),
            ("s14-activity-narrow", ActivityRows.state(selected: 85), CGSize(width: 860, height: 1700)),
            ("s14-activity-disconnected", ActivityRows.state(connection: .channelLost("pipe closed")), wide),
        ]
        for (file, state, size) in cases {
            try DetailRows.render(ActivityView(state: state), screen: .activity, heading: heading, controls: controls, size: size, file: file)
        }
    }
}

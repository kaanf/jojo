import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

/// M6: Execution Plan follows a running step's log, decides a step that waits for the
/// human's decision and starts again a step that waits for the human (protocol 1.13).
enum RetryRows {
    static let held = DetailRows.id(51)

    /// The plan of `PlanRows`, its first step held for the human with the core's retries.
    static func detail(actions: [String] = ["Repair", "Restart"], state: String = "NeedsHuman",
                       retried: String? = nil) -> [String: JSON] {
        var detail = PlanRows.detail(run: "HALTED")
        guard case .list(var steps)? = detail["steps"], case .object(var first) = steps[0] else { return detail }
        first["state"] = .text(state)
        first["detail"] = .text("host validation failed; 2 of 2 repairs used")
        first["attempts"] = .list([PlanRows.attempt(51, outcome: "Succeeded", validation: "Failed")])
        first["blockers"] = .list([.object(["kind": .text("Step"), "reason": .text("host validation failed; 2 of 2 repairs used")])])
        first["retry"] = .object(["actions": .list(actions.map(JSON.text)), "attempt": .text(held)])
        first["retried"] = retried.map(JSON.text) ?? .null
        steps[0] = .object(first)
        detail["steps"] = .list(steps)
        return detail
    }
}

@MainActor
final class S18PlanRetryTests: XCTestCase {
    private func state(_ detail: [String: JSON] = RetryRows.detail(), selected: PlanNode? = .step(DetailRows.id(41)),
                       connection: HelperConnection = .ready(epoch: 7), candidate: CandidateReview? = nil,
                       log: StepLog? = nil, activity: AttemptActivity? = nil) -> ExecutionPlanState {
        ExecutionPlanState(connection: connection, admission: "Open", slice: true, scope: DetailRows.scope,
                           plans: [DetailRows.documents[DetailRows.id(3)]!], detail: PlanDetail(detail), detailLoad: .loaded,
                           selected: selected, candidate: candidate, log: log, activity: activity)
    }

    private static func candidate(_ attempt: String, decision: String = "NONE") -> CandidateReview {
        CandidateReview(["attempt": .text(attempt), "sealed": .bool(true), "outcome": .text("Succeeded"),
                         "candidate": .text("c1"), "log_records": .int(3), "decision": .text(decision),
                         "validation": .object(["state": .text("PASS"), "receipt": .int(9), "candidate": .text("c1")])])!
    }

    func test_s18_retries_go_at_1_13_with_exactly_the_helpers_fields() throws {
        XCTAssertEqual(Request.RetryPlanStep.minor, 13)
        XCTAssertEqual(Request.RetryPlanStep.fields, ["plan", "step", "attempt", "action", "note"])
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.refresh()
        h.fake.reply(h.fake.last("Snapshot")!, ["rows": .list([]), "as_of_event_seq": .int(30), "admission": .text("Open")])
        model.retryPlanStep(DetailRows.id(3), step: DetailRows.id(41), attempt: RetryRows.held, action: "Repair",
                            note: "Keep the name.")
        let retry = try XCTUnwrap(h.fake.last("RetryPlanStep"))
        XCTAssertEqual(retry.minor, 13)
        XCTAssertEqual(retry.payload, ["plan": .text(DetailRows.id(3)), "step": .text(DetailRows.id(41)),
                                       "attempt": .text(RetryRows.held), "action": .text("Repair"),
                                       "note": .text("Keep the name.")])
        h.fake.ack(retry, "repair recorded")
        XCTAssertEqual(model.status, "repair recorded")
        XCTAssertEqual(h.fake.last("PlanDetail")?.minor, 13)
        // A step that never started is retried after no attempt.
        model.retryPlanStep(DetailRows.id(3), step: DetailRows.id(42), attempt: nil, action: "Restart", note: "")
        XCTAssertEqual(h.fake.last("RetryPlanStep")?.payload["attempt"], .null)
    }

    func test_s18_a_step_decodes_the_retries_the_core_offers() throws {
        let step = try XCTUnwrap(PlanDetail(RetryRows.detail(retried: "Repair"))?.steps.first)
        XCTAssertEqual(step.retryActions, ["Repair", "Restart"])
        XCTAssertEqual(step.retryAttempt, RetryRows.held)
        XCTAssertEqual(step.retried, "Repair")
        // An answer before 1.13 has none.
        let older = try XCTUnwrap(PlanDetail(PlanRows.detail())?.steps.first)
        XCTAssertEqual(older.retryActions, [])
        XCTAssertNil(older.retryAttempt)
    }

    func test_s18_a_held_step_offers_only_what_the_core_allows() throws {
        let held = state()
        let step = try XCTUnwrap(held.step)
        XCTAssertNil(held.retryBlocked(step))
        XCTAssertEqual(state(connection: .channelLost("x")).retryBlocked(step), "Unavailable: the core is not connected.")
        let none = try XCTUnwrap(state(RetryRows.detail(actions: [])).step)
        XCTAssertEqual(held.retryBlocked(none), "The core offers no way to start this step again.")
        XCTAssertEqual(ExecutionPlanState.retryWords("Repair").button, "Repair Once More")
        XCTAssertEqual(ExecutionPlanState.retryWords("Restart").button, "Restart Step")
        XCTAssertFalse(ExecutionPlanState.live(step))
        XCTAssertEqual(ExecutionPlanState.group(step), "Blocked")
    }

    func test_s18_a_step_in_review_is_decided_on_its_loaded_candidate() throws {
        let plain = state(PlanRows.detail(), selected: .step(DetailRows.id(42)))
        let review = try XCTUnwrap(plain.step)
        XCTAssertEqual(review.state, "Review")
        XCTAssertEqual(plain.decideBlocked(review, accept: true), "Loading the candidate.")
        let loaded = state(PlanRows.detail(), selected: .step(DetailRows.id(42)), candidate: Self.candidate(DetailRows.id(12)))
        XCTAssertNil(loaded.decideBlocked(review, accept: true))
        XCTAssertNil(loaded.decideBlocked(review, accept: false))
        // Another attempt's candidate does not decide this step.
        let other = state(PlanRows.detail(), selected: .step(DetailRows.id(42)), candidate: Self.candidate(DetailRows.id(11)))
        XCTAssertEqual(other.decideBlocked(review, accept: true), "Loading the candidate.")
        let decided = state(PlanRows.detail(), selected: .step(DetailRows.id(42)),
                            candidate: Self.candidate(DetailRows.id(12), decision: "ACCEPTED"))
        XCTAssertEqual(decided.decideBlocked(review, accept: false), "Already decided: ACCEPTED.")
    }

    func test_s18_the_live_log_is_read_forward_and_keeps_the_newest_lines() throws {
        var log = StepLog()
        for n in 1...70 { log.append("line \(n)\nwith a break") }
        XCTAssertEqual(log.lines.count, StepLog.kept)
        XCTAssertEqual(log.lines.first, "line 7 with a break")
        log.append(String(repeating: "x", count: 300))
        XCTAssertEqual(log.lines.last?.count, 240)
        XCTAssertTrue(ExecutionPlanState.live(try XCTUnwrap(state(PlanRows.detail()).detail?.steps.first)))

        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        let attempt = DetailRows.id(11)
        model.tailLog(attempt)
        let first = try XCTUnwrap(h.fake.last("Log"))
        XCTAssertEqual(first.payload["sequence"], .int(1))
        h.fake.reply(first, ["type": .text("agent_message"), "text": .text("reading slugify.py")])
        let second = try XCTUnwrap(h.fake.last("Log"))
        XCTAssertEqual(second.payload["sequence"], .int(2))
        h.fake.replyNull(second)
        XCTAssertEqual(model.stepLogs[attempt]?.lines, ["agent_message: reading slugify.py"])
        XCTAssertEqual(model.stepLogs[attempt]?.next, 2)
        // The next read starts where this one stopped.
        model.tailLog(attempt)
        XCTAssertEqual(h.fake.last("Log")?.payload["sequence"], .int(2))
        XCTAssertEqual(h.fake.count("Log"), 3)
    }

    /// Review aid: with JOJO_SNAPSHOT_DIR set, writes the M6 places of Execution Plan.
    func test_s18_the_retry_the_live_log_and_the_decision_render_in_both_appearances() throws {
        let heading = ("Execution Plan", DetailRows.scope.place(nil))
        let log = StepLog(lines: ["agent_message: reading slugify.py", "command: python3 -m unittest",
                                  "agent_message: the tests pass"], next: 4)
        let cases: [(String, ExecutionPlanState)] = [
            ("s18-execution-plan-held", state()),
            ("s18-execution-plan-refused", state(RetryRows.detail(actions: ["Restart"], state: "Refused", retried: "Restart"))),
            ("s18-execution-plan-live", state(PlanRows.detail(), log: log,
                                              activity: AttemptActivity(text: "Ran python3 -m unittest")) ),
            ("s18-execution-plan-decision", state(PlanRows.detail(), selected: .step(DetailRows.id(42)),
                                                  candidate: Self.candidate(DetailRows.id(12)))),
        ]
        for (file, state) in cases {
            try DetailRows.render(ExecutionPlanView(state: state), screen: .executionPlan, heading: heading,
                                  size: CGSize(width: 1440, height: 1300), file: file)
        }
    }
}

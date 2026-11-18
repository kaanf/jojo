import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

/// The `PlanDetail` answer of a three-step plan: two independent steps, then one that needs
/// both.
enum PlanRows {
    static func attempt(_ n: Int, outcome: String?, validation: String?) -> JSON {
        .object(["attempt": .text(DetailRows.id(n)), "ordinal": .int(1), "outcome": outcome.map(JSON.text) ?? .null,
                 "started_at_unix_ms": .int(DetailRows.t0), "ended_at_unix_ms": outcome == nil ? .null : .int(DetailRows.t0 + 60_000),
                 "validation": validation.map(JSON.text) ?? .null, "accepted": .bool(false)])
    }

    static func step(_ key: String, _ title: String, n: Int, provider: String, depends: [String], wave: Int64, state: String,
                     eligible: Bool, attempts: [JSON] = [], blockers: [JSON] = []) -> JSON {
        .object(["step": .text(DetailRows.id(40 + n)), "key": .text(key), "title": .text(title), "kind": .text("backend"),
                 "provider": .text(provider), "depends_on": .list(depends.map(JSON.text)), "wave": .int(wave),
                 "write_scope": .list([.text("src/\(key).txt")]), "baseline": .list([]), "tests": .list([.text(key)]),
                 "command": .list([.text("/usr/bin/true")]), "state": .text(state), "detail": .text(""),
                 "eligible": .bool(eligible), "attempts": .list(attempts), "blockers": .list(blockers)])
    }

    static func detail(approval: String = "Approved", run: String? = nil, conflicts: [JSON] = []) -> [String: JSON] {
        let waits = JSON.object(["kind": .text("Dependency"), "reason": .text("Waits for api, usage.")])
        return ["plan": .text(DetailRows.id(3)), "revision": .int(1), "approval": .text(approval), "feature": .text(DetailRows.feature),
                "feature_name": .text("Slugify from the shell"), "run": run.map(JSON.text) ?? .null,
                "steps": .list([
                    step("api", "Add the entry point", n: 1, provider: "ClaudeCode", depends: [], wave: 1, state: "Running",
                         eligible: false, attempts: [attempt(11, outcome: nil, validation: nil)]),
                    step("usage", "Describe the usage", n: 2, provider: "Codex", depends: [], wave: 1, state: "Review", eligible: false,
                         attempts: [attempt(12, outcome: "Succeeded", validation: "Passed")]),
                    step("join", "Test the entry point", n: 3, provider: "Codex", depends: ["api", "usage"], wave: 2, state: "Waiting",
                         eligible: false, blockers: [waits]),
                ]),
                "checks": .object(["cycle": .bool(false), "missing": .list([]), "conflicts": .list(conflicts)])]
    }
}

@MainActor
final class S8ExecutionPlanTests: XCTestCase {
    private func state(_ detail: [String: JSON] = PlanRows.detail(), selected: PlanNode? = nil, filters: PlanFilters = PlanFilters(),
                       connection: HelperConnection = .ready(epoch: 7)) -> ExecutionPlanState {
        ExecutionPlanState(connection: connection, admission: "Open", slice: true, scope: DetailRows.scope,
                           plans: [DetailRows.documents[DetailRows.id(3)]!], detail: PlanDetail(detail), detailLoad: .loaded,
                           filters: filters, selected: selected)
    }

    func test_s8_a_plan_decodes_at_1_13_step_by_step_with_its_checks() throws {
        let conflict = JSON.object(["steps": .list([.text("api"), .text("usage")]), "paths": .list([.text("src")])])
        let detail = try XCTUnwrap(PlanDetail(PlanRows.detail(conflicts: [conflict])))
        XCTAssertEqual(detail.steps.map(\.label), ["api", "usage", "join"])
        XCTAssertEqual(detail.steps.map(\.wave), [1, 1, 2])
        XCTAssertEqual(detail.steps[2].dependsOn, ["api", "usage"])
        XCTAssertEqual(detail.steps[0].writeScope, ["src/api.txt"])
        XCTAssertEqual(detail.steps[1].attempts.first?.validation, "Passed")
        XCTAssertEqual(detail.conflicts.first?.steps, ["api", "usage"])
        XCTAssertEqual(Request.PlanDetail.minor, 13)
        XCTAssertNil(PlanDetail(["plan": .text("x")]))
    }

    func test_s8_steps_fall_into_the_boards_status_groups_and_the_filters_hide_them() {
        let all = state()
        XCTAssertEqual(all.waves, [1, 2])
        XCTAssertEqual(all.count(status: "Running"), 1)
        XCTAssertEqual(all.count(status: "Completed"), 1)
        XCTAssertEqual(all.count(provider: "Codex"), 2)
        XCTAssertEqual(state(filters: PlanFilters(hiddenProviders: ["Codex"])).shown.map(\.label), ["api"])
        XCTAssertEqual(state(filters: PlanFilters(wave: 2)).shown.map(\.label), ["join"])
        XCTAssertEqual(ExecutionPlanState.pill(all.detail!.steps[1]).0, "Passed")
        XCTAssertEqual(all.verified.passed, 1)
    }

    func test_s8_start_eligible_tasks_runs_an_approved_plan_and_resumes_a_paused_one() throws {
        XCTAssertEqual(state().runBlocked, "No step is eligible.")
        XCTAssertEqual(state(PlanRows.detail(approval: "Pending")).runBlocked, "Plan rev 1 is not approved.")
        XCTAssertTrue(state(PlanRows.detail(approval: "Pending")).approvalGate)
        XCTAssertEqual(state(connection: .channelLost("x")).runBlocked, "Unavailable: the core is not connected.")
        XCTAssertEqual(state().pauseBlocked, "The plan has no run to pause.")
        XCTAssertEqual(state().pauseTitle, "Pause Admission")
    }

    func test_s8_a_step_is_started_by_hand_only_when_eligible_and_no_run_owns_it() throws {
        var detail = PlanRows.detail()
        detail["steps"] = .list([PlanRows.step("solo", "One step", n: 1, provider: "Codex", depends: [], wave: 1, state: "Waiting",
                                               eligible: true)])
        let solo = state(detail)
        let step = try XCTUnwrap(solo.detail?.steps.first)
        XCTAssertEqual(solo.startBlocked(step), "The approved plan was not read yet.")
        XCTAssertNil(solo.runBlocked)
        XCTAssertEqual(solo.eligibleCount, 1)
        let join = try XCTUnwrap(state().detail?.steps.last)
        XCTAssertEqual(state().startBlocked(join), "Not eligible: Waits for api, usage.")
    }

    /// Review aid: with JOJO_SNAPSHOT_DIR set, writes Execution Plan in its states.
    func test_s8_execution_plan_renders_in_both_appearances() throws {
        let heading = ("Execution Plan", DetailRows.scope.place(nil))
        let controls = AnyView(HeaderButtons(buttons: [HeaderButton(title: "Review Plan"), HeaderButton(title: "Approve Plan"),
                                                       HeaderButton(title: "Pause Admission")]))
        let primary = AnyView(Button("Start Eligible Tasks · 0") {}.buttonStyle(.jojo(.primary)).disabled(true))
        let conflict = JSON.object(["steps": .list([.text("api"), .text("usage")]), "paths": .list([.text("src")])])
        let cases: [(String, ExecutionPlanState, CGSize)] = [
            ("s8-execution-plan", state(selected: .step(DetailRows.id(41))), CGSize(width: 1440, height: 1300)),
            ("s8-execution-plan-blocked", state(selected: .step(DetailRows.id(43))), CGSize(width: 1440, height: 1300)),
            ("s8-execution-plan-proposed", state(PlanRows.detail(approval: "Pending", conflicts: [conflict]), selected: .approvalGate),
             CGSize(width: 1440, height: 1300)),
            ("s8-execution-plan-verification-gate", state(selected: .verificationGate), CGSize(width: 1440, height: 1300)),
            ("s8-execution-plan-narrow", state(selected: .step(DetailRows.id(41))), CGSize(width: 960, height: 1900)),
            ("s8-execution-plan-disconnected", state(connection: .channelLost("pipe closed")), CGSize(width: 1440, height: 1300)),
        ]
        for (file, state, size) in cases {
            try DetailRows.render(ExecutionPlanView(state: state), screen: .executionPlan, heading: heading, controls: controls,
                                  primary: primary, size: size, file: file)
        }
    }
}

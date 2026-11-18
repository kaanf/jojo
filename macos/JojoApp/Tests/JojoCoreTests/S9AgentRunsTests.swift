import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

/// The `AttemptDetail` answer of a running attempt and the attempts it is listed with.
enum RunRows {
    static func detail(_ n: Int = 2, validations: [JSON] = [], live: Bool = true) -> [String: JSON] {
        let event = { (seq: Int64, kind: String, summary: String) -> JSON in
            .object(["seq": .int(seq), "at_unix_ms": .int(DetailRows.t0 + seq * 1000), "kind": .text(kind), "summary": .text(summary)])
        }
        return ["attempt": .text(DetailRows.id(n)), "step": .text(DetailRows.id(41)), "key": .text("api"),
                "title": .text("Add the entry point"), "plan": .text(DetailRows.id(3)), "feature": .text(DetailRows.feature),
                "feature_name": .text("Slugify from the shell"), "ordinal": .int(2), "repository_name": .text("slugify"),
                "project_name": .text("Sample"), "provider": .text("ClaudeCode"), "session": .text("ses_8f21c4e0"),
                "process": .object(["pid": .int(61204), "pgid": .int(61204)]), "heartbeat_at_unix_ms": .int(DetailRows.t0 + 590_000),
                "worktree": .object(["root": .text("~/.jojo/wt/slugify/api-2"), "base": .text("9c1e4b2aa0"), "state": .text("BOUND"),
                                     "snapshot": .text("9f2c00000041ab")]),
                "candidate_head": live ? .null : .text("a71f0de11"), "continues": .text(DetailRows.id(1)), "continued_by": .null,
                "claims": .list([.object(["resource": .text("repo:slugify"), "mode": .text("SHARED"), "state": .text("ACTIVE")])]),
                "write_scope": .list([.text("cli.py")]), "baseline": .list([.text("slugify.py")]), "tests": .list([.text("cli")]),
                "command": .list([.text("/usr/bin/python3"), .text("-m"), .text("unittest")]),
                "events": .list([event(1, "AttemptReserved", "api · attempt 2 reserved"),
                                 event(2, "ProcessLaunchIssued", "api · attempt 2 started on Claude Code"),
                                 event(3, "ProviderOutput", "api · attempt 2 is writing")]),
                "validations": .list(validations), "executable": .object(["version": .text("2.1.311"), "sha256": .text("ab"),
                                                                          "profiled": .bool(true)]),
                "evidence_records": .int(4)]
    }

    static func run(_ n: Int64, passed: Bool) -> JSON {
        .object(["ordinal": .int(n), "receipt": .int(90 + n), "at_unix_ms": .int(DetailRows.t0 + n * 60_000), "candidate": .text("c4c4c4c4c4c4"),
                 "base": .text("9c1e4b2"), "validator": .text("host"), "passed": .bool(passed),
                 "reasons": .list(passed ? [] : [.text("cli: exit 1")]), "report": .text("r"), "approvals": .text("7c1f00000000e02")])
    }

    static func attempt(_ n: Int, state: String, key: String = "api", ordinal: Int64, validation: String? = nil) -> JSON {
        guard case .object(var row) = OverviewRows.attempt(n, state: state, key: key,
                                                           ended: ["Succeeded", "Failed", "Interrupted"].contains(state) ? DetailRows.t0 + 60_000 : nil)
        else { return .null }
        row["ordinal"] = .int(ordinal)
        row["repository_name"] = .text("slugify")
        row["project_name"] = .text("Sample")
        row["validation"] = validation.map { .object(["state": .text($0), "at_unix_ms": .int(DetailRows.t0), "decision": .null]) } ?? .null
        return .object(row)
    }

    static let attempts = AttemptList(["attempts": .list([
        attempt(2, state: "Running", ordinal: 2),
        attempt(3, state: "Succeeded", key: "usage", ordinal: 1, validation: "Passed"),
        attempt(1, state: "Interrupted", ordinal: 1),
    ]), "next": .null])!
}

@MainActor
final class S9AgentRunsTests: XCTestCase {
    private func state(selected: String? = nil, tab: AttemptTab = .timeline, detail: [String: JSON]? = RunRows.detail(),
                       recovery: [RecoveryRow] = [], connection: HelperConnection = .ready(epoch: 7)) -> AgentRunsState {
        AgentRunsState(connection: connection, admission: "Open", scope: DetailRows.scope, summaries: RunRows.attempts,
                       recovery: recovery, selected: selected, detail: detail.flatMap(AttemptDetail.init), detailLoad: .loaded,
                       log: ["stdout: $ python3 -m unittest", "stdout: Ran 3 tests", "result: success"], tab: tab, now: DetailRows.now)
    }

    func test_s9_an_attempt_decodes_at_1_9_with_its_supervision_workspace_and_timeline() throws {
        let detail = try XCTUnwrap(AttemptDetail(RunRows.detail(validations: [RunRows.run(1, passed: true)])))
        XCTAssertEqual(detail.pid, 61204)
        XCTAssertEqual(detail.session, "ses_8f21c4e0")
        XCTAssertEqual(detail.worktree, "~/.jojo/wt/slugify/api-2")
        XCTAssertEqual(detail.claims.first?.mode, "SHARED")
        XCTAssertEqual(detail.events.map(\.seq), [1, 2, 3])
        XCTAssertEqual(detail.validations.first?.ordinal, 1)
        XCTAssertEqual(detail.executableVersion, "2.1.311")
        XCTAssertEqual(Request.AttemptDetail.minor, 14)
        XCTAssertNil(AttemptDetail(["key": .text("api")]))
    }

    func test_s9_attempts_are_grouped_by_task_newest_first_and_the_counter_counts_live_ones() {
        let state = state()
        XCTAssertEqual(state.groups.map(\.task), ["api · Add the entry point", "usage · Add the entry point"])
        XCTAssertEqual(state.groups.first?.attempts.map(\.ordinal), [2, 1])
        XCTAssertEqual(state.chosen?.id, DetailRows.id(2))
        XCTAssertEqual(AgentRunsState.counter(RunRows.attempts, rows: []), 1)
        XCTAssertEqual(AgentRunsState.counter(nil, rows: [SnapshotRow(key: "scheduler_attempt:\(DetailRows.id(7))", value: "RESERVED",
                                                                      detailComplete: true)]), 1)
        XCTAssertEqual(state.pill(RunRows.attempts.attempts[1]).0, "Completed")
        XCTAssertEqual(state.process(RunRows.attempts.attempts[0]), "Running · supervised")
    }

    func test_s9_provenance_says_where_an_attempt_came_from() {
        XCTAssertTrue(state().provenance.hasPrefix("Fresh continuation of attempt"))
        var fresh = RunRows.detail()
        fresh["continues"] = .null
        XCTAssertEqual(state(detail: fresh).provenance, "A fresh attempt.")
        XCTAssertEqual(state(detail: nil).provenance, "Not reported")
    }

    func test_s9_cancel_needs_a_connected_core_and_a_live_unheld_attempt() throws {
        let running = RunRows.attempts.attempts[0]
        XCTAssertNil(state().cancelBlocked(running))
        XCTAssertEqual(state().cancelBlocked(RunRows.attempts.attempts[1]), "The result is already sealed.")
        XCTAssertEqual(state(connection: .channelLost("x")).cancelBlocked(running), "Unavailable: the core is not connected.")
        let held = RecoveryRow(attempt: running.id, assessment: "held", findings: "", detailComplete: true)
        XCTAssertEqual(state(recovery: [held]).cancelBlocked(running), "On hold: resolve it in Recovery Center first.")
    }

    /// Review aid: with JOJO_SNAPSHOT_DIR set, writes Agent Runs in its states.
    func test_s9_agent_runs_renders_in_both_appearances() throws {
        let heading = ("Agent Runs", DetailRows.scope.place("attempts"))
        let controls = AnyView(HeaderButtons(buttons: [HeaderButton(title: "View Logs"), HeaderButton(title: "Inspect Attempt")]))
        let cases: [(String, AgentRunsState, CGSize)] = [
            ("s9-agent-runs", state(), CGSize(width: 1440, height: 1400)),
            ("s9-agent-runs-claims", state(tab: .claims), CGSize(width: 1440, height: 1400)),
            ("s9-agent-runs-evidence", state(selected: DetailRows.id(3), tab: .evidence,
                                             detail: RunRows.detail(3, validations: [RunRows.run(1, passed: true)], live: false)),
             CGSize(width: 1440, height: 1400)),
            ("s9-agent-runs-narrow", state(), CGSize(width: 960, height: 2000)),
            ("s9-agent-runs-disconnected", state(connection: .channelLost("pipe closed")), CGSize(width: 1440, height: 1400)),
        ]
        for (file, state, size) in cases {
            try DetailRows.render(AgentRunsView(state: state), screen: .agentRuns, heading: heading, controls: controls,
                                  size: size, file: file)
        }
    }
}

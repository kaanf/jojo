import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

/// Step 6: an inspection with three prior attempts, a quarantined claim of another holder,
/// and two resolutions of this session, as `RecoveryDetail` (protocol 1.11) reports them.
enum RecoveryRows {
    static let digest = String(repeating: "5e", count: 32)
    static let t0: Int64 = 1_763_400_000_000
    static let attempts = [DetailRows.id(71), DetailRows.id(72), DetailRows.id(73)]

    static func row(_ attempt: String, absent: Bool, choices: [String]) -> RecoveryRow {
        let structured: [String: JSON] = ["blockers": .list(absent ? [] : [.text("root process state is unknown")]),
                                          "blockers_complete": .bool(true), "process_absent": .bool(absent),
                                          "choices": .list(choices.map(JSON.text))]
        return RecoveryRow(attempt: attempt, assessment: "Assessment { … }", findings: "worktree=/tmp/wt", detailComplete: true,
                           structured: RecoveryAssessment(row: .object(structured)))
    }

    static var rows: [RecoveryRow] {
        [row(attempts[0], absent: true, choices: ["ValidateOutput", "ContinueWithHistory", "ContinueFreshContext", "Hold"]),
         row(attempts[1], absent: false, choices: ["Hold"]),
         row(attempts[2], absent: false, choices: ["Hold"])]
    }

    static func fact(_ state: String, _ detail: String = "") -> JSON { .object(["state": .text(state), "detail": .text(detail)]) }

    static func process(_ label: String, _ finding: String, _ detail: String = "") -> JSON {
        .object(["label": .text(label), "finding": .text(finding), "detail": .text(detail)])
    }

    static func subject(_ attempt: String, label: String, title: String, stage: String, processes: [JSON], worktree: JSON,
                        snapshot: String, gaps: [String] = [], session: JSON, absent: Bool, choices: [String],
                        claims: [JSON] = []) -> JSON {
        .object(["attempt": .text(attempt), "label": .text(label), "key": .text(String(label.prefix(2))), "title": .text(title),
                 "ordinal": .int(1), "repository_name": .text("sample"), "project": .text(DetailRows.id(3001)),
                 "project_name": .text("Sample"), "feature": .text(DetailRows.id(9)), "feature_name": .text("Slugify from the shell"),
                 "stage": .text(stage), "processes": .list(processes), "worktree": worktree,
                 "snapshot": .object(["state": .text(snapshot), "digest": snapshot == "Matches" ? .text(String(repeating: "c4", count: 32)) : .null]),
                 "evidence_gaps": .list(gaps.map(JSON.text)), "stale_approvals": .list([]), "unknown_effects": .list([]),
                 "session": session, "claims": .list(claims), "blockers": .list(absent ? [] : [.text("root process state is unknown")]),
                 "blockers_complete": .bool(true), "process_absent": .bool(absent), "choices": .list(choices.map(JSON.text))])
    }

    static func detail(inspected: Int64 = t0, digest: String = digest) -> RecoveryDetail {
        let subjects: [JSON] = [
            subject(attempts[0], label: "T2 · attempt 1", title: "Slugify the title", stage: "Spawned",
                    processes: [process("pid:47710 start:9001", "Absent"), process("group:47710", "Absent")],
                    worktree: fact("Intact"), snapshot: "Matches", session: fact("Recorded", "Claude Code"), absent: true,
                    choices: ["ValidateOutput", "ContinueWithHistory", "ContinueFreshContext", "Hold"],
                    claims: [.object(["resource": .text("path:src/slug/**"), "mode": .text("EXCLUSIVE"), "state": .text("QUARANTINED")])]),
            subject(attempts[1], label: "T3 · attempt 1", title: "Share sheet", stage: "Spawned",
                    processes: [process("pid:50233 start:9002", "Reused")], worktree: fact("Changed", "worktree changed since its recorded candidate (HEAD or snapshot)"),
                    snapshot: "Mismatch", gaps: ["stdout: spool longer than its durable offset"], session: fact("NotRecorded"),
                    absent: false, choices: ["Hold"]),
            subject(attempts[2], label: "T4 · attempt 1", title: "Inspector layout", stage: "Spawned",
                    processes: [process("pid:47902 start:9003", "Alive"), process("group:47902", "Alive")],
                    worktree: fact("Intact"), snapshot: "None", session: fact("Recorded", "Codex"), absent: false, choices: ["Hold"]),
        ]
        let payload: [String: JSON] = [
            "digest": .text(digest), "inspected_at_unix_ms": .int(inspected),
            "counts": .object(["examined": .int(3), "worktrees": .int(3), "worktrees_intact": .int(2), "snapshot_mismatches": .int(1),
                               "unknown_effects": .int(1), "stale_approvals": .int(0), "evidence_gaps": .int(1), "claims": .int(1)]),
            "subjects": .list(subjects),
            "claims": .list([.object(["resource": .text("path:shared/lobby/share/**"), "mode": .text("EXCLUSIVE"),
                                      "holder_kind": .text("scheduler_attempt"), "holder": .text(DetailRows.id(70)),
                                      "holder_label": .text("T1 · attempt 2")])]),
            "settled": .list([
                .object(["attempt": .text(DetailRows.id(69)), "label": .text("T1 · attempt 1"), "choice": .text("CONTINUE_FRESH_CONTEXT"),
                         "continued": .bool(true), "at_unix_ms": .int(t0 - 20 * 60_000)]),
                .object(["attempt": .text(DetailRows.id(68)), "label": .text("T5 · attempt 1"), "choice": .text("HOLD"),
                         "continued": .bool(false), "at_unix_ms": .int(t0 - 25 * 60_000)]),
            ]),
        ]
        return RecoveryDetail(payload)!
    }

    static func state(external: [ExternalAction] = [], detail: RecoveryDetail? = detail(), admission: String = "Open",
                      rows: [RecoveryRow] = rows, minutes: Int64 = 38, reportsDetail: Bool = true,
                      connection: HelperConnection = .ready(epoch: 7)) -> RecoveryCenterState {
        RecoveryCenterState(connection: connection, admission: admission, rows: rows, digest: digest, commandPending: false,
                            external: external, detail: detail, reportsDetail: reportsDetail,
                            now: helperDate(t0 + minutes * 60_000)!)
    }
}

@MainActor
final class S13RecoveryCenterTests: XCTestCase {
    func test_s13_recovery_detail_is_read_at_1_11_only_on_request_and_decoded() throws {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.refresh()
        h.fake.reply(h.fake.last("Snapshot")!, ["rows": .list([]), "as_of_event_seq": .int(30), "admission": .text("Open")])
        XCTAssertEqual(h.fake.count("RecoveryDetail"), 0, "nothing reads the detail unless the Recovery Center asks")
        XCTAssertEqual(Request.RecoveryDetail.minor, 11)
        XCTAssertEqual(Request.RecoveryDetail.fields, [])
        model.loadRecoveryDetail()
        let request = try XCTUnwrap(h.fake.last("RecoveryDetail"))
        XCTAssertEqual(request.minor, 11)
        XCTAssertEqual(request.payload, [:])
        model.loadRecoveryDetail()
        XCTAssertEqual(h.fake.count("RecoveryDetail"), 1, "one read at a time")
        let detail = RecoveryRows.detail()
        h.fake.reply(request, [
            "digest": .text(RecoveryRows.digest), "inspected_at_unix_ms": .int(RecoveryRows.t0),
            "counts": .object(["examined": .int(3), "worktrees": .int(3), "worktrees_intact": .int(2)]),
            "subjects": .list([]), "claims": .list([]), "settled": .list([]),
        ])
        XCTAssertEqual(model.recoveryDetailLoad, .loaded)
        XCTAssertEqual(model.recoveryDetail?.counts.examined, 3)
        XCTAssertEqual(model.recoveryDetail?.counts.snapshotMismatches, 0, "a count the core does not send is zero")
        XCTAssertEqual(model.recoveryDetail?.inspectedAt, helperDate(RecoveryRows.t0))

        // The whole fixture decodes field by field.
        XCTAssertEqual(detail.subjects.count, 3)
        let first = try XCTUnwrap(detail.subjects.first)
        XCTAssertEqual(first.label, "T2 · attempt 1")
        XCTAssertEqual(first.processes.map(\.state), ["Absent", "Absent"])
        XCTAssertEqual(first.worktree.state, "Intact")
        XCTAssertEqual(first.snapshot.state, "Matches")
        XCTAssertEqual(first.session.detail, "Claude Code")
        XCTAssertEqual(first.claims.first?.state, "QUARANTINED")
        XCTAssertEqual(first.choices, RecoveryChoice.allCases)
        XCTAssertEqual(detail.claims.first?.holder, "T1 · attempt 2")
        XCTAssertEqual(detail.settled.map(\.choice), ["CONTINUE_FRESH_CONTEXT", "HOLD"])
    }

    func test_s13_held_items_are_external_actions_attempts_and_claims_with_the_inspection_facts() throws {
        let state = RecoveryRows.state(external: ExternalRows.unknown.unresolved)
        XCTAssertEqual(state.held.map(\.kindTitle), ["External side effect", "Prior attempt", "Prior attempt", "Prior attempt", "Quarantined claim"])
        XCTAssertEqual(state.held.map(\.pill.0), ["Outcome unknown", "Confirmed dead", "Unknown", "Live · unsupervised", "Quarantined"])
        XCTAssertEqual(state.held[1].title, "T2 · Attempt 1")
        XCTAssertEqual(state.held[1].place, "Sample · Slugify from the shell · Slugify the title")
        XCTAssertEqual(state.held[4].title, "path:shared/lobby/share/**")
        XCTAssertEqual(state.held[4].place, "Held by T1 · attempt 2")
        XCTAssertEqual(state.settled.map(\.choice), ["CONTINUE_FRESH_CONTEXT"], "a Hold keeps the attempt held")
        XCTAssertEqual(HeldItem.settledWords("CONTINUE_FRESH_CONTEXT"), "Continued fresh")

        // Each fact on its own line.
        let facts = Dictionary(state.items[0].facts.map { ($0.key, $0.value) }, uniquingKeysWith: { a, _ in a })
        XCTAssertEqual(facts["Worktree"], "Present · matches its record")
        XCTAssertEqual(facts["Snapshot"], "sha256 verified · c4c4c4c4c4c4")
        XCTAssertEqual(facts["Session"], "Claude Code session recorded")
        XCTAssertEqual(facts["Evidence"], "Logs complete to their last durable record")
        XCTAssertEqual(facts["Process"], "pid 47710 · absent")
        XCTAssertEqual(facts["Process group"], "pgid 47710 · absent")
        XCTAssertEqual(facts["Claims"], "path:src/slug/** · quarantined")
        let changed = Dictionary(state.items[1].facts.map { ($0.key, $0.value) }, uniquingKeysWith: { a, _ in a })
        XCTAssertEqual(changed["Snapshot"], "Changed after snapshot")
        XCTAssertEqual(changed["Evidence"], "stdout: spool longer than its durable offset")
        XCTAssertTrue(state.items[2].sentence(.ValidateOutput).contains("alive but unsupervised"))

        // Freshness: five minutes, from the inspection's own time.
        XCTAssertEqual(state.fresh, false)
        XCTAssertTrue(state.inspectedLine?.hasSuffix("· 38 min ago") == true)
        XCTAssertEqual(RecoveryRows.state(minutes: 2).fresh, true)
        // A detail of another inspection is not shown beside this one.
        let other = RecoveryRows.state(detail: RecoveryRows.detail(digest: String(repeating: "0", count: 64)))
        XCTAssertNil(other.currentDetail)
        XCTAssertNil(other.fresh)
        XCTAssertEqual(other.items[0].place, "Names not reported")
        XCTAssertEqual(other.items[0].facts.map(\.key), ["Attempt", "OS evidence", "Findings"])
    }

    func test_s13_the_screen_reads_the_detail_only_while_shown_and_says_when_a_core_does_not_report_it() {
        let old = RecoveryRows.state(detail: nil, reportsDetail: false)
        XCTAssertNil(old.currentDetail)
        XCTAssertEqual(old.held.count, 3, "an older core still lists its prior attempts")
        XCTAssertNil(old.inspectedLine)
        XCTAssertTrue(MainView.built.contains(.recoveryCenter))
    }

    /// Review aid: the Recovery Center from its Mac board in its states.
    func test_s13_recovery_center_renders_in_both_appearances() throws {
        let heading = ("Recovery Center", "Workspace · 5 held")
        let wide = CGSize(width: 1440, height: 1000), opening = CGSize(width: 1120, height: 900)
        let cases: [(String, RecoveryCenterState, CGSize)] = [
            ("s13-recovery", RecoveryRows.state(external: ExternalRows.unknown.unresolved), wide),
            ("s13-recovery-attempt", RecoveryRows.state(), wide),
            ("s13-recovery-opening", RecoveryRows.state(), opening),
            ("s13-recovery-narrow", RecoveryRows.state(), CGSize(width: 900, height: 1600)),
            ("s13-recovery-fresh", RecoveryRows.state(minutes: 1), wide),
            ("s13-recovery-older-core", RecoveryRows.state(detail: nil, reportsDetail: false), wide),
            ("s13-recovery-closed", RecoveryRows.state(admission: "Closed"), wide),
            ("s13-recovery-empty", RecoveryRows.state(detail: nil, rows: []), wide),
            ("s13-recovery-disconnected", RecoveryRows.state(connection: .channelLost("pipe closed")), wide),
        ]
        for (file, state, size) in cases {
            try DetailRows.render(RecoveryCenterView(state: state), screen: .recoveryCenter, heading: heading, size: size, file: file)
        }
        try DetailRows.render(RecoveryCenterView(state: RecoveryRows.state(admission: "Closed"), reviewing: true),
                              screen: .recoveryCenter, heading: heading, size: wide, file: "s13-launch-review")
    }
}

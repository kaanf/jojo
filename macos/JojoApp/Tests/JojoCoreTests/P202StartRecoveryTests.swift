@testable import JojoCore
import XCTest

/// P2-02: the GUI side of protocol 1.1 Start and recovery resolution. The GUI sends
/// identities, a choice and the inspected digest; every decision stays with the helper.
@MainActor
final class P202StartRecoveryTests: XCTestCase {
    private let attempt = "000000000000000000000000000000a2"
    private let subjects = StartSubjects(prd: String(repeating: "0", count: 29) + "2bd",
                                         adr: String(repeating: "0", count: 29) + "2be",
                                         plan: String(repeating: "0", count: 29) + "2bf",
                                         validation: String(repeating: "0", count: 29) + "2c0",
                                         step: String(repeating: "0", count: 31) + "1")

    private func loadRecovery(_ h: Harness, _ model: WorkspaceModel, digest: String) {
        model.refresh()
        h.fake.reply(h.fake.last("Recovery")!, [
            "digest": .text(digest),
            "rows": .list([.object(["attempt": .text(attempt), "assessment": .text("Assessment { blockers: [\"process state unknown\"] }"),
                                    "findings": .text("stage=Spawned"), "detail_complete": .bool(true)])]),
        ])
        XCTAssertEqual(model.recoveryDigest, digest)
    }

    func test_p2_02_start_sends_only_five_identities_and_is_never_sent_twice() {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        // Anything that is not five identities sends nothing at all.
        model.start(StartSubjects(prd: "/bin/sh", adr: subjects.adr, plan: subjects.plan,
                                  validation: subjects.validation, step: subjects.step))
        model.start(StartSubjects())
        XCTAssertEqual(h.fake.count("Start"), 0)
        XCTAssertTrue(model.status.contains("five"))

        model.start(subjects)
        let start = h.fake.last("Start")!
        XCTAssertEqual(start.minor, 1, "Start exists only in protocol 1.1")
        XCTAssertNil(start.revision)
        XCTAssertEqual(start.payload, [
            "prd": .text(subjects.prd), "adr": .text(subjects.adr), "plan": .text(subjects.plan),
            "validation": .text(subjects.validation), "step": .text(subjects.step),
        ], "identities only: no executable, argument or environment field exists")
        XCTAssertTrue(model.commandPending)
        // Clicking again while unanswered sends nothing.
        model.start(subjects)
        model.start(subjects)
        XCTAssertEqual(h.fake.count("Start"), 1)

        // A refusal is shown as a refusal; nothing is retried automatically.
        h.fake.error(start, "AdmissionClosed")
        XCTAssertFalse(model.commandPending)
        XCTAssertEqual(model.status, "Start refused (AdmissionClosed); nothing was started")
        XCTAssertEqual(h.fake.count("Start"), 1)

        // A later explicit Start is a new command with a new identity.
        model.start(subjects)
        let second = h.fake.last("Start")!
        XCTAssertNotEqual(second.id, start.id)
        h.fake.reply(second, ["acknowledged": .bool(true), "operation_completed": .bool(false), "command": .text(second.id),
                              "epoch": .int(7), "event_seq": .int(30), "response": .text("accepted attempt=\(second.id)"),
                              "replayed": .bool(false), "attempt": .text(second.id), "dispatch": .text("launched")])
        XCTAssertFalse(model.commandPending)
        XCTAssertTrue(model.status.contains("Start accepted for attempt \(second.id) (launched)"), model.status)
        XCTAssertEqual(h.fake.count("Start"), 2)
        XCTAssertEqual(h.client.outstandingCommands, [:])
    }

    func test_p2_02_lost_start_response_stays_outstanding_and_is_not_resent() {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.start(subjects)
        let start = h.fake.last("Start")!
        h.fake.breakChannel("pipe closed")
        XCTAssertTrue(model.status.contains("outcome unknown"), model.status)
        XCTAssertTrue(model.commandPending, "an unknown outcome is not a free retry")
        XCTAssertEqual(h.client.outstandingCommands, [start.id: "Start"], "kept for receipt reconciliation")
        model.start(subjects)
        model.resolveRecovery(attempt, .Hold)
        model.startAfterRecovery()
        XCTAssertEqual(h.fake.count("Start"), 1)
        XCTAssertEqual(h.fake.count("RecoveryResolve") + h.fake.count("RecoveryStart"), 0)
    }

    func test_p2_02_recovery_choice_and_reviewed_start_bind_the_inspected_digest() {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        // Nothing can be resolved or started before an inspection is on screen.
        model.resolveRecovery(attempt, .Hold)
        model.startAfterRecovery()
        XCTAssertEqual(h.fake.count("RecoveryResolve") + h.fake.count("RecoveryStart"), 0)
        loadRecovery(h, model, digest: "digest-1")
        XCTAssertEqual(RecoveryChoice.allCases.map(\.label), recoveryChoices)

        // The helper refuses a choice its evidence does not offer; the GUI does not insist.
        model.resolveRecovery(attempt, .ValidateOutput)
        let refused = h.fake.last("RecoveryResolve")!
        XCTAssertEqual(refused.payload, ["digest": .text("digest-1"), "attempt": .text(attempt),
                                         "choice": .text("ValidateOutput")])
        h.fake.error(refused, "ValidateOutput is not offered: process state unknown")
        XCTAssertTrue(model.status.hasPrefix("Recovery choice refused"), model.status)
        XCTAssertEqual(h.fake.count("RecoveryResolve"), 1)
        XCTAssertEqual(h.fake.count("RecoveryInspect"), 0)

        // Hold is committed, then the inspection is refreshed: the old digest is not reused.
        model.resolveRecovery(attempt, .Hold)
        let hold = h.fake.last("RecoveryResolve")!
        XCTAssertEqual(hold.payload["choice"], .text("Hold"))
        h.fake.ack(hold, "HOLD")
        XCTAssertTrue(model.status.contains("committed"), model.status)
        let inspect = h.fake.last("RecoveryInspect")!
        XCTAssertEqual(inspect.payload, [:])
        h.fake.reply(inspect, ["digest": .text("digest-2"), "subjects": .int(1), "other_quarantined": .int(0)])
        h.fake.reply(h.fake.last("Recovery")!, ["digest": .text("digest-2"), "rows": .list([])])
        XCTAssertEqual(model.recoveryDigest, "digest-2")

        model.startAfterRecovery()
        let opened = h.fake.last("RecoveryStart")!
        XCTAssertEqual(opened.payload, ["digest": .text("digest-2")])
        h.fake.ack(opened, "admission open after reviewed recovery")
        XCTAssertEqual(model.status, "Admission open after reviewed recovery; nothing was started")
        XCTAssertEqual(h.fake.count("Start"), 0, "opening admission starts nothing")

        // An explicit inspection reports how many subjects still need a decision.
        model.inspectRecovery()
        h.fake.reply(h.fake.last("RecoveryInspect")!, ["digest": .text("digest-3"), "subjects": .int(1),
                                                       "other_quarantined": .int(0)])
        XCTAssertEqual(model.status, "Inspection refreshed: 1 subject(s) need a decision")
        // An identity that is not well formed never reaches the helper.
        model.resolveRecovery("../../etc", .Hold)
        XCTAssertEqual(h.fake.count("RecoveryResolve"), 2)
    }

    func test_p2_02_real_helper_refuses_start_until_reviewed_and_approved() throws {
        guard let path = ProcessInfo.processInfo.environment["JOJO_HELPER_BIN"] else {
            throw XCTSkip("JOJO_HELPER_BIN not set")
        }
        let base = NSTemporaryDirectory() + "jojo-p2-02-\(UUID().uuidString.prefix(8))"
        // The disposable root is only named here; no repository is registered in this test.
        let transport = ProcessHelperTransport(.init(executable: URL(fileURLWithPath: path), dataRoot: base + "-data",
                                                     worktreeRoot: base + "-worktrees",
                                                     slice: ["--disposable-root", base + "-disposable"]))
        let client = HelperClient(transport: transport)
        let model = WorkspaceModel(client: client)
        client.start()
        func spin(_ message: String, until condition: () -> Bool) {
            let deadline = Date().addingTimeInterval(20)
            while !condition() {
                if Date() > deadline { return XCTFail(message + ": " + model.status) }
                RunLoop.main.run(mode: .default, before: Date().addingTimeInterval(0.01))
            }
        }
        spin("handshake") { if case .ready = client.connection { return true }; return false }

        model.start(subjects)
        spin("first refusal") { !model.commandPending }
        XCTAssertEqual(model.status, "Start refused (AdmissionClosed); nothing was started")

        model.inspectRecovery()
        spin("inspection") { !model.recoveryDigest.isEmpty && model.status.hasPrefix("Inspection refreshed") }
        XCTAssertEqual(model.status, "Inspection refreshed: 0 subject(s) need a decision")
        model.startAfterRecovery()
        spin("reviewed start") { model.status.hasPrefix("Admission open") }
        model.refresh()
        spin("admission") { model.admission == "Open" }

        // Admission is not approval: nothing was imported, so nothing can be started.
        model.start(subjects)
        spin("second refusal") { model.status.hasPrefix("Start refused") }
        XCTAssertEqual(model.status,
                       "Start refused (StartRefused: missing or stale approval: PRD); nothing was started")
        XCTAssertEqual(client.outstandingCommands, [:])
    }
}

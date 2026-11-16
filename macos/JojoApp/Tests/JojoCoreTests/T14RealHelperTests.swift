@testable import JojoCore
import XCTest

/// The Swift transport/client/coordinator against the real `jojo-helper --serve` binary
/// (JOJO_HELPER_BIN). Real pipes, real ShutdownReady, real exit status. The P1-14 gate
/// requires these to run, not skip.
@MainActor
final class T14RealHelperTests: XCTestCase {
    private func helper() throws -> URL {
        guard let path = ProcessInfo.processInfo.environment["JOJO_HELPER_BIN"] else {
            throw XCTSkip("JOJO_HELPER_BIN not set")
        }
        return URL(fileURLWithPath: path)
    }

    private func roots() -> (String, String) {
        let base = NSTemporaryDirectory() + "jojo-t14-\(UUID().uuidString.prefix(8))"
        return (base + "-data", base + "-worktrees")
    }

    private func spin(until condition: () -> Bool, _ message: String) {
        let deadline = Date().addingTimeInterval(20)
        while !condition() {
            XCTAssertLessThan(Date(), deadline, message)
            if Date() > deadline { return }
            RunLoop.main.run(mode: .default, before: Date().addingTimeInterval(0.01))
        }
    }

    private func start(_ executable: URL, _ data: String, _ worktrees: String) -> (HelperClient, QuitCoordinator) {
        let transport = ProcessHelperTransport(.init(executable: executable, dataRoot: data, worktreeRoot: worktrees))
        let client = HelperClient(transport: transport)
        let quit = QuitCoordinator(client: client, scheduler: TimerScheduler())
        client.onConnectionChange = { quit.connectionChanged($0) }
        client.start()
        return (client, quit)
    }

    func test_t14_real_helper_idle_quit_is_clean_only_after_ready_and_exit() throws {
        let (data, worktrees) = roots()
        let (client, quit) = start(try helper(), data, worktrees)
        var replies: [Bool] = []
        quit.reply = { replies.append($0) }
        spin(until: { if case .ready = client.connection { return true }; return false }, "handshake")
        XCTAssertEqual(quit.requestQuit(.quitCommand), .later)
        XCTAssertEqual(quit.requestQuit(.quitCommand), .cancel)
        spin(until: { !replies.isEmpty }, "termination reply")
        XCTAssertEqual(replies, [true])
        guard case .clean(let proof, let unconfirmed) = quit.outcome else { return XCTFail("\(quit.phase)") }
        XCTAssertTrue(Identity.valid(proof.ticket) && proof.eventSeq > 0)
        XCTAssertEqual(unconfirmed, [])
        XCTAssertEqual(client.connection, .ended(status: 0))
        XCTAssertEqual(quit.duplicateRequests, 1)

        // Reopen: a new helper epoch reports a clean previous session; the receipt is durable.
        let (again, _) = start(try helper(), data, worktrees)
        spin(until: { if case .ready = again.connection { return true }; return false }, "reopen")
        var receipt: Reply?
        again.query("ShutdownReceipt", ["ticket": .text(proof.ticket)]) { receipt = $0 }
        spin(until: { receipt != nil }, "receipt")
        guard case .response(let r) = receipt else { return XCTFail("\(String(describing: receipt))") }
        XCTAssertEqual(r["outcome"], .text("Ready"))
        XCTAssertEqual(r["shutdown"], .text(proof.shutdown))
    }

    func test_t14_real_helper_null_answers_recovery_and_singleton_refusal() throws {
        let (data, worktrees) = roots()
        let (client, quit) = start(try helper(), data, worktrees)
        spin(until: { if case .ready = client.connection { return true }; return false }, "handshake")
        var answers: [String: Reply] = [:]
        client.query("ReviewBundle", ["subject": .text("000000000000000000000000000000e5")]) { answers["review"] = $0 }
        client.query("Receipt", ["command": .text("000000000000000000000000000000e6")]) { answers["receipt"] = $0 }
        client.query("Recovery", ["offset": .int(0), "limit": .int(8)]) { answers["recovery"] = $0 }
        client.query("Snapshot", ["section": .text("aggregates"), "after": .text(""), "cursor": .null, "limit": .int(8)]) { answers["snapshot"] = $0 }
        spin(until: { answers.count == 4 }, "queries")
        XCTAssertEqual(answers["review"], .null)
        XCTAssertEqual(answers["receipt"], .null)
        guard case .response(let rec) = answers["recovery"], case .response(let snap) = answers["snapshot"] else { return XCTFail("\(answers)") }
        XCTAssertNotNil(rec["digest"]?.string)
        XCTAssertEqual(snap["admission"], .text("Closed"), "startup stays paused")

        let (second, _) = start(try helper(), data, worktrees)
        spin(until: { second.connection == .ended(status: 3) }, "replacement refused")
        XCTAssertEqual(second.connection, .ended(status: 3))

        var replies: [Bool] = []
        quit.reply = { replies.append($0) }
        XCTAssertEqual(quit.requestQuit(.quitCommand), .later)
        spin(until: { !replies.isEmpty }, "close")
        XCTAssertEqual(replies, [true])
    }
}

@testable import JojoCore
import XCTest

/// T14 model tests against a barrier-driven fake helper: idle quit, pending draft-save
/// failure, active Cancel, confirmed close with progress, graceful timeout/stop failure,
/// duplicate Cmd-Q, window distinction, helper loss. No live providers, no elapsed sleeps.
@MainActor
final class Harness {
    let fake = FakeHelper()
    let scheduler = ManualScheduler()
    let client: HelperClient
    let quit: QuitCoordinator
    var replies: [Bool] = []
    var terminationRequests = 0

    init(start: Bool = true) {
        client = HelperClient(transport: fake)
        quit = QuitCoordinator(client: client, scheduler: scheduler)
        client.onConnectionChange = { [unowned self] in self.quit.connectionChanged($0) }
        quit.reply = { [unowned self] in self.replies.append($0) }
        quit.requestTermination = { [unowned self] in self.terminationRequests += 1 }
        if start { client.start() }
    }

    /// Cmd-Q with active work up to the confirmation dialog.
    func toConfirmation(active: Int64 = 2) {
        XCTAssertEqual(quit.requestQuit(.quitCommand), .later)
        fake.ack(fake.last("PrepareClose")!, "CONFIRM_REQUIRED \(active)")
        fake.status("AwaitingConfirmation", ticket: ticketT, active: active)
        XCTAssertEqual(quit.phase, .confirmRequired(ticket: ticketT, activeCount: Int(active)))
    }

    func toStopping() {
        toConfirmation()
        quit.confirm()
        fake.ack(fake.last("ConfirmClose")!, "CONFIRMED 2")
        XCTAssertEqual(quit.phase, .stopping(ticket: ticketT, activeCount: nil))
    }
}

@MainActor
final class T14QuitTests: XCTestCase {
    func test_t14_idle_quit_needs_ready_receipt_and_helper_exit_without_confirmation() {
        let h = Harness()
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .later)
        XCTAssertEqual(h.fake.count("PrepareClose"), 1)
        h.fake.ack(h.fake.last("PrepareClose")!, "NO_ACTIVE")
        XCTAssertEqual(h.quit.phase, .finalizing, "no active-work confirmation on idle close")
        h.fake.emit("ShutdownReady", readyPayload(ticket: "000000000000000000000000000000c3"))
        guard case .awaitingExit = h.quit.phase else { return XCTFail("\(h.quit.phase)") }
        XCTAssertEqual(h.replies, [], "READY alone does not allow termination")
        h.fake.end(0)
        guard case .clean = h.quit.phase, case .clean(_, let unconfirmed) = h.quit.outcome else { return XCTFail("\(h.quit.phase)") }
        XCTAssertEqual(unconfirmed, [])
        XCTAssertEqual(h.replies, [true])
        XCTAssertEqual(h.fake.count("ConfirmClose") + h.fake.count("CancelClose"), 0)
    }

    func test_t14_ready_without_exit_or_exit_without_ready_is_never_clean() {
        let a = Harness()
        _ = a.quit.requestQuit(.quitCommand)
        a.fake.ack(a.fake.last("PrepareClose")!, "NO_ACTIVE")
        a.fake.emit("ShutdownReady", readyPayload())
        a.scheduler.fire()
        XCTAssertEqual(a.quit.phase, .failed(.exitTimeout))
        a.fake.end(0)
        XCTAssertEqual(a.quit.phase, .failed(.exitTimeout), "late exit does not upgrade a failure")
        XCTAssertEqual(a.quit.requestQuit(.quitCommand), .now)
        XCTAssertEqual(a.quit.outcome, .helperGoneUnverified(status: 0))
        XCTAssertEqual(a.replies, [false])

        let b = Harness()
        _ = b.quit.requestQuit(.quitCommand)
        b.fake.ack(b.fake.last("PrepareClose")!, "NO_ACTIVE")
        b.fake.end(0)
        XCTAssertEqual(b.quit.phase, .failed(.helperEnded(status: 0, afterProof: false)))
        XCTAssertEqual(b.replies, [false])

        let c = Harness()
        _ = c.quit.requestQuit(.quitCommand)
        c.fake.ack(c.fake.last("PrepareClose")!, "NO_ACTIVE")
        c.fake.emit("ShutdownReady", readyPayload())
        c.fake.end(4)
        XCTAssertEqual(c.quit.phase, .failed(.helperEnded(status: 4, afterProof: true)))
        XCTAssertEqual(c.replies, [false])
    }

    func test_t14_active_cancel_sends_only_cancel_close_and_work_continues() {
        let h = Harness()
        h.toConfirmation()
        h.quit.cancel()
        XCTAssertEqual(h.replies, [false], "OS termination is cancelled immediately")
        XCTAssertEqual(h.fake.last("CancelClose")?.payload, ["ticket": .text(ticketT)])
        XCTAssertEqual(h.fake.count("ConfirmClose"), 0, "Cancel never requests a stop")
        XCTAssertEqual(h.fake.count("CancelAttempt"), 0)
        h.fake.ack(h.fake.last("CancelClose")!, "CANCELLED")
        XCTAssertEqual(h.quit.phase, .idle)
        h.quit.confirm()
        XCTAssertEqual(h.fake.count("ConfirmClose"), 0, "a stale Confirm press after Cancel is ignored")
        // A later quit starts a fresh close operation.
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .later)
        XCTAssertEqual(h.fake.count("PrepareClose"), 2)
    }

    func test_t14_confirmed_close_shows_progress_then_requires_matching_ready_and_exit() {
        let h = Harness()
        h.toStopping()
        XCTAssertEqual(h.fake.last("ConfirmClose")?.payload, ["ticket": .text(ticketT)])
        h.fake.status("Stopping", ticket: ticketT, active: 2)
        XCTAssertEqual(h.quit.phase, .stopping(ticket: ticketT, activeCount: 2))
        h.scheduler.fire()
        h.fake.status("Stopping", ticket: ticketT, active: 1)
        XCTAssertEqual(h.quit.phase, .stopping(ticket: ticketT, activeCount: 1))
        XCTAssertEqual(h.replies, [])
        h.fake.emit("ShutdownReady", readyPayload())
        h.fake.end(0)
        guard case .clean(let proof) = h.quit.phase else { return XCTFail("\(h.quit.phase)") }
        XCTAssertEqual(proof.ticket, ticketT)
        XCTAssertEqual(proof.recoveryItems, ["attempt:1"], "interrupted work stays a recovery item")
        XCTAssertEqual(h.replies, [true])
        XCTAssertEqual(h.fake.count("ConfirmClose"), 1)
    }

    func test_t14_active_count_reaching_zero_is_not_confirmation() {
        let h = Harness()
        h.toConfirmation()
        h.scheduler.fire()
        h.fake.status("AwaitingConfirmation", ticket: ticketT, active: 0)
        XCTAssertEqual(h.quit.phase, .confirmRequired(ticket: ticketT, activeCount: 0))
        XCTAssertEqual(h.fake.count("ConfirmClose"), 0)
        XCTAssertEqual(h.replies, [])
    }

    func test_t14_duplicate_cmd_q_and_repeated_presses_share_one_operation() {
        let h = Harness()
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .later)
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .cancel)
        XCTAssertEqual(h.quit.requestQuit(.lastPrimaryWindow), .cancel)
        XCTAssertEqual(h.fake.count("PrepareClose"), 1)
        h.fake.ack(h.fake.last("PrepareClose")!, "CONFIRM_REQUIRED 1")
        h.fake.status("AwaitingConfirmation", ticket: ticketT, active: 1)
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .cancel, "no stacked confirmation dialog")
        h.quit.confirm()
        h.quit.confirm()
        h.quit.cancel()
        XCTAssertEqual(h.fake.count("ConfirmClose"), 1)
        XCTAssertEqual(h.fake.count("CancelClose"), 0)
        XCTAssertEqual(h.quit.duplicateRequests, 3)
        XCTAssertEqual(h.replies, [])
    }

    func test_t14_graceful_timeout_stop_failure_stays_visible_and_never_clean() {
        let h = Harness()
        h.toStopping()
        h.fake.emit("ShutdownFailure", readyPayload(stopped: false, failures: ["attempt:1 stop unconfirmed after KILL"], outcome: "Failed"))
        guard case .failed(.shutdownFailed(let detail)) = h.quit.phase else { return XCTFail("\(h.quit.phase)") }
        XCTAssertTrue(detail.contains("stop unconfirmed"))
        XCTAssertEqual(h.replies, [false], "SHUTDOWN_FAILED cancels OS termination")
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .cancel, "failure stays visible; helper alive")
        XCTAssertEqual(h.quit.requestQuit(.lastPrimaryWindow), .cancel)
        h.fake.emit("ShutdownReady", readyPayload())
        guard case .failed = h.quit.phase else { return XCTFail("late READY must not hide the failure") }
        h.quit.disconnectAndQuit()
        XCTAssertEqual(h.terminationRequests, 1)
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .now)
        XCTAssertEqual(h.quit.outcome?.label, "emergency-disconnect-unclean")
    }

    func test_t14_stalled_worker_failure_then_helper_exit_is_unverified() {
        let h = Harness()
        h.toStopping()
        h.fake.emit("ShutdownFailure", ["code": .text("WorkerStalled: helper retains ownership")])
        XCTAssertEqual(h.quit.phase, .failed(.shutdownFailed("WorkerStalled: helper retains ownership")))
        h.fake.end(0)
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .now)
        XCTAssertEqual(h.quit.outcome, .helperGoneUnverified(status: 0))
    }

    func test_t14_invalid_or_foreign_ready_proof_is_rejected() {
        for bad in [readyPayload(ticket: "000000000000000000000000000000ff"), readyPayload(sealed: false),
                    readyPayload(stopped: false), readyPayload(failures: ["x"])] {
            let h = Harness()
            h.toStopping()
            h.fake.emit("ShutdownReady", bad)
            guard case .failed(.invalidProof) = h.quit.phase else { return XCTFail("\(bad) -> \(h.quit.phase)") }
            XCTAssertEqual(h.replies, [false])
        }
        let h = Harness()
        h.toStopping()
        h.fake.emit("ShutdownReady", readyPayload(), session: "0000000000000000000000000000dead")
        h.fake.emit("ShutdownReady", readyPayload(), epoch: 99)
        XCTAssertEqual(h.quit.phase, .stopping(ticket: ticketT, activeCount: nil), "foreign session/epoch ignored")
        // A READY with no bound ticket outside the NO_ACTIVE path is not proof.
        let u = Harness()
        _ = u.quit.requestQuit(.quitCommand)
        u.fake.emit("ShutdownReady", readyPayload())
        guard case .failed(.invalidProof) = u.quit.phase else { return XCTFail("\(u.quit.phase)") }
    }

    func test_t14_pending_draft_save_failure_blocks_clean_claim_and_never_resends() {
        let h = Harness()
        let id = h.client.command("CancelAttempt", ["attempt": .text(ticketT)]) { _ in }!
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .later)
        XCTAssertEqual(h.quit.phase, .reconciling(remaining: 1))
        XCTAssertEqual(h.fake.count("PrepareClose"), 0, "settle pending writes before PrepareClose")
        h.scheduler.fire()
        let lookup = h.fake.last("Receipt")!
        XCTAssertEqual(lookup.payload, ["command": .text(id)], "reconciled under the original identity")
        h.fake.replyNull(lookup)
        XCTAssertEqual(h.quit.phase, .failed(.unsettledCommands([id])))
        XCTAssertEqual(h.replies, [false])
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .cancel, "failure needs explicit acknowledgement")
        h.quit.acknowledgeUnsettled()
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .later)
        h.fake.ack(h.fake.last("PrepareClose")!, "NO_ACTIVE")
        h.fake.emit("ShutdownReady", readyPayload())
        h.fake.end(0)
        XCTAssertEqual(h.quit.outcome?.label, "clean-with-unconfirmed-commands")
        guard case .clean(_, let unconfirmed) = h.quit.outcome else { return XCTFail() }
        XCTAssertEqual(unconfirmed, [id])
        XCTAssertEqual(h.fake.count("CancelAttempt"), 1, "uncertain command never resent")
    }

    func test_t14_pending_command_settled_by_receipt_or_late_answer_proceeds() {
        let h = Harness()
        let id = h.client.command("CancelAttempt", ["attempt": .text(ticketT)]) { _ in }!
        _ = h.quit.requestQuit(.quitCommand)
        h.scheduler.fire()
        h.fake.reply(h.fake.last("Receipt")!, ["acknowledged": .bool(true), "command": .text(id), "response": .text("ok")])
        XCTAssertEqual(h.quit.phase, .preparing)

        let g = Harness()
        let cancel = g.fake
        g.client.command("CancelAttempt", ["attempt": .text(ticketT)]) { _ in }
        _ = g.quit.requestQuit(.quitCommand)
        cancel.ack(cancel.last("CancelAttempt")!, "cancel committed")
        XCTAssertEqual(g.quit.phase, .preparing)
        XCTAssertEqual(g.fake.count("Receipt"), 0)
    }

    func test_t14_helper_crash_or_channel_loss_during_close_is_unknown_not_stopped() {
        let h = Harness()
        h.toStopping()
        h.fake.end(137)
        XCTAssertEqual(h.quit.phase, .failed(.helperEnded(status: 137, afterProof: false)))
        XCTAssertEqual(h.replies, [false])

        let c = Harness()
        c.toStopping()
        c.fake.breakChannel("helper framing error")
        guard case .failed(.channelLost) = c.quit.phase else { return XCTFail("\(c.quit.phase)") }
        XCTAssertEqual(c.quit.requestQuit(.quitCommand), .cancel)
        c.quit.disconnectAndQuit()
        XCTAssertEqual(c.terminationRequests, 1)
        XCTAssertEqual(c.quit.outcome?.label, "emergency-disconnect-unclean")
    }

    func test_t14_lost_channel_requires_explicit_emergency_disconnect() {
        let h = Harness()
        h.fake.breakChannel("malformed helper frame")
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .later)
        guard case .disconnectRequired = h.quit.phase else { return XCTFail("\(h.quit.phase)") }
        h.quit.stay()
        XCTAssertEqual(h.replies, [false])
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .later)
        h.quit.disconnectAndQuit()
        XCTAssertEqual(h.replies, [false, true])
        XCTAssertEqual(h.quit.outcome?.label, "emergency-disconnect-unclean")
        XCTAssertEqual(h.fake.count("PrepareClose"), 0)
    }

    func test_t14_quit_during_handshake_waits_and_incompatible_helper_cannot_close() {
        let h = Harness(start: false)
        h.fake.autoHello = false
        h.client.start()
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .later)
        XCTAssertEqual(h.fake.count("PrepareClose"), 0)
        h.fake.reply(h.fake.last("Hello")!, ["compatible": .bool(true), "epoch": .int(7)])
        XCTAssertEqual(h.fake.count("PrepareClose"), 1)

        let i = Harness(start: false)
        i.fake.compatible = false
        i.client.start()
        XCTAssertEqual(i.quit.requestQuit(.quitCommand), .later)
        guard case .disconnectRequired = i.quit.phase else { return XCTFail("\(i.quit.phase)") }
        XCTAssertNil(i.client.command("CancelAttempt", ["attempt": .text(ticketT)]) { _ in })
        XCTAssertEqual(i.fake.count("PrepareClose"), 0)
    }

    func test_t14_no_helper_quits_without_clean_claim() {
        let h = Harness(start: false)
        h.fake.failLaunch = true
        h.client.start()
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .now)
        XCTAssertEqual(h.quit.outcome?.label, "no-helper")
    }

    func test_t14_close_rejection_is_visible_and_retryable() {
        let h = Harness()
        _ = h.quit.requestQuit(.quitCommand)
        h.fake.error(h.fake.last("PrepareClose")!, "Rejected(\"close ticket expired\")")
        guard case .failed(.closeRejected) = h.quit.phase else { return XCTFail("\(h.quit.phase)") }
        XCTAssertEqual(h.replies, [false])
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .later)
        XCTAssertEqual(h.fake.count("PrepareClose"), 2)
    }

    func test_t14_last_primary_window_routes_to_quit_auxiliary_closes_independently() {
        XCTAssertEqual(WindowPolicy.onClose(.primary), .routeToQuit)
        XCTAssertEqual(WindowPolicy.onClose(.auxiliary), .close)
        let h = Harness()
        XCTAssertEqual(h.quit.requestQuit(.lastPrimaryWindow), .later)
        XCTAssertEqual(h.quit.requestQuit(.quitCommand), .cancel, "window close and Cmd-Q share one operation")
        XCTAssertEqual(h.fake.count("PrepareClose"), 1)
    }
}

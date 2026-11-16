@testable import JojoCore
import XCTest

/// T14 review/recovery display and the GUI side of the P1-13 transport contract.
@MainActor
final class T14ReviewProtocolTests: XCTestCase {
    private let artifact = "000000000000000000000000000000e5"

    private func bundle(revision: Int64 = 3, displayed: String = "d1") -> [String: JSON] {
        ["subject": .text(artifact), "revision": .int(revision), "hash": .text("h1"), "content_hash": .text("c1"),
         "displayed_hash": .text(displayed), "context_hash": .text("x1"), "upstream": .list([.int(4), .int(9)]),
         "prior_decision": .null]
    }

    func test_t14_decision_binds_exact_displayed_bundle_and_stale_rejection_is_not_resent() {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.loadReview(artifact)
        h.fake.reply(h.fake.last("ReviewBundle")!, bundle())
        XCTAssertEqual(model.review?.displayedHash, "d1")
        model.decide(.Approve)
        let decision = h.fake.last("HumanDecision")!
        XCTAssertEqual(decision.revision, 3, "envelope revision is the displayed revision")
        XCTAssertEqual(decision.payload, [
            "subject": .text(artifact), "hash": .text("h1"), "content_hash": .text("c1"), "displayed_hash": .text("d1"),
            "context_hash": .text("x1"), "upstream": .list([.int(4), .int(9)]), "prior_decision": .null,
            "action": .text("Approve"),
        ])
        h.fake.error(decision, "Conflict(\"stale review: displayed hash changed\")")
        XCTAssertTrue(model.reviewStale)
        XCTAssertTrue(model.status.contains("stale"))
        model.decide(.Approve)
        model.decide(.Reject)
        XCTAssertEqual(h.fake.count("HumanDecision"), 1, "no automatic resend with a new identity")
        // Reload shows the current bundle; only then can a new decision be made.
        model.loadReview(artifact)
        h.fake.reply(h.fake.last("ReviewBundle")!, bundle(revision: 4, displayed: "d2"))
        XCTAssertFalse(model.reviewStale)
        model.decide(.Reject)
        XCTAssertEqual(h.fake.last("HumanDecision")?.revision, 4)
        XCTAssertEqual(h.fake.last("HumanDecision")?.payload["displayed_hash"], .text("d2"))
        let decisionIDs = h.fake.sent.filter { $0.type == "HumanDecision" }.map(\.id)
        XCTAssertEqual(Set(decisionIDs).count, 2, "distinct decisions have distinct identities")
    }

    func test_t14_missing_or_malformed_review_cannot_be_decided() {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.loadReview(artifact)
        h.fake.replyNull(h.fake.last("ReviewBundle")!)
        XCTAssertNil(model.review)
        model.loadReview(artifact)
        var partial = bundle()
        partial["context_hash"] = nil
        h.fake.reply(h.fake.last("ReviewBundle")!, partial)
        XCTAssertNil(model.review)
        model.decide(.Approve)
        model.loadReview("not-an-id")
        XCTAssertEqual(h.fake.count("HumanDecision"), 0)
        XCTAssertEqual(h.fake.count("ReviewBundle"), 2)
    }

    func test_t14_interrupted_work_is_displayed_without_automatic_execution() {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.refresh()
        h.fake.reply(h.fake.last("Snapshot")!, [
            "as_of_event_seq": .int(12), "epoch": .int(7), "admission": .text("Closed"), "shutdown": .text("Unclean"),
            "section": .text("aggregates"), "next": .text("attempt:2"),
            "rows": .list([.object(["key": .text("attempt:000000000000000000000000000000a2"), "value": .text("INTERRUPTED shutdown"), "detail_complete": .bool(true)])]),
        ])
        h.fake.reply(h.fake.last("Recovery")!, [
            "digest": .text("abc"),
            "rows": .list([.object(["attempt": .text("000000000000000000000000000000a2"), "assessment": .text("ConfirmedDead"),
                                    "findings": .text("worktree=/wt/a2 stdout_offset=120"), "detail_complete": .bool(true)])]),
        ])
        XCTAssertEqual(model.admission, "Closed")
        XCTAssertEqual(model.rows.map(\.kind), ["attempt"])
        XCTAssertEqual(model.recovery.first?.findings, "worktree=/wt/a2 stdout_offset=120")
        XCTAssertEqual(recoveryChoices.count, 4)
        let mutations = h.fake.sent.filter { ["PrepareClose", "ConfirmClose", "CancelClose", "CancelAttempt", "HumanDecision"].contains($0.type) }
        XCTAssertEqual(mutations, [], "reopen display issues queries only")
        XCTAssertTrue(h.client.outstandingCommands.isEmpty)
    }

    func test_t14_snapshot_change_reloads_instead_of_mixing_pages() {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.refresh()
        let rows = (0..<8).map { JSON.object(["key": .text("artifact:\($0)"), "value": .text("v"), "detail_complete": .bool(true)]) }
        h.fake.reply(h.fake.last("Snapshot")!, ["as_of_event_seq": .int(5), "next": .text("artifact:7"), "rows": .list(rows), "admission": .text("Closed")])
        let second = h.fake.last("Snapshot")!
        XCTAssertEqual(second.payload["cursor"], .int(5))
        h.fake.error(second, "SnapshotChanged: reload")
        let restart = h.fake.last("Snapshot")!
        XCTAssertEqual(restart.payload["cursor"], .null)
        XCTAssertEqual(restart.payload["after"], .text(""))
        XCTAssertEqual(model.rows, [])
    }

    func test_t14_strict_json_and_envelope_fail_closed() throws {
        for bad in [#"{"a":1,"a":2}"#, #"{"a":1.5}"#, #"{"a":1e3}"#, #"{"a":"\u0000"}"#, #"{"a":"\ud800"}"#, "{\"a\":1} x",
                    String(repeating: "[", count: 40) + String(repeating: "]", count: 40)] {
            XCTAssertThrowsError(try StrictJSON.parse(Array(bad.utf8)), bad)
        }
        let unicode = JSON.object(["k": .text("é\u{1F600}\n\"")])
        XCTAssertEqual(try StrictJSON.parse(Array(StrictJSON.encode(unicode).utf8)), unicode)
        let ok = Envelope(session: artifact, epoch: 1, id: artifact, type: "Hello")
        XCTAssertEqual(try Envelope.parse(Array(try ok.frame().dropLast())), ok)
        var nine = StrictJSON.encode(ok.json)
        nine.insert(contentsOf: #""extra":1,"#, at: nine.index(after: nine.startIndex))
        XCTAssertThrowsError(try Envelope.parse(Array(nine.utf8)))
        let nullError = #"{"epoch":1,"id":"\#(artifact)","major":1,"minor":0,"payload":null,"revision":null,"session":"\#(artifact)","type":"Error"}"#
        XCTAssertThrowsError(try Envelope.parse(Array(nullError.utf8)), "null payload only on Response")
        let nullResponse = nullError.replacingOccurrences(of: "\"Error\"", with: "\"Response\"")
        XCTAssertTrue(try Envelope.parse(Array(nullResponse.utf8)).payloadIsNull)
        let listResponse = nullResponse.replacingOccurrences(of: "\"payload\":null", with: "\"payload\":[]")
        XCTAssertEqual(try Envelope.parse(Array(listResponse.utf8)).payloadList, [], "Events answers with a list")
        XCTAssertThrowsError(try Envelope.parse(Array(listResponse.replacingOccurrences(of: "\"Response\"", with: "\"Hello\"").utf8)))
        XCTAssertThrowsError(try Envelope.parse(Array(nullResponse.replacingOccurrences(of: artifact, with: "ABC").utf8)))
    }

    func test_t14_fragmented_and_oversized_frames() throws {
        var s = FrameSplitter()
        let frame = try Envelope(session: artifact, epoch: 1, id: artifact, type: "Hello").frame()
        var got: [[UInt8]] = []
        for chunk in stride(from: 0, to: frame.count, by: 7) {
            got += try s.push(frame[chunk..<min(chunk + 7, frame.count)])
        }
        XCTAssertEqual(got, [Array(frame.dropLast())])
        XCTAssertNoThrow(try s.finish())
        _ = try s.push([0x7B])
        XCTAssertThrowsError(try s.finish(), "truncated frame")
        var big = FrameSplitter()
        XCTAssertThrowsError(try big.push([UInt8](repeating: 0x20, count: Envelope.maxFrame + 1)))
        let huge = Envelope(session: artifact, epoch: 1, id: artifact, type: "X", payload: ["p": .text(String(repeating: "a", count: Envelope.maxFrame))])
        XCTAssertThrowsError(try huge.frame())
    }

    func test_t14_client_ids_are_random_low_half_and_malformed_helper_output_loses_channel() {
        let ids = (0..<64).map { _ in Identity.randomClientID() }
        XCTAssertEqual(Set(ids).count, 64, "durable receipts span sessions; IDs must not repeat")
        XCTAssertTrue(ids.allSatisfy { Identity.valid($0) && Int($0.prefix(1), radix: 16)! < 8 })
        let h = Harness()
        var answer: Reply?
        h.client.query("CloseStatus") { answer = $0 }
        h.fake.onEvent?(.frame(Array(#"{"not":"an envelope"}"#.utf8)))
        guard case .channelLost = h.client.connection, case .unanswered = answer else { return XCTFail("\(h.client.connection)") }
        XCTAssertNil(h.client.command("PrepareClose", [:]) { _ in }, "no mutation over a lost channel")
    }

    func test_t14_epoch_change_or_protocol_error_frame_loses_channel() {
        let h = Harness()
        h.client.query("CloseStatus") { _ in }
        let q = h.fake.last("CloseStatus")!
        h.fake.deliver(Envelope(session: q.session, epoch: 8, id: q.id, type: "Response"))
        guard case .channelLost = h.client.connection else { return XCTFail() }
        let p = Harness()
        p.fake.deliver(Envelope(session: Envelope.zeroID, epoch: 0, id: Envelope.zeroID, type: "Error", payload: ["code": .text("ProtocolError: x")]))
        guard case .channelLost = p.client.connection else { return XCTFail() }
    }
}

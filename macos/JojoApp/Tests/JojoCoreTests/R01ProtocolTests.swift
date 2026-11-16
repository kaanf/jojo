@testable import JojoCore
import XCTest

/// R-01: the client names requests through one typed definition, sends exactly their
/// fields, and reads the recovery assessment from structured fields, never from text.
@MainActor
final class R01ProtocolTests: XCTestCase {
    private let attempt = "000000000000000000000000000000a2"

    func test_r01_every_request_has_a_distinct_name_and_no_repeated_field() {
        XCTAssertEqual(Set(Request.allCases.map(\.rawValue)).count, Request.allCases.count)
        for request in Request.allCases {
            XCTAssertEqual(Set(request.fields).count, request.fields.count, request.rawValue)
            XCTAssertTrue(request.accepts(Dictionary(uniqueKeysWithValues: request.fields.map { ($0, JSON.null) })))
        }
        XCTAssertFalse(Request.Start.accepts(["prd": .null]))
        XCTAssertFalse(Request.RecoveryStart.accepts(["digest": .null, "executable": .null]))
        XCTAssertFalse(Request.CloseStatus.accepts(["junk": .null]))
    }

    func test_r01_a_payload_that_is_not_the_requests_fields_is_never_sent() {
        let h = Harness()
        var replies: [Reply] = []
        h.client.query(.Recovery, ["offset": .int(0)]) { replies.append($0) }
        XCTAssertNil(h.client.command(.RecoveryStart, ["digest": .text("d"), "executable": .text("/bin/sh")]) { replies.append($0) })
        XCTAssertNil(h.client.command(.Start, [:]) { replies.append($0) })
        XCTAssertEqual(replies, [.error("InvalidClientPayload: Recovery"), .error("InvalidClientPayload: RecoveryStart"),
                                 .error("InvalidClientPayload: Start")])
        XCTAssertEqual(h.fake.sent.map(\.type), ["Hello"])
        XCTAssertTrue(h.client.outstandingCommands.isEmpty)

        // The same requests with exactly their fields are sent under their protocol names.
        h.client.query(.Recovery, ["offset": .int(0), "limit": .int(8)]) { replies.append($0) }
        let id = h.client.command(.RecoveryStart, ["digest": .text("d")]) { replies.append($0) }
        XCTAssertEqual(h.fake.sent.map(\.type), ["Hello", "Recovery", "RecoveryStart"])
        XCTAssertEqual(h.fake.last("RecoveryStart")?.payload, ["digest": .text("d")])
        XCTAssertEqual(h.client.outstandingCommands, [id!: "RecoveryStart"])
    }

    private func load(_ row: [String: JSON]) -> RecoveryRow? {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.refresh()
        let request = h.fake.last("Recovery")!
        XCTAssertEqual(request.payload, ["offset": .int(0), "limit": .int(8)])
        h.fake.reply(request, ["digest": .text("d1"), "rows": .list([.object(row)])])
        return model.recovery.first
    }

    func test_r01_recovery_row_is_read_from_structured_fields() {
        let text = "Assessment { blockers: [\"live orphan pid:7\"], process_absent: false, choices: [Hold] }"
        var row: [String: JSON] = ["attempt": .text(attempt), "assessment": .text(text), "findings": .text("stage=Spawned"),
                                   "detail_complete": .bool(true), "blockers": .list([.text("live orphan pid:7")]),
                                   "blockers_complete": .bool(true), "process_absent": .bool(false),
                                   "choices": .list([.text("Hold")])]
        let held = load(row)
        XCTAssertEqual(held?.assessment, text)
        XCTAssertEqual(held?.structured?.blockers, ["live orphan pid:7"])
        XCTAssertEqual(held?.structured?.blockersComplete, true)
        XCTAssertEqual(held?.structured?.processAbsent, false)
        XCTAssertEqual(held?.structured?.choices, [.Hold])
        XCTAssertEqual(held?.structured?.choicesText, "[Hold]")

        // The structured fields decide; text that says something else is not consulted.
        row["assessment"] = .text("Assessment { blockers: [], process_absent: true, choices: [Hold] }")
        row["blockers"] = .list([])
        row["process_absent"] = .bool(true)
        row["choices"] = .list(["ValidateOutput", "ContinueFreshContext", "Hold"].map(JSON.text))
        let free = load(row)
        XCTAssertEqual(free?.structured?.choices, [.ValidateOutput, .ContinueFreshContext, .Hold])
        XCTAssertEqual(free?.structured?.choicesText, "[ValidateOutput, ContinueFreshContext, Hold]")
        XCTAssertEqual(free?.structured?.processAbsent, true)
    }

    func test_r01_a_row_without_complete_structured_fields_offers_nothing_from_its_text() {
        let text = "Assessment { blockers: [], process_absent: true, choices: [ValidateOutput, Hold] }"
        let old: [String: JSON] = ["attempt": .text(attempt), "assessment": .text(text), "findings": .text("f"),
                                   "detail_complete": .bool(true)]
        // An older helper: the row is listed, and nothing is derived from its text.
        let listed = load(old)
        XCTAssertEqual(listed?.assessment, text)
        XCTAssertNil(listed?.structured)

        var partial = old
        partial["choices"] = .list([.text("Hold")])
        XCTAssertNil(load(partial)?.structured)

        var unknown = old
        unknown["blockers"] = .list([])
        unknown["blockers_complete"] = .bool(true)
        unknown["process_absent"] = .bool(true)
        unknown["choices"] = .list([.text("Hold"), .text("Adopt")])
        XCTAssertNil(load(unknown)?.structured, "a choice this client does not know is not dropped silently")
        unknown["choices"] = .list([.text("Hold")])
        XCTAssertEqual(load(unknown)?.structured?.choices, [.Hold])
    }
}

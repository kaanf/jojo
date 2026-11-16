@testable import JojoCore
import XCTest

/// P2-03: the GUI side of validation and candidate acceptance. The GUI shows the helper's
/// verdict and sends identities; it never derives a pass from anything else.
@MainActor
final class P203CandidateReviewTests: XCTestCase {
    private let attempt = "000000000000000000000000000000a2"
    private let digest = String(repeating: "ab", count: 32)

    private func view(_ state: String?, receipt: JSON = .null, validated: String? = nil, decision: String = "NONE",
                      detail: String = "", reasons: [String] = [], terminal: String = "turn.completed") -> [String: JSON] {
        var validation: [String: JSON] = ["detail": .text(detail), "receipt": receipt,
                                          "candidate": validated.map(JSON.text) ?? .null,
                                          "reasons": .list(reasons.map(JSON.text)), "reasons_total": .int(Int64(reasons.count))]
        if let state { validation["state"] = .text(state) }
        return ["attempt": .text(attempt), "sealed": .bool(true), "outcome": .text("Succeeded"), "reason": .text("ok"),
                "provider_terminal": .text(terminal), "candidate": .text(digest), "log_records": .int(1),
                "validation": .object(validation), "decision": .text(decision)]
    }

    /// Loads a candidate and answers the follow-up queries the model sends.
    private func load(_ h: Harness, _ model: WorkspaceModel, _ payload: [String: JSON], log: String = "agent output",
                      changes: [String: JSON] = ["scope": .text("PASS"), "rows": .list([])], refused: String? = nil) {
        model.loadCandidate(attempt)
        h.fake.reply(h.fake.last("Candidate")!, payload)
        if let refused {
            h.fake.error(h.fake.last("CandidateChanges")!, refused)
        } else {
            h.fake.reply(h.fake.last("CandidateChanges")!, changes)
        }
        h.fake.reply(h.fake.last("CandidateDiff")!, ["text": .text("+line\n"), "next": .int(6), "total": .int(6), "complete": .bool(true)])
        if payload["validation"]?["receipt"]?.integer != nil {
            h.fake.reply(h.fake.last("ValidationReport")!, ["text": .text("report"), "next": .int(6), "total": .int(6), "complete": .bool(true)])
        }
        h.fake.reply(h.fake.last("Log")!, ["type": .text("AgentMessage"), "text": .text(log)])
    }

    func test_p2_03_four_states_are_distinct_and_only_a_recorded_pass_is_a_pass() {
        XCTAssertEqual(Set([ValidationVerdict.pass, .fail, .unknown, .notRun].map(\.label)),
                       ["PASS", "FAIL", "UNKNOWN", "NOT RUN"])
        // Anything unrecognized, missing or malformed is UNKNOWN, never a pass.
        for odd in ["Passed", "pass", "SUCCEEDED", "OK", "", "PASS ", "tests passed"] {
            XCTAssertEqual(ValidationVerdict(helper: odd), .unknown, odd)
        }
        XCTAssertEqual(ValidationVerdict(helper: nil), .unknown)
        XCTAssertEqual(CandidateReview(view(nil))?.validation, .unknown)
        XCTAssertEqual(CandidateReview(["attempt": .text(attempt), "sealed": .bool(false)])?.validation, .notRun)
        XCTAssertNil(CandidateReview(["attempt": .text("/bin/sh"), "sealed": .bool(true)]))
        XCTAssertEqual(CandidateReview(view("PASS", receipt: .int(31), validated: digest))?.validation, .pass)
    }

    func test_p2_03_provider_claim_of_passing_tests_never_produces_a_pass_or_an_accept() {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        // The provider ended successfully and its output says the tests passed.
        load(h, model, view("NOT_RUN", terminal: "turn.completed"), log: "All tests passed! 12 passed, 0 failed.")
        XCTAssertEqual(model.agentLog, ["AgentMessage: All tests passed! 12 passed, 0 failed."])
        XCTAssertEqual(model.candidate?.validation, .notRun)
        XCTAssertEqual(model.candidate?.canAccept, false)
        model.decideCandidate(.Accept)
        XCTAssertEqual(h.fake.count("CandidateDecision"), 0, "nothing is sent without a passing host validation")
        XCTAssertTrue(model.status.contains("validation is NOT RUN"), model.status)

        // FAIL, UNKNOWN, a pass of another candidate, a pass without a receipt, a decided
        // candidate: none of them can be accepted from the GUI.
        for payload in [view("FAIL", receipt: .int(31), validated: digest, reasons: ["out-of-scope change \"README.md\""]),
                        view("UNKNOWN", detail: "validation is running"),
                        view("PASS", receipt: .int(31), validated: String(repeating: "cd", count: 32)),
                        view("PASS", validated: digest),
                        view("PASS", receipt: .int(31), validated: digest, decision: "REJECTED")] {
            load(h, model, payload)
            XCTAssertEqual(model.candidate?.canAccept, false)
            model.decideCandidate(.Accept)
        }
        XCTAssertEqual(h.fake.count("CandidateDecision"), 0)
    }

    func test_p2_03_validate_and_accept_bind_the_displayed_candidate_and_receipt() {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.validateCandidate()
        XCTAssertEqual(h.fake.count("Validate"), 0, "nothing loaded, nothing sent")
        load(h, model, view("NOT_RUN"))
        model.validateCandidate()
        let validate = h.fake.last("Validate")!
        XCTAssertEqual(validate.payload, ["attempt": .text(attempt), "candidate": .text(digest)],
                       "identities only: no command, argument or path can be sent")
        model.validateCandidate()
        XCTAssertEqual(h.fake.count("Validate"), 1, "an unanswered request is not sent again")
        h.fake.ack(validate, "validation requested")
        XCTAssertFalse(model.commandPending)

        load(h, model, view("PASS", receipt: .int(31), validated: digest),
             changes: ["scope": .text("PASS"), "rows": .list([.object([
                 "path": .text("src/feature.rs"), "kind": .text("Added"), "scope": .text("ALLOWED")])])])
        XCTAssertEqual(model.changedFiles, [ChangedFile(path: "src/feature.rs", kind: "Added", scope: "ALLOWED")])
        XCTAssertEqual(model.scope, .pass)
        XCTAssertEqual(model.diff, "+line\n")
        XCTAssertEqual(model.validationReport, "report")
        XCTAssertEqual(model.candidate?.canAccept, true)
        model.decideCandidate(.Accept)
        let accept = h.fake.last("CandidateDecision")!
        XCTAssertEqual(accept.payload, ["attempt": .text(attempt), "candidate": .text(digest), "receipt": .int(31),
                                        "action": .text("Accept")])
        // A stale refusal is shown and nothing is retried.
        h.fake.error(accept, "validation is stale against current approvals")
        XCTAssertEqual(model.status, "Accept refused (validation is stale against current approvals); the candidate is unchanged")
        XCTAssertEqual(h.fake.count("CandidateDecision"), 1)

        model.decideCandidate(.Reject)
        XCTAssertEqual(h.fake.last("CandidateDecision")!.payload["receipt"], .null)
        XCTAssertEqual(h.fake.last("CandidateDecision")!.payload["action"], .text("Reject"))
    }

    func test_p2_03_uninspectable_candidate_and_lost_response_stay_unknown() {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        load(h, model, view("UNKNOWN", detail: "sealed candidate packet rejected: snapshot digest/size mismatch"),
             refused: "CandidateUnavailable: sealed candidate packet rejected")
        XCTAssertEqual(model.scope, .unknown)
        XCTAssertEqual(model.scopeDetail, "CandidateUnavailable: sealed candidate packet rejected")
        XCTAssertEqual(model.candidate?.canAccept, false)

        model.validateCandidate()
        let validate = h.fake.last("Validate")!
        h.fake.breakChannel("pipe closed")
        XCTAssertTrue(model.status.contains("outcome unknown"), model.status)
        XCTAssertTrue(model.commandPending)
        XCTAssertEqual(h.client.outstandingCommands, [validate.id: "Validate"], "kept for receipt reconciliation")
        model.validateCandidate()
        model.decideCandidate(.Reject)
        XCTAssertEqual(h.fake.count("Validate"), 1)
        XCTAssertEqual(h.fake.count("CandidateDecision"), 0)
    }
}

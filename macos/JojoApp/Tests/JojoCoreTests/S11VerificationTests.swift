import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

@MainActor
final class S11VerificationTests: XCTestCase {
    private func state(_ result: CandidateReview? = CandidateRows.result(), detail: AttemptDetail? = CandidateRows.detail,
                       pending: Bool = false, connection: HelperConnection = .ready(epoch: 7)) -> VerificationState {
        VerificationState(connection: connection, admission: "Open", scope: DetailRows.scope, candidate: result,
                          files: CandidateRows.files, verdict: .fail, report: "Ran 3 tests\nOK", commandPending: pending,
                          detail: detail, candidates: CandidateRows.candidates, now: DetailRows.now)
    }

    func test_s11_the_verdict_is_the_core_s_and_unknown_is_never_passed() {
        XCTAssertEqual(state().headline.title, "V-2 passed")
        XCTAssertEqual(state().headline.pill.0, "Host validation passed")
        XCTAssertEqual(state(CandidateRows.result(validation: "UNKNOWN")).headline.pill.0, "Not verified")
        XCTAssertEqual(state(CandidateRows.result(validation: "FAIL")).headline.title, "V-2 failed")
        XCTAssertEqual(state(detail: nil).headline.title, "Host validation passed")
        XCTAssertEqual(state().counts.map(\.1), [1, 1, 0], "one run passed, one failed, none without a verdict")
        XCTAssertEqual(state(CandidateRows.result(validation: "UNKNOWN")).counts.map(\.1)[2], 1)
    }

    func test_s11_accept_follows_the_core_rule_and_actions_need_a_connected_core() {
        XCTAssertNil(state().acceptBlocked)
        XCTAssertNil(state().validateBlocked)
        XCTAssertEqual(state(CandidateRows.result(validation: "FAIL")).acceptBlocked,
                       "Unavailable: the core accepts only a passing host validation of exactly this candidate.")
        XCTAssertEqual(state(CandidateRows.result(decision: "REJECTED")).rejectBlocked, "Already decided: REJECTED.")
        XCTAssertEqual(state(CandidateRows.result(sealed: false)).validateBlocked, "No sealed candidate.")
        XCTAssertEqual(state(connection: .channelLost("x")).acceptBlocked, "Unavailable: the core is not connected.")
        XCTAssertEqual(state(pending: true).rejectBlocked, "Unavailable while a command awaits the core.")
    }

    func test_s11_the_seal_is_not_available_and_says_which_requirements_are_met() {
        let checks = state().sealChecks
        XCTAssertEqual(checks.map(\.0).last, "Deployment receipt present")
        XCTAssertEqual(checks.last?.1, false, "deployment receipts are not available yet")
        XCTAssertEqual(checks.first?.1, true)
        XCTAssertEqual(checks.first { $0.0 == "Candidate accepted" }?.1, false)
        XCTAssertEqual(state(CandidateRows.result(decision: "ACCEPTED")).sealChecks.first { $0.0 == "Candidate accepted" }?.1, true)
    }

    func test_s11_the_export_is_the_records_as_they_are_marked_unsealed() {
        let bundle = state().bundle
        XCTAssertTrue(bundle.hasPrefix("# Evidence bundle (unsealed)"))
        XCTAssertTrue(bundle.contains("- V-1 · failed"))
        XCTAssertTrue(bundle.contains("  - cli: exit 1"))
        XCTAssertTrue(bundle.contains("- Added cli.py (ALLOWED)"))
        XCTAssertTrue(bundle.contains("Exporting does not seal."))
        XCTAssertEqual(state(nil).bundle, "")
    }

    /// Review aid: with JOJO_SNAPSHOT_DIR set, writes Verification & Evidence in its states.
    func test_s11_verification_renders_in_both_appearances() throws {
        let heading = ("Verification & Evidence", DetailRows.scope.place(nil))
        let controls = AnyView(HeaderButtons(buttons: [HeaderButton(title: "Inspect Failure", disabled: "No failed validation loaded."),
                                                       HeaderButton(title: "Compare Validation Runs", disabled: "Not available yet"),
                                                       HeaderButton(title: "Export Evidence Bundle")]))
        let cases: [(String, VerificationState, CGSize)] = [
            ("s11-verification", state(), CGSize(width: 1440, height: 1240)),
            ("s11-verification-failed", state(CandidateRows.result(validation: "FAIL")), CGSize(width: 1440, height: 1240)),
            ("s11-verification-accepted", state(CandidateRows.result(decision: "ACCEPTED")), CGSize(width: 1440, height: 1240)),
            ("s11-verification-none", state(nil, detail: nil), CGSize(width: 1440, height: 1000)),
            ("s11-verification-narrow", state(), CGSize(width: 900, height: 2000)),
            ("s11-verification-disconnected", state(connection: .channelLost("pipe closed")), CGSize(width: 1440, height: 1240)),
        ]
        for (file, state, size) in cases {
            try DetailRows.render(VerificationView(state: state), screen: .verification, heading: heading, controls: controls,
                                  size: size, file: file)
        }
    }
}

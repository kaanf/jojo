import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

@MainActor
final class S7ApprovalInboxTests: XCTestCase {
    static let decisions: [String: JSON] = [
        "decisions": .list([
            .object(["order": .int(80), "document": .text(DetailRows.id(2)), "kind": .text("Adr"), "title": .null,
                     "feature": .text(DetailRows.feature), "feature_name": .text("Slugify from the shell"), "revision": .int(1),
                     "decision": .text("Approved"), "applies": .bool(true), "at_unix_ms": .int(DetailRows.t0 + 200_000)]),
            .object(["order": .int(70), "document": .text(DetailRows.id(1)), "kind": .text("Prd"), "title": .text("Slugify from the shell"),
                     "feature": .text(DetailRows.feature), "feature_name": .text("Slugify from the shell"), "revision": .int(1),
                     "decision": .text("Approved"), "applies": .bool(false), "at_unix_ms": .int(DetailRows.t0 + 90_000)]),
        ]),
        "next": .null,
    ]

    private func state(selected: String? = nil, filter: InboxFilter = .pending, type: InboxType = .all,
                       review: ReviewBundle? = nil, connection: HelperConnection = .ready(epoch: 7),
                       list: DocumentList? = DetailRows.documents) -> ApprovalInboxState {
        let decisions = (Self.decisions["decisions"]?.items ?? []).compactMap(DecisionSummary.init)
        return ApprovalInboxState(connection: connection, admission: "Open", list: list, review: review, selected: selected,
                                  detail: DocumentDetail(DetailRows.prd()), decisions: decisions, decisionsLoad: .loaded,
                                  features: FeatureList(features: [DetailRows.summary]), filter: filter, type: type, now: DetailRows.now)
    }

    func test_s7_decisions_decode_at_1_9_and_an_approval_that_no_longer_applies_is_invalidated() throws {
        let decisions = (Self.decisions["decisions"]?.items ?? []).compactMap(DecisionSummary.init)
        XCTAssertEqual(decisions.count, 2)
        XCTAssertEqual(ApprovalInboxState.decisionPill(decisions[0]).0, "Approved")
        XCTAssertEqual(ApprovalInboxState.decisionPill(decisions[1]).0, "Invalidated")
        XCTAssertEqual(Request.Decisions.minor, 9)
        XCTAssertEqual(Request.Decisions.fields, ["before"])
    }

    func test_s7_pending_lists_what_waits_newest_first_and_all_lists_every_request() {
        let pending = state()
        XCTAssertEqual(pending.items.map(\.id), [DetailRows.id(5), DetailRows.id(3), DetailRows.id(1), DetailRows.id(8)])
        XCTAssertEqual(pending.chosen?.kind, "Candidate", "without a choice, the newest request")
        XCTAssertEqual(state(filter: .all).items.count, 5)
        XCTAssertEqual(state(type: .plans).items.map(\.id), [DetailRows.id(3)])
        XCTAssertEqual(state(type: .specs).items.map(\.id), [DetailRows.id(1), DetailRows.id(8)])
        XCTAssertTrue(state(type: .external).items.isEmpty, "these rows hold no push or pull request request (S12 has them)")
        XCTAssertEqual(pending.place(pending.items[1]), "Sample · FT-3")
        XCTAssertEqual(pending.age(pending.items[2]), "Stale")
        XCTAssertEqual(pending.age(pending.items[1]), "5m")
    }

    func test_s7_a_document_is_decided_only_through_its_own_current_review() {
        let id = DetailRows.id(3)
        XCTAssertEqual(state(selected: id).decideBlocked, "Loading the review of this revision.")
        let ready = state(selected: id, review: DetailRows.bundle(id, revision: 1))
        XCTAssertNil(ready.decideBlocked)
        XCTAssertEqual(ready.status.0, "Awaiting approval")
        XCTAssertEqual(state(selected: id, review: DetailRows.bundle(id, revision: 1), connection: .channelLost("x")).decideBlocked,
                       "Unavailable: the core is not connected.")
        var outstanding = ready
        outstanding.decisionOutstanding = true
        XCTAssertEqual(outstanding.status.0, "Requested")
        XCTAssertEqual(state().decideBlocked, "No document chosen.", "a candidate is decided on Verification & Evidence")
    }

    func test_s7_the_request_says_what_it_asks_and_what_it_affects() throws {
        let plan = try XCTUnwrap(DetailRows.documents[DetailRows.id(3)])
        XCTAssertTrue(ApprovalInboxState.action(plan).hasPrefix("Approve plan rev 1 of Slugify from the shell."))
        XCTAssertEqual(ApprovalInboxState.kind("ExecutionPlan").approve, "Approve Plan")
        let prd = try XCTUnwrap(DetailRows.documents[DetailRows.id(1)])
        XCTAssertEqual(state(selected: prd.id).affected(prd), "ADR rev 1")
        XCTAssertEqual(ApprovalInboxState.confirmation(.Reject, prd)?.confirm, "Reject Revision")
        XCTAssertNil(ApprovalInboxState.confirmation(.Approve, prd), "an approval is sent at once")
    }

    /// Review aid: with JOJO_SNAPSHOT_DIR set, writes the Approval Inbox in its states.
    func test_s7_approval_inbox_renders_in_both_appearances() throws {
        let heading = ("Approval Inbox", "4 pending · no approve-all")
        let controls = AnyView(HeaderButtons(buttons: [HeaderButton(title: "Approval History")]))
        let plan = DetailRows.id(3)
        let cases: [(String, ApprovalInboxState, CGSize)] = [
            ("s7-approval-inbox", state(selected: DetailRows.id(1), review: DetailRows.bundle(DetailRows.id(1))), CGSize(width: 1440, height: 1160)),
            ("s7-approval-inbox-plan", state(selected: plan, review: DetailRows.bundle(plan, revision: 1)), CGSize(width: 1440, height: 1160)),
            ("s7-approval-inbox-candidate", state(), CGSize(width: 1440, height: 1160)),
            ("s7-approval-inbox-external", state(type: .external), CGSize(width: 1440, height: 1160)),
            ("s7-approval-inbox-empty", state(list: DocumentList(documents: [])), CGSize(width: 1440, height: 1000)),
            ("s7-approval-inbox-narrow", state(selected: DetailRows.id(1)), CGSize(width: 900, height: 1800)),
        ]
        for (file, state, size) in cases {
            try DetailRows.render(ApprovalInboxView(state: state), screen: .approvalInbox, heading: heading, controls: controls,
                                  size: size, file: file)
        }
    }
}

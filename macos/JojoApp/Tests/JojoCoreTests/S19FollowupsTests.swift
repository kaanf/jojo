import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

/// Follow-ups (protocol 1.14): the sidebar counters of Execution Plan and Feature Chat,
/// choice answers, session continuation shown on a turn, the repair of a failed turn, and
/// the advisory review of a candidate started by hand or of a drafted document.
enum FollowupRows {
    static let attempt = String(repeating: "5a", count: 16)

    static func review(state: String = "Approved", provider: String = "Codex") -> JSON {
        .object(["review": .text(String(repeating: "6e", count: 16)), "provider": .text(provider), "state": .text(state),
                 "notes": .text(state == "Approved" ? "Checked the criteria against the code." : ""),
                 "objection": .null, "detail": .text(""), "revision": .int(2),
                 "requested_at_unix_ms": .int(1_763_400_000_000),
                 "finished_at_unix_ms": state == "Running" ? .null : .int(1_763_400_060_000)])
    }

    static func advisory(review: JSON = .null, available: Bool = true, reason: String? = nil) -> JSON {
        .object(["review": review, "available": .bool(available),
                 "reviewer": available ? .text("Codex") : .null, "reason": reason.map(JSON.text) ?? .null])
    }

    /// A failed first turn, newest of its feature, with the 1.14 fields.
    static func failedTurn(repairable: Bool = true, repairedBy: String? = nil) -> JSON {
        guard case .object(var turn) = ChatRows.turn(1, state: "Failed", provider: "Codex") else { return .null }
        turn["choices"] = .list([])
        turn["session"] = .text("Fresh")
        turn["repairs"] = .null
        turn["repaired_by"] = repairedBy.map(JSON.text) ?? .null
        turn["repairable"] = .bool(repairable)
        return .object(turn)
    }
}

@MainActor
final class S19FollowupsTests: XCTestCase {
    private func ready() -> (Harness, WorkspaceModel) {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.refresh()
        h.fake.reply(h.fake.last("Snapshot")!, ["rows": .list([]), "as_of_event_seq": .int(30), "admission": .text("Open")])
        return (h, model)
    }

    func test_s19_the_new_requests_and_the_reads_that_carry_them_go_at_1_14() throws {
        XCTAssertEqual(Request.allCases.filter { $0.minor == 14 },
                       [.FeatureChat, .Documents, .Document, .AttemptDetail, .RepairTurn, .ReviewAttempt, .ReviewDocument])
        XCTAssertEqual(Request.RepairTurn.fields, ["turn", "note"])
        XCTAssertEqual(Request.ReviewAttempt.fields, ["attempt", "candidate"])
        XCTAssertEqual(Request.ReviewDocument.fields, ["document", "revision"])
        let (h, model) = ready()
        XCTAssertEqual(h.fake.last("Documents")?.minor, 14)
        model.repairTurn(ChatRows.turnID(1), note: "Mind the JSON.")
        let repair = try XCTUnwrap(h.fake.last("RepairTurn"))
        XCTAssertEqual(repair.minor, 14)
        XCTAssertEqual(repair.payload, ["turn": .text(ChatRows.turnID(1)), "note": .text("Mind the JSON.")])
        h.fake.reply(repair, ["response": .text("accepted"), "turn": .text(ChatRows.turnID(2)), "dispatch": .text("launched")])
        XCTAssertEqual(model.turnOutcome, .accepted(attempt: ChatRows.turnID(2), dispatch: "launched"))
        model.reviewAttempt(FollowupRows.attempt, candidate: "c1")
        let attempt = try XCTUnwrap(h.fake.last("ReviewAttempt"))
        XCTAssertEqual(attempt.payload, ["attempt": .text(FollowupRows.attempt), "candidate": .text("c1")])
        h.fake.reply(attempt, ["response": .text("accepted"), "review": .text("r"), "dispatch": .text("launched")])
        XCTAssertEqual(h.fake.last("AttemptDetail")?.payload, ["attempt": .text(FollowupRows.attempt)])
        model.reviewDocument(DetailRows.id(1), revision: 2)
        let document = try XCTUnwrap(h.fake.last("ReviewDocument"))
        XCTAssertEqual(document.payload, ["document": .text(DetailRows.id(1)), "revision": .int(2)])
        h.fake.error(document, "ReviewRefused: another review is running")
        XCTAssertEqual(model.status, "Review refused (ReviewRefused: another review is running); nothing was run")
    }

    func test_s19_the_documents_answer_counts_waiting_steps_and_open_questions_for_the_sidebar() throws {
        let list = try XCTUnwrap(DocumentList([
            "documents": .list([]), "candidates": .list([]), "pending": .int(0), "next": .null,
            "steps_waiting": .int(2),
            "open_questions": .list([.object(["feature": .text(ChatRows.feature), "count": .int(3)])]),
        ]))
        XCTAssertEqual(list.stepsWaiting, 2)
        XCTAssertEqual(list.openQuestions, [ChatRows.feature: 3])
        // An answer before 1.14 counts nothing.
        let older = try XCTUnwrap(DocumentList(["documents": .list([]), "candidates": .list([]), "pending": .int(0)]))
        XCTAssertEqual(older.stepsWaiting, 0)
        XCTAssertEqual(older.openQuestions, [:])
        XCTAssertNil(DocumentList(["documents": .list([]), "candidates": .list([]), "pending": .int(0),
                                   "open_questions": .list([.object(["feature": .text("f")])])]))
        let (h, model) = ready()
        h.fake.reply(h.fake.last("Documents")!, ["documents": .list([]), "candidates": .list([]), "pending": .int(0),
                                                "steps_waiting": .int(2),
                                                "open_questions": .list([.object(["feature": .text(ChatRows.feature),
                                                                                  "count": .int(3)])])])
        XCTAssertEqual(SidebarCounts.openQuestions(model), 0, "no feature is held")
        model.openChat(ChatRows.feature)
        XCTAssertEqual(SidebarCounts.openQuestions(model), 3)
        XCTAssertEqual(model.documents?.stepsWaiting, 2)
    }

    func test_s19_a_question_offers_its_choices_and_a_turn_says_how_it_used_the_session() throws {
        guard case .object(var turn) = ChatRows.turn(2, questions: [
            ChatRows.question(1, prompt: "Where from?", blocking: true),
            ChatRows.question(2, prompt: "Packaging?", blocking: false),
        ]) else { return XCTFail() }
        turn["choices"] = .list([.list([.text("Arguments"), .text("Standard input")]), .list([])])
        turn["session"] = .text("Continued")
        turn["repairs"] = .text(ChatRows.turnID(1))
        turn["repaired_by"] = .null
        turn["repairable"] = .bool(false)
        let page = try XCTUnwrap(FeatureChatPage(ChatRows.page([FollowupRows.failedTurn(repairable: false,
                                                                                         repairedBy: ChatRows.turnID(2)),
                                                                .object(turn)])))
        let parsed = page.turns[1]
        XCTAssertEqual(parsed.questions.map(\.choices), [["Arguments", "Standard input"], []])
        XCTAssertEqual(parsed.session, "Continued")
        XCTAssertEqual(parsed.repairs, ChatRows.turnID(1))
        XCTAssertEqual(page.turns[0].repairedBy, ChatRows.turnID(2))
        // A list that does not match the questions is not a chat this app reads.
        turn["choices"] = .list([.list([])])
        XCTAssertNil(FeatureChatPage(ChatRows.page([.object(turn)])))
        // An older turn has none of it.
        let old = try XCTUnwrap(FeatureChatPage(ChatRows.page([ChatRows.turn(1)])))
        XCTAssertNil(old.turns[0].session)
        XCTAssertFalse(old.turns[0].repairable)
        let state = FeatureChatState(connection: .ready(epoch: 7), admission: "Open", queries: ["FeatureChat"],
                                     index: ChatIndex(ChatRows.index), indexLoad: .loaded, feature: ChatRows.feature,
                                     chat: page, chatLoad: .loaded)
        XCTAssertTrue(state.separator(page.turns[1]).hasSuffix("Continued session"))
        XCTAssertEqual(FeatureChatState.session("Fallback"), "New session (the continuation failed)")
        XCTAssertNil(FeatureChatState.session("Fresh"))
        XCTAssertEqual(state.turnNumber(ChatRows.turnID(2)), "T2")
    }

    func test_s19_a_failed_turn_is_repaired_only_when_the_core_offers_it() throws {
        func state(_ turn: JSON, connection: HelperConnection = .ready(epoch: 7), pending: Bool = false,
                   note: String = "") -> (FeatureChatState, LeadTurn) {
            let page = FeatureChatPage(ChatRows.page([turn]))!
            let s = FeatureChatState(connection: connection, admission: "Open", queries: ["FeatureChat"],
                                     index: ChatIndex(ChatRows.index), indexLoad: .loaded, feature: ChatRows.feature,
                                     chat: page, chatLoad: .loaded, commandPending: pending, repairNote: note)
            return (s, page.turns[0])
        }
        let (open, failed) = state(FollowupRows.failedTurn())
        XCTAssertNil(open.repairBlocked(failed))
        XCTAssertEqual(state(FollowupRows.failedTurn(repairable: false)).0
                        .repairBlocked(state(FollowupRows.failedTurn(repairable: false)).1),
                       "Only the newest failed turn can be repaired.")
        XCTAssertEqual(state(FollowupRows.failedTurn(), pending: true).0.repairBlocked(failed),
                       "Unavailable while a command awaits the core.")
        XCTAssertEqual(state(FollowupRows.failedTurn(), note: String(repeating: "x", count: 4097)).0.repairBlocked(failed),
                       "The note takes at most 4 KiB.")
        XCTAssertEqual(state(FollowupRows.failedTurn(), connection: .channelLost("gone")).0.repairBlocked(failed),
                       "Unavailable: the core is not connected.")
    }

    func test_s19_an_advisory_review_is_decoded_and_offered_only_when_the_core_says_so() throws {
        let offer = try XCTUnwrap(AdvisoryOffer(FollowupRows.advisory(review: FollowupRows.review())))
        XCTAssertEqual(offer.review?.state, "Approved")
        XCTAssertEqual(offer.review?.provider, "Codex")
        XCTAssertEqual(offer.reviewer, "Codex")
        XCTAssertNotNil(offer.review?.finished)
        XCTAssertNil(AdvisoryOffer(nil), "an older helper reports none")
        XCTAssertNil(AdvisoryOffer(.object(["review": .null])))
        XCTAssertEqual(Advisory.blocked(link: .connected, commandPending: false, offer: offer), nil)
        XCTAssertEqual(Advisory.blocked(link: .connected, commandPending: false, offer: nil),
                       "This core reports no advisory review.")
        let running = AdvisoryOffer(review: AdvisoryReview(id: "r", provider: "Codex", state: "Running"), available: false,
                                    reason: "Another review is running; ask again when it ends.")
        XCTAssertEqual(Advisory.blocked(link: .connected, commandPending: false, offer: running), "The review is running.")
        let refused = AdvisoryOffer(available: false, reason: "a plan run reviews its own steps")
        XCTAssertEqual(Advisory.blocked(link: .connected, commandPending: false, offer: refused),
                       "a plan run reviews its own steps")
        XCTAssertEqual(Advisory.blocked(link: .lost, commandPending: false, offer: offer),
                       "Unavailable: the core is not connected.")
        XCTAssertEqual(Advisory.pill("Rejected").0, "Objects")
        XCTAssertEqual(Advisory.pill("Approved").1, .ok)
        // The attempt and document details carry it at 1.14.
        let attempt = try XCTUnwrap(AttemptDetail(["attempt": .text(FollowupRows.attempt),
                                                   "advisory": FollowupRows.advisory()]))
        XCTAssertEqual(attempt.advisory?.available, true)
        XCTAssertNil(AttemptDetail(["attempt": .text(FollowupRows.attempt)])?.advisory)
        var document = DetailRows.prd()
        document["advisory"] = FollowupRows.advisory(review: FollowupRows.review(state: "Rejected"))
        XCTAssertEqual(DocumentDetail(document)?.advisory?.review?.state, "Rejected")
        XCTAssertNil(DocumentDetail(DetailRows.prd())?.advisory)
    }

    func test_s19_the_screens_offer_the_second_opinion_and_say_why_not() throws {
        var detail = try XCTUnwrap(DocumentDetail(DetailRows.prd()))
        detail.advisory = AdvisoryOffer(available: true, reviewer: "Codex")
        let spec = SpecificationsState(connection: .ready(epoch: 7), admission: "Open", detail: detail)
        XCTAssertTrue(spec.reviewable)
        XCTAssertNil(spec.reviewBlocked)
        var pending = spec
        pending.commandPending = true
        XCTAssertEqual(pending.reviewBlocked, "Unavailable while a command awaits the core.")
        var brief = spec
        brief.detail = DocumentDetail(id: DetailRows.id(9), kind: "Brief")
        XCTAssertFalse(brief.reviewable)
        let verification = VerificationState(connection: .ready(epoch: 7), admission: "Open")
        XCTAssertEqual(verification.reviewBlocked, "No sealed candidate.")
    }

    func test_s19_snapshots_of_the_second_opinion_the_choices_and_a_failed_turn() throws {
        let section = AdvisorySection(offer: AdvisoryOffer(FollowupRows.advisory(review: FollowupRows.review())),
                                      subject: "revision 2", blocked: nil, request: {})
        try render(GlassInspector { section }, CGSize(width: 330, height: 320), "s19-advisory")
        guard case .object(var asked) = ChatRows.turn(2, provider: "Codex", questions: [
            ChatRows.question(1, prompt: "Where does the title come from?", blocking: true),
        ]) else { return XCTFail() }
        asked["choices"] = .list([.list([.text("Arguments"), .text("Standard input"), .text("Both")])])
        asked["session"] = .text("Continued")
        asked["repairs"] = .text(ChatRows.turnID(1))
        asked["repaired_by"] = .null
        asked["repairable"] = .bool(false)
        let page = try XCTUnwrap(FeatureChatPage(ChatRows.page([
            FollowupRows.failedTurn(repairable: false, repairedBy: ChatRows.turnID(2)), .object(asked),
        ])))
        let state = FeatureChatState(connection: .ready(epoch: 7), admission: "Open", queries: ["FeatureChat"],
                                     index: ChatIndex(ChatRows.index), indexLoad: .loaded, feature: ChatRows.feature,
                                     chat: page, chatLoad: .loaded, answers: [ChatRows.questionID(1): "Standard input"])
        try render(FeatureChatView(state: state), CGSize(width: 1180, height: 900), "s19-choices")
        let failed = try XCTUnwrap(FeatureChatPage(ChatRows.page([FollowupRows.failedTurn()])))
        let repair = FeatureChatState(connection: .ready(epoch: 7), admission: "Open", queries: ["FeatureChat"],
                                      index: ChatIndex(ChatRows.index), indexLoad: .loaded, feature: ChatRows.feature,
                                      chat: failed, chatLoad: .loaded)
        try render(FeatureChatView(state: repair), CGSize(width: 1180, height: 700), "s19-repair-turn")
    }

    private func render(_ view: some View, _ size: CGSize, _ file: String) throws {
        guard let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"] else { return }
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            let content = Themed { view }
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: size.width, height: size.height)
            let renderer = ImageRenderer(content: content)
            renderer.scale = 2
            let image = try XCTUnwrap(renderer.nsImage)
            let bitmap = try XCTUnwrap(image.tiffRepresentation.flatMap(NSBitmapImageRep.init(data:)))
            let png = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
            try png.write(to: URL(fileURLWithPath: directory).appendingPathComponent("\(file)-\(name).png"))
        }
    }
}

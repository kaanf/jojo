import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

/// Rows of the protocol 1.2 chat answers, written for these tests.
enum ChatRows {
    static let feature = String(repeating: "f0", count: 16)
    static let repository = String(repeating: "a1", count: 16)
    static let prd = String(repeating: "d0", count: 16)
    static func turnID(_ n: Int) -> String { String(repeating: String(format: "%02x", n), count: 16) }
    static func questionID(_ n: Int) -> String { String(repeating: String(format: "c%x", n), count: 16) }

    static func question(_ n: Int, prompt: String, blocking: Bool, answer: String? = nil, step: String? = nil) -> JSON {
        .object(["question": .text(questionID(n)), "revision": .int(Int64(n)), "prompt": .text(prompt),
                 "blocking": .bool(blocking), "answer": answer.map(JSON.text) ?? .null, "answer_complete": .bool(true),
                 "answered_by": answer == nil ? .null : .text("HUMAN"), "step": step.map(JSON.text) ?? .null])
    }

    static func turn(_ n: Int, state: String = "Replied", kind: String = "Discover", provider: String = "ClaudeCode",
                     message: String = "", summary: String = "", prd: Int64? = nil, plan: JSON = .null,
                     questions: [JSON] = []) -> JSON {
        .object(["turn": .text(turnID(n)), "ordinal": .int(Int64(n)), "kind": .text(kind), "provider": .text(provider),
                 "base": .text(String(repeating: "4", count: 40)), "message": .text(message),
                 "message_complete": .bool(true), "state": .text(state),
                 "detail": .text(state == "Failed" ? "the last message is not a reply: Json(Syntax)" : ""),
                 "summary": .text(summary),
                 "prd": prd.map { .object(["document": .text(Self.prd), "revision": .int($0)]) } ?? .null,
                 "plan": plan, "questions": .list(questions)])
    }

    static func page(_ turns: [JSON], next: Int64? = nil, running: String? = nil, prd: JSON = .null) -> [String: JSON] {
        ["feature": .text(feature), "turns": .list(turns), "next": next.map(JSON.int) ?? .null,
         "running": running.map { .object(["turn": .text($0), "stop_requested": .bool(false)]) } ?? .null, "prd": prd]
    }

    static func document(revision: Int64, ready: Bool, approved: Bool, blockers: Int64 = 0) -> JSON {
        .object(["document": .text(prd), "revision": .int(revision), "author": .text("MODEL"), "ready": .bool(ready),
                 "blockers": .int(blockers), "approved": .bool(approved), "length": .int(42)])
    }

    static func plan(drafted: Bool) -> JSON {
        .object(["adr": .text(String(repeating: "ad", count: 16)), "plan": .text(String(repeating: "9a", count: 16)),
                 "validation": .text(String(repeating: "7a", count: 16)), "drafted": .bool(drafted),
                 "steps": .list([("core", "Add the entry point"), ("docs", "Describe the usage"), ("check", "Check the entry point")]
                    .enumerated().map { offset, step in
                        .object(["step": .text(String(repeating: String(format: "5%x", offset), count: 16)),
                                 "key": .text(step.0), "title": .text(step.1)])
                    })])
    }

    static let index: [String: JSON] = [
        "repositories": .list([.object(["repository": .text(repository), "path": .text("/Users/someone/JojoDisposable/sample"),
                                        "path_complete": .bool(true)])]),
        "repositories_complete": .bool(true),
        "features": .list([.object(["feature": .text(feature), "repository": .text(repository),
                                    "objective": .text("slugify should be usable from the shell."),
                                    "objective_complete": .bool(true), "turns": .int(2), "running": .bool(false)])]),
        "features_complete": .bool(true),
    ]
}

@MainActor
final class M2FeatureChatTests: XCTestCase {
    private func ready() -> (Harness, WorkspaceModel) {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.refresh()
        h.fake.reply(h.fake.last("Snapshot")!, ["rows": .list([]), "as_of_event_seq": .int(30), "admission": .text("Open")])
        return (h, model)
    }

    func test_m2_chat_requests_are_sent_at_minor_2_and_every_other_request_stays_at_1() {
        let (h, model) = ready()
        // Since 1.6 the chat index is read at 1.6, where it names each feature.
        XCTAssertEqual(Request.allCases.filter { $0.minor == 2 },
                       [.ChatTurn, .AnswerQuestion, .CancelTurn, .FeatureChat, .ChatText])
        XCTAssertEqual(h.fake.last("Hello")?.minor, 1)
        XCTAssertEqual(h.fake.last("Snapshot")?.minor, 1)
        model.loadChatIndex()
        XCTAssertEqual(h.fake.last("ChatIndex")?.minor, 6)
        XCTAssertEqual(h.fake.last("ChatIndex")?.payload, [:])
        model.openChat(ChatRows.feature)
        XCTAssertEqual(h.fake.last("FeatureChat")?.minor, 2)
        model.loadPlans()
        XCTAssertEqual(h.fake.last("ApprovedPlan")?.minor, 1, "a 1.1 query keeps its 1.1 frame")
    }

    func test_m2_the_index_lists_repositories_and_chats_and_a_refusal_drops_it() {
        let (h, model) = ready()
        model.loadChatIndex()
        XCTAssertEqual(model.chatIndexLoad, .loading)
        model.loadChatIndex()
        XCTAssertEqual(h.fake.count("ChatIndex"), 1, "one read at a time")
        h.fake.reply(h.fake.last("ChatIndex")!, ChatRows.index)
        XCTAssertEqual(model.chatIndexLoad, .loaded)
        XCTAssertEqual(model.chatIndex?.repositories.map(\.id), [ChatRows.repository])
        XCTAssertEqual(model.chatIndex?.features.first?.objective, "slugify should be usable from the shell.")
        XCTAssertEqual(model.chatIndex?.features.first?.turns, 2)
        model.loadChatIndex()
        h.fake.error(h.fake.last("ChatIndex")!, "UnknownType")
        XCTAssertEqual(model.chatIndexLoad, .failed("UnknownType"))
        XCTAssertNil(model.chatIndex)
        var broken = ChatRows.index
        broken["features"] = .list([.object(["feature": .text("not an identity")])])
        XCTAssertNil(ChatIndex(broken), "one malformed row makes the index malformed")
    }

    func test_m2_a_chat_is_read_page_by_page_with_its_draft_text_and_dropped_when_malformed() {
        let (h, model) = ready()
        model.openChat(ChatRows.feature)
        XCTAssertEqual(h.fake.last("FeatureChat")?.payload, ["feature": .text(ChatRows.feature), "after": .int(0)])
        h.fake.reply(h.fake.last("FeatureChat")!, ChatRows.page([ChatRows.turn(1, message: "objective")], next: 1))
        XCTAssertEqual(h.fake.last("FeatureChat")?.payload, ["feature": .text(ChatRows.feature), "after": .int(1)],
                       "the next page starts after the last ordinal")
        XCTAssertEqual(model.chatLoad, .loading)
        let questions = [ChatRows.question(1, prompt: "Arguments or standard input?", blocking: true)]
        h.fake.reply(h.fake.last("FeatureChat")!,
                     ChatRows.page([ChatRows.turn(2, prd: 1, questions: questions)],
                                   prd: ChatRows.document(revision: 1, ready: false, approved: false, blockers: 1)))
        XCTAssertEqual(model.chatLoad, .loaded)
        XCTAssertEqual(model.chat?.turns.map(\.ordinal), [1, 2])
        XCTAssertEqual(model.chat?.questions.map(\.prompt), ["Arguments or standard input?"])
        XCTAssertEqual(model.chat?.prd?.revision, 1)
        // The draft's text is read in chunks from the core.
        let text = h.fake.last("ChatText")!
        XCTAssertEqual(text.minor, 2)
        XCTAssertEqual(text.payload, ["subject": .text(ChatRows.prd), "part": .text("Prd"), "offset": .int(0)])
        h.fake.reply(text, ["subject": .text(ChatRows.prd), "offset": .int(0), "next": .int(5), "total": .int(9),
                            "complete": .bool(false), "text": .text("# Slu")])
        XCTAssertEqual(h.fake.last("ChatText")?.payload["offset"], .int(5))
        h.fake.reply(h.fake.last("ChatText")!, ["subject": .text(ChatRows.prd), "offset": .int(5), "next": .int(9),
                                                 "total": .int(9), "complete": .bool(true), "text": .text("gify")])
        XCTAssertEqual(model.prdText, "# Slugify")
        XCTAssertEqual(model.prdTextRevision, 1)
        // The same revision is not read again; a new one is.
        let reads = h.fake.count("ChatText")
        model.loadChat()
        h.fake.reply(h.fake.last("FeatureChat")!, ChatRows.page([ChatRows.turn(1), ChatRows.turn(2, prd: 1)],
                                                                 prd: ChatRows.document(revision: 1, ready: true, approved: false)))
        XCTAssertEqual(h.fake.count("ChatText"), reads)
        // A malformed page drops what was held and says why.
        model.loadChat()
        h.fake.reply(h.fake.last("FeatureChat")!, ChatRows.page([.object(["turn": .text("x")])]))
        XCTAssertEqual(model.chatLoad, .failed("malformed chat"))
        XCTAssertNil(model.chat)
        // Another feature drops the chat and its text at once.
        model.openChat(nil)
        XCTAssertNil(model.chatFeature)
        XCTAssertEqual(model.prdText, "")
    }

    func test_m2_turns_answers_and_stops_are_commands_with_exactly_the_helper_fields() {
        let (h, model) = ready()
        model.sendTurn(feature: ChatRows.feature, kind: "Discover", repository: ChatRows.repository, provider: "Codex",
                       message: "slugify should be usable from the shell.")
        let turn = h.fake.last("ChatTurn")!
        XCTAssertEqual(turn.minor, 2)
        XCTAssertEqual(turn.payload, ["feature": .text(ChatRows.feature), "kind": .text("Discover"),
                                      "repository": .text(ChatRows.repository), "provider": .text("Codex"),
                                      "message": .text("slugify should be usable from the shell.")])
        XCTAssertEqual(model.turnOutcome, .sent)
        XCTAssertTrue(model.commandPending)
        h.fake.reply(turn, ["acknowledged": .bool(true), "response": .text("accepted"), "turn": .text(turn.id),
                            "dispatch": .text("launched")])
        XCTAssertEqual(model.turnOutcome, .accepted(attempt: turn.id, dispatch: "launched"))
        XCTAssertEqual(model.chatFeature, ChatRows.feature, "the new feature is held once the core accepted it")
        XCTAssertEqual(h.fake.last("FeatureChat")?.payload["feature"], .text(ChatRows.feature))

        let question = LeadQuestion(ChatRows.question(3, prompt: "Is packaging in scope?", blocking: false))!
        model.answer(question, .answer("No."))
        let answer = h.fake.last("AnswerQuestion")!
        XCTAssertEqual(answer.minor, 2)
        XCTAssertEqual(answer.payload, ["question": .text(ChatRows.questionID(3)), "revision": .int(3),
                                        "action": .text("Answer"), "answer": .text("No.")])
        h.fake.error(answer, "Rejected(RevisionConflict)")
        XCTAssertTrue(model.status.contains("refused"))
        model.answer(question, .deferQuestion)
        XCTAssertEqual(h.fake.last("AnswerQuestion")?.payload["action"], .text("Defer"))
        XCTAssertEqual(h.fake.last("AnswerQuestion")?.payload["answer"], .null)
        h.fake.reply(h.fake.last("AnswerQuestion")!, ["acknowledged": .bool(true), "response": .text("question revision=4")])
        model.answer(question, .block)
        XCTAssertEqual(h.fake.last("AnswerQuestion")?.payload["action"], .text("Block"))
        h.fake.reply(h.fake.last("AnswerQuestion")!, ["acknowledged": .bool(true), "response": .text("question revision=5")])

        model.cancelTurn(ChatRows.turnID(2))
        XCTAssertEqual(h.fake.last("CancelTurn")?.payload, ["turn": .text(ChatRows.turnID(2))])
        XCTAssertEqual(h.fake.last("CancelTurn")?.minor, 2)
        // An unknown outcome stays pending and is never sent again.
        h.fake.breakChannel("pipe closed")
        XCTAssertTrue(model.commandPending)
        let before = h.fake.count("ChatTurn")
        model.sendTurn(feature: ChatRows.feature, kind: "Discover", repository: ChatRows.repository, provider: "Codex", message: "")
        XCTAssertEqual(h.fake.count("ChatTurn"), before)
        // A refused turn says so; nothing was run.
        let (h2, model2) = ready()
        model2.sendTurn(feature: ChatRows.feature, kind: "Plan", repository: ChatRows.repository, provider: "ClaudeCode", message: "")
        h2.fake.error(h2.fake.last("ChatTurn")!, "TurnRefused: a plan turn needs the approved requirements document")
        XCTAssertEqual(model2.turnOutcome, .refused("TurnRefused: a plan turn needs the approved requirements document"))
        XCTAssertFalse(model2.commandPending)
    }

    private func state(_ turns: [JSON] = [], running: String? = nil, prd: JSON = .null, feature: String? = ChatRows.feature,
                       connection: HelperConnection = .ready(epoch: 7), admission: String = "Open") -> FeatureChatState {
        FeatureChatState(connection: connection, admission: admission, queries: ["FeatureChat"],
                         index: ChatIndex(ChatRows.index), indexLoad: .loaded, feature: feature,
                         chat: FeatureChatPage(ChatRows.page(turns, running: running, prd: prd)), chatLoad: .loaded)
    }

    func test_m2_the_screen_follows_the_four_discovery_states_and_says_why_a_turn_cannot_be_sent() {
        XCTAssertEqual(state().discovery, .empty)
        let running = state([ChatRows.turn(1, state: "Running", provider: "Codex")], running: ChatRows.turnID(1))
        XCTAssertEqual(running.discovery, .investigating(provider: "Codex", stopping: false))
        XCTAssertEqual(running.sendBlocked("Discover"), "A turn is running. Stop it or wait for its result.")
        let question = ChatRows.question(1, prompt: "Arguments or standard input?", blocking: true)
        let blocked = state([ChatRows.turn(1, questions: [question])])
        guard case .blocked(let q) = blocked.discovery else { return XCTFail("not blocked") }
        XCTAssertEqual(blocked.number(q), "Q1")
        // A question about one step never blocks the feature: it is listed on its own.
        let step = ChatRows.question(2, prompt: "Which shells?", blocking: true, step: "docs")
        let ready = ChatRows.document(revision: 2, ready: true, approved: false)
        let stepOnly = state([ChatRows.turn(1, prd: 2, questions: [step])], prd: ready)
        XCTAssertEqual(stepOnly.discovery, .complete(ChatDocument(ready)!))
        XCTAssertEqual(stepOnly.stepQuestions.map(\.step), ["docs"])
        let incomplete = state([ChatRows.turn(1, prd: 1)], prd: ChatRows.document(revision: 1, ready: false, approved: false, blockers: 2))
        XCTAssertEqual(incomplete.discovery, .continuing(ChatDocument(ChatRows.document(revision: 1, ready: false, approved: false, blockers: 2))))
        // A plan needs approved requirements; discovery does not.
        XCTAssertNil(stepOnly.sendBlocked("Discover"))
        XCTAssertEqual(stepOnly.sendBlocked("Plan"),
                       "A plan needs the approved requirements. Approve the draft in the Approval Inbox.")
        let approved = state([ChatRows.turn(1, prd: 2)], prd: ChatRows.document(revision: 2, ready: true, approved: true))
        XCTAssertNil(approved.sendBlocked("Plan"))
        XCTAssertEqual(approved.turnRepository, ChatRows.repository)
        // The core decides admission and connection; the screen only says so.
        XCTAssertEqual(state(admission: "Closed").sendBlocked("Discover"),
                       "Admission is closed. The core refuses a turn until it is open.")
        XCTAssertEqual(state(connection: .channelLost("gone")).sendBlocked("Discover"), "Unavailable: the core is not connected.")
        var old = state()
        old.queries = ["Snapshot", "ApprovedPlan"]
        XCTAssertEqual(old.sendBlocked("Discover"), "This core has no Feature Chat.")
        // Without a held feature the chat sends nothing; New Feature starts one (S4NewFeatureTests).
        XCTAssertEqual(state(feature: nil).sendBlocked("Discover"), "No feature is held. New Feature starts one.")
        // An answer needs words; a refusal is said in words, its code stays visible.
        let open = LeadQuestion(question)!
        var answering = blocked
        XCTAssertEqual(answering.answerBlocked(open), "Type an answer.")
        answering.answers[open.id] = "Arguments."
        XCTAssertNil(answering.answerBlocked(open))
        XCTAssertEqual(FeatureChatState.refusal("CapacityOne: a run is active"), "Another run is active. Nothing was run.")
        XCTAssertEqual(FeatureChatState.refusal("Unseen: detail"), "Nothing was run. The core's reason is below.")
        XCTAssertEqual(FeatureChatState.operational("Interrupted").1, "Interrupted")
        XCTAssertEqual(FeatureChatState.provider("ClaudeCode"), "Claude Code")
    }
}

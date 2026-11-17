import Foundation

// M2: Feature Chat as the protocol 1.2 queries return it. Every field is the helper's answer;
// a row that does not carry every field it should is malformed, and a page with a malformed
// row is not shown.

/// The registered repositories and the features that have a chat (`ChatIndex`).
public struct ChatIndex: Equatable, Sendable {
    public struct Repository: Equatable, Sendable, Identifiable {
        public let id: String
        public let path: String
        public let pathComplete: Bool
    }

    public struct Feature: Equatable, Sendable, Identifiable {
        public let id: String
        public let repository: String
        public let objective: String
        public let objectiveComplete: Bool
        public let turns: Int64
        public let running: Bool
        /// The title of its newest requirements document, else the first line of its first
        /// message (protocol 1.6); nil from an older helper.
        public var name: String? = nil

        /// The name, else the objective.
        public var shownName: String { name ?? objective }
    }

    public let repositories: [Repository]
    public let repositoriesComplete: Bool
    public let features: [Feature]
    public let featuresComplete: Bool

    public init?(_ p: [String: JSON]) {
        guard let repositoryItems = p["repositories"]?.items, let repositoriesComplete = p["repositories_complete"]?.boolean,
              let featureItems = p["features"]?.items, let featuresComplete = p["features_complete"]?.boolean else { return nil }
        let repositories = repositoryItems.compactMap { r -> Repository? in
            guard let id = r["repository"]?.string, Identity.valid(id), let path = r["path"]?.string,
                  let complete = r["path_complete"]?.boolean else { return nil }
            return Repository(id: id, path: path, pathComplete: complete)
        }
        let features = featureItems.compactMap { r -> Feature? in
            guard let id = r["feature"]?.string, Identity.valid(id), let repository = r["repository"]?.string,
                  let objective = r["objective"]?.string, let complete = r["objective_complete"]?.boolean,
                  let turns = r["turns"]?.integer, let running = r["running"]?.boolean else { return nil }
            return Feature(id: id, repository: repository, objective: objective, objectiveComplete: complete,
                           turns: turns, running: running, name: r["name"]?.string)
        }
        guard repositories.count == repositoryItems.count, features.count == featureItems.count else { return nil }
        (self.repositories, self.repositoriesComplete) = (repositories, repositoriesComplete)
        (self.features, self.featuresComplete) = (features, featuresComplete)
    }
}

/// One question the lead asked, with its newest answer.
public struct LeadQuestion: Equatable, Sendable, Identifiable {
    public let id: String
    public let revision: Int64
    public let prompt: String
    public let blocking: Bool
    public let answer: String?
    public let answerComplete: Bool
    /// `HUMAN` or `MODEL`; nil without an answer.
    public let answeredBy: String?
    /// The key of the one plan step the question is about; nil for the whole feature.
    public let step: String?

    /// A blocking question without an answer from a human holds what it is about.
    public var open: Bool { blocking && answeredBy != "HUMAN" }

    init?(_ r: JSON) {
        func text(_ key: String) -> String?? {
            guard let value = r[key] else { return nil }
            return value.isNull ? .some(nil) : value.string.map { .some($0) }
        }
        guard let id = r["question"]?.string, Identity.valid(id), let revision = r["revision"]?.integer,
              let prompt = r["prompt"]?.string, let blocking = r["blocking"]?.boolean, let answer = text("answer"),
              let complete = r["answer_complete"]?.boolean, let by = text("answered_by"), let step = text("step") else { return nil }
        (self.id, self.revision, self.prompt, self.blocking) = (id, revision, prompt, blocking)
        (self.answer, self.answerComplete, self.answeredBy, self.step) = (answer, complete, by, step)
    }
}

/// The plan a plan turn drafted: its three documents and its steps.
public struct LeadPlanDraft: Equatable, Sendable {
    public struct Step: Equatable, Sendable, Identifiable {
        public let id: String
        public let key: String
        public let title: String
    }

    public let adr: String
    public let plan: String
    public let validation: String
    /// The plan and validation plan exist: the ADR was approved.
    public let drafted: Bool
    public let steps: [Step]

    init?(_ r: JSON) {
        guard let adr = r["adr"]?.string, let plan = r["plan"]?.string, let validation = r["validation"]?.string,
              let drafted = r["drafted"]?.boolean, let items = r["steps"]?.items else { return nil }
        let steps = items.compactMap { s -> Step? in
            guard let id = s["step"]?.string, let key = s["key"]?.string, let title = s["title"]?.string else { return nil }
            return Step(id: id, key: key, title: title)
        }
        guard steps.count == items.count else { return nil }
        (self.adr, self.plan, self.validation, self.drafted, self.steps) = (adr, plan, validation, drafted, steps)
    }
}

/// One lead agent turn as the chat page reports it.
public struct LeadTurn: Equatable, Sendable, Identifiable {
    public let id: String
    public let ordinal: Int64
    /// `Discover` or `Plan`.
    public let kind: String
    /// `Codex` or `ClaudeCode`.
    public let provider: String
    public let base: String
    public let message: String
    public let messageComplete: Bool
    /// `Running`, `Stopping`, `Replied`, `Failed`, `Cancelled` or `Interrupted`.
    public let state: String
    public let detail: String
    public let summary: String
    /// The requirements document version this turn created.
    public let prd: (document: String, revision: Int64)?
    public let plan: LeadPlanDraft?
    public let questions: [LeadQuestion]
    /// Protocol 1.8: when the core accepted the turn and recorded its result; nil from an
    /// older helper, and the end while it runs.
    public let started: Date?
    public let ended: Date?

    public var running: Bool { state == "Running" || state == "Stopping" }

    init?(_ r: JSON) {
        guard let id = r["turn"]?.string, Identity.valid(id), let ordinal = r["ordinal"]?.integer,
              let kind = r["kind"]?.string, let provider = r["provider"]?.string, let base = r["base"]?.string,
              let message = r["message"]?.string, let complete = r["message_complete"]?.boolean,
              let state = r["state"]?.string, let detail = r["detail"]?.string, let summary = r["summary"]?.string,
              let prdValue = r["prd"], let planValue = r["plan"], let items = r["questions"]?.items else { return nil }
        var prd: (String, Int64)?
        if !prdValue.isNull {
            guard let document = prdValue["document"]?.string, let revision = prdValue["revision"]?.integer else { return nil }
            prd = (document, revision)
        }
        var plan: LeadPlanDraft?
        if !planValue.isNull {
            guard let draft = LeadPlanDraft(planValue) else { return nil }
            plan = draft
        }
        let questions = items.compactMap(LeadQuestion.init)
        guard questions.count == items.count else { return nil }
        (self.id, self.ordinal, self.kind, self.provider, self.base) = (id, ordinal, kind, provider, base)
        (self.message, self.messageComplete, self.state, self.detail, self.summary) = (message, complete, state, detail, summary)
        (self.prd, self.plan, self.questions) = (prd, plan, questions)
        (started, ended) = (helperDate(r["started_at_unix_ms"]?.integer), helperDate(r["ended_at_unix_ms"]?.integer))
    }

    public static func == (a: LeadTurn, b: LeadTurn) -> Bool {
        a.id == b.id && a.ordinal == b.ordinal && a.kind == b.kind && a.provider == b.provider && a.base == b.base
            && a.message == b.message && a.messageComplete == b.messageComplete && a.state == b.state
            && a.detail == b.detail && a.summary == b.summary && a.prd?.document == b.prd?.document
            && a.prd?.revision == b.prd?.revision && a.plan == b.plan && a.questions == b.questions
            && a.started == b.started && a.ended == b.ended
    }
}

/// The feature's requirements document as the core stores it.
public struct ChatDocument: Equatable, Sendable {
    public let document: String
    public let revision: Int64
    /// `HUMAN` or `MODEL`: who wrote the newest version.
    public let author: String
    /// Complete and without an unanswered blocking question: it can be offered for review.
    public let ready: Bool
    public let blockers: Int64
    public let approved: Bool
    /// Characters of the rendered text.
    public let length: Int64

    init?(_ r: JSON) {
        guard let document = r["document"]?.string, let revision = r["revision"]?.integer, let author = r["author"]?.string,
              let ready = r["ready"]?.boolean, let blockers = r["blockers"]?.integer, let approved = r["approved"]?.boolean,
              let length = r["length"]?.integer else { return nil }
        (self.document, self.revision, self.author, self.ready) = (document, revision, author, ready)
        (self.blockers, self.approved, self.length) = (blockers, approved, length)
    }
}

/// Protocol 1.8: an assumption of the feature's requirements document.
public struct ChatAssumption: Equatable, Sendable, Identifiable {
    public let id: String
    public let statement: String
    /// Current (in the newest revision) or Dropped (an earlier revision had it).
    public let state: String
    /// Where it first appeared, or the revision that dropped it.
    public let revision: Int64

    init?(_ r: JSON) {
        guard let id = r["id"]?.string, let statement = r["statement"]?.string, let state = r["state"]?.string,
              let revision = r["revision"]?.integer else { return nil }
        (self.id, self.statement, self.state, self.revision) = (id, statement, state, revision)
    }

    public init(id: String, statement: String, state: String = "Current", revision: Int64 = 1) {
        (self.id, self.statement, self.state, self.revision) = (id, statement, state, revision)
    }
}

/// One feature's chat: every turn read so far, the turn that runs and the draft document.
public struct FeatureChatPage: Equatable, Sendable {
    public let feature: String
    public var turns: [LeadTurn]
    /// The turn without a result, and whether a stop was asked for it.
    public let running: (turn: String, stopRequested: Bool)?
    public let prd: ChatDocument?
    /// The ordinal after which the next page starts; nil when this was the last page.
    public let next: Int64?
    /// Protocol 1.8: the requirements document's assumptions; nil from an older helper.
    public let assumptions: [ChatAssumption]?

    public init?(_ p: [String: JSON]) {
        guard let feature = p["feature"]?.string, Identity.valid(feature), let items = p["turns"]?.items,
              let nextValue = p["next"], let runningValue = p["running"], let prdValue = p["prd"] else { return nil }
        let turns = items.compactMap(LeadTurn.init)
        guard turns.count == items.count else { return nil }
        var running: (String, Bool)?
        if !runningValue.isNull {
            guard let turn = runningValue["turn"]?.string, let stop = runningValue["stop_requested"]?.boolean else { return nil }
            running = (turn, stop)
        }
        var prd: ChatDocument?
        if !prdValue.isNull {
            guard let document = ChatDocument(prdValue) else { return nil }
            prd = document
        }
        var next: Int64?
        if !nextValue.isNull {
            guard let ordinal = nextValue.integer else { return nil }
            next = ordinal
        }
        (self.feature, self.turns, self.running, self.prd, self.next) = (feature, turns, running, prd, next)
        if let items = p["assumptions"]?.items {
            let assumptions = items.compactMap(ChatAssumption.init)
            guard assumptions.count == items.count else { return nil }
            self.assumptions = assumptions
        } else {
            assumptions = nil
        }
    }

    public static func == (a: FeatureChatPage, b: FeatureChatPage) -> Bool {
        a.feature == b.feature && a.turns == b.turns && a.running?.turn == b.running?.turn
            && a.running?.stopRequested == b.running?.stopRequested && a.prd == b.prd && a.next == b.next
            && a.assumptions == b.assumptions
    }

    /// Every question of the chat, oldest first.
    public var questions: [LeadQuestion] { turns.flatMap(\.questions) }
    /// The newest plan draft.
    public var plan: LeadPlanDraft? { turns.last { $0.plan != nil }?.plan }
}

/// What the human does with one question.
public enum QuestionAnswer: Equatable, Sendable {
    case answer(String)
    case deferQuestion
    case block
}

extension WorkspaceModel {
    static let maxChatPages = 16
    static let maxChatText: Int64 = 256 * 1024

    /// Reads the registered repositories and the features that have a chat.
    public func loadChatIndex() {
        guard chatIndexLoad != .loading, snapshotSeq != nil else { return }
        chatIndexLoad = .loading
        onChange?()
        client.query(.ChatIndex, [:]) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                if let index = ChatIndex(p) {
                    (self.chatIndex, self.chatIndexLoad) = (index, .loaded)
                } else {
                    (self.chatIndex, self.chatIndexLoad) = (nil, .failed("malformed index"))
                }
            case .error(let reason), .unanswered(let reason):
                (self.chatIndex, self.chatIndexLoad) = (nil, .failed(reason))
            default:
                (self.chatIndex, self.chatIndexLoad) = (nil, .failed("unexpected answer"))
            }
            self.onChange?()
        }
    }

    /// Holds `feature` and reads its chat; nil holds none (New Feature).
    public func openChat(_ feature: String?) {
        guard feature != chatFeature else { return loadChat() }
        chatRead += 1
        (chatFeature, chat, chatLoad, prdText, prdTextRevision) = (feature, nil, .idle, "", nil)
        onChange?()
        loadChat()
    }

    /// Reads the held feature's chat again, page by page. What was held is replaced when the
    /// read ends and dropped when it fails.
    public func loadChat() {
        guard let feature = chatFeature, snapshotSeq != nil else { return }
        chatRead += 1
        chatLoad = .loading
        onChange?()
        readChat(feature, read: chatRead, after: 0, collected: [], pages: 0)
    }

    private func readChat(_ feature: String, read: Int, after: Int64, collected: [LeadTurn], pages: Int) {
        client.query(.FeatureChat, ["feature": .text(feature), "after": .int(after)]) { [weak self] reply in
            guard let self, read == self.chatRead else { return }
            switch reply {
            case .response(let p):
                guard var page = FeatureChatPage(p), page.feature == feature else {
                    (self.chat, self.chatLoad) = (nil, .failed("malformed chat"))
                    break
                }
                let all = collected + page.turns
                if let next = page.next, next > after, pages + 1 < Self.maxChatPages {
                    return self.readChat(feature, read: read, after: next, collected: all, pages: pages + 1)
                }
                page.turns = all
                (self.chat, self.chatLoad) = (page, .loaded)
                if let prd = page.prd, prd.revision != self.prdTextRevision {
                    self.readPrd(prd.document, revision: prd.revision, read: read, offset: 0, collected: "")
                }
            case .error(let reason), .unanswered(let reason):
                (self.chat, self.chatLoad) = (nil, .failed(reason))
            default:
                (self.chat, self.chatLoad) = (nil, .failed("unexpected answer"))
            }
            self.onChange?()
        }
    }

    /// The rendered requirements document, chunk by chunk.
    private func readPrd(_ document: String, revision: Int64, read: Int, offset: Int64, collected: String) {
        client.query(.ChatText, ["subject": .text(document), "part": .text("Prd"), "offset": .int(offset)]) { [weak self] reply in
            guard let self, read == self.chatRead else { return }
            guard case .response(let p) = reply, let text = p["text"]?.string, let next = p["next"]?.integer,
                  let total = p["total"]?.integer else {
                (self.prdText, self.prdTextRevision) = ("", nil)
                self.onChange?()
                return
            }
            let all = collected + text
            if next < total, next > offset, next < Self.maxChatText {
                return self.readPrd(document, revision: revision, read: read, offset: next, collected: all)
            }
            (self.prdText, self.prdTextRevision) = (all + (next < total ? "\n… (truncated)" : ""), revision)
            self.onChange?()
        }
    }

    /// One lead agent turn. A new feature gets its identity here, with its first turn.
    public func sendTurn(feature: String, kind: String, repository: String, provider: String, message: String) {
        guard Identity.valid(feature), Identity.valid(repository) else {
            status = "A turn needs a feature and a registered repository"
            onChange?()
            return
        }
        if !commandPending { turnOutcome = .sent }
        slice(.ChatTurn, ["feature": .text(feature), "kind": .text(kind), "repository": .text(repository),
                          "provider": .text(provider), "message": .text(message)],
              sent: "\(kind) turn sent; awaiting acknowledgement") { [weak self] p in
            let turn = p["turn"]?.string ?? "?", dispatch = p["dispatch"]?.string ?? "?"
            self?.turnOutcome = .accepted(attempt: turn, dispatch: dispatch)
            if self?.chatFeature != feature { self?.chatFeature = feature }
            self?.loadChat()
            self?.loadChatIndex()
            return "\(kind) turn \(turn) accepted (\(dispatch)); it reads the repository and changes nothing"
        } refused: { [weak self] code in
            self?.turnOutcome = .refused(code)
            return "Turn refused (\(code)); nothing was run"
        }
    }

    /// The human's answer to, or disposition of, one question at the revision shown.
    public func answer(_ question: LeadQuestion, _ action: QuestionAnswer) {
        let (word, body): (String, JSON) = switch action {
        case .answer(let text): ("Answer", .text(text))
        case .deferQuestion: ("Defer", .null)
        case .block: ("Block", .null)
        }
        slice(.AnswerQuestion, ["question": .text(question.id), "revision": .int(question.revision),
                                "action": .text(word), "answer": body],
              sent: "\(word) sent; awaiting acknowledgement") { [weak self] _ in
            self?.loadChat()
            return "\(word) committed for the question; it binds the next turn"
        } refused: { "\(word) refused (\($0)); the question is unchanged. Reload before answering again." }
    }

    /// The human's stop of the running turn. The turn ends with its result.
    public func cancelTurn(_ turn: String) {
        guard Identity.valid(turn) else { return }
        slice(.CancelTurn, ["turn": .text(turn)], sent: "Stop sent; awaiting acknowledgement") { [weak self] _ in
            self?.loadChat()
            return "Stop committed for turn \(turn); it ends with its result"
        } refused: { "Stop refused (\($0))" }
    }
}

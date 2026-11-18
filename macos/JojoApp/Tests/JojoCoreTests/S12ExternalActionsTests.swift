import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

/// M5: one finished plan run of the held feature, its push and its draft pull request.
enum ExternalRows {
    static let plan = DetailRows.id(40)
    static let head = String(repeating: "a1", count: 20)
    static let branch = "jojo/plan/\(plan)"
    static let url = "https://github.com/kaanf/jojo.git"
    static let digest = String(repeating: "d", count: 64)
    static let push = "external:push:\(plan):\(head):1"
    static let pr = "external:pr:\(plan):\(head):1"

    static func action(_ kind: String, state: String, observations: [JSON] = [], number: JSON = .null,
                       link: JSON = .null, body: JSON = .null) -> JSON {
        .object(["action": .text(kind == "Push" ? push : pr), "kind": .text(kind), "plan": .text(plan), "revision": .int(1),
                 "sequence": .int(1), "feature": .text(DetailRows.feature), "feature_name": .text("Slugify from the shell"),
                 "repository": .text(DetailRows.id(9)), "remote": .text("origin"), "url": .text(url),
                 "branch": .text(branch), "commit": .text(head), "base": kind == "Push" ? .null : .text("main"),
                 "title": kind == "Push" ? .null : .text("Slugify from the shell"), "body": body,
                 "push": kind == "Push" ? .null : .text(push), "digest": .text(digest), "state": .text(state),
                 "decision": state == "Requested" ? .null : .text("Authorized"),
                 "requested_at_unix_ms": .int(DetailRows.t0 + 300_000), "decided_at_unix_ms": .null,
                 "outcome": observations.isEmpty ? .null : .text("Committed"),
                 "detail": .text(observations.last?["detail"]?.string ?? ""), "number": number,
                 "pull_request_url": link, "observations": .list(observations)])
    }

    static func observation(_ observer: String, _ status: String, _ detail: String) -> JSON {
        .object(["observer": .text(observer), "status": .text(status), "detail": .text(detail),
                 "at_unix_ms": .int(DetailRows.t0 + 400_000)])
    }

    static func run(canPush: Bool, canOpen: Bool, push: JSON = .null, pr: JSON = .null) -> JSON {
        .object(["plan": .text(plan), "revision": .int(1), "feature": .text(DetailRows.feature),
                 "feature_name": .text("Slugify from the shell"), "repository": .text(DetailRows.id(9)),
                 "branch": .text(branch), "base": .text(String(repeating: "b2", count: 20)), "head": .text(head),
                 "state": .text("DONE"), "push": push, "pull_request": pr,
                 "can_push": .bool(canPush), "push_refusal": canPush ? .null : .text("the plan branch is pushed"),
                 "can_open_pull_request": .bool(canOpen),
                 "pull_request_refusal": canOpen ? .null : .text("the plan branch is not pushed yet")])
    }

    static func answer(runs: [JSON], actions: [JSON], gh: String = "Ready") -> [String: JSON] {
        ["runs": .list(runs), "actions": .list(actions),
         "auth": .object(["git": .object(["state": .text("Ready"), "detail": .text("credential helper: osxkeychain")]),
                          "gh": .object(["state": .text(gh), "detail": .text(gh == "Ready" ? "Logged in to github.com account kaanf (keyring)"
                                                                                : "You are not logged into any GitHub hosts. To log in, run: gh auth login")])])]
    }

    static let pushed = observation("git:push", "Committed", "pushed (new branch)")
    static let body = "Lets the shell call slugify.\n\n## Steps\n\n| Step | Title | Provider | Host validation | Commit |\n|---|---|---|---|---|\n| `cli` | Add the command line | Codex | passed, receipt 91 | `a1a1a1a1a1a1` |\n"

    /// Nothing requested yet: the run may be pushed.
    static let fresh = ExternalList(answer(runs: [run(canPush: true, canOpen: false)], actions: []))!
    static let pushRequested = ExternalList(answer(runs: [run(canPush: false, canOpen: false, push: .text(push))],
                                                   actions: [action("Push", state: "Requested")]))!
    static let prRequested = ExternalList(answer(
        runs: [run(canPush: false, canOpen: false, push: .text(push), pr: .text(pr))],
        actions: [action("PullRequest", state: "Requested", body: .text(body)),
                  action("Push", state: "Committed", observations: [pushed])], gh: "Missing"))!
    static let opened = ExternalList(answer(
        runs: [run(canPush: false, canOpen: false, push: .text(push), pr: .text(pr))],
        actions: [action("PullRequest", state: "Committed",
                         observations: [observation("gh:pr-create", "Committed", "draft pull request #3 opened")],
                         number: .int(3), link: .text("https://github.com/kaanf/jojo/pull/3")),
                  action("Push", state: "Committed", observations: [pushed])]))!
    static let unknown = ExternalList(answer(
        runs: [run(canPush: false, canOpen: false, push: .text(push))],
        actions: [action("Push", state: "Unknown",
                         observations: [observation("git:push", "Unknown", "git push did not finish (timed out); the remote may or may not have the branch"),
                                        observation("git:ls-remote", "Unknown", "the remote has no branch \(branch)")])]))!
}

@MainActor
final class S12ExternalActionsTests: XCTestCase {
    private func ready() -> (Harness, WorkspaceModel) {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.refresh()
        h.fake.reply(h.fake.last("Snapshot")!, ["rows": .list([]), "as_of_event_seq": .int(30), "admission": .text("Open")])
        return (h, model)
    }

    private func acknowledge(_ h: Harness, _ type: String, _ response: String) {
        h.fake.reply(h.fake.last(type)!, ["acknowledged": .bool(true), "response": .text(response)])
    }

    func test_s12_external_requests_go_at_1_10_with_exactly_the_helpers_fields() throws {
        XCTAssertEqual(Request.allCases.filter { $0.minor == 10 },
                       [.ExternalActions, .RequestExternal, .AuthorizeExternal, .InspectExternal, .ResolveExternal])
        let (h, model) = ready()
        // Step 7: the documents answer lists the briefs from 1.12; since 1.14 it counts what waits.
        XCTAssertEqual(h.fake.last("Documents")?.minor, 14)
        model.loadExternalActions()
        XCTAssertEqual(h.fake.last("ExternalActions")?.minor, 10)
        XCTAssertEqual(h.fake.last("ExternalActions")?.payload, ["plan": .null])
        h.fake.reply(h.fake.last("ExternalActions")!, ExternalRows.answer(runs: [ExternalRows.run(canPush: true, canOpen: false)],
                                                                          actions: []))
        XCTAssertEqual(model.external?.runs.first?.canPush, true)
        model.requestExternal(plan: ExternalRows.plan, kind: .Push)
        let request = try XCTUnwrap(h.fake.last("RequestExternal"))
        XCTAssertEqual(request.minor, 10)
        XCTAssertEqual(request.payload, ["plan": .text(ExternalRows.plan), "kind": .text("Push")])
        acknowledge(h, "RequestExternal", ExternalRows.push)
        XCTAssertTrue(model.status.contains("waits for its own authorization"), model.status)
        let push = try XCTUnwrap(ExternalRows.pushRequested[ExternalRows.push])
        model.decideExternal(push, authorize: true)
        XCTAssertEqual(h.fake.last("AuthorizeExternal")?.payload,
                       ["action": .text(ExternalRows.push), "digest": .text(ExternalRows.digest), "decision": .text("Authorize")])
        // An unanswered command stays outstanding: nothing is sent again.
        model.decideExternal(push, authorize: true)
        XCTAssertEqual(h.fake.count("AuthorizeExternal"), 1)
        h.fake.reply(h.fake.last("AuthorizeExternal")!, ["acknowledged": .bool(true), "response": .text("Push authorized; issued once"),
                                                        "dispatch": .text("started")])
        XCTAssertEqual(model.status, "Push authorized; issued once (started)")
        model.inspectExternal(ExternalRows.push)
        XCTAssertEqual(h.fake.last("InspectExternal")?.payload, ["action": .text(ExternalRows.push)])
        acknowledge(h, "InspectExternal", "inspection started")
        model.resolveExternal(ExternalRows.push)
        XCTAssertEqual(h.fake.last("ResolveExternal")?.payload, ["action": .text(ExternalRows.push)])
        h.fake.error(h.fake.last("ResolveExternal")!, "InspectFirst: inspect the remote before marking the action not performed")
        XCTAssertTrue(model.status.hasPrefix("Marking not performed refused (InspectFirst"), model.status)
    }

    func test_s12_the_answer_decodes_runs_actions_sign_in_and_the_waiting_requests() throws {
        let list = ExternalRows.unknown
        XCTAssertEqual(list.runs.first?.pushRefusal, "the plan branch is pushed")
        XCTAssertEqual(list.runs.first?.feature, DetailRows.feature)
        XCTAssertEqual(list.unresolved.map(\.id), [ExternalRows.push])
        XCTAssertEqual(list.newest(.Push, plan: ExternalRows.plan)?.observations.count, 2)
        XCTAssertEqual(ExternalRows.prRequested.gh.state, "Missing")
        XCTAssertEqual(ExternalRows.opened.newest(.PullRequest, plan: ExternalRows.plan)?.number, 3)
        XCTAssertNil(ExternalList(["runs": .list([.object(["plan": .text("x")])]), "actions": .list([])]), "a malformed run drops the answer")
        let waiting: JSON = .object(["action": .text(ExternalRows.push), "kind": .text("Push"), "plan": .text(ExternalRows.plan),
                                     "feature": .text(DetailRows.feature), "feature_name": .text("Slugify from the shell"),
                                     "branch": .text(ExternalRows.branch), "commit": .text(ExternalRows.head), "base": .null,
                                     "title": .null, "requested_at_unix_ms": .int(DetailRows.t0)])
        let documents = try XCTUnwrap(DocumentList(["documents": .list([]), "candidates": .list([]), "external": .list([waiting]),
                                                    "next": .null, "pending": .int(1)]))
        XCTAssertEqual(documents.external.map(\.id), [ExternalRows.push])
        XCTAssertEqual(documents.pending, 1)
        XCTAssertEqual(DocumentList(documents: [], external: documents.external).pending, 1)
        XCTAssertEqual(DocumentList(["documents": .list([]), "candidates": .list([]), "next": .null, "pending": .int(0)])?.external,
                       [], "a 1.9 answer has no external requests")
    }

    private func inbox(_ external: ExternalList?, selected: String? = nil, type: InboxType = .external,
                       pending: Bool = false) -> ApprovalInboxState {
        let waiting = (external?.actions ?? []).filter { $0.state == .Requested }.map {
            PendingExternal(id: $0.id, kind: $0.kind, plan: $0.plan, featureName: $0.featureName, branch: $0.branch,
                            commit: $0.commit, base: $0.base, title: $0.title, requested: $0.requested)
        }
        return ApprovalInboxState(connection: .ready(epoch: 7), admission: "Open",
                           list: DocumentList(documents: [], external: waiting),
                           selected: selected, features: FeatureList(features: [DetailRows.summary]), external: external,
                           externalLoad: .loaded, commandPending: pending, type: type, now: DetailRows.now)
    }

    func test_s12_the_inbox_lists_requests_and_authorizes_only_a_requested_one_after_a_confirmation() throws {
        let state = inbox(ExternalRows.pushRequested)
        XCTAssertEqual(state.items.map(\.id), [ExternalRows.push])
        XCTAssertEqual(state.chosen?.kind, "Push")
        XCTAssertEqual(state.status.0, "Awaiting authorization")
        XCTAssertEqual(state.place(state.items[0]), "Sample · FT-3")
        XCTAssertEqual(inbox(ExternalRows.pushRequested, pending: true).status.0, "Requested")
        XCTAssertTrue(inbox(ExternalRows.opened).items.isEmpty, "Pending lists only what waits")
        let push = try XCTUnwrap(ExternalRows.pushRequested[ExternalRows.push])
        XCTAssertNil(ExternalWords.blocked(push, list: ExternalRows.pushRequested, connected: true, pending: false))
        XCTAssertEqual(ExternalWords.blocked(push, list: ExternalRows.pushRequested, connected: false, pending: false),
                       "Unavailable: the core is not connected.")
        let confirmation = ExternalWords.confirmation(push)
        XCTAssertEqual(confirmation.confirm, "Authorize Push")
        XCTAssertTrue(confirmation.message.contains(ExternalRows.head) && confirmation.message.contains(ExternalRows.url)
                      && confirmation.message.contains("refs/heads/\(ExternalRows.branch)"), confirmation.message)
        XCTAssertEqual(ExternalWords.authorize(.PullRequest), "Create Draft PR…")
        let pr = try XCTUnwrap(ExternalRows.prRequested[ExternalRows.pr])
        XCTAssertEqual(ExternalWords.blocked(pr, list: ExternalRows.prRequested, connected: true, pending: false),
                       "gh is not ready: You are not logged into any GitHub hosts. To log in, run: gh auth login")
        XCTAssertEqual(pr.body, ExternalRows.body)
        let committed = try XCTUnwrap(ExternalRows.opened[ExternalRows.push])
        XCTAssertEqual(ExternalWords.blocked(committed, list: ExternalRows.opened, connected: true, pending: false),
                       "This request was decided.")
    }

    private func sourceControl(_ external: ExternalList?, pending: Bool = false) -> SourceControlState {
        SourceControlState(connection: .ready(epoch: 7), scope: DetailRows.scope, candidates: CandidateRows.candidates,
                           result: CandidateRows.result(decision: "ACCEPTED"), files: CandidateRows.files, diff: CandidateRows.diff,
                           base: "9c1e4b2aa0", verdict: .pass, detail: CandidateRows.detail, external: external, externalLoad: .loaded,
                           commandPending: pending)
    }

    func test_s12_source_control_states_follow_the_core_and_say_why_a_request_waits() {
        let status = { (s: SourceControlState, key: String) in s.status.first { $0.key == key }?.pill?.0 }
        XCTAssertEqual(status(sourceControl(ExternalRows.fresh), "Push"), "Not performed")
        XCTAssertEqual(status(sourceControl(ExternalRows.fresh), "Pull request"), "None")
        XCTAssertNil(sourceControl(ExternalRows.fresh).requestBlocked(.Push))
        XCTAssertEqual(sourceControl(ExternalRows.fresh).requestBlocked(.PullRequest), "the plan branch is not pushed yet")
        XCTAssertEqual(sourceControl(ExternalRows.fresh, pending: true).requestBlocked(.Push),
                       "Unavailable while a command awaits the core.")
        XCTAssertEqual(status(sourceControl(ExternalRows.pushRequested), "Push"), "Requested")
        XCTAssertEqual(status(sourceControl(ExternalRows.unknown), "Push"), "Outcome unknown")
        XCTAssertEqual(status(sourceControl(ExternalRows.opened), "Push"), "Pushed")
        XCTAssertEqual(status(sourceControl(ExternalRows.opened), "Pull request"), "PR #3 draft")
        XCTAssertEqual(sourceControl(ExternalRows.opened).status.first { $0.key == "Location" }?.value, "Plan branch on origin")
        XCTAssertEqual(sourceControl(nil).requestBlocked(.Push), "This core does not report push and pull requests.")
        var other = sourceControl(ExternalRows.fresh)
        other.scope = FeatureScope(feature: DetailRows.id(77))
        XCTAssertNil(other.run, "another feature's run is not this feature's")
        XCTAssertEqual(ExternalWords.pill(nil).0, "Not performed")
    }

    /// Review aid: with JOJO_SNAPSHOT_DIR set, writes the M5 places in their states.
    func test_s12_external_actions_render_in_both_appearances() throws {
        let inboxHeading = ("Approval Inbox", "1 pending · no approve-all")
        let cases: [(String, ApprovalInboxState, CGSize)] = [
            ("s12-inbox-push", inbox(ExternalRows.pushRequested), CGSize(width: 1440, height: 1160)),
            ("s12-inbox-pull-request", inbox(ExternalRows.prRequested), CGSize(width: 1440, height: 1500)),
            ("s12-inbox-unknown", inbox(ExternalRows.unknown, type: .external).with(filter: .all), CGSize(width: 1440, height: 1160)),
            ("s12-inbox-push-narrow", inbox(ExternalRows.pushRequested), CGSize(width: 900, height: 1700)),
        ]
        for (file, state, size) in cases {
            try DetailRows.render(ApprovalInboxView(state: state), screen: .approvalInbox, heading: inboxHeading, size: size, file: file)
        }
        let heading = ("Source Control", DetailRows.scope.place("api"))
        let sources: [(String, SourceControlState, CGSize)] = [
            ("s12-source-control-not-performed", sourceControl(ExternalRows.fresh), CGSize(width: 1440, height: 1360)),
            ("s12-source-control-requested", sourceControl(ExternalRows.pushRequested), CGSize(width: 1440, height: 1360)),
            ("s12-source-control-unknown", sourceControl(ExternalRows.unknown), CGSize(width: 1440, height: 1360)),
            ("s12-source-control-opened", sourceControl(ExternalRows.opened), CGSize(width: 1440, height: 1360)),
        ]
        for (file, state, size) in sources {
            try DetailRows.render(SourceControlView(state: state), screen: .sourceControl, heading: heading, size: size, file: file)
        }
        let recovery = RecoveryCenterState(connection: .ready(epoch: 7), admission: "Open", rows: [], digest: "digest-1",
                                           commandPending: false, external: ExternalRows.unknown.unresolved)
        try DetailRows.render(RecoveryCenterView(state: recovery), screen: .recoveryCenter, heading: ("Recovery Center", ""),
                              size: CGSize(width: 1440, height: 1000), file: "s12-recovery-external")
    }
}

private extension ApprovalInboxState {
    func with(filter: InboxFilter) -> Self {
        var copy = self
        copy.filter = filter
        return copy
    }
}

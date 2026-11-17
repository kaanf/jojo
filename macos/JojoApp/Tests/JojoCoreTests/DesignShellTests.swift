import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

/// Design wave 1: tokens, status model and shell against docs/design.
@MainActor
final class DesignShellTests: XCTestCase {
    /// WCAG relative luminance of an opaque color.
    private func luminance(_ c: RGBA) -> Double {
        func linear(_ v: Double) -> Double { v <= 0.03928 ? v / 12.92 : pow((v + 0.055) / 1.055, 2.4) }
        return 0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b)
    }

    private func over(_ top: RGBA, _ bottom: RGBA) -> RGBA {
        var c = bottom
        c.r = top.r * top.a + bottom.r * (1 - top.a)
        c.g = top.g * top.a + bottom.g * (1 - top.a)
        c.b = top.b * top.a + bottom.b * (1 - top.a)
        c.a = 1
        return c
    }

    private func contrast(_ a: RGBA, _ b: RGBA) -> Double {
        let (x, y) = (luminance(a), luminance(b))
        return (max(x, y) + 0.05) / (min(x, y) + 0.05)
    }

    func test_lg_tokens_match_the_design_system_table() {
        XCTAssertEqual(Tokens.light.window, RGBA(246, 246, 248, 0.74))
        XCTAssertEqual(Tokens.dark.glass, RGBA(58, 58, 64, 0.46))
        XCTAssertEqual(Tokens.light.glassStrong, .white(0.78))
        XCTAssertEqual(Tokens.dark.card, .white(0.065))
        XCTAssertEqual(Tokens.light.sep, .black(0.09))
        XCTAssertEqual(Tokens.light.accent, RGBA(0x0071E3))
        XCTAssertEqual(Tokens.dark.accent, RGBA(0x0A84FF))
        XCTAssertEqual(Tokens.light.orange, RGBA(0x9A4300))
        XCTAssertEqual(Tokens.dark.purpleBg, RGBA(191, 90, 242, 0.20))
        XCTAssertEqual(Tokens.light.grayBg, RGBA(120, 120, 128, 0.14))
        XCTAssertEqual(Tokens.dark.onAccent, Tokens.light.onAccent)
        XCTAssertEqual(Theme.resolve(.dark), .dark)
        XCTAssertEqual(Theme.resolve(.light), .light)
    }

    /// Reduce Transparency and off-screen snapshots draw glass as the token at alpha 1 over
    /// the opaque window; a card stays lighter than the window in light.
    func test_lg_materials_fall_back_to_opaque_tokens() {
        for tokens in [Tokens.dark, Tokens.light] {
            XCTAssertEqual(tokens.opaqueWindow.a, 1)
            XCTAssertEqual(tokens.opaque(\.glass).a, 1)
            XCTAssertEqual(tokens.opaque(\.glass), tokens.glass.over(tokens.opaqueWindow))
        }
        XCTAssertGreaterThan(luminance(Tokens.light.opaque(\.card)), luminance(Tokens.light.opaqueWindow))
        XCTAssertGreaterThan(luminance(Tokens.dark.opaque(\.card)), luminance(Tokens.dark.opaqueWindow))
    }

    func test_lg_status_text_on_its_tint_keeps_4_5_to_1_on_a_card() {
        for tokens in [Tokens.dark, Tokens.light] {
            let card = tokens.opaque(\.card)
            for tone in Tone.allCases {
                let background = over(tokens.tint(tone), card)
                XCTAssertGreaterThanOrEqual(contrast(tokens.foreground(tone), background), 4.5, "\(tone)")
            }
            // The board's dark accent (#0a84ff) holds white at 3.6:1: a control, not body text.
            XCTAssertGreaterThanOrEqual(contrast(tokens.onAccent, tokens.accent), 3)
        }
    }

    func test_lg_status_darkens_in_light() {
        XCTAssertEqual(Tone.allCases.count, 6)
        for tone in Tone.allCases {
            XCTAssertLessThan(luminance(Tokens.light.foreground(tone)), luminance(Tokens.dark.foreground(tone)), "\(tone)")
        }
    }

    func test_dw1_operational_states_follow_the_status_table() {
        let tones: [OperationalState: Tone] = [
            .idle: .neutral, .running: .info, .waiting: .neutral, .blocked: .warn, .awaitingApproval: .warn,
            .paused: .neutral, .recoveryRequired: .danger, .failed: .danger, .completed: .ok, .requested: .review,
            .repairing: .info,
        ]
        XCTAssertEqual(OperationalState.allCases.count, 11)
        for state in OperationalState.allCases {
            XCTAssertEqual(state.tone, tones[state], state.label)
        }
        XCTAssertEqual(Set(OperationalState.allCases.map(\.label)).count, 11)
        XCTAssertEqual(OperationalState.running.glyph, .haloDot)
        XCTAssertEqual(OperationalState.waiting.glyph, .ring)
        XCTAssertEqual(OperationalState.recoveryRequired.glyph, .ringDot)
        // States that share a color differ by glyph, so color is never the only signal.
        XCTAssertNotEqual(OperationalState.blocked.glyph, OperationalState.awaitingApproval.glyph)
        XCTAssertNotEqual(OperationalState.recoveryRequired.glyph, OperationalState.failed.glyph)
    }

    func test_dw1_unknown_evidence_is_never_success_or_failure() {
        XCTAssertEqual(EvidenceState.unknown.tone, .neutral)
        XCTAssertEqual(EvidenceState.unknown.symbol, "?")
        XCTAssertEqual(EvidenceState.allCases.map(\.symbol), ["✓", "◆", "✕", "◐", "!", "○", "↷", "⊘", "?"])
        XCTAssertEqual(EvidenceState.passed.tone, .ok)
        XCTAssertEqual(EvidenceState.sealed.tone, .ok)
        XCTAssertEqual(EvidenceState.failed.tone, .danger)
        XCTAssertEqual(EvidenceState.incomplete.tone, .warn)
        XCTAssertEqual(EvidenceState.needsApproval.tone, .review)
        for state in [EvidenceState.notRun, .skipped, .invalidated] {
            XCTAssertEqual(state.tone, .neutral, state.label)
        }
    }

    func test_dw1_document_tags_follow_the_status_table() {
        XCTAssertEqual(DocumentStatus.proposed.tone, .info)
        XCTAssertEqual(DocumentStatus.approved.tone, .ok)
        XCTAssertEqual(DocumentStatus.superseded.tone, .neutral)
        XCTAssertEqual(DocumentStatus.invalidated.tone, .danger)
        XCTAssertEqual(DocumentStatus.stale.tone, .warn)
    }

    func test_dw1_sidebar_lists_the_canvas_destinations_in_order() {
        XCTAssertEqual(Screen.screens(in: .workspace).map(\.title),
                       ["Dashboard", "Projects", "Approval Inbox", "Recovery Center"])
        XCTAssertEqual(Screen.screens(in: .feature).map(\.title),
                       ["Feature Chat", "Specifications", "Execution Plan", "Agent Runs", "Source Control", "Verification & Evidence"])
        XCTAssertEqual(Screen.screens(in: .system).map(\.title), ["Activity & Audit", "Settings & Providers"])
        XCTAssertEqual(Set(Screen.allCases.map(\.symbol)).count, Screen.allCases.count)
        XCTAssertEqual(Screen.settings.place, "System")
        XCTAssertEqual(Screen.recoveryCenter.place, "Workspace")
        XCTAssertEqual(Screen.agentRuns.place, "No feature selected")
        XCTAssertTrue(MainView.built.contains(.recoveryCenter))
        XCTAssertEqual(Set(Screen.allCases).subtracting(MainView.built), [], "every destination has a screen since the Dashboard (step 3)")
    }

    func test_dw1_admission_is_stated_only_while_the_core_is_connected() {
        let open = ShellStatus(connection: .ready(epoch: 7), admission: "Open", pid: 42)
        XCTAssertEqual(open.tone, .ok)
        XCTAssertEqual(open.admission, "Admission open")
        XCTAssertEqual(open.session, "epoch 7")
        XCTAssertEqual(open.core, "Core connected · pid 42")

        let closed = ShellStatus(connection: .ready(epoch: 7), admission: "Closed", pid: 42)
        XCTAssertEqual(closed.tone, .danger)
        XCTAssertEqual(closed.admission, "Admission closed")

        let unreported = ShellStatus(connection: .ready(epoch: 7), admission: "", pid: 42)
        XCTAssertNil(unreported.tone)
        XCTAssertEqual(unreported.admission, "Admission unknown")

        // The last admission the helper reported is not shown as current once it is gone.
        for connection in [HelperConnection.channelLost("pipe closed"), .ended(status: 1), .launchFailed("no helper"),
                           .handshaking, .notStarted, .incompatible(epoch: 7)] {
            let status = ShellStatus(connection: connection, admission: "Open", pid: 42)
            XCTAssertNil(status.tone, "\(connection)")
            XCTAssertEqual(status.admission, "Admission unknown")
        }
        XCTAssertTrue(ShellStatus(connection: .channelLost("pipe closed"), admission: "Open", pid: 42).core
            .contains("state unknown, not assumed stopped (pipe closed)"))
    }

    /// Review aid: with JOJO_SNAPSHOT_DIR set, writes the shell and the component gallery as
    /// PNG files in both appearances. Without it, only checks that the views can be built.
    func test_dw1_shell_and_components_render_in_both_appearances() throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            let shell = Themed {
                AppShell(model: model, screen: .constant(.dashboard), showInspector: {}) {
                    Gallery().padding(EdgeInsets(top: 20, leading: 20, bottom: 28, trailing: 20))
                }
            }
            .environment(\.offscreen, true)
            .environment(\.colorScheme, scheme)
            .frame(width: 1120, height: 3930)
            guard let directory else { continue }
            try write(shell, to: directory, "shell-\(name).png")
        }
    }

    private func write(_ view: some View, to directory: String, _ file: String) throws {
        let renderer = ImageRenderer(content: view)
        renderer.scale = 2
        let image = try XCTUnwrap(renderer.nsImage)
        let bitmap = try XCTUnwrap(image.tiffRepresentation.flatMap(NSBitmapImageRep.init(data:)))
        let png = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
        try png.write(to: URL(fileURLWithPath: directory).appendingPathComponent(file))
    }

    // MARK: Recovery Center (docs/design/screens/RECOVERY_CENTER.md)

    /// A recovery row as the helper sends it; `choices` nil is a helper older than 1.1.
    private func held(_ attempt: String, absent: Bool = false, blockers: [String] = [], choices: [RecoveryChoice]? = [.Hold],
                      complete: Bool = true) -> RecoveryRow {
        var row: [String: JSON] = [:]
        if let choices {
            row = ["blockers": .list(blockers.map(JSON.text)), "blockers_complete": .bool(complete),
                   "process_absent": .bool(absent), "choices": .list(choices.map { .text($0.rawValue) })]
        }
        return RecoveryRow(attempt: attempt, assessment: "Assessment { blockers: \(blockers) }",
                           findings: "stage=Spawned worktree=/data/worktrees/\(attempt.prefix(8)) stdout=4096",
                           detailComplete: complete, structured: RecoveryAssessment(row: .object(row)))
    }

    private var fixture: RecoveryCenterState {
        RecoveryCenterState(connection: .ready(epoch: 7), admission: "Open", rows: [
            held(String(repeating: "1a2b3c4d", count: 4), absent: true, choices: RecoveryChoice.allCases),
            held(String(repeating: "5e6f7a8b", count: 4), blockers: ["process state unknown", "worktree changed after snapshot"]),
            held(String(repeating: "9c0d1e2f", count: 4), choices: nil, complete: false),
        ], digest: "0f1e2d3c4b5a69788796a5b4c3d2e1f0", commandPending: false)
    }

    func test_dw1_recovery_actions_follow_what_the_core_offers() {
        let items = fixture.items
        let (dead, unknown, old) = (items[0], items[1], items[2])

        XCTAssertEqual(dead.title, "Attempt 1a2b3c4d")
        XCTAssertEqual(dead.pillLabel, "Confirmed dead")
        XCTAssertEqual(dead.primary, .ValidateOutput)
        XCTAssertTrue(RecoveryChoice.allCases.allSatisfy(dead.offers))
        XCTAssertEqual(dead.sentence(.Hold), RecoveryChoice.Hold.sentence)
        XCTAssertEqual(dead.blockerLine, "No blockers")

        // Not proven gone is unknown, never dead; only Hold is offered and the reason is stated.
        XCTAssertEqual(unknown.pillLabel, "Unknown")
        XCTAssertNil(unknown.primary)
        XCTAssertEqual(RecoveryChoice.allCases.filter(unknown.offers), [.Hold])
        XCTAssertTrue(unknown.sentence(.ValidateOutput).hasPrefix("Unavailable while process state is unknown."))
        XCTAssertTrue(unknown.footnote.hasPrefix("Only Hold is available."))
        XCTAssertEqual(unknown.blockerLine, "2 blockers")
        XCTAssertEqual(unknown.findingSentence, "process state unknown")

        // Proven gone but not offered: the reason is the inspection, not the process.
        let blocked = HeldItem(row: held("a", absent: true, blockers: ["snapshot mismatch"]))
        XCTAssertEqual(blocked.pillLabel, "Confirmed dead")
        XCTAssertEqual(blocked.sentence(.ContinueFreshContext), "Unavailable: the core's inspection does not offer this choice.")

        // An older helper reports no choices: nothing is claimed, the helper still decides.
        XCTAssertNil(old.row.structured)
        XCTAssertEqual(old.pillLabel, "Unknown")
        XCTAssertTrue(RecoveryChoice.allCases.allSatisfy(old.offers))
        XCTAssertEqual(old.blockerLine, "Blockers not reported")
        XCTAssertTrue(old.footnote.contains("does not report which choices"))

        XCTAssertEqual(RecoveryChoice.allCases.filter(\.confirms), [.ValidateOutput, .ContinueWithHistory, .ContinueFreshContext])
        XCTAssertEqual(Set(RecoveryChoice.allCases.map(\.buttonTitle)).count, 4)
    }

    func test_dw1_recovery_states_admission_and_actions_only_from_a_connected_core() {
        var state = fixture
        XCTAssertEqual(state.admissionState, .open)
        XCTAssertTrue(state.canAct)
        XCTAssertFalse(state.loading)
        XCTAssertEqual(state.digestShown, "0f1e2d3c4b5a")

        state.admission = "Closed"
        XCTAssertEqual(state.admissionState, .closed)
        state.admission = ""
        XCTAssertEqual(state.admissionState, .unknown)

        state.commandPending = true
        XCTAssertFalse(state.canAct, "the control that sent a command cannot send again")
        state.commandPending = false

        // The last reported admission is not current once the helper is gone; nothing can be sent.
        state.admission = "Open"
        for connection in [HelperConnection.channelLost("pipe closed"), .ended(status: 1), .launchFailed("no helper")] {
            state.connection = connection
            XCTAssertEqual(state.link, .lost)
            XCTAssertEqual(state.admissionState, .unknown)
            XCTAssertFalse(state.canAct)
            XCTAssertFalse(state.loading, "rows stay on screen as the last known state")
        }
        state.connection = .incompatible(epoch: 7)
        XCTAssertEqual(state.link, .incompatible)
        XCTAssertFalse(state.canAct)

        let waiting = RecoveryCenterState(connection: .ready(epoch: 7), admission: "Closed", rows: [], digest: "", commandPending: false)
        XCTAssertTrue(waiting.loading)
        XCTAssertFalse(waiting.canAct)
        XCTAssertEqual(waiting.digestShown, "None")
        let lost = RecoveryCenterState(connection: .ended(status: 1), admission: "", rows: [], digest: "", commandPending: false)
        XCTAssertFalse(lost.loading, "a helper that ended will not deliver an inspection")
    }

    func test_dw1_start_new_session_needs_every_finding_reviewed() {
        var state = fixture
        let all = Set(state.rows.map(\.attempt))
        XCTAssertEqual(state.startBlocked(reviewed: all), "Admission is already open.")

        state.admission = "Closed"
        XCTAssertEqual(state.startBlocked(reviewed: []), "Unavailable until every finding is marked reviewed.")
        XCTAssertEqual(state.startBlocked(reviewed: Set(all.prefix(2))), "Unavailable until every finding is marked reviewed.")
        XCTAssertNil(state.startBlocked(reviewed: all))

        state.commandPending = true
        XCTAssertEqual(state.startBlocked(reviewed: all), "Unavailable while a command awaits the core.")
        state.commandPending = false
        state.digest = ""
        XCTAssertEqual(state.startBlocked(reviewed: all), "Unavailable until an inspection is loaded.")
        state.digest = "d"
        state.connection = .channelLost("pipe closed")
        XCTAssertEqual(state.startBlocked(reviewed: all), "Unavailable: the core is not connected.")

        // An inspection that found nothing has nothing to review.
        let empty = RecoveryCenterState(connection: .ready(epoch: 7), admission: "Closed", rows: [], digest: "d", commandPending: false)
        XCTAssertNil(empty.startBlocked(reviewed: []))
    }

    func test_dw1_banner_and_card_shapes_follow_the_component_table() {
        XCTAssertEqual(Banner<EmptyView>.Kind.warning.tone, .warn)
        XCTAssertEqual(Banner<EmptyView>.Kind.danger.tone, .danger)
        XCTAssertNotEqual(Banner<EmptyView>.Kind.warning.symbol, Banner<EmptyView>.Kind.danger.symbol)
        XCTAssertEqual(CardShape.page, CardShape(radius: 12, vertical: 14, horizontal: 16))
        XCTAssertEqual(CardShape.list, CardShape(radius: 12, vertical: 0, horizontal: 0))
        XCTAssertEqual(CardShape.panel.radius, 10)
        XCTAssertEqual(CardShape.panelList.radius, 10)
    }

    /// Review aid, as above: the Recovery Center, its launch review, and the empty, loading
    /// and disconnected forms. The rows are test data; the app shows only what the helper reports.
    func test_dw1_recovery_center_renders_in_both_appearances() throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        var closed = fixture
        closed.admission = "Closed"
        var lost = fixture
        lost.connection = .channelLost("pipe closed")
        let nothing = RecoveryCenterState(connection: .ready(epoch: 7), admission: "Open", rows: [], digest: "d", commandPending: false)
        let loading = RecoveryCenterState(connection: .ready(epoch: 7), admission: "", rows: [], digest: "", commandPending: false)
        let cases: [(String, RecoveryCenterState, Bool)] = [
            ("recovery-center", fixture, false), ("recovery-closed", closed, false), ("launch-review", closed, true),
            ("recovery-disconnected", lost, false), ("recovery-empty", nothing, false), ("recovery-loading", loading, false),
        ]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            for (file, state, reviewing) in cases {
                let shell = Themed {
                    AppShell(model: model, screen: .constant(.recoveryCenter), showInspector: {}) {
                        RecoveryCenterView(state: state, reviewing: reviewing)
                    }
                }
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: 1120, height: 720)
                guard let directory else { continue }
                try write(shell, to: directory, "\(file)-\(name).png")
            }
        }
    }

    // MARK: Agent Runs (docs/design/screens/AGENT_RUNS.md)

    private static let ids = ["1a2b3c4d", "5e6f7a8b", "9c0d1e2f", "a1b2c3d4", "e5f6a7b8"].map { String(repeating: $0, count: 4) }

    /// The `Candidate` answer of one attempt, as the helper sends it.
    private func result(_ attempt: String, sealed: Bool = true, outcome: String = "Succeeded", reason: String = "exit 0",
                        validation: String = "PASS", decision: String = "NONE", records: Int64 = 3) -> CandidateReview {
        CandidateReview(["attempt": .text(attempt), "sealed": .bool(sealed), "outcome": .text(sealed ? outcome : ""),
                         "reason": .text(sealed ? reason : ""), "provider_terminal": .text(sealed ? "result: success" : ""),
                         "candidate": sealed ? .text(String(repeating: "c4", count: 20)) : .null, "log_records": .int(records),
                         "validation": .object(["state": .text(validation), "detail": .text(""), "receipt": .int(4),
                                                "candidate": .text(String(repeating: "c4", count: 20)), "reasons": .list([])]),
                         "decision": .text(decision)])!
    }

    // MARK: Verification & Evidence (docs/design/screens/VERIFICATION.md)

    private static let sampleDiff = """
    diff --git a/Sources/Invite.swift b/Sources/Invite.swift
    index 3f1c2aa..9d04e1b 100644
    --- a/Sources/Invite.swift
    +++ b/Sources/Invite.swift
    @@ -12,5 +12,6 @@ struct Invite {
         let token: String
    -    var uses = 1
    +    var uses: Int
    +    var revoked = false
         let lobby: String
     }
    @@ -40 +41 @@
    -+++ old marker
    +--- new marker
    \\ No newline at end of file
    """


    // MARK: Source Control (docs/design/screens/SOURCE_CONTROL.md)

    private static let controlDiff = """
    diff --git a/Sources/Invite.swift b/Sources/Invite.swift
    index 3f1c2aa..9d04e1b 100644
    --- a/Sources/Invite.swift
    +++ b/Sources/Invite.swift
    @@ -12,4 +12,5 @@ struct Invite {
         let token: String
    -    var uses = 1
    +    var uses: Int
    +    var revoked = false
         let lobby: String
     }
    diff --git a/Tests/InviteTests.swift b/Tests/InviteTests.swift
    new file mode 100644
    index 0000000..7d1a2c3
    --- /dev/null
    +++ b/Tests/InviteTests.swift
    @@ -0,0 +1,3 @@
    +import XCTest
    +
    +final class InviteTests: XCTestCase {}
    diff --git a/Sources/LegacyInvite.swift b/Sources/LegacyInvite.swift
    deleted file mode 100644
    index 5b2e9f1..0000000
    --- a/Sources/LegacyInvite.swift
    +++ /dev/null
    @@ -1,2 +0,0 @@
    -struct LegacyInvite {}
    -
    diff --git a/Package.resolved b/Package.resolved
    index 1111111..2222222 100644
    --- a/Package.resolved
    +++ b/Package.resolved
    @@ -3 +3 @@
    -  "version" : 2
    +  "version" : 3
    """

    /// The loaded attempt as Source Control reads it, with the kinds the helper sends.

    // MARK: Approval Inbox (docs/design/screens/APPROVAL_INBOX.md)

    /// A `ReviewBundle` answer as the helper sends it; the upstream approvals show the kind.
    private func bundle(_ subject: String, revision: Int64 = 5, upstream: [Int64] = [12], prior: Int64? = nil) -> ReviewBundle {
        ReviewBundle(["subject": .text(subject), "revision": .int(revision), "hash": .text(String(repeating: "7c1f", count: 16)),
                      "content_hash": .text(String(repeating: "c0de", count: 16)),
                      "displayed_hash": .text(String(repeating: "d15b", count: 16)),
                      "context_hash": .text(String(repeating: "c7e0", count: 16)),
                      "upstream": .list(upstream.map(JSON.int)), "prior_decision": prior.map(JSON.int) ?? .null])!
    }

    /// The snapshot's document and attempt rows, as Activity & Audit's "Go to" reads them.
    private func snapshotRows() -> [SnapshotRow] {
        let ids = Self.ids
        return [SnapshotRow(key: "artifact:\(ids[4])", value: String(repeating: "ab12", count: 16), detailComplete: true),
                SnapshotRow(key: "scheduler_attempt:\(ids[0])", value: "RESERVED", detailComplete: true),
                SnapshotRow(key: "review_document:\(ids[1])", value: String(repeating: "cd34", count: 16), detailComplete: true),
                SnapshotRow(key: "review_document:\(ids[2])", value: String(repeating: "ef56", count: 16), detailComplete: true)]
    }

    // MARK: Specifications (docs/design/screens/SPECIFICATIONS.md)

    // MARK: Activity & Audit (docs/design/screens/ACTIVITY.md)

    /// One row of the `Events` answer, as the helper sends it.
    private func event(_ seq: Int64, epoch: Int64 = 7, type: String = "WorktreeReady", kind: String? = nil, id: String? = nil,
                       revision: Int64? = nil, payload: String = "state=READY", complete: Bool = true) -> JSON {
        .object(["seq": .int(seq), "epoch": .int(epoch), "type": .text(type), "aggregate_kind": kind.map(JSON.text) ?? .null,
                 "aggregate_id": id.map(JSON.text) ?? .null, "revision": revision.map(JSON.int) ?? .null,
                 "payload": .text(payload), "detail_complete": .bool(complete)])
    }

    private func answerEvents(_ h: Harness, _ rows: [JSON]) {
        let request = h.fake.last("Events")!
        var e = Envelope(session: request.session, epoch: h.fake.epoch, id: request.id, type: "Response")
        e.payloadList = rows
        h.fake.deliver(e)
    }

    /// A workspace whose snapshot is at event `head`.
    private func workspace(_ h: Harness, head: Int64) -> WorkspaceModel {
        let model = WorkspaceModel(client: h.client)
        model.refresh()
        h.fake.reply(h.fake.last("Snapshot")!, ["rows": .list([]), "as_of_event_seq": .int(head), "admission": .text("Open")])
        return model
    }

    func test_dw1_activity_reads_the_newest_events_in_pages_and_afterwards_only_newer_ones() {
        let h = Harness()
        let model = workspace(h, head: 81)
        XCTAssertEqual(model.eventsLoad, .idle)
        XCTAssertEqual(h.fake.count("Events"), 0, "the journal is read only when the screen asks")
        model.loadEvents()
        XCTAssertEqual(model.eventsLoad, .loading)
        XCTAssertEqual(h.fake.last("Events")?.payload, ["after": .int(17), "limit": .int(8)], "the existing query, one helper page")
        model.loadEvents()
        XCTAssertEqual(h.fake.count("Events"), 1, "one read at a time")
        answerEvents(h, (18...25).map { event($0) })
        XCTAssertEqual(h.fake.last("Events")?.payload["after"], .int(25))
        answerEvents(h, (26...28).map { event($0, type: "ArtifactApproved", kind: "artifact", id: Self.ids[4], revision: 5) })
        XCTAssertEqual(model.eventsLoad, .loaded)
        XCTAssertEqual(model.events.map(\.seq), Array(18...28))
        XCTAssertEqual(model.events.last?.aggregateID, Self.ids[4])
        XCTAssertEqual(model.events.last?.revision, 5)
        XCTAssertNil(model.events.first?.aggregateKind)

        model.loadEvents()
        XCTAssertEqual(h.fake.last("Events")?.payload["after"], .int(28), "afterwards only what was committed since")
        answerEvents(h, [event(29)])
        XCTAssertEqual(model.events.map(\.seq), Array(18...29))

        model.pollEvents()
        XCTAssertEqual(h.fake.last("Events")?.payload, ["after": .int(81), "limit": .int(1)], "the poll is unchanged")
        answerEvents(h, [])
        XCTAssertEqual(model.events.count, 12, "the poll does not fill the list")
    }

    func test_dw1_activity_loads_older_events_without_gaps_and_keeps_them_when_a_read_fails() {
        let h = Harness()
        let model = workspace(h, head: 12)
        model.loadOlderEvents()
        XCTAssertEqual(h.fake.count("Events"), 0, "nothing is held yet")
        model.loadEvents()
        XCTAssertEqual(h.fake.last("Events")?.payload["after"], .int(0))
        answerEvents(h, (1...8).map { event($0) })
        answerEvents(h, (9...12).map { event($0) })
        model.loadOlderEvents()
        XCTAssertEqual(h.fake.count("Events"), 2, "the journal starts at the oldest held event")

        let late = Harness()
        let other = workspace(late, head: 100)
        other.loadEvents()
        answerEvents(late, [event(99), event(100)])
        XCTAssertEqual(late.fake.last("Events")?.payload["after"], .int(36))
        other.loadOlderEvents()
        XCTAssertEqual(late.fake.last("Events")?.payload["after"], .int(34), "the 64 events before the oldest held one")
        late.fake.error(late.fake.last("Events")!, "Unavailable")
        XCTAssertEqual(other.eventsLoad, .failed("Unavailable"))
        XCTAssertEqual(other.events.map(\.seq), [99, 100], "a failed read keeps what was read")
        other.loadOlderEvents()
        var malformed = event(35)
        if case .object(var fields) = malformed { fields["type"] = nil; malformed = .object(fields) }
        answerEvents(late, [malformed])
        XCTAssertEqual(other.eventsLoad, .failed("malformed event"))
        other.loadOlderEvents()
        answerEvents(late, (35...42).map { event($0) })
        XCTAssertEqual(late.fake.last("Events")?.payload["after"], .int(42))
        answerEvents(late, (43...50).map { event($0) })
        XCTAssertEqual(other.eventsLoad, .loading)
        late.fake.breakChannel("pipe closed")
        XCTAssertNotEqual(other.eventsLoad, .loading, "a lost channel ends the read")
        XCTAssertEqual(other.events.map(\.seq), [99, 100], "a read that did not finish adds nothing, so the list has no gap")
    }

    private func journal() -> [JournalEvent] {
        let ids = Self.ids
        return [
            event(60, epoch: 6, type: "CandidateAccepted", kind: "scheduler_attempt", id: ids[0], revision: 9, payload: "decision=ACCEPT;receipt=4"),
            event(61, epoch: 6, type: "EngineStopped", payload: "shutdown=CLEAN"),
            event(62, type: "EngineStarted", payload: "previous_epoch=6;previous_shutdown=CLEAN;quarantined_claims=0"),
            event(63, type: "RecoveryStarted", payload: String(repeating: "95eb9cc2", count: 8)),
            event(64, type: "ArtifactVersionCreated", kind: "artifact", id: ids[4], revision: 5, payload: "content=" + String(repeating: "c0de", count: 16)),
            event(65, type: "ReviewDocumentCreated", kind: "review_document", id: ids[1], revision: 5,
                  payload: "hash=" + String(repeating: "7c1f", count: 16) + ";upstream=12", complete: false),
            event(66, type: "ArtifactApproved", kind: "artifact", id: ids[4], revision: 5, payload: "decision=APPROVE"),
            event(67, type: "WorktreeReady", kind: "worktree_admin", id: ids[0], revision: 3),
            event(68, type: "ArtifactApproved", kind: "review_document", id: ids[1], revision: 6, payload: "decision=APPROVE"),
        ].compactMap(JournalEvent.init)
    }

    private func activity(query: String = "", hidden: Set<String> = [], selected: Int64? = nil) -> ActivityState {
        ActivityState(connection: .ready(epoch: 7), admission: "Open", rows: snapshotRows(), head: 68, events: journal(), load: .loaded,
                      query: query, hidden: hidden, selected: selected)
    }

    func test_dw1_activity_shows_the_loaded_events_newest_first_grouped_by_epoch() {
        let state = activity()
        XCTAssertEqual(state.events.count, 9)
        XCTAssertEqual(state.shown.map(\.seq), Array((60...68).reversed()))
        XCTAssertEqual(state.groups.map(\.title), ["Epoch 7", "Epoch 6"], "the core reports no time to group by")
        XCTAssertEqual(state.groups.map(\.events.count), [7, 2])
        XCTAssertEqual(state.types.first, ActivityState.TypeCount(type: "ArtifactApproved", count: 2))
        XCTAssertEqual(state.types.map(\.type), state.types.map(\.type).sorted())
        XCTAssertEqual(state.chosen?.seq, 68, "without a choice, the newest shown event")
        XCTAssertEqual(activity(selected: 63).chosen?.seq, 63)
        XCTAssertEqual(state.range, "Events 60 to 68 are loaded, newest first.")
        XCTAssertEqual(ActivityState.record(state.events[0]), "event 60")
        XCTAssertEqual(ActivityState.subject(state.events[0]), "1a2b3c4d1a2b")
        XCTAssertNil(ActivityState.subject(state.events[1]), "an event without an aggregate has no subject")
        XCTAssertTrue(state.isDocument(state.events[4]) && state.isDocument(state.events[5]))
        XCTAssertFalse(state.isDocument(state.events[0]) || state.isDocument(state.events[1]),
                       "only a document the snapshot lists can be opened")
        XCTAssertTrue(MainView.built.contains(.activity))
    }

    func test_dw1_activity_search_and_type_filters_cover_only_the_loaded_events() {
        XCTAssertFalse(activity().filtered)
        let approved = activity(hidden: ["ArtifactApproved", "EngineStopped"])
        XCTAssertTrue(approved.filtered)
        XCTAssertEqual(approved.shown.map(\.seq), [67, 65, 64, 63, 62, 60])
        XCTAssertEqual(approved.range, "Events 60 to 68 are loaded, newest first; 6 of 9 shown.")
        XCTAssertEqual(activity(hidden: ["ArtifactApproved"], selected: 68).chosen?.seq, 67, "a hidden event is not the chosen one")
        XCTAssertFalse(activity(hidden: ["NoSuchType"]).filtered, "a type that is not loaded filters nothing")

        XCTAssertEqual(activity(query: " approve ").shown.map(\.seq), [68, 66], "the payload is searched, without case")
        XCTAssertEqual(activity(query: "worktree").shown.map(\.seq), [67], "and the type and the aggregate kind")
        XCTAssertEqual(activity(query: Self.ids[1]).shown.map(\.seq), [68, 65], "and the subject")
        XCTAssertEqual(activity(query: "event 63").shown.map(\.seq), [63], "and the record")
        let none = activity(query: "09:42")
        XCTAssertTrue(none.shown.isEmpty && none.groups.isEmpty, "a time is not reported and cannot be found")
        XCTAssertNil(none.chosen)

        var state = activity()
        XCTAssertTrue(state.canLoadOlder)
        state.load = .loading
        XCTAssertFalse(state.canLoadOlder)
        state.load = .loaded
        state.connection = .channelLost("pipe closed")
        XCTAssertFalse(state.canLoadOlder)
        XCTAssertFalse(state.admissionClosed, "admission is stated only while the core is connected")
        state.connection = .ready(epoch: 7)
        state.admission = "Closed"
        XCTAssertTrue(state.admissionClosed)
        state.events = Array(journal().prefix(1)).compactMap { JournalEvent(event(1, payload: $0.payload)) }
        XCTAssertFalse(state.canLoadOlder, "the journal starts at event 1")
        XCTAssertEqual(state.range, "Event 1 is loaded.")
    }

    /// Review aid, as above: Activity & Audit with test events in two columns and in one,
    /// and its filtered, unmatched, loading, empty, failed, closed and disconnected forms.
    func test_dw1_activity_renders_in_both_appearances() throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        var loading = activity()
        (loading.events, loading.load) = ([], .loading)
        var none = loading
        none.load = .loaded
        var failed = loading
        failed.load = .failed("Unavailable")
        var stale = activity()
        stale.load = .failed("Unavailable")
        var lost = activity()
        (lost.connection, lost.admission) = (.channelLost("pipe closed"), "Closed")
        var closed = activity(selected: 61)
        closed.admission = "Closed"
        let opening = CGSize(width: 1120, height: 720), tall = CGSize(width: 1120, height: 1100), wide = CGSize(width: 1440, height: 1000)
        let cases: [(String, ActivityState, CGSize)] = [
            ("activity", activity(), opening), ("activity-tall", activity(selected: 65), tall),
            ("activity-wide", activity(selected: 64), wide),
            ("activity-narrow", activity(selected: 65), CGSize(width: 900, height: 1700)),
            ("activity-filtered", activity(query: "approve", hidden: ["WorktreeReady", "EngineStopped"]), wide),
            ("activity-unmatched", activity(query: "09:42"), wide),
            ("activity-loading", loading, opening), ("activity-empty", none, opening),
            ("activity-failed", failed, opening), ("activity-failed-loaded", stale, wide),
            ("activity-closed", closed, wide), ("activity-disconnected", lost, wide),
        ]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            for (file, state, size) in cases {
                let shell = Themed {
                    AppShell(model: model, screen: .constant(.activity), showInspector: {}) {
                        ActivityView(state: state)
                    }
                }
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: size.width, height: size.height)
                guard let directory else { continue }
                try write(shell, to: directory, "\(file)-\(name).png")
            }
        }
    }

    // MARK: Settings & Providers (docs/design/screens/SETTINGS.md)

    func test_dw1_helper_client_keeps_the_rest_of_the_hello_answer() {
        let fake = FakeHelper()
        fake.autoHello = false
        let client = HelperClient(transport: fake)
        client.start()
        XCTAssertNil(client.hello, "nothing is known before the helper answers")
        fake.reply(fake.sent[0], ["compatible": .bool(true), "epoch": .int(7), "max_frame": .int(1_048_576), "max_page": .int(8),
                                  "admission": .text("closed until reviewed recovery start"),
                                  "commands": .text("PrepareClose Start"), "queries": .text("Snapshot Events Log"), "slice": .bool(true)])
        XCTAssertEqual(client.connection, .ready(epoch: 7))
        XCTAssertEqual(client.hello, HelperHello(maxFrame: 1_048_576, maxPage: 8, slice: true,
                                                 commands: ["PrepareClose", "Start"], queries: ["Snapshot", "Events", "Log"]))

        // An answer without the fields leaves them unknown, never false or zero.
        let bare = FakeHelper()
        let older = HelperClient(transport: bare)
        older.start()
        XCTAssertEqual(older.hello, HelperHello())
    }

    private static let claudeDigest = String(repeating: "9f3c", count: 16)

    private func settings(_ section: SettingsSection = .claude, connection: HelperConnection = .ready(epoch: 7),
                          launch: SettingsLaunch? = nil) -> SettingsState {
        let options = LaunchOptions.parse([
            "JojoApp", "--helper", "/Applications/Jojo.app/Contents/MacOS/jojo-helper", "--data-root", "/Users/dev/JojoDisposable/data",
            "--disposable-root", "/Users/dev/JojoDisposable/run", "--claude-executable", "/Users/dev/.local/share/claude/versions/2.1.311",
            "--claude-version", "2.1.311 (Claude Code)", "--claude-sha256", Self.claudeDigest, "--claude-home", "/Users/dev/.claude-home",
            "--claude-user", "dev-account", "--claude-auth", "native-subscription",
        ])
        return SettingsState(
            connection: connection, admission: "Open", pid: 4242,
            hello: HelperHello(maxFrame: 1_048_576, maxPage: 8, slice: true,
                               commands: ["PrepareClose", "CancelClose", "ConfirmClose", "CancelAttempt", "HumanDecision", "Start", "RecoveryResolve",
                                          "RecoveryStart", "Validate", "CandidateDecision"],
                               queries: ["Snapshot", "Events", "Log", "Receipt", "CloseStatus", "ReviewBundle", "Recovery", "RecoveryInspect",
                                         "Candidate", "CandidateChanges", "CandidateDiff", "ValidationReport", "EventDetail"]),
            launch: launch ?? SettingsLaunch(options), section: section)
    }

    private func values(_ page: SettingsPage) -> [String: String] {
        Dictionary(uniqueKeysWithValues: page.groups.flatMap(\.lines).map { ($0.label, $0.value) })
    }

    func test_dw1_settings_show_the_launch_options_of_a_provider_and_nothing_that_could_be_a_credential() {
        let state = settings()
        let claude = state.page(.claude)
        XCTAssertEqual(claude.groups.map(\.title), ["Found by the core", "Passed to the core at launch", "Authentication", "Reported by the core"])
        let shown = values(claude)
        // D3: until the Executables answer arrives, the found executable is being read.
        XCTAssertEqual(shown["Executable"], "Being read…")
        XCTAssertEqual(shown["Path"], "/Users/dev/.local/share/claude/versions/2.1.311")
        XCTAssertEqual(shown["Version"], "2.1.311 (Claude Code)")
        XCTAssertEqual(shown["SHA-256 digest"], Self.claudeDigest)
        XCTAssertEqual(shown["Sign-in"], "Attested at launch as native-subscription. The sign-in itself is not checked.")
        XCTAssertEqual(shown["Slice configuration"], "Present. The core does not say which providers it holds.")
        // The home directory and the account name are options too; no section shows them.
        for section in SettingsSection.allCases {
            let page = state.page(section)
            let text = (page.groups.flatMap(\.lines).flatMap { [$0.value] + $0.names } + [page.footnote ?? "", page.notice ?? ""]).joined(separator: " ")
            XCTAssertFalse(text.contains(".claude-home"), "\(section)")
            XCTAssertFalse(text.contains("dev-account"), "\(section)")
        }
        // Nothing reads as confirmed: no pill on a provider's lines.
        XCTAssertTrue(claude.groups.flatMap(\.lines).allSatisfy { $0.trailing == .none })

        // A provider without options says so and shows no example.
        let codex = state.page(.codex)
        XCTAssertEqual(codex.empty?.title, "No Codex options were passed at launch")
        XCTAssertEqual(codex.groups.map(\.title), ["Reported by the core"])
    }

    func test_dw1_settings_core_section_says_what_the_sidebar_says_and_only_what_hello_reported() {
        let state = settings(.core)
        let shown = values(state.page)
        let sidebar = ShellStatus(connection: .ready(epoch: 7), admission: "Open", pid: 4242)
        XCTAssertEqual(shown["Core"], sidebar.core)
        XCTAssertEqual(shown["Session"], sidebar.session)
        XCTAssertEqual(shown["Admission"], sidebar.admission)
        XCTAssertEqual(shown["Largest frame"], "1048576 bytes")
        XCTAssertEqual(shown["Largest page"], "8 rows")
        let lines = state.page.groups.flatMap(\.lines)
        XCTAssertEqual(lines.first { $0.label == "Compatibility" }?.trailing, .evidence(.passed, "Compatible"))
        XCTAssertEqual(lines.first { $0.label == "Queries" }?.names.last, "EventDetail")

        var mismatch = settings(.core, connection: .incompatible(epoch: 7))
        XCTAssertEqual(mismatch.page.groups.flatMap(\.lines).first { $0.label == "Compatibility" }?.trailing, .evidence(.failed, "Mismatch"))
        mismatch.hello = HelperHello()
        XCTAssertEqual(values(mismatch.page)["Largest frame"], SettingsState.notReported, "a missing field is not a zero")

        // What an earlier session answered is not shown as current once the helper is gone.
        for connection in [HelperConnection.channelLost("pipe closed"), .ended(status: 1), .launchFailed("no helper"), .handshaking, .notStarted] {
            let gone = settings(.core, connection: connection)
            XCTAssertNil(gone.reported, "\(connection)")
            XCTAssertEqual(gone.page.groups.map(\.title), ["Connection"], "\(connection)")
            XCTAssertEqual(values(gone.page)["Admission"], "Admission unknown")
            XCTAssertEqual(gone.slice.form, .missing)
        }
        XCTAssertEqual(settings(.core, connection: .handshaking).page.notice, "Waiting for the core's answer…")
        XCTAssertEqual(settings(.core, connection: .ended(status: 1)).page.notice, "Nothing is reported while the core is not connected.")
    }

    func test_dw1_settings_state_open_decisions_and_missing_values_without_an_example() {
        let state = settings()
        XCTAssertEqual(SettingsSection.groups.flatMap { $0 }, SettingsSection.allCases, "every section is in the list once")
        XCTAssertEqual(SettingsSection.allCases.map(\.title),
                       ["Claude Code", "Codex", "Core", "Executable discovery", "Defaults & concurrency", "Git & GitHub", "Workspace storage",
                        "Evidence retention", "Security & approvals", "Keyboard shortcuts", "Appearance"])

        // D4 is decided (M3); the number lives in a project record and is shown on Projects, not here.
        let concurrency = state.page(.defaults).groups[1].lines
        XCTAssertTrue(concurrency.allSatisfy { $0.form == .missing })
        XCTAssertEqual(concurrency.first?.value, "Decided (D4): one number for the whole core, set in a project record. Shown on Projects.")
        // D2: the board's validator line no longer holds.
        XCTAssertFalse(values(state.page(.defaults))["Validator"]!.contains("no agent"))
        // M4: repair and review are built and set per project.
        XCTAssertEqual(values(state.page(.defaults))["Repair"], "Set per project (M4): automatic, at most two tries, then a hold for you; 0 switches it off. External actions are never retried. Shown on Projects.")
        // D3: a changed binary is warned about, not refused (a pinned one is refused).
        XCTAssertTrue(values(state.page(.discovery))["Changed binary"]!.hasPrefix("Warned, not refused (D3)"))
        XCTAssertFalse(state.page(.discovery).groups.flatMap(\.lines).contains { $0.value.contains("Not built") })
        // Approvals in force: four kinds, no approve-all.
        XCTAssertEqual(values(state.page(.git))["Combined approval"], "None. There is no approve-all.")
        XCTAssertTrue(values(state.page(.security))["Approval kinds"]!.contains("There is no approve-all."))

        XCTAssertEqual(values(state.page(.storage))["App data"], "/Users/dev/JojoDisposable/data")
        XCTAssertEqual(values(state.page(.storage))["Disposable root"], "/Users/dev/JojoDisposable/run")
        XCTAssertEqual(state.page(.git).groups[0].lines[0].form, .missing, "no worktree root was passed")
        XCTAssertEqual(state.page(.shortcuts).groups[0].lines.compactMap(\.keys), ["⌘Q", "⌘0", "⌥⌘I"])

        // The app sends no request that writes a setting.
        XCTAssertFalse(Request.allCases.contains { $0.rawValue.localizedCaseInsensitiveContains("setting") || $0.rawValue.localizedCaseInsensitiveContains("config") })
        XCTAssertTrue(MainView.built.contains(.settings))
    }

    /// D3: an `Executables` answer as the helper writes it: Claude Code discovered and changed
    /// since an earlier run, Codex refused.
    private static let executablesAnswer: [String: JSON] = {
        func run(_ path: String, _ version: String, _ digest: String, _ event: Int64) -> JSON {
            .object(["path": .text(path), "version": .text(version), "sha256": .text(digest), "profiled": .bool(version.hasPrefix("2.1.311")),
                     "event": .int(event)])
        }
        let old = run("/opt/homebrew/Caskroom/claude-code/2.1.311/claude", "2.1.311 (Claude Code)", String(repeating: "9f3c", count: 16), 40)
        let new = run("/opt/homebrew/Caskroom/claude-code/2.1.320/claude", "2.1.320 (Claude Code)", String(repeating: "a1b2", count: 16), 57)
        return ["providers": .list([
            .object(["provider": .text("ClaudeCode"), "source": .text("discovered"), "state": .text("ready"), "detail": .text(""),
                     "entry": .text("/opt/homebrew/bin/claude"), "path": .text("/opt/homebrew/Caskroom/claude-code/2.1.320/claude"),
                     "version": .text("2.1.320 (Claude Code)"), "sha256": .text(String(repeating: "a1b2", count: 16)), "profiled": .bool(false),
                     "last_run": new, "change": .object(["event": .int(58), "previous": old, "current": new])]),
            .object(["provider": .text("Codex"), "source": .text("discovered"), "state": .text("refused"),
                     "detail": .text("`codex` is not on the login shell's PATH"), "entry": .null, "path": .null, "version": .null,
                     "sha256": .null, "profiled": .null, "last_run": .null, "change": .null]),
        ])]
    }()

    private func discovered(_ section: SettingsSection = .claude, connection: HelperConnection = .ready(epoch: 7)) -> SettingsState {
        var state = settings(section, connection: connection)
        state.executables = ExecutableList(Self.executablesAnswer)
        state.executablesLoad = .loaded
        state.launch.codex = ProviderLaunch(auth: "native-subscription")
        return state
    }

    func test_d3_settings_show_the_executable_the_core_found_and_warn_when_it_changed() throws {
        let list = try XCTUnwrap(ExecutableList(Self.executablesAnswer))
        XCTAssertEqual(list["ClaudeCode"]?.source, .discovered)
        XCTAssertEqual(list["ClaudeCode"]?.change?.previous.version, "2.1.311 (Claude Code)")
        XCTAssertEqual(list["Codex"]?.state, .refused)
        XCTAssertNil(list["Codex"]?.path)
        // A malformed answer is not shown at all.
        var broken = Self.executablesAnswer
        broken["providers"] = .list([.object(["provider": .text("ClaudeCode"), "source": .text("elsewhere")])])
        XCTAssertNil(ExecutableList(broken))

        let claude = discovered().page(.claude)
        let found = try XCTUnwrap(claude.groups.first)
        XCTAssertEqual(found.title, "Found by the core")
        let line = { (label: String) in found.lines.first { $0.label == label } }
        XCTAssertEqual(line("Found")?.value, "On the login shell's PATH as /opt/homebrew/bin/claude")
        XCTAssertEqual(line("Path")?.value, "/opt/homebrew/Caskroom/claude-code/2.1.320/claude")
        XCTAssertEqual(line("Version")?.trailing, .note("Not the profiled release"))
        XCTAssertEqual(line("Last run")?.value, "2.1.320 (Claude Code) · a1b2a1b2a1b2… · recorded at event 57")
        // The change is a warning; nothing says the run was refused.
        let warning = try XCTUnwrap(claude.warning)
        XCTAssertEqual(warning.title, "Claude Code changed since an earlier run")
        XCTAssertTrue(warning.message.contains("2.1.311 (Claude Code) at /opt/homebrew/Caskroom/claude-code/2.1.311/claude"))
        XCTAssertTrue(warning.message.hasSuffix("The run was not refused."))
        XCTAssertFalse(claude.groups.flatMap(\.lines).contains { $0.trailing == .evidence(.failed, "Refused") })

        let codex = discovered().page(.codex)
        let state = try XCTUnwrap(codex.groups.first?.lines.first { $0.label == "State" })
        XCTAssertEqual(state.value, "`codex` is not on the login shell's PATH")
        XCTAssertEqual(state.trailing, .evidence(.failed, "Refused"))
        XCTAssertNil(codex.warning)
        XCTAssertEqual(codex.groups[1].lines.first?.value, "Not passed. The core finds it on the login shell's PATH.")

        // Without a connected core nothing found is shown as current.
        let gone = discovered(connection: .channelLost("pipe closed")).page(.claude)
        XCTAssertEqual(gone.groups.first?.lines.map(\.value), ["Unknown: the core is not connected."])
        XCTAssertNil(gone.warning)
    }

    /// Review aid, as above: Settings & Providers in each section, and its empty, loading,
    /// incompatible and disconnected forms.
    func test_dw1_settings_renders_in_both_appearances() throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        let opening = CGSize(width: 1120, height: 720), tall = CGSize(width: 1120, height: 900)
        var cases: [(String, SettingsState, CGSize)] = SettingsSection.allCases.map {
            ("settings-\($0.rawValue)", settings($0), $0 == .core ? tall : opening)
        }
        cases += [
            ("settings-narrow", settings(.claude), CGSize(width: 900, height: 720)),
            ("settings-empty", settings(.claude, launch: SettingsLaunch()), opening),
            ("settings-loading", settings(.core, connection: .handshaking), opening),
            ("settings-incompatible", settings(.core, connection: .incompatible(epoch: 7)), tall),
            ("settings-disconnected", settings(.core, connection: .channelLost("pipe closed")), opening),
            ("settings-disconnected-provider", settings(.claude, connection: .ended(status: 1)), opening),
            ("settings-claude-changed", discovered(.claude), tall),
            ("settings-codex-refused", discovered(.codex), tall),
        ]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            for (file, state, size) in cases {
                let shell = Themed {
                    AppShell(model: model, screen: .constant(.settings), showInspector: {}) {
                        SettingsView(state: state)
                    }
                }
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: size.width, height: size.height)
                guard let directory else { continue }
                try write(shell, to: directory, "\(file)-\(name).png")
            }
        }
    }

    // MARK: Execution Plan (docs/design/screens/EXECUTION_PLAN.md)

    private static func step(_ n: Int) -> String { String(repeating: String(format: "%x7%xd", n, 15 - n), count: 8) }

    /// M3: a run of `plan` with one step in each kind of state the board draws.
    static func run(_ plan: ApprovedPlan) -> PlanRun {
        let states = ["Integrated", "Review", "Running", "Waiting", "Failed"]
        let details = ["", "", "", "waits for s3", "host validation failed"]
        let steps = plan.steps.enumerated().map { index, step in
            PlanRun.Step(id: step.id, key: "s\(index + 1)", title: "Step title \(index + 1)", kind: index == 0 ? "backend" : nil,
                         provider: step.provider ?? "Codex", state: states[index % states.count],
                         detail: details[index % details.count], attempt: index < 3 ? ids[index] : nil,
                         commit: index == 0 ? String(repeating: "c", count: 40) : nil)
        }
        return PlanRun(plan: plan.plan, revision: plan.revision, state: "RUNNING", control: "RUNNING", repository: ids[4],
                       base: String(repeating: "b", count: 40), branch: "jojo/plan/\(plan.plan)",
                       head: String(repeating: "c", count: 40), steps: steps)
    }
    private static let repositories = ["a1", "b2"].map { String(repeating: $0, count: 16) }

    /// One step of an `ApprovedPlan` answer, as the helper sends it.
    private func stepRow(_ n: Int, repository: Int? = 0, provider: String? = "ClaudeCode") -> JSON {
        let bound = repository.map { JSON.text(Self.repositories[$0]) } ?? .null
        return .object(["step": .text(Self.step(n)), "revision": .int(1), "provider": provider.map(JSON.text) ?? .null,
                        "repository": bound, "priority": repository == nil ? .null : .int(0),
                        "repository_mode": repository == nil ? .null : .text("Shared"), "environment": .text("slice-v1"),
                        "sources": .list(repository == nil ? [] : [.object(["repository": bound, "commit": .text(String(repeating: "c0de", count: 10)),
                                                                             "snapshot": .null])])])
    }

    private func edgeRow(_ n: Int, _ from: Int, _ to: Int, approval: Bool = false) -> JSON {
        .object(["edge": .text(Self.step(100 + n)), "prerequisite": .text(Self.step(from)), "dependent": .text(Self.step(to)),
                 "predicate": .text(approval ? "ArtifactApproved" : "Evidence"),
                 "artifact": approval ? .text(Self.ids[1]) : .null, "artifact_revision": approval ? .int(5) : .null,
                 "evidence": approval ? .null : .text("step-\(from)-candidate"), "evidence_kind": approval ? .null : .text("OutputAvailable")])
    }

    /// An `ApprovedPlan` answer, as the helper sends it.
    private func planAnswer(_ plan: String = DesignShellTests.ids[2], revision: Int64 = 2, validation: String? = DesignShellTests.ids[3],
                            steps: [JSON], edges: [JSON] = [], next: String? = nil) -> [String: JSON] {
        ["plan": .text(plan), "revision": .int(revision), "digest": .text(String(repeating: "d1", count: 32)), "decision": .int(41),
         "prd": .text(Self.ids[0]), "adr": .text(Self.ids[1]), "validation": validation.map(JSON.text) ?? .null,
         "validation_digest": .text(String(repeating: "e2", count: 32)), "steps": .list(steps), "edges": .list(edges),
         "approvals": .list([.object(["artifact": .text(Self.ids[0]), "revision": .int(4), "decision": .int(12)]),
                             .object(["artifact": .text(Self.ids[1]), "revision": .int(5), "decision": .int(19)])]),
         "next": next.map(JSON.text) ?? .null]
    }

    /// Five steps in two repositories: 4 follows 1 and 2, 5 follows 3 and (by an approval) 4.
    private var fiveSteps: [String: JSON] {
        planAnswer(steps: [stepRow(1), stepRow(2), stepRow(3, repository: 1, provider: "Codex"), stepRow(4),
                           stepRow(5, repository: 1, provider: "Codex")],
                   edges: [edgeRow(1, 1, 4), edgeRow(2, 2, 4), edgeRow(3, 3, 5), edgeRow(4, 4, 5, approval: true)])
    }

    func test_dw1_approved_plans_are_read_only_when_the_screen_asks_one_query_per_plan() {
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        model.loadPlans()
        XCTAssertEqual(h.fake.count("ApprovedPlan"), 0, "nothing is read before the first snapshot")
        model.refresh()
        h.fake.reply(h.fake.last("Snapshot")!, ["rows": .list([]), "as_of_event_seq": .int(30), "admission": .text("Open")])
        XCTAssertEqual(h.fake.count("ApprovedPlan"), 0, "the snapshot does not read a plan")
        XCTAssertEqual(model.plansLoad, .idle)

        model.loadPlans()
        XCTAssertEqual(model.plansLoad, .loading)
        XCTAssertEqual(h.fake.last("ApprovedPlan")?.payload, ["after": .null])
        model.loadPlans()
        XCTAssertEqual(h.fake.count("ApprovedPlan"), 1, "one read at a time")
        var first = fiveSteps
        first["next"] = .text(Self.ids[2])
        h.fake.reply(h.fake.last("ApprovedPlan")!, first)
        XCTAssertEqual(h.fake.last("ApprovedPlan")?.payload, ["after": .text(Self.ids[2])], "the next plan is asked for by the cursor")
        XCTAssertEqual(model.plansLoad, .loading)
        h.fake.reply(h.fake.last("ApprovedPlan")!, planAnswer(Self.ids[4], revision: 1, validation: nil, steps: [stepRow(9)]))
        XCTAssertEqual(model.plansLoad, .loaded)
        XCTAssertEqual(model.plans.map(\.plan), [Self.ids[2], Self.ids[4]])
        XCTAssertFalse(model.plansTruncated)
        let plan = model.plans[0]
        XCTAssertEqual(plan.revision, 2)
        XCTAssertEqual(plan.steps.map(\.id), (1...5).map(Self.step))
        XCTAssertEqual(plan.steps[2].provider, "Codex")
        XCTAssertEqual(plan.steps[2].repository, Self.repositories[1])
        XCTAssertEqual(plan.edges.count, 4)
        XCTAssertEqual(plan.edges[3].predicate, "ArtifactApproved")
        // A step fills the five identities of a Start; a plan without a named validation plan leaves that one empty.
        XCTAssertEqual(plan.subjects(step: Self.step(4)),
                       StartSubjects(prd: Self.ids[0], adr: Self.ids[1], plan: Self.ids[2], validation: Self.ids[3], step: Self.step(4)))
        XCTAssertEqual(model.plans[1].subjects(step: Self.step(9)).validation, "")

        // No approved plan is an answer, not a failure.
        model.loadPlans()
        h.fake.replyNull(h.fake.last("ApprovedPlan")!)
        XCTAssertEqual(model.plansLoad, .loaded)
        XCTAssertTrue(model.plans.isEmpty)

        // A refusal, a malformed answer and a lost channel drop what was held and say why.
        model.loadPlans()
        h.fake.error(h.fake.last("ApprovedPlan")!, "PayloadTooLarge: use bounded queries")
        XCTAssertEqual(model.plansLoad, .failed("PayloadTooLarge: use bounded queries"))
        model.loadPlans()
        var broken = fiveSteps
        broken["steps"] = .list([stepRow(1), .object(["step": .text("not an identity")])])
        h.fake.reply(h.fake.last("ApprovedPlan")!, broken)
        XCTAssertEqual(model.plansLoad, .failed("malformed plan"))
        XCTAssertTrue(model.plans.isEmpty)
        XCTAssertNil(ApprovedPlan(planAnswer(steps: [stepRow(1), stepRow(1)])), "two steps with one identity are not a plan")

        // The poll and the refresh send what they sent before.
        let sent = h.fake.count("ApprovedPlan")
        model.pollEvents()
        model.refresh()
        XCTAssertEqual(h.fake.count("ApprovedPlan"), sent)
    }

    func test_dw1_start_outcome_is_what_the_helper_answered() {
        let ids = Self.ids
        let subjects = StartSubjects(prd: ids[0], adr: ids[1], plan: ids[2], validation: ids[3], step: ids[4])
        let h = Harness()
        let model = WorkspaceModel(client: h.client)
        XCTAssertNil(model.startOutcome)
        // What is not five identities is not sent and leaves no outcome.
        model.start(StartSubjects())
        XCTAssertNil(model.startOutcome)

        model.start(subjects)
        XCTAssertEqual(model.startOutcome, .sent)
        let first = h.fake.last("Start")!
        h.fake.error(first, "StartRefused: missing or stale approval: plan")
        XCTAssertEqual(model.startOutcome, .refused("StartRefused: missing or stale approval: plan"))
        XCTAssertEqual(model.status, "Start refused (StartRefused: missing or stale approval: plan); nothing was started",
                       "the status line keeps its words")

        model.start(subjects)
        let second = h.fake.last("Start")!
        h.fake.reply(second, ["acknowledged": .bool(true), "operation_completed": .bool(false), "command": .text(second.id),
                              "epoch": .int(7), "event_seq": .int(30), "response": .text("accepted attempt=\(second.id)"),
                              "replayed": .bool(false), "attempt": .text(second.id), "dispatch": .text("launched")])
        XCTAssertEqual(model.startOutcome, .accepted(attempt: second.id, dispatch: "launched"))
        XCTAssertTrue(model.status.contains("Start accepted for attempt \(second.id) (launched)"), model.status)

        // A lost answer is unknown, stays pending, and a further click changes nothing.
        model.start(subjects)
        h.fake.breakChannel("pipe closed")
        XCTAssertEqual(model.startOutcome, .unknown)
        XCTAssertTrue(model.commandPending)
        model.start(subjects)
        XCTAssertEqual(model.startOutcome, .unknown)
        XCTAssertEqual(h.fake.count("Start"), 3)
    }

}

extension DesignShellTests {
    /// M3: the Projects screen's state: drafts, parsing and what blocks a save.
    func test_m3_projects_draft_parses_kinds_and_blocks_an_incomplete_save() throws {
        var draft = ProjectDraft()
        draft.id = String(repeating: "a", count: 32)
        XCTAssertEqual(draft.problem, "Name the project.")
        draft.name = "Crew"
        XCTAssertEqual(draft.problem, "Choose at least one registered repository.")
        draft.repositories = [String(repeating: "b", count: 32)]
        draft.kinds = "backend=ClaudeCode, mobile=Codex"
        draft.testCommand = "/usr/bin/make test"
        XCTAssertNil(draft.problem)
        let record = try XCTUnwrap(draft.record())
        XCTAssertEqual(record.kinds, [.init(kind: "backend", provider: "ClaudeCode"), .init(kind: "mobile", provider: "Codex")])
        XCTAssertEqual(record.testCommand, ["/usr/bin/make", "test"])
        XCTAssertEqual(Set(record.payload(asNew: true).keys), Set(Request.SaveProject.fields))
        XCTAssertEqual(record.payload(asNew: true)["revision"], .int(0))
        draft.kinds = "backend:ClaudeCode"
        XCTAssertNotNil(draft.problem)
        draft.kinds = ""
        draft.capacity = "9"
        XCTAssertEqual(draft.problem, "Capacity is a number from 1 to 8.")
        // The runner's requests exist only from 1.3; everything else keeps its frame.
        for request in [Request.SaveProject, .Projects, .RunPlan, .ControlPlan, .PlanRun] { XCTAssertEqual(request.minor, 3) }
        XCTAssertEqual(Request.Executables.minor, 4)
        XCTAssertEqual(Request.ApprovedPlan.minor, 1)
        XCTAssertTrue(MainView.built.contains(.projects))
    }

    func test_m3_plan_run_and_projects_decode_the_helpers_answer() throws {
        let step: JSON = .object(["step": .text(Self.ids[0]), "key": .text("api"), "title": .text("Add the API"), "kind": .text("backend"),
                                  "provider": .text("ClaudeCode"), "depends_on": .list([]), "state": .text("Review"), "detail": .text(""),
                                  "attempt": .text(Self.ids[1]), "candidate": .text("d"), "receipt": .int(9), "commit": .null])
        let answer: [String: JSON] = ["plan": .text(Self.ids[2]), "revision": .int(1), "state": .text("RUNNING"), "control": .text("RUNNING"),
                                      "repository": .text(Self.ids[3]), "base": .text("b"), "branch": .text("jojo/plan/x"), "head": .text("b"),
                                      "steps": .list([step])]
        let run = try XCTUnwrap(PlanRun(answer))
        XCTAssertEqual(run.steps.first?.key, "api")
        XCTAssertEqual(run.steps.first?.receipt, 9)
        var broken = answer
        broken["steps"] = .list([.object(["step": .text("x")])])
        XCTAssertNil(PlanRun(broken), "a step without its fields makes the answer unreadable")
        let list = try XCTUnwrap(ProjectList(["capacity": .int(2), "projects": .list([
            .object(["project": .text(Self.ids[0]), "revision": .int(2), "name": .text("Crew"), "repositories": .list([.text(Self.ids[1])]),
                     "instructions": .text("Keep it small."), "test_command": .list([]), "ci_command": .list([]),
                     "kinds": .list([.object(["kind": .text("docs"), "provider": .text("Codex")])]),
                     "default_provider": .text("ClaudeCode"), "capacity": .int(2),
                     "repair_limit": .int(1), "review": .bool(false)]),
        ])]))
        XCTAssertEqual(list.projects.first?.repairLimit, 1)
        XCTAssertEqual(list.projects.first?.review, false)
        XCTAssertEqual(list.projects.first?.kinds.first?.provider, "Codex")
    }

    /// Review aid: Projects empty, listed, being edited, unreadable and disconnected.
    func test_m3_projects_renders_in_both_appearances() throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        let record = ProjectRecord(id: Self.ids[0], revision: 2, name: "Crew", repositories: [Self.ids[1]],
                                   instructions: "Kotlin only. Never edit migrations after merge.", testCommand: ["/usr/bin/make", "test"],
                                   kinds: [.init(kind: "backend", provider: "ClaudeCode"), .init(kind: "mobile", provider: "Codex")],
                                   defaultProvider: "ClaudeCode", capacity: 2)
        let list = ProjectList(projects: [record], capacity: 2)
        let repository = ChatIndex.Repository(id: Self.ids[1], path: "/Users/me/JojoDisposable/crew", pathComplete: true)
        let ready = HelperConnection.ready(epoch: 7)
        var editing = ProjectsState(connection: ready, list: list, load: .loaded, repositories: [repository])
        editing.draft = ProjectDraft(record)
        let cases: [(String, ProjectsState)] = [
            ("projects-empty", ProjectsState(connection: ready, list: ProjectList(projects: [], capacity: 1), load: .loaded)),
            ("projects-listed", ProjectsState(connection: ready, list: list, load: .loaded, repositories: [repository])),
            ("projects-editing", editing),
            ("projects-unreadable", ProjectsState(connection: ready, load: .failed("UnknownType"))),
            ("projects-disconnected", ProjectsState(connection: .channelLost("pipe closed"), list: list, load: .loaded)),
        ]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            for (file, state) in cases {
                let shell = Themed {
                    AppShell(model: model, screen: .constant(.projects), showInspector: {}) { ProjectsView(state: state) }
                }
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: 1120, height: 1150)
                guard let directory else { continue }
                try write(shell, to: directory, "\(file)-\(name).png")
            }
        }
    }
}

/// Every status component and button kind on a card, for the snapshot.
private struct Gallery: View {
    @Environment(\.theme) private var theme

    var body: some View {
        VStack(alignment: .leading, spacing: 22) {
            section("Operational state") {
                ForEach(OperationalState.allCases, id: \.self) { OperationalPill($0) }
            }
            section("Evidence and verification") {
                ForEach(EvidenceState.allCases, id: \.self) { EvidencePill($0) }
            }
            section("Document status and counters") {
                ForEach(DocumentStatus.allCases, id: \.self) { DocumentTag($0) }
                DocumentTag(.approved, label: "PRD rev 4")
                CounterBadge(3, tone: .warn)
                CounterBadge(2, tone: .info)
                CounterBadge(1, tone: .danger)
                CounterBadge(12)
                OperationalPill(.running, standalone: true)
            }
            section("Buttons") {
                Button {} label: { HStack(spacing: 6) { Label("New Feature", systemImage: "plus"); ShortcutHint("⌘N") } }.buttonStyle(.jojo(.primary))
                Button("Import…") {}.buttonStyle(.jojo(.secondary))
                Button("Show Details") {}.buttonStyle(.jojo(.ghost))
                Button("Cancel Attempt…") {}.buttonStyle(.jojo(.destructive))
                Button("Reinspect") {}.buttonStyle(.jojo(.secondary, compact: true))
                Button("Push") {}.buttonStyle(.jojo(.primary)).disabled(true)
                DisabledReason("Unavailable: 5 requirements unmet.")
            }
            section("Dashboard (COMPONENTS.md, section 25)") {
                KPITile(symbol: "play.fill", tile: Palette.blue, label: "Running now", value: "2", unit: "tasks",
                        detail: "T4 on Claude Code · T5 on Codex").frame(width: 230)
                SegmentedCapsule(selection: .constant(DashboardRange.today), label: "Time range")
                StageCell(stage: FeatureStage("Done", "Done"))
                StageCell(stage: FeatureStage("NeedsYou", "ADR rev 3"))
                StageCell(stage: FeatureStage("Running", "2 running"))
                StageCell(stage: .off)
                OperationalPill("Proposed", tone: .info)
                ItemRow(tile: Palette.indigo, symbol: "building.columns", title: "ADR rev 5", detail: "Architecture decision · FT-142",
                        first: true, action: {}) { OperationalPill("Proposed", tone: .info) }
                    .frame(width: 380).cardElevation(radius: 14)
            }
            section("Banners") {
                VStack(spacing: 8) {
                    Banner(.warning, title: "These facts are 38 minutes old",
                           message: "Inspected 09:04. Recovery actions stay unavailable until you reinspect.") {
                        Button("Reinspect") {}.buttonStyle(.jojo(.secondary, compact: true))
                    }
                    Banner(.danger, title: "Jojo core isn't responding",
                           message: "Showing the last known state, read-only. No actions can be requested.")
                }
            }
            HStack(alignment: .top, spacing: 16) {
                titled("Card, panel and notes") {
                    VStack(alignment: .leading, spacing: 8) {
                        Card { Text("Card: radius 12, padding 14 by 16").textStyle(.body).foregroundStyle(theme.text) }
                        SectionHeading("Section heading")
                        Card(.panel) {
                            HStack(spacing: 10) {
                                CheckBox(isOn: true)
                                CheckBox(isOn: false)
                                Text("Panel card: radius 10, padding 12 by 14").textStyle(.body).foregroundStyle(theme.text)
                            }
                        }
                        Footnote("Footnote: one sentence that says what did not change.")
                        LoadingBlock("Loading attempts…")
                    }
                }
            }
            HStack(alignment: .top, spacing: 16) {
                titled("Inspector section, facts and notice") {
                    Card(.panelList) {
                        VStack(spacing: 0) {
                            InspectorSection("Supervision") {
                                FactGrid {
                                    Fact("State") { OperationalPill(.running, label: "Running · supervised") }
                                    Fact("Liveness", "Heartbeat 2s ago")
                                    Fact("Stopped", "Not stopped")
                                }
                            }
                            InspectorSection("Actions", last: true) {
                                Button("Cancel Attempt…") {}.buttonStyle(.jojo(.destructive))
                                NoticeWell("A stopped process is not a successful attempt. Exit is recorded only when the OS confirms it.")
                            }
                        }
                    }
                }
            }
            HStack(alignment: .top, spacing: 16) {
            }
            .environment(\.offscreen, true)
            titled("Search field, filter chips, kind tags and record rows") {
                VStack(alignment: .leading, spacing: 12) {
                    HStack(spacing: 12) {
                        ListSearchField(prompt: "Search decisions, attempts, commits, record IDs…", text: .constant(""))
                        ListSearchField(prompt: "Search", text: .constant("ADR-031"))
                    }
                    .environment(\.offscreen, true)
                    HStack(spacing: 6) {
                        FilterChip(title: "Decision", isOn: true)
                        FilterChip(title: "Approval", count: 4, isOn: true)
                        FilterChip(title: "Attempt", count: 2, isOn: false)
                        FilterChip(title: "Recovery", isOn: false)
                        Spacer(minLength: 12)
                        KindTag("Approval", tone: .ok)
                        KindTag("Validation")
                        KindTag("Recovery", tone: .warn)
                        KindTag("Error", tone: .danger)
                        KindTag("ArtifactVersionCreated")
                    }
                    Card(.panelList) {
                        VStack(spacing: 0) {
                            RecordHeader()
                            RecordGroup("Today")
                            RecordRow(type: "Approval", tone: .ok, text: "Approval request a3 checked: still current",
                                      subject: "Push c44e910", record: "rec_8812")
                            RecordRow(type: "Recovery", tone: .warn, text: "Reinspected J2 · Attempt 1: confirmed dead",
                                      subject: "J2 #1", record: "rec_8805", selected: true)
                            RecordGroup("Epoch 7")
                            RecordRow(type: "ArtifactVersionCreated", text: "content=c0dec0dec0dec0dec0dec0dec0dec0dec0de", mono: true,
                                      subject: "e5f6a7b8e5f6", record: "event 64")
                            RecordRow(type: "EngineStarted", text: "previous_epoch=6;previous_shutdown=CLEAN", mono: true,
                                      record: "event 62", last: true)
                        }
                    }
                }
            }
            titled("Section items, section tiles and setting lines") {
                HStack(alignment: .top, spacing: 16) {
                    VStack(spacing: 1) {
                        SectionItem(title: "Claude Code", symbol: "sparkle", color: RGBA(0xC2643A), selected: true)
                        SectionItem(title: "Codex", symbol: "chevron.left.forwardslash.chevron.right", color: RGBA(0x3C3C44))
                        SectionItem(title: "Security & approvals", symbol: "lock.shield", color: RGBA(0xB5402F))
                        HStack(spacing: 10) {
                            SectionTile(symbol: "sparkle", color: RGBA(0xC2643A), size: 28)
                            SectionTile(symbol: "paintbrush", color: RGBA(0x7B4FD6), size: 28)
                            Spacer(minLength: 0)
                        }
                        .padding(EdgeInsets(top: 10, leading: 8, bottom: 0, trailing: 8))
                    }
                    .frame(width: 220)
                    Card(.panelList) {
                        VStack(spacing: 0) {
                            SettingLine("Path", "/opt/homebrew/bin/claude", form: .identifier)
                            SettingLine("Version", "Detected at launch · capability check passed") { EvidencePill(.passed, label: "Compatible") }
                            SettingLine("Approval binding", "Exact revision and content hash") {
                                Text("Always on").textStyle(.meta).foregroundStyle(theme.text3)
                            }
                            SettingLine("Command palette") { KeyCap("⌘K") } trailing: { EmptyView() }
                            SettingLine("Parallel attempts", "Not reported", form: .missing, last: true)
                        }
                    }
                }
            }
            StateBlock(symbol: "folder", title: "No projects yet",
                       message: "Create a project or import a folder that already has Git repositories.") {
                Button("Create Project") {}.buttonStyle(.jojo(.primary, compact: true))
                Button("Import…") {}.buttonStyle(.jojo(.secondary, compact: true))
            }
            .frame(height: 130)
            NotBuiltYet(screen: .dashboard).frame(height: 110)
            titled("Chat surfaces") {
                VStack(alignment: .leading, spacing: 8) {
                    TextArea(prompt: "Players should be able to…", text: .constant(""), lines: 3)
                    ChatMessage(author: "You", meta: "Discovery · turn 1", human: true) {
                        Text("Players share a link that drops a friend into the lobby.").textStyle(.body)
                    }
                    ChatMessage(author: "Jojo · Discovery", meta: "Claude Code · base 451471") {
                        OperationalPill(.completed, label: "Replied")
                        Text("I read the lobby controller and the navigation graph.").textStyle(.body)
                    }
                }
                .frame(width: 520)
            }
        }
    }

    /// Canvas examples for the gallery only; never shown by the app.
    private struct Sample: Identifiable {
        let title: String, meta: String, detail: String, symbol: String, tone: Tone
        var id: String { title }

        static let rows = [
            Sample(title: "Approve plan revision 3", meta: "FT-142", detail: "Two steps changed since revision 2",
                   symbol: "list.bullet.indent", tone: .warn),
            Sample(title: "PRD rev 5", meta: "FT-007", detail: "Proposed 09:12", symbol: "doc.text", tone: .info),
        ]
    }

    private func titled(_ title: String, @ViewBuilder _ content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title).textStyle(.sectionTitle).foregroundStyle(theme.textStrong)
            content()
        }
        .frame(maxWidth: .infinity, alignment: .topLeading)
    }

    private func section(_ title: String, @ViewBuilder _ content: () -> some View) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title).textStyle(.sectionTitle).foregroundStyle(theme.textStrong)
            HStack(spacing: 8) { content() }
                .padding(.vertical, 14)
                .padding(.horizontal, 16)
                .frame(maxWidth: .infinity, alignment: .leading)
                .cardElevation()
        }
    }
}

// MARK: Feature Chat (design wave 2, M2)

extension DesignShellTests {
    func test_m2_feature_chat_renders_in_both_appearances() throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        let wide = CGSize(width: 1320, height: 1500), narrow = CGSize(width: 900, height: 2200)
        func state(_ turns: [JSON] = [], running: String? = nil, prd: JSON = .null, feature: String? = ChatRows.feature,
                   connection: HelperConnection = .ready(epoch: 7), admission: String = "Open",
                   outcome: StartOutcome? = nil, prdText: String = "") -> FeatureChatState {
            FeatureChatState(connection: connection, admission: admission, queries: ["FeatureChat"],
                             index: ChatIndex(ChatRows.index), indexLoad: .loaded, feature: feature,
                             chat: FeatureChatPage(ChatRows.page(turns, running: running, prd: prd)), chatLoad: .loaded,
                             prdText: prdText, outcome: outcome)
        }
        let objective = "slugify should be usable from the shell."
        let asked = [ChatRows.question(1, prompt: "Should the tool read arguments, standard input, or both?", blocking: true),
                     ChatRows.question(2, prompt: "Is packaging in scope?", blocking: false)]
        let answered = [ChatRows.question(1, prompt: "Should the tool read arguments, standard input, or both?", blocking: true,
                                          answer: "Arguments only."),
                        ChatRows.question(2, prompt: "Is packaging in scope?", blocking: false, answer: "No.")]
        let first = ChatRows.turn(1, message: objective, summary: "The repository has a slugify stub and its tests.",
                                  questions: asked)
        let firstAnswered = ChatRows.turn(1, message: objective, summary: "The repository has a slugify stub and its tests.",
                                          questions: answered)
        let second = ChatRows.turn(2, provider: "Codex", summary: "Drafted the requirements from the answers.", prd: 1)
        let ready = ChatRows.document(revision: 1, ready: true, approved: false)
        let text = "# Slugify from the shell\n\n## Goal\nA title can be turned into a slug without writing code.\n\n## Acceptance criteria\n- **AC1** — A title given as arguments prints its slug and exits with status 0.\n"
        let step = ChatRows.question(3, prompt: "Which shells must the usage text cover?", blocking: true, step: "docs")
        let planTurn = ChatRows.turn(3, kind: "Plan", provider: "Codex",
                                     summary: "Three steps: the entry point and the usage text in parallel, then a check of both.",
                                     plan: ChatRows.plan(drafted: false), questions: [step])
        let cases: [(String, FeatureChatState, CGSize)] = [
            ("feature-chat-investigating", state([ChatRows.turn(1, state: "Running", message: objective)], running: ChatRows.turnID(1)), wide),
            ("feature-chat-blocked", state([first]), wide),
            ("feature-chat-complete", state([firstAnswered, second], prd: ready, prdText: text), wide),
            ("feature-chat-complete-narrow", state([firstAnswered, second], prd: ready, prdText: text), narrow),
            ("feature-chat-plan-draft", state([firstAnswered, second, planTurn],
                                              prd: ChatRows.document(revision: 1, ready: true, approved: true), prdText: text), wide),
            ("feature-chat-failed", state([ChatRows.turn(1, state: "Failed", message: objective)],
                                          outcome: .refused("CapacityOne: a run is active")), wide),
            ("feature-chat-admission-closed", state([first], admission: "Closed"), wide),
            ("feature-chat-disconnected", state([first], connection: .channelLost("pipe closed")), wide),
        ]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            for (file, state, size) in cases {
                let shell = Themed {
                    AppShell(model: model, screen: .constant(.featureChat), showInspector: {}) {
                        FeatureChatView(state: state)
                    }
                }
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: size.width, height: size.height)
                guard let directory else { continue }
                try write(shell, to: directory, "\(file)-\(name).png")
            }
        }
    }
}

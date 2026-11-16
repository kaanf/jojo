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

    func test_dw1_tokens_match_the_design_system_table() {
        XCTAssertEqual(Tokens.dark.surface, RGBA(0x232327))
        XCTAssertEqual(Tokens.light.content, RGBA(0xF5F5F7))
        XCTAssertEqual(Tokens.dark.fill, .white(0.07))
        XCTAssertEqual(Tokens.light.fill, .black(0.05))
        XCTAssertEqual(Tokens.dark.warn, RGBA(0xF4B163))
        XCTAssertEqual(Tokens.light.warn, RGBA(0x9A5800))
        XCTAssertEqual(Tokens.dark.dangerRing, RGBA(0xFF8677, 0.35))
        XCTAssertEqual(Tokens.light.infoTint, RGBA(0x2F6FEB, 0.10))
        XCTAssertEqual(Theme.resolve(.dark), .dark)
        XCTAssertEqual(Theme.resolve(.light), .light)
    }

    func test_dw1_solid_fills_stay_and_elevation_flips() {
        XCTAssertEqual(Tokens.dark.accent, Tokens.light.accent)
        XCTAssertEqual(Tokens.dark.onAccent, Tokens.light.onAccent)
        XCTAssertNil(Tokens.dark.cardShadow)
        XCTAssertNil(Tokens.dark.liftShadow)
        XCTAssertEqual(Tokens.light.cardShadow, Elevation(color: .black(0.05), y: 1, blur: 2))
        XCTAssertEqual(Tokens.light.liftShadow, Elevation(color: .black(0.07), y: 4, blur: 12))
        XCTAssertNotEqual(Tokens.light.content, Tokens.light.surface)
    }

    func test_dw1_status_text_on_its_tint_keeps_4_5_to_1_on_a_card() {
        for tokens in [Tokens.dark, Tokens.light] {
            for tone in Tone.allCases {
                let background = over(tokens.tint(tone), tokens.surface)
                XCTAssertGreaterThanOrEqual(contrast(tokens.foreground(tone), background), 4.5, "\(tone)")
            }
        }
    }

    func test_dw1_status_darkens_in_light() {
        for tone in Tone.allCases {
            XCTAssertLessThan(luminance(Tokens.light.foreground(tone)), luminance(Tokens.dark.foreground(tone)), "\(tone)")
        }
    }

    func test_dw1_operational_states_follow_the_status_table() {
        let tones: [OperationalState: Tone] = [
            .idle: .neutral, .running: .info, .waiting: .neutral, .blocked: .warn, .awaitingApproval: .warn,
            .paused: .neutral, .recoveryRequired: .danger, .failed: .danger, .completed: .ok,
        ]
        XCTAssertEqual(OperationalState.allCases.count, 9)
        for state in OperationalState.allCases {
            XCTAssertEqual(state.tone, tones[state], state.label)
        }
        XCTAssertEqual(Set(OperationalState.allCases.map(\.label)).count, 9)
        XCTAssertEqual(OperationalState.running.glyph, .haloDot)
        XCTAssertEqual(OperationalState.waiting.glyph, .ring)
        XCTAssertEqual(OperationalState.recoveryRequired.glyph, .ringDot)
        // States that share a color differ by glyph, so color is never the only signal.
        XCTAssertNotEqual(OperationalState.blocked.glyph, OperationalState.awaitingApproval.glyph)
        XCTAssertNotEqual(OperationalState.recoveryRequired.glyph, OperationalState.failed.glyph)
    }

    func test_dw1_unknown_evidence_is_never_success_or_failure() {
        XCTAssertEqual(EvidenceState.unknown.tone, .neutral)
        XCTAssertNil(EvidenceState.unknown.symbol)
        for state in EvidenceState.allCases where state != .unknown {
            XCTAssertNotNil(state.symbol, state.label)
        }
        XCTAssertEqual(Set(EvidenceState.allCases.compactMap(\.symbol)).count, 8)
        XCTAssertEqual(EvidenceState.passed.tone, .ok)
        XCTAssertEqual(EvidenceState.sealed.tone, .ok)
        XCTAssertEqual(EvidenceState.failed.tone, .danger)
        XCTAssertEqual(EvidenceState.incomplete.tone, .warn)
        XCTAssertEqual(EvidenceState.needsApproval.tone, .warn)
        for state in [EvidenceState.notRun, .skipped, .invalidated] {
            XCTAssertEqual(state.tone, .neutral, state.label)
        }
    }

    func test_dw1_document_tags_follow_the_status_table() {
        XCTAssertEqual(DocumentStatus.allCases.filter(\.outlined), [.proposed, .stale])
        XCTAssertEqual(DocumentStatus.proposed.tone, .info)
        XCTAssertEqual(DocumentStatus.approved.tone, .ok)
        XCTAssertEqual(DocumentStatus.superseded.tone, .neutral)
        XCTAssertEqual(DocumentStatus.invalidated.tone, .neutral)
        XCTAssertEqual(DocumentStatus.stale.tone, .warn)
    }

    func test_dw1_sidebar_lists_the_canvas_destinations_in_order() {
        XCTAssertEqual(Screen.screens(in: .workspace).map(\.title),
                       ["Dashboard", "Projects", "Approval Inbox", "Recovery Center"])
        XCTAssertEqual(Screen.screens(in: .feature).map(\.title),
                       ["Feature Chat", "Specifications", "Execution Plan", "Agent Runs", "Source Control", "Verification & Evidence"])
        XCTAssertEqual(Screen.screens(in: .system).map(\.title), ["Activity & Audit", "Settings & Providers"])
        XCTAssertEqual(Set(Screen.allCases.map(\.symbol)).count, Screen.allCases.count)
        XCTAssertEqual(Screen.settings.ancestors, ["Jojo", "System"])
        XCTAssertEqual(Screen.agentRuns.ancestors, ["Jojo"])
        XCTAssertTrue(MainView.built.contains(.recoveryCenter))
        XCTAssertFalse(MainView.built.contains(.dashboard))
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
            .environment(\.opaqueSidebar, true)
            .environment(\.colorScheme, scheme)
            .frame(width: 1120, height: 3640)
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
                .environment(\.opaqueSidebar, true)
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

    private var runs: AgentRunsState {
        let ids = Self.ids
        return AgentRunsState(
            connection: .ready(epoch: 7), admission: "Open",
            attempts: [AttemptRecord(id: ids[0], record: "RESERVED", worktree: "CREATING"), AttemptRecord(id: ids[1], record: "RESERVED"),
                       AttemptRecord(id: ids[2], record: "RESERVED", worktree: "CREATING"),
                       AttemptRecord(id: ids[3], record: "RESERVED", recordComplete: false), AttemptRecord(id: ids[4], record: "RESERVED")],
            recovery: [held(ids[1], blockers: ["process state unknown"]), held(ids[2], absent: true, choices: RecoveryChoice.allCases)],
            result: result(ids[0]),
            log: ["stdout: $ swift test", "stdout: Test Suite 'All tests' passed", "result: success"], commandPending: false)
    }

    func test_dw1_agent_runs_lists_the_snapshot_attempts_with_their_worktree_record() {
        let ids = Self.ids
        let rows = [SnapshotRow(key: "artifact:\(ids[4])", value: "digest", detailComplete: true),
                    SnapshotRow(key: "scheduler_attempt:\(ids[1])", value: "RESERVED", detailComplete: true),
                    SnapshotRow(key: "scheduler_attempt:\(ids[0])", value: "RESERVED", detailComplete: false),
                    SnapshotRow(key: "worktree_admin:\(ids[0])", value: "CREATING", detailComplete: true),
                    SnapshotRow(key: "worktree_admin:\(ids[3])", value: "CREATING", detailComplete: true)]
        XCTAssertEqual(AgentRunsState.attempts(in: rows),
                       [AttemptRecord(id: ids[1], record: "RESERVED"),
                        AttemptRecord(id: ids[0], record: "RESERVED", recordComplete: false, worktree: "CREATING")],
                       "only attempt rows, in the snapshot's order; a worktree record alone is not an attempt")
        XCTAssertEqual(AgentRunsState.attempts(in: []), [])
    }

    func test_dw1_agent_runs_state_is_what_the_core_reported_and_never_running() {
        let ids = Self.ids
        var state = runs
        XCTAssertEqual(state.runs.map(\.pill.label), ["Succeeded", "Unknown", "Confirmed dead", "Not loaded", "Not loaded"])
        XCTAssertEqual(state.runs.map(\.pill.state), [.completed, .recoveryRequired, .recoveryRequired, .idle, .idle])
        XCTAssertEqual(state.runs[3].line, "Scheduler record: RESERVED")
        XCTAssertEqual(state.runs[1].line, "Held in Recovery Center")
        for (outcome, expected) in [("Failed", OperationalState.failed), ("Interrupted", .recoveryRequired), ("Cancelled", .idle)] {
            state.result = result(ids[3], outcome: outcome)
            XCTAssertEqual(state.runs[3].pill.state, expected, outcome)
            XCTAssertEqual(state.runs[3].pill.label, outcome)
            XCTAssertEqual(state.runs[0].pill.label, "Not loaded", "one result is loaded at a time")
        }
        state.result = result(ids[3], sealed: false)
        XCTAssertEqual(state.runs[3].pill.label, "Not sealed", "an unsealed attempt may be running or not; the core does not say")
        // A hold outranks a loaded result.
        state.result = result(ids[1])
        XCTAssertEqual(state.runs[1].pill.label, "Unknown")
        for run in state.runs { XCTAssertNotEqual(run.pill.state, .running) }
    }

    func test_dw1_agent_runs_recorded_facts_and_console_say_what_is_loaded() {
        let ids = Self.ids
        var state = runs
        XCTAssertEqual(state.runs[0].facts.map(\.text),
                       ["Scheduler record: RESERVED", "Worktree record: CREATING", "Result sealed: Succeeded · exit 0", "Validation: PASS"])
        XCTAssertEqual(state.runs[0].facts.map(\.tone), [.ok, .ok, .ok, .ok])
        XCTAssertEqual(state.runs[1].facts.last, TimelineEntry(text: "Held for recovery: process state unknown", tone: .danger))
        XCTAssertEqual(state.runs[3].facts.map(\.text), ["Scheduler record: RESERVED … (partial)"])
        XCTAssertEqual(state.console(state.runs[0]).map(\.text), state.log)
        XCTAssertEqual(state.console(state.runs[3]).map(\.text), ["Not loaded."])
        state.result = result(ids[3], outcome: "Failed", reason: "exit 1", validation: "UNKNOWN", decision: "REJECTED", records: 80)
        XCTAssertEqual(state.runs[3].facts.suffix(3).map(\.tone), [.danger, .neutral, .neutral], "unknown is neither success nor failure")
        XCTAssertEqual(state.console(state.runs[3]).last, ConsoleLine(id: 3, text: "… (16 later records are not loaded)", kind: .status(.warn)))
        state.log = []
        XCTAssertEqual(state.console(state.runs[3]).map(\.text), ["Reading the log records…"])
        state.result = result(ids[3], sealed: false, records: 0)
        XCTAssertEqual(state.console(state.runs[3]).map(\.text), ["No log records."])
        XCTAssertEqual(state.runs[3].facts.last, TimelineEntry(text: "No sealed result yet", tone: .neutral))
    }

    func test_dw1_agent_runs_cancel_needs_a_connected_core_and_an_open_unheld_attempt() {
        var state = runs
        XCTAssertEqual(state.cancelBlocked(state.runs[0]), "The result is already sealed.")
        XCTAssertEqual(state.cancelBlocked(state.runs[1]), "On hold: resolve it in Recovery Center first.")
        XCTAssertNil(state.cancelBlocked(state.runs[3]))
        state.commandPending = true
        XCTAssertEqual(state.cancelBlocked(state.runs[3]), "Unavailable while a command awaits the core.")
        state.commandPending = false
        state.connection = .ended(status: 1)
        XCTAssertEqual(state.cancelBlocked(state.runs[3]), "Unavailable: the core is not connected.")
        XCTAssertEqual(state.link, .lost)
        XCTAssertFalse(state.loading)
        state.admission = "Closed"
        XCTAssertFalse(state.admissionClosed, "admission is stated only while the core is connected")
        state.connection = .ready(epoch: 7)
        XCTAssertTrue(state.admissionClosed)
        state.attempts = nil
        XCTAssertTrue(state.loading)
        XCTAssertEqual(state.runs, [])
    }

    /// Review aid, as above: Agent Runs with test rows, at the opening size and wide, and its
    /// empty, loading and disconnected forms.
    func test_dw1_agent_runs_renders_in_both_appearances() throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        let ids = Self.ids
        var lost = runs
        lost.connection = .channelLost("pipe closed")
        var closed = runs
        closed.admission = "Closed"
        var nothing = runs
        nothing.attempts = []
        var loading = runs
        loading.attempts = nil
        let opening = CGSize(width: 1120, height: 720), wide = CGSize(width: 1440, height: 1000)
        let cases: [(String, AgentRunsState, String?, CGSize)] = [
            ("agent-runs", runs, nil, opening), ("agent-runs-wide", runs, nil, wide), ("agent-runs-held", closed, ids[1], wide),
            ("agent-runs-unloaded", runs, ids[3], CGSize(width: 1120, height: 1100)),
            ("agent-runs-disconnected", lost, ids[3], wide), ("agent-runs-empty", nothing, nil, opening),
            ("agent-runs-loading", loading, nil, opening),
        ]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            for (file, state, selected, size) in cases {
                let shell = Themed {
                    AppShell(model: model, screen: .constant(.agentRuns), showInspector: {}) {
                        AgentRunsView(state: state, selected: selected)
                    }
                }
                .environment(\.opaqueSidebar, true)
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: size.width, height: size.height)
                guard let directory else { continue }
                try write(shell, to: directory, "\(file)-\(name).png")
            }
        }
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

    private func verification(_ validation: String = "PASS", sealed: Bool = true, decision: String = "NONE") -> VerificationState {
        VerificationState(
            connection: .ready(epoch: 7), admission: "Open",
            candidate: result(Self.ids[0], sealed: sealed, validation: validation, decision: decision),
            changedFiles: [ChangedFile(path: "Sources/Invite.swift", kind: "M", scope: "ALLOWED"),
                           ChangedFile(path: "Tests/InviteTests.swift", kind: "A", scope: "ALLOWED"),
                           ChangedFile(path: "Package.resolved", kind: "M", scope: "OUT_OF_SCOPE"),
                           ChangedFile(path: "scripts/gates/current.json", kind: "M", scope: "BASELINE"),
                           ChangedFile(path: "notes.txt", kind: "A", scope: "LATER")],
            scope: .fail, scopeDetail: "", diff: Self.sampleDiff,
            log: ["stdout: $ swift test", "stdout: Test Suite 'All tests' passed", "result: success"],
            report: "swift test\nexit 0\n38 tests, 0 failures", commandPending: false)
    }

    func test_dw1_verification_states_the_core_verdict_and_never_counts_unknown_as_passed() {
        let headline = { (s: VerificationState) in VerificationState.headline(s.candidate!) }
        XCTAssertEqual(headline(verification()).title, "Host validation passed")
        XCTAssertEqual(headline(verification()).state, .passed)
        for (validation, state) in [("FAIL", EvidenceState.failed), ("UNKNOWN", .unknown), ("NOT_RUN", .notRun), ("LATER", .unknown)] {
            XCTAssertEqual(headline(verification(validation)).title, "Not verified", validation)
            XCTAssertEqual(headline(verification(validation)).state, state, validation)
        }
        let unsealed = verification(sealed: false)
        XCTAssertEqual(headline(unsealed).title, "Not verified")
        XCTAssertEqual(headline(unsealed).label, "No sealed result")
        XCTAssertEqual(VerificationState.sentence(unsealed.candidate!),
                       "Unknown and not-run checks never count as passed. This attempt has no sealed candidate.")
        XCTAssertEqual(VerificationState.sentence(verification().candidate!),
                       "Unknown and not-run checks never count as passed. Results apply to candidate c4c4c4c4c4c4 only.")
        XCTAssertNil(VerificationState.validationNote(verification().candidate!))
        XCTAssertEqual(VerificationState.validationNote(verification("UNKNOWN").candidate!),
                       "The core could not determine the result. Never shown as passed or failed.")
        XCTAssertEqual(VerificationState.validationNote(verification("NOT_RUN").candidate!),
                       "Jojo has not validated this candidate. Tests the agent says it ran do not count.")
        XCTAssertEqual(VerificationState.decision(verification().candidate!), "None recorded")
        XCTAssertEqual(VerificationState.decision(verification(decision: "ACCEPTED").candidate!), "ACCEPTED")
    }

    func test_dw1_verification_accept_follows_the_core_rule_and_actions_need_a_connected_core() {
        let passing = verification()
        XCTAssertEqual(passing.candidate?.canAccept, true)
        XCTAssertNil(passing.acceptBlocked)
        XCTAssertNil(passing.validateBlocked)
        XCTAssertNil(passing.rejectBlocked)

        for validation in ["FAIL", "UNKNOWN", "NOT_RUN"] {
            let state = verification(validation)
            XCTAssertEqual(state.candidate?.canAccept, false)
            XCTAssertEqual(state.acceptBlocked, "Unavailable: the core accepts only a passing host validation of exactly this candidate.")
            XCTAssertNil(state.validateBlocked, "validation can run again")
            XCTAssertNil(state.rejectBlocked, "an unverified candidate can be rejected")
        }

        var lost = passing
        lost.connection = .channelLost("pipe closed")
        var pending = passing
        pending.commandPending = true
        var empty = passing
        empty.candidate = nil
        let cases = [(lost, "Unavailable: the core is not connected."), (pending, "Unavailable while a command awaits the core."),
                     (verification(decision: "REJECTED"), "Already decided: REJECTED."),
                     (verification(sealed: false), "No sealed candidate."), (empty, "No sealed candidate.")]
        for (state, reason) in cases {
            XCTAssertEqual(state.acceptBlocked, reason)
            XCTAssertEqual(state.validateBlocked, reason)
            XCTAssertEqual(state.rejectBlocked, reason)
        }
    }

    func test_dw1_verification_scope_and_records_say_what_the_core_reported() {
        let state = verification()
        XCTAssertEqual(state.changedFiles.map { VerificationState.scope($0).label },
                       ["In scope", "In scope", "Out of scope", "Protected baseline", "Unknown"])
        XCTAssertEqual(state.changedFiles.map { VerificationState.scope($0).state }, [.passed, .passed, .failed, .failed, .unknown])
        XCTAssertEqual(state.scopeCounts, "2 in scope · 1 out of scope · 1 protected baseline · 1 unknown")
        XCTAssertEqual(state.records.map(\.note), ["c4c4c4c4…c4c4", "3 records", "receipt 4", "3 lines"])
        XCTAssertEqual(state.records.map(\.state), [.sealed, .passed, .passed, .passed])
        XCTAssertEqual(state.console.map(\.text), state.log)

        var bare = verification(sealed: false)
        (bare.changedFiles, bare.scope, bare.diff, bare.log, bare.report) = ([], .notRun, "", [], "")
        XCTAssertNil(bare.scopeCounts)
        XCTAssertEqual(bare.noFiles, "No sealed candidate.")
        XCTAssertEqual(bare.records.map(\.label), ["Not sealed", "Recorded", "Recorded", "Not loaded"])
        XCTAssertEqual(bare.console.map(\.text), ["Reading the log records…"])
        XCTAssertEqual(bare.reportLines.map(\.text), ["No report text loaded."])

        var reading = verification()
        (reading.changedFiles, reading.scope) = ([], .notRun)
        XCTAssertEqual(reading.noFiles, "Reading the changed paths…")
        reading.scope = .pass
        XCTAssertEqual(reading.noFiles, "No changed paths.")
    }

    func test_dw1_diff_lines_are_numbered_from_their_hunk_headers() {
        let lines = DiffLine.parse(Self.sampleDiff)
        XCTAssertEqual(lines.map(\.kind), [.header, .header, .header, .header, .header, .context, .removed, .added, .added, .context,
                                           .context, .header, .removed, .added, .header])
        XCTAssertEqual(lines.map(\.number), [nil, nil, nil, nil, nil, 12, 13, 13, 14, 15, 16, nil, 40, 41, nil])
        // Inside a hunk a line is code even when it looks like a file header.
        XCTAssertEqual(lines[12].text, "+++ old marker")
        XCTAssertEqual(lines[13].text, "--- new marker")
        XCTAssertEqual(lines[6].text, "    var uses = 1")
        XCTAssertEqual(DiffLine.parse(""), [])
        XCTAssertEqual(DiffLine.parse("… (truncated)").map(\.kind), [.header])
        XCTAssertEqual(DiffView.background(dark: true), RGBA(0x17171A))
    }

    /// Review aid, as above: Verification & Evidence with test rows at the opening size and
    /// wide, and its failed, unknown, requested, disconnected and empty forms.
    func test_dw1_verification_renders_in_both_appearances() throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        var failed = verification("FAIL")
        failed.candidate = CandidateReview([
            "attempt": .text(Self.ids[0]), "sealed": .bool(true), "outcome": .text("Succeeded"), "reason": .text("exit 0"),
            "provider_terminal": .text("result: success"), "candidate": .text(String(repeating: "c4", count: 20)),
            "log_records": .int(80),
            "validation": .object(["state": .text("FAIL"), "detail": .text("validation command exited 1"), "receipt": .int(5),
                                   "candidate": .text(String(repeating: "c4", count: 20)),
                                   "reasons": .list([.text("InviteTests.testRevokesOnLobbyClose failed"),
                                                     .text("1 changed path is outside the write scope")])]),
            "decision": .text("NONE")])
        var unknown = verification("UNKNOWN")
        (unknown.changedFiles, unknown.scope, unknown.scopeDetail, unknown.diff) = ([], .unknown, "CandidateTampered", "")
        var pending = verification()
        pending.commandPending = true
        var lost = verification(decision: "ACCEPTED")
        lost.connection = .channelLost("pipe closed")
        var closed = verification("NOT_RUN")
        (closed.admission, closed.report) = ("Closed", "")
        var empty = verification()
        empty.candidate = nil
        let opening = CGSize(width: 1120, height: 720), tall = CGSize(width: 1120, height: 1500), wide = CGSize(width: 1440, height: 1500)
        let cases: [(String, VerificationState, CGSize)] = [
            ("verification", verification(), opening), ("verification-tall", verification(), tall),
            ("verification-failed", failed, wide), ("verification-unknown", unknown, tall),
            ("verification-requested", pending, opening), ("verification-disconnected", lost, tall),
            ("verification-not-run", closed, tall), ("verification-narrow", failed, CGSize(width: 900, height: 1900)),
            ("verification-empty", empty, opening),
        ]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            for (file, state, size) in cases {
                let shell = Themed {
                    AppShell(model: model, screen: .constant(.verification), showInspector: {}) {
                        VerificationView(state: state)
                    }
                }
                .environment(\.opaqueSidebar, true)
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: size.width, height: size.height)
                guard let directory else { continue }
                try write(shell, to: directory, "\(file)-\(name).png")
            }
        }
    }

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
    private func control(_ scope: ValidationVerdict = .fail) -> SourceControlState {
        var loaded = verification()
        loaded.changedFiles = [ChangedFile(path: "Sources/Invite.swift", kind: "Modified", scope: "ALLOWED"),
                               ChangedFile(path: "Tests/InviteTests.swift", kind: "Added", scope: "ALLOWED"),
                               ChangedFile(path: "Sources/LegacyInvite.swift", kind: "Deleted", scope: "ALLOWED")]
        if scope != .pass {
            loaded.changedFiles += [ChangedFile(path: "Package.resolved", kind: "Modified", scope: "OUT_OF_SCOPE"),
                                    ChangedFile(path: "scripts/gates/current.json", kind: "Modified", scope: "BASELINE"),
                                    ChangedFile(path: "notes.txt", kind: "TypeChanged", scope: "LATER")]
        }
        (loaded.scope, loaded.diff) = (scope, Self.controlDiff)
        return SourceControlState(loaded: loaded, base: "9c1e4b2a71f0de5d2c1aa41d07aa3e9ac21f0b6d")
    }

    func test_dw1_source_control_badges_and_paths_follow_the_core_words() {
        let badges = ["Added", "Modified", "Deleted", "TypeChanged", "Renamed"].map(SourceControlState.change)
        XCTAssertEqual(badges.map(\.letter), ["A", "M", "D", "T", "?"])
        XCTAssertEqual(badges.map(\.tone), [.ok, .warn, .danger, .warn, .neutral])
        XCTAssertEqual(badges.map(\.name), ["Added", "Modified", "Deleted", "Type changed", "Renamed"],
                       "a kind this app does not know is named by the core's word, never guessed")
        XCTAssertEqual(SourceControlState.split("src/main/kotlin/lobby/LobbyController.kt").name, "LobbyController.kt")
        XCTAssertEqual(SourceControlState.split("src/main/kotlin/lobby/LobbyController.kt").directory, "src/main/kotlin/lobby")
        XCTAssertEqual(SourceControlState.split("build.gradle.kts").name, "build.gradle.kts")
        XCTAssertEqual(SourceControlState.split("build.gradle.kts").directory, ".")
        XCTAssertEqual(control().ends, "9c1e4b2a71f0 → c4c4c4c4c4c4")
        var reading = control()
        reading.base = ""
        XCTAssertEqual(reading.ends, "base → sealed candidate")
    }

    func test_dw1_source_control_splits_the_diff_by_changed_path_and_counts_its_lines() {
        let state = control()
        let sections = state.sections
        XCTAssertEqual(sections.map(\.path), ["Sources/Invite.swift", "Tests/InviteTests.swift", "Sources/LegacyInvite.swift",
                                               "Package.resolved"])
        XCTAssertEqual(sections.map(\.counts), ["+2 −1", "+3", "−2", "+1 −1"])
        XCTAssertEqual(sections.flatMap(\.lines), DiffLine.parse(state.loaded.diff), "no line of the diff is dropped")
        XCTAssertEqual(sections[1].lines.first?.text, "diff --git a/Tests/InviteTests.swift b/Tests/InviteTests.swift")
        XCTAssertNil(sections.first { $0.path == "scripts/gates/current.json" }, "a path without a section has no counts")

        // A header that names no changed path, and lines before the first header, stay in the diff.
        let stray = DiffLine.parse("warning: note\ndiff --git a/other.txt b/other.txt\n@@ -1 +1 @@\n-a\n+b\n")
        let unnamed = SourceControlState.sections(of: stray, paths: ["Sources/Invite.swift"])
        XCTAssertEqual(unnamed.map(\.path), [nil, nil])
        XCTAssertEqual(unnamed.map(\.lines.count), [1, 4])
        XCTAssertNil(unnamed[0].counts)
        XCTAssertTrue(SourceControlState.sections(of: [], paths: []).isEmpty)
    }

    func test_dw1_source_control_scope_is_the_core_verdict_and_unknown_is_never_in_scope() {
        let failed = control()
        XCTAssertEqual(failed.violations.map(\.path), ["Package.resolved", "scripts/gates/current.json"],
                       "out of scope and protected baseline; a path of unknown scope is not counted as a violation")
        XCTAssertEqual(failed.scopePill.state, .failed)
        XCTAssertEqual(failed.scopePill.label, "2 violations")
        XCTAssertEqual(failed.scopeNotice?.title, "2 changes are outside the approved scope")
        XCTAssertEqual(failed.scopeNotice?.message, "Package.resolved, scripts/gates/current.json. The core's verdict on the write scope "
            + "is Failed. There is no command to revert a path.")
        XCTAssertEqual(failed.scopeNotice?.first, "Package.resolved")
        XCTAssertEqual(failed.scopeFacts.map(\.name), ["Write scope", "Paths inside scope", "Outside the approved scope",
                                                       "Protected baseline", "Scope unknown"])
        XCTAssertEqual(failed.scopeFacts.map(\.state), [.failed, .passed, .failed, .failed, .unknown])
        XCTAssertEqual(failed.scopeFacts.map(\.label), [nil, "3 of 6", "1", "1", "1"])

        var one = control()
        one.loaded.changedFiles.removeAll { $0.scope == "BASELINE" }
        XCTAssertEqual(one.scopePill.label, "1 violation")
        XCTAssertEqual(one.scopeNotice?.title, "1 change is outside the approved scope")
        var unmarked = control()
        unmarked.loaded.changedFiles.removeAll { $0.scope != "ALLOWED" }
        XCTAssertNil(unmarked.scopePill.label)
        XCTAssertEqual(unmarked.scopeNotice?.title, "The write scope check failed")
        XCTAssertNil(unmarked.scopeNotice?.first)

        let passed = control(.pass)
        XCTAssertEqual(passed.scopePill.state, .passed)
        XCTAssertEqual(passed.scopePill.label, "In scope")
        XCTAssertNil(passed.scopeNotice)
        XCTAssertEqual(passed.scopeFacts.map(\.label), [nil, "3 of 3", "None"])
        XCTAssertEqual(passed.scopeFacts.map(\.state), [.passed, .passed, .notRun])

        var unknown = control(.unknown)
        (unknown.loaded.changedFiles, unknown.loaded.scopeDetail, unknown.loaded.diff) = ([], "CandidateTampered", "")
        XCTAssertEqual(unknown.scopePill.state, .unknown)
        XCTAssertEqual(unknown.scopeNotice?.danger, false)
        XCTAssertEqual(unknown.scopeNotice?.message, "The core gave no verdict (CandidateTampered). Unknown is never shown as in scope.")
        XCTAssertEqual(unknown.scopeFacts.map(\.state), [.unknown])
        XCTAssertEqual(unknown.scopeFacts.first?.note, "CandidateTampered")

        var reading = control(.notRun)
        reading.loaded.changedFiles = []
        XCTAssertEqual(reading.scopePill.state, .notRun)
        XCTAssertNil(reading.scopeNotice, "a scope that was not read yet is not a finding")
        var unsealed = control(.unknown)
        unsealed.loaded = verification(sealed: false)
        (unsealed.loaded.changedFiles, unsealed.loaded.scope) = ([], .unknown)
        XCTAssertNil(unsealed.scopeNotice, "without a sealed candidate there is no scope to warn about")
    }

    /// Review aid, as above: Source Control with test rows in two and three columns and in
    /// one, with a chosen path, and its passed, unknown, disconnected and empty forms.
    func test_dw1_source_control_renders_in_both_appearances() throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        var unknown = control(.unknown)
        (unknown.loaded.changedFiles, unknown.loaded.scopeDetail, unknown.loaded.diff, unknown.base) = ([], "CandidateTampered", "", "")
        var lost = control()
        (lost.loaded.connection, lost.loaded.admission) = (.channelLost("pipe closed"), "Closed")
        var closed = control(.pass)
        closed.loaded.admission = "Closed"
        var empty = control()
        empty.loaded.candidate = nil
        let opening = CGSize(width: 1120, height: 720), tall = CGSize(width: 1120, height: 1500), wide = CGSize(width: 1440, height: 1000)
        let cases: [(String, SourceControlState, String?, CGSize)] = [
            ("source-control", control(), nil, opening), ("source-control-tall", control(), nil, tall),
            ("source-control-wide", control(), "Package.resolved", wide),
            ("source-control-missing", control(), "scripts/gates/current.json", opening),
            ("source-control-narrow", control(), nil, CGSize(width: 900, height: 2100)),
            ("source-control-passed", closed, nil, wide), ("source-control-unknown", unknown, nil, tall),
            ("source-control-disconnected", lost, nil, opening), ("source-control-empty", empty, nil, opening),
        ]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            for (file, state, selected, size) in cases {
                let shell = Themed {
                    AppShell(model: model, screen: .constant(.sourceControl), showInspector: {}) {
                        SourceControlView(state: state, selected: selected)
                    }
                }
                .environment(\.opaqueSidebar, true)
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: size.width, height: size.height)
                guard let directory else { continue }
                try write(shell, to: directory, "\(file)-\(name).png")
            }
        }
    }
    // MARK: Approval Inbox (docs/design/screens/APPROVAL_INBOX.md)

    /// A `ReviewBundle` answer as the helper sends it; the upstream approvals show the kind.
    private func bundle(_ subject: String, revision: Int64 = 5, upstream: [Int64] = [12], prior: Int64? = nil) -> ReviewBundle {
        ReviewBundle(["subject": .text(subject), "revision": .int(revision), "hash": .text(String(repeating: "7c1f", count: 16)),
                      "content_hash": .text(String(repeating: "c0de", count: 16)),
                      "displayed_hash": .text(String(repeating: "d15b", count: 16)),
                      "context_hash": .text(String(repeating: "c7e0", count: 16)),
                      "upstream": .list(upstream.map(JSON.int)), "prior_decision": prior.map(JSON.int) ?? .null])!
    }

    private func inbox(upstream: [Int64] = [12], prior: Int64? = nil, stale: Bool = false, outstanding: Bool = false) -> ApprovalInboxState {
        let ids = Self.ids
        return ApprovalInboxState(
            connection: .ready(epoch: 7), admission: "Open",
            rows: [SnapshotRow(key: "artifact:\(ids[4])", value: String(repeating: "ab12", count: 16), detailComplete: true),
                   SnapshotRow(key: "scheduler_attempt:\(ids[0])", value: "RESERVED", detailComplete: true),
                   SnapshotRow(key: "review_document:\(ids[1])", value: String(repeating: "cd34", count: 16), detailComplete: true),
                   SnapshotRow(key: "review_document:\(ids[2])", value: String(repeating: "ef56", count: 16), detailComplete: true)],
            review: bundle(ids[1], upstream: upstream, prior: prior), reviewStale: stale, decisionOutstanding: outstanding,
            candidate: result(ids[0]))
    }

    func test_dw1_approval_inbox_lists_the_snapshot_documents_and_never_calls_them_pending() {
        let ids = Self.ids
        let documents = inbox().documents
        XCTAssertEqual(documents.map(\.id), [ids[4], ids[1], ids[2]], "attempt rows are not documents")
        XCTAssertEqual(documents.map(\.kind), ["Specification", "Decision or plan", "Decision or plan"])
        XCTAssertEqual(documents[0].record, String(repeating: "ab12", count: 16))
        let twice = [SnapshotRow(key: "artifact:\(ids[4])", value: "a", detailComplete: true),
                     SnapshotRow(key: "artifact:\(ids[4])", value: "b", detailComplete: true)]
        XCTAssertEqual(ApprovalInboxState.documents(in: twice).count, 1)
        XCTAssertTrue(ApprovalInboxState.documents(in: []).isEmpty)
    }

    func test_dw1_approval_inbox_reads_the_kind_and_the_decision_from_the_bundle() {
        let ids = Self.ids
        let kinds = [([Int64](), "Specification", "Spec approval", "Approve Revision"),
                     ([12], "Architecture decision", "Spec approval", "Approve Revision"),
                     ([12, 14], "Execution plan", "Plan approval", "Approve Plan")]
        for (upstream, name, approval, approve) in kinds {
            let kind = ApprovalInboxState.kind(bundle(ids[1], upstream: upstream))
            XCTAssertEqual(kind.name, name)
            XCTAssertEqual(kind.approval, approval)
            XCTAssertEqual(kind.approve, approve)
        }
        XCTAssertEqual(ApprovalInboxState.title(bundle(ids[1])), "5e6f7a8b5e6f rev 5")
        XCTAssertEqual(ApprovalInboxState.upstream(bundle(ids[1], upstream: [])), "None")
        XCTAssertEqual(ApprovalInboxState.upstream(bundle(ids[1])), "event 12")
        XCTAssertEqual(ApprovalInboxState.upstream(bundle(ids[1], upstream: [12, 14])), "events 12, 14")
        XCTAssertEqual(ApprovalInboxState.latestDecision(bundle(ids[1])), "None recorded")
        XCTAssertEqual(ApprovalInboxState.latestDecision(bundle(ids[1], prior: 41)), "event 41 · the action is not reported",
                       "the bundle does not say what was decided")
    }

    func test_dw1_approval_inbox_decisions_need_a_connected_core_and_a_current_review() {
        let current = inbox()
        XCTAssertNil(current.decideBlocked)
        XCTAssertFalse(current.requested)
        XCTAssertFalse(current.stale)

        let sent = inbox(stale: true, outstanding: true)
        XCTAssertTrue(sent.requested, "a sent decision is Requested, never recorded")
        XCTAssertFalse(sent.stale)
        XCTAssertEqual(sent.decideBlocked, "Unavailable while a decision awaits the core.")

        let answered = inbox(stale: true)
        XCTAssertTrue(answered.stale)
        XCTAssertFalse(answered.requested)
        XCTAssertEqual(answered.decideBlocked, "Unavailable: this review is stale. Reload the document.")

        var other = inbox(outstanding: true)
        XCTAssertFalse(other.requested, "an outstanding decision on an earlier review does not mark this one")
        XCTAssertNil(other.decideBlocked)
        other.connection = .channelLost("pipe closed")
        XCTAssertEqual(other.decideBlocked, "Unavailable: the core is not connected.")
        other.review = nil
        XCTAssertEqual(other.decideBlocked, "No document loaded.")
        XCTAssertFalse(other.stale)

        let subject = bundle(Self.ids[1])
        XCTAssertNil(ApprovalInboxState.confirmation(.Approve, subject), "approve is sent at once")
        for action in [DecisionAction.Reject, .Revoke] {
            let dialog = ApprovalInboxState.confirmation(action, subject)
            XCTAssertEqual(dialog?.message.contains("revision 5 of document 5e6f7a8b5e6f"), true, "the dialog names the exact object")
            XCTAssertEqual(dialog?.keep.hasPrefix("Keep"), true, "the cancel button says what continues")
        }
        var closed = current
        closed.admission = "Closed"
        XCTAssertTrue(closed.admissionClosed)
        XCTAssertNil(closed.decideBlocked, "a closed admission does not stop a decision on a document")
    }

    /// Review aid, as above: the Approval Inbox with test rows in two and three columns and
    /// in one, and its plan, decided, requested, stale, disconnected and empty forms.
    func test_dw1_approval_inbox_renders_in_both_appearances() throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        var lost = inbox(prior: 41)
        (lost.connection, lost.admission) = (.channelLost("pipe closed"), "Closed")
        var closed = inbox(upstream: [])
        (closed.admission, closed.candidate) = ("Closed", nil)
        var empty = inbox()
        empty.review = nil
        var bare = empty
        (bare.rows, bare.candidate) = ([], nil)
        let opening = CGSize(width: 1120, height: 720), tall = CGSize(width: 1120, height: 1300), wide = CGSize(width: 1440, height: 1000)
        let cases: [(String, ApprovalInboxState, CGSize)] = [
            ("approval-inbox", inbox(), opening), ("approval-inbox-tall", inbox(), tall),
            ("approval-inbox-wide", inbox(upstream: [12, 14], prior: 41), wide),
            ("approval-inbox-narrow", inbox(), CGSize(width: 900, height: 1700)),
            ("approval-inbox-requested", inbox(stale: true, outstanding: true), wide),
            ("approval-inbox-stale", inbox(stale: true), wide),
            ("approval-inbox-closed", closed, wide), ("approval-inbox-disconnected", lost, wide),
            ("approval-inbox-unloaded", empty, opening), ("approval-inbox-empty", bare, opening),
        ]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            for (file, state, size) in cases {
                let shell = Themed {
                    AppShell(model: model, screen: .constant(.approvalInbox), showInspector: {}) {
                        ApprovalInboxView(state: state)
                    }
                }
                .environment(\.opaqueSidebar, true)
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: size.width, height: size.height)
                guard let directory else { continue }
                try write(shell, to: directory, "\(file)-\(name).png")
            }
        }
    }
    // MARK: Specifications (docs/design/screens/SPECIFICATIONS.md)

    func test_dw1_specifications_lists_the_inbox_documents_and_names_the_loaded_one_by_its_bundle() {
        let ids = Self.ids
        let state = SpecificationsState(inbox: inbox())
        XCTAssertEqual(state.documents, inbox().documents, "one list of documents, the Approval Inbox's")
        XCTAssertEqual(state.documents.map(state.detail),
                       ["Specification · record ab12ab12ab12", "Architecture decision · rev 5", "Decision or plan · record ef56ef56ef56"],
                       "only the loaded document has a revision and the kind of its bundle")
        XCTAssertEqual(SpecificationsState(inbox: inbox(upstream: [12, 14])).detail(state.documents[1]), "Execution plan · rev 5")
        XCTAssertEqual(state.record, String(repeating: "cd34", count: 16))

        var unlisted = inbox()
        unlisted.rows = [SnapshotRow(key: "artifact:\(ids[4])", value: "a", detailComplete: true)]
        XCTAssertNil(SpecificationsState(inbox: unlisted).record, "a loaded document the snapshot does not list has no record")
        var empty = inbox()
        empty.review = nil
        XCTAssertNil(SpecificationsState(inbox: empty).record)
        XCTAssertEqual(SpecificationsState(inbox: empty).documents.map(SpecificationsState(inbox: empty).detail)[1],
                       "Decision or plan · record cd34cd34cd34")
    }

    func test_dw1_specifications_traces_only_the_upstream_approvals_of_the_bundle() {
        let ids = Self.ids
        XCTAssertTrue(SpecificationsState.traces(bundle(ids[1], upstream: [])).isEmpty)
        XCTAssertEqual(SpecificationsState.traces(bundle(ids[1], upstream: [12, 14])).map(\.name), ["event 12", "event 14"])
        XCTAssertEqual(SpecificationsState.contentDigest(bundle(ids[1])), "sha256 c0dec0dec0de")
        XCTAssertTrue(MainView.built.contains(.specifications))
    }

    func test_dw1_approval_history_links_open_the_event_journal_and_say_so() {
        XCTAssertEqual(ApprovalInboxState.historyScreen, .activity)
        XCTAssertTrue(MainView.built.contains(ApprovalInboxState.historyScreen), "the link opens a built screen")
        for button in ["Approval History", "View all"] {
            let note = ApprovalInboxState.historyNote(button)
            XCTAssertTrue(note.hasPrefix("\"\(button)\" opens Activity & Audit, the core's event journal"),
                          "the note names the button and the screen it opens")
            XCTAssertTrue(note.contains("not one document's decisions"), "the journal is not filtered to a document")
        }
    }

    /// Review aid, as above: Specifications with test rows in two and three columns and in
    /// one, and its plan, stale, disconnected, unloaded and empty forms.
    func test_dw1_specifications_renders_in_both_appearances() throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        var lost = inbox(prior: 41)
        (lost.connection, lost.admission) = (.channelLost("pipe closed"), "Closed")
        var closed = inbox(upstream: [])
        closed.admission = "Closed"
        var empty = inbox()
        empty.review = nil
        var bare = empty
        bare.rows = []
        let opening = CGSize(width: 1120, height: 720), tall = CGSize(width: 1120, height: 1300), wide = CGSize(width: 1440, height: 1000)
        let cases: [(String, ApprovalInboxState, CGSize)] = [
            ("specifications", inbox(), opening), ("specifications-tall", inbox(), tall),
            ("specifications-wide", inbox(upstream: [12, 14], prior: 41), wide),
            ("specifications-narrow", inbox(), CGSize(width: 900, height: 1700)),
            ("specifications-stale", inbox(stale: true), wide),
            ("specifications-closed", closed, wide), ("specifications-disconnected", lost, wide),
            ("specifications-unloaded", empty, wide), ("specifications-empty", bare, opening),
        ]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            for (file, state, size) in cases {
                let shell = Themed {
                    AppShell(model: model, screen: .constant(.specifications), showInspector: {}) {
                        SpecificationsView(state: SpecificationsState(inbox: state))
                    }
                }
                .environment(\.opaqueSidebar, true)
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: size.width, height: size.height)
                guard let directory else { continue }
                try write(shell, to: directory, "\(file)-\(name).png")
            }
        }
    }

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
        ActivityState(connection: .ready(epoch: 7), admission: "Open", rows: inbox().rows, head: 68, events: journal(), load: .loaded,
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
                .environment(\.opaqueSidebar, true)
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
        XCTAssertEqual(claude.groups.map(\.title), ["Passed to the core at launch", "Authentication", "Reported by the core"])
        let shown = values(claude)
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

        // D4 is open: no capacity is shown, neither the board's nor the roadmap's.
        let concurrency = state.page(.defaults).groups[1].lines
        XCTAssertTrue(concurrency.allSatisfy { $0.form == .missing })
        XCTAssertNil(concurrency.map(\.value).joined().rangeOfCharacter(from: .decimalDigits))
        // D2: the board's validator line no longer holds.
        XCTAssertFalse(values(state.page(.defaults))["Validator"]!.contains("no agent"))
        XCTAssertEqual(values(state.page(.defaults))["Repair"], "Decided, not built: automatic, at most two tries, then a human hold.")
        // D3: a changed binary is refused today, not warned about.
        XCTAssertTrue(values(state.page(.discovery))["Changed binary"]!.hasPrefix("Refused, not warned"))
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
        ]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            for (file, state, size) in cases {
                let shell = Themed {
                    AppShell(model: model, screen: .constant(.settings), showInspector: {}) {
                        SettingsView(state: state)
                    }
                }
                .environment(\.opaqueSidebar, true)
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: size.width, height: size.height)
                guard let directory else { continue }
                try write(shell, to: directory, "\(file)-\(name).png")
            }
        }
    }

    // MARK: Execution Plan (docs/design/screens/EXECUTION_PLAN.md)

    private func plan(connection: HelperConnection = .ready(epoch: 7), admission: String = "Open", slice: Bool? = true,
                      filled: Bool = true, commandPending: Bool = false, outcome: StartOutcome? = nil) -> ExecutionPlanState {
        let ids = Self.ids
        return ExecutionPlanState(
            connection: connection, admission: admission, pid: 4242, slice: slice,
            subjects: filled ? StartSubjects(prd: ids[0], adr: ids[1], plan: ids[2], validation: ids[3], step: ids[4]) : StartSubjects(),
            commandPending: commandPending, outcome: outcome)
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

    func test_dw1_execution_plan_start_needs_a_connected_core_open_admission_and_five_identities() {
        XCTAssertNil(plan().startBlocked)
        XCTAssertEqual(ExecutionPlanState.fields.map(\.label), ["PRD", "ADR", "Plan", "Validation plan", "Step"])

        for connection in [HelperConnection.notStarted, .handshaking, .incompatible(epoch: 7), .channelLost("pipe closed"), .ended(status: 1),
                           .launchFailed("no helper")] {
            XCTAssertEqual(plan(connection: connection).startBlocked, "Unavailable: the core is not connected.", "\(connection)")
            XCTAssertFalse(plan(connection: connection, admission: "Closed").admissionClosed, "admission is not stated without a connection")
        }
        XCTAssertEqual(plan(slice: false).startBlocked, "The core has no slice configuration; it cannot start a step.")
        XCTAssertNil(plan(slice: nil).startBlocked, "a helper that does not say is asked")
        XCTAssertEqual(plan(admission: "Closed").startBlocked, "Admission is closed. The core refuses a Start until it is open.")
        XCTAssertNil(plan(admission: "").startBlocked, "before the first snapshot the helper decides")
        XCTAssertEqual(plan(commandPending: true).startBlocked, "Unavailable while a command awaits the core.")
        XCTAssertEqual(plan(filled: false).startBlocked, "Enter all five identities.")

        var typed = plan()
        typed.subjects.prd = "/bin/sh"
        typed.subjects.step = Self.ids[4].uppercased()
        XCTAssertEqual(typed.startBlocked, "Not an identity: PRD, Step.")
        XCTAssertTrue(typed.invalid(.prd))
        XCTAssertFalse(typed.invalid(.adr))
        typed.subjects.adr = ""
        XCTAssertFalse(typed.invalid(.adr), "an empty field is not marked")
        XCTAssertEqual(typed.startBlocked, "Enter all five identities.")
        // What the screen lets through is exactly what the model sends.
        XCTAssertNotNil(plan().subjects.payload)
        XCTAssertNil(typed.subjects.payload)
    }

    func test_dw1_execution_plan_says_a_refusal_in_words_and_shows_no_plan_and_no_capacity() {
        XCTAssertEqual(ExecutionPlanState.refusal("AdmissionClosed").message, "Admission is closed. Nothing was started.")
        XCTAssertTrue(ExecutionPlanState.refusal("StartRefused: missing or stale approval: validation plan").message.contains("Approval Inbox"))
        XCTAssertTrue(ExecutionPlanState.refusal("StartRefused: step is not part of the approved plan").message.contains("approved plan"))
        XCTAssertEqual(ExecutionPlanState.refusal("StartFailed: spawn; command 0a is committed and nothing was launched").title,
                       "The Start did not launch")
        // A code this app does not know is not explained away.
        let unseen = ExecutionPlanState.refusal("SomethingNew: detail")
        XCTAssertEqual(unseen.title, "The core refused the Start")
        XCTAssertEqual(unseen.message, "Nothing was started. The core's reason is below.")

        // Slice configuration, in words; nothing is stated without a connection.
        XCTAssertEqual(plan().sliceLine.value, "Present")
        XCTAssertEqual(plan(slice: nil).sliceLine.form, .missing)
        XCTAssertEqual(plan(connection: .ended(status: 1)).sliceLine.value, "Unknown: the core is not connected.")
        XCTAssertEqual(plan(connection: .ended(status: 1)).status.admission, "Admission unknown")

        // D4 is open: no number anywhere. D2: the decision in force, marked as not built.
        let missing = ExecutionPlanState.missing
        XCTAssertNil(missing.map(\.value).joined().rangeOfCharacter(from: .decimalDigits))
        XCTAssertEqual(missing.first { $0.label == "Repair" }?.value, "Decided, not built: automatic, at most two tries, then a human hold.")
        XCTAssertEqual(missing.first { $0.label == "Parallel capacity" }?.value, "Not reported, and not decided.")
        XCTAssertEqual(Set(missing.map(\.label)).count, missing.count)

        // The one request about a plan reads the approved one; nothing pauses admission. The screen has no counter.
        XCTAssertEqual(Request.allCases.filter { $0.rawValue.localizedCaseInsensitiveContains("plan") }, [.ApprovedPlan])
        XCTAssertEqual(Request.ApprovedPlan.fields, ["after"])
        XCTAssertFalse(Request.allCases.contains { $0.rawValue.localizedCaseInsensitiveContains("pause") })
        XCTAssertTrue(MainView.built.contains(.executionPlan))
    }

    /// Review aid, as above: Execution Plan empty, filled, with a wrong identity, and after
    /// each answer of the core.
    func test_dw1_execution_plan_renders_in_both_appearances() throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        let opening = CGSize(width: 1120, height: 900), tall = CGSize(width: 1120, height: 1100)
        var wrong = plan()
        wrong.subjects.plan = "plan-rev-3"
        let cases: [(String, ExecutionPlanState, CGSize)] = [
            ("execution-plan-empty", plan(filled: false), opening),
            ("execution-plan-ready", plan(), opening),
            ("execution-plan-invalid", wrong, opening),
            ("execution-plan-narrow", plan(), CGSize(width: 900, height: 900)),
            ("execution-plan-requested", plan(commandPending: true, outcome: .sent), opening),
            ("execution-plan-accepted", plan(outcome: .accepted(attempt: Self.ids[1], dispatch: "launched")), tall),
            ("execution-plan-refused", plan(outcome: .refused("StartRefused: missing or stale approval: plan")), tall),
            ("execution-plan-unknown", plan(commandPending: true, outcome: .unknown), tall),
            ("execution-plan-admission-closed", plan(admission: "Closed"), tall),
            ("execution-plan-no-slice", plan(slice: false), opening),
            ("execution-plan-loading", plan(connection: .handshaking, admission: ""), opening),
            ("execution-plan-incompatible", plan(connection: .incompatible(epoch: 7)), tall),
            ("execution-plan-disconnected", plan(connection: .channelLost("pipe closed")), tall),
        ]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            for (file, state, size) in cases {
                let shell = Themed {
                    AppShell(model: model, screen: .constant(.executionPlan), showInspector: {}) {
                        ExecutionPlanView(state: state)
                    }
                }
                .environment(\.opaqueSidebar, true)
                .environment(\.offscreen, true)
                .environment(\.colorScheme, scheme)
                .frame(width: size.width, height: size.height)
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
                titled("List and rows") {
                    Card(.list) {
                        VStack(spacing: 0) {
                            GroupHeader("Needs you", count: 2)
                            Rows(Sample.rows) { row in
                                ListRow(row.title, meta: row.meta, detail: row.detail, chevron: true) {
                                    LeadingTile(symbol: row.symbol, tone: row.tone)
                                } trailing: {
                                    if row.tone == .warn { EvidencePill(.needsApproval) } else { DocumentTag(.proposed) }
                                }
                            }
                        }
                    }
                }
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
                titled("Timeline and summary fields") {
                    VStack(alignment: .leading, spacing: 8) {
                        Card(CardShape(radius: 10, vertical: 8, horizontal: 14)) {
                            Timeline([TimelineEntry(text: "Attempt admitted by core · claims granted", time: "09:21:04"),
                                      TimelineEntry(text: "Supervision lost", tone: .danger, time: "09:29:12"),
                                      TimelineEntry(text: "No sealed result yet", tone: .neutral),
                                      TimelineEntry(text: "Running unit tests", tone: .info, time: "09:33:02")])
                        }
                        Card(.panel) {
                            WrappingRow {
                                LabeledField("Attempt") { Text("att_142_t4_02").textStyle(.mono).foregroundStyle(theme.text) }
                                LabeledField("State") { OperationalPill(.running, label: "Running · supervised") }
                                LabeledField("Started") { Text("09:21:04").textStyle(.control).foregroundStyle(theme.text) }
                                LabeledField("Duration") { Text("12m 08s").textStyle(.control).foregroundStyle(theme.text) }
                                LabeledField("Base commit") { Text("9c1e4b2").textStyle(.mono).foregroundStyle(theme.text) }
                            }
                        }
                        Console([ConsoleLine(id: 0, text: "$ ./gradlew test --tests InviteServiceTest", kind: .command),
                                 ConsoleLine(id: 1, text: "> Task :compileKotlin UP-TO-DATE"),
                                 ConsoleLine(id: 2, text: "InviteServiceTest > creates multi-use token PASSED", kind: .status(.ok)),
                                 ConsoleLine(id: 3, text: "InviteServiceTest > revokes on lobby close RUNNING", kind: .status(.info)),
                                 ConsoleLine(id: 4, text: "[supervision lost at 08:57:31]", kind: .status(.warn))])
                    }
                }
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
            titled("Verification result rows") {
                Card(.panelList) {
                    VStack(spacing: 0) {
                        ResultRow("Integration", scope: "crew-api", state: .failed, count: "37 / 38",
                                  command: "./gradlew integrationTest", note: "revokes on lobby close: expected 410, got 200") {
                            Button("Inspect Failure") {}.buttonStyle(.jojo(.secondary, compact: true))
                        }
                        Rectangle().fill(theme.hairline).frame(height: 1)
                        ResultRow("Contract check", scope: "crew-api", state: .unknown, count: "n/a",
                                  command: "./gradlew contractTest", note: "Runner killed by signal 9. No result recorded.")
                    }
                }
            }
            HStack(alignment: .top, spacing: 16) {
                titled("Evidence rows") {
                    VStack(alignment: .leading, spacing: 12) {
                        Card(.panelList) {
                            VStack(spacing: 0) {
                                EvidenceRow("Attempt logs · T4 #2", note: "sha256 9f2c…41ab", state: .passed, label: "Hash verified")
                                Rectangle().fill(theme.hairline).frame(height: 1)
                                EvidenceRow("iosApp/Info.plist", note: "M", state: .failed, label: "Out of scope", mono: true)
                            }
                        }
                    }
                }
                titled("Diff, input field and requested notice") {
                    VStack(alignment: .leading, spacing: 12) {
                        DiffView(DiffLine.parse("@@ -41,3 +41,4 @@ fun close(lobby: Lobby) {\n     lobby.state = CLOSED\n-    return 200\n+    invites.revokeAll(lobby)\n+    return 410\n }"))
                        HStack(spacing: 8) {
                            InputField(prompt: "Attempt identity (32 hex)", text: .constant(""), mono: true).frame(width: 240)
                            Button("Load") {}.buttonStyle(.jojo(.secondary, compact: true))
                        }
                        RequestedNotice()
                    }
                }
            }
            .environment(\.offscreen, true)
            titled("Changed files and change badges") {
                HStack(alignment: .top, spacing: 16) {
                    Card(.panelList) {
                        VStack(spacing: 2) {
                            ChangedFileRow(name: "LobbyController.kt", directory: "src/main/kotlin/lobby",
                                           badge: ChangeBadge("M", tone: .warn, name: "Modified"), counts: "+48 −6", selected: true)
                            ChangedFileRow(name: "InviteTokenRepository.kt", directory: "src/main/kotlin/lobby",
                                           badge: ChangeBadge("A", tone: .ok, name: "Added"), counts: "+92")
                            ChangedFileRow(name: "LegacyInvite.kt", directory: "src/main/kotlin/lobby",
                                           badge: ChangeBadge("D", tone: .danger, name: "Deleted"), counts: "−34")
                            ChangedFileRow(name: "build.gradle.kts", directory: ".",
                                           badge: ChangeBadge("M", tone: .warn, name: "Modified"), counts: "+2",
                                           scope: (.failed, "Out of scope"), flagged: true)
                        }
                        .padding(8)
                    }
                    .frame(width: 266)
                    HStack(spacing: 8) {
                        ChangeBadge("A", tone: .ok, name: "Added")
                        ChangeBadge("M", tone: .warn, name: "Modified")
                        ChangeBadge("D", tone: .danger, name: "Deleted")
                        ChangeBadge("T", tone: .warn, name: "Type changed")
                        ChangeBadge("?", tone: .neutral, name: "Unknown")
                    }
                }
            }
            titled("Request rows, value lines and fact lines") {
                HStack(alignment: .top, spacing: 16) {
                    Card(.panelList) {
                        VStack(spacing: 2) {
                            RequestRow(kind: "Architecture decision", age: "18m", title: "ADR-031 rev 5", detail: "Crew · FT-142",
                                       symbol: "building.columns", awaiting: true, selected: true)
                            RequestRow(kind: "Execution plan", age: "18m", title: "Plan rev 3", detail: "Crew · FT-142",
                                       symbol: "list.bullet.indent", awaiting: true)
                            RequestRow(kind: "Specification", age: "1d", title: "PRD-007 rev 4", detail: "Jojo · FT-007",
                                       status: .stale, symbol: "doc.text")
                            RequestRow(kind: "Decision or plan", title: "5e6f7a8b5e6f", mono: true, detail: "record cd34cd34cd34",
                                       symbol: "doc.on.doc")
                        }
                        .padding(8)
                    }
                    .frame(width: 270)
                    Card(.panelList) {
                        VStack(spacing: 0) {
                            ValueLine("Push and PR", "Separate approvals")
                            ValueLine("Combined push + PR", "Off · no policy")
                            ValueLine("Approvals expire", "On content change", last: true)
                        }
                    }
                    .frame(width: 272)
                }
                Card(.panelList) {
                    VStack(spacing: 0) {
                        FactLine("Requested action", "Approve this exact revision of the token storage decision.")
                        FactLine("Why approval", "Architecture decisions bind every task that builds on them. Rev 4 approval does not carry over to changed content.")
                        FactLine("Staleness", last: true) {
                            HStack(spacing: 8) {
                                EvidencePill(.passed, label: "Current")
                                Text("checked by core 09:42").foregroundStyle(theme.text3)
                            }
                        }
                    }
                }
            }
            titled("Document rows, document sections and trace lines") {
                HStack(alignment: .top, spacing: 16) {
                    Card(.panelList) {
                        VStack(spacing: 2) {
                            DocumentRow(name: "ADR-031", detail: "Invite token storage · rev 5", status: .proposed, selected: true)
                            DocumentRow(name: "ADR-031", detail: "Invite token storage · rev 4", status: .approved, indented: true)
                            DocumentRow(name: "ADR-031", detail: "Invite token storage · rev 3", status: .superseded,
                                        indented: true, quiet: true)
                            DocumentRow(name: "ADR-030", detail: "Deep link provider · rev 2", status: .invalidated, quiet: true)
                            DocumentRow(name: "5e6f7a8b5e6f", mono: true, detail: "Decision or plan · record cd34cd34cd34")
                        }
                        .padding(8)
                    }
                    .frame(width: 270)
                    VStack(alignment: .leading, spacing: 14) {
                        DocumentSection("Decision") {
                            Text("Store tokens in an invite_tokens table with expires_at = issued + 7 days.")
                                .textStyle(.body).foregroundStyle(theme.text)
                        }
                        DocumentSection("Traceability") {
                            Card(.panelList) {
                                VStack(spacing: 0) {
                                    TraceLine("AC-7", "Expired link returns 410", target: "T6")
                                    TraceLine("C-3", "Read-time expiry", target: "T4, T6")
                                    TraceLine("event 12", "Bound upstream approval", last: true)
                                }
                            }
                        }
                    }
                }
            }
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

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
            .frame(width: 1120, height: 2030)
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

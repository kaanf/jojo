import JojoCore
import SwiftUI
import XCTest

@testable import JojoApp

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
            .frame(width: 1120, height: 1240)
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

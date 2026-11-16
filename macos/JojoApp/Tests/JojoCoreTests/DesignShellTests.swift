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
            .frame(width: 1120, height: 720)
            guard let directory else { continue }
            let renderer = ImageRenderer(content: shell)
            renderer.scale = 2
            let image = try XCTUnwrap(renderer.nsImage)
            let bitmap = try XCTUnwrap(image.tiffRepresentation.flatMap(NSBitmapImageRep.init(data:)))
            let png = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
            try png.write(to: URL(fileURLWithPath: directory).appendingPathComponent("shell-\(name).png"))
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
            NotBuiltYet(screen: .dashboard)
        }
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

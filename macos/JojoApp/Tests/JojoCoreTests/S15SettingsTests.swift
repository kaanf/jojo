import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

@MainActor
final class S15SettingsTests: XCTestCase {
    private func state(_ section: SettingsSection, gh: String = "Ready", capacity: Int64? = 2,
                       appearance: AppAppearance = .light, connection: HelperConnection = .ready(epoch: 7)) -> SettingsState {
        let options = LaunchOptions.parse([
            "JojoApp", "--helper", "/Applications/Jojo.app/Contents/MacOS/jojo-helper", "--data-root", "/Users/dev/JojoDisposable/data",
            "--worktree-root", "/Users/dev/JojoDisposable/wt", "--claude-auth", "native-subscription",
        ])
        let executables = ExecutableList(providers: [
            ProviderExecutable(provider: "ClaudeCode", source: .discovered, state: .ready, entry: "/opt/homebrew/bin/claude",
                               path: "/opt/homebrew/bin/claude", version: "2.1.311 (Claude Code)",
                               sha256: String(repeating: "ab", count: 32), profiled: true),
        ])
        return SettingsState(connection: connection, admission: "Open", pid: 4242,
                             hello: HelperHello(maxFrame: 65_536, maxPage: 8, slice: true, commands: ["Start"], queries: ["Executables"]),
                             launch: SettingsLaunch(options), executables: executables, executablesLoad: .loaded, section: section,
                             external: ExternalList(ExternalRows.answer(runs: [], actions: [], gh: gh)), capacity: capacity,
                             appearance: appearance)
    }

    private func values(_ page: SettingsPage) -> [String: SettingsLine] {
        Dictionary(page.groups.flatMap(\.lines).map { ($0.label, $0) }, uniquingKeysWith: { a, _ in a })
    }

    func test_s15_git_and_github_show_the_sign_in_the_core_read_and_the_capacity_of_the_project_records() {
        let git = values(state(.git).page)
        XCTAssertEqual(git["Credential helper"]?.value, "credential helper: osxkeychain")
        XCTAssertEqual(git["Credential helper"]?.trailing, .evidence(.passed, "Ready"))
        XCTAssertEqual(git["Account"]?.value, "Via the gh CLI · Logged in to github.com account kaanf (keyring)")
        XCTAssertEqual(git["Branch prefix"]?.value, "jojo/plan/ — a finished plan's branch is pushed under this name")
        XCTAssertEqual(git["Worktree root"]?.value, "/Users/dev/JojoDisposable/wt")
        XCTAssertTrue(git["Push and pull request"]!.value.contains("its own authorization"))
        let signedOut = values(state(.git, gh: "Missing").page)
        XCTAssertEqual(signedOut["Account"]?.trailing, .evidence(.unknown, "Missing"))
        let gone = values(state(.git, connection: .channelLost("pipe closed")).page)
        XCTAssertEqual(gone["Account"]?.value, "Unknown: the core is not connected.")

        XCTAssertEqual(values(state(.defaults).page)["Parallel attempts"]?.value,
                       "2 across all projects, set by the project record saved last (D4). Edited on Projects.")
        XCTAssertTrue(values(state(.security).page)["Approval kinds"]!.value.hasPrefix("Specification, plan, push and pull request"))
    }

    func test_s15_a_provider_is_compatible_when_the_core_found_its_profiled_release() {
        let claude = state(.claude).page
        let found = values(claude)
        XCTAssertEqual(found["Version"]?.trailing, .evidence(.passed, "Compatible"))
        XCTAssertEqual(claude.groups.map(\.title).suffix(2), ["Capabilities", "Usage"])
        XCTAssertEqual(found["Session resume"]?.form, .missing)
        XCTAssertEqual(found["Tokens"]?.value, SettingsState.notReported)
    }

    func test_s15_appearance_is_the_one_setting_and_light_is_the_default() {
        XCTAssertEqual(AppAppearance.allCases.map(\.rawValue), ["System", "Light", "Dark"])
        XCTAssertEqual(AppAppearance.light.named, .aqua)
        XCTAssertEqual(AppAppearance.dark.named, .darkAqua)
        XCTAssertNil(AppAppearance.system.named, "System follows the Mac")
        XCTAssertEqual(SettingsState(connection: .ready(epoch: 1), admission: "Open").appearance, .light)
        XCTAssertEqual(state(.appearance).page.groups.flatMap(\.lines).map(\.label), ["Density", "Reduce motion"])
        XCTAssertTrue(state(.core).page.footnote?.contains("Appearance is this app's own setting") == true)
        // The core still has no request that writes a setting.
        XCTAssertFalse(Request.allCases.contains { $0.rawValue.localizedCaseInsensitiveContains("setting") })
    }

    /// Review aid: Settings & Providers from its Mac board in its sections.
    func test_s15_settings_renders_in_both_appearances() throws {
        let heading = ("Settings & Providers", "System · Settings & Providers")
        let size = CGSize(width: 1280, height: 900)
        let cases: [(String, SettingsState)] = [
            ("s15-settings-claude", state(.claude)), ("s15-settings-git", state(.git)),
            ("s15-settings-git-signed-out", state(.git, gh: "Missing")), ("s15-settings-defaults", state(.defaults)),
            ("s15-settings-appearance", state(.appearance)), ("s15-settings-appearance-dark", state(.appearance, appearance: .dark)),
            ("s15-settings-core", state(.core)),
            ("s15-settings-disconnected", state(.git, connection: .channelLost("pipe closed"))),
        ]
        for (file, state) in cases {
            try DetailRows.render(SettingsView(state: state), screen: .settings, heading: heading, size: size, file: file)
        }
    }
}

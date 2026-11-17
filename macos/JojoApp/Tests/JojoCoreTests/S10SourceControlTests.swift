import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

/// A sealed candidate of attempt 3 with three changed paths, one outside the write scope.
enum CandidateRows {
    static let attempt = DetailRows.id(3)

    static func result(validation: String = "PASS", decision: String = "NONE", sealed: Bool = true) -> CandidateReview {
        let digest = String(repeating: "c4", count: 20)
        return CandidateReview(["attempt": .text(attempt), "sealed": .bool(sealed), "outcome": .text("Succeeded"),
                                "reason": .text("exit 0"), "provider_terminal": .text("result: success"),
                                "candidate": sealed ? .text(digest) : .null, "log_records": .int(3),
                                "validation": .object(["state": .text(validation), "detail": .text(validation == "FAIL" ? "exit 1" : ""),
                                                       "receipt": .int(91), "candidate": .text(digest),
                                                       "reasons": .list(validation == "FAIL" ? [.text("cli: exit 1")] : [])]),
                                "decision": .text(decision)])!
    }

    static let files = [ChangedFile(path: "cli.py", kind: "Added", scope: "ALLOWED"),
                        ChangedFile(path: "docs/USAGE.md", kind: "Modified", scope: "ALLOWED"),
                        ChangedFile(path: "setup.cfg", kind: "Modified", scope: "OUT_OF_SCOPE")]

    static let diff = """
        diff --git a/cli.py b/cli.py
        new file mode 100644
        --- /dev/null
        +++ b/cli.py
        @@ -0,0 +1,3 @@
        +import sys
        +from slugify import slugify
        +print(slugify(" ".join(sys.argv[1:])))
        diff --git a/docs/USAGE.md b/docs/USAGE.md
        --- a/docs/USAGE.md
        +++ b/docs/USAGE.md
        @@ -4,3 +4,4 @@
         ## Usage
        -Call slugify() from Python.
        +Call slugify() from Python,
        +or run python3 cli.py Hello World.
         ## Example
        diff --git a/setup.cfg b/setup.cfg
        --- a/setup.cfg
        +++ b/setup.cfg
        @@ -1,2 +1,2 @@
        -name = slugify
        +name = slugify-cli
         version = 1.0
        """

    static let detail = AttemptDetail(RunRows.detail(3, validations: [RunRows.run(1, passed: false), RunRows.run(2, passed: true)],
                                                     live: false))!

    static let candidates = [AttemptSummary(RunRows.attempt(3, state: "Succeeded", key: "usage", ordinal: 1, validation: "Passed"))!]
}

@MainActor
final class S10SourceControlTests: XCTestCase {
    private func state(layout: DiffLayout = .unified, file: String? = nil, verdict: ValidationVerdict = .fail,
                       result: CandidateReview? = CandidateRows.result(), connection: HelperConnection = .ready(epoch: 7)) -> SourceControlState {
        SourceControlState(connection: connection, scope: DetailRows.scope, candidates: CandidateRows.candidates, result: result,
                           files: result == nil ? [] : CandidateRows.files, diff: result == nil ? "" : CandidateRows.diff,
                           base: "9c1e4b2aa0", verdict: verdict, detail: CandidateRows.detail, layout: layout, chosenFile: file)
    }

    func test_s10_the_diff_splits_by_file_with_both_line_numbers() throws {
        let diffs = state().diffs
        XCTAssertEqual(diffs.map(\.path), ["cli.py", "docs/USAGE.md", "setup.cfg"])
        XCTAssertEqual(diffs.map(\.counts), ["+3", "+2 −1", "+1 −1"])
        let usage = diffs[1].rows.filter { $0.kind != .hunk }
        XCTAssertEqual(usage.first?.old, 4)
        XCTAssertEqual(usage.first?.new, 4)
        XCTAssertEqual(usage[1].kind, .removed)
        XCTAssertEqual(usage[1].old, 5)
        XCTAssertEqual(usage[2].new, 5)
    }

    func test_s10_badges_scope_and_violations_follow_the_core_words() {
        XCTAssertEqual(SourceControlState.change("Added").letter, "A")
        XCTAssertEqual(SourceControlState.change("TypeChanged").name, "Type changed")
        XCTAssertEqual(SourceControlState.split("docs/USAGE.md").directory, "docs")
        XCTAssertEqual(SourceControlState.split("setup.cfg").directory, ".")
        XCTAssertEqual(SourceControlState.violations(in: CandidateRows.files).map(\.path), ["setup.cfg"])
        XCTAssertNil(SourceControlState.scope(ChangedFile(path: "x", kind: "Added", scope: "UNKNOWN")).ok, "unknown is never in scope")
        XCTAssertEqual(state().status.first { $0.key == "Scope" }?.pill?.0, "1 violation")
        XCTAssertEqual(state(verdict: .pass).status.first { $0.key == "Scope" }?.pill?.0, "In scope")
    }

    func test_s10_the_candidate_names_its_task_head_and_scope_patterns() {
        let state = state()
        XCTAssertEqual(state.from, "api · Attempt 2")
        XCTAssertEqual(state.patterns, ["cli.py"])
        XCTAssertEqual(SourceControlState.placeTail(CandidateRows.detail), "api")
        XCTAssertEqual(state.chosen?.path, "cli.py", "without a choice, the first file")
        XCTAssertEqual(self.state(file: "setup.cfg").chosen?.path, "setup.cfg")
        XCTAssertEqual(state.status.first { $0.key == "Push" }?.value, "Not available yet (M5)")
    }

    /// Review aid: with JOJO_SNAPSHOT_DIR set, writes Source Control in its states.
    func test_s10_source_control_renders_in_both_appearances() throws {
        let heading = ("Source Control", DetailRows.scope.place("api"))
        let controls = AnyView(HStack(spacing: 10) {
            SegmentedCapsule(selection: .constant(DiffLayout.unified), label: "Diff layout")
            HeaderButtons(buttons: [HeaderButton(title: "Review Changes"), HeaderButton(title: "Inspect CI", disabled: "Not available yet")])
        })
        let cases: [(String, SourceControlState, CGSize)] = [
            ("s10-source-control", state(file: "docs/USAGE.md"), CGSize(width: 1440, height: 1360)),
            ("s10-source-control-side-by-side", state(layout: .sideBySide, file: "docs/USAGE.md"), CGSize(width: 1440, height: 1360)),
            ("s10-source-control-in-scope", state(verdict: .pass), CGSize(width: 1440, height: 1360)),
            ("s10-source-control-none", state(result: nil), CGSize(width: 1440, height: 1000)),
            ("s10-source-control-narrow", state(), CGSize(width: 880, height: 2000)),
        ]
        for (file, state, size) in cases {
            try DetailRows.render(SourceControlView(state: state), screen: .sourceControl, heading: heading, controls: controls,
                                  size: size, file: file)
        }
    }
}

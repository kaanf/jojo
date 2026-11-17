import SwiftUI
import XCTest

@testable import JojoApp
@testable import JojoCore

/// Protocol 1.9 answers and the feature they belong to, written for the step 4 screens.
enum DetailRows {
    static let t0 = OverviewRows.t0
    static let feature = OverviewRows.feature
    static func id(_ n: Int) -> String { OverviewRows.id(n) }

    static let summary = FeatureSummary(id: feature, number: 3, name: "Slugify from the shell", repositoryName: "slugify",
                                        projectName: "Sample",
                                        stages: [FeatureStage("Done", "Done"), FeatureStage("Done", "Done"),
                                                 FeatureStage("NeedsYou", "Plan rev 1"), .off, .off],
                                        running: 1, blocked: 1, awaiting: 2)
    static let scope = FeatureScope(feature: feature, summary: summary)

    static let prdText = """
        # Slugify from the shell

        ## Goal
        Call `slugify` from a terminal.

        ## Acceptance criteria
        - AC-1 Prints the slug of its arguments
        - AC-2 Exits 2 without arguments
        """

    static let prdTextEarlier = """
        # Slugify from the shell

        ## Goal
        Call slugify from Python.

        ## Acceptance criteria
        - AC-1 Prints the slug of its arguments
        """

    static func revision(_ r: Int64, decision: String?, superseded: Bool) -> JSON {
        .object(["revision": .int(r), "created_at_unix_ms": .int(t0 + r * 60_000), "digest": .text("7c1f\(r)aa00e02"),
                 "author": .text("Model"), "decision": decision.map(JSON.text) ?? .null,
                 "decided_at_unix_ms": decision == nil ? .null : .int(t0 + r * 60_000 + 30_000), "superseded": .bool(superseded)])
    }

    /// The `Document` answer of a requirements document at revision 2, pending, whose
    /// revision 1 was approved.
    static func prd(revision: Int64 = 2, text: String = prdText, findings: [JSON] = []) -> [String: JSON] {
        ["document": .text(id(1)), "kind": .text("Prd"), "title": .text("Slugify from the shell"), "feature": .text(feature),
         "feature_name": .text("Slugify from the shell"), "revision": .int(revision), "newest": .int(2),
         "approval": .text("Pending"), "stale": .bool(true),
         "revisions": .list([Self.revision(2, decision: nil, superseded: false), Self.revision(1, decision: "Approved", superseded: true)]),
         "history": .list([.object(["revision": .int(1), "decision": .text("Approved"), "at_unix_ms": .int(t0 + 90_000),
                                    "digest": .text("7c1f1aa00e02"), "applies": .bool(false)])]),
         "upstream": .list([]),
         "downstream": .list([.object(["document": .text(id(2)), "kind": .text("Adr"), "revision": .int(1), "approval": .text("Waiting")])]),
         "counts": .object(["goal": .int(1), "non_goals": .int(0), "actors": .int(1), "use_cases": .int(2),
                            "acceptance_criteria": .int(2), "nfrs": .int(1), "assumptions": .int(1), "blockers": .int(0)]),
         "findings": .list(findings),
         "criteria": .list([.object(["id": .text("AC-1"), "statement": .text("Prints the slug of its arguments"),
                                     "use_cases": .list([.text("UC-1")])]),
                            .object(["id": .text("AC-2"), "statement": .text("Exits 2 without arguments"), "use_cases": .list([])])]),
         "offset": .int(0), "next": .int(Int64(text.count)), "total": .int(Int64(text.count)), "complete": .bool(true),
         "text": .text(text)]
    }

    static let documents = DocumentList(documents: [
        DocumentSummary(id: id(3), kind: "ExecutionPlan", revision: 1, feature: feature, featureName: "Slugify from the shell",
                        approval: "Pending", created: helperDate(t0 + 300_000)),
        DocumentSummary(id: id(2), kind: "Adr", revision: 1, feature: feature, featureName: "Slugify from the shell",
                        approval: "Approved", created: helperDate(t0 + 200_000)),
        DocumentSummary(id: id(1), kind: "Prd", revision: 2, title: "Slugify from the shell", feature: feature,
                        featureName: "Slugify from the shell", approval: "Pending", stale: true, created: helperDate(t0 + 120_000)),
        DocumentSummary(id: id(8), kind: "Prd", revision: 1, title: "Another feature", feature: id(9), approval: "Pending"),
    ], candidates: [PendingCandidate(id: id(5), key: "api", title: "Add the entry point", featureName: "Slugify from the shell",
                                     receipt: 91, validated: helperDate(t0 + 400_000))])

    static func bundle(_ subject: String, revision: Int64 = 2) -> ReviewBundle {
        let digest = String(repeating: "7c1f", count: 16)
        return ReviewBundle(["subject": .text(subject), "revision": .int(revision), "hash": .text(digest),
                             "content_hash": .text(digest), "displayed_hash": .text(digest), "context_hash": .text(digest),
                             "upstream": .list([]), "prior_decision": .null])!
    }

    static let now = helperDate(t0 + 600_000)!

    @MainActor static func render(_ view: some View, screen: Screen, heading: (title: String, subtitle: String)?, controls: AnyView? = nil,
                       primary: AnyView? = nil, size: CGSize, file: String) throws {
        let model = AppModel(options: LaunchOptions(), trace: Tracer(nil))
        let directory = ProcessInfo.processInfo.environment["JOJO_SNAPSHOT_DIR"]
        for (name, scheme) in [("dark", ColorScheme.dark), ("light", .light)] {
            let shell = Themed {
                AppShell(model: model, screen: .constant(screen), showInspector: {}, controls: controls, primary: primary,
                         heading: heading) { view }
            }
            .environment(\.offscreen, true)
            .environment(\.colorScheme, scheme)
            .frame(width: size.width, height: size.height)
            guard let directory else { continue }
            let renderer = ImageRenderer(content: shell)
            renderer.scale = 2
            let image = try XCTUnwrap(renderer.nsImage)
            let bitmap = try XCTUnwrap(image.tiffRepresentation.flatMap(NSBitmapImageRep.init(data:)))
            let png = try XCTUnwrap(bitmap.representation(using: .png, properties: [:]))
            try png.write(to: URL(fileURLWithPath: directory).appendingPathComponent("\(file)-\(name).png"))
        }
    }
}

@MainActor
final class S6SpecificationsTests: XCTestCase {
    private func state(_ detail: DocumentDetail? = DocumentDetail(DetailRows.prd()), review: ReviewBundle? = DetailRows.bundle(DetailRows.id(1)),
                       mode: SpecificationMode = .document, comparison: DocumentDetail? = nil,
                       connection: HelperConnection = .ready(epoch: 7)) -> SpecificationsState {
        SpecificationsState(connection: connection, admission: "Open", scope: DetailRows.scope, documents: DetailRows.documents,
                            review: review, detail: detail, detailLoad: detail == nil ? .loading : .loaded,
                            comparison: comparison, mode: mode, now: DetailRows.now)
    }

    func test_s6_a_document_decodes_at_1_9_with_its_revisions_decisions_and_criteria() throws {
        let detail = try XCTUnwrap(DocumentDetail(DetailRows.prd()))
        XCTAssertEqual(detail.revisions.map(\.revision), [2, 1])
        XCTAssertEqual(detail.revisions.last?.decision, "Approved")
        XCTAssertEqual(detail.previous?.revision, 1)
        XCTAssertEqual(detail.history.first?.applies, false)
        XCTAssertEqual(detail.counts?["acceptance_criteria"], 2)
        XCTAssertEqual(detail.criteria?.first?.useCases, ["UC-1"])
        XCTAssertEqual(detail.downstream.first?.kind, "Adr")
        XCTAssertEqual(Request.Document.minor, 9)
        XCTAssertEqual(Request.Document.fields, ["document", "revision", "offset"])
        XCTAssertNil(DocumentDetail(["document": .text("x")]), "an answer without its fields is not a document")
    }

    func test_s6_the_held_features_documents_are_listed_and_counted_in_the_sidebar() {
        let state = state()
        XCTAssertEqual(state.listed.map(\.id), [DetailRows.id(3), DetailRows.id(2), DetailRows.id(1)], "another feature's document is left out")
        XCTAssertEqual(SpecificationsState.proposed(DetailRows.documents, scope: DetailRows.scope), 2)
        XCTAssertEqual(SpecificationsState.proposed(DetailRows.documents, scope: FeatureScope()), 3, "every feature without a held one")
        XCTAssertEqual(state.title, "PRD · Slugify from the shell")
        XCTAssertEqual(DetailRows.scope.place("Specifications"), "Sample › FT-3 Slugify from the shell › Specifications")
    }

    func test_s6_the_text_is_cut_into_sections_and_two_revisions_compare_line_by_line() throws {
        let sections = SpecificationsState.sections(DetailRows.prdText)
        XCTAssertEqual(sections.map(\.heading), ["Slugify from the shell", "Goal", "Acceptance criteria"])
        let compared = SpecificationsState.compare(["a", "b", "c"], ["a", "c", "d"])
        XCTAssertEqual(compared.map(\.kind), [.same, .removed, .same, .added])
        let state = state(mode: .compare, comparison: DocumentDetail(DetailRows.prd(revision: 1, text: DetailRows.prdTextEarlier)))
        XCTAssertEqual(state.changedSections, 2, "the goal and the criteria changed; the title did not")
    }

    func test_s6_approve_is_offered_only_for_the_newest_revision_of_the_loaded_review() throws {
        XCTAssertNil(state().approveBlocked)
        XCTAssertEqual(state(DocumentDetail(DetailRows.prd(revision: 1))).approveBlocked, "Only the newest revision can be approved.")
        XCTAssertEqual(state(review: DetailRows.bundle(DetailRows.id(2))).approveBlocked, "Loading the review of this revision.")
        XCTAssertEqual(state(connection: .channelLost("gone")).approveBlocked, "Unavailable: the core is not connected.")
        var outstanding = state()
        outstanding.decisionOutstanding = true
        XCTAssertEqual(outstanding.approveBlocked, "Unavailable while a decision awaits the core.")
        XCTAssertEqual(state().changeRequest, "Changes requested on PRD rev 2: ")
    }

    func test_s6_the_approval_scope_says_what_an_approval_lets_the_core_do() throws {
        let detail = try XCTUnwrap(DocumentDetail(DetailRows.prd()))
        XCTAssertEqual(SpecificationsState.authorizes(detail).yes, "Planning may build on this revision (ADR rev 1).")
        XCTAssertTrue(SpecificationsState.authorizes(detail).no.hasPrefix("Start tasks, push, deploy"))
        XCTAssertEqual(SpecificationsState.findingText(DocumentFinding(path: "blockers[0]", problem: "OpenBlocker")),
                       "Still lists an unresolved blocker.")
    }

    /// Review aid: with JOJO_SNAPSHOT_DIR set, writes Specifications in its states.
    func test_s6_specifications_renders_in_both_appearances() throws {
        let heading = (state().title, DetailRows.scope.place("Specifications"))
        let controls = AnyView(HStack(spacing: 10) {
            SegmentedCapsule(selection: .constant(SpecificationMode.document), label: "View")
            HeaderButtons(buttons: [HeaderButton(title: "Approval History"), HeaderButton(title: "Affected Tasks")])
        })
        let findings = [JSON.object(["path": .text("blockers[0]"), "problem": .text("OpenBlocker")])]
        let cases: [(String, SpecificationsState, CGSize)] = [
            ("s6-specifications", state(), CGSize(width: 1440, height: 1320)),
            ("s6-specifications-compare", state(mode: .compare, comparison: DocumentDetail(DetailRows.prd(revision: 1, text: DetailRows.prdTextEarlier))),
             CGSize(width: 1440, height: 1320)),
            ("s6-specifications-findings", state(DocumentDetail(DetailRows.prd(findings: findings))), CGSize(width: 1440, height: 1320)),
            ("s6-specifications-loading", state(nil), CGSize(width: 1440, height: 900)),
            ("s6-specifications-narrow", state(), CGSize(width: 960, height: 1800)),
            ("s6-specifications-disconnected", state(connection: .channelLost("pipe closed")), CGSize(width: 1440, height: 1320)),
        ]
        for (file, state, size) in cases {
            try DetailRows.render(SpecificationsView(state: state), screen: .specifications, heading: heading, controls: controls,
                                  size: size, file: file)
        }
    }
}

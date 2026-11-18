# Specifications

Source: the canvas "Jojo macOS", page "Product", board "Specifications" (`SpecificationMac.dc.html`, 1440 × 1320, light by default). Read on 2026-11-17 from version `1794942048-8ee3`; the board file is byte-identical to the copy read for the design system at version `1794922102-b518` (sha256 `892261fa…6a56`). Tokens, components and states are named as in [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). The document written for design wave 1 from the first board is in the git history (this file at commit `921322a`).

Built as the third screen of step 4 of the Liquid Glass order (STATUS.md), with protocol 1.9 (DESIGN_ROADMAP_DIFFERENCES.md, E15).

## 1. The board

The content column (padding 10 6 12 10, gap 14):

| Element | Board | Design system |
|---|---|---|
| Header | back and forward capsule; "ADR-031 · Invite token storage" (17 bold) over "Crew › FT-142 Lobby invite links › Specifications"; a segmented "Document / Compare rev 4 → 5"; a capsule group "Approval History", "Affected Tasks" and the appearance switch | header row (section 4.3), segmented control (5.6) |
| Lifecycle card | stepper (Specification current) and the pills "2 running", "3 blocked", "2 awaiting approval" | lifecycle stepper (2.1) on a card |
| Leading column (230 to 260) | "FT-142 documents": rows with a 26 × 26 kind tile ("ADR" indigo, "PRD" blue, red for an invalidated one, gray for a superseded one), the code, a pill (Proposed, Approved, Superseded, Invalidated) and "title · rev n"; "Inside PRD-142 rev 4": a card of counts (acceptance criteria 9, functional requirements 6, non-functional 4, technical constraints 3, risks 3) | list rows (5.3), key-value card |
| Article (radius 18, padding 28 32 32) | pill and `mono` "ADR-031 · rev 5 · sha256 7c1f…e02", a 28 bold title, sections in `label` headings (Context with a green mark of what Q3 decided, Decision with inline code, Alternatives as three cards with + and − lines and "Chosen", Technical constraints C-1 to C-3 with "New", Risks); Compare: "ADR-031 rev 4 → rev 5", "3 sections changed · 1 constraint added", each section as two columns with struck red and green lines | card, inline code |
| Inspector (glass, 280 to 320, sections 18 apart) | "Approval scope" (Subject, Content hash, Authorizes, Does not), "Approve Revision" (primary, full width), "Request Changes", a footnote; "Approval history" with "View all" (rev 4 Approved "You · Nov 13 10:02 · sha256 …"; rev 3 Invalidated and Superseded); "Validation · 2 issues" (Conflict, Missing with a link to T7); "Traceability" with "Affected Tasks" (AC-7 → T6 …) | inspector (4.4), key-value cards |

The sidebar shows a blue "1" beside Specifications.

## 2. Where each value comes from

| Element | Source | Since |
|---|---|---|
| Header title, place | the chosen document's kind and title (`Document`); the held feature's project, code and name (`Features`) | 1.9, 1.7 |
| Lifecycle card | `Features`: stages, `running`, `blocked`, `awaiting` | 1.7 |
| Document list | `Documents`: the held feature's documents (every document when none is held) with kind, revision and approval state; the chosen document's earlier revisions from `Document.revisions` (decision, `superseded`) | 1.6, **1.9** |
| Sidebar counter | `Documents`: the held feature's documents in state `Pending` | 1.6 |
| Inside PRD rev n | `Document.counts` of a requirements document (acceptance criteria, use cases, non-functional requirements, assumptions, open blockers) | **1.9** |
| Article | `Document`: the revision's rendered Markdown, cut into sections at its headings; inline code in `mono` | **1.9** |
| Compare | `Document` of the chosen revision and of the one before it; the app compares each section's lines (longest common subsequence) | **1.9** |
| Approval scope | the revision and its content digest (`Document.revisions`); "Authorizes" and "Does not" are the core's approval rules for the kind, with the documents drafted on it (`Document.downstream`) | **1.9** |
| Approve Revision | `HumanDecision` for the review the screen loads (`ReviewBundle`, shared with the Approval Inbox) | 1.1 |
| Request Changes | opens Feature Chat holding the document's feature, with "Changes requested on PRD rev n: " in the composer; sending it is a `ChatTurn` | 1.2 |
| Approval history | `Document.history`: every decision on the document, its revision, time, digest and whether it still applies | **1.9** |
| Validation | `Document.findings` of a requirements document (`PrdDocument::validate`) | **1.9** |
| Traceability | `Document.criteria`: each acceptance criterion and the use cases that name it; for an ADR or a plan, `Document.upstream` | **1.9** |

### Not built, and why

| Board element | Why |
|---|---|
| Document codes "ADR-031", "PRD-142" | The core numbers no document; the screen writes the kind ("PRD", "ADR", "Plan") and the title or feature |
| Technical constraints, Risks (counts and sections) | The requirements schema has neither; the counts say "Not reported" |
| Functional requirements | Shown as use cases, the schema's nearest section |
| Alternatives cards, "Chosen", the green mark of a decided question, "New" constraint | The ADR is the lead's Markdown; it is shown as written, section by section |
| Tasks per criterion ("AC-7 → T6") | A plan step names no criterion; the note says so |
| Conflict findings of an ADR | The core checks only requirements documents |
| "Invalidated" in the history | An approval that no longer applies is "Superseded" beside its decision |
| Back and forward, appearance switch | No navigation history; the app is light (the human's decision, 2026-11-17) |

## 3. Decisions

Decided by the human on 2026-11-17 (the proposals as given): (1) Approve Revision sends the decision from this screen, through the Approval Inbox's `decide` and its rules; (2) Request Changes opens Feature Chat with a prefilled note to the lead; (3) Traceability lists criteria with their use cases and says that tasks per criterion are not reported; technical constraints and risks are "Not reported". The four queries of protocol 1.9 are derived, without a schema change; the app is light.

## 4. Implementation notes

- `macos/JojoApp/Sources/JojoApp/Specifications.swift`: `SpecificationsState` (plain value: scope, documents, review, detail, comparison, mode), `SpecificationsView`, `SpecificationsScreen` (reads `Features`, `ReviewBundle` and `Document`, and `Document` of the previous revision in Compare, while `visible` and with each new snapshot). The header's segmented control and buttons live in `MainView` (`specificationMode`); "Approval History" opens the Approval Inbox, "Affected Tasks" Execution Plan.
- `macos/JojoApp/Sources/JojoCore/Details.swift`: `DocumentDetail`, `DocumentRevision`, `DocumentDecision`, `DocumentLink`, `DocumentCriterion`, `loadDocument`, `loadComparison` (the text is read page by page up to 64 KiB).
- Choosing a document loads its review; the loaded review is shared with the Approval Inbox and Execution Plan.
- Layout: three columns from 1040 points of content width, two from 760, one below (`FeatureColumns`, COMPONENTS.md, section 28).
- Review aid: `JOJO_SNAPSHOT_DIR=<directory> swift test --filter S6SpecificationsTests` writes `s6-specifications-…`, `-compare-…`, `-findings-…`, `-loading-…`, `-narrow-…`, `-disconnected-…` in dark and light.

## 5. What the app shows today / Known gaps

Specifications shows the held feature's documents with their approval states, the chosen revision's text in sections, a comparison with the revision before it, what an approval covers and never does, the decisions on the document, the findings and acceptance criteria of a requirements document, and the documents a revision was drafted on. The human approves the newest revision here or asks the lead for changes. Known gaps:

- No document numbers; technical constraints, risks and tasks per criterion are not reported.
- An ADR and a plan are shown as the Markdown the core rendered, not as structured alternatives.
- The text is read up to 64 KiB; a longer document says so.
- Reject and Revoke are on the Approval Inbox only.

## 6. Follow-ups (protocol 1.14, 2026-11-18)

"Second opinion" in the inspector, below Approval scope, for a requirements document, architecture decision or execution plan a lead drafted: "Request Review" asks the provider that did not draft it to review the shown revision (`ReviewDocument`); the verdict and notes come from `Document.advisory`. It holds nothing back: Approve Revision is unchanged.

# Jojo screens: Specifications

Transcribed on 2026-11-16 from the canvas board "Specifications" (drawn in dark only; the documents of FT-142, with ADR-031 rev 5 proposed and selected). Tokens, components and states refer to [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). Sections 1 to 4 are the canvas. Section 5 is what the app builds today and why it differs.

## 1. Frame

| Part | Spec |
|---|---|
| Toolbar | Title "Specifications". Breadcrumb "Workspace › Crew › FT-142 Lobby invite links › ADR-031". Secondary buttons at the trailing edge, each with a glyph: "Compare Revisions", "View Approval History" and "View Affected Tasks" (opens Execution Plan) |
| Sidebar counter | "1" on the Specifications item. The board does not say what it counts; one document of the feature is proposed and not yet approved |
| Lifecycle strip | The strip of Agent Runs ([AGENT_RUNS.md](AGENT_RUNS.md), section 1): the compact stepper and the task pills "2 running", "3 blocked", "2 awaiting approval" |
| Columns | Three columns that fill the rest and wrap when narrow: a leading panel (from 230), main (flexible from 460) and a trailing panel (from 330). Both panels: background `#1E1E22`, a 1 pt edge (white at 6%) on the side that faces the main column |

## 2. Leading panel

**Documents.** The section heading "FT-142 documents" (padding 14 top, 16 leading, 14 trailing, 6 bottom), then one row per revision, gap 2, margin 8 at the sides.

| Part | Spec |
|---|---|
| Row that can be opened | Padding 7 by 10, radius 7, two lines, gap 3. Line one: the document at 12.5 semibold in `text` and, trailing, its document tag (COMPONENTS.md, section 2.4). Line two: the title and the revision at 11.5 `text3` ("Invite token storage · rev 5") |
| Earlier revision | The same row with 24 of leading padding, under the newest revision of its document |
| Row that cannot be opened | Padding 6 by 10, gap 2; the document at 12 regular in `text2` |
| Selected | Background `accent` at 16% and an inner ring (`#7AAFFF` at 40%) |

| Document | Tag | Second line | Form |
|---|---|---|---|
| ADR-031 | Proposed | Invite token storage · rev 5 | Can be opened, selected |
| ADR-031 | Approved | Invite token storage · rev 4 | Can be opened, earlier revision |
| ADR-031 | Superseded | Invite token storage · rev 3 | Cannot be opened, earlier revision |
| PRD-142 | Approved | Lobby invite links · rev 4 | Cannot be opened |
| ADR-030 | Invalidated (struck through) | Deep link provider · rev 2 | Cannot be opened |

**Outline.** The section heading "Inside PRD-142 rev 4" (padding 14 top, 16 sides, 6 bottom), then links at 12.5 in `text` (padding 16 at the sides, 14 below, gap 6), each with its count trailing as an identifier at 11 `text3`: "Acceptance criteria" 9, "Functional requirements" 6, "Non-functional requirements" 4, "Technical constraints" 3, "Risks" 3.

## 3. Main column

Padding 16 top, 24 sides, 26 bottom; gap 18.

### 3.1 View control

A row, the two parts pushed apart: the segmented control of Source Control ([SOURCE_CONTROL.md](SOURCE_CONTROL.md), section 2) with the segments "Document" and "Compare rev 4 → 5", and an icon button (28 × 28, radius 6, `text2`) "Export as Markdown".

### 3.2 Document

**Head.** Gap 6. A line (gap 8, wraps) with the document tag and one identifier at 11.5 `text3` ("ADR-031 · rev 5 · sha256 7c1f…e02"), then the title at 22 bold `textStrong` (tracking −0.02 em): "Invite token storage".

**Sections.** One block per section of the document: a heading at 12 semibold `text3`, 6 above its content.

| Section | Content |
|---|---|
| Context, Decision, Risks | A paragraph at 13.5, line height 1.6, at most 700 wide |
| Alternatives | A grid of option cards (from 200 wide, gap 10): radius 10, padding 12, gap 6, background `#232327`, an inner ring (white at 7%). The option at 13 semibold `textStrong`; one advantage in `ok` ("+ No database writes") and one drawback in `danger` ("− Can't revoke early"). The chosen option has a 1 pt ring in `#7AAFFF` and the document tag "Chosen" (the Proposed form) beside its name |
| Technical constraints | Lines, gap 4: an identifier at 11.5 in `info` ("C-1"), gap 8, the sentence |

Revision 4 (tag "Approved", "ADR-031 · rev 4 · sha256 3e9a…c21") has the same sections with its own text and two constraints.

### 3.3 Compare

A line (gap 8): "ADR-031 rev 4 → rev 5" at 12.5 medium in `text`, then "3 sections changed · 1 constraint added" at 11.5 `text3`. Then one block per changed section ("Decision", "Technical constraints", "Context"), each with the section heading of 3.2 over two columns (from 260 wide, gap 10).

| Part | Spec |
|---|---|
| Column | A caption at 11.5 `text3` ("rev 4 · approved", "rev 5 · proposed"), 6 above a well: radius 8, background `#17171A`, vertical padding 6, an inner ring (white at 5%) |
| Line | Padding 2 by 10, 13 with line height 1.55, in `text`. Removed: `danger` on `danger` at 12%. Added: `ok` on `ok` at 12%. An unchanged summary ("C-1, C-2 unchanged") in `text2` |

## 4. Trailing panel

Padding 14, gap 18. Four blocks, each a section heading (8 above its card) over a panel card (radius 10, inner ring).

**Approval scope.** Padding 12 by 14, gap 10. A grid of facts: labels 86 wide in `text3`, values in `text`, both at 12.5, baseline aligned, row gap 7, column gap 10.

| Label | Value |
|---|---|
| Subject | ADR-031 rev 5 |
| Content hash | An identifier at 11.5: "sha256 7c1f…e02" |
| Authorizes | Plan rev 3 may use this decision (T6) |
| Does not | Start tasks, push, deploy, or change rev 4 work |

Then a row of buttons (gap 6, wraps): primary "Approve Revision", which opens the Approval Inbox (tooltip "Opens the ADR approval with its exact hash"), and secondary "Request Changes". Then a glyph and one sentence at 11.5 `text3`: "Any edit after approval invalidates it. Approval never carries over to new content."

**Approval history.** The heading has a quiet button "View all". One line per revision: padding 9 by 14, a 1 pt edge below except after the last, two lines with gap 2. Line one (gap 8): the revision as an identifier at 11.5 in `text`, then its document tags. Line two at 12 `text2`.

| Revision | Tags | Line two |
|---|---|---|
| rev 4 | Approved | You · Nov 13 10:02 · sha256 3e9a…c21 |
| rev 3 | Invalidated (struck through), Superseded | Approved Nov 12 · invalidated Nov 13 09:40: content changed |

**Validation · 2 issues.** One line per finding: padding 10 by 14, a 1 pt edge below except after the last, gap 4. Line one (gap 8): an evidence pill and where the finding is, as an identifier at 11 `text3`. Then the sentence; a task it names is a link in `info`.

| Pill | Where | Sentence |
|---|---|---|
| "Conflict" (the Failed form) | § Alternatives | Option C (Redis) conflicts with NFR-2 "no new infrastructure". Not chosen, but still listed as viable. |
| "Missing" (the Incomplete form) | PRD-142 · AC | No acceptance criterion for what the app shows on an expired link. Affects T7. Related: Q4. |

**Traceability.** The heading has a compact secondary button "View Affected Tasks". One line per requirement: padding 8 by 14, gap 10, a 1 pt edge below except after the last: an identifier at 11.5 in `info`, the requirement (fills the width), and the tasks that implement it as a link in `info`.

| Identifier | Requirement | Tasks |
|---|---|---|
| AC-7 | Expired link returns 410 | T6 |
| AC-8 | Host can revoke any time | T6 |
| C-3 | Read-time expiry | T4, T6 |
| AC-6 | Revoked link shows reason | T7 |

## 5. What the app shows today

Built in `macos/JojoApp/Sources/JojoApp/Specifications.swift` on the shared surfaces of COMPONENTS.md, sections 14 to 19. Before it, this destination showed "Specifications is not built yet".

**The helper reports no document text.** The requests the core knows (`SPECS` in `crates/jojo-domain/src/protocol.rs`) have none that returns the text of a document, its name, its kind, its sections, its earlier revisions, its questions and assumptions or its validation. What exists is the snapshot (`artifact` and `review_document` rows: an identity and the core's record, a digest) and the `ReviewBundle` answer for one document (its revision, the bundle digest, the digests of its content, of what was displayed and of its context, the events of the upstream approvals it binds, and the event of the latest decision on it). The screen shows exactly that. A part of the canvas without a source says so on screen; nothing is filled with an example.

**The loaded document is shared with the Approval Inbox.** The screen has no load or query of its own. Choosing a document calls the workspace's `loadReview`, and the screen shows `WorkspaceModel.review`: the one review the workspace holds. The Approval Inbox shows and decides that same document, so a document chosen here is the loaded one there, and the other way round. The screen takes a plain value (`SpecificationsState`), which wraps the `ApprovalInboxState` of that review; the list of documents and the kind of a document are the Approval Inbox's functions (`ApprovalInboxState.documents(in:)`, `ApprovalInboxState.kind(_:)`), not a second copy of the rule.

**No decision is made here.** Approve, Reject and Revoke are on the Approval Inbox. The canvas agrees: its "Approve Revision" opens the Approval Inbox. The app names the button for what it does, "Open Approval Inbox", and has no "Request Changes" (there is no such command).

### 5.1 Mapping

| Canvas | App today |
|---|---|
| Toolbar buttons, breadcrumb | The shell toolbar has no screen buttons and its breadcrumb stays "Jojo › Specifications". "View Approval History" has no button of its own: the "View all" of "Approval history" opens the same destination (see that line). "Compare Revisions" and "View Affected Tasks" are not built: revisions cannot be compared and tasks do not exist yet |
| Sidebar counter | None. The board's "1" reads as one document that waits, and the helper does not report which documents wait for approval. The number of documents in the snapshot is reported, but as a counter it would read as work that waits; this is the choice the Approval Inbox made ([APPROVAL_INBOX.md](APPROVAL_INBOX.md), section 5.1). A counter comes back when the core reports proposed or pending documents |
| Lifecycle strip and task pills | Not built: features, lifecycle stages and tasks are not reported |
| Heading of the documents | "Documents · n". The feature of a document is not reported |
| Rows | One document row (COMPONENTS.md, section 19) per snapshot row of the kinds `artifact` and `review_document`, in the snapshot's order. Line one: the first twelve characters of the identity, as an identifier; the tag "Stale" on the loaded document when its review is stale, no other tag, because the status of a document is not reported. Line two of the loaded document: its kind and revision from the bundle ("Architecture decision · rev 5"); of any other: the kind of its snapshot row ("Specification" for an artifact, "Decision or plan" for a review document) and "record " with the first twelve characters of the core's record. Every row can be opened; the loaded document's row is the selected one. Without rows: "The snapshot lists no documents." |
| Earlier revisions | Not built: the core reports the current revision of a loaded document only |
| Outline ("Inside PRD-142 rev 4") | Not built: the sections of a document are not reported |
| (not on the canvas) Sentences under the rows | "Choosing a document loads its review. It is the same document the Approval Inbox shows and decides." and "Not reported: the names and statuses of documents, their earlier revisions and the sections inside them. Documents are listed by identity." |
| View control, "Export as Markdown" | Not built. There is no text to export and no earlier revision to compare with; COMPONENTS.md has no segmented control yet |
| Head | Line: the kind from the bundle ("Architecture decision", in `text2`), the decision mark of the Approval Inbox's head (5.2), and one identifier, "sha256 " with the first twelve characters of the content digest. Title: "1a2b3c4d1a2b rev 5" (the first twelve characters of the identity), because the name is not reported. The full values are tooltips |
| Sections of the document | Three document sections (COMPONENTS.md, section 19). "Text": a notice well, "The text of this document is not reported. The core gives its digests, not its sections; a human approves exactly those digests." "Recorded by the core": a panel card of fact lines, "Document" (the full identity), "Revision", "Snapshot record" (the record of its snapshot row, or "Not in the snapshot"), "Bundle", "Content", "Displayed", "Context" (the digests as identifiers, wrapping and selectable). "Compare revisions": a notice well, "Earlier revisions are not reported, so revision 5 cannot be compared with the one before it." |
| Compare | Not built (the notice above) |
| Approval scope | A panel card with the fact grid of COMPONENTS.md, section 15. "Subject": "1a2b3c4d1a2b rev 5". "Content hash": "sha256 " and the first twelve characters. "Authorizes": "Not reported: the plans and tasks that may use this revision.", in `text3`. "Does not": "Start tasks, push or deploy. Approving a document starts nothing." "Decision": the decision mark (5.2) |
| "Approve Revision", "Request Changes" | One compact primary button "Open Approval Inbox" (tooltip "Opens this revision in the Approval Inbox, where it is approved, rejected or revoked"). No decision is sent from this screen |
| Sentence under the buttons | The canvas sentence |
| Approval history | A panel card with one line in the shape of the board: the event as an identifier ("event 41"), under it "The latest decision on this document. What was decided is not reported." Without a decision: "No decision is recorded for this document." Footnote: "Not reported: what was decided, by whom and when, and the decisions on earlier revisions." The heading has the board's "View all" as a compact secondary button in its trailing place. It opens Activity & Audit as it is (decided by the human on 2026-11-16): it sets no search and no type filter, reads nothing itself, and is always enabled, also when no document is loaded. A second footnote, shown with and without a document: ""View all" opens Activity & Audit, the core's event journal: every loaded event, not one document's decisions. A decision is an event of type HumanDecision." (`ApprovalInboxState.historyNote`) |
| Validation | A notice well: "Not reported. The core sends no findings about a document, such as a conflict or a missing acceptance criterion." No count in the heading |
| Traceability | A panel card of trace lines (COMPONENTS.md, section 19), one per upstream approval the bundle binds: the event as an identifier ("event 12") and "Bound upstream approval". Without one: "This revision binds no upstream approval." Footnote: "Not reported: requirements, acceptance criteria and the tasks that implement them." No "View Affected Tasks" |
| Feature Chat links ("Q3", "Q4") | Not built. Feature Chat belongs to milestone M2 |

### 5.2 Kind and decision mark

The kind of the loaded document is read from its bundle by `ApprovalInboxState.kind(_:)` ([APPROVAL_INBOX.md](APPROVAL_INBOX.md), section 5.2): no upstream approval is a specification, one an architecture decision, two or more an execution plan. A document that is not loaded has only the kind of its snapshot row.

The decision mark is the one the Approval Inbox shows in its head, now one view (`ReviewMark`) used by both screens:

| Condition | Mark |
|---|---|
| A decision is on its way to the core | Operational pill "Requested" (the Waiting form) |
| The review is stale | Document tag "Stale" |
| The bundle names no earlier decision | Operational pill "No decision yet" (the Awaiting approval form) |
| The bundle names one | Operational pill "Decision recorded" (neutral). What was decided is not reported, so there is no "Approved" or "Proposed" tag |

### 5.3 Navigation and states

- Empty (no review loaded): the Empty form with "No document loaded" · "Choose a document in the list. What the core records about its current revision is shown here; decisions are made in the Approval Inbox." The panels stay; the trailing panel keeps its headings and says "No document is loaded." in each block.
- Disconnected (`channelLost`, `ended`, `launchFailed`): danger banner "Jojo core isn't responding" · "Showing the last known state, read-only. No document can be loaded." There is no "Reconnect" yet.
- "View all" opens Activity & Audit through the closure that sets the shell's screen (`ApprovalInboxState.historyScreen`, the destination of the Approval Inbox's "Approval History"). Why it is not filtered to the loaded document: [APPROVAL_INBOX.md](APPROVAL_INBOX.md), section 5.4.
- Incompatible protocol: danger banner "Jojo core speaks another protocol version" · "Read-only. No document can be loaded."
- Recovery required (admission reported and not `Open`): danger banner "Execution admission is closed" · "No new attempt starts until you review the inspection. Documents can still be read." with "Open Recovery Center".
- Loading: the workspace does not say that a read is pending; the last loaded review stays until the new one answers.
- Requested and Stale: the decision mark (5.2). A stale review is loaded again by choosing its row.
- Error: a failed or malformed read is reported in the status line under the screen ("Review unavailable: …"); a document the core does not know unloads the review.
- Permission denied and Unknown outcome have no source on this screen: it sends no command.
- Width of the content: three columns from 1030; from 730 the leading panel and the main column, with the blocks of the trailing panel under the sections; below 730 one column, the documents first. The window's opening size gives two columns.

### 5.4 Values that are not on the canvas

| Point | Value in the app |
|---|---|
| Panels | Background `sidebar`, edge `sidebarEdge`; leading 270 wide (the canvas starts at 230; the second line of a row needs the width), trailing 300 (the canvas starts at 330). These are the widths of the Approval Inbox, so the same document sits in the same place on both screens |
| Document row, document section, trace line | COMPONENTS.md, section 19 |
| Fact grid of "Approval scope" | The grid of COMPONENTS.md, section 15: labels 104 wide (the canvas has 86) |
| Title | The Approval Inbox's "identity rev n", at the board's 22 bold |
| Lines of "Approval history" and of an empty "Traceability" | Padding 9 by 14, the sentence at 12 in `text2` |
| Fact that is not reported or not loaded | The words in `text3` |
| Empty state glyph | `doc.text` |

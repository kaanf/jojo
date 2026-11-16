# Jojo screens: Approval Inbox

Transcribed on 2026-11-16 from the canvas board "Approval Inbox" (drawn in dark only; six pending requests, the push request selected). Tokens, components and states refer to [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). Sections 1 to 4 are the canvas. Section 5 is what the app builds today and why it differs.

## 1. Frame

| Part | Spec |
|---|---|
| Toolbar | Title "Approval Inbox". Breadcrumb "Workspace › Approval Inbox". Secondary buttons at the trailing edge, each with a glyph: "Filter" and "Approval History" (opens Activity & Audit) |
| Sidebar counter | "6" on the Approval Inbox item: the pending requests |
| Lifecycle strip | None. The inbox is a workspace destination, not a feature's |
| Columns | Three columns that fill the rest and wrap when narrow: a leading panel (from 270), main (flexible from 460) and a trailing panel (from 280). Both panels: background `#1E1E22`, a 1 pt edge (white at 6%) on the side that faces the main column |

## 2. Leading panel

**Heading and filter.** Padding 12 top, 16 leading, 12 trailing, 8 bottom; gap 8. The section heading "Pending · 6", then the segmented control of Source Control ([SOURCE_CONTROL.md](SOURCE_CONTROL.md), section 2) with the segments "All", "Specs", "Plans", "External", "Security".

**Requests.** One row per request, gap 2. Row: margin 8 at the sides, padding 9 by 10, radius 8, gap 10, aligned to the top. The selected row has the background `accent` at 16% and an inner ring (`#7AAFFF` at 40%).

| Part | Spec |
|---|---|
| Tile | 24 × 24, radius 6, a white glyph by kind. The approval color (`#B5701F`); `#6B6B74` for a stale request |
| Line one | The kind at 11 `text3`; trailing, the age as an identifier at 10.5 `text3` ("18m", "1h", "1d") |
| Line two | The object at 12.5 semibold, one line, truncated |
| Line three | Project and feature, repository or task at 11.5 `text3` ("Crew · FT-142"), gap 6, then a document tag when the request is not current ("Stale") |

| Kind | Object | Where | Age |
|---|---|---|---|
| Architecture decision | ADR-031 rev 5 | Crew · FT-142 | 18m |
| Execution plan | Plan rev 3 | Crew · FT-142 | 18m |
| External action | Push jojo/ft-142-t2-02 @ c44e910 | Crew · crew-app | 1h |
| Baseline change | 3 snapshot baselines · lobby | Crew · crew-app | 2m |
| Scope expansion | Write iosApp/Info.plist | Crew · T5 | 9m |
| Specification | PRD-007 rev 4 | Jojo · FT-007, tag "Stale" | 1d |

Under the rows, padding 14 by 16: a glyph and one sentence at 11.5 `text3`, "Approvals are type-specific. There is no approve-all."

## 3. Main column

The selected request. Padding 18 top, 22 sides, 24 bottom; gap 18.

### 3.1 Head

Gap 6. First a line (gap 8, wraps): a 22 × 22 tile (radius 6, the color of the row's tile), the kind and place at 12 `text2` ("Architecture decision · Crew · FT-142"), and an operational pill "Awaiting approval" (standalone size); a stale request has the document tag "Stale" instead. Then the object at 20 bold `textStrong` (tracking −0.02 em), then one identifier at 11.5 `text3`.

| Kind | Object | Identifier |
|---|---|---|
| Architecture decision | ADR-031 rev 5 | sha256 7c1f…e02 |
| Execution plan | Plan rev 3 | sha256 a40b…19d |
| External action | Push jojo/ft-142-t2-02 @ c44e910 | to origin · github.com/kaanf/crew-app |
| Baseline change | 3 snapshot baselines · lobby | V-3 · CAND-142-07 |
| Scope expansion | Write iosApp/Info.plist | Attempt att_142_t5_01 |
| Specification | PRD-007 rev 4 | sha256 51ee…a02 |

### 3.2 Facts

A panel card (radius 10, inner ring) of fact lines. A line: padding 11 by 14, a 1 pt edge below except after the last; a label 130 wide in `text3` and the value, both at 12.5, baseline aligned, column gap 10; the value wraps.

| Label | Value |
|---|---|
| Requested action | One sentence naming the exact object: "Approve this exact revision of the token storage decision." · "Push this exact commit to this exact remote ref. Does not create a pull request." |
| Why approval | Why a person has to decide: "Architecture decisions bind every task that builds on them. Rev 4 approval does not carry over to changed content." · "Only approved plans can admit tasks. Approving does not start anything." · "Pushing leaves this Mac and cannot be undone by Jojo. Each external action is authorized on its own." |
| Affected | One line per object, gap 8: an identifier at 11.5 in `text` ("crew-api"), then what is touched in `text2` ("Plan rev 3 · T6") |
| Risk and side effects | One sentence: "Adds a nightly job and an expires_at column. T4 keeps running on rev 4 until it settles." |
| Staleness | An evidence pill in the Passed form with the label "Current", then "checked by core 09:42" in `text3`. A stale request: the document tag "Stale", then "content changed after request" |

### 3.3 Supporting evidence

A section heading (8 above its card) over a panel card, padding 10 by 14, gap 8: one link per piece of evidence, a glyph and a title in `text` ("Diff rev 4 → 5", "Validation: 1 conflict, 1 missing AC", "Q3 resolution in Feature Chat", "V-2 passed on c44e910", "Idempotency key op-91c2 reserved").

### 3.4 Actions

A row of buttons that wraps, gap 8: one primary whose label names the kind and ends with an ellipsis (tooltip "Asks for confirmation, then sends the request to the core"), then secondary buttons.

| Kind | Primary | Secondary |
|---|---|---|
| Architecture decision, specification | "Approve Revision…" | "Request Changes", "Compare Revisions" |
| Execution plan | "Approve Plan…" | "Review Plan" |
| External action | "Authorize Push…" | "Decline" |
| Baseline change | "Approve Baselines…" | "Reject", "Compare" |
| Scope expansion | "Allow for This Attempt…" | "Deny" |

**Requested.** After the primary action the buttons are replaced by the notice well of COMPONENTS.md, section 3 (padding 10 by 12 here): a "Requested" pill and "Sent to core · waiting for confirmation. Nothing is shown as approved until the core records it."

**Stale.** Above the buttons, the warning banner of COMPONENTS.md, section 11, without a button: "This request is stale" · "PRD-007 changed after the request was created. Review rev 5 instead; rev 4 can't be approved." The primary button is disabled (tooltip "Unavailable: request is stale. Content changed to rev 5.") and has no ellipsis; the secondary button is "Open rev 5".

## 4. Trailing panel

Padding 14, gap 16. Two blocks, each a section heading (8 above its card) over a panel card of lines: padding 8 by 14, gap 10, a 1 pt edge below except after the last.

**Approval policy.** A name and, trailing, its value in `text2`.

| Name | Value |
|---|---|
| Push and PR | Separate approvals |
| Combined push + PR | Off · no policy |
| Approvals expire | On content change |

**Recently decided.** A document tag, the object, and a date at 11.5 `text3`.

| Tag | Object | Date |
|---|---|---|
| Approved | ADR-031 rev 4 | Nov 13 |
| Invalidated (struck through) | ADR-031 rev 3 | Nov 13 |
| Approved | Plan rev 2 | Nov 13 |

Last, a glyph and one sentence at 11.5 `text3`: "Approving sends a request. The core checks validity again before recording it."

## 5. What the app shows today

Built in `macos/JojoApp/Sources/JojoApp/ApprovalInbox.swift` on the shared surfaces of COMPONENTS.md, sections 14 to 18. It replaces the old Review screen, which ran unchanged inside the shell under this destination, and keeps everything that screen did: loading one document by its identity, Approve, Reject and Revoke, and no decision on a stale review.

The screen has no load or query of its own. It shows the one review the workspace holds (`WorkspaceModel.review`, loaded with `loadReview`, decided with `decide`) and takes a plain value (`ApprovalInboxState`). It shows only what the helper reports:

- the review bundle of the loaded document (`ReviewBundle`): its identity, revision, the bundle digest, the digests of its content, of what was displayed and of its context, the events of the upstream approvals it binds, and the event of the latest decision on it;
- whether that review can still be decided (`reviewStale`), and whether a decision is on its way to the core (`HelperClient.outstandingCommands`);
- the documents the snapshot knows: its `artifact` and `review_document` rows, each an identity and the core's record;
- the loaded attempt result (`Candidate`), read-only;
- the helper connection and the admission value.

A part of the canvas without a source says so on screen; nothing is filled with an example.

**The list is not a list of pending approvals.** The helper does not report which documents wait for approval, nor a request as an object with a kind, an age and a place. The snapshot does list the documents that exist, so the leading panel lists those, under the heading "Documents", and says that their approval state is not reported. Choosing one loads its review; this is the same `loadReview` the identity field calls.

**Four kinds, two built.** Approvals are of four kinds (spec, plan, push, PR) and there is no approve-all. Spec and plan approvals are decisions on a document and are built. Push and pull request approvals belong to milestone M5: the core has no such command and reports no such request, and the screen says so. Baseline changes and scope expansions, which the board also draws, are not approval kinds of the core.

**The candidate's decision stays where it is.** Accept and Reject of a candidate are on Verification & Evidence (its Decision card); the canvas puts them here. The human decided on 2026-11-16 that the Approval Inbox shows the loaded candidate's decision read-only, with a button that opens Verification & Evidence. Nothing was removed there.

### 5.1 Mapping

| Canvas | App today |
|---|---|
| Toolbar buttons, breadcrumb | The shell toolbar has no screen buttons and its breadcrumb stays "Jojo › Approval Inbox". "Approval History" is built in the content, in the heading of "Recently decided" (see that line). "Filter" is not built: there is no filter |
| Sidebar counter | None. The board counts the pending requests, and the helper reports no such number: the snapshot's documents are not all pending, and the count of documents would read as work that waits. A counter comes back when the core reports pending approvals |
| Heading and filter | "Documents · n". No segmented control: the kind of a listed document is not reported, and COMPONENTS.md has no segmented control yet |
| (not on the canvas) Load by identity | Under the heading: an input field "Document identity (32 hex)" and a compact secondary "Load". The field follows the loaded document |
| Requests | One request row (COMPONENTS.md, section 18) per snapshot row of the kinds `artifact` and `review_document`, in the snapshot's order. Tile: neutral, because "pending" is not reported; glyph `doc.text` for an artifact, `doc.on.doc` for a review document. Line one: "Specification" for an artifact, "Decision or plan" for a review document (5.2); no age. Line two: the first twelve characters of the identity, as an identifier. Line three: "record " and the first twelve characters of the core's record; the tag "Stale" on the loaded document when its review is stale (5.4). The loaded document's row is the selected one. Without rows: "The snapshot lists no documents." |
| Sentence under the rows | The canvas sentence, then: "The core does not report which documents wait for approval, nor their names, places or ages. Documents are listed by identity." |
| Head | Tile: the approval color while no decision is recorded, neutral otherwise; the glyph of the kind. Line: the kind and the approval kind ("Architecture decision · Spec approval"). Pill: "No decision yet" (the Awaiting approval form) while the bundle names no earlier decision, "Decision recorded" (neutral) when it does; "Requested" (the Waiting form) while a decision is outstanding; the document tag "Stale" when the review is stale. Object: "1a2b3c4d1a2b rev 5" (the first twelve characters of the identity). Identifier: "bundle " and the first twelve characters of the bundle digest; the full values are tooltips |
| Requested action | "Approve, reject or revoke exactly this revision. A decision binds every digest below; the core refuses it if any of them changed." |
| Why approval | By kind (5.2) |
| Affected, Risk and side effects | Not reported; named in the footnote of the facts |
| (not on the canvas) The bundle | Fact lines "Revision", "Content", "Displayed", "Context" (the digests as identifiers, wrapping), "Upstream approvals" ("events 12, 14", or "None") and "Latest decision" ("event 41 · the action is not reported", or "None recorded") |
| Staleness | Current: the evidence pill "Current" and "as loaded · the core checks again when you decide" (the time of a check is not reported). Requested: the pill "Requested" and "a decision was sent · waiting for the core". Stale: the document tag "Stale" and "a decision was sent or refused · reload before deciding again" |
| Footnote of the facts | "Not reported: the document's name and text, its feature and repository, what it affects, its risks, and the time of the request." |
| Supporting evidence | A notice well: "No supporting evidence is reported. The core gives the digests of the document, not its text, a comparison with the earlier revision or its validation." No links |
| Actions | Primary "Approve Revision" ("Approve Plan" for a plan), destructive "Reject…" and destructive "Revoke Approval…". Each sends `decide` with exactly the loaded bundle. Approve is sent at once, as Accept is on Verification & Evidence; Reject and Revoke ask once in a system confirmation dialog (5.3), because the dialog of COMPONENTS.md, section 7, does not exist yet. "Request Changes", "Compare Revisions" and "Review Plan" are not built: there are no such commands and the document text is not reported. One reason under the buttons when they are disabled (5.3) |
| Requested | The requested notice with the canvas sentence, in place of the reason, while a `HumanDecision` command is outstanding. The buttons stay, disabled |
| Stale | The warning banner "This review is stale" · "A decision was sent or refused for this revision. Reload the document before deciding again; nothing is resent." with a compact secondary "Reload", which loads the same identity again. The core does not say which revision replaced the one shown, so there is no "Open rev 5" |
| Approval policy | A panel card of value lines (COMPONENTS.md, section 18). These are fixed rules of the core, not settings: "Approval kinds" · "Spec, plan, push, PR"; "Approve all" · "None"; "Approvals expire" · "On content change"; "Push" and "Pull request" · "Not available yet". Footnote: "Push and pull request approvals belong to milestone M5. The core has no such command and reports no such request." |
| (not on the canvas) Candidate decision | A panel card: "Accept and reject of a candidate are on Verification & Evidence.", the facts Attempt, Candidate, Validation and Decision of the loaded attempt result (or "No attempt result is loaded."), and a compact secondary "Open Verification & Evidence" |
| Recently decided | The heading "Recently decided" with a compact secondary button "Approval History" in its trailing place, the canvas's toolbar button. It opens Activity & Audit as it is (decided by the human on 2026-11-16): it sets no search and no type filter, reads nothing itself, and is always enabled, with or without a loaded document. The lines of the board are not built: the core reports the event of the latest decision on the loaded document, not a history. Two footnotes under the heading: "Decisions already made are not listed: the core reports only the event of the latest decision on the loaded document." and ""Approval History" opens Activity & Audit, the core's event journal: every loaded event, not one document's decisions. A decision is an event of type HumanDecision." The button keeps the canvas's name; the second footnote says that the journal is not a decision history (`ApprovalInboxState.historyNote`) |
| Last sentence | The canvas sentence, after "Recently decided" |

### 5.2 Kind of a document

The helper sends no word for the kind of a document. The bundle shows it: a specification artifact binds no upstream approval, an architecture decision binds the approval of its PRD, and a plan binds the approvals of its PRD and of its architecture decision. `ApprovalInboxState.kind(_:)` reads the number of upstream approvals; the rule exists once.

| Upstream approvals | Kind | Approval kind | Glyph | "Why approval" |
|---|---|---|---|---|
| None | Specification | Spec approval | `doc.text` | "Requirements bind planning for the feature. An approval does not carry over to changed content." |
| One | Architecture decision | Spec approval | `building.columns` | "Architecture decisions bind every plan that builds on them. An approval does not carry over to changed content." |
| Two or more | Execution plan | Plan approval | `list.bullet.indent` | "Only approved plans can admit tasks. Approving does not start anything." |

A document in the list is not loaded, so its row has only the kind of its snapshot row: `artifact` is "Specification", `review_document` is "Decision or plan".

### 5.3 Actions and confirmation

| Condition | Reason under the buttons |
|---|---|
| The core is not connected | "Unavailable: the core is not connected." |
| A decision is outstanding | The requested notice |
| The review is stale | "Unavailable: this review is stale. Reload the document." |
| No document is loaded | The buttons are not drawn |

The three buttons are enabled or disabled together, as on the old screen: which decision the core accepts for a document is the core's rule, and a refusal is reported in the status line under the screen and makes the review stale.

| Action | Dialog title | Message | Buttons |
|---|---|---|---|
| Reject | "Reject this revision?" | "Records your decision to reject revision 5 of document 1a2b3c4d1a2b. Work that needs its approval does not start. A recorded decision is not removed from this app." | Destructive "Reject Revision", cancel "Keep Undecided" |
| Revoke | "Revoke the approval of this revision?" | "Records that revision 5 of document 1a2b3c4d1a2b is no longer approved. The core may stop attempts that depend on it. A recorded decision is not removed from this app." | Destructive "Revoke Approval", cancel "Keep Approval" |

### 5.4 Navigation and states

- Empty (no review loaded): the Empty form with "No document loaded" · "Choose a document in the list, or load one by its identity. Its review bundle is shown here, and you approve, reject or revoke exactly that revision." The panels stay.
- Disconnected (`channelLost`, `ended`, `launchFailed`): danger banner "Jojo core isn't responding" · "Showing the last known state, read-only. No actions can be requested." There is no "Reconnect" yet.
- Incompatible protocol: danger banner "Jojo core speaks another protocol version" · "Read-only. No actions can be requested."
- Recovery required (admission reported and not `Open`): danger banner "Execution admission is closed" · "No new attempt starts until you review the inspection. Documents can still be decided." with "Open Recovery Center".
- "Approval History" opens Activity & Audit through the closure that sets the shell's screen (`ApprovalInboxState.historyScreen`). Not filtered to the loaded document, and why: a decision is the event `HumanDecision`, which names no aggregate and carries the document's identity inside its payload in another notation than the one the app shows, so a search for the identity would find the document's other events and miss its decisions. Activity & Audit covers the loaded events only (the newest 64, more on "Load Older"); it says so itself.
- Loading: the workspace does not say that a read is pending; the last loaded review stays until the new one answers.
- Requested and Stale: 5.1. The workspace marks a review stale from the moment a decision is sent until the document is loaded again, whatever the core answered. The screen shows that as Requested while the `HumanDecision` command is outstanding and as Stale once it is answered.
- Unknown outcome: a decision the core did not answer stays outstanding (Requested) and is reconciled by receipt, never resent; the status line under the screen says so.
- Error: a failed or malformed read is reported in the status line ("Review unavailable: …", "No reviewable artifact …"); a document the core does not know unloads the review.
- Permission denied has no source on this screen.
- Width of the content: three columns from 1030; from 730 the leading panel and the main column, with the blocks of the trailing panel under the actions; below 730 one column, the documents first. The window's opening size gives two columns.

### 5.5 Values that are not on the canvas

| Point | Value in the app |
|---|---|
| Panels | Background `sidebar`, edge `sidebarEdge`; leading 270 wide, trailing 300 (the canvas has 280; the policy lines need the width) |
| Request row, fact line, value line | COMPONENTS.md, section 18 |
| Tile of a document without a known approval state | `tileNeutral` (the canvas has the approval color for pending and `#6B6B74` for stale) |
| Selected row ring | `info` at 40%, as in Agent Runs |
| Digests in the facts | Identifiers in `text`, wrapping and selectable |
| Fact that is not reported or not loaded | The words in `text3` |
| Empty state glyph | `tray` |

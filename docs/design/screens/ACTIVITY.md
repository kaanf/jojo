# Jojo screens: Activity & Audit

Transcribed on 2026-11-16 from the canvas board "Activity & Audit" (`Activity.dc.html`, drawn in dark only; thirteen records of two days, with the record of 09:24 selected). Tokens, components and states refer to [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). Sections 1 to 4 are the canvas. Section 5 is what the app builds today and why it differs.

## 1. Frame

| Part | Spec |
|---|---|
| Toolbar | Title "Activity & Audit". Breadcrumb "Workspace › Activity & Audit". One secondary button at the trailing edge, with a glyph: "Export" (tooltip "Export filtered records as JSON Lines") |
| Sidebar counter | None. The item sits in the "System" group, above "Settings & Providers" |
| Columns | Two columns that fill the rest and wrap when narrow: main (flexible from 460) and a trailing panel (from 320). The panel: background `#1E1E22`, a 1 pt edge (white at 6%) on its leading side |

## 2. Main column

Padding 14 top, 18 sides, 20 bottom; gap 12.

### 2.1 Search

A field 32 high: padding 10 at the sides, radius 8, background `#232327`, an inner ring (white at 8%), gap 8. A magnifier glyph in `text3`, the input at 13 in `text` (placeholder "Search decisions, attempts, commits, record IDs…" in `text3`), and the key cap "⌘F" (COMPONENTS.md, section 8).

### 2.2 Filters

One row that wraps, gap 6: the type chips, a flexible space, and two segmented controls.

| Part | Spec |
|---|---|
| Type chip | A capsule 24 high, padding 10 at the sides, 12 regular. Pressed: background `accent` at 16%, text `#7AAFFF`. The board draws all eight pressed and no chip that is off |
| Chips | "Decision", "Approval", "Attempt", "Recovery", "Git", "Validation", "Error", "Evidence" |
| Segmented control | The control of Source Control ([SOURCE_CONTROL.md](SOURCE_CONTROL.md), section 2): a group with padding 2, radius 7, background white at 7%; a segment 22 high, padding 10 at the sides, radius 5, 12 medium in `text2`; the selected one on `#55555C` in white with a shadow |
| Project | "All projects" (selected), "Crew", "Jojo" |
| Range | "24h" (selected), "7d", "All" |

### 2.3 Record table

One card: background `#232327`, radius 10, an inner ring (white at 5%). Six columns, gap 10: 52, 92, 86, the rest, 86, 80.

| Part | Spec |
|---|---|
| Captions | "Time", "Actor", "Type", "Event", "Subject", "Record" at 11 semibold `text3`; padding 8 by 14; a 1 pt edge below (white at 8%) |
| Group caption | A day ("Today", "Yesterday") at 11 semibold `text3`; padding 10 top, 14 sides, 6 bottom |
| Row | Padding 8 by 14, items centered, a 1 pt edge below (white at 5%) |
| Time | An identifier at 11.5 in `text3` ("09:42") |
| Actor | 12 in `text2`, one line: "Core", "Validator", "Claude Code", "Codex", "You" |
| Type | A tag: 11 semibold, padding 6 at the sides, line height 18, radius 4, background white at 5%. Its color follows the type: "Approval" and "Evidence" `ok`, "Recovery" `warn`, "Error" `danger`, "Decision" `info`, "Attempt", "Validation" and "Git" `text2` |
| Event | One sentence at 13 in `text`, one line, truncated ("Approval request a3 checked: still current") |
| Subject | A link to the screen of the subject: an identifier at 11.5 in `#7AAFFF`, one line ("Push c44e910" opens the Approval Inbox, "V-3" Verification & Evidence, "J2 #1" the Recovery Center, "T4 #2" Agent Runs) |
| Record | An identifier at 11 in `text3` ("rec_8812") |
| Selected row | Background `accent` at 16% and an inner ring (`#7AAFFF` at 40%) in place of the edge |

Under the card, one sentence at 11.5 `text3`: "Records are append-only and hash-chained. Older entries load as you scroll."

## 3. Trailing panel

Three parts: a head and the inspector sections of COMPONENTS.md, section 9 (heading at 11.5 semibold `text3`, 8 above the content, padding 12 by 16, a 1 pt edge below except after the last).

**Head.** Padding 14 top, 16 sides, 12 bottom, gap 6, an edge below. "Audit record · Approval" at 11 medium `text3`; the title at 15 semibold `textStrong` (tracking −0.01 em): "ADR-031 rev 3 approval invalidated"; then a row of tags, gap 6: the type tag ("Approval") and a document tag ("Invalidated", struck through).

**Record.** The fact grid of the inspector: labels 104 wide in `text3`, values in `text`, both at 12.5, baseline aligned, row gap 7, column gap 10.

| Label | Value |
|---|---|
| Record ID | An identifier at 11.5: "rec_8771" |
| Time | Today 09:24:11 |
| Actor | Jojo core |
| Cause | ADR-031 content changed (rev 3 → rev 4) |
| Approval | Given Nov 12 by you · sha256 b8d0…77e |

**Integrity.** The same grid.

| Label | Value |
|---|---|
| Record hash | An identifier: "sha256 0c4a…9e1" |
| Previous | An identifier: "rec_8770 · 51b2…0af" |
| Chain | An evidence pill "Verified" (the Passed form) |

**Go to.** A row of compact secondary buttons, gap 6, wraps: "ADR-031" (opens Specifications), "Affected tasks" (opens Execution Plan), "Approval history" (opens the Approval Inbox).

## 4. What the board does not draw

A chip that is off, an empty table, a search without a result, a failed or pending read, and the light appearance.

## 5. What the app shows today

Built in `macos/JojoApp/Sources/JojoApp/Activity.swift` on the shared surfaces of COMPONENTS.md, sections 14 to 20. Before it, this destination showed "Activity & Audit is not built yet".

**What the helper reports about an event.** The `Events` query (`after`, `limit`; at most 8 rows a page, in sequence order) answers with a list. One row has: `seq` (the sequence number), `epoch` (the engine epoch that committed it), `type`, `aggregate_kind` and `aggregate_id` (the aggregate it changed, or null), `revision` (of that aggregate, or null), `payload` (its first 512 characters) and `detail_complete` (false when the payload is longer). That is all (`ipc_events` in `crates/jojo-storage/src/ipc_queries.rs`). The answer has **no time**, **no actor**, **no category** above the type, **no sentence** that says what happened, and **no hash**. The screen shows exactly these fields. A part of the canvas without a source says so on screen; nothing is filled with an example.

**No hash chain is verified.** The board says "hash-chained" and shows a record hash, the previous record's hash and "Chain: Verified". The helper reports no hash for an event and the app computes none, so the app verifies nothing. The screen says "Not reported" for both hashes and shows the evidence pill "Not verified" (the Not run form), never "Verified". [DESIGN_ROADMAP_DIFFERENCES.md](../../roadmap/DESIGN_ROADMAP_DIFFERENCES.md), section 4, names this screen "Existing event journal (hash chain not verified)".

**The workspace reads the events when this screen asks** (decided by the human on 2026-11-16). Before, `WorkspaceModel` kept only the sequence number of its snapshot and asked `Events` with limit 1 to learn whether anything was committed (`pollEvents`); that poll is unchanged and does not fill the list. New, in Swift only and with the existing query: `WorkspaceModel.loadEvents()` reads the newest 64 events up to the snapshot's event the first time, and afterwards only the events committed since; `loadOlderEvents()` reads the 64 before the oldest held one. Both read pages of 8 and add the events only when the whole read has answered, so the held list (`WorkspaceModel.events`, oldest first) has no gap. `WorkspaceModel.eventsLoad` says where a read stands (`idle`, `loading`, `loaded`, `failed`). The screen asks when it becomes the shown destination and when the snapshot's event changes while it is shown; the events are not read while another screen is shown. The screen takes a plain value (`ActivityState`). The protocol and the Rust side are unchanged; `EventDetail` (the rest of a long payload) is not in the app's request list and is not sent.

**Search and type filters work on the loaded events only** (decided by the human on 2026-11-16). The core has no search and no filter. The screen filters the events it holds and says so.

### 5.1 Mapping

| Canvas | App today |
|---|---|
| Toolbar button "Export", breadcrumb | Not built. The shell breadcrumb stays "Jojo › System › Activity & Audit". There is no export command, and the app writes no file |
| Sidebar counter | None, as on the board. No number of the journal reads as work that waits |
| Search | The search field (COMPONENTS.md, section 20) with the prompt "Search the loaded events: type, payload, subject, event number". It matches, without case, the type, the payload excerpt, the aggregate kind, the aggregate identity and "event n". No "⌘F": the shortcut is not built |
| Type chips | One filter chip (COMPONENTS.md, section 20) per type that occurs in the loaded events, by name, with how many loaded events have it ("ArtifactApproved 2"). All are on at first; a chip that is off hides its events. The board's eight categories are not reported, so the chips carry the core's type names |
| Project and range controls | Not built: an event has no project and no time. COMPONENTS.md still has no segmented control |
| Columns | Four: "Type" (150), "Event" (the rest), "Subject" (92), "Record" (64). "Time" and "Actor" are not reported and have no column |
| Group caption | "Epoch n": the events of one engine epoch, newest epoch first. The core reports no time to group by day |
| Type | The kind tag (COMPONENTS.md, section 20) with the core's type name, in `text2`, truncated in the middle. No color by category: none is reported |
| Event | The payload excerpt as the core stored it, monospaced, one line ("decision=APPROVE"); "(empty payload)" when there is none. The core reports no sentence |
| Subject | The first twelve characters of the aggregate identity, as an identifier in `text2`; "—" in `text4` when the event names none. Not a link (see "Go to") |
| Record | "event n", the sequence number |
| Selected row | The board's form. Without a choice, or when the chosen event is filtered out, the newest shown event is the chosen one |
| Sentence under the card | A line: "Events 60 to 68 are loaded, newest first." (with filters: "…; 6 of 9 shown."), and trailing a compact secondary button "Load Older", or "The journal starts here." when event 1 is loaded. Then a footnote: "Events are append-only in the core. It reports no hash for them, so the app verifies no chain. Not reported: the time and the actor of an event; events are grouped by the engine epoch that committed them. There is no export and no filter by project or date." Older events load on the button, not on scroll |
| Panel head | "Audit record · event 68", the type as the title ("ArtifactApproved"), and two kind tags: "Epoch 7" and the aggregate kind when there is one. No document tag: what a record did to a document's status is not reported |
| Record | The fact grid: "Record ID" ("event 68"), "Time" and "Actor" ("Not reported", in `text3`), "Epoch", "Subject" (the aggregate kind and the full identity as an identifier, or "This event names none"), "Revision" (or "None"). No "Cause" and no "Approval": not reported |
| (not on the canvas) Payload | A section "Payload": the excerpt, monospaced at 11.5, selectable, in a well (radius 8, `fill2`). When `detail_complete` is false, a footnote: "The first 512 characters. The app has no request for the rest of a payload yet." |
| Integrity | "Record hash" and "Previous": "Not reported". "Chain": the evidence pill "Not verified" (the Not run form) |
| Go to | When the subject is a document the snapshot lists (`ApprovalInboxState.documents(in:)`): a compact secondary button "Open in Specifications", which loads that document's review (`loadReview`, the call Specifications makes) and opens Specifications. Otherwise one sentence in `text3`: "No screen opens this subject yet." or "This event names no subject." No "Affected tasks" (tasks do not exist) and no "Approval history" (see 5.3) |

### 5.2 States

- Loading (nothing held, a read pending): "Reading the event journal…". Before the first snapshot: "Waiting for the core's snapshot…". With events held, "Reading…" beside "Load Older", which is disabled.
- Empty (a read answered, nothing held): the Empty form with "No events yet" · "The core has committed no event after the one this read started from."
- No match: the Empty form with "No loaded event matches" · "Search and the type filters cover the n loaded events only, not the whole journal." and "Clear Filters".
- Error: a warning banner "The event journal could not be read" · "The core answered: …." (with events held: "… The events read before stay.") and "Try Again". Without events, the Empty form "No events loaded" · "The journal was not read. Nothing is shown in its place."
- Disconnected (`channelLost`, `ended`, `launchFailed`): danger banner "Jojo core isn't responding" · "Showing the events read before, read-only. No more can be read." "Load Older" is disabled. There is no "Reconnect" yet.
- Incompatible protocol: danger banner "Jojo core speaks another protocol version" · "Read-only. Events can still be read."
- Recovery required (admission reported and not `Open`): danger banner "Execution admission is closed" · "No new attempt starts until you review the inspection. Events can still be read." with "Open Recovery Center".
- Requested, Stale, Permission denied and Unknown outcome have no source on this screen: it sends no command.
- Width of the content: two columns from 780; below, one column with the record under the table in a card. The window's opening size gives two columns.

### 5.3 Links from other screens

The Approval Inbox ("Approval History") and Specifications ("View Approval History", "View all") did not build their links because this screen did not exist ([APPROVAL_INBOX.md](APPROVAL_INBOX.md), section 5.1; [SPECIFICATIONS.md](SPECIFICATIONS.md), section 5.1). They are built now (2026-11-16): "Approval History" in the heading of "Recently decided" on the Approval Inbox, and "View all" in the heading of "Approval history" on Specifications, which also stands for the board's toolbar button "View Approval History". Both are compact secondary buttons in the content, because the shell toolbar has no screen buttons, and both are always enabled.

**The link only opens this destination** (decided by the human on 2026-11-16). It sets the shell's screen; this screen then reads as it does when chosen in the sidebar (`visible`). The search, the type filters and the chosen event stay as they were: they are the state of `ActivityScreen` and nothing outside sets them. A link filtered to the loaded document was the alternative and was not built, for two reasons. A decision is the event `HumanDecision`: it names no aggregate, and its payload writes the document's identity as a decimal number inside the core's debug text, while the app shows 32 hexadecimal characters; a search for the identity finds the document's other events (`ReviewDocumentCreated`) and misses its decisions. And the search covers the loaded events only, so older decisions are found only after "Load Older". Both screens say beside the button what it opens: "… opens Activity & Audit, the core's event journal: every loaded event, not one document's decisions. A decision is an event of type HumanDecision." This screen was not changed.

### 5.4 Values that are not on the canvas

| Point | Value in the app |
|---|---|
| Panel | Background `sidebar`, edge `sidebarEdge`, 300 wide (the canvas starts at 320), the width of the trailing panels of the Approval Inbox and Specifications |
| Columns | 150, the rest, 92, 64 (the canvas has 52, 92, 86, the rest, 86, 80 for six) |
| Chip that is off, chip count, search field, kind tag, record row | COMPONENTS.md, section 20 |
| Number of events a read reaches back | 64, in pages of 8 (the helper's largest page); one read stops after 256 events |
| Empty state glyphs | `clock.arrow.circlepath`, and `magnifyingglass` for a search without a result |

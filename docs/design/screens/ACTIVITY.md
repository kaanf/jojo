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

Rebuilt on 2026-11-18 from `ActivityMac.dc.html` (step 6) in `macos/JojoApp/Sources/JojoApp/Activity.swift`. The workspace reads the newest 64 events with `Events` at protocol 1.11 while the screen is shown (and the Dashboard at launch), 64 older ones on "Load Older"; at 1.11 each event carries its time, actor, sentence, category and project.

### 5.1 Mapping

| Board | App |
|---|---|
| Header "Export" | Writes the shown events as Markdown through the save panel (time, actor, category, sentence, subject, record; the filters in force; "no hash chain was verified") |
| Search, "All types", "All projects", 24h / 7d / All | A search over the loaded events (type, sentence, category, project, actor, payload, subject, record); a category menu (Decision, Approval, Attempt, Recovery, Git, Validation, Error, Evidence and the core's other categories present); a project menu of the projects the loaded events concern; the range control. All filter the loaded events only; an event without a time is outside every range but All |
| Table: Time, Actor, Type, Event, Subject, Record, grouped by day | The same: "18:15", "You" / "Jojo core" / the provider, the category tag in its tone, the core's sentence, the aggregate's first twelve characters, "event n" |
| "Records are append-only and hash-chained. Older entries load as you scroll." | "Records are append-only. The core stores no record hash, so no chain is verified." and "Load Older" |
| Record panel: title, category, Record ID, Time, Actor, Cause, Approval | Title (sentence), category tag and type, Record ID, time to the second, actor, project, subject, session; Cause "Not reported" |
| Integrity: record hash, previous, "Chain Verified" | "Not reported", "Not reported", **"Not verified"** |
| (not on the board) | Payload: the 512-character excerpt, and "Load Full Payload" for a longer one (`EventDetail`, pages of 2,048 characters, at most 64 KiB) |
| Go to: document, affected tasks, approval history | Open Document (a document the snapshot lists), Approval Inbox (approval and decision events), Recovery Center (recovery events); Affected Tasks disabled |

### 5.2 States

Reading, no events yet, a lost or incompatible core, a failed read (with the events read before), admission closed (a banner with Open Recovery Center), a core older than 1.11 (a banner: the type stands in for the category and the project menu is empty), no match ("Clear Filters").

### 5.3 Known gaps

- **No hash chain is verified**: the core stores no record hash.
- Cause, approval and affected tasks of an event are not reported.
- The filters cover the loaded events only; there is no server-side filter or loading on scroll.
- The category is derived from the event type by the core (`event_category`), the project from the first identity the event records that leads to a registered repository; an event naming none has no project.
- Export writes the shown events without their payloads.

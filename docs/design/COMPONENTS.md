# Jojo design system: components

The reusable parts of the app and the status model they draw. Token names refer to [DESIGN_SYSTEM.md](DESIGN_SYSTEM.md). Source: canvas boards "Components and status model", "Themes and tokens" and "Dashboard"; see [README.md](README.md).

## 1. Status model

Three separate families, plus document status. They share colors and never share a shape, so they can sit side by side without being read as one progress value. A feature can be in Execution while its tasks are Running and Blocked and its evidence is Incomplete.

| Family | Answers | Shape |
|---|---|---|
| Feature lifecycle | Where the feature is | Stepper |
| Operational state | What is happening now | Capsule pill with a dot glyph |
| Evidence and verification | What is proven | Square pill (radius 4) with a symbol glyph |
| Document status | What is binding | Uppercase tag (radius 4) |

**Unknown is never drawn as success or failure.** It has its own dashed outline and a question mark.

## 2. Status components

### 2.1 Feature lifecycle stepper

Stages, in order: Discovery, Specification, Planning, Execution, Verification, Evidence.

| Stage state | Marker (14 × 14 circle) | Label | Connector after it (16 × 1) |
|---|---|---|---|
| Done | `ok` at 16% fill with a 9 pt check | 12, medium, `text2` | `ok` at 40% |
| Current | 1.5 pt `info` ring with a 6 pt `info` dot | 12, semibold, `textStrong` | white at 12% (dark) |
| Upcoming | 1.5 pt ring, white at 20% (dark) | 12, medium, `text3` | white at 12% (dark) |

### 2.2 Operational pill

Capsule, padding 7 leading and 8 trailing, gap 6, text 11 medium, height 18. Where the pill stands alone (the status legend, a state notice) it is 11.5 medium and 20 high.

| State | Text | Background | Glyph |
|---|---|---|---|
| Idle | `text2` | `fill` | 7 pt dot, filled |
| Running | `info` | `infoTint` | 7 pt dot with a 3 pt halo of `infoTint` |
| Waiting | `text2` | `fill` | 7 pt ring, 1.5 pt, not filled |
| Blocked | `warn` | `warnTint` | 9 pt symbol |
| Awaiting approval | `warn` | `warnTint` | 7 pt dot, filled |
| Paused | `text2` | `fill` | 8 pt pause symbol |
| Recovery required | `danger` | `dangerTint` | 9 pt ring, 1.5 pt, with a 3 pt dot inside |
| Failed | `danger` | `dangerTint` | 7 pt dot, filled |
| Completed | `ok` | `okTint` | 7 pt dot, filled |

"Held" (Recovery Center rows) and "Requested" use the Recovery required and Waiting forms with their own label. "Temporarily unavailable" uses the Blocked form.

### 2.3 Evidence pill

Radius 4, padding 6 leading and 7 trailing, gap 5, text 11.5 medium, height 18, 10 pt glyph.

| State | Text | Background | Other |
|---|---|---|---|
| Passed | `ok` | `okTint` | |
| Sealed | `ok` | `okTint` | |
| Failed | `danger` | `dangerTint` | |
| Incomplete | `warn` | `warnTint` | |
| Needs approval | `warn` | `warnTint` | |
| Not run | `text2` | `fill` | |
| Skipped | `text2` | `fill` | |
| Invalidated | `text2` | `fill` | Label struck through |
| Unknown | `text2` | none | 1 pt dashed outline (white at 30% in dark); glyph is "?" at 10 pt bold |

### 2.4 Document tag

Radius 4, horizontal padding 7, text 11 semibold uppercase with +0.02 em tracking, height 17 plus a 1 pt border.

| Status | Text | Background | Border |
|---|---|---|---|
| Proposed | `info` | none | `infoRing` |
| Approved | `ok` | `okTint` | none |
| Superseded | `text2` | `fill` | none |
| Invalidated | `text2` | `fill` | none; label struck through |
| Stale | `warn` | none | `warnRing` |

A revision reference ("PRD rev 4") uses the Approved form.

### 2.5 Counter

Capsule, horizontal padding 6, text 11 semibold, height 17, background `fill`. The text color carries the meaning: `warn` for items that need the human, `info` for running, `danger` for failures, `text3` for a plain count.

## 3. Buttons

Height 28, horizontal padding 11, radius 6, text 12.5 medium, gap 6, optional 14 pt leading glyph. Compact size (inspector, empty state): height 26, horizontal padding 10.

| Kind | Background | Text | Border |
|---|---|---|---|
| Primary | `accent` | `onAccent` | 1 pt black at 12% |
| Secondary | `fill` | `text` | 1 pt `hairline` |
| Ghost | none; `fill` under the pointer | `text2` | none |
| Destructive | `dangerTint` | `danger` | 1 pt `dangerRing` |

Rules:

- **One primary action per area.**
- **Destructive actions are red and always confirm.** Their label ends with an ellipsis when a dialog follows ("Cancel Attempt…").
- A keyboard shortcut is shown inside the button at 11 pt and 70–75% opacity ("⌘↵", "⌘N", "esc").
- **A disabled control always says why.** Opacity 0.42, a tooltip, and an inline reason where space allows: a 12 pt info glyph and 11.5 pt `text3` text ("Unavailable: 5 requirements unmet.").
- **Unavailable and temporarily disabled are different.** A capability that does not exist yet: disabled button with the text "Not available yet" beside it. A capability that is off for now: disabled button with a "Temporarily unavailable" pill.
- **Requested is not done.** After a command is sent and before the core confirms it, show a notice well (radius 8, padding 8 by 12, white at 5% in dark) with a "Requested" pill and the text "Sent to core · waiting for confirmation". Never show success before the core records it.

## 4. App shell

### 4.1 Sidebar

236 wide, system sidebar material, `sidebarEdge` on the right, padding 12 by 10, 1 pt between items.

| Part | Spec |
|---|---|
| Search field | Height 28, radius 7, background `fill`, 13 pt glyph, placeholder "Search or run" at 12.5 `text3`, key cap "⌘K" at the trailing edge. Opens the command palette |
| Item | Height 28, radius 6, padding 9 leading and 8 trailing, gap 9, 15 pt glyph in `icon`, label 13 medium `navText`, optional counter at the trailing edge |
| Selected item | Background `accent`, label and glyph `onAccent` |
| Item under the pointer | Background `hover` |
| Feature switcher | Radius 8, background `fill`, padding 7 by 8, 14 above and 4 below. A 24 pt avatar tile (radius 6, white 12 pt bold initial), project name at 11 `text3`, feature name at 12.5 semibold |
| Section label | 11 semibold `text3`, padding 14 top, 9 sides, 4 bottom |
| Status well, at the bottom | Radius 9, background `fill2`, padding 10, gap 8. First line: 7 pt status dot, "Admission open" or "Admission closed" at 12, session identifier in `mono` 11 `text3`. Second line: provider availability at 12 `text2` |

Order of items in the canvas: Dashboard, Projects, Approval Inbox, Recovery Center; then the feature switcher and Feature Chat, Specifications, Execution Plan, Agent Runs, Source Control, Verification & Evidence; then the section "System" with Activity & Audit and Settings & Providers.

### 4.2 Toolbar

Minimum height 52, padding 8 by 20, background `toolbar`, `toolbarEdge` below.

- Leading: screen title in `toolbarTitle`; under it a breadcrumb at 11.5 with ancestors in `text3`, "›" separators in `text4` and the current place in `text2`.
- Trailing: icon buttons (28 × 28, radius 6, 16 pt glyph, `text2`, `fill` under the pointer), then at most one primary button.

### 4.3 Content

Background `content`, padding 20 (28 at the bottom), 22 between sections. A section is a heading row (minimum height 26: `sectionTitle`, optional counter, optional trailing ghost link of height 26 with a chevron) and, 10 below it, a card.

## 5. Cards and lists

### 5.1 Card

Background `surface`, radius 12, `cardShadow`, padding 14 by 16, content clipped to the radius.

### 5.2 KPI tile

A clickable card (`liftShadow` under the pointer), minimum width 200, gap 6:

1. Label row: 8 pt status dot (Running adds the halo), label at 12 medium `text2`, trailing 12 pt chevron.
2. Value row, baseline aligned, gap 6: number in `kpiNumber` `textStrong`, unit at 13 medium `text3`.
3. Caption at 12 `text3`, one line.

### 5.3 List and row

A list is a card with no padding. Rows are separated by a 1 pt `hairline`; the last row has none.

| Part | Spec |
|---|---|
| Group header | Padding 9 top, 7 bottom, 16 sides; background `fill2`; `hairline` below; label in `groupHeader` `text3`; optional counter |
| Row | Padding 11 by 16, gap 12, vertically centered, `hover` under the pointer |
| Leading tile | 26 × 26, radius 7, fixed color by kind, white 14 pt glyph |
| Text column | Gap 1. Line one, baseline aligned with gap 8: title at 13 semibold `textStrong`, meta at 11.5 `text3`. Line two at 12 `text2`. Each line is cut with an ellipsis |
| Trailing | One status component (pill or tag), then a 12 pt chevron in `text4` |

### 5.4 Pipeline cells

A grid of equal columns with a gap of 6. Each cell: height 32, radius 7, centered, text 11.5 semibold, gap 5.

| Cell state | Treatment |
|---|---|
| Done | `okTint` background, `ok` 13 pt check, no visible label (the label "Done" is for assistive technology) |
| Sealed | `okTint` background, `ok` text with a 12 pt glyph |
| Needs the human | `warnTint` background, `warn` text, 1 pt `warnRing` |
| Running | `infoTint` background, `info` text with a 6 pt dot, 1 pt `infoRing` |
| Not reached | no background, 1 pt `ringEmpty` |

## 6. Dependency nodes

Task nodes are solid; gates are dashed.

**Task node.** 196 × 84, radius 10, background `surface`, padding 9 by 11, gap 4. Line one: task identifier in `mono` 11.5 medium on the left, operational or evidence pill on the right. Line two: title at 12.5 semibold `textStrong`. Line three: provider or blocker count at 11.5 `text3`.

| Node state | Outline (1 pt, inside) |
|---|---|
| Default | white at 7% (dark) |
| Running | `info` at 60% |
| Blocked | `warn` at 45% |

A task that may start carries the tag "Eligible": capsule, `accent` background, white 10.5 semibold text, height 16, horizontal padding 6, placed over the top right corner (9 above the top edge).

**Gate node.** 196 × 76, radius 10, 1 pt dashed outline (white at 32% in dark), background slightly darker than `surface` (`#1E1E22` in dark), padding 9 by 11, gap 4. Line one: 12 pt glyph and the gate kind at 11 semibold ("Approval gate" in `warn`; "Deployment gate" and "Verification gate" in `text2`). Line two: what the gate waits for. Line three: its progress ("0 of 2 approved", "Not run").

## 7. Dialogs

Only for consequential actions. **Each dialog names the exact object and says what the action does not do. There is no generic approve.**

Layout: 420 to 460 wide, radius 12, padding 18, 14 between blocks.

1. Header: 32 × 32 icon tile (radius 6; approval color for an authorization, destructive color for a destructive action), title in `dialogTitle`, one consequence sentence in `text2` ("This leaves your Mac. Jojo can't undo it.").
2. Facts: a well (background `surface`, radius 10) holding a two-column grid, label column 96 wide in `text3`, values at 12.5; identifiers in `mono`. Row and column gaps 7 and 10. For a push: Repository, Remote ref, Commit, Verification, and a row "Does not" naming what is excluded ("Create a pull request").
3. Acknowledgement, where the action is irreversible: a checkbox whose label states exactly what was reviewed ("I reviewed the changes in this exact commit"). The primary button stays disabled until it is checked.
4. Buttons, trailing: the cancel button first with its key ("esc"), then the action button with its key ("⌘↵"). The cancel button of a destructive dialog says what continues ("Keep Running"), not "Cancel".

In the app these are sheets on the window they concern.

## 8. Command palette, menus, popovers, notifications

Native patterns.

| Component | Spec |
|---|---|
| Command palette (⌘K) | 560 wide, radius 12. Input row: padding 12 by 14, 16 pt glyph, 15 pt text, "esc" key cap, separator below. Results: padding 6, a group label, rows with padding 7 by 10, radius 7, a 14 pt glyph, the title, and a trailing kind or shortcut at 11.5. Selected row: `accent` background, white text |
| Context menu | The system menu. Items carry their shortcut; an item that cannot run is disabled and shows the reason in place of the shortcut ("Start Task · 4 blockers"); destructive items are last, after a separator, in `danger`, ending with an ellipsis |
| Provider selector | A popover. Each provider is a row with a 24 pt tile, its name at 13 semibold and one status line at 11.5 `text2`. The selected row has an `accent` tint (16%). A provider that cannot be chosen is shown at 55% opacity with the reason and the time it was reported ("Limit reached · reported 09:12") |
| Notification | A system notification: title naming what is needed and for which object ("Approval needed: ADR-031 rev 5"), body naming the effect ("Unblocks T6 in FT-142") |
| Key cap | Text 11 medium `text2`, minimum width 16, horizontal padding 5, height 17, radius 5, background `fill`, `kbdShadow` |

## 9. Inspector

One header and one section rhythm for the Task, Agent and Recovery inspectors. 320 wide, radius 10 when it floats; a `hairline` separates its parts.

| Part | Spec |
|---|---|
| Header | Padding 14 top, 16 sides, 12 bottom, gap 6. Kicker at 11 medium `text3` (kind and scope), title in `inspectorTitle` `textStrong`, then a wrapping row of status components with a gap of 6 |
| Section | Padding 12 by 16, gap 8. Title at 11.5 semibold `text3`. Content is usually a two-column grid: label column 104 wide in `text3`, values at 12.5, identifiers in `mono` |
| Actions | The last section: compact buttons, one primary at most |

## 10. Verification result and diff

**Result row.** Padding 10 by 14, gap 10, `hairline` between rows: name at 13 semibold, evidence pill, count in `mono` ("37 / 38"), the first failure in `text3` ("expected 410, got 200"), a compact secondary button ("Inspect Failure").

**Evidence row.** Name, digest in `mono` 11 `text3` shortened in the middle ("sha256 9f2c…41ab"), evidence pill.

**Diff.** Radius 10, monospaced 12 pt with a line height of 22, vertical padding 6. Columns: line number (34 wide, right aligned, muted), sign (16 wide), code (whitespace kept).

| Line | Background | Sign |
|---|---|---|
| Removed | `danger` at 10% | "-" in `danger` |
| Added | `ok` at 9% | "+" in `ok` |
| Context | none | none |

## 11. Banners

Full-width notice at the top of a surface whose data cannot be acted on as usual (see [STATES.md](STATES.md)). Padding 10 by 14, radius 9, gap 12: a 15 pt glyph, a title at 13 semibold `text`, one sentence in `text2`, and one compact button at the trailing edge.

| Kind | Background | Outline (1 pt) |
|---|---|---|
| Warning (stale) | `warn` at 8% | `warn` at 28% |
| Danger (disconnected, recovery required) | `danger` at 8% | `danger` at 28% |

## 12. Gaps in the canvas

The component board is drawn in dark only, with literal colors. Where it and the token table disagree, the token table wins (it is read from the Dashboard stylesheet). These points need a decision or a light value when their screen is built:

| Point | Component board (dark) | Token table |
|---|---|---|
| Secondary button and neutral pill background | white at 8% and 6% | `fill`, white at 7% |
| Primary button border | black at 20% | black at 12% |
| Halo of the Running dot | `info` at 22% | `infoTint`, 12% |
| Card outline | white at 7% | `cardShadow`, white at 6% |
| Dialog and palette background | `#26262B` | no token; no light value |
| Popover and menu background | `rgba(40,40,45,0.97)` | no token; no light value. System material in the app |
| Gate node background | `#1E1E22` | no token; no light value |
| Diff background, line-number and code colors | `#17171A`, `#6E6E76`, `#E6E6EA` | no token; no light value |
| Dashed outlines, stepper rings and connectors | white at 12% to 32% | no token; no light value |
| Banner tint and outline | status color at 8% and 28% | differ from the `*Tint` and `*Ring` tokens |
| Pressed and focused states | not drawn | not defined |

## 13. Implementation notes: app shell and status components (design wave 1)

Built in `macos/JojoApp/Sources/JojoApp`: `Theme.swift` (tokens, typography, card elevation), `StatusComponents.swift` (pills, tag, counter, key cap, buttons), `Shell.swift` (sidebar, toolbar, content). Nothing in this section is from the canvas; these are the values the code needed that sections 1 to 4 do not have.

| Point | Value in the app |
|---|---|
| Window | System window with a full-size content view and a transparent, untitled title bar. The toolbar strip starts at the top of the window; the sidebar content starts below the traffic lights. Opens at 1120 × 720, minimum 900 × 620 |
| Appearance | Follows the system. There is no switch in the app yet |
| Sidebar glyphs (SF Symbols) | Dashboard `square.grid.2x2`, Projects `folder`, Approval Inbox `tray`, Recovery Center `cross.case`, Feature Chat `bubble.left.and.bubble.right`, Specifications `doc.text`, Execution Plan `list.bullet.indent`, Agent Runs `play.circle`, Source Control `arrow.triangle.branch`, Verification & Evidence `checkmark.shield`, Activity & Audit `clock.arrow.circlepath`, Settings & Providers `gearshape` |
| Operational pill symbols | Blocked `exclamationmark.triangle.fill`, Paused `pause.fill` |
| Evidence pill symbols | Passed `checkmark`, Sealed `lock.fill`, Failed `xmark`, Incomplete `circle.lefthalf.filled`, Needs approval `hand.raised.fill`, Not run `minus`, Skipped `forward.fill`, Invalidated `slash.circle`, Unknown a bold "?" |
| Toolbar symbols | Refresh `arrow.clockwise`, Show Inspector `sidebar.right` |
| Pressed button | Opacity 0.8. A custom button style has no system pressed state |
| Dashed outline of Unknown, light | Black at 30% (dark: white at 30%, from the canvas) |
| Counter on a selected sidebar item | `onAccent` text on white at 20% |
| Key cap | Height 18, horizontal padding 5 |
| Search field to first item | 8 |
| Search field | Drawn disabled (opacity 0.42) with the tooltip "The command palette is not available yet." until the palette exists |
| Feature switcher | No project or feature exists in the helper yet. It shows "No project" and "No feature selected" on a `tileNeutral` tile, never an example |
| Status well | Dot: `ok` for admission `Open`, `danger` for any other reported value, an empty ring when unknown. Admission is stated only while the helper connection is ready; otherwise "Admission unknown". The identifier is the helper epoch. The second line is the helper connection, because provider availability is not reported yet |
| Toolbar breadcrumb | "Jojo › screen", and "Jojo › System › screen" for the System group, until projects and features exist |
| Toolbar, command pending | A "Requested" pill (Waiting form) while `WorkspaceModel.commandPending` is true |
| Sidebar counters | Recovery Center: subjects on hold, `warn`. Agent Runs: the snapshot's attempt rows, plain count |
| Destinations without a screen | The Empty form of STATES.md with the destination's glyph and "… is not built yet" |

Three screens that existed before the shell are shown unchanged under these destinations until each is redesigned: Review under Approval Inbox, Start under Execution Plan, Candidate under Verification & Evidence. Recovery Center ([screens/RECOVERY_CENTER.md](screens/RECOVERY_CENTER.md)) and Agent Runs ([screens/AGENT_RUNS.md](screens/AGENT_RUNS.md)) are redesigned.

Review aid: `JOJO_SNAPSHOT_DIR=<directory> swift test --filter DesignShellTests` writes `shell-dark.png` and `shell-light.png` (the shell with every status component, button kind and shared surface), and each redesigned screen in its states (`recovery-center-…`, `recovery-closed-…`, `launch-review-…`, `recovery-disconnected-…`, `recovery-empty-…`, `recovery-loading-…`; `agent-runs-…` at the opening size, `agent-runs-wide-…`, `agent-runs-held-…`, `agent-runs-unloaded-…`, `agent-runs-disconnected-…`, `agent-runs-empty-…`, `agent-runs-loading-…`), rendered off screen.

## 14. Implementation notes: shared surfaces (design wave 1, screens)

Built in `macos/JojoApp/Sources/JojoApp/Surfaces.swift` for the first redesigned screen and meant for the next ones. The values below are the ones sections 5 and 11 and the canvas boards do not have.

| Component | Notes |
|---|---|
| `Banner` | Section 11. Warning glyph `exclamationmark.triangle.fill`, danger `exclamationmark.octagon.fill`, in the status color. The message may wrap. The button is optional: a state whose restoring action does not exist yet has none |
| `Card` | Section 5.1 as `CardShape.page`. `.list` is the same card without padding. `.panel` (radius 10, padding 12 by 14) and `.panelList` are the smaller cards the Recovery Center boards use inside a column; both take the card ring and shadow of the theme |
| `Rows`, `GroupHeader` | Section 5.3: hairline between rows, none after the last |
| `ListRow` | Section 5.3. The leading slot is any view (`LeadingTile`, a `CheckBox`); the chevron is optional. `wraps` lets the second line wrap at 13 pt and aligns the row to the top. `inset` replaces the 16 pt side padding |
| `LeadingTile` | 26 × 26, radius 7, 13 pt medium glyph in `onAccent`; `tileNeutral`, or the status color of a tone |
| `SectionHeading` | 12 semibold `text2`, minimum height 22, optional trailing slot |
| `Footnote` | 12 pt `info.circle` and one sentence in the `meta` style, `text3` |
| `StateBlock` | The centered block of STATES.md (Empty, Error, Permission denied) |
| `LoadingBlock` | The Loading form of STATES.md; bars in `fill`, at most 420 wide |
| `CheckBox` | 14 × 14, radius 3.5. On: `accent` with a 9 pt bold white check and the primary button border. Off: 1 pt `text4` ring. Drawn, because a system checkbox cannot be rendered off screen |
| `Scrolling` | A vertical scroll view; in the off-screen snapshot (`offscreen` in the environment) the content is laid out in place and clipped |

A redesigned screen fills the content area and sets its own padding; the screens not yet redesigned keep the 20 pt side padding of the shell.

## 15. Implementation notes: attempt surfaces (design wave 1, Agent Runs)

Added to `Surfaces.swift` for Agent Runs ([screens/AGENT_RUNS.md](screens/AGENT_RUNS.md)) and meant for the next screens. The measures are the canvas board's; the values it does not have are named.

| Component | Notes |
|---|---|
| `WrappingRow` | A row that continues on further lines when it runs out of width; gap 28, line gap 14 by default. The summary strip's fields |
| `LabeledField` | One fact of a summary strip: a label in the `meta` style, `text3`, over any value, gap 3 |
| `Timeline`, `TimelineEntry` | Entries oldest first: padding 5 vertical, gap 12, a 9 pt dot 4 below the top, the sentence at 13 in `text`, an optional time in `mono` `text3`. Dot: the status color of the entry's tone. Not on the canvas: a neutral entry is a 1.5 pt `text4` ring without fill; the connector is 1 pt `hairline` (canvas: white at 10%) and is left out after the last entry |
| `Console`, `ConsoleLine` | Terminal output: monospaced 12, line height 21, radius 10, padding 12 by 14, selectable, wrapping (no horizontal scroll). Line kinds: plain, command, or a status tone. Not on the canvas: background `#F1F1F4` in light (dark `#111113` from the canvas, no token); ring `cardRing`; plain lines in `text2` and commands in `text` (canvas `text3` and `#E6E6EA`) so that both read on the light console |
| `InspectorSection` | Title at 11.5 semibold `text3`, padding 12 by 16, gap 8, `hairline` below unless it is the last |
| `FactGrid`, `Fact` | The two-column facts of an inspector section: labels 104 wide at 12.5 `text3`, values in `text` and allowed to wrap; row gap 7, column gap 10, baseline aligned |
| `NoticeWell` | One sentence in the `meta` style, `text2`; padding 8 by 10, radius 8, background `fill2` (canvas: white at 4%) |

Not built from the board: the segmented control, the compact lifecycle stepper, the console's search field with its copy and save buttons, and the evidence tags "Recording", "Recorded" and "Truncated". Each needs data the helper does not report yet.

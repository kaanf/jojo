# Jojo design system: components

The reusable parts of the app and the status model they draw. Token names refer to [DESIGN_SYSTEM.md](DESIGN_SYSTEM.md). Source: the canvas page "Components & states", board "Components and status model" (`ComponentsMac.dc.html`), and the shell of the product boards (`DashboardMac.dc.html`, `AgentRunsMac.dc.html`, `RecoveryLaunchMac.dc.html`); see [README.md](README.md).

Sections 1 to 12 are the design. Sections 13 to 27 are implementation notes written for the first design system; each is rewritten when its surface moves (DESIGN_SYSTEM.md, section 12).

## 1. Status model

Three separate families plus document status. Colors are shared, shapes are not, so they can sit side by side without being read as one progress value: a feature can be in Execution while its tasks are Running and Blocked and its evidence is Incomplete.

| Family | Answers | Shape |
|---|---|---|
| Feature lifecycle | Where the feature is | Stepper of dots and bars |
| Operational state | What is happening now | Capsule with a dot |
| Evidence and verification | What is proven | Rounded square with a symbol |
| Document status | What is binding | Uppercase outline tag |

**Unknown is never drawn as success or failure.** It is gray with a question mark.

## 2. Status components

### 2.1 Feature lifecycle stepper

Stages, in order: Discovery, Specification, Planning, Execution, Verification, Evidence. Drawn on a card (radius 14, padding 10 14) with the feature's task counts after it: pills "n running" (blue), "n blocked" (red), "n awaiting approval" (orange). The stepper scrolls sideways when narrow; the counts wrap below.

| Stage | Dot (8) | Label (12) | Bar after it (2 high, flexible, at least 12) |
|---|---|---|---|
| Done | `green` | medium, `text2` | `green` |
| Current | `accent` with a 3 pt `accentTint` halo | semibold, `text` | `dotOff` |
| Upcoming | `dotOff` | medium, `text2` | `dotOff` |

Dots and bars are 6 apart, stages 8 apart.

### 2.2 Operational pill

Capsule, height 22, padding 0 10, gap 6, text 11 semibold, a 6 pt dot in the text color before the label. Text in the hue, background its tint.

| State | Hue |
|---|---|
| Idle | gray |
| Running | blue |
| Waiting | gray |
| Blocked | orange |
| Awaiting approval | orange |
| Paused | gray |
| Recovery required | red |
| Failed | red |
| Completed | green |
| Requested (sent to the core, not yet confirmed) | purple |
| Repairing (M4, D2; not on the canvas) | blue |

In list rows and inside cards the same pill is drawn 20 high with padding 0 8 and without the dot (`DashboardMac`: "Proposed", "Needed", "Held", "Review"). Counters in the sidebar are pills 18 high, padding 0 6.

### 2.3 Evidence pill

Rounded square, radius 6, height 22, padding 0 8, gap 5, text 11 semibold, a text symbol before the label.

| State | Symbol | Hue |
|---|---|---|
| Passed | ✓ | green |
| Failed | ✕ | red |
| Unknown | ? | gray |
| Not run | ○ | gray |
| Skipped | ↷ | gray |
| Incomplete | ◐ | orange |
| Sealed | ◆ | green |
| Invalidated | ⊘ | gray |
| Needs approval | ! | purple |

### 2.4 Document tag

Height 18, padding 0 6, radius 4, text 10 bold uppercase, tracking +0.06 em, a 1 pt ring in the text color and no fill.

| Status | Hue |
|---|---|
| Proposed | blue |
| Approved | green |
| Superseded | gray |
| Invalidated | red |
| Stale | orange |

Other uppercase tags use the same shape: "Eligible" (purple, on task nodes), "Not available yet" (gray), "Temporarily unavailable" (orange), "New" (blue, a changed line in a document).

## 3. Buttons and availability

One primary action per area, filled with the accent. Secondary actions are glass capsules. Destructive actions are red text and always confirm. Disabled controls say why.

| Kind | Look |
|---|---|
| Primary | `accent` fill, white text 13 medium, height 32, padding 0 14, capsule; a shortcut after the label at 75% opacity, 12 ("Start Discovery ⌘↵") |
| Secondary | glass capsule (`glassStrong`), `text` |
| Text (ghost) | no fill, `accent` text ("Reset", "View all") |
| Destructive | no fill, `red` text, label ends with "…" when a confirmation follows ("Cancel Attempt…") |
| Icon | 32 × 32 capsule, inside a glass capsule group with padding 2 |
| Small | height 28, text 12 (inspector actions, empty states, row actions) |
| Toolbar action | glass capsule, height 34, padding 0 16 |

**Disabled always explains why.** A disabled button has a `grayBg` fill and `text3` text, and the reason stands beside it in `secondary` style ("Seal Evidence · Unavailable: 5 requirements unmet.").

**Unavailable and temporarily disabled differ.** A feature that does not exist yet carries the gray tag "Not available yet" ("Import from Figma"). One that is off for now carries the orange tag "Temporarily unavailable" ("Assign to Codex").

**Requested, not yet confirmed.** No optimistic success: after a command is sent the control shows the purple operational pill "Requested" and "Sent to core · waiting for confirmation" until the core records the outcome.

## 4. Materials and the app shell

Four surfaces. Glass is for chrome that floats (sidebar, inspector, toolbar capsules, sheets). Content sits on cards. Never put glass on glass.

| Surface | Use |
|---|---|
| Glass panel | Sidebar, inspector, banners, alerts, menus, palette, notifications. Blur 30, rim highlight |
| Capsule | Toolbar groups, secondary buttons, segmented controls, the search field |
| Card | Content: lists, documents, tables, KPI tiles. Hairline ring, no blur |
| Well | Quiet grouping: the gates column, freshness strips. `grayBg`, no ring |

### 4.1 Window

The window material (radius 26) with padding 8. Inside it, side by side: the sidebar, the content column and, on screens that have one, the inspector. Sidebar and inspector are glass panels inset from the window edge (radius 18); the content column has no surface of its own.

### 4.2 Sidebar

Glass panel, 240 to 256 wide, padding 14 10 12, groups 14 apart.

1. The traffic lights (system), then the search capsule: height 30, a magnifier, the placeholder "Search or run" in `text3`, "⌘K" at 11.
2. Workspace: Dashboard, Projects, Approval Inbox (counter: the pending approvals), Recovery Center (counter: "n held").
3. The current feature, headed by the project badge (16 × 16, radius 5, the project's initial, 10 bold white) and "Project · Feature-ID Feature name" in `sidebarSection` style: Feature Chat, Specifications, Execution Plan, Agent Runs, Source Control, Verification & Evidence.
4. System: Activity & Audit, Settings & Providers.
5. At the bottom, a card (radius 12, padding 10): the admission dot and "Admission open" (12 semibold) with the session identifier ("S-0412", `text2`), then one pill per provider: green with its name when available, orange "Codex limited" when the CLI reports a limit.

A nav item is 30 high, radius 9, padding 0 9, gap 9: an `accent` glyph, the label (13), and a trailing counter. Plain counts are `text2` at 12 ("2", "5 held"); counts that need the human are pills 18 high: blue for open questions and proposed documents, orange for violations. The chosen item has `accentTint` and a semibold label; hover is `grayBg`.

### 4.3 Header

No separate toolbar strip: each screen starts with a header row (min height 44, gap 10 to 12, wraps).

- **Title block:** the screen title (`pageTitle` 20, Dashboard 22) and under it a `sub` line: the place ("Workspace · Monday, 16 November", "Workspace · Recovery Center · Launch review") or the path "Crew › FT-142 Lobby invite links › attempts".
- **Actions**, trailing: a glass capsule group of text buttons for related views (12, transparent), then toolbar actions (capsules, 34 high), then a capsule group of icon buttons (filter, inspector), and at most one primary button.
- **Feature screens** add the lifecycle card (section 2.1) under the header.

### 4.4 Inspector

A glass panel on the trailing side, 280 to 320 wide, radius 18, padding 16. Task, Agent and Recovery inspectors share one header and section rhythm:

- **Header:** a kicker in `sub` style ("Agent attempt · T4"), the title (`inspectorTitle` 16 bold), then the object's pills (operational and evidence side by side).
- **Sections**, 14 apart: an uppercase `label` heading, then a key-value card (radius 12): rows padding 9 12, gap 12, label column 70 to 110 in `text2`, values in `text` or `mono`, rows separated by a 0.5 `sep` line.
- **Actions:** the last section, small buttons (28), primary first.

### 4.5 Content

Sections 16 to 20 apart. A section heading is a row: the title (`sectionTitle` 15 semibold), a count in `sub` style, and a trailing text link (13 medium `accent`, "Approval Inbox") that opens the related screen. Group labels inside a section are 11 semibold `text3` in capitals ("APPROVALS · 4").

## 5. Cards, lists and rows

### 5.1 Card

`card` fill, inner 0.5 `sep` ring, radius 14. Lists inside a card clip to it.

### 5.2 KPI tile

A clickable card, radius 16, padding 14 16, gap 6: a 24 × 24 tile (radius 7, fixed color, white glyph) and a label (13 semibold); the value (30 bold, −0.03 em) with its unit in `sub` style on the same baseline; a `sub` line that names the items ("T4 on Claude Code · T5 on Codex"). Laid out in a grid of columns at least 190 wide, gap 12.

### 5.3 List row

A button, min height 52, padding 8 14, gap 12: a 28 × 28 tile (radius 8, fixed color by kind), a column with the title (13 semibold) and a `sub` line, a status pill (20 high), and a trailing chevron. Rows after the first have a 0.5 `sep` top line; hover is `cardHover`.

Tile colors by kind (DashboardMac): architecture decision indigo, plan blue, scope expansion orange, external action teal, recovery hold red, recovery review purple.

### 5.4 Picker row

For lists in a column (attempts, held items, sections): padding 9 10, radius 10, gap 10, no card; a title (13 semibold) with a status pill (18 high) and a `sub` line. Hover `grayBg`, chosen `accentTint`. Group headings in the column are `label` style.

### 5.5 Pipeline cells

The Dashboard's project table: one row per feature, one column per lifecycle stage (grid: the feature column at least 200 and 2 parts, six stage columns at least 86 and 1 part; gap 8, padding 12 16, rows separated by a 0.5 `sep` line). The feature column holds its name (13 semibold) and "Project · Feature-ID" in `sub`. A stage cell holds an 8 pt dot and a word (12 `text2`, gap 6): green "Done", orange for what waits on the human ("ADR rev 3", "Q open", "Stale"), the running dot with "n running", green "Sealed". Stages not reached are a `dotOff` dot alone.

### 5.6 Field, check box, segmented control

- **Field:** min height 34, padding 0 12, radius 9, `cardHover` fill, inner 0.5 `sep` ring and a faint inner top shadow; text 13.
- **Check box:** 18 × 18, radius 5, 1.5 ring `text3`; on, `accent` with a white check. Used as a row's leading slot in review lists.
- **Segmented control:** a capsule group with padding 2; segments 28 high, padding 0 13, text 12 medium; the chosen one is a `cardHover` capsule with a soft shadow.

## 6. Dependency nodes

Task nodes are solid cards; gates are dashed.

| Node | Look |
|---|---|
| Task | `cardHover`, inner 0.5 `sep` ring, radius 12, padding 10 12, gap 4, at least 170 wide. First line: the task key (`mono` bold) and its pill; then the name (13 semibold); then the provider in `sub` style, with the purple tag "Eligible" when it can start, or the blocker count |
| Running task | a 2 pt `accent` ring |
| Blocked task | a 2 pt `orange` ring |
| Passed task | the evidence pill "✓ Passed" in place of the operational pill |
| Gate | no fill, a 1.5 dashed `text3` outline, radius 12: the gate kind as a `label` ("Approval gate", "Deployment gate"), its subject (13 semibold), its progress in `sub` style ("0 of 2 approved", "0 of 4 checks") |

## 7. Alerts and confirmations

macOS alert layout, only for consequential actions: a glass panel, radius 22, padding 22, gap 12, 320 to 380 wide.

1. The app icon tile (44 × 44, radius 12, the app gradient, "J").
2. The question as the title (15 bold): "Authorize push to origin?", "Cancel T4 · Attempt 2?".
3. What it does, in 12 `text2`: "This leaves your Mac. Jojo can't undo it."
4. The exact object as a key-value card (labels 86 wide): Repository, Remote ref, Commit, Verification (with its evidence pill), and **Does not** ("Create a pull request").
5. For external actions, a check box the human ticks: "I reviewed the changes in this exact commit".
6. Buttons of equal width, the default on the right: a capsule "Cancel" / "Keep Running", and the action, primary ("Authorize Push") or destructive filled `#ff3b30` ("Cancel Attempt").

## 8. Command palette, notification, context menu, provider selector

Native patterns on Liquid Glass.

- **Command palette (⌘K):** a glass panel, radius 18, padding 8, at most 460 wide. The query (15) with a magnifier and "esc"; a separator; menu items with a trailing shortcut or kind ("Inspect T4 · Invite endpoint ⌘I", "Open ADR-031 · Invite token storage · Spec", "Go to FT-142 · Lobby invite links · Feature", "Search history for “inv” · Audit").
- **Notification:** a system notification: the app icon (36), "Jojo", the time, a title ("Approval needed: ADR-031 rev 5") and a line ("Unblocks T6 in FT-142").
- **Context menu:** a glass menu, radius 12, padding 5, at least 240 wide. Items 13, padding 5 10, radius 7, shortcuts in `kbd`; separators 1 high with 4 8 margins. An unavailable item is `text3` with its reason trailing ("Start Task · 4 blockers"); a destructive one is red ("Cancel Attempt…").
- **Provider selector:** a glass menu. Each provider: a 22 × 22 tile (radius 6, "CC" on `#c2643a`; "CX"), the name (semibold) and its capabilities in `sub` style ("Resume · events · tokens"), a check mark on the chosen one. An unavailable provider is drawn as an unavailable item with the reason ("Limit reached · reported 09:12") and a gray tile.

## 9. Inspector, verification result, diff

- **Inspector:** section 4.4.
- **Verification result row:** a list row without a tile: the suite name (13 semibold) and its detail as the `sub` line (red when failed: "expected 410, got 200"), the count in `mono` ("37 / 38"), the evidence pill, and a small primary action on the failing row ("Inspect Failure"). A manifest row shows its digest in `mono` `text2` ("sha256 9f2c…41ab").
- **Diff:** `code` text (`mono` 12, line height 1.6) on the card, padding 8 0, a 0.5 `sep` top line. Each line is a grid of three columns (30, 16, flexible; gap 6; padding 1 12): the line number in `text3`, the sign, the code. Added lines on `greenBg`, removed lines on `redBg`.

## 10. Banners

Two sizes.

- **Page banner:** a rounded panel (radius 18, padding 18 20, gap 14) on the hue's tint (`redBg` for closed admission), with a 36 × 36 tile (radius 10, fixed color, white glyph), a title (17 bold) and a sentence (13, line height 1.55). It stands above the content it blocks.
- **Inline banner:** a glass strip inside a content card (inset 12, radius 16, padding 10 12, gap 12, wraps) with a 26 × 26 tile, a title (13 semibold), a `sub` sentence and an action at its trailing edge ("Feature continues · T7 is blocked by Q4" · "Answer Q4").

## 11. Fact strip

A grid of small cards (radius 14, padding 12 14, gap 4, columns at least 180 wide, gap 10): a `label` and the value (13 semibold, `mono` 15 bold for identifiers, or a pill with a `sub` note). Used for the inspection on the launch review, the candidate on Verification, a branch on Source Control.

## 12. Not drawn on the canvas

These have no value on the boards; the app uses the system's until they are drawn: pressed and keyboard-focus states, a narrow-window layout of the inspector (the boards wrap it under the content), drag and drop targets, tooltips.

## 13. Implementation notes: app shell and status components (Liquid Glass migration)

Rewritten on 2026-11-17 for the Liquid Glass migration (STATUS.md, step 1). Built in `macos/JojoApp/Sources/JojoApp`: `Theme.swift` (the token tables of DESIGN_SYSTEM.md, sections 2 and 4, the fixed colors as `Palette`, typography, shadow levels, the materials `glassPanel`, `glassCapsule`, `cardElevation` and `well`, and `WindowMaterial`), `StatusComponents.swift` (pills, tag, counter, shortcut hint, buttons, `CapsuleGroup`), `Shell.swift` (window, sidebar, header row, `InspectorPanel`). The package targets macOS 26 (tools 6.2) for `glassEffect`.

| Point | Value in the app |
|---|---|
| Window | System window with a full-size content view and a transparent, untitled title bar. `WindowMaterial`: the `window` token over a behind-window blur; padding 8; sidebar and content 10 apart. The sidebar and header start at the top of the window, beside the traffic lights |
| Glass on screen | `.glassEffect(.regular, in:)`, a rounded rectangle 18 for panels, a capsule for capsules. At shadow level None a 0.5 `sep` ring is added |
| Glass off screen and under Reduce Transparency | The token at alpha 1 over the opaque window (`Tokens.opaque`), with the rim edges of section 2; set by `\.offscreen`, which replaces the earlier `\.opaqueSidebar` |
| Shadow level | `\.shadowLevel`, default Reduced. Not settable yet: Settings is read-only until it is rebuilt |
| Appearance | Follows the system. The Settings › Appearance switch is not built yet |
| Old token names | `sidebar`, `surface`, `fill`, `hairline`, `info`, `warn` and the other names of the first system are computed from the new tokens (an extension of `Tokens`), so screens not rebuilt yet draw the new values. The old text styles `nav`, `controlStrong`, `meta`, `groupHeader` map to the new table. Both go when the last screen moves |
| Tone | Six cases: `neutral` gray, `info` blue, `warn` orange, `ok` green, `danger` red, `review` purple |
| Sidebar | Glass panel 248 wide, padding 14 10 12, a 32 pt space for the traffic lights, which an empty unified toolbar places inside the panel, the search capsule (30 high, disabled at 60% with the tooltip "The command palette is not available yet." until the palette exists), groups 14 apart |
| Sidebar glyphs (SF Symbols) | Dashboard `square.grid.2x2`, Projects `folder`, Approval Inbox `tray`, Recovery Center `cross.case`, Feature Chat `bubble.left.and.bubble.right`, Specifications `doc.text`, Execution Plan `list.bullet.indent`, Agent Runs `play.circle`, Source Control `arrow.triangle.branch`, Verification & Evidence `checkmark.shield`, Activity & Audit `clock.arrow.circlepath`, Settings & Providers `gearshape` |
| Feature group heading | A gray 16 × 16 badge with "–" and "No project · No feature selected": the helper reports neither yet, so no example is shown |
| Status card | Card radius 12, padding 10. Dot: `ok` for admission `Open`, `danger` for any other reported value, an empty ring when unknown. Admission is stated only while the helper is connected. Provider pills are not shown: the helper does not report availability or limits yet |
| Sidebar counters | Recovery Center: subjects on hold, orange pill. Agent Runs: the snapshot's attempt rows, plain count in `text2`. Source Control: the loaded attempt's changed paths outside the write scope, orange pill |
| Header row | Built by the shell for every screen until each screen draws its own: the title (`pageTitle`, Dashboard `dashboardTitle`), the `sub` line "place · screen" with the place "Workspace", "System" or "No feature selected", then "Requested" (standing pill) while a command awaits the core, then a capsule group with Refresh `arrow.clockwise` and Show Inspector `sidebar.right` |
| Inspector | `InspectorPanel`: glass panel radius 18, 280 to 300 wide. Used by Agent Runs and Execution Plan; other screens keep their columns until rebuilt |
| Operational pill | Standing (header, inspector): 22 high with its glyph. In rows and cards: 20 high, no glyph. Glyphs: dot; Running a dot with an `accentTint` halo; Waiting a ring; Recovery required a ring with a dot; Blocked `exclamationmark.triangle.fill`; Paused `pause.fill`; Repairing `arrow.triangle.2.circlepath` |
| Requested | A state of its own (`OperationalState.requested`, purple) |
| Evidence pill | The text symbols of section 2.3 at 11 bold |
| Document tag | `DocumentTag(label, tone:)` draws the other uppercase tags of section 2.4 |
| Buttons | Primary: `accent` capsule with the inner top highlight of the shadow level. Secondary: glass capsule. Ghost: `accent` text. Destructive: `red` text. Disabled: `grayBg` and `text3`. Pressed: opacity 0.8 (a custom button style has no system pressed state) |
| Dark accent | White on `#0a84ff` is 3.6:1, under 4.5. Kept as drawn; the test holds it to 3:1 |
| Destinations without a screen | The Empty form of STATES.md with the destination's glyph and "… is not built yet" |

No screen from before the shell is left: the old Start screen was replaced by Execution Plan. Recovery Center ([screens/RECOVERY_CENTER.md](screens/RECOVERY_CENTER.md)), Agent Runs ([screens/AGENT_RUNS.md](screens/AGENT_RUNS.md)), Verification & Evidence ([screens/VERIFICATION.md](screens/VERIFICATION.md)), Source Control ([screens/SOURCE_CONTROL.md](screens/SOURCE_CONTROL.md)), the Approval Inbox ([screens/APPROVAL_INBOX.md](screens/APPROVAL_INBOX.md)), which replaced the old Review screen, Specifications ([screens/SPECIFICATIONS.md](screens/SPECIFICATIONS.md)), Activity & Audit ([screens/ACTIVITY.md](screens/ACTIVITY.md)), Settings & Providers ([screens/SETTINGS.md](screens/SETTINGS.md)) and Execution Plan ([screens/EXECUTION_PLAN.md](screens/EXECUTION_PLAN.md)) are redesigned. Execution Plan added no component: the lifecycle stepper (section 2.1), the pipeline cells (section 5.4) and the dependency nodes (section 6) stay unbuilt, because the core reports no plan to draw them from.

Review aid: `JOJO_SNAPSHOT_DIR=<directory> swift test --filter DesignShellTests` writes `shell-dark.png` and `shell-light.png` (the shell with every status component, button kind and shared surface), and each redesigned screen in its states (`recovery-center-…`, `recovery-closed-…`, `launch-review-…`, `recovery-disconnected-…`, `recovery-empty-…`, `recovery-loading-…`; `agent-runs-…` at the opening size, `agent-runs-wide-…`, `agent-runs-held-…`, `agent-runs-unloaded-…`, `agent-runs-disconnected-…`, `agent-runs-empty-…`, `agent-runs-loading-…`; `verification-…` at the opening size, `verification-tall-…`, `verification-failed-…` wide, `verification-narrow-…`, `verification-unknown-…`, `verification-not-run-…`, `verification-requested-…`, `verification-disconnected-…`, `verification-empty-…`; `source-control-…` at the opening size, `source-control-tall-…`, `source-control-wide-…` with a chosen path, `source-control-missing-…` for a chosen path without a section, `source-control-narrow-…`, `source-control-passed-…`, `source-control-unknown-…`, `source-control-disconnected-…`, `source-control-empty-…`; `approval-inbox-…` at the opening size, `approval-inbox-tall-…`, `approval-inbox-wide-…` with a decided plan, `approval-inbox-narrow-…`, `approval-inbox-requested-…`, `approval-inbox-stale-…`, `approval-inbox-closed-…`, `approval-inbox-disconnected-…`, `approval-inbox-unloaded-…` without a loaded document, `approval-inbox-empty-…` without documents; `specifications-…` at the opening size, `specifications-tall-…`, `specifications-wide-…` with a decided plan, `specifications-narrow-…`, `specifications-stale-…`, `specifications-closed-…`, `specifications-disconnected-…`, `specifications-unloaded-…` without a loaded document, `specifications-empty-…` without documents; `activity-…` at the opening size, `activity-tall-…`, `activity-wide-…` with a chosen event, `activity-narrow-…`, `activity-filtered-…` with a search and two types off, `activity-unmatched-…` for a search without a result, `activity-loading-…`, `activity-empty-…`, `activity-failed-…` without events, `activity-failed-loaded-…` with the events read before, `activity-closed-…`, `activity-disconnected-…`; `settings-claude-…` and one file per section (`settings-codex-…`, `settings-core-…`, `settings-discovery-…`, `settings-defaults-…`, `settings-git-…`, `settings-storage-…`, `settings-retention-…`, `settings-security-…`, `settings-shortcuts-…`, `settings-appearance-…`), `settings-narrow-…`, `settings-empty-…` for a provider without launch options, `settings-loading-…`, `settings-incompatible-…`, `settings-disconnected-…`, `settings-disconnected-provider-…`; `execution-plan-empty-…`, `execution-plan-ready-…`, `execution-plan-invalid-…`, `execution-plan-narrow-…`, `execution-plan-requested-…`, `execution-plan-accepted-…`, `execution-plan-refused-…`, `execution-plan-unknown-…`, `execution-plan-admission-closed-…`, `execution-plan-no-slice-…`, `execution-plan-loading-…`, `execution-plan-incompatible-…`, `execution-plan-disconnected-…`), rendered off screen.

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

## 16. Implementation notes: verification surfaces (design wave 1, Verification & Evidence)

Added to `Surfaces.swift` for Verification & Evidence ([screens/VERIFICATION.md](screens/VERIFICATION.md)). Section 10 and the canvas board give the measures; the values they do not have are named.

| Component | Notes |
|---|---|
| `ResultRow` | Section 10, with the columns of the board: the name at 13 semibold `textStrong` over an optional second line (`meta`, `text3`), 150 wide; the evidence pill in a column 130 wide; an optional count in `mono`, 80 wide; the command in `mono` over a note (`meta`, `text3`), flexible; one trailing control. Padding 10 by 14, gap 10. Not on the canvas: the name column is fixed at 150 and the row does not wrap; its texts do. The hairline between rows is drawn by the list (`Rows`), not by the row |
| `EvidenceRow` | Section 10: the name at 13 in `text` (or in `mono` when it is a path), a note in `mono` `text3` on one line, an evidence pill. Padding 9 by 14, gap 10 |
| `DiffLine`, `DiffView` | Section 10: monospaced 12, rows at least 22 high, vertical padding 6, radius 10; line number 34 wide and right aligned, sign 16 wide, code wrapping. `DiffLine.parse` reads a unified diff: a line is added, removed or context only inside the line counts of its hunk header, so code that looks like a file header stays code; removed lines carry the old file's number, added and context lines the new file's; file headers, hunk headers and anything outside a hunk have no number and no sign. Not on the canvas: background `#F1F1F4` in light (dark `#17171A`, section 12); ring `cardRing`; numbers and header lines in `text3` (canvas `#6E6E76`), code in `text` (canvas `#E6E6EA`); 14 of trailing padding |
| `InputField` | Not on the Verification board; the shape is the search field of the Agent Runs board: height 26, radius 6, background `fill`, horizontal padding 8, text at 12 (monospaced for an identity), prompt in `text3`. A plain system text field; in the off-screen snapshot its text or prompt is drawn in place |
| `RequestedNotice` | The "Requested" state of STATES.md and of section 3: a "Requested" pill (Waiting form) and "Sent to core · waiting for confirmation" in `meta`, `text2`; padding 8 by 12, radius 8, gap 8, background `fill2` (canvas: white at 5%) |

Not built from the board: the count pills of the verdict header, the "Missing" pill (it is not one of the nine evidence states of section 2.3), the requirement lines of "Seal evidence" and the history rows. Each needs data or a command the helper does not have yet.

## 17. Implementation notes: source control surfaces (design wave 1, Source Control)

Added to `Surfaces.swift` for Source Control ([screens/SOURCE_CONTROL.md](screens/SOURCE_CONTROL.md)). The measures are the canvas board's; the values it does not have are named.

| Component | Notes |
|---|---|
| `ChangeBadge` | The kind of change of a path: 15 × 15, radius 4, one letter at 10.5 semibold in the status color of a tone, on that color at 16%. The kind in words is the tooltip and the accessibility label. Not on the canvas: a neutral badge ("?") for a kind the app does not know, and "T" for a type change |
| `ChangedFileRow` | One changed path in two lines, gap 3, padding 6 by 8, radius 7: the badge, the file name at 12.5 in `text` (one line, truncated in the middle), 8 between them, and the line counts in `mono` `text3`; under it, indented 23, the directory in monospaced 10.5 `text3` (one line, truncated at its head) and, gap 6, an optional evidence pill. Flagged (outside the scope): background `danger` at 8% and a 1 pt ring in `dangerRing` (canvas: `danger` at 35%). Not on the canvas: a selected row has the background `fill` and keeps the ring of a flagged row. The row is a plain view; the screen wraps it in a button |

Not built from the board: the segmented control (repositories, "Unified" and "Side by side"), the two-number and the side-by-side diff, the facts lines of "Scope validation" (the evidence row is used), the entries of "External actions" and the CI lines. Repositories, external actions and CI are not reported; the side-by-side diff is left for a later step.

## 18. Implementation notes: approval surfaces (design wave 1, Approval Inbox)

Added to `Surfaces.swift` for the Approval Inbox ([screens/APPROVAL_INBOX.md](screens/APPROVAL_INBOX.md)). The measures are the canvas board's; the values it does not have are named.

| Component | Notes |
|---|---|
| `RequestTile` | The small icon tile of DESIGN_SYSTEM.md: 24 × 24 (22 in the head of a request), radius 6, a medium glyph at half the size in `onAccent`. The approval color (`#B5701F`, the same in both appearances) while a decision is awaited. Not on the canvas: `tileNeutral` otherwise (the board has `#6B6B74` for a stale request) |
| `RequestRow` | One request in three lines, gap 2, beside its tile (gap 10, aligned to the top); padding 9 by 10, radius 8. Line one: the kind at 11 `text3` and, trailing, an optional age in monospaced 10.5 `text3`. Line two: the object at 12.5 semibold in `text`, one line, truncated in the middle; monospaced when it is an identity. Line three: a note in the `meta` style, `text3`, and, gap 6, an optional document tag. Selected: background `accent` at 16% and a 1 pt ring in `info` at 40% (canvas: `#7AAFFF` at 40%), as the attempt rows of Agent Runs. The row is a plain view; the screen wraps it in a button |
| `FactLine` | One fact of a request: a label 130 wide in `text3` and a value in `text`, both at 12.5, baseline aligned, gap 10; padding 11 by 14; the value is any view and wraps; a `hairline` below unless it is the last line. Used inside a `.panelList` card |
| `ValueLine` | A name at 13 in `text` and its trailing value at 13 in `text2`; padding 8 by 14; a `hairline` below unless last. The value keeps its width and the name wraps first |

Not built from the board: the segmented filter, the links of "Supporting evidence", the lines of "Recently decided" (a document tag, the object and a date), and the "Filter" toolbar button. Each needs data the helper does not report yet. "Approval History" is not a toolbar button: it is a compact secondary button in the heading of "Recently decided" and opens Activity & Audit. The confirmation of Reject and Revoke is a system dialog until the dialog of section 7 is built.

## 19. Implementation notes: document surfaces (design wave 1, Specifications)

Added to `Surfaces.swift` for Specifications ([screens/SPECIFICATIONS.md](screens/SPECIFICATIONS.md)). The measures are the canvas board's; the values it does not have are named.

| Component | Notes |
|---|---|
| `DocumentRow` | One document of a list in two lines, gap 3; padding 7 by 10, radius 7. Line one: the document at 12.5 semibold in `text` (one line, truncated in the middle; monospaced when it is an identity) and, trailing, an optional document tag (section 2.4). Line two: a note in the `meta` style, `text3`, one line. `indented`: 24 of leading padding, for an earlier revision under its document. `quiet`, for a row that cannot be opened: padding 6 by 10, gap 2, the document at 12 regular in `text2`. Selected: background `accent` at 16% and a 1 pt ring in `info` at 40% (canvas: `#7AAFFF` at 40%), as a request row. The row is a plain view; the screen wraps it in a button |
| `DocumentSection` | One section of a document: a heading at 12 semibold `text3`, 6 above any content, the full width |
| `TraceLine` | One line of a traceability card: an identifier in monospaced 11.5 `info`, what it is at 13 in `text` (fills the width and wraps), and an optional trailing reference at 13 in `info`; padding 8 by 14, gap 10, baseline aligned; a `hairline` below unless it is the last line. Used inside a `.panelList` card. Not on the canvas: the trailing reference is text, not a link, until tasks exist |

`ReviewMark` (in `ApprovalInbox.swift`) is the mark in the head of a loaded review, shared by the Approval Inbox and Specifications: the pill "Requested", the tag "Stale", or the pill "No decision yet" or "Decision recorded" ([screens/SPECIFICATIONS.md](screens/SPECIFICATIONS.md), section 5.2).

Not built from the board: the segmented control ("Document", "Compare"), the paragraph of a section (13.5, line height 1.6, at most 700 wide), the option cards of "Alternatives", the two-column comparison of revisions, the finding lines of "Validation" with the pills "Conflict" and "Missing", the lines of "Approval history" with their tags, the outline of a document and the toolbar buttons. Each needs data the helper does not report yet: the text of a document, its earlier revisions, its validation and the history of its decisions. The "View all" of "Approval history" is built as a compact secondary button in that heading; it opens Activity & Audit and stands for the toolbar's "View Approval History".

## 20. Implementation notes: audit surfaces (design wave 1, Activity & Audit)

Added to `Surfaces.swift` for Activity & Audit ([screens/ACTIVITY.md](screens/ACTIVITY.md)). The measures are the canvas board's; the values it does not have are named.

| Component | Notes |
|---|---|
| `ListSearchField` | The search box above a list: 32 high, padding 10 at the sides, radius 8, background `surface`, a 1 pt ring in `ringEmpty` (canvas: white at 8%), gap 8; a `magnifyingglass` glyph at 12 medium in `text3`, then a one-line field at 13 in `text` with its prompt in `text3`. In the off-screen render its text or prompt is drawn in place. Not built: the key cap "⌘F" and the shortcut. (The toolbar's "Search or run" field of section 4.2 is another view, private to the shell) |
| `FilterChip` | A filter that is on or off, as a capsule 24 high with padding 10 at the sides: the name at 12 regular and an optional count in monospaced 11 at 75% opacity, gap 5. On: text `info` on `accent` at 16% (the board's pressed chip). Off, not on the canvas: text `text3` on `fill2` with a 1 pt ring in `hairline`. The chip is a plain view; the screen wraps it in a button |
| `KindTag` | The type of a record: 11 semibold, padding 6 at the sides, 18 high, radius 4, on `hover` (canvas: white at 5%), one line, truncated in the middle. Its color is the foreground of a tone; neutral (`text2`) unless a tone is given |
| `RecordHeader` | The captions of a record table at 11 semibold `text3`: padding 8 by 14, gap 10, a 1 pt edge below in `ringEmpty` (canvas: white at 8%). Four columns (`RecordColumns`): type 150, event the rest, subject 92, record 64. Not from the canvas, which has six columns (52, 92, 86, the rest, 86, 80): time and actor have no source today |
| `RecordGroup` | The caption of a group of records inside the table, at 11 semibold `text3`; padding 10 top, 14 sides, 6 bottom |
| `RecordRow` | One record in one line: padding 8 by 14, gap 10, a `hairline` below unless it is the last. A kind tag; what happened at 13 in `text` (at monospaced 12 when it is a raw payload), one line; the subject as an identifier at 11.5 in `text2`, truncated in the middle ("—" in `text4` without one); the record as an identifier at 11 in `text3`. Selected: background `accent` at 16% and a 1 pt ring in `info` at 40% in place of the edge. The row is a plain view; the screen wraps it in a button. Not built: the subject as a link |

Used inside a `.panelList` card. Not built from the board: the segmented controls for project and range, the "Export" toolbar button, the time and actor columns, the colors of a type tag by category, and the "Verified" chain pill. Each needs data or a command the helper does not have yet: a time, an actor, a project and a category per event, record hashes, an export.

## 21. Implementation notes: settings surfaces (design wave 1, Settings & Providers)

Added to `Surfaces.swift` for Settings & Providers ([screens/SETTINGS.md](screens/SETTINGS.md)). The measures are the canvas board's; the values it does not have are named.

| Component | Notes |
|---|---|
| `SectionTile` | The tile of a settings section: radius 6, a solid color that is the same in both appearances, a medium glyph at 55% of the size in `onAccent`. 20 × 20 in the section list, 28 × 28 in the title of the shown section. The glyphs are not from the canvas (SETTINGS.md, section 5.4) |
| `SectionItem` | One item of a section list: 30 high, padding 8 at the sides, radius 7, gap 9; the tile and the name at 13 in `text`, one line. Chosen: background `accent` at 16%. The row is a plain view; the screen wraps it in a button |
| `SettingLine` | One setting inside a `.panelList` card: padding 10 by 14, gap 10, vertically centered, a `hairline` below unless it is the last. A label at 13 in `text`, 170 wide (the canvas grows label and value 1 to 2 from 170 and 240), a value that takes the rest and wraps, and an optional trailing part: an evidence pill, a note in the `meta` style in `text3`, or nothing. The value is any view: `SettingText`, a key cap, or kind tags that wrap |
| `SettingText` | The value of a setting as read-only text in one of three forms (`SettingForm`): a sentence at 13 in `text2`; a path or identifier at monospaced 11.5 in `text`; and, not on the canvas, a missing value ("Not reported", "Not built") at 13 in `text3` |

Not built from the board: the compact buttons of a line ("Change…", "Edit", "Add…", "Back Up Now", "Restore…"), the switch (32 × 19) and the segmented control. Each would change a setting, and the app has no request that writes one. The operational pills of a line ("Ready", "Limited", "Connected") have no source.

D3 (2026-11-17) added no component. A provider section shows the change of its executable with the existing `Banner` in its warning kind above the groups, and the version of the found executable with the evidence pill "Profiled" (Passed form) or a note; a refused executable has the evidence pill "Refused" (Failed form). The board's "Compatible" pill still has no source.

## 22. Implementation notes: plan surfaces (design wave 1, Execution Plan)

Added to `Surfaces.swift` for Execution Plan ([screens/EXECUTION_PLAN.md](screens/EXECUTION_PLAN.md), section 7). The measures are those of section 6 and of the canvas board; the values they do not have are named in EXECUTION_PLAN.md, section 7.4.

| Component | Notes |
|---|---|
| `TaskNode` | The task node of section 6 in the one form the core's data allows: 196 × 84, radius 10, `surface`, padding 9 by 11, gap 4. Line one: an identifier in monospaced 11.5 medium, `text2`. Line two: a title at 12.5 semibold, `textStrong`. Line three: a detail at 11.5, `text3`. Outline 1 pt `liftRing`; chosen: 2 pt `accent`. The node is a plain view; the screen wraps it in a button |
| `GraphEdge` | An edge between two points: a cubic curve that leaves and arrives horizontally, 1.5 pt in `dashedOutline`, solid or dashed (4 on, 4 off), with a filled arrowhead (6 by 7, `text3`) at its end. Hidden from assistive technology; the inspector names the dependency in words |
| `LaneBand` | A lane of the graph: radius 8, `fill2` with a `hairline` outline, and a header 24 high with a neutral square (8 × 8, radius 2, `tileNeutral`) and a name at 11.5 semibold, `text2` |

Not built from the board: the status pill of a node, its Running and Blocked outlines, the "Eligible" tag, the gate node, the per-repository color of a lane, the green, white and amber edge colors, the pipeline cells of section 5.4 and the lifecycle stepper of section 2.1. Each needs data the helper does not report: a state of a step, an evaluation of a dependency, a gate as an object, a color of a repository, a lifecycle stage.

## 23. Implementation notes: chat surfaces (design wave 2, Feature Chat)

Added to `Surfaces.swift` for Feature Chat and New Feature ([screens/FEATURE_CHAT.md](screens/FEATURE_CHAT.md)). Values not on the canvas boards are marked there.

| Component | Notes |
|---|---|
| `TextArea` | A multi-line text field: 13 pt, padding 8 by 10, `fill`, radius 8; it grows from `lines` (default 3) to 8 lines. Off screen its text or prompt is drawn in place, as `InputField` does |
| `ChatMessage` | One message of a transcript: a 22 pt round avatar with the author's initial (`accent` with `onAccent` for the lead, `fill` with `text2` for the human), the author at 13 semibold `textStrong` and a meta line at 11.5 `text3` beside it, then the content with a gap of 6. Padding 10 by 12, radius 10; the human's message stands on `fill2`, the lead's has no fill |

## 24. Implementation notes: project and run surfaces (design wave 3, M3)

No new component. Projects (`docs/design/screens/PROJECTS.md`) is built from the list row, fact line, input field, filter chip (as a toggle for a repository and the default provider), setting line and state block. The plan run on Execution Plan (`docs/design/screens/EXECUTION_PLAN.md`, section 8) uses the fact line, list row and kind tag; a step's state is a kind tag whose tone follows the status model (Integrated `ok`; Running, Validating, Accepted `info`; Review, Blocked `warn`; Conflict, Failed, Refused `danger`; the rest `neutral`). A task node of a running plan shows the step's key, title, provider and state instead of its identity. Off-screen renders: `execution-plan-run-…`, `execution-plan-run-narrow-…`, `projects-empty-…`, `-listed-…`, `-editing-…`, `-unreadable-…`, `-disconnected-…`.

## 25. Implementation notes: Dashboard surfaces (Liquid Glass, step 3)

Built in `macos/JojoApp/Sources/JojoApp/Dashboard.swift` on the Liquid Glass tokens, for `docs/design/screens/DASHBOARD.md`; the first-system surfaces of sections 14 to 24 are not used there. Each is in the `Gallery` of `DesignShellTests.swift`.

| Component | Notes |
|---|---|
| `KPITile` | Section 5.2: a button, radius 16, padding 14 16, gap 6; 24 × 24 tile (radius 7, fixed color, white glyph) and label (`bodyStrong`); value (`kpiValue`) with its unit (`secondary`, `text2`) on one baseline; one `secondary` line. Under the pointer `cardHover` |
| `ItemRow` | Section 5.3: min height 52, padding 8 14, gap 12; 28 × 28 tile (radius 8) with a glyph or two letters ("CC"); title and `sub` line cut to one line; one trailing pill; a chevron only when the row opens something; rows after the first have a 0.5 `sep` top line |
| `OperationalPill(_:tone:)` | The row form of section 2.2 (20 high, no glyph) with a word and hue of its own: "Proposed" blue, "Needed" and "Held" orange, "Review" purple, "Available" green. Added to `StatusComponents.swift` |
| `SectionHead`, `TextLink`, `GroupLabel` | Section 4.5: `sectionTitle`, count in `secondary` `text2`, a trailing 13 medium `accent` link; group labels in `label` `text3` |
| `SegmentedCapsule` | Section 5.6, drawn (a system picker does not render off screen): glass capsule with padding 2, segments 28 high, padding 0 13, 12 medium; the chosen one a `cardHover` capsule with a soft shadow. Used for the time range in the header and the project filter |
| `PipelineTable`, `StageCell` | Section 5.5: the feature column at least 200 and the rest, six stage columns at least 86, gap 8, padding 12 16; below its minimum width (796) the table scrolls sideways. A cell is an 8 pt dot and a word: Done `green`, Current `accent`, Running the running dot with its `accentTint` halo, NeedsYou `orange`, Blocked and Failed `red`, Stale and Unknown `gray`, Off a `dotOff` dot alone |
| `RunCard` | A card of "Running now" (radius 14, padding 14, gap 10): key (12 bold `text2`), title, elapsed; place line and the activity pill (green while running, orange otherwise). No progress bar: the core reports none |
| `BlockedRow`, `HistoryLine` | A row with a dot (red for a step, orange for a question) and "Why" (small secondary) or "Answer" (small primary); a history line (12, padding 7 0) with a dot in the event's hue, the core's sentence and the time |
| `QuietCard`, `SkeletonCard`, `Placeholder`, `NoteLine` | A section with nothing to list; Loading (two skeleton bars on a card); the placeholder of STATES.md, section 2 (44 × 44 tile, 15 bold title, a sentence at most 280 wide, small buttons) for the empty Dashboard; a one-line `secondary` `text3` note under a section |

The shell's header (`Header`) takes a screen's own controls (before the icon group) and its one primary action (after it), and the Dashboard's sub line names the day. Other screens pass neither and keep their header.

## 26. Implementation notes: form surfaces (Liquid Glass, step 4, New Feature)

Built in `macos/JojoApp/Sources/JojoApp/NewFeature.swift` for `docs/design/screens/NEW_FEATURE.md`.

| Component | Notes |
|---|---|
| `FieldLabel` | A form label in `label` style, `text3`, with "· optional" in normal case |
| `CheckChip` | A capsule 30 high, padding 0 12, 12 medium, with a 14 × 14 check box (radius 4; on, `accent` with a white check); on, `accentTint`, else `grayBg`. The board's repository chips |
| `ChoiceCapsule` | `SegmentedCapsule` (section 25) over values known at run time (the projects) |
| `DropZone` | Radius 14, padding 14 16, a 1.5 dashed `dotOff` outline, a 36 × 36 tile on `accentTint` with an `accent` glyph, a title and `sub` lines, one trailing control |

The text fields are the existing `TextArea`; "What happens next" is a glass panel (radius 16). The header's title and place can be a screen's own (`AppShell`'s `heading`).

## 27. Implementation notes: chat surfaces (Liquid Glass, step 4, Feature Chat)

Built in `macos/JojoApp/Sources/JojoApp/FeatureChat.swift` for `docs/design/screens/FEATURE_CHAT.md`; they replace section 23's `ChatMessage` on that screen.

| Component | Notes |
|---|---|
| `LifecycleStepper` | Section 2.1 from the core's stages: Done `green` with a `green` bar after it; Running and Current the `accent` dot with its `accentTint` halo and a semibold label; NeedsYou `orange`; Blocked and Failed `red`; Off `dotOff`; Evidence is always Off. The stage's word is its help text |
| `HumanBubble`, `LeadBubble` | The human's message on the right, 14 on `accent` with white text, radius 16, at most 78% wide; the lead's with a 26 round indigo "J" avatar and a `cardHover` bubble (radius 16, 0.5 `sep` ring; 2 `accent` when its question is focused) |
| `Inline` | 14 text whose `code` spans (Markdown backticks) are drawn in `mono` |
| `Receipt` | A capsule on `grayBg` under a reply, indented 34: a dot in its hue, a 12 `text2` sentence and a 12 semibold `accent` link |
| `KeyValueCard` | The inspector's key-value card (section 4.4): rows padding 9 12, a `mono` key, the value, a green "Done" pill or a `sub` note |
| `LineField` | A frameless one-line text field; off screen its text or prompt as plain text, because a text field does not draw there |
| `ChatHeaderActions`, `CapsuleTextButtonStyle` | The header's capsule group of 12 text buttons ("Create Spike", "Export Transcript") |


## 28. Implementation notes: feature screen surfaces (Liquid Glass, step 4, the six screens after Feature Chat)

Built in `macos/JojoApp/Sources/JojoApp/FeatureScreens.swift` for Specifications, the Approval Inbox, Execution Plan, Agent Runs, Source Control and Verification & Evidence (`docs/design/screens/`). These screens use only Liquid Glass surfaces (sections 25 to 28).

| Component | Notes |
|---|---|
| `FeatureScope` | The feature Feature Chat holds and its `Features` row: the header's place ("Sample › FT-3 Slugify from the shell › Specifications") and which rows a screen lists; everything when none is held. The sidebar's feature heading names it |
| `LifecycleCard` | Section 2.1 on a card (radius 14, padding 10 14) with the count pills; also Feature Chat's |
| `HeaderButtons`, `HeaderButton` | A header capsule group of 12 text buttons (`CapsuleTextButtonStyle`, section 27); a disabled one says why in its help text |
| `GlassInspector`, `LabeledSection` | The trailing glass panel (radius 18, padding 16, sections 18 apart); a `label` heading with an optional trailing link |
| `Definition`, `DefinitionCard` | The boards' `dl.card`: rows padding 9 12, a fixed-width `text2` key, the value (plain or `mono`) and an optional pill, a 0.5 `sep` between rows, radius 12 |
| `FactStrip`, `FlowRow` | Label over a 13 semibold value, wrapping cells at least 110 wide, on a card (radius 14, padding 12 14); a layout that wraps its children |
| `LetterTile` | A 26 × 26 tile (radius 7) with "PRD", "ADR", "PLAN" on the kind's color; gray for a superseded revision |
| `SelectableRow` | A leading-column row (radius 10, padding 8 10): chosen `accentTint`, under the pointer `cardHover` |
| `TintedNotice` | A tinted banner (radius 14, padding 12 14) with a title, a sentence and trailing buttons: revision, scope and stale notices |
| `CoreBanners`, `NotReported` | A lost or incompatible core and a 1.9 query the core does not list; the sentence where the board has a value without a source |
| `FeatureColumns`, `InspectorColumns` | Three columns (leading, main, inspector) from a wide width, two below, one at the narrowest; main and inspector beside each other or stacked |
| `approvalPill`, `documentTile`, `documentCode`, `boardTime`, `elapsedText` | Proposed blue, Approved green, Superseded gray, Rejected and Revoked red, Stale and Has findings orange; "09:21" today, "Nov 13 10:02" before; "12m 08s" |

Screen-own pieces: the grid nodes and gate nodes of Execution Plan (dashed outline for a gate, 2 `accent` ring when chosen), the dark console of Agent Runs (`#15161B`, text `#D6D9E4`), the unified and side-by-side diff of Source Control (two number columns, `redBg` and `greenBg` lines), the verdict tiles of Verification & Evidence.

Removed with the first-system screens (2026-11-17): `LeadingTile`, `LabeledField`, `Timeline`, `TimelineEntry`, `Console`, `ResultRow`, `EvidenceRow`, `DiffLine`, `DiffView`, `ChangeBadge`, `ChangedFileRow`, `RequestTile`, `RequestRow`, `ValueLine`, `DocumentRow`, `DocumentSection`, `TraceLine`, `TaskNode`, `GraphEdge`, `LaneBand`, `GroupHeader` (sections 14 to 19 and 22 describe them as they were). `ConsoleLine`, `FactLine`, `InputField`, `RequestedNotice` and the other surfaces still used by Recovery Center, Activity & Audit, Settings and Projects stay.

## 29. Implementation notes: system screen surfaces (Liquid Glass, step 6)

Projects, the Recovery Center, Activity & Audit and Settings & Providers (`docs/design/screens/`) use the surfaces of section 28 (`FeatureColumns`, `InspectorColumns`, `GlassInspector`, `LabeledSection`, `DefinitionCard`, `SelectableRow`, `CoreBanners`, which now names the protocol version of a missing query) and two more:

| Component | Notes |
|---|---|
| `MenuCapsule` (`Activity.swift`) | A capsule (30 high, `glassStrong`) that opens a menu of choices: Activity & Audit's "All types" and "All projects". Off screen, where a menu does not draw, its title and a chevron stand in |
| `LetterBadge` (`Projects.swift`) | A project's first letter on the project gradient, 26 in rows, 40 in the head |

Screen-own pieces: the Recovery Center's freshness strip (a card radius 12 with the Fresh or Stale pill, the inspection time and Reinspect) and its action card (rows 0.5 `sep` apart, a 180-wide button column); Activity & Audit's six-column record table with day groups on a card radius 14; Settings' glass section list (230 wide) with `SelectableRow` items and the Theme segmented control; Projects' repository rows with a health pill and the tinted missing-configuration card. Renders: `S13RecoveryCenterTests`, `S14ActivityTests`, `S15SettingsTests`, `S16ProjectsTests`.


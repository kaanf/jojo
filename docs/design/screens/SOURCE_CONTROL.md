# Jojo screens: Source Control

Transcribed on 2026-11-16 from the canvas board "Source Control" (drawn in dark only; two repositories, crew-api with the sealed candidate of T4 · Attempt 2 and crew-app with the working tree of a running attempt). Tokens, components and states refer to [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). Sections 1 to 4 are the canvas. Section 5 is what the app builds today and why it differs.

## 1. Frame

| Part | Spec |
|---|---|
| Toolbar | Title "Source Control". Breadcrumb "Workspace › Crew › FT-142 Lobby invite links › T4". Buttons at the trailing edge, each with a glyph: primary "Review Changes" (tooltip "Mark each file reviewed. Local only; nothing leaves this Mac."), secondary "View Scope Violations" and "Inspect CI" |
| Sidebar counter | "1" on the Source Control item. The board does not say what it counts; the first repository has one scope violation, the second one path waiting for a scope decision |
| Lifecycle strip | The strip of Agent Runs ([AGENT_RUNS.md](AGENT_RUNS.md), section 1): the compact stepper and the task pills "2 running", "3 blocked", "2 awaiting approval" |
| Columns | Three columns that fill the rest and wrap when narrow: a leading panel (from 250), main (flexible from 460) and a trailing panel (from 310). Both panels: background `#1E1E22`, a 1 pt edge (white at 6%) on the side that faces the main column |

## 2. Leading panel

**Repository control.** Padding 12 top, 14 sides. A segmented control (COMPONENTS.md has none yet): track radius 7, padding 2, gap 2, white at 7%; segments 22 high, radius 5, horizontal padding 10, 12 medium `text2`; the selected segment `#55555C` with white text and a shadow (0 1 2, black at 35%). One segment per repository with its number of changed files: "crew-api · 5", "crew-app · 3".

**Facts.** Padding 12 top, 14 sides, 8 bottom; gap 10. Each fact is a label at 11.5 `text3` over its value.

| Fact | Value |
|---|---|
| Branch | Identifier at 11.5 ("jojo/ft-142-t4-02"), gap 4 |
| Base, Candidate | Side by side, gap 14; identifiers at 11.5, gap 2 ("9c1e4b2", "a71f0de"; "working tree" for an attempt that still runs) |
| From | A link at 12 in the link color (`#7AAFFF`) to Agent Runs: "T4 · Attempt 2", "T5 · Attempt 1 (running)" |

Then, 4 below, the section heading "Changed files · 5" with a trailing 28 pt icon button "Reveal worktree in Finder".

**Changed files.** One row per file, gap 2, 12 below the last. Row: margin 8 at the sides, padding 6 by 8, radius 7, two lines, gap 3.

| Part | Spec |
|---|---|
| Change badge | 15 × 15, radius 4, one letter at 10.5 semibold on the status color at 16%: "M" Modified (`warn`), "A" Added (`ok`), "D" Deleted (`danger`) |
| File name | 12.5, one line, truncated; 8 after the badge |
| Counts | Identifier at 11 `text3`: "+48 −6", "+92", "−34" |
| Second line | Indented 23: the directory as an identifier at 10.5 `text3` ("src/main/kotlin/lobby", "." for the root), then, gap 6, an evidence pill when the scope of the path is not plain: Failed form "Out of scope" (tooltip "Outside approved scope"), Needs approval form "Pending" |
| Row out of scope | Background `danger` at 8%, inner ring `danger` at 35% |

The board draws no selected row.

| Repository | Files |
|---|---|
| crew-api | M LobbyController.kt +48 −6 · A InviteTokenRepository.kt +92 · D LegacyInvite.kt −34 · M InviteServiceTest.kt +64 −2 · M build.gradle.kts +2, out of scope |
| crew-app | M DeepLinkRouter.kt +31 −4 · M NavGraph.kt +6 −1 · M Info.plist +8, pending |

## 3. Main column

Padding 14 top, 18 sides, 18 bottom; gap 12.

### 3.1 Status strip

Background `surface`, radius 10, inner ring (white at 5%), padding 10 by 14; fields in a wrapping row (gap 10 by 26), each a label at 11.5 `text3` over a value (gap 3). "Location" sits at the trailing edge.

| Field | crew-api | crew-app |
|---|---|---|
| Local review | Operational pill, neutral ring dot, "Not reviewed" | The same form, "Attempt still running" |
| Scope | Evidence pill, Failed form, "1 violation" | Needs approval form, "1 pending" |
| Push | Operational pill, neutral, "Not performed" | The same |
| Pull request | Operational pill, neutral, "None" | The same |
| Location | A glyph and "Local candidate only" | The same |

### 3.2 Scope banner

Under the strip, when a path is outside the scope or waits for a decision: radius 9, padding 10 by 14, gap 12, a glyph, a title at 13 semibold over a message in `text2`, compact buttons at the trailing edge that wrap.

| Case | Colors | Title · message | Buttons |
|---|---|---|---|
| Violation | `danger` at 8%, ring `danger` at 28% | "1 change is outside the approved scope" · "build.gradle.kts was modified by T4 · Attempt 2. Scope allows lobby sources and tests only. Review it before any external action." | Secondary "View Scope Violations"; destructive "Revert file" (tooltip "Reverts this file in the candidate worktree only. Asks for confirmation.") |
| Pending | The warning form | "1 path is waiting for a scope decision" · "iosApp/Info.plist was requested by T5. The write was refused until you approve the scope expansion." | "Review request" |

### 3.3 Diff

A segmented control at the trailing edge (the shape of section 2), each segment with a glyph: "Unified", "Side by side".

Then a line with the path as an identifier at 12 in `text` ("src/main/kotlin/lobby/LobbyController.kt") and, trailing, the two ends as an identifier at 11.5 `text3` ("9c1e4b2 → a71f0de", "41d07aa → working tree").

**Unified.** The diff of COMPONENTS.md, section 10, with two number columns: background `#17171A`, radius 10, inner ring (white at 5%), monospaced 12, line height 22, vertical padding 8; old number (36 wide, right aligned, `#6E6E76`), new number (36), sign (16), code (whitespace kept; context `#B4B4BC`, changed lines `#E6E6EA`). A removed line has no new number, an added line no old one.

**Side by side.** Two equal columns in the same card with a 1 pt edge (white at 6%) between them: number (34 wide) and code; the old file on the left with removed lines tinted, the new file on the right with added lines tinted, and filler lines (white at 2%) where one side is shorter.

## 4. Trailing panel

Padding 14, gap 18. Three blocks, each a section heading (8 above its card) over a panel card (radius 10, inner ring).

**Scope validation.** One line per fact: padding 8 by 14, gap 10, a 1 pt edge below except after the last; the fact at 13 and a trailing value.

| Fact | Value |
|---|---|
| "Approved scope" | Identifier at 11.5 `text2`: "2 globs" |
| "Paths inside scope" | Evidence pill, Passed form, "4 of 5" |
| "Unexpected changes" | Evidence pill, Failed form, "1" |

**External actions.** One entry per action: padding 10 by 14, gap 6, an edge below. An entry is the action at 13 semibold with a trailing pill, then a sentence in `text2` or a button, then a note at 11.5 `text3` with a glyph.

| Action | Pill | Body |
|---|---|---|
| "Push ft-142-t4 @ 5d2c1aa" | Unknown form, "Outcome unknown" | "op-7f3a · from Attempt 1 · in flight when Jojo quit. Never retried automatically." · secondary "Inspect in Recovery" |
| "Push ft-142-t4 @ a71f0de" | Operational pill, neutral, "Not performed" | Primary "Authorize Push…", disabled (tooltip "Unavailable: 1 scope violation, and an earlier push to this ref has an unknown outcome.") · note "Blocked by the scope violation and the unresolved op-7f3a." |
| "Draft pull request" | Operational pill, neutral, "Not performed" | Secondary "Create Draft PR…" and "Open Pull Request", both disabled (tooltips "Unavailable: needs a confirmed push of this exact commit.", "Unavailable: no pull request exists yet.") · note "Separate authorization from push. Requires a confirmed push of a71f0de." |

Last line of the card, at 11.5 `text3`: "Each external action is authorized on its own, for an exact ref and commit, and recorded with an idempotency key."

**CI.** Heading with a trailing compact secondary "Inspect CI" (with a glyph). Lines in the form of "Scope validation".

| Line | Pill |
|---|---|
| "Candidate a71f0de" | Neutral form, "Not pushed" |
| "main @ 9c1e4b2 · build" | Passed |
| "main @ 9c1e4b2 · test" | Passed |
| "main @ 9c1e4b2 · contract-check" | Unknown form, "Not reported" |

## 5. What the app shows today

Built in `macos/JojoApp/Sources/JojoApp/SourceControl.swift` on the shared surfaces of COMPONENTS.md, sections 14 to 17. Before this the destination showed "Source Control is not built yet".

The screen has no load of its own. It shows the one attempt result the workspace holds, the same one Agent Runs loads with "Load Result" and Verification & Evidence loads by identity. It takes a plain value (`SourceControlState`: the `VerificationState` of that result and the base) and shows only what the helper reports:

- the changed paths of the sealed candidate, with the kind of change and the scope the core gives each, the core's verdict on the write scope and the base the candidate was compared with (`CandidateChanges`; the workspace now keeps `base` of that answer as `candidateBase`);
- the diff from the base to the sealed candidate (`CandidateDiff`);
- the attempt, the candidate digest, the host validation state and the decision (`Candidate`);
- the helper connection and the admission value.

Verification & Evidence keeps its changed files, its diff and Accept and Reject; nothing was moved. A part of the canvas without a source says so on screen; nothing is filled with an example.

### 5.1 Mapping

| Canvas | App today |
|---|---|
| Toolbar buttons, breadcrumb to project, feature and task | Not built. The shell breadcrumb stays "Jojo › Source Control" |
| Sidebar counter | The changed paths of the loaded attempt that the core put outside the write scope (`OUT_OF_SCOPE` and `BASELINE`), in `danger`; no counter when there are none or nothing is loaded. Chosen because the "1" of the board matches its one violation, not its five files, and because the other counters also count what needs attention |
| Lifecycle strip and task pills | Not built: features, lifecycle stages and tasks are not reported |
| Repository control | Not built: repositories are not reported, and one attempt is loaded at a time |
| Facts | Branch: "Not reported" · Base: the first twelve characters of the base, "Not loaded" until the changed paths arrive · Candidate: the first twelve characters of the digest, "None" without a sealed candidate · From: "Attempt 1a2b3c4d1a2b" (the first twelve characters), a link that opens Agent Runs. The full values are the tooltips |
| "Reveal worktree in Finder" | Not built: the worktree path is not reported |
| Changed files | "Changed files · n" and one changed-file row per path (COMPONENTS.md, section 17). Badge from the core's word (5.2). Counts: the added and removed lines of that path's section of the loaded diff; left out when the diff has no section for the path or the section has no counted line. The scope pill is drawn for every path that is not in scope, with the labels of Verification & Evidence ([VERIFICATION.md](VERIFICATION.md), section 4.2); a path out of scope or in the protected baseline gets the tinted row. Without rows: the sentence Verification & Evidence shows ("Reading the changed paths…", "No changed paths.", "No sealed candidate.") |
| (not on the canvas) Selection | Choosing a row shows that path's section of the diff and marks the row; choosing it again, or "Show All Files" above the diff, shows the whole diff. A new attempt clears the choice |
| Status strip | Decision (the text of Verification & Evidence: "None recorded", or the core's word) · Write scope (5.3) · Host validation (the evidence pill of the core's state) · at the trailing edge Location: a glyph and "Local candidate". No "Local review": the core records one decision per candidate, not a review per file. Push and Pull request are in the trailing panel |
| Scope banner | Write scope `FAIL`: danger banner "1 change is outside the approved scope" ("n changes are …") · the first three such paths, "and n more", then "The core's verdict on the write scope is Failed. There is no command to revert a path." with a compact secondary "View Scope Violations" that chooses the first such path. Without a marked path: "The write scope check failed" · "No listed path is marked. The core's verdict on the write scope is Failed." Write scope `UNKNOWN` on a sealed candidate: warning banner "The write scope is unknown" · "The core gave no verdict (reason). Unknown is never shown as in scope." No "Revert file", no pending form: the core has no command to revert a path and reports no scope request |
| Unified / Side by side | Not built. The diff is unified, with the one number column of Verification & Evidence |
| Diff heading | The chosen path as an identifier at 12 in `text`, or "All changed files" at 12 semibold `text2`; trailing, the two ends as an identifier in `text3` ("9c1e4b2a71f0 → c4c4c4c4c4c4", "base → sealed candidate" while either is missing), then "Show All Files" and "Show All n Lines" (compact ghost) |
| Diff | The diff of COMPONENTS.md, sections 10 and 16. The first 300 lines are drawn; "Show All n Lines" draws the rest. Empty: "No diff loaded." A chosen path without a section: "The loaded diff has no section for this path." |
| Scope validation | A card with one evidence row per fact (5.3) and a footnote: "The patterns of the approved scope are not reported; the core gives one verdict for the candidate and a scope for each path. There is no command to revert a path or to widen the scope." |
| (not on the canvas) Review | A card: "Validation, accept and reject are on Verification & Evidence.", the facts Validation and Decision, and a secondary "Open Verification & Evidence" |
| External actions | A card with one sentence: "Jojo has not pushed or published this candidate. The core has no push or pull request command yet." and a footnote: "Not reported: the branch and commit of the candidate, external actions and their outcomes, pull requests and CI results." No entries, no buttons: push and pull request belong to milestone M5 |
| CI | Not built; named in the footnote above |

### 5.2 Change badge

The helper sends the kind of change as a word.

| Kind (the core's word) | Letter | Tone | Tooltip |
|---|---|---|---|
| `Added` | A | `ok` | Added |
| `Modified` | M | `warn` | Modified |
| `Deleted` | D | `danger` | Deleted |
| `TypeChanged` | T | `warn` | Type changed |
| anything else | ? | neutral | The core's word |

### 5.3 Scope

| Place | Shows |
|---|---|
| Strip, "Write scope" | The evidence pill of the core's verdict: Passed form "In scope"; Failed form "1 violation" ("n violations"; "Failed" when no path is marked); Unknown; Not run |
| Card, "Write scope" | The same verdict with its plain label, and the core's reason as the note when it gave one |
| Card, "Paths inside scope" | "4 of 5": Passed form, or Not run form when none is inside |
| Card, "Outside the approved scope" | The number of `OUT_OF_SCOPE` paths: Failed form, or Not run form "None" |
| Card, "Protected baseline" | The number of `BASELINE` paths, Failed form; left out at zero |
| Card, "Scope unknown" | The number of paths with any other scope, Unknown form; left out at zero |

The three count rows are drawn only when there are changed paths. The scope of a path is read with `VerificationState.scope(_:)`; the mapping exists once.

### 5.4 Navigation and states

- Empty (nothing loaded): the Empty form with "No attempt loaded" · "Source Control shows the changed paths, the write scope and the diff of the loaded attempt. Choose one in Agent Runs and use Load Result, or load it by its identity in Verification & Evidence." and two compact secondary buttons, "Open Agent Runs" and "Open Verification & Evidence". The panels are not drawn.
- Disconnected (`channelLost`, `ended`, `launchFailed`): danger banner "Jojo core isn't responding" · "Showing the last known state, read-only. No actions can be requested." There is no "Reconnect" yet.
- Incompatible protocol: danger banner "Jojo core speaks another protocol version" · "Read-only. No actions can be requested."
- Recovery required (admission reported and not `Open`): danger banner "Execution admission is closed" · "No new attempt starts until you review the inspection." with "Open Recovery Center".
- Loading: the workspace does not say that a read is pending. The changed files say "Reading the changed paths…" until the core's scope verdict arrives; the last loaded attempt stays until the new one answers.
- Unknown outcome: a write scope the core could not check has the Unknown pill and the warning banner of 5.1; it is never drawn as in scope.
- Requested: the screen sends no command. The toolbar's "Requested" pill shows a command sent from another screen.
- Error and Stale: a failed read is reported in the status line under the screen ("Candidate unavailable: …"); a failed `Candidate` read unloads the attempt, and a failed read of the changed paths leaves the scope Unknown with the core's code as the reason.
- Permission denied has no source on this screen.
- Width of the content: three columns from 1020; from 710 the leading panel and the main column, with the blocks of the trailing panel under the diff; below 710 one column, the facts and the changed files first. The window's opening size gives two columns.

### 5.5 Values that are not on the canvas

| Point | Value in the app |
|---|---|
| Panels | Background `sidebar`, edge `sidebarEdge`; leading 250 wide, trailing 310 |
| Changed-file row, change badge | COMPONENTS.md, section 17 |
| Chosen row | Background `fill`; the ring of a row out of scope stays |
| "From" link | 12 pt in `icon` (the canvas link color is the dark value of that token) |
| Fact that is not reported or not loaded | The words in `text3` |
| "Location" glyph | `internaldrive` |
| Empty state glyph | `arrow.triangle.branch` |
| Lines drawn before "Show All" | 300, as on Verification & Evidence |

# Jojo screens: Agent Runs

Transcribed on 2026-11-16 from the canvas board "Agent Runs" (drawn in dark only; six attempts, one selected at a time). Tokens, components and states refer to [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). Sections 1 to 3 are the canvas. Section 4 is what the app builds today and why it differs.

## 1. Frame

| Part | Spec |
|---|---|
| Toolbar | Title "Agent Runs". Breadcrumb "Workspace › Crew › FT-142 Lobby invite links › T4 › Attempt 2". Secondary buttons at the trailing edge: "View Logs" (⌘L, tooltip "Open full log for the selected attempt"), "Inspect Attempt" (⌘I, tooltip "Show identity, supervision and evidence") and "Inspect Recovery" |
| Sidebar counter | "2" on the Agent Runs item |
| Lifecycle strip | Under the toolbar, full width: background `#1D1D20`, a 1 pt edge below (white at 6%), padding 9 by 18, the two groups pushed apart and wrapping. Leading: "Lifecycle" at 11.5 `text3` and a compact stepper (below). Trailing: "Tasks" at 11.5 `text3` and standalone pills "2 running" (Running form), "3 blocked" (Blocked form), "2 awaiting approval" (Awaiting approval form), gap 6 |
| Compact stepper | Steps Discovery, Specification, Planning, Execution, Verification, Evidence, gap 7. A step is a 14 pt circle and a label at 12, gap 6. Done: circle `ok` at 16% with a check, label medium `text2`, connector 16 × 1 in `ok` at 40%. Current: 1.5 pt `info` ring with a 6 pt dot, label semibold `textStrong`. Not reached: 1.5 pt ring (white at 20%), label medium `text3`, connector white at 12% |
| Columns | Three columns that fill the rest and wrap when narrow: attempt list (flexible from 232), detail (from 460), inspector (from 320) |

## 2. Attempt list and detail

### 2.1 Attempt list (leading panel)

Background `#1E1E22`, a 1 pt edge (white at 6%) on its trailing side.

1. Section heading "Attempts · FT-142" (padding 14 top, 16 leading, 14 trailing, 4 bottom) with a trailing compact ghost button "Filter".
2. One group per task. Group header: padding 12 top, 16 sides, 4 bottom, baseline aligned with gap 6; the task identifier at 11.5 medium `text` ("T4") and its name at 12 `text2` ("Invite endpoint").
3. One item per attempt, newest first: margin 0 by 8, padding 8 by 10, radius 8, gap 3. Line one: title at 12.5 semibold ("Attempt 2") and a trailing operational pill. Line two at 11.5 `text3`.
4. The selected item has a background of `accent` at 16% and a 1 pt inner ring of `info` at 40%.

| Pill | Form | Second line |
|---|---|---|
| Running | Running (info, dot with halo) | "Claude Code · 12m 08s" |
| Completed | Completed (ok, plain dot) | "Exited 0 · validated" |
| Interrupted · settled | Recovery required (danger, ring with a dot) | "Confirmed dead · continued fresh" |
| Unknown | Recovery required | "Process identity not confirmed" |

### 2.2 Detail (center)

Padding 14 top, 18 sides, 18 bottom; gap 16.

**Header.** Leading, baseline aligned with gap 10: title at 17 semibold `textStrong`, tracking -0.01 ("T4 · Attempt 2"), and the task name in `text2` ("Invite endpoint + token service"). Trailing: a segmented control "Timeline · Logs · Evidence · Claims". Only Timeline is drawn.

**Segmented control.** Track: white at 7%, radius 7, padding 2, gap 2. Segment: height 22, padding 0 by 10, radius 5, 12 medium. Selected: background `#55555C`, white text, shadow 0 1 2 black at 35%. Others: no background, `text2`.

**Summary strip.** Background `surface`, radius 10, inner ring, padding 12 by 14; fields in a wrapping row (gap 14 by 28), each a label at 11.5 `text3` over a value (gap 3): Attempt (identifier, "att_142_t4_02") · Process (pill: "Running · supervised", "Exited · confirmed by OS", "Exited 0 · confirmed by OS", "Unknown · not stopped") · Started ("09:21:04", "Yesterday 16:02") · Duration ("12m 08s", "Unknown") · Base commit (identifier).

**Event timeline.** Section heading, then a panel card with padding 8 by 14. One entry per event, oldest first: padding 5 vertical, gap 12, top aligned; a 9 pt dot (4 below the top), the sentence in `text` at 13, and the time as an identifier at 11 `text3`. A 1 pt connector (white at 10%) runs from under each dot to the next; the last entry has none.

| Dot | Meaning | Examples |
|---|---|---|
| `ok` | Recorded | "Attempt admitted by core · claims granted", "Edited LobbyController.kt +48 −6", "Process exited 0", "Validated in V-2" |
| `info` | In progress or waiting on the human | "Running unit tests", "Requested scope expansion: iosApp/Info.plist" |
| `danger` | Supervision or identity lost | "Jojo quit unexpectedly · supervision lost", "RI-12: PID reused, original process unconfirmed" |

**Terminal output.** The section takes the remaining height, at least 220. Heading with a trailing search field (height 26, radius 6, white at 7%, placeholder "Search output", 150 wide) and two 28 pt icon buttons: "Copy output" (⇧⌘C) and "Save log". Console: background `#111113`, radius 10, inner ring (white at 5%), padding 12 by 14, monospaced 12 with a line height of 1.75, selectable, wrapping, horizontal scroll.

| Line | Color |
|---|---|
| Command ("$ ./gradlew test …") | `#E6E6EA` |
| Plain output | `text3` |
| Passed | `ok` |
| Running | `info` |
| Notice from Jojo ("[supervision lost at 08:57:31]", "[output truncated at 15:40:02]") | `warn` |

## 3. Inspector (trailing panel)

Same background as the list, with the edge on its leading side.

**Header.** Padding 14 top, 16 sides, 12 bottom, gap 6, a 1 pt `hairline` below: "Agent attempt · T4" at 11 medium `text3`, the attempt identifier at 15 semibold `textStrong`, then the attempt's pill at standalone size.

**Section.** Padding 12 by 16, gap 8, `hairline` below (none after the last); title at 11.5 semibold `text3`. Facts are a two-column grid: labels 104 wide in `text3`, values flexible and allowed to break anywhere; row gap 7, column gap 10, 12.5 pt, baseline aligned.

| Section | Content |
|---|---|
| Provider session | Provider ("Claude Code") · Session (identifier "ses_8f21…c4e0", or "Not resumable (CLI limitation)") · Resumable ("Yes, CLI session resume", "Yes, but not used", "No") |
| Process supervision | State (pill, as in the summary strip) · PID and Process group (identifiers: "61204", "48121 (absent)", "50233 (reused by another program)", "Unknown") · Liveness ("Heartbeat 2s ago", "Last heartbeat 08:57:31", "n/a") · Started · Stopped ("Not stopped", "08:57:31 (crash) · exit confirmed 09:04:12", "Unknown") |
| Workspace | Worktree (identifier path, with "(preserved)" or "(preserved, changed after snapshot)") · Branch · Base commit |
| Continuation provenance | One paragraph in `text2`: "Fresh continuation of attempt 1 (confirmed dead). History not replayed; validated output imported: none.", "First attempt." |
| Resource claims | One line per claim, baseline aligned with gap 8: the path as an identifier and its mode at 11.5 `text3` ("write", "released 09:05", "quarantined") |
| Checkpoints | One line per checkpoint, gap 8: its name as an identifier in `info` ("CP1"), the sentence, and the time as an identifier at 11 `text3` |
| Execution evidence | One line per record, gap 8: the name and a trailing tag (radius 4, 18 high, 11.5 medium, with a glyph). Event log and Terminal output: "Recording" (`warn` tint), "Recorded", "Truncated". Exit status: "Not exited" (neutral), "Exit 0", or the Unknown form (dashed outline, "?") with "No exit code (crash)" or "Unknown". Verification: "V-2". Then a compact secondary "View Evidence" |
| Recovery status | `text2`: "None required", "Inspected in RI-12 · settled as Continue Fresh at 09:06", "Held in Recovery Center · process identity unknown"; with a compact secondary "Inspect Recovery" when there is something to inspect |
| Actions | Gap 10. "Request Safe Stop" (secondary, tooltip "Asks the agent to finish its current step and exit") over a footnote "Asks the agent to finish its step and exit. The attempt stays open until its output is validated." · "Cancel Attempt…" (destructive: `danger` at 10%, `danger` text, 1 pt `danger` at 35%; tooltip "Terminates the process group and closes the attempt. Asks for confirmation.") over "Terminates the whole process group and closes the attempt as cancelled. The worktree is kept." · a notice well (padding 8 by 10, radius 8, white at 4%, 11.5 `text2`): "A stopped process is not a successful attempt. Exit is recorded only when the OS confirms it." |

Unavailable actions: both buttons are disabled (opacity 0.42), keep their titles, and one footnote replaces the sentences and the well: "Process has already exited." or "Process state is unknown. Resolve it in Recovery Center first."

## 4. What the app shows today

Built in `macos/JojoApp/Sources/JojoApp/AgentRuns.swift` on the shared surfaces of COMPONENTS.md, sections 14 and 15. The screen takes a plain value (`AgentRunsState`) and shows only what the helper reports:

- the snapshot's attempt rows: the identity and the scheduler's record text (`scheduler_attempt`; an `attempt` row is read the same way), and the worktree record of the same identity (`worktree_admin`);
- the recovery rows, to say that an attempt is on hold and whether its processes are proven gone;
- the result of one attempt (`Candidate` query: sealed, outcome, reason, the provider's own closing words, candidate digest, number of log records, validation state, decision) and its first 64 log records. The workspace holds one loaded result at a time, shared with Verification & Evidence, so it is read when the human asks for it ("Load Result"), never on selection;
- the helper connection, the admission value and whether a command is pending.

A part of the canvas without a source says so on screen; nothing is filled with an example.

### 4.1 Mapping

| Canvas | App today |
|---|---|
| Toolbar buttons, breadcrumb to task and attempt | Not built. The shell breadcrumb stays "Jojo › Agent Runs" |
| Sidebar counter | The number of attempt rows in the snapshot |
| Lifecycle strip and task pills | Not built: features, lifecycle stages and tasks are not reported. A footnote under the list says so |
| List heading and groups | "Attempts · n", one flat list in the snapshot's order. No task groups, no "Filter" |
| Item title | The first eight characters of the identity ("1a2b3c4d"); the heading already says "Attempts". The detail and the inspector say "Attempt 1a2b3c4d" |
| Item pill and second line | See 4.2 |
| Detail header | "Attempt 1a2b3c4d"; no task name. The segmented control is not built: Logs, Evidence and Claims have no data of their own, and the one view shows everything reported |
| Summary strip | A wrapping row (gap 14 by 28): Attempt (full identity) · State (the pill of 4.2) · Scheduler record (the core's text, "(partial)" when cut) · Worktree record (the core's text, or "None reported"). A footnote names what is missing: start time, duration, base commit |
| Event timeline | "Recorded facts": the records the core reports for the attempt, in lifecycle order and without times (4.3). A footnote says that events and their times are not reported per attempt |
| Terminal output | The loaded log records, one line each as "kind: text", all in the plain color. The workspace reads the first 64; beyond that a last line in `warn` says how many later records are not loaded. Before the result is loaded: "Not loaded."; a result without records: "No log records."; records not yet read: "Reading the log records…". No search, copy or save. The heading's trailing text is the count ("3 of 3 records") |
| Inspector header | "Agent attempt", "Attempt 1a2b3c4d", the pill at standalone size |
| Provider session, PID, process group, liveness, start and stop times, worktree path, branch, base commit, continuation, claims, checkpoints | One closing section "Not reported yet" that names them |
| Process supervision | "Supervision": State (pill) · Scheduler record · Worktree record |
| Execution evidence | From the loaded result, as facts in the section's grid instead of lines with trailing tags: Result (evidence pill: Passed form "Succeeded", Failed form "Failed", Incomplete form "Interrupted", Not run form "Not sealed" or the core's text for "Cancelled" and anything else) · Provider said (its closing words, "Nothing") · Log ("n records") · Candidate (first twelve characters of the digest, "None") · Validation (evidence pill: Passed, Failed, Not run, Unknown) · Decision (the core's text). "View Evidence" opens Verification & Evidence, which shows the same loaded attempt; a compact ghost "Reload" reads the result again. Without a loaded result: one sentence ("The result of this attempt is not loaded. One result is loaded at a time; Verification & Evidence shows the same one.") and a compact secondary "Load Result" |
| Recovery status | "None reported by the last inspection." or "Held in Recovery Center · every process proven gone" / "… · process state unknown", with "Inspect Recovery", which opens the Recovery Center |
| Request Safe Stop | Drawn disabled with "The core has no safe-stop command yet." |
| Cancel Attempt… | Sends `CancelAttempt` after a system confirmation dialog (the dialog of COMPONENTS.md, section 7, is not built). Sentence: "Asks the core to stop the attempt and record it as cancelled. It counts as cancelled only after the core seals a result." |
| Unavailable cancel | Sealed result: "The result is already sealed." · On hold: "On hold: resolve it in Recovery Center first." · Not connected: "Unavailable: the core is not connected." · Command pending: "Unavailable while a command awaits the core." |
| Notice well | The canvas sentence, shown while cancel is available |

### 4.2 Attempt state

The core does not report a lifecycle state per attempt, so the pill is derived, in this order:

| Known about the attempt | Pill | Second line in the list |
|---|---|---|
| A recovery row, every process proven gone | "Confirmed dead", Recovery required form | "Held in Recovery Center" |
| A recovery row otherwise | "Unknown", Recovery required form | "Held in Recovery Center" |
| Loaded result, sealed, outcome `Succeeded` | "Succeeded", Completed form | The result's reason |
| … `Failed` | "Failed", Failed form | The result's reason |
| … `Interrupted` | "Interrupted", Recovery required form | The result's reason |
| … `Cancelled` or any other text | The core's text, Idle form | The result's reason |
| Loaded result, not sealed | "Not sealed", Idle form | "No sealed result yet" |
| Nothing loaded | "Not loaded", Idle form | "Scheduler record: RESERVED" (the core's text) |

"Running" is never shown: an attempt without a sealed result may be running, starting or stopped, and the core does not say which.

### 4.3 Recorded facts

One entry per record the app holds, in this order; an entry without a record is left out.

| Entry | Dot |
|---|---|
| "Scheduler record: RESERVED" | `ok` |
| "Worktree record: CREATING" | `ok` |
| "Held for recovery: every process proven gone" / "… process state unknown" | `danger` |
| "Result sealed: Succeeded · reason" | `ok`; `danger` for Failed and Interrupted; neutral otherwise |
| "No sealed result yet" (result loaded, not sealed) | neutral |
| "Validation: PASS" (not shown for NOT_RUN) | `ok`; `danger` for FAIL; neutral for UNKNOWN |
| "Decision: …" (not shown for NONE) | neutral |

### 4.4 Navigation and states

- Disconnected (`channelLost`, `ended`, `launchFailed`): danger banner "Jojo core isn't responding" · "Showing the last known state, read-only. No actions can be requested." Cancel and Load Result are disabled. There is no "Reconnect" yet.
- Incompatible protocol: danger banner "Jojo core speaks another protocol version" · "Read-only. No actions can be requested."
- Recovery required (admission reported and not `Open`): danger banner "Execution admission is closed" · "No new attempt starts until you review the inspection." with "Open Recovery Center".
- Loading (connected or starting, no snapshot yet): "Loading attempts…" and three skeleton bars.
- Empty (a snapshot without attempt rows): the Empty form with "No attempts yet" · "An attempt appears here when an approved plan step is started." and a secondary "Open Execution Plan".
- Lost before the first snapshot: the centered block "No snapshot was loaded" · "The core stopped answering before it reported its attempts."
- Requested: the toolbar's "Requested" pill; cancel is disabled while a command is pending.
- Unknown outcome: an attempt on hold whose processes are not proven gone has the "Unknown" pill and no cancel; it is resolved in the Recovery Center.
- Error: a failed snapshot or result read is reported in the status line under the screen ("Snapshot unavailable: …", "Candidate unavailable: …"); the last rows stay visible.
- Stale and Permission denied have no source on this screen.
- Content under 960 wide (the window at its opening size): the inspector's sections move below the detail as one panel card. Three columns appear in a wider window.

### 4.5 Values that are not on the canvas

| Point | Value in the app |
|---|---|
| Side panel background | `sidebar` token; edge `sidebarEdge` (as in the Recovery Center) |
| Panel widths | List 240, inspector 280, detail takes the rest |
| Selected item, light | Same formula as dark: `accent` at 16%, ring `info` at 40% |
| Timeline connector | `hairline` (canvas: white at 10%) |
| Neutral timeline dot | A 1.5 pt `text4` ring, no fill |
| Console background | `#111113` in dark, `#F1F1F4` in light; ring `cardRing` |
| Console text | Command `text`, plain `text2` (canvas `text3`; raised for contrast on the light console), notices in the status colors |
| Notice well background | `fill2` (canvas white at 4%) |
| Identifiers | The `mono` text style (11 pt) |
| Glyphs (SF Symbols) | Load Result `arrow.down.circle`; Request Safe Stop `stop.circle`; Cancel Attempt `xmark.circle`; empty state `play.circle` |

## 5. Repairs and reviews (M4)

Not on the canvas; D2 asked for them (`docs/roadmap/DESIGN_ROADMAP_DIFFERENCES.md`, D2 and E10). The screen reads the `Repairs` query while it is shown, and again with each new snapshot. An attempt the answer lists is a plan step's:

- List row: the pill shows the step's state for the step's newest attempt: "Repairing" (the Repairing form of COMPONENTS.md, section 2.2), "Reviewing" (the Running form) or "Needs human" (the Blocked form); a recovery hold still outranks it. The second line is "<step key> · repair 1 of 2" or "<step key> · first attempt".
- Detail: a "Plan step" card after the summary: step, repair ("First attempt", "Repair 1 of 2"), review ("Approved by Claude Code", "Rejected by Codex", "No verdict from …", "… is reviewing", toned ok, danger, warn or info), why it was repaired, the reviewer's notes, the strongest objection, and the hold reason in a notice well. A footnote says that a failed step is repaired at most twice, then waits, and that a review approves nothing.
- Inspector, Supervision: "Repair" and "Review" facts.

There is no command here: accept and reject stay on Verification & Evidence, and nothing restarts a held step. Snapshots: `agent-runs-repairing`, `agent-runs-rejected`, `agent-runs-needs-human`.

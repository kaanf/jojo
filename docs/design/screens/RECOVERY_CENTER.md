# Jojo screens: Recovery Center and launch review

Transcribed on 2026-11-16 from the canvas boards "Recovery Center" and "Recovery Center · launch review" (both drawn in dark only, 1440 × 1000). Tokens, components and states refer to [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). Sections 1 to 3 are the canvas. Section 4 is what the app builds today and why it differs.

## 1. Shared parts of both boards

| Part | Spec |
|---|---|
| Toolbar | Title "Recovery Center". Breadcrumb "Workspace › Recovery Center", plus "› Launch review RI-12" on the launch review. Secondary buttons at the trailing edge: "Reinspect All" (tooltip "Read-only inspection of every held item") and "Inspect Recovery" on the center; only "Inspect Recovery" on the launch review |
| Sidebar counter | "5 held" on the Recovery Center item |
| Section heading | 12 semibold `text2`, tracking 0.01, minimum height 22, 8 below; an optional trailing slot |
| Panel card | Background `surface`, radius 10, 1 pt inner ring (white at 5%), content clipped. Rows inside are separated by a 1 pt `hairline`; the last row has none |
| Identifier | 11.5 regular `text`, tracking -0.01 |
| Footnote | 12 pt info glyph and one sentence at 11.5 `text3`, line height 1.4, gap 6, top aligned |

## 2. Recovery Center

Three columns that fill the content area and wrap when it is narrow.

### 2.1 Held list (leading panel)

Flexible from 260 wide. Background `#1E1E22`, a 1 pt edge (white at 6%) on its trailing side.

1. Heading "Held · 5" (padding 12 top, 16 leading, 12 trailing, 6 bottom).
2. One item per held subject, gap 2. Item: margin 0 by 8, padding 9 by 10, radius 8, gap 4, three lines:
   - kind at 11 `text3` ("External side effect", "Prior attempt", "Quarantined claim") and, trailing, an operational pill;
   - title at 12.5 semibold `text` ("Push ft-142-t4 @ 5d2c1aa", "J2 · Attempt 1", "shared/lobby/share/**");
   - context at 11.5 `text3` ("Crew · op-7f3a", "Jojo · FT-007 · Recovery inspector view model").
3. The selected item has a background of `accent` at 16% and a 1 pt inner ring of `info` at 40%.
4. Heading "Settled today" (padding 14 top, 16 sides, 6 bottom), then one line per settled subject: its name in `text`, the outcome at 11.5 `text3` ("T4 · Attempt 1" · "Continued fresh"). Each links to Agent Runs.

Pills in the list:

| Label | Form |
|---|---|
| Outcome unknown, Confirmed dead, Unknown, Quarantined | Recovery required (danger, ring with a dot) |
| Live · unsupervised | Running (info, dot with halo) |

### 2.2 Detail (center)

Flexible from 460 wide. Padding 18 top, 22 sides, 24 bottom; gap 16.

**Header.** Leading column, gap 5: context line at 12 `text2` ("Prior attempt · Jojo · FT-007 · Recovery inspector view model"), title at 20 bold `textStrong` with tracking -0.02 ("J2 · Attempt 1"), then a row with gap 8: the status pill (standalone size), a freshness tag and a note in `text3`. Trailing: secondary "Reinspect" (tooltip "Read-only. Collects fresh facts from the OS, Git and the remote.").

| Freshness | Tag | Note |
|---|---|---|
| Fresh | `okTint` background, `ok` text, 12 pt glyph, radius 4, 11.5 medium, 18 high | "inspected 09:40 · 2 min ago" |
| Stale | `warnTint` background, `warn` text, same shape | "inspected 09:04 · 38 min ago · reinspect before acting" |

**Inspected facts · read-only.** A panel card with padding 12 by 14 holding a two-column grid: labels 130 wide in `text3`, values flexible and allowed to wrap; row gap 7, column gap 10, 12.5 pt, baseline aligned. A value is an identifier, plain text, or a tag (`ok`, `warn` or `danger` tint with a glyph).

| Subject | Rows |
|---|---|
| Prior attempt | Attempt · Process ("pid 47710 · absent") · Process group · OS evidence (tag) · Worktree (tag) · Snapshot (tag) · Evidence (tag) · Session · Claims. A live attempt has Supervision ("Lost at 08:57 · not reattachable") instead of OS evidence |
| External side effect | Operation ("git push origin jojo/ft-142-t4") · Idempotency key · Local result (tag "No exit recorded") · Remote ref (tag "Not inspected since 09:04"), then a paragraph "How this resolves" |
| Quarantined claim | Held by · Mode · Blocks |

**Recovery actions.** A panel card with four rows, always in this order. Row: padding 10 by 14, gap 12, top aligned; a fixed 180 wide column with the button, then one sentence.

| Button | Sentence when available |
|---|---|
| Validate Output… | "Checks the preserved worktree against the task's verification requirements. Read-only for the attempt." |
| Continue with History… | "Starts a new attempt that resumes the provider session with its history." |
| Continue Fresh… | "Starts a new attempt from the preserved worktree with a new session." |
| Hold | "Keeps everything as is. Claims stay quarantined." |

- Available: the first action is the primary button, the others secondary; the sentence is in `text2`. The button's tooltip repeats the sentence.
- Unavailable: the button is disabled (opacity 0.42), loses its ellipsis, and the sentence in `text3` says why and what would change it:
  - "Unavailable while process state is unknown. Needs OS evidence that the process exited."
  - "Unavailable: the process is alive but unsupervised. Actions open only after the OS reports its exit."
  - "Unavailable: external outcome unknown. Reinspect the remote; Jojo never retries automatically."
  - "Unavailable: claim owner unconfirmed. Resolves when its attempt is settled."
- Hold is always available.

**Footnote** under the actions:

- "Eligible because the OS shows no process with the recorded identity, and worktree, snapshot and evidence check out."
- "Only Hold is available. Confirming in the UI never substitutes for OS evidence that a process has exited."

### 2.3 Admission and inspection (trailing panel)

Flexible from 290 wide, same background as the held list with the edge on its leading side. Padding 14, gap 16.

1. **Execution admission.** Panel card, padding 12 by 14, gap 10: an "Open" pill (ok, plain dot, standalone size) and the session identifier ("S-0412") in `text2`; a sentence in `text2` ("Started 09:05 by you after reviewing inspection RI-12. Held items stay held."); compact secondary buttons "Pause Admission" (tooltip "New attempts stop being admitted. Running attempts continue.") and "View RI-12", which opens the launch review.
2. **Last inspection.** Panel card of rows (padding 8 by 14): a label and a trailing number as an identifier, `warn` when it needs attention. "Attempts examined 6 · Worktrees intact 5 / 5 · Snapshot mismatches 1 · Unknown external effects 1 · Stale approvals 1".
3. Footnote: "Recovery inspection never changes anything. Only the actions you choose do, and the core checks eligibility again."

## 3. Launch review

Shown while admission is closed. One centered column, at most 980 wide, padding 18 top, 22 sides, 26 bottom, gap 16.

1. **Danger banner** (COMPONENTS.md, section 11), without a button: "Execution admission is closed" · "Jojo quit unexpectedly at 08:57. No new work starts and nothing continues until you review a fresh inspection and start a new session."
2. **Inspection strip.** Background `surface`, radius 10, inner ring, padding 12 by 14; fields in a wrapping row (gap 14 by 30), each a label at 11.5 `text3` over a value (gap 3): Inspection "RI-12" (12.5 medium) · Collected "09:02 → 09:04 · read-only" · Freshness (Fresh tag and "stale after 15 min" in `text3`) · Prior session "S-0411". Trailing: secondary "Reinspect" (tooltip "Collect a new read-only inspection").
3. **Review each finding · n of 6.** The count is at weight 500. A panel card with one row per finding: padding 11 by 14, gap 12, top aligned; a checkbox (accent when checked), a text column (title semibold `textStrong`, one sentence in `text2`, gap 3) and a trailing status component.

   | Finding | Trailing |
   |---|---|
   | "J3 · Attempt 1 is still alive" | Pill "Live · held" (Running form) |
   | "T2 · Attempt 1 process identity unknown" | Pill "Unknown" (Recovery required form) |
   | "T4 · Attempt 1 confirmed dead" | Pill "Exited" (danger, plain dot) |
   | "Push op-7f3a outcome unknown" | Evidence pill Unknown (dashed, "?") |
   | "PRD-007 rev 4 approval request is stale" | Document tag Stale |
   | "2 resource claims quarantined" | Pill "Quarantined" (Recovery required form) |

4. **Start bar.** Background white at 3.5%, radius 10, padding 12 by 14: a sentence in `text2`, at most 560 wide ("Starting a session reopens admission for new work. It does not continue, retry or settle any held item. Live and unknown processes stay held."), and a trailing primary button "Start New Session" with a play glyph (tooltip "Asks the core to open admission for session S-0412"). Until every finding is checked the button is disabled with the tooltip "Unavailable until every finding is marked reviewed."

## 4. What the app shows today

Built in `macos/JojoApp/Sources/JojoApp/RecoveryCenter.swift` on the shared surfaces of COMPONENTS.md, section 14. The screen shows only what the helper reports: the admission value, the helper connection and epoch, the recovery rows (attempt identity, assessment text, findings text, blockers, whether every process is proven gone, offered choices) and the inspection digest. A part of the canvas without a source says so on screen; nothing is filled with an example.

Since M5 (protocol 1.10, DESIGN_ROADMAP_DIFFERENCES.md, E16) the center column starts with "External actions · n with unknown outcome" while a push or draft pull request was issued and its outcome was not observed (a lost answer, a helper that stopped): its remote, reference, commit and what was last seen, "Check Remote" (`InspectExternal`: reads the remote and records what it shows, sends nothing) and "Mark Not Performed…" (`ResolveExternal`, confirmed, available after an inspection). Such an action is never sent again; it does not close admission. The screen reads `ExternalActions` only while it is shown (`visible`, a parameter since M5). Inspection counts still do not include external effects.

### 4.1 Mapping

| Canvas | App today |
|---|---|
| Held subject kinds | Only prior attempts are reported. External side effects and quarantined claims have no rows |
| Item title and context | "Attempt" and the first eight characters of the identity; the context line is the number of blockers ("2 blockers", "No blockers", "Blockers not reported"). Task, feature and project names are not reported |
| Status pill | "Confirmed dead" when every process is proven gone, otherwise "Unknown"; both in the Recovery required form. "Live · unsupervised" cannot be told apart from unknown and is not shown |
| Settled today | Heading with "Not reported yet." |
| Freshness tag and inspection time | "Inspection age not reported" in `text3`. A stale inspection is known only when the core refuses a command; the refusal appears in the status line |
| Reinspect | One inspection covers every held item, so the detail's button is the canvas's "Reinspect All". The toolbar buttons are not built |
| Inspected facts | Attempt (full identity) · OS evidence (evidence pill: Passed "Every process proven gone", or Unknown "Not proven gone") · Blockers (one per line; "None reported"; "The core shortened this list." when incomplete) · Assessment (the core's own text) · Findings (the core's own text, monospaced, "(partial)" when cut). A footnote names the facts that are not reported separately: worktree, snapshot, evidence, session, claims, process group |
| Recovery actions | Enabled exactly for the choices the row offers. A core that does not report choices leaves all four enabled and says so in the footnote; it still decides |
| Action sentences | Reworded to what the core states: Validate Output "Checks the existing output against the task's verification requirements. No new inference." · Continue with History "Starts a new attempt in a new worktree that resumes the provider session with its history." · Continue Fresh "Starts a new attempt in a new worktree with a new session." · Hold "Keeps everything as is. State, worktree and evidence are retained." |
| Unavailable sentences | Process not proven gone: the canvas sentence for unknown process state. Otherwise: "Unavailable: the core's inspection does not offer this choice." |
| Footnote | "Offered by the core's inspection. The core checks eligibility again when you choose." or the canvas's "Only Hold is available…" |
| Ellipsis | The three actions with an ellipsis ask once in a system confirmation dialog (the dialog of COMPONENTS.md, section 7, is not built). Hold is sent directly |
| Admission card | Pill "Open" (ok), "Closed" (Recovery required form) or the Unknown evidence pill; the identifier is the helper epoch. Sentence: "Held items stay held. Who started the session, and when, is not reported yet." "Pause Admission" is drawn disabled with "The core has no pause command yet." While admission is closed the card has a primary "Open Review" instead of "View RI-12" |
| Last inspection | Held attempts · Processes proven gone (n / total) · With blockers (`warn` above zero) · Offering more than Hold · Inspection (first twelve characters of the digest). A footnote names the canvas counts that are not reported |
| Launch review banner | "Execution admission is closed" · "No new work starts and nothing continues until you review this inspection and start a new session." The time and cause of the stop are not reported |
| Inspection strip | Inspection (digest, twelve characters) · Findings (count) · Collected and Freshness "Not reported yet" |
| Findings | One per recovery row: "Attempt 1a2b3c4d: every process proven gone" or "…: process state unknown", the first blocker or the assessment text as the sentence, and the item's pill. Checks are kept for the digest they were made on; a new inspection clears them |
| Finding rows | The list row of COMPONENTS.md, section 5.3, with a checkbox as its leading slot, a wrapping second line and an inset of 14 |
| Start New Session | Sends the reviewed start bound to the digest. Disabled, with the reason beside it, until every finding is checked, while a command is pending, without a digest or without a connection |

### 4.2 Navigation and states

- The Recovery Center destination shows the center. While the core reports an admission other than `Open`, the Recovery required banner of STATES.md sits above it with "Open Review", which shows the launch review; "Recovery Center" at the top of the review goes back. When admission opens, the center is shown again.
- Disconnected (`channelLost`, `ended`, `launchFailed`): danger banner "Jojo core isn't responding" · "Showing the last known state, read-only. No actions can be requested." Every action is disabled. There is no "Reconnect" yet; the app has no way to start the helper again.
- Incompatible protocol: danger banner "Jojo core speaks another protocol version" · "Read-only. No actions can be requested."
- Loading (connected, no digest yet): "Loading the recovery inspection…" and three skeleton bars.
- Empty (digest, no rows): the Empty form with "Nothing is on hold" · "The last inspection found no interrupted or unknown work." and a secondary "Reinspect".
- Requested: the toolbar's "Requested" pill; every action is disabled while a command is pending.
- Content under 920 wide (the window at its opening size): the trailing panel's cards move below the detail, so the detail keeps at least 420. Three columns appear in a wider window.

### 4.3 Values that are not on the canvas

| Point | Value in the app |
|---|---|
| Side panel background | `sidebar` token (canvas `#1E1E22`, no token); edge `sidebarEdge` |
| Panel widths | Held list 240, trailing panel 260, detail takes the rest |
| Selected item, light | Same formula as dark: `accent` at 16%, ring `info` at 40% |
| Start bar background | `fill2` (canvas white at 3.5%) |
| Checkbox | Drawn, 14 × 14, radius 3.5: `accent` with a white check when on, 1 pt `text4` ring when off |
| Identifiers | The `mono` text style (11 pt), as in the shell |
| Skeleton bars | `fill` in both appearances (canvas: white at 8% in dark) |
| Glyphs (SF Symbols) | Banner warning `exclamationmark.triangle.fill`, danger `exclamationmark.octagon.fill`; Reinspect `arrow.clockwise`; Start New Session `play.fill`; Pause Admission `pause.fill`; footnote `info.circle`; empty state `checkmark.circle`; back `chevron.left` |

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

Rebuilt on 2026-11-18 from `RecoveryCenterMac.dc.html` (step 6) in `macos/JojoApp/Sources/JojoApp/RecoveryCenter.swift`, on the Liquid Glass surfaces of COMPONENTS.md, section 28. Sections 1 to 3 above describe the first boards (now the canvas's Archive page); the Mac board has the same three columns. The screen reads `Recovery` (1.1, the existing poll) for the subjects and their offered choices, `ExternalActions` (1.10) for pushes and pull requests whose outcome is unknown, and `RecoveryDetail` (1.11) for the rest, the last two only while the screen is shown and again with each snapshot and each new inspection digest.

### 4.1 Mapping

| Board | App |
|---|---|
| Header "Recovery Center", "Workspace · n held", "Reinspect All", "Inspect Recovery" | The same; Reinspect All sends `RecoveryInspect`; Inspect Recovery opens the launch review while admission is closed and says why it is disabled otherwise |
| Held list: external side effect, prior attempts, quarantined claim | External actions whose outcome is unknown ("Outcome unknown"), then each prior attempt by its task and attempt ("T2 · Attempt 1") with project, feature and step title, then the live claims of earlier sessions whose holder is not a subject ("Quarantined", "Held by T1 · attempt 2"). An attempt's pill: "Live · unsupervised" when a recorded process is alive, "Confirmed dead" when every process is proven gone, else "Unknown" |
| "Settled today" | "Settled this session": the resolutions this session recorded other than Hold (`settled`), with the time and "Continued fresh", "Continued with history" or "Output validated". A Hold keeps the attempt in the held list |
| Freshness strip "Stale · inspected 09:04 · 38 min ago · reinspect before acting · Reinspect" | The same from `inspected_at_unix_ms`; older than five minutes is Stale |
| "Inspected facts · read-only" | Attempt, stage in words, PID and PGID with their findings, OS evidence, worktree ("Present · matches its record", "Changed · …"), snapshot ("sha256 verified · …", "Changed after snapshot", "No reviewed snapshot"), evidence (gaps, or "Logs complete to their last durable record"), session ("Claude Code session recorded"), claims, stale approvals, unknown effects and blockers, each on its own line |
| External side effect detail | Operation (`git push origin refs/heads/…` or `gh pr create --draft …`), idempotency key, commit, remote, last seen; "How this resolves"; M5's "External actions · n with unknown outcome" with Check Remote and Mark Not Performed… (kept from M5) |
| Quarantined claim detail | Paths, held by, mode, what it blocks; only Hold |
| Recovery actions | Validate Output…, Continue with History…, Continue Fresh…, Hold, enabled only as the core's inspection offers them; the unavailable sentence names the reason (alive, unknown, not offered) |
| Execution admission | Open or Closed, "Session n", Open Review while closed; Pause Admission disabled |
| "Last inspection": attempts examined, worktrees intact, snapshot mismatches, unknown external effects, stale approvals | The same from `counts`, plus evidence gaps, quarantined claims, the time and the inspection digest |

### 4.2 Navigation and states

Loading ("Loading the recovery inspection…"), no inspection, nothing on hold (Reinspect), a lost or incompatible core (CoreBanners), a core older than 1.11 (a banner, names "not reported", the facts of the 1.1 row and "Not reported by this core (protocol 1.11)" for the counts), admission closed (a danger banner with Open Review). The launch review is unchanged except that "Collected" and "Freshness" now come from the inspection's time.

### 4.3 Known gaps

- There is no pause-admission command and no way to reconnect a lost helper; Pause Admission is drawn disabled.
- Who started the session and after which inspection is not reported ("View RI-12" is not built).
- The inspection time is the helper's memory of its last inspection; a restart inspects again.
- The settled list is this session's resolutions, not "today's"; the held list shows at most 16 subjects and 16 claims.
- A claim's mode is the core's (`SHARED`, `EXCLUSIVE`); "Blocks" is derived from it, not reported.

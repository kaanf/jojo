# Agent Runs

Source: the canvas "Jojo macOS", page "Product", board "Agent Runs" (`AgentRunsMac.dc.html`, 1440 × 1400, light by default). Read on 2026-11-17 from version `1794942048-8ee3`; the board file is byte-identical to the copy read at version `1794922102-b518` (sha256 `fe7dfe82…875f`). Tokens, components and states are named as in [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). The design wave 1 document is in the git history (this file at commit `921322a`).

Built as the sixth screen of step 4 of the Liquid Glass order (STATUS.md), with protocol 1.9 (DESIGN_ROADMAP_DIFFERENCES.md, E15).

## 1. The board

| Element | Board | Design system |
|---|---|---|
| Header | "Agent Runs", "Crew › FT-142 Lobby invite links › attempts"; capsule "View Logs", "Inspect Attempt"; "Inspect Recovery" | header row (4.3) |
| Lifecycle card | Execution current, the three count pills | lifecycle stepper (2.1) |
| Attempts (220 to 250) | "Attempts · FT-142" with "Filter"; groups by task ("T4 · Invite endpoint"), rows "Attempt n" with a pill (Running, Interrupted · settled, Completed, Unknown) and a line ("Claude Code · 12m 08s", "Exited 0 · validated", "Confirmed dead · continued fresh") | list rows |
| Detail (card, radius 18, padding 22 24) | "T4 · Attempt 2", 22 bold title, pill; a segmented "Timeline / Logs / Evidence / Claims"; a facts card (Attempt, Process, Started, Duration, Base commit); "Event timeline" (dot, sentence, time); "Terminal output" with "Search output" and a dark console | segmented control, card |
| Inspector (glass, 280 to 320) | Provider session (Provider, Session, Resumable), Process supervision (State, PID, Process group, Liveness), Workspace (Worktree, Branch), Continuation (Provenance), Resource claims, Checkpoints, Actions ("Request Safe Stop", "Cancel Attempt…", two footnotes) | inspector (4.4) |

## 2. Where each value comes from

| Element | Source | Since |
|---|---|---|
| Attempts, grouped by task | `Attempts` (state, key, title, provider, times, ordinal, validation), the held feature's | 1.6, 1.7 |
| Pills | a recovery hold (`Recovery`), the step's M4 state (`Repairs`), else the lifecycle state | 1.1, 1.5, 1.6 |
| Sidebar counter | the live attempts (`Attempts`), else the snapshot's attempts | 1.6 |
| Facts: process, started, duration | `Attempts` | 1.6 |
| Base commit, worktree, PID, process group, liveness, session, claims, executable | `AttemptDetail` | **1.9** |
| Event timeline | `AttemptDetail.events`: the events about the attempt with the 1.7 sentence and their times | **1.9** |
| Terminal output, Logs | the first 64 records of `Log` (through `loadCandidate`), searched in the app | 1.1 |
| Evidence | `AttemptDetail.validations` ("V-n") and the number of evidence records; "View Evidence" opens Verification & Evidence | **1.9** |
| Continuation | `AttemptDetail.continues` / `continued_by`, or the M4 repair (`Repairs`) | **1.9**, 1.5 |
| Repair and review | `Repairs`: counter, verdict, hold | 1.5 |
| Cancel Attempt… | `CancelAttempt`, confirmed in a system dialog | 1.1 |

### Not built, and why

| Board element | Why |
|---|---|
| Resumable, CLI session resume | Each attempt is a fresh run; the field says so |
| Branch | An attempt works in a detached worktree; the plan branch is on Execution Plan |
| Checkpoints | The core records stream offsets, not checkpoints of the work; said in the section |
| Request Safe Stop | The core has no safe stop; the button is disabled with "Not available yet" |
| Filter | The list is the held feature's attempts |

## 3. Decisions

Decided by the human on 2026-11-17: checkpoints are not reported, liveness is the newest heartbeat, Request Safe Stop is "not available yet". The board's "Inspect Recovery" opens the Recovery Center.

## 4. Implementation notes

- `macos/JojoApp/Sources/JojoApp/AgentRuns.swift`: `AgentRunsState`, `AttemptTab`, `AgentRunsView`, `AgentRunsScreen` (reads `Features`, `Repairs`, `Attempts`, `AttemptDetail` of the chosen attempt and its result and log while `visible` and with each new snapshot). "Inspect Attempt" on Execution Plan and "Inspect Failure" on Verification choose an attempt here (`WorkspaceModel.focusAttempt`). The header's View Logs sets the Logs tab.
- The `--ui-script` step `repairs-state` reads the same state (`AgentRunsState.listed`, `pill`, `line`), so `gui_m4.py` checks what the screen shows.
- The boundary guard rejects the bare word for an OS process in the Swift sources; the labels are written "process" and "process supervision" (shown upper case by the `label` style).
- Review aid: `JOJO_SNAPSHOT_DIR=<directory> swift test --filter S9AgentRunsTests` writes `s9-agent-runs-…`, `-claims-…`, `-evidence-…`, `-narrow-…`, `-disconnected-…`.

## 5. What the app shows today / Known gaps

Agent Runs shows the held feature's attempts grouped by task with their state, provider, duration and validation; one attempt's facts, the events about it with their times and sentences, its terminal output (searchable), its validation runs and its claims; its session, supervision, worktree, provenance, repair and review. An attempt is cancelled here. Known gaps:

- No safe stop, no checkpoints of the work, no session resume, no branch per attempt.
- The app reads the newest 16 attempts, the first 64 log records and 64 events per attempt.

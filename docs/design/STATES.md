# Jojo design system: states

What every data surface shows when it has nothing, is waiting, failed, or cannot be trusted. Components and tokens refer to [COMPONENTS.md](COMPONENTS.md) and [DESIGN_SYSTEM.md](DESIGN_SYSTEM.md). Source: canvas board "States"; see [README.md](README.md).

## 1. Rules

1. **Every data surface handles all nine application states below.** A screen is not done until each of them has been drawn or ruled out.
2. **Unknown is never drawn as success or failure.** It has its own form, is never called done or failed, and is never retried automatically.
3. **No optimistic success.** A command that was sent is "Requested" until the core records it.
4. **A message says what happened, in the system's own terms, and what did not change.** "git status exited 128: not a git repository." "Nothing was changed."
5. **A state that removes actions says so and offers the one action that restores them.**

## 2. Application states

| State | When | Form | Content | Actions |
|---|---|---|---|---|
| Empty | The surface has no data yet | Centered block: 24 pt glyph, title at 13 semibold `textStrong`, one sentence in `text2` (at most 320 wide), buttons | "No projects yet" · "Create a project or import a folder that already has Git repositories." | One primary ("Create Project"), one secondary ("Import…") |
| Loading | Data was requested and has not arrived | A line of text in `text2` naming what loads, then three skeleton bars (height 10, radius 5, widths 70%, 90%, 55%, white at 8% in dark) | "Loading attempts…" | None |
| Error | A read failed | Centered block as Empty | "Couldn't read crew-app" · the exact cause | Primary "Retry", ghost "Show Details" |
| Permission denied | macOS refused access | Centered block as Empty | "Jojo can't access ~/Code/crew" · "macOS hasn't granted folder access. Nothing was changed." | Primary "Grant Access…" |
| Stale | The facts shown are older than the action needs | Warning banner above the data, which stays visible | "These facts are 38 minutes old" · "Inspected 09:04. Recovery actions stay unavailable until you reinspect." | "Reinspect" |
| Disconnected | The helper does not answer | Danger banner; the last known state stays visible, read-only | "Jojo core isn't responding" · "Showing the last known state, read-only. No actions can be requested." | "Reconnect" |
| Recovery required | Admission is closed until a reviewed start | Danger banner | "Execution admission is closed" · "Review inspection RI-12 to start a new session." | "Open Review" |
| Requested, awaiting core | A command was sent and is not yet confirmed | Notice well with a "Requested" pill | "Push authorization sent. Shown as authorized only after the core records it." | None. The control that sent it cannot send again |
| Unknown outcome | An external action's result cannot be determined | The object's name, an Unknown evidence pill ("?" with "Outcome unknown"), and one line in `text3` | "Never shown as done or failed. Never retried automatically." | Ghost "Inspect in Recovery" |

## 3. Discovery states

Feature Chat across the first lifecycle stage.

| State | Form | Content | Action |
|---|---|---|---|
| Empty | Centered block | "Describe what users should be able to do" · "Jojo reads the code and asks before planning." | Primary "Start Discovery" |
| Investigating | "Investigating" pill (Running form), progress in `text2`, two skeleton bars, a note in `text3` | "Reading crew-api · 3 of 5 areas" · "Read-only. No files are changed during discovery." | None |
| Blocked | Warning banner | "Whole feature blocked by Q2" · "FT-145 can't move to Specification until “Rank by season or all-time?” is answered." | "Answer Q2" |
| Complete | "Discovery complete" evidence pill (Passed form), a revision tag ("PRD rev 4"), a summary in `text2` | "5 questions resolved · 3 assumptions tracked. History stays with the feature across revisions." | Secondary "Open PRD" |

## 4. How the states map to what the helper reports today

The right column names what exists in the app and protocol now. Rows without a source cannot be built yet.

| State | Source today |
|---|---|
| Disconnected | `HelperConnection.channelLost`, `.ended`, `.launchFailed` |
| Recovery required | Snapshot `admission` is not `Open`; `Recovery` rows exist |
| Requested, awaiting core | `WorkspaceModel.commandPending`; `HelperClient.outstandingCommands` (the Approval Inbox and Specifications read an outstanding `HumanDecision` there); `WorkspaceModel.startOutcome` is `sent` for a Start on Execution Plan |
| Unknown outcome | Validation state `UNKNOWN`; an attempt outcome the core could not classify; recovery subjects with blockers; `WorkspaceModel.startOutcome` is `unknown` for a Start without a usable answer |
| Stale | `StaleInspection` and `StaleCandidate` refusals; `WorkspaceModel.reviewStale` for a document review after a decision was sent or refused. The age of an inspection is not reported by the helper yet |
| Error | An `Error` reply to a query; `WorkspaceModel.startOutcome` is `refused` with the code of a refused Start; `WorkspaceModel.eventsLoad` is `failed` for the event journal of Activity & Audit |
| Loading | A query without a reply yet; `WorkspaceModel.eventsLoad` is `loading` for the event journal; `HelperConnection.notStarted` and `.handshaking` for the Hello answer in the Core section of Settings & Providers |
| Empty | An empty page of rows |
| Permission denied | No source yet |
| Discovery states | No source yet (M2, Feature Chat) |

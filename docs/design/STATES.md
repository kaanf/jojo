# Jojo design system: states

What every data surface shows when it has nothing, is waiting, failed, or cannot be trusted. Components and tokens refer to [COMPONENTS.md](COMPONENTS.md) and [DESIGN_SYSTEM.md](DESIGN_SYSTEM.md). Source: the canvas page "Components & states", board "States" (`StatesMac.dc.html`); see [README.md](README.md).

## 1. Rules

1. **Every data surface handles all nine application states below.** A screen is not done until each of them has been drawn or ruled out.
2. **Unknown is never drawn as success or failure.** It has its own form, is never called done or failed, and is never retried automatically.
3. **No optimistic success.** A command that was sent is "Requested" until the core records it.
4. **A message says what happened, in the system's own terms, and what did not change.** "git status exited 128: not a git repository." "Nothing was changed."
5. **A state that removes actions says so and offers the one action that restores them.**
6. **Skeleton rows keep the layout; no spinner over content.** A spinner appears only inside the tile of a placeholder.

## 2. The placeholder

The states are drawn the way macOS apps present them inside a window: a centered placeholder on a card.

- Card: radius 18, padding 20, at least 220 high, content centered, gap 10.
- From top to bottom: the state's name as a `label` (shown on the reference board only), a 44 × 44 tile (radius 13) with a white glyph, the title (15 bold), one sentence in `sub` style (at most 280 wide, line height 1.45), then the actions (buttons 28 high, text 12, gap 6) or a pill.

Where a surface already shows content (the last known state, facts that went stale), the state is the page banner of COMPONENTS.md, section 10, above that content instead, with the same title, sentence and action. The board draws the placeholders only; this rule is ours.

## 3. Application states

| State | When | Tile | Title · sentence | Actions |
|---|---|---|---|---|
| Empty | The surface has no data yet | `accent` | "No projects yet" · "Create a project or import a folder that already has Git repositories." | Primary "Create Project", secondary "Import…" |
| Loading | Data was requested and has not arrived | `grayBg` with a spinner (22, 2.5 ring `accentTint`, top `accent`, 0.9 s a turn) | "Loading attempts…" · "Skeleton rows keep the layout; no spinner over content." Two skeleton bars (10 high, radius 5, `grayBg`, full and 70%, at most 260 wide) | None |
| Error | A read failed | red `#ff3b30` | "Couldn't read crew-app" · the exact cause ("`git status` exited 128: not a git repository.") | Primary "Retry", secondary "Show Details" |
| Permission denied | macOS refused access | orange `#ff9500` | "Jojo can't access ~/Code/crew" · "macOS hasn't granted folder access. Nothing was changed." | Primary "Grant Access…" |
| Stale | The facts shown are older than the action needs | orange `#ff9500` | "These facts are 38 minutes old" · "Inspected 09:04. Recovery actions stay unavailable until you reinspect." | Primary "Reinspect" |
| Disconnected | The core does not answer | gray `#8e8e93` | "Jojo core isn't responding" · "Showing the last known state, read-only. No actions can be requested." | Primary "Reconnect" |
| Recovery required | Admission is closed until a reviewed start | red `#ff3b30` | "Execution admission is closed" · "Review inspection RI-12 to start a new session." | Primary "Open Review" |
| Requested, awaiting core | A command was sent and is not yet confirmed | purple `#af52de` | "Push authorization sent" · "Shown as authorized only after the core records it." | None: the purple pill "Requested". The control that sent it cannot send again |
| Unknown outcome | An external action's result cannot be determined | gray `#8e8e93` | "Push op-7f3a" · "Never shown as done or failed. Never retried automatically." The evidence pill "? Outcome unknown" | Secondary "Inspect in Recovery" |

## 4. Discovery states

Feature Chat across its lifecycle, in the same placeholder.

| State | Tile | Title · sentence | Action |
|---|---|---|---|
| Empty | `accent` | "Describe what users should be able to do" · "Jojo reads the code and asks before planning." | Primary "Start Discovery" |
| Investigating | `grayBg` with the spinner | "Investigating" · "Reading crew-api · 3 of 5 areas. Read-only: no files are changed during discovery." | None: the blue pill "Running" |
| Blocked | orange `#ff9500` | "Whole feature blocked by Q2" · "FT-145 can't move to Specification until “Rank by season or all-time?” is answered." | Primary "Answer Q2" |
| Complete | green `#34c759` | "Discovery complete" · "PRD rev 4 · 5 questions resolved · 3 assumptions tracked. History stays with the feature across revisions." | Primary "Open PRD" |

## 5. How the states map to what the helper reports today

The right column names what exists in the app and protocol now. Rows without a source cannot be built yet.

| State | Source today |
|---|---|
| Disconnected | `HelperConnection.channelLost`, `.ended`, `.launchFailed` |
| Recovery required | Snapshot `admission` is not `Open`; `Recovery` rows exist |
| Requested, awaiting core | `WorkspaceModel.commandPending`; `HelperClient.outstandingCommands` (the Approval Inbox and Specifications read an outstanding `HumanDecision` there); `WorkspaceModel.startOutcome` is `sent` for a Start on Execution Plan |
| Unknown outcome | Validation state `UNKNOWN`; an attempt outcome the core could not classify; recovery subjects with blockers; `WorkspaceModel.startOutcome` is `unknown` for a Start without a usable answer |
| Stale | `StaleInspection` and `StaleCandidate` refusals; `WorkspaceModel.reviewStale` for a document review after a decision was sent or refused. The age of an inspection is not reported by the helper yet |
| Error | An `Error` reply to a query; `WorkspaceModel.startOutcome` is `refused` with the code of a refused Start; `WorkspaceModel.eventsLoad` is `failed` for the event journal of Activity & Audit |
| Loading | A query without a reply yet; the Dashboard's `featuresLoad`, `attemptsLoad` and `eventsLoad` while nothing was read (skeleton cards per section); `WorkspaceModel.eventsLoad` is `loading` for the event journal; `HelperConnection.notStarted` and `.handshaking` for the Hello answer in the Core section of Settings & Providers |
| Empty | An empty page of rows; the Dashboard when `Features` lists no feature and nothing runs or waits (placeholder with "New Feature") |
| Permission denied | No source yet |
| Discovery states | Since M2, the `FeatureChat` query: Empty is a feature without turns; Investigating is a turn in state `Running` (or `Stopping`); Blocked is an unanswered blocking question about the whole feature; Complete is a requirements document that is `ready`. A blocking question about one plan step is a separate warning, "Feature continues · step … is blocked by Q…" (`FeatureChatState.discovery`, `docs/design/screens/FEATURE_CHAT.md`) |

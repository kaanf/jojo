# Dashboard

Source: the canvas "Jojo macOS", page "Product", board "Dashboard" (`DashboardMac.dc.html`, 1440 × 1240, light by default). Read on 2026-11-17 from version `1794931431-3abf`; the board file is byte-identical to the one in version `1794922102-b518`, from which DESIGN_SYSTEM.md, COMPONENTS.md and STATES.md were written (sha256 `e2f224d4…72c0`). Tokens, components and states are named as in [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md).

Built in step 3 of the Liquid Glass order (STATUS.md), with protocol 1.7 (DESIGN_ROADMAP_DIFFERENCES.md, E12).

## 1. The board

The window and the sidebar are the shell (COMPONENTS.md, sections 4.1 and 4.2); "Dashboard" is the chosen nav item. The content column (padding 10 18 22 14, sections 20 apart) holds, from top to bottom:

### 1.1 Header (COMPONENTS.md, section 4.3)

| Element | Board | Design system |
|---|---|---|
| Title | "Dashboard" | `dashboardTitle` (22 bold, −0.02 em) |
| Sub line | "Workspace · Monday, 16 November" | `secondary`, `text2`; the place and the day |
| Time range | segmented "Today", "Week", "All" | segmented control (section 5.6) |
| Icon group | Filter, appearance switch | capsule group of icon buttons (section 3) |
| Primary | "New Feature ⌘N" | primary button with shortcut hint (section 3) |

### 1.2 Summary

A grid of four KPI tiles (section 5.2), columns at least 190 wide, gap 12.

| Tile | Tile color | Value · unit | Sub line |
|---|---|---|---|
| Needs you | orange `#ff9500` | 6 items | "4 approvals · 2 recovery reviews" |
| Running now | blue `#0a84ff` | 2 tasks | "T4 on Claude Code · T5 on Codex" |
| Blocked | red `#ff3b30` | 3 tasks | "T7, T6 and FT-145" |
| Providers | green `#34c759` | 1 of 2 available | "Codex limited since 09:12" |

### 1.3 Needs you, Running now, Providers

Two columns that wrap: "Needs you" (flex 3, at least 440) and an aside (flex 2, at least 320) with "Running now" and "Providers". Each section starts with a section heading (section 4.5): title, count, trailing link.

- **Needs you** · 6 · link "Approval Inbox". Group label "APPROVALS · 4" (`label`, `text3`), a list card of list rows (section 5.3): "ADR-031 rev 5" (indigo tile, "Architecture decision · FT-142 · Adds 7-day expiry, unblocks T6", blue "Proposed"), "Plan rev 3" (blue tile, "Execution plan · FT-142 · Adds T6 token expiry job", "Proposed"), "Write iosApp/Info.plist" (orange tile, "Scope expansion · T5 · Requested by Codex, write refused until decided", orange "Needed"), "Push jojo/ft-142-t2-02 @ c44e910" (teal tile, "External action · crew-app · Verified in V-2", "Needed"). Group label "RECOVERY · 2": "Push op-7f3a outcome unknown" (red tile, "Recovery · Crew · Hold only until remote is reinspected", orange "Held"), "J2 · Attempt 1 confirmed dead" (purple tile, "Recovery · Jojo · Eligible: validate output or continue", purple "Review"). Rows have a chevron.
- **Running now** · 2 · link "Agent Runs". One card per running task (radius 14, padding 14, gap 10): the key (12 bold `text2`), the title (13 semibold) and the elapsed time ("12m", `sub`); a progress bar (6 high, radius 3, `grayBg` track, `accent` or `orange` fill, 68% and 30%); then "Crew · crew-api · Claude Code · Attempt 2" (`sub`) and a pill with what the agent does ("Running unit tests" green, "Waiting on scope decision" orange).
- **Providers** · link "Settings". A list card, rows without chevron: provider tile ("CC" on `#d97757`, "CX" on `#3a3a3c`), name, "1 active session · signed in via CLI" / "Usage limit reported by CLI at 09:12", pill "Available" (green) / "Limited" (orange).

### 1.4 Active projects

Section heading "Active projects" · 4, a segmented control "Active", "Needs you", "Done", link "Projects". A card with the pipeline table (section 5.5): header row "Feature, Discovery, Spec, Planning, Execution, Verification, Evidence" (11 semibold `text3`); one row per feature: name and "Project · Feature-ID"; stage cells with a dot and a word ("Done" green, "ADR rev 3" and "Q open" orange, "2 running" the running dot, "Stale" gray, "Sealed" a green pill), unreached stages a `dotOff` dot alone. The table has a minimum width of 820 and scrolls sideways.

### 1.5 Blocked, Latest verification, Recent history

A grid of three sections, columns at least 300 wide, gap 20.

- **Blocked** · 3: a list card of rows without tile: a red or orange dot, "T7 · Lobby join screen" with "4 blockers incl. Q4", a small secondary "Why"; "FT-145 · Leaderboard API" with "Feature-level question open" and a small primary "Answer".
- **Latest verification** · link "Evidence": rows with a red or green tile, "FT-142 · V-3" with "CAND-142-07 · 1 failed, 1 unknown" and the red "Incomplete" pill; "FT-139 · V-5" with "evidence sealed" and a green "Sealed".
- **Recent history** · link "Activity": a card (padding 6 14) of lines (12, padding 7 0, 0.5 `sep` between): a dot (warn, running, red, done, gray), a sentence, the time in `text2`.

### 1.6 States

The board draws only the filled state. The others follow STATES.md: Disconnected and Recovery required as the page banner above the content, Loading as skeleton cards per section, Empty as the placeholder with "New Feature", a failed read as a sentence in the section's card.

## 2. Where each value comes from

| Element | Source | Since |
|---|---|---|
| Sub line date | The app's clock | — |
| Needs you count, approvals | `Documents`: `pending`, the documents in state `Pending` and the waiting `candidates` | 1.6 |
| Needs you, recovery | `Recovery` rows; a row with blockers is "Held", one without is "Review"; its name from `Attempts` | 1.1, 1.6 |
| Feature code "FT-n" | `Features.number`: the feature's place in the order features first appeared | **1.7** |
| Running now: key, title, provider, elapsed | `Attempts`: `key`, `title`, `provider`, `started_at_unix_ms`, a live `state` | 1.6 |
| Running now: project, repository, "Attempt n" | `Attempts` at 1.7: `project_name`, `repository_name`, `ordinal` | **1.7** |
| Running now: what the agent does | `Attempts` at 1.7: `activity` (the newest provider record in a few words) | **1.7** |
| Blocked count and list | `Features`: `blocked` and `blockers` (open blocking questions; plan steps `Blocked`, `NeedsHuman`, `Conflict`, `Refused`, each with its reason) | **1.7** |
| Providers: available, version | `Executables`: `state`, `version` | 1.4 |
| Providers: active count | live `Attempts` per provider | 1.6 |
| Active projects: name, project, five stages, filter | `Features`: `name`, `project_name`, `stages`, `awaiting`, `blocked`, `done` | **1.7** |
| Latest verification | `Attempts` at 1.7: `validation` (`state`, `at_unix_ms`, `decision`) | **1.7** |
| Recent history | `Events` at 1.7: `summary`, `at_unix_ms` | **1.7** |

How the core derives each 1.7 value is in DESIGN_ROADMAP_DIFFERENCES.md, E12, and the fields are in `docs/architecture/STDIO_PROTOCOL.md`, "Protocol 1.7". Nothing is stored for them (schema 15).

### Not reported, and why

| Board element | Why it is not shown |
|---|---|
| Progress bar of a running task | The core has no measure of progress. The bar is not drawn; the activity pill stands in for "Running unit tests" |
| "Codex limited since 09:12", "Limited" | The core does not detect usage limits (the stream's rate-limit record is kept only as a kind). Said on screen: "Sign-in state and usage limits are not reported." |
| "signed in via CLI", "1 active session" | Sign-in state is not reported; the count is of running attempts |
| Scope expansion and external action rows | No scope request command exists; push and pull request belong to M5. Said under "Needs you" |
| Evidence column, "Sealed", V-n | The core seals no evidence and numbers no validation runs. The Evidence column is a `dotOff` dot with a help text; "Latest verification" lists host validations with the evidence pill of their verdict |
| "Adds 7-day expiry, unblocks T6" | What a document changes is not reported |
| "4 blockers incl. Q4" | A step reports one reason, not a count |
| Filter and appearance icon buttons | No filter is defined; the appearance follows the system (Settings › Appearance is not built). The shell's Refresh and Show Inspector stand in that capsule group |

## 3. Decisions

Decided by the human on 2026-11-17 (all four proposals as given):

1. The core gets a `Features` query and `Attempts` four more fields, at protocol 1.7, derived and without a schema change.
2. Each event carries a sentence from the core at 1.7 (`summary`).
3. Elements without a real source are not built and are said to be not reported, except the agent's newest activity, which the core derives from its newest provider record.
4. The Dashboard is the screen the app opens on; it reads while shown and with each new snapshot. The GUI scenarios' "protocol 1.1 frames" check admits the Dashboard's frames at their minor.

Taken as defaults and stated before building: no sidebar counter for the Dashboard (the board has none); the time range filters only "Latest verification" and "Recent history"; the filter and appearance buttons are not built; the shell's header takes a screen's own controls and primary action, and other screens keep their header.

## 4. Implementation notes

- `macos/JojoApp/Sources/JojoApp/Dashboard.swift`: `DashboardState` (a plain value; every derived list is a computed property), `DashboardView`, `DashboardScreen` (reads `Features`, `Attempts`, `Executables` and `Events` while `visible` and on each new snapshot; `WorkspaceModel.dashboardShown`), the header's `NewFeatureButton` (⌘N only while the Dashboard is shown) and the components of COMPONENTS.md, section 25. The time range lives in `MainView` and is shared by the header's segmented control and the screen.
- `macos/JojoApp/Sources/JojoCore/Features.swift`: `FeatureSummary`, `FeatureStage`, `FeatureBlocker`, `FeatureList`, `loadFeatures`. `AttemptSummary` gained `ordinal`, `repositoryName`, `projectName`, `validation`, `activity`; `JournalEvent` gained `summary`.
- Glyphs: Needs you `tray.full`, Running now `play.fill`, Blocked `exclamationmark.octagon`, Providers `cpu`; ADR `building.columns`, execution plan `list.bullet.indent`, validation plan `checklist`, requirements `doc.text`, candidate `checkmark.seal`, recovery hold `pause.circle`, recovery review `arrow.uturn.backward.circle`.
- Layout: four KPI tiles in a row from 796 points of content width, else two; the two columns from 780; the lower grid three columns from 940, else two or one; the project table keeps a minimum of 796 and scrolls sideways (clipped off screen).
- "Needs you" rows open their object: a document loads into the Approval Inbox, a candidate into Verification & Evidence, a recovery row opens the Recovery Center. "Why" opens Execution Plan, "Answer" opens the feature's chat. KPI tiles open Approval Inbox, Agent Runs, Execution Plan and Settings. New Feature opens Feature Chat without a feature (NEW_FEATURE.md). A row of the project table opens that feature's chat (since New Feature, 2026-11-17, which no longer lists earlier chats).
- Recent history lists only event types with a sentence for a person (`DashboardState.historyTone`), newest first, at most six, from the events the workspace holds (the newest 64).
- Review aid: `JOJO_SNAPSHOT_DIR=<directory> swift test --filter S3DashboardTests` writes `dashboard-…` (wide), `dashboard-narrow-…`, `dashboard-all-…`, `dashboard-empty-…`, `dashboard-loading-…`, `dashboard-unreadable-…`, `dashboard-closed-…` and `dashboard-disconnected-…` in dark and light.

## 5. What the app shows today / Known gaps

The Dashboard shows only what the core reports, read at protocol 1.7 while it is the shown destination and again with each new snapshot: what waits for the human (pending documents, candidates whose host validation passed, recovery rows), the running attempts with task, project, repository, provider, attempt number, elapsed time and newest activity, what blocks each feature (open blocking questions and held plan steps with their reason), the providers' executables, each feature's five stages and its code, the latest host validations and the recent history as the core's sentences.

Known gaps:

- Not reported, and said on screen: progress of a running task, usage limits and sign-in state, scope expansion requests and external actions (M5), sealed evidence and validation run numbers (the Evidence column), what a document changes, the number of a step's blockers.
- The app reads the newest 16 features and 16 attempts; older ones are not listed. History comes from the newest 64 events the workspace holds.
- Feature codes ("FT-3") are derived from the order features first appeared, not stored; they are stable because the journal only grows.
- An imported plan's steps have no key or title; they show as "Attempt n" and "A step".
- No sidebar counter, filter button or appearance switch. The command palette (⌘K) is still not built.
- A stage's state is the core's derivation (E12); Discovery "In discussion" means turns exist and neither a blocking question nor a requirements document does.

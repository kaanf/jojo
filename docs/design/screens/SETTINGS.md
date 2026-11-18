# Jojo screens: Settings & Providers

Transcribed on 2026-11-16 from the canvas board "Settings & Providers" (`Settings.dc.html`, drawn in dark only; ten sections, with "Claude Code" shown). Tokens, components and states refer to [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). Sections 1 to 4 are the canvas. Section 5 is what the app builds today and why it differs.

## 1. Frame

| Part | Spec |
|---|---|
| Toolbar | Title "Settings & Providers". Breadcrumb "Workspace › Settings". No button |
| Sidebar counter | None. The item is the last of the "System" group |
| Columns | Two columns that fill the rest and wrap when narrow: a section list (flexible from 230) and the shown section (from 460). The list: background `#1E1E22`, a 1 pt edge (white at 6%) on its trailing side |

## 2. Section list

Padding 12 top and bottom; gap 1. One item per section: 30 high, 8 from the list's sides, padding 8 at the sides, radius 7, gap 9; a tile 20 × 20 with radius 6 in the section's solid color and a white glyph; the name at 13 in `text`. The chosen item has the background `accent` at 16% (`rgba(47,111,235,0.16)`). One section is shown at a time. A gap of 10 separates the two providers from the rest.

| Section | Tile |
|---|---|
| Claude Code | `#C2643A` |
| Codex | `#3C3C44` |
| Executable discovery | `#4F5BD5` |
| Defaults & concurrency | `#2F6FD0` |
| Git & GitHub | `#2E7D5B` |
| Workspace storage | `#5A5A64` |
| Evidence retention | `#2E7D5B` |
| Security & approvals | `#B5402F` |
| Keyboard shortcuts | `#5A5A64` |
| Appearance | `#7B4FD6` |

## 3. The shown section

Padding 18 top, 22 sides, 24 bottom; gap 18; at most 760 wide.

| Part | Spec |
|---|---|
| Title | The section's tile at 28 × 28 (radius 6), gap 10, the name at 18 bold in `textStrong` |
| Group | A heading at 12 semibold `text2` (minimum height 22, 2 from the sides, 8 above its card), then a card: background `surface` (`#232327`), radius 10, a 1 pt ring (white at 5%), no padding |
| Setting line | Padding 10 by 14, gap 10, vertically centered, a `hairline` below unless it is the last. A label in `text` (grows 1, from 170), a value in `text2` (grows 2, from 240), then an optional trailing part. A path or identifier as a value: monospaced 11.5 in `text` |
| Trailing part | One of: a compact secondary button ("Change…", "Edit", "Add…", "Back Up Now", "Restore…"); an evidence pill ("Compatible", "Yes", "No", "Unknown"); an operational pill ("Ready", "Limited", "Connected"); a note at 11.5 in `text3` ("From CLI session reports", "Always on", "Not verified by Jojo"); a switch; a segmented control; a key cap |
| Switch | 32 × 19, a capsule; on: `#34C759` with a white 15 pt knob at the trailing side; off: white at 18% with the knob leading |
| Segmented control | Padding 2, gap 2, radius 7, background white at 7%. A segment: 22 high, padding 10 at the sides, radius 5, 12 medium; chosen: `#55555C`, white text and a small shadow; otherwise `text2` |

### 3.1 What each section shows on the board

| Section | Groups and lines (label · value · trailing) |
|---|---|
| Claude Code | **Executable**: Path · `/opt/homebrew/bin/claude` · "Change…". Version · "Detected at launch · capability check passed" · pill "Compatible". Discovery · "Found on PATH via login shell". **Authentication**: Status · "Signed in with your subscription" · pill "Ready". Credentials · "Handled by the official CLI. Jojo never reads, stores or relays API keys or tokens." **Capabilities · reported by CLI**: Structured event stream, Session resume, Headless mode, Token reporting, each with "Yes". **Usage**: Tokens today · "412k input · 38k output" · "From CLI session reports". Plan limits · "Not exposed by the CLI, so not shown" |
| Codex | The same groups. Status · "Signed in · usage limit reached" · pill "Limited". Session resume · "No". Token reporting · "Not reported" · pill "Unknown". **Usage**: Limit status · "Reported by CLI at 09:12: limit reached". Reset · "CLI reports a reset time of 14:00" · "Not verified by Jojo". Tokens · "Not reported by this CLI" |
| Executable discovery | Search · "Login shell PATH, then Homebrew and ~/.local/bin". Re-check · "At launch and before each attempt" · a switch, on. Custom paths · "none" · "Add…" |
| Defaults & concurrency | **Default providers**: Backend tasks · "Claude Code" · "Change". Mobile tasks · "Codex" · "Change". Validator · "Command runner only · no agent". **Concurrency**: Parallel attempts · "3 across all projects" · "Edit". Per repository · "2" · "Edit". Per provider · "Follow provider limits; never retry on limit errors" |
| Git & GitHub | **Git**: Worktree root · `~/.jojo/wt` · "Change…". Branch prefix · `jojo/`. Sign commits · "Use your Git config". **GitHub**: Account · "Via gh CLI · kaanf" · pill "Connected". Push and PR · "Each needs its own authorization". Combined push + PR policy · "Off" · a switch, off |
| Workspace storage | App data · `~/Library/Application Support/Jojo`. Disk space · "Low-space warning below 10 GB free". Backups · "Daily · last: today 08:00" · "Back Up Now". Restore · "From a backup file" · "Restore…" |
| Evidence retention | Sealed manifests · "Kept forever". Raw logs · "90 days, then summarized" · "Edit". Sensitive values · "Masked in stored logs" |
| Security & approvals | **Approvals**: Approval binding · "Exact revision and content hash" · "Always on". Scope expansions · "Ask every time". Baseline changes · "Ask every time". **Scanning**: OSV-Scanner · "Required before sealing evidence" |
| Keyboard shortcuts | Eight lines, each with a key cap: Command palette ⌘K, New feature ⌘N, Switch project or feature ⌘P, Dashboard … Recovery ⌘1 … ⌘4, Hide sidebar ⌃⌘S, Inspect selection ⌘I, Search output ⌘F, Send message ⌘↵ |
| Appearance | Theme · "Follow system" · segments "System", "Light", "Dark". Density · "Compact" · segments "Compact", "Comfortable". Reduce motion · "Follow system" |

## 4. What the board does not draw

A provider that is not found or not signed in, a changed binary (the warning of D3), a disconnected core, a value that is being saved or was refused, an empty section, and the light appearance.

## 5. What the app shows today

Rebuilt on 2026-11-18 from `SettingsMac.dc.html` (step 6) in `macos/JojoApp/Sources/JojoApp/Settings.swift`: a glass section list ("Providers": Claude Code, Codex; "Jojo": Core and the nine sections of the board) and the chosen section's groups as cards. The screen reads `Executables` (D3), `ExternalActions` (M5, for the sign-in of git and gh) and `Projects` (for the capacity) while it is shown.

### 5.1 Mapping

| Board | App |
|---|---|
| Provider: executable, version "Compatible", discovery | What the core found; the version is "Compatible" when it is the profiled release, else "Not the profiled release" |
| Authentication | The sign-in kind passed at launch; the provider's sign-in state is not reported |
| Capabilities · reported by CLI | "Capabilities": the structured event stream and headless mode as Jojo uses them; session resume "Not built in Jojo"; token reporting not reported |
| Usage | Tokens and plan limits "Not reported" |
| Executable discovery | Login shell PATH, re-check before every run, launch options as custom paths |
| Defaults & concurrency | Providers per project (shown on Projects), the validator and review of M4, repair; parallel attempts from the newest project record ("2 across all projects") |
| Git & GitHub | Worktree root, branch prefix `jojo/plan/`, sign commits not reported, git's credential helper and gh's account as the core last read them with Ready / Missing / Checking; push and pull request each authorized on its own; no combined policy |
| Workspace storage, Evidence retention | The roots passed at launch; disk space, backups, restore and retention not built |
| Security & approvals | Approval binding; four approval kinds, no approve-all; scope expansions, baseline changes and scanning not built |
| Keyboard shortcuts | The three the app has |
| Appearance: Theme System / Light / Dark | **Writable**: the app's one setting, kept in this Mac's app settings (not the core), applied at once and at launch; Light is the default. Density and reduce motion are not settings |

The system Settings window (⌘,) says that Jojo's settings are in the main window.

### 5.2 States

A lost or incompatible core (banners; what the core reported is not shown), a provider without launch options (an empty form), the executables being read, a changed executable (a warning).

### 5.3 Known gaps

- The core reports no capability, sign-in state, token count or limit of a provider.
- No setting of the core is writable here: capacity, providers and repair are set per project on Projects.
- Disk space, backups, restore, retention, scanning, sign commits and the command palette are not built.
- The git and gh sign-in rows read the configuration and `gh auth status`; whether a push can really authenticate shows only when it runs.

### 5.4 Repair and review (M4)

Repair is automatic, at most two tries, then a hold; the reviewer is the other provider; both are set per project and shown on Projects.

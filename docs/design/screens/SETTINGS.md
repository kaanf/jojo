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

Built in `macos/JojoApp/Sources/JojoApp/Settings.swift` on the shared surfaces of COMPONENTS.md, sections 14 to 21. Before it, this destination showed "Settings & Providers is not built yet".

**What the app knows about settings and providers.** Three sources, and nothing else:

1. **The helper's Hello answer.** It has `compatible`, `epoch`, `max_frame`, `max_page`, `admission`, `commands`, `queries` and `slice` (`crates/jojo-helper/src/service.rs`). Until this step `HelperClient` kept `epoch` and `compatible` only. It now keeps the rest too (`HelperClient.hello`, a `HelperHello`): the largest frame, the largest page, the two name lists and `slice`, which says that the helper holds a slice configuration. Hello's `admission` is a fixed text, not the session's admission, and is not kept. No request was added; the protocol and the Rust side are unchanged.
2. **The connection and the snapshot's admission**, through `ShellStatus`, the value the sidebar's status well shows. The screen builds the same value and prints its words, so the two cannot disagree.
3. **The options the app itself was started with** (`LaunchOptions`), which it passes to the helper as given: the helper executable, `--data-root`, `--worktree-root`, `--disposable-root`, and per provider `--…-executable`, `--…-version`, `--…-sha256` and `--…-auth`. `AppModel` now keeps them.

**The helper reports no provider configuration and no setting.** No request returns the executable, version, digest or sign-in kind the helper holds; the snapshot has no settings row (its sections are `aggregates`, `claims` and `effects`); the scheduler's capacities are in the core's database without a request that reads them. So the provider sections show the launch options under the heading "Passed to the core at launch" and say that the core does not report them back (decided by the human on 2026-11-16; a new query was the alternative and is out of this step). The one thing the core confirms is `slice`.

**Nothing that could be a credential is shown.** Of the provider options the screen shows the executable path, the version text, the digest of the executable and the sign-in kind (the only accepted value is `native-subscription`). It does not show `--codex-home`, `--claude-home` (the directories the CLIs keep their sign-in in) or `--claude-user` (an account name).

**The screen is read-only** (no control that changes a setting). The app has no request that writes a setting (`Request` in `Protocol.swift`). The board's buttons ("Change…", "Edit", "Add…", "Back Up Now", "Restore…"), switches and segmented controls are therefore not built, and no value stands in their place as if it were chosen. A setting without a source says "Not reported" or "Not built" in `text3`.

**All ten sections of the board stay, and one is added** (decided by the human on 2026-11-16). "Core" is not on the canvas: it holds the connection and the Hello answer, which have no place on the board.

**Decisions in force are shown where the board disagrees, and marked as decisions.** A line that states a decision that is not built yet says so ("Decided, not built").

### 5.1 Mapping

| Canvas | App today |
|---|---|
| Toolbar, breadcrumb | The shell breadcrumb stays "Jojo › System › Settings & Providers" |
| Sidebar counter | None, as on the board. Nothing about a setting is work that waits |
| Section list | As on the board, 220 wide, with "Core" first in the second group. The chosen section lives in the screen; "Claude Code" at first |
| Title | As on the board |
| Claude Code, Codex · Executable | Heading "Passed to the core at launch". Path, Version and "SHA-256 digest" are the launch options, the path and digest monospaced; "Not passed" for one that is missing. Discovery: "Not built. The path is a launch option." No "Change…", no "Compatible" pill: a capability check is not reported. Without any option of the provider: the Empty form "No Claude Code options were passed at launch" · "Jojo was started without them. The core reports no provider configuration, so nothing else is known here." |
| · Authentication | Sign-in: "Attested at launch as native-subscription. The sign-in itself is not checked." No "Ready" pill: the sign-in state is not reported. Credentials: "Handled by the provider's CLI. The only sign-in kind the core accepts is native-subscription; there is no API key option." (the board's sentence "never reads, stores or relays" is a wider claim than this step verified) |
| · (not on the canvas) Reported by the core | One line, "Slice configuration": "Present. The core does not say which providers it holds.", "Absent", or "Unknown: the core is not connected." (and "Not reported" when Hello has no such field) |
| · Capabilities, Usage | Not built. A footnote: "The core does not report these values back, and it reports no capability, sign-in state, token count or limit of a provider. It hashes the executable itself and refuses one whose digest differs." |
| Executable discovery | Search and Re-check: "Not built. Each executable is a launch option." Changed binary: "Refused, not warned: the core runs only the executable whose digest was passed." A footnote names the decision: discovery is decided (D3) and not built. No switch, no "Add…" |
| Defaults & concurrency · Default providers | One line, "Provider per task": "Not reported. Nothing assigns a provider to a kind of task yet." The board's "Backend tasks" and "Mobile tasks" are not built |
| · Validator | D2: the board's "Command runner only · no agent" no longer holds. Validator: "The core's host validation of a candidate. No reviewing agent yet." Repair: "Decided, not built: automatic, at most two tries, then a human hold." |
| · Concurrency | D4 is open. Parallel attempts: "Not reported, and not decided. The limit is a value in the core's scheduler; no request reads it." Per repository and per provider: "Not reported". No number is shown, neither the board's 3 and 2 nor the roadmap's 2 |
| Git & GitHub · Git | Worktree root: the `--worktree-root` option, monospaced, or "Not passed. The core uses its default, which it does not report." Branch prefix, Sign commits: "Not reported" |
| · GitHub | Account: "Not built". Push and pull request: "Decided, not built: each is its own approval kind." (M5) Combined approval: "None. There is no approve-all." (the board has a switch for a combined policy; the decision in force is four approval kinds and no approve-all, so there is nothing to switch) |
| Workspace storage | App data: the `--data-root` option or "Not passed. The core uses its default, which it does not report." Not on the canvas: Disposable root, the `--disposable-root` option or "Not passed". Disk space, Backups, Restore: "Not built" |
| Evidence retention | Sealed manifests: "Not built. There is no request that seals evidence." Raw logs, Sensitive values: "Not reported" |
| Security & approvals · Approvals | Approval binding: "A decision names the document's revision and digests; the core refuses a stale one" · "Always on". Not on the canvas, Approval kinds: "Specification and plan. Push and pull request are decided and not built. There is no approve-all." Scope expansions, Baseline changes: "Not built" |
| · Scanning | OSV-Scanner: "Not built" |
| Keyboard shortcuts | The three the app has (`JojoApp.swift`), each with a key cap: Quit Jojo ⌘Q, Show Jojo ⌘0, Show Inspector ⌥⌘I. A footnote: "The command palette and the other shortcuts of the design are not built." |
| Appearance | Theme: "Follows the system". Density: "One density". Reduce motion: "No setting". A footnote: "The app has no switch for these yet." No segmented control (COMPONENTS.md, section 13) |
| (not on the canvas) Core · Connection | Three lines with the words of the sidebar's status well: Core (`ShellStatus.core`), Session (the helper epoch, or "None"), Admission (`ShellStatus.admission`). Then Helper executable, the path the app launched, or "None configured" |
| · Protocol | Only while the helper is connected or incompatible. Compatibility: "The helper accepts this app's protocol version" with the evidence pill "Compatible" (Passed form), or "The helper speaks another version; diagnostics only" with "Mismatch" (Failed form). Largest frame ("n bytes"), Largest page ("n rows"), Slice configuration |
| · Requests the core offers | Two lines, "Commands" and "Queries": the names of Hello as kind tags that wrap. A footnote: "None of them changes a setting, so every value on this screen is read-only." |

### 5.2 States

- Loading (`notStarted`, `handshaking`): the Core section says "Waiting for the core's answer…" in a notice well in place of Protocol and the request lists. The other sections do not wait: launch options are known at once.
- Disconnected (`channelLost`, `ended`, `launchFailed`): danger banner "Jojo core isn't responding" · "What the core reported is not shown. Launch options are shown as the app passed them." The Core section keeps the three status lines and says "Nothing is reported while the core is not connected." There is no "Reconnect" yet.
- Incompatible protocol: danger banner "Jojo core speaks another protocol version" · "Diagnostics only. The values below are what it answered."
- Recovery required: no banner of its own. The Core section's Admission line and the sidebar say it with the same words.
- Empty: a provider without launch options (5.1); the Empty form is 180 high and "Reported by the core" follows it.
- Requested, Stale, Error, Permission denied and Unknown outcome have no source on this screen: it sends no request of its own.
- Width: the section list keeps 220; the shown section takes the rest, at most 760.

### 5.3 Open points

- The system's Settings window (⌘,) still says "Jojo Phase 1 has no settings." (`JojoApp.swift`); it was not changed in this step.
- The shortcut list is written in `Settings.swift` beside the three menu commands of `JojoApp.swift`; a new shortcut has to be added in both.
- A query that returns the helper's provider configuration and the scheduler's capacities would let the provider sections and "Concurrency" show reported values.

### 5.4 Values that are not on the canvas

| Point | Value in the app |
|---|---|
| Section list | 220 wide (the canvas starts at 230), background `sidebar`, edge `sidebarEdge` on the trailing side |
| "Core" tile | `#5A5A64`, the board's neutral tile |
| Tile glyphs (SF Symbols) | Claude Code `sparkle`, Codex `chevron.left.forwardslash.chevron.right`, Core `cpu`, Executable discovery `magnifyingglass`, Defaults & concurrency `slider.horizontal.3`, Git & GitHub `arrow.triangle.branch`, Workspace storage `internaldrive`, Evidence retention `archivebox`, Security & approvals `lock.shield`, Keyboard shortcuts `keyboard`, Appearance `paintbrush` |
| Setting line | The label keeps 170 and the value takes the rest (the canvas grows them 1 to 2); a value without a source in `text3` |
| Section tile, section item, setting line | COMPONENTS.md, section 21 |

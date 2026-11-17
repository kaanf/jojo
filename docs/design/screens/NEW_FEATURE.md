# New Feature

Source: the canvas "Jojo macOS", page "Product", board "New Feature" (`NewFeatureMac.dc.html`, 1440 × 1160, light by default). Read on 2026-11-17 from version `1794931431-3abf`; the board file is byte-identical to the one in version `1794922102-b518` (sha256 `e10e58fb…8a34`). Tokens, components and states are named as in [DESIGN_SYSTEM.md](../DESIGN_SYSTEM.md), [COMPONENTS.md](../COMPONENTS.md) and [STATES.md](../STATES.md). It replaces the New Feature part of the wave 2 Feature Chat document (in the git history at commit `c3a9d64`); [FEATURE_CHAT.md](FEATURE_CHAT.md) is now the chat of a held feature.

Built as the first screen of step 4 of the Liquid Glass order (STATUS.md), without a core or protocol change (DESIGN_ROADMAP_DIFFERENCES.md, E13). New Feature is Feature Chat holding no feature; the Dashboard's New Feature (⌘N) and a chat's "New Feature" open it.

## 1. The board

No sidebar item is chosen on the board. The content column (padding 10 6 14 10, gap 16):

| Element | Board | Design system |
|---|---|---|
| Header | "New Feature", sub line "Workspace · Crew · New Feature", an icon group (appearance switch) | header row (section 4.3), `pageTitle` |
| Card | one card, at most 780 wide, centered, radius 22, padding 28 30, gap 22 | card (section 5.1) |
| Heading | "Describe the feature" (26 bold, −0.025 em); "Say what users should be able to do. Jojo investigates the code and asks before anything is planned." (14, line height 1.55, `text2`) | — |
| Project | label (90 wide) and a segmented control "Crew", "Jojo" | segmented control (section 5.6) |
| What should it do? | label, a field at least 120 high with the placeholder "Players should be able to share a link that drops a friend straight into my lobby…", and "No need to name files, APIs or architecture. That's what discovery is for." | field (section 5.6) |
| Repositories · optional | capsule chips with a check box, 30 high: "Let Jojo decide" (on), "crew-api", "crew-app"; "Let Jojo decide" excludes the others, and turning every repository off turns it back on | check box (section 5.6) |
| Reference files · optional | a dashed drop zone (radius 14, padding 14 16, 1.5 dashed `dotOff`): a 36 × 36 tile on `accentTint`, "Drop files here", "Specs, tickets, screenshots", "Choose…" | — |
| Design references · optional | the same zone: "Images or exported frames", "PNG, JPG, PDF", "Figma link import: not available yet", "Choose…" | — |
| Technical constraints · optional | a field at least 64 high, "For example: no new infrastructure" | field |
| What happens next | a glass panel (radius 16, padding 14 16): "1 Jojo reads the selected repositories, read-only." "2 It asks clarifying questions in Feature Chat." "3 It drafts a PRD for your approval. Nothing is executed before you approve a plan." (numbers in `mono` `accent` bold) | glass panel (section 4) |
| Footer | "Cancel esc" (capsule, 34 high; opens the Dashboard), then "Start Discovery ⌘↵" (primary, 34 high) | buttons (section 3) |

The two zones sit in a grid of columns at least 280 wide. The board has no provider control.

## 2. Where each value comes from

| Element | Source |
|---|---|
| Projects | `Projects` (protocol 1.3), read while New Feature is shown |
| Repositories | The chosen project's repositories that the chat index lists as registered (`ChatIndex`, protocol 1.2) |
| Provider of the lead | The chosen project's `default_provider`, said beside the project ("Lead: Codex, the project's default") |
| Start Discovery | One `ChatTurn` (protocol 1.2): a new feature identity, kind `Discover`, the repository, the provider and the message |
| Message | The description, then "## Technical constraints" and one "## Reference file: <name>" section per text file, each with its text |
| Outcome | `WorkspaceModel.turnOutcome`: sent ("Requested"), refused with the core's code, unknown. An accepted turn holds the new feature, and the screen becomes its chat |

The core stores the message with the turn (`lead_turns.message`) and carries the first turn's message into every later prompt, so the constraints and references reach every lead turn of the feature. The core takes at most 16 KiB of a human's text (`MAX_HUMAN_TEXT_BYTES`); the form says so before sending.

### Not built, and why

| Board element | Why |
|---|---|
| Several repositories, "Let Jojo decide" among several | A feature reads one registered repository until a plan can span several (2.4, after the demo). "Let Jojo decide" chooses the project's only repository; with several it says so and a repository is chosen |
| Drop onto the zones | Files are chosen with "Choose…" only |
| Screenshots, images, PDF, Figma | The lead reads text only; the design references zone says "not available yet" for each |
| Appearance switch | The appearance follows the system; the shell's Refresh and Show Inspector stand in that group |

## 3. Decisions

Decided by the human on 2026-11-17 (all four proposals as given): (1) one repository per feature; "Let Jojo decide" chooses the project's only one; (2) constraints and text reference files travel in the first message, images, PDF and Figma are "not available yet"; (3) the lead runs on the project's default provider, with no provider control; (4) the list of earlier chats leaves New Feature, and the Dashboard's project table opens a feature's chat.

## 4. Implementation notes

- `macos/JojoApp/Sources/JojoApp/NewFeature.swift`: `NewFeatureState` (a plain value: the chosen project, its repositories, the repository the turn reads, the provider, the composed message and why Start Discovery is unavailable), `NewFeatureView` and the components of COMPONENTS.md, section 26. `FeatureChatScreen` shows it while no feature is held and reads `ChatIndex` and `Projects` while shown.
- A reference file is read whole as UTF-8 from the file the human chose in the system's open panel (`fileImporter`, security-scoped). A file that is not UTF-8 text is named and left out. The app keeps no copy; the text is in the message.
- The header says "New Feature" and "Workspace · <project> · New Feature" (`AppShell`'s `heading`); the chosen project lives in `MainView` so the header and the form agree.
- Escape (Cancel) and ⌘↵ (Start Discovery) act only while the screen is shown.
- Review aid: `JOJO_SNAPSHOT_DIR=<directory> swift test --filter S4NewFeatureTests` writes `new-feature-…`, `-filled-…`, `-narrow-…`, `-sent-…`, `-refused-…`, `-no-project-…`, `-loading-…`, `-closed-…` and `-disconnected-…` in dark and light.

## 5. What the app shows today / Known gaps

New Feature shows the projects the core holds, the chosen project's registered repositories and default provider, and the form of the board; Start Discovery sends one discovery turn whose message carries the description, the constraints and the text of each reference file. Known gaps:

- One repository per feature; a project with several needs a choice.
- Images, PDF and Figma references are not available; text references count toward the core's 16 KiB.
- No drag and drop onto the zones.
- Without a project nothing can start; "Create Project" opens Projects.
- The app sends no request that registers a repository (`RegisterRepository` is not in its request list); a repository is registered with the core before New Feature can offer it.

# Jojo design documents

The design of the macOS app, written down so that SwiftUI code can be built and reviewed against text instead of against a canvas. Read these before changing anything under `macos/JojoApp/Sources/JojoApp`.

| File | What it holds |
|---|---|
| [DESIGN_SYSTEM.md](DESIGN_SYSTEM.md) | Rules, color tokens for dark and light, elevation, typography, spacing, shape, sizes, icons, interaction states, SwiftUI mapping |
| [COMPONENTS.md](COMPONENTS.md) | The status model and every reusable component: pills, tags, buttons, app shell, cards, lists, nodes, dialogs, palette, inspector, diff, banners; and the gaps in the canvas |
| [STATES.md](STATES.md) | The nine application states and the four discovery states, with their copy, and what the helper reports for each today |
| [screens/RECOVERY_CENTER.md](screens/RECOVERY_CENTER.md) | Recovery Center and launch review: the two canvas boards, and what the app shows of them today |
| [screens/AGENT_RUNS.md](screens/AGENT_RUNS.md) | Agent Runs: the canvas board, and what the app shows of it today |
| [screens/VERIFICATION.md](screens/VERIFICATION.md) | Verification & Evidence: the canvas board, and what the app shows of it today |
| [screens/SOURCE_CONTROL.md](screens/SOURCE_CONTROL.md) | Source Control: the canvas board, and what the app shows of it today |
| [screens/APPROVAL_INBOX.md](screens/APPROVAL_INBOX.md) | Approval Inbox: the canvas board, and what the app shows of it today |
| [screens/SPECIFICATIONS.md](screens/SPECIFICATIONS.md) | Specifications: the canvas board, and what the app shows of it today |
| [screens/ACTIVITY.md](screens/ACTIVITY.md) | Activity & Audit: the canvas board, and what the app shows of it today |

## Source

Claude Design canvas "Jojo macOS", https://claude.ai/artifact/UuyW4DFxPEciFKZm8ZGM24, version `1794801829-c71e`, read on 2026-11-16.

| Document | Canvas boards (page "Components & states") |
|---|---|
| DESIGN_SYSTEM.md | "Themes and tokens" (`ThemeTokens.dc.html`), and the shell of "Dashboard" (`Main.dc.html`) |
| COMPONENTS.md | "Components and status model" (`Components.dc.html`), "Themes and tokens", "Dashboard" |
| STATES.md | "States" (`States.dc.html`) |
| screens/RECOVERY_CENTER.md | "Recovery Center" (`RecoveryCenter.dc.html`), "Recovery Center · launch review" (`RecoveryLaunch.dc.html`) |
| screens/AGENT_RUNS.md | "Agent Runs" (`AgentRuns.dc.html`) |
| screens/VERIFICATION.md | "Verification & Evidence" (`Verification.dc.html`) |
| screens/SOURCE_CONTROL.md | "Source Control" (`SourceControl.dc.html`) |
| screens/APPROVAL_INBOX.md | "Approval Inbox" (`ApprovalInbox.dc.html`), read at version `1794818355-75fa` |
| screens/SPECIFICATIONS.md | "Specifications" (`Specification.dc.html`), read at version `1794818355-75fa` |
| screens/ACTIVITY.md | "Activity & Audit" (`Activity.dc.html`), read at version `1794823138-cd90` |

Values were read from the boards' markup and stylesheet, not measured from a picture. Section 4 of STATES.md and section 10 of DESIGN_SYSTEM.md (the mapping to the helper and to SwiftUI) are not from the canvas; they were written for this project.

## Which source wins

1. The token table of "Themes and tokens". The canvas states that it is read from the Dashboard stylesheet, which is the reference implementation.
2. The Dashboard board.
3. The component and state boards. They are drawn in dark only with literal colors, and differ from the tokens in a few values; COMPONENTS.md, section 12, lists each difference.

When the canvas changes, update these files in the same step and note the new version above. When code needs a value these files do not have, add it here first, marked as not from the canvas.

## Screens

The canvas has 23 boards. Which milestone builds which screen, and where the design and the roadmap disagree, is in [DESIGN_ROADMAP_DIFFERENCES.md](../roadmap/DESIGN_ROADMAP_DIFFERENCES.md), section 4. Seven screens are transcribed (the table above); each of the others is written down when it is built.

| Page | Boards |
|---|---|
| Product (dark) | Dashboard, New Feature, Projects, Approval Inbox, Recovery Center, Recovery Center · launch review, Activity & Audit, Settings & Providers, Feature Chat, Specifications, Execution Plan, Agent Runs, Source Control, Verification & Evidence |
| Light theme | Dashboard · light (the same Dashboard on light tokens), and five alternatives that are explorations, not the reference: Dashboard D · Minimal light, Dashboard E · Focus light, Dashboard F · Atelier, Feature Chat · Focus light, Feature Chat F · Atelier |
| Components & states | Components and status model, States, Themes and tokens |

## Open decisions

1. ~~Font.~~ Decided 2026-11-16: system fonts (SF Pro, SF Mono).
2. **Light values for the components listed in COMPONENTS.md, section 12.** Needed before those components ship in the light appearance.
3. **Pressed and keyboard-focus states.** Not drawn on the canvas; the system defaults are used until they are.
4. **The light alternatives (D, E, F).** Whether any of them replaces the reference Dashboard.

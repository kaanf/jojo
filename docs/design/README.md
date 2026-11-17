# Jojo design documents

The design of the macOS app, written down so that SwiftUI code can be built and reviewed against text instead of against a canvas. Read these before changing anything under `macos/JojoApp/Sources/JojoApp`.

| File | What it holds |
|---|---|
| [DESIGN_SYSTEM.md](DESIGN_SYSTEM.md) | Rules, Liquid Glass materials, shadow levels, color tokens for light and dark, typography, spacing, shape, sizes, icons, interaction states, SwiftUI mapping |
| [COMPONENTS.md](COMPONENTS.md) | The status model and every reusable component: pills, tags, buttons, app shell, cards, lists, nodes, alerts, palette, menus, inspector, diff, banners, fact strip |
| [STATES.md](STATES.md) | The placeholder, the nine application states and the four discovery states, with their copy, and what the helper reports for each today |
| [screens/RECOVERY_CENTER.md](screens/RECOVERY_CENTER.md) | Recovery Center and launch review: the two canvas boards, and what the app shows of them today |
| [screens/AGENT_RUNS.md](screens/AGENT_RUNS.md) | Agent Runs: the canvas board, and what the app shows of it today |
| [screens/VERIFICATION.md](screens/VERIFICATION.md) | Verification & Evidence: the canvas board, and what the app shows of it today |
| [screens/SOURCE_CONTROL.md](screens/SOURCE_CONTROL.md) | Source Control: the canvas board, and what the app shows of it today |
| [screens/APPROVAL_INBOX.md](screens/APPROVAL_INBOX.md) | Approval Inbox: the canvas board, and what the app shows of it today |
| [screens/SPECIFICATIONS.md](screens/SPECIFICATIONS.md) | Specifications: the canvas board, and what the app shows of it today |
| [screens/ACTIVITY.md](screens/ACTIVITY.md) | Activity & Audit: the canvas board, and what the app shows of it today |
| [screens/SETTINGS.md](screens/SETTINGS.md) | Settings & Providers: the canvas board, and what the app shows of it today |
| [screens/EXECUTION_PLAN.md](screens/EXECUTION_PLAN.md) | Execution Plan: the canvas board, and what the app shows of it today (the Start of one step; no graph) |
| [screens/FEATURE_CHAT.md](screens/FEATURE_CHAT.md) | Feature Chat and New Feature: the two canvas boards, and what the app shows of them today (M2) |

## Source

Claude Design canvas "Jojo macOS", https://claude.ai/artifact/UuyW4DFxPEciFKZm8ZGM24.

**The design system** (DESIGN_SYSTEM.md, COMPONENTS.md sections 1 to 12, STATES.md sections 1 to 4) was rewritten on 2026-11-17 from version `1794922102-b518`, page "Components & states": "Components and status model" (`ComponentsMac.dc.html`), "States" (`StatesMac.dc.html`), "Themes and tokens" (`ThemeTokensMac.dc.html`), and the shell of the product boards (`DashboardMac.dc.html`, `AgentRunsMac.dc.html`, `RecoveryLaunchMac.dc.html`). These are the macOS 26 Liquid Glass boards; light is their default appearance.

**The screen documents** under `screens/` were transcribed earlier from the first boards, which the canvas now keeps on its "Archive" page (version `1794801829-c71e` to `1794837270-18f1`; the version of each is in its document). Each screen document is rewritten from its `…Mac.dc.html` board on the "Product" page when that screen is rebuilt.

## Which source wins

1. The token table of "Themes and tokens". The canvas states that it is read from the stylesheet the product screens use.
2. The stylesheet of the product boards (the `.mac` rules), for values the table does not list (blur, shadow levels, component sizes).
3. The "Components and status model" and "States" boards.
4. A product board, for a value only it has.

When the canvas changes, update these files in the same step and note the new version above. When code needs a value these files do not have, add it here first, marked as not from the canvas.

## Screens

The canvas has 52 boards on seven pages. Which milestone builds which screen, and where the design and the roadmap disagree, is in [DESIGN_ROADMAP_DIFFERENCES.md](../roadmap/DESIGN_ROADMAP_DIFFERENCES.md), section 4.

| Page | Boards | Status |
|---|---|---|
| Product | Dashboard, New Feature, Projects, Approval Inbox, Recovery Center, Recovery Center · launch review, Activity & Audit, Settings & Providers, Feature Chat, Specifications, Execution Plan, Agent Runs, Source Control, Verification & Evidence (`…Mac.dc.html`) | The reference. To be built as drawn (decided 2026-11-17) |
| Onboarding | Onboarding (machine setup), New Project | The reference. Deferred by the human on 2026-11-17 |
| Components & states | Components and status model, States, Themes and tokens | The design system |
| Light theme | Dashboard · light and five alternatives (D, E, F, Feature Chat light and Atelier) | Explorations of the first design |
| Premium directions | Dashboard G · Obsidian, Dashboard H · Precision | Explorations |
| Mascot drafts | Four boards | On hold |
| Archive | The first boards (dark) | Superseded; the screen documents were transcribed from them |

## Open decisions

1. ~~Font.~~ Decided 2026-11-16: system fonts (SF Pro, SF Mono).
2. ~~Light values for the components.~~ Settled 2026-11-17: the new boards define every token for light and dark.
3. **Pressed and keyboard-focus states.** Not drawn on the canvas; the system defaults are used until they are.
4. **The explorations (Light theme D, E, F; Premium G, H; mascot).** The Product page's Dashboard is the reference unless the human picks another.

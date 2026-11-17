# Jojo design system: tokens

Materials, colors, type, spacing, shape and elevation for the SwiftUI app. Transcribed from the Claude Design canvas "Jojo macOS", page "Components & states", board "Themes and tokens" (`ThemeTokensMac.dc.html`), and from the stylesheet the product boards share (the `.mac` rules of `DashboardMac.dc.html`). Source version and conflict rules: [README.md](README.md).

This replaces the first design system (dark-first, opaque surfaces, `--jj-*` tokens, accent `#2F6FEB`), which is kept on the canvas's Archive page. The app's shell and status components use this system since 2026-11-17; screens move as they are rebuilt (section 12).

The canvas is written in CSS pixels. One CSS pixel is one SwiftUI point.

## 1. Rules

The six rules of the board, for every screen.

1. **Tokens, not hex.** A view reads a semantic token; the appearance (light or dark) decides the value. Nothing else changes between light and dark.
2. **Glass for chrome, cards for content.** Sidebar, inspector, toolbar capsules, menus, sheets and banners are glass. Lists, documents and tables sit on cards. Never stack glass on glass.
3. **Status darkens in light.** Status hues keep their meaning, but status text uses deep shades in light (blue `#0040dd`, orange `#9a4300`, green `#17712f`, red `#c11a20`) so text on its tint passes 4.5:1.
4. **One filled button per area.** Only the primary action is filled with the accent. Everything else is a glass capsule, a text button or a menu item.
5. **Shadows are a setting.** The window shadow and the glass rim stay; decorative glows go at the Reduced level, which is the default (section 3).
6. **Shape carries the status family.** Capsule with a dot = operational, rounded square with a symbol = evidence, uppercase outline tag = document. Color alone never decides (COMPONENTS.md, section 1).

Light is the default appearance of the boards. The app follows the system appearance, and Settings › Appearance can force Light or Dark (SETTINGS.md).

## 2. Materials

Four surfaces (COMPONENTS.md, section 4).

| Token | Use | Light | Dark |
|---|---|---|---|
| `window` | Window material | `rgba(246,246,248,.74)` | `rgba(28,28,32,.72)` |
| `glass` | Sidebar, inspector, banners, alerts, menus | `rgba(255,255,255,.58)` | `rgba(58,58,64,.46)` |
| `glassStrong` | Toolbar capsules, segmented controls, secondary buttons | `rgba(255,255,255,.78)` | `rgba(64,64,70,.66)` |
| `card` | Content cards and lists | `rgba(255,255,255,.82)` | `rgba(255,255,255,.065)` |
| `cardHover` | Hover, input fields, elevated rows, the chosen segment, task nodes | `rgba(255,255,255,.95)` | `rgba(255,255,255,.10)` |
| `rim` | Glass top highlight | `rgba(255,255,255,.85)` | `rgba(255,255,255,.22)` |
| `rimLow` | Glass inner edge | `rgba(255,255,255,.35)` | `rgba(255,255,255,.06)` |
| `shadow` | Window drop shadow | `rgba(16,24,40,.14)` | `rgba(0,0,0,.5)` |

Blur and saturation, from the stylesheet:

| Material | Backdrop | Edge |
|---|---|---|
| Window | blur 50, saturate 180% | 0.5 ring `rgba(0,0,0,.18)`, inner 0.5 ring `rimLow`, drop shadow 0 30 80 `shadow` |
| Glass | blur 30, saturate 190% | inner top line 1 `rim`, inner bottom line 1 `rimLow`, inner 0.5 ring `rimLow` |
| Capsule (`glassStrong`) | blur 24, saturate 190% | inner top line 1 `rim`, inner 0.5 ring `rimLow` |
| Card | none | inner 0.5 ring `sep` |
| Well | none, fill `grayBg` | none |

## 3. Shadow levels

A setting with three levels; **Reduced** is the default.

| Element | Full | Reduced (default) | None |
|---|---|---|---|
| Window | ring + drop shadow | ring + drop shadow | ring only |
| Glass | edges + 0 6 24 `rgba(0,0,0,.08)` | edges only | inner 0.5 ring `sep` |
| Capsule | edges + 0 2 10 `rgba(0,0,0,.08)` | edges only | inner 0.5 ring `sep` |
| Primary button | inner top 1 `rgba(255,255,255,.35)` + glow 0 2 10 `rgba(0,113,227,.35)` | inner top 1 `rgba(255,255,255,.3)` | none |
| Chosen segment | 0 1 4 `rgba(0,0,0,.14)` + inner 0.5 `sep` | the same | inner 0.5 `sep` |

## 4. Color tokens

### Text and lines

| Token | Use | Light | Dark |
|---|---|---|---|
| `text` | Titles and body | `#1d1d1f` | `#f5f5f7` |
| `text2` | Secondary copy | `#5b5b61` | `#b0b0b6` |
| `text3` | Meta, labels, timestamps, uppercase section labels | `#6e6e73` | `#98989e` |
| `sep` | Hairlines, row separators, card rings | `rgba(0,0,0,.09)` | `rgba(255,255,255,.10)` |
| `dotOff` | Empty lifecycle steps and dots, bars not done | `rgba(60,60,67,.22)` | `rgba(235,235,245,.20)` |

### Accent

| Token | Use | Light | Dark |
|---|---|---|---|
| `accent` | Primary button, selection, links, sidebar glyphs, checked boxes | `#0071e3` | `#0a84ff` |
| `accentTint` | Selected rows and sidebar items, chosen options, the running dot's halo | `rgba(0,113,227,.13)` | `rgba(10,132,255,.24)` |

Text on `accent` is white in both appearances.

### Status

Six hues. The text color is used for status text and glyphs; the `…Bg` tint sits behind it.

| Hue | Meaning | Text light | Text dark | Tint light | Tint dark |
|---|---|---|---|---|---|
| `blue` | Running, links, proposed | `#0040dd` | `#6cb2ff` | `rgba(0,113,227,.12)` | `rgba(10,132,255,.20)` |
| `orange` | Needs you, blocked, stale, incomplete | `#9a4300` | `#ffb54d` | `rgba(255,149,0,.16)` | `rgba(255,159,10,.18)` |
| `green` | Completed, passed, sealed, approved | `#17712f` | `#4cd97b` | `rgba(52,199,89,.16)` | `rgba(48,209,88,.17)` |
| `red` | Failed, held, recovery required, destructive | `#c11a20` | `#ff7a72` | `rgba(255,59,48,.13)` | `rgba(255,69,58,.18)` |
| `purple` | Requested, review, needs approval, eligible | `#7428b0` | `#d79bff` | `rgba(175,82,222,.14)` | `rgba(191,90,242,.20)` |
| `gray` | Idle, waiting, paused, unknown, not run, superseded | `#55555a` | `#aeaeb4` | `rgba(120,120,128,.14)` | `rgba(142,142,147,.20)` |

`grayBg` is also the hover of sidebar items and picker rows, the fill of disabled buttons, of skeleton bars and of wells.

### Fixed colors

The same in both appearances. Glyphs on them are white.

| Use | Value |
|---|---|
| Tile orange (Needs you, warnings, permission, stale) | `#ff9500` |
| Tile blue (running, plans) | `#0a84ff` |
| Tile red (blocked, errors, recovery required) | `#ff3b30` |
| Tile green (providers, success) | `#34c759` |
| Tile indigo (architecture decisions) | `#5e5ce6` |
| Tile teal (external actions) | `#30b0c7` |
| Tile purple (requested, reviews) | `#af52de` |
| Tile gray (disconnected, unknown, unavailable provider) | `#8e8e93` |
| App icon tile | linear gradient top to bottom `#6e6cf0` → `#3634a3` |
| Project badge (Crew on the boards) | linear gradient `#ff9f0a` → `#ff6b00` |
| Provider tile Claude Code | `#c2643a` |
| Destructive filled button (alerts only) | `#ff3b30` |
| Diff added line | `greenBg` |
| Diff removed line | `redBg` |

A project's badge color is chosen per project; the boards show one.

## 5. Typography

The system fonts: SF Pro (`-apple-system`) and SF Mono. Sizes in points.

| Style | Size | Weight | Tracking | Use |
|---|---|---|---|---|
| `referenceTitle` | 32 | bold | −0.03 em | Reference boards only |
| `kpiValue` | 30 | bold | −0.03 em | KPI number |
| `dashboardTitle` | 22 | bold | −0.02 em | Dashboard heading |
| `pageTitle` | 20 | bold | −0.02 em | Every other screen's heading |
| `panelTitle` | 17 | bold | −0.01 em | Panel heading, banner title |
| `inspectorTitle` | 16 | bold | 0 | Title of the selected object |
| `sectionTitle` | 15 | semibold | −0.01 em | Section heading (`h2`), alert title (bold), palette input |
| `body` | 13 | regular | 0 | Body, nav items, buttons (medium), menu items |
| `bodyStrong` | 13 | semibold | 0 | Row titles, KPI label |
| `secondary` | 12 | regular | 0 | `sub` lines in `text2`, key-value rows, lifecycle steps (medium; current step semibold), segmented buttons (medium), small buttons |
| `mono` | 12 | regular | 0 | Identifiers, commands, code (line height 1.6) |
| `label` | 11 | semibold | +0.02 em, uppercase | Section labels in `text3` |
| `sidebarSection` | 11 | semibold | 0 | Sidebar group headers in `text3` |
| `pill` | 11 | semibold | 0 | Operational and evidence pills, counters |
| `kbd` | 11 | regular | 0 | Shortcut hints in `text3` |
| `tag` | 10 | bold | +0.06 em, uppercase | Document tags |

Line heights: body copy 1.45 to 1.55; code 1.6.

## 6. Spacing

| Place | Value |
|---|---|
| Window padding (around sidebar, content and inspector) | 8 |
| Gap between sidebar, content and inspector | 8 to 14 |
| Content column padding | top 10, trailing 6 to 18, bottom 14 to 22, leading 10 to 14 |
| Gap between content sections | 16 to 20 |
| Sidebar padding | 14 top, 10 sides, 12 bottom; groups 14 apart, items 1 apart |
| Nav item | height 30, padding 0 9, gap 9 |
| Header | min height 44, gap 10 to 12 |
| Card padding | 14 to 16 (lists: 0, rows bring their own) |
| List row | min height 52, padding 8 14, gap 12 |
| Key-value row | padding 9 12, gap 12, label column 70 to 110 |
| Inspector | padding 16, sections 14 apart, 6 inside |
| Panel (reference boards, large groups) | padding 20 22, gap 14 |
| Grid gaps | KPI 12, cards 10 to 18 |

## 7. Shape

| Element | Corner radius |
|---|---|
| Window | 26 |
| Alert | 22 |
| Panel | 20 |
| Sidebar, inspector, palette, notification, banner, empty-state card | 18 to 20 |
| KPI card | 16 |
| Card, list card, lifecycle card | 14 |
| Node, menu, key-value card | 12 |
| Picker row | 10 |
| Nav item, field | 9 |
| Tile: 44 / 36 / 28 / 24 / 22 | 12–13 / 9–10 / 8 / 7 / 6 |
| Menu item | 7 |
| Evidence pill | 6 |
| Checkbox | 5 |
| Document tag | 4 |
| Pill, button, capsule, segmented control | full (999) |

## 8. Sizes

| Element | Size |
|---|---|
| Sidebar | 240 to 256 wide |
| Inspector | 280 to 320 wide |
| List column (Agent Runs, Recovery, Approval Inbox) | 220 to 250 wide |
| Button | height 32 (toolbar actions 34, inspector and empty-state actions 28), padding 0 14, gap 6 |
| Icon button | 32 × 32 |
| Segmented control | padding 2, segments 28 high, padding 0 13 |
| Search field (sidebar) | height 30, capsule |
| Field | min height 34, padding 0 12 |
| Operational pill | height 22, padding 0 10, dot 6, gap 6 |
| Evidence pill | height 22, padding 0 8, gap 5 |
| Status pill in a row | height 20, padding 0 8 |
| Counter pill in the sidebar | height 18, padding 0 6 |
| Document tag | height 18, padding 0 6, 1 pt ring in the text color |
| Dot | 8 (running: 3 pt `accentTint` halo) |
| Lifecycle bar | 2 high, at least 12 wide |
| Checkbox | 18 × 18, 1.5 ring `text3`; on: `accent` with a white check |
| Row tile | 28 × 28 |
| KPI tile | 24 × 24 |
| Empty-state tile, alert icon | 44 × 44 |
| Banner tile, notification icon | 36 × 36 |
| Menu | at least 240 wide, padding 5 |
| Palette | at most 460 wide |
| Alert | at most 320 to 380 wide |
| Content measure (centered reviews) | at most 860 |

## 9. Icons

The boards draw line icons with a 1.5 to 1.8 stroke on a 16-unit grid. The app uses SF Symbols; a symbol is chosen per glyph when its screen is built and recorded in that screen's notes. Sidebar glyphs are `accent`. Evidence pills use the text symbols ✓ ✕ ? ○ ↷ ◐ ◆ ⊘ ! (COMPONENTS.md, section 2.3). No emoji.

## 10. Interaction states

| State | Treatment |
|---|---|
| Nav item and picker row under the pointer | `grayBg` |
| Chosen nav item and picker row | `accentTint`, nav text semibold |
| List row under the pointer | `cardHover` |
| Highlighted menu item | `accent` fill, white text, shortcuts at 80% white |
| Disabled button | `grayBg` fill, `text3` text, and the reason beside it (COMPONENTS.md, section 3) |
| Unavailable menu item | `text3`, the reason as trailing text |
| Loading | skeleton bars; a spinner only in an empty-state tile, never over content |

Pressed and keyboard-focus states are not drawn; the system ones are used.

## 11. SwiftUI mapping

| Design concept | SwiftUI (macOS 26) |
|---|---|
| Appearance | The system color scheme, or the one Settings › Appearance forces. One token table per appearance, read from the environment |
| Window material | The system window with a transparent title bar and full-size content; content is laid out to the window edge |
| Glass panel (sidebar, inspector, banner) | `.glassEffect(.regular, in: RoundedRectangle(cornerRadius: 18))`, panels that sit side by side in one `GlassEffectContainer` |
| Capsule, secondary button, segmented control | `.buttonStyle(.glass)`, `.glassEffect(in: Capsule())` |
| Primary button | `.buttonStyle(.glassProminent)` tinted `accent` |
| Card | `card` fill and an inner 0.5 `sep` ring; no blur |
| Reduce Transparency | glass falls back to the opaque token value of the table (`glass`, `glassStrong` at alpha 1 over `window`) |
| Traffic lights, window corner, window shadow | the system window; never drawn by the app |
| Alert | `.alert` / `.confirmationDialog` where the layout fits; the boards' alert (key-value card, check box) as a sheet |
| Notification | `UNUserNotificationCenter` |
| Context menu | `.contextMenu` with `.keyboardShortcut` |
| Command palette | a glass panel opened with ⌘K |
| Tabular digits | `.monospacedDigit()` |
| Uppercase label or tag with tracking | `.textCase(.uppercase)` and `.tracking(size × em)` |
| Dashed gate outline | `StrokeStyle(lineWidth: 1.5, dash: [4, 3])` |

Accessibility: every status has a shape or a symbol as well as a color. Honor Reduce Transparency, Increase Contrast and Reduce Motion.

## 12. Moving the app (the Liquid Glass migration)

Step 1 is done (2026-11-17): `Theme.swift` holds these tokens, the materials and the type scale; the shell (window, sidebar, header row, inspector panel) and the status components are glass. The names of the first system remain as computed aliases of the new tokens until every screen is rebuilt (COMPONENTS.md, section 13). Each screen moves when it is rebuilt from its board (STATUS.md, steps 3, 4 and 6). COMPONENTS.md sections 14 to 24 describe the surfaces as built on the first system; each is rewritten when its screen moves.

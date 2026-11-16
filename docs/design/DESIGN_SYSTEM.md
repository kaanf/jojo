# Jojo design system: tokens

Colors, type, spacing, shape and elevation for the SwiftUI app. Values are transcribed from the Claude Design canvas "Jojo macOS" (boards "Themes and tokens" and "Dashboard"); see [README.md](README.md) for the source version and how conflicts are settled.

The canvas is written in CSS pixels. One CSS pixel is one SwiftUI point.

## 1. Rules

These five rules come from the canvas and hold for every screen.

1. **Tokens, not literals.** A view never writes a hex value. It reads a semantic token; the appearance (dark or light) decides the value. Nothing else changes between appearances.
2. **Status darkens in light.** Status hues keep their meaning but use darker shades in the light appearance, so that status text on its tint keeps a 4.5:1 contrast.
3. **Elevation flips.** Dark separates a card from its background with an inner white ring. Light uses a hairline outer ring and a soft 1–2 pt shadow. In light, cards sit on `content` (`#F5F5F7`), never on pure white.
4. **Solid fills stay.** The accent, the icon tiles and their white glyphs are the same in both appearances.
5. **`text4` is decoration only.** Chevrons and separators. Never readable copy.

## 2. Color tokens

Token names drop the canvas prefix `--jj-` and use camel case in Swift (`--jj-text-strong` becomes `textStrong`).

### Surfaces

| Token | Use | Dark | Light |
|---|---|---|---|
| `canvas` | Desktop behind the window. Canvas mock-up only; the app does not draw it | `#0B0C10` | `#DCDDE2` |
| `window` | Window body | `#1A1A1D` | `#FFFFFF` |
| `sidebar` | Translucent sidebar | `rgba(31,31,35,0.94)` | `rgba(236,236,240,0.94)` |
| `toolbar` | Toolbar strip | `#222226` | `#FBFBFC` |
| `content` | Main content area | `#1A1A1D` | `#F5F5F7` |
| `surface` | Cards, lists, inspectors | `#232327` | `#FFFFFF` |
| `fill` | Secondary buttons, key caps, counters | `rgba(255,255,255,0.07)` | `rgba(0,0,0,0.05)` |
| `fill2` | Group headers, quiet wells | `rgba(255,255,255,0.03)` | `rgba(0,0,0,0.025)` |
| `hover` | Row and navigation hover | `rgba(255,255,255,0.04)` | `rgba(0,0,0,0.03)` |
| `hairline` | Row separators, secondary button border | `rgba(255,255,255,0.06)` | `rgba(0,0,0,0.07)` |
| `ringEmpty` | Outline of an empty slot (for example an unreached pipeline cell) | `rgba(255,255,255,0.08)` | `rgba(0,0,0,0.09)` |
| `tileNeutral` | Neutral icon tile | `#3C3C44` | `#6A6A73` |

### Text

| Token | Use | Dark | Light |
|---|---|---|---|
| `textStrong` | Titles, primary text of a row | `#FFFFFF` | `#0E0E12` |
| `text` | Body | `#ECECEF` | `#1D1D22` |
| `text2` | Secondary copy | `#A7A7AF` | `#54545C` |
| `text3` | Meta, labels, timestamps | `#8D8D95` | `#6A6A73` |
| `text4` | Chevrons, separators only | `#5E5E66` | `#A3A3AB` |
| `navText` | Unselected sidebar item | `#E2E2E7` | `#2A2A30` |

### Accent and status

Each status has a text color, a tint (background of pills, cells and banners) and, except `ok`, a ring (outline).

| Token | Use | Dark | Light |
|---|---|---|---|
| `accent` | Primary button, selection | `#2F6FEB` | `#2F6FEB` |
| `onAccent` | Text and glyphs on `accent` | `#FFFFFF` | `#FFFFFF` |
| `icon` | Sidebar glyphs | `#7AAFFF` | `#2F6FEB` |
| `info` | Running, links, proposed | `#7AAFFF` | `#1F5FCC` |
| `infoTint` | | `rgba(122,175,255,0.12)` | `rgba(47,111,235,0.10)` |
| `infoRing` | | `rgba(122,175,255,0.5)` | `rgba(31,95,204,0.45)` |
| `warn` | Needs you, blocked, stale | `#F4B163` | `#9A5800` |
| `warnTint` | | `rgba(244,177,99,0.12)` | `rgba(214,138,30,0.14)` |
| `warnRing` | | `rgba(244,177,99,0.5)` | `rgba(154,88,0,0.45)` |
| `ok` | Completed, passed, sealed | `#5DD08E` | `#1C7A46` |
| `okTint` | | `rgba(93,208,142,0.12)` | `rgba(36,160,90,0.12)` |
| `danger` | Failed, held, destructive | `#FF8677` | `#B8382A` |
| `dangerTint` | | `rgba(255,134,119,0.12)` | `rgba(220,70,50,0.10)` |
| `dangerRing` | | `rgba(255,134,119,0.35)` | `rgba(184,56,42,0.35)` |

The neutral status (Idle, Not run, Superseded) uses `text2` on `fill`.

### Fixed colors

Not themed. They are the same in both appearances.

| Use | Value |
|---|---|
| Icon tile, approval | `#B5701F` |
| Icon tile, recovery | `#9A3B30` |
| Icon tile, destructive dialog | `#B5402F` |
| Icon tile, Claude Code | `#C2643A` |
| Project avatar (example project) | `#2E7D5B` |
| Glyph on any tile | `#FFFFFF` |

Colors that exist only on the dark component board and have no light value yet are listed in [COMPONENTS.md](COMPONENTS.md), section 12.

## 3. Elevation

| Token | Use | Dark | Light |
|---|---|---|---|
| `cardShadow` | Cards, lists, tiles | inner ring 1 pt `rgba(255,255,255,0.06)` | outer ring 1 pt `rgba(0,0,0,0.07)` and shadow y 1, blur 2, `rgba(0,0,0,0.05)` |
| `liftShadow` | A clickable card under the pointer | inner ring 1 pt `rgba(255,255,255,0.12)` | outer ring 1 pt `rgba(0,0,0,0.1)` and shadow y 4, blur 12, `rgba(0,0,0,0.07)` |
| `sidebarEdge` | Right edge of the sidebar | 1 pt `rgba(255,255,255,0.06)` | 1 pt `rgba(0,0,0,0.08)` |
| `toolbarEdge` | Bottom edge of the toolbar | 1 pt `rgba(0,0,0,0.55)` | 1 pt `rgba(0,0,0,0.09)` |
| `kbdShadow` | Bottom edge of a key cap | 1 pt `rgba(0,0,0,0.4)` | 1 pt `rgba(0,0,0,0.12)` |
| `windowShadow` | Canvas mock-up only. The app uses the system window shadow | — | — |

Floating panels (dialog, command palette, popover, menu) are defined on the dark board only: outer ring 1 pt `rgba(0,0,0,0.5–0.6)`, inner ring 1 pt `rgba(255,255,255,0.1)`, shadow y 24, blur 60, `rgba(0,0,0,0.55)` for dialogs and y 14, blur 34, `rgba(0,0,0,0.5)` for menus and popovers. In the app these are system sheets, menus and popovers, so the system draws their elevation.

## 4. Typography

The canvas sets Inter for text and JetBrains Mono for identifiers, each with the system font as fallback, and turns on the Inter features `cv11` and `ss01`.

**Decided 2026-11-16 (the human): the app uses the system fonts**, SF Pro for text and SF Mono for identifiers and code. Inter and JetBrains Mono are not bundled, so the app will not match the canvas glyph for glyph; sizes, weights and tracking below are kept.

Base: 13 pt, line height 1.45, text color `text`.

| Style | Size | Weight | Tracking | Use |
|---|---|---|---|---|
| `pageTitle` | 28 | bold (700) | −0.025 em | Title of a reference or document page |
| `kpiNumber` | 28 | bold (700) | −0.03 em, line height 1.1, tabular digits | Number in a KPI tile |
| `inspectorTitle` | 15 | semibold (600) | −0.01 em | Inspector title, section title of a page, palette input |
| `toolbarTitle` | 14.5 | semibold (600) | −0.01 em | Screen title in the toolbar |
| `dialogTitle` | 14 | semibold (600) | 0 | Dialog title |
| `sectionTitle` | 13 | semibold (600) | −0.005 em | Section heading inside a screen |
| `body` | 13 | regular (400) | 0 | Body, menu items |
| `bodyStrong` | 13 | semibold (600) | 0 | Primary text of a row |
| `nav` | 13 | medium (500) | 0 | Sidebar item |
| `control` | 12.5 | medium (500) | 0 | Buttons, search field, key-value grids |
| `controlStrong` | 12.5 | semibold (600) | 0 | Node title, feature name in the switcher |
| `secondary` | 12 | regular (400) or medium (500) | 0 | Row secondary line, KPI caption, ghost link |
| `meta` | 11.5 | regular (400) | 0 | Row meta, breadcrumbs, field labels |
| `pill` | 11 to 11.5 | medium (500) | 0 | Status pills (section 2 of COMPONENTS.md) |
| `tag` | 11 | semibold (600) | +0.02 em, uppercase | Document status tag |
| `groupHeader` | 11 | semibold (600) | +0.04 em, uppercase | Group header inside a list |
| `sidebarSection` | 11 | semibold (600) | 0 | Sidebar section label |
| `counter` | 11 | semibold (600) | 0 | Counter badge |
| `kbd` | 11 | medium (500) | 0 | Key cap |
| `mono` | 11 to 11.5 | regular (400) or medium (500) | −0.01 em | Identifiers, refs, digests |
| `code` | 12 | regular (400) | line height 22 | Diff and code |

Text that can be cut is cut with a trailing ellipsis on one line. Numbers that change in place use tabular digits.

## 5. Spacing

There is no fixed step scale. These are the values the canvas uses, by role.

| Role | Value |
|---|---|
| Glyph to label, inside a pill or button | 5 to 6 |
| Between pills, between buttons | 6 to 8 |
| Sidebar glyph to label | 9 |
| Row: tile to text to trailing status | 12 |
| Between cards in a row | 14 |
| Section heading to its card | 10 |
| Between sections of a screen | 22 |
| Content padding | 20 top and sides, 28 bottom |
| Card padding | 14 vertical, 16 horizontal |
| Row padding | 11 vertical, 16 horizontal |
| Group header padding | 9 top, 7 bottom, 16 horizontal |
| Inspector section padding | 12 vertical, 16 horizontal |
| Dialog padding | 18, with 14 between its blocks |
| Sidebar padding | 12 vertical, 10 horizontal |

## 6. Shape

| Element | Corner radius |
|---|---|
| Operational pill, counter, "Eligible" tag | capsule |
| Evidence pill, document tag | 4 |
| Key cap, context-menu item, skeleton bar | 5 |
| Button, sidebar item, small icon tile (24 and 32) | 6 |
| Row icon tile (26), pipeline cell, search field, palette row | 7 |
| Feature switcher, notice well | 8 |
| Banner, sidebar status well, context menu | 9 |
| Dependency node, inspector, inner well, popover | 10 |
| Card, list, dialog, command palette | 12 |

## 7. Sizes

| Element | Size |
|---|---|
| Button, sidebar item, search field, toolbar icon button | height 28 |
| Compact button (inspector, empty state), ghost link | height 26 |
| Toolbar | minimum height 52, padding 8 vertical and 20 horizontal |
| Sidebar | 236 wide |
| Pipeline cell | height 32 |
| Operational pill | height 18 in rows and nodes, 20 where it stands alone |
| Evidence pill, document tag | height 18 (tag: 17 plus a 1 pt border) |
| Row icon tile | 26 × 26 |
| Dialog and popover icon tile | 32 × 32 and 24 × 24 |
| Task node | 196 × 84 |
| Gate node | 196 × 76 |
| Inspector | 320 wide |
| Dialog | 420 to 460 wide |
| Command palette | 560 wide |

## 8. Icons

The canvas draws line icons with a 1.5 pt stroke on a 16-unit grid, round caps. The app uses SF Symbols at regular weight; the symbol for each glyph is chosen when its screen is built and recorded in that screen's notes.

| Place | Size |
|---|---|
| Glyph inside an operational pill | 9 (stroke 1.4) |
| Glyph inside an evidence pill | 10 (stroke 1.4) |
| Trailing chevron of a row | 12 |
| Button glyph, palette row glyph, tile glyph | 14 |
| Sidebar glyph, banner glyph | 15 |
| Toolbar glyph | 16 |
| Empty-state glyph | 24 |

No emoji anywhere.

## 9. Motion and interaction states

The canvas defines hover only.

| State | Treatment |
|---|---|
| Row and sidebar item under the pointer | background `hover` |
| Ghost button under the pointer | background `fill` |
| Clickable card under the pointer | `liftShadow` instead of `cardShadow` |
| Disabled control | opacity 0.42, and the reason is always shown (COMPONENTS.md, section 3) |
| Unavailable popover item | opacity 0.55 |

Pressed and keyboard-focus states are not drawn on the canvas. The app uses the system focus ring and the system pressed state until the design defines them.

## 10. SwiftUI mapping

| Design concept | SwiftUI |
|---|---|
| Appearance | The system color scheme. One theme value holds every token for dark and one for light; views read it from the environment |
| `rgba(255,255,255,a)` and `rgba(0,0,0,a)` overlays | `Color.white.opacity(a)`, `Color.black.opacity(a)` |
| Inner ring (dark cards) | `.overlay` with a rounded rectangle and `strokeBorder` of 1 pt |
| Outer ring and soft shadow (light cards) | the same 1 pt `strokeBorder` overlay plus `.shadow` |
| Translucent sidebar | the system sidebar material (`NavigationSplitView`); the `sidebar` color is the fallback when transparency is reduced |
| Traffic lights, window shadow, window corner | the system window. Never drawn by the app |
| Capsule pill | `Capsule()` |
| Dashed outline (gates, Unknown) | `StrokeStyle(lineWidth: 1, dash: [3, 3])` |
| Tabular digits | `.monospacedDigit()` |
| Uppercase tag with tracking | `.textCase(.uppercase)` and `.tracking(size × em)` |
| Dialog, notification, context menu, command palette | system sheet or alert, `UNUserNotificationCenter`, `.contextMenu`, a panel opened with ⌘K |

Accessibility: every status carries a glyph or a shape as well as a color, so it stays readable without color. Honor Reduce Transparency (opaque `sidebar`), Increase Contrast and Dynamic Type where the layout allows.

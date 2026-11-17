import AppKit
import SwiftUI

/// One sRGB color with alpha. Token values are kept as numbers so tests can check them and
/// compute contrast; views read them as `Color` through `Theme`.
struct RGBA: Equatable, Sendable {
    var r: Double
    var g: Double
    var b: Double
    var a: Double

    init(_ hex: UInt32, _ alpha: Double = 1) {
        r = Double((hex >> 16) & 0xFF) / 255
        g = Double((hex >> 8) & 0xFF) / 255
        b = Double(hex & 0xFF) / 255
        a = alpha
    }

    /// A CSS `rgba(r,g,b,a)` value as the canvas writes it.
    init(_ red: Int, _ green: Int, _ blue: Int, _ alpha: Double) {
        r = Double(red) / 255
        g = Double(green) / 255
        b = Double(blue) / 255
        a = alpha
    }

    static func white(_ alpha: Double) -> RGBA { RGBA(0xFFFFFF, alpha) }
    static func black(_ alpha: Double) -> RGBA { RGBA(0x000000, alpha) }

    func alpha(_ value: Double) -> RGBA {
        var c = self
        c.a = value
        return c
    }

    /// This color painted over `bottom`, opaque.
    func over(_ bottom: RGBA) -> RGBA {
        var c = bottom
        c.r = r * a + bottom.r * (1 - a)
        c.g = g * a + bottom.g * (1 - a)
        c.b = b * a + bottom.b * (1 - a)
        c.a = 1
        return c
    }

    var color: Color { Color(.sRGB, red: r, green: g, blue: b, opacity: a) }
}

/// A drop shadow in canvas terms (CSS blur).
struct Elevation: Equatable, Sendable {
    var color: RGBA
    var y: Double
    var blur: Double
}

/// The six status hues of docs/design/DESIGN_SYSTEM.md, section 4: gray (neutral), blue
/// (info), orange (warn), green (ok), red (danger) and purple (review).
enum Tone: CaseIterable, Sendable {
    case neutral, info, warn, ok, danger
    /// Purple: requested, review, needs approval, eligible.
    case review
}

/// Every token of docs/design/DESIGN_SYSTEM.md, sections 2 and 4, for one appearance.
struct Tokens: Equatable, Sendable {
    // Materials (section 2)
    var window: RGBA
    var glass: RGBA
    var glassStrong: RGBA
    var card: RGBA
    var cardHover: RGBA
    var rim: RGBA
    var rimLow: RGBA
    var shadow: RGBA
    // Text and lines
    var text: RGBA
    var text2: RGBA
    var text3: RGBA
    var sep: RGBA
    var dotOff: RGBA
    // Accent
    var accent: RGBA
    var accentTint: RGBA
    // Status: text color, then its tint
    var blue: RGBA
    var blueBg: RGBA
    var orange: RGBA
    var orangeBg: RGBA
    var green: RGBA
    var greenBg: RGBA
    var red: RGBA
    var redBg: RGBA
    var purple: RGBA
    var purpleBg: RGBA
    var gray: RGBA
    var grayBg: RGBA

    /// Text on `accent`, white in both appearances.
    var onAccent: RGBA { .white(1) }

    /// What a material falls back to when it cannot blur (Reduce Transparency, off-screen
    /// snapshots): the token at alpha 1 over the opaque window.
    var opaqueWindow: RGBA { window.alpha(1) }

    func opaque(_ material: KeyPath<Tokens, RGBA>) -> RGBA {
        self[keyPath: material].over(opaqueWindow)
    }

    func foreground(_ tone: Tone) -> RGBA {
        switch tone {
        case .neutral: return gray
        case .info: return blue
        case .warn: return orange
        case .ok: return green
        case .danger: return red
        case .review: return purple
        }
    }

    func tint(_ tone: Tone) -> RGBA {
        switch tone {
        case .neutral: return grayBg
        case .info: return blueBg
        case .warn: return orangeBg
        case .ok: return greenBg
        case .danger: return redBg
        case .review: return purpleBg
        }
    }

    static let light = Tokens(
        window: RGBA(246, 246, 248, 0.74), glass: .white(0.58), glassStrong: .white(0.78),
        card: .white(0.82), cardHover: .white(0.95), rim: .white(0.85), rimLow: .white(0.35),
        shadow: RGBA(16, 24, 40, 0.14),
        text: RGBA(0x1D1D1F), text2: RGBA(0x5B5B61), text3: RGBA(0x6E6E73),
        sep: .black(0.09), dotOff: RGBA(60, 60, 67, 0.22),
        accent: RGBA(0x0071E3), accentTint: RGBA(0, 113, 227, 0.13),
        blue: RGBA(0x0040DD), blueBg: RGBA(0, 113, 227, 0.12),
        orange: RGBA(0x9A4300), orangeBg: RGBA(255, 149, 0, 0.16),
        green: RGBA(0x17712F), greenBg: RGBA(52, 199, 89, 0.16),
        red: RGBA(0xC11A20), redBg: RGBA(255, 59, 48, 0.13),
        purple: RGBA(0x7428B0), purpleBg: RGBA(175, 82, 222, 0.14),
        gray: RGBA(0x55555A), grayBg: RGBA(120, 120, 128, 0.14))

    static let dark = Tokens(
        window: RGBA(28, 28, 32, 0.72), glass: RGBA(58, 58, 64, 0.46), glassStrong: RGBA(64, 64, 70, 0.66),
        card: .white(0.065), cardHover: .white(0.10), rim: .white(0.22), rimLow: .white(0.06),
        shadow: .black(0.5),
        text: RGBA(0xF5F5F7), text2: RGBA(0xB0B0B6), text3: RGBA(0x98989E),
        sep: .white(0.10), dotOff: RGBA(235, 235, 245, 0.20),
        accent: RGBA(0x0A84FF), accentTint: RGBA(10, 132, 255, 0.24),
        blue: RGBA(0x6CB2FF), blueBg: RGBA(10, 132, 255, 0.20),
        orange: RGBA(0xFFB54D), orangeBg: RGBA(255, 159, 10, 0.18),
        green: RGBA(0x4CD97B), greenBg: RGBA(48, 209, 88, 0.17),
        red: RGBA(0xFF7A72), redBg: RGBA(255, 69, 58, 0.18),
        purple: RGBA(0xD79BFF), purpleBg: RGBA(191, 90, 242, 0.20),
        gray: RGBA(0xAEAEB4), grayBg: RGBA(142, 142, 147, 0.20))
}

/// The names of the first design system, kept until each screen is rebuilt on the tokens
/// above (DESIGN_SYSTEM.md, section 12). Each maps to its nearest Liquid Glass token, so a
/// screen not moved yet already draws the new values. New code does not use them.
extension Tokens {
    var sidebar: RGBA { glass }
    var toolbar: RGBA { glassStrong }
    var content: RGBA { .white(0) }
    var surface: RGBA { card }
    var fill: RGBA { grayBg }
    var fill2: RGBA { grayBg }
    var hover: RGBA { grayBg }
    var hairline: RGBA { sep }
    var ringEmpty: RGBA { dotOff }
    var tileNeutral: RGBA { Palette.gray }
    var textStrong: RGBA { text }
    var text4: RGBA { text3 }
    var navText: RGBA { text }
    var icon: RGBA { accent }
    var info: RGBA { blue }
    var infoTint: RGBA { blueBg }
    var infoRing: RGBA { blue }
    var warn: RGBA { orange }
    var warnTint: RGBA { orangeBg }
    var warnRing: RGBA { orange }
    var ok: RGBA { green }
    var okTint: RGBA { greenBg }
    var danger: RGBA { red }
    var dangerTint: RGBA { redBg }
    var dangerRing: RGBA { red }
    var cardRing: RGBA { sep }
    var liftRing: RGBA { sep }
    var sidebarEdge: RGBA { sep }
    var toolbarEdge: RGBA { sep }
    var dashedOutline: RGBA { text3 }
}

/// The fixed colors of DESIGN_SYSTEM.md, section 4: the same in both appearances; glyphs on
/// them are white.
enum Palette {
    static let orange = RGBA(0xFF9500)
    static let blue = RGBA(0x0A84FF)
    static let red = RGBA(0xFF3B30)
    static let green = RGBA(0x34C759)
    static let indigo = RGBA(0x5E5CE6)
    static let teal = RGBA(0x30B0C7)
    static let purple = RGBA(0xAF52DE)
    static let gray = RGBA(0x8E8E93)
    static let claudeCode = RGBA(0xC2643A)
    /// The filled button of a destructive alert.
    static let destructive = RGBA(0xFF3B30)
    static let appIcon = [RGBA(0x6E6CF0), RGBA(0x3634A3)]
    static let projectBadge = [RGBA(0xFF9F0A), RGBA(0xFF6B00)]

    static func gradient(_ stops: [RGBA]) -> LinearGradient {
        LinearGradient(colors: stops.map(\.color), startPoint: .top, endPoint: .bottom)
    }
}

/// What a view reads from the environment: `theme.text2` is a `Color`. Views never write a
/// hex value; the appearance decides which token table is used.
@dynamicMemberLookup
struct Theme: Equatable, Sendable {
    let isDark: Bool
    let tokens: Tokens

    subscript(dynamicMember path: KeyPath<Tokens, RGBA>) -> Color {
        tokens[keyPath: path].color
    }

    func foreground(_ tone: Tone) -> Color { tokens.foreground(tone).color }
    func tint(_ tone: Tone) -> Color { tokens.tint(tone).color }

    static let dark = Theme(isDark: true, tokens: .dark)
    static let light = Theme(isDark: false, tokens: .light)

    static func resolve(_ scheme: ColorScheme) -> Theme {
        scheme == .dark ? .dark : .light
    }
}

private struct ThemeKey: EnvironmentKey {
    static let defaultValue = Theme.light
}

/// Shadow levels of DESIGN_SYSTEM.md, section 3. Reduced is the default.
enum ShadowLevel: CaseIterable, Sendable {
    case full, reduced, none
}

private struct ShadowLevelKey: EnvironmentKey {
    static let defaultValue = ShadowLevel.reduced
}

extension EnvironmentValues {
    var theme: Theme {
        get { self[ThemeKey.self] }
        set { self[ThemeKey.self] = newValue }
    }

    var shadowLevel: ShadowLevel {
        get { self[ShadowLevelKey.self] }
        set { self[ShadowLevelKey.self] = newValue }
    }
}

/// Puts the theme for the system color scheme into the environment. Applied once, at the
/// root of each window.
struct Themed<Content: View>: View {
    @Environment(\.colorScheme) private var scheme
    @ViewBuilder var content: Content

    var body: some View {
        content.environment(\.theme, .resolve(scheme))
    }
}

/// Typography of DESIGN_SYSTEM.md, section 5, on the system fonts.
struct TextStyle: Equatable, Sendable {
    var size: Double
    var weight: Font.Weight
    /// Tracking in em.
    var tracking: Double = 0
    var mono = false
    var uppercase = false

    var font: Font {
        .system(size: size, weight: weight, design: mono ? .monospaced : .default)
    }

    static let referenceTitle = TextStyle(size: 32, weight: .bold, tracking: -0.03)
    static let kpiValue = TextStyle(size: 30, weight: .bold, tracking: -0.03)
    static let dashboardTitle = TextStyle(size: 22, weight: .bold, tracking: -0.02)
    static let pageTitle = TextStyle(size: 20, weight: .bold, tracking: -0.02)
    static let panelTitle = TextStyle(size: 17, weight: .bold, tracking: -0.01)
    static let inspectorTitle = TextStyle(size: 16, weight: .bold)
    static let sectionTitle = TextStyle(size: 15, weight: .semibold, tracking: -0.01)
    static let body = TextStyle(size: 13, weight: .regular)
    static let bodyStrong = TextStyle(size: 13, weight: .semibold)
    /// Buttons and nav items: `body` at medium.
    static let control = TextStyle(size: 13, weight: .medium)
    static let secondary = TextStyle(size: 12, weight: .regular)
    static let mono = TextStyle(size: 12, weight: .regular, mono: true)
    static let label = TextStyle(size: 11, weight: .semibold, tracking: 0.02, uppercase: true)
    static let sidebarSection = TextStyle(size: 11, weight: .semibold)
    static let pill = TextStyle(size: 11, weight: .semibold)
    static let kbd = TextStyle(size: 11, weight: .regular)
    static let tag = TextStyle(size: 10, weight: .bold, tracking: 0.06, uppercase: true)

    // Names of the first design system, mapped to the table above until their screens move.
    static let nav = control
    static let controlStrong = TextStyle(size: 13, weight: .semibold)
    static let meta = secondary
    static let groupHeader = label
}

extension View {
    func textStyle(_ style: TextStyle) -> some View {
        font(style.font)
            .tracking(style.size * style.tracking)
            .textCase(style.uppercase ? .uppercase : nil)
    }

    /// A content card (DESIGN_SYSTEM.md, section 2): `card` fill, an inner 0.5 `sep` ring, no
    /// blur. `lifted` is the `cardHover` fill of task nodes and elevated rows.
    func cardElevation(radius: CGFloat = 14, lifted: Bool = false) -> some View {
        modifier(CardMaterial(radius: radius, lifted: lifted))
    }

    /// A glass panel: sidebar, inspector, banners, menus (radius 18 to 20).
    func glassPanel(radius: CGFloat = 18) -> some View {
        modifier(GlassMaterial(shape: RoundedRectangle(cornerRadius: radius), strong: false))
    }

    /// A glass capsule: toolbar groups, secondary buttons, segmented controls, search.
    func glassCapsule() -> some View {
        modifier(GlassMaterial(shape: Capsule(), strong: true))
    }

    /// A well: quiet grouping on `grayBg`, no ring.
    func well(radius: CGFloat = 12) -> some View {
        modifier(WellMaterial(radius: radius))
    }
}

private struct CardMaterial: ViewModifier {
    @Environment(\.theme) private var theme
    let radius: CGFloat
    let lifted: Bool

    func body(content: Content) -> some View {
        content
            .background(lifted ? theme.cardHover : theme.card, in: RoundedRectangle(cornerRadius: radius))
            .overlay(RoundedRectangle(cornerRadius: radius).strokeBorder(theme.sep, lineWidth: 0.5))
    }
}

private struct WellMaterial: ViewModifier {
    @Environment(\.theme) private var theme
    let radius: CGFloat

    func body(content: Content) -> some View {
        content.background(theme.grayBg, in: RoundedRectangle(cornerRadius: radius))
    }
}

/// Liquid Glass on screen. Off screen and under Reduce Transparency the material is the
/// opaque token value with the rim edges of the stylesheet, because neither can blur.
private struct GlassMaterial<S: InsettableShape>: ViewModifier {
    @Environment(\.theme) private var theme
    @Environment(\.offscreen) private var offscreen
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.shadowLevel) private var level
    let shape: S
    let strong: Bool

    func body(content: Content) -> some View {
        if offscreen || reduceTransparency {
            content
                .background(theme.tokens.opaque(strong ? \.glassStrong : \.glass).color, in: shape)
                .overlay { edges }
                .shadow(color: shadow.color.color, radius: shadow.blur / 2, y: shadow.y)
        } else {
            content
                .glassEffect(.regular, in: shape)
                .overlay { if level == .none { shape.strokeBorder(theme.sep, lineWidth: 0.5) } }
        }
    }

    /// Inner top line `rim`, inner 0.5 ring `rimLow`; at level None only a `sep` ring.
    @ViewBuilder private var edges: some View {
        if level == .none {
            shape.strokeBorder(theme.sep, lineWidth: 0.5)
        } else {
            shape.strokeBorder(theme.rimLow, lineWidth: 0.5)
            shape.strokeBorder(
                LinearGradient(stops: [.init(color: theme.rim, location: 0), .init(color: .clear, location: 0.06),
                                       .init(color: .clear, location: 0.94), .init(color: strong ? .clear : theme.rimLow, location: 1)],
                               startPoint: .top, endPoint: .bottom),
                lineWidth: 1)
        }
    }

    /// Glass 0 6 24, capsules 0 2 10, both at 8% black, only at level Full.
    private var shadow: Elevation {
        guard level == .full else { return Elevation(color: .black(0), y: 0, blur: 0) }
        return strong ? Elevation(color: .black(0.08), y: 2, blur: 10) : Elevation(color: .black(0.08), y: 6, blur: 24)
    }
}

/// The window material: `window` over the desktop, blurred behind the window. Off screen and
/// under Reduce Transparency it is the opaque token.
struct WindowMaterial: View {
    @Environment(\.theme) private var theme
    @Environment(\.offscreen) private var offscreen
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    var body: some View {
        if offscreen || reduceTransparency {
            theme.tokens.opaqueWindow.color
        } else {
            BehindWindow().overlay(theme.window)
        }
    }

    private struct BehindWindow: NSViewRepresentable {
        func makeNSView(context: Context) -> NSVisualEffectView {
            let view = NSVisualEffectView()
            view.material = .underWindowBackground
            view.blendingMode = .behindWindow
            view.state = .followsWindowActiveState
            return view
        }

        func updateNSView(_ view: NSVisualEffectView, context: Context) {}
    }
}

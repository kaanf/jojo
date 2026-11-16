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

    static func white(_ alpha: Double) -> RGBA { RGBA(0xFFFFFF, alpha) }
    static func black(_ alpha: Double) -> RGBA { RGBA(0x000000, alpha) }

    var color: Color { Color(.sRGB, red: r, green: g, blue: b, opacity: a) }
}

/// A soft drop shadow in canvas terms (CSS blur). Light appearance only.
struct Elevation: Equatable, Sendable {
    var color: RGBA
    var y: Double
    var blur: Double
}

/// The five status hues. Neutral is `text2` on `fill`.
enum Tone: CaseIterable, Sendable {
    case neutral, info, warn, ok, danger
}

/// Every token of docs/design/DESIGN_SYSTEM.md, sections 2 and 3, for one appearance.
struct Tokens: Equatable, Sendable {
    // Surfaces
    var window: RGBA
    var sidebar: RGBA
    var toolbar: RGBA
    var content: RGBA
    var surface: RGBA
    var fill: RGBA
    var fill2: RGBA
    var hover: RGBA
    var hairline: RGBA
    var ringEmpty: RGBA
    var tileNeutral: RGBA
    // Text
    var textStrong: RGBA
    var text: RGBA
    var text2: RGBA
    var text3: RGBA
    var text4: RGBA
    var navText: RGBA
    // Accent and status
    var accent: RGBA
    var onAccent: RGBA
    var icon: RGBA
    var info: RGBA
    var infoTint: RGBA
    var infoRing: RGBA
    var warn: RGBA
    var warnTint: RGBA
    var warnRing: RGBA
    var ok: RGBA
    var okTint: RGBA
    var danger: RGBA
    var dangerTint: RGBA
    var dangerRing: RGBA
    // Elevation
    var cardRing: RGBA
    var cardShadow: Elevation?
    var liftRing: RGBA
    var liftShadow: Elevation?
    var sidebarEdge: RGBA
    var toolbarEdge: RGBA
    var kbdShadow: RGBA
    // Not themed on the canvas (COMPONENTS.md, section 12); the light values are ours.
    var primaryBorder: RGBA
    var dashedOutline: RGBA

    func foreground(_ tone: Tone) -> RGBA {
        switch tone {
        case .neutral: return text2
        case .info: return info
        case .warn: return warn
        case .ok: return ok
        case .danger: return danger
        }
    }

    func tint(_ tone: Tone) -> RGBA {
        switch tone {
        case .neutral: return fill
        case .info: return infoTint
        case .warn: return warnTint
        case .ok: return okTint
        case .danger: return dangerTint
        }
    }

    static let dark = Tokens(
        window: RGBA(0x1A1A1D), sidebar: RGBA(0x1F1F23, 0.94), toolbar: RGBA(0x222226), content: RGBA(0x1A1A1D),
        surface: RGBA(0x232327), fill: .white(0.07), fill2: .white(0.03), hover: .white(0.04),
        hairline: .white(0.06), ringEmpty: .white(0.08), tileNeutral: RGBA(0x3C3C44),
        textStrong: RGBA(0xFFFFFF), text: RGBA(0xECECEF), text2: RGBA(0xA7A7AF), text3: RGBA(0x8D8D95),
        text4: RGBA(0x5E5E66), navText: RGBA(0xE2E2E7),
        accent: RGBA(0x2F6FEB), onAccent: RGBA(0xFFFFFF), icon: RGBA(0x7AAFFF),
        info: RGBA(0x7AAFFF), infoTint: RGBA(0x7AAFFF, 0.12), infoRing: RGBA(0x7AAFFF, 0.5),
        warn: RGBA(0xF4B163), warnTint: RGBA(0xF4B163, 0.12), warnRing: RGBA(0xF4B163, 0.5),
        ok: RGBA(0x5DD08E), okTint: RGBA(0x5DD08E, 0.12),
        danger: RGBA(0xFF8677), dangerTint: RGBA(0xFF8677, 0.12), dangerRing: RGBA(0xFF8677, 0.35),
        cardRing: .white(0.06), cardShadow: nil, liftRing: .white(0.12), liftShadow: nil,
        sidebarEdge: .white(0.06), toolbarEdge: .black(0.55), kbdShadow: .black(0.4),
        primaryBorder: .black(0.12), dashedOutline: .white(0.3))

    static let light = Tokens(
        window: RGBA(0xFFFFFF), sidebar: RGBA(0xECECF0, 0.94), toolbar: RGBA(0xFBFBFC), content: RGBA(0xF5F5F7),
        surface: RGBA(0xFFFFFF), fill: .black(0.05), fill2: .black(0.025), hover: .black(0.03),
        hairline: .black(0.07), ringEmpty: .black(0.09), tileNeutral: RGBA(0x6A6A73),
        textStrong: RGBA(0x0E0E12), text: RGBA(0x1D1D22), text2: RGBA(0x54545C), text3: RGBA(0x6A6A73),
        text4: RGBA(0xA3A3AB), navText: RGBA(0x2A2A30),
        accent: RGBA(0x2F6FEB), onAccent: RGBA(0xFFFFFF), icon: RGBA(0x2F6FEB),
        info: RGBA(0x1F5FCC), infoTint: RGBA(0x2F6FEB, 0.10), infoRing: RGBA(0x1F5FCC, 0.45),
        warn: RGBA(0x9A5800), warnTint: RGBA(0xD68A1E, 0.14), warnRing: RGBA(0x9A5800, 0.45),
        ok: RGBA(0x1C7A46), okTint: RGBA(0x24A05A, 0.12),
        danger: RGBA(0xB8382A), dangerTint: RGBA(0xDC4632, 0.10), dangerRing: RGBA(0xB8382A, 0.35),
        cardRing: .black(0.07), cardShadow: Elevation(color: .black(0.05), y: 1, blur: 2),
        liftRing: .black(0.1), liftShadow: Elevation(color: .black(0.07), y: 4, blur: 12),
        sidebarEdge: .black(0.08), toolbarEdge: .black(0.09), kbdShadow: .black(0.12),
        primaryBorder: .black(0.12), dashedOutline: .black(0.3))
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
    static let defaultValue = Theme.dark
}

extension EnvironmentValues {
    var theme: Theme {
        get { self[ThemeKey.self] }
        set { self[ThemeKey.self] = newValue }
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

/// Typography of DESIGN_SYSTEM.md, section 4, on the system fonts.
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

    static let pageTitle = TextStyle(size: 28, weight: .bold, tracking: -0.025)
    static let inspectorTitle = TextStyle(size: 15, weight: .semibold, tracking: -0.01)
    static let toolbarTitle = TextStyle(size: 14.5, weight: .semibold, tracking: -0.01)
    static let sectionTitle = TextStyle(size: 13, weight: .semibold, tracking: -0.005)
    static let body = TextStyle(size: 13, weight: .regular)
    static let bodyStrong = TextStyle(size: 13, weight: .semibold)
    static let nav = TextStyle(size: 13, weight: .medium)
    static let control = TextStyle(size: 12.5, weight: .medium)
    static let controlStrong = TextStyle(size: 12.5, weight: .semibold)
    static let secondary = TextStyle(size: 12, weight: .regular)
    static let meta = TextStyle(size: 11.5, weight: .regular)
    static let pill = TextStyle(size: 11, weight: .medium)
    static let pillStandalone = TextStyle(size: 11.5, weight: .medium)
    static let tag = TextStyle(size: 11, weight: .semibold, tracking: 0.02, uppercase: true)
    static let groupHeader = TextStyle(size: 11, weight: .semibold, tracking: 0.04, uppercase: true)
    static let sidebarSection = TextStyle(size: 11, weight: .semibold)
    static let counter = TextStyle(size: 11, weight: .semibold)
    static let kbd = TextStyle(size: 11, weight: .medium)
    static let mono = TextStyle(size: 11, weight: .regular, tracking: -0.01, mono: true)
}

extension View {
    func textStyle(_ style: TextStyle) -> some View {
        font(style.font)
            .tracking(style.size * style.tracking)
            .textCase(style.uppercase ? .uppercase : nil)
    }

    /// Card elevation: an inner ring in dark; a hairline ring and a soft shadow in light.
    func cardElevation(radius: CGFloat = 12, lifted: Bool = false) -> some View {
        modifier(CardElevation(radius: radius, lifted: lifted))
    }
}

private struct CardElevation: ViewModifier {
    @Environment(\.theme) private var theme
    let radius: CGFloat
    let lifted: Bool

    func body(content: Content) -> some View {
        let shadow = lifted ? theme.tokens.liftShadow : theme.tokens.cardShadow
        content
            .background(theme.surface, in: RoundedRectangle(cornerRadius: radius))
            .overlay(RoundedRectangle(cornerRadius: radius).strokeBorder(lifted ? theme.liftRing : theme.cardRing, lineWidth: 1))
            // A CSS blur of b is a SwiftUI radius of b / 2.
            .shadow(color: shadow?.color.color ?? .clear, radius: (shadow?.blur ?? 0) / 2, y: shadow?.y ?? 0)
    }
}

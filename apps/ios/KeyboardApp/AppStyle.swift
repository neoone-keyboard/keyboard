import SwiftUI
import KeyboardCore

/// The app's look (design-ref Main.dc.html and About.dc.html): grouped grey behind white
/// cards with 16 pt corners, small uppercase section headers, the accent on the main button.
/// System colours, so dark mode follows.
enum AppStyle {
    /// The app's accent (`AccentChoice.app`), from the keyboard's palette.
    static let accent: Color = {
        let rgb = AccentChoice.app.rgb
        return Color(.sRGB, red: rgb.red, green: rgb.green, blue: rgb.blue)
    }()
    /// Text on the accent.
    static let onAccent: Color = AccentChoice.app.lightText ? .white : Color(white: 0.08)
    /// The privacy lock: a deep green in light, the system green in dark.
    static let lock = Color(UIColor { traits in
        traits.userInterfaceStyle == .dark ? .systemGreen : UIColor(red: 0.18, green: 0.42, blue: 0.23, alpha: 1)
    })
    static let cardRadius: CGFloat = 16
    static let rowPadding = EdgeInsets(top: 14, leading: 16, bottom: 14, trailing: 16)
}

/// A white (dark: raised grey) rounded card.
struct Card<Content: View>: View {
    var padding: CGFloat = 0
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 0) { content }
            .padding(padding)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(.secondarySystemGroupedBackground),
                        in: RoundedRectangle(cornerRadius: AppStyle.cardRadius, style: .continuous))
    }
}

/// "TURN IT ON": 13 pt semibold, uppercase, spaced.
struct SectionHeader: View {
    let title: String

    init(_ title: String) { self.title = title }

    var body: some View {
        Text(title)
            .font(.system(size: 13, weight: .semibold))
            .textCase(.uppercase)
            .tracking(0.6)
            .foregroundStyle(.secondary)
            .accessibilityAddTraits(.isHeader)
    }
}

/// A header above a card, inset 4 pt like the design.
struct HeaderedSection<Content: View>: View {
    let title: String
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            SectionHeader(title).padding(.horizontal, 4)
            content
        }
    }
}

/// The hairline between rows of a card, inset from the leading edge.
struct RowDivider: View {
    var body: some View {
        Divider().padding(.leading, 16)
    }
}

/// A row's trailing chevron for rows that open another screen.
struct Chevron: View {
    var body: some View {
        Image(systemName: "chevron.right")
            .font(.system(size: 13, weight: .semibold))
            .foregroundStyle(.tertiary)
            .accessibilityHidden(true)
    }
}

/// The app icon's own picture (Assets `Logo`, rendered by tools/icon), masked like the icon.
struct AppLogo: View {
    let size: CGFloat

    var body: some View {
        Image("Logo")
            .resizable()
            .interpolation(.high)
            .frame(width: size, height: size)
            .clipShape(RoundedRectangle(cornerRadius: size * 0.2237, style: .continuous))
            .accessibilityHidden(true)
    }
}

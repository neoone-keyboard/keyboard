import UIKit
import KeyboardCore

/// Colours for one appearance of the keyboard.
struct KeyboardTheme {
    let isDark: Bool
    let background: UIColor
    let key: UIColor
    let specialKey: UIColor
    let keyText: UIColor
    let hintText: UIColor
    let accent: UIColor
    let onAccent: UIColor
    let shadow: UIColor

    static func resolve(_ settings: KeyboardSettings, style: UIUserInterfaceStyle) -> KeyboardTheme {
        let dark = switch settings.theme {
        case .system: style == .dark
        case .light: false
        case .dark: true
        }
        let (accent, onAccent) = accentColors(settings.accent)
        if dark {
            return KeyboardTheme(
                isDark: true,
                background: UIColor(white: 0.08, alpha: 1),
                key: UIColor(white: 0.26, alpha: 1),
                specialKey: UIColor(white: 0.17, alpha: 1),
                keyText: UIColor(white: 0.96, alpha: 1),
                hintText: UIColor(white: 0.62, alpha: 1),
                accent: accent,
                onAccent: onAccent,
                shadow: UIColor(white: 0, alpha: 0.6)
            )
        }
        return KeyboardTheme(
            isDark: false,
            background: UIColor(red: 0.82, green: 0.83, blue: 0.86, alpha: 1),
            key: UIColor(white: 0.995, alpha: 1),
            specialKey: UIColor(red: 0.68, green: 0.70, blue: 0.74, alpha: 1),
            keyText: UIColor(white: 0.08, alpha: 1),
            hintText: UIColor(white: 0.42, alpha: 1),
            accent: accent,
            onAccent: onAccent,
            shadow: UIColor(white: 0, alpha: 0.3)
        )
    }

    /// The colour shown for an accent choice, e.g. in the settings panel's swatches.
    static func accentColor(_ choice: AccentChoice) -> UIColor {
        accentColors(choice).0
    }

    private static func accentColors(_ choice: AccentChoice) -> (UIColor, UIColor) {
        let rgb = choice.rgb
        return (UIColor(red: rgb.red, green: rgb.green, blue: rgb.blue, alpha: 1),
                UIColor(white: choice.lightText ? 0.98 : 0.08, alpha: 1))
    }
}

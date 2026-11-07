import Foundation

public enum ThemeChoice: String, CaseIterable, Sendable {
    case system, light, dark
}

public enum AccentChoice: String, CaseIterable, Sendable {
    case saffron, teal, indigo, rose

    /// The colour, sRGB components 0...1. The keyboard's theme and the app both use these.
    public var rgb: (red: Double, green: Double, blue: Double) {
        switch self {
        case .saffron: (0.91, 0.64, 0.24)
        case .teal: (0.16, 0.62, 0.60)
        case .indigo: (0.33, 0.35, 0.80)
        case .rose: (0.84, 0.31, 0.45)
        }
    }

    /// Text on the accent is light (else dark), whichever reads better.
    public var lightText: Bool {
        switch self {
        case .saffron, .teal: false
        case .indigo, .rose: true
        }
    }

    /// The app's own accent: the indigo of its icon's lit 1 key.
    public static let app: AccentChoice = .indigo
}

/// Settings chosen in the keyboard's own settings panel. The keyboard keeps them in
/// its own container (`UserDefaults.standard`), so no Full Access or App Group is needed.
public struct KeyboardSettings: Equatable, Sendable {
    public var theme: ThemeChoice = .system
    public var accent: AccentChoice = .saffron
    public var numberRow = false
    public var keyClicks = false
    /// When off, space always keeps what was typed.
    public var autocorrect = true
    /// An emoji for the word in the bar's right slot (`EmojiSuggestion`).
    public var emojiSuggestions = true
    /// How many suggestions the bar holds: the three slots, and with more, the rest past the
    /// right slot when the bar is scrolled. One of `suggestionCounts`.
    public var suggestionCount = 10

    /// The choices offered for `suggestionCount`. The engine returns at most 20.
    public static let suggestionCounts = [3, 10, 20]

    public init() {}

    public static func load(from defaults: UserDefaults) -> KeyboardSettings {
        var settings = KeyboardSettings()
        settings.theme = defaults.string(forKey: Keys.theme).flatMap(ThemeChoice.init(rawValue:)) ?? settings.theme
        settings.accent = defaults.string(forKey: Keys.accent).flatMap(AccentChoice.init(rawValue:)) ?? settings.accent
        settings.numberRow = defaults.bool(forKey: Keys.numberRow)
        settings.keyClicks = defaults.bool(forKey: Keys.keyClicks)
        settings.autocorrect = defaults.object(forKey: Keys.autocorrect) as? Bool ?? settings.autocorrect
        settings.emojiSuggestions = defaults.object(forKey: Keys.emojiSuggestions) as? Bool ?? settings.emojiSuggestions
        if let count = defaults.object(forKey: Keys.suggestionCount) as? Int, suggestionCounts.contains(count) {
            settings.suggestionCount = count
        }
        return settings
    }

    public func save(to defaults: UserDefaults) {
        defaults.set(theme.rawValue, forKey: Keys.theme)
        defaults.set(accent.rawValue, forKey: Keys.accent)
        defaults.set(numberRow, forKey: Keys.numberRow)
        defaults.set(keyClicks, forKey: Keys.keyClicks)
        defaults.set(autocorrect, forKey: Keys.autocorrect)
        defaults.set(emojiSuggestions, forKey: Keys.emojiSuggestions)
        defaults.set(suggestionCount, forKey: Keys.suggestionCount)
    }

    private enum Keys {
        static let theme = "theme"
        static let accent = "accent"
        static let numberRow = "numberRow"
        static let keyClicks = "keyClicks"
        static let autocorrect = "autocorrect"
        static let emojiSuggestions = "emojiSuggestions"
        static let suggestionCount = "suggestionCount"
    }
}

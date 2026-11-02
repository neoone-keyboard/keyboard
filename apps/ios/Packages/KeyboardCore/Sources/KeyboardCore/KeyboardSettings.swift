import Foundation

public enum ThemeChoice: String, CaseIterable, Sendable {
    case system, light, dark
}

public enum AccentChoice: String, CaseIterable, Sendable {
    case saffron, teal, indigo, rose
}

/// Settings chosen in the containing app and read by the keyboard. They live in
/// the shared App Group, which the keyboard can read without Full Access.
public struct KeyboardSettings: Equatable, Sendable {
    public static let appGroup = "group.me.jishnu.keyboard"

    public var theme: ThemeChoice = .system
    public var accent: AccentChoice = .saffron
    public var numberRow = false
    public var keyClicks = false

    public init() {}

    /// The shared store, or nil when the App Group isn't available (e.g. a misconfigured build).
    public static var sharedDefaults: UserDefaults? {
        UserDefaults(suiteName: appGroup)
    }

    public static func load(from defaults: UserDefaults) -> KeyboardSettings {
        var settings = KeyboardSettings()
        settings.theme = defaults.string(forKey: Keys.theme).flatMap(ThemeChoice.init(rawValue:)) ?? settings.theme
        settings.accent = defaults.string(forKey: Keys.accent).flatMap(AccentChoice.init(rawValue:)) ?? settings.accent
        settings.numberRow = defaults.bool(forKey: Keys.numberRow)
        settings.keyClicks = defaults.bool(forKey: Keys.keyClicks)
        return settings
    }

    public func save(to defaults: UserDefaults) {
        defaults.set(theme.rawValue, forKey: Keys.theme)
        defaults.set(accent.rawValue, forKey: Keys.accent)
        defaults.set(numberRow, forKey: Keys.numberRow)
        defaults.set(keyClicks, forKey: Keys.keyClicks)
    }

    private enum Keys {
        static let theme = "theme"
        static let accent = "accent"
        static let numberRow = "numberRow"
        static let keyClicks = "keyClicks"
    }
}

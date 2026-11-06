/// What a key does when tapped. Typing logic depends only on this, never on UIKit.
public enum KeyAction: Hashable, Sendable {
    case character(String)
    case space
    case backspace
    case shift
    case returnKey
    case nextKeyboard
    /// Switch to another page of keys. `.letters` means "the alphabet of the current language".
    case page(KeyboardPage)
    /// Switch to the next language (`InputController.languages`).
    case toggleLanguage
    /// Show the emoji panel in place of the keys.
    case emoji
}

public enum KeyboardPage: Hashable, Sendable {
    case letters, symbols, moreSymbols
    /// A page of the current language's native layout, by its id in `layout.json` ("letters",
    /// "more", "vowels").
    case native(String)

    public var isNative: Bool {
        if case .native = self { true } else { false }
    }
}

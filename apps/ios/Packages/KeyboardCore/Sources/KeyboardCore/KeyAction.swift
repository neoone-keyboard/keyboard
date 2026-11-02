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
    /// Switch between English and Malayalam.
    case toggleLanguage
}

public enum KeyboardPage: Hashable, Sendable, CaseIterable {
    case letters, symbols, moreSymbols, malayalam, malayalamMore, malayalamVowels

    public var isMalayalam: Bool {
        switch self {
        case .malayalam, .malayalamMore, .malayalamVowels: true
        case .letters, .symbols, .moreSymbols: false
        }
    }
}

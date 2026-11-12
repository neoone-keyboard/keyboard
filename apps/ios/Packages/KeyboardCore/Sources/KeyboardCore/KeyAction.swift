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

/// The keys showing: a page of a language (`Language.id`). Two languages' pages can share an
/// id (Malayalam's and Hindi's first are both `.native("letters")`), so both are needed.
public struct KeysShown: Hashable, Sendable {
    public let language: String
    public let page: KeyboardPage
    /// The pack whose script's marks and digits the Latin pages show
    /// (`InputController.scriptPack`): turning a language on or off can change them.
    public let script: String?

    public init(language: String, page: KeyboardPage, script: String? = nil) {
        self.language = language
        self.page = page
        self.script = script
    }
}

/// What VoiceOver says for a key (`InputController.spokenKey`): its label, and a state such
/// as shift's "on" as the value, which VoiceOver reads after it.
public struct SpokenKey: Equatable, Sendable {
    public var label: String
    public var value: String?

    public init(_ label: String, value: String? = nil) {
        self.label = label
        self.value = value
    }

    /// Label and value as one phrase: "shift, on".
    public var text: String { value.map { "\(label), \($0)" } ?? label }
}

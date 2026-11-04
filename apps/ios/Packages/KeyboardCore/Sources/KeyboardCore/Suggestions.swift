/// What kind of guess a suggestion is. Mirrors the engine's `CandidateKind`.
public enum SuggestionKind: Sendable {
    /// Exactly what was typed.
    case verbatim
    case completion
    case correction
    /// A Malayalam word for Latin (Manglish) typing.
    case manglish
    /// A guess for the next word, shown after a space.
    case nextWord
    /// An emoji for the word (`EmojiSuggestion`), never from the engine.
    case emoji
}

public struct Suggestion: Equatable, Sendable {
    public var text: String
    public var kind: SuggestionKind

    public init(_ text: String, kind: SuggestionKind) {
        self.text = text
        self.kind = kind
    }
}

/// Everything the engine needs for one request.
public struct SuggestionRequest: Equatable, Sendable {
    public var contextBefore: String
    public var language: Language
    public var autocorrect: Bool

    public init(contextBefore: String, language: Language, autocorrect: Bool) {
        self.contextBefore = contextBefore
        self.language = language
        self.autocorrect = autocorrect
    }
}

public struct SuggestionResult: Equatable, Sendable {
    /// The word the engine saw being typed ("" after a space).
    public var typed: String
    /// `candidates[0]` belongs in the middle slot. When it isn't the typed
    /// word, the typed word is `candidates[1]`.
    public var candidates: [Suggestion]
    /// Vowel signs, most likely first (Malayalam, after a consonant).
    public var vowelSigns: [String]

    public init(typed: String, candidates: [Suggestion], vowelSigns: [String] = []) {
        self.typed = typed
        self.candidates = candidates
        self.vowelSigns = vowelSigns
    }

    public static let empty = SuggestionResult(typed: "", candidates: [])

    public var middle: Suggestion? { candidates.first }

    /// Whether this result was made for `word`, the word now before the cursor.
    /// Smart punctuation's ’ and a typed ' count as the same.
    public func isFor(word: String) -> Bool {
        Self.straight(typed) == Self.straight(word)
    }

    private static func straight(_ text: String) -> String {
        text.replacingOccurrences(of: "\u{2019}", with: "'")
    }
}

/// One slot of the bar. A quoted slot shows the typed word in quotes, so one tap keeps it.
public struct SuggestionSlot: Equatable, Sendable {
    public var suggestion: Suggestion
    public var quoted: Bool

    public init(suggestion: Suggestion, quoted: Bool) {
        self.suggestion = suggestion
        self.quoted = quoted
    }

    public var title: String { quoted ? "\u{201C}\(suggestion.text)\u{201D}" : suggestion.text }
}

/// The three-slot bar: best in the middle; when the middle would replace what
/// was typed, the typed word sits on the left in quotes.
public struct SuggestionBar: Equatable, Sendable {
    public var left: SuggestionSlot?
    public var middle: SuggestionSlot?
    public var right: SuggestionSlot?

    public init(_ result: SuggestionResult) {
        let candidates = result.candidates
        guard let first = candidates.first else { return }
        middle = SuggestionSlot(suggestion: first, quoted: false)
        var rest = Array(candidates.dropFirst())
        if first.kind != .verbatim, let index = rest.firstIndex(where: { $0.kind == .verbatim }) {
            left = SuggestionSlot(suggestion: rest.remove(at: index), quoted: true)
            right = rest.first.map { SuggestionSlot(suggestion: $0, quoted: false) }
        } else {
            left = rest.first.map { SuggestionSlot(suggestion: $0, quoted: false) }
            right = rest.dropFirst().first.map { SuggestionSlot(suggestion: $0, quoted: false) }
        }
    }

    /// The bar with `emoji` in the right slot: never the middle, which space commits, so an
    /// emoji is never put in by autocorrect. Nil leaves the bar as it is.
    public func withEmoji(_ emoji: String?) -> SuggestionBar {
        guard let emoji else { return self }
        var bar = self
        bar.right = SuggestionSlot(suggestion: Suggestion(emoji, kind: .emoji), quoted: false)
        return bar
    }

    /// Left, middle, right; nil for an empty slot.
    public var slots: [SuggestionSlot?] { [left, middle, right] }
}

/// Holding a bar slot asks whether to remove its word (SwiftKey's "Remove …?"). Once
/// removed, the engine never suggests the word again until the user learns it anew.
/// The bar drops the question after `timeout`, or as soon as the text changes.
public struct SuggestionRemoval: Equatable, Sendable {
    /// How long a slot must be held, in seconds; a shorter touch is a tap and commits the slot.
    public static let holdDuration: Double = 0.5
    public static let timeout: Duration = .seconds(4)

    /// The word itself, never the quoted title.
    public let word: String

    /// Nil for a slot with no word, and for the emoji slot.
    public init?(slot: SuggestionSlot) {
        guard !slot.suggestion.text.isEmpty, slot.suggestion.kind != .emoji else { return nil }
        word = slot.suggestion.text
    }

    public var prompt: String { "Remove \u{201C}\(word)\u{201D}?" }
}

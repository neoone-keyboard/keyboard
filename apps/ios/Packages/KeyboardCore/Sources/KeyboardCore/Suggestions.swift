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
    /// The id of the language the engine offered it as a word of ("en", "ml"); nil when it
    /// didn't come from the engine.
    public var language: String?

    public init(_ text: String, kind: SuggestionKind, language: String? = nil) {
        self.text = text
        self.kind = kind
        self.language = language
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

/// The bar's suggestions. The bar draws `ordered`: the best (what space commits) first, at its
/// leading edge, then the rest in the engine's order, as many as fit (`BarFit`). When the best
/// would replace what was typed, the typed word is among them in quotes, so one tap keeps it.
///
/// `left`, `middle` and `right` are the same words as a bar of three slots had them (the best
/// in the middle), with the candidates after those in `more`; `limit` counts them all
/// (`KeyboardSettings.suggestionCount`).
public struct SuggestionBar: Equatable, Sendable {
    /// The three slots, as a bar of three has always had them.
    public static let slotCount = 3

    public var left: SuggestionSlot?
    public var middle: SuggestionSlot?
    public var right: SuggestionSlot?
    /// The candidates after the three slots, at most `limit` minus three.
    public var more: [SuggestionSlot] = []
    /// How many suggestions the bar holds at most, slots and `more` together.
    public let limit: Int
    /// The bar's words in the order it draws them from its leading edge: the best first, then
    /// the engine's order, the typed word quoted in its place there. At most `limit`.
    public private(set) var ordered: [SuggestionSlot] = []

    public init(_ result: SuggestionResult, limit: Int = SuggestionBar.slotCount) {
        self.limit = max(limit, Self.slotCount)
        let candidates = result.candidates
        guard let first = candidates.first else { return }
        ordered = candidates.prefix(self.limit).enumerated().map { index, suggestion in
            SuggestionSlot(suggestion: suggestion, quoted: index > 0 && first.kind != .verbatim && suggestion.kind == .verbatim)
        }
        middle = SuggestionSlot(suggestion: first, quoted: false)
        var rest = Array(candidates.dropFirst())
        let after: ArraySlice<Suggestion>
        if first.kind != .verbatim, let index = rest.firstIndex(where: { $0.kind == .verbatim }) {
            left = SuggestionSlot(suggestion: rest.remove(at: index), quoted: true)
            right = rest.first.map { SuggestionSlot(suggestion: $0, quoted: false) }
            after = rest.dropFirst()
        } else {
            left = rest.first.map { SuggestionSlot(suggestion: $0, quoted: false) }
            right = rest.dropFirst().first.map { SuggestionSlot(suggestion: $0, quoted: false) }
            after = rest.dropFirst(2)
        }
        more = after.prefix(self.limit - Self.slotCount).map { SuggestionSlot(suggestion: $0, quoted: false) }
    }

    /// The bar with `emoji` in the right slot (third in `ordered`): never the middle, which space commits, so an
    /// emoji is never put in by autocorrect. The word it displaces leads `more` when the bar
    /// has room past three, else it is dropped. Nil leaves the bar as it is.
    public func withEmoji(_ emoji: String?) -> SuggestionBar {
        guard let emoji else { return self }
        var bar = self
        if limit > Self.slotCount, let displaced = right {
            bar.more.insert(displaced, at: 0)
            bar.more = Array(bar.more.prefix(limit - Self.slotCount))
        }
        let slot = SuggestionSlot(suggestion: Suggestion(emoji, kind: .emoji), quoted: false)
        bar.right = slot
        // Third, where the right slot was; never first, so space never puts it in.
        bar.ordered.insert(slot, at: min(2, bar.ordered.count))
        bar.ordered = Array(bar.ordered.prefix(limit))
        return bar
    }

    /// Left, middle, right; nil for an empty slot.
    public var slots: [SuggestionSlot?] { [left, middle, right] }

    /// The three slots in the order the bar draws them from where reading starts: left to
    /// right, `slots`. Right to left (a pack whose script is, `PackDescriptor.rightToLeft`),
    /// the bar is read from its right edge: the best word there, then the other word, then
    /// the typed word in quotes or the emoji; `more` follows further left.
    public func drawn(rightToLeft: Bool) -> [SuggestionSlot?] {
        guard rightToLeft else { return slots }
        // A word first, then the quoted typed word, then an emoji; empty slots last.
        func rank(_ slot: SuggestionSlot?) -> Int {
            guard let slot else { return 3 }
            return slot.suggestion.kind == .emoji ? 2 : slot.quoted ? 1 : 0
        }
        let others = [left, right].enumerated().sorted { (rank($0.element), $0.offset) < (rank($1.element), $1.offset) }
        return [middle] + others.map(\.element)
    }
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

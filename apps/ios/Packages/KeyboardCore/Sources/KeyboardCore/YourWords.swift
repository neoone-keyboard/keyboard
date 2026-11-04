/// The "Your words" screen: the learned words and the words removed from the bar, as the
/// engine lists them, and a search over both. A keyboard can't type into its own fields, so
/// the search is typed on the keyboard's own keys: while the screen is open the extension
/// gives each key to `handle` instead of the input controller.
public struct YourWords: Equatable, Sendable {
    /// What a key does while the screen is open.
    public enum KeyResult: Equatable, Sendable {
        /// The search changed (or backspace met an empty search).
        case edited
        /// The key changes the keys (a page, the language, the next keyboard): handle it as usual.
        case passThrough
        /// Nothing: no word holds a space or a line, and the search ignores case.
        case ignored
    }

    /// The longest search, in scalars: the engine stores no longer word (`MAX_WORD_CHARS`).
    public static let maxQueryLength = 48

    public private(set) var query = ""
    /// The learned words that match the search, most recently used first.
    public private(set) var learned: [String] = []
    /// The removed words that match the search, most recently removed first.
    public private(set) var removed: [String] = []

    private var allLearned: [Word]
    private var allRemoved: [Word]

    /// A word with its lowercased form, so a search doesn't lowercase every word on each key.
    private struct Word: Equatable, Sendable {
        let text: String
        let folded: String
        init(_ text: String) {
            self.text = text
            folded = text.lowercased()
        }
    }

    public init(learned: [String], removed: [String]) {
        allLearned = learned.map(Word.init)
        allRemoved = removed.map(Word.init)
        filter()
    }

    public mutating func handle(_ action: KeyAction) -> KeyResult {
        switch action {
        case .character(let c):
            type(c)
            return .edited
        case .backspace:
            // A scalar, not a character: `കി` loses its vowel sign, as typing put it on.
            if !query.isEmpty {
                query.unicodeScalars.removeLast()
                filter()
            }
            return .edited
        case .page, .toggleLanguage, .nextKeyboard:
            return .passThrough
        case .space, .returnKey, .shift, .emoji:
            return .ignored
        }
    }

    /// Adds to the search: a key's character, or a long-press alternate.
    public mutating func type(_ text: String) {
        let room = Self.maxQueryLength - query.unicodeScalars.count
        guard room > 0, !text.isEmpty else { return }
        query.unicodeScalars.append(contentsOf: text.unicodeScalars.prefix(room))
        filter()
    }

    public mutating func clearQuery() {
        query = ""
        filter()
    }

    /// Takes a forgotten word off the learned list (the engine forgets it separately).
    public mutating func forget(_ word: String) {
        allLearned.removeAll { $0.text == word }
        learned.removeAll { $0 == word }
    }

    /// Takes a restored word off the removed list (the engine restores it separately).
    public mutating func restore(_ word: String) {
        allRemoved.removeAll { $0.text == word }
        removed.removeAll { $0 == word }
    }

    /// What an empty learned list says, or nil when it isn't empty.
    public var emptyLearnedMessage: String? {
        guard learned.isEmpty else { return nil }
        return allLearned.isEmpty ? "No learned words yet" : "No learned words match"
    }

    /// What an empty removed list says, or nil when it isn't empty.
    public var emptyRemovedMessage: String? {
        guard removed.isEmpty else { return nil }
        return allRemoved.isEmpty ? "No removed words yet" : "No removed words match"
    }

    /// Anywhere in the word, ignoring case. Compared by scalar, so a consonant typed alone
    /// finds the syllables it begins (`ക` finds `കിളി`, where `കി` is one character).
    private mutating func filter() {
        let folded = query.lowercased()
        func matches(_ word: Word) -> Bool {
            folded.isEmpty || word.folded.unicodeScalars.firstRange(of: folded.unicodeScalars) != nil
        }
        learned = allLearned.filter(matches).map(\.text)
        removed = allRemoved.filter(matches).map(\.text)
    }
}

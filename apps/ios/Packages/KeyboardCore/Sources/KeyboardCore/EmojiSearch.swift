/// The emoji panel's search. A keyboard can't type into its own fields, so, as with "Your
/// words", the search is typed on the keyboard's own keys: while it is open the extension
/// gives each key to `handle` instead of the input controller, and shows `results`.
///
/// A query's words each match the start of a word of an emoji's name or keywords
/// (emoji-search.txt, English and Malayalam): "hea" finds ❤️ by "heart", "red hea" finds it
/// by both. Matches in the emoji's name ("red heart") come before matches in a keyword only
/// ("heat" for 🫠); within each, a word that is the query (or its plural) before one it only
/// begins, then the user's recents, then words that name more emoji, then the panel's order. No Manglish: CLDR has no romanised Malayalam, and
/// the transliterator is the engine's, a hop away; Malayalam script from the Malayalam pages
/// matches.
public struct EmojiSearch: Sendable {
    /// What a key does while the search is open.
    public enum KeyResult: Equatable, Sendable {
        /// The query changed (or backspace met an empty query).
        case edited
        /// The key changes the keys (a page, the language, the next keyboard): handle it as usual.
        case passThrough
        /// Back to the panel's grid (return, or the emoji key).
        case close
        /// Nothing (shift: case doesn't matter).
        case ignored
    }

    public static let maxQueryLength = 40
    /// Enough to scroll through; one letter can match hundreds.
    public static let maxResults = 120

    public private(set) var query = ""
    /// What the row shows, best first, in the user's tones: the recents while the query is empty.
    public private(set) var results: [String] = []

    private let words: EmojiWords
    private let history: EmojiHistory
    /// Toneless emoji → place in the panel; only what this system draws is in the panel.
    private let order: [String: Int]
    /// Toneless emoji → place in the recents.
    private let recentRank: [String: Int]

    public init(words: EmojiWords, catalog: EmojiCatalog, history: EmojiHistory) {
        self.words = words
        self.history = history
        var order: [String: Int] = [:]
        for item in catalog.categories.lazy.flatMap(\.items) {
            order[EmojiSupport.toneless(item.emoji)] = order.count
        }
        self.order = order
        var recentRank: [String: Int] = [:]
        for (rank, emoji) in history.recents.enumerated() {
            recentRank[EmojiSupport.toneless(emoji)] = rank
        }
        self.recentRank = recentRank
        filter()
    }

    public mutating func handle(_ action: KeyAction) -> KeyResult {
        switch action {
        case .character(let c):
            type(c)
            return .edited
        case .space:
            // Between words only.
            if !query.isEmpty, query.last != " " { type(" ") }
            return .edited
        case .backspace:
            // A scalar, not a character: `ഹൃ` loses its vowel sign, as typing put it on.
            if !query.isEmpty {
                query.unicodeScalars.removeLast()
                filter()
            }
            return .edited
        case .returnKey, .emoji:
            return .close
        case .page, .toggleLanguage, .nextKeyboard:
            return .passThrough
        case .shift:
            return .ignored
        }
    }

    /// Adds to the query: a key's character, or a long-press alternate.
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

    private mutating func filter() {
        let terms = query.lowercased().split(whereSeparator: \.isWhitespace).map(String.init)
        guard !terms.isEmpty else {
            results = history.recents.filter { order[EmojiSupport.toneless($0)] != nil }
            return
        }
        // Toneless emoji → its best match for every term so far, summed; only emoji every
        // term matches are kept.
        var found: [String: Match] = [:]
        for (index, term) in terms.enumerated() {
            var best: [String: Match] = [:]
            for (word, emoji) in words.words(startingWith: term) {
                // The emoji the word names, then "|" and those it is only a keyword of.
                let named = emoji.firstIndex(of: "|") ?? emoji.count
                let reach = emoji[..<named].count { order[EmojiSupport.toneless($0)] != nil }
                let exact = word == term || word == term + "s"
                for (position, each) in emoji.enumerated() where position != named {
                    let key = EmojiSupport.toneless(each)
                    guard order[key] != nil else { continue }
                    let match = Match(emoji: each, score: (position < named ? 2 : 0) + (exact ? 1 : 0), reach: reach)
                    if best[key].map({ match.isBetter(than: $0) }) ?? true { best[key] = match }
                }
            }
            if index == 0 {
                found = best
            } else {
                var both: [String: Match] = [:]
                for (key, match) in best {
                    guard let earlier = found[key] else { continue }
                    both[key] = Match(emoji: earlier.emoji, score: earlier.score + match.score, reach: earlier.reach + match.reach)
                }
                found = both
            }
            if found.isEmpty { break }
        }
        let never = Int.max
        results = found.sorted { a, b in
            if a.value.score != b.value.score { return a.value.score > b.value.score }
            let (ra, rb) = (recentRank[a.key] ?? never, recentRank[b.key] ?? never)
            if ra != rb { return ra < rb }
            if a.value.reach != b.value.reach { return a.value.reach > b.value.reach }
            return order[a.key, default: never] < order[b.key, default: never]
        }
        .prefix(Self.maxResults)
        .map { history.toned($0.value.emoji) }
    }

    /// How a query term found an emoji: `score` 2 for a word of its name and 1 for the term's
    /// own word (or its plural), and `reach`, how many emoji (drawn here) the word names: of
    /// the words "hea" begins, "heart" (28 emoji) is likelier meant than "head" (6).
    private struct Match {
        let emoji: String
        let score: Int
        let reach: Int

        func isBetter(than other: Match) -> Bool {
            score != other.score ? score > other.score : reach > other.reach
        }
    }
}

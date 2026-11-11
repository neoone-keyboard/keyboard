import Foundation
import Testing
import KeyboardCore

private let bundledEmoji = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent()
    .appending(path: "KeyboardExtension/Emoji")

private let catalog = EmojiCatalog(text: """
    =Smileys & Emotion
    😀 1.0
    😍 0.6
    🥰 11.0
    ❤️ 0.6
    🫩 16.0
    =People & Body
    👋 0.6 1.0 👋🏻 👋🏼 👋🏽 👋🏾 👋🏿
    =Food & Drink
    🍎 0.6
    🍕 0.6
    =Symbols
    🔴 0.6
    🫈 17.0
    """, system: OperatingSystemVersion(majorVersion: 26, minorVersion: 0, patchVersion: 0))

/// Sorted by UTF-8 bytes, as emoji_data.py writes it: the emoji a word names, then "|" and
/// those it is only a keyword of. 🫈 is newer than the system.
private let words = EmojiWords(data: Data("""
    # Emoji search
    apple\t🍎
    face\t😀 😍 🥰
    hand\t👋
    hea\t| 🫈
    heart\t😍 🥰 ❤️ | 🫈
    hearts\t🥰
    heat\t| 😀
    pizza\t🍕
    red\t❤️ 🔴 | 🍎
    tired\t🫩
    wave\t👋
    ഹൃദയം\t| 😍 ❤️

    """.utf8))

private func search(_ query: String, history: EmojiHistory = EmojiHistory()) -> [String] {
    var search = EmojiSearch(words: words, catalog: catalog, history: history)
    search.type(query)
    return search.results
}

// MARK: Results

@Test func aWordThatIsTheQueryComesBeforeOnesItBegins() {
    // "heart" names ❤️, 😍 and 🥰; "hearts" only adds to 🥰, whose best is still "heart".
    #expect(search("heart") == ["😍", "🥰", "❤️"])
    // For "hear", every match only begins its word: the panel's order.
    #expect(search("hear") == ["😍", "🥰", "❤️"])
    #expect(search("pizz") == ["🍕"])
    #expect(search("HEART") == search("heart"))
}

@Test func aWordOfTheNameComesBeforeAKeywordOnly() {
    // 😀 has "heat" only as a keyword (as 🫠 has in CLDR); the hearts are named "heart".
    #expect(search("hea") == ["😍", "🥰", "❤️", "😀"])
    // 🍎 has "red" as a keyword, even exactly; ❤️ and 🔴 are named "red".
    #expect(search("red") == ["❤️", "🔴", "🍎"])
}

@Test func recentsComeBeforeThePanelsOrderButNotBeforeABetterMatch() {
    var history = EmojiHistory()
    history.record("❤️")
    history.record("🔴")
    #expect(search("hear", history: history) == ["❤️", "😍", "🥰"])
    // "re" only begins "red", which names ❤️ and 🔴: the recents first.
    #expect(search("re", history: history) == ["🔴", "❤️", "🍎"])
    history.record("🍎")
    #expect(search("re", history: history) == ["🔴", "❤️", "🍎"], "a keyword only stays below the names")
}

@Test func everyWordOfTheQueryMustMatch() {
    #expect(search("red heart") == ["❤️"])
    #expect(search("red hea") == ["❤️"])
    #expect(search("red pizza").isEmpty)
    #expect(search("heart ").count == 3, "a trailing space is no extra word")
}

@Test func emojiTheSystemDoesntDrawAreLeftOut() {
    #expect(!search("hea").contains("🫈"))
    #expect(search("tired") == ["🫩"])
}

@Test func resultsComeInTheUsersTone() {
    var history = EmojiHistory()
    history.chooseTone("👋🏽", for: "👋")
    #expect(search("wave", history: history) == ["👋🏽"])
}

@Test func malayalamFindsItsKeywordsFromTheirFirstLetters() {
    #expect(search("ഹൃദയം") == ["😍", "❤️"])
    #expect(search("ഹ") == ["😍", "❤️"])
}

@Test func anEmptyQueryShowsTheRecents() {
    var history = EmojiHistory()
    history.record("🍕")
    history.record("👋🏾")
    #expect(search("", history: history) == ["👋🏾", "🍕"])
    #expect(search("zzz", history: history).isEmpty)
}

// MARK: Keys

@Test func lettersSpaceAndBackspaceEditTheQuery() {
    var search = EmojiSearch(words: words, catalog: catalog, history: EmojiHistory())
    #expect(search.handle(.space) == .edited)
    #expect(search.query == "", "no leading space")
    for c in ["r", "e", "d"] { #expect(search.handle(.character(c)) == .edited) }
    #expect(search.results == ["❤️", "🔴", "🍎"])
    #expect(search.handle(.space) == .edited)
    _ = search.handle(.space)
    #expect(search.query == "red ", "one space between words")
    for c in ["h", "e", "a"] { _ = search.handle(.character(c)) }
    #expect(search.results == ["❤️"])
    #expect(search.handle(.backspace) == .edited)
    #expect(search.query == "red he")
    search.clearQuery()
    #expect(search.query.isEmpty)
    #expect(search.handle(.backspace) == .edited)
}

@Test func backspaceTakesOneScalarOffMalayalam() {
    var search = EmojiSearch(words: words, catalog: catalog, history: EmojiHistory())
    search.type("ഹൃ")
    _ = search.handle(.backspace)
    #expect(search.query == "ഹ")
}

@Test func keysThatChangeTheKeysPassThroughAndReturnCloses() {
    var search = EmojiSearch(words: words, catalog: catalog, history: EmojiHistory())
    #expect(search.handle(.page(.symbols)) == .passThrough)
    #expect(search.handle(.page(.native("letters"))) == .passThrough)
    #expect(search.handle(.toggleLanguage) == .passThrough)
    #expect(search.handle(.nextKeyboard) == .passThrough)
    #expect(search.handle(.shift) == .ignored)
    #expect(search.handle(.returnKey) == .close)
    #expect(search.handle(.emoji) == .close)
    #expect(search.query.isEmpty)
}

@Test func theQueryHasALimit() {
    var search = EmojiSearch(words: words, catalog: catalog, history: EmojiHistory())
    search.type(String(repeating: "a", count: 100))
    #expect(search.query.count == EmojiSearch.maxQueryLength)
}

// MARK: The bundled index

@Test func prefixLookupFindsEveryWordItBegins() {
    #expect(words.words(startingWith: "hea").map(\.word) == ["hea", "heart", "hearts", "heat"])
    #expect(words.words(startingWith: "Heart").map(\.word) == ["heart", "hearts"])
    #expect(words.words(startingWith: "z").isEmpty)
    #expect(words.words(startingWith: "").isEmpty)
    #expect(words.words(startingWith: "ഹൃ").map(\.emoji) == [["|", "😍", "❤️"]])
}

/// What people type, against the bundled index: the emoji they mean first.
@Test func commonQueriesFindWhatTheyMean() throws {
    let data = try Data(contentsOf: bundledEmoji.appending(path: "emoji-search.txt"), options: .alwaysMapped)
    let text = try String(contentsOf: bundledEmoji.appending(path: "emoji.txt"), encoding: .utf8)
    let bundled = EmojiSearch(words: EmojiWords(data: data), catalog: EmojiCatalog(text: text), history: EmojiHistory())
    func top(_ query: String) -> [String] {
        var each = bundled
        each.type(query)
        return Array(each.results.prefix(8))
    }
    let hearts: Set = ["😍", "😻", "💘", "💝", "💖", "💗", "💓", "💟", "❤️", "🥰"]
    #expect(top("hea").allSatisfy(hearts.contains), "\(top("hea"))")
    #expect(!top("hea").contains("🫠"), "heat is only a keyword of 🫠")
    #expect(top("lau").first == "🤣")
    #expect(top("lau").contains("😂"))
    #expect(top("fire").prefix(3).contains("🔥"))
    // "thumbs" is as good as "thumb" itself, and names more emoji.
    #expect(top("thumb").prefix(2) == ["👍", "👎"])
    #expect(top("cry").prefix(2) == ["😢", "😭"])
    #expect(top("pizza").first == "🍕")
    #expect(top("love").first == "💌")
    #expect(top("love").contains("🥰") && top("love").contains("😍"))
}

@Test func theBundledIndexFindsTheRedHeartAndIsQuick() throws {
    let data = try Data(contentsOf: bundledEmoji.appending(path: "emoji-search.txt"), options: .alwaysMapped)
    let text = try String(contentsOf: bundledEmoji.appending(path: "emoji.txt"), encoding: .utf8)
    // The app's English index, and the Malayalam pack's own after it, as the keyboard reads them.
    let packs = try Data(contentsOf: #require(malayalamPack.emojiSearch), options: .alwaysMapped)
    let bundled = EmojiSearch(words: EmojiWords(files: [data, packs]), catalog: EmojiCatalog(text: text), history: EmojiHistory())
    var search = bundled
    search.type("heart")
    #expect(search.results.contains("❤️"))
    search.clearQuery()
    search.type("ഹൃദയം")
    #expect(search.results.contains("❤️"))
    var english = EmojiSearch(words: EmojiWords(data: data), catalog: EmojiCatalog(text: text), history: EmojiHistory())
    english.type("ഹൃദയം")
    #expect(english.results.isEmpty, "Malayalam's words come with its pack")
    // The worst case is one letter: every word it begins. Each key runs a search.
    let clock = ContinuousClock()
    var slowest = Duration.zero
    for letter in "abcdefghijklmnopqrstuvwxyz" {
        var each = bundled
        let elapsed = clock.measure { each.type(String(letter)) }
        slowest = max(slowest, elapsed)
    }
    print("slowest one-letter emoji search: \(slowest)")
    #expect(slowest < .milliseconds(30))
}

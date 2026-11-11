import Foundation
import Testing
import KeyboardCore

/// The keyboard's bundled emoji files, found from this file's path (…/apps/ios/Packages/KeyboardCore/Tests/KeyboardCoreTests/).
private let bundledEmoji = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent()
    .appending(path: "KeyboardExtension/Emoji")

private let catalogText = """
    # A comment
    =Smileys & Emotion
    😀 1.0
    🫩 16.0
    🫈 17.0
    🫪 18.0
    =People & Body
    👋 0.6 1.0 👋🏻 👋🏼 👋🏽 👋🏾 👋🏿
    🤝 3.0 14.0 🤝🏻 🤝🏼 🤝🏽 🤝🏾 🤝🏿
    =Flags
    🏁 0.6
    """

private let iOS26 = OperatingSystemVersion(majorVersion: 26, minorVersion: 0, patchVersion: 0)
private let iOS26_4 = OperatingSystemVersion(majorVersion: 26, minorVersion: 4, patchVersion: 0)

// MARK: Which emoji iOS can draw

@Test func emojiVersionsMapToTheFirstIOSThatDrawsThem() {
    #expect(EmojiSupport.isSupported("0.6", on: iOS26))
    #expect(EmojiSupport.isSupported("16.0", on: iOS26))
    #expect(!EmojiSupport.isSupported("17.0", on: iOS26))
    #expect(EmojiSupport.isSupported("17.0", on: iOS26_4))
    // A version newer than the table is hidden until the table learns it.
    #expect(!EmojiSupport.isSupported("18.0", on: OperatingSystemVersion(majorVersion: 99, minorVersion: 0, patchVersion: 0)))
    #expect(!EmojiSupport.isSupported("garbage", on: iOS26))
}

// MARK: Catalog

@Test func catalogKeepsGroupsInOrderAndOnlyEmojiTheSystemDraws() {
    let catalog = EmojiCatalog(text: catalogText, system: iOS26)
    #expect(catalog.categories.map(\.name) == ["Smileys & Emotion", "People & Body", "Flags"])
    #expect(catalog.categories[0].items.map(\.emoji) == ["😀", "🫩"])
    #expect(EmojiCatalog(text: catalogText, system: iOS26_4).categories[0].items.map(\.emoji) == ["😀", "🫩", "🫈"])
}

@Test func catalogKnowsSkinTonesAndTheirBase() {
    let catalog = EmojiCatalog(text: catalogText, system: iOS26)
    let wave = catalog.categories[1].items[0]
    #expect(wave.tones == ["👋🏻", "👋🏼", "👋🏽", "👋🏾", "👋🏿"])
    #expect(catalog.categories[2].items[0].tones.isEmpty)
    #expect(catalog.base(of: "👋🏽") == "👋")
    #expect(catalog.base(of: "👋") == "👋")
    #expect(catalog.base(of: "x") == nil)
    #expect(catalog.tones(of: "👋🏾").count == 5)
}

@Test func tonesNewerThanTheSystemAreLeftOut() {
    let old = EmojiCatalog(text: catalogText, system: OperatingSystemVersion(majorVersion: 15, minorVersion: 0, patchVersion: 0))
    let handshake = old.categories[1].items[1]
    #expect(handshake.emoji == "🤝")
    #expect(handshake.tones.isEmpty)
}

@Test func bundledCatalogHasTheNineUnicodeGroups() throws {
    let text = try String(contentsOf: bundledEmoji.appending(path: "emoji.txt"), encoding: .utf8)
    let catalog = EmojiCatalog(text: text, system: iOS26)
    #expect(catalog.categories.map(\.name) == [
        "Smileys & Emotion", "People & Body", "Animals & Nature", "Food & Drink", "Travel & Places",
        "Activities", "Objects", "Symbols", "Flags",
    ])
    #expect(catalog.categories.reduce(0) { $0 + $1.items.count } > 1800)
    #expect(catalog.categories[0].items.first?.emoji == "😀")
}

// MARK: Recents and tones

@Test func recentsAreMostRecentFirstWithoutRepeats() {
    var history = EmojiHistory()
    history.record("😀")
    history.record("🍕")
    history.record("😀")
    #expect(history.recents == ["😀", "🍕"])
}

@Test func aNewToneReplacesTheOldOneInRecents() {
    var history = EmojiHistory()
    history.record("👋🏻")
    history.record("🍕")
    history.record("👋🏾")
    #expect(history.recents == ["👋🏾", "🍕"])
}

@Test func recentsAreCapped() {
    var history = EmojiHistory()
    let many = (0..<40).map { String(UnicodeScalar(0x1F600 + $0)!) }
    many.forEach { history.record($0) }
    #expect(history.recents.count == EmojiHistory.recentLimit)
    #expect(EmojiHistory.recentLimit == 30)
    #expect(history.recents.first == many.last)
}

@Test func theChosenToneIsRememberedPerEmoji() {
    var history = EmojiHistory()
    #expect(history.toned("👋") == "👋")
    history.chooseTone("👋🏽", for: "👋")
    #expect(history.toned("👋") == "👋🏽")
    #expect(history.toned("🤝") == "🤝")
    history.chooseTone("👋", for: "👋") // back to no tone
    #expect(history.toned("👋") == "👋")
}

@Test func historyRoundTripsThroughAFileAndForgets() throws {
    let url = FileManager.default.temporaryDirectory.appending(path: "emoji-\(UUID().uuidString)/history.json")
    #expect(EmojiHistory.load(from: url) == EmojiHistory(), "no file is an empty history")
    var history = EmojiHistory()
    history.record("👋🏽")
    history.chooseTone("👋🏽", for: "👋")
    try history.save(to: url)
    #expect(EmojiHistory.load(from: url) == history)
    try Data("not json".utf8).write(to: url)
    #expect(EmojiHistory.load(from: url) == EmojiHistory(), "a damaged file is an empty history")
    try EmojiHistory.forget(at: url)
    #expect(!FileManager.default.fileExists(atPath: url.path))
    try EmojiHistory.forget(at: url) // nothing to forget is fine
}

// MARK: Words

private let wordsText = """
    # header
    # more header
    cat\t🐈 😺
    cats\t🐈
    happy\t😀 ☺️
    love\t🥰 ❤️
    pizza\t🍕
    zoo\t🦓
    സ്നേഹം\t❤️

    """

@Test func wordsAreLookedUpWholeAndLowercased() {
    let words = EmojiWords(data: Data(wordsText.utf8))
    #expect(words.emoji(for: "pizza") == ["🍕"])
    #expect(words.emoji(for: "Love") == ["🥰", "❤️"])
    #expect(words.emoji(for: "cat") == ["🐈", "😺"])
    #expect(words.emoji(for: "cats") == ["🐈"])
    #expect(words.emoji(for: "ca").isEmpty, "never a prefix")
    #expect(words.emoji(for: "zoos").isEmpty)
    #expect(words.emoji(for: "aaa").isEmpty)
    #expect(words.emoji(for: "zzz").isEmpty)
    #expect(words.emoji(for: "സ്നേഹം") == ["❤️"])
    #expect(words.emoji(for: "").isEmpty)
    #expect(words.emoji(for: "# header").isEmpty)
    #expect(EmojiWords(data: Data()).emoji(for: "cat").isEmpty)
}

@Test func everyWordInTheBundledListIsFound() throws {
    let data = try Data(contentsOf: bundledEmoji.appending(path: "emoji-words.txt"))
    let words = EmojiWords(data: data)
    let lines = String(decoding: data, as: UTF8.self).split(separator: "\n").filter { !$0.hasPrefix("#") }
    #expect(lines.count > 3000)
    for line in lines {
        let parts = line.split(separator: "\t")
        #expect(words.emoji(for: String(parts[0])) == parts[1].split(separator: " ").map(String.init), "\(parts[0])")
    }
    #expect(words.emoji(for: "pizza").first == "🍕")
    // The app's own list is English: a pack's language comes with the pack.
    #expect(!lines.contains { malayalamPack.writes(String($0)) })
    #expect(words.emoji(for: "സ്നേഹം").isEmpty)
}

/// The Malayalam pack brings its own emoji words (the `emojiWords` role); with them after the
/// app's, every Malayalam word finds its emoji, and English is as without the pack.
@Test func aPacksEmojiWordsAreReadAfterTheApps() throws {
    let own = try Data(contentsOf: bundledEmoji.appending(path: "emoji-words.txt"))
    let url = try #require(malayalamPack.emojiWords)
    let packs = try Data(contentsOf: url)
    let words = EmojiWords(files: [own, packs])
    let lines = String(decoding: packs, as: UTF8.self).split(separator: "\n").filter { !$0.hasPrefix("#") }
    #expect(lines.count > 2500)
    let alone = EmojiWords(data: own)
    var inBoth = 0
    for line in lines {
        let parts = line.split(separator: "\t")
        let word = String(parts[0])
        let english = alone.emoji(for: word)
        inBoth += english.isEmpty ? 0 : 1
        let both = english + parts[1].split(separator: " ").map(String.init).filter { !english.contains($0) }
        #expect(words.emoji(for: word) == Array(both.prefix(EmojiWords.maxPerWord)), "\(word)")
    }
    #expect(inBoth < 10, "a handful of Latin words are in both lists")
    #expect(words.emoji(for: "പൂച്ച").first == "🐈")
    #expect(words.emoji(for: "pizza") == alone.emoji(for: "pizza"))
    #expect(pack("hi").emojiWords == nil, "a pack without the role brings none")
    // The app's copy for a Malayalam pack published before the role is the same file.
    let fallback = try Data(contentsOf: bundledEmoji.appending(path: "packs/ml/emoji-words.txt"))
    #expect(fallback == packs)
    let search = try Data(contentsOf: bundledEmoji.appending(path: "packs/ml/emoji-search.txt"))
    #expect(try search == Data(contentsOf: #require(malayalamPack.emojiSearch)))
}

@Test func aWordInSeveralListsHasEachListsEmoji() {
    let first = Data("cat\t🐈 🐱\nlove\t❤️\n".utf8)
    let second = Data("cat\t🐱 😺\nപൂച്ച\t🐈\n".utf8)
    let words = EmojiWords(files: [first, second])
    #expect(words.emoji(for: "cat") == ["🐈", "🐱", "😺"])
    #expect(words.emoji(for: "love") == ["❤️"])
    #expect(words.emoji(for: "പൂച്ച") == ["🐈"])
    #expect(words.emoji(for: "dog").isEmpty)
    // Search rows: the emoji a word names, "|", those it is a keyword of.
    let a = Data("heart\t❤️ | 😍\nheat\t| 🫠\n".utf8)
    let b = Data("heart\t💘 | ❤️ 🥰\nhearts\t💕\n".utf8)
    let found = EmojiWords(files: [a, b]).words(startingWith: "hea")
    #expect(found.map(\.word) == ["heart", "heat", "hearts"])
    #expect(found[0].emoji == ["❤️", "💘", "|", "😍", "🥰"])
    #expect(found[1].emoji == ["|", "🫠"])
}

// MARK: The bar's emoji

@Test func theBarsEmojiIsForTheWordBeingTypedOrTheOneJustFinished() {
    #expect(EmojiSuggestion.word(contextBefore: "I want pizza", currentWord: "pizza") == "pizza")
    #expect(EmojiSuggestion.word(contextBefore: "I want pizza ", currentWord: "") == "pizza")
    #expect(EmojiSuggestion.word(contextBefore: "I want pizza  ", currentWord: "") == nil)
    #expect(EmojiSuggestion.word(contextBefore: "I want pizza.", currentWord: "") == nil)
    #expect(EmojiSuggestion.word(contextBefore: "pizza 🍕 ", currentWord: "") == nil)
    #expect(EmojiSuggestion.word(contextBefore: "", currentWord: "") == nil)
    #expect(EmojiSuggestion.word(contextBefore: "എനിക്ക് സ്നേഹം ", currentWord: "") == "സ്നേഹം")
}

@Test func theBarPrefersTheEmojiUsedLastInTheUsersTone() {
    var history = EmojiHistory()
    #expect(EmojiSuggestion.pick(["🥰", "❤️"], history: history) == "🥰")
    history.record("❤️")
    history.record("😀")
    #expect(EmojiSuggestion.pick(["🥰", "❤️"], history: history) == "❤️")
    history.chooseTone("👋🏾", for: "👋")
    #expect(EmojiSuggestion.pick(["👋"], history: history) == "👋🏾")
    #expect(EmojiSuggestion.pick([], history: history) == nil)
}

@Test func theEmojiGoesInTheRightSlotNeverTheMiddle() {
    let result = SuggestionResult(typed: "pizza", candidates: [
        Suggestion("pizza", kind: .verbatim), Suggestion("pizzas", kind: .completion), Suggestion("pizzeria", kind: .completion),
    ])
    let bar = SuggestionBar(result).withEmoji("🍕")
    #expect(bar.middle?.title == "pizza")
    #expect(bar.left?.title == "pizzas")
    #expect(bar.right?.suggestion == Suggestion("🍕", kind: .emoji))
    #expect(SuggestionBar(.empty).withEmoji("🍕").slots.map { $0?.title } == [nil, nil, "🍕"])
    #expect(SuggestionBar(result).withEmoji(nil) == SuggestionBar(result))
}

@Test func anEmojiSlotCannotBeRemoved() {
    #expect(SuggestionRemoval(slot: SuggestionSlot(suggestion: Suggestion("🍕", kind: .emoji), quoted: false)) == nil)
}

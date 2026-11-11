import Foundation
import Testing
import KeyboardCore

/// Which emoji word files the keyboard reads for the active packs: the app's own, then each
/// pack's, with the app's copy standing in for a pack published without them.

/// apps/ios/KeyboardExtension/Emoji, as the extension bundles it.
private let bundledEmoji = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent()
    .appending(path: "KeyboardExtension/Emoji", directoryHint: .isDirectory)

/// The lists as the view controller holds them: one value, asked again and again.
private final class Lists {
    private var lists: EmojiWordLists

    init(folder: URL?) {
        lists = EmojiWordLists(folder: folder)
    }

    func words(_ list: EmojiWordLists.List, packs: [PackDescriptor]) -> EmojiWords? {
        lists.words(list, packs: packs)
    }
}

/// A Malayalam pack published before packs carried their emoji words.
private let oldMalayalam = PackDescriptor(id: "ml", name: "Malayalam", nativeName: "മലയാളം", script: "mlym",
                                          layout: nil, folder: URL(filePath: "/nowhere"), version: "2026.11.06")

@Test func malayalamsEmojiWordsNeedMalayalamToBeActive() throws {
    let lists = Lists(folder: bundledEmoji)
    let english = try #require(lists.words(.bar, packs: []))
    #expect(english.emoji(for: "cat").first == "🐈")
    #expect(english.emoji(for: "പൂച്ച").isEmpty)

    let both = try #require(lists.words(.bar, packs: [malayalamPack]))
    #expect(both.emoji(for: "പൂച്ച").first == "🐈")
    #expect(both.emoji(for: "cat") == english.emoji(for: "cat"))

    // Hindi brings no emoji words, and the app has no copy for it.
    let hindi = try #require(lists.words(.bar, packs: [pack("hi")]))
    #expect(hindi.emoji(for: "പൂച്ച").isEmpty)
    #expect(hindi.emoji(for: "cat") == english.emoji(for: "cat"))

    let three = try #require(lists.words(.bar, packs: [pack("hi"), malayalamPack, pack("ta")]))
    #expect(three.emoji(for: "പൂച്ച").first == "🐈")
}

@Test func thePanelsSearchReadsThePacksSearchWords() throws {
    let lists = Lists(folder: bundledEmoji)
    let english = try #require(lists.words(.search, packs: []))
    #expect(english.words(startingWith: "പൂച്ച").isEmpty)
    #expect(english.words(startingWith: "pizza").first?.emoji.first == "🍕")

    let both = try #require(lists.words(.search, packs: [malayalamPack]))
    #expect(both.words(startingWith: "പൂച്ച").map(\.word).contains("പൂച്ചെണ്ട്"))
    #expect(both.words(startingWith: "pizza").first?.emoji.first == "🍕")
    // The bar's list isn't the search's: it has whole words only.
    let bar = try #require(lists.words(.bar, packs: [malayalamPack]))
    #expect(bar.emoji(for: "പൂച്ചയുടെ").isEmpty)
    #expect(!both.words(startingWith: "പൂച്ചയുടെ").isEmpty)
}

/// Every published Malayalam pack is without the two roles: the keyboard reads the app's
/// copy (Emoji/packs/ml/), and gives what the pack's own files give.
@Test func aMalayalamPackWithoutEmojiWordsGetsTheAppsCopy() throws {
    #expect(oldMalayalam.emojiWords == nil && oldMalayalam.emojiSearch == nil)
    for list in EmojiWordLists.List.allCases {
        let files = EmojiWordLists.files(list, in: bundledEmoji, packs: [oldMalayalam, malayalamPack, pack("hi")])
        #expect(files.app == bundledEmoji.appending(path: "\(list.rawValue).txt"))
        #expect(files.packs[0] == bundledEmoji.appending(path: "packs/ml/\(list.rawValue).txt"))
        #expect(files.packs[1] == packsFolder.appending(path: "ml.neopack/\(list.rawValue).txt"), "its own when it brings them")
        #expect(files.packs[2] == bundledEmoji.appending(path: "packs/hi/\(list.rawValue).txt"))
        #expect(FileManager.default.fileExists(atPath: files.packs[0].path))
        #expect(!FileManager.default.fileExists(atPath: files.packs[2].path))
        #expect(try Data(contentsOf: files.packs[0]) == Data(contentsOf: files.packs[1]), "the copy is the pack's file")
    }

    let lists = Lists(folder: bundledEmoji)
    let old = try #require(lists.words(.bar, packs: [oldMalayalam]))
    #expect(old.emoji(for: "പൂച്ച") == ["🐈", "😺", "😸", "😹", "😻", "😼"])
    #expect(old.emoji(for: "cat").first == "🐈")
    let search = try #require(lists.words(.search, packs: [oldMalayalam]))
    #expect(search.words(startingWith: "പൂച്ചെണ്ട്").first?.emoji == ["💐"])
}

/// A small Emoji folder and packs, to tell the files apart.
private struct EmojiFixture {
    let root: URL
    var emoji: URL { root.appending(path: "Emoji", directoryHint: .isDirectory) }

    init() throws {
        root = try temporaryFolder()
        try write("cat\t🐈\n", to: emoji.appending(path: "emoji-words.txt"))
        try write("cat\t🐈\n", to: emoji.appending(path: "emoji-search.txt"))
    }

    func write(_ text: String, to file: URL) throws {
        try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try Data(text.utf8).write(to: file)
    }

    /// A pack `id` whose own bar words are `words` (nil: it brings none).
    func pack(_ id: String, version: String = "1", words: String?) throws -> PackDescriptor {
        let folder = root.appending(path: "\(id)-\(version).neopack", directoryHint: .isDirectory)
        let file = folder.appending(path: "emoji-words.txt")
        if let words { try write(words, to: file) }
        return PackDescriptor(id: id, name: id, nativeName: id, script: "mlym", layout: nil, folder: folder,
                              version: version, emojiWords: words == nil ? nil : file)
    }

    func cleanUp() {
        try? FileManager.default.removeItem(at: root)
    }
}

@Test func aPacksOwnWordsAreReadRatherThanTheAppsCopy() throws {
    let fixture = try EmojiFixture()
    defer { fixture.cleanUp() }
    try fixture.write("പൂച്ച\t🐱\n", to: fixture.emoji.appending(path: "packs/ml/emoji-words.txt"))
    let lists = Lists(folder: fixture.emoji)
    let own = try fixture.pack("ml", words: "പൂച്ച\t🐈\n")
    #expect(lists.words(.bar, packs: [own])?.emoji(for: "പൂച്ച") == ["🐈"])
    let without = try fixture.pack("ml", version: "0", words: nil)
    #expect(lists.words(.bar, packs: [without])?.emoji(for: "പൂച്ച") == ["🐱"])
}

@Test func aWordsEmojiAreTheAppsThenEachActivePacksInOrder() throws {
    let fixture = try EmojiFixture()
    defer { fixture.cleanUp() }
    let lists = Lists(folder: fixture.emoji)
    let first = try fixture.pack("aa", words: "cat\t😺\n")
    let second = try fixture.pack("bb", words: "cat\t😸\n")
    #expect(lists.words(.bar, packs: [first, second])?.emoji(for: "cat") == ["🐈", "😺", "😸"])
    #expect(lists.words(.bar, packs: [second, first])?.emoji(for: "cat") == ["🐈", "😸", "😺"])
    #expect(lists.words(.bar, packs: [second])?.emoji(for: "cat") == ["🐈", "😸"])
    #expect(lists.words(.bar, packs: [])?.emoji(for: "cat") == ["🐈"])
}

@Test func aPackWhoseFileCannotBeReadIsLeftOut() throws {
    let fixture = try EmojiFixture()
    defer { fixture.cleanUp() }
    let lists = Lists(folder: fixture.emoji)
    let gone = try fixture.pack("aa", words: "cat\t😺\n")
    try FileManager.default.removeItem(at: gone.folder)
    let there = try fixture.pack("bb", words: "cat\t😸\n")
    #expect(lists.words(.bar, packs: [gone, there])?.emoji(for: "cat") == ["🐈", "😸"])
}

/// The lists are held, not read per key; an update to a pack (a new version) or a change of
/// the active languages reads them again.
@Test func theListsAreHeldUntilTheActivePacksChange() throws {
    let fixture = try EmojiFixture()
    defer { fixture.cleanUp() }
    let lists = Lists(folder: fixture.emoji)
    let old = try fixture.pack("ml", version: "1", words: "പൂച്ച\t🐱\n")
    #expect(lists.words(.bar, packs: [old])?.emoji(for: "പൂച്ച") == ["🐱"])

    let new = try fixture.pack("ml", version: "2", words: "പൂച്ച\t🐈\n")
    #expect(new == old, "one pack per id, whatever the version")
    #expect(lists.words(.bar, packs: [new])?.emoji(for: "പൂച്ച") == ["🐈"])
    #expect(lists.words(.bar, packs: [])?.emoji(for: "പൂച്ച") == [])
    #expect(lists.words(.bar, packs: [new])?.emoji(for: "പൂച്ച") == ["🐈"])

    // Held: the same packs don't read the folder again.
    try FileManager.default.removeItem(at: fixture.emoji)
    #expect(lists.words(.bar, packs: [new])?.emoji(for: "പൂച്ച") == ["🐈"])
    #expect(lists.words(.search, packs: [new]) == nil, "the search list is held apart, and was never read")
}

@Test func withoutTheAppsFileThereAreNoEmojiWords() throws {
    let fixture = try EmojiFixture()
    defer { fixture.cleanUp() }
    try FileManager.default.removeItem(at: fixture.emoji.appending(path: "emoji-words.txt"))
    let ml = try fixture.pack("ml", words: "പൂച്ച\t🐈\n")
    let lists = Lists(folder: fixture.emoji)
    #expect(lists.words(.bar, packs: [ml]) == nil)
    #expect(lists.words(.search, packs: [ml]) != nil)
    let none = Lists(folder: nil)
    #expect(none.words(.bar, packs: [ml]) == nil)
}

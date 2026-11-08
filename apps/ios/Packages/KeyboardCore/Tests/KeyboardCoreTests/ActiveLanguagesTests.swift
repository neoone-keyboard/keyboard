import Foundation
import Testing
@testable import KeyboardCore

@Test func theChoiceIsWrittenAndReadBack() throws {
    let folder = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let file = folder.appending(path: "active-languages.json")
    try ActiveLanguages(["ml", "hi"]).write(to: file)
    #expect(ActiveLanguages.read(from: file) == ActiveLanguages(["ml", "hi"]))
}

@Test func aMissingOrBrokenFileIsNoChoice() throws {
    let folder = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let file = folder.appending(path: "active-languages.json")
    #expect(ActiveLanguages.read(from: file) == nil)
    try Data("not json".utf8).write(to: file)
    #expect(ActiveLanguages.read(from: file) == nil)
    try Data(#"{"languages": []}"#.utf8).write(to: file)
    #expect(ActiveLanguages.read(from: file) == nil)
}

@Test func idsTheKeyboardHasNoPackForAreDropped() {
    let choice = ActiveLanguages(["en", "hi"])
    #expect(choice.usable(packIDs: ["ml", "hi"]) == ["en", "hi"])
    #expect(choice.usable(packIDs: ["ml"]) == ["en"])
    #expect(ActiveLanguages(["hi"]).usable(packIDs: ["ml"]) == nil)
    // More than the engine takes: the first ones.
    #expect(ActiveLanguages(["en", "ml", "hi", "as", "ta"]).usable(packIDs: ["ml", "hi", "as", "ta"]) == ["en", "ml", "hi", "as"])
    #expect(ActiveLanguages(["en", "ml", "hi"]).usable(packIDs: ["ml", "hi"]) == ["en", "ml", "hi"])
    // A repeat once.
    #expect(ActiveLanguages(["ml", "ml"]).usable(packIDs: ["ml"]) == ["ml"])
}

@Test func theLimitIsTheEngines() {
    // engine/core/src/lang.rs MAX_ACTIVE, English counted.
    #expect(ActiveLanguages.maxActive == 4)
}

@Test func theDefaultIsEnglishAndTheFirstPack() {
    #expect(ActiveLanguages.engineDefault(packIDs: ["ml", "hi"]).languages == ["en", "ml"])
    #expect(ActiveLanguages.engineDefault(packIDs: ["ml", "hi", "as"]).languages == ["en", "ml"])
    #expect(ActiveLanguages.engineDefault(packIDs: []).languages == ["en"])
}

@Test func togglingKeepsListOrderAtMostTheLimitAndAtLeastOne() {
    let order = ["en", "ml", "hi", "as", "ta"]
    var choice = ActiveLanguages(["en"])
    choice = choice.toggling("ml", order: order)
    #expect(choice.languages == ["en", "ml"])
    // A third and a fourth, in list order whatever the tap order.
    choice = choice.toggling("as", order: order).toggling("hi", order: order)
    #expect(choice.languages == ["en", "ml", "hi", "as"])
    // Full: a fifth is not added.
    #expect(!choice.canTurnOn("ta"))
    #expect(choice.toggling("ta", order: order).languages == ["en", "ml", "hi", "as"])
    // English off, then Tamil on: list order, not tap order.
    choice = choice.toggling("en", order: order).toggling("ta", order: order)
    #expect(choice.languages == ["ml", "hi", "as", "ta"])
    choice = ActiveLanguages(["en", "ml"]).toggling("en", order: order).toggling("hi", order: order)
    #expect(choice.languages == ["ml", "hi"])
    choice = ActiveLanguages(["hi"]).toggling("en", order: order)
    #expect(choice.languages == ["en", "hi"])
    // The last one stays on.
    #expect(!ActiveLanguages(["ml"]).canTurnOff("ml"))
    #expect(ActiveLanguages(["ml"]).toggling("ml", order: order).languages == ["ml"])
}

@Test func removingAPackTakesItOutOfTheChoice() {
    #expect(ActiveLanguages(["en", "hi"]).removing("hi").languages == ["en"])
    #expect(ActiveLanguages(["hi"]).removing("hi").languages == ["en"])
    #expect(ActiveLanguages(["ml", "hi"]).removing("en").languages == ["ml", "hi"])
}

@Test func aPacksSizeIsItsFiles() throws {
    let folder = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    try Data(count: 1000).write(to: folder.appending(path: "a.fst"))
    try Data(count: 24).write(to: folder.appending(path: "manifest.json"))
    #expect(PackSize.of(folder: folder) == 1024)
    #expect(PackSize.of(folder: folder.appending(path: "missing")) == 0)
}

@Test func theNewerChoiceWins() {
    let early = Date(timeIntervalSince1970: 1000)
    let late = Date(timeIntervalSince1970: 2000)
    let app = ActiveLanguages(["en", "ml"])
    let keyboard = ActiveLanguages(["en", "hi"])
    // A later change in the keyboard wins over an older app choice, and the other way round.
    #expect(DatedLanguages.newer(app: DatedLanguages(app, date: early), keyboard: DatedLanguages(keyboard, date: late)) == keyboard)
    #expect(DatedLanguages.newer(app: DatedLanguages(app, date: late), keyboard: DatedLanguages(keyboard, date: early)) == app)
    // The same moment: the app's.
    #expect(DatedLanguages.newer(app: DatedLanguages(app, date: late), keyboard: DatedLanguages(keyboard, date: late)) == app)
    // Only one: that one.
    #expect(DatedLanguages.newer(app: DatedLanguages(app, date: late), keyboard: nil) == app)
    #expect(DatedLanguages.newer(app: nil, keyboard: DatedLanguages(keyboard, date: early)) == keyboard)
    #expect(DatedLanguages.newer(app: nil, keyboard: nil) == nil)
}

@Test func theAppGroupFileIsDatedByItsModificationDate() throws {
    let folder = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let file = folder.appending(path: "active-languages.json")
    #expect(DatedLanguages.read(from: file) == nil)
    try ActiveLanguages(["en", "hi"]).write(to: file)
    let date = Date(timeIntervalSince1970: 1_800_000_000)
    try FileManager.default.setAttributes([.modificationDate: date], ofItemAtPath: file.path)
    #expect(DatedLanguages.read(from: file) == DatedLanguages(ActiveLanguages(["en", "hi"]), date: date))
}

@Test func theKeyboardsChoiceIsSavedAndLoaded() throws {
    let suite = "KeyboardLanguageChoiceTests-\(UUID().uuidString)"
    let defaults = try #require(UserDefaults(suiteName: suite))
    defer { defaults.removePersistentDomain(forName: suite) }
    #expect(KeyboardLanguageChoice.load(from: defaults) == nil)
    let dated = DatedLanguages(ActiveLanguages(["ml", "hi"]), date: Date(timeIntervalSince1970: 1234.5))
    KeyboardLanguageChoice.save(dated, to: defaults)
    #expect(KeyboardLanguageChoice.load(from: defaults) == dated)
    // An empty choice is no choice.
    KeyboardLanguageChoice.save(DatedLanguages(ActiveLanguages([]), date: .now), to: defaults)
    #expect(KeyboardLanguageChoice.load(from: defaults) == nil)
}

@Test func thePanelsSwitchesFollowTheRules() {
    // The keyboard's settings panel: English then its packs, a switch enabled when it can toggle.
    let order = ["en", "ml", "hi", "as", "ta"]
    func enabled(_ choice: ActiveLanguages) -> [String] { order.filter(choice.canToggle) }
    // Not full: every switch works.
    #expect(enabled(ActiveLanguages(["en", "ml"])) == order)
    #expect(enabled(ActiveLanguages(["en", "ml", "hi"])) == order)
    // Full (four on): the fifth is disabled, every one on can go off.
    #expect(enabled(ActiveLanguages(["en", "ml", "hi", "as"])) == ["en", "ml", "hi", "as"])
    // One on: it can't go off, every other can come on.
    #expect(enabled(ActiveLanguages(["ml"])) == ["en", "hi", "as", "ta"])
    // Malayalam off, Hindi on: English and Hindi, in list order.
    let next = ActiveLanguages(["en", "ml"]).toggling("ml", order: order).toggling("hi", order: order)
    #expect(next.languages == ["en", "hi"])
}

/// The app's Languages summary reads the file again on each reload, so a choice the keyboard's
/// settings panel writes meanwhile shows without relaunching the app.
@Test func theShownChoiceFollowsTheFileTheKeyboardRewrites() throws {
    let folder = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let file = folder.appending(path: "active-languages.json")
    let packIDs = ["ml", "hi", "as"]
    // Nothing stored, or no App Group: the engine's default.
    #expect(ActiveLanguages.shown(storedIn: file, packIDs: packIDs) == ActiveLanguages(["en", "ml"]))
    #expect(ActiveLanguages.shown(storedIn: nil, packIDs: packIDs) == ActiveLanguages(["en", "ml"]))
    try ActiveLanguages(["en", "ml"]).write(to: file)
    #expect(!ActiveLanguages.shown(storedIn: file, packIDs: packIDs).isOn("hi"))
    // The keyboard turns Hindi on.
    try ActiveLanguages(["en", "ml", "hi"]).write(to: file)
    #expect(ActiveLanguages.shown(storedIn: file, packIDs: packIDs).isOn("hi"))
    // And off again.
    try ActiveLanguages(["en", "ml"]).write(to: file)
    #expect(!ActiveLanguages.shown(storedIn: file, packIDs: packIDs).isOn("hi"))
    // A language the app no longer has is not shown on.
    try ActiveLanguages(["en", "ta"]).write(to: file)
    #expect(ActiveLanguages.shown(storedIn: file, packIDs: packIDs) == ActiveLanguages(["en"]))
}

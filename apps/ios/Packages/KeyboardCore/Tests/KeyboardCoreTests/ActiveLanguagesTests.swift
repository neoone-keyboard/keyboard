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
    #expect(ActiveLanguages(["en", "ml", "hi"]).usable(packIDs: ["ml", "hi"]) == ["en", "ml"])
    // A repeat once.
    #expect(ActiveLanguages(["ml", "ml"]).usable(packIDs: ["ml"]) == ["ml"])
}

@Test func theLimitIsTheEngines() {
    // engine/core/src/lang.rs MAX_ACTIVE, English counted.
    #expect(ActiveLanguages.maxActive == 2)
}

@Test func theDefaultIsEnglishAndTheFirstPack() {
    #expect(ActiveLanguages.engineDefault(packIDs: ["ml", "hi"]).languages == ["en", "ml"])
    #expect(ActiveLanguages.engineDefault(packIDs: []).languages == ["en"])
}

@Test func togglingKeepsListOrderAtMostTheLimitAndAtLeastOne() {
    let order = ["en", "ml", "hi"]
    var choice = ActiveLanguages(["en"])
    choice = choice.toggling("ml", order: order)
    #expect(choice.languages == ["en", "ml"])
    // Full: a third is not added.
    #expect(!choice.canTurnOn("hi"))
    #expect(choice.toggling("hi", order: order).languages == ["en", "ml"])
    // English off, then Hindi on: list order, not tap order.
    choice = choice.toggling("en", order: order).toggling("hi", order: order)
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

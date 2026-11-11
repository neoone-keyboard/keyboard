import Foundation
import Testing
import KeyboardCore

/// What the keyboard extension reads when it opens (`EngineSuggestionProvider.readPackState`)
/// and hands on: the packs, the active languages, the language key's list and the packs whose
/// scripts autocorrect leaves alone.

/// A folder of packs like the extension bundle's `packs/` or the App Group's `Packs/`, with
/// the parts of the repository's packs that Swift reads (no lexicons or models).
private struct PacksFixture {
    let root: URL
    var bundled: URL { root.appending(path: "bundle/packs", directoryHint: .isDirectory) }
    var installed: URL { root.appending(path: "group/Packs", directoryHint: .isDirectory) }
    var appChoice: URL { root.appending(path: "group/active-languages.json") }

    init() throws {
        root = try temporaryFolder()
    }

    /// Copies packs/<id>.neopack's manifest, layout, script rules and emoji words into
    /// `folder`, with `version` in place of its own, and without the files of `dropping`
    /// (manifest roles: "layout", "lang").
    @discardableResult
    func add(_ id: String, to folder: URL, version: String? = nil, dropping: [String] = []) throws -> URL {
        let source = packsFolder.appending(path: "\(id).neopack")
        let target = folder.appending(path: "\(id).neopack", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: target, withIntermediateDirectories: true)
        var manifest = try #require(JSONSerialization.jsonObject(with: Data(contentsOf: source.appending(path: "manifest.json"))) as? [String: Any])
        var files = try #require(manifest["files"] as? [String: String])
        for role in ["layout", "lang", "emojiWords", "emojiSearch"] {
            guard let name = files[role] else { continue }
            if dropping.contains(role) {
                files[role] = nil
            } else {
                try FileManager.default.copyItem(at: source.appending(path: name), to: target.appending(path: name))
            }
        }
        manifest["files"] = files
        if let version { manifest["version"] = version }
        try JSONSerialization.data(withJSONObject: manifest).write(to: target.appending(path: "manifest.json"))
        return target
    }

    func read(keyboardChoice: DatedLanguages? = nil, debugList: String = "") -> KeyboardPacks {
        KeyboardPacks.read(bundledPacks: bundled, installedPacks: installed, appChoice: appChoice,
                           keyboardChoice: keyboardChoice, debugList: debugList)
    }

    /// Writes the app's choice, dated `date`.
    func chooseInApp(_ ids: [String], at date: Date) throws {
        try ActiveLanguages(ids).write(to: appChoice)
        try FileManager.default.setAttributes([.modificationDate: date], ofItemAtPath: appChoice.path)
    }

    func cleanUp() {
        try? FileManager.default.removeItem(at: root)
    }
}

private let earlier = Date(timeIntervalSince1970: 1_800_000_000)
private let later = earlier.addingTimeInterval(60)

@Test func withoutAChoiceTheKeyboardHasEnglishAndItsFirstPack() throws {
    let fixture = try PacksFixture()
    defer { fixture.cleanUp() }
    try fixture.add("ml", to: fixture.bundled)
    try fixture.add("hi", to: fixture.installed)
    try fixture.add("ta", to: fixture.installed)

    let state = fixture.read()
    #expect(state.packs.map(\.id) == ["ml", "hi", "ta"], "the bundled pack first, then the installed ones by id")
    #expect(state.active == nil, "no choice: the engine's default")
    #expect(state.activeChoice == ActiveLanguages(["en", "ml"]))
    #expect(state.activePacks.map(\.id) == ["ml"])
    #expect(state.defaultLanguages.map(\.id) == ["en", "ml"])
    #expect(state.engineLanguages == ["en", "ml"])
    #expect(state.availableLanguages.map(\.id) == ["en", "ml", "hi", "ta"])
    #expect(state.availableLanguages.map(\.name) == ["English", "മലയാളം", "हिन्दी", "தமிழ்"])
}

@Test func aKeyboardWithoutPacksIsEnglishOnly() throws {
    let fixture = try PacksFixture()
    defer { fixture.cleanUp() }
    // Neither folder exists: a release build before any download.
    let state = fixture.read()
    #expect(state.packs.isEmpty)
    #expect(state.activeChoice == ActiveLanguages(["en"]))
    #expect(state.activePacks.isEmpty)
    #expect(state.defaultLanguages == [.english])
    #expect(state.availableLanguages.map(\.id) == ["en"])
    // No bundle folder and no App Group at all (an unsigned build).
    let none = KeyboardPacks.read(bundledPacks: nil, installedPacks: nil, appChoice: nil, keyboardChoice: nil, debugList: "")
    #expect(none == state)
}

@Test func theNewerOfTheAppsAndTheKeyboardsChoiceIsActive() throws {
    let fixture = try PacksFixture()
    defer { fixture.cleanUp() }
    try fixture.add("ml", to: fixture.bundled)
    try fixture.add("hi", to: fixture.installed)
    try fixture.add("ta", to: fixture.installed)
    try fixture.chooseInApp(["en", "hi"], at: earlier)

    // The app's choice alone.
    var state = fixture.read()
    #expect(state.active == ["en", "hi"])
    #expect(state.activePacks.map(\.id) == ["hi"])
    #expect(state.defaultLanguages.map(\.id) == ["en", "hi"])

    // Then one made in the keyboard's settings panel.
    let inKeyboard = DatedLanguages(ActiveLanguages(["ta", "ml"]), date: later)
    state = fixture.read(keyboardChoice: inKeyboard)
    #expect(state.active == ["ta", "ml"])
    #expect(state.activePacks.map(\.id) == ["ml", "ta"], "in the packs' order")
    #expect(state.defaultLanguages.map(\.id) == ["en", "ta", "ml"], "the language key: English, then the choice's order")
    #expect(!state.activeChoice.isOn("en"), "English switched off stays off in the panel")

    // Then the app's again.
    try fixture.chooseInApp(["en", "ml", "hi", "ta"], at: later.addingTimeInterval(60))
    state = fixture.read(keyboardChoice: inKeyboard)
    #expect(state.active == ["en", "ml", "hi", "ta"])
    #expect(state.activePacks.map(\.id) == ["ml", "hi", "ta"])
}

@Test func aChosenLanguageWithoutItsPackIsLeftOut() throws {
    let fixture = try PacksFixture()
    defer { fixture.cleanUp() }
    try fixture.add("ml", to: fixture.bundled)
    try fixture.add("hi", to: fixture.installed)
    try fixture.chooseInApp(["en", "as", "hi"], at: earlier)
    #expect(fixture.read().active == ["en", "hi"], "Assamese was removed since")

    // A choice of nothing the keyboard has is no choice.
    try fixture.chooseInApp(["as", "ta"], at: earlier)
    let state = fixture.read()
    #expect(state.active == nil)
    #expect(state.activePacks.map(\.id) == ["ml"])
}

@Test func aDebugBuildsListIsUsedOnlyWithoutAStoredChoice() throws {
    let fixture = try PacksFixture()
    defer { fixture.cleanUp() }
    try fixture.add("ml", to: fixture.bundled)
    try fixture.add("hi", to: fixture.bundled)
    #expect(fixture.read(debugList: "ml, hi").active == ["ml", "hi"])
    #expect(fixture.read(debugList: " ").active == nil)
    #expect(fixture.read(debugList: "").active == nil)
    try fixture.chooseInApp(["en", "hi"], at: earlier)
    #expect(fixture.read(debugList: "ml,hi").active == ["en", "hi"])
}

@Test func anInstalledPackReplacesTheBundledOneOnlyWhenNewer() throws {
    let fixture = try PacksFixture()
    defer { fixture.cleanUp() }
    try fixture.add("ml", to: fixture.bundled, version: "2026.11.05")
    let installed = try fixture.add("ml", to: fixture.installed, version: "2026.11.09")
    var state = fixture.read()
    #expect(state.packs.map(\.version) == ["2026.11.09"])
    #expect(state.packs[0].folder.standardizedFileURL == installed.standardizedFileURL)
    #expect(state.activePacks.map(\.version) == ["2026.11.09"], "the active pack is the one the engine opens")

    try FileManager.default.removeItem(at: installed)
    try fixture.add("ml", to: fixture.installed, version: "2026.11.01")
    state = fixture.read()
    #expect(state.packs.map(\.version) == ["2026.11.05"])
}

/// A pack without a keyboard of its own is typed on the Latin keys only: it is active, but
/// not one of the language key's languages.
@Test func aPackWithoutALayoutIsActiveButNotOnTheLanguageKey() throws {
    let fixture = try PacksFixture()
    defer { fixture.cleanUp() }
    try fixture.add("ml", to: fixture.bundled)
    try fixture.add("hi", to: fixture.installed, dropping: ["layout"])
    try fixture.chooseInApp(["en", "ml", "hi"], at: earlier)

    let state = fixture.read()
    #expect(state.activePacks.map(\.id) == ["ml", "hi"])
    #expect(state.defaultLanguages.map(\.id) == ["en", "ml"])
    #expect(state.language(forPack: "hi") == nil)
    #expect(state.language(forPack: "ml")?.id == "ml")
    #expect(state.language(forPack: "ta") == nil, "no such pack")

    // And without a choice, a first pack that can't be drawn leaves the key with English.
    let only = try PacksFixture()
    defer { only.cleanUp() }
    try only.add("hi", to: only.installed, dropping: ["layout"])
    #expect(only.read().defaultLanguages == [.english])
    #expect(only.read().activePacks.map(\.id) == ["hi"])
}

/// The view controller gives `InputController` the state's languages and active packs
/// (`applyLanguages`): autocorrect on the Latin keys then leaves a word in any active pack's
/// script alone, with or without a keyboard for it, and corrects the others.
@MainActor
@Test func autocorrectLeavesTheActivePacksScriptsAlone() throws {
    let fixture = try PacksFixture()
    defer { fixture.cleanUp() }
    try fixture.add("ml", to: fixture.bundled)
    try fixture.add("hi", to: fixture.installed, dropping: ["layout"])
    try fixture.add("ta", to: fixture.installed)
    try fixture.add("as", to: fixture.installed, dropping: ["lang"])
    try fixture.chooseInApp(["en", "hi", "as"], at: earlier)
    let state = fixture.read()
    #expect(state.activePacks.map(\.id) == ["as", "hi"])

    /// Space after `word` with a replacement in the middle slot: what the field then holds.
    func spaced(_ word: String, state: KeyboardPacks) -> String {
        let output = RecordingOutput()
        let input = InputController(output: output, languages: state.defaultLanguages)
        input.activePacks = state.activePacks
        output.text = word
        input.updateSuggestions(SuggestionResult(typed: word, candidates: [Suggestion("x", kind: .manglish), Suggestion(word, kind: .verbatim)]))
        input.handle(.space, at: 0)
        return output.text
    }

    #expect(spaced("नमस्त", state: state) == "नमस्त ", "Hindi is on, though it has no keyboard here")
    #expect(spaced("নমস্ক", state: state) == "নমস্ক ", "Assamese is on: its script's block, the pack having no lang.json")
    #expect(spaced("വണക്ക", state: state) == "x ", "Malayalam is off")
    #expect(spaced("வணக்க", state: state) == "x ", "Tamil is off")
    #expect(spaced("namast", state: state) == "x ")

    // The user turns Tamil on and Hindi off in the app.
    try fixture.chooseInApp(["en", "ta", "as"], at: later)
    let next = fixture.read()
    #expect(spaced("வணக்க", state: next) == "வணக்க ")
    #expect(spaced("नमस्त", state: next) == "x ")
}

@Test func aChoiceMadeInTheKeyboardIsGivenToTheEngineAsItCanLoadIt() throws {
    let fixture = try PacksFixture()
    defer { fixture.cleanUp() }
    try fixture.add("ml", to: fixture.bundled)
    try fixture.add("hi", to: fixture.installed)
    let state = fixture.read()
    #expect(state.engineLanguages(for: ActiveLanguages(["hi", "en"])) == ["hi", "en"])
    #expect(state.engineLanguages(for: ActiveLanguages(["en", "ta", "hi", "hi"])) == ["en", "hi"])
    #expect(state.engineLanguages(for: ActiveLanguages(["ta"])) == ["en", "ml"], "nothing usable: the engine's default")
    #expect(KeyboardPacks(packs: state.packs, active: ["hi"]).engineLanguages == ["hi"])
}

@Test func theEngineReopensWhenAPackComesOrGoesAndOnlyTakesANewChoiceOtherwise() throws {
    let fixture = try PacksFixture()
    defer { fixture.cleanUp() }
    try fixture.add("ml", to: fixture.bundled)
    let hindi = try fixture.add("hi", to: fixture.installed)
    let before = fixture.read()
    #expect(fixture.read().change(from: before) == .none)

    try fixture.chooseInApp(["en", "hi"], at: earlier)
    let chosen = fixture.read()
    #expect(chosen.change(from: before) == .choice)

    try fixture.add("ta", to: fixture.installed)
    let added = fixture.read()
    #expect(added.change(from: chosen) == .packs)

    try FileManager.default.removeItem(at: hindi)
    let removed = fixture.read()
    #expect(removed.change(from: added) == .packs, "and the choice lost Hindi with it")
    #expect(removed.active == ["en"])
}

/// A pack replaced by a newer version (an update from the app's Languages screen) is a
/// change of packs though its id is the same: the engine has the old version's files open,
/// and reopens (`EngineSuggestionProvider.reloadIfChanged`).
@Test func aPackReplacedByANewerVersionReopensTheEngine() throws {
    let fixture = try PacksFixture()
    defer { fixture.cleanUp() }
    try fixture.add("ml", to: fixture.bundled)
    let hindi = try fixture.add("hi", to: fixture.installed, version: "2026.11.05")
    let before = fixture.read()

    // As the installer does it: the new version takes the old one's folder.
    try FileManager.default.removeItem(at: hindi)
    try fixture.add("hi", to: fixture.installed, version: "2026.11.09")
    let after = fixture.read()
    #expect(after.packs.map(\.id) == before.packs.map(\.id))
    #expect(after.packs[1].folder == before.packs[1].folder)
    #expect(after.packs[1].version == "2026.11.09")
    #expect(after.change(from: before) == .packs)
    #expect(after.packs == before.packs, "still the same languages: packs are equal by id")
}

/// An installed copy newer than the bundled one takes over from it: another folder, so the
/// engine reopens. And back, when the installed copy is removed.
@Test func anInstalledCopyTakingOverFromTheBundledOneReopensTheEngine() throws {
    let fixture = try PacksFixture()
    defer { fixture.cleanUp() }
    try fixture.add("ml", to: fixture.bundled, version: "2026.11.05")
    let before = fixture.read()
    let installed = try fixture.add("ml", to: fixture.installed, version: "2026.11.09")
    let after = fixture.read()
    #expect(after.packs[0].version == "2026.11.09")
    #expect(after.packs[0].folder != before.packs[0].folder)
    #expect(after.change(from: before) == .packs)

    try FileManager.default.removeItem(at: installed)
    let back = fixture.read()
    #expect(back.packs[0].version == "2026.11.05")
    #expect(back.change(from: after) == .packs)
    #expect(back.change(from: before) == .none)
}

/// The folder alone decides too: the same version in the bundle and installed are two sets
/// of files.
@Test func theSameVersionInTheOtherFolderIsAChangeOfPacks() throws {
    let fixture = try PacksFixture()
    defer { fixture.cleanUp() }
    let bundled = try fixture.add("ml", to: fixture.bundled, version: "2026.11.05")
    let installed = try fixture.add("ml", to: fixture.installed, version: "2026.11.05")
    let one = KeyboardPacks(packs: [try PackDescriptor.load(folder: bundled)], active: nil)
    let other = KeyboardPacks(packs: [try PackDescriptor.load(folder: installed)], active: nil)
    #expect(other.change(from: one) == .packs)
    #expect(one.change(from: one) == .none)
}

/// Reading the same packs again, updated or not, is no change: the open engine stays. An
/// installed copy that doesn't take over (not newer than the bundled one) changes nothing
/// either.
@Test func packsReadAgainUnchangedDoNotReopenTheEngine() throws {
    let fixture = try PacksFixture()
    defer { fixture.cleanUp() }
    try fixture.add("ml", to: fixture.bundled, version: "2026.11.05")
    try fixture.add("hi", to: fixture.installed, version: "2026.11.09")
    try fixture.chooseInApp(["en", "hi"], at: earlier)
    let before = fixture.read()
    #expect(fixture.read().change(from: before) == .none)
    #expect(before.change(from: before) == .none)

    try fixture.add("ml", to: fixture.installed, version: "2026.11.05")
    #expect(fixture.read().change(from: before) == .none, "the bundled copy is still the one in use")
}

/// Turning a language on or off is only a choice, set on the open engine, as before; with a
/// pack updated at the same time the engine reopens, and takes the choice as it opens.
@Test func turningALanguageOnOrOffOnlySetsTheChoice() throws {
    let fixture = try PacksFixture()
    defer { fixture.cleanUp() }
    try fixture.add("ml", to: fixture.bundled)
    let hindi = try fixture.add("hi", to: fixture.installed, version: "2026.11.05")
    let before = fixture.read()

    try fixture.chooseInApp(["en", "ml", "hi"], at: earlier)
    let on = fixture.read()
    #expect(on.change(from: before) == .choice)
    #expect(on.engineLanguages == ["en", "ml", "hi"])

    try fixture.chooseInApp(["en", "ml"], at: later)
    let off = fixture.read()
    #expect(off.change(from: on) == .choice)
    #expect(off.engineLanguages == ["en", "ml"])

    // Made in the keyboard's settings panel instead.
    let inKeyboard = fixture.read(keyboardChoice: DatedLanguages(ActiveLanguages(["en", "hi"]), date: later.addingTimeInterval(60)))
    #expect(inKeyboard.change(from: off) == .choice)
    #expect(inKeyboard.change(from: inKeyboard) == .none)

    try FileManager.default.removeItem(at: hindi)
    try fixture.add("hi", to: fixture.installed, version: "2026.11.09")
    try fixture.chooseInApp(["en", "hi"], at: later.addingTimeInterval(120))
    #expect(fixture.read().change(from: off) == .packs)
}

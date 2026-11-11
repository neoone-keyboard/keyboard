import XCTest
import KeyboardCore
@testable import KeyboardApp

/// Languages → Import from Files, from the file the picker hands over to the list the screen
/// shows: `LanguagesModel.importPack(from:)` with real `.neopack` files, the app's own
/// installer (the App Group's Packs/, the engine's check of the unpacked pack) and the
/// sentence the user reads when a file is refused.
///
/// The pack is the engine's small Gujarati test pack (engine/core/tests/fixtures/gu.neopack),
/// zipped here under its own id, so no language the simulator has is touched. The stored
/// active languages are put back afterwards. Runs in the app on a simulator: `make test-app`.
@MainActor
final class LanguagesImportTests: XCTestCase {
    /// The test pack's id: no real pack has it.
    private let id = "zzimport"
    private var folder: URL!
    private var storedChoice: Data?
    private var model: LanguagesModel!

    override func setUp() async throws {
        guard let choice = PackLocations.activeLanguagesFile(), InstalledPacks.installer != nil else {
            throw XCTSkip("no App Group in this build (unsigned): packs can't be installed")
        }
        folder = FileManager.default.temporaryDirectory.appending(path: "import-tests-\(UUID().uuidString)", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        // English alone, so there is room to turn the new language on.
        storedChoice = try? Data(contentsOf: choice)
        try? InstalledPacks.remove(id: id)
        try InstalledPacks.store(ActiveLanguages([ActiveLanguages.english]))
        model = LanguagesModel()
        XCTAssertNil(row, "the test pack isn't installed yet")
    }

    override func tearDown() async throws {
        guard let choice = PackLocations.activeLanguagesFile() else { return }
        try? InstalledPacks.remove(id: id)
        if let storedChoice {
            try storedChoice.write(to: choice, options: .atomic)
        } else {
            try? FileManager.default.removeItem(at: choice)
        }
        try? FileManager.default.removeItem(at: folder)
    }

    func testAValidPackIsInstalledListedAndTurnedOn() async throws {
        let file = try packFile()
        let before = model.rows.map(\.id)
        model.importPack(from: file)
        XCTAssertTrue(model.importing, "the Import row shows progress and is disabled meanwhile")
        try await importFinished()

        XCTAssertNil(model.message)
        XCTAssertEqual(model.rows.map(\.id), before + [id], "listed after the languages already there")
        let row = try XCTUnwrap(self.row)
        XCTAssertEqual(row.name, "Gujarati (test)")
        XCTAssertEqual(row.nativeName, "ગુજરાતી")
        XCTAssertEqual(row.version, "2026.11.10")
        XCTAssertTrue(row.removable)
        XCTAssertFalse(row.builtIn)
        XCTAssertEqual(row.languageKey, "ગુ")
        XCTAssertGreaterThan(row.size, 5_000)

        // Turned on, and stored for the keyboard.
        XCTAssertTrue(model.isOn(id))
        XCTAssertEqual(InstalledPacks.storedChoice(), ActiveLanguages([ActiveLanguages.english, id]))

        // Where the keyboard reads it, whole, and the picked file is left where it was.
        let installed = try XCTUnwrap(PackLocations.installedPacks()).appending(path: "\(id).neopack")
        for name in ["manifest.json", "gu.fst", "romanised.fst", "next.fst", "romanisation.json", "layout.json", "lang.json"] {
            XCTAssertTrue(FileManager.default.fileExists(atPath: installed.appending(path: name).path), name)
        }
        XCTAssertTrue(InstalledPacks.all().contains { $0.id == id })
        XCTAssertTrue(FileManager.default.fileExists(atPath: file.path))

        // Removing it takes it off the list and out of the stored choice.
        model.remove(id)
        XCTAssertNil(model.message)
        XCTAssertNil(self.row)
        XCTAssertEqual(InstalledPacks.storedChoice(), ActiveLanguages([ActiveLanguages.english]))
        XCTAssertFalse(FileManager.default.fileExists(atPath: installed.path))
    }

    func testACorruptZipIsRefused() async throws {
        let whole = try Data(contentsOf: packFile())
        let file = folder.appending(path: "cut-short.neopack")
        try whole.prefix(whole.count / 2).write(to: file)
        try await importRefused(file, saying: "This file is damaged or isn't a language pack. Download it again.")

        // A pack whose bytes changed on the way (the archive's own checksum catches it).
        var damaged = whole
        let lexicon = try XCTUnwrap(damaged.range(of: TestPackFile.file("gu.fst", in: Bundle(for: Self.self))))
        damaged[lexicon.lowerBound + 100] ^= 0xFF
        try damaged.write(to: file)
        try await importRefused(file, saying: "This file is damaged or isn't a language pack. Download it again.")

        // A file that is no zip at all.
        try Data("not a zip".utf8).write(to: file)
        try await importRefused(file, saying: "This file is damaged or isn't a language pack. Download it again.")
    }

    func testAPackNeedingANewerEngineIsRefused() async throws {
        let file = try packFile { $0["minEngine"] = 99 }
        try await importRefused(file, saying: "This pack needs a newer NeoOne Keyboard. Update the app, then try again.")
    }

    func testAZipThatIsNotAPackIsRefused() async throws {
        let file = folder.appending(path: "holiday-photos.zip")
        try TestPackFile.zip([("photos/readme.txt", Data("hello".utf8)), ("photos/list.json", Data("[]".utf8))]).write(to: file)
        try await importRefused(file, saying: "This file isn't a NeoOne Keyboard language pack.")
    }

    func testAPackTheEngineCannotOpenIsRefused() async throws {
        // The manifest's checksums hold, but the script's rules are none: only the engine,
        // which the app asks before a pack goes where the keyboard looks, can tell.
        let file = try packFile(replacing: ["lang.json": Data("{}".utf8)])
        try await importRefused(file, saying: "This pack wasn't made correctly, so it can't be added.")

        // The same for a lexicon that is there, with its checksum, but isn't one: the engine
        // opens the pack's files, it doesn't only look for them.
        let garbage = try packFile(replacing: ["gu.fst": Data("not an fst".utf8)])
        try await importRefused(garbage, saying: "This pack wasn't made correctly, so it can't be added.")
    }

    func testANewerVersionReplacesTheOlderAndAnOlderOneIsRefused() async throws {
        model.importPack(from: try packFile())
        try await importFinished()
        XCTAssertEqual(row?.version, "2026.11.10")
        // The user turns it off; an update leaves the choice as it was.
        model.toggle(id)
        XCTAssertFalse(model.isOn(id))

        let newer = try packFile { manifest in
            manifest["version"] = "2026.12.01"
            manifest["name"] = "Gujarati (newer)"
        }
        model.importPack(from: newer)
        try await importFinished()
        XCTAssertNil(model.message)
        XCTAssertEqual(model.rows.filter { $0.id == id }.count, 1)
        XCTAssertEqual(row?.version, "2026.12.01")
        XCTAssertEqual(row?.name, "Gujarati (newer)")
        XCTAssertFalse(model.isOn(id), "an update doesn't turn the language back on")
        XCTAssertEqual(InstalledPacks.installed().first { $0.id == id }?.version, "2026.12.01")

        // The first file again: older than what is there now.
        model.importPack(from: try packFile())
        try await importFinished()
        XCTAssertEqual(model.message?.title, "Couldn't add the language")
        XCTAssertEqual(model.message?.text, "You already have a newer version of this language (2026.12.01).")
        XCTAssertEqual(row?.version, "2026.12.01")
    }

    // MARK: - Helpers

    private var row: LanguagesModel.Row? { model.rows.first { $0.id == id } }

    /// Waits for the import started by `importPack(from:)`.
    private func importFinished() async throws {
        let deadline = ContinuousClock.now + .seconds(120)
        while model.importing {
            guard ContinuousClock.now < deadline else {
                XCTFail("the import didn't finish")
                return
            }
            try await Task.sleep(for: .milliseconds(20))
        }
    }

    /// Imports `file` and expects the alert "Couldn't add the language" with `text`, and the
    /// languages as they were.
    private func importRefused(_ file: URL, saying text: String, line: UInt = #line) async throws {
        let rows = model.rows
        let stored = InstalledPacks.storedChoice()
        model.message = nil
        model.importPack(from: file)
        try await importFinished()
        XCTAssertEqual(model.message?.title, "Couldn't add the language", line: line)
        XCTAssertEqual(model.message?.text, text, line: line)
        XCTAssertEqual(model.rows, rows, line: line)
        XCTAssertEqual(InstalledPacks.storedChoice(), stored, line: line)
        let packs = try XCTUnwrap(PackLocations.installedPacks())
        let left = (try? FileManager.default.contentsOfDirectory(atPath: packs.path)) ?? []
        XCTAssertFalse(left.contains { $0.hasPrefix(".incoming") || $0 == "\(id).neopack" }, "nothing half installed: \(left)", line: line)
    }

    /// The Gujarati test pack as a `.neopack` file under the test id (`TestPackFile`).
    private func packFile(replacing: [String: Data] = [:], edit: (inout [String: Any]) -> Void = { _ in }) throws -> URL {
        let file = folder.appending(path: "\(id)-\(UUID().uuidString.prefix(8)).neopack")
        try TestPackFile.data(id: id, in: Bundle(for: Self.self), replacing: replacing, edit: edit).write(to: file)
        return file
    }
}

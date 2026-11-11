import XCTest

/// Languages → Import from Files with the real Files picker: a `.neopack` saved under On My
/// iPhone is picked, and the language appears in the list, switched on. The pack is the
/// engine's small Gujarati test pack under its own id (`TestPackFile`), removed again at the
/// end. Simulator only: the test writes the file into the simulator's Files storage from the
/// Mac's side, which a device doesn't allow.
@MainActor
final class ImportFromFilesUITests: XCTestCase {
    private let id = "zzfiles"
    private let fileName = "zzfiles-test-pack"
    private var app: XCUIApplication!

    override func setUp() async throws {
        continueAfterFailure = false
        app = XCUIApplication()
        // Debug builds open the Languages screen at launch with -showLanguages.
        app.launchArguments += ["-chooseLanguagesDone", "YES", "-showLanguages"]
        XCUIDevice.shared.orientation = .portrait
    }

    func testAPackPickedInFilesIsAddedAndSwitchedOn() throws {
        let storage = try filesStorage()
        let file = storage.appending(path: "\(fileName).neopack")
        try TestPackFile.data(id: id, in: Bundle(for: Self.self)).write(to: file)
        addTeardownBlock { try? FileManager.default.removeItem(at: file) }

        app.launch()
        XCTAssertTrue(app.navigationBars["Languages"].waitForExistence(timeout: 30))
        removeTheTestLanguage()
        XCTAssertFalse(languageRow.exists)

        let importButton = app.buttons["Import from Files"]
        reveal(importButton)
        importButton.tap()
        try pickTheFile()

        XCTAssertTrue(languageRow.waitForExistence(timeout: 30), "the picked pack is listed")
        XCTAssertFalse(app.alerts.firstMatch.exists, "no refusal")
        XCTAssertEqual(languageRow.value as? String, "1", "and switched on")
        shot("import-listed")

        removeTheTestLanguage()
        XCTAssertFalse(languageRow.exists)
    }

    // MARK: - The picker

    /// Finds the file in the Files picker and taps it. The picker opens where it was last
    /// (Recents, or a folder), so this goes to Browse and On My iPhone when the file isn't
    /// already showing.
    private func pickTheFile() throws {
        let file = app.descendants(matching: .any).matching(NSPredicate(format: "label BEGINSWITH %@", fileName)).firstMatch
        if !file.waitForExistence(timeout: 10) {
            let browse = app.buttons["Browse"]
            if browse.waitForExistence(timeout: 5) { browse.tap() }
            if !file.waitForExistence(timeout: 5) {
                // On the list of locations: a second tap on Browse goes there from a folder.
                let onMyiPhone = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "On My iPhone")).firstMatch
                if !onMyiPhone.waitForExistence(timeout: 3), browse.exists { browse.tap() }
                if onMyiPhone.waitForExistence(timeout: 5) { onMyiPhone.tap() }
            }
        }
        guard file.waitForExistence(timeout: 10) else {
            shot("import-picker-no-file")
            dump("import-picker")
            XCTFail("the Files picker doesn't show \(fileName)")
            return
        }
        shot("import-picker")
        file.tap()
    }

    /// "On My iPhone" as the simulator keeps it on the Mac: the File Provider Storage folder
    /// of the Files app's local-storage App Group.
    private func filesStorage() throws -> URL {
        #if targetEnvironment(simulator)
        let home = NSHomeDirectory()
        guard let range = home.range(of: "/data/Containers/") else {
            throw XCTSkip("not a simulator's file system: \(home)")
        }
        let groups = URL(fileURLWithPath: String(home[..<range.upperBound])).appending(path: "Shared/AppGroup", directoryHint: .isDirectory)
        let fm = FileManager.default
        for group in (try? fm.contentsOfDirectory(at: groups, includingPropertiesForKeys: nil)) ?? [] {
            let metadata = group.appending(path: ".com.apple.mobile_container_manager.metadata.plist")
            guard let plist = NSDictionary(contentsOf: metadata),
                  plist["MCMMetadataIdentifier"] as? String == "group.com.apple.FileProvider.LocalStorage" else { continue }
            let storage = group.appending(path: "File Provider Storage", directoryHint: .isDirectory)
            try fm.createDirectory(at: storage, withIntermediateDirectories: true)
            return storage
        }
        throw XCTSkip("the simulator has no Files storage yet (open the Files app once)")
        #else
        throw XCTSkip("simulator only: the file is put in Files from the Mac's side")
        #endif
    }

    // MARK: - The list

    /// The test language's row: a switch named for it (ગુજરાતી).
    private var languageRow: XCUIElement {
        app.switches.matching(NSPredicate(format: "label CONTAINS %@", "Gujarati (test)")).firstMatch
    }

    /// Removes the test language if it is listed: swipe left on its row, Remove, and confirm.
    private func removeTheTestLanguage() {
        guard languageRow.waitForExistence(timeout: 3) else { return }
        reveal(languageRow)
        languageRow.swipeLeft()
        let remove = app.buttons["Remove"]
        XCTAssertTrue(remove.waitForExistence(timeout: 5))
        remove.tap()
        // The confirmation's Remove.
        let confirm = app.buttons.matching(NSPredicate(format: "label == %@", "Remove")).allElementsBoundByIndex.last
        if let confirm, confirm.waitForExistence(timeout: 5) { confirm.tap() }
        let gone = NSPredicate(format: "exists == false")
        wait(for: [expectation(for: gone, evaluatedWith: languageRow)], timeout: 10)
    }

    private func reveal(_ element: XCUIElement) {
        for _ in 0..<5 where !element.isHittable { app.swipeUp() }
    }

    private func shot(_ name: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
        guard let dir = ProcessInfo.processInfo.environment["SHOTS_DIR"], !dir.isEmpty else { return }
        try? XCUIScreen.main.screenshot().pngRepresentation
            .write(to: URL(fileURLWithPath: dir).appending(path: "\(name).png"))
    }

    /// The screen's elements, for working out what a picker shows when the file isn't found.
    private func dump(_ name: String) {
        let attachment = XCTAttachment(string: app.debugDescription)
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
        guard let dir = ProcessInfo.processInfo.environment["SHOTS_DIR"], !dir.isEmpty else { return }
        try? app.debugDescription.write(to: URL(fileURLWithPath: dir).appending(path: "\(name).txt"), atomically: true, encoding: .utf8)
    }
}

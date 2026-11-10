import XCTest

/// Footage for the how-to videos, for recordings only: skips without VIDEO_STEPS=1
/// (`TEST_RUNNER_VIDEO_STEPS=1 xcodebuild … test -only-testing:KeyboardUITests/VideoFootage/<test>`),
/// with `xcrun simctl io <sim> recordVideo` running around the test. Same simulator setup as
/// AppStoreScreenshots. testA removes NeoOne Keyboard from Settings (not recorded); testB adds it
/// back the way a user would; testC switches to it with the globe key and types "mera".
@MainActor
final class VideoFootage: XCTestCase {
    private let settings = XCUIApplication(bundleIdentifier: "com.apple.Preferences")
    private let app = XCUIApplication()

    override func setUp() async throws {
        continueAfterFailure = false
        guard ProcessInfo.processInfo.environment["VIDEO_STEPS"] == "1" else {
            throw XCTSkip("footage only: set VIDEO_STEPS=1")
        }
        XCUIDevice.shared.appearance = .light
    }

    /// Not recorded: removes NeoOne Keyboard so testB can add it.
    func testARemoveKeyboard() throws {
        openKeyboards()
        // iOS keeps at least one keyboard: add Apple's English one first (the globe-key clip
        // switches from it).
        if !settings.cells.containing(.staticText, identifier: "English (US)").firstMatch.exists {
            tapRow("Add New Keyboard")
            tapRow("English (US)")
            sleep(1)
        }
        let row = settings.cells.containing(.staticText, identifier: "NeoOne Keyboard").firstMatch
        guard row.waitForExistence(timeout: 3) else { return }
        settings.buttons["Edit"].tap()
        sleep(1)
        // The red minus at the row's leading edge.
        row.coordinate(withNormalizedOffset: CGVector(dx: 0.08, dy: 0.5)).tap()
        let confirm = settings.buttons["Delete"].firstMatch
        XCTAssertTrue(confirm.waitForExistence(timeout: 3))
        confirm.tap()
        settings.buttons["Done"].firstMatch.tap()
        settings.terminate()
    }

    /// Settings → General → Keyboard → Keyboards → Add New Keyboard… → NeoOne Keyboard.
    func testBAddKeyboard() throws {
        settings.terminate()
        XCUIDevice.shared.press(.home)
        sleep(2)
        settings.launch()
        sleep(2)
        tapRow("General")
        tapRow("Keyboard")
        tapRow("Keyboards")
        tapRow("Add New Keyboard")
        let neo = settings.staticTexts["NeoOne Keyboard"].firstMatch
        XCTAssertTrue(neo.waitForExistence(timeout: 5))
        sleep(1)
        neo.tap()
        sleep(3)
    }

    /// In the app's Try it field: hold the globe key, pick NeoOne Keyboard, type "mera".
    func testCSwitchAndType() throws {
        app.launchArguments += ["-chooseLanguagesDone", "YES"]
        app.terminate()
        XCUIDevice.shared.press(.home)
        sleep(1)
        app.launch()
        let field = app.textViews["try-it"].exists ? app.textViews["try-it"] : app.textFields["try-it"]
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.tap()
        sleep(2)
        let globe = app.buttons["Next keyboard"].firstMatch
        XCTAssertTrue(globe.waitForExistence(timeout: 5), "no globe key")
        globe.press(forDuration: 1.5)
        let pick = app.staticTexts["NeoOne Keyboard"].firstMatch
        if pick.waitForExistence(timeout: 3) { pick.tap() } else { globe.tap() }
        sleep(2)
        for word in ["mera"] {
            for ch in word {
                let key = app.keys.matching(NSPredicate(format: "label ==[c] %@", String(ch))).firstMatch
                XCTAssertTrue(key.waitForExistence(timeout: 3), "key \(ch)")
                key.tap()
                usleep(450_000)
            }
        }
        sleep(2)
        let hindi = app.staticTexts["मेरा"].firstMatch
        if hindi.waitForExistence(timeout: 3) { hindi.tap() }
        sleep(3)
    }

    /// Adding a language: the app's Languages screen, Get beside Assamese (downloads from the
    /// site), then back home where it shows as On.
    func testDAddLanguage() throws {
        launchApp()
        app.buttons["languages"].tap()
        XCTAssertTrue(app.navigationBars["Languages"].waitForExistence(timeout: 5))
        sleep(3) // the pack index downloads
        let get = app.buttons.matching(NSPredicate(format: "label BEGINSWITH 'Get'")).firstMatch
        XCTAssertTrue(get.waitForExistence(timeout: 10), "no Get button")
        get.tap()
        sleep(12) // the download and install
        app.navigationBars.buttons.firstMatch.tap()
        sleep(3)
    }

    /// Changing languages: the language key cycles the layouts; holding it opens the settings,
    /// where a language is switched off and on.
    func testESwitchLanguages() throws {
        launchApp()
        let field = app.textViews["try-it"].exists ? app.textViews["try-it"] : app.textFields["try-it"]
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.tap()
        sleep(2)
        // The key names the language it goes to next; not the current one's own label
        // (Malayalam has a letter key മ).
        let keyLabels = ["EN": "en", "മ": "ml", "हि": "hi", "অ": "as"]
        var current = "en"
        func key() -> XCUIElement {
            let labels = keyLabels.filter { $0.value != current }.map(\.key)
            return app.keys.matching(NSPredicate(format: "identifier IN %@", labels)).firstMatch
        }
        for _ in 0..<4 {
            let k = key()
            XCTAssertTrue(k.waitForExistence(timeout: 5), "no language key")
            let next = keyLabels[k.identifier] ?? "en"
            k.tap()
            current = next
            sleep(2)
        }
        key().press(forDuration: 1.2)
        XCTAssertTrue(app.buttons["settings-done"].waitForExistence(timeout: 5))
        sleep(2)
        for id in ["as", "as"] {
            let toggle = app.switches["language-\(id)"]
            if toggle.exists {
                toggle.switches.firstMatch.exists ? toggle.switches.firstMatch.tap() : toggle.tap()
                sleep(2)
            }
        }
        app.buttons["settings-done"].tap()
        sleep(2)
    }

    private func launchApp() {
        app.launchArguments += ["-chooseLanguagesDone", "YES"]
        app.terminate()
        XCUIDevice.shared.press(.home)
        sleep(1)
        app.launch()
    }

    private func languageKey() -> XCUIElement {
        app.keys.matching(NSPredicate(format: "identifier IN %@", ["മ", "हि", "অ", "EN"])).firstMatch
    }

    private func openKeyboards() {
        settings.terminate()
        settings.launch()
        tapRow("General")
        tapRow("Keyboard")
        tapRow("Keyboards")
    }

    private func tapRow(_ label: String) {
        // A list row, not the navigation title of the same name ("Keyboards").
        let row = settings.cells.containing(NSPredicate(format: "label BEGINSWITH %@", label)).firstMatch
        if !row.waitForExistence(timeout: 5) {
            settings.swipeUp()
        }
        XCTAssertTrue(row.waitForExistence(timeout: 5), "no \(label)")
        row.tap()
        sleep(1)
    }
}

import XCTest

/// Raw takes for the how-to video (v3), for recordings only: skips without PREVIEW_DIR. Same
/// simulator setup as AppStorePreview. Each test sets its scene up, writes `ready` to
/// PREVIEW_DIR and waits for `go` (the host has started `simctl io recordVideo` by then), plays
/// the take, and writes `done`, so the recording holds little more than the take.
/// The host prepares the App Group first: take 1 with Tamil not installed, the others with
/// active-languages.json {"languages":["en","hi","ml","ta"]}.
@MainActor
final class HowToTakes: XCTestCase {
    private var dir = ""
    private let app = XCUIApplication()

    override func setUp() async throws {
        continueAfterFailure = false
        guard let dir = ProcessInfo.processInfo.environment["PREVIEW_DIR"], !dir.isEmpty else {
            throw XCTSkip("footage only: set PREVIEW_DIR")
        }
        self.dir = dir
        for name in ["ready", "go", "done"] {
            try? FileManager.default.removeItem(atPath: "\(dir)/\(name)")
        }
        app.launchArguments += ["-chooseLanguagesDone", "YES"]
        XCUIDevice.shared.appearance = .light
        XCUIDevice.shared.orientation = .portrait
    }

    /// Home → Add or remove languages → the download list with Tamil, held → Get → installed.
    func test1TamilDownload() throws {
        launchApp()
        XCTAssertTrue(app.buttons["languages"].waitForExistence(timeout: 10))
        sleep(1)
        try roll {
            sleep(1)
            app.buttons["languages"].tap()
            XCTAssertTrue(app.navigationBars["Languages"].waitForExistence(timeout: 5))
            let get = app.buttons["Get"].firstMatch
            XCTAssertTrue(get.waitForExistence(timeout: 15), "no Get: is Tamil installed?")
            XCTAssertTrue(app.staticTexts.matching(NSPredicate(format: "label CONTAINS 'Tamil'")).firstMatch.exists,
                          "Tamil to download")
            sleep(1)
            // Down to the Download section, slowly.
            for _ in 0..<4 where !(get.isHittable && get.frame.maxY < app.frame.maxY - 120) {
                let start = app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.75))
                start.press(forDuration: 0.05, thenDragTo: start.withOffset(CGVector(dx: 0, dy: -420)),
                            withVelocity: .slow, thenHoldForDuration: 0.2)
                usleep(500_000)
            }
            sleep(2)
            get.tap()
            // Installed: the pack's folder in the App Group (PACKS_DIR, the host's path to it).
            // Watched on disk rather than on screen: an XCUITest query takes about a second,
            // and the hold should start when the row moves.
            let packs = ProcessInfo.processInfo.environment["PACKS_DIR"] ?? ""
            let deadline = Date().addingTimeInterval(120)
            while !FileManager.default.fileExists(atPath: "\(packs)/ta.neopack/manifest.json"), Date() < deadline {
                usleep(50_000)
            }
            XCTAssertTrue(FileManager.default.fileExists(atPath: "\(packs)/ta.neopack/manifest.json"), "Tamil installed")
            sleep(3)
        }
    }

    /// Malayalam on (active en, hi, ml: with Tamil on too, Tamil's words take the slots):
    /// "thookkal" typed in Try it until തൂക്കൽ is in the bar, held, tapped.
    func test2MissingWord() throws {
        try showKeyboard()
        showLatinKeys()
        try roll {
            sleep(1)
            for ch in "thookkal" {
                key(String(ch)).tap()
                usleep(150_000)
            }
            let word = app.buttons["തൂക്കൽ"].firstMatch
            XCTAssertTrue(word.waitForExistence(timeout: 3), "തൂക്കൽ in the bar")
            let width = app.windows.firstMatch.frame.width
            XCTAssertTrue(word.frame.minX >= 0 && word.frame.maxX <= width, "തൂക്കൽ in view: \(word.frame)")
            sleep(1)
            word.tap()
            sleep(2)
        }
    }

    /// The language key, tapped round English → Hindi → Malayalam → Tamil → English.
    func test3LanguageCycle() throws {
        try showKeyboard()
        showLatinKeys()
        try roll {
            sleep(1)
            for _ in 0..<4 {
                languageKey().tap()
                usleep(1_300_000)
            }
            sleep(1)
        }
    }

    /// Hold the language key: the panel; Hindi off and back on (the first switches in view); Done.
    func test4HoldPanel() throws {
        try showKeyboard()
        showLatinKeys()
        try roll {
            sleep(1)
            languageKey().press(forDuration: 1.2)
            XCTAssertTrue(app.buttons["settings-done"].waitForExistence(timeout: 5))
            sleep(2)
            let toggle = app.switches["language-hi"]
            XCTAssertTrue(toggle.isHittable, "Hindi's switch in view")
            for _ in 0..<2 {
                toggle.switches.firstMatch.exists ? toggle.switches.firstMatch.tap() : toggle.tap()
                sleep(2)
            }
            app.buttons["settings-done"].tap()
            sleep(2)
        }
    }

    /// The system keyboard up in Try it: hold 🌐, pick NeoOne Keyboard.
    func test5GlobeSwitch() throws {
        try showKeyboard()
        // To the system keyboard first (not recorded): 🌐 below the keys goes to the next one.
        let globe = app.buttons["Next keyboard"].firstMatch
        XCTAssertTrue(globe.waitForExistence(timeout: 5), "no globe")
        globe.tap()
        sleep(2)
        XCTAssertFalse(languageKey().exists, "the system keyboard is up")
        try roll {
            sleep(1)
            globe.press(forDuration: 1.5)
            let pick = app.staticTexts["NeoOne Keyboard"].firstMatch
            XCTAssertTrue(pick.waitForExistence(timeout: 3), "the keyboard list")
            sleep(1)
            pick.tap()
            XCTAssertTrue(languageKey().waitForExistence(timeout: 5), "NeoOne Keyboard is up")
            sleep(2)
        }
    }

    /// A name typed in Try it (WORD, e.g. sreelakshmi), the bar held with EXPECT (ശ്രീലക്ഷ്മി)
    /// in it, then space commits the default; TAP=1 taps EXPECT instead. LOWER=1 turns shift
    /// off first: at the field's start a capitalised name keeps its English spelling as the
    /// default. Active en, ml, hi.
    func test6Name() throws {
        let env = ProcessInfo.processInfo.environment
        let word = env["WORD"] ?? "sreelakshmi", expect = env["EXPECT"] ?? "ശ്രീലക്ഷ്മി"
        try showKeyboard()
        showLatinKeys()
        let lower = env["LOWER"] == "1"
        try roll {
            sleep(1)
            if lower {
                // Shift off: the field's start capitalises the first letter.
                app.keys.matching(NSPredicate(format: "label BEGINSWITH 'shift'")).firstMatch.tap()
                usleep(400_000)
            }
            for ch in word {
                key(String(ch)).tap()
                usleep(150_000)
            }
            XCTAssertTrue(app.buttons[expect].firstMatch.waitForExistence(timeout: 3), "\(expect) in the bar")
            sleep(1)
            if env["TAP"] == "1" {
                // Not the default: tapped in its slot.
                app.buttons[expect].firstMatch.tap()
            } else {
                let ret = app.keys["return"].frame
                app.windows.firstMatch.coordinate(withNormalizedOffset: .zero)
                    .withOffset(CGVector(dx: app.windows.firstMatch.frame.midX, dy: ret.midY)).tap()
            }
            sleep(2)
        }
    }

    // MARK: -

    /// Signals `ready`, waits for the host's `go`, plays `take`, signals `done`.
    private func roll(_ take: () throws -> Void) throws {
        FileManager.default.createFile(atPath: "\(dir)/ready", contents: nil)
        let deadline = Date().addingTimeInterval(60)
        while !FileManager.default.fileExists(atPath: "\(dir)/go") {
            guard Date() < deadline else { throw XCTSkip("no go from the host") }
            usleep(100_000)
        }
        try take()
        FileManager.default.createFile(atPath: "\(dir)/done", contents: nil)
    }

    private func launchApp() {
        XCUIApplication(bundleIdentifier: "me.jishnu.keyboard.extension").terminate()
        app.terminate()
        XCUIDevice.shared.press(.home)
        sleep(1)
        app.launch()
    }

    private func showKeyboard() throws {
        launchApp()
        let field = app.textViews["try-it"].exists ? app.textViews["try-it"] : app.textFields["try-it"]
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.tap()
        if !languageKey().waitForExistence(timeout: 5) {
            // The system keyboard is up: 🌐 to NeoOne Keyboard.
            app.buttons["Next keyboard"].firstMatch.tap()
        }
        XCTAssertTrue(languageKey().waitForExistence(timeout: 5), "NeoOne Keyboard isn't the keyboard shown")
        sleep(1)
    }

    private func languageKey() -> XCUIElement {
        app.keys.matching(NSPredicate(format: "label BEGINSWITH 'switch to'")).firstMatch
    }

    /// The Latin keys (English): the language key until q shows.
    private func showLatinKeys() {
        for _ in 0..<5 where !key("q").exists {
            languageKey().tap()
            usleep(800_000)
        }
        XCTAssertTrue(key("q").exists, "Latin keys")
    }

    private func key(_ letter: String) -> XCUIElement {
        app.keys.matching(NSPredicate(format: "label ==[c] %@", letter)).firstMatch
    }
}

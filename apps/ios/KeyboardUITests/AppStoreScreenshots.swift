import XCTest

/// The App Store screenshots (docs/appstore/screenshots/6.9), for screenshots only: skips
/// without SHOTS_DIR. Run on an iPhone 17 Pro Max simulator with:
/// - a Release build (ARCHS=arm64): a Debug keyboard has no packs and is far over memory;
/// - NeoOne Keyboard added in the simulator's Settings;
/// - ml.neopack and hi.neopack unzipped into the App Group's Packs/, and active-languages.json
///   {"languages":["en","ml","hi"]} beside it (Assamese is left to download);
/// - the keyboard's memory budget (MemoryBudget.keyboardExtension) raised for the run, and
///   not committed: the simulator's footprint is well above a device's, so the memory gate
///   empties the bar on the first key;
/// - `xcrun simctl status_bar <sim> override --time 9:41 --batteryState charged
///   --batteryLevel 100 --cellularBars 4 --wifiBars 3`.
/// SHOTS_DIR goes in the environment: `TEST_RUNNER_SHOTS_DIR=<dir> xcodebuild … test
/// -only-testing:KeyboardUITests/AppStoreScreenshots`.
@MainActor
final class AppStoreScreenshots: XCTestCase {
    private var dir = ""
    private let app = XCUIApplication()

    override func setUp() async throws {
        continueAfterFailure = false
        guard let dir = ProcessInfo.processInfo.environment["SHOTS_DIR"], !dir.isEmpty else {
            throw XCTSkip("screenshots only: set SHOTS_DIR")
        }
        self.dir = dir
        app.launchArguments += ["-chooseLanguagesDone", "YES"]
        XCUIDevice.shared.appearance = .light
    }

    override func tearDown() async throws {
        XCUIDevice.shared.appearance = .light
    }

    /// Manglish in the app's Try it field: "ente" with എന്റെ in the bar.
    func test1Malayalam() throws {
        try showKeyboard()
        type("njan ")
        type("veettil ")
        type("ethi ")
        type("ente")
        shot("01-malayalam")
    }

    /// Hinglish: "mera" with मेरा in the bar.
    func test2Hindi() throws {
        try showKeyboard()
        type("aap ")
        type("kaise ")
        type("hain ")
        type("mera")
        shot("02-hindi")
    }

    /// The keyboard's settings panel (hold the language key), with its language switches.
    func test3SettingsPanel() throws {
        try showKeyboard()
        languageKey().press(forDuration: 1.2)
        XCTAssertTrue(app.buttons["settings-done"].waitForExistence(timeout: 5))
        sleep(1)
        shot("03-settings-panel")
        app.buttons["settings-done"].tap()
    }

    /// The app's Languages screen: the installed languages and the ones to download.
    func test4Languages() throws {
        launchApp()
        app.buttons["languages"].tap()
        XCTAssertTrue(app.navigationBars["Languages"].waitForExistence(timeout: 5))
        sleep(4) // the pack index downloads
        shot("04-languages")
    }

    /// The emoji panel, in dark mode.
    func test5EmojiDark() throws {
        XCUIDevice.shared.appearance = .dark
        try showKeyboard()
        type("party ")
        type("tonight ")
        // The emoji key: the second key of the bottom row (it has no label).
        let numbers = app.keys["123"].frame
        let window = app.windows.firstMatch
        window.coordinate(withNormalizedOffset: .zero)
            .withOffset(CGVector(dx: numbers.maxX + 28, dy: numbers.midY)).tap()
        sleep(2)
        shot("05-emoji-dark")
    }

    /// The app's home.
    func test6Home() throws {
        launchApp()
        XCTAssertTrue(app.staticTexts["NeoOne Keyboard"].waitForExistence(timeout: 10))
        sleep(1)
        shot("06-home")
    }

    // MARK: -

    /// Launches the app and focuses its Try it field (empty at launch).
    private func showKeyboard() throws {
        launchApp()
        let field = app.textViews["try-it"].exists ? app.textViews["try-it"] : app.textFields["try-it"]
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.tap()
        XCTAssertTrue(languageKey().waitForExistence(timeout: 5), "NeoOne Keyboard isn't the keyboard shown")
        sleep(1)
    }

    /// Launches the app from the Home Screen (no "back to" another app in the status bar).
    private func launchApp() {
        app.terminate()
        XCUIDevice.shared.press(.home)
        sleep(1)
        app.launch()
    }

    private func languageKey() -> XCUIElement {
        app.keys.matching(NSPredicate(format: "identifier IN %@", ["മ", "हि", "অ", "EN"])).firstMatch
    }

    /// Types `text` by tapping the keyboard's keys (lower-case letters and space).
    private func type(_ text: String) {
        for ch in text {
            if ch == " " {
                let ret = app.keys["return"].frame
                let window = app.windows.firstMatch
                window.coordinate(withNormalizedOffset: .zero)
                    .withOffset(CGVector(dx: window.frame.midX, dy: ret.midY)).tap()
                continue
            }
            let key = app.keys.matching(NSPredicate(format: "label ==[c] %@", String(ch))).firstMatch
            XCTAssertTrue(key.waitForExistence(timeout: 3), "key \(ch)")
            key.tap()
        }
        sleep(1)
    }

    private func shot(_ name: String) {
        try? XCUIScreen.main.screenshot().pngRepresentation
            .write(to: URL(fileURLWithPath: dir).appending(path: "\(name).png"))
    }
}

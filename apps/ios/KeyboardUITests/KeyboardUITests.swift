import XCTest

/// The home and About screens, and the keyboard's settings panel (held language key).
/// The panel tests need NeoOne Keyboard added in the simulator's Settings; they skip otherwise.
/// Set SHOTS_DIR (xcodebuild TEST_RUNNER_SHOTS_DIR=<dir>) to save screenshots there.
@MainActor
final class KeyboardUITests: XCTestCase {
    private var app: XCUIApplication!

    override func setUp() async throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments += ["-chooseLanguagesDone", "YES"]
    }

    func testHome() throws {
        app.launch()
        XCTAssertTrue(app.staticTexts["NeoOne Keyboard"].waitForExistence(timeout: 10))
        XCTAssertTrue(app.staticTexts["The last keyboard you will ever need"].exists)
        XCTAssertTrue(app.buttons["open-settings"].exists)
        XCTAssertTrue(app.textFields["try-it"].exists || app.textViews["try-it"].exists)
        XCTAssertTrue(app.staticTexts["English"].exists)
        shot("home")
        app.swipeUp()
        shot("home-scrolled")

        app.buttons["languages"].tap()
        XCTAssertTrue(app.navigationBars["Languages"].waitForExistence(timeout: 5))
        app.navigationBars.buttons.firstMatch.tap()

        app.staticTexts["About and credits"].tap()
        XCTAssertTrue(app.staticTexts["Open-source software"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.staticTexts["English Wikipedia"].exists)
        shot("about")
        app.swipeUp()
        app.swipeUp()
        shot("about-scrolled")
    }

    func testSettingsPanel() throws {
        try openPanel()
        let width = app.windows.firstMatch.frame.width
        let scroll = app.scrollViews["settings-scroll"]
        XCTAssertTrue(app.switches["language-en"].exists)
        shot("panel")
        // Every trailing control keeps the 16 pt margin of its card, itself 16 pt in from the
        // panel (which is 3 pt in from the screen): iOS 26's wider switch touched the edge.
        let limit = width - 3 - 16 - 16 + 1 // half a point either way for rounding
        for identifier in ["language-en", "suggestion-count", "number-row", "autocorrect", "emoji-suggestions", "key-clicks"] {
            let control = app.descendants(matching: .any)[identifier]
            reveal(control, in: scroll)
            XCTAssertTrue(scroll.frame.contains(control.frame), identifier)
            XCTAssertLessThanOrEqual(control.frame.maxX, limit, "\(identifier) reaches the margin")
            if identifier == "autocorrect" { shot("panel-typing") }
        }
        let accent = app.buttons["accent-indigo"]
        reveal(accent, in: scroll)
        XCTAssertLessThanOrEqual(accent.frame.maxX, limit)
        shot("panel-look")
        let diagnostics = app.buttons["insert-diagnostics"]
        reveal(diagnostics, in: scroll)
        XCTAssertTrue(diagnostics.isHittable)
        shot("panel-bottom")
        app.buttons["settings-done"].tap()
        XCTAssertFalse(app.buttons["settings-done"].isHittable)
    }

    /// The keys in both languages (screenshots, for comparing with the design): the language
    /// key switches to the other and back.
    func testKeyboardLanguages() throws {
        let key = try showKeyboard()
        let first = key.label
        shot("keyboard-\(first == "EN" ? "ml" : "en")")
        key.tap()
        let other = app.staticTexts[first == "EN" ? "മ" : "EN"]
        XCTAssertTrue(other.waitForExistence(timeout: 5))
        shot("keyboard-\(first == "EN" ? "en" : "ml")")
        other.tap()
        XCTAssertTrue(app.staticTexts[first].waitForExistence(timeout: 5))
    }

    /// The icon on the Home Screen, for screenshots only (skips without SHOTS_DIR).
    func testIconOnHomeScreen() throws {
        guard ProcessInfo.processInfo.environment["SHOTS_DIR"]?.isEmpty == false else {
            throw XCTSkip("screenshots only")
        }
        let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")
        // Twice: the second press goes to the first page.
        XCUIDevice.shared.press(.home)
        sleep(1)
        XCUIDevice.shared.press(.home)
        let icon = springboard.icons["NeoOne Keyboard"]
        XCTAssertTrue(icon.waitForExistence(timeout: 5))
        for _ in 0..<3 where !icon.isHittable {
            springboard.swipeLeft()
            sleep(1)
        }
        XCTAssertTrue(icon.isHittable)
        sleep(1)
        shot("springboard")
    }

    /// Shows the keyboard for the Try it field; returns its language key.
    private func showKeyboard() throws -> XCUIElement {
        app.launch()
        let field = app.textViews["try-it"].exists ? app.textViews["try-it"] : app.textFields["try-it"]
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.tap()
        let key = app.staticTexts.matching(NSPredicate(format: "label IN %@", ["മ", "EN"])).firstMatch
        guard key.waitForExistence(timeout: 5) else {
            throw XCTSkip("NeoOne Keyboard isn't the keyboard shown; add it in the simulator's Settings")
        }
        return key
    }

    /// Opens the panel by holding the language key, which reads മ or EN (the next language).
    private func openPanel() throws {
        try showKeyboard().press(forDuration: 1.2)
        XCTAssertTrue(app.buttons["settings-done"].waitForExistence(timeout: 5))
    }

    /// Drags `scroll` a little at a time until `element` is inside it.
    private func reveal(_ element: XCUIElement, in scroll: XCUIElement) {
        for _ in 0..<12 where !scroll.frame.contains(element.frame) {
            let up = element.frame.maxY > scroll.frame.maxY
            let start = scroll.coordinate(withNormalizedOffset: CGVector(dx: 0.1, dy: up ? 0.7 : 0.3))
            let end = start.withOffset(CGVector(dx: 0, dy: up ? -50 : 50))
            start.press(forDuration: 0.05, thenDragTo: end, withVelocity: .slow, thenHoldForDuration: 0.3)
        }
    }

    private func shot(_ name: String) {
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
        guard let dir = ProcessInfo.processInfo.environment["SHOTS_DIR"], !dir.isEmpty else { return }
        let style = XCUIDevice.shared.appearance == .dark ? "dark" : "light"
        try? XCUIScreen.main.screenshot().pngRepresentation
            .write(to: URL(fileURLWithPath: dir).appending(path: "\(name)-\(style).png"))
    }
}

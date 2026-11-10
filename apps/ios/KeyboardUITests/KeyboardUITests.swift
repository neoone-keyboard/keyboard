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
        let first = key.identifier
        shot("keyboard-\(first == "EN" ? "ml" : "en")")
        key.tap()
        let other = app.keys[first == "EN" ? "മ" : "EN"]
        XCTAssertTrue(other.waitForExistence(timeout: 5))
        shot("keyboard-\(first == "EN" ? "en" : "ml")")
        other.tap()
        XCTAssertTrue(app.keys[first].waitForExistence(timeout: 5))
    }

    /// Four languages at once (screenshots): the panel with English, Malayalam, Hindi and
    /// Assamese on, the language key cycling through every layout and page, and a Manglish
    /// and a Hinglish sentence on the Latin layout. Needs the Hindi and Assamese packs (a
    /// Debug build with ML_DEBUG_PACKS="hi as"); skips without them.
    func testFourLanguages() throws {
        try openPanel()
        try switchOn(["en", "ml", "hi", "as"])
        shot("four-panel")
        app.buttons["settings-done"].tap()

        // Latin, then each native layout (in the panel's order) with each of its pages. The
        // key names the language it goes to next.
        let keyLabels = ["EN": "en", "മ": "ml", "हि": "hi", "অ": "as"]
        let pages: [String: [String]] = [
            "ml": ["കഖ", "ശഷ", "അആ"], "hi": ["कख", "शष", "अआ"], "as": ["কখ", "শষ", "অআ"],
        ]
        var current = "en"
        var visited: [String] = []
        for index in 1...4 {
            visited.append(current)
            // Not the language's own label: Malayalam has a letter key മ.
            let labels = keyLabels.filter { $0.value != current }.map(\.key)
            let key = app.keys.matching(NSPredicate(format: "identifier IN %@", labels)).firstMatch
            XCTAssertTrue(key.waitForExistence(timeout: 5), "\(current): a language key")
            let next = keyLabels[key.identifier] ?? "?"
            shot("four-\(index)-\(current)")
            // Every page but the first, by its page key, then back to the letters.
            let own = pages[current] ?? []
            for label in own.dropFirst() {
                let page = app.keys[label]
                XCTAssertTrue(page.waitForExistence(timeout: 5), "\(current): page \(label)")
                page.tap()
                shot("four-\(index)-\(current)-\(label)")
            }
            if let letters = own.first { app.keys[letters].tap() }
            key.tap()
            current = next
        }
        XCTAssertEqual(Set(visited), ["en", "ml", "hi", "as"])
        XCTAssertEqual(current, "en", "round to Latin")

        // Manglish after a Malayalam word, and Hinglish after a Hindi one.
        type("ente ")
        type("pani")
        shot("four-bar-manglish")
        // A fresh field; the keyboard keeps its languages.
        app.terminate()
        _ = try showKeyboard()
        type("mera ")
        type("pani")
        shot("four-bar-hinglish")
    }

    /// The keyboard's memory with the languages in TEST_RUNNER_LANGS (SHOTS_DIR set): they are
    /// switched on in the panel, words of each are typed letter by letter, and the panel's
    /// diagnostics line (footprint and peak) is written to SHOTS_DIR/footprint-<langs>.txt.
    /// The host kills the extension before each run, so the peak is this run's.
    func testFootprintWhileTyping() throws {
        let env = ProcessInfo.processInfo.environment
        guard let dir = env["SHOTS_DIR"], !dir.isEmpty, let langs = env["LANGS"], !langs.isEmpty else {
            throw XCTSkip("measurement only: set SHOTS_DIR and LANGS")
        }
        let wanted = langs.split(separator: ",").map(String.init)
        try openPanel()
        try switchOn(wanted, only: true)
        app.buttons["settings-done"].tap()
        let words: [String: [String]] = [
            "en": ["the ", "because ", "tomorrow ", "beautiful "],
            "ml": ["ente ", "veettil ", "njan ", "sukhamano "],
            "hi": ["mera ", "kya ", "nahi ", "bahut "],
            "as": ["moi ", "tumi ", "manuh ", "kotha "],
        ]
        for id in wanted {
            for word in words[id] ?? [] { type(word) }
        }
        shot("footprint-\(langs)")
        try openPanel(launch: false)
        let diagnostics = app.buttons["insert-diagnostics"]
        reveal(diagnostics, in: app.scrollViews["settings-scroll"])
        diagnostics.tap()
        // Inserting the report closes the panel.
        if app.buttons["settings-done"].isHittable { app.buttons["settings-done"].tap() }
        let report = (field.value as? String) ?? ""
        let line = report.split(separator: "\n").first { $0.contains("footprint") }.map(String.init) ?? report
        try line.write(to: URL(fileURLWithPath: dir).appending(path: "footprint-\(langs).txt"), atomically: true, encoding: .utf8)
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

    private var field: XCUIElement {
        app.textViews["try-it"].exists ? app.textViews["try-it"] : app.textFields["try-it"]
    }

    /// Shows the keyboard for the Try it field; returns its language key.
    private func showKeyboard(launch: Bool = true) throws -> XCUIElement {
        if launch { app.launch() }
        let field = self.field
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.tap()
        let key = app.keys.matching(NSPredicate(format: "identifier IN %@", ["മ", "हि", "অ", "EN"])).firstMatch
        guard key.waitForExistence(timeout: 5) else {
            throw XCTSkip("NeoOne Keyboard isn't the keyboard shown; add it in the simulator's Settings")
        }
        return key
    }

    /// VoiceOver's view of the keys: each key is a keyboard key named for what it types or
    /// does, and the language key names the language it goes to.
    func testKeysForVoiceOver() throws {
        let languageKey = try showKeyboard()
        XCTAssertTrue(languageKey.label.hasPrefix("switch to "), languageKey.label)
        for label in ["space", "delete", "return", "numbers and symbols", "emoji"] {
            XCTAssertTrue(spokenKey(label).exists, label)
        }
        // At the start of a sentence shift is on and the letters are spoken as capitals;
        // with shift off, in lower case.
        let shift = app.keys.matching(NSPredicate(format: "label IN %@", ["shift", "caps lock"])).firstMatch
        XCTAssertTrue(shift.exists)
        let shifted = (shift.value as? String) == "on"
        XCTAssertTrue(spokenKey(shifted ? "Q" : "q").exists, "q, shift \(shifted ? "on" : "off")")
        spokenKey("numbers and symbols").tap()
        XCTAssertTrue(spokenKey("more symbols").waitForExistence(timeout: 5))
        XCTAssertTrue(spokenKey("letters").exists)
        spokenKey("letters").tap()
        XCTAssertTrue(spokenKey("numbers and symbols").waitForExistence(timeout: 5))
    }

    /// The key VoiceOver calls `label`.
    private func spokenKey(_ label: String) -> XCUIElement {
        app.keys.matching(NSPredicate(format: "label == %@", label)).firstMatch
    }

    /// Opens the panel by holding the language key, which reads the next language (മ, हि, অ
    /// or EN).
    private func openPanel(launch: Bool = true) throws {
        try showKeyboard(launch: launch).press(forDuration: 1.2)
        XCTAssertTrue(app.buttons["settings-done"].waitForExistence(timeout: 5))
    }

    /// Switches the languages `ids` on in the open panel (and, with `only`, the others off);
    /// skips when a language has no switch (its pack isn't in this build).
    private func switchOn(_ ids: [String], only: Bool = false) throws {
        let scroll = app.scrollViews["settings-scroll"]
        let all = ["en", "ml", "hi", "as"]
        for id in ids where !app.switches["language-\(id)"].exists {
            throw XCTSkip("no \(id) switch: build with ML_DEBUG_PACKS=\"hi as\"")
        }
        // On first, then off: at least one stays on.
        for id in ids + (only ? all.filter { !ids.contains($0) } : []) {
            let toggle = app.switches["language-\(id)"]
            guard toggle.exists else { continue }
            reveal(toggle, in: scroll)
            let on = (toggle.value as? String) == "1"
            if on != ids.contains(id) {
                toggle.switches.firstMatch.exists ? toggle.switches.firstMatch.tap() : toggle.tap()
            }
            XCTAssertEqual((toggle.value as? String) == "1", ids.contains(id), id)
        }
        reveal(app.switches["language-\(ids[0])"], in: scroll)
    }

    /// Types `text` by tapping the keyboard's keys (lower-case letters and space).
    private func type(_ text: String) {
        for ch in text {
            if ch == " " {
                // The space bar: the middle of the bottom row, which holds return. ("English",
                // its label, is also a row of the app's screen.)
                let ret = app.keys["return"].frame
                let window = app.windows.firstMatch
                window.coordinate(withNormalizedOffset: .zero)
                    .withOffset(CGVector(dx: window.frame.midX, dy: ret.midY)).tap()
                continue
            }
            let letter = String(ch)
            let key = app.keys.matching(NSPredicate(format: "label ==[c] %@", letter)).firstMatch
            if !key.waitForExistence(timeout: 3) { shot("missing-key-\(letter)") }
            XCTAssertTrue(key.exists, "key \(letter)")
            key.tap()
        }
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

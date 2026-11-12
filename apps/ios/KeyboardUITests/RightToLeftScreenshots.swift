import XCTest

/// Screenshots of Roman Urdu typed into Messages, a host whose field takes the direction of
/// what is typed: the mirrored suggestion bar in Nastaliq, Urdu's ۔ and ؟, the symbols page
/// with Urdu's marks, the full stop key's options. For screenshots only: skips without
/// SHOTS_DIR. Run on an iPhone 17 Pro Max simulator as `AppStoreScreenshots` says (a Release
/// build, NeoOne Keyboard added, the memory budget raised for the run and not committed),
/// with packs/ur.neopack in the App Group's Packs/ and active-languages.json
/// {"languages":["en","ur"]} beside it; `testEnglish` with {"languages":["en"]}.
@MainActor
final class RightToLeftScreenshots: XCTestCase {
    private var dir = ""
    private let messages = XCUIApplication(bundleIdentifier: "com.apple.MobileSMS")

    override func setUp() async throws {
        continueAfterFailure = false
        guard let dir = ProcessInfo.processInfo.environment["SHOTS_DIR"], !dir.isEmpty else {
            throw XCTSkip("screenshots only: set SHOTS_DIR")
        }
        self.dir = dir
        XCUIDevice.shared.appearance = .light
        XCUIDevice.shared.orientation = .portrait
    }

    func testUrdu() throws {
        try showKeyboard(languageKeys: ["ار", "EN"])
        // Shift off: the engine keeps a capitalised Latin word as typed (a name, Mera), and
        // the first word of a message is capitalised. The first key also opens the engine.
        tapKey("shift")
        type("m")
        sleep(3)
        type("era naam ")
        type("ali hai.")
        type(" aap kaise hain")
        tapKey("123")
        tapKey("؟")
        type(" shukriya")
        shot("ur_after_ios_bar")
        type("  ")
        shot("ur_after_ios_letters")
        tapKey("123")
        sleep(1)
        shot("ur_after_ios_symbols")
    }

    /// Holds the full stop key long enough for a screenshot from outside the test
    /// (`xcrun simctl io booted screenshot`): XCUITest can't take one mid-press.
    func testFullStopOptions() throws {
        try showKeyboard(languageKeys: ["ار", "EN"])
        type("m")
        sleep(3)
        type("era naam ali hai")
        let stop = messages.keys["."]
        XCTAssertTrue(stop.waitForExistence(timeout: 3))
        shot("fullstop-press-start")
        stop.press(forDuration: 8)
    }

    func testEnglish() throws {
        try showKeyboard(languageKeys: ["EN", "English"])
        type("hello there.  how are you")
        shot("ur_after_ios_english")
    }

    // MARK: -

    /// A new message in Messages, its body focused, with NeoOne Keyboard up.
    private func showKeyboard(languageKeys: [String]) throws {
        messages.terminate()
        messages.launch()
        // The simulator's first conversation.
        let conversation = messages.staticTexts.matching(NSPredicate(format: "label BEGINSWITH '+1'")).firstMatch
        if conversation.waitForExistence(timeout: 5) {
            conversation.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
        }
        sleep(2)
        // The message's field: the lowest text field or view (a new message also has its To field).
        let fields = (messages.textViews.allElementsBoundByIndex + messages.textFields.allElementsBoundByIndex)
            .filter { $0.exists && $0.isHittable }
        let body = try XCTUnwrap(fields.max { $0.frame.minY < $1.frame.minY }, "no field to type in")
        body.tap()
        let ours = messages.keys["return"]
        for _ in 0..<4 where !ours.waitForExistence(timeout: 2) {
            let globe = messages.buttons["Next keyboard"].exists ? messages.buttons["Next keyboard"] : messages.keys["Next keyboard"]
            globe.tap()
        }
        XCTAssertTrue(ours.waitForExistence(timeout: 5), "NeoOne Keyboard isn't the keyboard shown")
        sleep(2)
    }

    private func tapKey(_ label: String) {
        let key = messages.keys.matching(NSPredicate(format: "label == %@ OR identifier == %@", label, label)).firstMatch
        XCTAssertTrue(key.waitForExistence(timeout: 3), "key \(label)")
        key.tap()
        usleep(400_000)
    }

    /// Types `text` by tapping the keys: lower-case letters, `.` and space.
    private func type(_ text: String) {
        for ch in text {
            if ch == " " {
                let ret = messages.keys["return"].frame
                let window = messages.windows.firstMatch
                window.coordinate(withNormalizedOffset: .zero)
                    .withOffset(CGVector(dx: window.frame.midX, dy: ret.midY)).tap()
                usleep(300_000)
                continue
            }
            let key = messages.keys.matching(NSPredicate(format: "label ==[c] %@", String(ch))).firstMatch
            XCTAssertTrue(key.waitForExistence(timeout: 3), "key \(ch)")
            key.tap()
            usleep(150_000)
        }
        sleep(1)
    }

    private func shot(_ name: String) {
        try? XCUIScreen.main.screenshot().pngRepresentation
            .write(to: URL(fileURLWithPath: dir).appending(path: "\(name).png"))
    }
}

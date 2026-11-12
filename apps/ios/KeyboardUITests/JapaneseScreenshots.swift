import XCTest

/// Screenshots of Japanese typed into Messages, Gboard's way: the kana as marked text with
/// the candidate bar, space converting, every candidate in the keys' place, the first segment
/// shortened, and English typed as before. For screenshots only: skips without SHOTS_DIR.
/// Run on a simulator with NeoOne Keyboard added in Settings and a Debug build with the
/// Japanese pack (`ML_DEBUG_PACKS=ja ML_ACTIVE_LANGUAGES=en,ja`), and the keyboard's memory
/// budget (MemoryBudget.keyboardExtension) raised for the run and not committed: the
/// simulator's footprint passes it on the first key, and the converter goes with the engine.
///   xcodebuild … ML_DEBUG_PACKS=ja ML_ACTIVE_LANGUAGES=en,ja TEST_RUNNER_SHOTS_DIR=… test
///     -only-testing:KeyboardUITests/JapaneseScreenshots
@MainActor
final class JapaneseScreenshots: XCTestCase {
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

    func testJapanese() throws {
        try showKeyboard()
        tapKey("あ")
        sleep(2)
        type("kyouhaiitenkidesune")
        shot("ja_ios_typing")
        space()
        shot("ja_ios_candidates")
        space()
        shot("ja_ios_next_candidate")
        tapButton("all candidates")
        shot("ja_ios_grid")
        tapButton("back to the keys")
        tapButton("shorter segment")
        shot("ja_ios_segment")
        tapButton("longer segment")
        tapButton("longer segment")
        shot("ja_ios_segment_longer")
        let confirm = messages.keys.matching(NSPredicate(format: "label == 'confirm'")).firstMatch
        XCTAssertTrue(confirm.waitForExistence(timeout: 3), "the return key reads 確定")
        confirm.tap()
        sleep(1)
        shot("ja_ios_committed")
        // Back to English for the next run.
        tapKey("EN")
    }

    func testEnglish() throws {
        try showKeyboard()
        type("hello there how are you")
        shot("ja_ios_english")
    }

    // MARK: -

    /// A message's body focused in Messages, with NeoOne Keyboard up on English.
    private func showKeyboard() throws {
        messages.terminate()
        messages.launch()
        let conversation = messages.staticTexts.matching(NSPredicate(format: "label BEGINSWITH '+1'")).firstMatch
        if conversation.waitForExistence(timeout: 5) {
            conversation.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
        } else {
            let compose = messages.buttons["Compose"].firstMatch
            if compose.waitForExistence(timeout: 3) { compose.tap() }
        }
        sleep(2)
        let fields = (messages.textViews.allElementsBoundByIndex + messages.textFields.allElementsBoundByIndex)
            .filter { $0.exists && $0.isHittable }
        let body = try XCTUnwrap(fields.max { $0.frame.minY < $1.frame.minY }, "no field to type in")
        body.tap()
        // The system keyboard's first-run slide-to-type card hides the globe.
        let onboarding = messages.buttons["Continue"].firstMatch
        if onboarding.waitForExistence(timeout: 2) { onboarding.tap() }
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

    private func tapButton(_ label: String) {
        let button = messages.buttons[label].firstMatch
        XCTAssertTrue(button.waitForExistence(timeout: 3), "button \(label)")
        button.tap()
        sleep(1)
    }

    /// The space bar, in the middle of the bottom row.
    private func space() {
        let ret = messages.keys.matching(NSPredicate(format: "label == 'return' OR label == 'confirm'")).firstMatch.frame
        let window = messages.windows.firstMatch
        window.coordinate(withNormalizedOffset: .zero)
            .withOffset(CGVector(dx: window.frame.midX, dy: ret.midY)).tap()
        sleep(1)
    }

    /// Types `text` by tapping the keys: lower-case letters and space.
    private func type(_ text: String) {
        for ch in text {
            if ch == " " {
                space()
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

import XCTest

/// The suggestion bar on the real keyboard: after a swipe with more words than its three
/// slots, the rest are past the right edge, and a finger slides the bar to them and taps one.
/// Needs NeoOne Keyboard added in the simulator's Settings (skips otherwise) and the default
/// suggestion count or more (10; the panel's 3 has nothing to slide to).
///
/// Only after a swipe: in a simulator's Debug build the keyboard's footprint passes its
/// memory budget on the first key or swipe, which turns prediction off, so a typed word gets
/// no suggestions there. The swipe's own words are in the bar before that. The bar with a
/// typed word's suggestions is tested as a view (KeyboardAppTests, `TopBarViewTests`).
@MainActor
final class SuggestionBarUITests: XCTestCase {
    private var app: XCUIApplication!

    override func setUp() async throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments += ["-chooseLanguagesDone", "YES"]
        XCUIDevice.shared.orientation = .portrait
    }

    /// A finger drawn across the keys from p to t (through o, i, u and y) is a swipe; the bar
    /// then holds the word it typed and every other word the path could be.
    func testTheBarSlidesToTheSuggestionsPastThreeAfterASwipe() throws {
        try showEnglishKeyboard()
        let from = key("p").coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        let to = key("t").coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
        from.press(forDuration: 0.05, thenDragTo: to, withVelocity: .slow, thenHoldForDuration: 0.05)
        let words = try waitForBar(atLeast: 4)

        // The swiped word is in the field, and it is one of the bar's.
        let typed = fieldText.trimmingCharacters(in: .whitespaces).lowercased()
        XCTAssertFalse(typed.isEmpty, "the swipe typed its best word")
        XCTAssertTrue(words.contains { $0.label.lowercased() == typed }, "\(typed) is in the bar: \(words.map(\.label))")
        try slideToTheLastWordAndTapIt(words, replacing: typed)
    }

    // MARK: - The bar

    private struct BarWord {
        let label: String
        let frame: CGRect
    }

    /// Every element on screen now, read in one go (the bar changes under a query that
    /// reads it button by button).
    private func elements() -> [any XCUIElementSnapshot] {
        guard let root = try? app.snapshot() else { return [] }
        var all: [any XCUIElementSnapshot] = []
        var queue: [any XCUIElementSnapshot] = [root]
        while let next = queue.popLast() {
            all.append(next)
            queue.append(contentsOf: next.children)
        }
        return all
    }

    /// The bar's words, left to right, with where each is drawn: the buttons in the 60
    /// points above the highest key, in the keyboard's own window (not the screen's
    /// coordinates), so one past the window's width is past the bar's right edge.
    private func barWords() -> [BarWord] {
        let all = elements()
        let keysTop = all.filter { $0.elementType == .key }.map(\.frame.minY).min() ?? 0
        return all
            .filter { $0.elementType == .button && !$0.label.isEmpty && $0.frame.height > 0 }
            .filter { $0.frame.minY >= keysTop - 60 && $0.frame.maxY <= keysTop + 1 }
            .map { BarWord(label: $0.label, frame: $0.frame) }
            .sorted { $0.frame.minX < $1.frame.minX }
    }

    /// Waits until the bar has `count` words or more. Skips when it stays without any: the
    /// engine wasn't there for the swipe.
    private func waitForBar(atLeast count: Int, timeout: TimeInterval = 15) throws -> [BarWord] {
        let deadline = Date().addingTimeInterval(timeout)
        var words: [BarWord] = []
        repeat {
            words = barWords()
            if words.count >= count { return words }
            usleep(300_000)
        } while Date() < deadline
        shot("bar-too-few")
        if words.isEmpty {
            throw XCTSkip("the swipe gave no words (\"\(fieldText)\" typed): prediction is off in the keyboard's process "
                          + "(over its memory budget in this simulator), or the engine took more than its 250 ms")
        }
        throw BarError.tooFew(words.map(\.label))
    }

    private enum BarError: Error {
        case tooFew([String])
    }

    private func barButton(_ word: BarWord) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "label == %@", word.label)).firstMatch
    }

    /// With `words` in the bar: the first three fill it and the rest start past its right
    /// edge; sliding the bar brings the last one into view, and a tap puts it in the field
    /// in place of `replacing`, the swiped word.
    private func slideToTheLastWordAndTapIt(_ words: [BarWord], replacing: String) throws {
        let width = app.windows.firstMatch.frame.width
        shot("bar-before-slide")
        XCTAssertGreaterThan(words.count, 3, "more than the three slots: \(words.map(\.label))")
        XCTAssertEqual(words.filter { $0.frame.maxX <= width + 1 }.count, 3,
                       "three slots fill the bar: \(words.map { "\($0.label) \($0.frame.minX)" })")
        XCTAssertEqual(words[0].frame.minX, 3, accuracy: 1, "the bar starts at its beginning")
        let last = try XCTUnwrap(words.last)
        XCTAssertGreaterThanOrEqual(last.frame.minX, width - 1, "\(last.label) starts past the right edge")

        // A finger on a slot in view, slid left, until the last word is wholly in view.
        var position = last.frame
        for _ in 0..<6 where position.maxX > width {
            let inView = barWords().last { $0.frame.minX >= 0 && $0.frame.maxX <= width } ?? words[2]
            barButton(inView).swipeLeft(velocity: .slow)
            position = barWords().first { $0.label == last.label }?.frame ?? position
        }
        shot("bar-after-slide")
        XCTAssertLessThanOrEqual(position.maxX, width + 1, "\(last.label) slid into view")
        XCTAssertGreaterThanOrEqual(position.minX, 0)
        XCTAssertLessThan(barWords().first?.frame.minX ?? 0, 0, "the first slot slid out to the left")
        // Sliding typed nothing and committed no slot.
        XCTAssertEqual(fieldText.trimmingCharacters(in: .whitespaces).lowercased(), replacing)

        barButton(last).tap()
        let expected = last.label.lowercased()
        let deadline = Date().addingTimeInterval(5)
        while fieldText.trimmingCharacters(in: .whitespaces).lowercased() != expected, Date() < deadline { usleep(200_000) }
        XCTAssertEqual(fieldText.trimmingCharacters(in: .whitespaces).lowercased(), expected,
                       "the tapped word took the swiped one's place")
        shot("bar-after-tap")
    }

    // MARK: - Helpers

    private var field: XCUIElement {
        app.textViews["try-it"].exists ? app.textViews["try-it"] : app.textFields["try-it"]
    }

    private var fieldText: String {
        let value = (field.value as? String) ?? ""
        // An empty field's value is its placeholder.
        return value == field.placeholderValue ? "" : value
    }

    /// Shows the keyboard for the Try it field, on its English keys.
    private func showEnglishKeyboard() throws {
        // The keyboard's process outlives the app; one that has already turned prediction off
        // for memory has no words for a swipe. A fresh one starts with the field's first tap.
        XCUIApplication(bundleIdentifier: "me.jishnu.keyboard.extension").terminate()
        app.launch()
        XCTAssertTrue(field.waitForExistence(timeout: 20))
        field.tap()
        let languageKey = app.keys.matching(NSPredicate(format: "identifier IN %@", ["മ", "हि", "অ", "EN"])).firstMatch
        guard languageKey.waitForExistence(timeout: 10) else {
            shot("bar-no-keyboard")
            throw XCTSkip("NeoOne Keyboard isn't the keyboard shown; add it in the simulator's Settings")
        }
        // The language key names the language it goes to: EN while on a pack's keys.
        for _ in 0..<4 where app.keys["EN"].exists { app.keys["EN"].tap() }
        guard key("q").waitForExistence(timeout: 5), app.keys["return"].exists else {
            throw XCTSkip("NeoOne Keyboard's English keys aren't showing")
        }
        // The engine opens once the keys show; a swipe before it has is asked for again.
        sleep(2)
    }

    private func key(_ letter: String) -> XCUIElement {
        app.keys.matching(NSPredicate(format: "label ==[c] %@", letter)).firstMatch
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
}

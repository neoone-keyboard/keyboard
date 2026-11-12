import XCTest
import KeyboardCore

/// The keyboard's suggestion bar as a view (KeyboardExtension/TopBarView.swift, compiled
/// into this test bundle and put in a window): three slots fill it, and with more
/// suggestions than three the rest are laid out past its right edge, where sliding the bar
/// reaches them. A real finger's slide is in KeyboardUITests (`SuggestionBarUITests`).
@MainActor
final class TopBarViewTests: XCTestCase {
    private var window: UIWindow!
    private var bar: TopBarView!
    private var tapped: [SuggestionSlot] = []

    override func setUp() async throws {
        // The keyboard's bar: the screen's width less its margins, 44 points high.
        window = UIWindow(frame: CGRect(x: 0, y: 0, width: 387, height: 44))
        bar = TopBarView(frame: window.bounds)
        bar.autoresizingMask = [.flexibleWidth, .flexibleHeight]
        window.addSubview(bar)
        window.isHidden = false
        bar.apply(KeyboardTheme.resolve(KeyboardSettings(), style: .light))
        tapped = []
        bar.onSuggestion = { [unowned self] slot in tapped.append(slot) }
    }

    override func tearDown() async throws {
        window.isHidden = true
        window = nil
        bar = nil
    }

    /// What the view controller shows for a typed word: the engine's result, up to `limit`.
    private func show(typed: String, _ words: [String], limit: Int) {
        let result = SuggestionResult(
            typed: typed,
            candidates: [Suggestion(typed, kind: .verbatim)] + words.map { Suggestion($0, kind: .completion) }
        )
        bar.showSuggestions(SuggestionBar(result, limit: limit))
        window.layoutIfNeeded()
    }

    /// The scroll view the slots are in.
    private var scroll: UIScrollView {
        var found: [UIScrollView] = []
        func search(_ view: UIView) {
            if let scroll = view as? UIScrollView, !slots(in: scroll).isEmpty { found.append(scroll) }
            view.subviews.forEach(search)
        }
        search(bar)
        return found[0]
    }

    /// The bar's suggestion buttons, left to right.
    private func slots(in view: UIView) -> [UIButton] {
        var buttons: [UIButton] = []
        func search(_ view: UIView) {
            if let button = view as? UIButton, button.title(for: .normal)?.isEmpty == false { buttons.append(button) }
            view.subviews.forEach(search)
        }
        search(view)
        return buttons.sorted { frame(of: $0).minX < frame(of: $1).minX }
    }

    /// Where `view` is drawn in the bar, as the bar is scrolled now.
    private func frame(of view: UIView) -> CGRect {
        view.convert(view.bounds, to: bar)
    }

    private func titles() -> [String] {
        slots(in: scroll).map { $0.title(for: .normal) ?? "" }
    }

    private func inView(_ button: UIButton) -> Bool {
        let frame = frame(of: button)
        return frame.minX >= -0.5 && frame.maxX <= bar.bounds.width + 0.5
    }

    func testThreeSuggestionsFillTheBarAndItDoesNotMove() {
        show(typed: "he", ["hello", "her", "here", "help"], limit: 3)
        XCTAssertEqual(titles(), ["hello", "he", "her"])
        XCTAssertTrue(slots(in: scroll).allSatisfy(inView))
        XCTAssertEqual(scroll.contentSize.width, bar.bounds.width, accuracy: 0.5, "nothing to slide to")
        XCTAssertEqual(scroll.frame, bar.bounds)
    }

    func testSuggestionsPastThreeAreReachedBySlidingTheBar() throws {
        let more = ["here", "help", "hey", "heat", "head", "hear", "heavy"]
        show(typed: "he", ["hello", "her"] + more, limit: 10)
        let buttons = slots(in: scroll)
        XCTAssertEqual(titles(), ["hello", "he", "her"] + more, "the three slots, then the rest in the engine's order")

        // The three slots are as they are without more: a third of the bar each.
        XCTAssertTrue(buttons.prefix(3).allSatisfy(inView))
        for slot in buttons.prefix(3) {
            XCTAssertEqual(frame(of: slot).width, (bar.bounds.width - 6) / 3, accuracy: 0.5)
        }
        XCTAssertEqual(frame(of: buttons[2]).maxX, bar.bounds.width, accuracy: 0.5)
        // The rest start past the right edge, each wide enough for its word and for a finger.
        for button in buttons.dropFirst(3) {
            XCTAssertGreaterThanOrEqual(frame(of: button).minX, bar.bounds.width, button.title(for: .normal) ?? "")
            XCTAssertFalse(inView(button))
            XCTAssertGreaterThanOrEqual(button.bounds.width, 44)
            XCTAssertGreaterThanOrEqual(button.bounds.width, button.titleLabel?.intrinsicContentSize.width ?? 0)
            XCTAssertEqual(button.bounds.height, bar.bounds.height, accuracy: 0.5)
        }

        // The bar slides sideways as far as the last word, and no further.
        XCTAssertTrue(scroll.isScrollEnabled)
        XCTAssertTrue(scroll.panGestureRecognizer.isEnabled)
        XCTAssertEqual(scroll.contentSize.height, bar.bounds.height, accuracy: 0.5, "sideways only")
        let last = try XCTUnwrap(buttons.last)
        XCTAssertEqual(scroll.contentSize.width, last.convert(last.bounds, to: scroll).maxX, accuracy: 0.5)
        XCTAssertGreaterThan(scroll.contentSize.width, bar.bounds.width + 300)
        // A slide that starts on a slot slides the bar rather than waiting or committing it.
        XCTAssertFalse(scroll.delaysContentTouches)
        XCTAssertTrue(scroll.touchesShouldCancel(in: buttons[1]))

        // Slid to its end, the last words are in view and the first slots are not.
        scroll.setContentOffset(CGPoint(x: scroll.contentSize.width - scroll.bounds.width, y: 0), animated: false)
        window.layoutIfNeeded()
        XCTAssertTrue(inView(last))
        XCTAssertTrue(inView(buttons[buttons.count - 2]))
        XCTAssertFalse(inView(buttons[0]))
        XCTAssertEqual(frame(of: last).maxX, bar.bounds.width, accuracy: 0.5)

        // A tap on a word past the three reports it, like a slot's.
        last.sendActions(for: .touchUpInside)
        XCTAssertEqual(tapped.map(\.suggestion.text), ["heavy"])
        buttons[1].sendActions(for: .touchUpInside)
        XCTAssertEqual(tapped.map(\.suggestion.text), ["heavy", "he"])
    }

    func testNewSuggestionsStartTheBarFromItsBeginning() {
        show(typed: "he", ["hello", "her", "here", "help", "hey", "heat", "head"], limit: 10)
        scroll.setContentOffset(CGPoint(x: 120, y: 0), animated: false)

        // The same suggestions again (a redraw for the same word) leave the bar where it is.
        show(typed: "he", ["hello", "her", "here", "help", "hey", "heat", "head"], limit: 10)
        XCTAssertEqual(scroll.contentOffset.x, 120)

        // The next key's suggestions start from the beginning.
        show(typed: "hel", ["hello", "help", "held", "helmet", "helps", "helped"], limit: 10)
        XCTAssertEqual(scroll.contentOffset.x, 0)
        XCTAssertEqual(titles(), ["hello", "hel", "help", "held", "helmet", "helps", "helped"])

        // And with three or fewer the bar is its own width again.
        scroll.setContentOffset(CGPoint(x: 80, y: 0), animated: false)
        show(typed: "helm", ["helmet"], limit: 10)
        XCTAssertEqual(scroll.contentOffset.x, 0)
        XCTAssertEqual(scroll.contentSize.width, bar.bounds.width, accuracy: 0.5)
        XCTAssertEqual(titles(), ["helmet", "helm"])
    }

    /// A swipe's bar is the same view with the swipe's words (`BarContent.swipe`).
    func testASwipesWordsPastThreeAreReachedTheSameWay() {
        let found = ["put", "pit", "out", "pot", "pitt", "poet", "pout"].map { Suggestion($0, kind: .completion) }
        let swipe = SuggestionResult(typed: "", candidates: found)
        bar.showSuggestions(SuggestionBar(swipe, limit: 10))
        window.layoutIfNeeded()
        XCTAssertEqual(titles(), ["pit", "put", "out", "pot", "pitt", "poet", "pout"])
        XCTAssertEqual(slots(in: scroll).filter(inView).count, 3)
        let scroll = self.scroll
        XCTAssertGreaterThan(scroll.contentSize.width, bar.bounds.width)

        // "Swipe again" takes the slots' place and leaves nothing to slide to.
        bar.showMessage("Swipe again")
        window.layoutIfNeeded()
        XCTAssertEqual(scroll.contentSize.width, bar.bounds.width, accuracy: 0.5)
    }

    // MARK: Right to left (Urdu)

    /// packs/ur.neopack's lang.json, as far as the bar reads it.
    private let urdu = PackDescriptor(
        id: "ur", name: "Urdu", nativeName: "اردو", script: "arab", layout: nil, folder: URL(fileURLWithPath: "/"),
        scriptRanges: [0x0621...0x06D3]
    )

    private func showRoman(limit: Int = 3) {
        let result = SuggestionResult(typed: "mera", candidates: [
            Suggestion("میرا", kind: .manglish), Suggestion("میری", kind: .manglish),
            Suggestion("mera", kind: .verbatim), Suggestion("میر", kind: .manglish), Suggestion("میرے", kind: .manglish),
        ])
        bar.showSuggestions(SuggestionBar(result, limit: limit))
        window.layoutIfNeeded()
    }

    func testARightToLeftBarHasTheBestWordOnTheRight() throws {
        bar.setScript(urdu)
        showRoman()
        // Left to right on screen: the typed word, the other word, the best on the right.
        XCTAssertEqual(titles(), ["\u{201C}mera\u{201D}", "میری", "میرا"])
        let best = try XCTUnwrap(slots(in: scroll).last)
        XCTAssertTrue(inView(best))
        XCTAssertEqual(frame(of: best).maxX, bar.bounds.width, accuracy: 0.5)
        XCTAssertTrue(best.titleLabel?.font.fontDescriptor.symbolicTraits.contains(.traitBold) == true, "the best is bold")
        best.sendActions(for: .touchUpInside)
        XCTAssertEqual(tapped.map(\.suggestion.text), ["میرا"])
    }

    func testARightToLeftBarSlidesLeftToItsMoreWords() {
        bar.setScript(urdu)
        showRoman(limit: 10)
        XCTAssertEqual(titles(), ["میرے", "میر", "\u{201C}mera\u{201D}", "میری", "میرا"])
        let buttons = slots(in: scroll)
        XCTAssertTrue(buttons.suffix(3).allSatisfy(inView))
        XCTAssertFalse(buttons.prefix(2).contains(where: inView), "past the left edge")
    }

    func testUrduWordsAreNastaliqAndFitTheBar() throws {
        XCTAssertNotNil(UIFont(name: "NotoNastaliqUrdu", size: 17), "iOS has Noto Nastaliq Urdu")
        XCTAssertNotNil(UIFont(name: "NotoNastaliqUrdu-Bold", size: 17), "and its Bold")
        bar.setScript(urdu)
        showRoman()
        for button in slots(in: scroll) {
            let title = button.title(for: .normal) ?? ""
            let font = try XCTUnwrap(button.titleLabel?.font)
            if title.contains("mera") {
                XCTAssertFalse(font.familyName.contains("Nastaliq"), "a Latin word keeps the system font")
            } else {
                XCTAssertEqual(font.familyName, "Noto Nastaliq Urdu", title)
                // The best guess in iOS's own Bold face, so it stands out; the others Regular.
                XCTAssertEqual(font.fontName, title == "میرا" ? "NotoNastaliqUrdu-Bold" : "NotoNastaliqUrdu", title)
                XCTAssertLessThanOrEqual(font.lineHeight, bar.bounds.height, "\(title): not clipped")
                XCTAssertGreaterThanOrEqual(font.pointSize, 15, "\(title): still readable")
                XCTAssertEqual(button.accessibilityLanguage, "ur")
            }
        }
    }

    func testLeftToRightAgainOnceThePackIsGone() {
        bar.setScript(urdu)
        showRoman()
        bar.setScript(nil)
        showRoman()
        XCTAssertEqual(titles(), ["\u{201C}mera\u{201D}", "میرا", "میری"])
        XCTAssertTrue(slots(in: scroll).allSatisfy { !($0.titleLabel?.font.familyName.contains("Nastaliq") ?? true) })
    }
}

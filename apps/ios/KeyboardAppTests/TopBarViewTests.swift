import XCTest
import KeyboardCore

/// The keyboard's suggestion bar as a view (KeyboardExtension/TopBarView.swift, compiled
/// into this test bundle and put in a window): as many whole words as fit its width, the
/// default first at its leading edge, and the rest past its trailing edge, where sliding the
/// bar reaches them and a small accent mark hints at them. A real finger's slide is in
/// KeyboardUITests (`SuggestionBarUITests`).
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
    private func show(typed: String, _ words: [String], limit: Int = 10) {
        let result = SuggestionResult(
            typed: typed,
            candidates: [Suggestion(typed, kind: .verbatim)] + words.map { Suggestion($0, kind: .completion) }
        )
        bar.showSuggestions(SuggestionBar(result, limit: limit))
        window.layoutIfNeeded()
    }

    /// The scroll view the words are in.
    private var scroll: UIScrollView {
        var found: [UIScrollView] = []
        func search(_ view: UIView) {
            if let scroll = view as? UIScrollView, !slots(in: scroll).isEmpty { found.append(scroll) }
            view.subviews.forEach(search)
        }
        search(bar)
        return found[0]
    }

    /// The bar's suggestion buttons, left to right on screen.
    private func slots(in view: UIView) -> [UIButton] {
        var buttons: [UIButton] = []
        func search(_ view: UIView) {
            if let button = view as? UIButton, !button.isHidden, button.title(for: .normal)?.isEmpty == false {
                buttons.append(button)
            }
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

    private func isBold(_ button: UIButton) -> Bool {
        button.titleLabel?.font.fontDescriptor.symbolicTraits.contains(.traitBold) == true
    }

    /// The whole title is drawn: the button is at least as wide as its text at its font.
    private func showsWholeWord(_ button: UIButton) -> Bool {
        guard let label = button.titleLabel, let text = button.title(for: .normal), let font = label.font else { return false }
        let width = (text as NSString).size(withAttributes: [.font: font]).width
        return button.bounds.width >= width.rounded(.up)
    }

    private var hint: CALayer { bar.moreHintView.layer }

    private var hintShows: Bool { !bar.moreHintView.isHidden && bar.moreHintView.alpha > 0 }

    private let heWords = ["hello", "her", "here", "help", "hey", "heat", "head", "hear", "heavy"]

    func testShortWordsFitManyAndTheDefaultLeadsInBold() throws {
        show(typed: "he", ["hey", "her", "hen", "hem"])
        XCTAssertEqual(titles(), ["he", "hey", "her", "hen", "hem"], "the default first, then the engine's order")
        let buttons = slots(in: scroll)
        XCTAssertTrue(buttons.allSatisfy(inView), "all five fit")
        XCTAssertTrue(buttons.allSatisfy(showsWholeWord))
        XCTAssertTrue(isBold(buttons[0]), "the default is bold")
        XCTAssertFalse(buttons.dropFirst().contains(where: isBold))
        XCTAssertEqual(frame(of: buttons[0]).minX, 0, accuracy: 0.5, "at the leading edge")
        XCTAssertEqual(frame(of: try XCTUnwrap(buttons.last)).maxX, bar.bounds.width, accuracy: 0.5, "filling the bar")
        XCTAssertEqual(scroll.contentSize.width, bar.bounds.width, accuracy: 0.5, "nothing to slide to")
        XCTAssertFalse(hintShows, "nothing more to hint at")

        buttons[0].sendActions(for: .touchUpInside)
        buttons[2].sendActions(for: .touchUpInside)
        XCTAssertEqual(tapped.map(\.suggestion.text), ["he", "her"])
    }

    func testAsManyAsFitThenTheRestAreReachedBySlidingTheBar() throws {
        show(typed: "he", heWords)
        let buttons = slots(in: scroll)
        XCTAssertEqual(titles(), ["he"] + heWords)
        let shown = buttons.filter(inView)
        XCTAssertGreaterThanOrEqual(shown.count, 4)
        XCTAssertLessThan(shown.count, buttons.count)
        XCTAssertEqual(shown, Array(buttons.prefix(shown.count)), "the first ones")
        XCTAssertTrue(buttons.allSatisfy(showsWholeWord), "no word is cut")
        for button in buttons {
            XCTAssertGreaterThanOrEqual(button.bounds.width, 44)
            XCTAssertEqual(button.bounds.height, bar.bounds.height, accuracy: 0.5)
        }
        // The rest start past the trailing edge; the words in view are clear of the hint.
        for button in buttons.dropFirst(shown.count) {
            XCTAssertGreaterThanOrEqual(frame(of: button).minX, bar.bounds.width, button.title(for: .normal) ?? "")
        }
        XCTAssertTrue(hintShows)
        XCTAssertLessThanOrEqual(frame(of: try XCTUnwrap(shown.last)).maxX, bar.moreHintView.frame.minX + 0.5)

        // The bar slides sideways as far as the last word, and no further.
        XCTAssertTrue(scroll.isScrollEnabled)
        XCTAssertEqual(scroll.contentSize.height, bar.bounds.height, accuracy: 0.5, "sideways only")
        let last = try XCTUnwrap(buttons.last)
        XCTAssertEqual(scroll.contentSize.width, last.convert(last.bounds, to: scroll).maxX, accuracy: 0.5)
        // A slide that starts on a word slides the bar rather than waiting or committing it.
        XCTAssertFalse(scroll.delaysContentTouches)
        XCTAssertTrue(scroll.touchesShouldCancel(in: buttons[1]))

        // Slid to its end, the last words are in view and the first are not.
        scroll.setContentOffset(CGPoint(x: scroll.contentSize.width - scroll.bounds.width, y: 0), animated: false)
        window.layoutIfNeeded()
        XCTAssertTrue(inView(last))
        XCTAssertFalse(inView(buttons[0]))
        XCTAssertEqual(frame(of: last).maxX, bar.bounds.width, accuracy: 0.5)
        XCTAssertFalse(hintShows, "once slid, the hint has done its job")

        last.sendActions(for: .touchUpInside)
        XCTAssertEqual(tapped.map(\.suggestion.text), ["heavy"])
    }

    func testTheHintPulsesForNewSuggestionsThenStaysStill() throws {
        show(typed: "he", heWords)
        XCTAssertTrue(hintShows)
        // On the trailing edge.
        XCTAssertEqual(bar.moreHintView.frame.maxX, bar.bounds.width, accuracy: 0.5)
        let keys = try XCTUnwrap(hint.animationKeys())
        XCTAssertEqual(keys.count, 1, "one short animation, on one layer")
        let pulse = try XCTUnwrap(hint.animation(forKey: keys[0]))
        XCTAssertFalse(pulse.repeatCount.isInfinite, "no endless loop")
        XCTAssertLessThanOrEqual(pulse.duration * Double(max(pulse.repeatCount, 1)), 3)

        // The same suggestions again (a redraw for the same word) don't pulse it again.
        hint.removeAllAnimations()
        show(typed: "he", heWords)
        XCTAssertNil(hint.animationKeys())
        XCTAssertTrue(hintShows, "still there, and still")

        // New ones do.
        show(typed: "hea", ["heat", "head", "hear", "heavy", "heart", "heaven", "heap", "heated"])
        XCTAssertNotNil(hint.animationKeys())

        // Suggestions that all fit take it away.
        show(typed: "heav", ["heavy"])
        XCTAssertFalse(hintShows)
    }

    func testNoHintWhenTheSettingIsOffButTheBarStillSlides() {
        bar.moreHintEnabled = false
        show(typed: "he", heWords)
        XCTAssertFalse(hintShows)
        XCTAssertNil(hint.animationKeys())
        XCTAssertGreaterThan(scroll.contentSize.width, bar.bounds.width, "overflow still scrolls")
        XCTAssertLessThan(slots(in: scroll).filter(inView).count, slots(in: scroll).count)

        bar.moreHintEnabled = true
        show(typed: "he", heWords)
        XCTAssertTrue(hintShows, "on again, it shows for what's there")
    }

    func testAFirstWordWiderThanTheBarShrinksRatherThanIsCut() throws {
        let long = "pneumonoultramicroscopicsilicovolcanoconiosis"
        show(typed: long, ["pneumonia"])
        let first = try XCTUnwrap(slots(in: scroll).first)
        XCTAssertEqual(first.title(for: .normal), long)
        XCTAssertTrue(inView(first))
        XCTAssertLessThan(try XCTUnwrap(first.titleLabel?.font.pointSize), 17, "smaller")
        XCTAssertTrue(showsWholeWord(first), "but whole")
    }

    func testNewSuggestionsStartTheBarFromItsBeginning() {
        show(typed: "he", heWords)
        scroll.setContentOffset(CGPoint(x: 120, y: 0), animated: false)

        // The same suggestions again (a redraw for the same word) leave the bar where it is.
        show(typed: "he", heWords)
        XCTAssertEqual(scroll.contentOffset.x, 120)

        // The next key's suggestions start from the beginning.
        show(typed: "hel", ["hello", "help", "held", "helmet", "helps", "helped", "helpful", "helping"])
        XCTAssertEqual(scroll.contentOffset.x, 0)
        XCTAssertEqual(titles().first, "hel")

        // And when all fit the bar is its own width again.
        scroll.setContentOffset(CGPoint(x: 80, y: 0), animated: false)
        show(typed: "helm", ["helmet"])
        XCTAssertEqual(scroll.contentOffset.x, 0)
        XCTAssertEqual(scroll.contentSize.width, bar.bounds.width, accuracy: 0.5)
        XCTAssertEqual(titles(), ["helm", "helmet"])
    }

    /// A swipe's bar is the same view with the swipe's words (`BarContent.swipe`).
    func testASwipesWordsAreShownTheSameWay() {
        let found = ["put", "pit", "out", "pot", "pitt", "poet", "pout", "putt", "pity", "outer", "pottery", "putting"]
            .map { Suggestion($0, kind: .completion) }
        bar.showSuggestions(SuggestionBar(SuggestionResult(typed: "", candidates: found), limit: 20))
        window.layoutIfNeeded()
        XCTAssertEqual(titles().first, "put", "the swipe's word first")
        let scroll = self.scroll
        XCTAssertGreaterThan(scroll.contentSize.width, bar.bounds.width)

        // "Swipe again" takes the words' place and leaves nothing to slide to.
        bar.showMessage("Swipe again")
        window.layoutIfNeeded()
        XCTAssertEqual(scroll.contentSize.width, bar.bounds.width, accuracy: 0.5)
        XCTAssertFalse(hintShows)
    }

    func testLongMalayalamWordsFitFew() {
        show(typed: "ente", ["എന്റേതായിരുന്നു", "എന്തിനാണെന്നറിയില്ല", "എന്നെക്കുറിച്ചുള്ള", "എന്നിരുന്നാലും", "എന്റെ"])
        let buttons = slots(in: scroll)
        let shown = buttons.filter(inView).count
        XCTAssertGreaterThanOrEqual(shown, 2)
        XCTAssertLessThanOrEqual(shown, 3)
        XCTAssertTrue(buttons.allSatisfy(showsWholeWord))
        XCTAssertTrue(hintShows)
    }

    /// A word's width is measured once: the same words again are laid out from the cache.
    func testWordsAreMeasuredOnce() {
        show(typed: "he", ["hello", "her", "here"])
        let measured = bar.measuredWords
        show(typed: "her", ["hello", "here"])
        XCTAssertEqual(bar.measuredWords, measured + 1, "only the new default, in bold, is measured")
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
            Suggestion("میرٹھ", kind: .manglish), Suggestion("میراث", kind: .manglish),
            Suggestion("میرپور", kind: .manglish), Suggestion("میرزا", kind: .manglish), Suggestion("میراثی", kind: .manglish),
        ])
        bar.showSuggestions(SuggestionBar(result, limit: limit))
        window.layoutIfNeeded()
    }

    func testARightToLeftBarHasTheDefaultOnTheRight() throws {
        bar.setScript(urdu)
        showRoman()
        // Left to right on screen: the typed word, the other word, the default on the right.
        XCTAssertEqual(titles(), ["\u{201C}mera\u{201D}", "میری", "میرا"])
        let best = try XCTUnwrap(slots(in: scroll).last)
        XCTAssertTrue(slots(in: scroll).allSatisfy(inView))
        XCTAssertEqual(frame(of: best).maxX, bar.bounds.width, accuracy: 0.5)
        XCTAssertTrue(isBold(best), "the default is bold")
        best.sendActions(for: .touchUpInside)
        XCTAssertEqual(tapped.map(\.suggestion.text), ["میرا"])
    }

    func testARightToLeftBarSlidesLeftToItsMoreWords() throws {
        bar.setScript(urdu)
        showRoman(limit: 10)
        XCTAssertEqual(titles(), ["میراثی", "میرزا", "میرپور", "میراث", "میرٹھ", "میرے", "میر", "\u{201C}mera\u{201D}", "میری", "میرا"])
        let buttons = slots(in: scroll)
        let shown = buttons.filter(inView)
        XCTAssertGreaterThanOrEqual(shown.count, 2)
        XCTAssertLessThan(shown.count, buttons.count)
        XCTAssertEqual(shown, Array(buttons.suffix(shown.count)), "the first ones, from the right")
        XCTAssertEqual(frame(of: try XCTUnwrap(buttons.last)).maxX, bar.bounds.width, accuracy: 0.5)
        // The hint is at the trailing edge, here the left.
        XCTAssertTrue(hintShows)
        XCTAssertEqual(bar.moreHintView.frame.minX, 0, accuracy: 0.5)
        XCTAssertGreaterThanOrEqual(frame(of: try XCTUnwrap(shown.first)).minX, bar.moreHintView.frame.maxX - 0.5)

        // Slid right, the words past the left edge come into view.
        scroll.setContentOffset(.zero, animated: false)
        window.layoutIfNeeded()
        XCTAssertTrue(inView(buttons[0]))
        XCTAssertFalse(hintShows)
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
                // The default in iOS's own Bold face, so it stands out; the others Regular.
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
        XCTAssertEqual(titles(), ["میرا", "میری", "\u{201C}mera\u{201D}"])
        XCTAssertTrue(slots(in: scroll).allSatisfy { !($0.titleLabel?.font.familyName.contains("Nastaliq") ?? true) })
    }
}

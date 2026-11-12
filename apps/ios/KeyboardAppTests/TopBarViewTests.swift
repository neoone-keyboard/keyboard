import XCTest
import KeyboardCore

/// The keyboard's suggestion bar as a view (KeyboardExtension/TopBarView.swift, compiled
/// into this test bundle and put in a window): three equal slots, the default in the middle
/// and bold, and any more past the trailing edge in slots as wide, where sliding the bar
/// reaches them and a small accent mark hints at them. A real finger's slide is in
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

    /// A third of the bar, less the two gaps between the three.
    private var slotWidth: CGFloat { (bar.bounds.width - 6) / 3 }

    private var hint: CALayer { bar.moreHintView.layer }

    private var hintShows: Bool { !bar.moreHintView.isHidden && bar.moreHintView.alpha > 0 }

    private let heWords = ["hello", "her", "here", "help", "hey", "heat", "head", "hear", "heavy"]

    func testThreeEqualSlotsWithTheDefaultInTheMiddleInBold() throws {
        show(typed: "he", ["hello", "her"])
        XCTAssertEqual(titles(), ["hello", "he", "her"], "left, the default, right")
        let buttons = slots(in: scroll)
        XCTAssertTrue(buttons.allSatisfy(inView))
        for button in buttons {
            XCTAssertEqual(button.bounds.width, slotWidth, accuracy: 0.5, "equal thirds")
            XCTAssertEqual(button.bounds.height, bar.bounds.height, accuracy: 0.5)
        }
        XCTAssertEqual(frame(of: buttons[0]).minX, 0, accuracy: 0.5)
        XCTAssertEqual(frame(of: buttons[2]).maxX, bar.bounds.width, accuracy: 0.5)
        XCTAssertTrue(isBold(buttons[1]), "the default is bold")
        XCTAssertFalse(isBold(buttons[0]) || isBold(buttons[2]))
        XCTAssertEqual(scroll.contentSize.width, bar.bounds.width, accuracy: 0.5, "nothing to slide to")
        XCTAssertFalse(hintShows, "nothing more to hint at")

        buttons[1].sendActions(for: .touchUpInside)
        buttons[2].sendActions(for: .touchUpInside)
        XCTAssertEqual(tapped.map(\.suggestion.text), ["he", "her"])
    }

    func testOneSuggestionStaysInTheMiddle() throws {
        show(typed: "he", [])
        let only = try XCTUnwrap(slots(in: scroll).first)
        XCTAssertEqual(titles(), ["he"])
        XCTAssertEqual(frame(of: only).midX, bar.bounds.midX, accuracy: 0.5)
        XCTAssertEqual(only.bounds.width, slotWidth, accuracy: 0.5)
    }

    func testSuggestionsPastThreeAreReachedBySlidingTheBar() throws {
        show(typed: "he", heWords)
        let buttons = slots(in: scroll)
        XCTAssertEqual(titles(), ["hello", "he", "her", "here", "help", "hey", "heat", "head", "hear", "heavy"])
        let shown = buttons.filter(inView)
        XCTAssertEqual(shown, Array(buttons.prefix(3)), "the three slots, and only them")
        for button in buttons {
            XCTAssertEqual(button.bounds.width, slotWidth, accuracy: 0.5, "every slot as wide as the three")
        }
        // The rest start past the trailing edge.
        for button in buttons.dropFirst(3) {
            XCTAssertGreaterThanOrEqual(frame(of: button).minX, bar.bounds.width, button.title(for: .normal) ?? "")
        }
        XCTAssertTrue(hintShows)
        // The third slot's word is kept clear of the hint.
        let third = buttons[2]
        let label = try XCTUnwrap(third.titleLabel)
        XCTAssertLessThanOrEqual(label.convert(label.bounds, to: bar).maxX, bar.moreHintView.frame.minX + 0.5)

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

        // Three or fewer take it away.
        show(typed: "heav", ["heavy", "heaven"])
        XCTAssertFalse(hintShows)
    }

    func testNoHintWhenTheSettingIsOffButTheBarStillSlides() {
        bar.moreHintEnabled = false
        show(typed: "he", heWords)
        XCTAssertFalse(hintShows)
        XCTAssertNil(hint.animationKeys())
        XCTAssertGreaterThan(scroll.contentSize.width, bar.bounds.width, "overflow still scrolls")
        XCTAssertEqual(slots(in: scroll).filter(inView).count, 3)

        bar.moreHintEnabled = true
        show(typed: "he", heWords)
        XCTAssertTrue(hintShows, "on again, it shows for what's there")
    }

    func testALongWordShrinksIntoItsSlot() throws {
        let long = "pneumonoultramicroscopic"
        show(typed: long, ["pneumonia"])
        let middle = try XCTUnwrap(slots(in: scroll).first { $0.title(for: .normal) == long })
        XCTAssertTrue(inView(middle))
        XCTAssertEqual(middle.bounds.width, slotWidth, accuracy: 0.5, "the slot keeps its width")
        let label = try XCTUnwrap(middle.titleLabel)
        XCTAssertLessThanOrEqual(label.frame.width, middle.bounds.width)
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
        XCTAssertEqual(titles().prefix(3), ["hello", "hel", "help"])

        // And with three the bar is its own width again.
        scroll.setContentOffset(CGPoint(x: 80, y: 0), animated: false)
        show(typed: "helm", ["helmet", "helms"])
        XCTAssertEqual(scroll.contentOffset.x, 0)
        XCTAssertEqual(scroll.contentSize.width, bar.bounds.width, accuracy: 0.5)
        XCTAssertEqual(titles(), ["helmet", "helm", "helms"])
    }

    /// A swipe's bar is the same view with the swipe's words (`BarContent.swipe`).
    func testASwipesWordsPastThreeAreReachedTheSameWay() {
        let found = ["put", "pit", "out", "pot", "pitt", "poet", "pout", "putt", "pity", "outer", "pottery", "putting"]
            .map { Suggestion($0, kind: .completion) }
        bar.showSuggestions(SuggestionBar(SuggestionResult(typed: "", candidates: found), limit: 20))
        window.layoutIfNeeded()
        XCTAssertEqual(titles().prefix(3), ["pit", "put", "out"], "the swipe's word in the middle")
        let scroll = self.scroll
        XCTAssertGreaterThan(scroll.contentSize.width, bar.bounds.width)

        // "Swipe again" takes the words' place and leaves nothing to slide to.
        bar.showMessage("Swipe again")
        window.layoutIfNeeded()
        XCTAssertEqual(scroll.contentSize.width, bar.bounds.width, accuracy: 0.5)
        XCTAssertFalse(hintShows)
    }

    func testAnEmojiIsInTheRightSlot() throws {
        let result = SuggestionResult(typed: "pizza", candidates: [
            Suggestion("pizza", kind: .verbatim), Suggestion("pizzas", kind: .completion), Suggestion("pizzeria", kind: .completion),
        ])
        bar.showSuggestions(SuggestionBar(result, limit: 10).withEmoji("🍕"))
        window.layoutIfNeeded()
        XCTAssertEqual(titles(), ["pizzas", "pizza", "🍕", "pizzeria"])
        let emoji = slots(in: scroll)[2]
        XCTAssertEqual(try XCTUnwrap(emoji.titleLabel?.font.pointSize), 24, "drawn larger")
        XCTAssertNil(emoji.accessibilityCustomActions, "no word to remove")
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

    /// On the Latin keys (Roman Urdu) the bar reads left to right, as SwiftKey's does: the
    /// typed word on the left, the default bold in the middle, more past the right edge. Each
    /// Urdu word is still Nastaliq.
    func testUrduOnTheLatinKeysIsLeftToRight() throws {
        bar.setScript(urdu)
        showRoman(limit: 10)
        XCTAssertEqual(Array(titles().prefix(3)), ["\u{201C}mera\u{201D}", "میرا", "میری"])
        let buttons = slots(in: scroll)
        XCTAssertEqual(buttons.filter(inView), Array(buttons.prefix(3)), "the three slots, from the left")
        XCTAssertEqual(frame(of: buttons[0]).minX, 0, accuracy: 0.5)
        let best = buttons[1]
        XCTAssertEqual(frame(of: best).midX, bar.bounds.midX, accuracy: 0.5)
        XCTAssertTrue(isBold(best), "the default is bold")
        XCTAssertEqual(best.titleLabel?.font.familyName, "Noto Nastaliq Urdu")
        XCTAssertTrue(hintShows)
        XCTAssertEqual(bar.moreHintView.frame.maxX, bar.bounds.width, accuracy: 0.5, "the hint on the right")
    }

    func testARightToLeftBarIsMirroredWithTheDefaultInTheMiddle() throws {
        bar.setScript(urdu, rightToLeft: true)
        showRoman()
        // Left to right on screen: the other word, the default, the typed word on the right.
        XCTAssertEqual(titles(), ["میری", "میرا", "\u{201C}mera\u{201D}"])
        let buttons = slots(in: scroll)
        XCTAssertTrue(buttons.allSatisfy(inView))
        let best = buttons[1]
        XCTAssertEqual(frame(of: best).midX, bar.bounds.midX, accuracy: 0.5)
        XCTAssertTrue(isBold(best), "the default is bold")
        best.sendActions(for: .touchUpInside)
        XCTAssertEqual(tapped.map(\.suggestion.text), ["میرا"])
    }

    func testARightToLeftBarSlidesLeftToItsMoreWords() throws {
        bar.setScript(urdu, rightToLeft: true)
        showRoman(limit: 10)
        XCTAssertEqual(titles(), ["میراثی", "میرزا", "میرپور", "میراث", "میرٹھ", "میرے", "میر", "میری", "میرا", "\u{201C}mera\u{201D}"])
        let buttons = slots(in: scroll)
        let shown = buttons.filter(inView)
        XCTAssertEqual(shown, Array(buttons.suffix(3)), "the three slots, from the right")
        XCTAssertTrue(buttons.allSatisfy { abs($0.bounds.width - slotWidth) < 0.5 })
        XCTAssertEqual(frame(of: try XCTUnwrap(buttons.last)).maxX, bar.bounds.width, accuracy: 0.5)
        // The hint is at the trailing edge, here the left, and the third slot's word clear of it.
        XCTAssertTrue(hintShows)
        XCTAssertEqual(bar.moreHintView.frame.minX, 0, accuracy: 0.5)
        let label = try XCTUnwrap(shown[0].titleLabel)
        XCTAssertGreaterThanOrEqual(label.convert(label.bounds, to: bar).minX, bar.moreHintView.frame.maxX - 0.5)

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

    func testLeftToRightAgainOnTheLatinKeys() {
        bar.setScript(urdu, rightToLeft: true)
        showRoman()
        bar.setScript(urdu)
        showRoman()
        XCTAssertEqual(titles(), ["\u{201C}mera\u{201D}", "میرا", "میری"])
        XCTAssertEqual(slots(in: scroll)[1].titleLabel?.font.familyName, "Noto Nastaliq Urdu", "still Urdu's font")
    }

    func testLeftToRightAgainOnceThePackIsGone() {
        bar.setScript(urdu, rightToLeft: true)
        showRoman()
        bar.setScript(nil)
        showRoman()
        XCTAssertEqual(titles(), ["\u{201C}mera\u{201D}", "میرا", "میری"])
        XCTAssertTrue(slots(in: scroll).allSatisfy { !($0.titleLabel?.font.familyName.contains("Nastaliq") ?? true) })
    }
}

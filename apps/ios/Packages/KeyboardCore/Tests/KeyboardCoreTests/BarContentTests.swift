import Testing
import KeyboardCore

/// What the view controller puts in the bar (`KeyboardViewController.refreshTopBar`): the
/// engine's result for the word before the cursor, a swipe's words, a pack's vowel signs, or
/// nothing new while a result is on its way.

private func words(_ texts: [String], kind: SuggestionKind = .completion) -> [Suggestion] {
    texts.map { Suggestion($0, kind: kind) }
}

private func result(for typed: String, _ texts: [String], vowelSigns: [String] = []) -> SuggestionResult {
    SuggestionResult(typed: typed, candidates: [Suggestion(typed, kind: .verbatim)] + words(texts), vowelSigns: vowelSigns)
}

/// No pack's strip: English, or a cursor that isn't after a consonant.
private let noBase: @Sendable (PackDescriptor) -> String? = { _ in nil }

private func titles(_ bar: SuggestionBar) -> [String] {
    bar.slots.map { $0?.title ?? "" } + bar.more.map(\.title)
}

@Test func aTypedWordsBarHoldsAsManySuggestionsAsTheSettingSays() {
    var state = BarState()
    state.word = "he"
    state.latest = result(for: "he", ["hello", "her", "here", "help", "hey", "heat", "head", "hear", "heavy", "held", "hero"])
    for (limit, shown) in [(3, 3), (10, 10), (20, 12)] {
        state.limit = limit
        guard case .suggestions(let bar, let hasResult) = state.content(markBase: noBase) else {
            Issue.record("limit \(limit): not the slots")
            continue
        }
        #expect(hasResult)
        #expect(titles(bar).count == shown, "limit \(limit)")
        #expect(titles(bar).prefix(3) == ["hello", "he", "her"], "the typed word in the middle, the rest in the engine's order")
        #expect(bar.more.map(\.title) == Array(["here", "help", "hey", "heat", "head", "hear", "heavy", "held", "hero"].prefix(shown - 3)))
    }
}

/// The bug of d812939: after a swipe the bar had the word and two alternatives only.
@MainActor
@Test func aSwipesBarHoldsEveryWordUpToTheSetting() {
    let output = RecordingOutput()
    let input = InputController(output: output)
    let found = words(["hello", "help", "held", "hells", "hero", "helm", "hell"])
    #expect(input.insertSwipe(found))

    var state = BarState()
    state.word = input.currentWord
    state.swipe = input.showsSwipe ? input.suggestions : nil
    // What the engine last said is for another word; the swipe's words stay until the next key.
    state.latest = result(for: "x", ["xylophone"])
    state.awaitingResult = true
    state.limit = 10
    guard case .swipe(let bar) = state.content(markBase: noBase) else {
        Issue.record("not the swipe's bar")
        return
    }
    #expect(titles(bar) == ["help", "hello", "held", "hells", "hero", "helm", "hell"])
    #expect(bar.more.count == 4)
    #expect(BarContent.swipe(bar).isForCurrentWord, "so a tap on any of them is taken")

    state.limit = 3
    #expect(state.content(markBase: noBase) == .swipe(SuggestionBar(input.suggestions, limit: 3)))
    state.limit = 5
    guard case .swipe(let five) = state.content(markBase: noBase) else { return }
    #expect(five.more.map(\.title) == ["hells", "hero"])

    // The next key ends it: the bar is the typed word's again.
    input.handle(.character("a"), at: 0)
    #expect(!input.showsSwipe)
}

@Test func aSwipeWithoutAWordSaysSwipeAgain() {
    var state = BarState()
    state.swipeFailed = true
    state.word = "he"
    state.latest = result(for: "he", ["hello"])
    state.swipe = result(for: "", ["old"])
    let content = state.content(markBase: noBase)
    #expect(content == .message("Swipe again"))
    #expect(!content.isForCurrentWord, "the slots under it are still another word's")
}

@Test func onlyAResultForTheWordBeforeTheCursorIsShown() {
    var state = BarState()
    state.limit = 10
    state.word = "hel"
    state.latest = result(for: "he", ["hello", "her"])
    state.barWord = "he"

    // The result for "hel" is on its way and the bar is another word's: it empties.
    state.awaitingResult = true
    #expect(state.content(markBase: noBase) == .suggestions(SuggestionBar(.empty, limit: 10), hasResult: false))

    // Its timeout arrived: empty as well.
    state.awaitingResult = false
    #expect(state.content(markBase: noBase) == .suggestions(SuggestionBar(.empty, limit: 10), hasResult: false))

    // The result arrived.
    state.latest = result(for: "hel", ["hello", "help"])
    guard case .suggestions(let bar, true) = state.content(markBase: noBase) else {
        Issue.record("not the result's slots")
        return
    }
    #expect(titles(bar) == ["hello", "hel", "help"])
}

/// A second redraw for the same word while its result is on its way (after the key, then
/// after `textDidChange`) leaves the bar alone, so it isn't drawn twice per key.
@Test func theBarStaysWhileItsWordsResultIsOnItsWay() {
    var state = BarState()
    state.word = "hel"
    state.latest = result(for: "he", ["hello"])
    state.barWord = "hel"
    state.awaitingResult = true
    #expect(state.content(markBase: noBase) == .unchanged)
    #expect(!BarContent.unchanged.isForCurrentWord)

    // Not while the vowel strip shows: the slots take its place at once.
    state.showsVowelSigns = true
    #expect(state.content(markBase: noBase) == .suggestions(SuggestionBar(.empty), hasResult: false))
}

@Test func aCurlyApostropheIsTheSameWord() {
    var state = BarState()
    state.word = "don\u{2019}t"
    state.latest = result(for: "don't", ["don't"])
    #expect(state.straightWord == "don't")
    guard case .suggestions(_, let hasResult) = state.content(markBase: noBase) else { return }
    #expect(hasResult)
    state.barWord = "don't"
    state.latest = .empty
    state.awaitingResult = true
    #expect(state.content(markBase: noBase) == .unchanged)
}

@Test func afterAPacksConsonantTheBarIsItsVowelSigns() throws {
    let layout = try #require(malayalamPack.layout)
    let strip = layout.vowelSignStrip
    #expect(strip.count == 15)
    var asked: [String] = []
    let afterKa: (PackDescriptor) -> String? = { pack in
        asked.append(pack.id)
        return "ക"
    }
    var state = BarState()
    state.language = malayalam
    state.word = "ക"
    state.limit = 10

    // The engine's result: its signs first, the rest in the layout's order, and its best
    // completion beside them.
    state.latest = SuggestionResult(
        typed: "ക",
        candidates: [Suggestion("ക", kind: .verbatim), Suggestion("കഥ", kind: .completion), Suggestion("കടല", kind: .completion)],
        vowelSigns: [strip[4], strip[1]]
    )
    let content = state.content(markBase: afterKa)
    let expected = [strip[4], strip[1]] + strip.filter { $0 != strip[4] && $0 != strip[1] }
    #expect(content == .vowelSigns(base: "ക", signs: expected, completion: Suggestion("കഥ", kind: .completion)))
    #expect(content.isForCurrentWord)
    #expect(asked == ["ml"])

    // The next consonant, its result on the way: an older strip stays…
    state.word = "കമ"
    state.awaitingResult = true
    state.showsVowelSigns = true
    #expect(state.content(markBase: { _ in "മ" }) == .unchanged)
    // …but slots give way to a strip in the layout's order at once.
    state.showsVowelSigns = false
    #expect(state.content(markBase: { _ in "മ" }) == .vowelSigns(base: "മ", signs: strip, completion: nil))
    // And once the timeout has passed, the strip falls back to the layout's order.
    state.awaitingResult = false
    state.showsVowelSigns = true
    #expect(state.content(markBase: { _ in "മ" }) == .vowelSigns(base: "മ", signs: strip, completion: nil))
}

@Test func theVowelStripIsOnlyForAPackKeyboardAfterAConsonant() throws {
    var state = BarState()
    state.word = "ക"
    state.latest = SuggestionResult(typed: "ക", candidates: [Suggestion("കഥ", kind: .completion)], vowelSigns: ["ാ"])

    // On the English keys the engine isn't asked for a consonant at all.
    var asked = 0
    let counting: (PackDescriptor) -> String? = { _ in
        asked += 1
        return "ക"
    }
    state.language = .english
    guard case .suggestions = state.content(markBase: counting) else {
        Issue.record("English: not the slots")
        return
    }
    #expect(asked == 0)

    // On a pack's keys, not after a consonant: the slots.
    state.language = malayalam
    guard case .suggestions(let bar, true) = state.content(markBase: noBase) else {
        Issue.record("no consonant: not the slots")
        return
    }
    #expect(bar.middle?.title == "കഥ")

    // A pack whose layout has no strip never shows one.
    let plain = PackDescriptor(
        id: "xx", name: "X", nativeName: "X", script: "mlym",
        layout: PackLayout(spaceBar: "X", languageKey: "X", pages: try #require(malayalamPack.layout).pages),
        folder: malayalamPack.folder
    )
    state.language = .pack(plain)
    guard case .suggestions = state.content(markBase: counting) else {
        Issue.record("no strip: not the slots")
        return
    }
    #expect(asked == 0)

    // A swipe's words and "Swipe again" come before the strip.
    state.language = malayalam
    state.swipe = result(for: "", ["hello"])
    guard case .swipe = state.content(markBase: counting) else {
        Issue.record("a swipe: not its bar")
        return
    }
    #expect(asked == 0)
}

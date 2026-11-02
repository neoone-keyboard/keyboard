import Testing
import KeyboardCore

@Test func emptyResultGivesAnEmptyBar() {
    let bar = SuggestionBar(.empty)
    #expect(bar.slots == [nil, nil, nil])
}

@Test func typedWordInTheMiddleWithCompletionsBeside() {
    let bar = SuggestionBar(SuggestionResult(typed: "hel", candidates: [
        Suggestion("hel", kind: .verbatim), Suggestion("help", kind: .completion), Suggestion("hello", kind: .completion),
    ]))
    #expect(bar.middle?.title == "hel")
    #expect(bar.left?.title == "help")
    #expect(bar.right?.title == "hello")
}

@Test func correctionInTheMiddleQuotesTheTypedWordOnTheLeft() {
    let bar = SuggestionBar(SuggestionResult(typed: "teh", candidates: [
        Suggestion("the", kind: .correction), Suggestion("teh", kind: .verbatim), Suggestion("ten", kind: .correction),
    ]))
    #expect(bar.middle?.title == "the")
    #expect(bar.left?.title == "\u{201C}teh\u{201D}")
    #expect(bar.left?.quoted == true)
    #expect(bar.left?.suggestion == Suggestion("teh", kind: .verbatim))
    #expect(bar.right?.title == "ten")
}

@Test func nextWordsFillAllThreeSlots() {
    let bar = SuggestionBar(SuggestionResult(typed: "", candidates: [
        Suggestion("am", kind: .nextWord), Suggestion("have", kind: .nextWord), Suggestion("was", kind: .nextWord),
    ]))
    #expect(bar.slots.map { $0?.title } == ["have", "am", "was"])
}

@Test func manglishMiddleQuotesTheLatinWord() {
    let bar = SuggestionBar(SuggestionResult(typed: "ente", candidates: [
        Suggestion("എന്റെ", kind: .manglish), Suggestion("ente", kind: .verbatim),
    ]))
    #expect(bar.middle?.title == "എന്റെ")
    #expect(bar.left?.title == "\u{201C}ente\u{201D}")
    #expect(bar.right == nil)
}

@Test func stripPutsTheLikeliestSignsFirst() {
    let ordered = Malayalam.orderedStripSigns(preferred: ["\u{0D41}", "\u{0D3F}"])
    #expect(ordered.prefix(2) == ["\u{0D41}", "\u{0D3F}"])
    #expect(ordered.count == Malayalam.stripSigns.count)
    #expect(Set(ordered) == Set(Malayalam.stripSigns))
    #expect(Array(ordered.dropFirst(2)) == Malayalam.stripSigns.filter { $0 != "\u{0D41}" && $0 != "\u{0D3F}" })
}

@Test func stripIgnoresUnknownAndRepeatedSigns() {
    #expect(Malayalam.orderedStripSigns(preferred: ["x", "\u{0D3E}", "\u{0D3E}"]) == Malayalam.stripSigns)
    #expect(Malayalam.orderedStripSigns(preferred: []) == Malayalam.stripSigns)
}

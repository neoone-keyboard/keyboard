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

@Test func stripPutsTheLikeliestSignsFirst() throws {
    let layout = try #require(malayalamPack.layout)
    let ordered = layout.orderedStripSigns(preferred: ["\u{0D41}", "\u{0D3F}"])
    #expect(ordered.prefix(2) == ["\u{0D41}", "\u{0D3F}"])
    #expect(ordered.count == layout.vowelSignStrip.count)
    #expect(Set(ordered) == Set(layout.vowelSignStrip))
    #expect(Array(ordered.dropFirst(2)) == layout.vowelSignStrip.filter { $0 != "\u{0D41}" && $0 != "\u{0D3F}" })
}

@Test func stripIgnoresUnknownAndRepeatedSigns() throws {
    let layout = try #require(malayalamPack.layout)
    #expect(layout.orderedStripSigns(preferred: ["x", "\u{0D3E}", "\u{0D3E}"]) == layout.vowelSignStrip)
    #expect(layout.orderedStripSigns(preferred: []) == layout.vowelSignStrip)
}

@Test func holdingASlotAsksToRemoveItsWord() {
    let removal = SuggestionRemoval(slot: SuggestionSlot(suggestion: Suggestion("the", kind: .correction), quoted: false))
    #expect(removal?.word == "the")
    #expect(removal?.prompt == "Remove \u{201C}the\u{201D}?")
}

@Test func theQuotedTypedWordIsRemovedWithoutItsQuotes() {
    let removal = SuggestionRemoval(slot: SuggestionSlot(suggestion: Suggestion("teh", kind: .verbatim), quoted: true))
    #expect(removal?.word == "teh")
    #expect(removal?.prompt == "Remove \u{201C}teh\u{201D}?")
}

@Test func aSlotWithNoWordCannotBeRemoved() {
    #expect(SuggestionRemoval(slot: SuggestionSlot(suggestion: Suggestion("", kind: .verbatim), quoted: false)) == nil)
}

@Test func removalTimings() {
    #expect(SuggestionRemoval.holdDuration == 0.5)
    #expect(SuggestionRemoval.timeout == .seconds(4))
}

// MARK: More than three

private func words(_ texts: [String], first: SuggestionKind = .verbatim, rest: SuggestionKind = .completion) -> [Suggestion] {
    texts.enumerated().map { index, text in Suggestion(text, kind: index == 0 ? first : rest) }
}

@Test func theFirstThreeSlotsDoNotDependOnTheLimit() {
    let results = [
        SuggestionResult(typed: "he", candidates: words(["he", "help", "hello", "her", "here", "hey"])),
        SuggestionResult(typed: "teh", candidates: [Suggestion("the", kind: .correction)]
            + words(["teh", "ten", "tea", "tech"], rest: .correction)),
        SuggestionResult(typed: "", candidates: words(["am", "have", "was", "will"], first: .nextWord, rest: .nextWord)),
        SuggestionResult(typed: "ente", candidates: [Suggestion("എന്റെ", kind: .manglish), Suggestion("ente", kind: .verbatim)]),
        .empty,
    ]
    for result in results {
        let three = SuggestionBar(result)
        for limit in [10, 20] {
            let longer = SuggestionBar(result, limit: limit)
            #expect(longer.slots == three.slots)
            #expect(three.more.isEmpty)
        }
    }
}

@Test func candidatesAfterTheSlotsGoInMoreInOrder() {
    let bar = SuggestionBar(SuggestionResult(typed: "he", candidates: words(["he", "help", "hello", "her", "here", "hey"])), limit: 10)
    #expect(bar.slots.map { $0?.title } == ["help", "he", "hello"])
    #expect(bar.more.map(\.title) == ["her", "here", "hey"])
    #expect(bar.more.allSatisfy { !$0.quoted })
}

@Test func moreSkipsTheQuotedTypedWord() {
    let bar = SuggestionBar(SuggestionResult(typed: "teh", candidates: [Suggestion("the", kind: .correction)]
        + words(["teh", "ten", "tea", "tech"], rest: .correction)), limit: 10)
    #expect(bar.slots.map { $0?.title } == ["\u{201C}teh\u{201D}", "the", "ten"])
    #expect(bar.more.map(\.title) == ["tea", "tech"])
}

@Test func theLimitCountsTheSlots() {
    let texts = (0..<25).map { "w\($0)" }
    let bar = SuggestionBar(SuggestionResult(typed: "w", candidates: words(texts)), limit: 10)
    #expect(bar.more.count == 7)
    #expect(bar.more.last?.title == "w9")
    #expect(SuggestionBar(SuggestionResult(typed: "w", candidates: words(texts)), limit: 20).more.count == 17)
    // Below three is three.
    #expect(SuggestionBar(SuggestionResult(typed: "w", candidates: words(texts)), limit: 1).slots.compactMap { $0 }.count == 3)
}

@Test func anEmojiKeepsTheRightSlotAndTheWordItDisplacesLeadsMore() {
    let result = SuggestionResult(typed: "he", candidates: words(["he", "help", "hello", "her"]))
    let three = SuggestionBar(result).withEmoji("👋")
    #expect(three.right?.suggestion == Suggestion("👋", kind: .emoji))
    #expect(three.more.isEmpty)
    let ten = SuggestionBar(result, limit: 10).withEmoji("👋")
    #expect(ten.slots == three.slots)
    #expect(ten.more.map(\.title) == ["hello", "her"])
    // Still at most the limit.
    let full = SuggestionBar(SuggestionResult(typed: "w", candidates: words((0..<10).map { "w\($0)" })), limit: 10).withEmoji("👋")
    #expect(full.more.count == 7)
    #expect(full.more.first?.title == "w2")
    #expect(SuggestionBar(result, limit: 10).withEmoji(nil) == SuggestionBar(result, limit: 10))
}

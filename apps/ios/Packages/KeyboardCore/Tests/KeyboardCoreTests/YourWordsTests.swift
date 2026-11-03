import Testing
import KeyboardCore

@Test func yourWordsShowsEverythingUntilSearched() {
    let words = YourWords(learned: ["Thom", "Anu"], removed: ["hello"])
    #expect(words.query == "")
    #expect(words.learned == ["Thom", "Anu"])
    #expect(words.removed == ["hello"])
}

@Test func lettersAndBackspaceEditTheSearchAndFilterBothLists() {
    var words = YourWords(learned: ["Thom", "Anu", "Athira"], removed: ["hello", "that"])
    #expect(words.handle(.character("t")) == .edited)
    #expect(words.handle(.character("h")) == .edited)
    #expect(words.query == "th")
    #expect(words.learned == ["Thom", "Athira"], "anywhere in the word, ignoring case")
    #expect(words.removed == ["that"])
    #expect(words.handle(.backspace) == .edited)
    #expect(words.query == "t")
    #expect(words.handle(.backspace) == .edited)
    #expect(words.handle(.backspace) == .edited, "backspace on an empty search does nothing else")
    #expect(words.query == "")
    #expect(words.learned == ["Thom", "Anu", "Athira"])
}

@Test func keysThatChangeTheKeysStillWorkAndTheRestDoNothing() {
    var words = YourWords(learned: ["Thom"], removed: [])
    #expect(words.handle(.page(.symbols)) == .passThrough)
    #expect(words.handle(.toggleLanguage) == .passThrough)
    #expect(words.handle(.nextKeyboard) == .passThrough)
    // Words hold no spaces or lines, and the search ignores case.
    #expect(words.handle(.space) == .ignored)
    #expect(words.handle(.returnKey) == .ignored)
    #expect(words.handle(.shift) == .ignored)
    #expect(words.query == "")
}

@Test func searchMatchesMalayalamInsideASyllable() {
    var words = YourWords(learned: ["കിളി", "അമ്മ"], removed: [])
    words.type("ക")
    #expect(words.learned == ["കിളി"])
    words.type("ി")
    #expect(words.query == "കി")
    #expect(words.learned == ["കിളി"])
}

@Test func backspaceTakesOffOneScalarSoAVowelSignCanBeRetyped() {
    var words = YourWords(learned: ["കിളി"], removed: [])
    words.type("കി")
    _ = words.handle(.backspace)
    #expect(words.query == "ക")
}

@Test func clearingTheSearchShowsEverythingAgain() {
    var words = YourWords(learned: ["Thom", "Anu"], removed: [])
    words.type("an")
    #expect(words.learned == ["Anu"])
    words.clearQuery()
    #expect(words.query == "")
    #expect(words.learned == ["Thom", "Anu"])
}

@Test func theSearchIsNoLongerThanAWord() {
    var words = YourWords(learned: [], removed: [])
    for _ in 0..<60 { _ = words.handle(.character("a")) }
    #expect(words.query.unicodeScalars.count == YourWords.maxQueryLength)
}

@Test func forgettingAndRestoringTakeTheWordOffItsListEvenWhenSearched() {
    var words = YourWords(learned: ["Thom", "Thomas", "Anu"], removed: ["that", "hello"])
    words.type("th")
    words.forget("Thom")
    #expect(words.learned == ["Thomas"])
    words.restore("that")
    #expect(words.removed.isEmpty)
    words.clearQuery()
    #expect(words.learned == ["Thomas", "Anu"])
    #expect(words.removed == ["hello"])
}

@Test func emptyStatesSayWhetherNothingIsThereOrNothingMatches() {
    var words = YourWords(learned: [], removed: ["hello"])
    #expect(words.emptyLearnedMessage == "No learned words yet")
    #expect(words.emptyRemovedMessage == nil)
    words.type("x")
    #expect(words.emptyLearnedMessage == "No learned words yet")
    #expect(words.emptyRemovedMessage == "No removed words match")
    words = YourWords(learned: ["Thom"], removed: [])
    #expect(words.emptyRemovedMessage == "No removed words yet")
    words.type("x")
    #expect(words.emptyLearnedMessage == "No learned words match")
}

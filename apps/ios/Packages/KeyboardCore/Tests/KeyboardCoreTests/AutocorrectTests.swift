import Testing
import KeyboardCore

/// Space, punctuation and backspace around the suggestion bar's middle slot.
@MainActor
struct AutocorrectTests {
    let output = RecordingOutput()
    let input: InputController
    let events: EventLog

    @MainActor
    final class EventLog {
        var events: [LearningEvent] = []
    }

    init() {
        input = InputController(output: output)
        let log = EventLog()
        events = log
        input.onLearningEvent = { log.events.append($0) }
    }

    func corrected(_ typed: String, to replacement: String, kind: SuggestionKind = .correction) -> SuggestionResult {
        SuggestionResult(typed: typed, candidates: [Suggestion(replacement, kind: kind), Suggestion(typed, kind: .verbatim)])
    }

    @Test func spaceCommitsTheMiddleSuggestion() {
        output.text = "I said teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        input.handle(.space, at: 0)
        #expect(output.text == "I said the ")
        #expect(events.events.isEmpty)
    }

    @Test func spaceKeepsAVerbatimMiddleAndReportsTheWord() {
        output.text = "hello"
        input.updateSuggestions(SuggestionResult(typed: "hello", candidates: [Suggestion("hello", kind: .verbatim)]))
        input.handle(.space, at: 0)
        #expect(output.text == "hello ")
        #expect(events.events == [.committed("hello")])
    }

    @Test func aResultForAnotherWordIsNeverCommitted() {
        output.text = "teh"
        input.updateSuggestions(corrected("te", to: "the"))
        input.handle(.space, at: 0)
        #expect(output.text == "teh ")
    }

    @Test func manglishMiddleIsCommittedToo() {
        output.text = "ente"
        input.updateSuggestions(corrected("ente", to: "എന്റെ", kind: .manglish))
        input.handle(.space, at: 0)
        #expect(output.text == "എന്റെ ")
    }

    @Test func punctuationCommitsTheMiddleThenTypesItself() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        input.handle(.character("."), at: 0)
        #expect(output.text == "the.")
    }

    @Test func punctuationWithoutASuggestionJustTypes() {
        output.text = "hi"
        input.handle(.character("!"), at: 0)
        #expect(output.text == "hi!")
        #expect(events.events == [.committed("hi")])
    }

    @Test func backspaceRightAfterAReplacementRestoresAndLearns() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        input.handle(.space, at: 0)
        input.handle(.backspace, at: 0.1)
        #expect(output.text == "teh")
        #expect(events.events == [.learn("teh")])
        // The restored word is not replaced again, even by a stale result.
        input.updateSuggestions(corrected("teh", to: "the"))
        input.handle(.space, at: 0.2)
        #expect(output.text == "teh ")
    }

    @Test func backspaceUndoesPunctuationCommitsToo() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        input.handle(.character(","), at: 0)
        input.handle(.backspace, at: 0.1)
        #expect(output.text == "teh")
    }

    @Test func backspaceLaterIsAnOrdinaryDelete() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        input.handle(.space, at: 0)
        input.handle(.character("a"), at: 0.1)
        input.handle(.backspace, at: 0.2)
        #expect(output.text == "the ")
        #expect(events.events.isEmpty)
    }

    @Test func nextWordGuessesAreNotCommittedBySpace() {
        output.text = "I "
        input.updateSuggestions(SuggestionResult(typed: "", candidates: [Suggestion("am", kind: .nextWord)]))
        input.handle(.space, at: 0)
        #expect(output.text == "I  ")
    }

    @Test func nativeMalayalamIsNeverReplaced() {
        input.handle(.toggleLanguage, at: 0)
        output.text = "എന്റ"
        input.updateSuggestions(SuggestionResult(typed: "എന്റ", candidates: [Suggestion("എന്റെ", kind: .completion)]))
        input.handle(.space, at: 0.1)
        #expect(output.text == "എന്റ ")
        #expect(events.events == [.committed("എന്റ")])
    }

    @Test func tappingTheQuotedTypedWordKeepsAndLearnsIt() throws {
        output.text = "Thom"
        let bar = SuggestionBar(corrected("Thom", to: "them"))
        let left = try #require(bar.left)
        input.commitSuggestion(left)
        #expect(output.text == "Thom ")
        #expect(events.events == [.learn("Thom")])
    }

    @Test func tappingAnotherSlotDoesNotLearn() throws {
        output.text = "teh"
        let bar = SuggestionBar(corrected("teh", to: "the"))
        let middle = try #require(bar.middle)
        input.commitSuggestion(middle)
        #expect(output.text == "the ")
        #expect(events.events == [.committed("the")])
    }

    @Test func currentWordFollowsTheText() {
        output.text = "say hel"
        #expect(input.currentWord == "hel")
        output.text = "say "
        #expect(input.currentWord == "")
    }

    // MARK: Review fixes

    @Test func nothingIsReplacedWhenAutocorrectIsOff() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        input.autocorrectEnabled = false
        input.handle(.space, at: 0)
        #expect(output.text == "teh ")
    }

    @Test func nothingIsReplacedBeforePunctuationWhenAutocorrectIsOff() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        input.autocorrectEnabled = false
        input.handle(.character("."), at: 0)
        #expect(output.text == "teh.")
    }

    @Test func switchingLanguageDropsTheOldResult() {
        input.handle(.toggleLanguage, at: 0)
        output.text = "എന്റ"
        input.updateSuggestions(SuggestionResult(typed: "എന്റ", candidates: [Suggestion("എന്റെ", kind: .completion)]))
        input.handle(.toggleLanguage, at: 0.1)
        input.handle(.space, at: 0.2)
        #expect(output.text == "എന്റ ")
    }

    @Test func aPageChangeKeepsTheResultTheBarShows() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        input.handle(.page(.symbols), at: 0)
        #expect(input.suggestions == corrected("teh", to: "the"))
        input.handle(.character(","), at: 0)
        #expect(output.text == "the,")
    }

    @Test func completionsNeverReplaceOnSpace() {
        output.text = "cou"
        input.updateSuggestions(corrected("cou", to: "could", kind: .completion))
        input.handle(.space, at: 0)
        #expect(output.text == "cou ")
    }

    @Test func malayalamScriptIsNeverReplaced() {
        output.text = "എന്റ"
        input.updateSuggestions(corrected("എന്റ", to: "എന്റെ"))
        input.handle(.space, at: 0)
        #expect(output.text == "എന്റ ")
    }

    @Test func backspaceAfterACursorMoveIsAnOrdinaryDelete() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        input.handle(.space, at: 0)
        input.moveCursor(by: -1)
        input.handle(.backspace, at: 0.1)
        #expect(output.text == "the")
        #expect(events.events.isEmpty)
    }

    @Test func aCursorMoveDropsTheResult() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        input.moveCursor(by: -1)
        input.handle(.space, at: 0)
        #expect(output.text == "teh ")
    }

    @Test func externalTextChangesForgetTheReplacementAndResult() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        input.handle(.space, at: 0)
        input.textDidChangeExternally()
        input.handle(.backspace, at: 0.1)
        #expect(output.text == "the")
        #expect(events.events.isEmpty)
        input.updateSuggestions(corrected("teh", to: "the"))
        input.textDidChangeExternally()
        #expect(input.suggestions == .empty)
    }

    @Test func undoNeedsTheContextToEndWithWhatWeInserted() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        input.handle(.space, at: 0)
        output.text = "something else"   // changed behind our back
        input.handle(.backspace, at: 0.1)
        #expect(output.text == "something els")
        #expect(events.events.isEmpty)
    }

    @Test func dottedNamesAreNotCorrected() {
        output.text = "tom.smith"
        input.updateSuggestions(corrected("smith", to: "Smith"))
        input.handle(.space, at: 0)
        #expect(output.text == "tom.smith ")
    }

    @Test func urlPathsAreNotCorrected() {
        output.text = "github.com/example"
        input.updateSuggestions(corrected("jishnu", to: "Jishnu"))
        input.handle(.space, at: 0)
        #expect(output.text == "github.com/example ")
    }

    @Test func wordsAfterSchemeWwwEmailAndHashtagAreNotCorrected() {
        for prefix in ["https://", "www.", "me@", "a_", "#", "\\", "x:"] {
            output.text = prefix + "teh"
            input.updateSuggestions(corrected("teh", to: "the"))
            input.handle(.space, at: 0)
            #expect(output.text == prefix + "teh ")
        }
    }

    @Test func aKeptWordIsReplacedAgainOnceTheNextWordStarts() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        input.handle(.space, at: 0)
        input.handle(.backspace, at: 0.1)
        input.handle(.space, at: 0.2)
        #expect(output.text == "teh ")
        input.handle(.character("o"), at: 0.3)
        input.handle(.character("k"), at: 0.4)
        input.handle(.space, at: 0.5)
        input.handle(.character("t"), at: 0.6)
        input.handle(.character("e"), at: 0.7)
        input.handle(.character("h"), at: 0.8)
        input.updateSuggestions(corrected("teh", to: "the"))
        input.handle(.space, at: 0.9)
        #expect(output.text == "teh ok the ")
    }

    @Test func undoKeepsTheSeparatorOutAndRestoresTheWord() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        input.handle(.character(","), at: 0)
        #expect(output.text == "the,")
        input.handle(.backspace, at: 0.1)
        #expect(output.text == "teh")
    }

    @Test func committingWithSpaceLeavesShiftAlone() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        input.handle(.shift, at: 0)
        input.handle(.space, at: 0.1)
        input.handle(.character("a"), at: 0.2)
        #expect(output.text == "the A")
    }

    @Test func curlyApostropheMatchesTheEnginesStraightOne() {
        output.text = "I don\u{2019}"
        input.updateSuggestions(corrected("don'", to: "don't"))
        input.handle(.space, at: 0)
        #expect(output.text == "I don't ")
    }

    @Test func returnReportsTheCommittedWord() {
        output.text = "hello"
        input.handle(.returnKey, at: 0)
        #expect(events.events == [.committed("hello")])
    }

    @Test func tappingASuggestionReportsTheCommittedWord() throws {
        output.text = "cou"
        let bar = SuggestionBar(SuggestionResult(typed: "cou", candidates: [Suggestion("could", kind: .completion)]))
        input.commitSuggestion(try #require(bar.middle))
        #expect(output.text == "could ")
        #expect(events.events == [.committed("could")])
    }
}

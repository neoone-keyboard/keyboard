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
        #expect(events.events.isEmpty)
    }

    @Test func currentWordFollowsTheText() {
        output.text = "say hel"
        #expect(input.currentWord == "hel")
        output.text = "say "
        #expect(input.currentWord == "")
    }
}

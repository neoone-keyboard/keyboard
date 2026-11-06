import Testing
import KeyboardCore

/// A swiped word goes in whole with a space after it; until the next key, backspace takes it
/// back out and a bar slot swaps it for another reading of the same path.
extension InputControllerTests {
    func swipe(_ word: String, _ alternatives: String...) {
        input.insertSwipe(
            Suggestion(word, kind: .completion),
            alternatives: alternatives.map { Suggestion($0, kind: .completion) }
        )
    }

    func slot(_ text: String) -> SuggestionSlot {
        SuggestionSlot(suggestion: Suggestion(text, kind: .completion), quoted: false)
    }

    @Test func aSwipeTypesTheWordAndASpace() {
        swipe("hello")
        #expect(output.text == "hello ")
    }

    @Test func aSwipeAfterAWordStartsWithASpace() {
        output.text = "I said"
        swipe("hello")
        #expect(output.text == "I said hello ")
    }

    @Test func aSwipeAfterSpaceOrOpeningMarkAddsNoSpaceBefore() {
        for start in ["I ", "line\n", "(", "\""] {
            output.text = start
            swipe("hello")
            #expect(output.text == start + "hello ", "\(start)")
        }
    }

    @Test func swipesFollowShift() {
        type(.shift)
        swipe("hello")
        #expect(output.text == "Hello ")
        #expect(input.shift == .off)
        input.handle(.shift, at: 1)
        input.handle(.shift, at: 1.1)
        swipe("ok")
        #expect(output.text == "Hello OK ")
    }

    @Test func theBarShowsTheSwipedWordAndItsAlternatives() {
        swipe("hello", "help", "hell")
        #expect(input.suggestions.candidates.map(\.text) == ["hello", "help", "hell"])
        #expect(SuggestionBar(input.suggestions).middle?.suggestion.text == "hello")
    }

    @Test func backspaceStraightAfterASwipeDeletesTheWord() {
        output.text = "I said "
        swipe("hello")
        type(.backspace)
        #expect(output.text == "I said ")
        type(.backspace)
        #expect(output.text == "I said")
    }

    @Test func aBarSlotStraightAfterASwipeReplacesTheWord() {
        output.text = "I said "
        swipe("hello", "help")
        input.commitSuggestion(slot("help"))
        #expect(output.text == "I said help ")
    }

    @Test func aReplacementKeepsTheSwipesCapital() {
        type(.shift)
        swipe("hello", "help")
        input.commitSuggestion(slot("help"))
        #expect(output.text == "Help ")
    }

    @Test func aBarSlotAfterTypingMoreIsAnOrdinarySuggestion() {
        swipe("hello", "help")
        type(.character("w"))
        input.commitSuggestion("world")
        #expect(output.text == "hello world ")
    }

    @Test func punctuationStraightAfterASwipeTakesItsSpace() {
        swipe("hello")
        type(.character(","))
        #expect(output.text == "hello,")
        swipe("world")
        type(.character("."), .space)
        #expect(output.text == "hello, world. ")
    }

    @Test func aSwipedWordIsReportedOnceItStays() {
        output.text = "I said "
        var learned: [LearningEvent] = []
        let reported = uses {
            learned = events {
                swipe("hello", "help")
                type(.character("x"))
            }
        }
        #expect(reported == [use("hello", after: "I said ")])
        #expect(learned == [.committed("hello")])
    }

    @Test func aReplacedSwipeReportsOnlyTheChoice() {
        output.text = "I said "
        let reported = uses {
            swipe("hello", "help")
            input.commitSuggestion(slot("help"))
        }
        #expect(reported == [use("help", after: "I said ")])
    }

    @Test func aDeletedSwipeIsNeverReported() {
        let reported = uses {
            swipe("hello")
            type(.backspace, .character("x"))
        }
        #expect(reported.isEmpty)
    }

    @Test func eachSwipeEndsKeptReplacedOrDeleted() {
        var outcomes: [SwipeOutcome] = []
        input.onSwipeOutcome = { outcomes.append($0) }
        swipe("hello", "help")
        input.commitSuggestion(slot("help"))
        swipe("world")
        type(.backspace)
        swipe("again")
        type(.character("x"))
        #expect(outcomes == [.replaced, .deleted, .kept])
        #expect(!input.showsSwipe)
    }
}

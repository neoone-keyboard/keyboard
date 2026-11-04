import Testing
import KeyboardCore

@MainActor
struct EmojiInputTests {
    let output = RecordingOutput()
    let input: InputController

    init() { input = InputController(output: output) }

    @Test func aPanelEmojiGoesInAsItIs() {
        output.text = "hi "
        input.insertEmoji("👋🏽")
        #expect(output.text == "hi 👋🏽")
    }

    @Test func backspaceAfterAPanelEmojiDoesNotUndoAnEarlierCorrection() {
        output.text = "teh"
        input.updateSuggestions(SuggestionResult(typed: "teh", candidates: [
            Suggestion("the", kind: .correction), Suggestion("teh", kind: .verbatim),
        ]))
        input.handle(.space, at: 0)
        #expect(output.text == "the ")
        input.insertEmoji("😀")
        input.handle(.backspace, at: 0.1)
        #expect(output.text.hasPrefix("the "))
    }

    @Test func theBarsEmojiFollowsTheTypedWordAndASpace() {
        output.text = "I want pizza"
        input.commitEmojiSuggestion("🍕")
        #expect(output.text == "I want pizza 🍕 ")
    }

    @Test func theBarsEmojiAfterAFinishedWordFollowsIt() {
        output.text = "I want pizza "
        input.commitEmojiSuggestion("🍕")
        #expect(output.text == "I want pizza 🍕 ")
    }

    @Test func theBarsEmojiNeverReplacesTheWord() {
        output.text = "piza"
        input.updateSuggestions(SuggestionResult(typed: "piza", candidates: [
            Suggestion("pizza", kind: .correction), Suggestion("piza", kind: .verbatim),
        ]))
        input.commitEmojiSuggestion("🍕")
        #expect(output.text == "piza 🍕 ")
    }

    @Test func theTypedWordIsReportedButNeverTheEmoji() {
        var uses: [WordUse] = []
        input.onWordUsed = { uses.append($0) }
        output.text = "I want pizza"
        input.commitEmojiSuggestion("🍕")
        input.insertEmoji("😀")
        #expect(uses.map(\.word) == ["pizza"])
    }

    @Test func backspaceDeletesAWholeEmojiEvenWhereTheHostTakesOneCodePoint() {
        // RecordingOutput deletes one code point per call, as iOS does in Malayalam.
        output.text = "hi 👋🏽"
        input.deleteBackwardWholeEmoji()
        #expect(output.text == "hi ")
        output.text = "ok 🧑🏻‍🤝‍🧑🏻"
        input.deleteBackwardWholeEmoji()
        #expect(output.text == "ok ")
        output.text = "flag 🇮🇳"
        input.deleteBackwardWholeEmoji()
        #expect(output.text == "flag ")
    }

    @Test func backspaceInThePanelStillTakesOneMarkFromMalayalam() {
        output.text = "കി"
        input.deleteBackwardWholeEmoji()
        #expect(output.text == "ക")
        output.text = "ab"
        input.deleteBackwardWholeEmoji()
        #expect(output.text == "a")
    }
}

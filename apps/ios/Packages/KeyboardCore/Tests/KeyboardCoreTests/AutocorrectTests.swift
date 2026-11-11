import Foundation
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
        input = InputController(output: output, languages: englishAndMalayalam)
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
        input.handle(.character("?"), at: 0)
        #expect(output.text == "the?")
    }

    @Test func punctuationWithoutASuggestionJustTypes() {
        output.text = "hi"
        input.handle(.character("!"), at: 0)
        #expect(output.text == "hi!")
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

    /// On the Latin keys, a word in an active pack's script is not a mistyped English word,
    /// whatever the bar's middle says: Malayalam's, and any other pack's the same.
    @Test func aWordInAnActivePacksScriptIsNotReplacedOnTheLatinKeys() {
        for (packs, word, other) in [
            ([malayalamPack], "എന്റ", "एक"),
            ([pack("hi")], "नमस्त", "എന്റ"),
            ([pack("as")], "অসম", "एक"),
            ([pack("ta")], "தமிழ", "অসম"),
        ] {
            let output = RecordingOutput()
            let input = InputController(output: output, languages: [.english] + packs.map(Language.pack))
            output.text = word
            input.updateSuggestions(SuggestionResult(typed: word, candidates: [Suggestion("x", kind: .manglish)]))
            input.handle(.space, at: 0)
            #expect(output.text == word + " ", "\(packs[0].id) is active")
            // A script no active pack writes is not protected by this rule.
            output.text = other
            input.updateSuggestions(SuggestionResult(typed: other, candidates: [Suggestion("x", kind: .manglish)]))
            input.handle(.space, at: 1)
            #expect(output.text == "x ", "\(packs[0].id) is active, \(other) is another script")
        }
    }

    /// A pack that is on but has no keyboard of its own is not among `languages`; the keyboard
    /// names it in `activePacks`.
    @Test func anActivePackWithoutAKeyboardProtectsItsScriptToo() {
        input.activePacks = [pack("hi")]
        output.text = "नमस्त"
        input.updateSuggestions(SuggestionResult(typed: "नमस्त", candidates: [Suggestion("x", kind: .manglish)]))
        input.handle(.space, at: 0)
        #expect(output.text == "नमस्त ")
    }

    @Test func aPacksScriptRangesComeFromItsLangFile() {
        #expect(malayalamPack.scriptRanges == [0x0D00...0x0D7F])
        #expect(pack("hi").scriptRanges == [0x0900...0x097F])
        #expect(pack("ta").scriptRanges == [0x0B80...0x0BFF])
        // A pack published before packs carried lang.json: the three scripts the engine has
        // its own rules for.
        let old = PackDescriptor(id: "as", name: "Assamese", nativeName: "অসমীয়া", script: "beng", layout: nil,
                                 folder: URL(filePath: "/nowhere"))
        #expect(old.scriptRanges == [0x0980...0x09FF])
        let unknown = PackDescriptor(id: "xx", name: "X", nativeName: "X", script: "qaaa", layout: nil,
                                     folder: URL(filePath: "/nowhere"))
        #expect(unknown.scriptRanges.isEmpty)
        #expect(malayalamPack.writes("ക") && !malayalamPack.writes("क") && !malayalamPack.writes("k"))
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
        input.updateSuggestions(SuggestionResult(typed: "hello", candidates: [Suggestion("hello", kind: .verbatim)]))
        input.handle(.returnKey, at: 0)
        #expect(events.events == [.committed("hello")])
    }

    // MARK: Learning only from words autocorrect saw

    @Test func aWordIsNotReportedWhenAutocorrectIsOff() {
        input.autocorrectEnabled = false
        for key in [KeyAction.space, .character("."), .returnKey] {
            output.text = "thnaks"
            input.updateSuggestions(corrected("thnaks", to: "thanks"))
            input.handle(key, at: 0)
        }
        #expect(events.events.isEmpty)
    }

    @Test func aWordIsNotReportedBeforeItsResultArrives() {
        for key in [KeyAction.space, .character(","), .returnKey] {
            output.text = "thnaks"
            input.updateSuggestions(SuggestionResult(typed: "thnak", candidates: [Suggestion("thnak", kind: .verbatim)]))
            input.handle(key, at: 0)
        }
        #expect(events.events.isEmpty)
    }

    @Test func aWordAutocorrectDeclinedIsReported() {
        output.text = "Anu"
        input.updateSuggestions(SuggestionResult(typed: "Anu", candidates: [Suggestion("Anu", kind: .verbatim), Suggestion("Ann", kind: .correction)]))
        input.handle(.space, at: 0)
        #expect(output.text == "Anu ")
        #expect(events.events == [.committed("Anu")])
    }

    // MARK: Leading apostrophes

    @Test func aLeadingQuoteIsNotPartOfTheWord() {
        output.text = "say 'teh"
        #expect(input.currentWord == "teh")
        output.text = "say \u{2019}teh"
        #expect(input.currentWord == "teh")
        output.text = "say '"
        #expect(input.currentWord == "")
        output.text = "say don't"
        #expect(input.currentWord == "don't")
    }

    @Test func correctingAQuotedWordKeepsTheQuote() {
        output.text = "say 'teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        input.handle(.space, at: 0)
        #expect(output.text == "say 'the ")
        input.handle(.backspace, at: 0.1)
        #expect(output.text == "say 'teh")
    }

    @Test func tappingASuggestionForAQuotedWordKeepsTheQuote() throws {
        output.text = "say 'cou"
        let bar = SuggestionBar(SuggestionResult(typed: "cou", candidates: [Suggestion("could", kind: .completion)]))
        input.commitSuggestion(try #require(bar.middle))
        #expect(output.text == "say 'could ")
    }

    @Test func tappingASuggestionReportsTheCommittedWord() throws {
        output.text = "cou"
        let bar = SuggestionBar(SuggestionResult(typed: "cou", candidates: [Suggestion("could", kind: .completion)]))
        input.commitSuggestion(try #require(bar.middle))
        #expect(output.text == "could ")
        #expect(events.events == [.committed("could")])
    }
}

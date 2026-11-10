import Testing
import KeyboardCore

@MainActor
final class RecordingOutput: TextOutput {
    var text = ""
    var cursorMoves: [Int] = []
    /// Simulates a host that doesn't share the text before the cursor.
    var hidesContext = false
    var contextBefore: String? { hidesContext ? nil : text }
    var hasText: Bool { !text.isEmpty }
    func insertText(_ text: String) { self.text += text }
    /// Like iOS: one code point per call, so കി loses only its vowel sign.
    func deleteBackward() { if !text.isEmpty { text.unicodeScalars.removeLast() } }
    func adjustTextPosition(byCharacterOffset offset: Int) { cursorMoves.append(offset) }
}

@MainActor
struct InputControllerTests {
    let output = RecordingOutput()
    let input: InputController

    init() { input = InputController(output: output, languages: englishAndMalayalam) }

    func type(_ actions: KeyAction..., startingAt time: Double = 0) {
        for (i, action) in actions.enumerated() {
            input.handle(action, at: time + Double(i) * 0.1)
        }
    }

    // MARK: M0 behaviour

    @Test func typesLowercaseLettersSpaceAndReturn() {
        type(.character("h"), .character("i"), .space, .returnKey)
        #expect(output.text == "hi \n")
    }

    @Test func backspaceDeletes() {
        type(.character("a"), .character("b"), .backspace)
        #expect(output.text == "a")
    }

    @Test func shiftCapitalisesOnlyTheNextLetter() {
        type(.shift, .character("h"), .character("i"))
        #expect(output.text == "Hi")
        #expect(input.shift == .off)
    }

    @Test func doubleTapShiftLocksCaps() {
        input.handle(.shift, at: 0)
        input.handle(.shift, at: 0.2)
        type(.character("o"), .character("k"), startingAt: 1)
        #expect(output.text == "OK")
        #expect(input.shift == .locked)
    }

    /// What a letter's key-up will type, for starting its suggestions at touch-down: exactly
    /// the text the tap then adds, and nothing for keys whose effect depends on more.
    @Test func insertionIsWhatALetterTapTypes() {
        for (action, shifted) in [(KeyAction.character("k"), false), (.character("k"), true)] {
            let output = RecordingOutput()
            let input = InputController(output: output)
            if shifted { input.handle(.shift, at: 0) }
            let predicted = input.insertion(for: action)
            input.handle(action, at: 1)
            #expect(predicted == output.text)
        }
        for action in [KeyAction.character("."), .character(","), .character("'"), .character("1"),
                       .character("ക"), .character("\u{0D3F}"), .space, .backspace, .returnKey] {
            #expect(input.insertion(for: action) == nil, "\(action)")
        }
    }

    @Test func contextAfterSpaceIsWhatASpaceTapLeaves() {
        let cases: [(text: String, suggestions: SuggestionResult?, fullStop: Bool)] = [
            ("I said hello", nil, false),
            ("I said teh", corrected("teh", to: "the"), false),
            ("I said teh", corrected("teh", to: "the"), true),
            ("I said hello", nil, true),
        ]
        for c in cases {
            let output = RecordingOutput()
            let input = InputController(output: output)
            output.text = c.text
            if let suggestions = c.suggestions { input.updateSuggestions(suggestions) }
            if c.fullStop { input.handle(.character("."), at: 0) }
            let predicted = input.contextAfterSpace()
            input.handle(.space, at: 1)
            #expect(predicted == output.text, "\(c.text) \(c.fullStop)")
        }
    }

    @Test func labelsFollowShiftState() {
        #expect(input.label(for: .character("a")) == "a")
        #expect(input.label(for: .shift) == "⇧")
        input.handle(.shift, at: 0)
        #expect(input.label(for: .character("a")) == "A")
        #expect(input.label(for: .shift) == "⬆︎")
        input.handle(.shift, at: 0.1)
        #expect(input.label(for: .shift) == "⇪")
    }

    @Test func combiningMarkLabelsSitOnADottedCircle() {
        type(.toggleLanguage)
        #expect(input.label(for: .character("\u{0D3F}")) == "\u{25CC}\u{0D3F}")
        #expect(input.label(for: .character("\u{0D4D}")) == "\u{25CC}\u{0D4D}")
        #expect(input.label(for: .character("\u{0D02}")) == "\u{25CC}\u{0D02}")
        #expect(input.label(for: .character("ക")) == "ക")
        #expect(input.label(for: .character("കി")) == "കി")
    }

    // MARK: Pages and language

    @Test func symbolsPageReturnsToLettersAfterSpace() {
        type(.page(.symbols))
        #expect(input.page == .symbols)
        type(.character("1"), .space)
        #expect(output.text == "1 ")
        #expect(input.page == .letters)
    }

    @Test func toggleLanguageSwitchesToMalayalamAndBack() {
        type(.toggleLanguage)
        #expect(input.language == malayalam)
        #expect(input.page == .native("letters"))
        #expect(input.label(for: .space) == "മലയാളം")
        #expect(input.label(for: .toggleLanguage) == "EN")
        type(.toggleLanguage)
        #expect(input.language == .english)
        #expect(input.page == .letters)
        #expect(input.label(for: .space) == "English")
        #expect(input.label(for: .toggleLanguage) == "മ")
    }

    @Test func lettersPageKeyReturnsToTheCurrentAlphabet() {
        type(.toggleLanguage, .page(.symbols))
        #expect(input.label(for: .page(.letters)) == "കഖ")
        type(.page(.letters))
        #expect(input.page == .native("letters"))
    }

    @Test func malayalamTypingIsNeverUppercased() {
        type(.shift, .toggleLanguage, .character("ക"), .character("\u{0D3F}"))
        #expect(output.text == "കി")
    }

    @Test func layoutFollowsPageAndOptions() {
        input.numberRow = true
        #expect(input.layout == KeyboardLayout.forPage(.letters, numberRow: true))
        input.showsNextKeyboard = false
        #expect(!input.layout.rows.flatMap { $0 }.contains { $0.action == .nextKeyboard })
    }

    // MARK: Alternates, cursor, word delete

    @Test func alternatesFollowShift() {
        type(.shift)
        input.insertAlternate("é")
        #expect(output.text == "É")
        #expect(input.shift == .off)
    }

    @Test func moveCursorForwardsToOutput() {
        input.moveCursor(by: -3)
        #expect(output.cursorMoves == [-3])
    }

    @Test func deleteWordRemovesWordAndTrailingSpaces() {
        output.text = "hello world  "
        input.deleteWordBackward()
        #expect(output.text == "hello ")
    }

    @Test func deleteWordHandlesMalayalam() {
        output.text = "ഞാൻ എന്റെ"
        input.deleteWordBackward()
        #expect(output.text == "ഞാൻ ")
    }

    @Test func suggestionReplacesTheWordBeingTyped() {
        output.text = "say hel"
        input.commitSuggestion("hello")
        #expect(output.text == "say hello ")
    }

    @Test func suggestionAfterASpaceIsAppended() {
        output.text = "say "
        input.commitSuggestion("hello")
        #expect(output.text == "say hello ")
    }

    @Test func suggestionReplacesAPartialMalayalamWord() {
        output.text = "ഞാൻ എന്റ"
        input.commitSuggestion("എന്റെ")
        #expect(output.text == "ഞാൻ എന്റെ ")
    }

    @Test func suggestionTurnsOffAOneShotShift() {
        type(.shift)
        input.commitSuggestion("Hello")
        #expect(input.shift == .off)
    }

    @Test func deleteWordRemovesOnePunctuationMark() {
        output.text = "hi!"
        input.deleteWordBackward()
        #expect(output.text == "hi")
    }

    // MARK: Auto-capitalisation

    @Test func capitalisesTheStartOfEachSentence() {
        input.applyAutoCapitalization(.sentences)
        #expect(input.shift == .once)
        type(.character("h"), .character("i"), .character("."), .space)
        input.applyAutoCapitalization(.sentences)
        #expect(output.text == "Hi. ")
        #expect(input.shift == .once)
    }

    @Test func automaticShiftTurnsOffWhenNoLongerNeeded() {
        input.applyAutoCapitalization(.sentences)
        output.text = "abc"
        input.applyAutoCapitalization(.sentences)
        #expect(input.shift == .off)
    }

    @Test func leavesAShiftTheUserTappedAlone() {
        output.text = "abc"
        type(.shift)
        input.applyAutoCapitalization(.sentences)
        #expect(input.shift == .once)
    }

    @Test func leavesCapsLockAlone() {
        input.handle(.shift, at: 0)
        input.handle(.shift, at: 0.1)
        output.text = "abc"
        input.applyAutoCapitalization(.sentences)
        #expect(input.shift == .locked)
    }

    @Test func noAutoCapitalisationWhenOff() {
        input.applyAutoCapitalization(.off)
        #expect(input.shift == .off)
    }

    @Test func shiftTappedOffAtSentenceStartStaysOff() {
        input.applyAutoCapitalization(.sentences)
        #expect(input.shift == .once)
        type(.shift)
        input.applyAutoCapitalization(.sentences)
        #expect(input.shift == .off)
        type(.character("h"))
        input.applyAutoCapitalization(.sentences)
        #expect(output.text == "h")
    }

    @Test func autoCapitalisationResumesAfterTheContextChanges() {
        input.applyAutoCapitalization(.sentences)
        type(.shift)
        input.applyAutoCapitalization(.sentences)
        type(.character("h"), .character("i"), .character("."), .space, startingAt: 1)
        input.applyAutoCapitalization(.sentences)
        #expect(output.text == "hi. ")
        #expect(input.shift == .once)
    }

    @Test func hiddenContextInAnEmptyFieldCapitalises() {
        output.hidesContext = true
        input.applyAutoCapitalization(.sentences)
        #expect(input.shift == .once)
    }

    @Test func hiddenContextWithTextLeavesShiftAlone() {
        output.hidesContext = true
        output.text = "abc"
        input.applyAutoCapitalization(.sentences)
        #expect(input.shift == .off)
        type(.character("d"))
        input.applyAutoCapitalization(.sentences)
        #expect(output.text == "abcd")
    }

    @Test func wordsModeCapitalisesAfterEverySpace() {
        output.text = "hello "
        input.applyAutoCapitalization(.words)
        #expect(input.shift == .once)
    }

    // MARK: Emails and URLs

    /// Learning events reported while `body` runs.
    func events(_ body: () -> Void) -> [LearningEvent] {
        var events: [LearningEvent] = []
        input.onLearningEvent = { events.append($0) }
        body()
        input.onLearningEvent = nil
        return events
    }

    func typeText(_ text: String) {
        for c in text { input.handle(.character(String(c)), at: 0) }
    }

    func verbatim(_ word: String) -> SuggestionResult {
        SuggestionResult(typed: word, candidates: [Suggestion(word, kind: .verbatim)])
    }

    @Test func anAddressIsOneWord() {
        for (text, word) in [
            ("mail priya@example.com", "priya@example.com"),
            ("mail priya@example.", "priya@example."),
            ("see https://x.com/a-b", "https://x.com/a-b"),
            ("(www.x.com", "www.x.com"),
            ("see example.com/blog", "example.com/blog"),
            ("Google.COM", "Google.COM"),
            ("x.com", "x.com"),
            ("end.it", "end.it"),
            ("so a.b.c", "a.b.c"),
            ("googel.c", "googel.c"),
            ("(e.g", "e.g"),
            ("hello.", ""),
            ("(hello.)", ""),
            ("me @", ""),
        ] {
            output.text = text
            #expect(input.currentWord == word, "\(text)")
        }
    }

    @Test func punctuationInsideAnAddressNeitherCorrectsNorCommits() {
        let reported = events {
            output.text = "jishnu"
            input.updateSuggestions(SuggestionResult(typed: "jishnu", candidates: [Suggestion("Kishan", kind: .correction)]))
            typeText("@gmial")
            #expect(output.text == "priya@exmaple")
            for mark in [".", ",", "?", "!"] {
                input.updateSuggestions(SuggestionResult(typed: input.currentWord, candidates: [Suggestion("gmail", kind: .correction)]))
                typeText(mark)
            }
            #expect(output.text == "priya@exmaple.,?!")
        }
        #expect(reported.isEmpty)
    }

    @Test func aFullStopThatMakesAnAddressDoesNotCorrect() {
        output.text = "www"
        input.updateSuggestions(SuggestionResult(typed: "www", candidates: [Suggestion("wow", kind: .correction)]))
        typeText(".")
        #expect(output.text == "www.")
    }

    @Test func spaceAfterAnAddressKeepsItAndReportsItWhole() {
        let reported = events {
            output.text = "mail priya@exmaple.com"
            input.updateSuggestions(SuggestionResult(typed: "priya@exmaple.com", candidates: [Suggestion("priya@example.com", kind: .correction)]))
            input.handle(.space, at: 0)
            #expect(output.text == "mail priya@exmaple.com ")
            output.text = "mail priya@example.com"
            input.updateSuggestions(verbatim("priya@example.com"))
            input.handle(.returnKey, at: 0)
        }
        #expect(reported == [.committed("priya@exmaple.com"), .committed("priya@example.com")])
    }

    @Test func anAddressIsReportedWhereAutocorrectIsOff() {
        input.autocorrectEnabled = false
        let reported = events {
            output.text = "priya@example.com"
            input.handle(.space, at: 0)
        }
        #expect(reported == [.committed("priya@example.com")])
    }

    @Test func nothingIsLearnedInASecureField() {
        input.secureTextEntry = true
        let reported = events {
            output.text = "priya@example.com"
            input.handle(.space, at: 0)
            output.text = "hello"
            input.updateSuggestions(verbatim("hello"))
            input.handle(.space, at: 0)
            output.text = "teh"
            input.updateSuggestions(SuggestionResult(typed: "teh", candidates: [Suggestion("the", kind: .correction), Suggestion("teh", kind: .verbatim)]))
            input.handle(.space, at: 0)
            input.handle(.backspace, at: 0.1)
            input.commitSuggestion("could")
        }
        #expect(reported.isEmpty)
    }

    @Test func aCompletionReplacesTheWholeAddress() {
        output.text = "mail priya@e"
        input.commitSuggestion("priya@example.com")
        #expect(output.text == "mail priya@example.com ")
    }

    @Test func lettersThatBeginALearnedAddressAreOneWord() {
        input.learnedAddresses = ["sam.2024@example.com"]
        let reported = events {
            output.text = "hi neo"
            input.updateSuggestions(SuggestionResult(typed: "neo", candidates: [Suggestion("new", kind: .correction)]))
            typeText(".")
            #expect(output.text == "hi neo.")
            #expect(input.currentWord == "neo.")
            input.updateSuggestions(SuggestionResult(typed: "neo.", candidates: [Suggestion("neo", kind: .correction)]))
            typeText("9")
            #expect(input.currentWord == "neo.9")
            #expect(SuggestionResult(typed: "neo.9", candidates: []).isFor(word: input.currentWord))
        }
        #expect(reported.isEmpty)
        input.commitSuggestion("sam.2024@example.com")
        #expect(output.text == "hi sam.2024@example.com ")
    }

    @Test func aLearnedAddressIsMatchedInAnyCase() {
        input.learnedAddresses = ["sam.2024@example.com"]
        output.text = "Neo.9"
        #expect(input.currentWord == "Neo.9")
    }

    @Test func otherLettersStillEndAtPunctuation() {
        input.learnedAddresses = ["sam.2024@example.com"]
        for (text, word) in [("neon.", ""), ("n.", ""), ("neo", "neo"), ("neon. x", "x")] {
            output.text = text
            #expect(input.currentWord == word, "\(text)")
        }
        output.text = "hello"
        input.updateSuggestions(SuggestionResult(typed: "hello", candidates: [Suggestion("help", kind: .correction)]))
        typeText(".")
        input.handle(.space, at: 0)
        #expect(output.text == "help. ")
    }

    @Test func noCapitalAfterAFullStopInsideAnAddress() {
        output.text = "Mail priya@example."
        input.applyAutoCapitalization(.sentences)
        #expect(input.shift == .off)
        typeText("c")
        #expect(output.text == "Mail priya@example.c")
    }

    // MARK: Full stops

    func corrected(_ typed: String, to replacement: String) -> SuggestionResult {
        SuggestionResult(typed: typed, candidates: [Suggestion(replacement, kind: .correction), Suggestion(typed, kind: .verbatim)])
    }

    @Test func aFullStopCorrectsTheWordBeforeItOnlyAtTheSpace() {
        let reported = events {
            output.text = "I said teh"
            input.updateSuggestions(corrected("teh", to: "the"))
            typeText(".")
            #expect(output.text == "I said teh.")
            input.updateSuggestions(SuggestionResult(typed: "", candidates: [Suggestion("It", kind: .nextWord)]))
            input.handle(.space, at: 0)
            #expect(output.text == "I said the. ")
        }
        #expect(reported.isEmpty)
        input.applyAutoCapitalization(.sentences)
        #expect(input.shift == .once)
    }

    @Test func returnAfterAFullStopCorrectsToo() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        typeText(".")
        input.handle(.returnKey, at: 0)
        #expect(output.text == "the.\n")
    }

    @Test func shiftBetweenTheFullStopAndTheSpaceKeepsTheCorrection() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        typeText(".")
        input.handle(.shift, at: 0)
        input.handle(.space, at: 0.1)
        #expect(output.text == "the. ")
    }

    @Test func backspaceUndoesACorrectionMadeAtTheSpace() {
        let reported = events {
            output.text = "teh"
            input.updateSuggestions(corrected("teh", to: "the"))
            typeText(".")
            input.handle(.space, at: 0)
            input.handle(.backspace, at: 0.1)
            #expect(output.text == "teh.")
            input.handle(.space, at: 0.2)
            #expect(output.text == "teh. ")
        }
        #expect(reported == [.learn("teh")])
    }

    @Test func lettersAfterAFullStopMakeOneWordThatIsNotCorrected() {
        let reported = events {
            output.text = "go to googel"
            input.updateSuggestions(corrected("googel", to: "google"))
            for c in [".", "c", "o", "m"] {
                typeText(c)
                input.updateSuggestions(corrected(input.currentWord, to: "google"))
            }
            #expect(output.text == "go to googel.com")
            #expect(input.currentWord == "googel.com")
            input.handle(.space, at: 0)
            #expect(output.text == "go to googel.com ")
        }
        #expect(reported == [.committed("googel.com")])
    }

    @Test func aLetterAfterAFullStopDropsTheCorrection() {
        output.text = "teh"
        input.updateSuggestions(corrected("teh", to: "the"))
        typeText(".x")
        #expect(output.text == "teh.x")
        #expect(input.currentWord == "teh.x")
        #expect(SuggestionResult(typed: "teh.x", candidates: []).isFor(word: input.currentWord))
    }

    @Test func aWordKeptAtAFullStopIsReportedAtTheSpace() {
        let reported = events {
            output.text = "hello"
            input.updateSuggestions(verbatim("hello"))
            typeText(".")
            #expect(output.text == "hello.")
            input.handle(.space, at: 0)
        }
        #expect(output.text == "hello. ")
        #expect(reported == [.committed("hello")])
    }

    @Test func aWordFollowedByAFullStopAndALetterIsNotReportedAlone() {
        let reported = events {
            output.text = "hello"
            input.updateSuggestions(verbatim("hello"))
            typeText(".x")
        }
        #expect(reported.isEmpty)
    }

    @Test func movingTheCursorOrAnOutsideChangeDropsTheCorrection() {
        for drop in [{ input.moveCursor(by: 0) }, { input.textDidChangeExternally() }] {
            output.text = "teh"
            input.updateSuggestions(corrected("teh", to: "the"))
            typeText(".")
            drop()
            input.handle(.space, at: 0)
            #expect(output.text == "teh. ")
        }
    }

    @Test func otherPunctuationStillCorrectsAtOnce() {
        for mark in [",", "?", "!"] {
            output.text = "teh"
            input.updateSuggestions(corrected("teh", to: "the"))
            typeText(mark)
            #expect(output.text == "the" + mark)
        }
    }

    @Test func aDottedWordIsOneWord() {
        output.text = "so a.b.c"
        #expect(input.currentWord == "a.b.c")
        output.text = "so a.b"
        #expect(SuggestionResult(typed: "a.b", candidates: []).isFor(word: input.currentWord))
    }

    // MARK: Word uses

    /// Word uses reported while `body` runs.
    func uses(_ body: () -> Void) -> [WordUse] {
        var uses: [WordUse] = []
        input.onWordUsed = { uses.append($0) }
        body()
        input.onWordUsed = nil
        return uses
    }

    func use(_ word: String, after before: String, chosenFor: String? = nil) -> WordUse {
        WordUse(word: word, before: before, language: .english, chosenFor: chosenFor)
    }

    @Test func everyWordThatGoesInIsReportedWithTheTextBeforeIt() {
        let reported = uses {
            output.text = "hello"
            input.updateSuggestions(verbatim("hello"))
            input.handle(.space, at: 0)
            typeText("hpw")
            input.updateSuggestions(corrected("hpw", to: "how"))
            input.handle(.space, at: 0)
            typeText("ar")
            input.commitSuggestion(SuggestionSlot(suggestion: Suggestion("are", kind: .completion), quoted: false))
            typeText("you")
            typeText(".")
            input.handle(.space, at: 0)
            typeText("ok")
            input.handle(.returnKey, at: 0)
        }
        #expect(output.text == "hello how are you. ok\n")
        #expect(reported == [
            use("hello", after: ""),
            use("how", after: "hello "),
            use("are", after: "hello how "),
            use("you", after: "hello how are "),
            use("ok", after: "hello how are you. "),
        ])
    }

    @Test func aCorrectionAtAFullStopOrPunctuationIsReportedAsCorrected() {
        let reported = uses {
            output.text = "so teh"
            input.updateSuggestions(corrected("teh", to: "the"))
            typeText(".")
            input.handle(.space, at: 0)
            typeText("hpw")
            input.updateSuggestions(corrected("hpw", to: "how"))
            typeText(",")
        }
        #expect(output.text == "so the. how,")
        #expect(reported == [use("the", after: "so "), use("how", after: "so the. ")])
    }

    @Test func usesAreReportedWithAutocorrectOffAndInMalayalam() {
        input.autocorrectEnabled = false
        let english = uses {
            output.text = "hello"
            input.handle(.space, at: 0)
        }
        #expect(english == [use("hello", after: "")])
        type(.toggleLanguage)
        let native = uses {
            output.text = "എന്റെ വീട്"
            input.handle(.space, at: 0)
        }
        #expect(native == [WordUse(word: "വീട്", before: "എന്റെ ", language: malayalam)])
    }

    @Test func onlyTheEndOfTheTextIsSent() {
        let long = String(repeating: "a ", count: 100)
        let reported = uses {
            output.text = long + "hello"
            input.handle(.space, at: 0)
        }
        #expect(reported.map(\.before) == [String(long.suffix(WordUse.contextLength))])
    }

    @Test func addressesAreNeverReportedAsUses() {
        input.learnedAddresses = ["sam.2024@example.com"]
        let reported = uses {
            output.text = "mail priya@example.com"
            input.handle(.space, at: 0)
            output.text = "see www.x.com"
            input.handle(.returnKey, at: 0)
            output.text = "hi neo"
            input.commitSuggestion("sam.2024@example.com")
        }
        #expect(reported.isEmpty)
    }

    /// Digits are typed on the 123 page (iOS never offers a keyboard of one's own in a phone
    /// pad field), and space goes back to the letters after each group.
    @Test func aNumberTypedOnTheSymbolsPageIsReportedGroupByGroup() {
        input.autocorrectEnabled = false
        let reported = uses {
            output.text = "call "
            type(.page(.symbols))
            typeText("+91")
            input.handle(.space, at: 0)
            #expect(input.page == .letters)
            type(.page(.symbols))
            typeText("55501")
            input.handle(.space, at: 0)
            type(.page(.symbols))
            typeText("12345")
            input.handle(.returnKey, at: 0)
        }
        #expect(output.text == "call +91 55501 12345\n")
        #expect(reported == [
            use("91", after: "call +"),
            use("55501", after: "call +91 "),
            use("12345", after: "call +91 55501 "),
        ])
    }

    @Test func aDottedNumberIsReportedThoughADottedWordIsNot() {
        let reported = uses {
            output.text = "98.470.12345"
            input.handle(.space, at: 0)
            output.text = "x.com"
            input.handle(.space, at: 0)
        }
        #expect(reported == [use("98.470.12345", after: "")])
    }

    @Test func digitsThatBeginALearnedNumberAreOneWord() {
        input.learnedAddresses = ["+91 55501 12345", "55501-12345", "sam@example.com"]
        for (text, word) in [
            ("call +91 98", "+91 98"),
            ("call +91 9", "+91 9"),
            ("55501-1", "55501-1"),
            ("\"+91 98", "+91 98"),
            ("call 98", "98"),
            ("call +91  98", "98"),
            ("call 99 55501", "55501"),
            ("call +91", "91"),
        ] {
            output.text = text
            #expect(input.currentWord == word, "\(text)")
        }
        output.text = "call +91 98"
        input.updateSuggestions(SuggestionResult(typed: "98", candidates: [Suggestion("98", kind: .verbatim)]))
        input.commitSuggestion("+91 55501 12345")
        #expect(output.text == "call +91 55501 12345 ")
    }

    @Test func aFullStopInsideALearnedNumberIsPartOfIt() {
        input.learnedAddresses = ["98.470.12345"]
        output.text = "98"
        typeText(".")
        #expect(input.currentWord == "98.")
        typeText("4")
        #expect(input.currentWord == "98.4")
    }

    @Test func noUseIsReportedInASecureField() {
        input.secureTextEntry = true
        let reported = uses {
            typeText("55501")
            input.handle(.space, at: 0)
            typeText("12345")
            input.handle(.space, at: 0)
            output.text = "hello"
            input.updateSuggestions(verbatim("hello"))
            input.handle(.space, at: 0)
            typeText("teh")
            input.updateSuggestions(corrected("teh", to: "the"))
            input.handle(.space, at: 0)
            typeText("x.")
            input.handle(.space, at: 0)
            input.commitSuggestion("could")
            input.commitSuggestion(SuggestionSlot(suggestion: Suggestion("ഇന്ന്", kind: .manglish), quoted: false))
        }
        #expect(reported.isEmpty)
    }

    @Test func tappingAnotherCorrectionOrMalayalamWordIsAChoice() {
        let innu = SuggestionResult(typed: "innu", candidates: [
            Suggestion("ഇന്നു", kind: .manglish), Suggestion("innu", kind: .verbatim),
            Suggestion("ഇന്ന്", kind: .manglish),
        ])
        func tap(_ suggestion: Suggestion, quoted: Bool = false) -> WordUse? {
            output.text = "so innu"
            input.updateSuggestions(innu)
            return uses { input.commitSuggestion(SuggestionSlot(suggestion: suggestion, quoted: quoted)) }.first
        }
        #expect(tap(Suggestion("ഇന്ന്", kind: .manglish)) == use("ഇന്ന്", after: "so ", chosenFor: "innu"))
        #expect(tap(Suggestion("ഇന്നു", kind: .manglish))?.chosenFor == nil, "the middle is the default")
        #expect(tap(Suggestion("innu", kind: .verbatim), quoted: true)?.chosenFor == nil)
        #expect(tap(Suggestion("innumerable", kind: .completion))?.chosenFor == nil)
        // A result for other letters says nothing of these.
        output.text = "so innu"
        input.updateSuggestions(SuggestionResult(typed: "inn", candidates: [Suggestion("inn", kind: .verbatim)]))
        let late = uses { input.commitSuggestion(SuggestionSlot(suggestion: Suggestion("ഇന്ന്", kind: .manglish), quoted: false)) }
        #expect(late.first?.chosenFor == nil)
    }

    /// The engine learns a tapped word it doesn't have as a word of the language that offered it.
    @Test func aChoiceCarriesTheLanguageTheSuggestionWasOfferedIn() {
        let raubdar = SuggestionResult(typed: "raubdar", candidates: [
            Suggestion("raubdar", kind: .verbatim, language: "en"),
            Suggestion("रोबदार", kind: .manglish, language: "hi"),
            Suggestion("रौबदार", kind: .manglish, language: "hi"),
        ])
        func tap(_ index: Int) -> WordUse? {
            output.text = "so raubdar"
            input.updateSuggestions(raubdar)
            return uses { input.commitSuggestion(SuggestionSlot(suggestion: raubdar.candidates[index], quoted: false)) }.first
        }
        var chosen = use("रौबदार", after: "so ", chosenFor: "raubdar")
        chosen.chosenLanguage = "hi"
        #expect(tap(2) == chosen)
        #expect(tap(0)?.chosenLanguage == nil, "the middle is no choice")
        // A suggestion that names no language is still a choice: the engine reads the script.
        output.text = "so raubdar"
        input.updateSuggestions(raubdar)
        let plain = uses { input.commitSuggestion(SuggestionSlot(suggestion: Suggestion("रौबदार", kind: .manglish), quoted: false)) }
        #expect(plain.first == use("रौबदार", after: "so ", chosenFor: "raubdar"))
    }
}

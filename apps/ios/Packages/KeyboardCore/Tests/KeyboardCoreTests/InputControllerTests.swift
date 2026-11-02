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

    init() { input = InputController(output: output) }

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
        #expect(input.language == .malayalam)
        #expect(input.page == .malayalam)
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
        #expect(input.page == .malayalam)
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
}

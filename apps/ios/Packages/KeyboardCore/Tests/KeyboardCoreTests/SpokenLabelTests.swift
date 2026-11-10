import Testing
import KeyboardCore

/// What VoiceOver says for each key (`InputController.spokenKey`), as Android's SpokenLabelTest.
@MainActor
struct SpokenLabelTests {
    let input = InputController(output: RecordingOutput(), languages: englishAndMalayalam)

    @Test func lettersAreSpokenAsTheyWouldBeTyped() {
        #expect(input.spokenLabel(for: .character("a")) == "a")
        input.handle(.shift, at: 0)
        #expect(input.spokenLabel(for: .character("a")) == "A")
    }

    @Test func shiftSaysWhetherItIsOn() {
        #expect(input.spokenKey(for: .shift) == SpokenKey("shift"))
        #expect(input.spokenLabel(for: .shift) == "shift")
        input.handle(.shift, at: 0)
        #expect(input.spokenKey(for: .shift) == SpokenKey("shift", value: "on"))
        #expect(input.spokenLabel(for: .shift) == "shift, on")
        input.handle(.shift, at: 0.1)
        #expect(input.spokenKey(for: .shift) == SpokenKey("caps lock", value: "on"))
        #expect(input.spokenLabel(for: .shift) == "caps lock, on")
    }

    @Test func specialKeysSayWhatTheyDo() {
        #expect(input.spokenLabel(for: .space) == "space")
        #expect(input.spokenLabel(for: .backspace) == "delete")
        #expect(input.spokenLabel(for: .returnKey) == "return")
        #expect(input.spokenLabel(for: .returnKey, returnTitle: "search") == "search")
        #expect(input.spokenLabel(for: .nextKeyboard) == "next keyboard")
        #expect(input.spokenLabel(for: .emoji) == "emoji")
        #expect(input.spokenLabel(for: .page(.symbols)) == "numbers and symbols")
        #expect(input.spokenLabel(for: .page(.moreSymbols)) == "more symbols")
        #expect(input.spokenLabel(for: .page(.letters)) == "letters")
    }

    @Test func theLanguageKeyNamesTheLanguageItSwitchesTo() {
        #expect(input.spokenLabel(for: .toggleLanguage) == "switch to Malayalam")
        input.handle(.toggleLanguage, at: 0)
        #expect(input.spokenLabel(for: .toggleLanguage) == "switch to English")
    }

    @Test func aNativePageKeyIsSpokenAsItIsDrawn() {
        input.handle(.toggleLanguage, at: 0)
        let page = KeyAction.page(mlPage("vowels"))
        #expect(input.spokenLabel(for: page) == input.label(for: page))
        #expect(input.spokenLabel(for: page) != "vowels")
    }

    @Test func aLoneVowelSignIsSpokenWithoutItsPlaceholder() {
        input.handle(.toggleLanguage, at: 0)
        #expect(input.spokenLabel(for: .character("\u{0D3F}")) == "\u{0D3F}")
        #expect(input.spokenLabel(for: .character("ക")) == "ക")
    }
}

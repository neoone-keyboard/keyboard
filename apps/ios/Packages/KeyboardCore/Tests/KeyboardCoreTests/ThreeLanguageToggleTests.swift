import Testing
@testable import KeyboardCore

@MainActor
struct ThreeLanguageToggleTests {
    let hindi = Language.pack(pack("hi"))

    @Test func theLanguageKeyStepsThroughAllThreeFromTheLettersPage() {
        let output = RecordingOutput()
        let input = InputController(output: output, languages: [.english, malayalam, hindi])
        input.handle(.toggleLanguage, at: 0)
        #expect(input.language == malayalam)
        #expect(input.label(for: .toggleLanguage) == "हि")
        input.handle(.toggleLanguage, at: 1)
        #expect(input.language == hindi)
        #expect(input.page == .native("letters"))
        input.handle(.toggleLanguage, at: 2)
        #expect(input.language == .english)
    }

    /// Malayalam's and Hindi's first pages are both `.native("letters")`: the page alone
    /// doesn't change from one to the other, so the keys shown must also name the language,
    /// or the extension keeps drawing Malayalam's keys after the switch to Hindi.
    @Test func switchingBetweenTwoNativeLettersPagesChangesTheKeysShown() {
        let input = InputController(output: RecordingOutput(), languages: [.english, malayalam, hindi])
        input.handle(.toggleLanguage, at: 0)
        let malayalamKeys = input.keysShown
        input.handle(.toggleLanguage, at: 1)
        #expect(input.page == malayalamKeys.page)
        #expect(input.keysShown != malayalamKeys)
        #expect(input.keysShown == KeysShown(language: "hi", page: .native("letters")))
    }

    @Test func shiftAloneLeavesTheKeysShownAsTheyWere() {
        let input = InputController(output: RecordingOutput(), languages: [.english, malayalam, hindi])
        let before = input.keysShown
        input.handle(.shift, at: 0)
        #expect(input.keysShown == before)
    }
}

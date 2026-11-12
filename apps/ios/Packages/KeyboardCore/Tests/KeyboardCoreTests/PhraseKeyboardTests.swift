import Foundation
import Testing
import KeyboardCore

/// packs/ja.neopack, as the keyboard reads it: a conversion pack with a QWERTY layout.
let japanesePack = pack("ja")

private func characters(_ layout: KeyboardLayout) -> [String] {
    layout.rows.flatMap { $0 }.compactMap { key in
        if case .character(let c) = key.action { c } else { nil }
    }
}

/// Japanese on the keyboard: its layout's romaji keys compose through `PhraseInput` while
/// Japanese is the language, and English is exactly as before.
@MainActor
struct PhraseKeyboardTests {
    let output = RecordingOutput()
    let converter = FakeConverter()
    let input: InputController

    init() {
        input = InputController(output: output, languages: [.english, .pack(japanesePack)])
        input.activePacks = [japanesePack]
        input.phraseConverter = converter
    }

    func type(_ actions: KeyAction...) {
        for action in actions { input.handle(action, at: 0) }
    }

    func letters(_ text: String) {
        for c in text { input.handle(.character(String(c)), at: 0) }
    }

    func toJapanese() {
        type(.toggleLanguage)
        #expect(input.language.id == "ja")
    }

    @Test func japaneseIsTypedOnItsLayoutsRomajiKeys() {
        #expect(japanesePack.conversion)
        toJapanese()
        #expect(input.page == .native("letters"))
        #expect(characters(input.layout).prefix(10) == ["q", "w", "e", "r", "t", "y", "u", "i", "o", "p"])
        #expect(input.language.convertsPhrases)
        #expect(!Language.english.convertsPhrases)
        #expect(input.label(for: .character("k")) == "k")
    }

    @Test func lettersComposeAndSpaceConverts() {
        toJapanese()
        letters("kyouhaamegafuru")
        #expect(output.marked == "きょうはあめがふる")
        #expect(input.composing?.isComposing == true)
        type(.space)
        #expect(output.marked == "今日は雨が降る")
        type(.space)
        #expect(output.marked == "京は雨が降る")
        type(.returnKey)
        #expect(output.text == "京は雨が降る")
        #expect(output.marked == "")
    }

    @Test func spaceAndReturnAreThemselvesWithNothingComposing() {
        toJapanese()
        type(.space, .returnKey)
        #expect(output.text == " \n")
    }

    @Test func backspaceDeletesInTheCompositionThenTheText() {
        output.insertText("x")
        toJapanese()
        letters("ka")
        type(.backspace)
        #expect(output.text == "x")
        type(.backspace)
        #expect(output.text == "")
    }

    @Test func theSpaceAndReturnKeysSayWhatTheyDoWhileComposing() {
        toJapanese()
        #expect(input.label(for: .space) == "日本語")
        #expect(input.label(for: .returnKey) == "return")
        letters("a")
        #expect(input.label(for: .space) == "次候補")
        #expect(input.label(for: .returnKey) == "確定")
        #expect(input.spokenLabel(for: .space) == "next candidate")
        #expect(input.spokenLabel(for: .returnKey) == "confirm")
        type(.returnKey)
        #expect(input.label(for: .space) == "日本語")
    }

    @Test func theLanguageKeyNamesJapaneseByItsLayout() {
        #expect(input.label(for: .toggleLanguage) == "あ")
        toJapanese()
        #expect(input.label(for: .toggleLanguage) == "EN")
    }

    @Test func switchingLanguageCommitsTheComposition() {
        toJapanese()
        letters("ame")
        type(.space, .toggleLanguage)
        #expect(output.text == "雨")
        #expect(output.marked == "")
        #expect(input.language == .english)
        letters("ame")
        #expect(output.text == "雨ame", "English is typed as before")
        #expect(output.marked == "")
    }

    @Test func otherPagesKeepTheComposition() {
        toJapanese()
        letters("ame")
        type(.page(.native("marks")))
        #expect(output.marked == "あめ")
        type(.character("「"))
        #expect(output.marked == "あめ「")
        type(.page(.symbols))
        type(.character("。"))
        #expect(output.marked == "あめ「。")
    }

    @Test func aLongPressAlternateJoinsTheComposition() {
        toJapanese()
        letters("ame")
        input.insertAlternate("〜")
        #expect(output.marked == "あめ〜")
    }

    @Test func punctuationWithNothingComposingIsTheJapaneseMark() {
        toJapanese()
        type(.character("."), .character(","))
        #expect(output.text == "。、")
        #expect(output.marked == "")
    }

    @Test func shiftNeverComesOnByItselfInJapanese() {
        toJapanese()
        input.applyAutoCapitalization(.sentences)
        #expect(input.shift == .off)
    }

    @Test func movingTheCursorOrAnEmojiCommitsFirst() {
        toJapanese()
        letters("ame")
        input.insertEmoji("🙂")
        #expect(output.text == "あめ🙂")
        #expect(output.marked == "")
        letters("ame")
        input.moveCursor(by: -1)
        #expect(output.marked == "")
        #expect(output.text == "あめ🙂あめ")
    }

    @Test func deletingTheWordDeletesTheComposition() {
        output.insertText("x ")
        toJapanese()
        letters("ame")
        input.deleteWordBackward()
        #expect(output.text == "x ")
        #expect(input.composing?.isComposing == false)
    }

    @Test func anOutsideChangeForgetsTheComposition() {
        toJapanese()
        letters("ame")
        input.textDidChangeExternally()
        #expect(input.composing?.isComposing == false)
    }

    @Test func withoutAConverterJapaneseTypesLetters() {
        input.phraseConverter = nil
        toJapanese()
        letters("ame")
        #expect(output.text == "ame")
        #expect(output.marked == "")
    }

    @Test func englishIsUnchanged() {
        letters("kyou")
        type(.space)
        #expect(output.text == "kyou ")
        #expect(output.marked == "")
        #expect(input.label(for: .space) == "English")
        #expect(input.composing == nil)
        #expect(converter.conversions == 0)
    }

    /// With Japanese on beside English, the English keys and their symbols are the Latin ones:
    /// Japanese's marks are its own layout's.
    @Test func englishKeysAreTheLatinOnesWithJapaneseOn() {
        #expect(input.scriptPack == nil)
        for page in [KeyboardPage.letters, .symbols, .moreSymbols] {
            if page != .letters { type(.page(page)) }
            #expect(input.layout == KeyboardLayout.forPage(page), "\(page)")
        }
    }
}

@MainActor
struct PhraseLayoutTests {
    let layout = japanesePack.layout

    @Test func theSymbolsPageHasTheJapaneseMarks() {
        let symbols = characters(.forPage(.symbols, native: layout, script: layout))
        for mark in ["、", "。", "ー", "？", "！", "・", "¥"] {
            #expect(symbols.contains(mark), "\(mark)")
        }
    }

    @Test func theMarksPageHasTheBrackets() {
        let marks = characters(.forPage(.native("marks"), native: layout, script: layout))
        for mark in ["「", "」", "、", "。", "ー", "〜", "『", "』"] {
            #expect(marks.contains(mark), "\(mark)")
        }
        #expect(layout?.longPress["「"]?.contains("『") == true)
    }
}

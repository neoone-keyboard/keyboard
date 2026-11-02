import Testing
import KeyboardCore

private func characters(_ layout: KeyboardLayout) -> [String] {
    layout.rows.flatMap { $0 }.compactMap { key in
        if case .character(let c) = key.action { return c }
        return nil
    }
}

private func actions(_ layout: KeyboardLayout) -> [KeyAction] {
    layout.rows.flatMap { $0 }.map(\.action)
}

@Test func lettersPageRows() {
    #expect(KeyboardLayout.forPage(.letters).rows.map(\.count) == [10, 9, 9, 6])
}

@Test func lettersPageContainsEveryLetterOnce() {
    let letters = characters(.forPage(.letters)).filter { $0 != "." }
    #expect(letters.sorted() == "abcdefghijklmnopqrstuvwxyz".map(String.init))
}

@Test func numberRowAddsDigitsOnTop() {
    let layout = KeyboardLayout.forPage(.letters, numberRow: true)
    #expect(layout.rows.count == 5)
    #expect(layout.rows[0].map(\.action) == "1234567890".map { .character(String($0)) })
}

@Test func hidingNextKeyboardRemovesGlobe() {
    for page in KeyboardPage.allCases {
        #expect(!actions(.forPage(page, showsNextKeyboard: false)).contains(.nextKeyboard))
        #expect(actions(.forPage(page)).contains(.nextKeyboard))
    }
}

@Test func everyPageHasSpaceBackspaceReturnAndLanguageToggle() {
    for page in KeyboardPage.allCases {
        let all = actions(.forPage(page))
        #expect(all.contains(.space), "page \(page)")
        #expect(all.contains(.backspace), "page \(page)")
        #expect(all.contains(.returnKey), "page \(page)")
        #expect(all.contains(.toggleLanguage), "page \(page)")
    }
}

@Test func topRowLettersHintTheirDigit() {
    let top = KeyboardLayout.forPage(.letters).rows[0]
    #expect(top.map { $0.alternates.first } == "1234567890".map { String($0) })
}

@Test func malayalamPagesContainEveryModernConsonantOnce() {
    let typed = characters(.forPage(.malayalam)) + characters(.forPage(.malayalamMore))
    let consonants = typed.filter { $0.unicodeScalars.count == 1 && (0x0D15...0x0D39).contains($0.unicodeScalars.first!.value) }
    let expected = (0x0D15...0x0D39).filter { $0 != 0x0D29 }.map { String(UnicodeScalar($0)!) } // U+0D29 ഩ is archaic
    #expect(consonants.sorted() == expected.sorted())
}

@Test func chilluOnLongPress() {
    let keys = KeyboardLayout.forPage(.malayalam).rows.flatMap { $0 }
    let na = keys.first { $0.action == .character("ന") }
    #expect(na?.alternates == ["ൻ"])
    let ka = keys.first { $0.action == .character("ക") }
    #expect(ka?.alternates == ["ൿ"])
}

@Test func malayalamPageKeysSwapToReturnHome() {
    let vowels = actions(.forPage(.malayalamVowels))
    #expect(vowels.contains(.page(.letters)))
    #expect(!vowels.contains(.page(.malayalamVowels)))
    let more = actions(.forPage(.malayalamMore))
    #expect(more.contains(.page(.letters)))
    #expect(!more.contains(.page(.malayalamMore)))
}

@Test func vowelSignsAreThirteenSingleCombiningMarks() {
    #expect(Malayalam.vowelSigns.count == 13)
    for sign in Malayalam.vowelSigns {
        #expect(sign.unicodeScalars.count == 1)
        #expect((0x0D3E...0x0D57).contains(sign.unicodeScalars.first!.value))
    }
}

@Test func vowelsPageHasEveryVowelSign() {
    let layout = KeyboardLayout.forPage(.malayalamVowels)
    let typed = characters(layout)
    for sign in Malayalam.vowelSigns {
        #expect(typed.contains(sign), "missing U+\(String(sign.unicodeScalars.first!.value, radix: 16))")
    }
    #expect(layout.rows.map(\.count) == [10, 10, 9, 7])
    #expect(layout.rows[2].last == Key(.backspace, width: 1.5))
    #expect(actions(layout).contains(.page(.letters)))
}

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

/// Every page there is with English and Malayalam: the Latin ones, then Malayalam's.
private let allPages: [KeyboardPage] = [.letters, .symbols, .moreSymbols] + ["letters", "more", "vowels"].map(KeyboardPage.native)

private func page(_ page: KeyboardPage, showsNextKeyboard: Bool = true) -> KeyboardLayout {
    page.isNative ? mlLayout(page, showsNextKeyboard: showsNextKeyboard) : .forPage(page, showsNextKeyboard: showsNextKeyboard)
}

/// layout.json's vowel signs: the strip less ം and ഃ.
private let vowelSigns: [String] = {
    let strip = malayalamPack.layout?.vowelSignStrip ?? []
    return strip.filter { $0 != "\u{0D02}" && $0 != "\u{0D03}" }
}()

@Test func lettersPageRows() {
    #expect(KeyboardLayout.forPage(.letters).rows.map(\.count) == [10, 9, 9, 7])
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
    for shown in allPages {
        #expect(!actions(page(shown, showsNextKeyboard: false)).contains(.nextKeyboard))
        #expect(actions(page(shown)).contains(.nextKeyboard))
    }
}

@Test func everyPageHasSpaceBackspaceReturnAndLanguageToggle() {
    for page in allPages {
        let all = actions(LayoutTestsSupport.layout(page))
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
    let typed = characters(mlLayout(.native("letters"))) + characters(mlLayout(.native("more")))
    let consonants = typed.filter { $0.unicodeScalars.count == 1 && (0x0D15...0x0D39).contains($0.unicodeScalars.first!.value) }
    let expected = (0x0D15...0x0D39).filter { $0 != 0x0D29 }.map { String(UnicodeScalar($0)!) } // U+0D29 ഩ is archaic
    #expect(consonants.sorted() == expected.sorted())
}

@Test func chilluOnLongPress() {
    let keys = mlLayout(.native("letters")).rows.flatMap { $0 }
    let na = keys.first { $0.action == .character("ന") }
    #expect(na?.alternates == ["ൻ"])
    let ka = keys.first { $0.action == .character("ക") }
    #expect(ka?.alternates == ["ൿ"])
}

@Test func malayalamPageKeysSwapToReturnHome() {
    let vowels = actions(mlLayout(.native("vowels")))
    #expect(vowels.contains(.page(.letters)))
    #expect(!vowels.contains(.page(.native("vowels"))))
    let more = actions(mlLayout(.native("more")))
    #expect(more.contains(.page(.letters)))
    #expect(!more.contains(.page(.native("more"))))
}

@Test func vowelSignsAreThirteenSingleCombiningMarks() {
    #expect(vowelSigns.count == 13)
    for sign in vowelSigns {
        #expect(sign.unicodeScalars.count == 1)
        #expect((0x0D3E...0x0D57).contains(sign.unicodeScalars.first!.value))
    }
}

@Test func vowelsPageHasEveryVowelSign() {
    let layout = mlLayout(.native("vowels"))
    let typed = characters(layout)
    for sign in vowelSigns {
        #expect(typed.contains(sign), "missing U+\(String(sign.unicodeScalars.first!.value, radix: 16))")
    }
    #expect(layout.rows.map(\.count) == [10, 10, 9, 8])
    #expect(layout.rows[2].last == Key(.backspace, width: 1.5))
    #expect(actions(layout).contains(.page(.letters)))
}

@Test func stripOffersAnusvaraAndVisargaBeforeVirama() {
    // ക → കാ … കൗ കം കഃ ക്
    let strip = malayalamPack.layout?.vowelSignStrip ?? []
    #expect(strip.suffix(3) == ["\u{0D02}", "\u{0D03}", "\u{0D4D}"])
    #expect(strip.dropLast(3) == vowelSigns.dropLast())
}

@Test func emojiKeyFollowsTheSymbolsKeyOnEveryPage() {
    for page in allPages {
        for globe in [true, false] {
            let bottom = LayoutTestsSupport.layout(page, showsNextKeyboard: globe).rows.last!
            #expect(bottom[1].action == .emoji, "page \(page)")
            if case .page = bottom[0].action {} else { Issue.record("page \(page) starts with \(bottom[0].action)") }
        }
    }
}

/// Without the globe (every Face ID iPhone) the bottom row fits in ten letter widths, so it
/// isn't scaled down, and space stays the widest key.
@Test func bottomRowFitsWithSpaceWidest() {
    for page in allPages {
        let bottom = LayoutTestsSupport.layout(page, showsNextKeyboard: false).rows.last!
        #expect(bottom.reduce(0) { $0 + $1.width } <= 10, "page \(page)")
        let space = bottom.first { $0.action == .space }!
        #expect(bottom.allSatisfy { $0.action == .space || $0.width < space.width }, "page \(page)")
        #expect(space.width >= 3.5, "page \(page)")
    }
}

/// A page that names no page of the layout still has keys: the Latin letters.
@Test func anUnknownNativePageFallsBackToLatinLetters() {
    #expect(mlLayout(.native("nope")) == .forPage(.letters))
    #expect(KeyboardLayout.forPage(.native("letters")) == .forPage(.letters))
}

/// A layout of fewer pages has fewer page keys; space takes their width.
@Test func aOnePageLayoutHasNoPageKeys() {
    let one = PackLayout(spaceBar: "x", languageKey: "x", pages: [.init(id: "letters", label: "x", rows: [["a"]])])
    let bottom = KeyboardLayout.forPage(.native("letters"), native: one, showsNextKeyboard: false).rows.last!
    #expect(!bottom.contains { if case .page(.native) = $0.action { true } else { false } })
    #expect(bottom.reduce(0) { $0 + $1.width } == 10)
}

private enum LayoutTestsSupport {
    static func layout(_ shown: KeyboardPage, showsNextKeyboard: Bool = true) -> KeyboardLayout {
        page(shown, showsNextKeyboard: showsNextKeyboard)
    }
}

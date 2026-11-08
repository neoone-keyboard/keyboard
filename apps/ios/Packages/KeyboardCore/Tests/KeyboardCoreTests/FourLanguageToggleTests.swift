import Testing
@testable import KeyboardCore

/// English, Malayalam, Hindi and Assamese at once (`ActiveLanguages.maxActive`): the language
/// key cycles all four, says where it goes next, and every page of every layout is its own
/// keys. Every pack's first page is `.native("letters")`, so the keys shown must name the
/// language too, or the extension keeps drawing the previous language's keys.
@MainActor
struct FourLanguageToggleTests {
    let hindi = Language.pack(pack("hi"))
    let assamese = Language.pack(pack("as"))
    var all: [Language] { [.english, malayalam, hindi, assamese] }

    /// The characters on the keys shown.
    private func characters(_ input: InputController) -> [String] {
        input.layout.rows.flatMap { $0 }.compactMap { key in
            if case .character(let c) = key.action { c } else { nil }
        }
    }

    /// Each language's pages, in its layout's order.
    private func pages(_ language: Language) -> [KeyboardPage] {
        guard let layout = language.nativeLayout else { return [.letters, .symbols, .moreSymbols] }
        return layout.pages.map { .native($0.id) }
    }

    @Test func theLanguageKeyCyclesAllFourAndNamesTheNext() {
        let input = InputController(output: RecordingOutput(), languages: all)
        var seen: [String] = []
        for step in 0..<all.count {
            seen.append(input.language.id)
            let next = all[(step + 1) % all.count]
            #expect(input.label(for: .toggleLanguage) == (next.nativeLayout?.languageKey ?? "EN"))
            input.handle(.toggleLanguage, at: Double(step))
        }
        #expect(seen == ["en", "ml", "hi", "as"])
        #expect(input.language == .english, "round to English")
        // The key's labels: മ from English, then हि, then অ, then EN.
        #expect(all.map { $0.nativeLayout?.languageKey ?? "EN" } == ["EN", "മ", "हि", "অ"])
    }

    @Test func everyPageOfEveryLayoutIsItsOwnKeys() {
        let input = InputController(output: RecordingOutput(), languages: all)
        var shown: [KeysShown: [String]] = [:]
        for (step, language) in all.enumerated() {
            #expect(input.language == language)
            #expect(input.page == pages(language)[0], "\(language.id) starts on its letters")
            for page in pages(language) {
                input.handle(.page(page), at: Double(step))
                let keys = input.keysShown
                #expect(keys == KeysShown(language: language.id, page: page))
                #expect(shown[keys] == nil, "\(keys) seen twice")
                shown[keys] = characters(input)
                if case .native(let id) = page, let layout = language.nativeLayout?.page(id) {
                    let expected = Set(layout.rows.flatMap { $0 })
                    #expect(Set(characters(input)).isSuperset(of: expected), "\(language.id) \(id)")
                }
            }
            // From its last page, not its letters: the key still goes to the next language's
            // first page.
            input.handle(.toggleLanguage, at: Double(step))
        }
        #expect(shown.count == all.map { pages($0).count }.reduce(0, +))
        // The three letters pages share an id but no keys.
        let letters = ["ml", "hi", "as"].map { Set(shown[KeysShown(language: $0, page: .native("letters"))] ?? []) }
        #expect(letters.allSatisfy { !$0.isEmpty })
        #expect(letters[0].isDisjoint(with: letters[1]))
        #expect(letters[1].isDisjoint(with: letters[2]))
        #expect(letters[0].isDisjoint(with: letters[2]))
    }

    @Test func droppingALanguageWhileOnItGoesBackToTheFirst() {
        let input = InputController(output: RecordingOutput(), languages: all)
        for step in 0..<3 { input.handle(.toggleLanguage, at: Double(step)) }
        #expect(input.language == assamese)
        input.languages = [.english, malayalam, hindi]
        #expect(input.language == .english)
        #expect(input.keysShown == KeysShown(language: "en", page: .letters))
        // Staying on a language still listed keeps it.
        input.handle(.toggleLanguage, at: 4)
        input.languages = [.english, malayalam]
        #expect(input.language == malayalam)
    }
}

import Foundation
import Testing
import KeyboardCore

/// The native keyboard comes from packs/ml.neopack: its manifest (`PackDescriptor`) and its
/// layout.json (`PackLayout`, format in docs/design/pack-format.md).
@MainActor
struct PackLayoutTests {
    @Test func theManifestDescribesMalayalam() throws {
        #expect(malayalamPack.id == "ml")
        #expect(malayalamPack.name == "Malayalam")
        #expect(malayalamPack.nativeName == "മലയാളം")
        #expect(malayalamPack.script == "mlym")
        #expect(malayalamPack.romanisedName == "Manglish")
        let layout = try #require(malayalamPack.layout)
        #expect(layout.pages.map(\.id) == ["letters", "more", "vowels"])
        #expect(layout.pages.map(\.label) == ["കഖ", "ശഷ", "അആ"])
    }

    @Test func labelsComeFromTheLayout() throws {
        let layout = try #require(malayalamPack.layout)
        let input = InputController(output: RecordingOutput(), languages: englishAndMalayalam)
        #expect(input.label(for: .toggleLanguage) == layout.languageKey)
        input.handle(.toggleLanguage, at: 0)
        #expect(input.language == malayalam)
        #expect(input.page == .native("letters"))
        #expect(input.label(for: .space) == layout.spaceBar)
        #expect(layout.pages.map { input.label(for: .page(.native($0.id))) } == layout.pages.map(\.label))
        #expect(input.label(for: .page(.letters)) == layout.pages[0].label)
        #expect(input.label(for: .toggleLanguage) == "EN")
    }

    @Test func pagesTypeWhatTheFileLists() throws {
        let layout = try #require(malayalamPack.layout)
        for page in layout.pages {
            let rows = mlLayout(.native(page.id)).rows.dropLast().map { row in
                row.compactMap { key -> String? in if case .character(let c) = key.action { c } else { nil } }
            }
            #expect(rows == page.rows, "page \(page.id)")
        }
        let keys = layout.pages.map { mlLayout(.native($0.id)) }.flatMap(\.rows).joined()
        for (text, options) in layout.longPress {
            #expect(keys.first { $0.action == .character(text) }?.alternates == options, "\(text)")
        }
    }

    @Test func aLayoutOfAnotherFormatOrWithoutPagesIsRefused() {
        let json = { (format: Int, pages: String) in
            Data(#"{"format": \#(format), "spaceBar": "x", "languageKey": "x", "pages": \#(pages)}"#.utf8)
        }
        #expect(throws: PackLayout.Invalid.self) { try PackLayout.parse(json(2, #"[{"id": "a", "label": "a", "rows": [["a"]]}]"#)) }
        #expect(throws: PackLayout.Invalid.self) { try PackLayout.parse(json(1, "[]")) }
        #expect(throws: PackLayout.Invalid.self) { try PackLayout.parse(json(1, #"[{"id": "a", "label": "a", "rows": [[""]]}]"#)) }
        // Optional fields may be missing and unknown ones are ignored.
        let parsed = try? PackLayout.parse(Data(#"{"format": 1, "spaceBar": "x", "languageKey": "y", "later": 1, "pages": [{"id": "a", "label": "a", "rows": [["a"]]}]}"#.utf8))
        #expect(parsed?.longPress == [:])
        #expect(parsed?.vowelSignStrip == [])
    }

    /// Every committed pack has a layout (Hindi and Assamese since M6 phase 4).
    @Test func scanningFindsEveryPack() {
        let packs = PackDescriptor.scan(packsFolder)
        #expect(Set(packs.map(\.id)).isSuperset(of: ["ml", "hi", "as"]))
        for pack in packs {
            #expect(pack.layout != nil, "\(pack.id) has no layout.json")
        }
        #expect(PackDescriptor.scan(packsFolder.appending(path: "nonexistent")).isEmpty)
    }

    /// Hindi's and Assamese's layouts (docs/design/pack-format.md, "layout.json"):
    /// with English, the language key goes EN → the pack's letter and back, and every page draws
    /// what the file lists, as Malayalam's does, with no code of their own.
    @Test(arguments: [
        ("hi", "हि", "हिन्दी", ["कख", "शष", "अआ"], ["ा", "ि", "ी", "ु", "ू", "ृ", "े", "ै", "ो", "ौ", "ं", "ँ", "ः", "्", "़"]),
        ("as", "অ", "অসমীয়া", ["কখ", "শষ", "অআ"], ["া", "ি", "ী", "ু", "ূ", "ৃ", "ে", "ৈ", "ো", "ৌ", "ং", "ঁ", "ঃ", "ৎ", "্"]),
    ])
    func aPacksNativeKeyboardComesFromItsLayout(id: String, key: String, spaceBar: String, labels: [String], strip: [String]) throws {
        let descriptor = pack(id)
        let layout = try #require(descriptor.layout, "\(id) has a layout.json")
        #expect(layout.languageKey == key)
        #expect(layout.spaceBar == spaceBar)
        #expect(layout.pages.map(\.id) == ["letters", "more", "vowels"])
        #expect(layout.pages.map(\.label) == labels)
        #expect(layout.vowelSignStrip == strip, "the design's strip order")

        let language = Language.pack(descriptor)
        let input = InputController(output: RecordingOutput(), languages: [.english, language])
        #expect(input.language == .english)
        #expect(input.label(for: .toggleLanguage) == key)
        input.handle(.toggleLanguage, at: 0)
        #expect(input.language == language)
        #expect(input.page == .native("letters"))
        #expect(input.label(for: .space) == spaceBar)
        #expect(input.label(for: .toggleLanguage) == "EN")
        #expect(layout.pages.map { input.label(for: .page(.native($0.id))) } == labels)

        for page in layout.pages {
            let drawn = KeyboardLayout.forPage(.native(page.id), native: layout)
            let rows = drawn.rows.dropLast().map { row in
                row.compactMap { key -> String? in if case .character(let c) = key.action { c } else { nil } }
            }
            #expect(rows == page.rows, "\(id) page \(page.id)")
            #expect(drawn.rows.last?.contains { $0.action == .space } == true)
        }
        let keys = layout.pages.map { KeyboardLayout.forPage(.native($0.id), native: layout) }.flatMap(\.rows).joined()
        for (text, options) in layout.longPress {
            #expect(keys.first { $0.action == .character(text) }?.alternates == options, "\(text)")
        }

        input.handle(.toggleLanguage, at: 0)
        #expect(input.language == .english)
        #expect(input.page == .letters)
    }

    @Test func theLanguageKeyCyclesTheLanguagesGiven() {
        let input = InputController(output: RecordingOutput(), languages: [malayalam])
        #expect(input.language == malayalam)
        #expect(input.page == .native("letters"))
        input.languages = englishAndMalayalam
        #expect(input.language == malayalam, "a language still listed stays")
        input.languages = [.english]
        #expect(input.language == .english)
        #expect(input.page == .letters)
        input.languages = []
        #expect(input.languages == [.english])
    }
}

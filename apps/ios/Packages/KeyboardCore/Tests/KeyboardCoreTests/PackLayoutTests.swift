import Foundation
import Testing
import KeyboardCore

/// The native keyboard comes from packs/ml.neopack: its manifest (`PackDescriptor`) and its
/// layout.json (`PackLayout`, format in docs/design/2026-11-06-m6-language-packs-design.md).
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

    /// Only the bundled pack has a layout today; a pack without one is still described.
    @Test func scanningFindsEveryPack() {
        let packs = PackDescriptor.scan(packsFolder)
        #expect(packs.map(\.id).contains("ml"))
        #expect(packs.first { $0.id == "ml" }?.layout != nil)
        for pack in packs where pack.id != "ml" {
            #expect(pack.layout == nil, "\(pack.id) has a layout.json now: check it draws")
        }
        #expect(PackDescriptor.scan(packsFolder.appending(path: "nonexistent")).isEmpty)
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

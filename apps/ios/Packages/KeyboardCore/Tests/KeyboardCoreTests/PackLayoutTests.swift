import Foundation
import Testing
import KeyboardCore

/// packs/ml.neopack/layout.json, found from this file's path
/// (…/apps/ios/Packages/KeyboardCore/Tests/KeyboardCoreTests/).
private let layoutFile = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent()
    .appendingPathComponent("packs/ml.neopack/layout.json")

/// The format in docs/design/2026-11-06-m6-language-packs-design.md ("layout.json").
private struct PackLayout: Decodable {
    struct Page: Decodable {
        let id: String
        let label: String
        let rows: [[String]]
    }

    let format: Int
    let spaceBar: String
    let languageKey: String
    let pages: [Page]
    let longPress: [String: [String]]
    let vowelSignStrip: [String]
}

private func load() throws -> PackLayout {
    try JSONDecoder().decode(PackLayout.self, from: Data(contentsOf: layoutFile))
}

/// The keys of a page that type text, row by row.
private func characterRows(_ page: KeyboardPage) -> [[Key]] {
    KeyboardLayout.forPage(page).rows.dropLast().map { row in
        row.filter { if case .character = $0.action { true } else { false } }
    }
}

private func text(_ key: Key) -> String {
    if case .character(let c) = key.action { return c }
    return ""
}

/// Until the keyboard reads layout.json (phase 2, task 11), the file must say exactly what
/// the compiled-in Malayalam layout does.
@MainActor
struct PackLayoutTests {
    private let pages: [KeyboardPage] = [.malayalam, .malayalamMore, .malayalamVowels]

    @Test func pagesEqualTheCompiledInLayout() throws {
        let layout = try load()
        #expect(layout.format == 1)
        #expect(layout.pages.map(\.id) == ["letters", "more", "vowels"])
        for (page, filed) in zip(pages, layout.pages) {
            #expect(characterRows(page).map { $0.map(text) } == filed.rows, "page \(filed.id)")
        }
    }

    @Test func longPressEqualsTheCompiledInAlternates() throws {
        let layout = try load()
        var alternates: [String: [String]] = [:]
        for page in pages {
            for key in characterRows(page).joined() where !key.alternates.isEmpty {
                alternates[text(key)] = key.alternates
            }
        }
        #expect(layout.longPress == alternates)
    }

    @Test func labelsEqualTheCompiledInOnes() throws {
        let layout = try load()
        let input = InputController(output: RecordingOutput())
        #expect(input.label(for: .toggleLanguage) == layout.languageKey)
        input.handle(.toggleLanguage, at: 0)
        #expect(input.language == .malayalam)
        #expect(input.label(for: .space) == layout.spaceBar)
        #expect(pages.map { input.label(for: .page($0)) } == layout.pages.map(\.label))
        #expect(input.label(for: .page(.letters)) == layout.pages[0].label)
    }

    @Test func vowelSignStripEqualsTheCompiledInOne() throws {
        #expect(try load().vowelSignStrip == Malayalam.stripSigns)
    }
}

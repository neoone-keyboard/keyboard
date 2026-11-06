import Foundation

/// A pack's `layout.json` (format 1): the native-script keyboard. The format is in
/// docs/design/2026-11-06-m6-language-packs-design.md ("layout.json"); the engine
/// reads the same file for the strip's order (engine/core/src/pack_layout.rs).
public struct PackLayout: Equatable, Sendable {
    public struct Page: Equatable, Sendable, Decodable {
        /// "letters", "more", "vowels".
        public let id: String
        /// The label of the key that shows this page: "കഖ".
        public let label: String
        /// The text each key types, row by row. The keyboard adds its own keys.
        public let rows: [[String]]

        public init(id: String, label: String, rows: [[String]]) {
            self.id = id
            self.label = label
            self.rows = rows
        }
    }

    /// The space bar's name while this layout is up: "മലയാളം".
    public let spaceBar: String
    /// The language key's label on another layout, to switch to this one: "മ".
    public let languageKey: String
    /// The pages, the first being the alphabet the layout opens on.
    public let pages: [Page]
    /// Long-press options by key text: a consonant's chillu.
    public let longPress: [String: [String]]
    /// What the strip offers after a letter a sign can attach to, in the order shown when the
    /// engine has no preference. Empty: no strip.
    public let vowelSignStrip: [String]

    public struct Invalid: Error, CustomStringConvertible {
        public let description: String
    }

    public init(spaceBar: String, languageKey: String, pages: [Page], longPress: [String: [String]] = [:], vowelSignStrip: [String] = []) {
        self.spaceBar = spaceBar
        self.languageKey = languageKey
        self.pages = pages
        self.longPress = longPress
        self.vowelSignStrip = vowelSignStrip
    }

    public static func load(_ url: URL) throws -> PackLayout {
        try parse(Data(contentsOf: url))
    }

    /// Refuses another format, a layout without pages, and empty keys, as the engine does.
    public static func parse(_ data: Data) throws -> PackLayout {
        let file = try JSONDecoder().decode(File.self, from: data)
        guard file.format == 1 else { throw Invalid(description: "layout format \(file.format), this keyboard reads 1") }
        guard !file.pages.isEmpty else { throw Invalid(description: "a layout has at least one page") }
        for page in file.pages where page.rows.joined().contains("") {
            throw Invalid(description: "page \(page.id) has an empty key")
        }
        return PackLayout(
            spaceBar: file.spaceBar, languageKey: file.languageKey, pages: file.pages,
            longPress: file.longPress ?? [:], vowelSignStrip: file.vowelSignStrip ?? []
        )
    }

    /// The alphabet: the page the layout opens on.
    public var alphabet: Page { pages[0] }

    public func page(_ id: String) -> Page? {
        pages.first { $0.id == id }
    }

    /// The strip's signs with the engine's likeliest first (`preferred`, most likely first),
    /// then the rest in `vowelSignStrip` order. Unknown signs in `preferred` are ignored, so
    /// the strip always offers exactly `vowelSignStrip`.
    public func orderedStripSigns(preferred: [String]) -> [String] {
        var seen = Set<String>()
        let first = preferred.filter { vowelSignStrip.contains($0) && seen.insert($0).inserted }
        return first + vowelSignStrip.filter { !seen.contains($0) }
    }

    /// Unknown keys are ignored, so later versions can add fields.
    private struct File: Decodable {
        let format: Int
        let spaceBar: String
        let languageKey: String
        let pages: [Page]
        let longPress: [String: [String]]?
        let vowelSignStrip: [String]?
    }
}

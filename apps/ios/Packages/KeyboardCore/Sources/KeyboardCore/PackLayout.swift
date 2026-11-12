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
    /// The script's own marks for the Latin punctuation keys (`punctuation`, optional):
    /// Urdu's {".": "۔", ",": "،", "?": "؟", ";": "؛"}. After a word of the script those keys
    /// type its marks, and so do two spaces; the symbols page shows them. Empty: Latin marks.
    public let punctuation: [String: String]
    /// The script's digits, zero to nine (`digits`, optional): Urdu's ۰ to ۹. The symbols
    /// page's top row, and long-press options on the Latin letters' top row. Empty unless ten.
    public let digits: [String]
    /// The symbols page's currency sign (`currency`, optional): Urdu's ₨. Nil: the
    /// keyboard's own (₹).
    public let currency: String?

    public struct Invalid: Error, CustomStringConvertible {
        public let description: String
    }

    public init(spaceBar: String, languageKey: String, pages: [Page], longPress: [String: [String]] = [:], vowelSignStrip: [String] = [],
                punctuation: [String: String] = [:], digits: [String] = [], currency: String? = nil) {
        self.spaceBar = spaceBar
        self.languageKey = languageKey
        self.pages = pages
        self.longPress = longPress
        self.vowelSignStrip = vowelSignStrip
        self.punctuation = punctuation.filter { !$0.key.isEmpty && !$0.value.isEmpty }
        self.digits = digits.count == 10 && !digits.contains("") ? digits : []
        self.currency = currency?.isEmpty == false ? currency : nil
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
            longPress: file.longPress ?? [:], vowelSignStrip: file.vowelSignStrip ?? [],
            punctuation: file.punctuation?.value ?? [:], digits: file.digits?.value ?? [],
            currency: file.currency?.value ?? nil
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
        let punctuation: Lenient<[String: String]>?
        let digits: Lenient<[String]>?
        let currency: Lenient<String>?
    }

    /// An optional field that is left out, not refused, when it isn't what it should be: the
    /// layout still types without the script's marks or digits.
    private struct Lenient<Value: Decodable>: Decodable {
        let value: Value?

        init(from decoder: Decoder) throws {
            value = try? decoder.singleValueContainer().decode(Value.self)
        }
    }
}

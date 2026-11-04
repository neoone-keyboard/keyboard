import Foundation

/// Which emoji this iPhone can draw. Apple adds each Emoji version in a point release some
/// months after Unicode publishes it; an emoji drawn on an older system is an empty box.
public enum EmojiSupport {
    /// The first iOS with each Emoji version's images; anything up to 13.1 has been drawn
    /// since iOS 14.5. A version past the table is hidden until the table learns it.
    private static let firstSystem: [(emoji: (Int, Int), system: (Int, Int))] = [
        ((13, 1), (14, 5)),
        ((14, 0), (15, 4)),
        ((15, 0), (16, 4)),
        ((15, 1), (17, 4)),
        ((16, 0), (18, 4)),
        ((17, 0), (26, 4)),
    ]

    /// Whether emoji of `version` ("15.1", as in emoji.txt) are drawn on `system`.
    public static func isSupported(_ version: String, on system: OperatingSystemVersion) -> Bool {
        let parts = version.split(separator: ".").compactMap { Int($0) }
        guard parts.count == 2 else { return false }
        let wanted = (parts[0], parts[1])
        guard let entry = firstSystem.first(where: { $0.emoji >= wanted }) else { return false }
        return (system.majorVersion, system.minorVersion) >= entry.system
    }

    /// Whether `character` is an emoji (one that a backspace should take whole), rather than
    /// a letter with its marks such as `കി`.
    public static func isEmoji(_ character: Character) -> Bool {
        let scalars = character.unicodeScalars
        if scalars.count == 1 { return scalars.first?.properties.isEmojiPresentation ?? false }
        return scalars.contains { scalar in
            scalar.properties.isEmojiPresentation || scalar == "\u{FE0F}" || scalar == "\u{200D}" || scalar == "\u{20E3}"
                || isTone(scalar) || (0x1F1E6...0x1F1FF).contains(scalar.value) // regional indicators: flags
        }
    }

    /// The emoji without skin tones or FE0F: a toned emoji and its base have the same one.
    static func toneless(_ emoji: String) -> String {
        var scalars = String.UnicodeScalarView()
        scalars.append(contentsOf: emoji.unicodeScalars.filter { !isTone($0) && $0 != "\u{FE0F}" })
        return String(scalars)
    }

    private static func isTone(_ scalar: Unicode.Scalar) -> Bool {
        (0x1F3FB...0x1F3FF).contains(scalar.value)
    }
}

/// One emoji in the panel and its skin tones, light to dark (none if it takes none).
public struct EmojiItem: Equatable, Sendable {
    public let emoji: String
    public let tones: [String]
}

/// One of Unicode's emoji groups ("Smileys & Emotion"), in Unicode's order.
public struct EmojiCategory: Equatable, Sendable {
    public let name: String
    public let items: [EmojiItem]
}

/// The emoji panel's contents, from the bundled emoji.txt (data/pipeline/emoji_data.py):
/// "=Group" lines, then "emoji version [toneVersion tone…]" per emoji. Only what `system`
/// draws is kept. The panel holds it only while it shows.
public struct EmojiCatalog: Sendable {
    public let categories: [EmojiCategory]
    /// Each toned emoji's base.
    private let bases: [String: String]
    private let tonesByBase: [String: [String]]

    public init(text: String, system: OperatingSystemVersion = ProcessInfo.processInfo.operatingSystemVersion) {
        var categories: [EmojiCategory] = []
        var name: String?
        var items: [EmojiItem] = []
        var bases: [String: String] = [:]
        var tonesByBase: [String: [String]] = [:]
        func finishCategory() {
            if let name, !items.isEmpty { categories.append(EmojiCategory(name: name, items: items)) }
            items = []
        }
        for line in text.split(separator: "\n") {
            if line.hasPrefix("#") { continue }
            if line.hasPrefix("=") {
                finishCategory()
                name = String(line.dropFirst())
                continue
            }
            let fields = line.split(separator: " ").map(String.init)
            guard fields.count >= 2, EmojiSupport.isSupported(fields[1], on: system) else { continue }
            let tones = fields.count > 3 && EmojiSupport.isSupported(fields[2], on: system) ? Array(fields[3...]) : []
            items.append(EmojiItem(emoji: fields[0], tones: tones))
            if !tones.isEmpty {
                tonesByBase[fields[0]] = tones
                for tone in tones { bases[tone] = fields[0] }
            }
            bases[fields[0]] = fields[0]
        }
        finishCategory()
        self.categories = categories
        self.bases = bases
        self.tonesByBase = tonesByBase
    }

    /// The emoji `emoji` is a skin tone of, or itself; nil if it isn't in the panel.
    public func base(of emoji: String) -> String? {
        bases[emoji]
    }

    /// The skin tones of `emoji` (a base or one of its tones), light to dark.
    public func tones(of emoji: String) -> [String] {
        base(of: emoji).flatMap { tonesByBase[$0] } ?? []
    }
}

/// The emoji the user picked lately and the skin tone chosen for each emoji, kept in the
/// keyboard's own container (`EmojiHistory.load`) and forgotten with the learned words.
public struct EmojiHistory: Equatable, Codable, Sendable {
    public static let recentLimit = 30

    /// Most recent first, as inserted (with its tone); one entry per emoji whatever its tone.
    public private(set) var recents: [String] = []
    /// Base → its chosen tone; no entry means no tone.
    private var tones: [String: String] = [:]

    public init() {}

    public mutating func record(_ emoji: String) {
        let key = EmojiSupport.toneless(emoji)
        recents.removeAll { EmojiSupport.toneless($0) == key }
        recents.insert(emoji, at: 0)
        if recents.count > Self.recentLimit { recents.removeLast(recents.count - Self.recentLimit) }
    }

    /// Remembers `tone` (one of `base`'s, or `base` itself for none) for `base`.
    public mutating func chooseTone(_ tone: String, for base: String) {
        tones[base] = tone == base ? nil : tone
    }

    /// `base` in its chosen tone.
    public func toned(_ base: String) -> String {
        tones[base] ?? base
    }

    /// An empty history when there is no file yet or it can't be read.
    public static func load(from url: URL) -> EmojiHistory {
        guard let data = try? Data(contentsOf: url) else { return EmojiHistory() }
        return (try? JSONDecoder().decode(EmojiHistory.self, from: data)) ?? EmojiHistory()
    }

    public func save(to url: URL) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try JSONEncoder().encode(self).write(to: url, options: .atomic)
    }

    /// Deletes the file at `url`, if there is one ("Forget learned words").
    public static func forget(at url: URL) throws {
        guard FileManager.default.fileExists(atPath: url.path) else { return }
        try FileManager.default.removeItem(at: url)
    }
}

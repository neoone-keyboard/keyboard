import Foundation

/// How the suggestion bar lays out its words (`SuggestionBar.ordered`): as many whole words
/// as fit across it at their own width, sharing whatever room is left, and the rest past its
/// trailing edge, reached by sliding the bar. A word is never cut: a first word wider than the
/// bar is drawn smaller (`firstScale`) instead.
///
/// With more words than fit and the hint on, a small mark at the trailing edge says so
/// (`showsHint`); the words then fit in the bar less the mark's room, so none is under it.
///
/// Right to left the bar is a mirror image: the first word at the right edge, the rest
/// leftward, and the bar shown from its right end (`startOffset`).
public struct BarFit: Equatable, Sendable {
    public struct Spacing: Equatable, Sendable {
        /// Room each side of a word.
        public var padding: Double = 12
        /// Between two words.
        public var gap: Double = 3
        /// The narrowest a word's button is, for a finger.
        public var minimumWidth: Double = 44
        /// The trailing edge's room for the hint.
        public var hintWidth: Double = 18

        public init() {}
    }

    /// Where one word is drawn in the bar's scrolled content, in points from its left.
    public struct Item: Equatable, Sendable {
        public var x: Double
        public var width: Double
    }

    /// One per word, in the bar's order (the default first).
    public let items: [Item]
    /// How many of `items`, from the first, are in view before the bar is slid.
    public let visibleCount: Int
    /// The scrolled content's width: the bar's own when every word fits.
    public let contentWidth: Double
    /// The first word's text is drawn at this fraction of its size (1 unless it is wider than the bar).
    public let firstScale: Double
    /// The trailing edge shows the hint: there is more, and the hint is on.
    public let showsHint: Bool
    /// The content offset the bar starts at: its right end when right to left.
    public let startOffset: Double

    /// Some words are past the edge.
    public var overflows: Bool { visibleCount < items.count }

    /// `textWidths`: each word's text as drawn at the bar's size, in the bar's order.
    public init(
        textWidths: [Double], barWidth: Double, rightToLeft: Bool = false,
        hintEnabled: Bool = true, spacing: Spacing = Spacing()
    ) {
        let natural = textWidths.map { max($0 + 2 * spacing.padding, spacing.minimumWidth) }
        var count = Self.fitting(natural, in: barWidth, gap: spacing.gap)
        let overflows = count < natural.count
        let hint = overflows && hintEnabled
        let room = hint ? max(barWidth - spacing.hintWidth, 0) : barWidth
        if hint { count = Self.fitting(natural, in: room, gap: spacing.gap) }

        var items: [Item] = []
        items.reserveCapacity(natural.count)
        var scale = 1.0
        if count > 0 {
            let used = natural.prefix(count).reduce(0, +) + Double(count - 1) * spacing.gap
            if used > room, let first = textWidths.first, first > 0 {
                // Only ever the first word alone: shrink its text into the room less its padding.
                scale = min(1, max(room - 2 * spacing.padding, 1) / first)
            }
            // The words in view share the spare room, so they fill the bar edge to edge.
            let share = max(room - used, 0) / Double(count)
            var x = 0.0
            for width in natural.prefix(count) {
                let drawn = used > room ? room : width + share
                items.append(Item(x: x, width: drawn))
                x += drawn + spacing.gap
            }
            // The rest begin past the edge (and past the hint's room), each at its own width.
            if overflows { x = barWidth + spacing.gap }
            for width in natural.dropFirst(count) {
                items.append(Item(x: x, width: width))
                x += width + spacing.gap
            }
        }
        let content = overflows ? (items.last.map { $0.x + $0.width } ?? barWidth) : barWidth
        if rightToLeft {
            items = items.map { Item(x: content - $0.x - $0.width, width: $0.width) }
        }
        self.items = items
        visibleCount = count
        contentWidth = content
        firstScale = scale
        showsHint = hint
        startOffset = rightToLeft ? content - barWidth : 0
    }

    /// How many of `widths`, from the first, fit `room` with `gap` between them; the first
    /// always counts, even when it alone is wider.
    private static func fitting(_ widths: [Double], in room: Double, gap: Double) -> Int {
        var used = 0.0
        var count = 0
        for width in widths {
            let need = width + (count > 0 ? gap : 0)
            if count > 0 && used + need > room + 0.0001 { break }
            used += need
            count += 1
        }
        return count
    }
}

/// Each word's measured width, kept so a word is measured once rather than on every key.
/// Holds at most `capacity` words and starts again when full: the bar's words change with
/// every key, and an old word is cheap to measure again.
public struct TextWidthCache: Sendable {
    public let capacity: Int
    private var widths: [String: Double] = [:]

    public init(capacity: Int = 512) {
        self.capacity = max(capacity, 1)
    }

    public var count: Int { widths.count }

    /// `key`'s width, measured by `measure` the first time it is asked for. The key names the
    /// word and the font it is drawn in.
    public mutating func width(of key: String, measure: (String) -> Double) -> Double {
        if let known = widths[key] { return known }
        let width = measure(key)
        if widths.count >= capacity { widths.removeAll(keepingCapacity: true) }
        widths[key] = width
        return width
    }

    public mutating func removeAll() { widths.removeAll(keepingCapacity: true) }
}

/// Settings the app keeps in the App Group's shared defaults for the keyboard to read.
/// Android keeps the same keys.
public enum SharedSettings {
    /// "Hint when there are more suggestions": the bar's trailing edge marks words past it.
    public static let suggestionMoreHintKey = "suggestionMoreHint"

    /// The App Group's shared defaults; nil without the App Group (an unsigned build).
    public static func appGroupDefaults() -> UserDefaults? {
        guard FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: PackLocations.appGroup) != nil
        else { return nil }
        return UserDefaults(suiteName: PackLocations.appGroup)
    }

    /// Where those defaults are saved, which the keyboard reads directly if its defaults have
    /// nothing (it can read the App Group's files without Full Access).
    public static func appGroupPreferencesFile(fileManager: FileManager = .default) -> URL? {
        fileManager.containerURL(forSecurityApplicationGroupIdentifier: PackLocations.appGroup)?
            .appending(path: "Library/Preferences/\(PackLocations.appGroup).plist")
    }

    /// Whether the bar hints at more suggestions: on unless turned off, in `defaults` or else
    /// in `preferencesFile`.
    public static func suggestionMoreHint(defaults: UserDefaults?, preferencesFile: URL? = nil) -> Bool {
        if let on = defaults?.object(forKey: suggestionMoreHintKey) as? Bool { return on }
        if let file = preferencesFile, let data = try? Data(contentsOf: file),
           let saved = try? PropertyListSerialization.propertyList(from: data, format: nil) as? [String: Any],
           let on = saved[suggestionMoreHintKey] as? Bool {
            return on
        }
        return true
    }
}

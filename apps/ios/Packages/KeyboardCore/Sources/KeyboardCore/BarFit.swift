import Foundation

/// How the suggestion bar lays out its slots (`SuggestionBar.laidOut`): three equal slots
/// across it, left, middle (the default, which space commits) and right, as the bar has
/// always had them, and any more past its trailing edge, each as wide as one of the three,
/// reached by sliding the bar. Every slot is wide enough to tap; a long word shrinks into it.
///
/// With more than three and the hint on, a small mark at the trailing edge says so
/// (`showsHint`); the third slot's word is then kept clear of it (`hintInset`).
///
/// Right to left the bar is a mirror image: the left slot on the right, the default still in
/// the middle, the rest past the left edge, and the bar shown from its right end (`startOffset`).
public struct BarFit: Equatable, Sendable {
    /// The slots in view before the bar is slid.
    public static let visibleSlots = 3

    public struct Spacing: Equatable, Sendable {
        /// Between two slots.
        public var gap: Double = 3
        /// The trailing edge's room for the hint.
        public var hintWidth: Double = 18

        public init() {}
    }

    /// Where one slot is drawn in the bar's scrolled content, in points from its left.
    public struct Item: Equatable, Sendable {
        public var x: Double
        public var width: Double
    }

    /// One per slot, in the bar's order: left, middle, right, then the rest.
    public let items: [Item]
    /// How many of `items`, from the first, are in view before the bar is slid.
    public let visibleCount: Int
    /// Every slot's width: a third of the bar, less the gaps between the three.
    public let slotWidth: Double
    /// The scrolled content's width: the bar's own with three slots or fewer.
    public let contentWidth: Double
    /// The trailing edge shows the hint: there are more than three, and the hint is on.
    public let showsHint: Bool
    /// Room the third slot leaves at the trailing edge for the hint: its word stays clear of it.
    public let hintInset: Double
    /// The content offset the bar starts at: its right end when right to left.
    public let startOffset: Double

    /// Some slots are past the edge.
    public var overflows: Bool { visibleCount < items.count }

    /// `count`: how many slots the bar has, the three and any more.
    public init(
        count: Int, barWidth: Double, rightToLeft: Bool = false,
        hintEnabled: Bool = true, spacing: Spacing = Spacing()
    ) {
        let shown = Double(Self.visibleSlots)
        let width = max((barWidth - (shown - 1) * spacing.gap) / shown, 0)
        var items: [Item] = []
        items.reserveCapacity(max(count, 0))
        for index in 0..<max(count, 0) {
            items.append(Item(x: Double(index) * (width + spacing.gap), width: width))
        }
        let content = max(barWidth, items.last.map { $0.x + $0.width } ?? 0)
        if rightToLeft {
            items = items.map { Item(x: content - $0.x - $0.width, width: $0.width) }
        }
        let hint = items.count > Self.visibleSlots && hintEnabled
        self.items = items
        visibleCount = min(items.count, Self.visibleSlots)
        slotWidth = width
        contentWidth = content
        showsHint = hint
        hintInset = hint ? spacing.hintWidth : 0
        startOffset = rightToLeft ? content - barWidth : 0
    }
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

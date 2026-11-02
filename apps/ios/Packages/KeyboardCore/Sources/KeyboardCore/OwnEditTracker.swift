/// Tells this keyboard's own edits apart from changes made by the host or the
/// user (a tap elsewhere, a paste), including when the host still reports the
/// text before the cursor as it was before the last edit.
///
/// The extension calls `beginEdit` before a key's edits, mirrors each edit, and
/// calls `endEdit` after; `textDidChange` then asks `isOwn` about what the host
/// reports.
public struct OwnEditTracker: Sendable {
    /// Recent contexts this keyboard produced or started from, oldest first.
    private var known: [String?] = []
    /// What the text before the cursor should be once the current edit lands;
    /// nil while it is unknown (the cursor moved).
    private var expected: String?
    private var tracking = false

    /// How many own contexts count, so a host lagging by an update or two still matches.
    private static let remembered = 3
    /// Hosts may trim the start of a long context; a suffix this long still matches.
    private static let trimmedMatchLength = 16

    public init() {}

    /// Before an edit. A host still reporting an earlier own context is lagging,
    /// so the edit builds on what the last edit should have produced.
    public mutating func beginEdit(current: String?) {
        if tracking, isOwn(current) {
            remember(expected)
        } else {
            known = [current]
            expected = current
            tracking = true
        }
    }

    public mutating func inserted(_ text: String) {
        expected = expected.map { $0 + text }
    }

    /// Mirrors iOS: one code point per delete, except a whole emoji.
    public mutating func deletedBackward() {
        guard var text = expected, let last = text.last else { return }
        if last.unicodeScalars.contains(where: { $0.properties.isEmojiPresentation || $0 == "\u{FE0F}" }) {
            text.removeLast()
        } else {
            text.unicodeScalars.removeLast()
        }
        expected = text
    }

    /// The cursor moved: nothing after this is known until the next edit starts.
    public mutating func movedCursor() {
        expected = nil
        tracking = false
        known = []
    }

    /// After the edit; `current` is what the host reports now (it may lag).
    public mutating func endEdit(current: String?) {
        guard tracking else { return }
        remember(expected)
        if !isOwn(current) { remember(current) }
    }

    /// The host or the user changed the text: start over from `current`.
    public mutating func reset(current: String?) {
        known = [current]
        expected = current
        tracking = true
    }

    /// Whether `context` is one this keyboard produced or started from.
    public func isOwn(_ context: String?) -> Bool {
        known.contains { Self.matches($0, context) }
    }

    private mutating func remember(_ context: String?) {
        guard context != nil, !known.contains(where: { $0 == context }) else { return }
        known.append(context)
        if known.count > Self.remembered { known.removeFirst(known.count - Self.remembered) }
    }

    private static func matches(_ a: String?, _ b: String?) -> Bool {
        guard let a, let b else { return a == nil && b == nil }
        if a == b { return true }
        let (short, long) = a.count < b.count ? (a, b) : (b, a)
        return short.count >= trimmedMatchLength && long.hasSuffix(short)
    }
}

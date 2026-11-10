import CoreGraphics

/// How tall the keyboard is: the suggestion bar, the keys, and the insets around them.
///
/// Portrait keeps the heights the keyboard has always had: 270 pt in all, 320 with the
/// number row. In landscape (the screen wider than tall) the keyboard is shorter, as SwiftKey
/// and Gboard are, so more of the app shows: each row is
/// `min(portrait row, (screen height × 0.5 − bar) / rows)`, never below 24 pt, and the bar
/// is 34 pt, not 40. The screen height is the height the keyboard's window has as turned.
/// The Android port uses the same rule (with its usable screen height), so both match.
/// "Your words" and the emoji search add their extra height on top.
///
/// The keys' height is set for the nominal row count (5 with the number row, 4 without) and
/// every page (letters, symbols, a pack's own layout) shares it, so switching pages never
/// changes the keyboard's height; a page with more rows splits it finer.
public struct KeyboardHeight: Equatable, Sendable {
    /// The suggestion bar's height.
    public let bar: CGFloat
    /// The keys' height: `rowHeight × rows`.
    public let keys: CGFloat
    /// One row of keys, gaps included.
    public let rowHeight: CGFloat
    public let isLandscape: Bool

    /// Everything: the insets, the bar and the keys.
    public var total: CGFloat { Self.insets + bar + keys }

    /// The space outside the bar and the keys: 4 pt above the bar, 4 between it and the
    /// keys, 2 below the keys.
    public static let insets: CGFloat = 10
    public static let portraitBar: CGFloat = 40
    public static let landscapeBar: CGFloat = 34
    /// The share of the screen's height the bar and the keys may take in landscape.
    public static let landscapeShare: CGFloat = 0.5
    /// A landscape row is never shorter than this, however short the screen.
    public static let minimumRowHeight: CGFloat = 24

    /// The nominal row count: the letters' rows.
    public static func rows(numberRow: Bool) -> Int { numberRow ? 5 : 4 }

    /// A portrait row: 270 pt (320 with the number row) less the insets and the bar, split
    /// between the rows. 55 pt without the number row, 54 with it.
    public static func portraitRowHeight(numberRow: Bool) -> CGFloat {
        let total: CGFloat = numberRow ? 320 : 270
        return (total - insets - portraitBar) / CGFloat(rows(numberRow: numberRow))
    }

    /// The heights for a screen of `screen` points as it is now oriented.
    public static func resolve(numberRow: Bool, screen: CGSize) -> KeyboardHeight {
        let rows = CGFloat(rows(numberRow: numberRow))
        let portraitRow = portraitRowHeight(numberRow: numberRow)
        guard screen.width > screen.height else {
            return KeyboardHeight(bar: portraitBar, keys: portraitRow * rows, rowHeight: portraitRow, isLandscape: false)
        }
        let room = screen.height * landscapeShare - landscapeBar
        let row = max(minimumRowHeight, min(portraitRow, room / rows))
        return KeyboardHeight(bar: landscapeBar, keys: row * rows, rowHeight: row, isLandscape: true)
    }

    /// The screen as it will be once the keyboard is `width` wide: rotation hands over the
    /// new width before the screen's bounds turn. A keyboard wider than the screen's short
    /// side is in landscape (not as wide as the long side: on an iPhone with a notch the
    /// system keeps the globe and the microphone beside the keys).
    public static func screen(forKeyboardWidth width: CGFloat, screen: CGSize) -> CGSize {
        let long = max(screen.width, screen.height), short = min(screen.width, screen.height)
        guard long > short else { return screen }
        return width > short + 1 ? CGSize(width: long, height: short) : CGSize(width: short, height: long)
    }
}

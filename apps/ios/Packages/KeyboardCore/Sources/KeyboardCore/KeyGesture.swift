import CoreGraphics
import Foundation

/// What a touch on a key means once interpreted.
public enum GestureEvent: Equatable, Sendable {
    case tap(KeyAction)
    case showAlternates([String])
    case highlightAlternate(Int)
    case commitAlternate(String)
    case moveCursor(Int)
    case deleteWord
    /// Long-press on the language key: show the in-keyboard settings.
    case openSettings
}

public struct GestureTiming: Sendable {
    public var longPress: TimeInterval = 0.35
    /// The punctuation key (`. , ? ! '`) opens almost at once, as SwiftKey's does. Its own
    /// character is in the popup and stays picked until the finger slides, so a tap held a
    /// little long still types a full stop.
    public var punctuationLongPress: TimeInterval = 0.1
    public var repeatDelay: TimeInterval = 0.45
    public var repeatInterval: TimeInterval = 0.08
    public var cursorStep: CGFloat = 12
    /// How far the spacebar must slide before it moves the cursor instead of typing a space.
    public var cursorActivation: CGFloat = 25
    public var deleteWordSwipe: CGFloat = 40

    public init() {}
}

/// Interprets one finger on one key: tap, long-press alternates, spacebar cursor
/// drag, backspace repeat, swipe-left-to-delete-word and long-press-language for settings. Pure logic: the view feeds
/// it touches and timer ticks (monotonic seconds) and acts on the returned events.
public struct KeyGesture: Sendable {
    public let key: Key

    /// `swipeCancelled`: a backspace swipe slid back, so releasing does nothing.
    /// `settingsOpened`: the language key was held, so releasing doesn't toggle.
    private enum Mode { case pressing, alternates, cursor, repeating, deleteSwipe, swipeCancelled, settingsOpened }

    private let start: CGPoint
    private let startTime: TimeInterval
    private let alternateWidth: CGFloat
    private let alternateHeight: CGFloat
    private let timing: GestureTiming
    private var mode: Mode = .pressing
    private var alternatesOriginX: CGFloat = 0
    private var highlighted = 0
    /// The key's own character is picked and the finger hasn't slid off it yet.
    private var holdingOwn = false
    private var cursorAnchorX: CGFloat = 0
    private var nextRepeat: TimeInterval = 0

    /// `alternateHeight` is a popup row's height (the key's); a popup with several rows
    /// picks the row above once the finger has slid up that far.
    public init(
        key: Key, at point: CGPoint, time: TimeInterval, alternateWidth: CGFloat,
        alternateHeight: CGFloat? = nil, timing: GestureTiming = GestureTiming()
    ) {
        self.key = key
        self.start = point
        self.startTime = time
        self.alternateWidth = max(alternateWidth, 1)
        self.alternateHeight = max(alternateHeight ?? alternateWidth, 1)
        self.timing = timing
    }

    /// Where the alternates popup's first option starts, once the view has placed it.
    /// The popup may be shifted to fit on screen, so this re-picks the option under
    /// the finger and returns a highlight event if that changed.
    public mutating func setAlternatesOrigin(x: CGFloat) -> [GestureEvent] {
        alternatesOriginX = x
        guard mode == .alternates else { return [] }
        // The popup lights the key's own character itself.
        if holdingOwn { return [] }
        let index = alternateIndex(at: start)
        guard index != highlighted else { return [] }
        highlighted = index
        return [.highlightAlternate(index)]
    }

    private var alternatesDelay: TimeInterval {
        if case .character(let text) = key.action, text.allSatisfy(\.isPunctuation) {
            return timing.punctuationLongPress
        }
        return timing.longPress
    }

    public mutating func tick(at time: TimeInterval) -> [GestureEvent] {
        switch mode {
        case .pressing:
            if key.action == .backspace, time - startTime >= timing.repeatDelay {
                mode = .repeating
                nextRepeat = time + timing.repeatInterval
                return [.tap(.backspace)]
            }
            if key.action == .toggleLanguage, time - startTime >= timing.longPress {
                mode = .settingsOpened
                return [.openSettings]
            }
            if !key.alternates.isEmpty, time - startTime >= alternatesDelay {
                mode = .alternates
                alternatesOriginX = start.x - alternateWidth / 2
                if case .character(let own) = key.action, let index = key.alternates.firstIndex(of: own) {
                    highlighted = index
                    holdingOwn = true
                } else {
                    highlighted = 0
                }
                return [.showAlternates(key.alternates)]
            }
            return []
        case .repeating:
            guard time >= nextRepeat else { return [] }
            nextRepeat = time + timing.repeatInterval
            return [.tap(.backspace)]
        case .alternates, .cursor, .deleteSwipe, .swipeCancelled, .settingsOpened:
            return []
        }
    }

    public mutating func moved(to point: CGPoint) -> [GestureEvent] {
        switch mode {
        case .pressing:
            let dx = point.x - start.x
            if key.action == .space, abs(dx) >= timing.cursorActivation {
                mode = .cursor
                cursorAnchorX = start.x
                return moveCursor(to: point.x)
            }
            if key.action == .backspace, dx <= -timing.deleteWordSwipe {
                mode = .deleteSwipe
            }
            return []
        case .cursor:
            return moveCursor(to: point.x)
        case .alternates:
            if holdingOwn {
                guard abs(point.x - start.x) >= alternateWidth / 2 || start.y - point.y >= alternateHeight / 2
                else { return [] }
                holdingOwn = false
            }
            let index = alternateIndex(at: point)
            guard index != highlighted else { return [] }
            highlighted = index
            return [.highlightAlternate(index)]
        case .deleteSwipe, .swipeCancelled:
            mode = point.x - start.x <= -timing.deleteWordSwipe ? .deleteSwipe : .swipeCancelled
            return []
        case .repeating, .settingsOpened:
            return []
        }
    }

    public func ended() -> [GestureEvent] {
        switch mode {
        case .pressing: [.tap(key.action)]
        case .alternates: [.commitAlternate(key.alternates[highlighted])]
        case .deleteSwipe: [.deleteWord]
        case .cursor, .repeating, .swipeCancelled, .settingsOpened: []
        }
    }

    private mutating func moveCursor(to x: CGFloat) -> [GestureEvent] {
        let steps = Int((x - cursorAnchorX) / timing.cursorStep)
        guard steps != 0 else { return [] }
        cursorAnchorX += CGFloat(steps) * timing.cursorStep
        return [.moveCursor(steps)]
    }

    /// The option under `point`: the column from x, the row from how far the finger has
    /// slid up from where it pressed. The popup's bottom row sits about one row above the
    /// press, so the next row up starts one and a half rows up.
    private func alternateIndex(at point: CGPoint) -> Int {
        let (columns, rows) = key.alternatesGrid
        let column = min(max(Int(((point.x - alternatesOriginX) / alternateWidth).rounded(.down)), 0), columns - 1)
        let rowsUp = Int(max(0, (start.y - point.y) / alternateHeight - 0.5).rounded(.down))
        let row = max(rows - 1 - rowsUp, 0)
        return min(row * columns + column, key.alternates.count - 1)
    }
}

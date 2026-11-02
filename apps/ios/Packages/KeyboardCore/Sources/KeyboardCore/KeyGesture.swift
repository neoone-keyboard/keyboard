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
}

public struct GestureTiming: Sendable {
    public var longPress: TimeInterval = 0.35
    public var repeatDelay: TimeInterval = 0.45
    public var repeatInterval: TimeInterval = 0.08
    public var cursorStep: CGFloat = 12
    public var deleteWordSwipe: CGFloat = 40

    public init() {}
}

/// Interprets one finger on one key: tap, long-press alternates, spacebar cursor
/// drag, backspace repeat and swipe-left-to-delete-word. Pure logic: the view feeds
/// it touches and timer ticks (monotonic seconds) and acts on the returned events.
public struct KeyGesture: Sendable {
    public let key: Key

    private enum Mode { case pressing, alternates, cursor, repeating, deleteSwipe }

    private let start: CGPoint
    private let startTime: TimeInterval
    private let alternateWidth: CGFloat
    private let timing: GestureTiming
    private var mode: Mode = .pressing
    private var alternatesOriginX: CGFloat = 0
    private var highlighted = 0
    private var cursorAnchorX: CGFloat = 0
    private var nextRepeat: TimeInterval = 0

    public init(key: Key, at point: CGPoint, time: TimeInterval, alternateWidth: CGFloat, timing: GestureTiming = GestureTiming()) {
        self.key = key
        self.start = point
        self.startTime = time
        self.alternateWidth = max(alternateWidth, 1)
        self.timing = timing
    }

    /// Where the alternates popup's first option starts, once the view has placed it.
    public mutating func setAlternatesOrigin(x: CGFloat) {
        alternatesOriginX = x
    }

    public mutating func tick(at time: TimeInterval) -> [GestureEvent] {
        switch mode {
        case .pressing:
            if key.action == .backspace, time - startTime >= timing.repeatDelay {
                mode = .repeating
                nextRepeat = time + timing.repeatInterval
                return [.tap(.backspace)]
            }
            if !key.alternates.isEmpty, time - startTime >= timing.longPress {
                mode = .alternates
                alternatesOriginX = start.x - alternateWidth / 2
                highlighted = 0
                return [.showAlternates(key.alternates)]
            }
            return []
        case .repeating:
            guard time >= nextRepeat else { return [] }
            nextRepeat = time + timing.repeatInterval
            return [.tap(.backspace)]
        case .alternates, .cursor, .deleteSwipe:
            return []
        }
    }

    public mutating func moved(to point: CGPoint) -> [GestureEvent] {
        switch mode {
        case .pressing:
            let dx = point.x - start.x
            if key.action == .space, abs(dx) >= timing.cursorStep {
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
            let index = alternateIndex(at: point.x)
            guard index != highlighted else { return [] }
            highlighted = index
            return [.highlightAlternate(index)]
        case .repeating, .deleteSwipe:
            return []
        }
    }

    public func ended() -> [GestureEvent] {
        switch mode {
        case .pressing: [.tap(key.action)]
        case .alternates: [.commitAlternate(key.alternates[highlighted])]
        case .deleteSwipe: [.deleteWord]
        case .cursor, .repeating: []
        }
    }

    private mutating func moveCursor(to x: CGFloat) -> [GestureEvent] {
        let steps = Int((x - cursorAnchorX) / timing.cursorStep)
        guard steps != 0 else { return [] }
        cursorAnchorX += CGFloat(steps) * timing.cursorStep
        return [.moveCursor(steps)]
    }

    private func alternateIndex(at x: CGFloat) -> Int {
        let raw = Int(((x - alternatesOriginX) / alternateWidth).rounded(.down))
        return min(max(raw, 0), key.alternates.count - 1)
    }
}

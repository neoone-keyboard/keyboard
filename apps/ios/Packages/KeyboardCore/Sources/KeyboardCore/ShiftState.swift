import Foundation

public enum ShiftState: Equatable, Sendable {
    case off, once, locked

    public static let doubleTapWindow: TimeInterval = 0.3

    /// The state after tapping shift. `interval` is the time since the previous shift tap.
    public func tapped(sinceLastTap interval: TimeInterval?) -> ShiftState {
        switch self {
        case .off:
            return .once
        case .once:
            if let interval, interval <= Self.doubleTapWindow { return .locked }
            return .off
        case .locked:
            return .off
        }
    }

    /// The state after typing a character.
    public var afterCharacter: ShiftState {
        self == .once ? .off : self
    }
}

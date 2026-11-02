import Foundation

/// Where typed text goes. The keyboard extension wraps `textDocumentProxy`; tests record text.
@MainActor
public protocol TextOutput: AnyObject {
    func insertText(_ text: String)
    func deleteBackward()
}

/// Turns key actions into text. Has no dependency on the engine, so typing
/// keeps working when prediction fails (spec §4 rule 3).
@MainActor
public final class InputController {
    public private(set) var shift: ShiftState = .off
    private let output: TextOutput
    private var lastShiftTap: TimeInterval?

    public init(output: TextOutput) {
        self.output = output
    }

    /// - Parameter time: Monotonic seconds, e.g. `ProcessInfo.processInfo.systemUptime`.
    ///   Used only to detect a double tap on shift.
    public func handle(_ action: KeyAction, at time: TimeInterval) {
        switch action {
        case .character(let c):
            output.insertText(shift == .off ? c : c.uppercased())
            shift = shift.afterCharacter
        case .space:
            output.insertText(" ")
        case .returnKey:
            output.insertText("\n")
        case .backspace:
            output.deleteBackward()
        case .shift:
            let interval = lastShiftTap.map { time - $0 }
            lastShiftTap = time
            shift = shift.tapped(sinceLastTap: interval)
        case .nextKeyboard:
            break // The host view controller switches keyboards.
        }
    }

    public func label(for action: KeyAction) -> String {
        switch action {
        case .character(let c): shift == .off ? c : c.uppercased()
        case .space: "space"
        case .backspace: "⌫"
        case .shift: shift == .locked ? "⇪" : "⇧"
        case .returnKey: "return"
        case .nextKeyboard: "🌐"
        }
    }
}

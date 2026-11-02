public struct Key: Equatable, Sendable {
    public let action: KeyAction
    /// Width relative to a letter key (1.0).
    public let width: Double

    public init(_ action: KeyAction, width: Double = 1) {
        self.action = action
        self.width = width
    }
}

public struct KeyboardLayout: Sendable {
    public let rows: [[Key]]

    public static let qwerty = KeyboardLayout(rows: [
        letters("qwertyuiop"),
        letters("asdfghjkl"),
        [Key(.shift, width: 1.5)] + letters("zxcvbnm") + [Key(.backspace, width: 1.5)],
        [Key(.nextKeyboard, width: 1.5), Key(.space, width: 5), Key(.returnKey, width: 2)],
    ])

    private static func letters(_ string: String) -> [Key] {
        string.map { Key(.character(String($0))) }
    }
}

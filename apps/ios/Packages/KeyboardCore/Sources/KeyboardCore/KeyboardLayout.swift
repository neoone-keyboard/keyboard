public struct Key: Equatable, Sendable {
    public let action: KeyAction
    /// Width relative to a letter key (1.0).
    public let width: Double
    /// Long-press options; the first one is printed on the key as a hint.
    public let alternates: [String]

    public init(_ action: KeyAction, width: Double = 1, alternates: [String] = []) {
        self.action = action
        self.width = width
        self.alternates = alternates
    }
}

public struct KeyboardLayout: Equatable, Sendable {
    public let rows: [[Key]]

    public init(rows: [[Key]]) {
        self.rows = rows
    }
}

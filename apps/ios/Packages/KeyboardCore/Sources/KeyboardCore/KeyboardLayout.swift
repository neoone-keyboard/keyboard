public struct Key: Equatable, Sendable {
    public let action: KeyAction
    /// Width relative to a letter key (1.0).
    public let width: Double
    /// Long-press options; the first one is printed on the key as a hint.
    public let alternates: [String]
    /// Options per popup row; the rows are `alternates` in order, top row first, so the
    /// last row is the one nearest the key. One row when nil.
    public let alternatesPerRow: Int?

    public init(_ action: KeyAction, width: Double = 1, alternates: [String] = [], alternatesPerRow: Int? = nil) {
        self.action = action
        self.width = width
        self.alternates = alternates
        self.alternatesPerRow = alternatesPerRow
    }

    /// Columns and rows of the alternates popup.
    public var alternatesGrid: (columns: Int, rows: Int) {
        let columns = max(1, min(alternatesPerRow ?? alternates.count, alternates.count))
        return (columns, (alternates.count + columns - 1) / columns)
    }
}

public struct KeyboardLayout: Equatable, Sendable {
    public let rows: [[Key]]

    public init(rows: [[Key]]) {
        self.rows = rows
    }
}

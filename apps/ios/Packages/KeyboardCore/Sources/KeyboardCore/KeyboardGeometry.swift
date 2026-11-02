import CoreGraphics

public struct KeyFrame: Equatable, Sendable {
    public let key: Key
    /// Where the key cap is drawn.
    public let rect: CGRect
    /// The touch area: the cap plus half the gap around it.
    public let hitRect: CGRect
}

public enum KeyboardGeometry {
    /// Lays keys out row by row. A row narrower than `baseUnits` letter keys is
    /// centred; a wider one is scaled down to fit.
    public static func frames(
        for layout: KeyboardLayout,
        in size: CGSize,
        keyGap: CGFloat = 6,
        rowGap: CGFloat = 10,
        baseUnits: Double = 10
    ) -> [KeyFrame] {
        guard !layout.rows.isEmpty, size.width > 0, size.height > 0 else { return [] }
        let rowHeight = size.height / CGFloat(layout.rows.count)
        let unit = size.width / CGFloat(baseUnits)
        var frames: [KeyFrame] = []
        for (rowIndex, row) in layout.rows.enumerated() {
            let units = row.reduce(0) { $0 + $1.width }
            let scale = units > baseUnits ? CGFloat(baseUnits / units) : 1
            var x = (size.width - CGFloat(units) * unit * scale) / 2
            let y = CGFloat(rowIndex) * rowHeight
            for key in row {
                let width = CGFloat(key.width) * unit * scale
                let cell = CGRect(x: x, y: y, width: width, height: rowHeight)
                frames.append(KeyFrame(key: key, rect: cell.insetBy(dx: keyGap / 2, dy: rowGap / 2), hitRect: cell))
                x += width
            }
        }
        return frames
    }

    /// The key under `point`, or the nearest one when the point falls outside every key.
    public static func index(at point: CGPoint, in frames: [KeyFrame]) -> Int? {
        if let hit = frames.firstIndex(where: { $0.hitRect.contains(point) }) { return hit }
        return frames.indices.min { distance(point, frames[$0].hitRect) < distance(point, frames[$1].hitRect) }
    }

    private static func distance(_ point: CGPoint, _ rect: CGRect) -> CGFloat {
        let dx = max(rect.minX - point.x, 0, point.x - rect.maxX)
        let dy = max(rect.minY - point.y, 0, point.y - rect.maxY)
        return dx * dx + dy * dy
    }
}

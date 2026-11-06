import CoreGraphics

/// The line drawn under a swiping finger: its newest `length` points, thin at the old end and
/// `headWidth` wide at the finger, as a closed outline to fill.
public enum SwipeTrail {
    public static let length = 30
    public static let headWidth: CGFloat = 6
    public static let tailWidth: CGFloat = 1

    /// The outline round the last `length` of `points`: down one side, back along the other.
    /// Empty for fewer than two points.
    public static func outline(_ points: [CGPoint]) -> [CGPoint] {
        let points = Array(points.suffix(length))
        guard points.count >= 2 else { return [] }
        var left: [CGPoint] = [], right: [CGPoint] = []
        for i in points.indices {
            let before = points[max(i - 1, 0)], after = points[min(i + 1, points.count - 1)]
            let dx = after.x - before.x, dy = after.y - before.y
            let norm = max(hypot(dx, dy), .ulpOfOne)
            let half = (tailWidth + (headWidth - tailWidth) * CGFloat(i) / CGFloat(points.count - 1)) / 2
            let nx = -dy / norm * half, ny = dx / norm * half
            left.append(CGPoint(x: points[i].x + nx, y: points[i].y + ny))
            right.append(CGPoint(x: points[i].x - nx, y: points[i].y - ny))
        }
        return left + right.reversed()
    }
}

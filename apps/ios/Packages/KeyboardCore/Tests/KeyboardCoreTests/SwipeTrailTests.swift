import CoreGraphics
import Testing
import KeyboardCore

struct SwipeTrailTests {
    @Test func aSinglePointDrawsNothing() {
        #expect(SwipeTrail.outline([CGPoint(x: 1, y: 1)]).isEmpty)
    }

    @Test func theTrailWidensTowardsTheFinger() {
        let points = (0..<10).map { CGPoint(x: CGFloat($0) * 10, y: 0) }
        let outline = SwipeTrail.outline(points)
        #expect(outline.count == 20)
        // Down the left side, back along the right: the first pair is the tail, the middle pair the head.
        let tail = abs(outline[0].y - outline[19].y), head = abs(outline[9].y - outline[10].y)
        #expect(abs(tail - SwipeTrail.tailWidth) < 0.001)
        #expect(abs(head - SwipeTrail.headWidth) < 0.001)
    }

    @Test func onlyTheNewestPointsAreDrawn() {
        let points = (0..<100).map { CGPoint(x: CGFloat($0), y: 0) }
        let outline = SwipeTrail.outline(points)
        #expect(outline.count == 2 * SwipeTrail.length)
        #expect(outline.allSatisfy { $0.x >= 70 })
    }
}

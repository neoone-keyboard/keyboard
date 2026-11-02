import Testing
import CoreGraphics
import KeyboardCore

private func row(_ widths: [Double]) -> KeyboardLayout {
    KeyboardLayout(rows: [widths.map { Key(.character("x"), width: $0) }])
}

private let size = CGSize(width: 400, height: 50)

@Test func fullRowFillsTheWidth() {
    let frames = KeyboardGeometry.frames(for: row(Array(repeating: 1, count: 10)), in: size)
    #expect(frames.count == 10)
    #expect(frames.first?.hitRect.minX == 0)
    #expect(frames.last?.hitRect.maxX == 400)
}

@Test func narrowRowIsCentred() {
    let frames = KeyboardGeometry.frames(for: row(Array(repeating: 1, count: 9)), in: size)
    #expect(frames.first?.hitRect.minX == 20)
    #expect(frames.last?.hitRect.maxX == 380)
}

@Test func wideRowIsScaledToFit() {
    let frames = KeyboardGeometry.frames(for: row(Array(repeating: 1, count: 10) + [1]), in: size)
    #expect(frames.first?.hitRect.minX == 0)
    #expect(abs((frames.last?.hitRect.maxX ?? 0) - 400) < 0.001)
}

@Test func capsLeaveGapsInsideTheirTouchArea() {
    let frame = KeyboardGeometry.frames(for: row([1]), in: size)[0]
    #expect(frame.rect.width == frame.hitRect.width - 6)
    #expect(frame.rect.height == frame.hitRect.height - 10)
}

@Test func rowsSplitTheHeight() {
    let layout = KeyboardLayout(rows: [[Key(.space)], [Key(.space)]])
    let frames = KeyboardGeometry.frames(for: layout, in: CGSize(width: 100, height: 100))
    #expect(frames[1].hitRect.minY == 50)
}

@Test func pointInsideAKeyHitsIt() {
    let frames = KeyboardGeometry.frames(for: row(Array(repeating: 1, count: 10)), in: size)
    #expect(KeyboardGeometry.index(at: CGPoint(x: 65, y: 25), in: frames) == 1)
}

@Test func pointOutsideEveryKeyHitsTheNearest() {
    let frames = KeyboardGeometry.frames(for: row(Array(repeating: 1, count: 9)), in: size)
    #expect(KeyboardGeometry.index(at: CGPoint(x: 5, y: 25), in: frames) == 0)
    #expect(KeyboardGeometry.index(at: CGPoint(x: 399, y: 60), in: frames) == 8)
}

@Test func emptyLayoutHasNoFrames() {
    #expect(KeyboardGeometry.frames(for: KeyboardLayout(rows: []), in: size).isEmpty)
    #expect(KeyboardGeometry.index(at: .zero, in: []) == nil)
}

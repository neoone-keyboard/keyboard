import Testing
import CoreGraphics
import KeyboardCore

private func gesture(_ key: Key, x: CGFloat = 100) -> KeyGesture {
    KeyGesture(key: key, at: CGPoint(x: x, y: 10), time: 0, alternateWidth: 30)
}

@Test func tapTypesTheKey() {
    var g = gesture(Key(.character("a")))
    #expect(g.tick(at: 0.1) == [])
    #expect(g.ended() == [.tap(.character("a"))])
}

@Test func keyWithoutAlternatesNeverLongPresses() {
    var g = gesture(Key(.character("a")))
    #expect(g.tick(at: 1.0) == [])
    #expect(g.ended() == [.tap(.character("a"))])
}

@Test func longPressShowsAlternatesAndCommitsTheHighlightedOne() {
    var g = gesture(Key(.character("e"), alternates: ["3", "é", "è"]))
    #expect(g.tick(at: 0.4) == [.showAlternates(["3", "é", "è"])])
    g.setAlternatesOrigin(x: 0)
    #expect(g.moved(to: CGPoint(x: 45, y: 10)) == [.highlightAlternate(1)])
    #expect(g.moved(to: CGPoint(x: 50, y: 10)) == [])
    #expect(g.ended() == [.commitAlternate("é")])
}

@Test func longPressWithoutMovingCommitsTheFirstAlternate() {
    var g = gesture(Key(.character("e"), alternates: ["3", "é"]))
    _ = g.tick(at: 0.4)
    #expect(g.ended() == [.commitAlternate("3")])
}

@Test func alternateIndexIsClamped() {
    var g = gesture(Key(.character("e"), alternates: ["3", "é"]))
    _ = g.tick(at: 0.4)
    g.setAlternatesOrigin(x: 0)
    #expect(g.moved(to: CGPoint(x: 500, y: 10)) == [.highlightAlternate(1)])
    #expect(g.moved(to: CGPoint(x: -500, y: 10)) == [.highlightAlternate(0)])
}

@Test func spaceDragMovesTheCursorInsteadOfTyping() {
    var g = gesture(Key(.space))
    #expect(g.moved(to: CGPoint(x: 105, y: 10)) == [])
    #expect(g.moved(to: CGPoint(x: 120, y: 10)) == [])
    #expect(g.moved(to: CGPoint(x: 125, y: 10)) == [.moveCursor(2)])
    #expect(g.moved(to: CGPoint(x: 111, y: 10)) == [.moveCursor(-1)])
    #expect(g.ended() == [])
}

@Test func smallSpaceWiggleStillTypesSpace() {
    var g = gesture(Key(.space))
    #expect(g.moved(to: CGPoint(x: 104, y: 10)) == [])
    #expect(g.ended() == [.tap(.space)])
}

@Test func spaceSlideShortOfActivationStillTypesSpace() {
    var g = gesture(Key(.space))
    #expect(g.moved(to: CGPoint(x: 112, y: 10)) == [])
    #expect(g.ended() == [.tap(.space)])
}

@Test func spaceDragLeftActivatesToo() {
    var g = gesture(Key(.space))
    #expect(g.moved(to: CGPoint(x: 80, y: 10)) == [])
    #expect(g.moved(to: CGPoint(x: 75, y: 10)) == [.moveCursor(-2)])
    #expect(g.ended() == [])
}

@Test func backspaceTapDeletesOnce() {
    var g = gesture(Key(.backspace))
    #expect(g.ended() == [.tap(.backspace)])
}

@Test func backspaceHoldRepeats() {
    var g = gesture(Key(.backspace))
    #expect(g.tick(at: 0.3) == [])
    #expect(g.tick(at: 0.45) == [.tap(.backspace)])
    #expect(g.tick(at: 0.5) == [])
    #expect(g.tick(at: 0.54) == [.tap(.backspace)])
    #expect(g.ended() == [])
}

@Test func backspaceSwipeLeftDeletesAWord() {
    var g = gesture(Key(.backspace))
    #expect(g.moved(to: CGPoint(x: 50, y: 10)) == [])
    #expect(g.ended() == [.deleteWord])
}

@Test func backspaceSwipeCancelledByMovingBack() {
    var g = gesture(Key(.backspace))
    #expect(g.moved(to: CGPoint(x: 50, y: 10)) == [])
    #expect(g.moved(to: CGPoint(x: 90, y: 10)) == [])
    #expect(g.ended() == [])
}

@Test func cancelledBackspaceSwipeCanBeRearmed() {
    var g = gesture(Key(.backspace))
    _ = g.moved(to: CGPoint(x: 50, y: 10))
    _ = g.moved(to: CGPoint(x: 90, y: 10))
    _ = g.moved(to: CGPoint(x: 55, y: 10))
    #expect(g.ended() == [.deleteWord])
}

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
    #expect(g.setAlternatesOrigin(x: 85) == [])
    #expect(g.moved(to: CGPoint(x: 125, y: 10)) == [.highlightAlternate(1)])
    #expect(g.moved(to: CGPoint(x: 130, y: 10)) == [])
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
    #expect(g.setAlternatesOrigin(x: 85) == [])
    #expect(g.moved(to: CGPoint(x: 500, y: 10)) == [.highlightAlternate(1)])
    #expect(g.moved(to: CGPoint(x: -500, y: 10)) == [.highlightAlternate(0)])
}

@Test func popupClampedAtTheEdgeHighlightsTheOptionUnderTheFinger() {
    var g = gesture(Key(.character("e"), alternates: ["3", "é", "è"]))
    _ = g.tick(at: 0.4)
    #expect(g.setAlternatesOrigin(x: 50) == [.highlightAlternate(1)])
    #expect(g.moved(to: CGPoint(x: 101, y: 10)) == [])
    #expect(g.ended() == [.commitAlternate("é")])
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

@Test func languageKeyTapTogglesLanguage() {
    var g = gesture(Key(.toggleLanguage))
    #expect(g.tick(at: 0.1) == [])
    #expect(g.ended() == [.tap(.toggleLanguage)])
}

@Test func languageKeyLongPressOpensSettingsOnceAndDoesNotToggle() {
    var g = gesture(Key(.toggleLanguage))
    #expect(g.tick(at: 0.34) == [])
    #expect(g.tick(at: 0.35) == [.openSettings])
    #expect(g.tick(at: 1.0) == [])
    #expect(g.moved(to: CGPoint(x: 140, y: 10)) == [])
    #expect(g.ended() == [])
}

private let period = Key(.character("."), alternates: [
    "&", "%", "+", "=", "\"", "-", ":", "'", "@",
    ";", "/", "(", ")", "#", "!", ",", ".", "?",
], alternatesPerRow: 9)

private func periodGesture() -> KeyGesture {
    KeyGesture(key: period, at: CGPoint(x: 100, y: 100), time: 0, alternateWidth: 30, alternateHeight: 40)
}

@Test func punctuationKeyShowsItsAlternatesSooner() {
    var g = periodGesture()
    #expect(g.tick(at: 0.05) == [])
    #expect(g.tick(at: 0.11) == [.showAlternates(period.alternates)])
    // A letter held as long is still a tap.
    var letter = gesture(Key(.character("e"), alternates: ["3", "é"]))
    #expect(letter.tick(at: 0.11) == [])
}

@Test func heldPunctuationKeyStillTypesItselfUntilTheFingerSlides() {
    var g = periodGesture()
    _ = g.tick(at: 0.11)
    // The popup is placed with "." (column 7) above the finger; a small wobble keeps it.
    #expect(g.setAlternatesOrigin(x: 100 - 7 * 30 - 15) == [])
    #expect(g.moved(to: CGPoint(x: 110, y: 90)) == [])
    #expect(g.ended() == [.commitAlternate(".")])
}

@Test func slidingRightOnAHeldPunctuationKeyPicksTheQuestionMark() {
    var g = periodGesture()
    _ = g.tick(at: 0.11)
    _ = g.setAlternatesOrigin(x: 100 - 7 * 30 - 15)
    #expect(g.moved(to: CGPoint(x: 130, y: 100)) == [.highlightAlternate(17)])
    #expect(g.ended() == [.commitAlternate("?")])
}

@Test func slidingUpOnAHeldPunctuationKeyPicksFromTheTopRow() {
    var g = periodGesture()
    _ = g.tick(at: 0.11)
    _ = g.setAlternatesOrigin(x: 100 - 7 * 30 - 15)
    // Over the bottom row (one row up) it is still ".", then the row above.
    #expect(g.moved(to: CGPoint(x: 100, y: 55)) == [])
    #expect(g.moved(to: CGPoint(x: 100, y: 35)) == [.highlightAlternate(7)])
    #expect(g.ended() == [.commitAlternate("'")])
}

// MARK: Swipe

@Test func slidingFromALetterPastAKeyWidthSwipes() {
    var g = gesture(Key(.character("h"), alternates: ["ĥ"]))
    #expect(g.moved(to: CGPoint(x: 110, y: 10)) == [])
    #expect(!g.isSwiping)
    #expect(g.moved(to: CGPoint(x: 135, y: 12)) == [])
    #expect(g.isSwiping)
    #expect(g.moved(to: CGPoint(x: 160, y: 30)) == [])
    #expect(g.tick(at: 1.0) == [], "a swipe never long-presses")
    #expect(g.ended() == [.swipe([CGPoint(x: 100, y: 10), CGPoint(x: 110, y: 10), CGPoint(x: 135, y: 12), CGPoint(x: 160, y: 30)])])
}

@Test func aShortWobbleStillTaps() {
    var g = gesture(Key(.character("a")))
    _ = g.moved(to: CGPoint(x: 112, y: 14))
    #expect(!g.isSwiping)
    #expect(g.ended() == [.tap(.character("a"))])
}

@Test func afterTheLongPressSlidingPicksAnAlternate() {
    var g = gesture(Key(.character("e"), alternates: ["3", "é", "è"]))
    _ = g.tick(at: 0.4)
    _ = g.setAlternatesOrigin(x: 85)
    #expect(g.moved(to: CGPoint(x: 135, y: 10)) == [.highlightAlternate(1)])
    #expect(!g.isSwiping)
}

@Test func onlyLatinLettersSwipe() {
    for action in [KeyAction.character("1"), .character("."), .character("ക"), .space, .backspace, .shift] {
        var g = gesture(Key(action))
        _ = g.moved(to: CGPoint(x: 150, y: 10))
        #expect(!g.isSwiping, "\(action)")
    }
}

@Test func swipePointsAreSpacedAndCapped() {
    var g = gesture(Key(.character("q")))
    _ = g.moved(to: CGPoint(x: 140, y: 10))
    _ = g.moved(to: CGPoint(x: 141, y: 10))
    for i in 0..<400 { _ = g.moved(to: CGPoint(x: 150 + CGFloat(i) * 5, y: 10)) }
    guard case .swipe(let points) = g.ended().first else { Issue.record("no swipe"); return }
    #expect(points.count == KeyGesture.maxSwipePoints)
    #expect(!points.contains(CGPoint(x: 141, y: 10)), "closer than 4 pt to the last point")
    #expect(points.last == CGPoint(x: 150 + 399 * 5, y: 10), "the end is kept")
}

import Testing
import CoreGraphics
import KeyboardCore

/// iPhone 15 / 16 / 17 (6.1"): 393 × 852 pt.
private let portrait = CGSize(width: 393, height: 852)
private let landscape = CGSize(width: 852, height: 393)

@Test func portraitKeepsItsHeights() {
    let plain = KeyboardHeight.resolve(numberRow: false, screen: portrait)
    #expect(plain.total == 270)
    #expect(plain.bar == 40)
    #expect(plain.rowHeight == 55)
    #expect(!plain.isLandscape)
    let numbers = KeyboardHeight.resolve(numberRow: true, screen: portrait)
    #expect(numbers.total == 320)
    #expect(numbers.rowHeight == 54)
}

@Test func landscapeBarAndKeysTakeAtMostHalfTheScreen() {
    for numberRow in [false, true] {
        let height = KeyboardHeight.resolve(numberRow: numberRow, screen: landscape)
        #expect(height.isLandscape)
        #expect(height.bar == 34)
        #expect(height.bar + height.keys <= 393 * 0.5)
        #expect(height.keys == height.rowHeight * CGFloat(KeyboardHeight.rows(numberRow: numberRow)))
    }
}

@Test func landscapeRowFollowsTheRule() {
    // (393 × 0.5 − 34) / 4 = 40.625; with the 10 pt of insets, 206.5 in all.
    #expect(KeyboardHeight.resolve(numberRow: false, screen: landscape).rowHeight == 40.625)
    #expect(KeyboardHeight.resolve(numberRow: false, screen: landscape).total == 206.5)
    // (393 × 0.5 − 34) / 5 = 32.5.
    #expect(KeyboardHeight.resolve(numberRow: true, screen: landscape).rowHeight == 32.5)
    #expect(KeyboardHeight.resolve(numberRow: true, screen: landscape).total == 206.5)
}

@Test func landscapeRowIsNeverTallerThanPortrait() {
    // A screen tall enough in landscape (an iPad) keeps the portrait rows, with the shorter bar.
    let height = KeyboardHeight.resolve(numberRow: false, screen: CGSize(width: 1366, height: 1024))
    #expect(height.rowHeight == 55)
    #expect(height.keys == 220)
}

@Test func landscapeRowIsNeverBelow24() {
    let height = KeyboardHeight.resolve(numberRow: true, screen: CGSize(width: 300, height: 200))
    #expect(height.rowHeight == 24)
    #expect(height.keys == 120)
}

@Test func rotationScreenFollowsTheKeyboardWidth() {
    // Turning to landscape: the keyboard is as wide as the long side before the screen turns.
    #expect(KeyboardHeight.screen(forKeyboardWidth: 852, screen: portrait) == landscape)
    #expect(KeyboardHeight.screen(forKeyboardWidth: 393, screen: landscape) == portrait)
    #expect(KeyboardHeight.screen(forKeyboardWidth: 852, screen: landscape) == landscape)
    // With the globe and the microphone beside the keys, the keyboard is narrower than the screen.
    #expect(KeyboardHeight.screen(forKeyboardWidth: 760, screen: portrait) == landscape)
}

@Test func shortRowsGetSmallerGapsButKeepTheTouchArea() {
    #expect(KeyboardGeometry.rowGap(forRowHeight: 55) == 10)
    #expect(KeyboardGeometry.rowGap(forRowHeight: 30) == 6)
    let layout = KeyboardLayout(rows: [[Key(.space)], [Key(.space)]])
    let frames = KeyboardGeometry.frames(for: layout, in: CGSize(width: 100, height: 60))
    #expect(frames[0].rect.height == 24)
    #expect(frames[0].hitRect.height == 30)
    #expect(frames[1].hitRect.minY == frames[0].hitRect.maxY)
}

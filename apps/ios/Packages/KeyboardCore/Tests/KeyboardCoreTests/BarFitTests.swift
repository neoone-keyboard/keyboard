import Testing
import Foundation
import KeyboardCore

// The suggestion bar's three equal slots, the default in the middle (`SuggestionBar.laidOut`),
// and any more past its trailing edge, each a slot as wide (`BarFit`), with a hint at the edge.

private func words(_ texts: [String], first: SuggestionKind = .verbatim, rest: SuggestionKind = .completion) -> [Suggestion] {
    texts.enumerated().map { Suggestion($1, kind: $0 == 0 ? first : rest) }
}

// MARK: Order: left, the default in the middle, right, then the rest

@Test func theDefaultIsTheMiddleSlotAndTheRestFollowTheThree() {
    let bar = SuggestionBar(SuggestionResult(typed: "he", candidates: words(["he", "help", "hello", "her", "here"])), limit: 10)
    #expect(bar.laidOut.map { $0?.title } == ["help", "he", "hello", "her", "here"])
    #expect(bar.laidOut[1] == bar.middle, "the middle is what space commits")
}

@Test func theQuotedTypedWordIsOnTheLeft() {
    let bar = SuggestionBar(SuggestionResult(typed: "teh", candidates: [
        Suggestion("the", kind: .correction), Suggestion("ten", kind: .correction), Suggestion("teh", kind: .verbatim),
        Suggestion("tea", kind: .correction), Suggestion("tech", kind: .correction),
    ]), limit: 10)
    #expect(bar.laidOut.map { $0?.title } == ["\u{201C}teh\u{201D}", "the", "ten", "tea", "tech"])
}

@Test func emptySlotsKeepTheirPlaceSoTheDefaultStaysInTheMiddle() {
    let one = SuggestionBar(SuggestionResult(typed: "hi", candidates: words(["hi"])), limit: 10)
    #expect(one.laidOut.map { $0?.title } == [nil, "hi", nil])
    #expect(SuggestionBar(.empty, limit: 10).laidOut == [nil, nil, nil])
}

@Test func laidOutHoldsAtMostTheLimit() {
    let texts = (0..<25).map { "w\($0)" }
    #expect(SuggestionBar(SuggestionResult(typed: "w", candidates: words(texts)), limit: 10).laidOut.count == 10)
    #expect(SuggestionBar(SuggestionResult(typed: "w", candidates: words(texts)), limit: 3).laidOut.count == 3)
}

@Test func anEmojiGoesInTheRightSlotNeverTheMiddle() {
    let result = SuggestionResult(typed: "he", candidates: words(["he", "help", "hello", "her"]))
    let ten = SuggestionBar(result, limit: 10).withEmoji("👋")
    #expect(ten.laidOut.map { $0?.title } == ["help", "he", "👋", "hello", "her"])
    let three = SuggestionBar(result).withEmoji("👋")
    #expect(three.laidOut.map { $0?.title } == ["help", "he", "👋"])
}

// MARK: Three equal slots

private let spacing = BarFit.Spacing()

@Test func threeSlotsFillTheBarInEqualThirds() {
    let fit = BarFit(count: 3, barWidth: 387)
    #expect(fit.items.count == 3)
    #expect(fit.visibleCount == 3)
    #expect(!fit.overflows)
    #expect(fit.contentWidth == 387)
    let width = (387 - 2 * spacing.gap) / 3
    #expect(fit.slotWidth == width)
    for (index, item) in fit.items.enumerated() {
        #expect(item.width == width)
        #expect(abs(item.x - Double(index) * (width + spacing.gap)) < 0.001)
    }
    #expect(fit.items[0].x == 0, "from the leading edge")
    #expect(abs(fit.items[2].x + fit.items[2].width - 387) < 0.001, "to the trailing edge")
    #expect(!fit.showsHint)
    #expect(fit.hintInset == 0)
}

@Test func moreSlotsArePastTheEdgeEachAsWideAsTheThree() {
    let fit = BarFit(count: 10, barWidth: 387)
    #expect(fit.visibleCount == 3)
    #expect(fit.overflows)
    #expect(fit.items.allSatisfy { $0.width == fit.slotWidth })
    #expect(fit.items[3].x >= 387, "the fourth starts past the edge")
    for (a, b) in zip(fit.items, fit.items.dropFirst()) {
        #expect(abs(b.x - (a.x + a.width + spacing.gap)) < 0.001)
    }
    let last = fit.items[9]
    #expect(abs(fit.contentWidth - (last.x + last.width)) < 0.001, "the bar slides as far as the last")
    #expect(fit.startOffset == 0)
}

@Test func theSlotsAreAsWideAtAnyCount() {
    let three = BarFit(count: 3, barWidth: 390)
    for count in [1, 2, 4, 10, 20] {
        #expect(BarFit(count: count, barWidth: 390).slotWidth == three.slotWidth)
    }
    // Fewer than three keep their places; the bar is still its own width.
    let one = BarFit(count: 1, barWidth: 390)
    #expect(one.contentWidth == 390)
    #expect(BarFit(count: 0, barWidth: 390).items.isEmpty)
}

@Test func theHintShowsWithMoreThanThreeWhenOn() {
    #expect(BarFit(count: 4, barWidth: 387, hintEnabled: true).showsHint)
    #expect(BarFit(count: 4, barWidth: 387, hintEnabled: true).hintInset == spacing.hintWidth)
    #expect(!BarFit(count: 4, barWidth: 387, hintEnabled: false).showsHint)
    #expect(BarFit(count: 4, barWidth: 387, hintEnabled: false).hintInset == 0)
    #expect(BarFit(count: 4, barWidth: 387, hintEnabled: false).overflows, "overflow still scrolls")
    #expect(!BarFit(count: 3, barWidth: 387, hintEnabled: true).showsHint)
    #expect(!BarFit(count: 0, barWidth: 387, hintEnabled: true).showsHint)
}

@Test func theHintLeavesTheSlotsTheirWidth() {
    let on = BarFit(count: 10, barWidth: 387, hintEnabled: true)
    let off = BarFit(count: 10, barWidth: 387, hintEnabled: false)
    #expect(on.items == off.items)
}

// MARK: Right to left

@Test func rightToLeftMirrorsTheSlotsSoTheDefaultStaysInTheMiddle() {
    let ltr = BarFit(count: 7, barWidth: 300)
    let rtl = BarFit(count: 7, barWidth: 300, rightToLeft: true)
    #expect(rtl.visibleCount == ltr.visibleCount)
    #expect(rtl.contentWidth == ltr.contentWidth)
    for (l, r) in zip(ltr.items, rtl.items) {
        #expect(r.width == l.width)
        #expect(abs(r.x - (rtl.contentWidth - l.x - l.width)) < 0.001)
    }
    // Shown from the right end: the left slot touches the bar's right edge, the middle is in
    // the middle, the rest further left.
    #expect(ltr.startOffset == 0)
    #expect(rtl.startOffset == rtl.contentWidth - 300)
    let first = rtl.items[0]
    #expect(abs(first.x + first.width - rtl.startOffset - 300) < 0.001)
    let middle = rtl.items[1]
    #expect(abs(middle.x - rtl.startOffset - ltr.items[1].x) < 0.001)
    #expect(rtl.items[3].x < rtl.startOffset, "the fourth is past the left edge")
}

@Test func rightToLeftWithThreeFillsTheBarFromTheRight() {
    let fit = BarFit(count: 3, barWidth: 387, rightToLeft: true)
    #expect(fit.startOffset == 0)
    #expect(abs(fit.items[0].x + fit.items[0].width - 387) < 0.001)
    #expect(fit.items[2].x == 0)
}

// MARK: The setting

private func freshDefaults() -> UserDefaults {
    let name = "test-\(UUID().uuidString)"
    let defaults = UserDefaults(suiteName: name)!
    defaults.removePersistentDomain(forName: name)
    return defaults
}

@Test func theMoreHintIsOnUntilTurnedOff() throws {
    #expect(SharedSettings.suggestionMoreHintKey == "suggestionMoreHint", "the same key as Android")
    let defaults = freshDefaults()
    #expect(SharedSettings.suggestionMoreHint(defaults: defaults))
    #expect(SharedSettings.suggestionMoreHint(defaults: nil), "no App Group: on")
    defaults.set(false, forKey: SharedSettings.suggestionMoreHintKey)
    #expect(!SharedSettings.suggestionMoreHint(defaults: defaults))
    defaults.set(true, forKey: SharedSettings.suggestionMoreHintKey)
    #expect(SharedSettings.suggestionMoreHint(defaults: defaults))
}

@Test func theMoreHintIsReadFromThePreferencesFileWhenTheDefaultsHaveNothing() throws {
    let folder = FileManager.default.temporaryDirectory.appending(path: "hint-\(UUID().uuidString)")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
    defer { try? FileManager.default.removeItem(at: folder) }
    let file = folder.appending(path: "group.plist")
    let data = try PropertyListSerialization.data(
        fromPropertyList: [SharedSettings.suggestionMoreHintKey: false], format: .binary, options: 0)
    try data.write(to: file)
    #expect(!SharedSettings.suggestionMoreHint(defaults: freshDefaults(), preferencesFile: file))
    #expect(!SharedSettings.suggestionMoreHint(defaults: nil, preferencesFile: file))
    // The defaults, when they have it, win.
    let defaults = freshDefaults()
    defaults.set(true, forKey: SharedSettings.suggestionMoreHintKey)
    #expect(SharedSettings.suggestionMoreHint(defaults: defaults, preferencesFile: file))
    // A missing or unreadable file is on.
    #expect(SharedSettings.suggestionMoreHint(defaults: nil, preferencesFile: folder.appending(path: "none.plist")))
}

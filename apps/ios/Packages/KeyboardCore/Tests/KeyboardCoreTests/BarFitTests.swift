import Testing
import Foundation
import KeyboardCore

// The suggestion bar fits as many whole words as its width allows (`BarFit`), the default
// first (`SuggestionBar.ordered`), and hints at the rest past its trailing edge.

private func words(_ texts: [String], first: SuggestionKind = .verbatim, rest: SuggestionKind = .completion) -> [Suggestion] {
    texts.enumerated().map { Suggestion($1, kind: $0 == 0 ? first : rest) }
}

// MARK: Order: the default first, then the engine's order

@Test func theDefaultLeadsAndTheRestFollowInTheEnginesOrder() {
    let bar = SuggestionBar(SuggestionResult(typed: "he", candidates: words(["he", "help", "hello", "her", "here"])), limit: 10)
    #expect(bar.ordered.map(\.title) == ["he", "help", "hello", "her", "here"])
    #expect(bar.ordered.first == bar.middle, "the first is what space commits")
}

@Test func theQuotedTypedWordTakesItsPlaceInTheEnginesOrder() {
    let bar = SuggestionBar(SuggestionResult(typed: "teh", candidates: [Suggestion("the", kind: .correction)]
        + words(["teh", "ten", "tea"], first: .verbatim, rest: .correction)), limit: 10)
    #expect(bar.ordered.map(\.title) == ["the", "\u{201C}teh\u{201D}", "ten", "tea"])
    #expect(bar.ordered[1].quoted)

    // Further down the engine's list, it stays there.
    let later = SuggestionBar(SuggestionResult(typed: "teh", candidates: [
        Suggestion("the", kind: .correction), Suggestion("ten", kind: .correction),
        Suggestion("tea", kind: .correction), Suggestion("teh", kind: .verbatim),
    ]), limit: 10)
    #expect(later.ordered.map(\.title) == ["the", "ten", "tea", "\u{201C}teh\u{201D}"])
}

@Test func theTypedWordAsTheDefaultIsNotQuoted() {
    let bar = SuggestionBar(SuggestionResult(typed: "hel", candidates: words(["hel", "help"])))
    #expect(bar.ordered.map(\.title) == ["hel", "help"])
}

@Test func orderedHoldsAtMostTheLimit() {
    let texts = (0..<25).map { "w\($0)" }
    #expect(SuggestionBar(SuggestionResult(typed: "w", candidates: words(texts)), limit: 10).ordered.count == 10)
    #expect(SuggestionBar(SuggestionResult(typed: "w", candidates: words(texts)), limit: 3).ordered.count == 3)
    #expect(SuggestionBar(.empty).ordered.isEmpty)
}

@Test func anEmojiGoesThirdNeverFirst() {
    let result = SuggestionResult(typed: "he", candidates: words(["he", "help", "hello", "her"]))
    let ten = SuggestionBar(result, limit: 10).withEmoji("👋")
    #expect(ten.ordered.map(\.title) == ["he", "help", "👋", "hello", "her"])
    let three = SuggestionBar(result).withEmoji("👋")
    #expect(three.ordered.map(\.title) == ["he", "help", "👋"])
    let one = SuggestionBar(SuggestionResult(typed: "hi", candidates: words(["hi"])), limit: 10).withEmoji("👋")
    #expect(one.ordered.map(\.title) == ["hi", "👋"])
    #expect(SuggestionBar(.empty, limit: 10).withEmoji("👋").ordered.map(\.title) == ["👋"])
}

// MARK: How many fit

/// The default spacing: 12 points each side of a word, 3 between, at least 44 wide.
private let spacing = BarFit.Spacing()

@Test func shortWordsFitManyAcrossTheBar() {
    // Five words 30 wide: 54 each with padding, 5 * 54 + 4 * 3 = 282.
    let fit = BarFit(textWidths: Array(repeating: 30, count: 5), barWidth: 387)
    #expect(fit.visibleCount == 5)
    #expect(!fit.overflows)
    #expect(!fit.showsHint)
    #expect(fit.contentWidth == 387, "nothing to slide to")
    // The words share the spare width, filling the bar edge to edge.
    #expect(fit.items[0].x == 0)
    #expect(abs(fit.items[4].x + fit.items[4].width - 387) < 0.001)
    for (a, b) in zip(fit.items, fit.items.dropFirst()) {
        #expect(abs(b.x - (a.x + a.width) - spacing.gap) < 0.001)
    }
}

@Test func sixShortWordsAndSeven() {
    // 44 wide each (the minimum): 6 * 44 + 5 * 3 = 279 fits 300; a seventh needs 326.
    let fit = BarFit(textWidths: Array(repeating: 10, count: 7), barWidth: 300, hintEnabled: false)
    #expect(fit.visibleCount == 6)
    #expect(fit.overflows)
}

@Test func longWordsFitFewAndNoneIsCut() {
    // Long words of 120 points: 144 each with padding. Two fit 320, a third would need 438.
    let fit = BarFit(textWidths: [120, 120, 120, 120], barWidth: 320, hintEnabled: false)
    #expect(fit.visibleCount == 2)
    #expect(fit.overflows)
    #expect(fit.firstScale == 1)
    for item in fit.items {
        #expect(item.width >= 144, "every word gets its whole width")
    }
    // The rest start past the trailing edge, at their own width.
    #expect(fit.items[2].x >= 320)
    #expect(fit.items[2].width == 144)
    #expect(fit.contentWidth == fit.items[3].x + fit.items[3].width)
}

@Test func theHintTakesRoomOnlyWhenThereIsMore() {
    // Three fit 387 exactly without a hint...
    let widths: [Double] = [100, 100, 100, 50]
    let off = BarFit(textWidths: widths, barWidth: 387, hintEnabled: false)
    #expect(off.visibleCount == 3)
    // ...but with more to come and the hint on, its room at the edge leaves two.
    let on = BarFit(textWidths: widths, barWidth: 387, hintEnabled: true)
    #expect(on.visibleCount == 2)
    #expect(on.showsHint)
    let lastShown = on.items[on.visibleCount - 1]
    #expect(lastShown.x + lastShown.width <= 387 - spacing.hintWidth + 0.001, "clear of the hint")
    // Words that all fit don't make room for a hint that isn't there.
    let all = BarFit(textWidths: [100, 100, 100], barWidth: 387, hintEnabled: true)
    #expect(all.visibleCount == 3)
    #expect(!all.showsHint)
}

@Test func theHintShowsOnlyOnOverflowAndOnlyWhenOn() {
    let many = Array(repeating: 60.0, count: 10)
    #expect(BarFit(textWidths: many, barWidth: 387, hintEnabled: true).showsHint)
    #expect(!BarFit(textWidths: many, barWidth: 387, hintEnabled: false).showsHint)
    #expect(BarFit(textWidths: many, barWidth: 387, hintEnabled: false).overflows, "overflow still scrolls")
    #expect(!BarFit(textWidths: [60, 60], barWidth: 387, hintEnabled: true).showsHint)
    #expect(!BarFit(textWidths: [], barWidth: 387, hintEnabled: true).showsHint)
}

@Test func aFirstWordWiderThanTheBarShrinksRatherThanIsCut() {
    let fit = BarFit(textWidths: [400, 50], barWidth: 320, hintEnabled: false)
    #expect(fit.visibleCount == 1)
    #expect(fit.items[0].width == 320)
    // Its text gets the bar less its padding: 296 of 400.
    #expect(abs(fit.firstScale - 296.0 / 400) < 0.001)
    #expect(fit.firstScale < 1)
    // A word that fits is never shrunk.
    #expect(BarFit(textWidths: [200], barWidth: 320).firstScale == 1)
}

@Test func aFirstWordJustOverTheBarWithTheHintShrinksToTheRoomLeft() {
    let fit = BarFit(textWidths: [300, 50], barWidth: 320, hintEnabled: true)
    #expect(fit.visibleCount == 1)
    #expect(fit.showsHint)
    #expect(fit.items[0].width == 320 - spacing.hintWidth)
    #expect(fit.firstScale < 1)
}

@Test func anEmptyBarHasNothing() {
    let fit = BarFit(textWidths: [], barWidth: 387)
    #expect(fit.visibleCount == 0)
    #expect(fit.items.isEmpty)
    #expect(fit.contentWidth == 387)
    #expect(fit.startOffset == 0)
}

@Test func moreRoomFitsMore() {
    let widths: [Double] = [70, 90, 60, 110, 80, 75, 95]
    var last = 0
    for width in stride(from: 200.0, through: 900, by: 50) {
        let fit = BarFit(textWidths: widths, barWidth: width)
        #expect(fit.visibleCount >= last)
        last = fit.visibleCount
    }
    #expect(last == widths.count)
}

// MARK: Right to left

@Test func rightToLeftMirrorsTheItemsSoTheDefaultIsOnTheRight() {
    let widths: [Double] = [60, 60, 60, 60, 60, 60, 60]
    let ltr = BarFit(textWidths: widths, barWidth: 300)
    let rtl = BarFit(textWidths: widths, barWidth: 300, rightToLeft: true)
    #expect(rtl.visibleCount == ltr.visibleCount)
    #expect(rtl.contentWidth == ltr.contentWidth)
    for (l, r) in zip(ltr.items, rtl.items) {
        #expect(r.width == l.width)
        #expect(abs(r.x - (rtl.contentWidth - l.x - l.width)) < 0.001)
    }
    // Shown from the right end: the default touches the bar's right edge, the rest further left.
    #expect(ltr.startOffset == 0)
    #expect(rtl.startOffset == rtl.contentWidth - 300)
    let first = rtl.items[0]
    #expect(abs(first.x + first.width - rtl.startOffset - 300) < 0.001)
    #expect(rtl.items[1].x < first.x)
}

@Test func rightToLeftWithoutOverflowFillsTheBarFromTheRight() {
    let fit = BarFit(textWidths: [50, 50], barWidth: 387, rightToLeft: true)
    #expect(fit.startOffset == 0)
    #expect(abs(fit.items[0].x + fit.items[0].width - 387) < 0.001)
    #expect(fit.items[1].x == 0)
}

// MARK: Measuring each word once

@Test func eachWordIsMeasuredOnce() {
    var cache = TextWidthCache(capacity: 100)
    var measured: [String] = []
    let measure: (String) -> Double = { measured.append($0); return Double($0.count * 10) }
    #expect(cache.width(of: "hello", measure: measure) == 50)
    #expect(cache.width(of: "hello", measure: measure) == 50)
    #expect(cache.width(of: "he", measure: measure) == 20)
    #expect(measured == ["hello", "he"])
    #expect(cache.count == 2)
}

@Test func theCacheStaysWithinItsCapacity() {
    var cache = TextWidthCache(capacity: 4)
    for i in 0..<20 { _ = cache.width(of: "w\(i)") { _ in 1 } }
    #expect(cache.count <= 4)
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

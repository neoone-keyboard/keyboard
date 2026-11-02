import Testing
import KeyboardCore

/// Telling the keyboard's own edits from the host's, with hosts that report late.
struct OwnEditTrackerTests {
    @Test func aHostThatCaughtUpReportsAnOwnContext() {
        var tracker = OwnEditTracker()
        tracker.beginEdit(current: "te")
        tracker.inserted("h")
        tracker.endEdit(current: "teh")
        #expect(tracker.isOwn("teh"))
        #expect(!tracker.isOwn("hello"))
    }

    @Test func aHostOneUpdateLateStillCountsAsOwn() {
        var tracker = OwnEditTracker()
        tracker.beginEdit(current: "I said teh")
        // Autocorrect on space: the word is deleted and replaced.
        for _ in 0..<3 { tracker.deletedBackward() }
        tracker.inserted("the ")
        tracker.endEdit(current: "I said teh") // the host hasn't updated yet
        #expect(tracker.isOwn("I said teh"))
        #expect(tracker.isOwn("I said the "))
    }

    @Test func fastTypingOnALaggingHostBuildsOnTheExpectedText() {
        var tracker = OwnEditTracker()
        tracker.beginEdit(current: "")
        tracker.inserted("t")
        tracker.endEdit(current: "")
        tracker.beginEdit(current: "") // still lagging
        tracker.inserted("e")
        tracker.endEdit(current: "t")
        #expect(tracker.isOwn("t"))
        #expect(tracker.isOwn("te"))
    }

    @Test func aDeleteRemovesOneCodePointButAWholeEmoji() {
        var tracker = OwnEditTracker()
        tracker.beginEdit(current: "കി👍")
        tracker.deletedBackward()
        tracker.deletedBackward()
        tracker.endEdit(current: "കി👍")
        #expect(tracker.isOwn("ക"))
    }

    @Test func aCursorMoveMakesTheNextChangeExternal() {
        var tracker = OwnEditTracker()
        tracker.beginEdit(current: "hello")
        tracker.movedCursor()
        tracker.endEdit(current: "hello")
        #expect(!tracker.isOwn("hello"))
        #expect(!tracker.isOwn("hell"))
    }

    @Test func anExternalChangeStartsOver() {
        var tracker = OwnEditTracker()
        tracker.beginEdit(current: "teh")
        tracker.inserted(" ")
        tracker.endEdit(current: "teh ")
        tracker.reset(current: "pasted")
        #expect(tracker.isOwn("pasted"))
        #expect(!tracker.isOwn("teh "))
        tracker.beginEdit(current: "pasted")
        tracker.inserted("!")
        tracker.endEdit(current: "pasted!")
        #expect(tracker.isOwn("pasted!"))
    }

    @Test func aHostThatTrimsALongContextStillMatches() {
        var tracker = OwnEditTracker()
        tracker.beginEdit(current: "a long sentence that the host trims")
        tracker.inserted("x")
        tracker.endEdit(current: "a long sentence that the host trimsx")
        #expect(tracker.isOwn("sentence that the host trimsx"))
        #expect(!tracker.isOwn("trimsx"))
    }
}

@Test func aResultIsForTheWordWhateverItsApostrophe() {
    let result = SuggestionResult(typed: "don\u{2019}t", candidates: [])
    #expect(result.isFor(word: "don't"))
    #expect(!result.isFor(word: "don"))
}

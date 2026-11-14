import CoreGraphics
import Foundation
import Testing
import KeyboardCore

/// The letter keys of the English letters page on a 390 × 220 pt keyboard, as the view lays
/// them out (`KeyboardView.letterKeys`).
private func letterKeys() -> [(letter: String, rect: CGRect)] {
    KeyboardGeometry.frames(for: .forPage(.letters), in: CGSize(width: 390, height: 220)).compactMap { frame in
        guard case .character(let c) = frame.key.action, c.count == 1,
              let scalar = c.unicodeScalars.first, scalar.isASCII, scalar.properties.isAlphabetic else { return nil }
        return (c, frame.rect)
    }
}

private func rect(_ letter: String) -> CGRect {
    letterKeys().first { $0.letter == letter }!.rect
}

/// The Android port's `TapReadingTest` checks the same points: keep them in step.
struct TapReadingTests {
    @Test func aTapNearTheEdgeNamesTheKeyBesideIt() throws {
        let k = rect("k")
        // On k's left edge, towards j (still k's: the gap between the caps is touch area too).
        let point = CGPoint(x: k.minX - 1, y: k.midY)
        let reading = try #require(TapReading.read(at: point, typed: "k", keys: letterKeys()))
        #expect(reading.letter == "k")
        #expect(reading.others.first?.letter == "j")
        let j = try #require(reading.others.first).probability
        #expect(reading.probability > j && j > 0.3)
        #expect(reading.others.count <= TapReading.maxOthers)
    }

    @Test func aTapInTheMiddleNamesNoOtherKey() throws {
        let k = rect("k")
        let reading = try #require(TapReading.read(at: CGPoint(x: k.midX, y: k.midY), typed: "k", keys: letterKeys()))
        #expect(reading.others.isEmpty)
        #expect(reading.probability > 0.9)
    }

    @Test func otherKeysAreTheLikeliestAboveTheFloor() throws {
        let g = rect("g")
        // g's top-right corner: h, y and t are all near.
        let reading = try #require(TapReading.read(at: CGPoint(x: g.maxX, y: g.minY), typed: "g", keys: letterKeys()))
        #expect(reading.others.count == TapReading.maxOthers)
        #expect(reading.others.allSatisfy { $0.probability >= TapReading.floor })
        #expect(reading.others[0].probability >= reading.others[1].probability)
        let total = reading.probability + reading.others.map(\.probability).reduce(0, +)
        #expect(total <= 1.0001)
    }

    @Test func onlyLetterKeysOfTheLayoutAreRead() {
        #expect(TapReading.read(at: .zero, typed: "é", keys: letterKeys()) == nil)
        #expect(TapReading.read(at: .zero, typed: "a", keys: []) == nil)
    }

    /// Privacy: a reading printed by mistake says nothing of the tap.
    @Test func aReadingPrintsNothingOfTheTap() throws {
        let k = rect("k")
        let reading = try #require(TapReading.read(at: CGPoint(x: k.minX + 2, y: k.midY), typed: "k", keys: letterKeys()))
        let request = SuggestionRequest(contextBefore: "", language: .english, autocorrect: true, taps: [reading])
        for line in ["\(reading)", "\([reading])", "\(request.taps)"] {
            let digits = line.filter(\.isNumber)
            #expect(digits.isEmpty, "\(line)")
            #expect(line == "tap" || line == "[tap]", "\(line)")
        }
    }
}

@MainActor
struct InputControllerTapTests {
    let output = RecordingOutput()
    let input: InputController

    init() { input = InputController(output: output, languages: englishAndMalayalam) }

    private func tap(_ letter: String, _ p: Float = 0.6, other: String = "x") -> TapAlternatives {
        TapAlternatives(letter: letter, probability: p, others: [.init(letter: other, probability: 1 - p)])
    }

    private func type(_ letters: String) {
        for c in letters.map(String.init) {
            input.handle(.character(c), at: 0, tap: tap(c))
        }
    }

    @Test func eachTapGoesWithItsLetterAsTyped() {
        input.handle(.shift, at: 0)
        input.handle(.character("n"), at: 0, tap: tap("n"))
        type("kan")
        #expect(output.text == "Nkan")
        #expect(input.taps.map(\.letter) == ["N", "k", "a", "n"])
        #expect(input.taps[1] == tap("k"))
    }

    @Test func spaceReturnAndPunctuationEndTheWord() {
        type("ab")
        input.handle(.space, at: 0)
        #expect(input.taps.isEmpty)
        type("cd")
        input.handle(.returnKey, at: 0)
        #expect(input.taps.isEmpty)
        type("ef")
        input.handle(.character(","), at: 0)
        #expect(input.taps.isEmpty)
    }

    @Test func backspaceTakesTheLastTapOff() {
        type("abc")
        input.handle(.backspace, at: 0)
        #expect(input.taps.map(\.letter) == ["a", "b"])
        input.handle(.backspace, at: 0)
        input.handle(.backspace, at: 0)
        input.handle(.backspace, at: 0)
        #expect(input.taps.isEmpty)
    }

    @Test func aLetterWithNoTouchStartsTheListAgain() {
        type("ab")
        input.handle(.character("c"), at: 0) // VoiceOver
        #expect(input.taps.isEmpty)
        type("d")
        #expect(input.taps.map(\.letter) == ["d"])
        input.insertAlternate("é")
        #expect(input.taps.isEmpty)
    }

    @Test func aKeyThatIsNoLetterKeepsTheList() {
        type("don")
        input.handle(.page(.symbols), at: 0)
        input.handle(.character("'"), at: 0)
        input.handle(.page(.letters), at: 0)
        type("t")
        #expect(output.text == "don't")
        #expect(input.taps.map(\.letter) == ["d", "o", "n", "'", "t"])
        #expect(input.taps[3].others.isEmpty)
    }

    @Test func movingTheCursorOrAnEditByTheHostDropsThem() {
        type("ab")
        input.moveCursor(by: -1)
        #expect(input.taps.isEmpty)
        type("cd")
        input.textDidChangeExternally()
        #expect(input.taps.isEmpty)
        type("ef")
        input.deleteWordBackward()
        #expect(input.taps.isEmpty)
    }

    @Test func aSuggestionOrASwipeDropsThem() {
        type("tge")
        input.commitSuggestion("the")
        #expect(input.taps.isEmpty)
        type("ab")
        input.insertSwipe(Suggestion("hello", kind: .completion), alternatives: [])
        #expect(input.taps.isEmpty)
        type("ab")
        input.insertEmoji("👋")
        #expect(input.taps.isEmpty)
    }

    @Test func theRequestMadeWhileTheKeyIsDownCarriesItsTap() {
        type("tg")
        let ahead = input.taps(after: .character("e"), tap: tap("e"))
        input.handle(.character("e"), at: 0, tap: tap("e"))
        #expect(ahead == input.taps)
        // Not a letter: nothing would be added for it.
        #expect(input.taps(after: .space, tap: nil) == input.taps)
    }

    @Test func theListStaysShort() {
        type(String(repeating: "a", count: 60))
        #expect(input.taps.count == 48)
    }
}

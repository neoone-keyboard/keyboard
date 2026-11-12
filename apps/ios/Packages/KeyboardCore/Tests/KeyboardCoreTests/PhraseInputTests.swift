import Foundation
import Testing
import KeyboardCore

/// A converter like the engine's for a handful of words: romaji by longest match, the text
/// cut into known readings, each reading's surfaces best first, then its hiragana and katakana.
final class FakeConverter: PhraseConverter, @unchecked Sendable {
    static let romaji: [String: String] = [
        "kyo": "きょ", "u": "う", "ha": "は", "a": "あ", "me": "め", "ga": "が", "fu": "ふ",
        "ru": "る", "ka": "か", "ki": "き", "i": "い", "nn": "ん",
        ",": "、", ".": "。", "-": "ー", "[": "「", "]": "」", "?": "？", "!": "！", "/": "・", "~": "〜",
    ]
    /// Reading → surfaces, best first.
    static let words: [String: [String]] = [
        "きょうは": ["今日は", "京は"], "きょう": ["今日", "京"], "あめが": ["雨が", "飴が"],
        "あめ": ["雨", "飴"], "ふる": ["降る", "振る"],
    ]
    /// How the best conversion cuts text, longest first.
    static let segmentReadings = ["きょうは", "あめが", "ふる", "あめ"]

    var conversions = 0

    func compose(_ typed: String, finish: Bool) -> ComposingText? {
        var kana = ""
        var rest = Substring(typed.lowercased())
        while !rest.isEmpty {
            if let rule = Self.romaji.keys.filter({ rest.hasPrefix($0) }).max(by: { $0.count < $1.count }) {
                kana += Self.romaji[rule]!
                rest = rest.dropFirst(rule.count)
            } else if Self.romaji.keys.contains(where: { $0.count > rest.count && $0.hasPrefix(rest) }) {
                break
            } else {
                kana.append(rest.removeFirst())
            }
        }
        if finish {
            kana += rest
            rest = ""
        }
        return ComposingText(kana: kana, pending: String(rest))
    }

    func convert(_ kana: String, limit: Int) -> ConvertedPhrase? {
        conversions += 1
        let segments = Self.segments(kana)
        guard let head = segments.first else { return ConvertedPhrase(segments: [], candidates: [], first: []) }
        let whole = segments.map(\.surface).joined()
        let candidates = Self.withKana([whole], kana)
        let first = Self.withKana(Self.words[head.reading] ?? [], head.reading)
        return ConvertedPhrase(segments: segments, candidates: Array(candidates.prefix(limit)), first: Array(first.prefix(limit)))
    }

    func convert(_ kana: String, firstLength: Int, limit: Int) -> ConvertedPhrase? {
        conversions += 1
        guard firstLength >= 1, firstLength <= kana.count else { return nil }
        let head = String(kana.prefix(firstLength))
        let first = Self.withKana(Self.words[head] ?? [], head)
        let segments = [ConvertedSegment(reading: head, surface: first[0])] + Self.segments(String(kana.dropFirst(firstLength)))
        let whole = segments.map(\.surface).joined()
        return ConvertedPhrase(segments: segments, candidates: Self.withKana([whole], kana), first: Array(first.prefix(limit)))
    }

    static func segments(_ kana: String) -> [ConvertedSegment] {
        var out: [ConvertedSegment] = []
        var rest = Substring(kana)
        while !rest.isEmpty {
            if let reading = segmentReadings.first(where: { rest.hasPrefix($0) }) {
                out.append(ConvertedSegment(reading: reading, surface: words[reading]![0]))
                rest = rest.dropFirst(reading.count)
            } else {
                let c = String(rest.removeFirst())
                out.append(ConvertedSegment(reading: c, surface: c))
            }
        }
        return out
    }

    static func withKana(_ list: [String], _ kana: String) -> [String] {
        var out = list
        for form in [kana, katakana(kana)] where !out.contains(form) { out.append(form) }
        return out
    }

    static func katakana(_ text: String) -> String {
        text.applyingTransform(.hiraganaToKatakana, reverse: false) ?? text
    }
}

/// The composing state machine of a language typed as a reading and converted as a phrase
/// (Japanese, Gboard's way): marked text, space to convert and cycle, return to commit, the
/// candidates and the first segment's boundary.
@MainActor
struct PhraseInputTests {
    let output = RecordingOutput()
    let converter = FakeConverter()
    let phrase: PhraseInput

    init() { phrase = PhraseInput(output: output, converter: converter) }

    func type(_ letters: String) {
        for c in letters { #expect(phrase.type(String(c))) }
    }

    // MARK: Composing

    @Test func typedLettersShowAsMarkedKanaWithTheLettersStillWaiting() {
        type("kyouk")
        #expect(output.marked == "きょうk")
        #expect(output.committed == "")
        #expect(phrase.isComposing)
        #expect(!phrase.isConverting)
        #expect(phrase.kana == "きょう")
        #expect(phrase.pending == "k")
        type("a")
        #expect(output.marked == "きょうか")
        #expect(phrase.pending == "")
    }

    @Test func textBeforeTheCompositionIsLeftAlone() {
        output.insertText("A: ")
        type("ame")
        #expect(output.text == "A: あめ")
        #expect(output.marked == "あめ")
    }

    @Test func whileTypingTheBarHasTheWholeTextThenItsKatakanaThenTheFirstSegments() {
        type("kyouhaamegafuru")
        let texts = phrase.candidates.map(\.text)
        #expect(texts.first == "今日は雨が降る")
        #expect(texts.dropFirst().first == "キョウハアメガフル")
        #expect(texts.contains("きょうはあめがふる"))
        let kyou = try? #require(phrase.candidates.first { $0.text == "京は" })
        #expect(kyou?.readingLength == 4)
        #expect(phrase.candidates[0].readingLength == 9)
        #expect(Set(texts).count == texts.count, "no candidate twice")
        #expect(phrase.highlighted == nil)
    }

    @Test func onlyLettersWaitingHaveNoCandidates() {
        type("k")
        #expect(phrase.candidates.isEmpty)
        #expect(output.marked == "k")
    }

    // MARK: Space

    @Test func theFirstSpaceConvertsAndTheNextCycleTheFirstSegment() {
        type("kyouhaamegafuru")
        #expect(phrase.space())
        #expect(phrase.isConverting)
        #expect(output.marked == "今日は雨が降る")
        #expect(phrase.candidates.map(\.text) == ["今日は", "京は", "きょうは", "キョウハ"])
        #expect(phrase.highlighted == 0)
        #expect(phrase.space())
        #expect(output.marked == "京は雨が降る")
        #expect(phrase.highlighted == 1)
        #expect(phrase.space())
        #expect(phrase.space())
        #expect(output.marked == "キョウハ雨が降る")
        #expect(phrase.space())
        #expect(output.marked == "今日は雨が降る", "back to the best after the last")
        #expect(output.committed == "")
    }

    @Test func spaceWithNothingComposingIsNotTaken() {
        #expect(!phrase.space())
        #expect(output.text == "")
    }

    @Test func spaceFinishesLettersStillWaiting() {
        type("amen")
        #expect(output.marked == "あめn")
        #expect(phrase.space())
        #expect(phrase.isConverting)
        #expect(phrase.kana == "あめn")
        #expect(phrase.pending == "")
    }

    // MARK: Return

    @Test func returnCommitsTheMarkedTextAsShown() {
        type("kyouk")
        #expect(phrase.confirm())
        #expect(output.text == "きょうk")
        #expect(output.marked == "")
        #expect(!phrase.isComposing)
        #expect(!phrase.confirm(), "nothing left to commit")
    }

    @Test func returnCommitsTheHighlightedCandidate() {
        type("kyouhaamegafuru")
        _ = phrase.space()
        _ = phrase.space()
        #expect(phrase.confirm())
        #expect(output.text == "京は雨が降る")
        #expect(output.marked == "")
        #expect(!phrase.isComposing)
        #expect(!phrase.isConverting)
    }

    // MARK: Backspace

    @Test func backspaceDeletesALetterThenAKana() {
        type("kyouky")
        #expect(output.marked == "きょうky")
        #expect(phrase.backspace())
        #expect(output.marked == "きょうk")
        #expect(phrase.backspace())
        #expect(output.marked == "きょう")
        #expect(phrase.backspace())
        #expect(output.marked == "きょ")
        #expect(phrase.backspace())
        #expect(phrase.backspace())
        #expect(output.text == "")
        #expect(!phrase.isComposing)
        #expect(!phrase.backspace(), "the host's own backspace from here")
    }

    @Test func backspaceWhileConvertingGoesBackToTheKana() {
        type("kyouhaamegafuru")
        _ = phrase.space()
        #expect(phrase.backspace())
        #expect(!phrase.isConverting)
        #expect(output.marked == "きょうはあめがふる")
        #expect(phrase.backspace())
        #expect(output.marked == "きょうはあめがふ")
    }

    @Test func typingAfterTheLastKanaIsDeletedStartsAfresh() {
        type("a")
        _ = phrase.backspace()
        type("me")
        #expect(output.text == "め")
    }

    // MARK: Candidates

    @Test func tappingAWholeCandidateCommitsIt() {
        type("kyouhaamegafuru")
        phrase.choose(0)
        #expect(output.text == "今日は雨が降る")
        #expect(output.marked == "")
        #expect(!phrase.isComposing)
    }

    @Test func tappingTheKatakanaFormCommitsIt() {
        type("kyouhaamegafuru")
        phrase.choose(1)
        #expect(output.text == "キョウハアメガフル")
    }

    @Test func tappingAFirstSegmentCandidateWhileTypingLeavesTheRestComposing() {
        type("kyouhaamegafuru")
        let index = try? #require(phrase.candidates.firstIndex { $0.text == "京は" })
        phrase.choose(index ?? 0)
        #expect(output.committed == "京は")
        #expect(output.marked == "あめがふる")
        #expect(phrase.isComposing)
        #expect(!phrase.isConverting)
        #expect(phrase.candidates.first?.text == "雨が降る")
    }

    @Test func tappingWhileConvertingCommitsTheFirstSegmentAndConvertsTheRest() {
        type("kyouhaamegafuru")
        _ = phrase.space()
        phrase.choose(1)
        #expect(output.committed == "京は")
        #expect(output.marked == "雨が降る")
        #expect(phrase.isConverting)
        #expect(phrase.candidates.map(\.text) == ["雨が", "飴が", "あめが", "アメガ"])
        phrase.choose(0)
        phrase.choose(1)
        #expect(output.text == "京は雨が振る")
        #expect(!phrase.isComposing)
    }

    @Test func aTapOutOfRangeDoesNothing() {
        type("ame")
        phrase.choose(99)
        #expect(output.marked == "あめ")
    }

    // MARK: Segments

    @Test func theFirstSegmentShrinksAndGrows() {
        type("kyouhaamegafuru")
        _ = phrase.space()
        #expect(phrase.firstReading == "きょうは")
        #expect(phrase.canShrink)
        #expect(phrase.canGrow)
        phrase.shrink()
        #expect(phrase.firstReading == "きょう")
        #expect(phrase.candidates.map(\.text).prefix(2) == ["今日", "京"])
        #expect(output.marked == "今日は雨が降る")
        #expect(phrase.highlighted == 0)
        _ = phrase.space()
        #expect(output.marked == "京は雨が降る")
        phrase.grow()
        #expect(phrase.firstReading == "きょうは")
        #expect(output.marked == "今日は雨が降る")
        phrase.choose(0)
        #expect(output.committed == "今日は")
        #expect(output.marked == "雨が降る")
    }

    @Test func theFirstSegmentStopsAtOneKanaAndAtTheWholeText() {
        type("ame")
        _ = phrase.space()
        #expect(!phrase.canGrow)
        phrase.grow()
        #expect(phrase.firstReading == "あめ")
        phrase.shrink()
        #expect(phrase.firstReading == "あ")
        #expect(!phrase.canShrink)
        phrase.shrink()
        #expect(phrase.firstReading == "あ")
        #expect(output.marked == "あめ")
    }

    @Test func movingTheBoundaryWhileTypingConvertsFirst() {
        type("kyouhaamegafuru")
        phrase.shrink()
        #expect(phrase.isConverting)
        #expect(phrase.firstReading == "きょう")
    }

    // MARK: Other keys

    @Test func aLetterAfterAConversionCommitsItAndStartsAgain() {
        type("kyouhaamegafuru")
        _ = phrase.space()
        type("k")
        #expect(output.committed == "今日は雨が降る")
        #expect(output.marked == "k")
        #expect(!phrase.isConverting)
    }

    @Test func punctuationWithNothingComposingGoesInAsTheLanguagesMark() {
        type(",")
        #expect(output.text == "、")
        #expect(!phrase.isComposing)
        type("x")
        #expect(output.marked == "x")
    }

    @Test func punctuationWhileComposingJoinsTheComposition() {
        type("ame.")
        #expect(output.marked == "あめ。")
        type("「")
        #expect(output.marked == "あめ。「")
    }

    @Test func commitLeavesTheTextAsShownAndResetLeavesItAlone() {
        type("ame")
        phrase.commit()
        #expect(output.text == "あめ")
        #expect(output.marked == "")
        type("fu")
        phrase.reset()
        #expect(!phrase.isComposing)
        #expect(output.text == "あめふ", "the host keeps what it had")
    }

    @Test func cancelDeletesTheComposition() {
        output.insertText("x")
        type("ame")
        phrase.cancel()
        #expect(output.text == "x")
        #expect(!phrase.isComposing)
    }
}

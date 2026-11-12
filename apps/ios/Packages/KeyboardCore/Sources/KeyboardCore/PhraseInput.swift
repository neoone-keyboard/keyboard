import Foundation

/// The composing text of letters typed so far: the kana written and the letters still waiting
/// for more (`k` of `kyouk`). The engine's `Composition`.
public struct ComposingText: Equatable, Sendable {
    public var kana: String
    public var pending: String

    public init(kana: String, pending: String) {
        self.kana = kana
        self.pending = pending
    }
}

/// One segment of a conversion: a word and the particles or endings after it (今日は), with
/// its reading (きょうは). The engine's `PhraseSegment`.
public struct ConvertedSegment: Equatable, Sendable {
    public var reading: String
    public var surface: String

    public init(reading: String, surface: String) {
        self.reading = reading
        self.surface = surface
    }
}

/// What converting kana gives. The engine's `PhraseConversion`.
public struct ConvertedPhrase: Equatable, Sendable {
    /// The best conversion, segment by segment; the readings join to the kana converted.
    public var segments: [ConvertedSegment]
    /// Candidates for the whole text, best first, its hiragana and katakana among them.
    public var candidates: [String]
    /// Candidates for the first segment, best first, its hiragana and katakana among them.
    public var first: [String]

    public init(segments: [ConvertedSegment], candidates: [String], first: [String]) {
        self.segments = segments
        self.candidates = candidates
        self.first = first
    }
}

/// A candidate in the bar: its text, and how many characters of the kana it replaces from
/// the start (the whole text, or the first segment); what follows stays composing.
public struct PhraseCandidate: Equatable, Sendable {
    public let text: String
    public let readingLength: Int

    public init(text: String, readingLength: Int) {
        self.text = text
        self.readingLength = readingLength
    }
}

/// What the space and return keys read while text is composing, as Japanese keyboards label
/// them (Japanese is the one phrase-conversion language).
public enum PhraseLabels {
    /// Space: convert, then the next candidate.
    public static let nextCandidate = "次候補"
    /// Return: commit the text as shown.
    public static let confirm = "確定"
}

/// A language's composer and converter (the engine's `compose`, `convert` and
/// `convertWithFirst` for one conversion language). Called on the main thread: a conversion
/// takes well under a millisecond (`make bench-ja`).
public protocol PhraseConverter: AnyObject, Sendable {
    /// `typed`, letters typed from the start of a composition, as kana and the letters still
    /// waiting; `finish` writes those too (`n` → ん). Nil when the converter can't be used.
    func compose(_ typed: String, finish: Bool) -> ComposingText?
    /// Up to `limit` candidates for `kana` whole and for its first segment.
    func convert(_ kana: String, limit: Int) -> ConvertedPhrase?
    /// The same with the first segment fixed to the first `firstLength` characters of `kana`.
    /// Nil when that isn't from 1 to the length of `kana`.
    func convert(_ kana: String, firstLength: Int, limit: Int) -> ConvertedPhrase?
}

/// Typing a language converted as a phrase (Japanese), as Gboard does it:
///
/// - letters compose: the kana, then any letters still waiting, are the host's marked text;
/// - the bar offers the whole text converted, best first, its katakana second, then
///   candidates for its first segment, each replacing what it covers;
/// - space converts the whole text into segments, the first of them active, and each space
///   after shows the active segment's next candidate in the marked text;
/// - the first segment's boundary moves by a kana either way (`shrink`, `grow`), converting
///   it and the rest again;
/// - return commits the marked text as shown; backspace deletes a waiting letter, else a kana,
///   and while converting goes back to the kana;
/// - a candidate tapped while converting commits the first segment and converts the rest.
@MainActor
public final class PhraseInput {
    /// Candidates asked of the converter: enough to fill the expanded grid.
    public static let limit = 30

    public let converter: PhraseConverter
    private let output: TextOutput
    /// The kana written so far, and the letters waiting for more.
    public private(set) var kana = ""
    public private(set) var pending = ""
    /// The conversion shown, from the first space until the text is committed or unconverted.
    private var converted: ConvertedPhrase?
    /// The first segment's candidate shown (`converted.first`).
    private var selected = 0
    /// The text converted as it is typed, for the bar.
    private var prediction: ConvertedPhrase?

    public init(output: TextOutput, converter: PhraseConverter) {
        self.output = output
        self.converter = converter
    }

    public var isComposing: Bool { !kana.isEmpty || !pending.isEmpty }
    /// Space has converted the text: the bar has the first segment's candidates.
    public var isConverting: Bool { converted != nil }

    /// The active (first) segment's reading while converting.
    public var firstReading: String? { converted?.segments.first?.reading }
    /// The first segment can lose a kana to the next.
    public var canShrink: Bool { (firstReading?.count ?? 0) > 1 }
    /// The first segment can take a kana from the next.
    public var canGrow: Bool { firstReading.map { $0.count < kana.count } ?? false }

    /// What the host shows as marked text: the kana and the letters waiting, or the conversion
    /// with the first segment's candidate shown.
    public var markedText: String {
        guard let converted else { return kana + pending }
        return shownFirst(converted) + converted.segments.dropFirst().map(\.surface).joined()
    }

    /// The bar's candidates, best first.
    public var candidates: [PhraseCandidate] {
        if let converted {
            let length = converted.segments.first?.reading.count ?? kana.count
            return converted.first.map { PhraseCandidate(text: $0, readingLength: length) }
        }
        guard !kana.isEmpty, let prediction else { return [] }
        let whole = Self.katakanaSecond(prediction.candidates)
        let total = prediction.segments.reduce(0) { $0 + $1.reading.count }
        let firstLength = prediction.segments.first?.reading.count ?? total
        var seen = Set<String>()
        return (whole.map { PhraseCandidate(text: $0, readingLength: total) }
                + prediction.first.map { PhraseCandidate(text: $0, readingLength: firstLength) })
            .filter { seen.insert($0.text).inserted }
    }

    /// The candidate shown in the marked text while converting; nil while typing.
    public var highlighted: Int? { converted == nil ? nil : selected }

    // MARK: Keys

    /// Types `text` (a letter, a mark, a long-press alternate). A letter after a conversion
    /// commits it first. With nothing composing, anything but a letter goes straight in as the
    /// language writes it (`,` → 、). Always handled.
    @discardableResult
    public func type(_ text: String) -> Bool {
        if converted != nil { commit() }
        if !isComposing, !Self.startsComposition(text) {
            output.insertText(converter.compose(text, finish: true)?.kana ?? text)
            return true
        }
        for c in text {
            let composed = converter.compose(pending + String(c), finish: false)
                ?? ComposingText(kana: pending + String(c), pending: "")
            kana += composed.kana
            pending = composed.pending
        }
        typingChanged()
        return true
    }

    /// Space: converts, then shows the next candidate for the first segment. False with
    /// nothing composing, and the key types a space.
    public func space() -> Bool {
        guard isComposing else { return false }
        if let converted {
            if !converted.first.isEmpty { selected = (selected + 1) % converted.first.count }
            render()
        } else {
            startConverting()
        }
        return true
    }

    /// Return: commits the marked text as shown. False with nothing composing.
    public func confirm() -> Bool {
        guard isComposing else { return false }
        commitShown()
        return true
    }

    /// Backspace: back from a conversion to the kana; else the last waiting letter, else the
    /// last kana. False with nothing composing, and the key deletes as usual.
    public func backspace() -> Bool {
        guard isComposing else { return false }
        if converted != nil {
            converted = nil
            selected = 0
            typingChanged()
            return true
        }
        if !pending.isEmpty { pending.removeLast() } else { kana.removeLast() }
        if isComposing {
            typingChanged()
        } else {
            prediction = nil
            output.setMarkedText("")
            output.unmarkText()
        }
        return true
    }

    /// A tap on the bar's candidate `index`: commits it in place of the kana it covers; what
    /// follows stays composing, converted again if it was.
    public func choose(_ index: Int) {
        let list = candidates
        guard list.indices.contains(index) else { return }
        let candidate = list[index]
        let wasConverting = converted != nil
        finishLetters()
        let rest = String(kana.dropFirst(candidate.readingLength))
        output.setMarkedText(candidate.text)
        output.unmarkText()
        clear()
        guard !rest.isEmpty else { return }
        kana = rest
        if wasConverting { startConverting() } else { typingChanged() }
    }

    /// Gives the first segment's last kana to the next segment.
    public func shrink() { moveBoundary(by: -1) }

    /// Takes the next segment's first kana into the first.
    public func grow() { moveBoundary(by: 1) }

    /// Commits the marked text as shown, if anything is composing: before the language
    /// changes, the cursor moves or an emoji goes in.
    public func commit() {
        if isComposing { commitShown() }
    }

    /// Deletes the composition (delete-word on it).
    public func cancel() {
        guard isComposing else { return }
        output.setMarkedText("")
        output.unmarkText()
        clear()
    }

    /// The host changed the text: whatever it kept of the marked text stays as it is, and
    /// typing starts afresh.
    public func reset() {
        guard isComposing else { return }
        output.unmarkText()
        clear()
    }

    // MARK: Inside

    private static func startsComposition(_ text: String) -> Bool {
        text.unicodeScalars.first.map { $0.isASCII && $0.properties.isAlphabetic } ?? false
    }

    /// The whole text's katakana form moved to second place: the katakana toggle in reach.
    private static func katakanaSecond(_ list: [String]) -> [String] {
        guard list.count > 2, let first = list.first,
              let index = list.indices.dropFirst().first(where: { isKatakana(list[$0]) && list[$0] != first })
        else { return list }
        var out = list
        let katakana = out.remove(at: index)
        out.insert(katakana, at: 1)
        return out
    }

    private static func isKatakana(_ text: String) -> Bool {
        !text.isEmpty && text.unicodeScalars.allSatisfy { (0x30A0...0x30FF).contains($0.value) }
    }

    private func shownFirst(_ converted: ConvertedPhrase) -> String {
        converted.first.indices.contains(selected) ? converted.first[selected] : converted.segments.first?.surface ?? ""
    }

    /// The letters waiting written as kana, as before converting.
    private func finishLetters() {
        guard !pending.isEmpty else { return }
        kana += converter.compose(pending, finish: true)?.kana ?? pending
        pending = ""
    }

    private func startConverting() {
        finishLetters()
        prediction = nil
        selected = 0
        if let found = converter.convert(kana, limit: Self.limit), !found.segments.isEmpty, !found.first.isEmpty {
            converted = found
        } else {
            converted = ConvertedPhrase(segments: [ConvertedSegment(reading: kana, surface: kana)],
                                        candidates: [kana], first: [kana])
        }
        render()
    }

    private func moveBoundary(by delta: Int) {
        if converted == nil {
            guard isComposing else { return }
            startConverting()
        }
        guard let reading = firstReading else { return }
        let length = reading.count + delta
        guard length >= 1, length <= kana.count, length != reading.count,
              let found = converter.convert(kana, firstLength: length, limit: Self.limit), !found.first.isEmpty
        else { return }
        converted = found
        selected = 0
        render()
    }

    /// The kana or the letters changed while typing: the bar's conversion and the marked text.
    private func typingChanged() {
        if kana.isEmpty {
            prediction = nil
        } else {
            let waiting = pending.isEmpty ? "" : converter.compose(pending, finish: true)?.kana ?? pending
            prediction = converter.convert(kana + waiting, limit: Self.limit)
        }
        render()
    }

    private func render() {
        output.setMarkedText(markedText)
    }

    private func commitShown() {
        output.setMarkedText(markedText)
        output.unmarkText()
        clear()
    }

    private func clear() {
        kana = ""
        pending = ""
        converted = nil
        selected = 0
        prediction = nil
    }
}

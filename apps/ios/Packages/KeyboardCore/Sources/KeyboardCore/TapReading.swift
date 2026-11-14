import CoreGraphics

/// Where a finger landed on a letter key, as the engine reads it: the letter typed (the key
/// under the finger, as inserted, so in its case) with its probability, and up to
/// `TapReading.maxOthers` other letter keys the tap could have meant, with theirs. Only
/// suggestions and autocorrect use it (`SuggestionRequest.taps`); the letter typed is always
/// the key under the finger.
///
/// Privacy: a reading holds no touch point, only letters and probabilities, and lives in
/// memory for the word being typed (`InputController.taps`). It is never logged, never
/// written to the diagnostics log, the learned words or any file; its `description` says
/// nothing of it, so a log line that interpolates one by mistake still leaks nothing.
public struct TapAlternatives: Equatable, Sendable, CustomStringConvertible {
    public struct Other: Equatable, Sendable {
        public let letter: String
        public let probability: Float

        public init(letter: String, probability: Float) {
            self.letter = letter
            self.probability = probability
        }
    }

    public let letter: String
    public let probability: Float
    /// Most likely first; empty for a tap in the middle of its key.
    public let others: [Other]

    public init(letter: String, probability: Float, others: [Other]) {
        self.letter = letter
        self.probability = probability
        self.others = others
    }

    /// A reading for `letter` as inserted (shift's case), keeping the probabilities.
    public func typed(as inserted: String) -> TapAlternatives {
        TapAlternatives(letter: inserted, probability: probability, others: others)
    }

    public var description: String { "tap" }
}

/// Works out a tap's `TapAlternatives` from its touch point: a 2-D Gaussian around each letter
/// key's centre, `sigma` key widths wide, normalised over the letter keys of the visible
/// layout. The other keys with at least `floor` of the probability are kept, at most
/// `maxOthers`, most likely first.
///
/// The numbers were chosen on the synthetic-tap eval (`make eval-taps`,
/// engine/core/tests/tap_eval.rs, whose `APP_SIGMA`, `APP_FLOOR` and `APP_OTHERS` are these):
/// Android's `TapReading.kt` uses the same ones. Change all three together.
public enum TapReading {
    /// The Gaussian's spread, in key widths.
    public static let sigma: CGFloat = 0.4
    /// Other keys below this probability are left out.
    public static let floor: Float = 0.05
    public static let maxOthers = 2

    /// The reading of a tap at `point` that typed the letter key `typed` (lower case), among
    /// `keys` (each letter key's lower-case letter and cap rect, in the same points as `point`).
    /// Nil when `typed` isn't among them: only letter keys of the visible layout are read.
    public static func read(
        at point: CGPoint, typed: String, keys: [(letter: String, rect: CGRect)],
        sigma: CGFloat = TapReading.sigma, floor: Float = TapReading.floor, maxOthers: Int = TapReading.maxOthers
    ) -> TapAlternatives? {
        guard let own = keys.first(where: { $0.letter == typed }), own.rect.width > 0 else { return nil }
        // A key width is the step from one key's centre to the next in its row (the cap and
        // the gap beside it), as the eval's key widths are.
        let pitch = keys
            .filter { $0.letter != typed && abs($0.rect.midY - own.rect.midY) < own.rect.height / 2 }
            .map { abs($0.rect.midX - own.rect.midX) }
            .filter { $0 > 0 }
            .min() ?? own.rect.width
        let spread = sigma * pitch
        let weights = keys.map { key -> Double in
            let dx = point.x - key.rect.midX, dy = point.y - key.rect.midY
            return exp(-Double(dx * dx + dy * dy) / Double(2 * spread * spread))
        }
        let total = weights.reduce(0, +)
        guard total > 0 else {
            return TapAlternatives(letter: typed, probability: 1, others: [])
        }
        var probability: Float = 0
        var others: [TapAlternatives.Other] = []
        for (key, weight) in zip(keys, weights) {
            let p = Float(weight / total)
            if key.letter == typed {
                probability = max(probability, p)
            } else if p >= floor {
                others.append(.init(letter: key.letter, probability: p))
            }
        }
        others.sort { $0.probability != $1.probability ? $0.probability > $1.probability : $0.letter < $1.letter }
        return TapAlternatives(letter: typed, probability: probability, others: Array(others.prefix(maxOthers)))
    }
}

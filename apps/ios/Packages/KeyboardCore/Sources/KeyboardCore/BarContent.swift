/// What the strip above the keys shows next (`BarState.content`).
public enum BarContent: Equatable, Sendable {
    /// The bar stays as it is: a result for the word is on its way, so it isn't redrawn
    /// twice per key.
    case unchanged
    /// A short note in place of the slots.
    case message(String)
    /// The last swipe's word and its alternatives, which stay until the next key.
    case swipe(SuggestionBar)
    /// After a consonant of a pack's script: the vowel signs drawn on `base`, likeliest
    /// first, with the best completion beside them.
    case vowelSigns(base: String, signs: [String], completion: Suggestion?)
    /// The suggestion slots, before the emoji for the word goes in the right one. `hasResult`
    /// is whether the engine's result for the word is in them (else they are empty).
    case suggestions(SuggestionBar, hasResult: Bool)

    /// The bar is now made for the word before the cursor (a tap on a bar made for another
    /// word is ignored).
    public var isForCurrentWord: Bool {
        switch self {
        case .unchanged, .message: false
        case .swipe, .vowelSigns, .suggestions: true
        }
    }
}

/// What the keyboard knows when it redraws the bar.
public struct BarState: Sendable {
    public static let swipeFailedMessage = "Swipe again"

    /// The last swipe gave no word: the bar says so until the next key.
    public var swipeFailed = false
    /// The word before the cursor (`InputController.currentWord`).
    public var word = ""
    /// The last swipe's word and alternatives while the bar shows them
    /// (`InputController.showsSwipe`, `suggestions`); nil otherwise.
    public var swipe: SuggestionResult?
    /// The engine's last delivered result.
    public var latest = SuggestionResult.empty
    /// A request was sent and neither its result nor its timeout has arrived.
    public var awaitingResult = false
    /// The vowel strip is showing now (rather than the slots).
    public var showsVowelSigns = false
    /// The word the bar's slots were made for (straight apostrophes).
    public var barWord: String?
    public var language = Language.english
    /// How many suggestions the bar holds (`KeyboardSettings.suggestionCount`).
    public var limit = SuggestionBar.slotCount

    public init() {}

    /// `word` with ’ as ', as `barWord` keeps it.
    public var straightWord: String {
        word.replacingOccurrences(of: "\u{2019}", with: "'")
    }

    /// After a consonant of a pack with a vowel strip (`markBase` gives the consonant, nil
    /// when the cursor isn't after one): the best completion and the vowel signs, likeliest
    /// first. Otherwise the suggestion slots, as many as `limit`.
    ///
    /// Only a result for the word now before the cursor is shown. While that result is on its
    /// way the bar stays as it is (a strip, or slots for this same word). Once the result or
    /// its timeout arrives without a match, the strip falls back to the layout's order and
    /// the slots go empty.
    public func content(markBase: (PackDescriptor) -> String?) -> BarContent {
        if swipeFailed { return .message(Self.swipeFailedMessage) }
        let word = straightWord
        if let swipe { return .swipe(SuggestionBar(swipe, limit: limit)) }
        let current = latest.isFor(word: word) ? latest : nil
        if case .pack(let pack) = language, let layout = pack.layout, !layout.vowelSignStrip.isEmpty,
           let base = markBase(pack) {
            // Vowel signs apply to whatever consonant is there, so an older strip can stay.
            if current == nil, awaitingResult, showsVowelSigns { return .unchanged }
            return .vowelSigns(
                base: base,
                signs: layout.orderedStripSigns(preferred: current?.vowelSigns ?? []),
                completion: current?.candidates.first { $0.kind == .completion }
            )
        }
        if current == nil, awaitingResult, !showsVowelSigns, barWord == word { return .unchanged }
        return .suggestions(SuggestionBar(current ?? .empty, limit: limit), hasResult: current != nil)
    }
}

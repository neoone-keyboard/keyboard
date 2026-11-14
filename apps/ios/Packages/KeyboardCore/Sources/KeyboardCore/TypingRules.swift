/// What the host's text field is for, as far as the keyboard cares (its `UIKeyboardType`).
public enum FieldKind: Sendable {
    case text, url, email, webSearch
}

/// What the keyboard does in a field, from the user's settings and the field's traits.
public enum FieldRules {
    /// Autocorrect needs the user's setting, a host field that allows it, and a field that
    /// isn't for URLs, email addresses or web searches.
    public static func autocorrects(setting: Bool, field: FieldKind, fieldAllows: Bool) -> Bool {
        switch field {
        case .url, .email, .webSearch: false
        case .text: setting && fieldAllows
        }
    }

    /// The bar offers an emoji for the word if the setting is on and the field isn't for a
    /// password, a URL or an email address.
    public static func offersEmoji(setting: Bool, field: FieldKind, secure: Bool) -> Bool {
        setting && !secure && field != .url && field != .email
    }
}

extension SuggestionRequest {
    /// How much of the text before the cursor the engine is given.
    public static let contextLength = 64

    /// The request for the text before the cursor, of which the engine gets the end.
    public init(textBefore: String, language: Language, autocorrect: Bool, taps: [TapAlternatives] = []) {
        self.init(contextBefore: String(textBefore.suffix(Self.contextLength)), language: language,
                  autocorrect: autocorrect, taps: taps)
    }
}

/// When a swipe is decoded and when its words are used.
public enum SwipeRules {
    /// A swipe's words arriving later than this are dropped, and the bar asks for it again.
    public static let timeout: Duration = .milliseconds(250)

    /// Swipes are decoded with prediction on, on the English keys, outside password fields.
    public static func decodes(predictionOn: Bool, language: Language, secure: Bool) -> Bool {
        predictionOn && language == .english && !secure
    }

    /// The context the engine ranks a swipe's words in: the end of the text before the
    /// cursor, with the space the word goes in after (`InputController.insertSwipe`).
    public static func context(textBefore: String) -> String {
        var context = String(textBefore.suffix(SuggestionRequest.contextLength))
        if let last = context.last, !last.isWhitespace { context += " " }
        return context
    }

    /// The words to type and show, every one the engine gave; nil when it gave none or they
    /// came after `timeout`.
    public static func accepted(_ found: [Suggestion]?, after elapsed: Duration) -> [Suggestion]? {
        guard let found, !found.isEmpty, elapsed <= timeout else { return nil }
        return found
    }
}

/// The engine's cutoffs, in milliseconds from the start of a suggest (`EngineConfig`).
public struct EngineDeadlines: Equatable, Sendable {
    /// After which the transliterator stops and its finished spellings are used. Typing
    /// leaves the CPU idle between keys, and a suggest that starts from idle runs 5–7× slower
    /// than the same one back to back (iPhone 15 Pro Max, debug app's `-suggestCheck`): a
    /// 4 ms suggest took 20–25 ms, past the coordinator's 15 ms, and the bar went empty. With
    /// 8 ms, the slowest still end by about 11 ms.
    public var model: UInt32
    /// LM cutoffs (design §4). Prefill while typing stops at 3 ms, so the transliterator
    /// keeps most of its 8 ms. After a space nothing else runs, so the LM may use 10 ms.
    /// Tier-1 rows stop at 11 ms, near where M3a's slowest suggests already end, under the
    /// coordinator's 15 ms.
    public var lmPrefill: UInt32
    public var lmAfterSpace: UInt32
    public var lm: UInt32
    /// When the keyboard appears, before any key: the context is filled for up to 6 ms.
    public var lmPrepare: UInt32

    public init(model: UInt32, lmPrefill: UInt32, lmAfterSpace: UInt32, lm: UInt32, lmPrepare: UInt32) {
        self.model = model
        self.lmPrefill = lmPrefill
        self.lmAfterSpace = lmAfterSpace
        self.lm = lm
        self.lmPrepare = lmPrepare
    }

    public static let keyboard = EngineDeadlines(model: 8, lmPrefill: 3, lmAfterSpace: 10, lm: 11, lmPrepare: 6)
}

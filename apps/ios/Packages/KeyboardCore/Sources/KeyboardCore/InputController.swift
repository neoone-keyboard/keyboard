import Foundation

/// Where typed text goes. The keyboard extension wraps `textDocumentProxy`; tests record text.
@MainActor
public protocol TextOutput: AnyObject {
    func insertText(_ text: String)
    func deleteBackward()
    func adjustTextPosition(byCharacterOffset offset: Int)
    /// Text before the cursor, as far as the host app shares it.
    var contextBefore: String? { get }
    /// Whether the field has any text, even when `contextBefore` is nil.
    var hasText: Bool { get }
}

public enum AutoCapitalization: Sendable {
    case off, words, sentences, allCharacters
}

public enum Language: Sendable {
    case english, malayalam
}

/// Something the engine should learn from what the user just did.
public enum LearningEvent: Equatable, Sendable {
    /// A word was committed as typed. Words in no lexicon are learned the second time.
    case committed(String)
    /// The user rejected a replacement: never replace this word again.
    case learn(String)
}

/// A word that went into the text, typed, corrected or tapped: the engine learns from it
/// which words follow which, how often each is used, and which language the user writes.
public struct WordUse: Equatable, Sendable {
    /// The word as it went in.
    public var word: String
    /// The end of the text before it (`contextLength` characters at most), for the words
    /// it follows.
    public var before: String
    public var language: Language
    /// The typed letters, when the user tapped this correction or Malayalam word though the
    /// bar's middle offered something else: it is preferred for those letters next time.
    public var chosenFor: String?

    /// Enough for the two words before.
    public static let contextLength = 64

    public init(word: String, before: String, language: Language, chosenFor: String? = nil) {
        self.word = word
        self.before = before
        self.language = language
        self.chosenFor = chosenFor
    }
}

/// Turns key actions into text and tracks which page is showing. Has no
/// dependency on the engine, so typing keeps working when prediction fails
/// (spec §4 rule 3).
@MainActor
public final class InputController {
    public private(set) var shift: ShiftState = .off
    public private(set) var page: KeyboardPage = .letters
    public private(set) var language: Language = .english
    public var numberRow = false
    public var showsNextKeyboard = true

    /// Reports words to learn. The extension forwards these to the engine off the main thread.
    public var onLearningEvent: (@MainActor (LearningEvent) -> Void)?
    /// Reports every word that goes into the text (never from a password field, never an
    /// email or URL). The engine learns phone numbers from these, with the groups typed
    /// before them. Forwarded to the engine like `onLearningEvent`.
    public var onWordUsed: (@MainActor (WordUse) -> Void)?
    /// The latest suggestions; space and punctuation commit their middle slot.
    public private(set) var suggestions: SuggestionResult = .empty
    /// Whether space and punctuation may replace the word. The extension sets
    /// this from the user's setting and the host field's traits.
    public var autocorrectEnabled = true
    /// A password field: nothing typed is reported for learning or use.
    public var secureTextEntry = false
    /// The engine's learned emails, URLs and phone numbers. Letters that begin one are one
    /// word with their punctuation (`neo.9` of sam.2024@example.com), and digits that begin a
    /// number with its spaces (`+91 98` of +91 55501 12345), as in the engine. May be a little stale.
    public var learnedAddresses: [String] = []

    private let output: TextOutput
    private var lastShiftTap: TimeInterval?
    /// The automatic replacement made by the last key, which backspace can undo.
    private var lastReplacement: Replacement?
    /// A word the user restored with backspace; it is never replaced again.
    private var keptWord: String?

    /// A full stop after a word that whitespace has not yet followed (`holdFullStop`).
    private var heldStop: HeldStop?

    private struct Replacement {
        let typed: String
        let replacement: String
        /// Typed after the word and put back with it by undo: the `.` of `teh.` + space.
        var kept = ""
        let separator: String
    }

    /// The word before a full stop, with what ending it at whitespace does: put in
    /// `replacement`, or report the word when `reports`.
    private struct HeldStop {
        let word: String
        let replacement: String?
        let reports: Bool
    }

    /// Typed straight after a word, these commit the middle suggestion first. A full stop
    /// waits for the next key: only whitespace after it ends the word (`googel.com`).
    private static let committingPunctuation: Set<String> = [".", ",", "?", "!"]
    private var shiftIsAutomatic = false
    /// Set when the user taps shift: auto-capitalisation leaves shift alone until
    /// the text before the cursor differs from `manualShiftContext`.
    private var manualShiftPending = false
    private var manualShiftContext: String?

    public init(output: TextOutput) {
        self.output = output
    }

    public var layout: KeyboardLayout {
        .forPage(page, numberRow: numberRow, showsNextKeyboard: showsNextKeyboard)
    }

    /// `time` is monotonic seconds, e.g. `ProcessInfo.processInfo.systemUptime`.
    public func handle(_ action: KeyAction, at time: TimeInterval) {
        let undoable = lastReplacement
        lastReplacement = nil
        let stop = heldStop
        heldStop = nil
        switch action {
        case .character(".") where !continuesAddress(with: "."):
            holdFullStop()
        case .character(let c) where Self.committingPunctuation.contains(c) && !continuesAddress(with: c):
            if !commitMiddle(separator: c) {
                noteCommittedWord()
                type(c)
            }
        case .character(let c):
            type(c)
        case .space:
            if !end(stop, with: " "), !commitMiddle(separator: " ") {
                noteCommittedWord()
                output.insertText(" ")
            }
            if page == .symbols || page == .moreSymbols { page = alphabetPage }
        case .returnKey:
            if !end(stop, with: "\n") {
                noteCommittedWord()
                output.insertText("\n")
            }
        case .backspace:
            if let undoable, undo(undoable) { break }
            output.deleteBackward()
        case .shift:
            let interval = lastShiftTap.map { time - $0 }
            lastShiftTap = time
            heldStop = stop // Nothing was typed after the full stop yet.
            shift = shift.tapped(sinceLastTap: interval)
            shiftIsAutomatic = false
            manualShiftPending = true
            manualShiftContext = output.contextBefore
        case .nextKeyboard, .emoji:
            break // The host view controller switches keyboards, or shows the emoji panel.
        case .page(let target):
            // The text is unchanged, so the result still describes the word:
            // `teh`, 123, `,` gives `the,`, as the bar showed.
            heldStop = stop
            page = target == .letters ? alphabetPage : target
        case .toggleLanguage:
            suggestions = .empty
            language = language == .english ? .malayalam : .english
            page = alphabetPage
            shift = .off
            shiftIsAutomatic = false
            manualShiftPending = false
        }
    }

    /// Types a long-press alternate; follows shift like a normal letter.
    public func insertAlternate(_ text: String) {
        lastReplacement = nil
        heldStop = nil
        type(text)
    }

    /// Types an emoji from the panel as it is, with no space. Never reported for learning.
    public func insertEmoji(_ emoji: String) {
        lastReplacement = nil
        heldStop = nil
        output.insertText(emoji)
        shift = shift.afterCharacter
        if shift == .off { shiftIsAutomatic = false }
    }

    /// A tap on the bar's emoji. The word being typed stays as typed, as space without
    /// autocorrect would leave it, and the emoji follows it and a space: `pizza` and
    /// `pizza ` both give `pizza 🍕 `.
    public func commitEmojiSuggestion(_ emoji: String) {
        if !currentWord.isEmpty {
            noteCommittedWord()
            output.insertText(" ")
        }
        insertEmoji(emoji + " ")
        suggestions = .empty
    }

    /// The emoji panel's backspace: an emoji goes whole, even in a host that takes one code
    /// point per delete (👋🏽 is two); anything else as the backspace key does it.
    public func deleteBackwardWholeEmoji() {
        let context = output.contextBefore ?? ""
        guard let last = context.last, EmojiSupport.isEmoji(last) else {
            handle(.backspace, at: 0)
            return
        }
        forgetPendingState()
        delete(String(last), from: context)
    }

    public func moveCursor(by offset: Int) {
        forgetPendingState()
        output.adjustTextPosition(byCharacterOffset: offset)
    }

    /// Deletes the word before the cursor plus the spaces after it, or one
    /// character when there is no word (e.g. punctuation).
    public func deleteWordBackward() {
        forgetPendingState()
        let context = output.contextBefore ?? ""
        let word = Self.wordSuffix(of: context, includingSpaces: true)
        if let removed = word.isEmpty ? context.last.map(String.init) : String(word) {
            delete(removed, from: context)
        } else {
            output.deleteBackward()
        }
    }

    /// Stores the engine's latest result. Only a result for the word now being
    /// typed is ever committed, so a late result can't replace the wrong word.
    public func updateSuggestions(_ result: SuggestionResult) {
        suggestions = result
    }

    /// Call from `textDidChange` when the text isn't what this keyboard just
    /// produced (the host or the user changed it). Drops the undo and the result.
    public func textDidChangeExternally() {
        forgetPendingState()
    }

    private func forgetPendingState() {
        lastReplacement = nil
        heldStop = nil
        suggestions = .empty
    }

    /// The word before the cursor, as the host shares it ("" after a space).
    /// A leading quote isn't part of it (`'teh` → `teh`), as in the engine.
    public var currentWord: String {
        String(word(endingAt: output.contextBefore ?? ""))
    }

    /// A tap on a bar slot. Tapping the quoted typed word keeps it and learns it.
    public func commitSuggestion(_ slot: SuggestionSlot) {
        if slot.quoted { report(.learn(slot.suggestion.text)) }
        insert(suggestion: slot.suggestion.text, reportsCommit: !slot.quoted, chosenFor: choice(slot))
    }

    /// Replaces the word being typed with `text` and adds a space, as a tap on a suggestion does.
    public func commitSuggestion(_ text: String) {
        insert(suggestion: text, reportsCommit: true, chosenFor: nil)
    }

    /// The typed letters `slot` is a choice for: a correction or Malayalam word tapped
    /// though the middle offered something else. Completions only finish the letters.
    private func choice(_ slot: SuggestionSlot) -> String? {
        let typed = currentWord
        guard !slot.quoted, !typed.isEmpty, suggestions.isFor(word: typed),
              slot.suggestion.kind == .correction || slot.suggestion.kind == .manglish,
              suggestions.middle?.text != slot.suggestion.text
        else { return nil }
        return typed
    }

    private func insert(suggestion text: String, reportsCommit: Bool, chosenFor: String?) {
        lastReplacement = nil
        heldStop = nil
        suggestions = .empty
        let context = output.contextBefore ?? ""
        let word = self.word(endingAt: context)
        if !word.isEmpty { delete(String(word), from: context) }
        output.insertText(text + " ")
        shift = shift.afterCharacter
        if shift == .off { shiftIsAutomatic = false }
        if reportsCommit { report(.committed(text)) }
        reportUse(text, before: String(context[..<word.startIndex]), chosenFor: chosenFor)
    }

    /// Turns shift on at the start of a sentence (or word) and off again when it
    /// no longer applies. Never overrides caps lock or a shift the user tapped,
    /// including tapping it off, until the text context changes.
    public func applyAutoCapitalization(_ mode: AutoCapitalization) {
        if manualShiftPending {
            guard output.contextBefore != manualShiftContext else { return }
            manualShiftPending = false
        }
        guard page == .letters, shift != .locked, shift == .off || shiftIsAutomatic else { return }
        let context: String
        if let shared = output.contextBefore {
            context = shared
        } else if output.hasText {
            return // The host hides its text, so where the cursor sits is unknown.
        } else {
            context = ""
        }
        let wanted = Self.shouldCapitalize(context, mode: mode)
        shift = wanted ? .once : .off
        shiftIsAutomatic = wanted
    }

    public func label(for action: KeyAction) -> String {
        switch action {
        case .character(let c): Self.isLoneMark(c) ? "\u{25CC}" + c : showsUppercase ? c.uppercased() : c
        case .space: language == .english ? "English" : "മലയാളം"
        case .backspace: "⌫"
        case .shift:
            switch shift {
            case .off: "⇧"
            case .once: "⬆︎"
            case .locked: "⇪"
            }
        case .returnKey: "return"
        case .nextKeyboard: "🌐"
        case .page(.letters): language == .english ? "ABC" : "കഖ"
        case .page(.symbols): "123"
        case .page(.moreSymbols): "#+="
        case .page(.malayalam): "കഖ"
        case .page(.malayalamMore): "ശഷ"
        case .page(.malayalamVowels): "അആ"
        case .toggleLanguage: language == .english ? "മ" : "EN"
        case .emoji: "☺︎" // Drawn as a symbol; this is the fallback.
        }
    }

    /// A single combining mark (e.g. a vowel sign) is drawn on a dotted circle, as is conventional.
    private static func isLoneMark(_ text: String) -> Bool {
        let scalars = text.unicodeScalars
        guard scalars.count == 1, let scalar = scalars.first else { return false }
        switch scalar.properties.generalCategory {
        case .nonspacingMark, .spacingMark: return true
        default: return false
        }
    }

    private var showsUppercase: Bool {
        page == .letters && shift != .off
    }

    private var alphabetPage: KeyboardPage {
        language == .english ? .letters : .malayalam
    }

    /// Replaces the typed word with the middle suggestion (`correction`), then types `separator`.
    private func commitMiddle(separator: String) -> Bool {
        let context = output.contextBefore ?? ""
        guard let (word, replacement) = correction(endingAt: context) else { return false }
        delete(word, from: context)
        output.insertText(replacement + separator)
        reportUse(replacement, before: String(context.dropLast(word.count)))
        lastReplacement = Replacement(typed: word, replacement: replacement, separator: separator)
        suggestions = .empty
        if separator != " " {
            // Punctuation moves shift on like any typed character; space doesn't.
            shift = shift.afterCharacter
            if shift == .off { shiftIsAutomatic = false }
        }
        return true
    }

    /// The word at the end of `context` and the middle suggestion autocorrect puts in for
    /// it, if any. English only: native Malayalam is never replaced.
    private func correction(endingAt context: String) -> (String, String)? {
        guard autocorrectEnabled, language == .english else { return nil }
        let word = String(self.word(endingAt: context))
        guard !word.isEmpty,
              Self.straight(suggestions.typed) == Self.straight(word),
              keptWord.map({ Self.straight($0) != Self.straight(word) }) ?? true,
              !Self.containsMalayalam(word),
              address(endingAt: context) == nil,
              !Self.looksLikeCodeOrAddress(before: context.dropLast(word.count)),
              let middle = suggestions.middle,
              middle.kind == .correction || middle.kind == .manglish,
              middle.text != word
        else { return nil }
        return (word, middle.text)
    }

    /// A full stop after a word ends it only if whitespace comes next: a letter makes one
    /// word of both (`googel.com`, `e.g`). So it is typed as it is, and what ending the
    /// word does (a replacement, or the report) waits for `end(_:with:)`.
    private func holdFullStop() {
        let context = output.contextBefore ?? ""
        let word = currentWord
        if !word.isEmpty {
            heldStop = HeldStop(word: word, replacement: correction(endingAt: context)?.1, reports: committableWord() != nil)
        }
        type(".")
    }

    /// Whitespace straight after a held full stop: the word before it ends there, so it
    /// is replaced (`teh.` → `the. `, undone like any replacement) or reported. False
    /// when nothing is held or the text no longer ends in it.
    private func end(_ stop: HeldStop?, with separator: String) -> Bool {
        let context = output.contextBefore ?? ""
        guard let stop, context.hasSuffix(stop.word + ".") else { return false }
        reportUse(stop.replacement ?? stop.word, before: String(context.dropLast(stop.word.count + 1)))
        if let replacement = stop.replacement {
            delete(stop.word + ".", from: context)
            output.insertText(replacement + "." + separator)
            lastReplacement = Replacement(typed: stop.word, replacement: replacement, kept: ".", separator: separator)
            suggestions = .empty
        } else {
            if stop.reports { report(.committed(stop.word)) }
            output.insertText(separator)
        }
        return true
    }

    private static func straight(_ text: String) -> String {
        text.replacingOccurrences(of: "\u{2019}", with: "'")
    }

    private static func containsMalayalam(_ text: String) -> Bool {
        text.unicodeScalars.contains { (0x0D00...0x0D7F).contains($0.value) }
    }

    /// URLs, emails, handles, hashtags, paths and versions: the character just
    /// before the word is one of `@ . / : _ # \` or a digit.
    private static func looksLikeCodeOrAddress(before prefix: Substring) -> Bool {
        guard let last = prefix.last else { return false }
        return "@./:_#\\".contains(last) || last.isNumber
    }

    /// An email or handle (it has an `@`), a URL (`http://`, `https://`, `www.`) or a
    /// dotted word (`isDotted`) with a letter or digit: one word up to the whitespace
    /// before it, never corrected. Matches the engine's `text::is_address`.
    private static func isAddress(_ token: Substring) -> Bool {
        guard token.contains(where: { $0.isLetter || $0.isNumber }) else { return false }
        let lower = token.lowercased()
        return token.contains("@") || ["http://", "https://", "www."].contains { lower.hasPrefix($0) }
            || isDotted(token)
    }

    /// `a.b.c`, `e.g.`, `googel.com`: a full stop with more typed after it than punctuation,
    /// brackets and quotes. Matches the engine's `text::is_dotted`.
    private static func isDotted(_ token: Substring) -> Bool {
        token.reversed().drop { ".,;:?!)]}>\"'\u{201D}\u{2019}".contains($0) }.contains(".")
    }

    /// The text after the last whitespace in `context`, less opening brackets and quotes.
    private static func token(endingAt context: String) -> Substring {
        let start = context.lastIndex(where: \.isWhitespace).map { context.index(after: $0) } ?? context.startIndex
        return context[start...].drop { "([{<\"'\u{201C}\u{2018}".contains($0) }
    }

    /// Whether typing `mark` keeps or makes an address (`priya@example` + `.`, `www` + `.`):
    /// then it is part of the word, not punctuation that ends it.
    private func continuesAddress(with mark: String) -> Bool {
        address(endingAt: (output.contextBefore ?? "") + mark) != nil
    }

    /// The address at the end of `context`: digits that begin a learned phone number
    /// (`learnedNumber`), a `token` that `isAddress`, or one of two characters or more that
    /// begins a learned address and runs past the plain word (`neo.9`). Matches the engine's
    /// `Engine::current_word`.
    private func address(endingAt context: String) -> Substring? {
        if let number = learnedNumber(endingAt: context) { return number }
        let token = Self.token(endingAt: context)
        if Self.isAddress(token) { return token }
        guard token.count >= 2, token != Self.plainWord(endingAt: context) else { return nil }
        let lower = token.lowercased()
        // A phone number only when it is dotted (`98.470.12345`), as in the engine.
        return learnedAddresses.contains { Self.isAddress(Substring($0)) && $0.lowercased().hasPrefix(lower) } ? token : nil
    }

    /// Digits typed before a learned phone number they begin is one word. Matches the
    /// engine's `NUMBER_PREFIX_DIGITS`.
    private static let numberPrefixDigits = 3
    /// Matches the engine's `text::MAX_NUMBER_GROUPS`.
    private static let maxNumberGroups = 4

    /// The longest end of `context` (`numberTails`) with `numberPrefixDigits` or more that
    /// begins a learned phone number (`+91 98` of +91 55501 12345). Matches the engine's
    /// `Engine::begun_number`. Of the learned addresses, the phone numbers are those all
    /// digits, spaces and `+-.()`: an email or URL learned has a letter.
    private func learnedNumber(endingAt context: String) -> Substring? {
        let numbers = learnedAddresses.filter { Self.isNumber($0) }
        guard !numbers.isEmpty else { return nil }
        return Self.numberTails(endingAt: context).first { tail in
            tail.filter { $0.isASCII && $0.isNumber }.count >= Self.numberPrefixDigits
                && numbers.contains { $0.hasPrefix(tail) }
        }
    }

    /// The ends of `context` that may be a phone number or its start, longest first: its last
    /// words of digits and `+-.()` back over single spaces, at most `maxNumberGroups`
    /// (`call +91 98` → `+91 98`, `98`). An opening quote before the first is no part of it.
    /// Matches the engine's `text::number_tails`.
    private static func numberTails(endingAt context: String) -> [Substring] {
        var tails: [Substring] = []
        var end = context.endIndex
        while tails.count < maxNumberGroups {
            let start = context[..<end].lastIndex(where: \.isWhitespace).map { context.index(after: $0) } ?? context.startIndex
            let token = context[start..<end]
            let word = token.drop { "[{<\"'\u{201C}\u{2018}".contains($0) }
            guard !word.contains(" "), isNumber(word) else { break }
            tails.append(context[word.startIndex...])
            // One space before it, and a word before that.
            guard word.startIndex == token.startIndex, start > context.startIndex else { break }
            let space = context.index(before: start)
            guard context[space] == " ", space > context.startIndex,
                  !context[context.index(before: space)].isWhitespace
            else { break }
            end = space
        }
        return tails.reversed()
    }

    /// Digits, spaces and `+-.()` only, with a digit: a phone number or part of one.
    private static func isNumber(_ text: some StringProtocol) -> Bool {
        text.contains { $0.isASCII && $0.isNumber } && text.allSatisfy { "0123456789+-.() ".contains($0) }
    }

    /// Learning events go to the engine, except from a password field.
    private func report(_ event: LearningEvent) {
        guard !secureTextEntry else { return }
        onLearningEvent?(event)
    }

    /// Backspace straight after a replacement puts back what was typed and learns it.
    private func undo(_ replacement: Replacement) -> Bool {
        let context = output.contextBefore ?? ""
        let committed = replacement.replacement + replacement.kept + replacement.separator
        guard context.hasSuffix(committed) else { return false }
        delete(committed, from: context)
        output.insertText(replacement.typed + replacement.kept)
        keptWord = replacement.typed
        suggestions = .empty
        report(.learn(replacement.typed))
        return true
    }

    /// Reports a word committed as typed, but only one autocorrect looked at
    /// and left alone: with autocorrect off, or before the word's result
    /// arrived, a typo (`thnaks` in a search field) would be learned. An address
    /// is never corrected, so it is reported anyway (an email field has no autocorrect).
    private func noteCommittedWord() {
        if let word = committableWord() { report(.committed(word)) }
        let context = output.contextBefore ?? ""
        let word = self.word(endingAt: context)
        if !word.isEmpty { reportUse(String(word), before: String(context[..<word.startIndex])) }
    }

    /// Reports a word that went into the text after `before`, except from a password field
    /// and except an address: an email or URL is no part of a phrase. A number is reported,
    /// dotted too (`98.470.12345`), for the engine to learn phone numbers from. Unlike
    /// `.committed`, with autocorrect off too: the engine counts only words it knows or learned.
    private func reportUse(_ word: String, before: String, chosenFor: String? = nil) {
        guard !secureTextEntry, !word.isEmpty, Self.isNumber(word) || !Self.isAddress(Substring(word)) else { return }
        let use = WordUse(word: word, before: String(before.suffix(WordUse.contextLength)), language: language, chosenFor: chosenFor)
        onWordUsed?(use)
    }

    /// The word before the cursor, if `noteCommittedWord` would report it.
    private func committableWord() -> String? {
        let word = currentWord
        guard !word.isEmpty,
              address(endingAt: output.contextBefore ?? "") != nil
                || (autocorrectEnabled && suggestions.isFor(word: word))
        else { return nil }
        return word
    }

    private func type(_ text: String) {
        if keptWord != nil, let first = text.first, Self.isWordCharacter(first), currentWord.isEmpty {
            keptWord = nil // A new word starts after the kept one.
        }
        output.insertText(showsUppercase ? text.uppercased() : text)
        shift = shift.afterCharacter
        if shift == .off { shiftIsAutomatic = false }
    }

    /// Deletes `suffix` from the end of `context`. iOS removes one code point per
    /// deleteBackward in Malayalam (കി → ക) but a whole cluster for emoji, so this
    /// stops as soon as the host's context shows the suffix is gone.
    private func delete(_ suffix: String, from context: String) {
        let scalars = suffix.unicodeScalars.count
        let target = context.unicodeScalars.count - scalars
        for _ in 0..<scalars {
            output.deleteBackward()
            if let now = output.contextBefore, now.unicodeScalars.count <= target { break }
        }
    }

    /// The word at the end of `context`, optionally with the spaces after it.
    private static func wordSuffix(of context: String, includingSpaces: Bool) -> Substring {
        var index = context.endIndex
        while includingSpaces, index > context.startIndex, context[context.index(before: index)].isWhitespace {
            index = context.index(before: index)
        }
        while index > context.startIndex, isWordCharacter(context[context.index(before: index)]) {
            index = context.index(before: index)
        }
        return context[index...]
    }

    /// The word being typed at the end of `context`: an address (`address(endingAt:)`)
    /// whole, otherwise the `plainWord`.
    private func word(endingAt context: String) -> Substring {
        address(endingAt: context) ?? Self.plainWord(endingAt: context)
    }

    /// The letters, digits and apostrophes at the end of `context`, without leading
    /// apostrophes (an opening quote), and "" when it has no letter or digit.
    /// Matches the engine's `text::current_word` for a word that is no address.
    private static func plainWord(endingAt context: String) -> Substring {
        let word = wordSuffix(of: context, includingSpaces: false).drop { isApostrophe($0) }
        return word.contains { $0.isLetter || $0.isNumber } ? word : word.suffix(0)
    }

    private static func isApostrophe(_ c: Character) -> Bool {
        c == "'" || c == "\u{2019}"
    }

    private static func isWordCharacter(_ c: Character) -> Bool {
        c.isLetter || c.isNumber || c == "'" || c == "\u{2019}"
    }

    private static func shouldCapitalize(_ context: String, mode: AutoCapitalization) -> Bool {
        switch mode {
        case .off:
            return false
        case .allCharacters:
            return true
        case .words:
            return context.isEmpty || context.last?.isWhitespace == true
        case .sentences:
            guard let last = context.last else { return true }
            if last.isNewline { return true }
            guard last.isWhitespace else { return false }
            guard let end = context.trimmingCharacters(in: .whitespaces).last else { return true }
            return end == "." || end == "?" || end == "!" || end.isNewline
        }
    }
}

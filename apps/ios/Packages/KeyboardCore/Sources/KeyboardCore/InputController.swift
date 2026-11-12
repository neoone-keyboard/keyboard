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
    /// Shows `text` as the host's marked (composing, underlined) text, in place of the marked
    /// text before, with the cursor after it. Empty removes it.
    func setMarkedText(_ text: String)
    /// Commits the marked text as it is.
    func unmarkText()
}

public enum AutoCapitalization: Sendable {
    case off, words, sentences, allCharacters
}

/// A language the keyboard types: English on the Latin keys, or a pack's language on its
/// native layout (`PackDescriptor.layout`).
public enum Language: Hashable, Sendable {
    case english
    case pack(PackDescriptor)

    /// The language id: "en", "ml".
    public var id: String {
        switch self {
        case .english: "en"
        case .pack(let pack): pack.id
        }
    }

    /// The native layout; nil for English.
    public var nativeLayout: PackLayout? {
        if case .pack(let pack) = self { pack.layout } else { nil }
    }

    /// Typed as a reading and converted as a phrase (Japanese, `PackDescriptor.conversion`).
    public var convertsPhrases: Bool {
        if case .pack(let pack) = self { pack.conversion } else { false }
    }
}

/// Something the engine should learn from what the user just did.
public enum LearningEvent: Equatable, Sendable {
    /// A word was committed as typed. Words in no lexicon are learned the second time.
    case committed(String)
    /// The user rejected a replacement: never replace this word again.
    case learn(String)
}

public enum SwipeOutcome: Equatable, Sendable {
    /// The word stayed: something else came next.
    case kept
    /// Swapped for another word from the bar.
    case replaced
    /// Deleted by the next backspace.
    case deleted
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
    /// With `chosenFor`, the language the tapped suggestion was offered as a word of
    /// (`Suggestion.language`): a word the engine doesn't have is learned as one of that language.
    public var chosenLanguage: String?

    /// Enough for the two words before.
    public static let contextLength = 64

    public init(word: String, before: String, language: Language, chosenFor: String? = nil, chosenLanguage: String? = nil) {
        self.word = word
        self.before = before
        self.language = language
        self.chosenFor = chosenFor
        self.chosenLanguage = chosenLanguage
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
    /// What the language key cycles through, in order (the engine's `layouts()`); never empty.
    /// Setting it keeps the current language, and its page, if a language with the same id is
    /// still listed (a pack read again is a new value with the same id); else shows the first.
    public var languages: [Language] = [.english] {
        didSet {
            if languages.isEmpty { languages = [.english] }
            if let same = languages.first(where: { $0.id == language.id }) {
                language = same
            } else {
                switchLanguage(to: languages[0])
            }
        }
    }
    /// The packs whose languages are on, whether or not they have a keyboard of their own
    /// (`languages` lists only those that do). A word in one of their scripts, or in the script
    /// of a pack among `languages`, is never replaced by autocorrect on the Latin keys.
    public var activePacks: [PackDescriptor] = []
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
    /// What became of each swipe: for the diagnostics' count of words fixed straight away.
    public var onSwipeOutcome: (@MainActor (SwipeOutcome) -> Void)?
    /// The bar shows the last swipe's word and its alternatives until the next key.
    public var showsSwipe: Bool { lastSwipe != nil }
    /// Whether space and punctuation may replace the word. The extension sets
    /// this from the user's setting and the host field's traits.
    public var autocorrectEnabled = true
    /// A password field: nothing typed is reported for learning or use.
    public var secureTextEntry = false
    /// The engine's learned emails, URLs and phone numbers. Letters that begin one are one
    /// word with their punctuation (`neo.9` of sam.2024@example.com), and digits that begin a
    /// number with its spaces (`+91 98` of +91 55501 12345), as in the engine. May be a little stale.
    public var learnedAddresses: [String] = []

    /// The composer and converter of the phrase-conversion language among `languages`
    /// (Japanese), once the engine has opened it; nil before, and then its keys type letters.
    /// A composition under way is committed when it changes.
    public var phraseConverter: PhraseConverter? {
        didSet {
            guard phraseConverter !== oldValue else { return }
            phrase?.commit()
            phrase = phraseConverter.map { PhraseInput(output: output, converter: $0) }
        }
    }
    private var phrase: PhraseInput?
    /// The composition while a phrase-conversion language is typed; nil for the others.
    public var composing: PhraseInput? { language.convertsPhrases ? phrase : nil }

    private let output: TextOutput
    private var lastShiftTap: TimeInterval?
    /// The automatic replacement made by the last key, which backspace can undo.
    private var lastReplacement: Replacement?
    /// A word the user restored with backspace; it is never replaced again.
    private var keptWord: String?

    /// A full stop after a word that whitespace has not yet followed (`holdFullStop`).
    private var heldStop: HeldStop?

    /// The last swipe, until the next key: backspace deletes it, a bar slot replaces it.
    /// Its word is reported only once it stays (`settleSwipe`).
    private var lastSwipe: Swipe?

    private struct Swipe {
        /// As it went in, with shift applied, and the space after it.
        let text: String
        let before: String
        let capitalisation: ShiftState
    }

    /// Typed straight after a swipe, these take the place of its space: `hello` + `,` is `hello,`.
    private static let closingPunctuation: Set<String> = [".", ",", "?", "!", ":", ";"]
    /// After these a swipe needs no space before it.
    private static let openingMarks: Set<Character> = ["(", "[", "{", "\"", "'", "\u{201C}", "\u{2018}"]

    private struct Replacement {
        let typed: String
        let replacement: String
        /// Typed after the word and put back with it by undo: the `.` of `teh.` + space.
        var kept = ""
        /// What undo puts back for `kept`: the `.` typed where Urdu's `۔` went in after the word.
        var keptTyped: String?
        let separator: String
    }

    /// The word before a full stop, with what ending it at whitespace does: put in
    /// `replacement`, or report the word when `reports`. `mark` is the full stop as typed:
    /// `.`, or a script's own (`۔`) after a word in that script.
    private struct HeldStop {
        let word: String
        var mark = "."
        let replacement: String?
        let reports: Bool
    }

    /// The text before the cursor just after the space key typed a space, until the next key:
    /// a second space then may end the sentence in the script's full stop (`spacesMark`).
    private var afterSpace: String?

    /// Typed straight after a word, these commit the middle suggestion first. A full stop
    /// waits for the next key: only whitespace after it ends the word (`googel.com`).
    private static let committingPunctuation: Set<String> = [".", ",", "?", "!"]
    private var shiftIsAutomatic = false
    /// Set when the user taps shift: auto-capitalisation leaves shift alone until
    /// the text before the cursor differs from `manualShiftContext`.
    private var manualShiftPending = false
    private var manualShiftContext: String?

    public init(output: TextOutput, languages: [Language] = [.english]) {
        self.output = output
        self.languages = languages.isEmpty ? [.english] : languages
        language = self.languages[0]
        page = alphabetPage
    }

    /// Which keys show: when this changes they must be drawn again (a shift change only
    /// relabels them). The page alone isn't enough: every pack's first page may be `letters`.
    public var keysShown: KeysShown {
        KeysShown(language: language.id, page: page, script: language == .english ? scriptPack?.id : nil)
    }

    public var layout: KeyboardLayout {
        .forPage(page, native: language.nativeLayout, numberRow: numberRow, showsNextKeyboard: showsNextKeyboard,
                 script: scriptPack?.layout)
    }

    /// The pack whose conventions the keys and the bar follow: on a pack's own layout that
    /// pack; on the Latin keys the one pack typed beside English (Roman Urdu with Urdu on),
    /// and none when several are on, as they may differ.
    public var scriptPack: PackDescriptor? {
        if case .pack(let pack) = language { return pack }
        // A conversion language (Japanese) isn't typed on the Latin keys' English: its marks
        // stay on its own layout.
        let typed = activePacks.filter { !$0.conversion }
        return typed.count == 1 ? typed[0] : nil
    }

    /// The suggestion bar is mirrored: `scriptPack` is written right to left. The keys never are.
    public var rightToLeft: Bool { scriptPack?.rightToLeft ?? false }

    /// The language the language key switches to: the next in `languages`.
    private var nextLanguage: Language {
        let index = languages.firstIndex { $0.id == language.id } ?? -1
        return languages[(index + 1) % languages.count]
    }

    private func switchLanguage(to next: Language) {
        phrase?.commit()
        suggestions = .empty
        language = next
        page = alphabetPage
        shift = .off
        shiftIsAutomatic = false
        manualShiftPending = false
    }

    /// `time` is monotonic seconds, e.g. `ProcessInfo.processInfo.systemUptime`.
    public func handle(_ action: KeyAction, at time: TimeInterval) {
        if let phrase = composing, handlePhrase(action, phrase) { return }
        if let swipe = takeSwipe() {
            if action == .backspace, deleteSwipe(swipe) { return }
            settleSwipe(swipe)
            if case .character(let c) = action, Self.closingPunctuation.contains(latinTwin(of: c)),
               (output.contextBefore ?? "").hasSuffix(swipe.text) {
                output.deleteBackward() // The swipe's space.
            }
        }
        let undoable = lastReplacement
        lastReplacement = nil
        let stop = heldStop
        heldStop = nil
        let spaced = afterSpace
        afterSpace = nil
        switch action {
        case .character(let c) where latinTwin(of: c) == "." && !continuesAddress(with: c):
            holdFullStop(c)
        case .character(let c) where Self.committingPunctuation.contains(latinTwin(of: c)) && !continuesAddress(with: c):
            if !commitMiddle(separator: c) {
                noteCommittedWord()
                type(mark(c, after: output.contextBefore ?? ""))
            }
        case .character(let c):
            type(c)
        case .space:
            if let spaced, spaced == output.contextBefore, let stop = spacesMark(endingAt: spaced) {
                // The second space: the sentence ends in the script's full stop (Urdu's ۔).
                output.deleteBackward()
                output.insertText(stop + " ")
            } else {
                if !end(stop, with: " "), !commitMiddle(separator: " ") {
                    noteCommittedWord()
                    output.insertText(" ")
                }
                afterSpace = output.contextBefore
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
            switchLanguage(to: nextLanguage)
        }
    }

    /// A key of a phrase-conversion language: characters compose, space converts and cycles,
    /// return commits, backspace edits the composition, each doing what it always does with
    /// nothing composing (no word is reported: the word engine doesn't know the language).
    /// Keys that leave the language commit the composition first. False for the keys the
    /// usual path handles.
    private func handlePhrase(_ action: KeyAction, _ phrase: PhraseInput) -> Bool {
        switch action {
        case .character(let c):
            phrase.type(c)
            shift = shift.afterCharacter
        case .space:
            if !phrase.space() { output.insertText(" ") }
            if page == .symbols || page == .moreSymbols { page = alphabetPage }
        case .returnKey:
            if !phrase.confirm() { output.insertText("\n") }
        case .backspace:
            if !phrase.backspace() { output.deleteBackward() }
        case .toggleLanguage, .nextKeyboard, .emoji:
            phrase.commit()
            return false
        case .shift, .page:
            return false
        }
        return true
    }

    /// Types a long-press alternate; follows shift like a normal letter.
    public func insertAlternate(_ text: String) {
        if let phrase = composing {
            phrase.type(text)
            return
        }
        settlePendingSwipe()
        lastReplacement = nil
        heldStop = nil
        type(text)
    }

    /// Types an emoji from the panel as it is, with no space. Never reported for learning.
    public func insertEmoji(_ emoji: String) {
        composing?.commit()
        settlePendingSwipe()
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
        composing?.commit()
        forgetPendingState()
        output.adjustTextPosition(byCharacterOffset: offset)
    }

    /// Deletes the word before the cursor plus the spaces after it, or one
    /// character when there is no word (e.g. punctuation).
    public func deleteWordBackward() {
        if let phrase = composing, phrase.isComposing {
            phrase.cancel()
            return
        }
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
        phrase?.reset()
        forgetPendingState()
    }

    private func forgetPendingState() {
        settlePendingSwipe()
        lastReplacement = nil
        heldStop = nil
        afterSpace = nil
        suggestions = .empty
    }

    /// The word before the cursor, as the host shares it ("" after a space).
    /// A leading quote isn't part of it (`'teh` → `teh`), as in the engine.
    public var currentWord: String {
        String(word(endingAt: output.contextBefore ?? ""))
    }

    /// A tap on a bar slot. Tapping the quoted typed word keeps it and learns it.
    public func commitSuggestion(_ slot: SuggestionSlot) {
        if let swipe = takeSwipe() {
            if replaceSwipe(swipe, with: slot.suggestion.text) { return }
            settleSwipe(swipe)
        }
        if slot.quoted { report(.learn(slot.suggestion.text)) }
        let chosenFor = choice(slot)
        insert(suggestion: slot.suggestion.text, reportsCommit: !slot.quoted, chosenFor: chosenFor,
               chosenLanguage: chosenFor == nil ? nil : slot.suggestion.language)
    }

    /// Replaces the word being typed with `text` and adds a space, as a tap on a suggestion does.
    public func commitSuggestion(_ text: String) {
        settlePendingSwipe()
        insert(suggestion: text, reportsCommit: true, chosenFor: nil)
    }

    // MARK: Swipes

    /// Types the best of the words a swipe could have drawn; every other one is the bar's,
    /// which shows as many as it does for a typed word. False when there is no word.
    @discardableResult
    public func insertSwipe(_ found: [Suggestion]) -> Bool {
        guard let best = found.first else { return false }
        insertSwipe(best, alternatives: Array(found.dropFirst()))
        return true
    }

    /// Types a swiped word: a space first unless one is there already, the word as shift
    /// has it, and a space after. The bar shows the word and `alternatives`.
    public func insertSwipe(_ word: Suggestion, alternatives: [Suggestion]) {
        settlePendingSwipe()
        lastReplacement = nil
        heldStop = nil
        let context = output.contextBefore ?? ""
        if let last = context.last, !last.isWhitespace, !Self.openingMarks.contains(last) {
            output.insertText(" ")
        }
        let before = output.contextBefore ?? context + " "
        let capitalisation = shift
        let text = Self.cased(word.text, capitalisation) + " "
        output.insertText(text)
        shift = shift.afterCharacter
        if shift == .off { shiftIsAutomatic = false }
        lastSwipe = Swipe(text: text, before: before, capitalisation: capitalisation)
        suggestions = SuggestionResult(typed: "", candidates: [word] + alternatives)
    }

    private func takeSwipe() -> Swipe? {
        defer { lastSwipe = nil }
        return lastSwipe
    }

    private func settlePendingSwipe() {
        if let swipe = takeSwipe() { settleSwipe(swipe) }
    }

    /// The swiped word stays: it is used and committed, as a tapped suggestion is.
    private func settleSwipe(_ swipe: Swipe) {
        reportSwiped(swipe)
        onSwipeOutcome?(.kept)
    }

    private func reportSwiped(_ swipe: Swipe) {
        let word = String(swipe.text.dropLast())
        report(.committed(word))
        reportUse(word, before: swipe.before)
    }

    private func deleteSwipe(_ swipe: Swipe) -> Bool {
        let context = output.contextBefore ?? ""
        guard context.hasSuffix(swipe.text) else { return false }
        delete(swipe.text, from: context)
        suggestions = .empty
        onSwipeOutcome?(.deleted)
        return true
    }

    private func replaceSwipe(_ swipe: Swipe, with word: String) -> Bool {
        let context = output.contextBefore ?? ""
        guard context.hasSuffix(swipe.text) else { return false }
        delete(swipe.text, from: context)
        let replacement = Swipe(text: Self.cased(word, swipe.capitalisation) + " ", before: swipe.before,
                                capitalisation: swipe.capitalisation)
        output.insertText(replacement.text)
        suggestions = .empty
        reportSwiped(replacement)
        onSwipeOutcome?(.replaced)
        return true
    }

    private static func cased(_ word: String, _ shift: ShiftState) -> String {
        switch shift {
        case .off: word
        case .once: word.prefix(1).uppercased() + word.dropFirst()
        case .locked: word.uppercased()
        }
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

    private func insert(suggestion text: String, reportsCommit: Bool, chosenFor: String?, chosenLanguage: String? = nil) {
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
        reportUse(text, before: String(context[..<word.startIndex]), chosenFor: chosenFor, chosenLanguage: chosenLanguage)
    }

    /// Turns shift on at the start of a sentence (or word) and off again when it
    /// no longer applies. Never overrides caps lock or a shift the user tapped,
    /// including tapping it off, until the text context changes.
    public func applyAutoCapitalization(_ mode: AutoCapitalization) {
        // Romaji has no capitals: the composer reads every letter as lower case.
        guard !language.convertsPhrases else { return }
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

    /// The text a tap on `action` would add before the cursor when that is all the tap does: a
    /// Latin letter (English and Manglish), shifted as `type` shifts it. Nil for anything else,
    /// whose effect depends on more than the key (punctuation, space, Malayalam marks).
    public func insertion(for action: KeyAction) -> String? {
        guard case .character(let c) = action, c.unicodeScalars.count == 1,
              let scalar = c.unicodeScalars.first, scalar.isASCII, scalar.properties.isAlphabetic else { return nil }
        return showsUppercase ? c.uppercased() : c
    }

    /// The text before the cursor once a space tap has done its work: the word corrected if
    /// the tap would correct it (after a held full stop too), then the space.
    public func contextAfterSpace() -> String {
        let context = output.contextBefore ?? ""
        if let stop = heldStop, context.hasSuffix(stop.word + stop.mark) {
            guard let replacement = stop.replacement else { return context + " " }
            return String(context.dropLast(stop.word.count + stop.mark.count)) + replacement
                + mark(stop.mark, after: replacement) + " "
        }
        if let spaced = afterSpace, spaced == output.contextBefore, let stop = spacesMark(endingAt: spaced) {
            return String(context.dropLast()) + stop + " "
        }
        guard let (word, replacement) = correction(endingAt: context) else { return context + " " }
        return String(context.dropLast(word.count)) + replacement + " "
    }

    public func label(for action: KeyAction) -> String {
        switch action {
        case .character(let c): Self.isLoneMark(c) ? "\u{25CC}" + c : showsUppercase ? c.uppercased() : c
        case .space where isPhraseComposing: PhraseLabels.nextCandidate
        case .space: language.nativeLayout?.spaceBar ?? "English"
        case .returnKey where isPhraseComposing: PhraseLabels.confirm
        case .backspace: "⌫"
        case .shift:
            switch shift {
            case .off: "⇧"
            case .once: "⬆︎"
            case .locked: "⇪"
            }
        case .returnKey: "return"
        case .nextKeyboard: "🌐"
        case .page(.letters): language.nativeLayout?.alphabet.label ?? "ABC"
        case .page(.symbols): "123"
        case .page(.moreSymbols): "#+="
        case .page(.native(let id)): language.nativeLayout?.page(id)?.label ?? id
        case .toggleLanguage: nextLanguage.nativeLayout?.languageKey ?? "EN"
        case .emoji: "☺︎" // Drawn as a symbol; this is the fallback.
        }
    }

    /// What VoiceOver says for a key: the letter as it would be typed, the state of shift, the
    /// page or language a key goes to. `returnTitle` is the return key's action ("search").
    /// As Android's `InputController.spokenLabel`; `SpokenKey.text` is that string.
    public func spokenKey(for action: KeyAction, returnTitle: String = "return") -> SpokenKey {
        switch action {
        case .character(let c): SpokenKey(Self.isLoneMark(c) ? c : label(for: action))
        case .space where isPhraseComposing: SpokenKey("next candidate")
        case .space: SpokenKey("space")
        case .returnKey where isPhraseComposing: SpokenKey("confirm")
        case .backspace: SpokenKey("delete")
        case .shift:
            switch shift {
            case .off: SpokenKey("shift")
            case .once: SpokenKey("shift", value: "on")
            case .locked: SpokenKey("caps lock", value: "on")
            }
        case .returnKey: SpokenKey(returnTitle)
        case .nextKeyboard: SpokenKey("next keyboard")
        case .page(.letters): SpokenKey("letters")
        case .page(.symbols): SpokenKey("numbers and symbols")
        case .page(.moreSymbols): SpokenKey("more symbols")
        case .page(.native(let id)): SpokenKey(language.nativeLayout?.page(id)?.label ?? id)
        case .toggleLanguage: SpokenKey("switch to \(Self.name(of: nextLanguage))")
        case .emoji: SpokenKey("emoji")
        }
    }

    /// `spokenKey(for:returnTitle:)` as one string: "shift, on".
    public func spokenLabel(for action: KeyAction, returnTitle: String = "return") -> String {
        spokenKey(for: action, returnTitle: returnTitle).text
    }

    private static func name(of language: Language) -> String {
        switch language {
        case .english: "English"
        case .pack(let pack): pack.name
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

    /// A phrase-conversion language has text composing: space and return label what they
    /// do to it.
    private var isPhraseComposing: Bool { composing?.isComposing ?? false }

    private var showsUppercase: Bool {
        page == .letters && shift != .off
    }

    private var alphabetPage: KeyboardPage {
        language.nativeLayout.map { .native($0.alphabet.id) } ?? .letters
    }

    /// Replaces the typed word with the middle suggestion (`correction`), then types `separator`.
    private func commitMiddle(separator: String) -> Bool {
        let context = output.contextBefore ?? ""
        guard let (word, replacement) = correction(endingAt: context) else { return false }
        // After a word in a script with its own marks, the script's: `kya?` → `کیا؟`.
        let separator = mark(separator, after: replacement)
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
    /// it, if any. English only: a word in a pack's own script is never replaced.
    private func correction(endingAt context: String) -> (String, String)? {
        guard autocorrectEnabled, language == .english else { return nil }
        let word = String(self.word(endingAt: context))
        guard !word.isEmpty,
              Self.straight(suggestions.typed) == Self.straight(word),
              keptWord.map({ Self.straight($0) != Self.straight(word) }) ?? true,
              !containsNativeScript(word),
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
    /// `typed` is the key's: `.`, or a script's own full stop (`۔`). A `.` after a word in a
    /// script with its own goes in as that (`شکریہ.` is `شکریہ۔`); after a Latin word that
    /// will be replaced, it waits for the replacement (`end(_:with:)`).
    private func holdFullStop(_ typed: String = ".") {
        let context = output.contextBefore ?? ""
        let word = currentWord
        let mark = mark(typed, after: context)
        if !word.isEmpty {
            heldStop = HeldStop(word: word, mark: mark, replacement: correction(endingAt: context)?.1,
                                reports: committableWord() != nil)
        }
        type(mark)
    }

    /// Whitespace straight after a held full stop: the word before it ends there, so it
    /// is replaced (`teh.` → `the. `, undone like any replacement) or reported. False
    /// when nothing is held or the text no longer ends in it. A replacement in a script with
    /// its own full stop takes it (`mera.` → `میرا۔ `).
    private func end(_ stop: HeldStop?, with separator: String) -> Bool {
        let context = output.contextBefore ?? ""
        guard let stop, context.hasSuffix(stop.word + stop.mark) else { return false }
        let typed = stop.word + stop.mark
        reportUse(stop.replacement ?? stop.word, before: String(context.dropLast(typed.count)))
        if let replacement = stop.replacement {
            let mark = mark(stop.mark, after: replacement)
            delete(typed, from: context)
            output.insertText(replacement + mark + separator)
            lastReplacement = Replacement(typed: stop.word, replacement: replacement, kept: mark,
                                          keptTyped: stop.mark, separator: separator)
            suggestions = .empty
        } else {
            if stop.reports { report(.committed(stop.word)) }
            output.insertText(separator)
        }
        return true
    }

    /// The packs whose scripts the text may be in: the active ones and those with a layout.
    private var writingPacks: [PackDescriptor] {
        activePacks + languages.compactMap { language in
            if case .pack(let pack) = language { pack } else { nil }
        }
    }

    /// The script's own mark for the Latin `mark` when `text` ends, past any whitespace, in a
    /// letter of a pack whose script has one (`PackDescriptor.punctuation`); else `mark`.
    private func mark(_ mark: String, after text: some StringProtocol) -> String {
        guard let last = text.last(where: { !$0.isWhitespace }), last.isLetter else { return mark }
        let letter = String(last)
        return writingPacks.lazy.compactMap { pack in pack.writes(letter) ? pack.punctuation[mark] : nil }.first ?? mark
    }

    /// The Latin mark a script's own one stands for (`۔` is `.`), so it ends a word as that
    /// does; any other text as it is.
    private func latinTwin(of text: String) -> String {
        for pack in writingPacks {
            if let latin = pack.punctuation.first(where: { $0.value == text })?.key { return latin }
        }
        return text
    }

    /// What a second space after `context` (which ends in the first) puts in its place: the
    /// script's full stop after a word in a script with its own (`میرا ` → `میرا۔ `); nil
    /// elsewhere, where two spaces stay two (iOS's own shortcut, if on, still applies there).
    private func spacesMark(endingAt context: String) -> String? {
        guard context.hasSuffix(" ") else { return nil }
        let before = context.dropLast()
        guard let last = before.last, !last.isWhitespace else { return nil }
        let stop = mark(".", after: before)
        return stop == "." ? nil : stop
    }

    private static func straight(_ text: String) -> String {
        text.replacingOccurrences(of: "\u{2019}", with: "'")
    }

    /// Whether `text` has a character of an active pack's script (`PackDescriptor.writes`).
    private func containsNativeScript(_ text: String) -> Bool {
        activePacks.contains { $0.writes(text) } || languages.contains { language in
            if case .pack(let pack) = language { pack.writes(text) } else { false }
        }
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
        output.insertText(replacement.typed + (replacement.keptTyped ?? replacement.kept))
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
    private func reportUse(_ word: String, before: String, chosenFor: String? = nil, chosenLanguage: String? = nil) {
        guard !secureTextEntry, !word.isEmpty, Self.isNumber(word) || !Self.isAddress(Substring(word)) else { return }
        let use = WordUse(word: word, before: String(before.suffix(WordUse.contextLength)), language: language,
                          chosenFor: chosenFor, chosenLanguage: chosenLanguage)
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

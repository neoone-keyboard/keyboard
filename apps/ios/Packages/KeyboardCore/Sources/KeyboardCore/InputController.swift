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
    /// The latest suggestions; space and punctuation commit their middle slot.
    public private(set) var suggestions: SuggestionResult = .empty

    private let output: TextOutput
    private var lastShiftTap: TimeInterval?
    /// The automatic replacement made by the last key, which backspace can undo.
    private var lastReplacement: Replacement?
    /// A word the user restored with backspace; it is never replaced again.
    private var keptWord: String?

    private struct Replacement {
        let typed: String
        let replacement: String
        let separator: String
    }

    /// Typed straight after a word, these commit the middle suggestion first.
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
        switch action {
        case .character(let c) where Self.committingPunctuation.contains(c):
            if !commitMiddle(separator: c) {
                noteCommittedWord()
                type(c)
            }
        case .character(let c):
            type(c)
        case .space:
            if !commitMiddle(separator: " ") {
                noteCommittedWord()
                output.insertText(" ")
            }
            if page == .symbols || page == .moreSymbols { page = alphabetPage }
        case .returnKey:
            noteCommittedWord()
            output.insertText("\n")
        case .backspace:
            if let undoable, undo(undoable) { break }
            output.deleteBackward()
        case .shift:
            let interval = lastShiftTap.map { time - $0 }
            lastShiftTap = time
            shift = shift.tapped(sinceLastTap: interval)
            shiftIsAutomatic = false
            manualShiftPending = true
            manualShiftContext = output.contextBefore
        case .nextKeyboard:
            break // The host view controller switches keyboards.
        case .page(let target):
            page = target == .letters ? alphabetPage : target
        case .toggleLanguage:
            language = language == .english ? .malayalam : .english
            page = alphabetPage
            shift = .off
            shiftIsAutomatic = false
            manualShiftPending = false
        }
    }

    /// Types a long-press alternate; follows shift like a normal letter.
    public func insertAlternate(_ text: String) {
        type(text)
    }

    public func moveCursor(by offset: Int) {
        output.adjustTextPosition(byCharacterOffset: offset)
    }

    /// Deletes the word before the cursor plus the spaces after it, or one
    /// character when there is no word (e.g. punctuation).
    public func deleteWordBackward() {
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

    /// The word before the cursor, as the host shares it ("" after a space).
    public var currentWord: String {
        String(Self.wordSuffix(of: output.contextBefore ?? "", includingSpaces: false))
    }

    /// A tap on a bar slot. Tapping the quoted typed word keeps it and learns it.
    public func commitSuggestion(_ slot: SuggestionSlot) {
        if slot.quoted { onLearningEvent?(.learn(slot.suggestion.text)) }
        commitSuggestion(slot.suggestion.text)
    }

    /// Replaces the word being typed with `text` and adds a space, as a tap on a suggestion does.
    public func commitSuggestion(_ text: String) {
        lastReplacement = nil
        suggestions = .empty
        let context = output.contextBefore ?? ""
        let word = Self.wordSuffix(of: context, includingSpaces: false)
        if !word.isEmpty { delete(String(word), from: context) }
        output.insertText(text + " ")
        shift = shift.afterCharacter
        if shift == .off { shiftIsAutomatic = false }
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

    /// English only: replaces the typed word with the middle suggestion, then
    /// types `separator`. Native Malayalam is never replaced.
    private func commitMiddle(separator: String) -> Bool {
        guard language == .english else { return false }
        let context = output.contextBefore ?? ""
        let word = String(Self.wordSuffix(of: context, includingSpaces: false))
        guard !word.isEmpty, word != keptWord, suggestions.typed == word,
              let middle = suggestions.middle, middle.kind != .verbatim, middle.text != word
        else { return false }
        delete(word, from: context)
        output.insertText(middle.text + separator)
        lastReplacement = Replacement(typed: word, replacement: middle.text, separator: separator)
        suggestions = .empty
        shift = shift.afterCharacter
        if shift == .off { shiftIsAutomatic = false }
        return true
    }

    /// Backspace straight after a replacement puts back what was typed and learns it.
    private func undo(_ replacement: Replacement) -> Bool {
        let context = output.contextBefore ?? ""
        let committed = replacement.replacement + replacement.separator
        guard context.hasSuffix(committed) else { return false }
        delete(committed, from: context)
        output.insertText(replacement.typed)
        keptWord = replacement.typed
        suggestions = .empty
        onLearningEvent?(.learn(replacement.typed))
        return true
    }

    private func noteCommittedWord() {
        let word = currentWord
        if !word.isEmpty { onLearningEvent?(.committed(word)) }
    }

    private func type(_ text: String) {
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

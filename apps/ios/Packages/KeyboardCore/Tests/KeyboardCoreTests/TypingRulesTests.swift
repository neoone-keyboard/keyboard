import Testing
import KeyboardCore

/// The small decisions the view controller makes from the host field, the settings and the
/// clock: autocorrect, the bar's emoji, what a request and a swipe are given, and the
/// engine's cutoffs.

@Test func autocorrectNeedsTheSettingAndAFieldThatAllowsIt() {
    #expect(FieldRules.autocorrects(setting: true, field: .text, fieldAllows: true))
    #expect(!FieldRules.autocorrects(setting: false, field: .text, fieldAllows: true))
    #expect(!FieldRules.autocorrects(setting: true, field: .text, fieldAllows: false), "the host said no")
    for field in [FieldKind.url, .email, .webSearch] {
        #expect(!FieldRules.autocorrects(setting: true, field: field, fieldAllows: true), "\(field)")
    }
}

@Test func theBarsEmojiNeedsTheSettingAndIsNeverForPasswordsAddressesOrURLs() {
    #expect(FieldRules.offersEmoji(setting: true, field: .text, secure: false))
    #expect(FieldRules.offersEmoji(setting: true, field: .webSearch, secure: false), "a search may have an emoji")
    #expect(!FieldRules.offersEmoji(setting: false, field: .text, secure: false))
    #expect(!FieldRules.offersEmoji(setting: true, field: .text, secure: true))
    #expect(!FieldRules.offersEmoji(setting: true, field: .url, secure: false))
    #expect(!FieldRules.offersEmoji(setting: true, field: .email, secure: false))
}

@Test func aRequestCarriesTheEndOfTheTextTheLanguageAndAutocorrect() {
    let long = String(repeating: "ab ", count: 40) + "wor"
    let request = SuggestionRequest(textBefore: long, language: malayalam, autocorrect: false)
    #expect(request.contextBefore.count == 64)
    #expect(long.hasSuffix(request.contextBefore))
    #expect(request.language == malayalam)
    #expect(!request.autocorrect)
    #expect(SuggestionRequest(textBefore: "hi", language: .english, autocorrect: true)
        == SuggestionRequest(contextBefore: "hi", language: .english, autocorrect: true))
    // A key held down asks ahead for the text its key-up will leave: the same request.
    #expect(SuggestionRequest(textBefore: "hel" + "l", language: .english, autocorrect: true)
        == SuggestionRequest(textBefore: "hell", language: .english, autocorrect: true))
}

@Test func swipesAreDecodedOnTheEnglishKeysWithPredictionOnOutsidePasswords() {
    #expect(SwipeRules.decodes(predictionOn: true, language: .english, secure: false))
    #expect(!SwipeRules.decodes(predictionOn: false, language: .english, secure: false), "memory is short")
    #expect(!SwipeRules.decodes(predictionOn: true, language: malayalam, secure: false))
    #expect(!SwipeRules.decodes(predictionOn: true, language: .english, secure: true))
}

@Test func aSwipesContextEndsWithTheSpaceItsWordGoesInAfter() {
    #expect(SwipeRules.context(textBefore: "") == "")
    #expect(SwipeRules.context(textBefore: "I said") == "I said ")
    #expect(SwipeRules.context(textBefore: "I said ") == "I said ")
    #expect(SwipeRules.context(textBefore: "line\n") == "line\n")
    let long = String(repeating: "word ", count: 30) + "end"
    let context = SwipeRules.context(textBefore: long)
    #expect(context.count == 65, "the last 64 characters and the space")
    #expect(context.hasSuffix("word end "))
}

/// Every word the engine found goes on (the bar shows as many as the setting says); none
/// when it found none or took too long, and the bar then says "Swipe again".
@Test func aSwipesWordsAreAllKeptUnlessTheyComeLate() {
    let found = ["hello", "help", "held", "hells", "hero", "helm"].map { Suggestion($0, kind: .completion) }
    #expect(SwipeRules.accepted(found, after: .milliseconds(20)) == found)
    #expect(SwipeRules.accepted(found, after: SwipeRules.timeout) == found)
    #expect(SwipeRules.accepted(found, after: SwipeRules.timeout + .milliseconds(1)) == nil)
    #expect(SwipeRules.accepted([], after: .milliseconds(20)) == nil)
    #expect(SwipeRules.accepted(nil, after: .milliseconds(20)) == nil)
    #expect(SwipeRules.timeout == .milliseconds(250))
}

/// The engine stops its own work before the bar stops waiting for it: a suggest that ran to
/// a cutoff still arrives in time to be shown.
@Test func theEnginesCutoffsEndBeforeTheBarStopsWaiting() {
    let deadlines = EngineDeadlines.keyboard
    #expect(SuggestionCoordinator.defaultTimeout == .milliseconds(15))
    for cutoff in [deadlines.model, deadlines.lmPrefill, deadlines.lmAfterSpace, deadlines.lm, deadlines.lmPrepare] {
        #expect(.milliseconds(Int(cutoff)) < SuggestionCoordinator.defaultTimeout)
    }
    #expect(deadlines.lmPrefill < deadlines.model, "the LM's prefill leaves the transliterator most of its time")
    #expect(deadlines.model < deadlines.lm)
    #expect(deadlines.lmAfterSpace <= deadlines.lm)
    // As measured on a phone (see `EngineDeadlines`).
    #expect(deadlines == EngineDeadlines(model: 8, lmPrefill: 3, lmAfterSpace: 10, lm: 11, lmPrepare: 6))
}

/// The engine is asked for as many suggestions as the bar holds, and the bar never shows
/// more than the setting, which is one of the counts the panel offers.
@Test func theSettingsCountIsWhatTheBarHolds() {
    #expect(KeyboardSettings().suggestionCount == 10)
    #expect(KeyboardSettings.suggestionCounts == [3, 10, 20])
    let twenty = SuggestionResult(typed: "a", candidates: (0..<20).map { Suggestion("w\($0)", kind: .completion) })
    for count in KeyboardSettings.suggestionCounts {
        var state = BarState()
        state.word = "a"
        state.latest = twenty
        state.limit = count
        guard case .suggestions(let bar, _) = state.content(markBase: { _ in nil }) else { continue }
        #expect(bar.slots.compactMap { $0 }.count + bar.more.count == count)
    }
}

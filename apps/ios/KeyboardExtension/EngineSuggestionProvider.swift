import Foundation
import KeyboardCore
import MLEngine
import os

/// Runs the Rust engine off the main thread. The engine is created on first use
/// so the keys appear before anything else loads (spec §4 rule 2). Opening it
/// reads and verifies every lexicon file, so that only ever happens here, on
/// the actor. Callers reach it only through `Task { await … }`, so the main
/// thread never waits.
actor EngineSuggestionProvider: SuggestionProvider {
    private let log = Diagnostics(category: "engine")
    private var config: EngineConfig
    private var engine: Engine?
    /// Set while memory is short: the engine stays unloaded until `resume`.
    private var shed = false
    /// When opening last failed; it is tried again after `retryDelay`.
    private var failedAt: ContinuousClock.Instant?
    private static let retryDelay: Duration = .seconds(30)
    /// The last reason a request was refused, so each refusal is logged once, not per key.
    private var refusalLogged: String?
    private var loadedSources: [String] = []
    /// The last `learnedAddresses` read from the engine.
    private var addresses: [String] = []
    /// A result worked out while the finger was still on the key (`prefetch`), for the request
    /// its key-up will make. Dropped by anything that changes what the engine would suggest.
    private var ahead: (request: SuggestionRequest, result: SuggestionResult)?
    /// Of the requests a prefetch was made for, how many found it ready; logged every
    /// `aheadLogEvery` prefetches.
    private var aheadMade = 0
    private var aheadUsed = 0
    private static let aheadLogEvery = 200
    /// Swipe timings and what became of each swipe, logged every `swipeLogEvery` swipes:
    /// counts and milliseconds only, never a word.
    private var swipeTimes: [Duration] = []
    private var swipesFailed = 0
    private var swipesReplaced = 0
    private var swipesDeleted = 0
    private static let swipeLogEvery = 100
    /// Past this a swipe is logged as slow.
    private static let slowSwipe: Duration = .milliseconds(25)

    init(config: EngineConfig) {
        self.config = config
    }

    deinit {
        Diagnostics(category: "engine").info("Provider released; footprint \(Diagnostics.footprint())")
    }

    /// One per extension process. iOS keeps the process alive between keyboard sessions and
    /// makes a new view controller for each; an engine per controller left each old one's
    /// memory behind (about 2–4 MB a session, until the memory gate turned prediction off).
    static let shared = bundled()

    /// The language packs and the active choice, as last read (`readPackState`). Read when the
    /// process starts and again each time the keyboard appears (`reloadIfChanged`): iOS keeps
    /// the process alive across sessions and switching keyboards, so a choice made or a pack
    /// installed in the app meanwhile would otherwise wait for the process to end.
    struct PackState: Equatable, Sendable {
        /// Those in the extension bundle (`packs/`) and those the app installed in the App
        /// Group (`PackLocations`, phase 3), one per id: an installed pack only replaces a
        /// bundled one with a higher version. A pack the engine then can't load is left out
        /// with a reported problem, and the keys follow the engine's layouts, so the keyboard
        /// carries on with English and the packs that did load.
        var packs: [PackDescriptor]
        /// The active languages set when the engine opens: the user's latest choice, the newer
        /// of the app's (App Group `active-languages.json`, dated by its modification date) and
        /// the keyboard's own (made in its settings panel, `KeyboardLanguageChoice`), its ids
        /// the keyboard has no pack for left out (`DatedLanguages.newer`); else a debug build's Info.plist `MLActiveLanguages` ("ml,hi", from
        /// `make twopacks`); else nil, the engine's default.
        var active: [String]?
    }

    static func readPackState() -> PackState {
        let packs = PackDescriptor.merge(
            bundled: Bundle.main.resourceURL
                .map { PackDescriptor.scan($0.appending(path: "packs", directoryHint: .isDirectory)) } ?? [],
            installed: PackLocations.installedPacks().map(PackDescriptor.scan) ?? []
        )
        var active: [String]?
        let fromApp = PackLocations.activeLanguagesFile().flatMap(DatedLanguages.read(from:))
        let fromKeyboard = KeyboardLanguageChoice.load(from: .standard)
        if let stored = DatedLanguages.newer(app: fromApp, keyboard: fromKeyboard),
           let ids = stored.usable(packIDs: packs.map(\.id)) {
            active = ids
        } else {
            let ids = (Bundle.main.object(forInfoDictionaryKey: "MLActiveLanguages") as? String ?? "")
                .split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
            active = ids.isEmpty ? nil : ids
        }
        return PackState(packs: packs, active: active)
    }

    private static let state = OSAllocatedUnfairLock(initialState: readPackState())

    static var packs: [PackDescriptor] { state.withLock { $0.packs } }
    static var configuredActive: [String]? { state.withLock { $0.active } }

    /// The active languages as the settings panel shows them: the configured list, else the
    /// engine's default.
    static var activeChoice: ActiveLanguages {
        configuredActive.map(ActiveLanguages.init) ?? ActiveLanguages.engineDefault(packIDs: packs.map(\.id))
    }

    /// The languages the settings panel offers: English, then every pack, by native name.
    static var availableLanguages: [(id: String, name: String)] {
        [(ActiveLanguages.english, "English")] + packs.map { ($0.id, $0.nativeName) }
    }

    /// The languages the language key cycles before the engine has said (`layouts`): English,
    /// then the first pack with a keyboard, as the engine's default active list has it (or the
    /// configured list's packs with a keyboard).
    static var defaultLanguages: [Language] {
        guard let active = configuredActive else {
            return [.english] + packs.prefix(1).filter { $0.layout != nil }.map(Language.pack)
        }
        return [.english] + active.compactMap { id in packs.first { $0.id == id && $0.layout != nil } }.map(Language.pack)
    }

    /// The keyboard's languages for the engine's layouts, in order: Latin is English, a
    /// native layout its pack's language. A pack the keyboard can't draw is left out.
    static func languages(for layouts: [InputLayout]) -> [Language] {
        layouts.compactMap { layout in
            switch layout {
            case .latin: return .english
            case .native(let id): return packs.first { $0.id == id && $0.layout != nil }.map(Language.pack)
            }
        }
    }

    /// What the engine is told the text was typed on.
    static func inputLayout(_ language: Language) -> InputLayout {
        switch language {
        case .english: .latin
        case .pack(let pack): .native(language: pack.id)
        }
    }

    /// English from the extension bundle, with its own next words (`en-next.fst`, used only
    /// while no active pack has a `next.fst`), and every pack (`packs`), whose manifests name
    /// their lexicons and models; learned words in the extension's own container. A file
    /// missing from a pack disables only that source (without the transliterator, Manglish
    /// uses the lexicons alone; without the LM, ranking is M3a's).
    static func bundled() -> EngineSuggestionProvider {
        EngineSuggestionProvider(config: config(packs: packs))
    }

    private static func config(packs: [PackDescriptor]) -> EngineConfig {
        func lexicon(_ name: String) -> String? {
            Bundle.main.url(forResource: name, withExtension: "fst", subdirectory: "Lexicon")?.path
        }
        let userDir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?
            .appending(path: "LearnedWords", directoryHint: .isDirectory).path
        return EngineConfig(
            english: lexicon("en"),
            englishNext: lexicon("en-next"),
            userDir: userDir,
            packDirs: packs.map(\.folder.path),
            modelDeadlineMs: modelDeadlineMs,
            lmPrefillMs: lmPrefillMs,
            lmAfterSpaceMs: lmAfterSpaceMs,
            lmDeadlineMs: lmDeadlineMs,
            lmPrepareMs: lmPrepareMs,
            maxSuggestions: UInt32(KeyboardSettings.load(from: .standard).suggestionCount)
        )
    }

    /// Milliseconds into a suggest after which the transliterator stops and its finished
    /// spellings are used (`EngineConfig.modelDeadlineMs`). Typing
    /// leaves the CPU idle between keys, and a suggest that starts from idle runs 5–7× slower
    /// than the same one back to back (iPhone 15 Pro Max, debug app's `-suggestCheck`): a
    /// 4 ms suggest took 20–25 ms, past the coordinator's 15 ms, and the bar went empty. With
    /// 8 ms, the slowest still end by about 11 ms.
    static let modelDeadlineMs: UInt32 = 8

    /// LM cutoffs, in milliseconds from the start of a suggest (design §4). Prefill while typing
    /// stops at 3 ms, so the transliterator keeps most of its 8 ms. After a space nothing else
    /// runs, so the LM may use 10 ms. Tier-1 rows stop at 11 ms, near where M3a's slowest
    /// suggests already end, under the coordinator's 15 ms.
    static let lmPrefillMs: UInt32 = 3
    static let lmAfterSpaceMs: UInt32 = 10
    static let lmDeadlineMs: UInt32 = 11
    /// When the keyboard appears, before any key: the context is filled for up to 6 ms.
    static let lmPrepareMs: UInt32 = 6

    func suggestions(for request: SuggestionRequest) async throws -> SuggestionResult {
        try Task.checkCancellation()
        if let ahead, ahead.request == request {
            self.ahead = nil
            aheadUsed += 1
            return ahead.result
        }
        return try suggest(request)
    }

    /// Works out the suggestions for `request` before it is made: a key is down, and this is
    /// the text its key-up would leave. A suggest that starts from idle runs 5–7× slower than
    /// one straight after another (see `modelDeadlineMs`); the finger stays down for some
    /// 80–100 ms, so the result is usually ready when the key-up asks. The same request, so
    /// the same suggestions, only sooner. Best effort: a failure just leaves nothing ahead.
    func prefetch(_ request: SuggestionRequest) {
        guard ahead?.request != request else { return }
        ahead = nil
        guard let result = try? suggest(request) else { return }
        ahead = (request, result)
        aheadMade += 1
        if aheadMade == Self.aheadLogEvery {
            log.info("Suggestions ahead: \(aheadUsed) of \(aheadMade) used at key-up")
            aheadMade = 0
            aheadUsed = 0
        }
    }

    private func suggest(_ request: SuggestionRequest) throws -> SuggestionResult {
        let engine = try loadedEngine()
        let start = ContinuousClock.now
        let result = try engine.suggest(
            contextBefore: request.contextBefore,
            layout: Self.inputLayout(request.language),
            autocorrect: request.autocorrect
        )
        let elapsed = ContinuousClock.now - start
        if elapsed > .milliseconds(10) {
            log.warning("Slow suggest: \(Diagnostics.milliseconds(elapsed)) for \(result.typed.count) chars, \(result.candidates.count) candidates, \(request.language.id)")
        }
        return SuggestionResult(
            typed: result.typed,
            candidates: result.candidates.map { Suggestion($0.text, kind: Self.kind($0.kind)) },
            vowelSigns: result.vowelSigns
        )
    }

    /// The words a swipe along `path` (the keyboard view's points) may have meant, best first;
    /// nil when the engine can't be used or found none. `keys` are the letter keys' frames
    /// in the same points.
    func swipe(_ path: [SwipePoint], keys: [SwipeKey], contextBefore: String) -> [Suggestion]? {
        ahead = nil
        guard let engine = try? loadedEngine() else {
            noteSwipe(nil)
            return nil
        }
        let start = ContinuousClock.now
        let candidates = (try? engine.swipe(path: path, keys: keys, contextBefore: contextBefore)) ?? []
        let elapsed = ContinuousClock.now - start
        if elapsed > Self.slowSwipe {
            log.warning("Slow swipe: \(Diagnostics.milliseconds(elapsed)) for \(path.count) points, \(candidates.count) candidates")
        }
        noteSwipe(candidates.isEmpty ? nil : elapsed)
        guard !candidates.isEmpty else { return nil }
        return candidates.map { Suggestion($0.text, kind: Self.kind($0.kind)) }
    }

    func noteSwipeOutcome(_ outcome: SwipeOutcome) {
        switch outcome {
        case .kept: break
        case .replaced: swipesReplaced += 1
        case .deleted: swipesDeleted += 1
        }
    }

    /// `elapsed` is nil for a swipe that gave no word.
    private func noteSwipe(_ elapsed: Duration?) {
        if let elapsed { swipeTimes.append(elapsed) } else { swipesFailed += 1 }
        let count = swipeTimes.count + swipesFailed
        guard count >= Self.swipeLogEvery else { return }
        let sorted = swipeTimes.sorted()
        func percentile(_ p: Double) -> String {
            sorted.isEmpty ? "-" : Diagnostics.milliseconds(sorted[min(sorted.count - 1, Int(Double(sorted.count) * p))])
        }
        log.info("Swipes: \(count), \(swipesFailed) without a word, \(swipesReplaced) replaced from the bar, \(swipesDeleted) deleted straight away; p50 \(percentile(0.5)), p90 \(percentile(0.9))")
        swipeTimes = []
        swipesFailed = 0
        swipesReplaced = 0
        swipesDeleted = 0
    }

    /// The layouts the language key cycles (`Engine.layouts`), once the engine is open; nil
    /// before, or when it can't say.
    func layouts() -> [InputLayout]? {
        try? engine?.layouts()
    }

    /// Opens the lexicons ahead of the first request, once the keys are showing, and fills the
    /// LM's context with the text already in the field (`lmPrepareMs`).
    func warmUp(contextBefore: String) {
        guard let engine = try? loadedEngine() else { return }
        try? engine.prepare(contextBefore: contextBefore)
    }

    /// Returns once the work queued on the actor before it is done.
    func idle() {}

    /// How many suggestions the engine returns (`KeyboardSettings.suggestionCount`): set on the
    /// open engine, which is cheap, and kept for when it next opens. Nothing is reopened.
    func setMaxSuggestions(_ count: Int) {
        let count = UInt32(clamping: count)
        guard config.maxSuggestions != count else { return }
        ahead = nil
        config.maxSuggestions = count
        try? engine?.setMaxSuggestions(n: count)
        log.info("Suggestions per request now \(count)")
    }

    /// Best effort: a failure only means the word isn't learned.
    func handle(_ event: LearningEvent) {
        ahead = nil
        switch event {
        case .committed(let word): try? loadedEngine().noteCommitted(word: word)
        case .learn(let word): try? loadedEngine().learn(word: word)
        }
    }

    /// A word went into the text: its phrase, its use, the language habit, and a choice of
    /// suggestion. Best effort, like `handle`.
    func noteUse(_ use: WordUse) {
        ahead = nil
        guard let engine = try? loadedEngine() else { return }
        try? engine.noteUsed(contextBefore: use.before, word: use.word, layout: Self.inputLayout(use.language))
        if let typed = use.chosenFor {
            try? engine.noteChoice(typed: typed, chosen: use.word)
        }
    }

    /// Must work even when the engine is shed or failed to open: an engine with
    /// no lexicons opens only the learned-word store, which is cheap.
    func forgetLearned() {
        ahead = nil
        addresses = []
        if let engine {
            try? engine.forgetLearned()
        } else {
            try? Engine(config: EngineConfig(userDir: config.userDir)).forgetLearned()
        }
    }

    /// The user removed `word` from the bar. Best effort, like `handle`; works while the
    /// engine is shed, as `forgetLearned` does, so the choice is never silently lost.
    func remove(_ word: String) {
        ahead = nil
        if let engine {
            try? engine.removeWord(word: word)
        } else {
            try? Engine(config: EngineConfig(userDir: config.userDir)).removeWord(word: word)
        }
    }

    /// The learned and removed words for "Your words", read when the screen opens. Works
    /// while the engine is shed, as `forgetLearned` does; the caller drops the lists on close.
    func yourWords() -> YourWords {
        let engine = engine ?? (try? Engine(config: EngineConfig(userDir: config.userDir)))
        let learned = (try? engine?.learnedWords()) ?? []
        let removed = (try? engine?.removedWords()) ?? []
        return YourWords(learned: learned, removed: removed)
    }

    /// Forgets one learned word from "Your words", without blocking it. Best effort, and
    /// works while the engine is shed, like `remove`.
    func forget(_ word: String) {
        ahead = nil
        // The last list read stands in while the engine is shed; it mustn't keep the word.
        addresses.removeAll { $0 == word }
        if let engine {
            try? engine.forgetWord(word: word)
        } else {
            try? Engine(config: EngineConfig(userDir: config.userDir)).forgetWord(word: word)
        }
    }

    /// Un-removes a word from "Your words", so it may be suggested again. Like `forget`.
    func restore(_ word: String) {
        ahead = nil
        if let engine {
            try? engine.restoreWord(word: word)
        } else {
            try? Engine(config: EngineConfig(userDir: config.userDir)).restoreWord(word: word)
        }
    }

    /// The learned emails, URLs and phone numbers, for `InputController.learnedAddresses`. Asked after the
    /// engine opens and after each learning, removal or forget; while the engine is shed or
    /// not yet open, the last list read.
    func learnedAddresses() -> [String] {
        if let engine, let found = try? engine.learnedAddresses() {
            addresses = found
        }
        return addresses
    }

    /// For the diagnostics report: what is open now, and why not if nothing is.
    func status() -> String {
        if engine != nil {
            // Pack sources are named "<pack id>:<part>" ("ml:translit").
            let model = loadedSources.contains { $0.hasSuffix(":translit") } ? "yes" : "no"
            let lm = loadedSources.contains { $0.hasSuffix(":lm") } ? "yes" : "no"
            return "engine open; sources \(loadedSources.joined(separator: ",")); model loaded \(model); lm loaded \(lm)"
        }
        if shed { return "engine unloaded (memory)" }
        if failedAt != nil { return "engine failed to open (retrying)" }
        return "engine not opened yet"
    }

    /// Drops the engine so its memory is released, and keeps it unloaded until `resume`.
    func unload() {
        ahead = nil
        if engine != nil { log.info("Engine unloaded; footprint before \(Diagnostics.footprint())") }
        engine = nil
        shed = true
    }

    /// Reads the packs and the active choice again; if a pack was installed, replaced or
    /// removed the engine reopens with the new packs on its next use, and if only the choice
    /// changed it is set on the open engine. Cheap when nothing changed: a folder listing and
    /// a few small JSON files.
    func reloadIfChanged() {
        let fresh = Self.readPackState()
        let old = Self.state.withLock { state in
            let old = state
            state = fresh
            return old
        }
        guard fresh != old else { return }
        ahead = nil
        if fresh.packs != old.packs {
            log.info("Packs changed (\(fresh.packs.map(\.id).joined(separator: ","))); the engine reopens")
            let count = config.maxSuggestions
            config = Self.config(packs: fresh.packs)
            config.maxSuggestions = count
            engine = nil
            failedAt = nil
            return
        }
        guard let engine else { return }
        let active = fresh.active ?? ActiveLanguages.engineDefault(packIDs: fresh.packs.map(\.id)).languages
        do {
            try engine.setActiveLanguages(languages: active)
            log.info("Active languages now \(active.joined(separator: ","))")
        } catch {
            log.error("Active languages \(active.joined(separator: ",")) refused: \(String(describing: error))")
        }
    }

    /// The user switched languages in the keyboard's settings panel: stored (`store`), and set
    /// at once on the engine, opening it if need be. Returns the engine's layouts for the
    /// language key; nil when the engine can't be used (the caller falls back to
    /// `defaultLanguages`, which follows the new choice).
    func choose(_ choice: ActiveLanguages) -> [InputLayout]? {
        store(choice)
        let packIDs = Self.packs.map(\.id)
        let active = choice.usable(packIDs: packIDs) ?? ActiveLanguages.engineDefault(packIDs: packIDs).languages
        Self.state.withLock { $0.active = active }
        ahead = nil
        guard let engine = try? loadedEngine() else { return nil }
        do {
            try engine.setActiveLanguages(languages: active)
            log.info("Active languages now \(active.joined(separator: ",")) (keyboard settings)")
        } catch {
            log.error("Active languages \(active.joined(separator: ",")) refused: \(String(describing: error))")
        }
        return try? engine.layouts()
    }

    /// Kept twice. Always in the keyboard's own defaults, with the time, since without Full
    /// Access writing the App Group is untested (reading it works). Also written to the App
    /// Group file, so the app's Languages screen shows it; whether that worked is logged, to
    /// learn it from a device. If it fails, the app may show an older choice, while the
    /// keyboard uses its own as long as it is the newer one (`readPackState`).
    private func store(_ choice: ActiveLanguages) {
        KeyboardLanguageChoice.save(DatedLanguages(choice, date: .now), to: .standard)
        guard let file = PackLocations.activeLanguagesFile() else {
            log.warning("Active languages kept in the keyboard only: no App Group")
            return
        }
        do {
            try choice.write(to: file)
            log.info("Active languages written to the App Group")
        } catch {
            log.warning("Active languages kept in the keyboard only: App Group write failed: \(String(describing: error))")
        }
    }

    /// Memory has recovered: the engine opens again on the next request.
    func resume() {
        shed = false
        refusalLogged = nil
    }

    private struct Shed: Error {}

    private func loadedEngine() throws -> Engine {
        if let engine { return engine }
        if shed {
            noteRefusal("engine unloaded for memory")
            throw Shed()
        }
        if let failedAt, ContinuousClock.now - failedAt < Self.retryDelay {
            noteRefusal("engine open failed; waiting to retry")
            throw Shed()
        }
        do {
            let start = ContinuousClock.now
            let created = try Engine(config: config)
            engine = created
            failedAt = nil
            refusalLogged = nil
            loadedSources = (try? created.loadedSources()) ?? []
            for problem in created.packProblems() {
                log.error("Pack refused: \(problem)")
            }
            if let active = Self.configuredActive {
                do {
                    try created.setActiveLanguages(languages: active)
                } catch {
                    log.error("Active languages \(active.joined(separator: ",")) refused: \(String(describing: error))")
                }
            }
            log.info("Engine open in \(Diagnostics.milliseconds(ContinuousClock.now - start)); sources \(loadedSources.joined(separator: ",")); kernel \(nnKernel()); config en=\(config.english != nil) packs=\(config.packDirs.isEmpty ? "none" : config.packDirs.map { URL(fileURLWithPath: $0).lastPathComponent }.joined(separator: ",")); footprint \(Diagnostics.footprint())")
            return created
        } catch {
            log.error("Engine failed to open: \(String(describing: error)); footprint \(Diagnostics.footprint())")
            // Don't re-read every lexicon on each keystroke: wait before trying again.
            failedAt = .now
            throw error
        }
    }

    private func noteRefusal(_ reason: String) {
        guard refusalLogged != reason else { return }
        refusalLogged = reason
        log.warning("Suggest refused: \(reason)")
    }

    private static func kind(_ kind: CandidateKind) -> SuggestionKind {
        switch kind {
        case .verbatim: .verbatim
        case .completion: .completion
        case .correction: .correction
        case .manglish: .manglish
        case .nextWord: .nextWord
        }
    }
}

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
    private let config: EngineConfig
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

    /// English from the extension bundle and Malayalam from the bundled pack
    /// (`packs/ml.neopack`, M6), whose manifest names its lexicons and models; learned words in
    /// the extension's own container. A file missing from the pack disables only that source
    /// (without the transliterator, Manglish uses the lexicons alone; without the LM, ranking
    /// is M3a's).
    static func bundled() -> EngineSuggestionProvider {
        func lexicon(_ name: String) -> String? {
            Bundle.main.url(forResource: name, withExtension: "fst", subdirectory: "Lexicon")?.path
        }
        let pack = Bundle.main.resourceURL?.appending(path: "packs/ml.neopack", directoryHint: .isDirectory)
        let userDir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?
            .appending(path: "LearnedWords", directoryHint: .isDirectory).path
        return EngineSuggestionProvider(config: EngineConfig(
            english: lexicon("en"),
            userDir: userDir,
            packDir: pack?.path,
            modelDeadlineMs: modelDeadlineMs,
            lmPrefillMs: lmPrefillMs,
            lmAfterSpaceMs: lmAfterSpaceMs,
            lmDeadlineMs: lmDeadlineMs,
            lmPrepareMs: lmPrepareMs
        ))
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
            layout: request.language == .malayalam ? .malayalam : .latin,
            autocorrect: request.autocorrect
        )
        let elapsed = ContinuousClock.now - start
        if elapsed > .milliseconds(10) {
            log.warning("Slow suggest: \(Diagnostics.milliseconds(elapsed)) for \(result.typed.count) chars, \(result.candidates.count) candidates, \(request.language == .malayalam ? "ml" : "latin")")
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

    /// Opens the lexicons ahead of the first request, once the keys are showing, and fills the
    /// LM's context with the text already in the field (`lmPrepareMs`).
    func warmUp(contextBefore: String) {
        guard let engine = try? loadedEngine() else { return }
        try? engine.prepare(contextBefore: contextBefore)
    }

    /// Returns once the work queued on the actor before it is done.
    func idle() {}

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
        let layout: InputLayout = use.language == .malayalam ? .malayalam : .latin
        try? engine.noteUsed(contextBefore: use.before, word: use.word, layout: layout)
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
            if let problem = created.packProblem() {
                log.error("Pack refused: \(problem)")
            }
            log.info("Engine open in \(Diagnostics.milliseconds(ContinuousClock.now - start)); sources \(loadedSources.joined(separator: ",")); kernel \(nnKernel()); config en=\(config.english != nil) pack=\(config.packDir.map { URL(fileURLWithPath: $0).lastPathComponent } ?? "none"); footprint \(Diagnostics.footprint())")
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

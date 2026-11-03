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

    /// Lexicons and the transliterator model from the extension bundle; learned words in the
    /// extension's own container. A file missing from the bundle is passed as nil, which
    /// disables only that source (without the model, Manglish uses the lexicons alone).
    static func bundled() -> EngineSuggestionProvider {
        func lexicon(_ name: String) -> String? {
            Bundle.main.url(forResource: name, withExtension: "fst", subdirectory: "Lexicon")?.path
        }
        func model(_ name: String) -> String? {
            Bundle.main.url(forResource: name, withExtension: "mlkbnn", subdirectory: "Models")?.path
        }
        let userDir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?
            .appending(path: "LearnedWords", directoryHint: .isDirectory).path
        return EngineSuggestionProvider(config: EngineConfig(
            english: lexicon("en"),
            malayalam: lexicon("ml"),
            manglish: lexicon("manglish"),
            nextWords: lexicon("next"),
            translit: model("translit"),
            userDir: userDir,
            modelDeadlineMs: modelDeadlineMs
        ))
    }

    /// Milliseconds into a suggest after which the transliterator stops and its finished
    /// spellings are used (`EngineConfig.modelDeadlineMs`). Typing
    /// leaves the CPU idle between keys, and a suggest that starts from idle runs 5–7× slower
    /// than the same one back to back (iPhone 15 Pro Max, debug app's `-suggestCheck`): a
    /// 4 ms suggest took 20–25 ms, past the coordinator's 15 ms, and the bar went empty. With
    /// 8 ms, the slowest still end by about 11 ms.
    static let modelDeadlineMs: UInt32 = 8

    func suggestions(for request: SuggestionRequest) async throws -> SuggestionResult {
        try Task.checkCancellation()
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

    /// Opens the lexicons ahead of the first request, once the keys are showing.
    func warmUp() {
        _ = try? loadedEngine()
    }

    /// Best effort: a failure only means the word isn't learned.
    func handle(_ event: LearningEvent) {
        switch event {
        case .committed(let word): try? loadedEngine().noteCommitted(word: word)
        case .learn(let word): try? loadedEngine().learn(word: word)
        }
    }

    /// Must work even when the engine is shed or failed to open: an engine with
    /// no lexicons opens only the learned-word store, which is cheap.
    func forgetLearned() {
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
        if let engine {
            try? engine.restoreWord(word: word)
        } else {
            try? Engine(config: EngineConfig(userDir: config.userDir)).restoreWord(word: word)
        }
    }

    /// The learned emails and URLs, for `InputController.learnedAddresses`. Asked after the
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
            let model = loadedSources.contains("translit") ? "yes" : "no"
            return "engine open; sources \(loadedSources.joined(separator: ",")); model loaded \(model)"
        }
        if shed { return "engine unloaded (memory)" }
        if failedAt != nil { return "engine failed to open (retrying)" }
        return "engine not opened yet"
    }

    /// Drops the engine so its memory is released, and keeps it unloaded until `resume`.
    func unload() {
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
            log.info("Engine open in \(Diagnostics.milliseconds(ContinuousClock.now - start)); sources \(loadedSources.joined(separator: ",")); kernel \(nnKernel()); config en=\(config.english != nil) ml=\(config.malayalam != nil) translit=\(config.translit != nil); footprint \(Diagnostics.footprint())")
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

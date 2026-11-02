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
    private let log = Logger(subsystem: "me.jishnu.keyboard.extension", category: "engine")
    private let config: EngineConfig
    private var engine: Engine?
    /// Set while memory is short: the engine stays unloaded until `resume`.
    private var shed = false
    /// When opening last failed; it is tried again after `retryDelay`.
    private var failedAt: ContinuousClock.Instant?
    private static let retryDelay: Duration = .seconds(30)

    init(config: EngineConfig) {
        self.config = config
    }

    /// Lexicons from the extension bundle; learned words in the extension's own container.
    /// A lexicon missing from the bundle is passed as nil, which disables only that source.
    static func bundled() -> EngineSuggestionProvider {
        func lexicon(_ name: String) -> String? {
            Bundle.main.url(forResource: name, withExtension: "fst", subdirectory: "Lexicon")?.path
        }
        let userDir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first?
            .appending(path: "LearnedWords", directoryHint: .isDirectory).path
        return EngineSuggestionProvider(config: EngineConfig(
            english: lexicon("en"),
            malayalam: lexicon("ml"),
            manglish: lexicon("manglish"),
            nextWords: lexicon("next"),
            userDir: userDir
        ))
    }

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
            log.warning("Slow suggest: \(elapsed, privacy: .public) for \(result.typed.count) chars")
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
        if let engine {
            try? engine.forgetLearned()
        } else {
            try? Engine(config: EngineConfig(userDir: config.userDir)).forgetLearned()
        }
    }

    /// Drops the engine so its memory is released, and keeps it unloaded until `resume`.
    func unload() {
        engine = nil
        shed = true
    }

    /// Memory has recovered: the engine opens again on the next request.
    func resume() {
        shed = false
    }

    private struct Shed: Error {}

    private func loadedEngine() throws -> Engine {
        if let engine { return engine }
        if shed { throw Shed() }
        if let failedAt, ContinuousClock.now - failedAt < Self.retryDelay { throw Shed() }
        do {
            let start = ContinuousClock.now
            let created = try Engine(config: config)
            engine = created
            failedAt = nil
            log.info("Engine open in \(ContinuousClock.now - start, privacy: .public); sources \((try? created.loadedSources()) ?? [], privacy: .public); config en=\(self.config.english != nil) ml=\(self.config.malayalam != nil)")
            return created
        } catch {
            log.error("Engine failed to open: \(String(describing: error), privacy: .public)")
            // Don't re-read every lexicon on each keystroke: wait before trying again.
            failedAt = .now
            throw error
        }
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

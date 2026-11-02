import Foundation
import KeyboardCore
import MLEngine

/// Runs the Rust engine off the main thread. The engine is created on first use
/// so the keys appear before anything else loads (spec §4 rule 2). Opening it
/// reads and verifies every lexicon file, so that only ever happens here, on
/// the actor. Callers reach it only through `Task { await … }`, so the main
/// thread never waits.
actor EngineSuggestionProvider: SuggestionProvider {
    private let config: EngineConfig
    private var engine: Engine?
    /// Set when memory ran short or the engine failed to open: it stays
    /// unloaded until iOS recreates the extension.
    private var shed = false

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
        let result = try loadedEngine().suggest(
            contextBefore: request.contextBefore,
            layout: request.language == .malayalam ? .malayalam : .latin,
            autocorrect: request.autocorrect
        )
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

    /// Drops the engine so its memory is released, and keeps it unloaded.
    func unload() {
        engine = nil
        shed = true
    }

    private struct Shed: Error {}

    private func loadedEngine() throws -> Engine {
        if let engine { return engine }
        if shed { throw Shed() }
        do {
            let created = try Engine(config: config)
            engine = created
            return created
        } catch {
            // Don't re-read every lexicon on each keystroke: prediction stays off.
            shed = true
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

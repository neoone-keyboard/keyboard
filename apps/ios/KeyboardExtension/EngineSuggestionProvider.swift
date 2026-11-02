import KeyboardCore
import MLEngine

/// Runs the Rust engine off the main thread. The engine is created on first use
/// so the keys appear before anything else loads (spec §4 rule 2).
actor EngineSuggestionProvider: SuggestionProvider {
    private var engine: Engine?

    func suggestions(for request: SuggestionRequest) async throws -> SuggestionResult {
        try Task.checkCancellation()
        let engine = try self.engine ?? Engine(config: EngineConfig())
        self.engine = engine
        let result = try engine.suggest(
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

    /// Drops the engine so its memory is released; it is recreated on the next request.
    func unload() { engine = nil }

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

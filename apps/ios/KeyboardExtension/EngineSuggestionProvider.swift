import KeyboardCore
import MLEngine

/// Runs the Rust engine off the main thread. The engine is created on first use
/// so the keys appear before anything else loads (spec §4 rule 2).
actor EngineSuggestionProvider: SuggestionProvider {
    private var engine: Engine?

    func suggestions(contextBefore: String) async throws -> [String] {
        try Task.checkCancellation()
        let engine = try self.engine ?? Engine(config: EngineConfig())
        self.engine = engine
        return try engine.suggest(contextBefore: contextBefore, layout: .latin, autocorrect: true)
            .candidates.map(\.text)
    }

    /// Drops the engine so its memory is released; it is recreated on the next request.
    func unload() { engine = nil }
}

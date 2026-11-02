import KeyboardCore
import MLEngine

/// Runs the Rust engine off the main thread. The engine is created on first use
/// so the keys appear before anything else loads (spec §4 rule 2).
actor EngineSuggestionProvider: SuggestionProvider {
    private var engine: Engine?

    func suggestions(contextBefore: String) async throws -> [String] {
        try Task.checkCancellation()
        let engine = try self.engine ?? Engine()
        self.engine = engine
        return try engine.suggest(contextBefore: contextBefore).map(\.text)
    }
}

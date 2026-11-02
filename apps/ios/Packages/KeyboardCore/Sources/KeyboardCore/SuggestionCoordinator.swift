/// Anything that can produce suggestions: the Rust engine in the app, fakes in tests.
public protocol SuggestionProvider: Sendable {
    func suggestions(contextBefore: String) async throws -> [String]
}

/// Asks the provider for suggestions off the main thread. A result from an
/// older request is dropped, never waited on; a slow or failing provider shows
/// nothing instead of blocking typing (spec §4 rules 3 and 5).
@MainActor
public final class SuggestionCoordinator {
    public var isEnabled = true

    private let provider: any SuggestionProvider
    private let timeout: Duration
    private let onUpdate: @MainActor ([String]) -> Void
    private var generation = 0

    public init(
        provider: any SuggestionProvider,
        timeout: Duration = .milliseconds(15),
        onUpdate: @escaping @MainActor ([String]) -> Void
    ) {
        self.provider = provider
        self.timeout = timeout
        self.onUpdate = onUpdate
    }

    @discardableResult
    public func request(contextBefore: String) -> Task<Void, Never> {
        generation += 1
        let requestGeneration = generation
        guard isEnabled else {
            onUpdate([])
            return Task {}
        }
        let provider = self.provider
        let timeout = self.timeout
        return Task { [weak self] in
            let result = await Self.fetch(from: provider, contextBefore: contextBefore, timeout: timeout)
            guard let self, requestGeneration == self.generation else { return }
            self.onUpdate(result)
        }
    }

    /// Invalidates any in-flight request and clears the bar.
    public func clear() {
        generation += 1
        onUpdate([])
    }

    private nonisolated static func fetch(
        from provider: any SuggestionProvider,
        contextBefore: String,
        timeout: Duration
    ) async -> [String] {
        await withTaskGroup(of: [String]?.self) { group in
            group.addTask { try? await provider.suggestions(contextBefore: contextBefore) }
            group.addTask {
                _ = try? await Task.sleep(for: timeout)
                return nil
            }
            let first = await group.next() ?? nil
            group.cancelAll()
            return first ?? []
        }
    }
}

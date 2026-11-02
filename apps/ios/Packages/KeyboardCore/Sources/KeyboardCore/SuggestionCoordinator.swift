import Synchronization

/// Anything that can produce suggestions: the Rust engine in the app, fakes in tests.
///
/// Implementations should call `try Task.checkCancellation()` before doing
/// synchronous work, so a request that is already stale never starts it.
public protocol SuggestionProvider: Sendable {
    func suggestions(for request: SuggestionRequest) async throws -> SuggestionResult
}

/// Asks the provider for suggestions off the main thread. A result from an
/// older request is dropped, never waited on; a slow or failing provider shows
/// nothing instead of blocking typing (spec §4 rules 3 and 5).
///
/// The caller must retain the coordinator; results for a deallocated
/// coordinator are dropped. `onUpdate` must not call back into the coordinator.
@MainActor
public final class SuggestionCoordinator {
    /// Turning this off clears the bar and drops any in-flight result immediately.
    public var isEnabled = true {
        didSet { if oldValue && !isEnabled { clear() } }
    }

    private let provider: any SuggestionProvider
    private let timeout: Duration
    private let onUpdate: @MainActor (SuggestionResult) -> Void
    private var generation = 0
    private var inFlight: Task<Void, Never>?

    public init(
        provider: any SuggestionProvider,
        timeout: Duration = .milliseconds(15),
        onUpdate: @escaping @MainActor (SuggestionResult) -> Void
    ) {
        self.provider = provider
        self.timeout = timeout
        self.onUpdate = onUpdate
    }

    isolated deinit {
        inFlight?.cancel()
    }

    @discardableResult
    public func request(_ request: SuggestionRequest) -> Task<Void, Never> {
        generation += 1
        let requestGeneration = generation
        inFlight?.cancel()
        guard isEnabled else {
            // Disabling already cleared the bar; nothing to show or fetch.
            inFlight = nil
            return Task {}
        }
        let provider = self.provider
        let timeout = self.timeout
        let task = Task { [weak self] in
            let result = await Self.fetch(from: provider, request: request, timeout: timeout)
            guard let self, requestGeneration == self.generation else { return }
            self.onUpdate(result)
        }
        inFlight = task
        return task
    }

    /// Invalidates any in-flight request and clears the bar.
    public func clear() {
        generation += 1
        inFlight?.cancel()
        inFlight = nil
        onUpdate(.empty)
    }

    /// Returns whichever finishes first: the provider's result or `.empty` on
    /// timeout, error or cancellation. Nothing waits for the loser, so a
    /// provider that blocks its thread (a synchronous FFI call ignores
    /// cancellation) cannot delay the timeout.
    @concurrent
    private static func fetch(
        from provider: any SuggestionProvider,
        request: SuggestionRequest,
        timeout: Duration
    ) async -> SuggestionResult {
        let gate = FirstResultGate()
        return await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                gate.install(continuation)
                let work = Task {
                    gate.finish((try? await provider.suggestions(for: request)) ?? .empty)
                }
                let timer = Task {
                    try? await Task.sleep(for: timeout)
                    gate.finish(.empty)
                }
                gate.attach([work, timer])
            }
        } onCancel: {
            gate.finish(.empty)
        }
    }
}

/// Resumes its continuation exactly once, with the first result offered, then
/// cancels the tasks racing for it.
private final class FirstResultGate: Sendable {
    private struct State {
        var continuation: CheckedContinuation<SuggestionResult, Never>?
        var result: SuggestionResult?
        var tasks: [Task<Void, Never>] = []
    }

    private let state = Mutex(State())

    func install(_ continuation: CheckedContinuation<SuggestionResult, Never>) {
        let early = state.withLock { state -> SuggestionResult? in
            if let result = state.result { return result }
            state.continuation = continuation
            return nil
        }
        if let early { continuation.resume(returning: early) }
    }

    func attach(_ tasks: [Task<Void, Never>]) {
        let finished = state.withLock { state -> Bool in
            if state.result != nil { return true }
            state.tasks = tasks
            return false
        }
        if finished { tasks.forEach { $0.cancel() } }
    }

    func finish(_ result: SuggestionResult) {
        let (continuation, tasks) = state.withLock { state -> (CheckedContinuation<SuggestionResult, Never>?, [Task<Void, Never>]) in
            guard state.result == nil else { return (nil, []) }
            state.result = result
            defer { state.continuation = nil; state.tasks = [] }
            return (state.continuation, state.tasks)
        }
        continuation?.resume(returning: result)
        tasks.forEach { $0.cancel() }
    }
}

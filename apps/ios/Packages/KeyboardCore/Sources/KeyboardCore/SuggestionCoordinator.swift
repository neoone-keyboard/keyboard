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

    /// How a fetch ended. A timeout or failure shows as an empty result.
    public enum Outcome: String, Equatable, Sendable {
        case result, timedOut, failed, cancelled
    }

    /// Called just before `onUpdate` for each delivered fetch, with how it ended and how
    /// long it took. For diagnostics only.
    public var onOutcome: (@MainActor (Outcome, Duration) -> Void)?

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
            let start = ContinuousClock.now
            let fetched = await Self.fetch(from: provider, request: request, timeout: timeout)
            guard let self, requestGeneration == self.generation else { return }
            self.onOutcome?(fetched.outcome, ContinuousClock.now - start)
            self.onUpdate(fetched.result)
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
    ) async -> Fetched {
        let gate = FirstResultGate()
        return await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                gate.install(continuation)
                let work = Task {
                    do {
                        gate.finish(Fetched(result: try await provider.suggestions(for: request), outcome: .result))
                    } catch is CancellationError {
                        gate.finish(Fetched(result: .empty, outcome: .cancelled))
                    } catch {
                        gate.finish(Fetched(result: .empty, outcome: .failed))
                    }
                }
                let timer = Task {
                    try? await Task.sleep(for: timeout)
                    gate.finish(Fetched(result: .empty, outcome: .timedOut))
                }
                gate.attach([work, timer])
            }
        } onCancel: {
            gate.finish(Fetched(result: .empty, outcome: .cancelled))
        }
    }
}

private struct Fetched: Sendable {
    var result: SuggestionResult
    var outcome: SuggestionCoordinator.Outcome
}

/// Resumes its continuation exactly once, with the first result offered, then
/// cancels the tasks racing for it.
private final class FirstResultGate: Sendable {
    private struct State {
        var continuation: CheckedContinuation<Fetched, Never>?
        var result: Fetched?
        var tasks: [Task<Void, Never>] = []
    }

    private let state = Mutex(State())

    func install(_ continuation: CheckedContinuation<Fetched, Never>) {
        let early = state.withLock { state -> Fetched? in
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

    func finish(_ result: Fetched) {
        let (continuation, tasks) = state.withLock { state -> (CheckedContinuation<Fetched, Never>?, [Task<Void, Never>]) in
            guard state.result == nil else { return (nil, []) }
            state.result = result
            defer { state.continuation = nil; state.tasks = [] }
            return (state.continuation, state.tasks)
        }
        continuation?.resume(returning: result)
        tasks.forEach { $0.cancel() }
    }
}

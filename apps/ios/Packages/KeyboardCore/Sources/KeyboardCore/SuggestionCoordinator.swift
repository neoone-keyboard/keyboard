import Synchronization

/// Anything that can produce suggestions: the Rust engine in the app, fakes in tests.
///
/// Implementations should call `try Task.checkCancellation()` before doing
/// synchronous work, so a request that is already stale never starts it.
public protocol SuggestionProvider: Sendable {
    func suggestions(for request: SuggestionRequest) async throws -> SuggestionResult
}

/// Asks the provider for suggestions off the main thread. A result from an
/// older request is dropped, never waited on; a failing provider shows nothing
/// and a slow one never blocks typing (spec §4 rules 3 and 5).
///
/// A result is due within `timeout`. Past it the bar is cleared (an empty
/// result), so words for the previous key can't be tapped, and the result is
/// still shown when it comes, unless a newer request has started meanwhile:
/// a slow phone (throttled, older, low power) gets late suggestions, not none.
///
/// The caller must retain the coordinator; results for a deallocated
/// coordinator are dropped. `onUpdate` must not call back into the coordinator.
@MainActor
public final class SuggestionCoordinator {
    /// Turning this off clears the bar and drops any in-flight result immediately.
    public var isEnabled = true {
        didSet { if oldValue && !isEnabled { clear() } }
    }

    /// How a fetch ended. A timeout or failure shows as an empty result. `late` is a
    /// result that came after its request's `timedOut` and was shown then.
    public enum Outcome: String, Equatable, Sendable {
        case result, late, timedOut, failed, cancelled
    }

    /// Called just before `onUpdate` for each delivery, with how the fetch ended and how
    /// long since the request. A request delivers once, or twice when it times out and its
    /// result comes later (`timedOut`, then `late`). For diagnostics only.
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

    /// The returned task ends once the request is settled: its result shown (on time or
    /// late), or it failed, or a newer request or `clear` dropped it.
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
        // `self` is only ever borrowed between awaits, so a pending fetch doesn't keep a
        // released coordinator alive.
        let task = Task { [weak self] in
            // Stale before it started: a newer request already came.
            guard !Task.isCancelled else { return }
            let start = ContinuousClock.now
            let work = Self.startWork(provider: provider, request: request)
            let first = await Self.first(of: work, timeout: timeout)
            guard self?.deliver(first, generation: requestGeneration, since: start) == true,
                  first.outcome == .timedOut
            else { return }
            // The bar is empty for now; the result is still wanted while this is the newest request.
            var late = await Self.first(of: work, timeout: nil)
            guard late.outcome == .result else { return }
            late.outcome = .late
            self?.deliver(late, generation: requestGeneration, since: start)
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

    /// Shows `fetched` if its request is still the newest; returns whether it did.
    @discardableResult
    private func deliver(_ fetched: Fetched, generation requestGeneration: Int, since start: ContinuousClock.Instant) -> Bool {
        guard requestGeneration == generation else { return false }
        onOutcome?(fetched.outcome, ContinuousClock.now - start)
        onUpdate(fetched.result)
        return true
    }

    /// Runs the provider off the main actor. Its value is the result, or `.empty` with
    /// `failed` or `cancelled`.
    nonisolated private static func startWork(
        provider: any SuggestionProvider,
        request: SuggestionRequest
    ) -> Task<Fetched, Never> {
        Task.detached {
            do {
                return Fetched(result: try await provider.suggestions(for: request), outcome: .result)
            } catch is CancellationError {
                return Fetched(result: .empty, outcome: .cancelled)
            } catch {
                return Fetched(result: .empty, outcome: .failed)
            }
        }
    }

    /// Returns whichever finishes first: `work`, `.empty` with `timedOut` once `timeout`
    /// passes (never, when nil), or `.empty` with `cancelled` when the calling task is
    /// cancelled, which also cancels `work`. A timeout leaves `work` running, to be awaited
    /// again. Nothing waits for the loser, so a provider that blocks its thread (a
    /// synchronous FFI call ignores cancellation) cannot delay the timeout or a cancellation.
    @concurrent
    private static func first(of work: Task<Fetched, Never>, timeout: Duration?) async -> Fetched {
        let gate = FirstResultGate()
        return await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                gate.install(continuation)
                var racers = [Task { gate.finish(await work.value) }]
                if let timeout {
                    racers.append(Task {
                        try? await Task.sleep(for: timeout)
                        gate.finish(Fetched(result: .empty, outcome: .timedOut))
                    })
                }
                gate.attach(racers)
            }
        } onCancel: {
            work.cancel()
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

/// How suggestion fetches ended, for the diagnostics report: on time, late (shown after the
/// timeout emptied the bar), timed out and never shown, failed; and the p50 and p99 time to a
/// shown result, over the latest `latencyWindow` of them.
public struct SuggestionOutcomeTally: Sendable {
    public private(set) var onTime = 0
    public private(set) var late = 0
    public private(set) var timedOut = 0
    public private(set) var failed = 0
    private var latencies: [Duration] = []
    private var nextLatency = 0
    public static let latencyWindow = 1_000

    public init() {}

    /// Timed out, and its result never shown: it failed, or a newer request came first.
    public var neverShown: Int { timedOut - late }

    public mutating func record(_ outcome: SuggestionCoordinator.Outcome, after elapsed: Duration) {
        switch outcome {
        case .result: onTime += 1
        case .late: late += 1
        case .timedOut: timedOut += 1
        case .failed: failed += 1
        case .cancelled: return
        }
        guard outcome == .result || outcome == .late else { return }
        if latencies.count < Self.latencyWindow {
            latencies.append(elapsed)
        } else {
            latencies[nextLatency] = elapsed
            nextLatency = (nextLatency + 1) % Self.latencyWindow
        }
    }

    /// The time to a shown result (on time or late) that `p` of them were within.
    public func percentile(_ p: Double) -> Duration? {
        guard !latencies.isEmpty else { return nil }
        let sorted = latencies.sorted()
        return sorted[min(sorted.count - 1, Int(Double(sorted.count) * p))]
    }

    /// "suggest: 812 on time, 40 late, 3 timed out and never shown, 0 failed; p50 4.1 ms, p99 38.0 ms"
    public var summary: String {
        func ms(_ duration: Duration?) -> String {
            guard let duration else { return "-" }
            let (seconds, attoseconds) = duration.components
            return String(format: "%.1f ms", Double(seconds) * 1000 + Double(attoseconds) / 1e15)
        }
        return "suggest: \(onTime) on time, \(late) late, \(neverShown) timed out and never shown, \(failed) failed; p50 \(ms(percentile(0.5))), p99 \(ms(percentile(0.99)))"
    }
}

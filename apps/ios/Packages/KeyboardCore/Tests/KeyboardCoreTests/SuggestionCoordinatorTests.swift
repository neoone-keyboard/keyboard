import Foundation
import Testing
import KeyboardCore

/// The result a provider that echoes `context` returns.
func echo(_ context: String) -> SuggestionResult {
    SuggestionResult(typed: context, candidates: [Suggestion(context, kind: .verbatim)])
}

func englishRequest(_ context: String) -> SuggestionRequest {
    SuggestionRequest(contextBefore: context, language: .english, autocorrect: true)
}

/// Echoes the context back. "late" takes 40 ms, "slow" 100 ms, "hang" 5 s; "fail" throws.
struct ScriptedProvider: SuggestionProvider {
    struct Failure: Error {}

    func suggestions(for request: SuggestionRequest) async throws -> SuggestionResult {
        switch request.contextBefore {
        case "late": try await Task.sleep(for: .milliseconds(40))
        case "slow": try await Task.sleep(for: .milliseconds(100))
        case "hang": try await Task.sleep(for: .seconds(5))
        case "fail": throw Failure()
        default: break
        }
        return echo(request.contextBefore)
    }
}

/// Blocks its thread in a synchronous call that ignores cancellation, like the Rust engine.
actor BlockingProvider: SuggestionProvider {
    func suggestions(for request: SuggestionRequest) async throws -> SuggestionResult {
        usleep(200_000)
        return echo(request.contextBefore)
    }
}

/// Throws 40 ms in, after any timeout shorter than that.
struct LateFailingProvider: SuggestionProvider {
    func suggestions(for request: SuggestionRequest) async throws -> SuggestionResult {
        try await Task.sleep(for: .milliseconds(40))
        throw ScriptedProvider.Failure()
    }
}

@MainActor
final class Recorder {
    var updates: [SuggestionResult] = []
    var outcomes: [SuggestionCoordinator.Outcome] = []
}

@MainActor
struct SuggestionCoordinatorTests {
    let recorder = Recorder()

    /// Until the bar has had `count` updates (a timeout's empty one, say), or a second passed.
    func waitForUpdates(_ count: Int) async {
        let deadline = ContinuousClock.now + .seconds(1)
        while recorder.updates.count < count, ContinuousClock.now < deadline {
            try? await Task.sleep(for: .milliseconds(1))
        }
    }

    func makeCoordinator(timeout: Duration = .seconds(1)) -> SuggestionCoordinator {
        SuggestionCoordinator(provider: ScriptedProvider(), timeout: timeout) { [recorder] in
            recorder.updates.append($0)
        }
    }

    @Test func deliversResults() async {
        let coordinator = makeCoordinator()
        await coordinator.request(englishRequest("wor")).value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [echo("wor")])
    }

    @Test func dropsStaleResults() async {
        let coordinator = makeCoordinator()
        let old = coordinator.request(englishRequest("slow"))
        let new = coordinator.request(englishRequest("new"))
        await new.value
        await old.value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [echo("new")])
    }

    @Test func timeoutFallsBackToEmpty() async {
        let coordinator = makeCoordinator(timeout: .milliseconds(20))
        let task = coordinator.request(englishRequest("hang"))
        await waitForUpdates(1)
        try? await Task.sleep(for: .milliseconds(30))
        #expect(recorder.updates == [.empty])
        coordinator.clear()
        await task.value
    }

    /// A slow phone misses the timeout on every key: the bar empties at the timeout, then
    /// shows the result when it comes.
    @Test func lateResultIsShownAfterTheEmptyOne() async {
        let coordinator = makeCoordinator(timeout: .milliseconds(15))
        coordinator.onOutcome = { [recorder] outcome, _ in recorder.outcomes.append(outcome) }
        await coordinator.request(englishRequest("late")).value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [.empty, echo("late")])
        #expect(recorder.outcomes == [.timedOut, .late])
    }

    @Test func lateResultIsDroppedOnceANewerRequestStarts() async {
        let coordinator = makeCoordinator(timeout: .milliseconds(15))
        let old = coordinator.request(englishRequest("late"))
        await waitForUpdates(1)
        #expect(recorder.updates == [.empty])
        let new = coordinator.request(englishRequest("new"))
        await new.value
        await old.value
        try? await Task.sleep(for: .milliseconds(40))
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [.empty, echo("new")])
    }

    @Test func lateResultIsDroppedAfterClear() async {
        let coordinator = makeCoordinator(timeout: .milliseconds(15))
        let task = coordinator.request(englishRequest("late"))
        await waitForUpdates(1)
        coordinator.clear()
        await task.value
        try? await Task.sleep(for: .milliseconds(40))
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [.empty, .empty])
    }

    /// Overlapping slow requests, each started after the one before timed out: every one
    /// clears the bar, only the newest result is shown, and an older one never lands after it.
    @Test func overlappingLateRequestsShowOnlyTheNewest() async {
        let coordinator = makeCoordinator(timeout: .milliseconds(15))
        coordinator.onOutcome = { [recorder] outcome, _ in recorder.outcomes.append(outcome) }
        var tasks: [Task<Void, Never>] = []
        for timedOut in 1...3 {
            tasks.append(coordinator.request(englishRequest("late")))
            await waitForUpdates(timedOut)
        }
        tasks.append(coordinator.request(englishRequest("now")))
        for task in tasks { await task.value }
        try? await Task.sleep(for: .milliseconds(60))
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [.empty, .empty, .empty, echo("now")])
        #expect(recorder.outcomes == [.timedOut, .timedOut, .timedOut, .result])
    }

    /// The newest request, slow, still shows its result once the older ones are dropped.
    @Test func newestOfOverlappingRequestsArrivesLate() async {
        let coordinator = makeCoordinator(timeout: .milliseconds(15))
        let first = coordinator.request(englishRequest("slow"))
        await waitForUpdates(1)
        let second = coordinator.request(englishRequest("late"))
        await second.value
        await first.value
        try? await Task.sleep(for: .milliseconds(100))
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [.empty, .empty, echo("late")])
    }

    @Test func providerFailingAfterTheTimeoutShowsNothingMore() async {
        let coordinator = SuggestionCoordinator(provider: LateFailingProvider(), timeout: .milliseconds(15)) { [recorder] in
            recorder.updates.append($0)
        }
        coordinator.onOutcome = { [recorder] outcome, _ in recorder.outcomes.append(outcome) }
        await coordinator.request(englishRequest("x")).value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [.empty])
        #expect(recorder.outcomes == [.timedOut])
    }

    @Test func errorsFallBackToEmpty() async {
        let coordinator = makeCoordinator()
        await coordinator.request(englishRequest("fail")).value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [.empty])
    }

    @Test func disabledCoordinatorShowsNothing() async {
        let coordinator = makeCoordinator()
        coordinator.isEnabled = false
        await coordinator.request(englishRequest("wor")).value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [.empty])
    }

    @Test func timeoutFiresEvenWhenProviderBlocksItsThread() async {
        let coordinator = SuggestionCoordinator(provider: BlockingProvider(), timeout: .milliseconds(20)) { [recorder] in
            recorder.updates.append($0)
        }
        let clock = ContinuousClock()
        let start = clock.now
        var firstUpdate: Duration?
        coordinator.onOutcome = { _, _ in firstUpdate = firstUpdate ?? clock.now - start }
        await coordinator.request(englishRequest("x")).value
        withExtendedLifetime(coordinator) {}
        print("blocking-provider timeout elapsed: \(String(describing: firstUpdate))")
        // The bar empties at the timeout; the blocked call's result follows when it ends.
        #expect(recorder.updates == [.empty, echo("x")])
        #expect(firstUpdate.map { $0 < .milliseconds(100) } == true)
    }

    @Test func onlyTheLastOfBackToBackRequestsIsDelivered() async {
        let coordinator = makeCoordinator()
        _ = coordinator.request(englishRequest("slow"))
        _ = coordinator.request(englishRequest("slow"))
        _ = coordinator.request(englishRequest("slow"))
        _ = coordinator.request(englishRequest("slow"))
        let last = coordinator.request(englishRequest("last"))
        await last.value
        try? await Task.sleep(for: .milliseconds(200))
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [echo("last")])
    }

    @Test func disablingDropsInFlightResult() async {
        let coordinator = makeCoordinator()
        let task = coordinator.request(englishRequest("slow"))
        coordinator.isEnabled = false
        await task.value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [.empty])
    }

    @Test func clearDropsInFlightResult() async {
        let coordinator = makeCoordinator()
        let task = coordinator.request(englishRequest("slow"))
        coordinator.clear()
        await task.value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [.empty])
    }

    @Test func reportsHowEachFetchEnded() async {
        let coordinator = makeCoordinator(timeout: .milliseconds(20))
        coordinator.onOutcome = { [recorder] outcome, _ in recorder.outcomes.append(outcome) }
        await coordinator.request(englishRequest("wor")).value
        await coordinator.request(englishRequest("late")).value
        await coordinator.request(englishRequest("fail")).value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.outcomes == [.result, .timedOut, .late, .failed])
        #expect(recorder.updates == [echo("wor"), .empty, echo("late"), .empty])
    }
}

struct SuggestionOutcomeTallyTests {
    @Test func countsOutcomesAndLatencyOfShownResults() {
        var tally = SuggestionOutcomeTally()
        for ms in 1...98 { tally.record(.result, after: .milliseconds(ms)) }
        tally.record(.timedOut, after: .milliseconds(15))
        tally.record(.late, after: .milliseconds(40))
        tally.record(.timedOut, after: .milliseconds(15))
        tally.record(.timedOut, after: .milliseconds(15))
        tally.record(.late, after: .milliseconds(200))
        tally.record(.failed, after: .milliseconds(2))
        tally.record(.cancelled, after: .milliseconds(1))
        #expect(tally.onTime == 98)
        #expect(tally.late == 2)
        #expect(tally.timedOut == 3)
        #expect(tally.neverShown == 1)
        #expect(tally.failed == 1)
        #expect(tally.percentile(0.5) == .milliseconds(50))
        #expect(tally.percentile(0.99) == .milliseconds(200))
        #expect(tally.summary == "suggest: 98 on time, 2 late, 1 timed out and never shown, 1 failed; p50 50.0 ms, p99 200.0 ms")
    }

    @Test func emptyTallySaysSo() {
        #expect(SuggestionOutcomeTally().summary == "suggest: 0 on time, 0 late, 0 timed out and never shown, 0 failed; p50 -, p99 -")
    }
}

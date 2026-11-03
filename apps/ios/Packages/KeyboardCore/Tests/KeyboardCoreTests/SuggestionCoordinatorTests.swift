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

/// Echoes the context back. "slow" takes 100 ms, "hang" takes 5 s, "fail" throws.
struct ScriptedProvider: SuggestionProvider {
    struct Failure: Error {}

    func suggestions(for request: SuggestionRequest) async throws -> SuggestionResult {
        switch request.contextBefore {
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

@MainActor
final class Recorder {
    var updates: [SuggestionResult] = []
    var outcomes: [SuggestionCoordinator.Outcome] = []
}

@MainActor
struct SuggestionCoordinatorTests {
    let recorder = Recorder()

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
        await coordinator.request(englishRequest("hang")).value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [.empty])
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
        await coordinator.request(englishRequest("x")).value
        let elapsed = clock.now - start
        withExtendedLifetime(coordinator) {}
        print("blocking-provider timeout elapsed: \(elapsed)")
        #expect(recorder.updates == [.empty])
        #expect(elapsed < .milliseconds(100))
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
        await coordinator.request(englishRequest("hang")).value
        await coordinator.request(englishRequest("fail")).value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.outcomes == [.result, .timedOut, .failed])
        #expect(recorder.updates == [echo("wor"), .empty, .empty])
    }
}

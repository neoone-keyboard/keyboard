import Foundation
import Testing
import KeyboardCore

/// Echoes the context back. "slow" takes 100 ms, "hang" takes 5 s, "fail" throws.
struct ScriptedProvider: SuggestionProvider {
    struct Failure: Error {}

    func suggestions(contextBefore: String) async throws -> [String] {
        switch contextBefore {
        case "slow": try await Task.sleep(for: .milliseconds(100))
        case "hang": try await Task.sleep(for: .seconds(5))
        case "fail": throw Failure()
        default: break
        }
        return [contextBefore]
    }
}

/// Blocks its thread in a synchronous call that ignores cancellation, like the Rust engine.
actor BlockingProvider: SuggestionProvider {
    func suggestions(contextBefore: String) async throws -> [String] {
        usleep(200_000)
        return [contextBefore]
    }
}

@MainActor
final class Recorder {
    var updates: [[String]] = []
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
        await coordinator.request(contextBefore: "wor").value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [["wor"]])
    }

    @Test func dropsStaleResults() async {
        let coordinator = makeCoordinator()
        let old = coordinator.request(contextBefore: "slow")
        let new = coordinator.request(contextBefore: "new")
        await new.value
        await old.value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [["new"]])
    }

    @Test func timeoutFallsBackToEmpty() async {
        let coordinator = makeCoordinator(timeout: .milliseconds(20))
        await coordinator.request(contextBefore: "hang").value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [[]])
    }

    @Test func errorsFallBackToEmpty() async {
        let coordinator = makeCoordinator()
        await coordinator.request(contextBefore: "fail").value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [[]])
    }

    @Test func disabledCoordinatorShowsNothing() async {
        let coordinator = makeCoordinator()
        coordinator.isEnabled = false
        await coordinator.request(contextBefore: "wor").value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [[]])
    }

    @Test func timeoutFiresEvenWhenProviderBlocksItsThread() async {
        let coordinator = SuggestionCoordinator(provider: BlockingProvider(), timeout: .milliseconds(20)) { [recorder] in
            recorder.updates.append($0)
        }
        let clock = ContinuousClock()
        let start = clock.now
        await coordinator.request(contextBefore: "x").value
        let elapsed = clock.now - start
        withExtendedLifetime(coordinator) {}
        print("blocking-provider timeout elapsed: \(elapsed)")
        #expect(recorder.updates == [[]])
        #expect(elapsed < .milliseconds(100))
    }

    @Test func onlyTheLastOfBackToBackRequestsIsDelivered() async {
        let coordinator = makeCoordinator()
        _ = coordinator.request(contextBefore: "slow")
        _ = coordinator.request(contextBefore: "slow")
        _ = coordinator.request(contextBefore: "slow")
        _ = coordinator.request(contextBefore: "slow")
        let last = coordinator.request(contextBefore: "last")
        await last.value
        try? await Task.sleep(for: .milliseconds(200))
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [["last"]])
    }

    @Test func disablingDropsInFlightResult() async {
        let coordinator = makeCoordinator()
        let task = coordinator.request(contextBefore: "slow")
        coordinator.isEnabled = false
        await task.value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [[]])
    }

    @Test func clearDropsInFlightResult() async {
        let coordinator = makeCoordinator()
        let task = coordinator.request(contextBefore: "slow")
        coordinator.clear()
        await task.value
        withExtendedLifetime(coordinator) {}
        #expect(recorder.updates == [[]])
    }
}

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
}

import Foundation
import Testing
import MLEngine

@Test func engineWithoutLexiconsSuggestsTheTypedWord() throws {
    let engine = try Engine(config: EngineConfig())
    let result = try engine.suggest(contextBefore: "hello wor", layout: .latin, autocorrect: true)
    #expect(result.typed == "wor")
    #expect(result.candidates.map(\.text) == ["wor"])
    #expect(result.candidates.first?.kind == .verbatim)
    #expect(try engine.loadedSources().isEmpty)
}

@Test func suggestHandlesMalayalam() throws {
    let engine = try Engine(config: EngineConfig())
    let result = try engine.suggest(contextBefore: "ഞാൻ എന്റ", layout: .malayalam, autocorrect: true)
    #expect(result.candidates.map(\.text) == ["എന്റ"])
}

@Test func missingLexiconFilesAreSwitchedOff() throws {
    let engine = try Engine(config: EngineConfig(english: "/nonexistent/en.fst"))
    #expect(try engine.loadedSources().isEmpty)
    #expect(try engine.suggest(contextBefore: "teh", layout: .latin, autocorrect: true).candidates.map(\.text) == ["teh"])
}

@Test func learnedWordsPersistAndCanBeForgotten() throws {
    let dir = FileManager.default.temporaryDirectory.appending(path: "mlengine-\(UUID().uuidString)")
    defer { try? FileManager.default.removeItem(at: dir) }
    let config = EngineConfig(userDir: dir.path)
    try Engine(config: config).learn(word: "Thomasina")
    let reloaded = try Engine(config: config)
    let completions = { try reloaded.suggest(contextBefore: "Thoma", layout: .latin, autocorrect: true).candidates.map(\.text) }
    #expect(try completions().contains("Thomasina"))
    try reloaded.forgetLearned()
    #expect(try !completions().contains("Thomasina"))
}

/// Spec §10 risk: Swift ↔ Rust call overhead. The whole keystroke budget is 15 ms;
/// one FFI round-trip must be a tiny fraction of it.
@Test func ffiRoundTripIsFast() throws {
    let engine = try Engine(config: EngineConfig())
    let clock = ContinuousClock()
    var samples: [Duration] = []
    samples.reserveCapacity(10_000)
    for _ in 0..<10_000 {
        let start = clock.now
        _ = try engine.suggest(contextBefore: "ഞാൻ ഇന്ന് വീട്ടിൽ പോ", layout: .malayalam, autocorrect: true)
        samples.append(clock.now - start)
    }
    samples.sort()
    let p99 = samples[samples.count * 99 / 100]
    #expect(p99 < .microseconds(200), "p99 FFI round-trip was \(p99)")
}

@Test func vowelSignBaseCrossesFFI() throws {
    #expect(try malayalamVowelSignBase(contextBefore: "ന്റ") == "റ")
    #expect(try malayalamVowelSignBase(contextBefore: "കി") == nil)
}

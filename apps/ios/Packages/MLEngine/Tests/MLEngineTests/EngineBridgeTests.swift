import Testing
import MLEngine

@Test func suggestEchoesCurrentWord() throws {
    let engine = try Engine()
    #expect(try engine.suggest(contextBefore: "hello wor").map(\.text) == ["wor"])
}

@Test func suggestHandlesMalayalam() throws {
    let engine = try Engine()
    #expect(try engine.suggest(contextBefore: "ഞാൻ എന്റ").map(\.text) == ["എന്റ"])
}

/// Spec §10 risk: Swift ↔ Rust call overhead. The whole keystroke budget is 15 ms;
/// one FFI round-trip must be a tiny fraction of it.
@Test func ffiRoundTripIsFast() throws {
    let engine = try Engine()
    let clock = ContinuousClock()
    var samples: [Duration] = []
    samples.reserveCapacity(10_000)
    for _ in 0..<10_000 {
        let start = clock.now
        _ = try engine.suggest(contextBefore: "ഞാൻ ഇന്ന് വീട്ടിൽ പോ")
        samples.append(clock.now - start)
    }
    samples.sort()
    let p99 = samples[samples.count * 99 / 100]
    #expect(p99 < .microseconds(200), "p99 FFI round-trip was \(p99)")
}

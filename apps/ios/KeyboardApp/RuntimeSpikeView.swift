#if DEBUG
import CoreML
import KeyboardCore
import MLEngine
import SwiftUI

/// The M3 runtime spike (design §5): the same random full-size transliterator timed in the
/// Rust engine and in Core ML. Debug builds only; `make spike-assets` puts the models in the
/// app bundle under SpikeAssets/.
struct RuntimeSpikeView: View {
    @State private var results: [SpikeResult] = []
    @State private var running = false

    var body: some View {
        List {
            Section {
                Button(running ? "Running…" : "Run") { run() }
                    .disabled(running)
                Text("\(Spike.words) words. Per word: encode + \(Spike.steps) decoder steps × beam \(Spike.beams). Rust and Core ML (all units) run 3 times each, then Core ML once on CPU only and once on CPU + Neural Engine.")
                    .font(.footnote)
                    .foregroundStyle(.secondary)
            }
            Section("Results") {
                ForEach(results) { result in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(result.title).font(.headline)
                        Text(result.detail).font(.footnote.monospaced())
                    }
                }
            }
        }
        .navigationTitle("Runtime check")
    }

    private func run() {
        running = true
        results = []
        Task.detached(priority: .userInitiated) {
            await RuntimeCheck.runAll { result in await MainActor.run { results.append(result) } }
            await MainActor.run { running = false }
        }
    }
}

/// Runs every job and also writes the results to Documents/runtime-check.txt, so they can be
/// copied off the phone with `xcrun devicectl device copy from --domain-type appDataContainer`.
/// Launching the app with `-runtimeCheck` runs it without touching the screen.
enum RuntimeCheck {
    static var output: URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("runtime-check.txt")
    }

    /// `-runtimeCheckCPU` runs only Core ML on the CPU, 3 times, so its memory isn't mixed
    /// with the other runtimes'.
    @MainActor static func runFromLaunchArguments() {
        let arguments = ProcessInfo.processInfo.arguments
        if arguments.contains("-suggestCheck") {
            // Keeps running for a while if the app leaves the foreground.
            let background = UIApplication.shared.beginBackgroundTask()
            Thread.detachNewThread {
                SuggestCheck.run()
                DispatchQueue.main.async { UIApplication.shared.endBackgroundTask(background) }
            }
        } else if arguments.contains("-runtimeCheckCPU") {
            Task.detached(priority: .userInitiated) {
                await runAll((1...3).map { (.coreML(.cpuOnly), $0) }) { _ in }
            }
        } else if arguments.contains("-runtimeCheck") {
            Task.detached(priority: .userInitiated) { await runAll { _ in } }
        }
    }

    fileprivate static func runAll(_ jobs: [(Runtime, Int)] = Spike.jobs, _ each: (SpikeResult) async -> Void) async {
        var text = ""
        try? text.write(to: output, atomically: true, encoding: .utf8)
        for job in jobs {
            let result = Spike.measure(job)
            text += "\(result.title)\n\(result.detail)\n\n"
            try? text.write(to: output, atomically: true, encoding: .utf8)
            await each(result)
        }
        text += "done\n"
        try? text.write(to: output, atomically: true, encoding: .utf8)
    }
}

/// The keyboard's full suggest, timed per keystroke on the phone: the extension's bundled
/// lexicons and model, words typed letter by letter after "I said ". Each word runs typed
/// back to back (the CPU stays busy, as in a benchmark) and with a pause between keys (as when
/// typing, so each suggest starts from an idle CPU), then paused with the keyboard's model
/// deadline, marking keys whose bar it changed. Launch the app with `-suggestCheck`;
/// the result is Documents/suggest-check.txt.
enum SuggestCheck {
    static let words = ["preferenc", "apostrophe", "definitely", "enthu", "sukhamano", "avishyam"]
    /// As the keyboard's `EngineSuggestionProvider.modelDeadlineMs`.
    static let deadlineMs: UInt32 = 8
    static var output: URL {
        FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("suggest-check.txt")
    }

    static func run() {
        var text = "suggest check, kernel \(nnKernel())\n"
        let write = { try? text.write(to: output, atomically: true, encoding: .utf8) }
        write()
        guard let appex = Bundle.main.builtInPlugInsURL?.appendingPathComponent("KeyboardExtension.appex") else {
            text += "no extension bundle\n"
            write()
            return
        }
        let file = { (dir: String, name: String) in appex.appendingPathComponent(dir).appendingPathComponent(name).path }
        let config = { (deadline: UInt32?) in
            EngineConfig(
                english: file("Lexicon", "en.fst"),
                malayalam: file("Lexicon", "ml.fst"),
                manglish: file("Lexicon", "manglish.fst"),
                nextWords: file("Lexicon", "next.fst"),
                translit: file("Models", "translit.mlkbnn"),
                userDir: nil,
                modelDeadlineMs: deadline
            )
        }
        guard let unbounded = try? Engine(config: config(nil)),
              let bounded = try? Engine(config: config(deadlineMs))
        else {
            text += "engine failed to open\n"
            write()
            return
        }
        text += "sources \((try? unbounded.loadedSources()) ?? [])\n"
        let bar = { (engine: Engine, context: String) in
            ((try? engine.suggest(contextBefore: context, layout: .latin, autocorrect: true))?.candidates ?? []).map(\.text)
        }
        var full: [String: [String]] = [:]
        for word in words {  // warm the page cache; the bar without a deadline
            for end in 1...word.count {
                let context = "I said " + word.prefix(end)
                full[context] = bar(unbounded, context)
                _ = bar(bounded, context)
            }
        }
        let phases: [(String, Double, Engine)] = [
            ("back to back, no deadline", 0, unbounded),
            ("150 ms between keys, model deadline \(deadlineMs) ms (* bar differs)", 0.15, bounded),
            ("150 ms between keys, no deadline", 0.15, unbounded),
        ]
        for (label, pause, engine) in phases {
            var all: [Double] = []
            var differs: [String] = []
            text += "\n\(label)\n"
            for word in words {
                var line = word
                for end in 1...word.count {
                    let context = "I said " + word.prefix(end)
                    if pause > 0 { Thread.sleep(forTimeInterval: pause) }
                    let start = ContinuousClock.now
                    let shown = bar(engine, context)
                    let ms = (ContinuousClock.now - start).milliseconds
                    all.append(ms)
                    line += String(format: " %.1f", ms) + (shown == full[context] ? "" : "*")
                    if shown != full[context] {
                        differs.append("\(word.prefix(end)): \(shown) for \(full[context] ?? [])")
                    }
                }
                text += line + "\n"
            }
            all.sort()
            text += String(format: "p50 %.1f ms, p90 %.1f ms, max %.1f ms\n",
                           all[all.count / 2], all[all.count * 9 / 10], all[all.count - 1])
            text += differs.map { "* \($0)\n" }.joined()
            write()
        }
        text += "done\n"
        write()
    }
}

struct SpikeResult: Identifiable, Sendable {
    let id = UUID()
    let title: String
    let detail: String
}

/// What one run measured. Times in microseconds unless named otherwise.
private struct Timings {
    var loadMs: Double
    var p50: UInt64
    var p99: UInt64
    var encodeP50: UInt64
}

private enum Runtime: Sendable {
    case rust
    case coreML(MLComputeUnits)

    var name: String {
        switch self {
        case .rust: "Rust"
        case .coreML(.all): "Core ML, all units"
        case .coreML(.cpuOnly): "Core ML, CPU only"
        case .coreML(.cpuAndNeuralEngine): "Core ML, CPU + ANE"
        case .coreML: "Core ML"
        }
    }
}

private enum Spike {
    static let words = 200
    static let steps = 10
    static let beams = 4
    static let srcLen = 16
    static let cacheLen = 32
    static let masked: Float16 = -1e4

    static let jobs: [(Runtime, Int)] =
        (1...3).map { (.rust, $0) } + (1...3).map { (.coreML(.all), $0) }
        + [(.coreML(.cpuOnly), 1), (.coreML(.cpuAndNeuralEngine), 1)]

    static var assets: URL { Bundle.main.resourceURL!.appendingPathComponent("SpikeAssets") }

    static func measure(_ job: (Runtime, Int)) -> SpikeResult {
        let (runtime, run) = job
        let title = "\(runtime.name), run \(run)"
        let before = MemoryMonitor.footprintBytes()
        let sampler = PeakSampler()
        sampler.start()
        var afterLoad: UInt64 = 0
        let timings: Timings
        do {
            switch runtime {
            case .rust:
                timings = try rust { afterLoad = MemoryMonitor.footprintBytes() }
            case .coreML(let units):
                timings = try coreML(units) { afterLoad = MemoryMonitor.footprintBytes() }
            }
        } catch {
            sampler.stop()
            return SpikeResult(title: title, detail: "failed: \(error)")
        }
        let peak = max(sampler.stop(), afterLoad)
        let mb = { (bytes: UInt64) in String(format: "%.1f", Double(bytes) / 1_048_576) }
        let delta = { (bytes: UInt64) in bytes >= before ? "+" + mb(bytes - before) : "-" + mb(before - bytes) }
        let ms = { (us: UInt64) in String(format: "%.2f", Double(us) / 1000) }
        let detail = """
            load \(String(format: "%.1f", timings.loadMs)) ms
            per word p50 \(ms(timings.p50)) ms, p99 \(ms(timings.p99)) ms
            encoder only p50 \(ms(timings.encodeP50)) ms
            footprint after load \(mb(afterLoad)) MB (\(delta(afterLoad)) MB)
            peak \(delta(peak)) MB
            """
        return SpikeResult(title: title, detail: detail)
    }

    private static func rust(loaded: () -> Void) throws -> Timings {
        let model = try NnBenchmark(path: assets.appendingPathComponent("translit-spike.mlkbnn").path)
        loaded()
        let bench = try model.run(words: UInt32(words))
        return Timings(loadMs: bench.loadMs, p50: bench.p50Us, p99: bench.p99Us, encodeP50: bench.encodeP50Us)
    }

    /// The same work as `NnBenchmark.run` in the engine: the same synthetic words, one encode,
    /// then `steps` batched decoder steps over `beams` beams, each beam continuing the next
    /// beam's cache after every step (the `parent` input) and fed its own argmax token.
    private static func coreML(_ units: MLComputeUnits, loaded: () -> Void) throws -> Timings {
        let url = assets.appendingPathComponent("translit-spike.mlmodelc")
        let start = ContinuousClock.now
        let config = MLModelConfiguration()
        config.computeUnits = units
        config.functionName = "encoder"
        let encoder = try MLModel(contentsOf: url, configuration: config)
        config.functionName = "decoder"
        let decoder = try MLModel(contentsOf: url, configuration: config)
        let loadMs = (ContinuousClock.now - start).milliseconds
        loaded()

        let src = try MLMultiArray(shape: [1, srcLen as NSNumber], dataType: .int32)
        let srcMask = try MLMultiArray(shape: [1, srcLen as NSNumber], dataType: .float16)
        let token = try MLMultiArray(shape: [beams as NSNumber], dataType: .int32)
        let pos = try MLMultiArray(shape: [1, cacheLen as NSNumber], dataType: .float16)
        let attnMask = try MLMultiArray(shape: [1, cacheLen as NSNumber], dataType: .float16)
        let identity = try parentMatrix { $0 }
        let fork = try parentMatrix { ($0 + 1) % beams }

        var total: [UInt64] = []
        var encode: [UInt64] = []
        var rng = Lcg(seed: 7)
        for _ in 0..<words {
            let word = rng.word()
            try autoreleasepool {
                let start = ContinuousClock.now
                fill(src, Int32.self) { $0 < word.count ? Int32(word[$0]) : 0 }
                fill(srcMask, Float16.self) { $0 < word.count ? 0 : masked }
                let enc = try encoder.prediction(from: MLDictionaryFeatureProvider(dictionary: [
                    "src": src, "src_mask": srcMask,
                ]))
                encode.append((ContinuousClock.now - start).microseconds)
                let crossK = enc.featureValue(for: "cross_k")!
                let crossV = enc.featureValue(for: "cross_v")!
                let state = decoder.makeState()
                var tokens = [Int32](repeating: 1, count: beams)
                for step in 0..<steps {
                    fill(token, Int32.self) { tokens[$0] }
                    fill(pos, Float16.self) { $0 == step ? 1 : 0 }
                    fill(attnMask, Float16.self) { $0 <= step ? 0 : masked }
                    let out = try decoder.prediction(
                        from: MLDictionaryFeatureProvider(dictionary: [
                            "token": token, "pos": pos, "attn_mask": attnMask,
                            "parent": step == 0 ? identity : fork,
                            "cross_k": crossK, "cross_v": crossV, "src_mask": srcMask,
                        ]),
                        using: state
                    )
                    tokens = argmax(out.featureValue(for: "logits")!.multiArrayValue!)
                }
                total.append((ContinuousClock.now - start).microseconds)
            }
        }
        total.sort()
        encode.sort()
        return Timings(
            loadMs: loadMs, p50: percentile(total, 0.5), p99: percentile(total, 0.99),
            encodeP50: percentile(encode, 0.5)
        )
    }

    /// Row `i` picks the beam whose cache beam `i` continues.
    private static func parentMatrix(_ parent: (Int) -> Int) throws -> MLMultiArray {
        let m = try MLMultiArray(shape: [beams as NSNumber, beams as NSNumber], dataType: .float16)
        fill(m, Float16.self) { parent($0 / beams) == $0 % beams ? 1 : 0 }
        return m
    }

    private static func fill<T: MLShapedArrayScalar>(_ array: MLMultiArray, _: T.Type, _ value: (Int) -> T) {
        array.withUnsafeMutableBufferPointer(ofType: T.self) { buffer, _ in
            for i in buffer.indices { buffer[i] = value(i) }
        }
    }

    /// Each beam's best non-special token (ids 3 and up), from `[beams, vocab]` logits.
    private static func argmax(_ logits: MLMultiArray) -> [Int32] {
        logits.dataType == .float16 ? argmax(logits, Float16.self) : argmax(logits, Float.self)
    }

    private static func argmax<T: BinaryFloatingPoint & MLShapedArrayScalar>(_ logits: MLMultiArray, _: T.Type) -> [Int32] {
        let vocab = logits.shape[1].intValue
        let rowStride = logits.strides[0].intValue
        return logits.withUnsafeBufferPointer(ofType: T.self) { buffer in
            (0..<beams).map { b in
                var best = 3
                for t in 4..<vocab where buffer[b * rowStride + t] > buffer[b * rowStride + best] {
                    best = t
                }
                return Int32(best)
            }
        }
    }

    private static func percentile(_ sorted: [UInt64], _ p: Double) -> UInt64 {
        sorted.isEmpty ? 0 : sorted[Int((Double(sorted.count - 1) * p).rounded())]
    }
}

/// The engine's benchmark words: a 64-bit LCG, words of 4–12 letters as Latin token ids
/// (bos = 1, eos = 2, a…z = 3…28).
private struct Lcg {
    var seed: UInt64

    mutating func next() -> UInt64 {
        seed = seed &* 6_364_136_223_846_793_005 &+ 1_442_695_040_888_963_407
        return seed >> 33
    }

    mutating func word() -> [UInt32] {
        let length = 4 + Int(next() % 9)
        return [1] + (0..<length).map { _ in 3 + UInt32(next() % 26) } + [2]
    }
}

/// Polls the footprint every millisecond on its own thread and keeps the highest value.
private final class PeakSampler: @unchecked Sendable {
    private let lock = NSLock()
    private var peak: UInt64 = 0
    private var running = true

    func start() {
        Thread.detachNewThread { [self] in
            while lock.withLock({ running }) {
                let now = MemoryMonitor.footprintBytes()
                lock.withLock { peak = max(peak, now) }
                usleep(1000)
            }
        }
    }

    /// Stops polling and returns the peak.
    @discardableResult
    func stop() -> UInt64 {
        lock.withLock {
            running = false
            return peak
        }
    }
}

private extension Duration {
    var microseconds: UInt64 {
        let (s, attos) = components
        return UInt64(s) * 1_000_000 + UInt64(attos / 1_000_000_000_000)
    }

    var milliseconds: Double { Double(microseconds) / 1000 }
}
#endif

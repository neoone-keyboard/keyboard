import CoreGraphics
import CoreText
import Foundation
import Synchronization

/// The emoji panel's pictures: each emoji drawn by CoreText into a small image, off the main
/// thread, and a few of them kept.
///
/// Emoji drawn as text stay decoded in the system emoji font's cache for the life of the
/// process (about 48 KB each at 3x: over 100 MB for a scroll through the whole panel), and a
/// memory warning doesn't free them. So the drawing uses a private copy of the font, made
/// from the font file's mapped bytes (`EmojiFontFile`: clean pages, never copied), and
/// swapped for a fresh copy after every `fontLifetime` emoji: the cache goes with the old
/// one. Without the file it falls back to the system font, which looks the same and costs
/// that memory.
///
/// Emoji are drawn in batches of up to `batchSize`, those on screen (`onScreen`) first, then
/// those prefetched, then any others asked for; a batch is split between two threads of the
/// renderer's own, which end on `close`. (CoreText holds on to the font a thread last drew
/// with until that thread draws with another or ends, so pooled threads would keep old
/// fonts, and their caches, alive.)
@MainActor
public final class EmojiRenderer {
    /// The images' width and height in pixels: the emoji's point size × the screen's scale.
    public let pixelSide: Int
    /// Emoji drawn so far, and the private fonts made for them.
    public private(set) var drawn = 0
    public private(set) var fontsMade = 0
    public var usesPrivateFont: Bool { EmojiFontFile.shared != nil }
    /// The private fonts in this process that CoreText hasn't freed yet.
    public nonisolated static var fontCopiesAlive: Int { EmojiFontFile.copiesAlive.load(ordering: .relaxed) }
    /// The emoji on screen, top to bottom, drawn before any others. Without it, in the order
    /// asked for.
    public var onScreen: (@MainActor () -> [String])?
    /// Lines for the diagnostics log (never typed text): the font file once, each private
    /// font made and dropped, the timings every `summaryEvery` emoji and on close.
    public var onLog: (@MainActor (String) -> Void)? {
        didSet { if oldValue == nil { onLog?(EmojiFontFile.report) } }
    }

    private let cache = NSCache<NSString, Picture>()
    /// Asked for by cells and not yet drawn: who to tell, and when each was first asked.
    private var waiting: [String: [@MainActor (CGImage) -> Void]] = [:]
    private var askedAt: [String: ContinuousClock.Instant] = [:]
    private var askOrder: [String] = []
    /// Prefetched, in the order asked.
    private var ahead: [String] = []
    private var drawing = false
    /// Sent to be drawn and not back yet.
    private var inFlight: Set<String> = []
    private var pumpQueued = false
    private var closed = false
    private var timings = EmojiTimings()
    private let painter: Painter

    /// Emoji drawn with one private font before it is replaced: two batches, about 1.5 MB of
    /// its cache at 3x.
    public nonisolated static let fontLifetime = 32
    /// Emoji sent to be drawn at once: two rows, so what is on screen is looked at again soon
    /// after a scroll.
    public nonisolated static let batchSize = 16
    /// Prefetched emoji not yet drawn that are kept; older ones have scrolled past.
    static let aheadLimit = 64
    /// Asked-for emoji off screen that are kept; older ones are dropped (a cell asks again
    /// when it shows).
    static let offScreenLimit = 64
    /// A timings line in the log every so many emoji drawn.
    public nonisolated static let summaryEvery = 100

    /// `cacheBytes` bounds the kept images, as their pages take (48 KB for 96 × 96): those on
    /// screen are held by their cells anyway, so this is what is kept off screen, to scroll
    /// back to. The first font is made at once, while the panel is laid out.
    public init(pixelSide: Int, cacheBytes: Int = 1_048_576) {
        self.pixelSide = pixelSide
        painter = Painter(side: pixelSide)
        cache.totalCostLimit = cacheBytes
        painter.thread.async { [painter, weak self] in
            let made = painter.prepare()
            DispatchQueue.main.async { MainActor.assumeIsolated { self?.noteFont(made) } }
        }
    }

    /// Dropped without `close`: the threads still end.
    isolated deinit {
        if !closed { painter.stop() }
    }

    /// `emoji`'s image if it's kept; otherwise nil, and `then` gets it once drawn.
    public func image(for emoji: String, then: @escaping @MainActor (CGImage) -> Void) -> CGImage? {
        if let picture = cache.object(forKey: emoji as NSString) { return picture.image }
        guard !closed else { return nil }
        if waiting[emoji] == nil {
            askedAt[emoji] = askedAt[emoji] ?? .now
            askOrder.append(emoji)
        }
        waiting[emoji, default: []].append(then)
        queuePump()
        return nil
    }

    /// `emoji`'s image if it's kept, without asking for it.
    public func kept(_ emoji: String) -> CGImage? {
        cache.object(forKey: emoji as NSString)?.image
    }

    /// Where `emoji` is, for the log: "kept", "drawing", "waiting", "ahead" or "not asked".
    public func state(of emoji: String) -> String {
        if kept(emoji) != nil { return "kept" }
        if inFlight.contains(emoji) { return "drawing" }
        if waiting[emoji] != nil { return askOrder.contains(emoji) ? "waiting" : "waiting, not queued" }
        return ahead.contains(emoji) ? "ahead" : "not asked"
    }

    /// Starts drawing emoji that are about to show.
    public func prefetch(_ emoji: [String]) {
        guard !closed else { return }
        for each in emoji where cache.object(forKey: each as NSString) == nil && !ahead.contains(each) {
            ahead.append(each)
            askedAt[each] = askedAt[each] ?? .now
        }
        if ahead.count > Self.aheadLimit { ahead.removeFirst(ahead.count - Self.aheadLimit) }
        queuePump()
    }

    /// Emoji no longer about to show.
    public func cancelPrefetch(_ emoji: [String]) {
        let gone = Set(emoji)
        ahead.removeAll { gone.contains($0) && waiting[$0] == nil }
    }

    /// Drops the kept images and the font with its cache (on a memory warning).
    public func empty() {
        cache.removeAllObjects()
        painter.thread.async { [painter, weak self] in
            guard let report = painter.dropFont() else { return }
            DispatchQueue.main.async { MainActor.assumeIsolated { self?.onLog?(report) } }
        }
    }

    /// Stops drawing and lets everything go: the pictures now; the font, and the threads,
    /// once the emoji being drawn is done. Logs the timings not yet logged, then the
    /// footprint once the font is dropped.
    public func close() {
        guard !closed else { return }
        closed = true
        logTimings()
        cache.removeAllObjects()
        waiting = [:]
        askedAt = [:]
        askOrder = []
        ahead = []
        let log = onLog, before = MemoryMonitor.footprintBytes()
        painter.stop { dropped in
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    log?("Emoji renderer closed: \(dropped ?? "no font to drop"); footprint \(megabytes(before)) at close")
                }
            }
        }
    }

    /// Logs the timings not yet summed up.
    public func logTimings() {
        if !timings.isEmpty { onLog?(timings.summary()) }
        timings = EmojiTimings()
    }

    // MARK: Batches

    private func queuePump() {
        // After the layout pass that asked, so `onScreen` has its cells.
        guard !pumpQueued else { return }
        pumpQueued = true
        DispatchQueue.main.async { [weak self] in
            MainActor.assumeIsolated {
                self?.pumpQueued = false
                self?.pump()
            }
        }
    }

    private func pump() {
        guard !drawing, !closed else { return }
        let screen = onScreen?()
        let batch = Self.nextBatch(onScreen: screen, asked: askOrder, waiting: Set(waiting.keys), ahead: ahead,
                                   limit: Self.batchSize)
        guard !batch.isEmpty else { return }
        let sent = Set(batch)
        inFlight.formUnion(sent)
        ahead.removeAll { sent.contains($0) }
        askOrder.removeAll { sent.contains($0) }
        // Off screen and long ago: the cell has gone (or asks again when it shows).
        if let screen, askOrder.count > Self.offScreenLimit {
            let shown = Set(screen)
            let old = Set(askOrder.prefix(askOrder.count - Self.offScreenLimit).filter { !shown.contains($0) })
            for emoji in old {
                waiting[emoji] = nil
                askedAt[emoji] = nil
            }
            askOrder.removeAll { old.contains($0) }
        }
        drawing = true
        painter.thread.async { [painter, weak self] in
            let made = painter.prepare()
            if let made { DispatchQueue.main.async { MainActor.assumeIsolated { self?.noteFont(made) } } }
            let fresh = painter.isFresh
            painter.draw(batch) { emoji, image, milliseconds, blank in
                let picture = image.map(Picture.init)
                DispatchQueue.main.async {
                    MainActor.assumeIsolated {
                        if let blank { self?.onLog?("Emoji \(emoji) drawn blank; \(blank)") }
                        self?.finish(emoji, picture, milliseconds: milliseconds, fresh: fresh)
                    }
                }
            }
            // Not logged: a scroll makes and drops dozens of fonts; the timings line counts them.
            _ = painter.endBatch()
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    self?.drawing = false
                    self?.pump()
                }
            }
        }
    }

    /// What to draw next: the emoji on screen that were asked for, top to bottom, then those
    /// prefetched, then the others asked for, latest first; without `onScreen`, in the order
    /// asked.
    nonisolated static func nextBatch(onScreen: [String]?, asked: [String], waiting: Set<String>, ahead: [String],
                                      limit: Int) -> [String] {
        var batch: [String] = []
        var taken = Set<String>()
        func take(_ emoji: some Sequence<String>) {
            for each in emoji where batch.count < limit && !taken.contains(each) {
                batch.append(each)
                taken.insert(each)
            }
        }
        if let onScreen {
            take(onScreen.filter(waiting.contains))
            take(ahead)
            take(asked.reversed().filter(waiting.contains))
        } else {
            take(asked.filter(waiting.contains))
            take(ahead)
        }
        return batch
    }

    private func noteFont(_ made: Painter.FontMade?) {
        guard let made else { return }
        fontsMade += 1
        timings.fontMakes.append(made.milliseconds)
    }

    private func finish(_ emoji: String, _ picture: Picture?, milliseconds: Double, fresh: Bool) {
        guard !closed else { return }
        inFlight.remove(emoji)
        drawn += 1
        timings.add(draw: milliseconds, withNewFont: fresh)
        if let asked = askedAt.removeValue(forKey: emoji) { timings.waits.append(EmojiTimings.milliseconds(.now - asked)) }
        if drawn % Self.summaryEvery == 0 { logTimings() }
        let callbacks = waiting.removeValue(forKey: emoji) ?? []
        guard let picture else { return }
        cache.setObject(picture, forKey: emoji as NSString, cost: picture.bytes)
        for callback in callbacks { callback(picture.image) }
    }

    /// Draws `emoji` as `pixelSide` would, waiting for it (for tests).
    nonisolated static func draw(_ emoji: String, side: Int) -> CGImage? {
        let painter = Painter(side: side)
        nonisolated(unsafe) var image: CGImage?
        painter.thread.sync {
            _ = painter.prepare()
            painter.draw([emoji]) { _, drawn, _, _ in image = drawn }
        }
        painter.stop()
        return image
    }
}

/// How long the renderer's work took, in milliseconds, summed up for the log.
struct EmojiTimings {
    var draws: [Double] = []
    /// The draws in a font's first batch, which read the font's tables.
    var firstDraws: [Double] = []
    /// From an emoji being asked for to its picture being handed over.
    var waits: [Double] = []
    var fontMakes: [Double] = []

    var isEmpty: Bool { draws.isEmpty && firstDraws.isEmpty }

    mutating func add(draw: Double, withNewFont: Bool) {
        if withNewFont { firstDraws.append(draw) } else { draws.append(draw) }
    }

    /// "Emoji timings: 84 drawn, 0.4 ms p50, 3.1 ms max; 16 in a font's first batch, 1.0 ms
    /// p50, 12.1 ms max; shown 85.0 ms p50, 640.2 ms max after asked; 2 fonts made, 0.3 ms
    /// p50, 1.2 ms max"
    func summary() -> String {
        "Emoji timings: \(draws.count) drawn, \(Self.spread(draws)); \(firstDraws.count) in a font's first batch, "
            + "\(Self.spread(firstDraws)); shown \(Self.spread(waits)) after asked; "
            + "\(fontMakes.count) fonts made, \(Self.spread(fontMakes))"
    }

    private static func spread(_ values: [Double]) -> String {
        guard !values.isEmpty else { return "-" }
        let sorted = values.sorted()
        return String(format: "%.1f ms p50, %.1f ms max", sorted[sorted.count / 2], sorted[sorted.count - 1])
    }

    static func milliseconds(_ duration: Duration) -> Double {
        let (seconds, attoseconds) = duration.components
        return Double(seconds) * 1000 + Double(attoseconds) / 1e15
    }
}

/// "31.4 MB"
func megabytes(_ bytes: UInt64) -> String {
    String(format: "%.1f MB", Double(bytes) / 1_048_576)
}

/// The footprint now, for the log.
func footprintText() -> String {
    megabytes(MemoryMonitor.footprintBytes())
}

/// The system emoji font's file, mapped into memory once per process: clean pages of a
/// file the system has open anyway, which the footprint doesn't count. Each private font
/// reads straight from them (a CGDataProvider over the bytes, no copy), and counts itself
/// in `copiesAlive` until CoreText frees it.
final class EmojiFontFile: @unchecked Sendable {
    let bytes: UnsafeRawPointer
    let count: Int

    static var shared: EmojiFontFile? { loaded.file }
    /// For the log: where the file is, its size, how it was read and what that took.
    static var report: String { loaded.report }
    private static let loaded = load()
    static let copiesAlive = Atomic<Int>(0)

    private init(bytes: UnsafeRawPointer, count: Int) {
        self.bytes = bytes
        self.count = count
    }

    private static func load() -> (file: EmojiFontFile?, report: String) {
        let system = CTFontCreateWithName("AppleColorEmoji" as CFString, 32, nil)
        guard let url = CTFontCopyAttribute(system, kCTFontURLAttribute) as? URL else {
            return (nil, "Emoji font file not found")
        }
        let before = footprintText(), start = ContinuousClock.now
        let descriptor = open(url.path, O_RDONLY)
        guard descriptor >= 0 else { return (nil, "Emoji font file \(url.path) unreadable (errno \(errno))") }
        defer { close(descriptor) }
        var status = stat()
        guard fstat(descriptor, &status) == 0, status.st_size > 0 else {
            return (nil, "Emoji font file \(url.path) has no size (errno \(errno))")
        }
        let count = Int(status.st_size)
        guard let bytes = mmap(nil, count, PROT_READ, MAP_SHARED, descriptor, 0), bytes != MAP_FAILED else {
            return (nil, "Emoji font file \(url.path) could not be mapped (errno \(errno))")
        }
        let report = String(format: "Emoji font file %@, %.1f MB, mapped (mmap, read-only, no copy) in %.2f ms; footprint %@ → %@",
                            url.path, Double(count) / 1_048_576, EmojiTimings.milliseconds(.now - start), before, footprintText())
        return (EmojiFontFile(bytes: UnsafeRawPointer(bytes), count: count), report)
    }

    /// A copy of the font of its own at `size`, sharing nothing with the system's but the
    /// file's pages; nil if CoreGraphics can't read it.
    func makeFont(size: CGFloat) -> CTFont? {
        guard let provider = CGDataProvider(dataInfo: nil, data: bytes, size: count, releaseData: { _, _, _ in
            EmojiFontFile.copiesAlive.subtract(1, ordering: .relaxed)
        }) else { return nil }
        Self.copiesAlive.add(1, ordering: .relaxed)
        guard let graphicsFont = CGFont(provider) else { return nil }
        return CTFontCreateWithGraphicsFont(graphicsFont, size, nil, nil)
    }
}

/// A drawn emoji, for NSCache.
private final class Picture: @unchecked Sendable {
    let image: CGImage
    /// What it takes in memory: whole 16 KB pages.
    let bytes: Int

    init(_ image: CGImage) {
        self.image = image
        bytes = (image.bytesPerRow * image.height + 16_383) / 16_384 * 16_384
    }
}

/// A thread of its own that runs work in order, until stopped.
final class WorkThread: @unchecked Sendable {
    private let condition = NSCondition()
    private var work: [@Sendable () -> Void] = []
    private var stopping = false

    init(name: String) {
        let thread = Thread { [self] in run() }
        thread.name = name
        thread.qualityOfService = .userInitiated
        thread.start()
    }

    func async(_ job: @escaping @Sendable () -> Void) {
        condition.lock()
        work.append(job)
        condition.signal()
        condition.unlock()
    }

    func sync(_ job: @escaping @Sendable () -> Void) {
        let done = DispatchSemaphore(value: 0)
        async {
            job()
            done.signal()
        }
        done.wait()
    }

    /// The thread ends once the work already given is done.
    func stop() {
        condition.lock()
        stopping = true
        condition.signal()
        condition.unlock()
    }

    private func run() {
        while true {
            condition.lock()
            while work.isEmpty && !stopping { condition.wait() }
            if work.isEmpty {
                condition.unlock()
                return
            }
            let job = work.removeFirst()
            condition.unlock()
            autoreleasepool { job() }
        }
    }
}

/// Draws on its own two threads: `thread` runs the renderer's work and draws half of each
/// batch, `helper` the other half.
private final class Painter: @unchecked Sendable {
    let side: Int
    let thread = WorkThread(name: "me.jishnu.keyboard.emoji-renderer")
    private let helper = WorkThread(name: "me.jishnu.keyboard.emoji-renderer.helper")
    // Only touched on `thread`.
    private var font: CTFont?
    private var drawsWithFont = 0
    private var fontNumber = 0
    private let stopped = Atomic<Bool>(false)
    private static let colorSpace = CGColorSpace(name: CGColorSpace.displayP3) ?? CGColorSpaceCreateDeviceRGB()

    struct FontMade: Sendable {
        let milliseconds: Double
        let report: String
    }

    init(side: Int) {
        self.side = side
    }

    /// The font has drawn nothing yet.
    var isFresh: Bool { drawsWithFont == 0 }

    /// Makes a font if there is none: what that took, for the log.
    func prepare() -> FontMade? {
        guard font == nil, !stopped.load(ordering: .relaxed) else { return nil }
        let before = footprintText(), start = ContinuousClock.now
        let size = CGFloat(side)
        font = EmojiFontFile.shared?.makeFont(size: size) ?? CTFontCreateWithName("AppleColorEmoji" as CFString, size, nil)
        let took = EmojiTimings.milliseconds(.now - start)
        fontNumber += 1
        drawsWithFont = 0
        return FontMade(milliseconds: took, report: String(
            format: "Emoji font %d made in %.2f ms, footprint %@ → %@; %d private fonts alive", fontNumber, took, before,
            footprintText(), EmojiRenderer.fontCopiesAlive))
    }

    /// Draws `emoji` with the font, half on each thread (CoreText's fonts are thread-safe),
    /// handing each image over as it is drawn, with the milliseconds it took. Stops early
    /// once stopped.
    /// An emoji drawn blank is drawn again with the same font, then with the system's; `each`
    /// gets what happened to it.
    func draw(_ emoji: [String], each: @escaping @Sendable (String, CGImage?, Double, String?) -> Void) {
        guard let font else { return }
        nonisolated(unsafe) let shared = font
        let lane: @Sendable (Int) -> Void = { [self] first in
            for index in stride(from: first, to: emoji.count, by: 2) {
                if stopped.load(ordering: .relaxed) { return }
                let start = ContinuousClock.now
                // What CoreText autoreleases holds on to the font: let go of it with each emoji.
                var (image, blank) = autoreleasepool { render(emoji[index], font: shared) }
                var redrawn: String?
                if blank {
                    (image, blank) = autoreleasepool { render(emoji[index], font: shared) }
                    redrawn = blank ? nil : "drawn again with the same font, fine"
                }
                if blank {
                    let system = CTFontCreateWithName("AppleColorEmoji" as CFString, CGFloat(side), nil)
                    (image, blank) = autoreleasepool { render(emoji[index], font: system) }
                    redrawn = blank ? "blank again with the same font and the system's" : "fine with the system font"
                }
                each(emoji[index], image, EmojiTimings.milliseconds(.now - start), redrawn)
            }
        }
        let other = DispatchSemaphore(value: 0)
        if emoji.count > 1 {
            helper.async {
                lane(1)
                other.signal()
            }
        }
        lane(0)
        if emoji.count > 1 { other.wait() }
        drawsWithFont += emoji.count
    }

    /// Drops the font once it has drawn its lifetime: the log line, if it did.
    func endBatch() -> String? {
        drawsWithFont >= EmojiRenderer.fontLifetime ? dropFont() : nil
    }

    /// What dropping the font did to the footprint, if there was one. CoreText holds on to
    /// the font a thread last drew with until it draws with another, so both threads draw a
    /// space with a system font.
    func dropFont() -> String? {
        guard font != nil else { return nil }
        let before = footprintText()
        autoreleasepool {
            font = nil
            flush()
        }
        helper.sync { [self] in flush() }
        return "Emoji font \(fontNumber) dropped, footprint \(before) → \(footprintText()); "
            + "\(EmojiRenderer.fontCopiesAlive) private fonts alive"
    }

    /// Stops drawing (from any thread), then drops the font and ends both threads once the
    /// work already given is done; `then` gets the line for the log, on `thread`.
    func stop(then: (@Sendable (String?) -> Void)? = nil) {
        stopped.store(true, ordering: .relaxed)
        thread.async { [self] in
            let dropped = dropFont()
            helper.stop()
            then?(dropped)
        }
        thread.stop()
    }

    private func flush() {
        let system = CTFontCreateUIFontForLanguage(.system, 1, nil) ?? CTFontCreateWithName("Helvetica" as CFString, 1, nil)
        _ = render(" ", font: system).image
    }

    /// The picture, and whether nothing was drawn in it.
    private func render(_ emoji: String, font: CTFont) -> (image: CGImage?, blank: Bool) {
        guard let context = CGContext(
            data: nil, width: side, height: side, bitsPerComponent: 8, bytesPerRow: 0, space: Self.colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
        else { return (nil, true) }
        let text = NSAttributedString(string: emoji, attributes: [NSAttributedString.Key(kCTFontAttributeName as String): font])
        let line = CTLineCreateWithAttributedString(text)
        // An emoji's picture fills a square the font's size; centred, and made smaller in
        // the rare case of more than one picture (a sequence the font doesn't join).
        let ink = CTLineGetImageBounds(line, context)
        let size = CGFloat(side)
        let fit = min(1, size / max(ink.width, ink.height, 1))
        context.translateBy(x: size / 2, y: size / 2)
        context.scaleBy(x: fit, y: fit)
        context.textPosition = CGPoint(x: -ink.midX, y: -ink.midY)
        CTLineDraw(line, context)
        return (context.makeImage(), Self.isEmpty(context))
    }

    /// No pixel drawn: every alpha byte (the 4th of each BGRA pixel) is 0.
    private static func isEmpty(_ context: CGContext) -> Bool {
        guard let data = context.data else { return true }
        let bytes = data.assumingMemoryBound(to: UInt8.self)
        for row in 0..<context.height {
            let start = row * context.bytesPerRow
            for alpha in stride(from: start + 3, to: start + context.width * 4, by: 4) where bytes[alpha] != 0 {
                return false
            }
        }
        return true
    }
}

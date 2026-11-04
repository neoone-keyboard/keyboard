import CoreGraphics
import CoreText
import Foundation

/// The emoji panel's pictures: each emoji drawn by CoreText into a small image, off the main
/// thread, and a few of them kept.
///
/// Emoji drawn as text stay decoded in the system emoji font's cache for the life of the
/// process (about 48 KB each at 3x: over 100 MB for a scroll through the whole panel), and a
/// memory warning doesn't free them (Simulator). So the drawing uses a private copy of the
/// font, made from the font file's bytes (memory-mapped, so clean pages), and swapped for a
/// fresh copy after every `fontLifetime` emoji: the cache goes with the old one. Without the
/// file it falls back to the system font, which looks the same and costs that memory.
@MainActor
public final class EmojiRenderer {
    /// The images' width and height in pixels: the emoji's point size × the screen's scale.
    public let pixelSide: Int
    /// Emoji drawn so far, and the private fonts made for them.
    public private(set) var drawn = 0
    public private(set) var fontsMade = 0
    public var usesPrivateFont: Bool { painter.fontData != nil }
    /// Lines for the diagnostics log (never typed text): the font file once, each private
    /// font made and dropped, and the timings every `summaryEvery` emoji.
    public var onLog: (@MainActor (String) -> Void)? {
        didSet { if oldValue == nil { onLog?(painter.fileReport) } }
    }

    private let cache = NSCache<NSString, Picture>()
    private var waiting: [String: [@MainActor (CGImage) -> Void]] = [:]
    /// When each emoji in `waiting` was asked for.
    private var askedAt: [String: ContinuousClock.Instant] = [:]
    private var timings = EmojiTimings()
    private let painter: Painter
    /// Each draw's leftovers (the old font among them) are freed as it ends, not in a batch.
    private let queue = DispatchQueue(label: "me.jishnu.keyboard.emoji-renderer", qos: .userInitiated,
                                      autoreleaseFrequency: .workItem)

    /// Emoji drawn with one private font before it is replaced: about 1.2 MB of its cache at
    /// 3x. Making one takes about 0.2 ms; longer lifetimes measured higher peaks (Simulator).
    public nonisolated static let fontLifetime = 25
    /// A timings line in the log every so many emoji drawn.
    public nonisolated static let summaryEvery = 100

    /// `cacheBytes` bounds the kept images, as their pages take (48 KB for 96 × 96).
    public init(pixelSide: Int, cacheBytes: Int = 3 * 1_048_576) {
        self.pixelSide = pixelSide
        painter = Painter(side: pixelSide)
        cache.totalCostLimit = cacheBytes
    }

    /// `emoji`'s image if it's kept; otherwise nil, and `then` gets it once drawn.
    public func image(for emoji: String, then: @escaping @MainActor (CGImage) -> Void) -> CGImage? {
        if let picture = cache.object(forKey: emoji as NSString) { return picture.image }
        request(emoji, then: then)
        return nil
    }

    /// Starts drawing emoji that are about to show.
    public func prefetch(_ emoji: [String]) {
        for each in emoji where cache.object(forKey: each as NSString) == nil {
            request(each, then: nil)
        }
    }

    /// Drops the kept images and the font's cache (on a memory warning).
    public func empty() {
        cache.removeAllObjects()
        queue.async { [painter, weak self] in
            guard let report = painter.dropFont() else { return }
            DispatchQueue.main.async { MainActor.assumeIsolated { self?.onLog?(report) } }
        }
    }

    /// Logs the timings not yet summed up (on close).
    public func logTimings() {
        if !timings.isEmpty { onLog?(timings.summary()) }
        timings = EmojiTimings()
    }

    private func request(_ emoji: String, then: (@MainActor (CGImage) -> Void)?) {
        if waiting[emoji] != nil {
            if let then { waiting[emoji]?.append(then) }
            return
        }
        waiting[emoji] = then.map { [$0] } ?? []
        askedAt[emoji] = .now
        queue.async { [painter, weak self] in
            let drawing = painter.draw(emoji)
            let picture = drawing.image.map(Picture.init)
            DispatchQueue.main.async {
                MainActor.assumeIsolated { self?.finish(emoji, picture, drawing) }
            }
        }
    }

    private func finish(_ emoji: String, _ picture: Picture?, _ drawing: Painter.Drawing) {
        drawn += 1
        if let font = drawing.newFont {
            fontsMade += 1
            timings.fontMakes.append(font.milliseconds)
            onLog?(font.report)
        }
        timings.add(draw: drawing.milliseconds, withNewFont: drawing.newFont != nil)
        if let asked = askedAt.removeValue(forKey: emoji) { timings.waits.append(EmojiTimings.milliseconds(.now - asked)) }
        if drawn % Self.summaryEvery == 0 { logTimings() }
        let callbacks = waiting.removeValue(forKey: emoji) ?? []
        guard let picture else { return }
        cache.setObject(picture, forKey: emoji as NSString, cost: picture.bytes)
        for callback in callbacks { callback(picture.image) }
    }

    /// Draws `emoji` as `pixelSide` would, on the calling thread (for tests).
    nonisolated static func draw(_ emoji: String, side: Int) -> CGImage? {
        Painter(side: side).draw(emoji).image
    }
}

/// How long the renderer's work took, in milliseconds, summed up for the log.
struct EmojiTimings {
    var draws: [Double] = []
    /// Each new font's first draw, which reads the font's tables.
    var firstDraws: [Double] = []
    /// From an emoji being asked for to its picture being handed over.
    var waits: [Double] = []
    var fontMakes: [Double] = []

    var isEmpty: Bool { draws.isEmpty && firstDraws.isEmpty }

    mutating func add(draw: Double, withNewFont: Bool) {
        if withNewFont { firstDraws.append(draw) } else { draws.append(draw) }
    }

    /// "Emoji timings: 100 drawn, 0.4 ms p50, 3.1 ms max; 4 first with a new font, 12.0 ms
    /// p50, 14.1 ms max; shown 85.0 ms p50, 640.2 ms max after asked; 4 fonts made, 0.3 ms
    /// p50, 1.2 ms max"
    func summary() -> String {
        "Emoji timings: \(draws.count) drawn, \(Self.spread(draws)); \(firstDraws.count) first with a new font, "
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

/// The footprint now, for the log ("31.4 MB").
func footprintText() -> String {
    String(format: "%.1f MB", Double(MemoryMonitor.footprintBytes()) / 1_048_576)
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

/// Draws on the renderer's queue only.
private final class Painter: @unchecked Sendable {
    let side: Int
    /// The emoji font file, memory-mapped; nil if it can't be read.
    let fontData: Data?
    /// Where the font file is, its size, and what reading it took.
    let fileReport: String
    private var font: CTFont?
    private var drawsWithFont = 0
    private var fontNumber = 0
    private static let colorSpace = CGColorSpace(name: CGColorSpace.displayP3) ?? CGColorSpaceCreateDeviceRGB()

    init(side: Int) {
        self.side = side
        let system = CTFontCreateWithName("AppleColorEmoji" as CFString, CGFloat(side), nil)
        let url = CTFontCopyAttribute(system, kCTFontURLAttribute) as? URL
        let before = footprintText(), start = ContinuousClock.now
        fontData = url.flatMap { try? Data(contentsOf: $0, options: .alwaysMapped) }
        let took = EmojiTimings.milliseconds(.now - start)
        if let url, let fontData {
            fileReport = String(format: "Emoji font file %@, %.1f MB, read (alwaysMapped) in %.1f ms; footprint %@ → %@",
                                url.path, Double(fontData.count) / 1_048_576, took, before, footprintText())
        } else {
            fileReport = "Emoji font file \(url?.path ?? "not found") unreadable"
        }
    }

    /// What dropping it did to the footprint, if there was a font to drop.
    func dropFont() -> String? {
        guard font != nil else { return nil }
        let before = footprintText()
        font = nil
        return "font \(fontNumber) dropped, footprint \(before) → \(footprintText())"
    }

    struct Drawing {
        var image: CGImage?
        var milliseconds: Double = 0
        /// When a new font was made for it: what that took, and the log line.
        var newFont: (milliseconds: Double, report: String)?
    }

    func draw(_ emoji: String) -> Drawing {
        var drawing = Drawing()
        var dropped: String?
        if drawsWithFont >= EmojiRenderer.fontLifetime { dropped = dropFont() }
        if font == nil {
            let before = footprintText(), start = ContinuousClock.now
            font = makeFont()
            let took = EmojiTimings.milliseconds(.now - start)
            fontNumber += 1
            drawsWithFont = 0
            let made = String(format: "Emoji font %d made in %.1f ms, footprint %@ → %@", fontNumber, took, before, footprintText())
            drawing.newFont = (took, made + (dropped.map { "; \($0)" } ?? ""))
        }
        drawsWithFont += 1
        let start = ContinuousClock.now
        drawing.image = render(emoji)
        drawing.milliseconds = EmojiTimings.milliseconds(.now - start)
        return drawing
    }

    private func render(_ emoji: String) -> CGImage? {
        guard let font, let context = CGContext(
            data: nil, width: side, height: side, bitsPerComponent: 8, bytesPerRow: 0, space: Self.colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
        else { return nil }
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
        return context.makeImage()
    }

    /// A copy of the system emoji font of its own, so its cache can be let go; the system
    /// font itself without the file.
    private func makeFont() -> CTFont {
        let size = CGFloat(side)
        guard let fontData,
              let all = CTFontManagerCreateFontDescriptorsFromData(fontData as CFData) as? [CTFontDescriptor],
              let descriptor = all.first(where: {
                  CTFontDescriptorCopyAttribute($0, kCTFontNameAttribute) as? String == "AppleColorEmoji"
              }) ?? all.first
        else { return CTFontCreateWithName("AppleColorEmoji" as CFString, size, nil) }
        return CTFontCreateWithFontDescriptor(descriptor, size, nil)
    }
}

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

    private let cache = NSCache<NSString, Picture>()
    private var waiting: [String: [@MainActor (CGImage) -> Void]] = [:]
    private let painter: Painter
    /// Each draw's leftovers (the old font among them) are freed as it ends, not in a batch.
    private let queue = DispatchQueue(label: "me.jishnu.keyboard.emoji-renderer", qos: .userInitiated,
                                      autoreleaseFrequency: .workItem)

    /// Emoji drawn with one private font before it is replaced: about 1.2 MB of its cache at
    /// 3x. Making one takes about 0.2 ms; longer lifetimes measured higher peaks (Simulator).
    public nonisolated static let fontLifetime = 25

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
        queue.async { [painter] in painter.dropFont() }
    }

    private func request(_ emoji: String, then: (@MainActor (CGImage) -> Void)?) {
        if waiting[emoji] != nil {
            if let then { waiting[emoji]?.append(then) }
            return
        }
        waiting[emoji] = then.map { [$0] } ?? []
        queue.async { [painter, weak self] in
            let (image, newFont) = painter.draw(emoji)
            let picture = image.map(Picture.init)
            DispatchQueue.main.async {
                MainActor.assumeIsolated { self?.finish(emoji, picture, newFont: newFont) }
            }
        }
    }

    private func finish(_ emoji: String, _ picture: Picture?, newFont: Bool) {
        drawn += 1
        if newFont { fontsMade += 1 }
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
    private var font: CTFont?
    private var drawsWithFont = 0
    private static let colorSpace = CGColorSpace(name: CGColorSpace.displayP3) ?? CGColorSpaceCreateDeviceRGB()

    init(side: Int) {
        self.side = side
        let system = CTFontCreateWithName("AppleColorEmoji" as CFString, CGFloat(side), nil)
        fontData = (CTFontCopyAttribute(system, kCTFontURLAttribute) as? URL)
            .flatMap { try? Data(contentsOf: $0, options: .alwaysMapped) }
    }

    func dropFont() {
        font = nil
    }

    /// The image, and whether a new font was made for it.
    func draw(_ emoji: String) -> (image: CGImage?, newFont: Bool) {
        if drawsWithFont >= EmojiRenderer.fontLifetime { font = nil }
        let newFont = font == nil
        if newFont {
            font = makeFont()
            drawsWithFont = 0
        }
        drawsWithFont += 1
        guard let font, let context = CGContext(
            data: nil, width: side, height: side, bitsPerComponent: 8, bytesPerRow: 0, space: Self.colorSpace,
            bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)
        else { return (nil, newFont) }
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
        return (context.makeImage(), newFont)
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

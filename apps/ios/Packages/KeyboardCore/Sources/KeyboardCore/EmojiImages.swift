import Compression
import CoreGraphics
import CoreText
import Foundation
import ImageIO

/// Emoji pictures read straight from the system's Apple Color Emoji font, one at a time, for
/// the emoji panel. Drawing emoji as text keeps a decoded copy of every emoji ever drawn in
/// CoreText's cache for the life of the process: scrolling the whole panel at 32 pt grew the
/// footprint by 60 MB (Simulator), and closing the panel gave none of it back. An image made
/// here belongs to whoever holds it, so only the cells on screen hold any.
///
/// The font's `sbix` table holds a picture per glyph at a few sizes (strikes): Apple's `emjc`
/// images on iOS, PNGs on macOS. `CTFontCopyTable` maps the table rather than copying it
/// (138 MB of table cost about 2 MB of footprint).
public final class EmojiImages {
    /// The pixel size of the strike images come from.
    public let strikeSize: Int
    private let font: CTFont
    private let table: Data
    private let strikeStart: Int
    private let glyphCount: Int
    /// Scratch space for `decodeEMJC`.
    private var raw: [UInt8] = []
    private var values: [Int32] = []

    /// Nil when the font or its `sbix` table is missing. `pixelSize` is how many pixels high
    /// an emoji is drawn: the smallest strike at least that big is used.
    public init?(pixelSize: Int) {
        let font = CTFontCreateWithName("AppleColorEmoji" as CFString, 32, nil)
        guard let data = CTFontCopyTable(font, CTFontTableTag(kCTFontTableSbix), []) as Data?,
              let strikes = Self.u32(data, 4), strikes > 0 else { return nil }
        var choices: [(size: Int, start: Int)] = []
        for index in 0..<strikes {
            guard let start = Self.u32(data, 8 + 4 * index), let size = Self.u16(data, start) else { return nil }
            choices.append((size, start))
        }
        let chosen = choices.filter { $0.size >= pixelSize }.min { $0.size < $1.size }
            ?? choices.max { $0.size < $1.size }!
        self.font = font
        table = data
        strikeSize = chosen.size
        strikeStart = chosen.start
        glyphCount = CTFontGetGlyphCount(font)
    }

    /// The picture for one emoji, or nil for text, several emoji, or one the font lacks.
    public func image(for emoji: String) -> CGImage? {
        guard !emoji.isEmpty, let glyph = glyph(for: emoji) else { return nil }
        return image(glyph: Int(glyph), depth: 0)
    }

    /// The font's one glyph for `emoji` (a sequence with tones or ZWJ is one ligature).
    private func glyph(for emoji: String) -> CGGlyph? {
        let text = NSAttributedString(string: emoji, attributes: [NSAttributedString.Key(kCTFontAttributeName as String): font])
        let line = CTLineCreateWithAttributedString(text)
        guard let runs = CTLineGetGlyphRuns(line) as? [CTRun], runs.count == 1, CTRunGetGlyphCount(runs[0]) == 1 else { return nil }
        let attributes = CTRunGetAttributes(runs[0]) as NSDictionary
        guard let used = attributes[kCTFontAttributeName as String],
              CFGetTypeID(used as CFTypeRef) == CTFontGetTypeID(),
              CTFontCopyPostScriptName(used as! CTFont) as String == "AppleColorEmoji" else { return nil }
        var glyph = CGGlyph()
        CTRunGetGlyphs(runs[0], CFRange(location: 0, length: 1), &glyph)
        return glyph
    }

    private func image(glyph: Int, depth: Int) -> CGImage? {
        guard glyph < glyphCount, depth < 4,
              let from = Self.u32(table, strikeStart + 4 + 4 * glyph),
              let to = Self.u32(table, strikeStart + 4 + 4 * (glyph + 1)),
              to - from > 8, strikeStart + to <= table.count else { return nil }
        let record = strikeStart + from
        let type = table.subdata(in: (record + 4)..<(record + 8))
        let body = (record + 8)..<(strikeStart + to)
        switch String(decoding: type, as: UTF8.self) {
        case "png ":
            guard let source = CGImageSourceCreateWithData(table.subdata(in: body) as CFData, nil) else { return nil }
            return CGImageSourceCreateImageAtIndex(source, 0, [kCGImageSourceShouldCacheImmediately: true] as CFDictionary)
        case "emjc":
            return Self.decodeEMJC(table[body], raw: &raw, values: &values)
        case "dupe":
            return Self.u16(table, body.lowerBound).flatMap { image(glyph: $0, depth: depth + 1) }
        default:
            return nil
        }
    }

    /// Decodes Apple's `emj1` image (iOS's emoji font has these, macOS's has PNGs). Ported
    /// from emjc.py in PoomSmart/EmojiFonts (MIT), itself after cc4966/emjc-decoder. After a
    /// 16-byte header (magic, version, ?, width, height, appendix length, all u16 little-endian
    /// after the magic), LZFSE-compressed: alpha per pixel, a filter per row, three zigzagged
    /// residuals per pixel in a (base, p, q) colour space, and an appendix of larger offsets.
    public static func decodeEMJC(_ data: Data) -> CGImage? {
        var raw: [UInt8] = [], values: [Int32] = []
        return decodeEMJC(data, raw: &raw, values: &values)
    }

    /// `raw` and `values` are scratch space, kept between calls so that scrolling the panel
    /// doesn't leave freed buffers dirty in malloc.
    private static func decodeEMJC(_ data: Data, raw: inout [UInt8], values: inout [Int32]) -> CGImage? {
        guard data.count > 16, data.prefix(4) == Data("emj1".utf8) else { return nil }
        let start = data.startIndex
        func le16(_ at: Int) -> Int { Int(data[start + at]) | Int(data[start + at + 1]) << 8 }
        let width = le16(8), height = le16(10), appendixLength = le16(12)
        let pixels = width * height, colors = pixels * 3
        let expected = pixels + height + colors + appendixLength
        guard width > 0, height > 0, width <= 1024, height <= 1024 else { return nil }
        if raw.count < expected { raw = [UInt8](repeating: 0, count: expected) }
        let written = data.withUnsafeBytes { (source: UnsafeRawBufferPointer) in
            raw.withUnsafeMutableBufferPointer { target in
                compression_decode_buffer(
                    target.baseAddress!, expected, source.baseAddress!.assumingMemoryBound(to: UInt8.self) + 16,
                    source.count - 16, nil, COMPRESSION_LZFSE)
            }
        }
        guard written == expected else { return nil }
        let rgb = pixels + height, appendix = rgb + colors

        if values.count < colors { values = [Int32](repeating: 0, count: colors) }
        for index in 0..<colors { values[index] = 0 }
        var position = 0
        for index in 0..<appendixLength {
            position += Int(raw[appendix + index]) / 4
            if position >= colors { break }
            values[position] = 128 * Int32(raw[appendix + index] % 4)
            position += 1
        }
        // A bitmap context's image left more behind after scrolling the panel (Simulator).
        let bytesPerRow = width * 4
        let output = NSMutableData(length: pixels * 4)!
        let out = output.mutableBytes.bindMemory(to: UInt8.self, capacity: pixels * 4)
        for y in 0..<height {
            let filter = raw[pixels + y]
            for x in 0..<width {
                let i = y * width + x, c = i * 3, o = y * bytesPerRow + x * 4
                for k in 0..<3 {
                    let encoded = Int32(raw[rgb + c + k]), offset = values[c + k]
                    values[c + k] = encoded & 1 == 1 ? -(encoded >> 1) - offset : (encoded >> 1) + offset
                }
                let left = c - 3, up = c - width * 3
                var from: Int? // the neighbour added whole, for filters 1-3
                switch filter {
                case 1 where x > 0 && y > 0:
                    let leftUp = up - 3
                    from = abs(values[left] - values[leftUp]) < abs(values[up] - values[leftUp]) ? up : left
                case 1, 4:
                    from = x > 0 ? left : y > 0 ? up : nil
                case 2:
                    from = x > 0 ? left : nil
                case 3:
                    from = y > 0 ? up : nil
                default:
                    break
                }
                if filter == 4, x > 0, y > 0 {
                    for k in 0..<3 {
                        let sum = values[left + k] + values[up + k] + 1
                        values[c + k] += sum < 0 ? -((-sum) / 2) : sum / 2
                    }
                } else if let from {
                    for k in 0..<3 { values[c + k] += values[from + k] }
                }
                let base = values[c], p = values[c + 1], q = values[c + 2]
                let r = base + (p < 0 ? floorHalf(p) : floorHalf(p + 1)) - (q < 0 ? floorHalf(q + 1) : floorHalf(q))
                let g = base + (q < 0 ? floorHalf(q) : floorHalf(q + 1))
                let b = base - (p < 0 ? floorHalf(p + 1) : floorHalf(p)) - (q < 0 ? floorHalf(q + 1) : floorHalf(q))
                out[o] = UInt8(truncatingIfNeeded: modulo257(b))
                out[o + 1] = UInt8(truncatingIfNeeded: modulo257(g))
                out[o + 2] = UInt8(truncatingIfNeeded: modulo257(r))
                out[o + 3] = raw[i]
            }
        }
        guard let provider = CGDataProvider(data: output) else { return nil }
        // BGRA in memory, premultiplied, as the font's pictures are.
        return CGImage(
            width: width, height: height, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: bytesPerRow,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue),
            provider: provider, decode: nil, shouldInterpolate: true, intent: .defaultIntent
        )
    }

    /// Python's `//` by 2: rounds down, also for negative numbers.
    private static func floorHalf(_ value: Int32) -> Int32 {
        value >= 0 ? value / 2 : -((-value + 1) / 2)
    }

    /// Python's `%` by 257: never negative.
    private static func modulo257(_ value: Int32) -> Int32 {
        ((value % 257) + 257) % 257
    }

    private static func u16(_ data: Data, _ offset: Int) -> Int? {
        guard offset >= 0, offset + 2 <= data.count else { return nil }
        return data.withUnsafeBytes { Int(UInt16(bigEndian: $0.loadUnaligned(fromByteOffset: offset, as: UInt16.self))) }
    }

    private static func u32(_ data: Data, _ offset: Int) -> Int? {
        guard offset >= 0, offset + 4 <= data.count else { return nil }
        return data.withUnsafeBytes { Int(UInt32(bigEndian: $0.loadUnaligned(fromByteOffset: offset, as: UInt32.self))) }
    }
}

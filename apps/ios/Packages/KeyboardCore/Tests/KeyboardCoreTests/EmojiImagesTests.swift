import Testing
import Compression
import CoreGraphics
import Foundation
import KeyboardCore

@Test func emojiImagesComeFromTheSmallestStrikeThatIsSharp() throws {
    let images = try #require(EmojiImages(pixelSize: 90))
    #expect(images.strikeSize >= 90)
    let grin = try #require(images.image(for: "😀"))
    #expect(grin.width >= 80 && grin.width <= images.strikeSize + 8)
    let small = try #require(EmojiImages(pixelSize: 30))
    #expect(small.strikeSize < images.strikeSize)
}

@Test func sequencesTonesAndFlagsAreOneImage() throws {
    let images = try #require(EmojiImages(pixelSize: 90))
    for emoji in ["👋🏽", "🧑🏻‍🤝‍🧑🏻", "🇮🇳", "❤️", "#️⃣", "🏳️‍🌈"] {
        #expect(images.image(for: emoji) != nil, "\(emoji)")
    }
}

/// An `emj1` image as iOS's emoji font stores it: alpha, a filter per row, then zigzagged
/// residuals of a (base, p, q) colour transform, all LZFSE-compressed.
private func emjc(width: Int, height: Int, alpha: [UInt8], filters: [UInt8], residuals: [UInt8]) -> Data {
    let raw = alpha + filters + residuals
    var compressed = [UInt8](repeating: 0, count: raw.count + 1024)
    let size = compression_encode_buffer(&compressed, compressed.count, raw, raw.count, nil, COMPRESSION_LZFSE)
    var header = Data("emj1".utf8)
    for value in [0, 0xA101, width, height, 0, 0] {
        header.append(contentsOf: [UInt8(value & 0xFF), UInt8(value >> 8)])
    }
    return header + Data(compressed.prefix(size))
}

private func pixels(_ image: CGImage) -> [UInt8] {
    var out = [UInt8](repeating: 0, count: image.width * image.height * 4)
    let context = CGContext(data: &out, width: image.width, height: image.height, bitsPerComponent: 8,
                            bytesPerRow: image.width * 4, space: CGColorSpaceCreateDeviceRGB(),
                            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    context.draw(image, in: CGRect(x: 0, y: 0, width: image.width, height: image.height))
    return out
}

@Test func emjcImagesDecodeToTheirColours() throws {
    // R 110 G 100 B 90: p = R - B = 20, q = 0, base 100; zigzag doubles each. The second
    // pixel's row uses filter 2 (left), so residuals of 0 repeat the first pixel.
    let data = emjc(width: 2, height: 1, alpha: [255, 255], filters: [2], residuals: [200, 40, 0, 0, 0, 0])
    let image = try #require(EmojiImages.decodeEMJC(data))
    #expect(image.width == 2 && image.height == 1)
    #expect(pixels(image) == [110, 100, 90, 255, 110, 100, 90, 255])
    #expect(EmojiImages.decodeEMJC(Data("nope".utf8)) == nil)
}

@Test func textIsNoEmojiImage() throws {
    let images = try #require(EmojiImages(pixelSize: 90))
    #expect(images.image(for: "a") == nil)
    #expect(images.image(for: "😀😀") == nil, "one emoji only")
    #expect(images.image(for: "") == nil)
}

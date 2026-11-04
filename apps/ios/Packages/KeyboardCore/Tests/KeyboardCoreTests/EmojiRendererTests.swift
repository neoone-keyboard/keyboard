import CoreGraphics
import Foundation
import Testing
@testable import KeyboardCore

/// The alpha of the pixel at (`x`, `y`) from the top left.
private func alpha(_ image: CGImage, _ x: Int, _ y: Int) -> UInt8 {
    var pixel: [UInt8] = [0, 0, 0, 0]
    let context = CGContext(data: &pixel, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
                            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    context.draw(image, in: CGRect(x: -x, y: y - image.height + 1, width: image.width, height: image.height))
    return pixel[3]
}

@Test func drawsTheEmojiCentredInItsSquare() throws {
    let image = try #require(EmojiRenderer.draw("😀", side: 96))
    #expect(image.width == 96 && image.height == 96)
    #expect(alpha(image, 48, 48) == 255)
    // A round face leaves the corners empty and nearly fills the square, centred across.
    #expect(alpha(image, 1, 1) == 0)
    #expect(alpha(image, 94, 94) == 0)
    let across = (0..<96).filter { alpha(image, $0, 48) > 0 }
    let down = (0..<96).filter { alpha(image, 48, $0) > 0 }
    let left = try #require(across.first), right = try #require(across.last)
    #expect(abs(left - (95 - right)) <= 1)
    #expect(right - left > 80 && (down.last ?? 0) - (down.first ?? 0) > 80)
}

@Test func flagsAndSequencesAreOnePicture() throws {
    for emoji in ["🇮🇳", "👨‍👩‍👧‍👦", "1️⃣", "🏳️‍🌈"] {
        let image = try #require(EmojiRenderer.draw(emoji, side: 64))
        #expect(alpha(image, 32, 32) > 0, "\(emoji)")
    }
}

@MainActor
@Test func drawsWithItsOwnCopyOfTheFontAndKeepsImages() async {
    let renderer = EmojiRenderer(pixelSide: 64)
    #expect(renderer.usesPrivateFont)
    #expect(renderer.image(for: "🍕") { _ in } == nil)
    let image = await withCheckedContinuation { done in
        _ = renderer.image(for: "🐘") { done.resume(returning: $0) }
    }
    #expect(image.width == 64)
    #expect(renderer.image(for: "🐘") { _ in } === image)
    #expect(renderer.drawn >= 1 && renderer.fontsMade == 1)
    renderer.empty()
    #expect(renderer.image(for: "🐘") { _ in } == nil)
}

/// The point of the private font: a fresh one every `fontLifetime` emoji, so its cache of
/// drawn emoji goes with the old one. (The memory itself is measured in the Simulator: macOS
/// doesn't keep them either way.)
@MainActor
@Test func replacesItsFontAfterEachLifetimeOfEmoji() async throws {
    let scalars = (0x1F400...0x1F4FF).compactMap(Unicode.Scalar.init).filter(\.properties.isEmojiPresentation)
    let emoji = scalars.map { String(Character($0)) }
    try #require(emoji.count > 4 * EmojiRenderer.fontLifetime)
    let renderer = EmojiRenderer(pixelSide: 32)
    for each in emoji {
        _ = await withCheckedContinuation { done in
            if let image = renderer.image(for: each, then: { done.resume(returning: $0) }) { done.resume(returning: image) }
        }
    }
    #expect(renderer.drawn == emoji.count)
    #expect(renderer.fontsMade == (emoji.count + EmojiRenderer.fontLifetime - 1) / EmojiRenderer.fontLifetime)
}

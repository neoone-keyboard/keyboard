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

@Test func drawsWhatIsOnScreenFirstThenWhatIsAhead() {
    let batch = EmojiRenderer.nextBatch(onScreen: ["b", "c", "x"], asked: ["a", "b", "c", "d"], waiting: ["a", "b", "c", "d"],
                                        ahead: ["e", "c"], limit: 5)
    // "x" is on screen but drawn already; "d" was asked for after "a", so it goes first of those.
    #expect(batch == ["b", "c", "e", "d", "a"])
    #expect(EmojiRenderer.nextBatch(onScreen: ["b"], asked: ["a", "b"], waiting: ["a", "b"], ahead: [], limit: 1) == ["b"])
    // Without a screen to go by (the search's row): in the order asked.
    #expect(EmojiRenderer.nextBatch(onScreen: nil, asked: ["a", "b"], waiting: ["a", "b"], ahead: ["e"], limit: 5)
            == ["a", "b", "e"])
}

/// One at a time: the last test counts the private fonts left in the process.
@Suite(.serialized)
struct EmojiRendererTests {
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
    /// drawn emoji goes with the old one. (The memory itself is measured in the Simulator:
    /// macOS doesn't keep them either way.)
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
        renderer.close()
    }

    /// Each private font is a CGFont over the mapped file, counted until CoreText frees it:
    /// once closed, none is left.
    @MainActor
    @Test func closingLetsGoOfEveryPrivateFont() async throws {
        let renderer = EmojiRenderer(pixelSide: 64)
        var lines: [String] = []
        let (closed, closing) = AsyncStream<Void>.makeStream()
        renderer.onLog = { line in
            lines.append(line)
            if line.hasPrefix("Emoji renderer closed") { closing.yield() }
        }
        for each in ["🐘", "🍕", "🇮🇳"] {
            _ = await withCheckedContinuation { done in
                if let image = renderer.image(for: each, then: { done.resume(returning: $0) }) { done.resume(returning: image) }
            }
        }
        #expect(EmojiRenderer.fontCopiesAlive >= 1)
        renderer.close()
        var waited = closed.makeAsyncIterator()
        _ = await waited.next()
        // The earlier tests' renderers end their threads as they go.
        for _ in 0..<100 where EmojiRenderer.fontCopiesAlive > 0 { try await Task.sleep(for: .milliseconds(10)) }
        #expect(EmojiRenderer.fontCopiesAlive == 0)
        #expect(lines.first?.contains("mapped (mmap") == true)
        #expect(!lines.contains { $0.hasPrefix("Emoji font 1 made in") })
        #expect(lines.contains { $0.hasPrefix("Emoji timings: 2 drawn") && $0.contains("1 in a font's first batch") && $0.contains("1 fonts made") })
        #expect(lines.contains { $0.hasPrefix("Emoji renderer closed: Emoji font 1 dropped") })
        #expect(renderer.image(for: "🐘") { _ in } == nil)
    }
}

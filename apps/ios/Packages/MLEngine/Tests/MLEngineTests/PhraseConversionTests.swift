import Foundation
import Testing
import MLEngine

/// packs/ja.neopack, found from this file's path (…/apps/ios/Packages/MLEngine/Tests/MLEngineTests/).
private let japanesePack = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent()
    .appending(path: "packs/ja.neopack")

/// The Japanese pack through the FFI, as the keyboard calls it: compose, convert, and convert
/// with the first segment moved, counted in characters.
@Test func japaneseComposesAndConvertsThroughTheFFI() throws {
    let engine = try Engine(config: EngineConfig(packDirs: [japanesePack.path]))
    #expect(try engine.packProblems().isEmpty)
    #expect(try engine.conversionLanguages() == ["ja"])
    #expect(try engine.layouts() == [.latin], "not a language of the word engine")

    let typing = try #require(try engine.compose(language: "ja", typed: "kyouhaiitenkidesun", finish: false))
    #expect(typing.kana == "きょうはいいてんきです")
    #expect(typing.pending == "n")
    let kana = try #require(try engine.compose(language: "ja", typed: "kyouhaiitenkidesune", finish: true)).kana
    #expect(kana == "きょうはいいてんきですね")

    let conversion = try #require(try engine.convert(language: "ja", kana: kana, limit: 30))
    #expect(conversion.candidates.first == "今日はいい天気ですね")
    #expect(conversion.candidates.contains("キョウハイイテンキデスネ"))
    #expect(conversion.segments.map(\.reading).joined() == kana)
    #expect(conversion.segments.first?.reading == "きょうは")
    #expect(conversion.first.first == "今日は")

    let shorter = try #require(try engine.convertWithFirst(language: "ja", kana: kana, firstLength: 3, limit: 30))
    #expect(shorter.segments.first?.reading == "きょう")
    #expect(shorter.first.first == "今日")
    #expect(shorter.segments.map(\.reading).joined() == kana)
    #expect(try engine.convertWithFirst(language: "ja", kana: kana, firstLength: 0, limit: 30) == nil)
    #expect(try engine.convertWithFirst(language: "ja", kana: kana, firstLength: 13, limit: 30) == nil)
    #expect(try engine.convert(language: "en", kana: kana, limit: 30) == nil)
}

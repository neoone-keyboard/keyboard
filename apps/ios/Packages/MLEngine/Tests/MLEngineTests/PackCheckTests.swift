import Foundation
import Testing
import MLEngine

/// What the app's installer asks the engine before a pack goes where the keyboard looks
/// (`packOpenProblem`): the pack is opened as the keyboard opens it.

private let repository = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent()

/// The engine's small Gujarati test pack.
private let gujarati = repository.appending(path: "engine/core/tests/fixtures/gu.neopack")

/// A copy of the Gujarati test pack in a folder named as the installer's working one, with
/// the files of `replacing` in place of its own. The manifest's checksums are the
/// installer's to check, not the engine's.
private func incomingPack(replacing: [String: Data] = [:]) throws -> URL {
    let root = FileManager.default.temporaryDirectory.appending(path: "pack-check-\(UUID().uuidString)", directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
    let folder = root.appending(path: ".incoming-\(UUID().uuidString)", directoryHint: .isDirectory)
    try FileManager.default.copyItem(at: gujarati, to: folder)
    for (name, data) in replacing {
        try data.write(to: folder.appending(path: name))
    }
    return folder
}

private func cleanUp(_ folder: URL) {
    try? FileManager.default.removeItem(at: folder.deletingLastPathComponent())
}

@Test func aPackTheEngineOpensHasNoProblem() throws {
    let folder = try incomingPack()
    defer { cleanUp(folder) }
    #expect(try packOpenProblem(packDir: folder.path) == nil)
    #expect(try packOpenProblem(packDir: repository.appending(path: "packs/ml.neopack").path) == nil)
}

/// The lexicon file is there, so `packProblem` alone finds nothing; opening it does.
@Test func aLexiconThatIsNotOneIsAProblem() throws {
    let folder = try incomingPack(replacing: ["gu.fst": Data("not an fst".utf8)])
    defer { cleanUp(folder) }
    #expect(try packProblem(packDir: folder.path) == nil)
    let why = try #require(try packOpenProblem(packDir: folder.path))
    #expect(why.hasPrefix("lexicon unreadable: "), "\(why)")
    #expect(!why.contains(".incoming-"), "the working folder isn't named: \(why)")
}

/// A lexicon whose bytes changed: the FST's own checksum fails.
@Test func aDamagedLexiconIsAProblem() throws {
    var bytes = try Data(contentsOf: gujarati.appending(path: "gu.fst"))
    bytes[bytes.count / 2] ^= 0xFF
    let folder = try incomingPack(replacing: ["gu.fst": bytes])
    defer { cleanUp(folder) }
    let why = try #require(try packOpenProblem(packDir: folder.path))
    #expect(why.hasPrefix("lexicon unreadable: "), "\(why)")
}

/// What `packProblem` finds is still answered, in its words.
@Test func whatTheManifestCheckFindsIsStillTheProblem() throws {
    let folder = try incomingPack(replacing: ["lang.json": Data("{}".utf8)])
    defer { cleanUp(folder) }
    let why = try #require(try packOpenProblem(packDir: folder.path))
    #expect(why == (try packProblem(packDir: folder.path)))
    #expect(why.hasPrefix("script rules: "), "\(why)")
}

/// An optional file the engine can't read doesn't refuse the pack: the keyboard uses the rest.
@Test func anUnreadableOptionalFileIsNotAProblem() throws {
    let folder = try incomingPack(replacing: ["next.fst": Data("not an fst".utf8)])
    defer { cleanUp(folder) }
    #expect(try packOpenProblem(packDir: folder.path) == nil)
}

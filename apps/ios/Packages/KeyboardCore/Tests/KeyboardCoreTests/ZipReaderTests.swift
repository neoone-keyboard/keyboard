import Foundation
import Testing
@testable import KeyboardCore

private func reader(_ zip: TestZip) throws -> ZipReader {
    let folder = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let url = folder.appending(path: "test.zip")
    try zip.write(to: url)
    return try ZipReader(url: url)
}

private func refusal(_ body: () throws -> Void) -> ZipReader.Failure? {
    do {
        try body()
        return nil
    } catch let failure as ZipReader.Failure {
        return failure
    } catch {
        return nil
    }
}

@Test func crc32MatchesTheReferenceValue() {
    #expect(CRC32.checksum(Data("123456789".utf8)) == 0xCBF4_3926)
    #expect(CRC32.checksum(Data()) == 0)
}

@Test func readsStoredEntries() throws {
    var zip = TestZip()
    zip.add("ml.neopack/", Data())
    zip.add("ml.neopack/manifest.json", "{}")
    zip.add("ml.neopack/ml.fst", Data([1, 2, 3]))
    let r = try reader(zip)
    #expect(r.entries.map(\.name) == ["ml.neopack/manifest.json", "ml.neopack/ml.fst"])
    #expect(r.entries[1].components == ["ml.neopack", "ml.fst"])
    #expect(try r.contents(of: r.entries[0]) == Data("{}".utf8))
    #expect(try r.contents(of: r.entries[1]) == Data([1, 2, 3]))
}

@Test func readsDeflatedEntriesFromTheSystemZip() throws {
    let folder = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let text = String(repeating: "the quick brown fox jumps over the lazy dog\n", count: 2000)
    try FileManager.default.createDirectory(at: folder.appending(path: "p.neopack"), withIntermediateDirectories: true)
    try Data(text.utf8).write(to: folder.appending(path: "p.neopack/words.txt"))
    let zip = Process()
    zip.executableURL = URL(fileURLWithPath: "/usr/bin/zip")
    zip.currentDirectoryURL = folder
    zip.arguments = ["-q", "-r", "p.zip", "p.neopack"]
    try zip.run()
    zip.waitUntilExit()
    #expect(zip.terminationStatus == 0)
    let r = try ZipReader(url: folder.appending(path: "p.zip"))
    let entry = try #require(r.entries.first { $0.name == "p.neopack/words.txt" })
    #expect(entry.method == 8, "zip deflates text")
    #expect(try r.contents(of: entry) == Data(text.utf8))
}

@Test func refusesWhatIsNotAZip() throws {
    let folder = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let url = folder.appending(path: "x.zip")
    try Data("not a zip at all".utf8).write(to: url)
    guard case .notAZip = refusal({ _ = try ZipReader(url: url) }) else {
        Issue.record("a text file is not a zip")
        return
    }
}

@Test func refusesNamesThatLeaveTheFolder() throws {
    for name in ["../evil", "/etc/passwd", "a/../../b", "a\\b", "a//b", "."] {
        var zip = TestZip()
        zip.add(name, "x")
        guard case .unsafeName = refusal({ _ = try reader(zip) }) else {
            Issue.record("\(name) should be refused")
            continue
        }
    }
}

@Test func refusesEncryptedEntries() throws {
    var zip = TestZip()
    zip.add("a", Data("x".utf8), flags: 1)
    guard case .unsupported = refusal({ _ = try reader(zip) }) else {
        Issue.record("an encrypted entry should be refused")
        return
    }
}

@Test func aBadCrcIsFoundWhenTheEntryIsRead() throws {
    var zip = TestZip()
    zip.add("a", Data("hello".utf8), crc: 1234)
    let r = try reader(zip)
    guard case .corrupt = refusal({ _ = try r.contents(of: r.entries[0]) }) else {
        Issue.record("a bad CRC should be refused")
        return
    }
}

@Test func aTruncatedArchiveIsRefused() throws {
    var zip = TestZip()
    zip.add("a", Data(repeating: 7, count: 1000))
    let folder = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let url = folder.appending(path: "cut.zip")
    // Cut inside the entry's data: the directory at the end is gone too.
    try zip.data.prefix(500).write(to: url)
    #expect(refusal({ _ = try ZipReader(url: url) }) != nil)
}

@Test func refusesMoreThanTheSizeLimit() throws {
    var zip = TestZip()
    zip.add("a", Data(repeating: 0, count: 100))
    let folder = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: folder) }
    let url = folder.appending(path: "big.zip")
    try zip.write(to: url)
    guard case .tooLarge = refusal({ _ = try ZipReader(url: url, maxTotalSize: 99) }) else {
        Issue.record("past the limit should be refused")
        return
    }
}

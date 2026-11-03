import Foundation
import Testing
import KeyboardCore

private func temporaryDirectory() -> URL {
    FileManager.default.temporaryDirectory.appending(path: "LineLogTests-\(UUID().uuidString)")
}

private func lines(at url: URL) -> [String] {
    guard let data = try? Data(contentsOf: url) else { return [] }
    return String(decoding: data, as: UTF8.self).split(separator: "\n").map(String.init)
}

@Test func appendsTimestampedLinesAndCreatesTheDirectory() throws {
    let dir = temporaryDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let log = LineLog(url: dir.appending(path: "nested/diagnostics.log"))
    log.append("first")
    log.append("second\nline")
    log.flush()
    let written = lines(at: log.url)
    #expect(written.count == 2)
    #expect(written[0].hasSuffix(" first"))
    #expect(written[1].hasSuffix(" second line"), "one event, one line")
    #expect(written[0].first == "2", "starts with an ISO 8601 year")
}

@Test func rollsOverSoTheFilesStayBounded() throws {
    let dir = temporaryDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let log = LineLog(url: dir.appending(path: "diagnostics.log"), maxBytes: 300)
    for i in 0..<100 { log.append("event \(i)") }
    log.flush()
    let current = try Data(contentsOf: log.url).count
    let previous = try Data(contentsOf: log.previousURL).count
    #expect(current <= 300)
    #expect(previous <= 300)
    #expect(log.previousURL.lastPathComponent == "diagnostics.1.log")
    #expect(lines(at: log.url).last?.hasSuffix(" event 99") == true)
    #expect(lines(at: log.previousURL).contains { $0.hasSuffix(" event 0") } == false, "oldest dropped")
}

@Test func recentLinesSpanBothFilesNewestLast() {
    let dir = temporaryDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    let log = LineLog(url: dir.appending(path: "diagnostics.log"), maxBytes: 300)
    // 34-byte lines: 8 per file, so after 20 the files hold events 8...15 and 16...19.
    for i in 0..<20 { log.append("event " + String(format: "%02d", i)) }
    let recent = log.recentLines(20)
    #expect(recent.count == 12)
    #expect(recent.first?.hasSuffix(" event 08") == true)
    #expect(recent.last?.hasSuffix(" event 19") == true)
    #expect(log.recentLines(3).first?.hasSuffix(" event 17") == true)
    let small = log.recentLines(12, maxBytes: 100)
    #expect(small.reduce(0) { $0 + $1.utf8.count + 1 } <= 100)
    #expect(small.last?.hasSuffix(" event 19") == true)
}

@Test func anUnwritablePathDropsLinesWithoutFailing() throws {
    let dir = temporaryDirectory()
    defer { try? FileManager.default.removeItem(at: dir) }
    try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
    let blocker = dir.appending(path: "file")
    try Data("x".utf8).write(to: blocker)
    // The log's directory would have to be inside a regular file.
    let log = LineLog(url: blocker.appending(path: "diagnostics.log"))
    log.append("lost")
    log.flush()
    #expect(log.recentLines(5).isEmpty)
}

import Foundation

/// A text file of timestamped lines that stays small: once it reaches `maxBytes` it becomes
/// `previousURL` (replacing the older one) and a new file starts, so the two together hold
/// at most about `2 × maxBytes`.
///
/// `append` only formats the time and hands the line to a background queue, so the caller
/// never waits for the disk. An I/O error drops that line and nothing else: the file is
/// reopened on the next one.
public final class LineLog: @unchecked Sendable {
    public let url: URL
    public let previousURL: URL
    private let maxBytes: Int
    private let queue = DispatchQueue(label: "me.jishnu.keyboard.line-log", qos: .utility)
    // Only touched on `queue`.
    private var handle: FileHandle?
    private var size = 0

    private static let timeStyle = Date.ISO8601FormatStyle(includingFractionalSeconds: true)

    public init(url: URL, maxBytes: Int = 200_000) {
        self.url = url
        self.maxBytes = maxBytes
        let ext = url.pathExtension
        let base = url.deletingPathExtension().lastPathComponent
        previousURL = url.deletingLastPathComponent()
            .appending(path: ext.isEmpty ? "\(base).1" : "\(base).1.\(ext)")
    }

    deinit {
        try? handle?.close()
    }

    /// One line, prefixed with the current time. Line breaks inside `message` become spaces.
    public func append(_ message: String, at date: Date = Date()) {
        let line = date.formatted(Self.timeStyle) + " "
            + message.replacingOccurrences(of: "\n", with: " ") + "\n"
        queue.async { [self] in write(Data(line.utf8)) }
    }

    /// Waits until every line appended so far has been written (or dropped).
    public func flush() {
        queue.sync {}
    }

    /// The newest lines, oldest first: at most `count` of them and at most `maxBytes` of
    /// text in all (dropping the oldest first). Waits for pending writes.
    public func recentLines(_ count: Int, maxBytes: Int = .max) -> [String] {
        queue.sync {
            var lines: [String] = []
            for file in [url, previousURL] where lines.count < count {
                guard let data = try? Data(contentsOf: file) else { continue }
                let fileLines = String(decoding: data, as: UTF8.self)
                    .split(separator: "\n", omittingEmptySubsequences: true)
                lines.insert(contentsOf: fileLines.suffix(count - lines.count).map(String.init), at: 0)
            }
            var total = 0
            var kept = 0
            for line in lines.reversed() {
                total += line.utf8.count + 1
                if total > maxBytes { break }
                kept += 1
            }
            return Array(lines.suffix(kept))
        }
    }

    private func write(_ data: Data) {
        do {
            if size + data.count > maxBytes, size > 0 { try roll() }
            let handle = try openHandle()
            try handle.write(contentsOf: data)
            size += data.count
        } catch {
            try? handle?.close()
            handle = nil
        }
    }

    private func openHandle() throws -> FileHandle {
        if let handle { return handle }
        let files = FileManager.default
        try files.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        if !files.fileExists(atPath: url.path) {
            guard files.createFile(atPath: url.path, contents: nil) else {
                throw CocoaError(.fileWriteUnknown)
            }
        }
        let opened = try FileHandle(forWritingTo: url)
        size = Int(try opened.seekToEnd())
        handle = opened
        return opened
    }

    private func roll() throws {
        try? handle?.close()
        handle = nil
        let files = FileManager.default
        try? files.removeItem(at: previousURL)
        try files.moveItem(at: url, to: previousURL)
        size = 0
    }
}

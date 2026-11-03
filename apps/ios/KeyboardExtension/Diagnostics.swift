import Foundation
import KeyboardCore
import os
import UIKit

/// The extension's events, to the unified log and to `Library/Caches/diagnostics.log` in the
/// extension's own container (readable with `xcrun devicectl device copy from … --domain-type
/// appDataContainer --domain-identifier me.jishnu.keyboard.extension`, or typed into a field
/// from the settings panel). Messages must never contain typed text: lengths and kinds only.
struct Diagnostics: Sendable {
    static let file: LineLog? = FileManager.default
        .urls(for: .cachesDirectory, in: .userDomainMask).first
        .map { LineLog(url: $0.appending(path: "diagnostics.log")) }

    private let logger: Logger
    private let category: String

    init(category: String) {
        logger = Logger(subsystem: "me.jishnu.keyboard.extension", category: category)
        self.category = category
    }

    func info(_ message: String) {
        logger.info("\(message, privacy: .public)")
        Self.file?.append("\(category) \(message)")
    }

    func warning(_ message: String) {
        logger.warning("\(message, privacy: .public)")
        Self.file?.append("\(category) WARN \(message)")
    }

    func error(_ message: String) {
        logger.error("\(message, privacy: .public)")
        Self.file?.append("\(category) ERROR \(message)")
    }

    /// "31.4 MB"
    static func megabytes(_ bytes: UInt64) -> String {
        String(format: "%.1f MB", Double(bytes) / 1_048_576)
    }

    static func footprint() -> String {
        megabytes(MemoryMonitor.footprintBytes())
    }

    static func milliseconds(_ duration: Duration) -> String {
        let (seconds, attoseconds) = duration.components
        return String(format: "%.1f ms", Double(seconds) * 1000 + Double(attoseconds) / 1e15)
    }

    /// Version, build, the commit it was built from (when `make` passed it) and when the
    /// extension binary was built.
    static var buildDescription: String {
        let info = Bundle.main.infoDictionary ?? [:]
        let version = info["CFBundleShortVersionString"] as? String ?? "?"
        let build = info["CFBundleVersion"] as? String ?? "?"
        let commit = (info["MLGitCommit"] as? String).flatMap { $0.isEmpty ? nil : $0 } ?? "unknown"
        var built = "?"
        if let path = Bundle.main.executablePath,
           let date = (try? FileManager.default.attributesOfItem(atPath: path))?[.modificationDate] as? Date {
            built = date.formatted(.iso8601)
        }
        return "v\(version) (\(build)) commit \(commit) built \(built)"
    }

    /// A report to type into a field: one header line, then the newest log lines, at most
    /// `maxBytes` in all.
    static func report(header: String, maxLines: Int = 60, maxBytes: Int = 6_000) -> String {
        let head = "MLKB diagnostics: \(header)\n"
        // Time of day only ("12:34:56.789Z"): the header has the date.
        let lines = (file?.recentLines(maxLines * 2) ?? []).map { line in
            line.count > 11 && line.dropFirst(10).first == "T" ? String(line.dropFirst(11)) : line
        }
        var kept: [String] = []
        var size = head.utf8.count
        for line in lines.reversed() {
            size += line.utf8.count + 1
            if size > maxBytes || kept.count == maxLines { break }
            kept.append(line)
        }
        return head + kept.reversed().joined(separator: "\n") + "\n"
    }
}

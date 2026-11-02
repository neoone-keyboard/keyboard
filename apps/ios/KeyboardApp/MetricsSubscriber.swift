import Foundation
import MetricKit

/// Saves MetricKit crash, hang and memory reports to disk. Nothing is uploaded (spec §4 rule 6).
/// Holds no mutable state, so sharing it across threads is safe.
final class MetricsSubscriber: NSObject, MXMetricManagerSubscriber, @unchecked Sendable {
    static let shared = MetricsSubscriber()

    static var reportsDirectory: URL {
        URL.applicationSupportDirectory.appending(path: "metrickit", directoryHint: .isDirectory)
    }

    static func reportCount() -> Int {
        (try? FileManager.default.contentsOfDirectory(atPath: reportsDirectory.path()).count) ?? 0
    }

    func start() {
        MXMetricManager.shared.add(self)
    }

    func didReceive(_ payloads: [MXDiagnosticPayload]) {
        payloads.forEach { save($0.jsonRepresentation(), kind: "diagnostic") }
    }

    func didReceive(_ payloads: [MXMetricPayload]) {
        payloads.forEach { save($0.jsonRepresentation(), kind: "metric") }
    }

    private func save(_ json: Data, kind: String) {
        var directory = Self.reportsDirectory
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? directory.setResourceValues(values)
        let name = "\(kind)-\(Int(Date().timeIntervalSince1970))-\(UUID().uuidString.prefix(8)).json"
        try? json.write(to: directory.appending(path: name))
    }
}

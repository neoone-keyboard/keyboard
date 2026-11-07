import Foundation

/// A pack manifest's `version` ("2026.11.06"), ordered part by part at the dots: numerically
/// where both parts are numbers, else as text; a missing part counts as "0". So
/// 2026.11.6 equals 2026.11.06, and both come before 2026.11.06.1.
public struct PackVersion: Comparable, Sendable, CustomStringConvertible {
    public let text: String

    public init(_ text: String) {
        self.text = text
    }

    public var description: String { text }

    private var parts: [Substring] { text.split(separator: ".", omittingEmptySubsequences: false) }

    /// -1, 0 or 1.
    private static func compare(_ a: PackVersion, _ b: PackVersion) -> Int {
        let (x, y) = (a.parts, b.parts)
        for i in 0 ..< max(x.count, y.count) {
            let p = i < x.count ? x[i] : "0"
            let q = i < y.count ? y[i] : "0"
            if let m = Int(p), let n = Int(q) {
                if m != n { return m < n ? -1 : 1 }
            } else if p != q {
                return p < q ? -1 : 1
            }
        }
        return 0
    }

    public static func < (a: PackVersion, b: PackVersion) -> Bool { compare(a, b) < 0 }
    public static func == (a: PackVersion, b: PackVersion) -> Bool { compare(a, b) == 0 }
}

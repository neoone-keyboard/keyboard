import CryptoKit
import Foundation

/// A pack index (`index.json`, M6 spec "Installing"): the packs a host offers for download.
///
///     {"format": 1, "packs": [{"id": "hi", "name": "Hindi", "nativeName": "हिन्दी",
///       "version": "2026.11.07", "script": "deva", "size": 12345678,
///       "sha256": "<of the .neopack file>", "url": "hi.neopack",
///       "englishFingerprint": "…", "minEngine": 2}]}
///
/// `url` may be relative to the index's own URL. `size` and `sha256` are of the `.neopack`
/// archive. `nativeName` and `englishFingerprint` may be left out. An entry that doesn't decode
/// is skipped; an index of another `format` is refused.
public struct PackIndex: Sendable, Equatable {
    public static let format = 1

    public struct Entry: Sendable, Equatable, Identifiable {
        public let id: String
        public let name: String
        public let nativeName: String?
        public let version: String
        public let script: String
        public let size: Int64
        public let sha256: String
        /// Absolute, resolved against the index's URL.
        public let url: URL
        public let englishFingerprint: String?
        public let minEngine: Int

        public init(id: String, name: String, nativeName: String? = nil, version: String, script: String,
                    size: Int64, sha256: String, url: URL, englishFingerprint: String? = nil, minEngine: Int) {
            self.id = id
            self.name = name
            self.nativeName = nativeName
            self.version = version
            self.script = script
            self.size = size
            self.sha256 = sha256
            self.url = url
            self.englishFingerprint = englishFingerprint
            self.minEngine = minEngine
        }

        /// What the update check (`PackUpdates.offers`) reads.
        public var available: AvailablePack {
            AvailablePack(id: id, version: version, script: script, minEngine: minEngine,
                          englishFingerprint: englishFingerprint)
        }

        /// Whether this build's engine reads the pack.
        public func isReadable(by support: PackSupport) -> Bool {
            minEngine <= support.engine && support.scripts.contains(script)
        }
    }

    public enum Refusal: Error, Equatable, CustomStringConvertible {
        case notJSON
        case format(Int)

        public var description: String {
            switch self {
            case .notJSON: "the pack index is not JSON of the expected shape"
            case .format(let v): "pack index format \(v), this app reads \(PackIndex.format)"
            }
        }
    }

    public let packs: [Entry]

    public init(packs: [Entry]) {
        self.packs = packs
    }

    /// The index in `data`, fetched from `base` (for relative pack URLs).
    public static func decode(_ data: Data, base: URL) throws -> PackIndex {
        guard let file = try? JSONDecoder().decode(File.self, from: data) else { throw Refusal.notJSON }
        guard file.format == format else { throw Refusal.format(file.format) }
        let entries = file.packs.compactMap { lenient -> Entry? in
            guard let raw = lenient.value, PackInstaller.allowedID(raw.id), raw.size > 0,
                  let url = URL(string: raw.url, relativeTo: base)?.absoluteURL else { return nil }
            return Entry(id: raw.id, name: raw.name, nativeName: raw.nativeName, version: raw.version,
                         script: raw.script, size: raw.size, sha256: raw.sha256.lowercased(), url: url,
                         englishFingerprint: raw.englishFingerprint, minEngine: raw.minEngine)
        }
        return PackIndex(packs: entries)
    }

    private struct File: Decodable {
        let format: Int
        let packs: [Lenient<RawEntry>]
    }

    private struct RawEntry: Decodable {
        let id: String
        let name: String
        let nativeName: String?
        let version: String
        let script: String
        let size: Int64
        let sha256: String
        let url: String
        let englishFingerprint: String?
        let minEngine: Int
    }

    /// One array element that may fail to decode without failing the array.
    private struct Lenient<T: Decodable>: Decodable {
        let value: T?
        init(from decoder: Decoder) throws {
            value = try? T(from: decoder)
        }
    }
}

/// Fetches the pack index and packs from it (M6 spec, "Download from the index"). A pack goes to
/// a temporary file, its size and SHA-256 are checked against the index, then the installer
/// installs it with every check a Files import gets. The temporary file is always removed.
/// Redirects are followed (URLSession's default, with no delegate): a GitHub release asset
/// answers with a 302 to its storage host, and the size and digest are of what arrives there.
public struct PackDownloader: Sendable {
    public enum Failure: Error, Equatable, CustomStringConvertible {
        /// Not an https (or file) URL.
        case insecure(URL)
        /// The request failed (no connection, timeout).
        case network(String)
        /// The server answered with this HTTP status.
        case http(Int)
        /// The download's size differs from the index's.
        case size(expected: Int64, got: Int64)
        /// The download's SHA-256 differs from the index's.
        case checksum

        public var description: String {
            switch self {
            case .insecure(let url): "refusing a non-https URL: \(url.absoluteString)"
            case .network(let why): "download failed: \(why)"
            case .http(let status): "the server answered \(status)"
            case .size(let expected, let got): "downloaded \(got) bytes, the index says \(expected)"
            case .checksum: "the download's SHA-256 differs from the index"
            }
        }
    }

    public let session: URLSession

    public init(session: URLSession = .shared) {
        self.session = session
    }

    /// The index at `url`.
    public func index(at url: URL) async throws -> PackIndex {
        let data = try await fetch(url)
        return try PackIndex.decode(data, base: url)
    }

    /// Downloads `entry`, checks it and installs it with `installer`.
    @discardableResult
    public func install(_ entry: PackIndex.Entry, with installer: PackInstaller) async throws -> PackDescriptor {
        try Self.checkScheme(entry.url)
        let downloaded: URL
        let response: URLResponse
        do {
            (downloaded, response) = try await session.download(from: entry.url)
        } catch {
            throw Failure.network(error.localizedDescription)
        }
        defer { try? FileManager.default.removeItem(at: downloaded) }
        try Self.checkStatus(response)
        try Self.verify(downloaded, size: entry.size, sha256: entry.sha256)
        return try installer.install(archive: downloaded)
    }

    private func fetch(_ url: URL) async throws -> Data {
        try Self.checkScheme(url)
        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(from: url)
        } catch {
            throw Failure.network(error.localizedDescription)
        }
        try Self.checkStatus(response)
        return data
    }

    private static func checkScheme(_ url: URL) throws {
        guard let scheme = url.scheme?.lowercased(), scheme == "https" || scheme == "file" else {
            throw Failure.insecure(url)
        }
    }

    private static func checkStatus(_ response: URLResponse) throws {
        if let http = response as? HTTPURLResponse, !(200 ..< 300).contains(http.statusCode) {
            throw Failure.http(http.statusCode)
        }
    }

    /// The file's size and SHA-256, read in chunks (a pack is tens of megabytes).
    static func verify(_ file: URL, size: Int64, sha256: String) throws {
        let got = (try? FileManager.default.attributesOfItem(atPath: file.path)[.size] as? NSNumber)?.int64Value ?? -1
        guard got == size else { throw Failure.size(expected: size, got: got) }
        guard let handle = try? FileHandle(forReadingFrom: file) else { throw Failure.checksum }
        defer { try? handle.close() }
        var hasher = SHA256()
        while let chunk = try? handle.read(upToCount: 1 << 20), !chunk.isEmpty {
            hasher.update(data: chunk)
        }
        let digest = hasher.finalize().map { String(format: "%02x", $0) }.joined()
        guard digest == sha256.lowercased() else { throw Failure.checksum }
    }
}

import CryptoKit
import Foundation
import Synchronization
import Testing
@testable import KeyboardCore

private let support = PackSupport(format: 1, engine: 2, scripts: ["mlym", "deva", "beng"])

private func sha256(_ data: Data) -> String {
    SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}

/// A small valid pack archive with id `id`.
private func packArchive(id: String = "xx", version: String = "2026.11.07") -> Data {
    let lexicon = Data("lexicon".utf8)
    let manifest: [String: Any] = [
        "format": 1, "id": id, "version": version, "name": "Test", "nativeName": "Test",
        "script": "deva", "minEngine": 1, "files": ["lexicon": "x.fst"], "sha256": ["x.fst": sha256(lexicon)],
    ]
    var zip = TestZip()
    zip.add("\(id).neopack/manifest.json", try! JSONSerialization.data(withJSONObject: manifest, options: [.sortedKeys]))
    zip.add("\(id).neopack/x.fst", lexicon)
    return zip.data
}

/// Answers requests from a table instead of the network.
final class StubProtocol: URLProtocol {
    struct Reply: Sendable {
        var status: Int = 200
        var body: Data
        /// Answer with a 302 to this URL instead, as GitHub does for a release asset.
        var redirect: URL? = nil
    }

    static let replies = Mutex<[URL: Reply]>([:])

    static func session(_ table: [URL: Reply]) -> URLSession {
        replies.withLock { $0.merge(table) { _, new in new } }
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [StubProtocol.self]
        return URLSession(configuration: config)
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        guard let url = request.url, let reply = Self.replies.withLock({ $0[url] }) else {
            client?.urlProtocol(self, didFailWithError: URLError(.cannotConnectToHost))
            return
        }
        if let target = reply.redirect {
            let response = HTTPURLResponse(url: url, statusCode: 302, httpVersion: "HTTP/1.1",
                                           headerFields: ["Location": target.absoluteString])!
            client?.urlProtocol(self, wasRedirectedTo: URLRequest(url: target), redirectResponse: response)
            return
        }
        let response = HTTPURLResponse(url: url, statusCode: reply.status, httpVersion: "HTTP/1.1", headerFields: nil)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: reply.body)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}

private func host(_ name: String) -> URL { URL(string: "https://\(name).test/packs/")! }

private func entry(_ base: URL, data: Data, id: String = "xx", size: Int64? = nil, digest: String? = nil) -> PackIndex.Entry {
    PackIndex.Entry(id: id, name: "Test", version: "2026.11.07", script: "deva", size: size ?? Int64(data.count),
                    sha256: digest ?? sha256(data), url: base.appending(path: "\(id).neopack"), minEngine: 1)
}

@Test func anIndexDecodesWithRelativeURLs() throws {
    let json = #"""
    {"format": 1, "packs": [
      {"id": "hi", "name": "Hindi", "nativeName": "हिन्दी", "version": "2026.11.07", "script": "deva",
       "size": 100, "sha256": "ABCD", "url": "hi.neopack", "englishFingerprint": "0011", "minEngine": 2},
      {"id": "as", "name": "Assamese", "version": "1", "script": "beng", "size": 5, "sha256": "ef",
       "url": "https://elsewhere.test/as.neopack", "minEngine": 1}
    ]}
    """#
    let index = try PackIndex.decode(Data(json.utf8), base: URL(string: "https://example.test/v1/index.json")!)
    #expect(index.packs.map(\.id) == ["hi", "as"])
    #expect(index.packs[0].url.absoluteString == "https://example.test/v1/hi.neopack")
    #expect(index.packs[0].nativeName == "हिन्दी")
    #expect(index.packs[0].sha256 == "abcd")
    #expect(index.packs[0].available == AvailablePack(id: "hi", version: "2026.11.07", script: "deva", minEngine: 2, englishFingerprint: "0011"))
    #expect(index.packs[1].url.absoluteString == "https://elsewhere.test/as.neopack")
    #expect(index.packs[1].englishFingerprint == nil)
}

@Test func aBadEntryIsSkippedAndAnotherFormatIsRefused() throws {
    let json = #"""
    {"format": 1, "packs": [
      {"id": "hi", "name": "Hindi"},
      {"id": "en", "name": "English", "version": "1", "script": "latn", "size": 5, "sha256": "ef", "url": "en.neopack", "minEngine": 1},
      {"id": "as", "name": "Assamese", "version": "1", "script": "beng", "size": 5, "sha256": "ef", "url": "as.neopack", "minEngine": 1}
    ]}
    """#
    let base = URL(string: "https://example.test/index.json")!
    #expect(try PackIndex.decode(Data(json.utf8), base: base).packs.map(\.id) == ["as"])
    #expect(throws: PackIndex.Refusal.format(2)) { try PackIndex.decode(Data(#"{"format": 2, "packs": []}"#.utf8), base: base) }
    #expect(throws: PackIndex.Refusal.notJSON) { try PackIndex.decode(Data("<html>".utf8), base: base) }
}

@Test func whatThisBuildReads() {
    let base = host("readable")
    let e = entry(base, data: Data())
    #expect(e.isReadable(by: support))
    let newer = PackIndex.Entry(id: "xx", name: "T", version: "1", script: "deva", size: 1, sha256: "", url: base, minEngine: 3)
    #expect(!newer.isReadable(by: support))
    let script = PackIndex.Entry(id: "xx", name: "T", version: "1", script: "taml", size: 1, sha256: "", url: base, minEngine: 1)
    #expect(!script.isReadable(by: support))
}

@Test func theIndexIsFetched() async throws {
    let url = host("index").appending(path: "index.json")
    let body = Data(#"{"format": 1, "packs": [{"id": "hi", "name": "Hindi", "version": "1", "script": "deva", "size": 9, "sha256": "aa", "url": "hi.neopack", "minEngine": 1}]}"#.utf8)
    let downloader = PackDownloader(session: StubProtocol.session([url: .init(body: body)]))
    let index = try await downloader.index(at: url)
    #expect(index.packs.first?.url == host("index").appending(path: "hi.neopack"))
}

@Test func aDownloadedPackIsInstalled() async throws {
    let root = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: root) }
    let installer = PackInstaller(packsFolder: root.appending(path: "Packs"), support: support)
    let data = packArchive()
    let e = entry(host("good"), data: data)
    let downloader = PackDownloader(session: StubProtocol.session([e.url: .init(body: data)]))
    let pack = try await downloader.install(e, with: installer)
    #expect(pack.id == "xx")
    #expect(PackDescriptor.scan(installer.packsFolder).map(\.id) == ["xx"])
}

/// GitHub serves a release asset (`…/releases/download/<tag>/<file>`) as a 302 to its storage
/// host; the download follows it, and the index can be redirected too.
@Test func aRedirectedDownloadIsFollowed() async throws {
    let root = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: root) }
    let installer = PackInstaller(packsFolder: root.appending(path: "Packs"), support: support)
    let data = packArchive()
    let asset = URL(string: "https://github.test/owner/repo/releases/download/packs-xx-2026.11.07/xx.neopack")!
    let storage = URL(string: "https://objects.storage.test/12345/xx.neopack?signature=abc")!
    let e = PackIndex.Entry(id: "xx", name: "Test", version: "2026.11.07", script: "deva", size: Int64(data.count),
                            sha256: sha256(data), url: asset, minEngine: 1)
    let indexURL = host("moved").appending(path: "index.json")
    let indexTarget = host("pages").appending(path: "index.json")
    let indexBody = Data(#"{"format": 1, "packs": [{"id": "xx", "name": "Test", "version": "1", "script": "deva", "size": 9, "sha256": "aa", "url": "https://github.test/owner/repo/releases/download/packs-xx-1/xx.neopack", "minEngine": 1}]}"#.utf8)
    let downloader = PackDownloader(session: StubProtocol.session([
        asset: .init(body: Data(), redirect: storage), storage: .init(body: data),
        indexURL: .init(body: Data(), redirect: indexTarget), indexTarget: .init(body: indexBody),
    ]))
    let pack = try await downloader.install(e, with: installer)
    #expect(pack.id == "xx")
    #expect(PackDescriptor.scan(installer.packsFolder).map(\.id) == ["xx"])
    let index = try await downloader.index(at: indexURL)
    #expect(index.packs.map { $0.url.host() } == ["github.test"])
}

@Test func aWrongSizeOrDigestIsRefusedAndNothingIsInstalled() async throws {
    let root = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: root) }
    let installer = PackInstaller(packsFolder: root.appending(path: "Packs"), support: support)
    let data = packArchive()
    let sized = entry(host("size"), data: data, size: Int64(data.count + 1))
    let digested = entry(host("digest"), data: data, digest: String(repeating: "0", count: 64))
    let downloader = PackDownloader(session: StubProtocol.session([sized.url: .init(body: data), digested.url: .init(body: data)]))
    await #expect(throws: PackDownloader.Failure.size(expected: Int64(data.count + 1), got: Int64(data.count))) {
        try await downloader.install(sized, with: installer)
    }
    await #expect(throws: PackDownloader.Failure.checksum) { try await downloader.install(digested, with: installer) }
    #expect(PackDescriptor.scan(installer.packsFolder).isEmpty)
}

@Test func httpAndNetworkErrorsAndPlainHTTPAreReported() async throws {
    let root = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: root) }
    let installer = PackInstaller(packsFolder: root.appending(path: "Packs"), support: support)
    let missing = entry(host("missing"), data: Data("x".utf8))
    let offline = entry(host("offline"), data: Data("x".utf8))
    let downloader = PackDownloader(session: StubProtocol.session([missing.url: .init(status: 404, body: Data("x".utf8))]))
    await #expect(throws: PackDownloader.Failure.http(404)) { try await downloader.install(missing, with: installer) }
    do {
        try await downloader.install(offline, with: installer)
        Issue.record("installed without a connection")
    } catch PackDownloader.Failure.network {
    } catch {
        Issue.record("not a network failure: \(error)")
    }
    let plain = PackIndex.Entry(id: "xx", name: "T", version: "1", script: "deva", size: 1, sha256: "",
                                url: URL(string: "http://plain.test/xx.neopack")!, minEngine: 1)
    await #expect(throws: PackDownloader.Failure.insecure(plain.url)) { try await downloader.install(plain, with: installer) }
}

@Test func aDamagedArchiveWithTheRightDigestIsRefusedByTheInstaller() async throws {
    let root = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: root) }
    let installer = PackInstaller(packsFolder: root.appending(path: "Packs"), support: support)
    let data = Data("not a zip".utf8)
    let e = entry(host("notzip"), data: data)
    let downloader = PackDownloader(session: StubProtocol.session([e.url: .init(body: data)]))
    do {
        try await downloader.install(e, with: installer)
        Issue.record("installed a non-zip")
    } catch is PackInstaller.Refusal {
    }
    #expect(PackDescriptor.scan(installer.packsFolder).isEmpty)
}

@Test func verifyChecksSizeAndDigest() throws {
    let root = try temporaryFolder()
    defer { try? FileManager.default.removeItem(at: root) }
    let file = root.appending(path: "a")
    try Data("abc".utf8).write(to: file)
    try PackDownloader.verify(file, size: 3, sha256: sha256(Data("abc".utf8)))
    #expect(throws: PackDownloader.Failure.checksum) { try PackDownloader.verify(file, size: 3, sha256: "00") }
}

/// The index the site publishes (site/packs/index.json, from scripts/publish-packs.py) decodes,
/// lists every pack in packs/ at its manifest's version, script and minEngine, and points at
/// https pack files named by id and version.
@Test func thePublishedIndexMatchesThePacks() throws {
    let repoRoot = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent()
    let file = repoRoot.appending(path: "site/packs/index.json")
    let index = try PackIndex.decode(Data(contentsOf: file), base: URL(string: "https://example.test/packs/index.json")!)
    let packs = PackDescriptor.scan(repoRoot.appending(path: "packs"))
    #expect(!packs.isEmpty)
    #expect(index.packs.map(\.id) == packs.map(\.id))
    for (entry, pack) in zip(index.packs, packs) {
        #expect(entry.version == pack.version)
        #expect(entry.script == pack.script)
        #expect(entry.minEngine == pack.minEngine)
        #expect(entry.url.scheme == "https")
        #expect(entry.url.lastPathComponent == "\(pack.id)-\(pack.version).neopack")
        #expect(entry.sha256.count == 64)
    }
}

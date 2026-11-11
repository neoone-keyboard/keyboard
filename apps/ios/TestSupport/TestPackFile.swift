import CryptoKit
import Foundation
import zlib

/// `.neopack` files for the app's tests, made from the engine's small Gujarati test pack
/// (engine/core/tests/fixtures/gu.neopack, a folder in each test bundle): a real pack the
/// engine opens, under an id no real pack has.
enum TestPackFile {
    struct Missing: Error {}

    /// The pack as a `.neopack` file's bytes under `id`, as `scripts/pack.py --zip` writes
    /// one (stored entries under `<id>.neopack/`). `edit` changes the manifest; `replacing`
    /// swaps a file's bytes, and its checksum in the manifest with them.
    static func data(id: String, in bundle: Bundle, replacing: [String: Data] = [:],
                     edit: (inout [String: Any]) -> Void = { _ in }) throws -> Data {
        guard let source = bundle.url(forResource: "gu", withExtension: "neopack"),
              var manifest = try JSONSerialization.jsonObject(
                with: Data(contentsOf: source.appending(path: "manifest.json"))) as? [String: Any],
              var sums = manifest["sha256"] as? [String: String]
        else { throw Missing() }
        manifest["id"] = id
        var entries: [(name: String, data: Data)] = []
        for name in sums.keys.sorted() {
            let data = try replacing[name] ?? Data(contentsOf: source.appending(path: name))
            if replacing[name] != nil {
                sums[name] = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
            }
            entries.append(("\(id).neopack/\(name)", data))
        }
        manifest["sha256"] = sums
        edit(&manifest)
        entries.insert(("\(id).neopack/manifest.json", try JSONSerialization.data(withJSONObject: manifest)), at: 0)
        return zip(entries)
    }

    /// The bytes of one of the pack's files, as they are in the pack file.
    static func file(_ name: String, in bundle: Bundle) throws -> Data {
        guard let source = bundle.url(forResource: "gu", withExtension: "neopack") else { throw Missing() }
        return try Data(contentsOf: source.appending(path: name))
    }

    /// A zip of stored entries.
    static func zip(_ entries: [(name: String, data: Data)]) -> Data {
        var out = Data()
        var central = Data()
        func le16(_ value: Int) -> Data { withUnsafeBytes(of: UInt16(value).littleEndian) { Data($0) } }
        func le32(_ value: Int) -> Data { withUnsafeBytes(of: UInt32(truncatingIfNeeded: value).littleEndian) { Data($0) } }
        for entry in entries {
            let name = Data(entry.name.utf8)
            let crc = entry.data.withUnsafeBytes { Int(crc32(0, $0.bindMemory(to: UInt8.self).baseAddress, UInt32($0.count))) }
            let offset = out.count
            // Version 2.0, no flags, stored, no time or date; the CRC and both sizes; the
            // name's length, no extra field.
            let common = le16(20) + le16(0) + le16(0) + le16(0) + le16(0)
                + le32(crc) + le32(entry.data.count) + le32(entry.data.count) + le16(name.count) + le16(0)
            out += le32(0x0403_4b50) + common + name + entry.data
            central += le32(0x0201_4b50) + le16(20) + common + le16(0) + le16(0) + le16(0) + le32(0) + le32(offset) + name
        }
        let directory = out.count
        out += central
        out += le32(0x0605_4b50) + le16(0) + le16(0) + le16(entries.count) + le16(entries.count)
            + le32(central.count) + le32(directory) + le16(0)
        return out
    }
}

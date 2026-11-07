import Compression
import Foundation

/// Reads a zip archive (a `.neopack` file: `scripts/pack.py --zip` stores, Finder and `zip`
/// deflate). Only what a pack needs: one disk, no zip64, no encryption, stored or deflated
/// entries. Every entry name is checked when the archive opens, and every entry's size and
/// CRC-32 when it is read, so a damaged or hostile file is refused before anything is written.
struct ZipReader {
    struct Entry: Equatable {
        /// As stored, "ml.neopack/ml.fst".
        let name: String
        /// The name's parts; never empty, never "." or "..".
        let components: [String]
        /// 0 stored, 8 deflate.
        let method: UInt16
        let crc: UInt32
        let compressedSize: Int
        let size: Int
        let localHeaderOffset: Int
    }

    enum Failure: Error, Equatable, CustomStringConvertible {
        case notAZip(String)
        case unsupported(String)
        case unsafeName(String)
        case tooLarge(Int)
        case corrupt(String)

        var description: String {
            switch self {
            case .notAZip(let why): "not a zip archive: \(why)"
            case .unsupported(let why): "unsupported zip: \(why)"
            case .unsafeName(let name): "unsafe file name in the archive: \(name)"
            case .tooLarge(let size): "archive unpacks to \(size) bytes, more than allowed"
            case .corrupt(let why): "damaged archive: \(why)"
            }
        }
    }

    /// The default limit on the unpacked size of every entry together (the Malayalam pack is
    /// about 30 MB).
    static let defaultMaxTotalSize = 1 << 30

    private let data: Data
    /// The file entries, in directory order; folder entries ("ml.neopack/") are left out.
    let entries: [Entry]

    init(url: URL, maxTotalSize: Int = defaultMaxTotalSize) throws {
        let data: Data
        do {
            data = try Data(contentsOf: url, options: .alwaysMapped)
        } catch {
            throw Failure.notAZip("unreadable: \(error.localizedDescription)")
        }
        self.data = data
        entries = try Self.directory(data, maxTotalSize: maxTotalSize)
    }

    /// The entry's bytes, checked against its size and CRC-32.
    func contents(of entry: Entry) throws -> Data {
        let header = entry.localHeaderOffset
        guard try data.le32(at: header) == 0x0403_4b50 else { throw Failure.corrupt("no local header for \(entry.name)") }
        let nameLength = Int(try data.le16(at: header + 26))
        let extraLength = Int(try data.le16(at: header + 28))
        let start = header + 30 + nameLength + extraLength
        guard start <= data.count, entry.compressedSize <= data.count - start else {
            throw Failure.corrupt("\(entry.name) runs past the end")
        }
        let stored = data.subdata(in: start ..< start + entry.compressedSize)
        let bytes: Data
        switch entry.method {
        case 0:
            bytes = stored
        case 8:
            bytes = try Self.inflate(stored, size: entry.size, name: entry.name)
        default:
            throw Failure.unsupported("compression method \(entry.method) for \(entry.name)")
        }
        guard bytes.count == entry.size else { throw Failure.corrupt("\(entry.name) has the wrong size") }
        guard CRC32.checksum(bytes) == entry.crc else { throw Failure.corrupt("\(entry.name) fails its CRC") }
        return bytes
    }

    private static func inflate(_ stored: Data, size: Int, name: String) throws -> Data {
        if size == 0 { return Data() }
        var out = Data(count: size)
        let written = out.withUnsafeMutableBytes { dst in
            stored.withUnsafeBytes { src in
                // COMPRESSION_ZLIB is raw DEFLATE (RFC 1951), as zip stores it.
                compression_decode_buffer(
                    dst.bindMemory(to: UInt8.self).baseAddress!, size,
                    src.bindMemory(to: UInt8.self).baseAddress!, stored.count,
                    nil, COMPRESSION_ZLIB
                )
            }
        }
        guard written == size else { throw Failure.corrupt("\(name) does not inflate to its size") }
        return out
    }

    private static func directory(_ data: Data, maxTotalSize: Int) throws -> [Entry] {
        let end = try endOfDirectory(data)
        guard try data.le16(at: end + 4) == 0, try data.le16(at: end + 6) == 0 else {
            throw Failure.unsupported("split across disks")
        }
        let count = Int(try data.le16(at: end + 10))
        let directorySize = Int(try data.le32(at: end + 12))
        let directoryOffset = Int(try data.le32(at: end + 16))
        if count == 0xFFFF || directorySize == 0xFFFF_FFFF || directoryOffset == 0xFFFF_FFFF {
            throw Failure.unsupported("zip64")
        }
        guard directoryOffset <= end, directorySize <= end - directoryOffset else {
            throw Failure.corrupt("central directory out of bounds")
        }
        var entries: [Entry] = []
        var total = 0
        var at = directoryOffset
        for _ in 0 ..< count {
            guard try data.le32(at: at) == 0x0201_4b50 else { throw Failure.corrupt("bad central directory entry") }
            let flags = try data.le16(at: at + 8)
            let method = try data.le16(at: at + 10)
            let crc = try data.le32(at: at + 16)
            let compressedSize = try data.le32(at: at + 20)
            let size = try data.le32(at: at + 24)
            let nameLength = Int(try data.le16(at: at + 28))
            let extraLength = Int(try data.le16(at: at + 30))
            let commentLength = Int(try data.le16(at: at + 32))
            let offset = try data.le32(at: at + 42)
            guard at + 46 + nameLength <= data.count else { throw Failure.corrupt("name runs past the end") }
            let nameBytes = data.subdata(in: at + 46 ..< at + 46 + nameLength)
            at += 46 + nameLength + extraLength + commentLength
            guard let name = String(data: nameBytes, encoding: .utf8) else { throw Failure.unsafeName("(not UTF-8)") }
            if flags & 1 != 0 { throw Failure.unsupported("\(name) is encrypted") }
            if compressedSize == 0xFFFF_FFFF || size == 0xFFFF_FFFF || offset == 0xFFFF_FFFF {
                throw Failure.unsupported("zip64")
            }
            if name.hasSuffix("/") {
                // A folder entry: its name must be as safe as a file's.
                _ = try components(String(name.dropLast()), original: name)
                continue
            }
            let parts = try components(name, original: name)
            total += Int(size)
            if total > maxTotalSize { throw Failure.tooLarge(total) }
            entries.append(Entry(
                name: name, components: parts, method: method, crc: crc,
                compressedSize: Int(compressedSize), size: Int(size), localHeaderOffset: Int(offset)
            ))
        }
        return entries
    }

    /// `name` split at "/", refused when it could name anything outside the folder it is
    /// unpacked into: absolute, "." or "..", empty parts, a backslash or a NUL.
    private static func components(_ name: String, original: String) throws -> [String] {
        let parts = name.split(separator: "/", omittingEmptySubsequences: false).map(String.init)
        let bad = name.isEmpty || name.contains("\\") || name.contains("\0")
            || parts.contains { $0.isEmpty || $0 == "." || $0 == ".." }
        if bad { throw Failure.unsafeName(original) }
        return parts
    }

    /// The offset of the end-of-central-directory record, searched for from the end (it may
    /// be followed by a comment of up to 65,535 bytes).
    private static func endOfDirectory(_ data: Data) throws -> Int {
        guard data.count >= 22 else { throw Failure.notAZip("too short") }
        let lowest = max(0, data.count - 22 - 0xFFFF)
        var at = data.count - 22
        while at >= lowest {
            if try data.le32(at: at) == 0x0605_4b50 { return at }
            at -= 1
        }
        throw Failure.notAZip("no end of central directory")
    }
}

private extension Data {
    func le16(at offset: Int) throws -> UInt16 {
        guard offset >= 0, offset + 2 <= count else { throw ZipReader.Failure.corrupt("read past the end") }
        let i = startIndex + offset
        return UInt16(self[i]) | UInt16(self[i + 1]) << 8
    }

    func le32(at offset: Int) throws -> UInt32 {
        guard offset >= 0, offset + 4 <= count else { throw ZipReader.Failure.corrupt("read past the end") }
        let i = startIndex + offset
        return UInt32(self[i]) | UInt32(self[i + 1]) << 8 | UInt32(self[i + 2]) << 16 | UInt32(self[i + 3]) << 24
    }
}

/// CRC-32 (IEEE 802.3, as zip uses it).
enum CRC32 {
    private static let table: [UInt32] = (0 ..< 256).map { n in
        var c = UInt32(n)
        for _ in 0 ..< 8 { c = c & 1 != 0 ? 0xEDB8_8320 ^ (c >> 1) : c >> 1 }
        return c
    }

    static func checksum(_ data: Data) -> UInt32 {
        var crc: UInt32 = 0xFFFF_FFFF
        data.withUnsafeBytes { (bytes: UnsafeRawBufferPointer) in
            table.withUnsafeBufferPointer { table in
                for byte in bytes {
                    crc = table[Int((crc ^ UInt32(byte)) & 0xFF)] ^ (crc >> 8)
                }
            }
        }
        return crc ^ 0xFFFF_FFFF
    }
}

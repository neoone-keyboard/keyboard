import Foundation
@testable import KeyboardCore

/// Writes small zip archives for the tests: stored entries only, with knobs to break them.
struct TestZip {
    struct Entry {
        var name: String
        var data: Data
        /// Overrides the CRC-32 written for the entry (a corrupt archive).
        var crc: UInt32?
        /// General-purpose flags (bit 0: encrypted).
        var flags: UInt16 = 0
    }

    var entries: [Entry] = []

    mutating func add(_ name: String, _ data: Data, crc: UInt32? = nil, flags: UInt16 = 0) {
        entries.append(Entry(name: name, data: data, crc: crc, flags: flags))
    }

    mutating func add(_ name: String, _ text: String) {
        add(name, Data(text.utf8))
    }

    var data: Data {
        var out = Data()
        var central = Data()
        for entry in entries {
            let name = Data(entry.name.utf8)
            let crc = entry.crc ?? CRC32.checksum(entry.data)
            let offset = UInt32(out.count)
            out.append(le32: 0x0403_4b50)
            out.append(le16: 20)
            out.append(le16: entry.flags)
            out.append(le16: 0) // stored
            out.append(le16: 0)
            out.append(le16: 0)
            out.append(le32: crc)
            out.append(le32: UInt32(entry.data.count))
            out.append(le32: UInt32(entry.data.count))
            out.append(le16: UInt16(name.count))
            out.append(le16: 0)
            out.append(name)
            out.append(entry.data)

            central.append(le32: 0x0201_4b50)
            central.append(le16: 20)
            central.append(le16: 20)
            central.append(le16: entry.flags)
            central.append(le16: 0)
            central.append(le16: 0)
            central.append(le16: 0)
            central.append(le32: crc)
            central.append(le32: UInt32(entry.data.count))
            central.append(le32: UInt32(entry.data.count))
            central.append(le16: UInt16(name.count))
            central.append(le16: 0)
            central.append(le16: 0)
            central.append(le16: 0)
            central.append(le16: 0)
            central.append(le32: 0)
            central.append(le32: offset)
            central.append(name)
        }
        let centralOffset = UInt32(out.count)
        out.append(central)
        out.append(le32: 0x0605_4b50)
        out.append(le16: 0)
        out.append(le16: 0)
        out.append(le16: UInt16(entries.count))
        out.append(le16: UInt16(entries.count))
        out.append(le32: UInt32(central.count))
        out.append(le32: centralOffset)
        out.append(le16: 0)
        return out
    }

    func write(to url: URL) throws {
        try data.write(to: url)
    }
}

extension Data {
    mutating func append(le16 value: UInt16) {
        Swift.withUnsafeBytes(of: value.littleEndian) { append(contentsOf: $0) }
    }

    mutating func append(le32 value: UInt32) {
        Swift.withUnsafeBytes(of: value.littleEndian) { append(contentsOf: $0) }
    }
}

/// A fresh, empty temporary folder; removed by the caller.
func temporaryFolder() throws -> URL {
    let url = FileManager.default.temporaryDirectory.appending(path: "keyboardcore-\(UUID().uuidString)", directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

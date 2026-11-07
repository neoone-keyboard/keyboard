import Foundation

/// The fingerprint a pack's `englishFingerprint` names: FNV-1a 64 of the `en.fst` file's bytes,
/// as 16 hex digits of its little-endian bytes in order (the engine's `Lexicon::fingerprint`,
/// which the LM's tables record).
public enum EnglishFingerprint {
    public static func of(_ url: URL) throws -> String {
        of(try Data(contentsOf: url, options: .alwaysMapped))
    }

    public static func of(_ data: Data) -> String {
        var hash: UInt64 = 0xCBF2_9CE4_8422_2325
        data.withUnsafeBytes { (bytes: UnsafeRawBufferPointer) in
            for byte in bytes {
                hash = (hash ^ UInt64(byte)) &* 0x0000_0100_0000_01B3
            }
        }
        return withUnsafeBytes(of: hash.littleEndian) { $0.map { String(format: "%02x", $0) }.joined() }
    }
}

/// One pack as a pack index lists it (spec, "Installing": `index.json`, published with the
/// releases). Only what the update check needs.
public struct AvailablePack: Sendable, Equatable, Codable {
    public let id: String
    public let version: String
    public let script: String
    public let minEngine: Int
    public let englishFingerprint: String?

    public init(id: String, version: String, script: String, minEngine: Int, englishFingerprint: String?) {
        self.id = id
        self.version = version
        self.script = script
        self.minEngine = minEngine
        self.englishFingerprint = englishFingerprint
    }
}

/// An update the app can offer for a pack the user has.
public struct PackOffer: Sendable, Equatable {
    public enum Reason: Sendable, Equatable {
        /// The index has a higher version.
        case newerVersion
        /// The pack was built against another English (its LM is off, spec decision 2) and the
        /// index has a build against this one.
        case matchesEnglish
    }

    public let id: String
    public let installedVersion: String
    public let offered: AvailablePack
    public let reason: Reason

    public init(id: String, installedVersion: String, offered: AvailablePack, reason: Reason) {
        self.id = id
        self.installedVersion = installedVersion
        self.offered = offered
        self.reason = reason
    }
}

/// Whether to offer an update (M6 spec, phase 3, "the upgrade offer"). Decides only; the
/// Languages screen shows it, and the download is not built yet.
public enum PackUpdates {
    /// Whether `pack` was built against an English other than this keyboard's (fingerprint
    /// `englishFingerprint`, of the bundled `en.fst`): its LM is then off until it is rebuilt.
    /// A pack that names no fingerprint (Malayalam's, built with `en.fst` in one run) never is.
    public static func isStale(_ pack: PackDescriptor, englishFingerprint: String) -> Bool {
        guard let fingerprint = pack.englishFingerprint else { return false }
        return fingerprint.lowercased() != englishFingerprint.lowercased()
    }

    /// For each pack in `installed` (the keyboard's, bundled or installed), the index entry
    /// to offer, if any. Entries this build can't read (`support`), and entries built against
    /// another English, are never offered. Of the rest for an id, the highest version is
    /// offered when it is higher than the installed one, or, for a stale pack, when it is at
    /// least as high (the installer takes the same version again). Packs that aren't
    /// installed get no offer: adding a language is the user's choice, not an update.
    public static func offers(installed: [PackDescriptor], available: [AvailablePack],
                              englishFingerprint: String, support: PackSupport) -> [PackOffer] {
        let base = englishFingerprint.lowercased()
        return installed.compactMap { pack in
            let usable = available.filter { entry in
                entry.id == pack.id && entry.minEngine <= support.engine && support.scripts.contains(entry.script)
                    && (entry.englishFingerprint.map { $0.lowercased() == base } ?? true)
            }
            guard let best = usable.max(by: { PackVersion($0.version) < PackVersion($1.version) }) else { return nil }
            let have = PackVersion(pack.version)
            let offered = PackVersion(best.version)
            if have < offered {
                return PackOffer(id: pack.id, installedVersion: pack.version, offered: best, reason: .newerVersion)
            }
            if isStale(pack, englishFingerprint: base), best.englishFingerprint != nil, !(offered < have) {
                return PackOffer(id: pack.id, installedVersion: pack.version, offered: best, reason: .matchesEnglish)
            }
            return nil
        }
    }
}

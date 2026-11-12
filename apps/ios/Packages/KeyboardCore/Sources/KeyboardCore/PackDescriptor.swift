import Foundation

/// What the keyboard and the app know of a language pack (`<id>.neopack/`): its
/// `manifest.json` and its `layout.json`. The engine opens the same folder for the word lists
/// and models; this is the part Swift draws and credits.
public struct PackDescriptor: Sendable, Identifiable, Hashable {
    /// The language id: "ml".
    public let id: String
    /// In English: "Malayalam".
    public let name: String
    /// In the language: "മലയാളം".
    public let nativeName: String
    /// ISO 15924, lower case: "mlym". The engine's script rules are keyed by it.
    public let script: String
    /// The romanised way of typing it: "Manglish". Nil when the pack has none.
    public let romanisedName: String?
    /// Where the pack's data comes from, as the About screen lists it.
    public let credits: [Credit]
    /// The native keyboard; nil when the pack has no `layout.json` (or it couldn't be read),
    /// and then the language is typed on the Latin keyboard only.
    public let layout: PackLayout?
    /// The pack folder.
    public let folder: URL
    /// The manifest's `version` ("2026.11.06"); compare with `PackVersion`.
    public let version: String
    /// The engine level the pack needs (`minEngine`; the engine's is `PACK_ENGINE`).
    public let minEngine: Int
    /// The `en.fst` the pack's LM and next words were built against: FNV-1a 64 of the file as 16
    /// hex digits of its little-endian bytes (`EnglishFingerprint`). Nil when the manifest has
    /// none (Malayalam's, built in the same run as `en.fst`).
    public let englishFingerprint: String?
    /// The characters of the pack's script, as code point ranges: `block` and `letters` of its
    /// `lang.json` (the engine's script rules read the same). For a pack without the file,
    /// the script's own block when the engine has rules for it (`embeddedScriptRanges`), else
    /// none.
    public let scriptRanges: [ClosedRange<UInt32>]
    /// The pack's own emoji words, when it brings them (the optional `emojiWords` and
    /// `emojiSearch` roles): its language's words for the bar's emoji and for the emoji
    /// panel's search, in the formats of the app's emoji-words.txt and emoji-search.txt.
    public let emojiWords: URL?
    public let emojiSearch: URL?
    /// The script is written right to left: `lang.json`'s `direction` ("rtl" or "ltr") when
    /// given, else the Unicode direction of the script's first letter (`isRightToLeft`). The
    /// suggestion bar is mirrored while this pack's conventions are followed
    /// (`InputController.scriptPack`). The keys are never mirrored.
    public let rightToLeft: Bool
    /// A language typed as a reading and converted as a phrase (the manifest's `"conversion":
    /// "phrase"`, Japanese): it has no word list for the bar and no keys of its own. It is
    /// typed on the Latin keys through `PhraseInput` and converted by the engine apart from its
    /// word languages (`Engine.conversionLanguages`).
    public let conversion: Bool
    /// Licence texts that must travel with the pack's data, shown as they are (the optional
    /// `notices` role: Japanese's dictionary-notices.txt); nil when it has none.
    public let notices: URL?

    /// The script's own marks for the Latin punctuation keys (`layout.json`'s `punctuation`):
    /// Urdu's `"."` to `"۔"`. Empty when the script uses the Latin marks.
    public var punctuation: [String: String] { layout?.punctuation ?? [:] }

    /// The script's digits, zero to nine (`layout.json`'s `digits`); empty when it has none.
    public var digits: [String] { layout?.digits ?? [] }

    /// The blocks of the scripts the engine has its own rules for (`script::supported`: packs
    /// in them were published before packs carried `lang.json`), as `block` in
    /// data/langs/ml, hi and as/lang.json.
    public static let embeddedScriptRanges: [String: [ClosedRange<UInt32>]] = [
        "mlym": [0x0D00...0x0D7F],
        "deva": [0x0900...0x097F],
        "beng": [0x0980...0x09FF],
    ]

    public init(id: String, name: String, nativeName: String, script: String, romanisedName: String? = nil,
                credits: [Credit] = [], layout: PackLayout?, folder: URL,
                version: String = "0", minEngine: Int = 1, englishFingerprint: String? = nil,
                scriptRanges: [ClosedRange<UInt32>]? = nil, emojiWords: URL? = nil, emojiSearch: URL? = nil,
                rightToLeft: Bool? = nil, conversion: Bool = false, notices: URL? = nil) {
        self.conversion = conversion
        self.notices = notices
        self.scriptRanges = scriptRanges ?? Self.embeddedScriptRanges[script] ?? []
        self.rightToLeft = rightToLeft ?? Self.isRightToLeft(self.scriptRanges)
        self.emojiWords = emojiWords
        self.emojiSearch = emojiSearch
        self.id = id
        self.name = name
        self.nativeName = nativeName
        self.script = script
        self.romanisedName = romanisedName
        self.credits = credits
        self.layout = layout
        self.folder = folder
        self.version = version
        self.minEngine = minEngine
        self.englishFingerprint = englishFingerprint
    }

    /// Whether the first letter in `ranges` is written right to left: Unicode's default
    /// bidirectional class R or AL, which covers Hebrew, Arabic, Syriac, Thaana, N'Ko and the
    /// other right-to-left blocks (DerivedBidiClass.txt's default ranges).
    public static func isRightToLeft(_ ranges: [ClosedRange<UInt32>]) -> Bool {
        let rightToLeft: [ClosedRange<UInt32>] = [
            0x0590...0x08FF, 0xFB1D...0xFDFF, 0xFE70...0xFEFF, 0x10800...0x10FFF, 0x1E800...0x1EFFF,
        ]
        for range in ranges {
            for value in range {
                guard let scalar = Unicode.Scalar(value), scalar.properties.isAlphabetic else { continue }
                return rightToLeft.contains { $0.contains(value) }
            }
        }
        return false
    }

    /// Whether `text` has a character of the pack's script.
    public func writes(_ text: String) -> Bool {
        text.unicodeScalars.contains { scalar in scriptRanges.contains { $0.contains(scalar.value) } }
    }

    /// One pack per language id: a pack is the same language whatever its version, so the
    /// language key's list and the current language (`Language.pack`) don't change when a
    /// pack is updated. Whether its files changed is `files`.
    public static func == (lhs: PackDescriptor, rhs: PackDescriptor) -> Bool { lhs.id == rhs.id }
    public func hash(into hasher: inout Hasher) { hasher.combine(id) }

    /// What identifies the files the engine opens for a pack: its id, its version and where
    /// it is (the extension bundle's copy or the App Group's). An update, or an installed copy
    /// taking over from the bundled one, changes it (`KeyboardPacks.change`).
    public struct Files: Hashable, Sendable {
        public let id: String
        public let version: String
        /// The pack folder's path.
        public let folder: String
    }

    public var files: Files {
        Files(id: id, version: version, folder: folder.standardizedFileURL.path)
    }

    public struct Refused: Error, CustomStringConvertible {
        public let description: String
    }

    /// Reads `folder/manifest.json` and the layout it names. A manifest that is missing,
    /// malformed or of another format refuses the pack; a bad layout only leaves it without one.
    public static func load(folder: URL) throws -> PackDescriptor {
        let manifest = try JSONDecoder().decode(Manifest.self, from: Data(contentsOf: folder.appending(path: "manifest.json")))
        guard manifest.format == 1 else { throw Refused(description: "pack format \(manifest.format), this keyboard reads 1") }
        // A file name, never a path out of the pack (as the engine's `Pack::file`).
        let file = { (role: String) -> URL? in
            manifest.files[role]
                .flatMap { name in name.isEmpty || name.contains("/") || name.hasPrefix(".") ? nil : name }
                .map { folder.appending(path: $0) }
        }
        let isFile = { (url: URL) in FileManager.default.fileExists(atPath: url.path) }
        let layout = file("layout").flatMap { try? PackLayout.load($0) }
        let lang = file("lang").flatMap { try? Data(contentsOf: $0) }
            .flatMap { try? JSONDecoder().decode(Lang.self, from: $0) }
        return PackDescriptor(
            id: manifest.id, name: manifest.name, nativeName: manifest.nativeName, script: manifest.script,
            romanisedName: manifest.romanised?.name,
            credits: manifest.credits?.map(\.credit) ?? [],
            layout: layout, folder: folder,
            version: manifest.version, minEngine: manifest.minEngine, englishFingerprint: manifest.englishFingerprint,
            scriptRanges: lang?.ranges,
            emojiWords: file("emojiWords").flatMap { isFile($0) ? $0 : nil },
            emojiSearch: file("emojiSearch").flatMap { isFile($0) ? $0 : nil },
            rightToLeft: lang?.rightToLeft,
            conversion: manifest.conversion == "phrase",
            notices: file("notices").flatMap { isFile($0) ? $0 : nil }
        )
    }

    /// One pack per id from the bundle's packs and the App Group's (`PackLocations`): the
    /// installed one when its version is higher than the bundled one's (spec, "Where packs
    /// live"), else the bundled one. Bundled ids keep their place and come first, then the
    /// installed-only ones by id, so installing a pack never changes which pack the engine's
    /// default active list (English and the first pack) takes.
    public static func merge(bundled: [PackDescriptor], installed: [PackDescriptor]) -> [PackDescriptor] {
        var installedByID: [String: PackDescriptor] = [:]
        for pack in installed where installedByID[pack.id] == nil {
            installedByID[pack.id] = pack
        }
        var seen = Set<String>()
        var out: [PackDescriptor] = []
        for pack in bundled where seen.insert(pack.id).inserted {
            if let newer = installedByID[pack.id], PackVersion(pack.version) < PackVersion(newer.version) {
                out.append(newer)
            } else {
                out.append(pack)
            }
        }
        let rest = installedByID.values.filter { !seen.contains($0.id) }.sorted { $0.id < $1.id }
        return out + rest
    }

    /// The packs in `folder` (each `*.neopack` folder in it), by folder name. Refused packs are
    /// left out; the engine reports why (`Engine.packProblems`).
    public static func scan(_ folder: URL) -> [PackDescriptor] {
        let entries = (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)) ?? []
        return entries
            .filter { $0.pathExtension == "neopack" }
            .sorted { $0.lastPathComponent < $1.lastPathComponent }
            .compactMap { try? load(folder: $0) }
    }

    /// The part of a pack's `lang.json` Swift reads: the script's characters, and its
    /// `direction` (the apps', which the engine ignores).
    private struct Lang: Decodable {
        let block: [String]?
        let letters: [[String]]?
        let direction: String?

        /// `direction` when it says "rtl" or "ltr"; nil leaves it to the script's letters.
        var rightToLeft: Bool? {
            switch direction?.lowercased() {
            case "rtl": true
            case "ltr": false
            default: nil
            }
        }

        /// `block`, then `letters`, as the engine's `ScriptDef` takes them; nil when a range
        /// isn't two hex code points in order, or there is none.
        var ranges: [ClosedRange<UInt32>]? {
            var out: [ClosedRange<UInt32>] = []
            for pair in (block.map { [$0] } ?? []) + (letters ?? []) {
                guard pair.count == 2, let a = UInt32(pair[0], radix: 16), let b = UInt32(pair[1], radix: 16), a <= b
                else { return nil }
                out.append(a...b)
            }
            return out.isEmpty ? nil : out
        }
    }

    private struct Manifest: Decodable {
        struct Romanised: Decodable { let name: String }
        struct PackCredit: Decodable {
            let id: String?
            let name: String
            let licence: String
            /// The source's site.
            let url: String?
            /// The licence's text; the source's site when absent.
            let licenceUrl: String?
            let attribution: String?

            var credit: Credit {
                Credit(id: id ?? name, name: name, licence: licence,
                       licenceURL: licenceUrl ?? url ?? "", attribution: attribution ?? url ?? "")
            }
        }

        let format: Int
        let id: String
        let version: String
        let minEngine: Int
        let englishFingerprint: String?
        let name: String
        let nativeName: String
        let script: String
        let romanised: Romanised?
        let files: [String: String]
        let credits: [PackCredit]?
        let conversion: String?
    }
}

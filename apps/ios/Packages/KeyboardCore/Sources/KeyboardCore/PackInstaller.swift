import CryptoKit
import Foundation

/// What packs this keyboard's engine reads: the app fills it from the engine's
/// `pack_support()` (FFI), so the installer refuses exactly what the engine would.
public struct PackSupport: Sendable, Equatable {
    /// The manifest `format` (1).
    public let format: Int
    /// The engine's `PACK_ENGINE`: the highest `minEngine` it reads.
    public let engine: Int
    /// ISO 15924 codes the engine has its own copy of the rules for ("mlym", "deva", "beng"):
    /// a pack in one of them may lack the `lang` file.
    public let scripts: Set<String>
    /// Whether the engine reads a pack in any other script that brings its script's rules (the
    /// `lang` role, a `lang.json`; engine 4).
    public let packScripts: Bool

    public init(format: Int, engine: Int, scripts: Set<String>, packScripts: Bool = false) {
        self.format = format
        self.engine = engine
        self.scripts = scripts
        self.packScripts = packScripts
    }

    /// Whether a pack in `script` can be read, given that it brings its rules when the engine
    /// has none (`PackInstaller` checks that it does).
    public func reads(script: String) -> Bool {
        packScripts || scripts.contains(script)
    }
}

/// Installs `.neopack` archives into a packs folder (the App Group's `Packs/`, see
/// `PackLocations`), only ever as whole, checked packs (M6 spec, "Where packs live"):
///
/// 1. leftovers of an earlier install that didn't finish are removed;
/// 2. the archive's names, sizes and CRCs are checked (`ZipReader`), then the manifest:
///    `format`, `id`, `script` and `minEngine` against `support`, the file names, the lexicon,
///    a checksum for every file, and that it isn't older than the installed version;
/// 3. the manifest and the files it names are unpacked into `Packs/.incoming-<uuid>/`, each
///    file's SHA-256 checked as it is written; files it doesn't name are left out;
/// 4. the folder is renamed to `Packs/<id>.neopack` in one step, swapped with the installed
///    version if there is one, and the old version is deleted.
///
/// A dot folder without the `.neopack` extension is never scanned, so until the last step
/// nothing is visible to the keyboard, and whatever goes wrong before it leaves the installed
/// packs as they were.
public struct PackInstaller: Sendable {
    public enum Refusal: Error, Equatable, CustomStringConvertible {
        /// Not a zip the reader can use, or its entries are laid out wrongly.
        case archive(String)
        case noManifest
        /// `manifest.json` doesn't decode, or the keyboard can't read the pack it describes.
        case manifest(String)
        case format(Int)
        /// Not 1–16 of a-z, 0-9 and "-" starting with a letter, or "en" (the built-in English).
        case id(String)
        /// A script this engine has no rules for.
        case script(String)
        /// A script the engine has no copy of the rules for, and the pack brings none (no
        /// `lang` file).
        case noScriptRules(String)
        /// The engine can't use the unpacked pack (`PackInstaller.engineCheck`): why, in its words.
        case unusable(String)
        /// The pack's `minEngine`, above this engine's.
        case engine(Int)
        /// A name in `files` that isn't a plain file name.
        case fileName(String)
        /// No `lexicon` role, or its file isn't in the archive.
        case noLexicon
        /// A file in the archive that the manifest names but has no `sha256` for.
        case noChecksum(String)
        /// A `sha256` entry for a file that isn't in the archive.
        case missingFile(String)
        /// A file whose SHA-256 differs from the manifest's.
        case checksum(String)
        /// An older version than the installed one.
        case older(installed: String, offered: String)
        /// Writing the pack failed (disk full, permissions).
        case io(String)

        public var description: String {
            switch self {
            case .archive(let why): "not a usable pack archive: \(why)"
            case .noManifest: "no manifest.json in the archive"
            case .manifest(let why): "manifest.json: \(why)"
            case .format(let v): "pack format \(v); needs a newer NeoOne Keyboard"
            case .id(let id): "pack id \(id) is not allowed"
            case .script(let s): "script \(s); needs a newer NeoOne Keyboard"
            case .noScriptRules(let s): "script \(s) has no rules in the pack (lang.json)"
            case .unusable(let why): "the keyboard can't use this pack: \(why)"
            case .engine(let v): "pack needs engine \(v); needs a newer NeoOne Keyboard"
            case .fileName(let name): "file name \(name) is not allowed"
            case .noLexicon: "pack has no lexicon"
            case .noChecksum(let name): "\(name) has no checksum"
            case .missingFile(let name): "\(name) is missing"
            case .checksum(let name): "\(name) is damaged (checksum differs)"
            case .older(let installed, let offered): "version \(offered) is older than the installed \(installed)"
            case .io(let why): "could not write the pack: \(why)"
            }
        }
    }

    /// The prefix of an install's working folder in `packsFolder`.
    static let incomingPrefix = ".incoming-"

    public let packsFolder: URL
    public let support: PackSupport
    /// The engine's own check of an unpacked pack folder (MLEngine's `packOpenProblem`: the
    /// manifest, the script's rules from its `lang.json`, and the lexicon opened as the
    /// keyboard opens it): why it can't be used, or nil. Run before the pack is moved where
    /// the keyboard looks.
    public let engineCheck: (@Sendable (URL) -> String?)?

    public init(packsFolder: URL, support: PackSupport, engineCheck: (@Sendable (URL) -> String?)? = nil) {
        self.packsFolder = packsFolder
        self.support = support
        self.engineCheck = engineCheck
    }

    /// Installs the pack in `archive` (a `.neopack` zip) and returns it as installed.
    @discardableResult
    public func install(archive: URL) throws -> PackDescriptor {
        let fm = FileManager.default
        do {
            try fm.createDirectory(at: packsFolder, withIntermediateDirectories: true)
        } catch {
            throw Refusal.io(error.localizedDescription)
        }
        removeLeftovers()

        let zip: ZipReader
        do {
            zip = try ZipReader(url: archive)
        } catch {
            throw Refusal.archive(String(describing: error))
        }
        let files = try Self.packFiles(zip)
        guard let manifestEntry = files["manifest.json"] else { throw Refusal.noManifest }
        let manifestData = try Self.read(zip, manifestEntry)
        let manifest: Manifest
        do {
            manifest = try JSONDecoder().decode(Manifest.self, from: manifestData)
        } catch {
            throw Refusal.manifest(String(describing: error))
        }
        try check(manifest, files: files)

        let destination = packsFolder.appending(path: "\(manifest.id).neopack", directoryHint: .isDirectory)
        if let installed = try? PackDescriptor.load(folder: destination),
           PackVersion(manifest.version) < PackVersion(installed.version) {
            throw Refusal.older(installed: installed.version, offered: manifest.version)
        }

        let incoming = packsFolder.appending(path: Self.incomingPrefix + UUID().uuidString, directoryHint: .isDirectory)
        do {
            try unpack(zip, files: files, manifest: manifest, manifestData: manifestData, into: incoming)
            // What the keyboard will read, read the same way before it is put where it looks.
            do {
                _ = try PackDescriptor.load(folder: incoming)
            } catch {
                throw Refusal.manifest(String(describing: error))
            }
            if let why = engineCheck?(incoming) { throw Refusal.unusable(why) }
            try Self.moveIntoPlace(incoming, destination)
        } catch {
            try? fm.removeItem(at: incoming)
            throw error
        }
        do {
            return try PackDescriptor.load(folder: destination)
        } catch {
            throw Refusal.manifest(String(describing: error))
        }
    }

    /// Removes what an install that didn't finish left (`.incoming-*` folders).
    public func removeLeftovers() {
        let fm = FileManager.default
        let entries = (try? fm.contentsOfDirectory(atPath: packsFolder.path)) ?? []
        for name in entries where name.hasPrefix(Self.incomingPrefix) {
            try? fm.removeItem(at: packsFolder.appending(path: name))
        }
    }

    /// Removes the installed pack `id`, if there is one. It is first renamed out of sight, so
    /// the keyboard never sees part of it.
    public func remove(id: String) throws {
        let fm = FileManager.default
        let folder = packsFolder.appending(path: "\(id).neopack", directoryHint: .isDirectory)
        guard fm.fileExists(atPath: folder.path) else { return }
        let gone = packsFolder.appending(path: Self.incomingPrefix + UUID().uuidString, directoryHint: .isDirectory)
        do {
            try fm.moveItem(at: folder, to: gone)
        } catch {
            throw Refusal.io(error.localizedDescription)
        }
        try? fm.removeItem(at: gone)
    }

    // MARK: - Checks

    private struct Manifest: Decodable {
        let format: Int
        let id: String
        let version: String
        let script: String
        let minEngine: Int
        let files: [String: String]
        let sha256: [String: String]?
    }

    private func check(_ manifest: Manifest, files: [String: ZipReader.Entry]) throws {
        guard manifest.format == support.format else { throw Refusal.format(manifest.format) }
        guard Self.allowedID(manifest.id) else { throw Refusal.id(manifest.id) }
        if !support.scripts.contains(manifest.script) {
            // Any other script is read when the pack brings its rules (engine 4).
            guard support.packScripts else { throw Refusal.script(manifest.script) }
            guard let lang = manifest.files["lang"], files[lang] != nil else {
                throw Refusal.noScriptRules(manifest.script)
            }
        }
        guard manifest.minEngine <= support.engine else { throw Refusal.engine(manifest.minEngine) }
        for name in manifest.files.values.sorted() where !Self.plainName(name) {
            throw Refusal.fileName(name)
        }
        guard let lexicon = manifest.files["lexicon"], files[lexicon] != nil else { throw Refusal.noLexicon }
        let sums = manifest.sha256 ?? [:]
        for name in Set(manifest.files.values).sorted() where files[name] != nil && sums[name] == nil {
            throw Refusal.noChecksum(name)
        }
        for name in sums.keys.sorted() where files[name] == nil {
            throw Refusal.missingFile(name)
        }
    }

    /// 1–16 of a-z, 0-9 and "-", starting with a letter; never "en", the built-in English.
    static func allowedID(_ id: String) -> Bool {
        guard id != "en", (1 ... 16).contains(id.count), let first = id.first, ("a" ... "z").contains(first) else {
            return false
        }
        return id.allSatisfy { ("a" ... "z").contains($0) || ("0" ... "9").contains($0) || $0 == "-" }
    }

    /// A file name in the pack folder, never a path out of it (as the engine's `Pack::file`).
    static func plainName(_ name: String) -> Bool {
        !name.isEmpty && !name.contains("/") && !name.contains("\\") && !name.hasPrefix(".") && name != "manifest.json"
    }

    /// The archive's files by name in the pack folder. The archive holds either one folder
    /// ("hi.neopack/…", as `scripts/pack.py --zip` and Finder write it) or the files alone.
    /// What Finder adds beside them (`__MACOSX/`, `.DS_Store`, `._*`) is ignored.
    private static func packFiles(_ zip: ZipReader) throws -> [String: ZipReader.Entry] {
        let entries = zip.entries.filter { entry in
            let last = entry.components.last ?? ""
            return entry.components.first != "__MACOSX" && last != ".DS_Store" && !last.hasPrefix("._")
        }
        let depths = Set(entries.map(\.components.count))
        let roots = Set(entries.map { $0.components.count == 2 ? $0.components[0] : "" })
        guard depths.isSubset(of: [1]) || (depths == [2] && roots.count == 1) else {
            throw Refusal.archive("files must be in one folder")
        }
        var files: [String: ZipReader.Entry] = [:]
        for entry in entries {
            let name = entry.components.last!
            guard files[name] == nil else { throw Refusal.archive("\(name) is in the archive twice") }
            files[name] = entry
        }
        return files
    }

    private static func read(_ zip: ZipReader, _ entry: ZipReader.Entry) throws -> Data {
        do {
            return try zip.contents(of: entry)
        } catch {
            throw Refusal.archive(String(describing: error))
        }
    }

    // MARK: - Unpacking and moving

    private func unpack(_ zip: ZipReader, files: [String: ZipReader.Entry], manifest: Manifest,
                        manifestData: Data, into folder: URL) throws {
        let fm = FileManager.default
        do {
            try fm.createDirectory(at: folder, withIntermediateDirectories: false)
            try manifestData.write(to: folder.appending(path: "manifest.json"))
        } catch {
            throw Refusal.io(error.localizedDescription)
        }
        let sums = manifest.sha256 ?? [:]
        for name in Set(manifest.files.values).sorted() {
            guard let entry = files[name], let want = sums[name] else { continue }
            let data = try Self.read(zip, entry)
            let have = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
            guard have == want.lowercased() else { throw Refusal.checksum(name) }
            do {
                try data.write(to: folder.appending(path: name))
            } catch {
                throw Refusal.io(error.localizedDescription)
            }
        }
    }

    /// `incoming` becomes `destination` in one rename: swapped with the installed version when
    /// there is one (`RENAME_SWAP`), whose folder, now at `incoming`, is then deleted.
    private static func moveIntoPlace(_ incoming: URL, _ destination: URL) throws {
        let fm = FileManager.default
        let result: Int32 = incoming.withUnsafeFileSystemRepresentation { from in
            destination.withUnsafeFileSystemRepresentation { to in
                guard let from, let to else { return -1 }
                if fm.fileExists(atPath: destination.path) {
                    return renamex_np(from, to, UInt32(RENAME_SWAP))
                }
                return rename(from, to)
            }
        }
        guard result == 0 else {
            throw Refusal.io(String(cString: strerror(errno)))
        }
        // After a swap, the old version is at `incoming`; after a plain rename, nothing is.
        try? fm.removeItem(at: incoming)
    }
}

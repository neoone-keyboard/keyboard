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

    public init(id: String, name: String, nativeName: String, script: String, romanisedName: String? = nil,
                credits: [Credit] = [], layout: PackLayout?, folder: URL) {
        self.id = id
        self.name = name
        self.nativeName = nativeName
        self.script = script
        self.romanisedName = romanisedName
        self.credits = credits
        self.layout = layout
        self.folder = folder
    }

    /// One pack per language id.
    public static func == (lhs: PackDescriptor, rhs: PackDescriptor) -> Bool { lhs.id == rhs.id }
    public func hash(into hasher: inout Hasher) { hasher.combine(id) }

    public struct Refused: Error, CustomStringConvertible {
        public let description: String
    }

    /// Reads `folder/manifest.json` and the layout it names. A manifest that is missing,
    /// malformed or of another format refuses the pack; a bad layout only leaves it without one.
    public static func load(folder: URL) throws -> PackDescriptor {
        let manifest = try JSONDecoder().decode(Manifest.self, from: Data(contentsOf: folder.appending(path: "manifest.json")))
        guard manifest.format == 1 else { throw Refused(description: "pack format \(manifest.format), this keyboard reads 1") }
        // A file name, never a path out of the pack (as the engine's `Pack::file`).
        let layout = manifest.files["layout"]
            .flatMap { name in name.isEmpty || name.contains("/") || name.hasPrefix(".") ? nil : name }
            .flatMap { try? PackLayout.load(folder.appending(path: $0)) }
        return PackDescriptor(
            id: manifest.id, name: manifest.name, nativeName: manifest.nativeName, script: manifest.script,
            romanisedName: manifest.romanised?.name,
            credits: manifest.credits?.map(\.credit) ?? [],
            layout: layout, folder: folder
        )
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
        let name: String
        let nativeName: String
        let script: String
        let romanised: Romanised?
        let files: [String: String]
        let credits: [PackCredit]?
    }
}

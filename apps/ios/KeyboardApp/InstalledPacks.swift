import Foundation
import KeyboardCore
import MLEngine

/// The app's side of language packs (M6 phase 3): it installs them into the App Group, which
/// the keyboard reads. The logic is in KeyboardCore (`PackInstaller`, `PackImport`,
/// `PackUpdates`); this adds the App Group folder, the keyboard's bundle and what its engine
/// reads. The Languages and first-launch screens use it through `LanguagesModel`.
enum InstalledPacks {
    /// The App Group isn't available to this build (an unsigned simulator build).
    struct NoAppGroup: Error, CustomStringConvertible {
        var description: String { "the App Group \(PackLocations.appGroup) is not available" }
    }

    /// What the keyboard's engine reads: the same library is linked into the app.
    static var support: KeyboardCore.PackSupport {
        let engine = MLEngine.packSupport()
        return KeyboardCore.PackSupport(format: Int(engine.format), engine: Int(engine.engine),
                                        scripts: Set(engine.scripts), packScripts: engine.packScripts)
    }

    /// The installer for the App Group's `Packs/`; nil without the App Group. The engine opens
    /// each pack before it is installed (`packOpenProblem`), so a pack it can't use (a bad
    /// `lang.json`, a lexicon that isn't one) is refused with its reason.
    static var installer: PackInstaller? {
        PackLocations.installedPacks().map {
            PackInstaller(packsFolder: $0, support: support, engineCheck: { folder in
                (try? MLEngine.packOpenProblem(packDir: folder.path)) ?? nil
            })
        }
    }

    /// The keyboard extension inside this app.
    static var keyboardBundle: URL? {
        Bundle.main.builtInPlugInsURL?.appending(path: "KeyboardExtension.appex", directoryHint: .isDirectory)
    }

    /// Installs the `.neopack` the document picker returned
    /// (`UIDocumentPickerViewController(forOpeningContentTypes: [PackImport.contentType])`).
    /// Throws `PackInstaller.Refusal` with the reason when the pack can't be used.
    @discardableResult
    static func importPack(from url: URL) throws -> PackDescriptor {
        guard let installer else { throw NoAppGroup() }
        return try PackImport.install(from: url, with: installer)
    }

    /// The packs installed in the App Group.
    static func installed() -> [PackDescriptor] {
        PackLocations.installedPacks().map(PackDescriptor.scan) ?? []
    }

    /// The packs the keyboard bundles (`packs/` in the extension).
    static func bundled() -> [PackDescriptor] {
        #if DEBUG
        // Screenshots of the state a release build without Malayalam will have.
        if ProcessInfo.processInfo.arguments.contains("-hideBundledPacks") { return [] }
        #endif
        return keyboardBundle.map { PackDescriptor.scan($0.appending(path: "packs", directoryHint: .isDirectory)) } ?? []
    }

    /// Every pack the keyboard loads, as it merges them (`KeyboardPacks.read`).
    static func all() -> [PackDescriptor] {
        PackDescriptor.merge(bundled: bundled(), installed: installed())
    }

    /// The fingerprint of the keyboard's `en.fst`, which packs name as `englishFingerprint`.
    static func englishFingerprint() -> String? {
        keyboardBundle.flatMap { try? EnglishFingerprint.of($0.appending(path: "Lexicon/en.fst")) }
    }

    /// The updates to offer for the user's packs, given the entries of a pack index.
    static func updateOffers(available: [AvailablePack]) -> [PackOffer] {
        guard let fingerprint = englishFingerprint() else { return [] }
        return PackUpdates.offers(installed: all(), available: available, englishFingerprint: fingerprint, support: support)
    }

    /// Removes the pack `id` from the App Group (a bundled one stays).
    static func remove(id: String) throws {
        guard let installer else { throw NoAppGroup() }
        try installer.remove(id: id)
    }

    /// The bytes of English as the keyboard bundles it (`en.fst`, `en-next.fst`).
    static func englishSize() -> Int64 {
        guard let lexicon = keyboardBundle?.appending(path: "Lexicon", directoryHint: .isDirectory) else { return 0 }
        return PackSize.of(files: ["en.fst", "en-next.fst"].map { lexicon.appending(path: $0) })
    }

    /// The user's active languages as stored for the keyboard; nil when none is stored yet (the
    /// keyboard then uses the engine's default).
    static func storedChoice() -> ActiveLanguages? {
        PackLocations.activeLanguagesFile().flatMap(ActiveLanguages.read(from:))
    }

    /// Stores the active languages where the keyboard reads them when its engine opens.
    static func store(_ choice: ActiveLanguages) throws {
        guard let file = PackLocations.activeLanguagesFile() else { throw NoAppGroup() }
        try choice.write(to: file)
    }

    /// The pack index to download from (Info.plist `NeoPackIndexURL`, build setting
    /// `ML_PACK_INDEX_URL`); nil while it is empty, as it is until a host is chosen.
    static var indexURL: URL? {
        let text = (Bundle.main.object(forInfoDictionaryKey: "NeoPackIndexURL") as? String ?? "")
            .trimmingCharacters(in: .whitespaces)
        guard !text.isEmpty, let url = URL(string: text), url.scheme != nil else { return nil }
        return url
    }
}

import Foundation
import KeyboardCore
import MLEngine

/// The app's side of language packs (M6 phase 3): it installs them into the App Group, which
/// the keyboard reads. The logic is in KeyboardCore (`PackInstaller`, `PackImport`,
/// `PackUpdates`); this adds the App Group folder, the keyboard's bundle and what its engine
/// reads. No screen calls it yet: the Languages and first-launch screens are still to be
/// designed.
enum InstalledPacks {
    /// The App Group isn't available to this build (an unsigned simulator build).
    struct NoAppGroup: Error, CustomStringConvertible {
        var description: String { "the App Group \(PackLocations.appGroup) is not available" }
    }

    /// What the keyboard's engine reads: the same library is linked into the app.
    static var support: KeyboardCore.PackSupport {
        let engine = MLEngine.packSupport()
        return KeyboardCore.PackSupport(format: Int(engine.format), engine: Int(engine.engine), scripts: Set(engine.scripts))
    }

    /// The installer for the App Group's `Packs/`; nil without the App Group.
    static var installer: PackInstaller? {
        PackLocations.installedPacks().map { PackInstaller(packsFolder: $0, support: support) }
    }

    /// The keyboard extension inside this app.
    private static var keyboardBundle: URL? {
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
        keyboardBundle.map { PackDescriptor.scan($0.appending(path: "packs", directoryHint: .isDirectory)) } ?? []
    }

    /// Every pack the keyboard loads, as it merges them (`EngineSuggestionProvider.packs`).
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
}

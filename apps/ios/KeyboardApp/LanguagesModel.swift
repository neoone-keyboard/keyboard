import Foundation
import KeyboardCore
import Observation
import os

/// What the Languages and first-launch screens show and do: English and the keyboard's packs,
/// which are active, import, remove, and downloads from the pack index when one is configured.
@MainActor
@Observable
final class LanguagesModel {
    struct Row: Identifiable, Equatable {
        let id: String
        /// In English: "Malayalam".
        let name: String
        /// In its own script: "മലയാളം".
        let nativeName: String
        /// Nil for English, the base, which has no pack version.
        let version: String?
        let size: Int64
        /// The keyboard uses the copy inside the app.
        let builtIn: Bool
        /// An installed copy is in the App Group, so it can be removed.
        let removable: Bool
        /// What the keyboard's language key shows for it ("EN", "മ"); nil when its pack has no
        /// layout of its own.
        let languageKey: String?
    }

    enum IndexState: Equatable {
        case idle
        case loading
        case loaded(PackIndex)
        case failed(String)
    }

    struct Message: Identifiable {
        let id = UUID()
        let title: String
        let text: String
    }

    private let log = Logger(subsystem: "me.jishnu.keyboard", category: "languages")

    private(set) var rows: [Row] = []
    /// The active languages: the stored choice, else the engine's default for these packs.
    private(set) var active = ActiveLanguages([ActiveLanguages.english])
    private(set) var offers: [PackOffer] = []
    private(set) var index: IndexState = .idle
    /// Pack ids being downloaded.
    private(set) var downloading: Set<String> = []
    private(set) var importing = false
    var message: Message?

    /// The pack index; nil hides the download section.
    let indexURL: URL? = InstalledPacks.indexURL

    init() {
        reload()
    }

    /// Reads the packs and the choice again: after an install or a removal, and whenever the
    /// app becomes active or the keyboard goes away, since the keyboard's settings panel
    /// switches languages in the App Group behind the app's back.
    func reload() {
        let installedIDs = Set(InstalledPacks.installed().map(\.id))
        let installedFolder = PackLocations.installedPacks()?.standardizedFileURL.path
        let packs = InstalledPacks.all()
        let english = Row(id: ActiveLanguages.english, name: "English", nativeName: "English", version: nil,
                          size: InstalledPacks.englishSize(), builtIn: true, removable: false, languageKey: "EN")
        rows = [english] + packs.map { pack in
            let inGroup = installedFolder.map { pack.folder.standardizedFileURL.path.hasPrefix($0) } ?? false
            return Row(id: pack.id, name: pack.name, nativeName: pack.nativeName, version: pack.version,
                       size: PackSize.of(folder: pack.folder), builtIn: !inGroup,
                       removable: installedIDs.contains(pack.id), languageKey: pack.layout?.languageKey)
        }
        let packIDs = packs.map(\.id)
        active = .shown(storedIn: PackLocations.activeLanguagesFile(), packIDs: packIDs)
        if case .loaded(let index) = index {
            offers = InstalledPacks.updateOffers(available: index.packs.map(\.available))
        }
    }

    // MARK: - The active choice

    var order: [String] { rows.map(\.id) }

    func isOn(_ id: String) -> Bool { active.isOn(id) }

    /// Whether the toggle for `id` can be switched now.
    func canToggle(_ id: String) -> Bool {
        active.isOn(id) ? active.canTurnOff(id) : active.canTurnOn(id)
    }

    /// Switches `id` and stores the result for the keyboard.
    func toggle(_ id: String) {
        let next = active.toggling(id, order: order)
        guard next != active else { return }
        active = next
        store()
    }

    /// Stores the shown choice (the first-launch step stores it even when it is the default).
    func store() {
        do {
            try InstalledPacks.store(active)
        } catch {
            log.error("Storing active languages failed: \(String(describing: error), privacy: .public)")
            message = Message(title: "Couldn't save your languages", text: PackMessages.unavailable)
        }
    }

    // MARK: - Import and remove

    /// Installs the `.neopack` the document picker returned.
    func importPack(from url: URL) {
        importing = true
        Task {
            let result = await Task.detached { () -> Result<PackDescriptor, Error> in
                Result { try InstalledPacks.importPack(from: url) }
            }.value
            importing = false
            finishInstall(result, title: "Couldn't add the language")
        }
    }

    /// Removes the installed pack `id` and takes it out of the active choice.
    func remove(_ id: String) {
        do {
            try InstalledPacks.remove(id: id)
        } catch {
            log.error("Removing \(id, privacy: .public) failed: \(String(describing: error), privacy: .public)")
            message = Message(title: "Couldn't remove the language", text: Self.text(for: error))
            return
        }
        reload()
        // A bundled copy of the same id may remain; only a language that is gone leaves the
        // stored choice (the keyboard would skip it anyway, but a later import shouldn't bring
        // it back on).
        if !rows.contains(where: { $0.id == id }), let stored = InstalledPacks.storedChoice(), stored.isOn(id) {
            active = stored.removing(id).usable(packIDs: order).map(ActiveLanguages.init) ?? active
            store()
        }
    }

    // MARK: - Downloads

    /// Fetches the index (when one is configured), for the download section and the updates.
    func loadIndex() async {
        guard let indexURL, index != .loading else { return }
        index = .loading
        do {
            let fetched = try await PackDownloader().index(at: indexURL)
            index = .loaded(fetched)
        } catch {
            log.error("Pack index failed: \(String(describing: error), privacy: .public)")
            index = .failed(Self.text(for: error))
        }
        reload()
    }

    /// The index's packs this build reads that the user doesn't have, best version per id.
    var downloadable: [PackIndex.Entry] {
        guard case .loaded(let index) = index else { return [] }
        let have = Set(rows.map(\.id))
        let support = InstalledPacks.support
        var best: [String: PackIndex.Entry] = [:]
        for entry in index.packs where !have.contains(entry.id) && entry.isReadable(by: support) {
            if let other = best[entry.id], !(PackVersion(other.version) < PackVersion(entry.version)) { continue }
            best[entry.id] = entry
        }
        return best.values.sorted { $0.name < $1.name }
    }

    func offer(for id: String) -> PackOffer? { offers.first { $0.id == id } }

    /// The index entry of `offer`, to download.
    func entry(for offer: PackOffer) -> PackIndex.Entry? {
        guard case .loaded(let index) = index else { return nil }
        return index.packs.first { $0.id == offer.id && $0.version == offer.offered.version }
    }

    func download(_ entry: PackIndex.Entry) {
        guard let installer = InstalledPacks.installer else {
            message = Message(title: "Couldn't add the language", text: PackMessages.unavailable)
            return
        }
        downloading.insert(entry.id)
        Task {
            let result = await Task.detached { () -> Result<PackDescriptor, Error> in
                do {
                    return .success(try await PackDownloader().install(entry, with: installer))
                } catch {
                    return .failure(error)
                }
            }.value
            downloading.remove(entry.id)
            finishInstall(result, title: "Couldn't download \(entry.name)")
        }
    }

    /// After an import or a download: the list again, and the new language turned on when there
    /// is room (an update leaves the choice as it was).
    private func finishInstall(_ result: Result<PackDescriptor, Error>, title: String) {
        switch result {
        case .success(let pack):
            let wasListed = rows.contains { $0.id == pack.id }
            reload()
            if !wasListed, active.canTurnOn(pack.id) {
                toggle(pack.id)
            }
        case .failure(let error):
            log.error("Install failed: \(String(describing: error), privacy: .public)")
            message = Message(title: title, text: Self.text(for: error))
        }
    }

    /// The plain sentence for `error`.
    private static func text(for error: Error) -> String {
        error is InstalledPacks.NoAppGroup ? PackMessages.unavailable : PackMessages.text(for: error)
    }
}

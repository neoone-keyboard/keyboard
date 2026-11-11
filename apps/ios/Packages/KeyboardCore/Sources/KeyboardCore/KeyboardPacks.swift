import Foundation

/// The language packs the keyboard has and which of its languages are on, as last read
/// (`read`): what the engine opens, what the language key cycles, what the settings panel
/// offers and which scripts autocorrect leaves alone all follow from it.
public struct KeyboardPacks: Equatable, Sendable {
    /// Those in the extension bundle (`packs/`) and those the app installed in the App Group
    /// (`PackLocations`), one per id: an installed pack only replaces a bundled one with a
    /// higher version (`PackDescriptor.merge`). A pack the engine then can't load is left out
    /// with a reported problem, and the keys follow the engine's layouts, so the keyboard
    /// carries on with English and the packs that did load.
    public var packs: [PackDescriptor]
    /// The active languages set when the engine opens (`active(app:keyboard:packIDs:debugList:)`);
    /// nil for the engine's default.
    public var active: [String]?

    public init(packs: [PackDescriptor], active: [String]?) {
        self.packs = packs
        self.active = active
    }

    /// Reads the packs in the two folders and the stored choices. `bundledPacks` is the
    /// extension bundle's `packs/`, `installedPacks` the App Group's (`PackLocations`),
    /// `appChoice` the App Group's `active-languages.json`, `keyboardChoice` the keyboard's
    /// own (`KeyboardLanguageChoice`), `debugList` a debug build's Info.plist
    /// `MLActiveLanguages` ("ml,hi", from `make twopacks`; empty otherwise).
    public static func read(bundledPacks: URL?, installedPacks: URL?, appChoice: URL?,
                            keyboardChoice: DatedLanguages?, debugList: String) -> KeyboardPacks {
        let packs = PackDescriptor.merge(
            bundled: bundledPacks.map(PackDescriptor.scan) ?? [],
            installed: installedPacks.map(PackDescriptor.scan) ?? []
        )
        let active = active(app: appChoice.flatMap(DatedLanguages.read(from:)), keyboard: keyboardChoice,
                            packIDs: packs.map(\.id), debugList: debugList)
        return KeyboardPacks(packs: packs, active: active)
    }

    /// The user's latest choice, the newer of the app's (dated by its file's modification
    /// date) and the keyboard's own (made in its settings panel), its ids the keyboard has no
    /// pack for left out (`DatedLanguages.newer`, `ActiveLanguages.usable`); else `debugList`;
    /// else nil, the engine's default.
    public static func active(app: DatedLanguages?, keyboard: DatedLanguages?, packIDs: [String],
                              debugList: String) -> [String]? {
        if let stored = DatedLanguages.newer(app: app, keyboard: keyboard), let ids = stored.usable(packIDs: packIDs) {
            return ids
        }
        let ids = debugList.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        return ids.isEmpty ? nil : ids
    }

    /// The active languages as the settings panel shows them: the configured list, else the
    /// engine's default.
    public var activeChoice: ActiveLanguages {
        active.map(ActiveLanguages.init) ?? ActiveLanguages.engineDefault(packIDs: packs.map(\.id))
    }

    /// The packs whose languages are on (`activeChoice`), with or without a keyboard: the
    /// scripts autocorrect leaves alone (`InputController.activePacks`) and the emoji words
    /// read (`EmojiWordLists`).
    public var activePacks: [PackDescriptor] {
        let choice = activeChoice
        return packs.filter { choice.isOn($0.id) }
    }

    /// The languages the settings panel offers: English, then every pack, by native name.
    public var availableLanguages: [(id: String, name: String)] {
        [(ActiveLanguages.english, "English")] + packs.map { ($0.id, $0.nativeName) }
    }

    /// The languages the language key cycles before the engine has said: English, then the
    /// first pack with a keyboard, as the engine's default active list has it (or the
    /// configured list's packs with a keyboard).
    public var defaultLanguages: [Language] {
        guard let active else {
            return [.english] + packs.prefix(1).filter { $0.layout != nil }.map(Language.pack)
        }
        return [.english] + active.compactMap(language(forPack:))
    }

    /// The language of the pack `id`, when the keyboard can draw it (it has a layout).
    public func language(forPack id: String) -> Language? {
        packs.first { $0.id == id && $0.layout != nil }.map(Language.pack)
    }

    /// What the engine is given for a choice made in the keyboard's settings panel: the ids
    /// it can load, else its default for these packs.
    public func engineLanguages(for choice: ActiveLanguages) -> [String] {
        let packIDs = packs.map(\.id)
        return choice.usable(packIDs: packIDs) ?? ActiveLanguages.engineDefault(packIDs: packIDs).languages
    }

    /// What the open engine is given when only the choice changed: the configured list, else
    /// its default for these packs.
    public var engineLanguages: [String] {
        active ?? ActiveLanguages.engineDefault(packIDs: packs.map(\.id)).languages
    }

    /// What reading the packs again found, against what was read before
    /// (`EngineSuggestionProvider.reloadIfChanged`).
    public enum Change: Equatable, Sendable {
        case none
        /// The packs differ: the engine reopens with the new ones.
        case packs
        /// Only the choice changed: it is set on the open engine.
        case choice
    }

    /// The packs differ when one came or went, or one is now another version or in the other
    /// folder (`PackDescriptor.files`): the engine has the old one's files open. Packs
    /// themselves are equal by id alone, so they are not compared here.
    public func change(from old: KeyboardPacks) -> Change {
        if packs.map(\.files) != old.packs.map(\.files) { return .packs }
        return active == old.active ? .none : .choice
    }
}

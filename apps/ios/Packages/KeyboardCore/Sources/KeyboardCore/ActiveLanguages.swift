import Foundation

/// The languages the user turned on in the app (M6 spec decision 4): ids in order, "en" for
/// English. The app writes it to the App Group (`PackLocations.activeLanguagesFile`); the
/// keyboard reads it when its engine opens and sets it (`Engine.setActiveLanguages`). No file
/// means no choice: the engine keeps its default, English and the first pack.
public struct ActiveLanguages: Sendable, Equatable, Codable {
    /// The engine's `MAX_ACTIVE` (engine/core/src/lang.rs), English counted. The FFI doesn't
    /// expose it; `theLimitIsTheEngines` pins the copy.
    public static let maxActive = 4
    public static let english = "en"

    public let languages: [String]

    public init(_ languages: [String]) {
        self.languages = languages
    }

    /// The engine's default for these packs (in the keyboard's order): English and the first
    /// pack. More languages are the user's choice.
    public static func engineDefault(packIDs: [String]) -> ActiveLanguages {
        ActiveLanguages([english] + packIDs.prefix(1))
    }

    /// The stored choice; nil when there is none, it doesn't decode or it is empty.
    public static func read(from file: URL) -> ActiveLanguages? {
        guard let data = try? Data(contentsOf: file),
              let choice = try? JSONDecoder().decode(ActiveLanguages.self, from: data),
              !choice.languages.isEmpty else { return nil }
        return choice
    }

    /// Writes the choice in one step, so the keyboard never reads half a file.
    public func write(to file: URL) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys]
        try FileManager.default.createDirectory(at: file.deletingLastPathComponent(), withIntermediateDirectories: true)
        try encoder.encode(self).write(to: file, options: .atomic)
    }

    /// What the keyboard sets: the ids it can load (English, or one of `packIDs`), each once,
    /// at most `maxActive`; nil when none is left, and the engine keeps its default.
    public func usable(packIDs: [String]) -> [String]? {
        var seen = Set<String>()
        let ids = languages.filter { ($0 == Self.english || packIDs.contains($0)) && seen.insert($0).inserted }
        return ids.isEmpty ? nil : Array(ids.prefix(Self.maxActive))
    }

    public func isOn(_ id: String) -> Bool { languages.contains(id) }

    public func canTurnOn(_ id: String) -> Bool { isOn(id) || languages.count < Self.maxActive }

    /// At least one language stays on.
    public func canTurnOff(_ id: String) -> Bool { !isOn(id) || languages.count > 1 }

    /// Whether `id`'s switch does anything: off while full, or on and the last one.
    public func canToggle(_ id: String) -> Bool { isOn(id) ? canTurnOff(id) : canTurnOn(id) }

    /// `id` switched, when the rules allow it; the result is in `order` (the Languages list).
    public func toggling(_ id: String, order: [String]) -> ActiveLanguages {
        if isOn(id) {
            guard canTurnOff(id) else { return self }
            return ActiveLanguages(languages.filter { $0 != id })
        }
        guard canTurnOn(id) else { return self }
        let on = Set(languages + [id])
        let ordered = order.filter(on.contains)
        // Ids not in `order` (none in practice) keep their place after it.
        return ActiveLanguages(ordered + languages.filter { !order.contains($0) && $0 != id })
    }

    /// Without the pack `id` (it was removed); English alone when nothing is left.
    public func removing(_ id: String) -> ActiveLanguages {
        let rest = languages.filter { $0 != id }
        return ActiveLanguages(rest.isEmpty ? [Self.english] : rest)
    }
}

/// A language choice and when it was made: the App Group file's (its modification date) or the
/// keyboard's own copy (`KeyboardLanguageChoice`). The keyboard has no Full Access, and whether
/// it may write the App Group is untested, so a choice made in the keyboard is also kept in its
/// own container; the newer of the two is the user's latest choice, wherever it was made.
public struct DatedLanguages: Sendable, Equatable, Codable {
    public let choice: ActiveLanguages
    public let date: Date

    public init(_ choice: ActiveLanguages, date: Date) {
        self.choice = choice
        self.date = date
    }

    /// The App Group file's choice, dated by its modification date; nil like
    /// `ActiveLanguages.read`, or when the date can't be read.
    public static func read(from file: URL) -> DatedLanguages? {
        guard let choice = ActiveLanguages.read(from: file),
              let date = (try? file.resourceValues(forKeys: [.contentModificationDateKey]))?
                .contentModificationDate else { return nil }
        return DatedLanguages(choice, date: date)
    }

    /// The later of the app's and the keyboard's choice; on the same date the app's. Nil when
    /// neither exists.
    public static func newer(app: DatedLanguages?, keyboard: DatedLanguages?) -> ActiveLanguages? {
        switch (app, keyboard) {
        case let (app?, keyboard?): keyboard.date > app.date ? keyboard.choice : app.choice
        case let (app?, nil): app.choice
        case let (nil, keyboard?): keyboard.choice
        case (nil, nil): nil
        }
    }
}

/// The choice made in the keyboard's settings panel, with when, in the keyboard's own defaults.
public enum KeyboardLanguageChoice {
    static let key = "activeLanguages"

    public static func load(from defaults: UserDefaults) -> DatedLanguages? {
        guard let data = defaults.data(forKey: key),
              let dated = try? JSONDecoder().decode(DatedLanguages.self, from: data),
              !dated.choice.languages.isEmpty else { return nil }
        return dated
    }

    public static func save(_ dated: DatedLanguages, to defaults: UserDefaults) {
        guard let data = try? JSONEncoder().encode(dated) else { return }
        defaults.set(data, forKey: key)
    }
}

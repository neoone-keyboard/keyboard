import Foundation

/// The languages the user turned on in the app (M6 spec decision 4): ids in order, "en" for
/// English. The app writes it to the App Group (`PackLocations.activeLanguagesFile`); the
/// keyboard reads it when its engine opens and sets it (`Engine.setActiveLanguages`). No file
/// means no choice: the engine keeps its default, English and the first pack.
public struct ActiveLanguages: Sendable, Equatable, Codable {
    /// The engine's `MAX_ACTIVE` (engine/core/src/lang.rs), English counted. The FFI doesn't
    /// expose it; `theLimitIsTheEngines` pins the copy.
    public static let maxActive = 2
    public static let english = "en"

    public let languages: [String]

    public init(_ languages: [String]) {
        self.languages = languages
    }

    /// The engine's default for these packs (in the keyboard's order): English, then packs,
    /// up to `maxActive`.
    public static func engineDefault(packIDs: [String]) -> ActiveLanguages {
        ActiveLanguages(Array(([english] + packIDs).prefix(maxActive)))
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

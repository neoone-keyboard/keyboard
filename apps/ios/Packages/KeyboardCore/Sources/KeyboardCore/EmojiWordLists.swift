import Foundation

/// The keyboard's word → emoji lists, memory-mapped: the app's own file (English), then each
/// active pack's (`PackDescriptor.emojiWords`, `emojiSearch`). A pack published before packs
/// carried their emoji words gets the app's copy for its id, if the bundle has one
/// (`Emoji/packs/<id>/`: Malayalam's). Each list is held until the active packs change.
public struct EmojiWordLists {
    public enum List: String, Sendable, CaseIterable {
        /// The bar's word → emoji list.
        case bar = "emoji-words"
        /// The panel's search words.
        case search = "emoji-search"

        /// The pack's own file for this list, when it brings one.
        func file(of pack: PackDescriptor) -> URL? {
            switch self {
            case .bar: pack.emojiWords
            case .search: pack.emojiSearch
            }
        }
    }

    /// The bundle's `Emoji/` folder; nil when the bundle has none.
    private let folder: URL?
    private var held: [List: (packs: [String], words: EmojiWords?)] = [:]

    public init(folder: URL?) {
        self.folder = folder
    }

    /// The files of `list` for `packs`: the app's own, then one per pack in the packs' order:
    /// the pack's own, else the app's copy for its id (which may not exist).
    public static func files(_ list: List, in folder: URL, packs: [PackDescriptor]) -> (app: URL, packs: [URL]) {
        let name = "\(list.rawValue).txt"
        return (folder.appending(path: name),
                packs.map { list.file(of: $0) ?? folder.appending(path: "packs/\($0.id)/\(name)") })
    }

    /// The list for the active `packs`; nil if the app's file is missing from the bundle. A
    /// pack's file that can't be read is left out.
    public mutating func words(_ list: List, packs: [PackDescriptor]) -> EmojiWords? {
        let key = packs.map { "\($0.id) \($0.version)" }
        if let held = held[list], held.packs == key { return held.words }
        let mapped = { (url: URL) in try? Data(contentsOf: url, options: .alwaysMapped) }
        let files = folder.map { Self.files(list, in: $0, packs: packs) }
        let words = files.flatMap { files in
            mapped(files.app).map { EmojiWords(files: [$0] + files.packs.compactMap(mapped)) }
        }
        held[list] = (key, words)
        return words
    }
}

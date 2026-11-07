import Foundation
import KeyboardCore

/// The repository's packs/ folder, found from this file's path
/// (…/apps/ios/Packages/KeyboardCore/Tests/KeyboardCoreTests/).
let packsFolder = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent()
    .appending(path: "packs")

/// packs/ml.neopack, as the keyboard bundles it.
let malayalamPack: PackDescriptor = {
    do {
        return try PackDescriptor.load(folder: packsFolder.appending(path: "ml.neopack"))
    } catch {
        fatalError("packs/ml.neopack: \(error)")
    }
}()

let malayalam = Language.pack(malayalamPack)

/// packs/hi.neopack and packs/as.neopack, as an installed pack would be read.
func pack(_ id: String) -> PackDescriptor {
    do {
        return try PackDescriptor.load(folder: packsFolder.appending(path: "\(id).neopack"))
    } catch {
        fatalError("packs/\(id).neopack: \(error)")
    }
}

/// The user's languages today: English, then Malayalam.
let englishAndMalayalam: [Language] = [.english, malayalam]

/// Malayalam's layout pages by id.
func mlPage(_ id: String) -> KeyboardPage { .native(id) }

func mlLayout(_ page: KeyboardPage, showsNextKeyboard: Bool = true) -> KeyboardLayout {
    .forPage(page, native: malayalamPack.layout, showsNextKeyboard: showsNextKeyboard)
}

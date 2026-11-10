import Foundation

/// What the app tells the user when a pack can't be added (M6 phase 3): one plain sentence per
/// kind of failure, never the technical reason, which goes to the log (`String(describing:)`).
public enum PackMessages {
    public static let damaged = "This pack is damaged. Download it again."
    public static let notAPack = "This file isn't a NeoOne Keyboard language pack."
    public static let newerApp = "This pack needs a newer NeoOne Keyboard. Update the app, then try again."
    public static let badPack = "This pack wasn't made correctly, so it can't be added."
    public static let noRoom = "The pack couldn't be saved. Check that your iPhone has free space, then try again."
    public static let offline = "The pack couldn't be downloaded. Check your connection, then try again."
    public static let server = "The pack isn't available for download right now. Try again later."
    public static let unavailable = "Languages can't be added in this build of the app."
    public static let other = "The pack couldn't be added. Try again."

    /// The sentence for `error`, from the installer, the downloader or the index.
    public static func text(for error: Error) -> String {
        switch error {
        case let refusal as PackInstaller.Refusal: text(for: refusal)
        case let failure as PackDownloader.Failure: text(for: failure)
        case is PackIndex.Refusal: server
        default: other
        }
    }

    public static func text(for refusal: PackInstaller.Refusal) -> String {
        switch refusal {
        // A zip that is cut short or corrupt is, to the user, a damaged download; a file that is
        // no zip at all usually was picked by mistake, but the reader can't tell the two apart.
        case .archive: "This file is damaged or isn't a language pack. Download it again."
        case .noManifest: notAPack
        case .manifest, .checksum, .missingFile: damaged
        case .format, .script, .engine: newerApp
        case .id, .fileName, .noLexicon, .noChecksum, .noScriptRules, .unusable: badPack
        case .older(let installed, _): "You already have a newer version of this language (\(installed))."
        case .io: noRoom
        }
    }

    public static func text(for failure: PackDownloader.Failure) -> String {
        switch failure {
        case .network: offline
        case .http, .insecure: server
        case .size, .checksum: damaged
        }
    }
}

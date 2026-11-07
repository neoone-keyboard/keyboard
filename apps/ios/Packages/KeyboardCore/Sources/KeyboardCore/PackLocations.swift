import Foundation

/// Where installed packs live (M6 spec, "Where packs live"): `Packs/` in the App Group
/// container that the app and the keyboard share. The app writes it (`PackInstaller`); the
/// keyboard only reads it, which works without Full Access (tested on iOS 26; undocumented, so
/// the keyboard keeps working from its bundle and English if it ever stops).
public enum PackLocations {
    /// The App Group on both targets (project.yml).
    public static let appGroup = "group.me.jishnu.keyboard"
    public static let packsFolderName = "Packs"

    /// The App Group's `Packs/` folder; nil when this process has no App Group (an unsigned
    /// simulator build). It may not exist yet: `PackDescriptor.scan` then finds nothing and the
    /// installer creates it.
    public static func installedPacks(fileManager: FileManager = .default) -> URL? {
        fileManager.containerURL(forSecurityApplicationGroupIdentifier: appGroup)?
            .appending(path: packsFolderName, directoryHint: .isDirectory)
    }

    public static let activeLanguagesFileName = "active-languages.json"

    /// The user's active languages (`ActiveLanguages`), at the App Group's root; nil without the
    /// App Group. The app writes it; the keyboard reads it when its engine opens.
    public static func activeLanguagesFile(fileManager: FileManager = .default) -> URL? {
        fileManager.containerURL(forSecurityApplicationGroupIdentifier: appGroup)?
            .appending(path: activeLanguagesFileName)
    }
}

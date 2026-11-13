import Foundation

/// Where installed packs live (M6 spec, "Where packs live"): `Packs/` in the App Group
/// container that the app and the keyboard share. The app writes it (`PackInstaller`); the
/// keyboard only reads it, which works without Full Access (tested on iOS 26; undocumented, so
/// the keyboard keeps working from its bundle and English if it ever stops).
public enum PackLocations {
    /// The App Group on both targets: `NeoAppGroup` in the app's and the keyboard's Info.plist,
    /// the APP_GROUP build setting (apps/ios/Signing.xcconfig). A process with no such key (the
    /// package's tests on the Mac) gets `defaultAppGroup`.
    public static let appGroup: String = appGroup(infoDictionary: Bundle.main.infoDictionary)

    /// The published app's App Group, Signing.xcconfig's default.
    public static let defaultAppGroup = "group.me.jishnu.keyboard"
    static let appGroupInfoKey = "NeoAppGroup"

    /// The App Group an Info.plist names, else `defaultAppGroup`.
    static func appGroup(infoDictionary: [String: Any]?) -> String {
        guard let value = infoDictionary?[appGroupInfoKey] as? String, !value.isEmpty else { return defaultAppGroup }
        return value
    }
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

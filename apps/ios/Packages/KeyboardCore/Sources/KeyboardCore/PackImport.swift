import Foundation
import UniformTypeIdentifiers

/// Installing a pack the user picked in Files (M6 spec, "First launch": "import a `.neopack`
/// from Files"). The app's document picker hands over a URL; this reads it and runs the
/// installer. No screen calls it yet.
public enum PackImport {
    /// The `.neopack` type the app declares (project.yml, `UTExportedTypeDeclarations`): a zip,
    /// so a picker for it also offers zips.
    public static let typeIdentifier = "me.jishnu.keyboard.neopack"

    public static var contentType: UTType {
        UTType(exportedAs: typeIdentifier, conformingTo: .zip)
    }

    /// Installs the pack at `url`, a file the document picker returned (security-scoped when
    /// the picker didn't copy it; possibly in iCloud Drive and not downloaded yet). The file
    /// is copied to a temporary file under file coordination, which also downloads it, and the
    /// copy is installed and deleted. The picked file is left where it was.
    @discardableResult
    public static func install(from url: URL, with installer: PackInstaller) throws -> PackDescriptor {
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }

        let copy = FileManager.default.temporaryDirectory.appending(path: "import-\(UUID().uuidString).neopack")
        defer { try? FileManager.default.removeItem(at: copy) }
        var coordinationError: NSError?
        var copyError: Error?
        NSFileCoordinator().coordinate(readingItemAt: url, options: .withoutChanges, error: &coordinationError) { readable in
            do {
                try FileManager.default.copyItem(at: readable, to: copy)
            } catch {
                copyError = error
            }
        }
        if let error = coordinationError ?? copyError {
            throw PackInstaller.Refusal.archive("unreadable: \(error.localizedDescription)")
        }
        return try installer.install(archive: copy)
    }
}

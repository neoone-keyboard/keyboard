import Foundation

/// How much a language takes on the phone, as the Languages screen shows it.
public enum PackSize {
    /// The bytes of the files directly in `folder` (a pack folder holds no subfolders).
    public static func of(folder: URL) -> Int64 {
        of(files: (try? FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: [.fileSizeKey])) ?? [])
    }

    /// The bytes of `files`; a file that can't be read counts 0.
    public static func of(files: [URL]) -> Int64 {
        files.reduce(0) { sum, url in
            sum + Int64((try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0)
        }
    }

    /// "31.2 MB".
    public static func text(_ bytes: Int64) -> String {
        ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
    }
}

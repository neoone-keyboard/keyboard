import Foundation

/// Which build this is, from a bundle's Info.plist: the version, the build number and the
/// commit `make` built it from (`MLGitCommit`, from ML_GIT_COMMIT; "8e49416+" when the working
/// tree had changes). One wording for the app's home screen and About, the keyboard's settings
/// panel and its diagnostics report, so a screenshot and a report name the same build.
public enum AppVersion {
    /// "NeoOne Keyboard 1.0 (9) · 8e49416", as the home screen and the settings panel show it.
    public static func display(info: [String: Any]) -> String {
        "NeoOne Keyboard \(number(info: info))"
    }

    /// "Version 1.0 (9) · 8e49416", for About's card, where the app's name is already the title.
    public static func short(info: [String: Any]) -> String {
        "Version \(number(info: info))"
    }

    /// "1.0 (9) · 8e49416": the version, the build in brackets when known, then the commit when
    /// known. A missing version is "?".
    public static func number(info: [String: Any]) -> String {
        var text = version(info)
        if let build = value(info, "CFBundleVersion") { text += " (\(build))" }
        if let commit = commit(info: info) { text += " · \(commit)" }
        return text
    }

    /// What VoiceOver reads for `display`: "Version 1.0, build 9, commit 8e49416".
    public static func spoken(info: [String: Any]) -> String {
        var text = "Version \(version(info))"
        if let build = value(info, "CFBundleVersion") { text += ", build \(build)" }
        if let commit = commit(info: info) { text += ", commit \(commit)" }
        return text
    }

    /// The commit, or nil when the build didn't record one (an Xcode build, not `make`).
    public static func commit(info: [String: Any]) -> String? {
        value(info, "MLGitCommit")
    }

    private static func version(_ info: [String: Any]) -> String {
        value(info, "CFBundleShortVersionString") ?? "?"
    }

    /// The key's text, trimmed; nil when it is missing, empty, or an unexpanded "$(…)".
    private static func value(_ info: [String: Any], _ key: String) -> String? {
        guard let text = (info[key] as? String)?.trimmingCharacters(in: .whitespaces),
              !text.isEmpty, !text.hasPrefix("$(") else { return nil }
        return text
    }
}

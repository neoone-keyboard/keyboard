import Testing
import KeyboardCore

private var base: [String: Any] { ["CFBundleShortVersionString": "1.0", "CFBundleVersion": "9"] }

@Test func versionAndBuild() {
    #expect(AppVersion.number(info: base) == "1.0 (9)")
    #expect(AppVersion.display(info: base) == "NeoOne Keyboard 1.0 (9)")
    #expect(AppVersion.short(info: base) == "Version 1.0 (9)")
    #expect(AppVersion.spoken(info: base) == "Version 1.0, build 9")
    #expect(AppVersion.commit(info: base) == nil)
}

@Test func withTheCommit() {
    var info = base
    info["MLGitCommit"] = "8e49416"
    #expect(AppVersion.display(info: info) == "NeoOne Keyboard 1.0 (9) · 8e49416")
    #expect(AppVersion.short(info: info) == "Version 1.0 (9) · 8e49416", "About: no name, its card's title has it")
    #expect(AppVersion.spoken(info: info) == "Version 1.0, build 9, commit 8e49416")
    info["MLGitCommit"] = "8e49416+"
    #expect(AppVersion.number(info: info) == "1.0 (9) · 8e49416+", "a dirty tree's mark is kept")
}

@Test func anEmptyCommitIsLeftOut() {
    for empty in ["", "  ", "$(ML_GIT_COMMIT)"] {
        var info = base
        info["MLGitCommit"] = empty
        #expect(AppVersion.display(info: info) == "NeoOne Keyboard 1.0 (9)", "\(empty)")
        #expect(AppVersion.commit(info: info) == nil)
    }
}

@Test func missingKeys() {
    #expect(AppVersion.display(info: [:]) == "NeoOne Keyboard ?")
    #expect(AppVersion.spoken(info: [:]) == "Version ?")
    #expect(AppVersion.short(info: [:]) == "Version ?")
    #expect(AppVersion.number(info: ["CFBundleShortVersionString": "1.0"]) == "1.0")
    #expect(AppVersion.number(info: ["CFBundleVersion": "9", "MLGitCommit": "abc1234"]) == "? (9) · abc1234")
    #expect(AppVersion.number(info: ["CFBundleShortVersionString": 1]) == "?", "not text")
}

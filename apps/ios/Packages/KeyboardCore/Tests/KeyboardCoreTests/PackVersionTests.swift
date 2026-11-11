import Foundation
import Testing
import KeyboardCore

@Test func versionsCompareByDottedParts() {
    let v = PackVersion.init
    #expect(v("2026.11.06") < v("2026.11.07"))
    #expect(v("2026.11.06") < v("2026.12.01"))
    #expect(v("2026.9.30") < v("2026.10.01"), "numerically, not as text")
    #expect(v("2026.11.6") == v("2026.11.06"))
    #expect(v("2026.11.06") < v("2026.11.06.1"))
    #expect(v("2026.11.06") == v("2026.11.06.0"), "a missing part is 0")
    #expect(v("1.0-beta") < v("1.1"))
    #expect(!(v("2") < v("2")))
}

@Test func theCommittedPacksHaveTheirVersionAndFingerprint() throws {
    let hi = try PackDescriptor.load(folder: packsFolder.appending(path: "hi.neopack"))
    #expect(hi.version == "2026.11.14")
    #expect(hi.minEngine == 3)
    #expect(hi.englishFingerprint == "9a50d0c85c79a0df")
    #expect(malayalamPack.minEngine == 1)
    #expect(malayalamPack.englishFingerprint == nil)
}

private func pack(_ id: String, _ version: String, in folder: String) -> PackDescriptor {
    PackDescriptor(id: id, name: id, nativeName: id, script: "mlym", layout: nil,
                   folder: URL(fileURLWithPath: "/\(folder)/\(id).neopack"), version: version)
}

@Test func anInstalledPackWinsOnlyWithAHigherVersion() {
    let bundled = [pack("ml", "2026.11.06", in: "bundle")]
    let newer = PackDescriptor.merge(bundled: bundled, installed: [pack("ml", "2026.12.01", in: "group")])
    #expect(newer.map(\.folder.path) == ["/group/ml.neopack"])
    let same = PackDescriptor.merge(bundled: bundled, installed: [pack("ml", "2026.11.6", in: "group")])
    #expect(same.map(\.folder.path) == ["/bundle/ml.neopack"])
    let older = PackDescriptor.merge(bundled: bundled, installed: [pack("ml", "2026.01.01", in: "group")])
    #expect(older.map(\.folder.path) == ["/bundle/ml.neopack"])
}

@Test func installedPacksFollowTheBundledOnesInIdOrder() {
    // The engine's default active list is English and the first pack: an installed pack
    // doesn't displace the bundled one from it.
    let merged = PackDescriptor.merge(
        bundled: [pack("ml", "1", in: "bundle")],
        installed: [pack("hi", "1", in: "group"), pack("as", "1", in: "group"), pack("ml", "2", in: "group")]
    )
    #expect(merged.map(\.id) == ["ml", "as", "hi"])
    #expect(merged[0].folder.path == "/group/ml.neopack")
    #expect(PackDescriptor.merge(bundled: [], installed: []).isEmpty)
}

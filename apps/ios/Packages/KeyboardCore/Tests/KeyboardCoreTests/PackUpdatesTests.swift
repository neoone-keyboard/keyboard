import Foundation
import Testing
import KeyboardCore

private let support = PackSupport(format: 1, engine: 2, scripts: ["mlym", "deva", "beng"])
private let base = "9a50d0c85c79a0df"
private let other = "0123456789abcdef"

private func installed(_ id: String, _ version: String, fingerprint: String? = base) -> PackDescriptor {
    PackDescriptor(id: id, name: id, nativeName: id, script: "deva", layout: nil,
                   folder: URL(fileURLWithPath: "/Packs/\(id).neopack"), version: version,
                   englishFingerprint: fingerprint)
}

private func entry(_ id: String, _ version: String, fingerprint: String? = base, script: String = "deva",
                   minEngine: Int = 2) -> AvailablePack {
    AvailablePack(id: id, version: version, script: script, minEngine: minEngine, englishFingerprint: fingerprint)
}

@Test func theEnglishFingerprintIsTheEnginesFnv1a() throws {
    let en = packsFolder.deletingLastPathComponent().appending(path: "apps/ios/KeyboardExtension/Lexicon/en.fst")
    let hi = try PackDescriptor.load(folder: packsFolder.appending(path: "hi.neopack"))
    #expect(try EnglishFingerprint.of(en) == hi.englishFingerprint)
    #expect(EnglishFingerprint.of(Data()) == "25232284e49cf2cb", "the FNV-1a offset basis, little-endian")
}

@Test func aNewerVersionIsOffered() {
    let offers = PackUpdates.offers(installed: [installed("hi", "2026.11.06")],
                                    available: [entry("hi", "2026.12.01")], englishFingerprint: base, support: support)
    #expect(offers == [PackOffer(id: "hi", installedVersion: "2026.11.06", offered: entry("hi", "2026.12.01"), reason: .newerVersion)])
}

@Test func theSameOrAnOlderVersionIsNotOffered() {
    for version in ["2026.11.06", "2026.11.6", "2026.01.01"] {
        let offers = PackUpdates.offers(installed: [installed("hi", "2026.11.06")],
                                        available: [entry("hi", version)], englishFingerprint: base, support: support)
        #expect(offers.isEmpty, "\(version)")
    }
}

@Test func aPackBuiltForAnotherEnglishIsStaleAndARebuildIsOffered() {
    let stale = installed("hi", "2026.11.06", fingerprint: other)
    #expect(PackUpdates.isStale(stale, englishFingerprint: base))
    #expect(!PackUpdates.isStale(installed("hi", "1"), englishFingerprint: base))
    // The same version rebuilt for this English: offered, and the installer takes it.
    let offers = PackUpdates.offers(installed: [stale], available: [entry("hi", "2026.11.06")],
                                    englishFingerprint: base, support: support)
    #expect(offers.map(\.reason) == [.matchesEnglish])
    // Not when the index's build names the old English too.
    let none = PackUpdates.offers(installed: [stale], available: [entry("hi", "2026.11.06", fingerprint: other)],
                                  englishFingerprint: base, support: support)
    #expect(none.isEmpty)
}

@Test func aPackWithoutAFingerprintIsNeverStale() {
    let ml = installed("ml", "2026.11.06", fingerprint: nil)
    #expect(!PackUpdates.isStale(ml, englishFingerprint: base))
    #expect(PackUpdates.offers(installed: [ml], available: [entry("ml", "2026.11.06", script: "mlym")],
                               englishFingerprint: base, support: support).isEmpty)
}

@Test func anEntryBuiltForAnotherEnglishIsNotOfferedEvenIfNewer() {
    let offers = PackUpdates.offers(installed: [installed("hi", "1")], available: [entry("hi", "2", fingerprint: other)],
                                    englishFingerprint: base, support: support)
    #expect(offers.isEmpty)
}

@Test func anEntryThisBuildCannotReadIsNotOffered() {
    let pack = [installed("hi", "1")]
    let newerEngine = PackUpdates.offers(installed: pack, available: [entry("hi", "2", minEngine: 3)],
                                         englishFingerprint: base, support: support)
    #expect(newerEngine.isEmpty)
    let unknownScript = PackUpdates.offers(installed: pack, available: [entry("hi", "2", script: "taml")],
                                           englishFingerprint: base, support: support)
    #expect(unknownScript.isEmpty)
}

@Test func theBestReadableEntryIsOfferedOnce() {
    let offers = PackUpdates.offers(
        installed: [installed("hi", "1")],
        available: [entry("hi", "2"), entry("hi", "4", minEngine: 9), entry("hi", "3"), entry("as", "5", script: "beng")],
        englishFingerprint: base, support: support
    )
    #expect(offers.map(\.offered.version) == ["3"])
}

@Test func aPackNotInstalledIsNotAnUpdate() {
    #expect(PackUpdates.offers(installed: [], available: [entry("hi", "1")], englishFingerprint: base, support: support).isEmpty)
}

@Test func installedPacksLiveInTheAppGroup() {
    #expect(PackLocations.appGroup == "group.me.jishnu.keyboard")
    #expect(PackLocations.packsFolderName == "Packs")
}

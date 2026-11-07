import CryptoKit
import Foundation
import Testing
@testable import KeyboardCore

/// What this keyboard's engine reads (the FFI's `pack_support()` in the app).
private let support = PackSupport(format: 1, engine: 2, scripts: ["mlym", "deva", "beng"])

private func sha256(_ data: Data) -> String {
    SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
}

/// A pack archive like `scripts/pack.py --zip` writes: `<id>.neopack/manifest.json` and the
/// files it names, with their checksums. `change` edits the manifest before it is written.
private func packZip(
    id: String = "xx", version: String = "2026.11.07", script: String = "mlym", minEngine: Int = 1,
    files: [String: Data] = ["x.fst": Data("lexicon".utf8), "next.fst": Data("pairs".utf8)],
    roles: [String: String] = ["lexicon": "x.fst", "nextWords": "next.fst"],
    change: (inout [String: Any]) -> Void = { _ in }
) -> TestZip {
    var manifest: [String: Any] = [
        "format": 1, "id": id, "version": version, "name": "Test", "nativeName": "Test",
        "script": script, "minEngine": minEngine, "files": roles,
        "sha256": files.mapValues(sha256),
    ]
    change(&manifest)
    var zip = TestZip()
    let json = try! JSONSerialization.data(withJSONObject: manifest, options: [.sortedKeys])
    zip.add("\(id).neopack/manifest.json", json)
    for (name, data) in files.sorted(by: { $0.key < $1.key }) {
        zip.add("\(id).neopack/\(name)", data)
    }
    return zip
}

/// A temporary App Group stand-in: `Packs/` inside it, and a folder for archives.
private struct Fixture {
    let root: URL
    var packs: URL { root.appending(path: "Packs", directoryHint: .isDirectory) }
    var installer: PackInstaller { PackInstaller(packsFolder: packs, support: support) }

    init() throws {
        root = try temporaryFolder()
    }

    func archive(_ zip: TestZip, named name: String = "pack.neopack") throws -> URL {
        let url = root.appending(path: name)
        try zip.write(to: url)
        return url
    }

    /// Everything in `Packs/`, hidden entries included.
    func listing() -> [String] {
        ((try? FileManager.default.contentsOfDirectory(atPath: packs.path)) ?? []).sorted()
    }

    func cleanUp() {
        try? FileManager.default.removeItem(at: root)
    }
}

private func refusal(_ body: () throws -> Void) -> PackInstaller.Refusal? {
    do {
        try body()
        return nil
    } catch let refusal as PackInstaller.Refusal {
        return refusal
    } catch {
        Issue.record("not a refusal: \(error)")
        return nil
    }
}

@Test func aGoodPackIsInstalledUnderItsId() throws {
    let f = try Fixture()
    defer { f.cleanUp() }
    let pack = try f.installer.install(archive: f.archive(packZip()))
    #expect(pack.id == "xx")
    #expect(pack.version == "2026.11.07")
    #expect(pack.folder.standardizedFileURL == f.packs.appending(path: "xx.neopack").standardizedFileURL)
    #expect(f.listing() == ["xx.neopack"])
    let files = try FileManager.default.contentsOfDirectory(atPath: pack.folder.path).sorted()
    #expect(files == ["manifest.json", "next.fst", "x.fst"])
    #expect(try Data(contentsOf: pack.folder.appending(path: "x.fst")) == Data("lexicon".utf8))
    #expect(PackDescriptor.scan(f.packs).map(\.id) == ["xx"])
}

@Test func aBadChecksumIsRefusedAndNothingIsLeft() throws {
    let f = try Fixture()
    defer { f.cleanUp() }
    let zip = packZip { manifest in
        var sums = manifest["sha256"] as! [String: String]
        sums["next.fst"] = String(repeating: "0", count: 64)
        manifest["sha256"] = sums
    }
    #expect(refusal { try f.installer.install(archive: f.archive(zip)) } == .checksum("next.fst"))
    #expect(f.listing().isEmpty)
}

@Test func aFileWithoutAChecksumOrAChecksumWithoutAFileIsRefused() throws {
    let f = try Fixture()
    defer { f.cleanUp() }
    let unsummed = packZip { manifest in
        var sums = manifest["sha256"] as! [String: String]
        sums["next.fst"] = nil
        manifest["sha256"] = sums
    }
    #expect(refusal { try f.installer.install(archive: f.archive(unsummed)) } == .noChecksum("next.fst"))
    let missing = packZip { manifest in
        var sums = manifest["sha256"] as! [String: String]
        sums["lm.mlkbnn"] = String(repeating: "0", count: 64)
        manifest["sha256"] = sums
        var files = manifest["files"] as! [String: String]
        files["lm"] = "lm.mlkbnn"
        manifest["files"] = files
    }
    #expect(refusal { try f.installer.install(archive: f.archive(missing)) } == .missingFile("lm.mlkbnn"))
    #expect(f.listing().isEmpty)
}

@Test func anOptionalFileMayBeAbsent() throws {
    let f = try Fixture()
    defer { f.cleanUp() }
    // `lm` is named but neither in the archive nor checksummed: that feature is just off.
    let zip = packZip { manifest in
        var files = manifest["files"] as! [String: String]
        files["lm"] = "lm.mlkbnn"
        manifest["files"] = files
    }
    let pack = try f.installer.install(archive: f.archive(zip))
    #expect(pack.id == "xx")
}

@Test func aDamagedArchiveLeavesNothingHalfInstalledAndKeepsTheOldVersion() throws {
    let f = try Fixture()
    defer { f.cleanUp() }
    try f.installer.install(archive: f.archive(packZip(version: "1")))
    // Version 2, cut short by an interrupted copy: the archive's directory is gone.
    let cut = packZip(version: "2").data
    let url = f.root.appending(path: "cut.neopack")
    try cut.prefix(cut.count - 30).write(to: url)
    guard case .archive = refusal({ try f.installer.install(archive: url) }) else {
        Issue.record("a cut archive should be refused")
        return
    }
    // Version 2 again, its second file damaged: the first was already unpacked when the
    // second failed its CRC.
    var damaged = packZip(version: "2")
    damaged.entries[2].crc = 1
    guard case .archive = refusal({ try f.installer.install(archive: f.archive(damaged)) }) else {
        Issue.record("a damaged entry should be refused")
        return
    }
    #expect(f.listing() == ["xx.neopack"])
    #expect(PackDescriptor.scan(f.packs).map(\.version) == ["1"])
}

@Test func aLeftoverIncomingFolderIsNeverScannedAndIsRemovedByTheNextInstall() throws {
    let f = try Fixture()
    defer { f.cleanUp() }
    // As an app killed mid-install leaves it: a manifest and part of a lexicon.
    let leftover = f.packs.appending(path: ".incoming-\(UUID().uuidString)", directoryHint: .isDirectory)
    try FileManager.default.createDirectory(at: leftover, withIntermediateDirectories: true)
    try packZip().entries[0].data.write(to: leftover.appending(path: "manifest.json"))
    try Data("lexi".utf8).write(to: leftover.appending(path: "x.fst"))
    #expect(PackDescriptor.scan(f.packs).isEmpty)
    try f.installer.install(archive: f.archive(packZip(id: "yy")))
    #expect(f.listing() == ["yy.neopack"])
    // And without an install.
    try FileManager.default.createDirectory(at: leftover, withIntermediateDirectories: true)
    f.installer.removeLeftovers()
    #expect(f.listing() == ["yy.neopack"])
}

@Test func aNewerVersionReplacesTheOldOneAndAnOlderOneIsRefused() throws {
    let f = try Fixture()
    defer { f.cleanUp() }
    try f.installer.install(archive: f.archive(packZip(version: "2026.11.07")))
    let newer = packZip(version: "2026.12.01", files: ["x.fst": Data("lexicon 2".utf8)], roles: ["lexicon": "x.fst"])
    let pack = try f.installer.install(archive: f.archive(newer))
    #expect(pack.version == "2026.12.01")
    #expect(f.listing() == ["xx.neopack"])
    // The old version's files are gone with it.
    let files = try FileManager.default.contentsOfDirectory(atPath: pack.folder.path).sorted()
    #expect(files == ["manifest.json", "x.fst"])
    #expect(try Data(contentsOf: pack.folder.appending(path: "x.fst")) == Data("lexicon 2".utf8))
    #expect(refusal { try f.installer.install(archive: f.archive(packZip(version: "2026.11.30"))) }
        == .older(installed: "2026.12.01", offered: "2026.11.30"))
    // The same version again repairs it.
    try f.installer.install(archive: f.archive(newer))
    #expect(PackDescriptor.scan(f.packs).map(\.version) == ["2026.12.01"])
}

@Test func aScriptTheEngineDoesNotKnowIsRefused() throws {
    let f = try Fixture()
    defer { f.cleanUp() }
    #expect(refusal { try f.installer.install(archive: f.archive(packZip(script: "taml"))) } == .script("taml"))
    #expect(f.listing().isEmpty)
}

@Test func aPackNeedingANewerEngineIsRefused() throws {
    let f = try Fixture()
    defer { f.cleanUp() }
    #expect(refusal { try f.installer.install(archive: f.archive(packZip(minEngine: 3))) } == .engine(3))
    #expect(f.listing().isEmpty)
}

@Test func manifestsThatCannotBeUsedAreRefused() throws {
    let f = try Fixture()
    defer { f.cleanUp() }
    let format = packZip { $0["format"] = 2 }
    #expect(refusal { try f.installer.install(archive: f.archive(format)) } == .format(2))
    #expect(refusal { try f.installer.install(archive: f.archive(packZip(id: "en"))) } == .id("en"))
    #expect(refusal { try f.installer.install(archive: f.archive(packZip(id: "Bad_Id"))) } == .id("Bad_Id"))
    let path = packZip { $0["files"] = ["lexicon": "../x.fst"] }
    #expect(refusal { try f.installer.install(archive: f.archive(path)) } == .fileName("../x.fst"))
    let noLexicon = packZip(roles: ["nextWords": "next.fst"])
    #expect(refusal { try f.installer.install(archive: f.archive(noLexicon)) } == .noLexicon)
    var noManifest = TestZip()
    noManifest.add("xx.neopack/x.fst", "lexicon")
    #expect(refusal { try f.installer.install(archive: f.archive(noManifest)) } == .noManifest)
    var garbled = TestZip()
    garbled.add("xx.neopack/manifest.json", "{")
    guard case .manifest = refusal({ try f.installer.install(archive: f.archive(garbled)) }) else {
        Issue.record("a malformed manifest should be refused")
        return
    }
    var twoFolders = packZip()
    twoFolders.add("other.neopack/x.fst", "x")
    guard case .archive = refusal({ try f.installer.install(archive: f.archive(twoFolders)) }) else {
        Issue.record("two top folders should be refused")
        return
    }
    #expect(f.listing().isEmpty)
}

@Test func filesTheManifestDoesNotNameAreNotInstalled() throws {
    let f = try Fixture()
    defer { f.cleanUp() }
    var zip = packZip()
    zip.add("xx.neopack/notes.txt", "hello")
    // What Finder's Compress adds beside a folder.
    zip.add("__MACOSX/xx.neopack/._x.fst", "resource fork")
    zip.add("xx.neopack/.DS_Store", "finder")
    let pack = try f.installer.install(archive: f.archive(zip))
    let files = try FileManager.default.contentsOfDirectory(atPath: pack.folder.path).sorted()
    #expect(files == ["manifest.json", "next.fst", "x.fst"])
}

@Test func aFlatArchiveInstallsToo() throws {
    let f = try Fixture()
    defer { f.cleanUp() }
    var flat = TestZip()
    for entry in packZip().entries {
        flat.add(String(entry.name.split(separator: "/").last!), entry.data)
    }
    #expect(try f.installer.install(archive: f.archive(flat)).id == "xx")
}

@Test func aPackCanBeRemoved() throws {
    let f = try Fixture()
    defer { f.cleanUp() }
    try f.installer.install(archive: f.archive(packZip()))
    try f.installer.remove(id: "xx")
    #expect(f.listing().isEmpty)
    try f.installer.remove(id: "xx")
}

/// The committed Hindi pack, zipped by the pack builder, installs and reads as it did.
@Test func theCommittedHindiPackInstalls() throws {
    let f = try Fixture()
    defer { f.cleanUp() }
    let archive = f.root.appending(path: "hi.neopack")
    let python = Process()
    python.executableURL = URL(fileURLWithPath: "/usr/bin/env")
    python.arguments = ["python3", "-I", packsFolder.deletingLastPathComponent().appending(path: "scripts/pack.py").path,
                        packsFolder.appending(path: "hi.neopack").path, "--zip", archive.path]
    python.standardOutput = FileHandle.nullDevice
    try python.run()
    python.waitUntilExit()
    #expect(python.terminationStatus == 0)
    let pack = try f.installer.install(archive: archive)
    let committed = try PackDescriptor.load(folder: packsFolder.appending(path: "hi.neopack"))
    #expect(pack.id == "hi")
    #expect(pack.version == committed.version)
    #expect(pack.englishFingerprint == committed.englishFingerprint)
    let lexicon = try Data(contentsOf: pack.folder.appending(path: "hi.fst"))
    #expect(lexicon == (try Data(contentsOf: packsFolder.appending(path: "hi.neopack/hi.fst"))))
}

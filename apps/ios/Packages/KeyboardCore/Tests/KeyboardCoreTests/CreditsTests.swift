import Foundation
import Testing
import KeyboardCore

/// The repository root, found from this file's path (…/apps/ios/Packages/KeyboardCore/Tests/KeyboardCoreTests/).
private let repoRoot = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent()

private struct Manifest: Decodable {
    let id: String
    let licence: String
    let verdict: String?
    let status: String?
    let used_for: String?

    /// Only third-party data that reaches the app needs a credit. A manifest with a
    /// `status` ("audited, not fetched", "fetched, not used yet") isn't in
    /// anything shipped yet, eval-only data never is, and "own work" (hand-written
    /// lists in data/) has no one else to credit.
    var isShipped: Bool { verdict != "eval only" && verdict != "own work" && status == nil }

    /// Text for the M3b language model, which ships in the keyboard (Models/lm.mlkbnn).
    var isM3bTraining: Bool { used_for?.contains("M3b LM text (training)") ?? false }
}

/// The keyboard's data sources with the bundled pack: English's own, then Malayalam's manifest's.
private let dataSources = Credits.dataSources(packs: [malayalamPack])

@Test func everyShippedManifestIsCreditedWithItsLicence() throws {
    let dir = repoRoot.appending(path: "data/manifests")
    let files = try FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: nil)
        .filter { $0.pathExtension == "json" }
    #expect(!files.isEmpty, "no manifests in \(dir.path)")
    let all = try files.map { try JSONDecoder().decode(Manifest.self, from: Data(contentsOf: $0)) }
    let manifests = all.filter(\.isShipped)
    for manifest in manifests {
        let credit = dataSources.first { $0.id == manifest.id }
        #expect(credit?.licence == manifest.licence, "\(manifest.id) is not credited with \(manifest.licence)")
    }
    let training = all.filter(\.isM3bTraining)
    #expect(!training.isEmpty, "no manifest is marked as M3b LM training text")
    for manifest in training {
        #expect(manifest.isShipped, "\(manifest.id) trains the shipped LM but is not marked as shipped")
    }
    #expect(Set(manifests.map(\.id)) == Set(dataSources.map(\.id)), "a credit has no manifest")
}

/// engine/shipped-crates.txt is generated from `cargo tree -p mlengine-ffi` and kept fresh by
/// scripts/shipped-crates.sh --check (part of `make test-engine`). Each line is "name<TAB>licence".
@Test func everyShippedCrateIsCreditedWithItsLicence() throws {
    let list = try String(contentsOf: repoRoot.appending(path: "engine/shipped-crates.txt"), encoding: .utf8)
    let rows = list.split(separator: "\n").map { $0.split(separator: "\t", maxSplits: 1).map(String.init) }
    #expect(rows.count > 10, "engine/shipped-crates.txt looks empty")
    for row in rows {
        #expect(row.count == 2, "malformed line \(row)")
        guard row.count == 2 else { continue }
        let credit = Credits.software.first { $0.id == row[0] }
        #expect(credit?.licence == row[1], "crate \(row[0]) is not credited with \(row[1])")
    }
    #expect(Set(rows.map { $0[0] }) == Set(Credits.software.map(\.id)), "a software credit is not a shipped crate")
}

@Test func creditsHaveUniqueIdsAndUsableLinks() {
    for list in [dataSources, Credits.software] {
        #expect(Set(list.map(\.id)).count == list.count, "duplicate credit id")
        for credit in list {
            #expect(URL(string: credit.licenceURL)?.scheme != nil, "\(credit.id) has a bad link")
        }
    }
}

/// Every credit in a pack's manifest names its source fully: the About screen shows the
/// attribution and links the licence.
@Test func packCreditsAreComplete() {
    #expect(!malayalamPack.credits.isEmpty)
    for credit in malayalamPack.credits {
        #expect(credit.id != credit.name, "\(credit.name) has no id")
        #expect(credit.licenceURL.hasPrefix("http"), "\(credit.id) has no licence link")
        #expect(credit.attribution.count > 40, "\(credit.id) has no attribution")
    }
}

/// A source credited by English and a pack (English Wikipedia) is listed once.
@Test func eachSourceIsListedOnce() {
    #expect(dataSources.filter { $0.id == "enwiki" }.count == 1)
    #expect(Array(dataSources.prefix(Credits.english.count)) == Credits.english)
    #expect(Credits.dataSources(packs: []) == Credits.english)
}

/// A pack's `notices` (Japanese's dictionary-notices.txt: the IPAdic, ICOT and Google notices)
/// are shown as they are, apart from the credits, which are CC BY-SA and the like.
@Test func aPacksNoticesAreShownVerbatimApartFromTheCredits() throws {
    let ja = pack("ja")
    let file = packsFolder.appending(path: "ja.neopack/dictionary-notices.txt")
    #expect(ja.notices?.standardizedFileURL == file.standardizedFileURL)
    #expect(malayalamPack.notices == nil)
    let notices = Credits.notices(packs: [malayalamPack, ja])
    #expect(notices.count == 1)
    let notice = try #require(notices.first)
    #expect(notice.id == "ja")
    #expect(notice.title == "Japanese dictionary notices")
    #expect(notice.text == (try String(contentsOf: file, encoding: .utf8)), "verbatim")
    #expect(notice.text.contains("NO WARRANTY"))
    #expect(notice.text.contains("Nara Institute of Science and Technology"))
    #expect(Credits.notices(packs: [malayalamPack]).isEmpty)
}

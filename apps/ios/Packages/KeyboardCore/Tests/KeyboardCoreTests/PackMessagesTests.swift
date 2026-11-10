import Foundation
import Testing
@testable import KeyboardCore

private let refusals: [PackInstaller.Refusal] = [
    .archive("truncated central directory"), .noManifest, .manifest("keyNotFound"), .format(2),
    .id("EN"), .script("taml"), .engine(9), .fileName("../x"), .noLexicon, .noChecksum("x.fst"),
    .missingFile("x.fst"), .checksum("x.fst"), .older(installed: "2026.11.07", offered: "2026.11.01"),
    .io("No space left on device"), .noScriptRules("taml"), .unusable("script rules: lang.json: no characters"),
]

private let failures: [PackDownloader.Failure] = [
    .insecure(URL(string: "http://x.test")!), .network("The Internet connection appears to be offline."),
    .http(404), .size(expected: 1, got: 2), .checksum,
]

@Test func everyFailureHasAPlainSentence() {
    let errors: [Error] = refusals + failures + [PackIndex.Refusal.notJSON, PackIndex.Refusal.format(2), URLError(.badURL)]
    for error in errors {
        let text = PackMessages.text(for: error)
        #expect(text.hasSuffix(".") || text.hasSuffix(").") , "\(error)")
        // No log text: neither the technical description nor file names, codes or ids.
        #expect(!text.contains(String(describing: error)), "\(error)")
        for word in ["x.fst", "manifest", "sha", "SHA", "404", "taml", "zip", "central", "keyNotFound", "space left"] {
            #expect(!text.contains(word), "\(error): \(text)")
        }
    }
}

@Test func theCommonOnesSayWhatToDo() {
    #expect(PackMessages.text(for: PackInstaller.Refusal.checksum("x.fst")) == "This pack is damaged. Download it again.")
    #expect(PackMessages.text(for: PackDownloader.Failure.checksum) == "This pack is damaged. Download it again.")
    #expect(PackMessages.text(for: PackInstaller.Refusal.script("taml")) == "This pack needs a newer NeoOne Keyboard. Update the app, then try again.")
    #expect(PackMessages.text(for: PackInstaller.Refusal.older(installed: "2", offered: "1")).contains("newer version"))
}

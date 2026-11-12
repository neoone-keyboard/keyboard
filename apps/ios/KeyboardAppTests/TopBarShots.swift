import XCTest
import KeyboardCore

/// Pictures of the suggestion bar (TopBarView) as the keyboard draws it on an iPhone 17, for
/// comparing with the design: short English words, long Malayalam words with the hint at the
/// edge, and Urdu mirrored. For screenshots only: skips without SHOTS_DIR
/// (`xcodebuild … TEST_RUNNER_SHOTS_DIR=<dir> test -only-testing:KeyboardAppTests/TopBarShots`).
@MainActor
final class TopBarShots: XCTestCase {
    private var dir = ""

    override func setUp() async throws {
        guard let dir = ProcessInfo.processInfo.environment["SHOTS_DIR"], !dir.isEmpty else {
            throw XCTSkip("screenshots only: set SHOTS_DIR")
        }
        self.dir = dir
    }

    func testEnglish() {
        shot("bar_ios_english", typed: "he", [
            Suggestion("he", kind: .verbatim), Suggestion("her", kind: .completion), Suggestion("hey", kind: .completion),
            Suggestion("help", kind: .completion), Suggestion("here", kind: .completion), Suggestion("head", kind: .completion),
        ])
    }

    func testMalayalam() {
        shot("bar_ios_malayalam", typed: "ente", [
            Suggestion("എന്റേതായിരുന്നു", kind: .manglish), Suggestion("ente", kind: .verbatim),
            Suggestion("എന്തിനാണെന്നറിയില്ല", kind: .manglish), Suggestion("എന്നെക്കുറിച്ചുള്ള", kind: .manglish),
            Suggestion("എന്നിരുന്നാലും", kind: .manglish), Suggestion("എന്റെ", kind: .manglish),
        ])
    }

    func testUrdu() {
        let urdu = PackDescriptor(
            id: "ur", name: "Urdu", nativeName: "اردو", script: "arab", layout: nil, folder: URL(fileURLWithPath: "/"),
            scriptRanges: [0x0621...0x06D3], rightToLeft: true
        )
        shot("bar_ios_urdu", typed: "mera", script: urdu, [
            Suggestion("میرا", kind: .manglish), Suggestion("mera", kind: .verbatim), Suggestion("میری", kind: .manglish),
            Suggestion("میرے", kind: .manglish), Suggestion("میراث", kind: .manglish), Suggestion("میرپور", kind: .manglish),
            Suggestion("میرزا", kind: .manglish), Suggestion("میراثی", kind: .manglish),
        ])
    }

    /// The bar over the keyboard's background, as wide as an iPhone 17's keyboard.
    private func shot(_ name: String, typed: String, script: PackDescriptor? = nil, _ candidates: [Suggestion]) {
        let theme = KeyboardTheme.resolve(KeyboardSettings(), style: .light)
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 402, height: 56))
        window.overrideUserInterfaceStyle = .light
        window.backgroundColor = theme.background
        let bar = TopBarView(frame: CGRect(x: 6, y: 6, width: 390, height: 44))
        window.addSubview(bar)
        window.isHidden = false
        bar.apply(theme)
        bar.setScript(script)
        bar.showSuggestions(SuggestionBar(SuggestionResult(typed: typed, candidates: candidates), limit: 10))
        window.layoutIfNeeded()
        let image = UIGraphicsImageRenderer(bounds: window.bounds).image { context in
            window.layer.render(in: context.cgContext)
        }
        try? image.pngData()?.write(to: URL(fileURLWithPath: dir).appending(path: "\(name).png"))
        window.isHidden = true
    }
}

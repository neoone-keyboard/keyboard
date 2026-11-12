import XCTest
import KeyboardCore

/// Pictures of the suggestion bar (TopBarView) as the keyboard draws it on an iPhone 17, for
/// comparing with the design: English, Malayalam for `thookkal` (the engine's ten, English
/// and Malayalam on), Urdu mirrored, and the English bar slid to its end. For screenshots
/// only: skips without SHOTS_DIR
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

    /// The engine's ten for `he`, English and Malayalam on.
    private let english: [Suggestion] = [
        Suggestion("he", kind: .verbatim), Suggestion("the", kind: .correction), Suggestion("be", kind: .correction),
        Suggestion("her", kind: .completion), Suggestion("she", kind: .correction), Suggestion("de", kind: .correction),
        Suggestion("ഹേ", kind: .manglish), Suggestion("e", kind: .correction), Suggestion("us", kind: .correction),
        Suggestion("we", kind: .correction),
    ]

    func testEnglish() {
        shot("bar_ios_english", typed: "he", english)
    }

    func testEnglishSlid() {
        shot("bar_ios_english_slid", typed: "he", english, slid: true)
    }

    func testMalayalam() {
        shot("bar_ios_malayalam_thookkal", typed: "thookkal", [Suggestion("thookkal", kind: .verbatim)]
            + ["തൂക്കിൽ", "തൂക്കൽ", "തുകൽ", "ടുക്കൽ", "ത്തുകൾ", "ടുകൾ", "ടുകളെ", "തുകലിൽ", "തുകലും"]
                .map { Suggestion($0, kind: .manglish) })
    }

    func testUrdu() {
        let urdu = PackDescriptor(
            id: "ur", name: "Urdu", nativeName: "اردو", script: "arab", layout: nil, folder: URL(fileURLWithPath: "/"),
            scriptRanges: [0x0621...0x06D3], rightToLeft: true
        )
        let words = [
            Suggestion("میرا", kind: .manglish), Suggestion("mera", kind: .verbatim), Suggestion("میری", kind: .manglish),
            Suggestion("میرے", kind: .manglish), Suggestion("میراث", kind: .manglish), Suggestion("میرپور", kind: .manglish),
            Suggestion("میرزا", kind: .manglish), Suggestion("میراثی", kind: .manglish),
        ]
        // Roman Urdu on the Latin keys reads left to right; on Urdu's own letters, mirrored.
        shot("bar_ios_urdu_latin_keys", typed: "mera", script: urdu, words)
        shot("bar_ios_urdu_urdu_keys", typed: "mera", script: urdu, rightToLeft: true, words)
    }

    /// The bar over the keyboard's background, as wide as an iPhone 17's keyboard; `slid`
    /// slides it to its far end, as a finger would.
    private func shot(
        _ name: String, typed: String, script: PackDescriptor? = nil, rightToLeft: Bool = false,
        _ candidates: [Suggestion], slid: Bool = false
    ) {
        let theme = KeyboardTheme.resolve(KeyboardSettings(), style: .light)
        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 402, height: 56))
        window.overrideUserInterfaceStyle = .light
        window.backgroundColor = theme.background
        let bar = TopBarView(frame: CGRect(x: 6, y: 6, width: 390, height: 44))
        window.addSubview(bar)
        window.isHidden = false
        bar.apply(theme)
        bar.setScript(script, rightToLeft: rightToLeft)
        bar.showSuggestions(SuggestionBar(SuggestionResult(typed: typed, candidates: candidates), limit: 10))
        window.layoutIfNeeded()
        if slid, let scroll = scrollView(in: bar) {
            scroll.setContentOffset(CGPoint(x: scroll.contentSize.width - scroll.bounds.width, y: 0), animated: false)
            window.layoutIfNeeded()
        }
        let image = UIGraphicsImageRenderer(bounds: window.bounds).image { context in
            window.layer.render(in: context.cgContext)
        }
        try? image.pngData()?.write(to: URL(fileURLWithPath: dir).appending(path: "\(name).png"))
        window.isHidden = true
    }

    private func scrollView(in view: UIView) -> UIScrollView? {
        for sub in view.subviews {
            if let scroll = sub as? UIScrollView, scroll.subviews.contains(where: { $0 is UIButton }) { return scroll }
            if let found = scrollView(in: sub) { return found }
        }
        return nil
    }
}

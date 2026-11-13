import XCTest

/// The App Store app preview video, for recordings only: skips without PREVIEW_DIR
/// (`TEST_RUNNER_PREVIEW_DIR=<dir> xcodebuild … test -only-testing:KeyboardUITests/AppStorePreview`),
/// with `xcrun simctl io <sim> recordVideo` running around the test. Same simulator setup as
/// AppStoreScreenshots (Release build, raised memory budget, status bar override), with the
/// ml, hi, ta and as packs installed; active-languages.json is {"languages":["en","ml","hi","ta"]}
/// for every take but Assamese's, recorded apart with ["en","as","ml","hi"]. The test writes
/// markers.txt to PREVIEW_DIR: the wall-clock time of each step, for cutting the recording.
@MainActor
final class AppStorePreview: XCTestCase {
    private var dir = ""
    private let app = XCUIApplication()
    private var keys: [String: CGPoint] = [:]
    private var spacePoint = CGPoint.zero
    private var offset = CGVector.zero
    private var markers = ""

    override func setUp() async throws {
        continueAfterFailure = false
        guard let dir = ProcessInfo.processInfo.environment["PREVIEW_DIR"], !dir.isEmpty else {
            throw XCTSkip("preview only: set PREVIEW_DIR")
        }
        self.dir = dir
        app.launchArguments += ["-chooseLanguagesDone", "YES"]
        XCUIDevice.shared.appearance = .light
        XCUIDevice.shared.orientation = .portrait
    }

    /// The recorded run: the takes named in TAKES (comma-separated, all of them without it),
    /// each in an empty Try it field (the app relaunched between them; the edit cuts the
    /// relaunch out). markers.txt says when each take starts and ends and when each word's
    /// suggestions are on the bar ("pause"/"resume"). The keyboard uses three packs at a
    /// time, so the Assamese take is recorded with as among the active languages' first three.
    func testPreview() throws {
        let takes: [(String, () -> Void)] = [
            ("malayalam", {
                self.typeWord("ente", pause: 1.4); self.space()
                self.typeWord("veettil", pause: 0.9)
                self.slideBar()
                self.space()
            }),
            ("hindi", {
                self.typeWord("mera", pause: 0.9); self.space()
                self.typeWord("naam", pause: 0.9); self.space()
            }),
            ("tamil", {
                self.typeWord("vanakkam", pause: 1.0); self.space()
            }),
            ("assamese", {
                self.typeWord("apuni", pause: 0.9); self.space()
                self.typeWord("kene", pause: 0.9); self.space()
                self.typeWord("asa", pause: 0.9); self.space()
            }),
            ("mix", {
                self.typeWord("aaj", pause: 0.9); self.space()
                self.typeWord("meeting", pause: 1.0); self.space()
                self.typeWord("hai", pause: 0.9); self.space()
            }),
            ("swipe", {
                self.swipe(through: "nandri"); self.log("pause nandri"); usleep(1_400_000); self.log("resume nandri")
            }),
        ]
        let wanted = (ProcessInfo.processInfo.environment["TAKES"] ?? "").split(separator: ",").map(String.init)
        for (name, body) in takes where wanted.isEmpty || wanted.contains(name) {
            take(name, body)
        }
        flush()
    }

    private func take(_ name: String, _ body: () -> Void) {
        try? showKeyboard()
        showLatinKeys()
        sleep(1)
        log("start \(name)")
        body()
        usleep(800_000)
        log("end \(name) field=\(fieldText)")
    }

    /// A finger on the bar's right slot slid to its left one: the bar's words past the third.
    private func slideBar() {
        let words = barWords()
        log("bar \(words.map(\.label))")
        guard words.count > 3 else { return }
        let y = words[0].frame.midY + offset.dy
        let width = app.windows.firstMatch.frame.width
        log("pause slide")
        drag([CGPoint(x: width * 0.85, y: y), CGPoint(x: width * 0.2, y: y)], seconds: 0.6)
        usleep(1_400_000)
        log("resume slide")
        log("bar slid \(barWords().filter { $0.frame.minX >= 0 }.prefix(3).map(\.label))")
    }

    private func returnKey() {
        app.keys["return"].tap()
        usleep(300_000)
    }

    // MARK: - Typing

    /// Launches the app and focuses its Try it field (empty at launch).
    private func showKeyboard() throws {
        XCUIApplication(bundleIdentifier: "me.jishnu.keyboard.extension").terminate()
        app.terminate()
        XCUIDevice.shared.press(.home)
        sleep(1)
        app.launch()
        XCTAssertTrue(field.waitForExistence(timeout: 10))
        field.tap()
        XCTAssertTrue(languageKey().waitForExistence(timeout: 5), "NeoOne Keyboard isn't the keyboard shown")
        sleep(1)
    }

    private var field: XCUIElement {
        app.textViews["try-it"].exists ? app.textViews["try-it"] : app.textFields["try-it"]
    }

    private var fieldText: String {
        let value = (field.value as? String) ?? ""
        return value == field.placeholderValue ? "" : value
    }

    private static let languageKeys = ["EN": "en", "മ": "ml", "हि": "hi", "অ": "as"]

    private func languageKey() -> XCUIElement {
        app.keys.matching(NSPredicate(format: "identifier IN %@", Array(Self.languageKeys.keys))).firstMatch
    }

    /// Taps the language key until the Latin keys show (the English layout: Manglish and
    /// Hinglish are typed on it; the packs' own letters are a language key away).
    private func showLatinKeys() {
        for _ in 0..<4 where !app.keys["q"].exists && !app.keys["Q"].exists {
            app.keys.matching(NSPredicate(format: "label BEGINSWITH 'switch to'")).firstMatch.tap()
            usleep(800_000)
        }
        XCTAssertTrue(app.keys["q"].exists || app.keys["Q"].exists, "Latin keys")
        readKeys()
    }

    /// Where every letter key is, on screen, read once (tapping a coordinate is quicker than a
    /// query). The keyboard's frames are its own window's; one key's screen point gives the
    /// offset.
    private func readKeys() {
        keys = [:]
        let snaps = allSnapshots().filter { $0.elementType == .key }
        guard let ret = snaps.first(where: { $0.identifier == "return" }) else { XCTFail("no return"); return }
        let screen = app.keys["return"].coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).screenPoint
        offset = CGVector(dx: screen.x - ret.frame.midX, dy: screen.y - ret.frame.midY)
        for k in snaps {
            if k.label.count == 1, let ch = k.label.lowercased().first, ch.isLetter, ch.isASCII {
                keys[String(ch)] = CGPoint(x: k.frame.midX + offset.dx, y: k.frame.midY + offset.dy)
            }
            if k.label == "space" {
                spacePoint = CGPoint(x: k.frame.midX + offset.dx, y: k.frame.midY + offset.dy)
            }
        }
        log("keys: \(keys.count) offset=\(offset) space=\(spacePoint)")
    }

    private func allSnapshots() -> [any XCUIElementSnapshot] {
        guard let root = try? app.snapshot() else { return [] }
        var all: [any XCUIElementSnapshot] = []
        var queue: [any XCUIElementSnapshot] = [root]
        while let next = queue.popLast() {
            all.append(next)
            queue.append(contentsOf: next.children)
        }
        return all
    }

    private func at(_ point: CGPoint) -> XCUICoordinate {
        app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: point.x, dy: point.y))
    }

    private func typeWord(_ word: String, perKey: useconds_t = 90_000, pause: Double = 0.7) {
        for ch in word {
            guard let p = keys[String(ch)] else { XCTFail("no key \(ch)"); return }
            at(p).tap()
            usleep(perKey)
        }
        log("pause \(word)")
        usleep(useconds_t(pause * 1_000_000))
        log("resume \(word)")
    }

    private func space() {
        at(spacePoint).tap()
        usleep(250_000)
    }

    // MARK: - Swipe

    /// One finger down on the first letter, through each next one, lifted on the last: a
    /// single touch (XCUICoordinate's drag lifts between segments), built with XCTest's event
    /// synthesizer.
    private func swipe(through word: String, secondsPerLetter: Double = 0.18) {
        let points = word.compactMap { keys[String($0)] }
        guard points.count == word.count else { XCTFail("swipe keys"); return }
        drag(points, seconds: secondsPerLetter * Double(points.count - 1), name: "swipe \(word)")
    }

    private func drag(_ points: [CGPoint], seconds: Double, name: String = "drag") {
        guard let first = points.first else { return }
        let secondsPerLetter = seconds / Double(max(points.count - 1, 1))
        guard let pathClass = NSClassFromString("XCPointerEventPath") as? NSObject.Type,
              let recordClass = NSClassFromString("XCSynthesizedEventRecord") as? NSObject.Type else {
            XCTFail("no event synthesizer"); return
        }
        typealias InitTouch = @convention(c) (AnyObject, Selector, CGPoint, Double) -> AnyObject
        typealias Move = @convention(c) (AnyObject, Selector, CGPoint, Double) -> Void
        typealias Lift = @convention(c) (AnyObject, Selector, Double) -> Void
        typealias InitRecord = @convention(c) (AnyObject, Selector, NSString, Int) -> AnyObject
        typealias Add = @convention(c) (AnyObject, Selector, AnyObject) -> Void
        typealias Synthesize = @convention(c) (AnyObject, Selector, UnsafeMutablePointer<NSError?>?) -> Bool

        func imp<T>(_ cls: AnyClass, _ name: String, _: T.Type) -> (Selector, T) {
            let sel = NSSelectorFromString(name)
            return (sel, unsafeBitCast(class_getMethodImplementation(cls, sel), to: T.self))
        }
        let (initSel, initTouch) = imp(pathClass, "initForTouchAtPoint:offset:", InitTouch.self)
        let (moveSel, move) = imp(pathClass, "moveToPoint:atOffset:", Move.self)
        let (liftSel, lift) = imp(pathClass, "liftUpAtOffset:", Lift.self)
        let (recSel, initRecord) = imp(recordClass, "initWithName:interfaceOrientation:", InitRecord.self)
        let (addSel, add) = imp(recordClass, "addPointerEventPath:", Add.self)
        let (synSel, synthesize) = imp(recordClass, "synthesizeWithError:", Synthesize.self)

        let path = initTouch(pathClass.perform(NSSelectorFromString("alloc")).takeUnretainedValue(), initSel, first, 0)
        var t = 0.05
        for (i, p) in points.enumerated().dropFirst() {
            // Along the straight line to the next letter in small steps, so the path is drawn.
            let from = points[i - 1]
            let steps = 8
            for s in 1...steps {
                let f = CGFloat(s) / CGFloat(steps)
                t += secondsPerLetter / Double(steps)
                move(path, moveSel, CGPoint(x: from.x + (p.x - from.x) * f, y: from.y + (p.y - from.y) * f), t)
            }
        }
        lift(path, liftSel, t + 0.05)
        let record = initRecord(recordClass.perform(NSSelectorFromString("alloc")).takeUnretainedValue(), recSel,
                                name as NSString, UIDeviceOrientation.portrait.rawValue)
        add(record, addSel, path)
        var error: NSError?
        if !synthesize(record, synSel, &error) { XCTFail("swipe: \(String(describing: error))") }
    }

    // MARK: - The bar

    private struct BarWord {
        let label: String
        let frame: CGRect
    }

    private func barWords() -> [BarWord] {
        guard let root = try? app.snapshot() else { return [] }
        var all: [any XCUIElementSnapshot] = []
        var queue: [any XCUIElementSnapshot] = [root]
        while let next = queue.popLast() {
            all.append(next)
            queue.append(contentsOf: next.children)
        }
        let keysTop = all.filter { $0.elementType == .key }.map(\.frame.minY).min() ?? 0
        return all
            .filter { $0.elementType == .button && !$0.label.isEmpty && $0.frame.height > 0 }
            .filter { $0.frame.minY >= keysTop - 60 && $0.frame.maxY <= keysTop + 1 }
            .map { BarWord(label: $0.label, frame: $0.frame) }
            .sorted { $0.frame.minX < $1.frame.minX }
    }

    // MARK: - Markers

    private func log(_ line: String) {
        let stamp = String(format: "%.3f", Date().timeIntervalSince1970)
        markers += "\(stamp) \(line)\n"
        print("PREVIEW \(stamp) \(line)")
    }

    private func flush() {
        try? markers.write(to: URL(fileURLWithPath: dir).appending(path: "markers.txt"), atomically: true, encoding: .utf8)
    }
}

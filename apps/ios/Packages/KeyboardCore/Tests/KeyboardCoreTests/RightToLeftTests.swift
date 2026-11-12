import Foundation
import Testing
import KeyboardCore

/// A language written right to left (packs/ur.neopack): what its `lang.json` says, the
/// mirrored suggestion bar, the script's own punctuation, and keys that are never mirrored.
@MainActor
struct RightToLeftTests {
    let urdu = pack("ur")
    let output = RecordingOutput()
    let input: InputController

    init() {
        input = InputController(output: output, languages: [.english])
        input.activePacks = [urdu]
    }

    func roman(_ typed: String, is word: String) -> SuggestionResult {
        SuggestionResult(typed: typed, candidates: [
            Suggestion(word, kind: .manglish, language: "ur"), Suggestion(typed, kind: .verbatim, language: "en"),
        ])
    }

    func type(_ actions: KeyAction...) {
        for (i, action) in actions.enumerated() { input.handle(action, at: Double(i)) }
    }

    // MARK: The pack's data

    @Test func urdusPackIsRightToLeftWithItsMarksAndDigits() {
        #expect(urdu.rightToLeft, "by its letters: lang.json gives no direction")
        #expect(urdu.punctuation == [".": "۔", ",": "،", "?": "؟", ";": "؛"])
        #expect(urdu.digits == ["۰", "۱", "۲", "۳", "۴", "۵", "۶", "۷", "۸", "۹"])
    }

    @Test func leftToRightPacksSayNothing() {
        for id in ["ml", "hi", "ta"] {
            let other = pack(id)
            #expect(!other.rightToLeft, "\(id)")
            #expect(other.punctuation.isEmpty, "\(id)")
            #expect(other.digits.isEmpty, "\(id)")
        }
    }

    @Test func theDirectionIsTheScriptLettersUnlessLangJsonSays() throws {
        #expect(PackDescriptor.isRightToLeft([0x0621...0x063A]), "Arabic")
        #expect(PackDescriptor.isRightToLeft([0x05D0...0x05EA]), "Hebrew")
        #expect(!PackDescriptor.isRightToLeft([0x0900...0x097F]), "Devanagari")
        #expect(!PackDescriptor.isRightToLeft([]))
        // A pack's lang.json may say otherwise.
        let folder = FileManager.default.temporaryDirectory.appending(path: "rtl-\(UUID().uuidString).neopack")
        try FileManager.default.copyItem(at: urdu.folder, to: folder)
        defer { try? FileManager.default.removeItem(at: folder) }
        let langFile = folder.appending(path: "lang.json")
        var lang = try JSONSerialization.jsonObject(with: Data(contentsOf: langFile)) as! [String: Any]
        lang["direction"] = "ltr"
        try JSONSerialization.data(withJSONObject: lang).write(to: langFile)
        #expect(try !PackDescriptor.load(folder: folder).rightToLeft)
        lang["direction"] = "rtl"
        lang["letters"] = [["0900", "097F"]]
        try JSONSerialization.data(withJSONObject: lang).write(to: langFile)
        #expect(try PackDescriptor.load(folder: folder).rightToLeft)
    }

    @Test func aLayoutsMarksAndDigitsAreOptionalAndLenient() throws {
        let json = #"{"format": 1, "spaceBar": "x", "languageKey": "x", "pages": [{"id": "letters", "label": "x", "rows": [["ا"]]}], "punctuation": 3, "digits": ["۱", "۲"]}"#
        let layout = try PackLayout.parse(Data(json.utf8))
        #expect(layout.punctuation.isEmpty)
        #expect(layout.digits.isEmpty, "not ten")
    }

    // MARK: Which pack the keys and bar follow

    @Test func theLatinKeysFollowTheOnePackTypedBesideEnglish() {
        #expect(input.scriptPack?.id == "ur")
        #expect(input.rightToLeft)
        input.activePacks = [urdu, pack("hi")]
        #expect(input.scriptPack == nil, "two packs: neither's conventions")
        #expect(!input.rightToLeft)
        input.activePacks = [malayalamPack]
        #expect(!input.rightToLeft)
    }

    @Test func aPacksOwnLayoutFollowsThatPack() {
        let input = InputController(output: output, languages: [.english, .pack(urdu), malayalam])
        input.activePacks = [urdu, malayalamPack]
        #expect(!input.rightToLeft)
        input.handle(.toggleLanguage, at: 0)
        #expect(input.language.id == "ur")
        #expect(input.rightToLeft)
        input.handle(.toggleLanguage, at: 1)
        #expect(input.language.id == "ml")
        #expect(!input.rightToLeft)
    }

    // MARK: The bar

    @Test func aRightToLeftBarReadsFromTheRight() {
        let result = SuggestionResult(typed: "mera", candidates: [
            Suggestion("میرا", kind: .manglish), Suggestion("میری", kind: .manglish),
            Suggestion("mera", kind: .verbatim), Suggestion("میر", kind: .manglish),
        ])
        let bar = SuggestionBar(result, limit: 4)
        // Left to right as ever: the typed word quoted on the left, the best in the middle.
        #expect(bar.drawn(rightToLeft: false).map { $0?.title } == ["\u{201C}mera\u{201D}", "میرا", "میری"])
        // Right to left, best first from the right: the other word, then the typed one.
        #expect(bar.drawn(rightToLeft: true).map { $0?.title } == ["میرا", "میری", "\u{201C}mera\u{201D}"])
        #expect(bar.more.map(\.title) == ["میر"], "more still follows, further left")
    }

    @Test func anEmojiComesAfterTheWordsRightToLeft() {
        let result = SuggestionResult(typed: "dil", candidates: [
            Suggestion("دل", kind: .manglish), Suggestion("dil", kind: .verbatim), Suggestion("دلی", kind: .manglish),
        ])
        let bar = SuggestionBar(result).withEmoji("❤️")
        #expect(bar.drawn(rightToLeft: true).map { $0?.title } == ["دل", "\u{201C}dil\u{201D}", "❤️"])
    }

    @Test func emptySlotsKeepTheirPlaceRightToLeft() {
        let bar = SuggestionBar(SuggestionResult(typed: "", candidates: [Suggestion("ہے", kind: .nextWord)]))
        #expect(bar.drawn(rightToLeft: true).map { $0?.title } == ["ہے", nil, nil])
    }

    // MARK: Punctuation

    @Test func aFullStopAfterAnUrduWordIsUrdus() {
        output.text = "mera"
        input.updateSuggestions(roman("mera", is: "میرا"))
        type(.character("."), .space)
        #expect(output.text == "میرا۔ ")
    }

    @Test func aQuestionMarkAndCommaAfterAnUrduWordAreUrdus() {
        output.text = "kya"
        input.updateSuggestions(roman("kya", is: "کیا"))
        type(.character("?"))
        #expect(output.text == "کیا؟")
        output.text = "haan"
        input.updateSuggestions(roman("haan", is: "ہاں"))
        type(.character(","))
        #expect(output.text == "ہاں،")
    }

    @Test func marksAfterAnUrduWordAlreadyInTheTextAreUrdus() {
        output.text = "شکریہ"
        type(.character("."), .space)
        #expect(output.text == "شکریہ۔ ")
        output.text = "شکریہ"
        type(.character("?"))
        #expect(output.text == "شکریہ؟")
    }

    @Test func anEnglishWordKeepsItsMarks() {
        output.text = "hello"
        input.updateSuggestions(SuggestionResult(typed: "hello", candidates: [Suggestion("hello", kind: .verbatim)]))
        type(.character("."), .space)
        #expect(output.text == "hello. ")
        output.text = "ok"
        type(.character("?"), .character(","))
        #expect(output.text == "ok?,")
    }

    @Test func twoSpacesAfterAnUrduWordGiveUrdusFullStop() {
        output.text = "mera"
        input.updateSuggestions(roman("mera", is: "میرا"))
        type(.space, .space)
        #expect(output.text == "میرا۔ ")
    }

    @Test func twoSpacesAfterAnEnglishWordAreTwoSpaces() {
        output.text = "hello"
        type(.space, .space)
        #expect(output.text == "hello  ")
    }

    @Test func aSpaceAfterSomethingElseIsJustASpace() {
        output.text = "میرا "
        type(.space)
        #expect(output.text == "میرا  ", "the first space wasn't the space key's")
    }

    @Test func noPackNoUrduMarks() {
        input.activePacks = [malayalamPack]
        output.text = "میرا"
        type(.character("."), .space)
        #expect(output.text == "میرا. ")
    }

    @Test func aLongPressedLatinMarkStaysLatin() {
        output.text = "شکریہ"
        input.insertAlternate(".")
        #expect(output.text == "شکریہ.")
    }

    @Test func urdusOwnFullStopKeyCommitsTheWordToo() {
        output.text = "mera"
        input.updateSuggestions(roman("mera", is: "میرا"))
        type(.character("۔"), .space)
        #expect(output.text == "میرا۔ ")
        output.text = "kya"
        input.updateSuggestions(roman("kya", is: "کیا"))
        type(.character("؟"))
        #expect(output.text == "کیا؟")
    }

    @Test func backspaceUndoesTheWordAndKeepsUrdusMark() {
        output.text = "kya"
        input.updateSuggestions(roman("kya", is: "کیا"))
        type(.character("?"), .backspace)
        #expect(output.text == "kya")
    }

    @Test func noDirectionMarksAreTyped() {
        output.text = "mera"
        input.updateSuggestions(roman("mera", is: "میرا"))
        type(.character("."), .space, .character("n"), .space, .space)
        let marks: Set<Unicode.Scalar> = ["\u{200E}", "\u{200F}", "\u{061C}", "\u{202A}", "\u{202B}", "\u{202C}",
                                          "\u{202D}", "\u{202E}", "\u{2066}", "\u{2067}", "\u{2068}", "\u{2069}"]
        #expect(!output.text.unicodeScalars.contains { marks.contains($0) })
    }

    // MARK: The keys

    @Test func theLatinRowsAreNeverMirrored() {
        let layout = input.layout
        let letters = layout.rows.prefix(3).map { row in
            row.compactMap { key -> String? in if case .character(let c) = key.action { c } else { nil } }.joined()
        }
        #expect(letters == ["qwertyuiop", "asdfghjkl", "zxcvbnm"])
        #expect(layout.rows.prefix(3).map { $0.map(\.action) } == KeyboardLayout.forPage(.letters).rows.prefix(3).map { $0.map(\.action) },
                "the same keys as English, in the same order")
        #expect(layout.rows[3].map(\.action) == KeyboardLayout.forPage(.letters).rows[3].map(\.action),
                "the bottom row's keys in the same order")
    }

    @Test func theUrduPagesKeepTheirOrder() {
        let input = InputController(output: output, languages: [.pack(urdu)])
        let first = input.layout.rows[0].compactMap { key -> String? in
            if case .character(let c) = key.action { c } else { nil }
        }
        #expect(first == urdu.layout?.alphabet.rows[0])
    }

    @Test func theFullStopKeyOffersUrdusMarksButKeepsTheLatinFullStop() {
        let key = input.layout.rows[3].first { $0.action == .character(".") }!
        #expect(key.alternatesGrid.columns == 9)
        #expect(key.alternates == [
            "&", "%", "+", "=", "\"", "-", ":", "'", "@",
            "؛", "/", "(", ")", "#", "!", "،", ".", "؟",
        ])
    }

    @Test func englishFullStopKeyIsUnchanged() {
        input.activePacks = []
        let key = input.layout.rows[3].first { $0.action == .character(".") }!
        #expect(key == KeyboardLayout.forPage(.letters).rows[3].first { $0.action == .character(".") }!)
    }

    @Test func theTopLettersOfferUrdusDigitsAfterTheLatinOnes() {
        let top = input.layout.rows[0]
        #expect(top[0].alternates == ["1", "۱"])
        #expect(top[2].alternates.prefix(3) == ["3", "۳", "é"])
        #expect(top[9].alternates == ["0", "۰"])
        input.numberRow = true
        #expect(input.layout.rows[0].map(\.alternates).first == ["۱"])
    }

    @Test func theSymbolsPageHasUrdusDigitsAndMarksWithTheLatinOnALongPress() {
        type(.page(.symbols))
        let rows = input.layout.rows
        #expect(rows[0].map(\.action) == ["۱", "۲", "۳", "۴", "۵", "۶", "۷", "۸", "۹", "۰"].map { KeyAction.character($0) })
        #expect(rows[0].map(\.alternates) == "1234567890".map { [String($0)] })
        // Above the bottom row, whose full stop key is the letters page's.
        let keys = rows.dropLast().flatMap { $0 }
        func key(_ text: String) -> Key? { keys.first { $0.action == .character(text) } }
        #expect(key("۔")?.alternates == ["."])
        #expect(key("،")?.alternates == [","])
        #expect(key("؟")?.alternates == ["?"])
        #expect(key("؛")?.alternates == [";"])
        #expect(key(".") == nil && key(",") == nil && key("?") == nil && key(";") == nil)
        type(.page(.moreSymbols))
        #expect(input.layout.rows.dropLast() == KeyboardLayout.forPage(.moreSymbols).rows.dropLast())
    }

    @Test func theSymbolsPageShowsThePacksCurrency() throws {
        #expect(urdu.layout?.currency == "₨")
        #expect(pack("hi").layout?.currency == nil)
        type(.page(.symbols))
        let row = input.layout.rows[1].map(\.action)
        #expect(row.contains(.character("₨")) && !row.contains(.character("₹")))
        // A layout with only a currency changes only that key.
        let base = #""format": 1, "spaceBar": "x", "languageKey": "x", "pages": [{"id": "a", "label": "a", "rows": [["a"]]}]"#
        let euro = try PackLayout.parse(Data("{\(base), \"currency\": \"€\"}".utf8))
        let latin = KeyboardLayout.forPage(.symbols).rows.map { $0.map(\.action) }
        let swapped = latin.map { $0.map { $0 == .character("₹") ? .character("€") : $0 } }
        #expect(KeyboardLayout.forPage(.symbols, script: euro).rows.map { $0.map(\.action) } == swapped)
        #expect(try PackLayout.parse(Data("{\(base), \"currency\": \"\"}".utf8)).currency == nil, "an empty one is none")
        #expect(try PackLayout.parse(Data("{\(base), \"currency\": 5}".utf8)).currency == nil, "not a string: none")
    }

    @Test func theKeysRedrawWhenTheScriptChanges() {
        let before = input.keysShown
        input.activePacks = []
        #expect(input.keysShown != before)
    }
}

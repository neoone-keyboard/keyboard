extension KeyboardLayout {
    /// The keys for one page. `native` is the current language's layout, for its pages;
    /// `numberRow` only affects the English letters page. `script` is the layout of the pack
    /// whose script the keys follow (`InputController.scriptPack`): its `punctuation` and
    /// `digits` make the symbols page and the long-press options its own (Urdu's ۔ ، ؟ ؛ and
    /// ۱۲۳, as Gboard's phonetic Urdu has them). The letters stay QWERTY, left to right,
    /// whatever way the script is written.
    public static func forPage(_ page: KeyboardPage, native: PackLayout? = nil, numberRow: Bool = false,
                               showsNextKeyboard: Bool = true, script: PackLayout? = nil) -> KeyboardLayout {
        let digits = script?.digits ?? []
        let marks = script?.punctuation ?? [:]
        let rows: [[Key]]
        let bottom: [Key]
        switch page {
        case .letters:
            // The number row's digits offer the script's on a long press.
            let numbers = keys("1234567890".map(String.init)).enumerated().map { i, key in
                digits.isEmpty ? key : Key(key.action, alternates: [digits[(i + 1) % 10]])
            }
            rows = (numberRow ? [numbers] : []) + letterRows(digits)
            bottom = latinBottomRow(page, marks: marks)
        case .symbols:
            let currency = script?.currency ?? defaultCurrency
            rows = digits.isEmpty && marks.isEmpty ? symbolRows(currency) : scriptSymbolRows(digits, marks, currency)
            bottom = latinBottomRow(page, marks: marks)
        case .moreSymbols:
            rows = moreSymbolRows
            bottom = latinBottomRow(page, marks: marks)
        case .native(let id):
            if let native, let shown = native.page(id) {
                rows = nativeRows(shown, longPress: native.longPress)
                bottom = nativeBottomRow(page, layout: native)
            } else {
                // No such page: the Latin letters, so there are always keys.
                rows = letterRows(digits)
                bottom = latinBottomRow(.letters, marks: marks)
            }
        }
        return KeyboardLayout(rows: rows + [bottom.filter { showsNextKeyboard || $0.action != .nextKeyboard }])
    }

    // MARK: English

    /// QWERTY. With a script's `digits`, each top-row key offers the script's digit after the
    /// Latin one (q: 1 ۱).
    private static func letterRows(_ digits: [String]) -> [[Key]] {
        let top = letters("qwertyuiop").enumerated().map { i, key in
            guard !digits.isEmpty else { return key }
            let alternates = Array(key.alternates.prefix(1)) + [digits[(i + 1) % 10]] + key.alternates.dropFirst()
            return Key(key.action, width: key.width, alternates: alternates)
        }
        return [
            top,
            letters("asdfghjkl"),
            [Key(.shift, width: 1.5)] + letters("zxcvbnm") + [Key(.backspace, width: 1.5)],
        ]
    }

    /// The symbols page in a script with its own `digits` and `marks`: its digits on top (the
    /// Latin one on a long press), its marks in place of the Latin ones, which stay on a long
    /// press.
    private static func scriptSymbolRows(_ digits: [String], _ marks: [String: String], _ currency: String) -> [[Key]] {
        let latinDigits = "1234567890".map(String.init)
        let top = digits.isEmpty ? keys(latinDigits)
            : latinDigits.map { d in Key(.character(digits[Int(d)!]), alternates: [d]) }
        func mark(_ latin: String) -> Key {
            guard let own = marks[latin] else { return Key(.character(latin)) }
            return Key(.character(own), alternates: [latin])
        }
        return [
            top,
            ["-", "/", ":", ";", "(", ")", currency, "&", "@", "\""].map(mark),
            [Key(.page(.moreSymbols), width: 1.5)] + [".", ",", "?", "!", "'"].map(mark) + [Key(.backspace, width: 1.5)],
        ]
    }

    private static let letterAlternates: [Character: [String]] = [
        "q": ["1"], "w": ["2"], "e": ["3", "é", "è", "ê", "ë", "ē"], "r": ["4"], "t": ["5"],
        "y": ["6"], "u": ["7", "ú", "ù", "û", "ü", "ū"], "i": ["8", "í", "ì", "î", "ï", "ī"],
        "o": ["9", "ó", "ò", "ô", "ö", "õ", "ō"], "p": ["0"],
        "a": ["@", "á", "à", "â", "ä", "ã", "å", "ā"], "s": ["#"], "d": ["&"], "f": ["*"],
        "g": ["-"], "h": ["+"], "j": ["="], "k": ["("], "l": [")"],
        "z": ["_"], "x": ["$"], "c": ["\"", "ç"], "v": ["'"], "b": [":"], "n": [";", "ñ"], "m": ["/"],
    ]

    private static func letters(_ string: String) -> [Key] {
        string.map { Key(.character(String($0)), alternates: letterAlternates[$0] ?? []) }
    }

    /// The symbols page's currency sign when the script's layout names none (`currency`).
    public static let defaultCurrency = "₹"

    private static func symbolRows(_ currency: String) -> [[Key]] {
        [
            keys("1234567890".map(String.init)),
            keys(["-", "/", ":", ";", "(", ")", currency, "&", "@", "\""]),
            [Key(.page(.moreSymbols), width: 1.5)] + keys([".", ",", "?", "!", "'"]) + [Key(.backspace, width: 1.5)],
        ]
    }

    private static let moreSymbolRows: [[Key]] = [
        keys(["[", "]", "{", "}", "#", "%", "^", "*", "+", "="]),
        keys(["_", "\\", "|", "~", "<", ">", "$", "€", "£", "•"]),
        [Key(.page(.symbols), width: 1.5)] + keys([".", ",", "?", "!", "'"]) + [Key(.backspace, width: 1.5)],
    ]

    private static func latinBottomRow(_ page: KeyboardPage, marks: [String: String] = [:]) -> [Key] {
        // Ten letter widths without the globe (Face ID iPhones), so nothing is scaled down.
        [
            Key(page == .letters ? .page(.symbols) : .page(.letters), width: 1.25),
            Key(.emoji),
            Key(.nextKeyboard),
            Key(.toggleLanguage),
            Key(.space, width: 4.25),
            // Two rows, as Gboard's: the full stop right above the key (as SwiftKey's), ? to
            // its right, the rest to the left and in the row above.
            Key(.character("."), alternates: fullStopAlternates(marks), alternatesPerRow: 9),
            Key(.returnKey, width: 1.5),
        ]
    }

    private static let latinFullStopAlternates = [
        "&", "%", "+", "=", "\"", "-", ":", "'", "@",
        ";", "/", "(", ")", "#", "!", ",", ".", "?",
    ]

    /// The full stop's long-press with a script's `marks`: each Latin mark it has replaced by
    /// the script's, but for the full stop itself. The key types the script's full stop after
    /// a word of the script, so its long-press keeps the Latin one (as Android's).
    public static func fullStopAlternates(_ marks: [String: String]) -> [String] {
        latinFullStopAlternates.map { $0 == "." ? $0 : marks[$0] ?? $0 }
    }

    // MARK: Native (a pack's layout.json)

    /// The page's keys, each with its long-press options, and backspace at the end of the last row.
    private static func nativeRows(_ page: PackLayout.Page, longPress: [String: [String]]) -> [[Key]] {
        var rows = page.rows.map { row in row.map { Key(.character($0), alternates: longPress[$0] ?? []) } }
        if !rows.isEmpty { rows[rows.count - 1].append(Key(.backspace, width: 1.5)) }
        return rows
    }

    /// The second page's key right of space and the third's left of it, each turning back
    /// into the alphabet key while its page shows. Ten letter widths without the globe, as
    /// the English row: space takes the width of a page key the layout hasn't got.
    private static func nativeBottomRow(_ page: KeyboardPage, layout: PackLayout) -> [Key] {
        func toggle(_ index: Int) -> Key? {
            guard layout.pages.indices.contains(index) else { return nil }
            let target = KeyboardPage.native(layout.pages[index].id)
            return Key(page == target ? .page(.letters) : .page(target))
        }
        let left = toggle(2), right = toggle(1)
        let space = 3.5 + (left == nil ? 1 : 0) + (right == nil ? 1 : 0)
        return [Key(.page(.symbols)), Key(.emoji), Key(.nextKeyboard), Key(.toggleLanguage)]
            + [left].compactMap { $0 }
            + [Key(.space, width: space)]
            + [right].compactMap { $0 }
            + [Key(.returnKey, width: 1.5)]
    }

    private static func keys(_ list: [String]) -> [Key] {
        list.map { Key(.character($0)) }
    }
}

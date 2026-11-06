extension KeyboardLayout {
    /// The keys for one page. `native` is the current language's layout, for its pages;
    /// `numberRow` only affects the English letters page.
    public static func forPage(_ page: KeyboardPage, native: PackLayout? = nil, numberRow: Bool = false,
                               showsNextKeyboard: Bool = true) -> KeyboardLayout {
        let rows: [[Key]]
        let bottom: [Key]
        switch page {
        case .letters:
            rows = (numberRow ? [keys("1234567890".map(String.init))] : []) + letterRows
            bottom = latinBottomRow(page)
        case .symbols:
            rows = symbolRows
            bottom = latinBottomRow(page)
        case .moreSymbols:
            rows = moreSymbolRows
            bottom = latinBottomRow(page)
        case .native(let id):
            if let native, let shown = native.page(id) {
                rows = nativeRows(shown, longPress: native.longPress)
                bottom = nativeBottomRow(page, layout: native)
            } else {
                // No such page: the Latin letters, so there are always keys.
                rows = letterRows
                bottom = latinBottomRow(.letters)
            }
        }
        return KeyboardLayout(rows: rows + [bottom.filter { showsNextKeyboard || $0.action != .nextKeyboard }])
    }

    // MARK: English

    private static let letterRows: [[Key]] = [
        letters("qwertyuiop"),
        letters("asdfghjkl"),
        [Key(.shift, width: 1.5)] + letters("zxcvbnm") + [Key(.backspace, width: 1.5)],
    ]

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

    private static let symbolRows: [[Key]] = [
        keys("1234567890".map(String.init)),
        keys(["-", "/", ":", ";", "(", ")", "₹", "&", "@", "\""]),
        [Key(.page(.moreSymbols), width: 1.5)] + keys([".", ",", "?", "!", "'"]) + [Key(.backspace, width: 1.5)],
    ]

    private static let moreSymbolRows: [[Key]] = [
        keys(["[", "]", "{", "}", "#", "%", "^", "*", "+", "="]),
        keys(["_", "\\", "|", "~", "<", ">", "$", "€", "£", "•"]),
        [Key(.page(.symbols), width: 1.5)] + keys([".", ",", "?", "!", "'"]) + [Key(.backspace, width: 1.5)],
    ]

    private static func latinBottomRow(_ page: KeyboardPage) -> [Key] {
        // Ten letter widths without the globe (Face ID iPhones), so nothing is scaled down.
        [
            Key(page == .letters ? .page(.symbols) : .page(.letters), width: 1.25),
            Key(.emoji),
            Key(.nextKeyboard),
            Key(.toggleLanguage),
            Key(.space, width: 4.25),
            // Two rows, as Gboard's: the full stop right above the key (as SwiftKey's), ? to
            // its right, the rest to the left and in the row above.
            Key(.character("."), alternates: [
                "&", "%", "+", "=", "\"", "-", ":", "'", "@",
                ";", "/", "(", ")", "#", "!", ",", ".", "?",
            ], alternatesPerRow: 9),
            Key(.returnKey, width: 1.5),
        ]
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

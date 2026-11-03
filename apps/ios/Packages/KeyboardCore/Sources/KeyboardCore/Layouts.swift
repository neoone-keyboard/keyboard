extension KeyboardLayout {
    /// The keys for one page. `numberRow` only affects the English letters page.
    public static func forPage(_ page: KeyboardPage, numberRow: Bool = false, showsNextKeyboard: Bool = true) -> KeyboardLayout {
        let rows: [[Key]] = switch page {
        case .letters: (numberRow ? [keys("1234567890".map(String.init))] : []) + letterRows
        case .symbols: symbolRows
        case .moreSymbols: moreSymbolRows
        case .malayalam: malayalamRows
        case .malayalamMore: malayalamMoreRows
        case .malayalamVowels: malayalamVowelRows
        }
        let bottom = page.isMalayalam ? malayalamBottomRow(page) : latinBottomRow(page)
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
        [
            Key(page == .letters ? .page(.symbols) : .page(.letters), width: 1.5),
            Key(.nextKeyboard),
            Key(.toggleLanguage),
            Key(.space, width: 4.5),
            // As SwiftKey: the full stop shows right above the key, ? to its right, the rest left.
            Key(.character("."), alternates: ["'", "!", ",", ".", "?"]),
            Key(.returnKey, width: 2),
        ]
    }

    // MARK: Malayalam

    private static let malayalamRows: [[Key]] = [
        malayalam("ക ഖ ഗ ഘ ങ ച ഛ ജ ഝ ഞ"),
        malayalam("ട ഠ ഡ ഢ ണ ത ഥ ദ ധ ന"),
        malayalam("പ ഫ ബ ഭ മ യ ര ല വ") + [Key(.backspace, width: 1.5)],
    ]

    private static let malayalamMoreRows: [[Key]] = [
        malayalam("ശ ഷ സ ഹ ള ഴ റ"),
        keys(Malayalam.chillus),
        keys(["ം", "ഃ", "്", ".", ",", "?"]) + [Key(.backspace, width: 1.5)],
    ]

    private static let malayalamVowelRows: [[Key]] = [
        keys(["അ", "ആ", "ഇ", "ഈ", "ഉ", "ഊ", "ഋ", "എ", "ഏ", "ഐ"]),
        keys(["ഒ", "ഓ", "ഔ", "ം", "ഃ"] + Malayalam.vowelSigns.prefix(5)), // ാ ി ീ ു ൂ
        keys(Array(Malayalam.vowelSigns.dropFirst(5))) + [Key(.backspace, width: 1.5)], // ൃ െ േ ൈ ൊ ോ ൗ ്
    ]

    private static func malayalamBottomRow(_ page: KeyboardPage) -> [Key] {
        [
            Key(.page(.symbols), width: 1.25),
            Key(.nextKeyboard),
            Key(.toggleLanguage),
            Key(page == .malayalamVowels ? .page(.letters) : .page(.malayalamVowels), width: 1.25),
            Key(.space, width: 3.5),
            Key(page == .malayalamMore ? .page(.letters) : .page(.malayalamMore), width: 1.25),
            Key(.returnKey, width: 1.5),
        ]
    }

    /// Space-separated Malayalam letters; consonants with a chillu get it as their long-press option.
    private static func malayalam(_ spaced: String) -> [Key] {
        spaced.split(separator: " ").map { letter in
            let text = String(letter)
            return Key(.character(text), alternates: Malayalam.chillu(for: text).map { [$0] } ?? [])
        }
    }

    private static func keys(_ list: [String]) -> [Key] {
        list.map { Key(.character($0)) }
    }
}

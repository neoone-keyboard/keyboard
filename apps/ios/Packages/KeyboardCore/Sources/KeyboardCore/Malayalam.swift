/// Malayalam data used by the native layout.
public enum Malayalam {
    /// Vowel signs offered after a consonant, in alphabet order, then virama for conjuncts.
    /// Uses ൗ (U+0D57) for "au", as modern spelling does.
    public static let vowelSigns: [String] = [
        "\u{0D3E}", "\u{0D3F}", "\u{0D40}", "\u{0D41}", "\u{0D42}", "\u{0D43}",
        "\u{0D46}", "\u{0D47}", "\u{0D48}", "\u{0D4A}", "\u{0D4B}", "\u{0D57}", "\u{0D4D}",
    ]

    /// What the strip offers after a consonant, in aksharamala order:
    /// കാ … കൗ, then കം കഃ, then ക് for conjuncts.
    public static let stripSigns: [String] = vowelSigns.dropLast() + ["\u{0D02}", "\u{0D03}", "\u{0D4D}"]

    /// The strip's signs with the engine's likeliest first (`preferred`, most
    /// likely first), then the rest in aksharamala order. Unknown signs in
    /// `preferred` are ignored, so the strip always offers exactly `stripSigns`.
    public static func orderedStripSigns(preferred: [String]) -> [String] {
        var seen = Set<String>()
        let first = preferred.filter { stripSigns.contains($0) && seen.insert($0).inserted }
        return first + stripSigns.filter { !seen.contains($0) }
    }

    /// Atomic chillu letters (U+0D7A–U+0D7F): ൺ ൻ ർ ൽ ൾ ൿ.
    public static let chillus: [String] = ["\u{0D7A}", "\u{0D7B}", "\u{0D7C}", "\u{0D7D}", "\u{0D7E}", "\u{0D7F}"]

    private static let chilluByConsonant: [String: String] = [
        "\u{0D23}": "\u{0D7A}", // ണ → ൺ
        "\u{0D28}": "\u{0D7B}", // ന → ൻ
        "\u{0D30}": "\u{0D7C}", // ര → ർ
        "\u{0D32}": "\u{0D7D}", // ല → ൽ
        "\u{0D33}": "\u{0D7E}", // ള → ൾ
        "\u{0D15}": "\u{0D7F}", // ക → ൿ
    ]

    /// The atomic chillu for a consonant, offered on long-press.
    public static func chillu(for consonant: String) -> String? {
        chilluByConsonant[consonant]
    }
}

//! Malayalam script rules shared by every platform.

/// True for Malayalam consonants that can take a vowel sign (U+0D15–U+0D3A).
/// Chillu letters (U+0D7A–U+0D7F) are excluded: they never take a vowel sign.
pub fn is_consonant(c: char) -> bool {
    ('\u{0D15}'..='\u{0D3A}').contains(&c)
}

/// The consonant at the very end of `context_before`, if typing a vowel sign
/// next would attach to it. Used to show the vowel-sign strip.
pub fn vowel_sign_base(context_before: &str) -> Option<char> {
    context_before.chars().last().filter(|&c| is_consonant(c))
}

/// The dependent vowel signs a loose spelling may add
/// (ാ ി ീ ു ൂ ൃ െ േ ൈ ൊ ോ).
const VOWEL_SIGNS: [char; 11] = [
    '\u{0D3E}', '\u{0D3F}', '\u{0D40}', '\u{0D41}', '\u{0D42}', '\u{0D43}', '\u{0D46}', '\u{0D47}',
    '\u{0D48}', '\u{0D4A}', '\u{0D4B}',
];

/// Every spelling of `word` with one vowel sign dropped (ആവിശ്യം → ആവശ്യം):
/// the vowel loose spelling inserts between consonants.
pub fn vowel_sign_drops(word: &str) -> Vec<String> {
    let chars: Vec<char> = word.chars().collect();
    (0..chars.len())
        .filter(|&i| VOWEL_SIGNS.contains(&chars[i]))
        .map(|i| chars[..i].iter().chain(&chars[i + 1..]).collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vowel_sign_drops_drop_one_sign() {
        assert_eq!(vowel_sign_drops("ആവിശ്യം"), ["ആവശ്യം"]);
        assert_eq!(vowel_sign_drops("കിളി"), ["കളി", "കിള"]);
        assert!(vowel_sign_drops("അവൻ").is_empty());
    }
    use proptest::prelude::*;

    #[test]
    fn bare_consonant_is_a_base() {
        assert_eq!(vowel_sign_base("ക"), Some('ക'));
        assert_eq!(vowel_sign_base("അവ"), Some('വ'));
    }

    #[test]
    fn conjunct_ending_in_consonant_uses_last_consonant() {
        // ന + ് + റ: the next vowel sign attaches to റ.
        assert_eq!(vowel_sign_base("ന്റ"), Some('റ'));
    }

    #[test]
    fn no_base_after_vowel_sign_virama_chillu_or_space() {
        assert_eq!(vowel_sign_base("കി"), None);
        assert_eq!(vowel_sign_base("ക്"), None);
        assert_eq!(vowel_sign_base("അവൻ"), None);
        assert_eq!(vowel_sign_base("ക "), None);
        assert_eq!(vowel_sign_base("അ"), None);
        assert_eq!(vowel_sign_base("abc"), None);
        assert_eq!(vowel_sign_base(""), None);
    }

    #[test]
    fn consonant_range_bounds() {
        assert!(is_consonant('\u{0D15}'));
        assert!(is_consonant('\u{0D3A}'));
        assert!(!is_consonant('\u{0D14}'));
        assert!(!is_consonant('\u{0D3B}'));
        assert!(!is_consonant('\u{0D7B}'));
    }

    proptest! {
        #[test]
        fn base_is_always_the_last_char_and_a_consonant(s in "\\PC{0,24}") {
            if let Some(c) = vowel_sign_base(&s) {
                prop_assert!(is_consonant(c));
                prop_assert_eq!(s.chars().last(), Some(c));
            }
        }

        #[test]
        fn malayalam_text_never_panics(s in "[\u{0D00}-\u{0D7F}\u{200C}\u{200D} ]{0,24}") {
            let _ = vowel_sign_base(&s);
        }
    }
}

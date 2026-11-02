//! One normal form for every word the engine stores or looks up: NFC with
//! atomic chillu letters. `lexbuild` and the runtime both call `normalize`,
//! so the two can never disagree about how a word is spelled. The curly
//! apostrophe (iOS smart punctuation, most corpus text) folds to `'`.

use unicode_normalization::UnicodeNormalization;

const VIRAMA: char = '\u{0D4D}';
const ZWJ: char = '\u{200D}';

/// The atomic chillu for a consonant written as consonant + virama + ZWJ (old encoding).
fn chillu(consonant: char) -> Option<char> {
    match consonant {
        'ണ' => Some('ൺ'),
        'ന' => Some('ൻ'),
        'ര' => Some('ർ'),
        'ല' => Some('ൽ'),
        'ള' => Some('ൾ'),
        'ക' => Some('ൿ'),
        _ => None,
    }
}

/// NFC, then every consonant + virama + ZWJ sequence that has an atomic chillu becomes that chillu,
/// and `’` becomes `'`.
pub fn normalize(text: &str) -> String {
    let nfc: Vec<char> = text.nfc().map(|c| if c == '\u{2019}' { '\'' } else { c }).collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < nfc.len() {
        let c = nfc[i];
        if nfc.get(i + 1) == Some(&VIRAMA)
            && nfc.get(i + 2) == Some(&ZWJ)
            && let Some(ch) = chillu(c)
        {
            out.push(ch);
            i += 3;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::normalize;
    use proptest::prelude::*;

    #[test]
    fn old_chillu_encoding_becomes_atomic() {
        assert_eq!(normalize("അവന\u{0D4D}\u{200D}"), "അവൻ");
        assert_eq!(normalize("അവർ"), "അവർ");
        assert_eq!(normalize("കാല\u{0D4D}\u{200D}"), "കാൽ");
    }

    #[test]
    fn virama_without_zwj_is_kept() {
        assert_eq!(normalize("എന്റെ"), "എന്റെ");
    }

    #[test]
    fn composes_two_part_vowel_signs() {
        // െ + ാ is canonically ൊ.
        assert_eq!(normalize("ക\u{0D46}\u{0D3E}"), "ക\u{0D4A}");
    }

    #[test]
    fn latin_is_untouched_except_nfc() {
        assert_eq!(normalize("Hello"), "Hello");
        assert_eq!(normalize("cafe\u{301}"), "caf\u{e9}");
    }

    #[test]
    fn curly_apostrophe_folds_to_straight() {
        assert_eq!(normalize("don\u{2019}t"), "don't");
    }

    proptest! {
        #[test]
        fn normalize_is_idempotent(s in "[\u{0D00}-\u{0D7F}\u{200C}\u{200D}a-z ]{0,24}") {
            let once = normalize(&s);
            prop_assert_eq!(normalize(&once), once.clone());
        }
    }
}

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
/// a ZWJ or ZWNJ is dropped unless it follows a virama inside a word, and `’` becomes `'`.
///
/// Dropping a joiner can leave text that one more pass would change: `െ` + ZWNJ + `ാ` becomes
/// `ൊ`, which NFC composes to `ൊ`, and `ന്` + ZWNJ + ZWJ + `ക` becomes `ന്` + ZWJ + `ക`, an old
/// chillu. So the pass repeats until nothing changes. Each pass that changes anything removes at
/// least one character, so this ends, and the second pass is the only one ordinary text needs.
pub fn normalize(text: &str) -> String {
    let mut out = pass(text);
    loop {
        let next = pass(&out);
        if next == out {
            return out;
        }
        out = next;
    }
}

fn pass(text: &str) -> String {
    let nfc: Vec<char> = text
        .nfc()
        .map(|c| if c == '\u{2019}' { '\'' } else { c })
        .collect();
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
        if is_joiner(c)
            && !(out.ends_with(VIRAMA) && nfc.get(i + 1).is_some_and(|&n| is_malayalam(n)))
        {
            i += 1; // only virama + joiner + letter changes how a word is drawn
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

fn is_joiner(c: char) -> bool {
    matches!(c, '\u{200C}' | ZWJ)
}

fn is_malayalam(c: char) -> bool {
    ('\u{0D00}'..='\u{0D7F}').contains(&c)
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
    fn joiners_at_word_edges_are_dropped() {
        // Wikipedia writes "ആണ്‌" with a trailing ZWNJ; it must count as "ആണ്".
        assert_eq!(normalize("ആണ്\u{200C}"), "ആണ്");
        assert_eq!(normalize("ആണ്\u{200C} ഒരു"), "ആണ് ഒരു");
        assert_eq!(normalize("\u{200C}തന്നെ"), "തന്നെ");
        assert_eq!(normalize("x \u{200D}കെ."), "x കെ.");
        assert_eq!(normalize("ഴ്\u{200D}"), "ഴ്");
        assert_eq!(normalize("\u{200C}\u{200D}"), "");
    }

    #[test]
    fn joiners_inside_a_word_are_kept() {
        assert_eq!(normalize("ഛത്തീസ്\u{200C}ഗഡ്"), "ഛത്തീസ്\u{200C}ഗഡ്");
        assert_eq!(normalize("ഗ്\u{200D}ക"), "ഗ്\u{200D}ക");
        assert_eq!(normalize("ക്\u{200C}\u{200C}ക"), "ക്\u{200C}ക");
    }

    #[test]
    fn joiners_that_change_nothing_are_dropped() {
        // Before a vowel sign, or after an atomic chillu, a joiner is invisible.
        assert_eq!(normalize("ഷ്ട്യ\u{200D}ാ"), "ഷ്ട്യാ");
        assert_eq!(normalize("റെയിൽ\u{200C}വേ"), "റെയിൽവേ");
        assert_eq!(normalize("ക\u{200C}ക"), "കക");
    }

    #[test]
    fn text_left_by_a_dropped_joiner_is_normalised_too() {
        // Found by normalize_is_idempotent: the vowel sign halves meet once the ZWNJ goes.
        assert_eq!(normalize("\u{0D46}\u{200C}\u{0D3E}"), "\u{0D4A}");
        assert_eq!(normalize("ക\u{0D46}\u{200C}\u{0D3E}"), "ക\u{0D4A}");
        // The stray ZWNJ goes and leaves an old-style chillu (virama + ZWJ).
        assert_eq!(normalize("ന\u{0D4D}\u{200C}\u{200D}ക"), "ൻക");
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

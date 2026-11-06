//! Devanagari (deva), for Hindi. The normal form is the engine's `normalize::normalize`, as
//! `lexbuild --lang` used for `hi.fst`: NFC (a nukta letter such as ज़ U+095B becomes its
//! consonant + nukta U+093C, while ऩ ऱ ऴ stay single code points), `’` to `'`, and every ZWJ and
//! ZWNJ dropped (only the Malayalam virama keeps one). A word is a run of the block's letters
//! and signs, virama and nukta included; the danda । and double danda ॥ end a phrase, and the
//! digits and the abbreviation sign are not letters. No joins, so a Devanagari pack has no
//! suffix splitting.

use std::ops::RangeInclusive;

use super::ScriptRules;

const NUKTA: char = '\u{093C}';

#[derive(Debug)]
pub struct Deva;

/// A consonant a vowel sign can follow: क–ह, the precomposed nukta letters (which NFC
/// decomposes, but text may still hold), and the extra letters at the end of the block.
fn is_consonant(c: char) -> bool {
    matches!(c, '\u{0915}'..='\u{0939}' | '\u{0958}'..='\u{095F}' | '\u{0978}'..='\u{097F}')
}

impl ScriptRules for Deva {
    fn code(&self) -> &'static str {
        "deva"
    }
    fn block(&self) -> RangeInclusive<char> {
        '\u{0900}'..='\u{097F}'
    }
    fn is_letter(&self, c: char) -> bool {
        // Danda, double danda, the digits and the abbreviation sign (U+0964–U+0970) are not,
        // as in lexbuild's table for the script.
        self.block().contains(&c) && !('\u{0964}'..='\u{0970}').contains(&c)
    }
    fn normalize(&self, word: &str) -> String {
        crate::normalize::normalize(word)
    }
    fn mark_base(&self, context_before: &str) -> Option<char> {
        let mut chars = context_before.chars().rev();
        match chars.next()? {
            NUKTA => chars.next().filter(|&c| is_consonant(c)),
            c => Some(c).filter(|&c| is_consonant(c)),
        }
    }
    fn ends_phrase(&self, c: char) -> bool {
        matches!(c, '\u{0964}' | '\u{0965}')
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_form_is_nfc_without_joiners() {
        let d = Deva;
        // ज़ (U+095B) is a composition exclusion: NFC gives ज + nukta.
        assert_eq!(d.normalize("\u{095B}िंदगी"), "\u{091C}\u{093C}िंदगी");
        assert_eq!(d.normalize("\u{095E}िल्म"), "\u{092B}\u{093C}िल्म");
        // ऩ ऱ ऴ compose and stay one code point.
        assert_eq!(d.normalize("\u{0928}\u{093C}"), "\u{0929}");
        assert_eq!(d.normalize("\u{0931}"), "\u{0931}");
        assert_eq!(d.normalize("\u{0934}"), "\u{0934}");
        // A joiner after a virama only changes the drawing; the lexicon has none.
        assert_eq!(d.normalize("क्\u{200D}ष"), "क्ष");
        assert_eq!(d.normalize("क्\u{200C}ष"), "क्ष");
        assert_eq!(d.normalize("नमस्ते"), "नमस्ते");
        assert_eq!(d.normalize("कह’"), "कह'");
    }

    #[test]
    fn danda_and_digits_are_not_letters() {
        let d = Deva;
        for c in ['।', '॥', '०', '९', '॰'] {
            assert!(!d.is_letter(c), "{c}");
        }
        for c in ['क', 'अ', 'ा', '्', '़', 'ं', 'ँ', 'ः', 'ॐ', 'ऽ', '\u{0929}']
        {
            assert!(d.is_letter(c), "{c}");
        }
        assert!(!d.is_letter('a'));
        assert!(!d.is_letter('ക'));
        assert!(d.ends_phrase('।') && d.ends_phrase('॥') && !d.ends_phrase('.'));
    }

    #[test]
    fn mark_base_is_the_last_consonant() {
        let d = Deva;
        assert_eq!(d.mark_base("क"), Some('क'));
        assert_eq!(d.mark_base("नमस"), Some('स'));
        assert_eq!(d.mark_base("ज\u{093C}"), Some('ज'), "consonant + nukta");
        assert_eq!(d.mark_base("\u{095B}"), Some('\u{095B}'));
        assert_eq!(d.mark_base("कि"), None, "a vowel sign already");
        assert_eq!(d.mark_base("क्"), None, "after a virama");
        assert_eq!(d.mark_base("अ"), None, "an independent vowel");
        assert_eq!(d.mark_base("कं"), None);
        assert_eq!(d.mark_base("\u{093C}"), None);
        assert_eq!(d.mark_base("abc"), None);
        assert_eq!(d.mark_base(""), None);
    }
}

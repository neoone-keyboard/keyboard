//! Bengali-Assamese (beng), written today for Assamese. The normal form is the one
//! `lexbuild-native` built `as.fst` with (`data/langs/as/lang.json`, `Lang::normalize`): the
//! replacements below, the engine's `normalize::normalize` (NFC, `’` to `'`, every ZWJ and ZWNJ
//! dropped, as only the Malayalam virama keeps one), and the replacements again:
//!
//! - ত + virama + ZWJ, the old encoding of khanda ta, becomes ৎ (before the joiner is lost);
//! - the Bengali ra র (U+09B0) folds to the Assamese ৰ (U+09F0); ৰ ৱ ব are kept;
//! - া + ে (the halves of ো in visual order) becomes ো, and অ + া becomes আ.
//!
//! NFC leaves য় ড় ঢ় as base + nukta (U+09DF, U+09DC, U+09DD are composition exclusions).
//!
//! The ra fold is Assamese's, not the script's: a Bengali pack would need its own normal form
//! (a language-level rule, or a plug-in per language). Assamese is the only beng pack.
//!
//! A word is a run of the block's letters and signs, with apostrophes inside (হ'ল, ক'ত: the
//! apostrophe marks the vowel o); the digits and the currency and fraction signs are not
//! letters, and the danda ends a phrase (। ॥, and ৷ U+09F7, often typed for a danda).

use std::ops::RangeInclusive;

use super::ScriptRules;

const NUKTA: char = '\u{09BC}';

/// `data/langs/as/lang.json` `replace`, in its order.
const REPLACE: [(&str, &str); 4] = [
    ("\u{09A4}\u{09CD}\u{200D}", "\u{09CE}"),
    ("\u{09B0}", "\u{09F0}"),
    ("\u{09BE}\u{09C7}", "\u{09CB}"),
    ("\u{0985}\u{09BE}", "\u{0986}"),
];

#[derive(Debug)]
pub struct Beng;

fn replace(text: &str) -> String {
    let mut out = text.to_string();
    for (from, to) in REPLACE {
        if out.contains(from) {
            out = out.replace(from, to);
        }
    }
    out
}

/// A consonant a vowel sign can follow (`lang.json` `bases`, less the nukta). Khanda ta ৎ
/// takes none.
fn is_consonant(c: char) -> bool {
    matches!(c, '\u{0995}'..='\u{09B9}' | '\u{09DC}'..='\u{09DF}' | '\u{09F0}' | '\u{09F1}')
}

impl ScriptRules for Beng {
    fn code(&self) -> &'static str {
        "beng"
    }
    fn block(&self) -> RangeInclusive<char> {
        '\u{0980}'..='\u{09FF}'
    }
    fn is_letter(&self, c: char) -> bool {
        // `lang.json` `notWord`: the digits (U+09E6–U+09EF) and the currency, fraction and
        // other signs (U+09F2–U+09FE). ৰ U+09F0 and ৱ U+09F1 are letters.
        self.block().contains(&c)
            && !('\u{09E6}'..='\u{09EF}').contains(&c)
            && !('\u{09F2}'..='\u{09FE}').contains(&c)
    }
    fn normalize(&self, word: &str) -> String {
        // As `lexbuild::native::Lang::normalize`: a replacement can make text that NFC or the
        // joiner rules would change again, so one more round when it does.
        let before = replace(&crate::normalize::normalize(&replace(word)));
        let after = replace(&crate::normalize::normalize(&before));
        if after == before { before } else { after }
    }
    fn mark_base(&self, context_before: &str) -> Option<char> {
        let mut chars = context_before.chars().rev();
        match chars.next()? {
            NUKTA => chars.next().filter(|&c| is_consonant(c)),
            c => Some(c).filter(|&c| is_consonant(c)),
        }
    }
    fn ends_phrase(&self, c: char) -> bool {
        matches!(c, '\u{0964}' | '\u{0965}' | '\u{09F7}')
    }
    fn inner_apostrophe(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assamese_letters_are_kept_and_bengali_ra_folds() {
        let b = Beng;
        assert_eq!(b.normalize("ৰাজ্য"), "ৰাজ্য");
        assert_eq!(b.normalize("ৱিকি"), "ৱিকি");
        assert_eq!(b.normalize("বাট"), "বাট");
        assert_eq!(b.normalize("তোমার"), "তোমাৰ");
        assert_eq!(b.normalize("প্রথম"), "প্ৰথম");
    }

    #[test]
    fn khanda_ta_becomes_atomic() {
        let b = Beng;
        assert_eq!(b.normalize("উৎসৱ"), "উৎসৱ");
        assert_eq!(b.normalize("উত\u{09CD}\u{200D}সৱ"), "উৎসৱ");
    }

    #[test]
    fn visual_order_o_and_aa_are_mended() {
        let b = Beng;
        assert_eq!(b.normalize("মােৰ"), "মোৰ");
        assert_eq!(b.normalize("অাৰু"), "আৰু");
        // ে + া is canonically ো already.
        assert_eq!(b.normalize("ম\u{09C7}\u{09BE}ৰ"), "মোৰ");
    }

    #[test]
    fn nukta_letters_are_base_plus_nukta() {
        let b = Beng;
        assert_eq!(b.normalize("\u{09DF}"), "\u{09AF}\u{09BC}");
        assert_eq!(b.normalize("\u{09DC}"), "\u{09A1}\u{09BC}");
        assert_eq!(b.normalize("\u{09DD}"), "\u{09A2}\u{09BC}");
        assert_eq!(b.normalize("অসমীয়া"), "অসমীয\u{09BC}া");
    }

    #[test]
    fn other_joiners_are_dropped() {
        let b = Beng;
        assert_eq!(b.normalize("ক্\u{200C}ষ"), "ক্ষ");
        assert_eq!(b.normalize("ক্\u{200D}ষ"), "ক্ষ");
        assert_eq!(b.normalize("\u{200C}মই"), "মই");
        assert_eq!(b.normalize("হ’ল"), "হ'ল");
    }

    #[test]
    fn digits_and_signs_are_not_letters() {
        let b = Beng;
        for c in ['০', '৯', '৳', '৷', '৺', '\u{09FE}'] {
            assert!(!b.is_letter(c), "{c}");
        }
        for c in [
            'ক', 'অ', 'া', '্', '়', 'ং', 'ঁ', 'ঃ', 'ৎ', 'ৰ', 'ৱ', '\u{09DC}',
        ] {
            assert!(b.is_letter(c), "{c}");
        }
        assert!(!b.is_letter('।'), "the danda is Devanagari's");
        assert!(b.ends_phrase('।') && b.ends_phrase('৷') && !b.ends_phrase(','));
    }

    #[test]
    fn mark_base_is_the_last_consonant() {
        let b = Beng;
        assert_eq!(b.mark_base("ক"), Some('ক'));
        assert_eq!(b.mark_base("অসম"), Some('ম'));
        assert_eq!(b.mark_base("মোৰ"), Some('ৰ'));
        assert_eq!(b.mark_base("ৱ"), Some('ৱ'));
        assert_eq!(b.mark_base("য\u{09BC}"), Some('য'), "consonant + nukta");
        assert_eq!(b.mark_base("কি"), None);
        assert_eq!(b.mark_base("ক্"), None);
        assert_eq!(b.mark_base("উৎ"), None, "khanda ta takes no vowel sign");
        assert_eq!(b.mark_base("অ"), None);
        assert_eq!(b.mark_base("হ'"), None);
        assert_eq!(b.mark_base(""), None);
    }
}

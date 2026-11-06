//! Script plug-ins (M6 phase 2). Each script a pack can be written in has one implementation of
//! [`ScriptRules`], keyed by its ISO 15924 code in lower case. The rules are code because they
//! are behaviour (how a word is normalised, where a vowel sign attaches); a pack's tables and
//! word lists are data and live in the pack.

mod mlym;

use std::ops::RangeInclusive;

/// What the engine needs to know about one native script.
pub trait ScriptRules: Send + Sync + std::fmt::Debug {
    /// ISO 15924, lower case: "mlym".
    fn code(&self) -> &'static str;
    /// The script's Unicode block. Corpus tokenising and the romanised-lexicon packing use it.
    fn block(&self) -> RangeInclusive<char>;
    /// A letter or sign a word in this script may be made of (ZWJ and ZWNJ are handled by the
    /// caller). Narrower than `block`: digits and punctuation in the block are not letters.
    fn is_letter(&self, c: char) -> bool;
    /// The script's half of the normal form. The engine normalises every word with
    /// `normalize::normalize`, which today includes Malayalam's chillu rules.
    fn normalize(&self, word: &str) -> String;
    /// The consonant at the end of `context_before` that a vowel sign typed next would attach
    /// to, if any. The keyboard shows the vowel-sign strip when there is one.
    fn mark_base(&self, context_before: &str) -> Option<char>;
    /// Spellings of `word` with one medial vowel sign dropped (loose spelling inserts them).
    fn vowel_sign_drops(&self, _word: &str) -> Vec<String> {
        Vec::new()
    }
    /// The other spelling of a word whose ending is written two ways, if the script has one.
    fn other_final_u(&self, _word: &str) -> Option<String> {
        None
    }
}

static REGISTRY: &[&dyn ScriptRules] = &[&mlym::Mlym];

/// The rules for a script code, if this engine has them.
pub fn rules(code: &str) -> Option<&'static dyn ScriptRules> {
    REGISTRY.iter().copied().find(|r| r.code() == code)
}

/// Every script code this engine has rules for.
pub fn supported() -> Vec<&'static str> {
    REGISTRY.iter().map(|r| r.code()).collect()
}

/// Every registered script.
pub fn all() -> impl Iterator<Item = &'static dyn ScriptRules> {
    REGISTRY.iter().copied()
}

/// True if `c` is in the block of any registered script.
pub fn in_any_block(c: char) -> bool {
    REGISTRY.iter().any(|r| r.block().contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malayalam_is_registered() {
        assert_eq!(supported(), vec!["mlym"]);
        let ml = rules("mlym").unwrap();
        assert_eq!(ml.code(), "mlym");
        assert!(ml.block().contains(&'ക'));
        assert!(rules("deva").is_none());
    }

    #[test]
    fn malayalam_rules_match_the_old_functions() {
        let ml = rules("mlym").unwrap();
        for ctx in ["ക", "ന്റ", "കി", "ക്", "അവൻ", "abc", ""] {
            assert_eq!(
                ml.mark_base(ctx),
                crate::malayalam::vowel_sign_base(ctx),
                "{ctx:?}"
            );
        }
        for w in ["ഇന്ന്", "അതു", "വേണം", "ആവിശ്യം"] {
            assert_eq!(ml.other_final_u(w), crate::malayalam::other_final_u(w));
            assert_eq!(
                ml.vowel_sign_drops(w),
                crate::malayalam::vowel_sign_drops(w)
            );
            assert_eq!(ml.normalize(w), crate::normalize::normalize(w));
        }
    }
}

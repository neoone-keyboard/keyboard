//! Script plug-ins (M6 phase 2). Each script a pack can be written in has one implementation of
//! [`ScriptRules`], keyed by its ISO 15924 code in lower case. The rules are code because they
//! are behaviour (how a word is normalised, where a vowel sign attaches); a pack's tables and
//! word lists are data and live in the pack.

mod beng;
mod deva;
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
    /// A stem and a pack suffix (`suffix::Suffixes`) joined the way `how` says, or None when
    /// they don't join. A script without joins never joins, so its packs are never split.
    fn join_suffix(&self, _stem: &str, _suffix: &str, _how: crate::suffix::Join) -> Option<String> {
        None
    }
    /// A char of the block that is part of a word when splitting text (`text::words`).
    /// `is_letter` unless the script says otherwise.
    fn is_word_char(&self, c: char) -> bool {
        self.is_letter(c)
    }
    /// Punctuation of the script that ends a phrase as a full stop does (the danda): next-word
    /// pairs never span it.
    fn ends_phrase(&self, _c: char) -> bool {
        false
    }
    /// Whether an apostrophe may stand inside a word (Assamese হ'ল), never first or last.
    fn inner_apostrophe(&self) -> bool {
        false
    }
}

/// Malayalam first: `text::script` asks in this order, and the blocks don't overlap.
static REGISTRY: &[&dyn ScriptRules] = &[&mlym::Mlym, &deva::Deva, &beng::Beng];

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

/// True if `c` is part of a word in any registered script (`ScriptRules::is_word_char`).
pub fn is_word_char(c: char) -> bool {
    REGISTRY.iter().any(|r| r.is_word_char(c))
}

/// True if `c` ends a phrase in any registered script (`ScriptRules::ends_phrase`).
pub fn ends_phrase(c: char) -> bool {
    REGISTRY.iter().any(|r| r.ends_phrase(c))
}

/// A word's normal form: `normalize::normalize`, then, for a word of a native script
/// (`text::script`), that script's own rules (`ScriptRules::normalize`), which are what built
/// its pack's lexicon. Malayalam's rules are `normalize` itself, so its words are unchanged.
pub fn normalize_word(word: &str) -> String {
    let normal = crate::normalize::normalize(word);
    match crate::text::script(&normal) {
        crate::text::Script::Native(code) => match rules(code) {
            Some(r) => r.normalize(&normal),
            None => normal,
        },
        _ => normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_are_registered() {
        assert_eq!(supported(), vec!["mlym", "deva", "beng"]);
        let ml = rules("mlym").unwrap();
        assert_eq!(ml.code(), "mlym");
        assert!(ml.block().contains(&'ക'));
        assert!(rules("deva").unwrap().block().contains(&'क'));
        assert!(rules("beng").unwrap().block().contains(&'ৰ'));
        assert!(rules("taml").is_none());
    }

    #[test]
    fn blocks_do_not_overlap() {
        let scripts: Vec<_> = all().collect();
        for (i, a) in scripts.iter().enumerate() {
            for b in &scripts[i + 1..] {
                let overlap =
                    a.block().start() <= b.block().end() && b.block().start() <= a.block().end();
                assert!(!overlap, "{} and {}", a.code(), b.code());
            }
        }
    }

    #[test]
    fn malayalam_word_chars_are_its_whole_block() {
        // As before the other scripts came: text::words splits Malayalam text exactly as then.
        for c in '\u{0D00}'..='\u{0D7F}' {
            assert!(is_word_char(c), "{c:?}");
        }
        assert!(!is_word_char('।') && !is_word_char('০') && is_word_char('্'));
    }

    #[test]
    fn words_are_normalised_by_their_script() {
        assert_eq!(normalize_word("তোমার"), "তোমাৰ");
        assert_eq!(normalize_word("\u{095B}रा"), "ज\u{093C}रा");
        assert_eq!(normalize_word("അവന\u{0D4D}\u{200D}"), "അവൻ");
        assert_eq!(normalize_word("don\u{2019}t"), "don't");
        assert_eq!(
            normalize_word("ab তোমার"),
            "ab তোমার",
            "not one word of the script"
        );
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

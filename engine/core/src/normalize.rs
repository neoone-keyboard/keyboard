//! One normal form for every word the engine stores or looks up: NFC, the curly apostrophe
//! (iOS smart punctuation, most corpus text) folded to `'`, and ZWJ and ZWNJ kept only where a
//! language's rules say they change a word (`Joiners`, from its pack's `lang.json`).
//! `lexbuild` and the runtime both call `normalize`, so the two can never disagree about how a
//! word is spelled.
//!
//! Nothing here is written for one script. Malayalam's rules (a joiner is kept between its
//! virama and one of its letters; a consonant + virama + ZWJ that has an atomic chillu becomes
//! that chillu) are its `lang.json`'s `joiners` and `zwjForms`.

use unicode_normalization::UnicodeNormalization;

const ZWJ: char = '\u{200D}';

/// What one script does with ZWJ and ZWNJ (`lang.json`: `virama`, `joiners`, `zwjForms`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Joiners {
    pub virama: char,
    /// The script's characters, as ranges.
    pub ranges: Vec<(char, char)>,
    /// `"joiners": "afterVirama"`: a ZWJ or ZWNJ between the virama and a character of the
    /// script is part of the word (it changes how the cluster is drawn). Otherwise dropped.
    pub keep: bool,
    /// `zwjForms`: `(consonant, letter)`. The consonant + virama + ZWJ is the old encoding of
    /// the letter (Malayalam's chillus), and becomes it.
    pub forms: Vec<(char, char)>,
}

impl Joiners {
    fn form(&self, consonant: char) -> Option<char> {
        self.forms.iter().find(|&&(c, _)| c == consonant).map(|&(_, f)| f)
    }

    fn has(&self, c: char) -> bool {
        self.ranges.iter().any(|&(a, b)| (a..=b).contains(&c))
    }
}

/// `normalize_with` the joiner rules of each script in use: those of the first rule set in
/// use for it (`script::firsts`). With two languages of one script loaded, the engine reads a
/// word by its own language's instead (`Engine::read`, `script::plain`).
pub fn normalize(text: &str) -> String {
    let scripts = crate::script::firsts();
    let rules: Vec<&Joiners> = scripts.iter().filter_map(|r| r.joiners()).collect();
    normalize_with(text, &rules)
}

/// NFC, then for each script of `rules`: every consonant + virama + ZWJ sequence that has an
/// atomic form (`zwjForms`) becomes it. A ZWJ or ZWNJ is dropped unless it stands between the
/// virama and a character of a script that keeps joiners there, and `’` becomes `'`.
///
/// Dropping a joiner can leave text that one more pass would change: `െ` + ZWNJ + `ാ` becomes
/// `ൊ`, which NFC composes to `ൊ`, and `ന്` + ZWNJ + ZWJ + `ക` becomes `ന്` + ZWJ + `ക`, an old
/// chillu. So the pass repeats until nothing changes. Each pass that changes anything removes at
/// least one character, so this ends, and the second pass is the only one ordinary text needs.
pub fn normalize_with(text: &str, rules: &[&Joiners]) -> String {
    let mut out = pass(text, rules);
    loop {
        let next = pass(&out, rules);
        if next == out {
            return out;
        }
        out = next;
    }
}

fn pass(text: &str, rules: &[&Joiners]) -> String {
    let nfc: Vec<char> = text
        .nfc()
        .map(|c| if c == '\u{2019}' { '\'' } else { c })
        .collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < nfc.len() {
        let c = nfc[i];
        if nfc.get(i + 2) == Some(&ZWJ)
            && let Some(form) = rules
                .iter()
                .filter(|r| nfc.get(i + 1) == Some(&r.virama))
                .find_map(|r| r.form(c))
        {
            out.push(form);
            i += 3;
            continue;
        }
        if is_joiner(c) {
            // Only virama + joiner + letter changes how a word is drawn.
            let kept = rules.iter().any(|r| {
                r.keep && out.ends_with(r.virama) && nfc.get(i + 1).is_some_and(|&n| r.has(n))
            });
            if !kept {
                i += 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

fn is_joiner(c: char) -> bool {
    matches!(c, '\u{200C}' | ZWJ)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn deva(keep: bool) -> Joiners {
        Joiners {
            virama: '\u{094D}',
            ranges: vec![('\u{0900}', '\u{097F}')],
            keep,
            forms: Vec::new(),
        }
    }

    #[test]
    fn a_script_keeps_joiners_after_its_virama_only_when_its_rules_say() {
        // Devanagari writes a half form with ZWJ and an explicit halant with ZWNJ.
        let (half, explicit) = ("क्\u{200D}ष", "क्\u{200C}ष");
        let dropped = deva(false);
        assert_eq!(normalize_with(half, &[&dropped]), "क्ष");
        assert_eq!(normalize_with(explicit, &[&dropped]), "क्ष");
        assert_eq!(normalize_with(half, &[]), "क्ष", "no rules: every joiner goes");
        let kept = deva(true);
        assert_eq!(normalize_with(half, &[&kept]), half);
        assert_eq!(normalize_with(explicit, &[&kept]), explicit);
        // Not at a word's end, before another script's letter, or after anything but the virama.
        assert_eq!(normalize_with("क्\u{200D}", &[&kept]), "क्");
        assert_eq!(normalize_with("क्\u{200D}ക", &[&kept]), "क्ക");
        assert_eq!(normalize_with("क\u{200C}ष", &[&kept]), "कष");
        // Malayalam's virama is not Devanagari's.
        assert_eq!(normalize_with("ക്\u{200C}ക", &[&kept]), "ക്ക");
    }

    #[test]
    fn atomic_forms_are_the_scripts_table() {
        let khanda = Joiners {
            virama: '\u{09CD}',
            ranges: vec![('\u{0980}', '\u{09FF}')],
            keep: false,
            forms: vec![('ত', 'ৎ')],
        };
        assert_eq!(normalize_with("উত\u{09CD}\u{200D}সৱ", &[&khanda]), "উৎসৱ");
        assert_eq!(normalize_with("ক\u{09CD}\u{200D}ষ", &[&khanda]), "ক্ষ", "ক has no such form");
        // Malayalam's chillus are data too: without its rules nothing is rewritten.
        assert_eq!(normalize_with("അവന\u{0D4D}\u{200D}", &[]), "അവന\u{0D4D}");
        assert_eq!(normalize_with("അവന\u{0D4D}\u{200D}", &[&khanda]), "അവന\u{0D4D}");
    }

    #[test]
    fn malayalams_rules_come_from_its_lang_file() {
        let rules = crate::script::embedded("mlym").unwrap().joiners().unwrap();
        assert!(rules.keep);
        assert_eq!(rules.virama, '\u{0D4D}');
        assert_eq!(rules.ranges, [('\u{0D00}', '\u{0D7F}')]);
        assert_eq!(
            rules.forms,
            [('ണ', 'ൺ'), ('ന', 'ൻ'), ('ര', 'ർ'), ('ല', 'ൽ'), ('ള', 'ൾ'), ('ക', 'ൿ')]
        );
        for code in ["deva", "beng"] {
            assert!(crate::script::embedded(code).unwrap().joiners().is_none(), "{code}");
        }
    }

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

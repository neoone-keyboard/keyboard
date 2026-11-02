//! Rule-based Latin → Malayalam, Mozhi-style, for names and words that aren't
//! in the lexicon. Deterministic and approximate: it can't know vowel length
//! the user didn't type, so it only ever fills a side slot.

const VIRAMA: char = '\u{0D4D}';
const VOCALIC_R_SIGN: char = '\u{0D43}';
const I_SIGN: &str = "\u{0D3F}";

/// Consonant spellings, longest first so "chh" wins over "ch" over "c".
const CONSONANTS: &[(&str, &str)] = &[
    ("chh", "ഛ"),
    ("nth", "ന്ത"),
    ("ndh", "ന്ധ"),
    ("nch", "ഞ്ച"),
    ("nj", "ഞ"),
    ("ng", "ങ"),
    ("nk", "ങ്ക"),
    ("nt", "ന്റ"),
    ("nd", "ന്ദ"),
    ("ch", "ച"),
    ("kh", "ഖ"),
    ("gh", "ഘ"),
    ("jh", "ഝ"),
    ("th", "ത"),
    ("dh", "ധ"),
    ("ph", "ഫ"),
    ("bh", "ഭ"),
    ("sh", "ഷ"),
    ("zh", "ഴ"),
    ("k", "ക"),
    ("g", "ഗ"),
    ("c", "ക"),
    ("q", "ക"),
    ("j", "ജ"),
    ("t", "ട"),
    ("d", "ദ"),
    ("n", "ന"),
    ("p", "പ"),
    ("f", "ഫ"),
    ("b", "ബ"),
    ("m", "മ"),
    ("y", "യ"),
    ("r", "ര"),
    ("l", "ല"),
    ("v", "വ"),
    ("w", "വ"),
    ("s", "സ"),
    ("z", "സ"),
    ("h", "ഹ"),
    ("x", "ക്സ"),
];

/// (spelling, independent vowel, vowel sign), longest first.
const VOWELS: &[(&str, &str, &str)] = &[
    ("aa", "ആ", "\u{0D3E}"),
    ("ai", "ഐ", "\u{0D48}"),
    ("au", "ഔ", "\u{0D57}"),
    ("ee", "ഈ", "\u{0D40}"),
    ("ii", "ഈ", "\u{0D40}"),
    ("oo", "ഊ", "\u{0D42}"),
    ("uu", "ഊ", "\u{0D42}"),
    ("a", "അ", ""),
    ("i", "ഇ", "\u{0D3F}"),
    ("u", "ഉ", "\u{0D41}"),
    ("e", "എ", "\u{0D46}"),
    ("o", "ഒ", "\u{0D4A}"),
];

/// Consonants that take the vocalic-r sign for a following "ri": krishna → കൃഷ്ണ.
/// Not "p" or "s": priya is പ്രിയ, sri is ശ്രീ.
const VOCALIC_R_AFTER: &[&str] = &["k", "h", "m", "v", "d", "th"];

type Vowel = (&'static str, &'static str, &'static str);
type Consonant = (&'static str, &'static str);

fn vowel_at(s: &str) -> Option<&'static Vowel> {
    VOWELS.iter().find(|(v, _, _)| s.starts_with(v))
}

fn consonant_at(s: &str) -> Option<&'static Consonant> {
    CONSONANTS.iter().find(|(c, _)| s.starts_with(c))
}

/// Chillu (or anusvara) for a word-final consonant: avan → അവൻ, maram → മരം.
fn final_form(letter: &str) -> Option<&'static str> {
    match letter {
        "ന" => Some("ൻ"),
        "ണ" => Some("ൺ"),
        "ല" => Some("ൽ"),
        "ര" => Some("ർ"),
        "മ" => Some("ം"),
        _ => None,
    }
}

/// True when `rest` (what follows a typed `e` or `o`) is one plain consonant
/// and then a vowel or the end of the word. Names spell long ഏ/ഓ that way
/// (sneha, suresh, mohan), while a short one is usually before a doubled
/// consonant or a cluster (kochi, kettu).
fn open_syllable_follows(rest: &str) -> bool {
    let Some(&(spelling, letter)) = consonant_at(rest) else {
        return false;
    };
    let after = &rest[spelling.len()..];
    spelling != "ch"
        && letter.chars().count() == 1
        && (after.is_empty() || vowel_at(after).is_some())
}

/// None unless `latin` is all ASCII letters.
pub fn transliterate(latin: &str) -> Option<String> {
    if latin.is_empty() || !latin.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let lower = latin.to_ascii_lowercase();
    let mut rest = lower.as_str();
    let mut out = String::new();
    // The Latin spelling and output offset of a consonant still waiting for its vowel.
    let mut pending: Option<(&str, usize)> = None;
    while !rest.is_empty() {
        if let Some(&(spelling, independent, sign)) = vowel_at(rest) {
            let after = &rest[spelling.len()..];
            let sign = match spelling {
                "e" if open_syllable_follows(after) => "\u{0D47}",
                "o" if open_syllable_follows(after) => "\u{0D4B}",
                _ => sign,
            };
            out.push_str(if pending.is_some() { sign } else { independent });
            pending = None;
            rest = after;
            continue;
        }
        let &(spelling, mut letter) = consonant_at(rest)?;
        let after = &rest[spelling.len()..];
        let previous = pending.map(|(s, _)| s);
        // A final "y" after a consonant is the vowel ി: revathy → രേവതി.
        if spelling == "y" && after.is_empty() && pending.is_some() {
            out.push_str(I_SIGN);
            pending = None;
            break;
        }
        // krishna → കൃഷ്ണ, not ക്രിഷ്ണ.
        if spelling == "r"
            && after.starts_with('i')
            && !after.starts_with("ii")
            && previous.is_some_and(|p| VOCALIC_R_AFTER.contains(&p))
        {
            out.push(VOCALIC_R_SIGN);
            pending = None;
            rest = &after[1..];
            continue;
        }
        match spelling {
            // jishnu → ജിഷ്ണു, karnan → കർണൻ.
            "n" if matches!(previous, Some("sh" | "r")) => letter = "ണ",
            // A single ch between vowels is nearly always doubled: kochi → കൊച്ചി.
            "ch" if pending.is_none() && !out.is_empty() && vowel_at(after).is_some() => {
                letter = "ച്ച"
            }
            _ => {}
        }
        if let Some((p, start)) = pending {
            // r before another consonant is the chillu: arjun → അർജുൻ.
            // Not before y or r, which join it: surya → സുര്യ.
            if p == "r" && !letter.starts_with(['ര', 'യ']) {
                out.replace_range(start.., "ർ");
            } else {
                out.push(VIRAMA);
            }
        }
        let start = out.len();
        out.push_str(letter);
        pending = Some((spelling, start));
        rest = after;
    }
    if let Some((_, start)) = pending {
        match final_form(&out[start..]) {
            Some(chillu) => out.replace_range(start.., chillu),
            None => out.push(VIRAMA),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::transliterate;

    fn t(latin: &str) -> String {
        transliterate(latin).unwrap()
    }

    #[test]
    fn common_words() {
        assert_eq!(t("amma"), "അമ്മ");
        assert_eq!(t("avan"), "അവൻ");
        assert_eq!(t("kutti"), "കുട്ടി");
        assert_eq!(t("ente"), "എന്റെ");
        assert_eq!(t("maram"), "മരം");
        assert_eq!(t("paal"), "പാൽ");
        // Really വീട്; the lexicon has it. `d` is ദ because names want it.
        assert_eq!(t("veedu"), "വീദു");
        assert_eq!(t("vannu"), "വന്നു");
    }

    #[test]
    fn names() {
        assert_eq!(t("Anu"), "അനു");
        assert_eq!(t("Kochi"), "കൊച്ചി");
        assert_eq!(t("Thomas"), "തോമസ്");
    }

    /// What a Malayali expects from common names typed Mozhi-style.
    #[test]
    fn names_as_malayalis_spell_them() {
        assert_eq!(t("jishnu"), "ജിഷ്ണു");
        assert_eq!(t("anu"), "അനു");
        assert_eq!(t("kochi"), "കൊച്ചി");
        assert_eq!(t("thiruvananthapuram"), "തിരുവനന്തപുരം");
        assert_eq!(t("meera"), "മീര");
        assert_eq!(t("krishna"), "കൃഷ്ണ");
        assert_eq!(t("arjun"), "അർജുൻ");
        assert_eq!(t("deepa"), "ദീപ");
        assert_eq!(t("sneha"), "സ്നേഹ");
        assert_eq!(t("suresh"), "സുരേഷ്");
        assert_eq!(t("santhosh"), "സന്തോഷ്");
        assert_eq!(t("mohan"), "മോഹൻ");
        assert_eq!(t("revathy"), "രേവതി");
        assert_eq!(t("priya"), "പ്രിയ");
        assert_eq!(t("lakshmi"), "ലക്ഷ്മി");
        assert_eq!(t("sindhu"), "സിന്ധു");
        assert_eq!(t("panchami"), "പഞ്ചമി");
        assert_eq!(t("karnan"), "കർണൻ");
        assert_eq!(t("radha"), "രധ"); // vowel length: see below
        // Known misses. Simple rules can't tell these apart, and they are left
        // to the lexicon:
        // - untyped vowel length: rahul → രഹുൽ (രാഹുൽ), radha → രധ (രാധ);
        // - ന/ണ and ല/ള: venu → വേനു (വേണു), kannan → കന്നൻ (കണ്ണൻ),
        //   keralam → കേരലം (കേരളം);
        // - ശ vs ഷ (`sh` is ഷ): shankar → ഷങ്കർ (ശങ്കർ);
        // - short e/o before one consonant: koduthu → കോദുതു (കൊടുത്തു),
        //   chechi → ചെച്ചി (ചേച്ചി);
        // - ട vs ദ, and a spoken final u: veedu → വീദു (വീട്);
        // - ശ്ശ and long vowels: thrissur → തൃസ്സുർ (തൃശ്ശൂർ).
        assert_eq!(t("rahul"), "രഹുൽ");
        assert_eq!(t("shankar"), "ഷങ്കർ");
    }

    #[test]
    fn rejects_non_letters() {
        assert_eq!(transliterate(""), None);
        assert_eq!(transliterate("don't"), None);
        assert_eq!(transliterate("abc1"), None);
        assert_eq!(transliterate("എ"), None);
    }

    proptest::proptest! {
        #[test]
        fn letters_always_transliterate(s in "[a-zA-Z]{1,20}") {
            let out = transliterate(&s).unwrap();
            let all_malayalam = out.chars().all(|c| ('\u{0D00}'..='\u{0D7F}').contains(&c));
            proptest::prop_assert!(all_malayalam, "{}", out);
        }
    }
}

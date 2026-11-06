//! Suffix splitting for Manglish. Chat Malayalam glues clitics and case
//! endings onto words (സുഖമാണോ, കഴിച്ചോ, വരുന്നുണ്ടോ), and many such forms
//! never occur in the corpus `manglish.fst` is built from. When a typed word
//! has no whole-form match, it is read as a stem plus one or two suffixes
//! from `SUFFIXES`: the stem is looked up by skeleton like any Manglish word,
//! and the Malayalam stem and suffix are joined by `join`.
//!
//! Only suffixes whose spelling ends the typed word are tried, and at most
//! `MAX_LOOKUPS` stem skeletons are looked up per word.

use crate::lexicon::Lexicon;
use crate::manglish;
use crate::romanisation::Romanisation;

/// Shorter typed words are not split: too many read as some stem plus -o.
pub const MIN_LETTERS: usize = 5;
/// Fewest Latin letters left for the stem.
const MIN_STEM: usize = 3;
/// Most stem skeletons looked up for one typed word.
const MAX_LOOKUPS: usize = 4;
/// Most stems joined per skeleton, most frequent first.
const STEMS_PER_LOOKUP: usize = 3;

/// How a suffix fuses with the end of its stem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    /// A clitic or auxiliary that starts with a vowel (ആണോ, ഉണ്ട്, ഇല്ല, ഓ):
    /// the vowel becomes a vowel sign on the stem's last consonant (see `join`).
    Vowel,
    /// A case ending that starts with a vowel (ഇൽ, ഉടെ, ഓട്). As `Vowel`, but a
    /// stem in ം takes the oblique ത്ത instead (മരം → മരത്തിൽ), which `Oblique`
    /// entries spell; here it doesn't join.
    Case,
    /// Starts with a consonant (ക്ക്, ട്ടെ, ണം): appended as is to a stem that
    /// ends in a vowel, ർ or ൾ (അമ്മ + ക്ക് → അമ്മക്ക്, അവൾ + ക്ക് → അവൾക്ക്).
    Consonant,
    /// The oblique case ending of a stem in ം: ം is replaced (മരം + ത്തിൽ →
    /// മരത്തിൽ). The typed letters (`marathil`) leave the stem's m out, so it
    /// is put back for the lookup.
    Oblique,
}

#[derive(Debug)]
pub struct Suffix {
    /// Ways people type it, matched against the end of the typed word.
    pub latin: &'static [&'static str],
    pub malayalam: &'static str,
    pub join: Join,
    /// May have another suffix after it (വരുന്നു + ഉണ്ട് + ഓ).
    pub inner: bool,
}

const fn suffix(
    latin: &'static [&'static str],
    malayalam: &'static str,
    join: Join,
    inner: bool,
) -> Suffix {
    Suffix {
        latin,
        malayalam,
        join,
        inner,
    }
}

/// The common chat suffixes. Small and explicit: every entry is a guess the
/// engine makes about words it has never seen, so each must be common.
#[rustfmt::skip]
pub const SUFFIXES: &[Suffix] = &[
    // Question, copula, existence, negation.
    suffix(&["o", "oo"], "ഓ", Join::Vowel, false),
    suffix(&["ano", "aano"], "ആണോ", Join::Vowel, false),
    suffix(&["anu", "aanu"], "ആണ്", Join::Vowel, false),
    suffix(&["und", "undu"], "ഉണ്ട്", Join::Vowel, true),
    suffix(&["undo", "undoo"], "ഉണ്ടോ", Join::Vowel, false),
    suffix(&["illa"], "ഇല്ല", Join::Vowel, false),
    suffix(&["ille", "illey"], "ഇല്ലേ", Join::Vowel, false),
    suffix(&["alla"], "അല്ല", Join::Vowel, false),
    suffix(&["alle", "alley"], "അല്ലേ", Join::Vowel, false),
    suffix(&["ayirunnu", "aayirunnu", "ayrunnu"], "ആയിരുന്നു", Join::Vowel, true),
    suffix(&["ennu", "enn"], "എന്ന്", Join::Vowel, false),
    suffix(&["um"], "ഉം", Join::Vowel, false),
    // Case endings.
    suffix(&["il"], "ഇൽ", Join::Case, true),
    suffix(&["ude"], "ഉടെ", Join::Case, false),
    suffix(&["inte", "inde"], "ഇന്റെ", Join::Case, false),
    suffix(&["odu", "ode", "od"], "ഓട്", Join::Case, false),
    suffix(&["thil"], "ത്തിൽ", Join::Oblique, true),
    suffix(&["thinte", "thinde"], "ത്തിന്റെ", Join::Oblique, false),
    suffix(&["thinu"], "ത്തിന്", Join::Oblique, false),
    suffix(&["thodu", "thode"], "ത്തോട്", Join::Oblique, false),
    // Consonant-initial: dative, permissive, obligation.
    suffix(&["kku", "kk"], "ക്ക്", Join::Consonant, false),
    suffix(&["tte"], "ട്ടെ", Join::Consonant, false),
    suffix(&["nam"], "ണം", Join::Consonant, false),
];

const VIRAMA: char = '\u{0D4D}';
const ANUSVARA: char = 'ം';
const SIGN_U: char = '\u{0D41}';

/// The vowel sign an independent vowel takes after a consonant; None for അ,
/// whose vowel is the consonant's own.
fn sign_of(vowel: char) -> Option<Option<char>> {
    Some(match vowel {
        'അ' => None,
        'ആ' => Some('\u{0D3E}'),
        'ഇ' => Some('\u{0D3F}'),
        'ഈ' => Some('\u{0D40}'),
        'ഉ' => Some('\u{0D41}'),
        'ഊ' => Some('\u{0D42}'),
        'എ' => Some('\u{0D46}'),
        'ഏ' => Some('\u{0D47}'),
        'ഐ' => Some('\u{0D48}'),
        'ഒ' => Some('\u{0D4A}'),
        'ഓ' => Some('\u{0D4B}'),
        'ഔ' => Some('\u{0D4C}'),
        _ => return None,
    })
}

/// The consonant a chillu stands for.
fn chillu_base(c: char) -> Option<char> {
    Some(match c {
        'ൻ' => 'ന',
        'ർ' => 'ര',
        'ൽ' => 'ല',
        'ൾ' => 'ള',
        'ൺ' => 'ണ',
        'ൿ' => 'ക',
        _ => return None,
    })
}

fn is_consonant(c: char) -> bool {
    ('\u{0D15}'..='\u{0D3A}').contains(&c)
}

/// The glide between a stem ending in a vowel and a suffix starting with
/// one: വ after u and o sounds (പൂ + ഇൽ → പൂവിൽ), യ after the rest
/// (അമ്മ + ഓട് → അമ്മയോട്, എത്തി + ഓ → എത്തിയോ). None if `last` isn't a vowel.
fn glide(last: char) -> Option<char> {
    match last {
        '\u{0D42}' | '\u{0D4A}' | '\u{0D4B}' | '\u{0D4C}' | '\u{0D57}' | 'ഉ' | 'ഊ' | 'ഒ' | 'ഓ' => {
            Some('വ')
        }
        '\u{0D3E}'..='\u{0D40}' | '\u{0D46}'..='\u{0D48}' | 'അ'..='ഈ' | 'എ'..='ഐ' => {
            Some('യ')
        }
        c if is_consonant(c) => Some('യ'), // its inherent a
        _ => None,
    }
}

/// Joins a Malayalam stem and suffix with the sandhi `how` calls for; None
/// when they don't join (a case ending on a stem in ം, say).
///
/// A vowel-initial suffix: a final ം becomes മ + the vowel sign (സുഖം + ആണോ
/// → സുഖമാണോ); a chillu becomes its consonant + the sign (അവൻ + ഓട് →
/// അവനോട്); a final ് or ു gives way to the sign (കഴിച്ചു + ഓ → കഴിച്ചോ);
/// any other vowel takes a glide (`glide`).
pub fn join(stem: &str, suffix: &str, how: Join) -> Option<String> {
    let last = stem.chars().last()?;
    let body = &stem[..stem.len() - last.len_utf8()];
    match how {
        Join::Consonant => {
            let first = suffix.chars().next()?;
            // ർ and ൾ take it too (അവർക്ക്, അവൾക്ക്); ൻ doesn't (അവന്).
            let joins = is_consonant(last) || glide(last).is_some() || matches!(last, 'ർ' | 'ൾ');
            (is_consonant(first) && joins).then(|| format!("{stem}{suffix}"))
        }
        Join::Oblique => (last == ANUSVARA).then(|| format!("{body}{suffix}")),
        Join::Vowel | Join::Case => {
            let mut chars = suffix.chars();
            let sign = sign_of(chars.next()?)?;
            let rest = chars.as_str();
            let sign: String = sign.into_iter().collect();
            if last == ANUSVARA {
                return (how == Join::Vowel).then(|| format!("{body}മ{sign}{rest}"));
            }
            if let Some(base) = chillu_base(last) {
                return Some(format!("{body}{base}{sign}{rest}"));
            }
            if last == VIRAMA || last == SIGN_U {
                return body
                    .chars()
                    .last()
                    .filter(|&c| is_consonant(c))
                    .map(|_| format!("{body}{sign}{rest}"));
            }
            let glide = glide(last)?;
            Some(format!("{stem}{glide}{sign}{rest}"))
        }
    }
}

/// A reading of the typed word as a stem plus suffixes.
#[derive(Debug, Clone, PartialEq)]
pub struct Split {
    /// The joined Malayalam word.
    pub word: String,
    /// Log probability of the stem.
    pub log_prob: f32,
    /// How many suffixes were joined (1 or 2).
    pub suffixes: usize,
}

/// One way to cut the typed word: a stem spelling and the suffixes after it,
/// innermost first.
struct Cut<'a> {
    stem: String,
    suffixes: Vec<&'a Suffix>,
    /// Latin letters the suffixes take; longer cuts are tried first.
    suffix_len: usize,
}

/// Stem spellings to look up for the Latin letters before a suffix: the
/// letters themselves, plus the stem's m for an oblique ending, or without a
/// typed glide (`ammay` + `odu` → amma) before a vowel.
fn stem_spellings(latin: &str, how: Join) -> Vec<String> {
    let mut out = Vec::with_capacity(2);
    match how {
        Join::Oblique => out.push(format!("{latin}m")),
        Join::Consonant => out.push(latin.to_string()),
        Join::Vowel | Join::Case => {
            out.push(latin.to_string());
            let b = latin.as_bytes();
            if b.len() > MIN_STEM
                && matches!(b[b.len() - 1], b'y' | b'v')
                && matches!(b[b.len() - 2], b'a' | b'e' | b'i' | b'o' | b'u')
            {
                out.push(latin[..latin.len() - 1].to_string());
            }
        }
    }
    out
}

/// The suffixes in `table` that end `latin`, each with the letters before it.
fn endings<'a>(
    latin: &'a str,
    table: &'static [Suffix],
) -> impl Iterator<Item = (&'static Suffix, &'a str, usize)> {
    table.iter().flat_map(move |s| {
        s.latin.iter().filter_map(move |spelling| {
            let stem = latin.strip_suffix(spelling)?;
            (stem.len() >= MIN_STEM).then_some((s, stem, spelling.len()))
        })
    })
}

/// Malayalam readings of `typed` as a stem plus one or two suffixes, for
/// stems at least `min_log_prob` likely. Unordered; may repeat a word.
pub fn splits(r: &Romanisation, lexicon: &Lexicon, typed: &str, min_log_prob: f32) -> Vec<Split> {
    splits_with(r, typed, |key| {
        let mut stems: Vec<(String, f32)> = manglish::same_skeleton(lexicon, key)
            .into_iter()
            .map(|(word, count)| (word, lexicon.log_prob(count)))
            .filter(|(_, lp)| *lp >= min_log_prob)
            .collect();
        stems.sort_by(|a, b| b.1.total_cmp(&a.1));
        stems.truncate(STEMS_PER_LOOKUP);
        stems
    })
}

/// Every way to cut `latin` into a stem and one or two suffixes: one suffix
/// first, and a longer suffix before a shorter one.
fn cuts(latin: &str) -> Vec<Cut<'static>> {
    let mut cuts: Vec<Cut> = Vec::new();
    for (outer, before, len) in endings(latin, SUFFIXES) {
        for stem in stem_spellings(before, outer.join) {
            cuts.push(Cut {
                stem,
                suffixes: vec![outer],
                suffix_len: len,
            });
        }
        for (inner, stem, inner_len) in endings(before, SUFFIXES).filter(|(s, _, _)| s.inner) {
            for stem in stem_spellings(stem, inner.join) {
                cuts.push(Cut {
                    stem,
                    suffixes: vec![inner, outer],
                    suffix_len: len + inner_len,
                });
            }
        }
    }
    cuts.sort_by_key(|c| (c.suffixes.len(), std::cmp::Reverse(c.suffix_len)));
    cuts
}

/// `splits`, with the stems of a skeleton found by `lookup` (called at most
/// `MAX_LOOKUPS` times).
fn splits_with(r: &Romanisation, typed: &str, mut lookup: impl FnMut(&str) -> Vec<(String, f32)>) -> Vec<Split> {
    let latin: String = typed
        .chars()
        .filter(char::is_ascii_alphabetic)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if latin.len() < MIN_LETTERS || latin.len() != typed.len() {
        return Vec::new();
    }
    let cuts = cuts(&latin);
    let mut looked_up: Vec<(String, Vec<(String, f32)>)> = Vec::new();
    let mut out = Vec::new();
    for cut in &cuts {
        let key = r.skeleton(&cut.stem);
        let i = match looked_up.iter().position(|(k, _)| *k == key) {
            Some(i) => i,
            None if looked_up.len() < MAX_LOOKUPS => {
                let stems = lookup(&key);
                looked_up.push((key, stems));
                looked_up.len() - 1
            }
            None => continue,
        };
        for (stem, log_prob) in &looked_up[i].1 {
            let word = cut
                .suffixes
                .iter()
                .try_fold(stem.clone(), |w, s| join(&w, s.malayalam, s.join));
            if let Some(word) = word {
                out.push(Split {
                    word,
                    log_prob: *log_prob,
                    suffixes: cut.suffixes.len(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::romanisation::ml;
    use crate::lexicon::build;

    #[test]
    fn vowel_suffixes_fuse_with_the_stem() {
        let j = |stem, suffix| join(stem, suffix, Join::Vowel);
        // ം → മ + sign
        assert_eq!(j("സുഖം", "ആണോ").as_deref(), Some("സുഖമാണോ"));
        assert_eq!(j("സുഖം", "അല്ല").as_deref(), Some("സുഖമല്ല"));
        assert_eq!(j("വരും", "എന്ന്").as_deref(), Some("വരുമെന്ന്"));
        // ് and ു give way
        assert_eq!(j("കഴിച്ചു", "ഓ").as_deref(), Some("കഴിച്ചോ"));
        assert_eq!(j("വരുന്നുണ്ട്", "ഓ").as_deref(), Some("വരുന്നുണ്ടോ"));
        assert_eq!(j("വരുന്നു", "ഉണ്ട്").as_deref(), Some("വരുന്നുണ്ട്"));
        assert_eq!(j("കഴിച്ചു", "ഇല്ല").as_deref(), Some("കഴിച്ചില്ല"));
        // chillu → consonant + sign
        assert_eq!(j("വീട്ടിൽ", "ആണോ").as_deref(), Some("വീട്ടിലാണോ"));
        assert_eq!(j("ഞാൻ", "ഉം").as_deref(), Some("ഞാനും"));
        assert_eq!(j("അവർ", "ഓ").as_deref(), Some("അവരോ"));
        // glides
        assert_eq!(j("എത്തി", "ഓ").as_deref(), Some("എത്തിയോ"));
        assert_eq!(j("അമ്മ", "ആണോ").as_deref(), Some("അമ്മയാണോ"));
        assert_eq!(j("പൂ", "ഇൽ").as_deref(), Some("പൂവിൽ"));
        assert_eq!(j("", "ഓ"), None);
        assert_eq!(j("്", "ഓ"), None, "a virama with no consonant");
    }

    #[test]
    fn case_endings_skip_stems_in_anusvara() {
        assert_eq!(join("മരം", "ഇൽ", Join::Case), None);
        assert_eq!(
            join("മരം", "ത്തിൽ", Join::Oblique).as_deref(),
            Some("മരത്തിൽ")
        );
        assert_eq!(join("അമ്മ", "ത്തിൽ", Join::Oblique), None);
        assert_eq!(join("അവൻ", "ഓട്", Join::Case).as_deref(), Some("അവനോട്"));
        assert_eq!(join("കുട്ടി", "ഉടെ", Join::Case).as_deref(), Some("കുട്ടിയുടെ"));
    }

    #[test]
    fn consonant_suffixes_need_a_vowel_before_them() {
        let j = |stem, suffix| join(stem, suffix, Join::Consonant);
        assert_eq!(j("അമ്മ", "ക്ക്").as_deref(), Some("അമ്മക്ക്"));
        assert_eq!(j("വര", "ട്ടെ").as_deref(), Some("വരട്ടെ"));
        assert_eq!(j("കഴിക്ക", "ണം").as_deref(), Some("കഴിക്കണം"));
        assert_eq!(j("കുട്ടി", "ക്ക്").as_deref(), Some("കുട്ടിക്ക്"));
        assert_eq!(j("അവൾ", "ക്ക്").as_deref(), Some("അവൾക്ക്"));
        assert_eq!(j("അവൻ", "ക്ക്"), None, "അവന്, not അവൻക്ക്");
        assert_eq!(j("വീട്", "ക്ക്"), None);
        assert_eq!(j("മരം", "ക്ക്"), None);
        assert_eq!(j("വന്നു", "ക്ക്"), None);
    }

    #[test]
    fn every_suffix_starts_the_way_its_join_expects() {
        for s in SUFFIXES {
            let first = s.malayalam.chars().next().unwrap();
            match s.join {
                Join::Vowel | Join::Case => assert!(sign_of(first).is_some(), "{s:?}"),
                Join::Consonant => assert!(is_consonant(first), "{s:?}"),
                Join::Oblique => assert!(s.malayalam.starts_with("ത്ത"), "{s:?}"),
            }
            assert!(
                s.latin
                    .iter()
                    .all(|l| l.bytes().all(|b| b.is_ascii_lowercase()))
            );
        }
    }

    fn lexicon(words: &[(&str, u64)]) -> Lexicon {
        let vocab: Vec<(String, u64)> = words.iter().map(|(w, c)| (w.to_string(), *c)).collect();
        Lexicon::from_bytes(build(manglish::entries(ml(), &vocab)).unwrap()).unwrap()
    }

    fn words(lex: &Lexicon, typed: &str) -> Vec<String> {
        splits(ml(), lex, typed, f32::NEG_INFINITY)
            .into_iter()
            .map(|s| s.word)
            .collect()
    }

    #[test]
    fn splits_typed_words_into_known_stems() {
        let lex = lexicon(&[
            ("സുഖം", 100),
            ("കഴിച്ചു", 100),
            ("വരുന്നു", 100),
            ("അമ്മ", 100),
            ("മരം", 100),
        ]);
        assert!(words(&lex, "sukhamano").contains(&"സുഖമാണോ".to_string()));
        assert!(words(&lex, "kazhicho").contains(&"കഴിച്ചോ".to_string()));
        assert!(words(&lex, "varunnundo").contains(&"വരുന്നുണ്ടോ".to_string()));
        assert!(
            words(&lex, "ammayodu").contains(&"അമ്മയോട്".to_string()),
            "typed glide"
        );
        assert!(
            words(&lex, "marathil").contains(&"മരത്തിൽ".to_string()),
            "oblique"
        );
        let two = splits(ml(), &lex, "varunnundo", f32::NEG_INFINITY);
        assert!(two.iter().any(|s| s.suffixes == 2), "{two:?}");
    }

    #[test]
    fn short_or_non_letter_words_are_not_split() {
        let lex = lexicon(&[("വന്നു", 100)]);
        assert!(words(&lex, "vano").is_empty());
        assert_eq!(words(&lex, "vanno"), ["വന്നോ"]);
        assert!(words(&lex, "vann'o").is_empty());
        assert!(words(&lex, "").is_empty());
    }

    #[test]
    fn rare_stems_are_left_out() {
        let lex = lexicon(&[("വന്നു", 1), ("ഇഷ്ടം", 1_000_000)]);
        assert!(splits(ml(), &lex, "vanno", lex.log_prob(1) + 0.1).is_empty());
    }

    #[test]
    fn lookups_are_bounded() {
        // Many suffix spellings end this word; only a few stems are looked up.
        let typed = "marathilundoo";
        let mut skeletons: Vec<String> = cuts(typed).iter().map(|c| ml().skeleton(&c.stem)).collect();
        skeletons.sort();
        skeletons.dedup();
        assert!(skeletons.len() > MAX_LOOKUPS, "{skeletons:?}");
        let mut calls = 0;
        let _ = splits_with(ml(), typed, |_| {
            calls += 1;
            Vec::new()
        });
        assert_eq!(calls, MAX_LOOKUPS);
    }
}

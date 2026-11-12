//! Suffix splitting for Manglish. Chat Malayalam glues clitics and case
//! endings onto words (സുഖമാണോ, കഴിച്ചോ, വരുന്നുണ്ടോ), and many such forms
//! never occur in the corpus `manglish.fst` is built from. When a typed word
//! has no whole-form match, it is read as a stem plus one or two suffixes
//! from the pack's `suffixes.tsv` (manifest role `suffixes`; the format is in
//! Malayalam's file and `Suffixes::parse`): the stem is looked up by skeleton like
//! any Manglish word, and the native stem and suffix are joined by the script's
//! `ScriptRules::join_suffix`, from its `lang.json`'s `suffixJoins` (`script::data::Joins`).
//!
//! Only suffixes whose spelling ends the typed word are tried, and at most
//! `MAX_LOOKUPS` stem skeletons are looked up per word. A pack without a suffix
//! file is never split.

use std::path::Path;

use crate::lexicon::Lexicon;
use crate::manglish;
use crate::romanisation::Romanisation;
use crate::script::ScriptRules;

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
    /// the vowel becomes a vowel sign on the stem's last consonant
    /// (`ScriptDef::join_suffix`).
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
    /// is put back for the lookup (`StemLetters::oblique`).
    Oblique,
}

/// The Latin letters a typed stem lacks or carries at the join, which its lexicon spelling
/// doesn't: read from the script's sandhi (`lang.json`'s `suffixJoins`) through the pack's
/// romanisation, so each language gets its own (Malayalam's are `m`, and `y` and `v`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StemLetters {
    /// How the nasal ending an oblique suffix replaces is spelled (`nasal`: ം is `m`). The
    /// typed stem leaves it out (`mara` + `thil`), so it is added for the lookup.
    oblique: String,
    /// How the glides are spelled (`glides`, `inherentGlide`: യ is `y`, വ is `v`). A typed
    /// stem may end in one after a vowel (`ammay` + `odu`); it is tried without it too.
    glides: Vec<String>,
}

impl StemLetters {
    /// Empty for a script without joins: nothing is added or dropped.
    pub fn new(rules: &dyn ScriptRules, r: &Romanisation) -> Self {
        let Some(joins) = rules.joins() else {
            return Self::default();
        };
        let oblique = joins
            .nasal
            .as_ref()
            .map(|(ending, _)| r.romanize(ending))
            .unwrap_or_default();
        let mut glides: Vec<String> = Vec::new();
        let letters = joins.glides.iter().map(|(glide, _)| *glide).chain(joins.inherent_glide);
        for latin in letters.filter_map(|glide| r.consonant(glide)) {
            if !latin.is_empty() && !glides.iter().any(|g| g == latin) {
                glides.push(latin.to_string());
            }
        }
        Self { oblique, glides }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suffix {
    /// Ways people type it, matched against the end of the typed word.
    pub latin: Vec<String>,
    /// The suffix in the pack's script.
    pub native: String,
    pub join: Join,
    /// May have another suffix after it (വരുന്നു + ഉണ്ട് + ഓ).
    pub inner: bool,
}

/// A pack's suffix table, in file order (the order breaks ties between cuts).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Suffixes {
    list: Vec<Suffix>,
}

impl Suffixes {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// `suffixes.tsv`: per line `latin<TAB>native<TAB>join[<TAB>inner]`, where `latin` is
    /// comma-separated lower-case spellings and `join` one of `vowel`, `case`, `consonant`,
    /// `oblique`. Lines starting with `#` and blank lines are skipped; anything else malformed
    /// is an error, so a typo can't silently drop a suffix.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut list = Vec::new();
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let bad = |why: &str| format!("line {}: {why}: {line:?}", i + 1);
            let fields: Vec<&str> = line.split('\t').collect();
            let (latin, native, join, inner) = match fields.as_slice() {
                [latin, native, join] => (latin, native, join, false),
                [latin, native, join, "inner"] => (latin, native, join, true),
                _ => return Err(bad("want latin, native, join and an optional \"inner\"")),
            };
            let latin: Vec<String> = latin.split(',').map(str::to_string).collect();
            if latin
                .iter()
                .any(|l| l.is_empty() || !l.bytes().all(|b| b.is_ascii_lowercase()))
            {
                return Err(bad("a spelling is not lower-case a–z"));
            }
            if native.is_empty() {
                return Err(bad("no native suffix"));
            }
            let join = match *join {
                "vowel" => Join::Vowel,
                "case" => Join::Case,
                "consonant" => Join::Consonant,
                "oblique" => Join::Oblique,
                _ => return Err(bad("join is vowel, case, consonant or oblique")),
            };
            list.push(Suffix {
                latin,
                native: native.to_string(),
                join,
                inner,
            });
        }
        Ok(Self { list })
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Suffix> {
        self.list.iter()
    }
}

/// Malayalam's table from `packs/ml.neopack`, for the engine's unit tests.
#[cfg(test)]
pub(crate) fn ml() -> &'static Suffixes {
    static ML: std::sync::OnceLock<Suffixes> = std::sync::OnceLock::new();
    ML.get_or_init(load_ml)
}

/// An owned copy of `ml()`, for a test's `Sources`.
#[cfg(test)]
pub(crate) fn load_ml() -> Suffixes {
    Suffixes::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/ml.neopack/suffixes.tsv"),
    )
    .unwrap()
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
/// letters themselves, plus the stem's nasal for an oblique ending (Malayalam's
/// `m`), or without a typed glide (`ammay` + `odu` → amma) before a vowel.
fn stem_spellings(latin: &str, how: Join, letters: &StemLetters) -> Vec<String> {
    let mut out = Vec::with_capacity(2);
    match how {
        Join::Oblique => out.push(format!("{latin}{}", letters.oblique)),
        Join::Consonant => out.push(latin.to_string()),
        Join::Vowel | Join::Case => {
            out.push(latin.to_string());
            for glide in &letters.glides {
                if let Some(stem) = latin.strip_suffix(glide.as_str())
                    && latin.len() > MIN_STEM
                    && stem.as_bytes().last().is_some_and(|b| b"aeiou".contains(b))
                {
                    out.push(stem.to_string());
                }
            }
        }
    }
    out
}

/// The suffixes in `table` that end `latin`, each with the letters before it.
fn endings<'a, 't>(
    latin: &'a str,
    table: &'t Suffixes,
) -> impl Iterator<Item = (&'t Suffix, &'a str, usize)> {
    table.iter().flat_map(move |s| {
        s.latin.iter().filter_map(move |spelling| {
            let stem = latin.strip_suffix(spelling)?;
            (stem.len() >= MIN_STEM).then_some((s, stem, spelling.len()))
        })
    })
}

/// Readings of `typed` as a native stem from `lexicon` plus one or two of
/// `suffixes`, joined by `rules`, for stems at least `min_log_prob` likely.
/// Unordered; may repeat a word.
pub fn splits(
    rules: &dyn ScriptRules,
    r: &Romanisation,
    suffixes: &Suffixes,
    lexicon: &Lexicon,
    typed: &str,
    min_log_prob: f32,
) -> Vec<Split> {
    let packing = rules.packing();
    splits_with(rules, r, suffixes, typed, |key| {
        let mut stems: Vec<(String, f32)> = manglish::same_skeleton(lexicon, key, packing)
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
fn cuts<'t>(latin: &str, suffixes: &'t Suffixes, letters: &StemLetters) -> Vec<Cut<'t>> {
    let mut cuts: Vec<Cut> = Vec::new();
    for (outer, before, len) in endings(latin, suffixes) {
        for stem in stem_spellings(before, outer.join, letters) {
            cuts.push(Cut {
                stem,
                suffixes: vec![outer],
                suffix_len: len,
            });
        }
        for (inner, stem, inner_len) in endings(before, suffixes).filter(|(s, _, _)| s.inner) {
            for stem in stem_spellings(stem, inner.join, letters) {
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
fn splits_with(
    rules: &dyn ScriptRules,
    r: &Romanisation,
    suffixes: &Suffixes,
    typed: &str,
    mut lookup: impl FnMut(&str) -> Vec<(String, f32)>,
) -> Vec<Split> {
    let latin: String = typed
        .chars()
        .filter(char::is_ascii_alphabetic)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if latin.len() < MIN_LETTERS || latin.len() != typed.len() {
        return Vec::new();
    }
    let cuts = cuts(&latin, suffixes, &StemLetters::new(rules, r));
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
            let word = cut.suffixes.iter().try_fold(stem.clone(), |w, s| {
                rules.join_suffix(&w, &s.native, s.join)
            });
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
    use crate::lexicon::build;
    use crate::romanisation;
    use crate::script::rules;

    fn mlym() -> &'static dyn ScriptRules {
        rules("mlym").unwrap()
    }

    /// Malayalam's joins, from its `lang.json`: the stem's ending decides.
    ///
    /// A vowel-initial suffix: a final ം becomes മ + the vowel sign (സുഖം + ആണോ → സുഖമാണോ); a
    /// chillu becomes its consonant + the sign (അവൻ + ഓട് → അവനോട്); a final ് or ു gives way
    /// to the sign (കഴിച്ചു + ഓ → കഴിച്ചോ); any other vowel takes a glide.
    fn join(stem: &str, suffix: &str, how: Join) -> Option<String> {
        mlym().join_suffix(stem, suffix, how)
    }

    #[test]
    fn malformed_lines_are_errors() {
        let ok = Suffixes::parse("# c\n\no,oo\tഓ\tvowel\nund\tഉണ്ട്\tvowel\tinner\n").unwrap();
        let list: Vec<&Suffix> = ok.iter().collect();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].latin, ["o", "oo"]);
        assert!(!list[0].inner && list[1].inner);
        for bad in [
            "o\tഓ\n",
            "o\tഓ\tvowels\n",
            "O\tഓ\tvowel\n",
            "o,\tഓ\tvowel\n",
            "o\t\tvowel\n",
            "o\tഓ\tvowel\touter\n",
        ] {
            assert!(Suffixes::parse(bad).is_err(), "{bad:?}");
        }
    }

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
        for s in ml().iter() {
            let first = s.native.chars().next().unwrap();
            match s.join {
                Join::Vowel | Join::Case => {
                    assert!(join("അമ്മ", &s.native, s.join).is_some(), "{s:?}")
                }
                Join::Consonant => {
                    assert!(('\u{0D15}'..='\u{0D3A}').contains(&first), "{s:?}");
                    assert!(join("അമ്മ", &s.native, s.join).is_some(), "{s:?}")
                }
                Join::Oblique => assert!(s.native.starts_with("ത്ത"), "{s:?}"),
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
        Lexicon::from_bytes(build(manglish::entries(romanisation::ml(), &vocab)).unwrap()).unwrap()
    }

    fn words(lex: &Lexicon, typed: &str) -> Vec<String> {
        splits(
            mlym(),
            romanisation::ml(),
            ml(),
            lex,
            typed,
            f32::NEG_INFINITY,
        )
        .into_iter()
        .map(|s| s.word)
        .collect()
    }

    /// What `stem_spellings` had as code for Malayalam: its joins and tables say the same.
    #[test]
    fn malayalams_stem_letters_are_the_ones_that_were_code() {
        let letters = StemLetters::new(mlym(), romanisation::ml());
        assert_eq!(letters.oblique, "m");
        let mut glides = letters.glides.clone();
        glides.sort();
        assert_eq!(glides, ["v", "y"]);
        let spellings = |latin, how| stem_spellings(latin, how, &letters);
        assert_eq!(spellings("mara", Join::Oblique), ["maram"]);
        assert_eq!(spellings("amma", Join::Consonant), ["amma"]);
        assert_eq!(spellings("ammay", Join::Case), ["ammay", "amma"]);
        assert_eq!(spellings("poov", Join::Vowel), ["poov", "poo"]);
        assert_eq!(spellings("avan", Join::Vowel), ["avan"]);
        assert_eq!(spellings("pay", Join::Vowel), ["pay"], "too short to lose a letter");
        assert_eq!(spellings("mazhy", Join::Vowel), ["mazhy"], "a glide follows a vowel");
    }

    /// Another language's suffixes get its own letters, or none.
    #[test]
    fn stem_letters_are_each_languages_own() {
        use crate::script::data::{DataScript, ScriptDef};
        let path = |id: &str| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../packs/{id}.neopack/romanisation.json"))
        };
        // Hindi has no joins: nothing is added or dropped, whatever the suffix file says.
        let hi = romanisation::Romanisation::load(&path("hi")).unwrap();
        let none = StemLetters::new(rules("deva").unwrap(), &hi);
        assert_eq!(none, StemLetters::default());
        assert_eq!(stem_spellings("ghar", Join::Oblique, &none), ["ghar"]);
        assert_eq!(stem_spellings("bhay", Join::Vowel, &none), ["bhay"]);
        // Tamil's ம் is `m`, its glides y and v; a Tamil suffix file splits by them.
        let ta = romanisation::Romanisation::load(&path("ta")).unwrap();
        let taml = DataScript::intern(ScriptDef::from_json(crate::script::data::TAMIL).unwrap());
        let letters = StemLetters::new(taml, &ta);
        assert_eq!(letters.oblique, "m");
        assert_eq!(letters.glides, ["v", "y"]);
        let suffixes = Suffixes::parse("il\tஇல்\tcase\nthil,athil\tத்தில்\toblique\na\tஆ\tvowel\n").unwrap();
        let stems = |key: &str| -> Vec<(String, f32)> {
            ["காலம்", "பள்ளி"]
                .iter()
                .filter(|w| ta.word_skeleton(w) == key)
                .map(|w| (w.to_string(), -5.0))
                .collect()
        };
        let words = |typed: &str| -> Vec<String> {
            let mut words: Vec<String> =
                splits_with(taml, &ta, &suffixes, typed, stems).into_iter().map(|s| s.word).collect();
            words.dedup(); // a stem typed with and without its glide is one skeleton here
            words
        };
        assert_eq!(words("kaalathil"), ["காலத்தில்"]);
        assert_eq!(words("palliyil"), ["பள்ளியில்"]);
        assert_eq!(words("kaalama"), ["காலமா"]);
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
        let two = splits(
            mlym(),
            romanisation::ml(),
            ml(),
            &lex,
            "varunnundo",
            f32::NEG_INFINITY,
        );
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
        assert!(
            splits(
                mlym(),
                romanisation::ml(),
                ml(),
                &lex,
                "vanno",
                lex.log_prob(1) + 0.1
            )
            .is_empty()
        );
    }

    #[test]
    fn lookups_are_bounded() {
        // Many suffix spellings end this word; only a few stems are looked up.
        let typed = "marathilundoo";
        let letters = StemLetters::new(mlym(), romanisation::ml());
        let mut skeletons: Vec<String> = cuts(typed, ml(), &letters)
            .iter()
            .map(|c| romanisation::ml().skeleton(&c.stem))
            .collect();
        skeletons.sort();
        skeletons.dedup();
        assert!(skeletons.len() > MAX_LOOKUPS, "{skeletons:?}");
        let mut calls = 0;
        let _ = splits_with(mlym(), romanisation::ml(), ml(), typed, |_| {
            calls += 1;
            Vec::new()
        });
        assert_eq!(calls, MAX_LOOKUPS);
    }
}

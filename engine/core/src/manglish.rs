//! Manglish lookup: `manglish.fst` maps `skeleton\0packed-word` → count.
//! The Malayalam word is packed one byte per character (see `pack`), which
//! keeps the file about a third of the size UTF-8 keys would need.
//! A typed Latin word is collapsed to its skeleton, then every Malayalam word
//! whose skeleton equals it or starts with it is a candidate, re-ranked by how
//! closely the typed letters match that word's full romanisation.
//!
//! Both lookups are exact and bounded: `Lexicon::top_completions` reads the
//! precomputed top keys of a large subtree, or scans a small one whole.

use crate::lexicon::{Lexicon, SEPARATOR, TOP_K};
use crate::romanisation::Romanisation;

/// Candidates kept before the finer (slower) re-ranking by edit distance.
const SHORTLIST: usize = 16;

const ZWNJ: char = '\u{200C}';
const ZWJ: char = '\u{200D}';

/// The first code point of the script block `r`'s words are packed from (`pack`): its
/// script's (U+0D00 for Malayalam, U+0900 for Devanagari). None when no script rules are in
/// use for the tables' script: its words can't be packed or unpacked, so nothing is keyed or
/// found. A pack never gets here so: the engine refuses one whose tables are for another
/// script than its own (`Sources::open`).
pub fn block_base(r: &Romanisation) -> Option<char> {
    crate::script::rules(r.script()).map(|rules| *rules.block().start())
}

/// One byte per character: the 128 code points of the script block from `base` become
/// 0x80–0xFF, ZWNJ 0x01, ZWJ 0x02, and the apostrophe 0x27 (Assamese হ'ল; never in a
/// Malayalam word). None if the word has any other character.
pub fn pack(word: &str, base: char) -> Option<Vec<u8>> {
    word.chars()
        .map(|c| match c {
            ZWNJ => Some(0x01),
            ZWJ => Some(0x02),
            '\'' => Some(b'\''),
            _ => {
                let offset = (c as u32).checked_sub(base as u32)?;
                (offset < 0x80).then(|| 0x80 + offset as u8)
            }
        })
        .collect()
}

/// The inverse of `pack`; None for bytes `pack` never writes.
pub fn unpack(bytes: &[u8], base: char) -> Option<String> {
    bytes
        .iter()
        .map(|&b| match b {
            0x01 => Some(ZWNJ),
            0x02 => Some(ZWJ),
            b'\'' => Some('\''),
            0x80..=0xFF => char::from_u32(base as u32 + u32::from(b - 0x80)),
            _ => None,
        })
        .collect()
}

/// The romanised-lexicon key for a native word; None if it has no skeleton or can't be packed.
pub fn key(r: &Romanisation, word: &str) -> Option<Vec<u8>> {
    let mut key = r.word_skeleton(word).into_bytes();
    if key.is_empty() {
        return None;
    }
    key.push(SEPARATOR);
    key.extend(pack(word, block_base(r)?)?);
    Some(key)
}

/// `manglish.fst` entries for a Malayalam vocabulary.
pub fn entries(r: &Romanisation, malayalam: &[(String, u64)]) -> Vec<(Vec<u8>, u64)> {
    malayalam
        .iter()
        .filter_map(|(word, count)| Some((key(r, word)?, *count)))
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub word: String,
    /// Log probability of the Malayalam word.
    pub log_prob: f32,
    /// Skeleton letters the word has beyond what was typed (0 = same skeleton).
    pub missing: usize,
    /// Edit distance between the typed letters and the word's romanisation
    /// (to its closest prefix, when the word is longer).
    pub distance: usize,
    /// The typed letters spell the whole word, up to what the pack's `looseSpelling` calls
    /// the same (Malayalam: vowel length and t/d, `nale` for നാളെ, romanised `naale`).
    /// `distance` is then 0.
    pub spelled: bool,
}

/// Malayalam words matching a typed Latin word: those with exactly its
/// skeleton first, then longer ones, each group by frequency.
pub fn lookup(r: &Romanisation, lexicon: &Lexicon, typed: &str) -> Vec<Match> {
    let typed_skeleton = r.skeleton(typed);
    let Some(base) = block_base(r).filter(|_| !typed_skeleton.is_empty()) else {
        return Vec::new();
    };
    let mut exact_prefix = typed_skeleton.clone().into_bytes();
    exact_prefix.push(SEPARATOR);
    // Words with exactly this skeleton, then the most frequent longer ones.
    // The second list can repeat words of the first; `seen` drops them.
    let mut keys = lexicon.top_completions(&exact_prefix, TOP_K);
    keys.extend(lexicon.top_completions(typed_skeleton.as_bytes(), TOP_K));

    let mut shortlist: Vec<(f32, Match)> = Vec::with_capacity(keys.len());
    let mut seen = std::collections::HashSet::new();
    for (key, count) in keys {
        if !seen.insert(key.clone()) {
            continue;
        }
        let Some(sep) = key.iter().position(|&b| b == SEPARATOR) else {
            continue;
        };
        let Some(word) = unpack(&key[sep + 1..], base) else {
            continue;
        };
        let missing = sep.saturating_sub(typed_skeleton.len());
        let log_prob = lexicon.log_prob(count);
        // Exact skeletons first, then frequency.
        let order = log_prob - missing as f32;
        shortlist.push((
            order,
            Match {
                word,
                log_prob,
                missing,
                distance: 0,
                spelled: false,
            },
        ));
    }
    shortlist.sort_by(|a, b| b.0.total_cmp(&a.0));
    shortlist.truncate(SHORTLIST);
    shortlist
        .into_iter()
        .map(|(_, mut m)| {
            (m.spelled, m.distance) = fit(r, typed, &m.word, m.missing > 0);
            m
        })
        .collect()
}

/// Native words whose skeleton is exactly `skeleton`, most frequent first, with their counts,
/// unpacked from the script block at `base` (`block_base`). One bounded lookup.
pub fn same_skeleton(lexicon: &Lexicon, skeleton: &str, base: char) -> Vec<(String, u64)> {
    if skeleton.is_empty() {
        return Vec::new();
    }
    let mut prefix = skeleton.as_bytes().to_vec();
    prefix.push(SEPARATOR);
    lexicon
        .top_completions(&prefix, TOP_K)
        .into_iter()
        .filter_map(|(key, count)| Some((unpack(&key[prefix.len()..], base)?, count)))
        .collect()
}

/// How closely the typed letters match a Malayalam word: whether they spell
/// it out (`Match::spelled`), and otherwise the edit distance to its
/// romanisation (`Match::distance`), to its closest prefix when `prefix` is set.
pub fn fit(r: &Romanisation, typed: &str, word: &str, prefix: bool) -> (bool, usize) {
    fit_latin(r, typed, &r.romanize(word), prefix)
}

/// `fit` against Latin letters that stand for a word: `roman`, its romanisation, or the
/// letters the user picked it for (`own::OwnWords`). Which spellings count as the same is the
/// pack's (`Romanisation::relaxed`, its `looseSpelling`).
pub fn fit_latin(r: &Romanisation, typed: &str, roman: &str, prefix: bool) -> (bool, usize) {
    let typed_lower: Vec<u8> = typed
        .bytes()
        .filter(u8::is_ascii_alphabetic)
        .map(|b| b.to_ascii_lowercase())
        .collect();
    if !prefix && r.relaxed(&typed_lower) == r.relaxed(roman.as_bytes()) {
        (true, 0)
    } else {
        (
            false,
            prefix_distance(&typed_lower, roman.as_bytes(), prefix),
        )
    }
}

/// Levenshtein distance from `typed` to `target`, or to the closest prefix of
/// `target` when `prefix` is set (the user hasn't finished typing).
fn prefix_distance(typed: &[u8], target: &[u8], prefix: bool) -> usize {
    let mut row: Vec<usize> = (0..=target.len()).collect();
    for (i, &t) in typed.iter().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for j in 1..=target.len() {
            let above = row[j];
            row[j] = (above + 1)
                .min(row[j - 1] + 1)
                .min(diagonal + usize::from(t != target[j - 1]));
            diagonal = above;
        }
    }
    if prefix {
        row.iter().copied().min().unwrap_or(0)
    } else {
        row[target.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::romanisation::ml;
    use crate::lexicon::build;

    fn lexicon(words: &[(&str, u64)]) -> Lexicon {
        let vocab: Vec<(String, u64)> = words.iter().map(|(w, c)| (w.to_string(), *c)).collect();
        Lexicon::from_bytes(build(entries(ml(), &vocab)).unwrap()).unwrap()
    }

    #[test]
    fn keys_are_skeleton_then_packed_word() {
        assert_eq!(key(ml(), "എന്റെ"), Some(b"ent\0\x8E\xA8\xCD\xB1\xC6".to_vec()));
        assert_eq!(key(ml(), "\u{0D4D}"), None, "a lone virama has no skeleton");
    }

    #[test]
    fn tables_for_a_script_without_rules_key_and_find_nothing() {
        // Not Malayalam's block by default: a word packed from the wrong base is another word.
        let json = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/ml.neopack/romanisation.json"),
        )
        .unwrap();
        let other = Romanisation::parse(&json.replace("\"script\": \"mlym\"", "\"script\": \"qzzq\"")).unwrap();
        assert_eq!(block_base(ml()), Some('\u{0D00}'));
        assert_eq!(block_base(&other), None);
        assert_eq!(key(&other, "എന്റെ"), None);
        let lex = lexicon(&[("എന്റെ", 100)]);
        assert_eq!(lookup(ml(), &lex, "ente").len(), 1);
        assert!(lookup(&other, &lex, "ente").is_empty());
    }

    #[test]
    fn packing_round_trips_the_script_only() {
        const ML: char = '\u{0D00}';
        for word in ["എന്റെ", "അവൻ", "ന\u{0D4D}\u{200D}", "ക\u{200C}ഷ"] {
            assert_eq!(unpack(&pack(word, ML).unwrap(), ML).as_deref(), Some(word));
        }
        assert_eq!(pack("abc", ML), None);
        assert_eq!(unpack(b"abc", ML), None);
        assert_eq!(pack("क्या", ML), None, "another script's letters don't pack");
        const DEVA: char = '\u{0900}';
        assert_eq!(pack("है", DEVA), Some(b"\xB9\xC8".to_vec()));
        assert_eq!(unpack(b"\xB9\xC8", DEVA).as_deref(), Some("है"));
        const BENG: char = '\u{0980}';
        let packed = pack("হ'ল", BENG).unwrap();
        assert_eq!(packed[1], b'\'');
        assert_eq!(unpack(&packed, BENG).as_deref(), Some("হ'ল"));
    }

    #[test]
    fn finds_words_by_skeleton() {
        let lex = lexicon(&[("എന്റെ", 100), ("എന്ത്", 80), ("അവൻ", 50)]);
        let found = lookup(ml(), &lex, "ende");
        let words: Vec<&str> = found.iter().map(|m| m.word.as_str()).collect();
        assert_eq!(words, ["എന്റെ", "എന്ത്"]);
        assert_eq!(found[0].missing, 0);
        assert!(found[0].spelled, "ende spells ente, up to t/d");
        assert_eq!(found[0].distance, 0);
        assert_eq!(found[1].distance, 3, "ende vs enthu");
    }

    #[test]
    fn finds_longer_words_from_a_prefix() {
        let lex = lexicon(&[("മലയാളം", 100)]);
        let found = lookup(ml(), &lex, "malay");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].word, "മലയാളം");
        assert!(found[0].missing > 0);
        assert_eq!(found[0].distance, 0, "malay is a prefix of malayaalam");
    }

    #[test]
    fn exact_skeletons_are_found_behind_many_longer_words() {
        // More words under the skeleton than a scan reads: the exact word
        // still comes back, because it is looked up under `skeleton\0`.
        let mut words: Vec<(String, u64)> = (0..2_000)
            .map(|i| {
                // എ + ന്റ + a run of distinct Malayalam letters: skeleton "ent…".
                let tail: String = format!("{i:04}")
                    .chars()
                    .map(|d| char::from_u32(0x0D15 + d.to_digit(10).unwrap()).unwrap())
                    .collect();
                (format!("എന്റ{tail}"), 1_000)
            })
            .collect();
        words.push(("എന്റെ".to_string(), 1));
        let lex = Lexicon::from_bytes(build(entries(ml(), &words)).unwrap()).unwrap();
        let found = lookup(ml(), &lex, "ente");
        assert!(
            found.iter().any(|m| m.word == "എന്റെ" && m.missing == 0),
            "{found:?}"
        );
    }

    #[test]
    fn nothing_for_empty_skeletons() {
        let lex = lexicon(&[("എന്റെ", 100)]);
        assert!(lookup(ml(), &lex, "").is_empty());
        assert!(lookup(ml(), &lex, "123").is_empty());
    }

    #[test]
    fn spelled_out_words_ignore_vowel_length() {
        let lex = lexicon(&[("നാളെ", 10), ("നാല്", 100)]);
        let found = lookup(ml(), &lex, "nale");
        let naale = found.iter().find(|m| m.word == "നാളെ").unwrap();
        assert!(naale.spelled && naale.distance == 0, "{naale:?}");
        let naalu = found.iter().find(|m| m.word == "നാല്").unwrap();
        assert!(!naalu.spelled && naalu.distance > 0, "{naalu:?}");
        let relaxed = |latin: &[u8]| ml().relaxed(latin);
        assert_eq!(relaxed(b"njaan"), relaxed(b"njan"));
        assert_eq!(relaxed(b"veedu"), relaxed(b"vidu"));
        assert_ne!(relaxed(b"te"), relaxed(b"teh"));
        assert_ne!(relaxed(b"veedu"), relaxed(b"vittu"));
    }

    #[test]
    fn which_spellings_are_the_same_is_the_packs() {
        use crate::romanisation::with_loose_spelling;
        // Malayalam types ട as t or d; a pack that keeps them apart says so.
        assert_eq!(fit_latin(ml(), "veedu", "veetu", false), (true, 0));
        let strict = with_loose_spelling(Some("{}"));
        assert_eq!(fit_latin(&strict, "veedu", "veetu", false), (false, 1));
        assert_eq!(fit_latin(&strict, "veetu", "veetu", false), (true, 0));
        let vowels = with_loose_spelling(Some(r#"{"pairs": [["ee", "i"]], "collapse": "a"}"#));
        assert_eq!(fit_latin(&vowels, "vitu", "veetu", false), (true, 0));
        assert_eq!(fit_latin(&vowels, "naale", "nale", false), (true, 0));
        assert!(!fit_latin(&vowels, "veedu", "veetu", false).0, "d is not t here");
        assert!(!fit_latin(&vowels, "pu", "poo", false).0, "oo is not u here");
        // A file from before the field is read as it always was.
        let old = with_loose_spelling(None);
        for (typed, roman) in [
            ("veedu", "veetu"),
            ("nale", "naale"),
            ("pu", "poo"),
            ("vittu", "veedu"),
            ("iee", "ii"),
        ] {
            assert_eq!(fit_latin(&old, typed, roman, false), fit_latin(ml(), typed, roman, false));
        }
    }

    #[test]
    fn distances() {
        assert_eq!(prefix_distance(b"ente", b"ente", false), 0);
        assert_eq!(prefix_distance(b"entey", b"ente", false), 1);
        assert_eq!(prefix_distance(b"mala", b"malayaalam", true), 0);
        assert_eq!(prefix_distance(b"mala", b"malayaalam", false), 6);
        assert_eq!(prefix_distance(b"", b"abc", false), 3);
    }
}

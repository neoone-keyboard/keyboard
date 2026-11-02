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
use crate::romanize::{romanize, skeleton};

/// Candidates kept before the finer (slower) re-ranking by edit distance.
const SHORTLIST: usize = 16;

const ZWNJ: char = '\u{200C}';
const ZWJ: char = '\u{200D}';

/// One byte per character: U+0D00–U+0D7F become 0x80–0xFF, ZWNJ 0x01, ZWJ 0x02.
/// None if the word has any other character.
pub fn pack(word: &str) -> Option<Vec<u8>> {
    word.chars()
        .map(|c| match c {
            ZWNJ => Some(0x01),
            ZWJ => Some(0x02),
            '\u{0D00}'..='\u{0D7F}' => Some(0x80 + (c as u32 - 0x0D00) as u8),
            _ => None,
        })
        .collect()
}

/// The inverse of `pack`; None for bytes `pack` never writes.
pub fn unpack(bytes: &[u8]) -> Option<String> {
    bytes
        .iter()
        .map(|&b| match b {
            0x01 => Some(ZWNJ),
            0x02 => Some(ZWJ),
            0x80..=0xFF => char::from_u32(0x0D00 + u32::from(b - 0x80)),
            _ => None,
        })
        .collect()
}

/// The `manglish.fst` key for a Malayalam word; None if it has no skeleton or can't be packed.
pub fn key(word: &str) -> Option<Vec<u8>> {
    let mut key = skeleton(&romanize(word)).into_bytes();
    if key.is_empty() {
        return None;
    }
    key.push(SEPARATOR);
    key.extend(pack(word)?);
    Some(key)
}

/// `manglish.fst` entries for a Malayalam vocabulary.
pub fn entries(malayalam: &[(String, u64)]) -> Vec<(Vec<u8>, u64)> {
    malayalam
        .iter()
        .filter_map(|(word, count)| Some((key(word)?, *count)))
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
}

/// Malayalam words matching a typed Latin word: those with exactly its
/// skeleton first, then longer ones, each group by frequency.
pub fn lookup(lexicon: &Lexicon, typed: &str) -> Vec<Match> {
    let typed_skeleton = skeleton(typed);
    if typed_skeleton.is_empty() {
        return Vec::new();
    }
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
        let Some(word) = unpack(&key[sep + 1..]) else {
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
            },
        ));
    }
    shortlist.sort_by(|a, b| b.0.total_cmp(&a.0));
    shortlist.truncate(SHORTLIST);
    let typed_lower: Vec<u8> = typed
        .bytes()
        .filter(u8::is_ascii_alphabetic)
        .map(|b| b.to_ascii_lowercase())
        .collect();
    shortlist
        .into_iter()
        .map(|(_, mut m)| {
            m.distance = prefix_distance(&typed_lower, romanize(&m.word).as_bytes(), m.missing > 0);
            m
        })
        .collect()
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
    use crate::lexicon::build;

    fn lexicon(words: &[(&str, u64)]) -> Lexicon {
        let vocab: Vec<(String, u64)> = words.iter().map(|(w, c)| (w.to_string(), *c)).collect();
        Lexicon::from_bytes(build(entries(&vocab)).unwrap()).unwrap()
    }

    #[test]
    fn keys_are_skeleton_then_packed_word() {
        assert_eq!(key("എന്റെ"), Some(b"ent\0\x8E\xA8\xCD\xB1\xC6".to_vec()));
        assert_eq!(key("\u{0D4D}"), None, "a lone virama has no skeleton");
    }

    #[test]
    fn packing_round_trips_malayalam_only() {
        for word in ["എന്റെ", "അവൻ", "ന\u{0D4D}\u{200D}", "ക\u{200C}ഷ"] {
            assert_eq!(unpack(&pack(word).unwrap()).as_deref(), Some(word));
        }
        assert_eq!(pack("abc"), None);
        assert_eq!(unpack(b"abc"), None);
    }

    #[test]
    fn finds_words_by_skeleton() {
        let lex = lexicon(&[("എന്റെ", 100), ("എന്ത്", 80), ("അവൻ", 50)]);
        let found = lookup(&lex, "ende");
        let words: Vec<&str> = found.iter().map(|m| m.word.as_str()).collect();
        assert_eq!(words, ["എന്റെ", "എന്ത്"]);
        assert_eq!(found[0].missing, 0);
        assert_eq!(found[0].distance, 1, "ende vs ente");
        assert_eq!(found[1].distance, 3, "ende vs enthu");
    }

    #[test]
    fn finds_longer_words_from_a_prefix() {
        let lex = lexicon(&[("മലയാളം", 100)]);
        let found = lookup(&lex, "malay");
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
        let lex = Lexicon::from_bytes(build(entries(&words)).unwrap()).unwrap();
        let found = lookup(&lex, "ente");
        assert!(
            found.iter().any(|m| m.word == "എന്റെ" && m.missing == 0),
            "{found:?}"
        );
    }

    #[test]
    fn nothing_for_empty_skeletons() {
        let lex = lexicon(&[("എന്റെ", 100)]);
        assert!(lookup(&lex, "").is_empty());
        assert!(lookup(&lex, "123").is_empty());
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

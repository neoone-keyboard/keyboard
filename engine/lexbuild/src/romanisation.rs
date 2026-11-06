//! A pack's `romanisation.json` (format 1): native script → Latin (the walk), the skeleton
//! people's spellings collapse to, and the romanised-lexicon keys built from both.
//!
//! The format, its walk and its skeleton are specified in
//! `docs/design/2026-11-06-m6-romanisation-format.md`; the phase-2 engine computes
//! the same skeleton of what was typed, so a typed word and a stored key meet.
//! `training/mlkb/data/romanisation.py` implements the same; `tests/romanisation.rs` checks the
//! two agree on a fixture written by the Python.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::{fs, io};

use mlengine_core::lexicon::SEPARATOR;
use serde::Deserialize;
use unicode_normalization::UnicodeNormalization;

const ZWNJ: char = '\u{200C}';
const ZWJ: char = '\u{200D}';

fn nfc(s: &str) -> String {
    s.nfc().collect()
}

/// What lexbuild needs to know about a script that `romanisation.json` doesn't say: its
/// 128-code-point block (the base of `pack`) and the chars in it that are not part of words
/// (dandas, digits, number signs).
struct Script {
    base: char,
    not_letters: &'static [(char, char)],
}

fn script(code: &str) -> Option<Script> {
    Some(match code {
        "deva" => Script {
            base: '\u{0900}',
            // danda, double danda, digits, abbreviation sign
            not_letters: &[('\u{0964}', '\u{0970}')],
        },
        "beng" => Script {
            base: '\u{0980}',
            // digits, currency and fraction signs, isshar
            not_letters: &[('\u{09E6}', '\u{09FB}'), ('\u{09FD}', '\u{09FD}')],
        },
        "mlym" => Script {
            base: '\u{0D00}',
            // digits, fractions, number signs, date mark
            not_letters: &[('\u{0D66}', '\u{0D79}')],
        },
        _ => return None,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Raw {
    format: u32,
    script: String,
    virama: String,
    inherent: String,
    at_word_end: AtWordEnd,
    independent_vowels: HashMap<String, String>,
    vowel_signs: HashMap<String, String>,
    consonants: HashMap<String, String>,
    finals: HashMap<String, String>,
    clusters: HashMap<String, String>,
    skeleton: SkeletonRules,
}

#[derive(Deserialize)]
struct AtWordEnd {
    virama: String,
    inherent: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkeletonRules {
    drop_trailing: DropTrailing,
    pairs: Vec<(String, String)>,
    singles: HashMap<char, String>,
    collapse_doubles: bool,
    drop_final_vowel: DropFinalVowel,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DropTrailing {
    letters: String,
    after_vowel: bool,
    min_length: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DropFinalVowel {
    vowels: String,
    after_consonant: bool,
    min_length: usize,
}

/// One of the walk's tables, keyed by NFC strings of one or more chars.
struct Keys {
    map: HashMap<Vec<char>, String>,
    longest: usize,
}

impl Keys {
    fn new(raw: HashMap<String, String>) -> Self {
        let map: HashMap<Vec<char>, String> = raw
            .into_iter()
            .map(|(k, v)| (nfc(&k).chars().collect(), v))
            .collect();
        let longest = map.keys().map(Vec::len).max().unwrap_or(0);
        Self { map, longest }
    }

    /// The longest key that starts at `chars[i]`: its length in chars and its Latin.
    fn longest_at(&self, chars: &[char], i: usize) -> Option<(usize, &str)> {
        (1..=self.longest.min(chars.len() - i))
            .rev()
            .find_map(|size| Some((size, self.map.get(&chars[i..i + size])?.as_str())))
    }

    fn first_chars(&self) -> impl Iterator<Item = char> + '_ {
        self.map.keys().filter_map(|k| k.first().copied())
    }
}

/// A loaded `romanisation.json`.
pub struct Table {
    script: Script,
    virama: Vec<char>,
    inherent: String,
    end_virama: String,
    end_inherent: String,
    independent: Keys,
    signs: Keys,
    consonants: Keys,
    finals: Keys,
    clusters: Keys,
    /// Chars a word may start with: the first char of a consonant, cluster or independent vowel.
    starts: HashSet<char>,
    skeleton: SkeletonRules,
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

impl Table {
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)
            .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
        Self::parse(&text).map_err(|e| io::Error::other(format!("{}: {e}", path.display())))
    }

    pub fn parse(json: &str) -> Result<Self, String> {
        let raw: Raw = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if raw.format != 1 {
            return Err(format!("romanisation format {} (want 1)", raw.format));
        }
        let script = script(&raw.script).ok_or_else(|| format!("unknown script {}", raw.script))?;
        let independent = Keys::new(raw.independent_vowels);
        let consonants = Keys::new(raw.consonants);
        let clusters = Keys::new(raw.clusters);
        let starts = independent
            .first_chars()
            .chain(consonants.first_chars())
            .chain(clusters.first_chars())
            .collect();
        Ok(Self {
            script,
            virama: nfc(&raw.virama).chars().collect(),
            inherent: raw.inherent,
            end_virama: raw.at_word_end.virama,
            end_inherent: raw.at_word_end.inherent,
            independent,
            signs: Keys::new(raw.vowel_signs),
            consonants,
            finals: Keys::new(raw.finals),
            clusters,
            starts,
            skeleton: raw.skeleton,
        })
    }

    /// The first code point of the script block (U+0900 for Devanagari): `pack`'s base.
    pub fn block_base(&self) -> char {
        self.script.base
    }

    /// A char of the script that can be part of a word: in the block, not punctuation or a digit.
    pub fn is_letter(&self, c: char) -> bool {
        let base = self.script.base as u32;
        (base..base + 0x80).contains(&(c as u32))
            && !self
                .script
                .not_letters
                .iter()
                .any(|&(lo, hi)| (lo..=hi).contains(&c))
    }

    /// Every char is a letter of the script (`is_letter`) or a joiner, and at least one is a letter.
    pub fn is_native(&self, word: &str) -> bool {
        word.chars()
            .all(|c| self.is_letter(c) || c == ZWJ || c == ZWNJ)
            && word.chars().any(|c| self.is_letter(c))
    }

    /// A word starts with an independent vowel or a consonant (not a sign left over from
    /// "2020में"), and is not a lone consonant + virama.
    pub fn plausible(&self, word: &str) -> bool {
        let chars: Vec<char> = nfc(word).chars().collect();
        let Some(first) = chars.first() else {
            return false;
        };
        let lone_virama = chars.len() <= 1 + self.virama.len() && chars.ends_with(&self.virama);
        self.starts.contains(first) && !lone_virama
    }

    /// The walk (format spec, "The walk"): भारत → bharat, समझना → samajhana.
    pub fn canonical(&self, word: &str) -> String {
        let chars: Vec<char> = nfc(word)
            .chars()
            .filter(|&c| c != ZWJ && c != ZWNJ)
            .collect();
        let n = chars.len();
        let mut out = String::new();
        let mut i = 0;
        while i < n {
            let consonant = self
                .clusters
                .longest_at(&chars, i)
                .or_else(|| self.consonants.longest_at(&chars, i));
            if let Some((len, latin)) = consonant {
                out.push_str(latin);
                i += len;
                if let Some((len, sign)) =
                    (i < n).then(|| self.signs.longest_at(&chars, i)).flatten()
                {
                    out.push_str(sign);
                    i += len;
                } else if !self.virama.is_empty() && chars[i..].starts_with(&self.virama) {
                    i += self.virama.len();
                    if i >= n {
                        out.push_str(&self.end_virama);
                    }
                } else {
                    out.push_str(if i >= n {
                        &self.end_inherent
                    } else {
                        &self.inherent
                    });
                }
                continue;
            }
            if let Some((len, latin)) = self
                .independent
                .longest_at(&chars, i)
                .or_else(|| self.finals.longest_at(&chars, i))
            {
                out.push_str(latin);
                i += len;
                continue;
            }
            if chars[i].is_ascii_alphabetic() {
                out.push(chars[i].to_ascii_lowercase());
            }
            i += 1;
        }
        out
    }

    /// The skeleton (format spec, "The skeleton") of Latin text, typed or walked.
    pub fn skeleton(&self, latin: &str) -> String {
        let rules = &self.skeleton;
        let mut s: Vec<u8> = latin
            .bytes()
            .filter(u8::is_ascii_alphabetic)
            .map(|b| b.to_ascii_lowercase())
            .collect();
        let drop = &rules.drop_trailing;
        if s.len() >= drop.min_length
            && s.last()
                .is_some_and(|&c| drop.letters.as_bytes().contains(&c))
            && (!drop.after_vowel || (s.len() >= 2 && is_vowel(s[s.len() - 2] as char)))
        {
            s.pop();
        }
        let mut out: Vec<char> = Vec::with_capacity(s.len());
        let mut i = 0;
        while i < s.len() {
            let (sym, used) = match rules
                .pairs
                .iter()
                .find(|(pat, _)| s[i..].starts_with(pat.as_bytes()))
            {
                Some((pat, sym)) => (sym.clone(), pat.len()),
                None => {
                    let c = s[i] as char;
                    (
                        rules
                            .singles
                            .get(&c)
                            .cloned()
                            .unwrap_or_else(|| c.to_string()),
                        1,
                    )
                }
            };
            for ch in sym.chars() {
                if !(rules.collapse_doubles && out.last() == Some(&ch)) {
                    out.push(ch);
                }
            }
            i += used.max(1);
        }
        let last = &rules.drop_final_vowel;
        let len = out.len();
        if len > last.min_length
            && last.vowels.contains(out[len - 1])
            && (!last.after_consonant || (len >= 2 && !is_vowel(out[len - 2])))
        {
            out.pop();
        }
        out.into_iter().collect()
    }

    /// The skeleton of the word's walk: what the romanised lexicon is keyed by.
    pub fn word_skeleton(&self, word: &str) -> String {
        self.skeleton(&self.canonical(word))
    }

    /// One byte per char: the script block (128 code points from `block_base`) becomes
    /// 0x80–0xFF, ZWNJ 0x01, ZWJ 0x02. None if the word has any other character. The same
    /// packing as `mlengine_core::manglish::pack`, with this script's base.
    pub fn pack(&self, word: &str) -> Option<Vec<u8>> {
        let base = self.script.base as u32;
        word.chars()
            .map(|c| match c {
                ZWNJ => Some(0x01),
                ZWJ => Some(0x02),
                _ => {
                    let offset = (c as u32).checked_sub(base)?;
                    (offset < 0x80).then(|| 0x80 + offset as u8)
                }
            })
            .collect()
    }

    /// The romanised-lexicon key `skeleton\0packed-word`; None without a skeleton or packing.
    pub fn key(&self, word: &str) -> Option<Vec<u8>> {
        let mut key = self.word_skeleton(word).into_bytes();
        if key.is_empty() {
            return None;
        }
        key.push(SEPARATOR);
        key.extend(self.pack(word)?);
        Some(key)
    }

    /// Romanised-lexicon entries for a native vocabulary.
    pub fn entries(&self, vocab: &[(String, u64)]) -> Vec<(Vec<u8>, u64)> {
        vocab
            .iter()
            .filter_map(|(word, count)| Some((self.key(word)?, *count)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hindi() -> Table {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/hi.neopack/romanisation.json");
        Table::load(&path).unwrap()
    }

    #[test]
    fn walks_with_final_schwa_dropped() {
        let t = hindi();
        for (word, latin) in [
            ("भारत", "bharat"),
            ("कमरा", "kamara"),
            ("समझना", "samajhana"),
            ("मित्र", "mitr"),
            ("दर्द", "dard"),
            ("क्या", "kya"),
            ("है", "hai"),
            ("में", "men"),
            ("नहीं", "nahin"),
            ("संभव", "sanbhav"),
            ("ज़िंदगी", "zindagi"),
            ("लड़का", "ladaka"),
            ("ज्ञान", "gyan"),
            ("क्षमा", "kshama"),
            ("न", "n"),
            ("सत्यमेव", "satyamev"),
        ] {
            assert_eq!(t.canonical(word), latin, "{word}");
        }
    }

    #[test]
    fn nukta_letters_are_taken_before_the_bare_consonant() {
        let t = hindi();
        // NFC decomposes ज़ (U+095B) to ज + nukta; both spellings walk the same.
        assert_eq!(t.canonical("\u{095B}रा"), "zara");
        assert_eq!(t.canonical("\u{091C}\u{093C}रा"), "zara");
        assert_eq!(t.canonical("फ़िल्म"), "film");
    }

    #[test]
    fn skeleton_follows_the_spec_steps() {
        let t = hindi();
        assert_eq!(t.skeleton("bhaarat"), t.word_skeleton("भारत"));
        assert_eq!(t.skeleton("Bharat!"), "barat");
        assert_eq!(t.skeleton("nahi"), t.skeleton("nahih"), "dropTrailing h");
        assert_eq!(
            t.skeleton("sambhav"),
            t.skeleton("sanbhav"),
            "mbh before bh"
        );
        assert_eq!(t.skeleton("zindagi"), "jindagi");
        assert_eq!(t.skeleton("mein"), t.skeleton("men"));
        assert_ne!(t.skeleton("mein"), t.skeleton("main"), "में and मैं stay apart");
        assert_eq!(t.skeleton("pakka"), "paka", "collapseDoubles");
    }

    #[test]
    fn packs_the_block_and_round_trips_keys() {
        let t = hindi();
        let key = t.key("है").unwrap();
        assert_eq!(key, b"hai\0\xB9\xC8".to_vec());
        assert_eq!(t.pack("abc"), None);
        assert_eq!(
            t.pack("\u{0D15}"),
            None,
            "another script's letters don't pack"
        );
    }

    #[test]
    fn script_letters_exclude_danda_and_digits() {
        let t = hindi();
        assert!(t.is_native("नमस्ते"));
        assert!(!t.is_native("नमस्ते।"));
        assert!(!t.is_native("२०२०"));
        assert!(t.plausible("नमस्ते"));
        assert!(
            !t.plausible("\u{093E}र"),
            "a word can't start with a vowel sign"
        );
        assert!(!t.plausible("न्"));
    }

    #[test]
    fn rejects_other_formats_and_unknown_scripts() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/hi.neopack/romanisation.json");
        let text = fs::read_to_string(path).unwrap();
        assert!(Table::parse(&text.replace("\"format\": 1", "\"format\": 2")).is_err());
        assert!(Table::parse(&text.replace("\"deva\"", "\"xxxx\"")).is_err());
    }
}

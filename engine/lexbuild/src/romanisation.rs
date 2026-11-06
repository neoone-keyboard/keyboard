//! A pack's `romanisation.json`: native script → Latin, the skeleton people's spellings
//! collapse to, and the romanised-lexicon keys built from both.
//!
//! This is the data-driven counterpart of `mlengine_core::romanize` (Malayalam's romaniser,
//! written as code). `training/mlkb/data/table_romanise.py` implements the same rules from the
//! same file; `tests/romanisation.rs` checks the two agree on a fixture. The phase-2 script
//! plug-in will read the same tables at type time, so a typed word and a stored key meet.

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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Raw {
    block: [char; 2],
    #[serde(default)]
    not_letters: Vec<char>,
    virama: char,
    #[serde(default)]
    nukta: Option<char>,
    #[serde(default = "default_inherent")]
    inherent_vowel: String,
    independent_vowels: HashMap<String, String>,
    vowel_signs: HashMap<String, String>,
    #[serde(default)]
    nasalised_vowel_signs: HashMap<String, String>,
    consonants: HashMap<String, String>,
    #[serde(default)]
    nukta_consonants: HashMap<String, String>,
    #[serde(default)]
    clusters: HashMap<String, String>,
    #[serde(default)]
    modifiers: HashMap<String, String>,
    #[serde(default)]
    anusvara_before_labials: Option<Labials>,
    #[serde(default)]
    schwa: Schwa,
    skeleton: SkeletonRules,
}

fn default_inherent() -> String {
    "a".to_string()
}

#[derive(Deserialize)]
struct Labials {
    letters: String,
    latin: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Schwa {
    #[serde(default)]
    delete_final: bool,
    #[serde(default)]
    keep_final_after_cluster_ending_in: String,
    #[serde(default)]
    delete_medial: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkeletonRules {
    #[serde(default)]
    drop_trailing_h_after_vowel: bool,
    #[serde(default)]
    rewrites: HashMap<String, String>,
    #[serde(default)]
    singles: HashMap<char, String>,
    #[serde(default)]
    collapse_runs: bool,
    #[serde(default)]
    drop_between_consonants: String,
    #[serde(default)]
    final_rewrites: HashMap<String, String>,
    #[serde(default)]
    strip_final: Vec<char>,
}

/// A loaded `romanisation.json`.
pub struct Table {
    block: (char, char),
    not_letters: HashSet<char>,
    virama: char,
    nukta: Option<char>,
    inherent: String,
    independent: HashMap<char, String>,
    signs: HashMap<char, String>,
    nasal_signs: HashMap<char, String>,
    /// Keyed by the consonant's NFC letters: one char, or a consonant plus nukta.
    consonants: HashMap<String, String>,
    /// Three-char conjuncts (consonant, virama, consonant) spelled as a unit.
    clusters: HashMap<String, String>,
    modifiers: HashMap<char, String>,
    labials: HashSet<char>,
    labial_latin: String,
    schwa: Schwa,
    skeleton: SkeletonRules,
    /// Longest `skeleton.rewrites` key, in bytes, and the final rewrites longest first.
    longest_rewrite: usize,
    final_rewrites: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A consonant or conjunct piece; its vowel is the next unit.
    Consonant,
    /// A vowel sign or independent vowel.
    Vowel,
    /// The inherent vowel, which schwa deletion may drop.
    Schwa,
    /// Anusvara, chandrabindu, visarga.
    Modifier,
    /// An ASCII letter passed through.
    Latin,
}

#[derive(Debug, Clone)]
struct Unit {
    kind: Kind,
    latin: String,
    /// The native letters this unit spells (empty for a schwa).
    src: String,
    /// A consonant unit that is one letter, not a conjunct.
    single: bool,
}

fn single_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

fn char_map(map: HashMap<String, String>) -> HashMap<char, String> {
    map.into_iter()
        .filter_map(|(k, v)| Some((single_char(&nfc(&k))?, v)))
        .collect()
}

const VOWELS: &[char] = &['a', 'e', 'i', 'o', 'u'];

fn is_vowel(c: char) -> bool {
    VOWELS.contains(&c)
}

impl Table {
    pub fn load(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)
            .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
        Self::parse(&text).map_err(|e| io::Error::other(format!("{}: {e}", path.display())))
    }

    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        let raw: Raw = serde_json::from_str(json)?;
        let mut consonants: HashMap<String, String> = raw
            .consonants
            .into_iter()
            .map(|(k, v)| (nfc(&k), v))
            .collect();
        consonants.extend(raw.nukta_consonants.into_iter().map(|(k, v)| (nfc(&k), v)));
        let (labials, labial_latin) = raw
            .anusvara_before_labials
            .map_or((HashSet::new(), String::new()), |l| {
                (nfc(&l.letters).chars().collect(), l.latin)
            });
        let longest_rewrite = raw
            .skeleton
            .rewrites
            .keys()
            .map(String::len)
            .max()
            .unwrap_or(1);
        let mut final_rewrites: Vec<(String, String)> = raw
            .skeleton
            .final_rewrites
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        final_rewrites.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        Ok(Self {
            block: (raw.block[0], raw.block[1]),
            not_letters: raw.not_letters.into_iter().collect(),
            virama: raw.virama,
            nukta: raw.nukta,
            inherent: raw.inherent_vowel,
            independent: char_map(raw.independent_vowels),
            signs: char_map(raw.vowel_signs),
            nasal_signs: char_map(raw.nasalised_vowel_signs),
            consonants,
            clusters: raw
                .clusters
                .into_iter()
                .map(|(k, v)| (nfc(&k), v))
                .collect(),
            modifiers: char_map(raw.modifiers),
            labials,
            labial_latin,
            schwa: raw.schwa,
            skeleton: raw.skeleton,
            longest_rewrite,
            final_rewrites,
        })
    }

    /// The first code point of the script block (U+0900 for Devanagari): `pack`'s base.
    pub fn block_base(&self) -> char {
        self.block.0
    }

    /// A char of the script that can be part of a word: in the block, not punctuation or a digit.
    pub fn is_letter(&self, c: char) -> bool {
        (self.block.0..=self.block.1).contains(&c) && !self.not_letters.contains(&c)
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
        let chars: Vec<char> = word.chars().collect();
        let Some(&first) = chars.first() else {
            return false;
        };
        let starts = self.independent.contains_key(&first)
            || self.consonants.contains_key(first.to_string().as_str());
        let lone_virama = chars.len() <= 2 && chars.last() == Some(&self.virama);
        starts && !lone_virama
    }

    /// The consonant at `chars[i]`: its letters, Latin and length (a nukta letter first).
    fn consonant_at(&self, chars: &[char], i: usize) -> Option<(String, &str, usize)> {
        if let (Some(nukta), Some(&next)) = (self.nukta, chars.get(i + 1))
            && next == nukta
        {
            let letters: String = [chars[i], nukta].iter().collect();
            if let Some(latin) = self.consonants.get(&letters) {
                return Some((letters, latin, 2));
            }
        }
        let letters = chars.get(i)?.to_string();
        let latin = self.consonants.get(&letters)?;
        Some((letters, latin, 1))
    }

    fn is_nasal(&self, c: char) -> bool {
        self.modifiers
            .get(&c)
            .is_some_and(|latin| latin == "n" || latin == "m")
    }

    fn units(&self, word: &str, delete_schwas: bool) -> Vec<Unit> {
        let chars: Vec<char> = nfc(word)
            .chars()
            .filter(|&c| c != ZWJ && c != ZWNJ)
            .collect();
        let n = chars.len();
        let mut units = Vec::new();
        let mut i = 0;
        while i < n {
            let c = chars[i];
            let three: String = chars[i..n.min(i + 3)].iter().collect();
            let consonant = match self.clusters.get(&three) {
                Some(latin) if i + 3 <= n => Some((three.clone(), latin.as_str(), 3, false)),
                _ => self
                    .consonant_at(&chars, i)
                    .map(|(letters, latin, len)| (letters, latin, len, true)),
            };
            if let Some((letters, latin, len, single)) = consonant {
                let mut j = i + len;
                units.push(Unit {
                    kind: Kind::Consonant,
                    latin: latin.to_string(),
                    src: letters,
                    single,
                });
                match chars.get(j) {
                    Some(&sign) if self.signs.contains_key(&sign) => {
                        let nasal_final = j + 2 == n && self.is_nasal(chars[j + 1]);
                        let latin = if nasal_final {
                            self.nasal_signs.get(&sign).unwrap_or(&self.signs[&sign])
                        } else {
                            &self.signs[&sign]
                        };
                        units.push(Unit {
                            kind: Kind::Vowel,
                            latin: latin.clone(),
                            src: sign.to_string(),
                            single: true,
                        });
                        j += 1;
                    }
                    Some(&v) if v == self.virama => j += 1, // the next consonant joins this one
                    _ => units.push(Unit {
                        kind: Kind::Schwa,
                        latin: self.inherent.clone(),
                        src: String::new(),
                        single: true,
                    }),
                }
                i = j;
                continue;
            }
            if let Some(latin) = self.independent.get(&c) {
                let nasal_final = i + 2 == n && self.is_nasal(chars[i + 1]);
                let latin = if nasal_final {
                    self.nasal_signs.get(&c).unwrap_or(latin)
                } else {
                    latin
                };
                units.push(vowel(latin, c));
            } else if let Some(latin) = self.signs.get(&c) {
                units.push(vowel(latin, c)); // a stray sign: spelled as its vowel
            } else if let Some(latin) = self.modifiers.get(&c) {
                let before_labial =
                    self.consonant_at(&chars, i + 1)
                        .is_some_and(|(letters, _, _)| {
                            letters
                                .chars()
                                .next()
                                .is_some_and(|l| self.labials.contains(&l))
                        });
                let latin = if c == '\u{0902}' && before_labial {
                    self.labial_latin.clone()
                } else {
                    latin.clone()
                };
                units.push(Unit {
                    kind: Kind::Modifier,
                    latin,
                    src: c.to_string(),
                    single: true,
                });
            } else if c.is_ascii_alphabetic() {
                units.push(Unit {
                    kind: Kind::Latin,
                    latin: c.to_ascii_lowercase().to_string(),
                    src: c.to_string(),
                    single: true,
                });
            }
            i += 1;
        }
        if delete_schwas {
            self.delete_schwas(units)
        } else {
            units
        }
    }

    /// Drops the schwas the language doesn't pronounce (the table's `schwa`).
    fn delete_schwas(&self, units: Vec<Unit>) -> Vec<Unit> {
        let rules = &self.schwa;
        let len = units.len();
        let mut keep = vec![true; len];
        let vowels = units
            .iter()
            .filter(|u| matches!(u.kind, Kind::Vowel | Kind::Schwa))
            .count();
        // Final: after a single consonant in a word of two or more syllables; kept after a
        // conjunct ending in one of `keepFinalAfterClusterEndingIn` (mitra, kaarya).
        if rules.delete_final && len >= 2 && units[len - 1].kind == Kind::Schwa && vowels >= 2 {
            let last = len - 1;
            let ending = units[last - 1].src.chars().last();
            let kept = cluster_before(&units, last)
                && ending.is_some_and(|c| rules.keep_final_after_cluster_ending_in.contains(c));
            if !kept {
                keep[last] = false;
            }
        }
        if rules.delete_medial && len >= 4 {
            // Right to left: V C _ C V, each C a single consonant, the right V not dropped.
            for k in (2..len - 1).rev() {
                if units[k].kind != Kind::Schwa || !keep[k] {
                    continue;
                }
                if units[k + 1].kind == Kind::Modifier || cluster_before(&units, k) {
                    continue; // a nasalised schwa stays (sant); so does one after a conjunct
                }
                let left = units[k - 1].kind == Kind::Consonant
                    && matches!(
                        units[k - 2].kind,
                        Kind::Vowel | Kind::Schwa | Kind::Modifier
                    )
                    && keep[k - 2];
                let right = k + 2 < len
                    && units[k + 1].kind == Kind::Consonant
                    && units[k + 1].single
                    && matches!(units[k + 2].kind, Kind::Vowel | Kind::Schwa)
                    && keep[k + 2];
                if left && right {
                    keep[k] = false;
                }
            }
        }
        units
            .into_iter()
            .zip(keep)
            .filter_map(|(u, k)| k.then_some(u))
            .collect()
    }

    /// The romanisation the romanised lexicon is keyed by: भारत → bharat, कमरा → kamra.
    pub fn canonical(&self, word: &str) -> String {
        self.units(word, true)
            .iter()
            .map(|u| u.latin.as_str())
            .collect()
    }

    /// `latin` collapsed to the distinctions people type consistently (the table's `skeleton`).
    pub fn skeleton(&self, latin: &str) -> String {
        let rules = &self.skeleton;
        let mut s: Vec<char> = latin
            .chars()
            .filter(char::is_ascii_alphabetic)
            .map(|c| c.to_ascii_lowercase())
            .collect();
        if rules.drop_trailing_h_after_vowel
            && s.len() >= 3
            && s[s.len() - 1] == 'h'
            && is_vowel(s[s.len() - 2])
        {
            s.pop();
        }
        let s: String = s.into_iter().collect();
        let mut out: Vec<char> = Vec::with_capacity(s.len());
        let mut i = 0;
        while i < s.len() {
            let rewrite = (2..=self.longest_rewrite.min(s.len() - i))
                .rev()
                .find_map(|size| Some((rules.rewrites.get(&s[i..i + size])?.clone(), size)));
            let (sym, used) = rewrite.unwrap_or_else(|| {
                let c = s.as_bytes()[i] as char;
                (
                    rules
                        .singles
                        .get(&c)
                        .cloned()
                        .unwrap_or_else(|| c.to_string()),
                    1,
                )
            });
            for ch in sym.chars() {
                if !(rules.collapse_runs && out.last() == Some(&ch)) {
                    out.push(ch);
                }
            }
            i += used;
        }
        if !rules.drop_between_consonants.is_empty() {
            let len = out.len();
            out = out
                .iter()
                .enumerate()
                .filter(|&(k, &ch)| {
                    !(rules.drop_between_consonants.contains(ch)
                        && k > 0
                        && k + 1 < len
                        && !is_vowel(out[k - 1])
                        && !is_vowel(out[k + 1]))
                })
                .map(|(_, &ch)| ch)
                .collect();
        }
        let mut ending: String = out.iter().collect();
        if let Some((end, sym)) = self
            .final_rewrites
            .iter()
            .find(|(end, _)| ending.len() > end.len() && ending.ends_with(end.as_str()))
        {
            ending.truncate(ending.len() - end.len());
            ending.push_str(sym);
        }
        let mut out: Vec<char> = ending.chars().collect();
        if out.len() >= 3
            && rules.strip_final.contains(&out[out.len() - 1])
            && !is_vowel(out[out.len() - 2])
        {
            out.pop();
        }
        out.into_iter().collect()
    }

    /// The skeleton of the word's canonical romanisation.
    pub fn word_skeleton(&self, word: &str) -> String {
        self.skeleton(&self.canonical(word))
    }

    /// One byte per char: the script block (128 code points from `block_base`) becomes
    /// 0x80–0xFF, ZWNJ 0x01, ZWJ 0x02. None if the word has any other character. The same
    /// packing as `mlengine_core::manglish::pack`, with this script's base.
    pub fn pack(&self, word: &str) -> Option<Vec<u8>> {
        let base = self.block.0 as u32;
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

fn vowel(latin: &str, c: char) -> Unit {
    Unit {
        kind: Kind::Vowel,
        latin: latin.to_string(),
        src: c.to_string(),
        single: true,
    }
}

/// Whether the consonant(s) before the vowel at `k` are a cluster (two consonant units, or a conjunct).
fn cluster_before(units: &[Unit], k: usize) -> bool {
    if k < 1 || units[k - 1].kind != Kind::Consonant {
        return false;
    }
    !units[k - 1].single || (k >= 2 && units[k - 2].kind == Kind::Consonant)
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
    fn romanises_with_schwa_deletion() {
        let t = hindi();
        for (word, latin) in [
            ("भारत", "bharat"),
            ("कमरा", "kamra"),
            ("समझना", "samajhna"),
            ("बचपन", "bachpan"),
            ("अपना", "apna"),
            ("मित्र", "mitra"),
            ("दर्द", "dard"),
            ("क्या", "kya"),
            ("है", "hai"),
            ("में", "mein"),
            ("नहीं", "nahin"),
            ("संभव", "sambhav"),
            ("ज़िंदगी", "zindgi"),
            ("लड़का", "ladka"),
            ("ज्ञान", "gyan"),
            ("न", "na"),
        ] {
            assert_eq!(t.canonical(word), latin, "{word}");
        }
    }

    #[test]
    fn typed_spellings_share_a_skeleton() {
        let t = hindi();
        let want = t.word_skeleton("समझना");
        for typed in ["samajhna", "samjhna", "samajna", "samajhnaa", "samjhana"] {
            assert_eq!(t.skeleton(typed), want, "{typed}");
        }
        assert_eq!(t.skeleton("bhaarat"), t.word_skeleton("भारत"));
        assert_eq!(t.skeleton("nahi"), t.skeleton("nahih"));
        assert_ne!(t.skeleton("mein"), t.skeleton("main"), "में and मैं stay apart");
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
}

//! A pack's `romanisation.json` (format 1): the native script → Latin walk, and the skeleton
//! that collapses the spellings people type inconsistently. The romanised lexicon
//! (`manglish.fst`, `hinglish.fst` …) is keyed by `skeleton(romanize(word))`; the engine looks up
//! `skeleton(typed)`. The format, the walk and the skeleton are specified in
//! `docs/design/2026-11-06-m6-romanisation-format.md`.
//!
//! `lexbuild` and the Python twin (`training/mlkb/data/romanisation.py`) read the same file, so
//! the builder and the engine can't disagree about a key.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;
use unicode_normalization::UnicodeNormalization;

const ZWNJ: char = '\u{200C}';
const ZWJ: char = '\u{200D}';

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
    skeleton: RawSkeleton,
    #[serde(default)]
    loose_spelling: Option<RawLoose>,
}

/// `looseSpelling`: every part optional.
#[derive(Deserialize)]
struct RawLoose {
    #[serde(default)]
    singles: HashMap<char, char>,
    #[serde(default)]
    pairs: Vec<(String, char)>,
    #[serde(default)]
    collapse: String,
}

#[derive(Deserialize)]
struct AtWordEnd {
    virama: String,
    inherent: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSkeleton {
    drop_trailing: DropTrailing,
    pairs: Vec<(String, String)>,
    singles: HashMap<char, String>,
    collapse_doubles: bool,
    #[serde(default)]
    drop_between_consonants: String,
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
#[derive(Debug)]
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

#[derive(Debug)]
struct Skeleton {
    trailing: Vec<u8>,
    trailing_after_vowel: bool,
    trailing_min: usize,
    pairs: Vec<(Vec<u8>, Vec<u8>)>,
    singles: [Option<Vec<u8>>; 26],
    collapse_doubles: bool,
    between: Vec<u8>,
    final_vowels: Vec<u8>,
    final_after_consonant: bool,
    final_min: usize,
}

/// `looseSpelling`: the Latin spellings that count as one when the engine asks whether the
/// typed letters spell a word out (`Romanisation::relaxed`). Narrower than the skeleton, which
/// only finds candidates: two words that differ here are told apart by what was typed.
#[derive(Debug, PartialEq)]
struct Loose {
    /// A letter read as another (Malayalam `d` as `t`: ട is typed either way).
    singles: [u8; 26],
    /// Two letters in a row read as one (`ee` as `i`): first, second, result.
    pairs: Vec<(u8, u8, u8)>,
    /// Letters whose repeats count once (`aa` as `a`).
    collapse: Vec<u8>,
}

impl Loose {
    fn new(singles: &[(u8, u8)], pairs: &[(u8, u8, u8)], collapse: &[u8]) -> Self {
        let mut map = LETTERS;
        for &(from, to) in singles {
            map[usize::from(from - b'a')] = to;
        }
        Self {
            singles: map,
            pairs: pairs.to_vec(),
            collapse: collapse.to_vec(),
        }
    }

    /// What a `romanisation.json` without `looseSpelling` gets: the packs published before the
    /// field were all read with Malayalam's habits (vowel length, and t for d), and still are.
    fn before_the_field() -> Self {
        Self::new(&[(b'd', b't')], &[(b'e', b'e', b'i'), (b'o', b'o', b'u')], b"aiu")
    }

    fn from_raw(raw: RawLoose) -> Result<Self, String> {
        let letter = |c: char, what: &str| {
            if c.is_ascii_lowercase() {
                Ok(c as u8)
            } else {
                Err(format!("looseSpelling {what} {c:?}: lower-case a–z"))
            }
        };
        let singles = raw
            .singles
            .into_iter()
            .map(|(from, to)| Ok((letter(from, "singles key")?, letter(to, "singles value")?)))
            .collect::<Result<Vec<_>, String>>()?;
        let pairs = raw
            .pairs
            .into_iter()
            .map(|(pat, to)| {
                let pat = ascii_letters(&pat, "looseSpelling pair")?;
                match pat[..] {
                    [a, b] => Ok((a, b, letter(to, "pairs value")?)),
                    _ => Err("a looseSpelling pair is two letters and the one they are read as".to_string()),
                }
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self::new(
            &singles,
            &pairs,
            &ascii_letters(&raw.collapse, "looseSpelling collapse")?,
        ))
    }
}

const LETTERS: [u8; 26] = *b"abcdefghijklmnopqrstuvwxyz";

/// A loaded `romanisation.json`.
#[derive(Debug)]
pub struct Romanisation {
    loose: Loose,
    script: String,
    virama: Vec<char>,
    inherent: String,
    end_virama: String,
    end_inherent: String,
    independent: Keys,
    signs: Keys,
    consonants: Keys,
    finals: Keys,
    clusters: Keys,
    skeleton: Skeleton,
}

fn nfc(s: &str) -> String {
    s.nfc().collect()
}

fn is_vowel(b: u8) -> bool {
    matches!(b, b'a' | b'e' | b'i' | b'o' | b'u')
}

fn ascii_letters(s: &str, what: &str) -> Result<Vec<u8>, String> {
    if s.bytes().all(|b| b.is_ascii_lowercase()) {
        Ok(s.as_bytes().to_vec())
    } else {
        Err(format!("{what} {s:?}: the skeleton is lower-case a–z"))
    }
}

impl Romanisation {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn parse(json: &str) -> Result<Self, String> {
        let raw: Raw = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if raw.format != 1 {
            return Err(format!(
                "romanisation format {} (this engine reads 1)",
                raw.format
            ));
        }
        let s = raw.skeleton;
        let mut singles: [Option<Vec<u8>>; 26] = Default::default();
        for (c, sym) in s.singles {
            if !c.is_ascii_lowercase() {
                return Err(format!("singles key {c:?}: the skeleton is lower-case a–z"));
            }
            singles[(c as u8 - b'a') as usize] = Some(ascii_letters(&sym, "singles value")?);
        }
        let pairs = s
            .pairs
            .into_iter()
            .map(|(pat, sym)| Ok((ascii_letters(&pat, "pair")?, ascii_letters(&sym, "pair")?)))
            .collect::<Result<Vec<_>, String>>()?;
        if pairs.iter().any(|(pat, _)| pat.is_empty()) {
            return Err("an empty pairs pattern".into());
        }
        let skeleton = Skeleton {
            trailing: ascii_letters(&s.drop_trailing.letters, "dropTrailing")?,
            trailing_after_vowel: s.drop_trailing.after_vowel,
            trailing_min: s.drop_trailing.min_length,
            pairs,
            singles,
            collapse_doubles: s.collapse_doubles,
            between: ascii_letters(&s.drop_between_consonants, "dropBetweenConsonants")?,
            final_vowels: ascii_letters(&s.drop_final_vowel.vowels, "dropFinalVowel")?,
            final_after_consonant: s.drop_final_vowel.after_consonant,
            final_min: s.drop_final_vowel.min_length,
        };
        let loose = match raw.loose_spelling {
            Some(raw) => Loose::from_raw(raw)?,
            None => Loose::before_the_field(),
        };
        Ok(Self {
            loose,
            script: raw.script,
            virama: nfc(&raw.virama).chars().collect(),
            inherent: raw.inherent,
            end_virama: raw.at_word_end.virama,
            end_inherent: raw.at_word_end.inherent,
            independent: Keys::new(raw.independent_vowels),
            signs: Keys::new(raw.vowel_signs),
            consonants: Keys::new(raw.consonants),
            finals: Keys::new(raw.finals),
            clusters: Keys::new(raw.clusters),
            skeleton,
        })
    }

    /// The script the tables are for (ISO 15924, lower case).
    pub fn script(&self) -> &str {
        &self.script
    }

    /// Chars a native word may start with: the first char of a consonant, cluster or
    /// independent vowel.
    pub fn word_starts(&self) -> impl Iterator<Item = char> + '_ {
        self.independent
            .first_chars()
            .chain(self.consonants.first_chars())
            .chain(self.clusters.first_chars())
    }

    /// The virama, as chars (empty for a script without one).
    pub fn virama(&self) -> &[char] {
        &self.virama
    }

    /// The walk: എന്റെ → ente, വീട് → veetu, भारत → bharat. ASCII letters pass through
    /// lower-cased; anything else the tables don't know is skipped.
    pub fn romanize(&self, word: &str) -> String {
        let chars: Vec<char> = nfc(word)
            .chars()
            .filter(|&c| c != ZWJ && c != ZWNJ)
            .collect();
        let n = chars.len();
        let mut out = String::with_capacity(word.len() * 2);
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

    /// The skeleton of Latin text, typed or walked (`ente`, `ende`, `entey` → `ent`).
    pub fn skeleton(&self, latin: &str) -> String {
        let rules = &self.skeleton;
        let mut s: Vec<u8> = latin
            .bytes()
            .filter(u8::is_ascii_alphabetic)
            .map(|b| b.to_ascii_lowercase())
            .collect();
        // 2. dropTrailing: entey → ente.
        if s.len() >= rules.trailing_min.max(1)
            && rules.trailing.contains(&s[s.len() - 1])
            && (!rules.trailing_after_vowel || (s.len() >= 2 && is_vowel(s[s.len() - 2])))
        {
            s.pop();
        }
        // 3–4. pairs, singles, collapseDoubles.
        let mut out: Vec<u8> = Vec::with_capacity(s.len());
        let mut i = 0;
        while i < s.len() {
            let (sym, used): (&[u8], usize) =
                match rules.pairs.iter().find(|(pat, _)| s[i..].starts_with(pat)) {
                    Some((pat, sym)) => (sym, pat.len()),
                    None => (
                        rules.singles[(s[i] - b'a') as usize]
                            .as_deref()
                            .unwrap_or(&s[i..i + 1]),
                        1,
                    ),
                };
            for &b in sym {
                if !(rules.collapse_doubles && out.last() == Some(&b)) {
                    out.push(b);
                }
            }
            i += used;
        }
        // 5. dropBetweenConsonants: samajhana, samjhna → smjna. Every such vowel at once.
        if !rules.between.is_empty() && out.len() >= 3 {
            let keep: Vec<bool> = (0..out.len())
                .map(|k| {
                    !(rules.between.contains(&out[k])
                        && k > 0
                        && k + 1 < out.len()
                        && !is_vowel(out[k - 1])
                        && !is_vowel(out[k + 1]))
                })
                .collect();
            out = out
                .into_iter()
                .zip(keep)
                .filter_map(|(b, k)| k.then_some(b))
                .collect();
        }
        // 6. dropFinalVowel: ente → ent. `minLength` is the length before the drop.
        let len = out.len();
        if len >= rules.final_min.max(2)
            && rules.final_vowels.contains(&out[len - 1])
            && (!rules.final_after_consonant || !is_vowel(out[len - 2]))
        {
            out.pop();
        }
        String::from_utf8(out).unwrap_or_default()
    }

    /// Lower-case letters with the spellings the pack's `looseSpelling` calls the same made
    /// one: `singles` map a letter, `pairs` read two letters in a row as one, `collapse` letters
    /// count once however often they repeat. Two spellings equal here spell the same word out
    /// (`manglish::fit_latin`). Malayalam: d is t, ee is i, oo is u, and aa, ii, uu are a, i, u;
    /// doubled consonants are kept (`veedu` is വീട്, veetu, not വിട്ടു, vittu).
    pub fn relaxed(&self, latin: &[u8]) -> Vec<u8> {
        let rules = &self.loose;
        let mut out: Vec<u8> = Vec::with_capacity(latin.len());
        for &b in latin {
            let b = b.to_ascii_lowercase();
            if !b.is_ascii_lowercase() {
                continue;
            }
            let b = rules.singles[usize::from(b - b'a')];
            let last = out.last().copied();
            if let Some(&(_, _, to)) = rules
                .pairs
                .iter()
                .find(|&&(first, second, _)| last == Some(first) && b == second)
            {
                out.pop();
                out.push(to);
            } else if !(last == Some(b) && rules.collapse.contains(&b)) {
                out.push(b);
            }
        }
        out
    }

    /// `skeleton(romanize(word))`: what the romanised lexicon is keyed by.
    pub fn word_skeleton(&self, word: &str) -> String {
        self.skeleton(&self.romanize(word))
    }
}

/// Malayalam's tables from `packs/ml.neopack`, for the engine's unit tests.
#[cfg(test)]
pub(crate) fn ml() -> &'static Romanisation {
    static ML: std::sync::OnceLock<Romanisation> = std::sync::OnceLock::new();
    ML.get_or_init(load_ml)
}

/// An owned copy of `ml()`, for a test's `Sources`.
#[cfg(test)]
pub(crate) fn load_ml() -> Romanisation {
    Romanisation::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/ml.neopack/romanisation.json"),
    )
    .unwrap()
}

/// Malayalam's tables with `looseSpelling` set to `json`, or without the field for None.
#[cfg(test)]
pub(crate) fn with_loose_spelling(json: Option<&str>) -> Romanisation {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/ml.neopack/romanisation.json");
    let mut file: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let fields = file.as_object_mut().unwrap();
    match json {
        Some(json) => fields.insert("looseSpelling".into(), serde_json::from_str(json).unwrap()),
        None => fields.remove("looseSpelling"),
    };
    Romanisation::parse(&file.to_string()).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack(id: &str) -> Romanisation {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../packs/{id}.neopack/romanisation.json"));
        Romanisation::load(&path).unwrap()
    }

    #[test]
    fn malayalam_walks_like_people_type() {
        let ml = pack("ml");
        for (word, latin) in [
            ("എന്റെ", "ente"),
            ("നിന്റെ", "ninte"),
            ("വീട്", "veetu"),
            ("മലയാളം", "malayaalam"),
            ("ഞാൻ", "njaan"),
            ("അമ്മ", "amma"),
            ("എനിക്ക്", "enikku"),
            ("കേരളം", "keralam"),
            ("സുഖമാണോ", "sukhamaano"),
            ("കുട്ടി", "kutti"),
            ("ഇല്ല", "illa"),
            ("പോയി", "poyi"),
            ("ചോറ്റ്", "chottu"),
            ("ഭക്ഷണം", "bhakshanam"),
        ] {
            assert_eq!(ml.romanize(word), latin, "{word}");
        }
    }

    /// Asserts every typed spelling collapses to the skeleton of `word`.
    fn typed_as(r: &Romanisation, word: &str, typed: &[&str]) {
        let want = r.word_skeleton(word);
        for t in typed {
            assert_eq!(r.skeleton(t), want, "{t} → {word}");
        }
    }

    #[test]
    fn malayalam_spellings_share_a_skeleton() {
        let ml = pack("ml");
        assert_eq!(ml.word_skeleton("എന്റെ"), "ent");
        typed_as(&ml, "എന്റെ", &["ente", "ende", "entey", "enthe", "Ente"]);
        typed_as(&ml, "വീട്", &["veedu", "veed", "vidu", "veetu"]);
        typed_as(&ml, "ഞാൻ", &["njan", "njaan"]);
        assert_ne!(
            ml.skeleton("njn"),
            ml.word_skeleton("ഞാൻ"),
            "middle vowels stay"
        );
        typed_as(&ml, "മലയാളം", &["malayalam", "malayaalam"]);
        typed_as(&ml, "എനിക്ക്", &["enikku", "eniku", "enikk", "enik"]);
        typed_as(&ml, "സുഖം", &["sukham", "sugham", "sukam"]);
        typed_as(&ml, "കൊള്ളാം", &["kollam", "kolam"]);
        typed_as(&ml, "മേഘം", &["megham", "mekham"]);
        assert_ne!(
            ml.skeleton("megam"),
            ml.word_skeleton("മേഘം"),
            "ഘ shares ഖ's k"
        );
        typed_as(&ml, "എന്താ", &["entha", "enthaa", "enta"]);
        assert_ne!(
            ml.skeleton("entha"),
            ml.word_skeleton("എന്ത്"),
            "a final -a is a real vowel"
        );
        assert_ne!(ml.skeleton("amma"), ml.skeleton("ammu"));
        assert_ne!(ml.skeleton("pani"), ml.skeleton("bani"));
        assert_ne!(ml.skeleton("kada"), ml.skeleton("gada"));
        assert_eq!(ml.skeleton("Ente!"), "ent");
        assert_eq!(ml.skeleton(""), "");
        assert_eq!(ml.skeleton("123"), "");
    }

    #[test]
    fn hindi_drops_the_vowel_between_consonants() {
        let hi = pack("hi");
        assert_eq!(hi.romanize("समझना"), "samajhana");
        typed_as(&hi, "समझना", &["samajhna", "samjhna", "samajhana"]);
        assert_eq!(hi.romanize("भारत"), "bharat");
        assert_eq!(hi.romanize("\u{095B}रा"), "zara", "nukta letters, NFC");
        assert_ne!(
            hi.skeleton("mein"),
            hi.skeleton("main"),
            "में and मैं stay apart"
        );
    }

    #[test]
    fn assamese_loads() {
        let as_ = pack("as");
        assert_eq!(as_.script(), "beng");
        assert_eq!(as_.romanize("মানুহ"), "manuh");
    }

    #[test]
    fn rejects_what_it_cannot_read() {
        let ml = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/ml.neopack/romanisation.json"),
        )
        .unwrap();
        assert!(Romanisation::parse(&ml.replace("\"format\": 1", "\"format\": 2")).is_err());
        assert!(Romanisation::parse("{").is_err());
    }

    #[test]
    fn never_panics_on_odd_text() {
        let ml = pack("ml");
        for s in [
            "",
            "്",
            "ാ",
            "a\u{0D4D}",
            "\u{200D}",
            "x",
            "ZZZ",
            "൧൨",
            "猫",
            "ന്‍",
        ] {
            let _ = ml.word_skeleton(s);
            assert!(ml.skeleton(s).bytes().all(|b| b.is_ascii_lowercase()));
        }
    }
}

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
    optional_finals: Vec<String>,
    #[serde(default)]
    loose_spelling: Option<RawLoose>,
    #[serde(default)]
    swipe: Option<RawSwipe>,
    #[serde(default)]
    from_latin: Option<crate::translit::RawFromLatin>,
}

/// `swipe`: each part optional, derived from the skeleton when absent.
#[derive(Deserialize)]
struct RawSwipe {
    #[serde(default)]
    aspirated: Option<String>,
    #[serde(default)]
    trail: Option<String>,
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
    #[serde(default)]
    also_between_consonants: String,
    #[serde(default)]
    also_beside_consonant: HashMap<char, String>,
    drop_final_vowel: DropFinalVowel,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DropTrailing {
    letters: String,
    after_vowel: bool,
    min_length: usize,
    #[serde(default)]
    also_kept: bool,
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
    /// `dropTrailing.alsoKept`: a word whose trailing letter hides a letter `between` would
    /// drop is also keyed with the trailing letter kept (`Romanisation::word_skeletons`).
    trailing_also_kept: bool,
    pairs: Vec<(Vec<u8>, Vec<u8>)>,
    singles: [Option<Vec<u8>>; 26],
    collapse_doubles: bool,
    between: Vec<u8>,
    /// `alsoBetweenConsonants`: letters people write between two consonants for the vowel
    /// `between` drops there (Hindi's `u`: `hum` for हम, `ham`).
    between_also: Vec<u8>,
    /// `alsoBesideConsonant`: a letter people write for that vowel only beside one of the
    /// consonants named (Hindi's `e` beside `h`: `kehna` for कहना, `behen` for बहन), sorted
    /// by letter. See `Skeleton::stands_in`.
    between_beside: Vec<(u8, Vec<u8>)>,
    final_vowels: Vec<u8>,
    final_after_consonant: bool,
    final_min: usize,
}

impl Skeleton {
    /// Which letters of `s` (a skeleton, or typed letters) may stand for the vowel `between`
    /// drops: an `alsoBesideConsonant` letter between two consonants when the one after it is
    /// one of its own (`kehte`, `rehna`), or the one before it is and the vowel before that
    /// one is dropped or stands for the dropped vowel too (`behen`, `bahen`, `sheher`; not
    /// `her`, whose h begins the word, nor `mohen`); and, with `anywhere`, an
    /// `alsoBetweenConsonants` letter between any two consonants (`humse`).
    fn stands_in(&self, s: &[u8], anywhere: bool) -> Vec<bool> {
        let mut out = vec![false; s.len()];
        for k in 1..s.len().saturating_sub(1) {
            if is_vowel(s[k - 1]) || is_vowel(s[k + 1]) {
                continue;
            }
            if self.between_also.contains(&s[k]) {
                out[k] = anywhere;
            } else if let Some((_, beside)) = self.between_beside.iter().find(|(l, _)| *l == s[k]) {
                // The letter two back: a consonant (the vowel between was dropped from the
                // skeleton), that vowel still written (typed letters), or a stand-in for it.
                let dropped_before = k >= 2
                    && (out[k - 2]
                        || !is_vowel(s[k - 2])
                        || (self.between.contains(&s[k - 2]) && k >= 3 && !is_vowel(s[k - 3])));
                out[k] = beside.contains(&s[k + 1]) || (beside.contains(&s[k - 1]) && dropped_before);
            }
        }
        out
    }
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

/// The swipe rules of a skeleton. A skeleton letter is drawn through its own key and through
/// every letter the skeleton folds into it: a `singles` letter that becomes it, and the first
/// letter of a `pairs` pattern that does (`gh` → `k`: the finger starts on `g`). Unless the
/// file's `swipe` section says otherwise, the letters an `h` may follow are those a pattern
/// ending in `h` becomes (`th` → `t`), and the letters drawn after the skeleton's end are the
/// ones it drops there: `dropTrailing.letters`, `dropFinalVowel.vowels`, and each `singles`
/// letter that becomes one of those vowels (Assamese drops a final `a`, and reads `o` as `a`:
/// the `o` of `bhalo` is dropped too). The letters a path may draw between two consonants are
/// the skeleton's `dropBetweenConsonants`.
fn swipe_rules(skeleton: &Skeleton, raw: Option<RawSwipe>) -> Result<crate::swipe::SkeletonRules, String> {
    let mut also: Vec<(u8, u8)> = Vec::new();
    for (typed, symbol) in LETTERS.iter().zip(&skeleton.singles) {
        if let Some([letter]) = symbol.as_deref() {
            also.push((*letter, *typed));
        }
    }
    let mut aspirated: Vec<u8> = Vec::new();
    for (pattern, symbol) in &skeleton.pairs {
        if let [letter] = symbol[..] {
            also.push((letter, pattern[0]));
        }
        if let (Some(b'h'), Some(&last)) = (pattern.last(), symbol.last())
            && pattern.len() > 1
            && last != b'h'
        {
            aspirated.push(last);
        }
    }
    let mut trail = skeleton.trailing.clone();
    for vowel in &skeleton.final_vowels {
        trail.push(*vowel);
        for (typed, symbol) in LETTERS.iter().zip(&skeleton.singles) {
            if symbol.as_deref() == Some(std::slice::from_ref(vowel)) {
                trail.push(*typed);
            }
        }
    }
    let (said_aspirated, said_trail) = raw.map_or((None, None), |r| (r.aspirated, r.trail));
    if let Some(said) = said_aspirated {
        aspirated = ascii_letters(&said, "swipe aspirated")?;
    }
    if let Some(said) = said_trail {
        trail = ascii_letters(&said, "swipe trail")?;
    }
    // The dropped letters themselves, and the ones written for them beside one consonant
    // (`alsoBesideConsonant`, Hindi's `e` beside `h`), which few skeletons have a place for.
    // Not the ones written for them anywhere (`alsoBetweenConsonants`, Hindi's `u`): those
    // are for typed letters. Drawn at no cost between any two consonants, a `u` let every
    // skeleton stand for its words with a real u too, and Hindi's held-out swipes fell from
    // 17.7% to 15.7% first.
    Ok(crate::swipe::SkeletonRules::new(&also, &aspirated, &trail)
        .dropping_between(&skeleton.between)
        .also_beside(&skeleton.between_beside))
}

/// A loaded `romanisation.json`.
#[derive(Debug)]
pub struct Romanisation {
    loose: Loose,
    swipe: crate::swipe::SkeletonRules,
    speller: crate::translit::Speller,
    script: String,
    /// The first code point its pack's words are packed from (`packed_from`).
    base: Option<char>,
    virama: Vec<char>,
    inherent: String,
    end_virama: String,
    end_inherent: String,
    independent: Keys,
    signs: Keys,
    consonants: Keys,
    finals: Keys,
    /// `optionalFinals`: the signs of `finals` people leave unwritten at a word's end, NFC.
    optional_finals: Vec<Vec<char>>,
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
            trailing_also_kept: s.drop_trailing.also_kept,
            pairs,
            singles,
            collapse_doubles: s.collapse_doubles,
            between: ascii_letters(&s.drop_between_consonants, "dropBetweenConsonants")?,
            between_also: ascii_letters(&s.also_between_consonants, "alsoBetweenConsonants")?,
            between_beside: {
                let mut beside = s
                    .also_beside_consonant
                    .into_iter()
                    .map(|(letter, consonants)| {
                        let consonants = ascii_letters(&consonants, "alsoBesideConsonant")?;
                        if !letter.is_ascii_lowercase() || consonants.iter().any(|&c| is_vowel(c)) {
                            return Err(format!(
                                "alsoBesideConsonant {letter:?}: a lower-case letter and the consonants it is written beside"
                            ));
                        }
                        Ok((letter as u8, consonants))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                beside.sort();
                beside
            },
            final_vowels: ascii_letters(&s.drop_final_vowel.vowels, "dropFinalVowel")?,
            final_after_consonant: s.drop_final_vowel.after_consonant,
            final_min: s.drop_final_vowel.min_length,
        };
        let loose = match raw.loose_spelling {
            Some(raw) => Loose::from_raw(raw)?,
            None => Loose::before_the_field(),
        };
        let swipe = swipe_rules(&skeleton, raw.swipe)?;
        let virama = nfc(&raw.virama);
        let independent = Keys::new(raw.independent_vowels);
        let signs = Keys::new(raw.vowel_signs);
        let consonants = Keys::new(raw.consonants);
        let clusters = Keys::new(raw.clusters);
        let finals = Keys::new(raw.finals);
        let optional_finals = raw
            .optional_finals
            .iter()
            .map(|sign| {
                let key: Vec<char> = nfc(sign).chars().collect();
                match finals.map.get(&key) {
                    Some(latin) if !latin.is_empty() => Ok(key),
                    Some(_) => Err(format!("optionalFinals {sign:?}: its Latin in finals is empty already")),
                    None => Err(format!("optionalFinals {sign:?}: not one of finals")),
                }
            })
            .collect::<Result<Vec<_>, String>>()?;
        // The file's own speller; else the one the engine holds for the script (Malayalam's,
        // for its packs published before the section); else one from the tables turned round.
        let speller = match raw.from_latin {
            Some(section) => crate::translit::Speller::from_raw(section, &virama)?,
            None => crate::translit::embedded(&raw.script).unwrap_or_else(|| {
                let mut letters = crate::translit::rows(&clusters.map);
                letters.extend(crate::translit::rows(&consonants.map));
                let singles: Vec<(u8, Vec<u8>)> = LETTERS
                    .iter()
                    .zip(&skeleton.singles)
                    .filter_map(|(l, s)| Some((*l, s.clone()?)))
                    .collect();
                crate::translit::Speller::derived(
                    &virama,
                    &letters,
                    &crate::translit::rows(&independent.map),
                    &crate::translit::rows(&signs.map),
                    &raw.inherent,
                    &singles,
                    raw.at_word_end.inherent.is_empty(),
                )
            }),
        };
        Ok(Self {
            loose,
            swipe,
            speller,
            script: raw.script,
            base: None,
            virama: virama.chars().collect(),
            inherent: raw.inherent,
            end_virama: raw.at_word_end.virama,
            end_inherent: raw.at_word_end.inherent,
            independent,
            signs,
            consonants,
            finals,
            optional_finals,
            clusters,
            skeleton,
        })
    }

    /// The script the tables are for (ISO 15924, lower case).
    pub fn script(&self) -> &str {
        &self.script
    }

    /// These tables for a pack whose romanised lexicon packs each word from `base`
    /// (`manglish::pack`): the first code point of the pack's own rules' block. Two packs of
    /// one script may differ in it, so the engine sets each pack's from its own `lang.json`.
    pub fn packed_from(mut self, base: char) -> Self {
        self.base = Some(base);
        self
    }

    /// The base `packed_from` set, if any.
    pub fn base(&self) -> Option<char> {
        self.base
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

    /// The walk without the word's last sign, when the pack's `optionalFinals` says people
    /// leave it unwritten there (Hindi's anusvara and chandrabindu: नहीं is `nahin` and
    /// `nahi`, हूँ `hun` and `hu`). The letters before the sign are as the whole walk has
    /// them. Only after a written vowel (a vowel sign or an independent vowel): after a bare
    /// consonant the sign is the syllable's whole ending and is written (स्वयं `svayan`, सं
    /// `san`). None for any other word.
    pub fn romanize_short(&self, word: &str) -> Option<String> {
        if self.optional_finals.is_empty() {
            return None;
        }
        let chars: Vec<char> = nfc(word)
            .chars()
            .filter(|&c| c != ZWJ && c != ZWNJ)
            .collect();
        let sign = self.optional_finals.iter().find(|sign| chars.ends_with(sign))?;
        let before = &chars[..chars.len() - sign.len()];
        let after_vowel = [&self.signs, &self.independent]
            .iter()
            .any(|table| table.map.keys().any(|vowel| before.ends_with(vowel)));
        if !after_vowel {
            return None;
        }
        let latin = self.finals.map.get(sign)?;
        let whole = self.romanize(word);
        let short = whole.strip_suffix(latin.as_str())?;
        (!short.is_empty()).then(|| short.to_string())
    }

    /// The skeleton of Latin text, typed or walked (`ente`, `ende`, `entey` → `ent`).
    pub fn skeleton(&self, latin: &str) -> String {
        self.skeleton_with(latin, true)
    }

    /// `skeleton`, with or without its `dropTrailing` step.
    fn skeleton_with(&self, latin: &str, drop_trailing: bool) -> String {
        let rules = &self.skeleton;
        let mut s: Vec<u8> = latin
            .bytes()
            .filter(u8::is_ascii_alphabetic)
            .map(|b| b.to_ascii_lowercase())
            .collect();
        // 2. dropTrailing: entey → ente.
        if drop_trailing
            && s.len() >= rules.trailing_min.max(1)
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

    /// The other ways the pack's `alsoBesideConsonant` and `alsoBetweenConsonants` let typed
    /// letters be read: each such letter taken for the vowel `dropBetweenConsonants` drops
    /// there (`Skeleton::stands_in`). Each reading is its skeleton, with those letters dropped
    /// too, and the letters written out with the vowel in their place, to measure against a
    /// word's romanisation (Hindi: `humse` is `hmse`, हमसे's, written `hamse`; `kehte` is
    /// `khte`, कहते's, written `kahte`). First with only the letters written beside a
    /// consonant read so, then with the others as well (`mehsoos`: `mhsus`, महसूस's, then
    /// `mhss`), when that is another reading. Empty when the tables name no such letter or
    /// the typed letters have none there: there is then only `skeleton`.
    pub fn respellings(&self, latin: &str) -> Vec<(String, String)> {
        let rules = &self.skeleton;
        let Some(&vowel) = rules.between.first() else {
            return Vec::new();
        };
        let whole = self.skeleton(latin).into_bytes();
        let letters: Vec<u8> = latin
            .bytes()
            .filter(u8::is_ascii_alphabetic)
            .map(|b| b.to_ascii_lowercase())
            .collect();
        let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap_or_default();
        let mut out: Vec<(String, String)> = Vec::new();
        for anywhere in [false, true] {
            let kept: Vec<u8> = whole
                .iter()
                .zip(rules.stands_in(&whole, anywhere))
                .filter_map(|(&b, dropped)| (!dropped).then_some(b))
                .collect();
            if kept.len() == whole.len() || kept.is_empty() {
                continue;
            }
            let skeleton = text(kept);
            if out.iter().all(|(seen, _)| *seen != skeleton) {
                let written = letters
                    .iter()
                    .zip(rules.stands_in(&letters, anywhere))
                    .map(|(&b, stands_in)| if stands_in { vowel } else { b })
                    .collect();
                out.push((skeleton, text(written)));
            }
        }
        out
    }

    /// The Latin a consonant of the tables is spelled with, without a vowel (യ → `y`).
    pub fn consonant(&self, letter: char) -> Option<&str> {
        self.consonants.map.get(&[letter][..]).map(String::as_str)
    }

    /// `latin` spelled in the script by rule (`translit::Speller`): a guess for a word no
    /// lexicon or model has. None unless it is all letters the tables can spell.
    pub fn spell(&self, latin: &str) -> Option<String> {
        self.speller.spell(latin)
    }

    /// What the skeleton means for a swipe over the Latin keys (`swipe::decode`).
    pub fn swipe_rules(&self) -> &crate::swipe::SkeletonRules {
        &self.swipe
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

    /// Every skeleton the romanised lexicon keys `word` by: `word_skeleton` first, then the
    /// skeleton of its shorter spelling (`romanize_short`) when it has one and it is another
    /// (नहीं: `nhin`, then `nhi`), then, with `dropTrailing.alsoKept`, the skeleton of its walk
    /// with the trailing letter kept when that lets `dropBetweenConsonants` drop the letter
    /// before it (जगह, `jagah`: `jga`, then `jgh`, which a finger going j-g-h draws; the
    /// first needs it back at `a` before the `h`). Only one of three letters or more: `kh`
    /// for कह would stand for every path from k to h. Empty for a word without a skeleton.
    pub fn word_skeletons(&self, word: &str) -> Vec<String> {
        let whole = self.word_skeleton(word);
        if whole.is_empty() {
            return Vec::new();
        }
        let short = self
            .romanize_short(word)
            .map(|latin| self.skeleton(&latin))
            .filter(|s| !s.is_empty() && *s != whole);
        let rules = &self.skeleton;
        let kept = (rules.trailing_also_kept && !rules.between.is_empty())
            .then(|| self.skeleton_with(&self.romanize(word), false))
            .filter(|s| *s != whole && s.len() <= whole.len() && s.len() >= 3);
        std::iter::once(whole).chain(short).chain(kept).collect()
    }
}

/// Malayalam's tables from `packs/ml.neopack`, for the engine's unit tests.
#[cfg(test)]
pub(crate) fn ml() -> &'static Romanisation {
    static ML: std::sync::OnceLock<Romanisation> = std::sync::OnceLock::new();
    ML.get_or_init(load_ml)
}

/// Hindi's tables from `packs/hi.neopack` (they name `optionalFinals`), for the engine's unit
/// tests.
#[cfg(test)]
pub(crate) fn hi() -> &'static Romanisation {
    static HI: std::sync::OnceLock<Romanisation> = std::sync::OnceLock::new();
    HI.get_or_init(|| {
        Romanisation::load(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/hi.neopack/romanisation.json"),
        )
        .unwrap()
    })
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
    fn a_final_sign_the_pack_calls_optional_gives_a_second_spelling() {
        let hi = pack("hi");
        for (word, whole, short) in [
            ("नहीं", "nahin", Some("nahi")),
            ("हूँ", "hun", Some("hu")),
            ("लोगों", "logon", Some("logo")),
            ("में", "men", Some("me")),
            ("दिन", "din", None),
            ("हिंदी", "hindi", None),
            ("ं", "n", None),
            ("स्वयं", "svayan", None),
            ("सं", "san", None),
            ("आँ", "an", Some("a")),
        ] {
            assert_eq!(hi.romanize(word), whole, "{word}");
            assert_eq!(hi.romanize_short(word).as_deref(), short, "{word}");
        }
        assert_eq!(hi.word_skeletons("नहीं"), ["nhin", "nhi"]);
        // `dropTrailing.alsoKept`: the h of jagah kept, the a before it is between consonants.
        assert_eq!(hi.word_skeletons("जगह"), ["jga", "jgh"]);
        assert_eq!(hi.word_skeletons("तरह"), ["tra", "trh"]);
        assert_eq!(hi.word_skeletons("कह"), ["ka"], "two letters would be any path from k to h");
        assert_eq!(hi.word_skeletons("तेरह"), ["tera", "terh"]);
        assert_eq!(hi.word_skeletons("वाह"), ["va"], "vah is longer: nothing more is dropped");
        assert_eq!(hi.word_skeletons("रखा"), ["rka"]);
        assert_eq!(hi.word_skeletons("दिन"), ["din"]);
        assert_eq!(hi.word_skeletons("नही"), ["nhi"]);
        assert!(hi.word_skeletons("्").is_empty());
        // A pack that names no such sign has one spelling and one skeleton for every word.
        let ml = pack("ml");
        assert_eq!(ml.romanize_short("കേരളം"), None);
        assert_eq!(ml.word_skeletons("കേരളം"), [ml.word_skeleton("കേരളം")]);
    }

    #[test]
    fn a_letter_written_for_the_dropped_vowel_gives_a_second_reading() {
        let hi = pack("hi");
        let readings = |typed: &str| hi.respellings(typed);
        let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
            list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
        };
        for (typed, skeleton, respelt) in [
            ("humse", "humse", &[("hmse", "hamse")][..]),
            ("hum", "hum", &[("hm", "ham")]),
            ("humara", "humra", &[("hmra", "hamara")]),
            ("kuch", "kuc", &[("kc", "kach")]),
            ("hamse", "hmse", &[]),
            ("tu", "tu", &[]),
            ("uttar", "utr", &[]),
            ("hui", "hui", &[]),
        ] {
            assert_eq!(hi.skeleton(typed), skeleton, "{typed}");
            assert_eq!(readings(typed), pairs(respelt), "{typed}");
        }
        assert_eq!(readings("humse")[0].0, hi.word_skeleton("हमसे"));
        // `alsoBesideConsonant`: an e stands for the dropped vowel only beside an h.
        for (typed, skeleton, respelt, written, word) in [
            ("kehte", "kehte", "khte", "kahte", "कहते"),
            ("rehna", "rehna", "rhna", "rahna", "रहना"),
            ("shehar", "sehr", "shr", "shahar", "शहर"),
            // After the h too, when the vowel before the h is the dropped one.
            ("behen", "behen", "bhn", "bahan", "बहन"),
            ("bahen", "bhen", "bhn", "bahan", "बहन"),
            ("sheher", "seher", "shr", "shahar", "शहर"),
            ("zeher", "jeher", "jhr", "zahar", "ज़हर"),
            ("pehchan", "pehcn", "phcn", "pahchan", "पहचान"),
        ] {
            assert_eq!(hi.skeleton(typed), skeleton, "{typed}");
            assert_eq!(readings(typed), pairs(&[(respelt, written)]), "{typed}");
            assert_eq!(respelt, hi.word_skeleton(word), "{typed}");
        }
        // With a u elsewhere: the e alone read so, then both.
        assert_eq!(readings("mehsoos"), pairs(&[("mhsus", "mahsoos"), ("mhss", "mahsoos")]));
        assert_eq!(readings("mehsus"), pairs(&[("mhsus", "mahsus"), ("mhss", "mahsas")]));
        assert_eq!(readings("mehsoos")[0].0, hi.word_skeleton("महसूस"));
        assert_eq!(readings("pehunch"), pairs(&[("phunc", "pahunch"), ("phnc", "pahanch")]));
        // Not an e away from an h, one whose h begins the word or follows a kept vowel, an
        // h that is the second letter of a pair (`khet`, `sher`), or an e at an end.
        for typed in ["her", "hello", "mohen", "khet", "sher", "leke", "dekh", "yeh", "ehsan", "pet"] {
            assert!(readings(typed).is_empty(), "{typed}");
        }
        // A word with a real े keeps its own skeleton; the other reading is beside it.
        assert_eq!(hi.word_skeleton("मेहनत"), hi.skeleton("mehnat"));
        assert_eq!(readings("mehnat"), pairs(&[("mhnt", "mahnat")]));
        // Tables that name no such letter read typed letters one way.
        let ml = pack("ml");
        assert!(ml.respellings("humse").is_empty());
    }

    #[test]
    fn a_letter_beside_a_consonant_is_a_letter_and_consonants() {
        let hi = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/hi.neopack/romanisation.json"),
        )
        .unwrap();
        let with = |beside: &str| {
            let mut file: serde_json::Value = serde_json::from_str(&hi).unwrap();
            file["skeleton"]
                .as_object_mut()
                .unwrap()
                .insert("alsoBesideConsonant".into(), serde_json::from_str(beside).unwrap());
            Romanisation::parse(&file.to_string())
        };
        let reading = |r: Romanisation, typed: &str| r.respellings(typed).into_iter().map(|(s, _)| s).collect::<Vec<_>>();
        assert_eq!(reading(with(r#"{"e": "h"}"#).unwrap(), "kehte"), ["khte"]);
        assert_eq!(reading(with(r#"{"e": "hy"}"#).unwrap(), "keyte"), ["kyte"]);
        // Without it only the u is read the other way.
        assert!(reading(with("{}").unwrap(), "kehte").is_empty());
        assert_eq!(reading(with("{}").unwrap(), "humse"), ["hmse"]);
        assert!(with(r#"{"E": "h"}"#).unwrap_err().contains("alsoBesideConsonant"));
        assert!(with(r#"{"e": "a"}"#).unwrap_err().contains("alsoBesideConsonant"));
        assert!(with(r#"{"e": "H"}"#).unwrap_err().contains("alsoBesideConsonant"));
        // A swipe may draw the letter there too (`swipe::SkeletonRules::also_beside`).
        let rules = with(r#"{"e": "h"}"#).unwrap();
        assert!(rules.swipe_rules().stood_in(b"khte", b"kehte"));
        assert!(!rules.swipe_rules().stood_in(b"kehte", b"kehte"));
        assert!(!with("{}").unwrap().swipe_rules().stood_in(b"khte", b"kehte"));
    }

    #[test]
    fn an_optional_final_must_be_a_final_with_latin() {
        let hi = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/hi.neopack/romanisation.json"),
        )
        .unwrap();
        let with = |list: &str| {
            let mut file: serde_json::Value = serde_json::from_str(&hi).unwrap();
            file.as_object_mut()
                .unwrap()
                .insert("optionalFinals".into(), serde_json::from_str(list).unwrap());
            Romanisation::parse(&file.to_string())
        };
        assert!(with(r#"["ं"]"#).is_ok());
        assert!(with("[]").unwrap().romanize_short("नहीं").is_none());
        assert!(with(r#"["क"]"#).unwrap_err().contains("not one of finals"));
        assert!(with(r#"["ऽ"]"#).unwrap_err().contains("empty"));
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

//! A pack language's native-script lexicon, from a language config (`data/langs/<id>/lang.json`)
//! instead of code: which Unicode block its words are in, which letters may start a word,
//! the replacements that make its text one spelling, and what ends a phrase. Used by
//! `lexbuild-native` for languages whose script the engine core has no rules for yet (the
//! Bengali-Assamese script of Assamese, say). Malayalam keeps its own path in `lib.rs`.
//!
//! Normal form: the engine's `normalize` (NFC, `’` to `'`, stray joiners dropped), with the
//! config's `replace` pairs applied first (an old khanda ta encoding, which would otherwise
//! lose its ZWJ) and again after. The romanised lookup keys are packed one byte per
//! character like `manglish.fst`'s (`pack`), from the block base.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead};
use std::path::Path;

use mlengine_core::lexicon::SEPARATOR;
use mlengine_core::normalize::normalize;
use serde::Deserialize;

use crate::{Counts, Entries, for_each_line};

const ZWNJ: char = '\u{200C}';
const ZWJ: char = '\u{200D}';

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConfig {
    id: String,
    script: String,
    block: [String; 2],
    #[serde(default)]
    not_word: Vec<[String; 2]>,
    first_letter: Vec<[String; 2]>,
    virama: String,
    #[serde(default)]
    signs: Vec<[String; 2]>,
    #[serde(default)]
    bases: Vec<[String; 2]>,
    #[serde(default)]
    apostrophe: bool,
    #[serde(default)]
    replace: Vec<[String; 2]>,
    #[serde(default)]
    phrase_breaks: Vec<String>,
    max_chars: usize,
    max_words: usize,
    min_count: u64,
}

/// A language config, parsed.
#[derive(Debug, Clone)]
pub struct Lang {
    pub id: String,
    pub script: String,
    pub block: (char, char),
    not_word: Vec<(char, char)>,
    first_letter: Vec<(char, char)>,
    pub virama: char,
    /// Vowel signs and the virama, which must follow a `bases` letter.
    signs: Vec<(char, char)>,
    bases: Vec<(char, char)>,
    /// Apostrophes may stand inside a word (Assamese হ'ল).
    pub apostrophe: bool,
    replace: Vec<(String, String)>,
    phrase_breaks: HashSet<char>,
    pub max_chars: usize,
    pub max_words: usize,
    pub min_count: u64,
}

fn in_ranges(ranges: &[(char, char)], c: char) -> bool {
    ranges.iter().any(|&(a, b)| (a..=b).contains(&c))
}

fn code_point(hex: &str) -> io::Result<char> {
    u32::from_str_radix(hex, 16)
        .ok()
        .and_then(char::from_u32)
        .ok_or_else(|| io::Error::other(format!("not a code point: {hex}")))
}

fn range(pair: &[String; 2]) -> io::Result<(char, char)> {
    Ok((code_point(&pair[0])?, code_point(&pair[1])?))
}

fn single_char(s: &str, what: &str) -> io::Result<char> {
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(c),
        _ => Err(io::Error::other(format!(
            "{what} must be one character: {s:?}"
        ))),
    }
}

impl Lang {
    pub fn from_json(text: &str) -> io::Result<Self> {
        let raw: RawConfig = serde_json::from_str(text).map_err(io::Error::other)?;
        let block = range(&raw.block)?;
        if (block.1 as u32) < (block.0 as u32) || block.1 as u32 - block.0 as u32 >= 0x80 {
            // `pack` gives each block character one byte, 0x80 and up.
            return Err(io::Error::other("block must be at most 128 code points"));
        }
        Ok(Self {
            id: raw.id,
            script: raw.script,
            block,
            not_word: raw.not_word.iter().map(range).collect::<io::Result<_>>()?,
            first_letter: raw
                .first_letter
                .iter()
                .map(range)
                .collect::<io::Result<_>>()?,
            virama: code_point(&raw.virama)?,
            signs: raw.signs.iter().map(range).collect::<io::Result<_>>()?,
            bases: raw.bases.iter().map(range).collect::<io::Result<_>>()?,
            apostrophe: raw.apostrophe,
            replace: raw.replace.into_iter().map(|[a, b]| (a, b)).collect(),
            phrase_breaks: raw
                .phrase_breaks
                .iter()
                .map(|s| single_char(s, "a phrase break"))
                .collect::<io::Result<_>>()?,
            max_chars: raw.max_chars,
            max_words: raw.max_words,
            min_count: raw.min_count,
        })
    }

    pub fn open(path: &Path) -> io::Result<Self> {
        let text = fs::read_to_string(path)
            .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
        Self::from_json(&text)
    }

    fn in_block(&self, c: char) -> bool {
        (self.block.0..=self.block.1).contains(&c)
    }

    /// A letter or sign of the language: in the block and not a digit or symbol.
    pub fn is_word_char(&self, c: char) -> bool {
        self.in_block(c) && !self.not_word.iter().any(|&(a, b)| (a..=b).contains(&c))
    }

    fn is_first_letter(&self, c: char) -> bool {
        in_ranges(&self.first_letter, c)
    }

    /// Every sign follows a base letter (a consonant or nukta), as in well-formed text.
    fn signs_follow_bases(&self, chars: &[char]) -> bool {
        chars.iter().enumerate().all(|(i, &c)| {
            !in_ranges(&self.signs, c) || (i > 0 && in_ranges(&self.bases, chars[i - 1]))
        })
    }

    pub fn is_phrase_break(&self, c: char) -> bool {
        self.phrase_breaks.contains(&c) || crate::is_phrase_break(c)
    }

    fn apply_replace(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (from, to) in &self.replace {
            if out.contains(from.as_str()) {
                out = out.replace(from.as_str(), to);
            }
        }
        out
    }

    /// The language's normal form of `text` (see the module comment).
    pub fn normalize(&self, text: &str) -> String {
        let before = self.apply_replace(&normalize(&self.apply_replace(text)));
        // A replacement can make text that NFC or the joiner rules would change again.
        let after = self.apply_replace(&normalize(&before));
        if after == before { before } else { after }
    }

    fn is_token_char(&self, c: char) -> bool {
        !self.is_phrase_break(c)
            && (c.is_alphanumeric() || self.in_block(c) || matches!(c, ZWNJ | ZWJ | '\''))
    }

    /// The tokens of a normalised line: runs of letters, the block, joiners and apostrophes,
    /// with apostrophes trimmed from the ends (as the engine's `text::words`).
    pub fn words<'a>(&self, line: &'a str) -> impl Iterator<Item = &'a str> {
        line.split(move |c: char| !self.is_token_char(c))
            .map(|w| w.trim_matches('\''))
            .filter(|w| !w.is_empty())
    }

    /// Whether a token is a word of the language: only its letters and signs (joiners, and
    /// one apostrophe inside if the language has them), starting with a letter, every sign
    /// after a base letter, not a lone consonant + virama, at most `max_chars` long.
    pub fn is_word(&self, word: &str) -> bool {
        let chars: Vec<char> = word.chars().collect();
        let apostrophes = chars.iter().filter(|&&c| c == '\'').count();
        let letters_only = chars.iter().all(|&c| {
            self.is_word_char(c) || matches!(c, ZWNJ | ZWJ) || (c == '\'' && self.apostrophe)
        });
        let starts = chars.first().is_some_and(|&c| self.is_first_letter(c));
        let lone_virama = chars.len() <= 2 && chars.last() == Some(&self.virama);
        letters_only
            && apostrophes <= 1
            && starts
            && self.signs_follow_bases(&chars)
            && !lone_virama
            && chars.len() <= self.max_chars
            && chars.last() != Some(&'\'')
    }
}

/// Counts every word of `lang` in the corpus, after normalising.
pub fn count_words(
    lang: &Lang,
    reader: impl BufRead,
    max_lines: Option<usize>,
) -> io::Result<Counts> {
    let mut counts = Counts::new();
    for_each_line(reader, max_lines, |line| {
        let line = lang.normalize(line);
        for word in lang.words(&line) {
            if lang.is_word(word) {
                *counts.entry(word.to_string()).or_default() += 1;
            }
        }
    })?;
    Ok(counts)
}

/// Reads a word list (one word per line), normalised; lines that aren't words of `lang` are skipped.
pub fn read_word_list(lang: &Lang, reader: impl BufRead) -> io::Result<HashSet<String>> {
    let mut set = HashSet::new();
    for line in reader.lines() {
        let line = line?;
        let word = lang.normalize(line.trim().trim_start_matches('\u{FEFF}'));
        if lang.is_word(&word) {
            set.insert(word);
        }
    }
    Ok(set)
}

/// The language's words: those seen at least `min_count` times, plus every listed word.
/// Sorted by count, highest first, at most `max_words`.
pub fn vocab(lang: &Lang, counts: &Counts, listed: &HashSet<String>) -> Vec<(String, u64)> {
    let mut vocab: HashMap<String, u64> = counts
        .iter()
        .filter(|(w, c)| lang.is_word(w) && (**c >= lang.min_count || listed.contains(*w)))
        .map(|(w, c)| (w.clone(), *c))
        .collect();
    for word in listed.iter().filter(|w| lang.is_word(w)) {
        vocab.entry(word.clone()).or_insert(1);
    }
    let mut vocab: Vec<(String, u64)> = vocab.into_iter().collect();
    crate::sort_and_cap(&mut vocab, lang.max_words);
    vocab
}

/// One byte per character: the block becomes 0x80 and up, ZWNJ 0x01, ZWJ 0x02, the
/// apostrophe 0x27. None if the word has any other character. `manglish::pack` with the
/// block base as a parameter.
pub fn pack(lang: &Lang, word: &str) -> Option<Vec<u8>> {
    word.chars()
        .map(|c| match c {
            ZWNJ => Some(0x01),
            ZWJ => Some(0x02),
            '\'' if lang.apostrophe => Some(b'\''),
            c if lang.in_block(c) => Some(0x80 + (c as u32 - lang.block.0 as u32) as u8),
            _ => None,
        })
        .collect()
}

/// The inverse of `pack`.
pub fn unpack(lang: &Lang, bytes: &[u8]) -> Option<String> {
    bytes
        .iter()
        .map(|&b| match b {
            0x01 => Some(ZWNJ),
            0x02 => Some(ZWJ),
            b'\'' if lang.apostrophe => Some('\''),
            0x80..=0xFF => char::from_u32(lang.block.0 as u32 + u32::from(b - 0x80)),
            _ => None,
        })
        .collect()
}

/// Romanised lookup entries (`skeleton\0packed-word` → count) from a TSV of
/// `skeleton<TAB>word<TAB>count` rows, as `training/mlkb/langs/romanised_keys.py` writes them.
/// Rows with an empty skeleton, a skeleton that isn't a–z, or a word that can't be packed are
/// skipped and counted.
pub fn romanised_entries(lang: &Lang, reader: impl BufRead) -> io::Result<(Entries, usize)> {
    let mut entries = Vec::new();
    let mut skipped = 0;
    for line in reader.lines() {
        let line = line?;
        let mut fields = line.split('\t');
        let (Some(skeleton), Some(word), Some(count)) =
            (fields.next(), fields.next(), fields.next())
        else {
            skipped += 1;
            continue;
        };
        let count: u64 = match count.trim().parse() {
            Ok(c) => c,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let word = lang.normalize(word);
        match pack(lang, &word) {
            Some(packed)
                if !skeleton.is_empty() && skeleton.bytes().all(|b| b.is_ascii_lowercase()) =>
            {
                let mut key = skeleton.as_bytes().to_vec();
                key.push(SEPARATOR);
                key.extend(packed);
                entries.push((key, count));
            }
            _ => skipped += 1,
        }
    }
    Ok((entries, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assamese() -> Lang {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/langs/as/lang.json");
        Lang::open(&path).unwrap()
    }

    /// The engine's beng plug-in normalises and classifies as this config built `as.fst`, so
    /// typed text meets the lexicon: on every char of the block, and on random strings of the
    /// chars the replacements and NFC act on.
    #[test]
    fn the_engine_beng_rules_match_the_config() {
        let lang = assamese();
        let rules = mlengine_core::script::rules("beng").unwrap();
        for c in '\u{0980}'..='\u{09FF}' {
            assert_eq!(rules.is_letter(c), lang.is_word_char(c), "{c:?}");
            let s = c.to_string();
            assert_eq!(rules.normalize(&s), lang.normalize(&s), "{c:?}");
        }
        let alphabet: Vec<char> =
            "অআকতমৰৱবযডঢৎািীুেোৌ়্ংঁঃ'’ \u{200C}\u{200D}\u{09DC}\u{09DD}\u{09DF}\u{09B0}"
                .chars()
                .collect();
        let mut seed: u64 = 20261107;
        for _ in 0..20_000 {
            let len = (seed >> 33) as usize % 9;
            let text: String = (0..len)
                .map(|_| {
                    seed = seed
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    alphabet[(seed >> 33) as usize % alphabet.len()]
                })
                .collect();
            assert_eq!(rules.normalize(&text), lang.normalize(&text), "{text:?}");
        }
    }

    #[test]
    fn assamese_letters_are_kept_and_bengali_ra_folds_to_them() {
        let lang = assamese();
        // ৰ U+09F0 and ৱ U+09F1 are kept; ব stays ব.
        assert_eq!(lang.normalize("ৰাজ্য ৱিকি বাট"), "ৰাজ্য ৱিকি বাট");
        // The Bengali ra is folded to the Assamese one.
        assert_eq!(lang.normalize("তোমার প্রথম"), "তোমাৰ প্ৰথম");
        assert!(!lang.normalize("ৰৱ").contains(['র', 'ব']));
    }

    #[test]
    fn old_khanda_ta_becomes_atomic() {
        let lang = assamese();
        assert_eq!(lang.normalize("উৎসৱ"), "উৎসৱ");
        assert_eq!(lang.normalize("উত\u{09CD}\u{200D}সৱ"), "উৎসৱ");
    }

    #[test]
    fn nukta_letters_are_one_spelling() {
        let lang = assamese();
        // U+09DF is a composition exclusion: NFC writes য় as য + nukta, whichever was typed.
        assert_eq!(
            lang.normalize("\u{09DF}"),
            lang.normalize("\u{09AF}\u{09BC}")
        );
        assert_eq!(lang.normalize("\u{09DF}"), "\u{09AF}\u{09BC}");
    }

    #[test]
    fn words_keep_viramas_and_inner_apostrophes() {
        let lang = assamese();
        let line = lang.normalize("মই কৰিছোঁ। হ’ল, ক'ত 'অসম' ২০২০ চন ৷ ab ক্ষমা");
        let words: Vec<&str> = lang.words(&line).filter(|w| lang.is_word(w)).collect();
        assert_eq!(words, ["মই", "কৰিছোঁ", "হ'ল", "ক'ত", "অসম", "চন", "ক্ষমা"]);
    }

    #[test]
    fn signs_and_lone_viramas_are_not_words() {
        let lang = assamese();
        assert!(!lang.is_word("ি"));
        assert!(!lang.is_word("ন্"));
        assert!(!lang.is_word("২০২০"));
        assert!(!lang.is_word("কa"));
        assert!(lang.is_word("ৰ"));
        // A sign after a vowel or another sign is mis-encoded text.
        assert!(!lang.is_word("আু"));
        assert!(!lang.is_word("অৰুি"));
        assert!(lang.is_word("খাওঁ"));
        assert!(lang.is_word("কোনো"));
    }

    #[test]
    fn visual_order_o_is_mended() {
        let lang = assamese();
        // া + ে, the halves of ো typed in the order they are drawn.
        assert_eq!(lang.normalize("মােৰ"), "মোৰ");
        assert_eq!(lang.normalize("অাৰু"), "আৰু");
        assert!(lang.is_word(&lang.normalize("মােৰ")));
    }

    #[test]
    fn packing_round_trips() {
        let lang = assamese();
        for word in ["অসমীয়া", "হ'ল", "ৰৱ", "ক্ষ\u{200C}"] {
            let packed = pack(&lang, word).unwrap();
            assert_eq!(packed.len(), word.chars().count());
            assert!(!packed.contains(&SEPARATOR));
            assert_eq!(unpack(&lang, &packed).unwrap(), word);
        }
        assert_eq!(pack(&lang, "অa"), None);
    }

    #[test]
    fn counts_and_vocab() {
        let lang = assamese();
        let text = "মই ভাত খাওঁ।\nমই ভাত খাওঁ\nমই ঘৰলৈ যাওঁ। মই\n";
        let counts = count_words(&lang, text.as_bytes(), None).unwrap();
        assert_eq!(counts.get("মই"), Some(&4));
        let listed: HashSet<String> = ["অসম".to_string()].into();
        let v = vocab(&lang, &counts, &listed);
        assert_eq!(v, [("মই".to_string(), 4), ("অসম".to_string(), 1)]);
    }

    #[test]
    fn romanised_rows_become_keys() {
        let lang = assamese();
        let tsv = "moi\tমই\t10\n\tমই\t3\nbad\tab\t1\nOK\tমই\t1\n";
        let (entries, skipped) = romanised_entries(&lang, tsv.as_bytes()).unwrap();
        assert_eq!(skipped, 3);
        let mut want = b"moi\0".to_vec();
        want.extend(pack(&lang, "মই").unwrap());
        assert_eq!(entries, [(want, 10)]);
    }
}

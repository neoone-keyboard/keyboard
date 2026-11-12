//! A pack language's native-script lexicon, from a language config (`data/langs/<id>/lang.json`)
//! instead of code: which characters its words are made of, which letters may start a word,
//! the replacements that make its text one spelling, and what ends a phrase. Used by
//! `lexbuild-native`. Malayalam keeps its own path in `lib.rs`.
//!
//! The script half of the config (characters, signs, bases, virama, replacements, phrase
//! breaks, apostrophes) is parsed by the engine core (`mlengine_core::script::data::ScriptDef`),
//! which reads the same file from a pack's `lang` role at run time: one definition for the
//! builder and the keyboard. This module adds what only the builder needs (`id`, `firstLetter`,
//! `maxChars`, `maxWords`, `minCount`).
//!
//! Normal form: `ScriptDef::normalize` (the config's `replace` pairs, the engine's `normalize`,
//! the pairs again). The romanised lookup keys are packed one byte per character like
//! `manglish.fst`'s (`pack`), by the script's packing: from the first code point of the script,
//! which needs a script of at most 128 code points, or by its `charTable`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead};
use std::path::Path;

use mlengine_core::lexicon::SEPARATOR;
use mlengine_core::packing::Packing;
use mlengine_core::script::data::ScriptDef;
use serde::Deserialize;

use crate::{Counts, Entries, for_each_line};

const ZWNJ: char = '\u{200C}';
const ZWJ: char = '\u{200D}';

/// The builder's own keys of `lang.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawConfig {
    id: String,
    #[serde(default)]
    first_letter: Vec<[String; 2]>,
    max_chars: usize,
    max_words: usize,
    min_count: u64,
}

/// A language config, parsed.
#[derive(Debug, Clone)]
pub struct Lang {
    pub id: String,
    /// The script, as the engine reads it from a pack's `lang` file.
    pub def: ScriptDef,
    /// Letters a word may start with; empty means any letter of the script.
    first_letter: Vec<(char, char)>,
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

impl Lang {
    pub fn from_json(text: &str) -> io::Result<Self> {
        let def = ScriptDef::from_json(text).map_err(io::Error::other)?;
        let raw: RawConfig = serde_json::from_str(text).map_err(io::Error::other)?;
        Ok(Self {
            id: raw.id,
            def,
            first_letter: raw
                .first_letter
                .iter()
                .map(range)
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
            .map_err(|e| io::Error::other(format!("{}: {e}", path.display())))
    }

    /// The script code ("beng").
    pub fn script(&self) -> &str {
        &self.def.code
    }

    /// How `pack` packs the script's characters (`ScriptDef::packing`).
    pub fn packing(&self) -> Packing {
        self.def.packing()
    }

    fn in_block(&self, c: char) -> bool {
        self.def.in_script(c)
    }

    /// A letter or sign of the language: in the script and not a digit or symbol.
    pub fn is_word_char(&self, c: char) -> bool {
        self.def.is_word_char(c)
    }

    fn is_first_letter(&self, c: char) -> bool {
        if self.first_letter.is_empty() {
            self.is_word_char(c) && !self.def.is_sign(c)
        } else {
            in_ranges(&self.first_letter, c)
        }
    }

    pub fn is_phrase_break(&self, c: char) -> bool {
        self.def.is_phrase_break(c) || crate::is_phrase_break(c)
    }

    /// The language's normal form of `text` (see the module comment).
    pub fn normalize(&self, text: &str) -> String {
        self.def.normalize(text)
    }

    fn is_token_char(&self, c: char) -> bool {
        !self.is_phrase_break(c)
            && (c.is_alphanumeric() || self.in_block(c) || matches!(c, ZWNJ | ZWJ | '\''))
    }

    /// The tokens of a normalised line: runs of letters, the script, joiners and apostrophes,
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
            self.is_word_char(c) || matches!(c, ZWNJ | ZWJ) || (c == '\'' && self.def.apostrophe)
        });
        let starts = chars.first().is_some_and(|&c| self.is_first_letter(c));
        let lone_virama =
            self.def.virama.is_some() && chars.len() <= 2 && chars.last() == self.def.virama.as_ref();
        letters_only
            && apostrophes <= 1
            && starts
            && self.def.signs_follow_bases(&chars)
            && !lone_virama
            && chars.len() <= self.max_chars
            && chars.last() != Some(&'\'')
    }

    /// The script fits `pack`'s one byte per character: it has a `charTable`, or spans at
    /// most 128 code points.
    fn packable(&self) -> io::Result<()> {
        let span = self.def.span();
        if self.def.char_table.is_none() && *span.end() as u32 - *span.start() as u32 >= 0x80 {
            return Err(io::Error::other(
                "romanised keys pack one byte per character: a script that spans more than 128 \
                 code points lists its characters in charTable",
            ));
        }
        Ok(())
    }
}

/// Counts every word of `lang` in the corpus, after normalising.
pub fn count_words(
    lang: &Lang,
    reader: impl BufRead,
    max_lines: Option<usize>,
) -> io::Result<Counts> {
    Ok(count_tokens(lang, reader, max_lines)?.0)
}

/// `count_words`, and beside it the tokens that were dropped: those with a letter of the
/// script that still aren't words of it (a sign after a vowel, a sign first, a foreign letter
/// inside, too long). The frequent ones are usually one mis-encoding repeated, which a
/// `replace` pair in the config would mend; the builder reports them for that.
pub fn count_tokens(
    lang: &Lang,
    reader: impl BufRead,
    max_lines: Option<usize>,
) -> io::Result<(Counts, Counts)> {
    let mut counts = Counts::new();
    let mut dropped = Counts::new();
    for_each_line(reader, max_lines, |line| {
        let line = lang.normalize(line);
        for word in lang.words(&line) {
            if lang.is_word(word) {
                *counts.entry(word.to_string()).or_default() += 1;
            } else if word.chars().any(|c| lang.is_word_char(c)) {
                *dropped.entry(word.to_string()).or_default() += 1;
            }
        }
    })?;
    Ok((counts, dropped))
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

/// One byte per character (`mlengine_core::manglish::pack` by the script's `packing`): the
/// script's characters become 0x80 and up, ZWNJ 0x01, ZWJ 0x02, the apostrophe 0x27 where the
/// language writes one inside words. None if the word has any other character.
pub fn pack(lang: &Lang, word: &str) -> Option<Vec<u8>> {
    let foreign = word
        .chars()
        .any(|c| !matches!(c, ZWNJ | ZWJ) && !lang.in_block(c) && (c != '\'' || !lang.def.apostrophe));
    if foreign {
        return None;
    }
    lang.packing().pack(word)
}

/// The inverse of `pack`.
pub fn unpack(lang: &Lang, bytes: &[u8]) -> Option<String> {
    if bytes.contains(&b'\'') && !lang.def.apostrophe {
        return None;
    }
    lang.packing().unpack(bytes)
}

/// Romanised lookup entries (`skeleton\0packed-word` → count) from a TSV of
/// `skeleton<TAB>word<TAB>count` rows, as `training/mlkb/langs/romanised_keys.py` writes them.
/// Rows with an empty skeleton, a skeleton that isn't a–z, or a word that can't be packed are
/// skipped and counted.
pub fn romanised_entries(lang: &Lang, reader: impl BufRead) -> io::Result<(Entries, usize)> {
    lang.packable()?;
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

    /// The engine's rules for an Assamese pack normalise and classify as this config built
    /// `as.fst`, so typed text meets the lexicon: the engine's own copy (a pack without a
    /// `lang` file) and the ones made from the pack's file are one definition. On every char
    /// of the block, and on random strings of the chars the replacements and NFC act on.
    #[test]
    fn the_engine_rules_for_assamese_match_the_config() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/langs/as/lang.json");
        let own = mlengine_core::script::for_pack("beng", None).unwrap();
        let data = mlengine_core::script::for_pack("beng", Some(&path)).unwrap();
        assert!(std::ptr::addr_eq(own, data), "the engine's copy is this file");
        check_rules(data);
    }

    fn check_rules(rules: &dyn mlengine_core::script::ScriptRules) {
        let lang = assamese();
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
    fn dropped_tokens_are_the_scripts_non_words() {
        let lang = assamese();
        // আু has a sign after a vowel, কa a Latin letter; hello and ২০২০ have no letter of the
        // script, so they are not the script's to drop.
        let text = "মই আু আু কa hello ২০২০\n";
        let (counts, dropped) = count_tokens(&lang, text.as_bytes(), None).unwrap();
        assert_eq!(counts.len(), 1);
        assert_eq!(dropped.get("আু"), Some(&2));
        assert_eq!(dropped.get("কa"), Some(&1));
        assert_eq!(dropped.len(), 2);
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

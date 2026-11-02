//! Builds the keyboard's lexicon bundle from plain-text corpora (one
//! paragraph per line). Uses the engine's own tokeniser, normaliser and key
//! rules, so build-time and type-time can't drift apart.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, BufRead};
use std::path::Path;

use mlengine_core::lexicon::{SEPARATOR, build};
use mlengine_core::normalize::normalize;
use mlengine_core::text::{Script, script, words};

pub type Counts = HashMap<String, u64>;
/// FST input: key bytes and a count.
pub type Entries = Vec<(Vec<u8>, u64)>;

#[derive(Debug, Clone)]
pub struct Config {
    /// Most English words kept.
    pub english_max: usize,
    /// Most Malayalam forms kept.
    pub malayalam_max: usize,
    /// A word must occur this often to be kept (unless a word list vouches for it).
    pub min_count: u64,
    /// An English word not in SCOWL must occur this often (names, new words).
    pub english_unlisted_min: u64,
    /// Next-word guesses are stored for this many of the most common words.
    pub next_word_max: usize,
    /// Next-word guesses stored per word.
    pub next_per_word: usize,
    /// Longest English word kept, in bytes (the engine corrects up to 24).
    pub max_english_len: usize,
    /// Longest Malayalam word kept, in chars.
    pub max_malayalam_chars: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            english_max: 150_000,
            malayalam_max: 300_000,
            min_count: 3,
            english_unlisted_min: 2_000,
            next_word_max: 50_000,
            next_per_word: 3,
            max_english_len: 24,
            max_malayalam_chars: 30,
        }
    }
}

/// The key a word is counted and looked up under: ASCII-lowercased for English, as-is for Malayalam.
pub fn lookup_key(word: &str) -> String {
    word.to_ascii_lowercase()
}

/// Counts every word of `want` script in the corpus, by surface form, after normalising.
pub fn count_words(
    reader: impl BufRead,
    want: Script,
    max_lines: Option<usize>,
) -> io::Result<Counts> {
    let mut counts = Counts::new();
    for (n, line) in reader.lines().enumerate() {
        if max_lines.is_some_and(|max| n >= max) {
            break;
        }
        let line = normalize(&line?);
        for word in words(&line) {
            if script(word) == want {
                *counts.entry(word.to_string()).or_default() += 1;
            }
        }
    }
    Ok(counts)
}

/// Reads a word list (one word per line, any BOM or whitespace trimmed), normalised.
pub fn read_word_list(reader: impl BufRead) -> io::Result<HashSet<String>> {
    let mut set = HashSet::new();
    for line in reader.lines() {
        let line = line?;
        let word = line.trim().trim_start_matches('\u{FEFF}');
        if !word.is_empty() {
            set.insert(normalize(word));
        }
    }
    Ok(set)
}

/// English words: surface forms merged by lowercase, filtered and capped.
/// Sorted by count, highest first. Each keeps one spelling (see `display_form`).
pub fn english_vocab(
    counts: &Counts,
    scowl: &HashSet<String>,
    config: &Config,
) -> Vec<(String, u64)> {
    let scowl_lower: HashSet<String> = scowl.iter().map(|w| lookup_key(w)).collect();
    let mut forms: HashMap<String, Vec<(&str, u64)>> = HashMap::new();
    for (form, &count) in counts {
        if form.len() <= config.max_english_len && form.matches('\'').count() <= 1 {
            forms
                .entry(lookup_key(form))
                .or_default()
                .push((form, count));
        }
    }
    let mut vocab: Vec<(String, u64)> = forms
        .into_iter()
        .filter_map(|(key, forms)| {
            let total: u64 = forms.iter().map(|(_, c)| c).sum();
            let listed = scowl_lower.contains(&key);
            let keep =
                (listed && total >= config.min_count) || total >= config.english_unlisted_min;
            keep.then(|| (display_form(&key, &forms, total, scowl), total))
        })
        .collect();
    sort_and_cap(&mut vocab, config.english_max);
    vocab
}

/// Which spelling a word is shown in. Wikipedia capitalises sentence starts
/// and titles ("Hello, Dolly!"), so the lowercase form wins whenever SCOWL
/// lists it and it is at least 5% of uses. Otherwise SCOWL's spelling wins
/// (London, I), then the most common one. Ties go to the alphabetically first.
fn display_form(key: &str, forms: &[(&str, u64)], total: u64, scowl: &HashSet<String>) -> String {
    if key == "i" {
        return "I".to_string(); // the pronoun, though "i.e." and Roman numerals say otherwise
    }
    let lowercase = forms.iter().find(|(f, _)| *f == key).map_or(0, |(_, c)| *c);
    if scowl.contains(key) && lowercase * 20 >= total {
        return key.to_string();
    }
    let most_common = |candidates: &mut dyn Iterator<Item = &(&str, u64)>| {
        candidates
            .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(a.0)))
            .map(|(f, _)| f.to_string())
    };
    most_common(&mut forms.iter().filter(|(f, _)| scowl.contains(*f)))
        .or_else(|| most_common(&mut forms.iter()))
        .unwrap_or_else(|| key.to_string())
}

/// A Malayalam word must start with a vowel or consonant (not a sign or
/// chillu left over from "2020ൽ"), and not be a lone consonant + virama ("ന്").
fn plausible_malayalam(word: &str) -> bool {
    let chars: Vec<char> = word.chars().collect();
    let starts_with_letter = chars
        .first()
        .is_some_and(|c| ('\u{0D05}'..='\u{0D3A}').contains(c));
    let lone_virama = chars.len() <= 2 && chars.last() == Some(&'\u{0D4D}');
    starts_with_letter && !lone_virama
}

/// Malayalam forms: frequent ones, plus every SMC word-list entry (names,
/// places, lemmas) even if the corpus never used it. Sorted by count, highest first.
pub fn malayalam_vocab(
    counts: &Counts,
    smc: &HashSet<String>,
    config: &Config,
) -> Vec<(String, u64)> {
    let ok = |w: &str| {
        script(w) == Script::Malayalam
            && plausible_malayalam(w)
            && w.chars().count() <= config.max_malayalam_chars
    };
    let mut vocab: HashMap<String, u64> = counts
        .iter()
        .filter(|(w, c)| ok(w) && (**c >= config.min_count || smc.contains(*w)))
        .map(|(w, c)| (w.clone(), *c))
        .collect();
    for word in smc.iter().filter(|w| ok(w)) {
        vocab.entry(word.clone()).or_insert(1);
    }
    let mut vocab: Vec<(String, u64)> = vocab.into_iter().collect();
    sort_and_cap(&mut vocab, config.malayalam_max);
    vocab
}

fn sort_and_cap(vocab: &mut Vec<(String, u64)>, max: usize) {
    vocab.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    vocab.truncate(max);
}

/// Counts which word follows which, inside a line, for the `next_word_max`
/// most common words of `vocab`. Only words in `vocab` are counted.
pub struct NextWords {
    ids: HashMap<String, u32>,
    forms: Vec<String>,
    previous_allowed: usize,
    pairs: HashMap<(u32, u32), u64>,
}

impl NextWords {
    /// `vocab` must be sorted by count, highest first (as the vocab functions return it).
    pub fn new(vocab: &[(String, u64)], config: &Config) -> Self {
        let mut ids = HashMap::new();
        let mut forms = Vec::new();
        for (form, _) in vocab {
            if let std::collections::hash_map::Entry::Vacant(slot) = ids.entry(lookup_key(form)) {
                slot.insert(forms.len() as u32);
                forms.push(form.clone());
            }
        }
        Self {
            ids,
            forms,
            previous_allowed: config.next_word_max,
            pairs: HashMap::new(),
        }
    }

    pub fn add_corpus(&mut self, reader: impl BufRead, max_lines: Option<usize>) -> io::Result<()> {
        for (n, line) in reader.lines().enumerate() {
            if max_lines.is_some_and(|max| n >= max) {
                break;
            }
            let line = normalize(&line?);
            // A pair never spans punctuation: "i.e." is not "i" followed by "e".
            for phrase in line.split(is_phrase_break) {
                let mut previous: Option<u32> = None;
                for word in words(phrase) {
                    let id = self.ids.get(&lookup_key(word)).copied();
                    if let (Some(p), Some(next)) = (previous, id)
                        && (p as usize) < self.previous_allowed
                    {
                        *self.pairs.entry((p, next)).or_default() += 1;
                    }
                    previous = id;
                }
            }
        }
        Ok(())
    }

    /// FST entries `previous\0next` → count, keeping the best `per_word` for each previous word.
    pub fn entries(&self, per_word: usize) -> Entries {
        let mut by_previous: HashMap<u32, Vec<(u32, u64)>> = HashMap::new();
        for (&(p, n), &count) in &self.pairs {
            by_previous.entry(p).or_default().push((n, count));
        }
        let mut entries = Vec::new();
        for (p, mut nexts) in by_previous {
            nexts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            for (n, count) in nexts.into_iter().take(per_word) {
                let mut key = lookup_key(&self.forms[p as usize]).into_bytes();
                key.push(SEPARATOR);
                key.extend_from_slice(self.forms[n as usize].as_bytes());
                entries.push((key, count));
            }
        }
        entries
    }
}

fn is_phrase_break(c: char) -> bool {
    matches!(
        c,
        '.' | ','
            | ';'
            | ':'
            | '?'
            | '!'
            | '('
            | ')'
            | '['
            | ']'
            | '"'
            | '\u{201C}'
            | '\u{201D}'
            | '\u{2013}'
            | '\u{2014}'
    )
}

fn to_entries(vocab: &[(String, u64)]) -> Entries {
    vocab
        .iter()
        .map(|(w, c)| (w.as_bytes().to_vec(), *c))
        .collect()
}

/// What `write_bundle` produced, written to `stats.txt` next to the files.
#[derive(Debug, Default)]
pub struct Stats {
    pub report: String,
}

/// Writes `en.fst`, `ml.fst` and `next.fst` into `out`, plus `stats.txt`.
pub fn write_bundle(
    out: &Path,
    english: &[(String, u64)],
    malayalam: &[(String, u64)],
    next: &[(Vec<u8>, u64)],
) -> io::Result<Stats> {
    fs::create_dir_all(out)?;
    let mut report = String::new();
    let files: [(&str, Entries, usize); 3] = [
        ("en.fst", to_entries(english), english.len()),
        ("ml.fst", to_entries(malayalam), malayalam.len()),
        ("next.fst", next.to_vec(), next.len()),
    ];
    for (name, entries, n) in files {
        let bytes = build(entries).map_err(io::Error::other)?;
        fs::write(out.join(name), &bytes)?;
        let _ = writeln!(report, "{name}: {n} keys, {} bytes", bytes.len());
    }
    for (label, vocab) in [("English", english), ("Malayalam", malayalam)] {
        let top: Vec<&str> = vocab.iter().take(20).map(|(w, _)| w.as_str()).collect();
        let _ = writeln!(report, "{label} top 20: {}", top.join(" "));
    }
    fs::write(out.join("stats.txt"), &report)?;
    Ok(Stats { report })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(text: &str, want: Script) -> Counts {
        count_words(text.as_bytes(), want, None).unwrap()
    }

    fn set(words: &[&str]) -> HashSet<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn counts_words_of_one_script() {
        let c = counts("The cat and the dog. എന്റെ cat 42", Script::Latin);
        assert_eq!(c.get("the"), Some(&1));
        assert_eq!(c.get("The"), Some(&1));
        assert_eq!(c.get("cat"), Some(&2));
        assert_eq!(c.get("42"), None);
        assert_eq!(c.get("എന്റെ"), None);
        let m = counts("എന്റെ വീട് എന്റെ cat", Script::Malayalam);
        assert_eq!(m.get("എന്റെ"), Some(&2));
    }

    #[test]
    fn counting_normalises_old_chillus() {
        let m = counts("അവന\u{0D4D}\u{200D} അവൻ", Script::Malayalam);
        assert_eq!(m.get("അവൻ"), Some(&2));
    }

    #[test]
    fn english_merges_case_and_picks_one_spelling() {
        let c = counts(
            "the the The London London london I i i Hello Hello hello Zork zork zork",
            Script::Latin,
        );
        let config = Config {
            min_count: 1,
            english_unlisted_min: 3,
            ..Config::default()
        };
        let vocab = english_vocab(&c, &set(&["the", "London", "I", "i", "hello"]), &config);
        let mut got: Vec<&str> = vocab.iter().map(|(w, _)| w.as_str()).collect();
        got.sort();
        // hello: listed in lowercase and used in lowercase, despite Hello being more common.
        // London: SCOWL's spelling. I: always the pronoun. zork: unlisted, most common form.
        assert_eq!(got, ["I", "London", "hello", "the", "zork"]);
        assert!(
            vocab.iter().all(|(_, c)| *c == 3),
            "counts are summed across spellings"
        );
    }

    #[test]
    fn english_unlisted_words_need_to_be_frequent() {
        let c = counts(&"Jishnu ".repeat(5), Script::Latin);
        let rare = Config {
            min_count: 1,
            english_unlisted_min: 10,
            ..Config::default()
        };
        assert!(english_vocab(&c, &HashSet::new(), &rare).is_empty());
        let common = Config {
            english_unlisted_min: 5,
            ..rare
        };
        assert_eq!(english_vocab(&c, &HashSet::new(), &common).len(), 1);
    }

    #[test]
    fn malayalam_keeps_frequent_and_listed_words() {
        let c = counts("എന്റെ എന്റെ എന്റെ വീട് ി ൽ ൽ ൽ ന് ന് ന്", Script::Malayalam);
        let config = Config {
            min_count: 2,
            ..Config::default()
        };
        let vocab = malayalam_vocab(&c, &set(&["വീട്", "കൊച്ചി"]), &config);
        assert_eq!(
            vocab,
            [
                ("എന്റെ".to_string(), 3),
                ("കൊച്ചി".to_string(), 1),
                ("വീട്".to_string(), 1)
            ]
        );
    }

    #[test]
    fn caps_keep_the_most_common() {
        let c = counts("a a a b b c", Script::Latin);
        let config = Config {
            english_max: 2,
            min_count: 1,
            ..Config::default()
        };
        let vocab = english_vocab(&c, &set(&["a", "b", "c"]), &config);
        assert_eq!(
            vocab.iter().map(|(w, _)| w.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
    }

    #[test]
    fn next_words_keep_the_top_followers() {
        let text = "I am here\nI am there\nI have it\nI was\nI will\nI will\ni.e. i.e. i.e.";
        let vocab: Vec<(String, u64)> =
            ["I", "am", "have", "was", "will", "here", "there", "it", "e"]
                .iter()
                .map(|w| (w.to_string(), 1))
                .collect();
        let mut next = NextWords::new(&vocab, &Config::default());
        next.add_corpus(text.as_bytes(), None).unwrap();
        let mut entries = next.entries(2);
        entries.sort();
        let i_entries: Vec<_> = entries
            .iter()
            .filter(|(k, _)| k.starts_with(b"i\0"))
            .collect();
        assert_eq!(
            i_entries,
            [&(b"i\0am".to_vec(), 2), &(b"i\0will".to_vec(), 2)]
        );
    }

    #[test]
    fn word_lists_trim_bom_and_blank_lines() {
        let list = read_word_list("\u{FEFF}അക്കാമ്മ\n\n  കൊച്ചി \n".as_bytes()).unwrap();
        assert_eq!(list, set(&["അക്കാമ്മ", "കൊച്ചി"]));
    }
}

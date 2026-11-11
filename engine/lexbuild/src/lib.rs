//! Builds the keyboard's lexicon bundle from plain-text corpora (one
//! paragraph per line). Uses the engine's own tokeniser, normaliser and key
//! rules, so build-time and type-time can't drift apart.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, BufRead};
use std::path::Path;

pub mod native;
pub mod romanisation;

use mlengine_core::lexicon::{SEPARATOR, build, build_with_aliases};
use mlengine_core::manglish;
use mlengine_core::normalize::normalize;
use mlengine_core::text::{Script, script, words};

use crate::romanisation::Table;

/// Longest English token counted, in bytes (the engine corrects up to 24).
pub const MAX_ENGLISH_LEN: usize = 24;
/// Longest Malayalam token counted, in chars.
pub const MAX_NATIVE_CHARS: usize = 30;
/// `NextWords` drops pairs seen once when it holds more pairs than this.
pub const PAIR_PRUNE_THRESHOLD: usize = 20_000_000;

/// Calls `f` with each valid UTF-8 line (up to `max_lines` lines read). Lines
/// that aren't valid UTF-8 are skipped, not fatal.
pub(crate) fn for_each_line(
    mut reader: impl BufRead,
    max_lines: Option<usize>,
    mut f: impl FnMut(&str),
) -> io::Result<()> {
    let mut buf = Vec::new();
    let mut n = 0;
    loop {
        if max_lines.is_some_and(|max| n >= max) {
            return Ok(());
        }
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            return Ok(());
        }
        n += 1;
        if let Ok(line) = std::str::from_utf8(&buf) {
            f(line.trim_end_matches(['\n', '\r']));
        }
    }
}

fn token_fits(word: &str, want: Script) -> bool {
    match want {
        Script::MALAYALAM => word.chars().count() <= MAX_NATIVE_CHARS,
        _ => word.len() <= MAX_ENGLISH_LEN,
    }
}

pub type Counts = HashMap<String, u64>;
/// FST input: key bytes and a count.
pub type Entries = Vec<(Vec<u8>, u64)>;

#[derive(Debug, Clone)]
pub struct Config {
    /// Most English words kept.
    pub english_max: usize,
    /// Most Malayalam forms kept.
    pub malayalam_max: usize,
    /// A word must occur this often to be kept. In English this applies to SCOWL
    /// words too; in Malayalam an SMC word list entry is kept regardless.
    pub min_count: u64,
    /// An English word not in SCOWL must occur this often (names, new words).
    pub english_unlisted_min: u64,
    /// Next-word guesses are stored for this many of the most common words.
    pub next_word_max: usize,
    /// Next-word guesses stored per word.
    pub next_per_word: usize,
    /// Longest English word kept, in bytes (the engine corrects up to 24).
    pub max_english_len: usize,
    /// Longest native word kept, in chars: Malayalam's, and a `--lang` pack's unless its
    /// build says otherwise (`lexbuild --max-chars`).
    pub max_native_chars: usize,
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
            max_english_len: MAX_ENGLISH_LEN,
            max_native_chars: MAX_NATIVE_CHARS,
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
    for_each_line(reader, max_lines, |line| {
        let line = normalize(line);
        for word in words(&line) {
            if script(word) == want && token_fits(word, want) {
                *counts.entry(word.to_string()).or_default() += 1;
            }
        }
    })?;
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

/// Reads a hand-written word list: one word per line, blank lines and lines
/// starting with `#` skipped, normalised.
pub fn read_hand_list(reader: impl BufRead) -> io::Result<HashSet<String>> {
    let mut set = HashSet::new();
    for line in reader.lines() {
        let line = line?;
        let word = line.trim();
        if !word.is_empty() && !word.starts_with('#') {
            set.insert(normalize(word));
        }
    }
    Ok(set)
}

/// English words: surface forms merged by lowercase, filtered and capped.
/// Sorted by count, highest first. Each keeps one spelling (see `display_form`).
///
/// `extra` is a hand-written list (`data/indian-english.txt`): words of Indian
/// English that Wikipedia uses too rarely to pass `english_unlisted_min`
/// (Vishu, biryani, PNR). Each is kept whatever its count (at least 1, like an
/// SMC entry in Malayalam) and shown in the list's spelling, unless it is a
/// word already (`PAN` is the English `pan`, which keeps its own spelling).
pub fn english_vocab(
    counts: &Counts,
    scowl: &HashSet<String>,
    extra: &HashSet<String>,
    config: &Config,
) -> Vec<(String, u64)> {
    let scowl_lower: HashSet<String> = scowl.iter().map(|w| lookup_key(w)).collect();
    let extra_lower: HashMap<String, &str> =
        extra.iter().map(|w| (lookup_key(w), w.as_str())).collect();
    let mut forms: HashMap<String, Vec<(&str, u64)>> = HashMap::new();
    for (form, &count) in counts {
        if form.len() <= config.max_english_len && form.matches('\'').count() <= 1 {
            forms
                .entry(lookup_key(form))
                .or_default()
                .push((form, count));
        }
    }
    // A listed word Wikipedia never uses is still kept.
    for key in extra_lower.keys() {
        forms.entry(key.clone()).or_default();
    }
    let mut vocab: Vec<(String, u64)> = forms
        .into_iter()
        .filter_map(|(key, forms)| {
            let total: u64 = forms.iter().map(|(_, c)| c).sum();
            let listed = scowl_lower.contains(&key);
            let keep =
                (listed && total >= config.min_count) || total >= config.english_unlisted_min;
            if keep {
                // Already a word: the list never respells it (`pan` stays, not `PAN`).
                Some((display_form(&key, &forms, total, scowl), total))
            } else {
                extra_lower
                    .get(&key)
                    .map(|word| (word.to_string(), total.max(1)))
            }
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
        script(w) == Script::MALAYALAM
            && plausible_malayalam(w)
            && w.chars().count() <= config.max_native_chars
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

pub(crate) fn sort_and_cap(vocab: &mut Vec<(String, u64)>, max: usize) {
    vocab.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    vocab.truncate(max);
}

/// Counts which word follows which, inside a line, for the `next_word_max`
/// most common words of `vocab`. Only words in `vocab` are counted.
pub struct NextWords {
    ids: HashMap<String, u32>,
    forms: Vec<String>,
    previous_allowed: usize,
    pairs: HashMap<(u32, u32), PairCount>,
    prune_at: usize,
    /// A pack language's normal form, words and phrase breaks (`native`), in place of the
    /// engine's English and Malayalam ones.
    lang: Option<native::Lang>,
}

/// How often a pair was seen, and how often the second word had a capital there.
#[derive(Debug, Default, Clone, Copy)]
struct PairCount {
    count: u32,
    capitalised: u32,
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
            prune_at: PAIR_PRUNE_THRESHOLD,
            lang: None,
        }
    }

    /// Reads the corpus as text of `lang`: its normal form, its words, its phrase breaks.
    pub fn with_lang(mut self, lang: native::Lang) -> Self {
        self.lang = Some(lang);
        self
    }

    /// Overrides when singletons are pruned (tests use a tiny value).
    pub fn with_prune_threshold(mut self, pairs: usize) -> Self {
        self.prune_at = pairs.max(1);
        self
    }

    /// Bounds memory on huge corpora: drop pairs seen once. If that frees
    /// little, the limit doubles so we don't rescan on every pair.
    fn prune_if_large(&mut self) {
        if self.pairs.len() > self.prune_at {
            self.pairs.retain(|_, pair| pair.count > 1);
            self.prune_at = self.prune_at.max(self.pairs.len() * 2);
        }
    }

    /// Counts the pairs of a corpus: as text of the `with_lang` language if one was set, else
    /// with the engine's normal form, words and phrase breaks.
    pub fn add_corpus(&mut self, reader: impl BufRead, max_lines: Option<usize>) -> io::Result<()> {
        match self.lang.take() {
            Some(lang) => {
                let result = self.add_corpus_normalised(
                    reader,
                    max_lines,
                    |line| lang.normalize(line),
                    |c| lang.is_phrase_break(c),
                    |phrase| lang.words(phrase).collect(),
                );
                self.lang = Some(lang);
                result
            }
            None => self.add_corpus_with(reader, max_lines, is_phrase_break, |phrase| {
                words(phrase).collect()
            }),
        }
    }

    /// `add_corpus` with another script's phrase breaks and tokeniser (`native_words`).
    pub fn add_corpus_with(
        &mut self,
        reader: impl BufRead,
        max_lines: Option<usize>,
        is_break: impl Fn(char) -> bool,
        tokens: impl Fn(&str) -> Vec<&str>,
    ) -> io::Result<()> {
        self.add_corpus_normalised(reader, max_lines, normalize, is_break, tokens)
    }

    /// `add_corpus_with` and a normal form other than the engine's.
    fn add_corpus_normalised(
        &mut self,
        reader: impl BufRead,
        max_lines: Option<usize>,
        normalise: impl Fn(&str) -> String,
        is_break: impl Fn(char) -> bool,
        tokens: impl Fn(&str) -> Vec<&str>,
    ) -> io::Result<()> {
        for_each_line(reader, max_lines, |line| {
            let line = normalise(line);
            // A pair never spans punctuation: "i.e." is not "i" followed by "e".
            for phrase in line.split(&is_break) {
                let mut previous: Option<u32> = None;
                let mut before: Option<&str> = None; // the word before `previous`
                let mut last: Option<&str> = None;
                for word in tokens(phrase) {
                    let id = self.ids.get(&lookup_key(word)).copied();
                    if let (Some(p), Some(next)) = (previous, id)
                        && (p as usize) < self.previous_allowed
                        && !last.is_some_and(|l| is_roman_one(l, before))
                    {
                        let pair = self.pairs.entry((p, next)).or_default();
                        pair.count = pair.count.saturating_add(1);
                        if word.starts_with(|c: char| c.is_uppercase()) {
                            pair.capitalised = pair.capitalised.saturating_add(1);
                        }
                        self.prune_if_large();
                    }
                    previous = id;
                    before = last;
                    last = Some(word);
                }
            }
        })
    }

    /// FST entries `previous\0next` → count, keeping the best `per_word` for each previous word.
    /// The next word is in its display spelling, capitalised if it mostly was
    /// after this word ("the United", though "united" alone).
    pub fn entries(&self, per_word: usize) -> Entries {
        let mut by_previous: HashMap<u32, Vec<(u32, u64, bool)>> = HashMap::new();
        for (&(p, n), pair) in &self.pairs {
            let capital = u64::from(pair.capitalised) * 2 > u64::from(pair.count);
            by_previous
                .entry(p)
                .or_default()
                .push((n, u64::from(pair.count), capital));
        }
        let mut entries = Vec::new();
        for (p, mut nexts) in by_previous {
            nexts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            for (n, count, capital) in nexts.into_iter().take(per_word) {
                let mut key = lookup_key(&self.forms[p as usize]).into_bytes();
                key.push(SEPARATOR);
                let form = &self.forms[n as usize];
                if capital && form.starts_with(|c: char| c.is_ascii_lowercase()) {
                    key.push(form.as_bytes()[0].to_ascii_uppercase());
                    key.extend_from_slice(&form.as_bytes()[1..]);
                } else {
                    key.extend_from_slice(form.as_bytes());
                }
                entries.push((key, count));
            }
        }
        entries
    }
}

/// Whether `word` is the Roman numeral one rather than the pronoun: a lowercase
/// "i" ("part i"), or "I" after a capitalised word ("World War I", "Elizabeth I").
/// Wikipedia has so many that "I" was otherwise followed by "of". This also
/// drops some real pronouns ("If I"), which only costs a few counts.
fn is_roman_one(word: &str, before: Option<&str>) -> bool {
    word == "i"
        || (word == "I" && before.is_some_and(|b| b.starts_with(|c: char| c.is_uppercase())))
}

pub(crate) fn is_phrase_break(c: char) -> bool {
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
            | '\u{2026}'
            | '/'
            | '|'
            | '\u{2018}'
            | '{'
            | '}'
            | '<'
            | '>'
            | '*'
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

/// The English pairs of a `next.fst`: those whose first word is ASCII. lexbuild's English
/// vocabulary is ASCII only (`count_words` with `Script::Latin`) and every other vocabulary is
/// in its own script, so this is exactly the English half of the file.
pub fn english_pairs(next: &mlengine_core::lexicon::Lexicon) -> Entries {
    let mut out = Vec::new();
    next.words(|key, count| {
        if let Some(sep) = key.iter().position(|&b| b == SEPARATOR)
            && key[..sep].is_ascii()
        {
            out.push((key.to_vec(), count));
        }
    });
    out
}

/// Writes `en.fst`, `en-next.fst` (the English base's own next words: `english_next`, the
/// English pairs of `next`), `ml.fst`, `manglish.fst` and `next.fst` into `out`, plus
/// `stats.txt`.
pub fn write_bundle(
    out: &Path,
    tables: &mlengine_core::romanisation::Romanisation,
    english: &[(String, u64)],
    malayalam: &[(String, u64)],
    english_next: &[(Vec<u8>, u64)],
    next: &[(Vec<u8>, u64)],
) -> io::Result<Stats> {
    fs::create_dir_all(out)?;
    let mut report = String::new();
    let manglish = manglish::entries(tables, malayalam);
    // A word's other keys, for tables that give it a second spelling (`optionalFinals`).
    let aliases = manglish::aliases(tables, malayalam);
    let manglish_len = manglish.len() + aliases.len();
    let files: [(&str, Entries, Entries, usize); 5] = [
        ("en.fst", to_entries(english), Vec::new(), english.len()),
        ("en-next.fst", english_next.to_vec(), Vec::new(), english_next.len()),
        ("ml.fst", to_entries(malayalam), Vec::new(), malayalam.len()),
        ("manglish.fst", manglish, aliases, manglish_len),
        ("next.fst", next.to_vec(), Vec::new(), next.len()),
    ];
    for (name, entries, aliases, n) in files {
        let bytes = build_with_aliases(entries, aliases).map_err(io::Error::other)?;
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

/// In the script's block, whether or not a letter (a danda or a native digit is not one).
fn in_block(table: &Table, c: char) -> bool {
    (c as u32)
        .checked_sub(table.block_base() as u32)
        .is_some_and(|offset| offset < 0x80)
}

/// Splits a line into the words of a native script described by `table`, and other words: a
/// word is a run of the script's letters and signs (`Table::is_letter`), joiners, and letters
/// and digits outside the block. The engine's `words` would split a Devanagari word at its
/// virama or nukta, which Unicode doesn't call alphanumeric.
pub fn native_words<'a>(table: &Table, line: &'a str) -> Vec<&'a str> {
    line.split(|c: char| {
        let token = table.is_letter(c)
            || matches!(c, '\u{200C}' | '\u{200D}')
            || (c.is_alphanumeric() && !in_block(table, c));
        !token
    })
    .filter(|w| !w.is_empty())
    .collect()
}

/// A phrase break in a native-script corpus: `is_phrase_break`, or the script's own
/// punctuation (the danda and double danda).
pub fn native_phrase_break(table: &Table, c: char) -> bool {
    is_phrase_break(c) || (in_block(table, c) && !table.is_letter(c) && !c.is_numeric())
}

/// Counts every word of the native script described by `table`, by surface form, after normalising.
pub fn count_native_words(
    reader: impl BufRead,
    table: &Table,
    max_chars: usize,
    max_lines: Option<usize>,
) -> io::Result<Counts> {
    let mut counts = Counts::new();
    for_each_line(reader, max_lines, |line| {
        let line = normalize(line);
        for word in native_words(table, &line) {
            if table.is_native(word) && word.chars().count() <= max_chars {
                *counts.entry(word.to_string()).or_default() += 1;
            }
        }
    })?;
    Ok(counts)
}

/// Native forms: frequent ones, plus every word of `listed` even if the corpus never used it.
/// Sorted by count, highest first. `malayalam_vocab` for any script with a `Table`.
pub fn native_vocab(
    counts: &Counts,
    listed: &HashSet<String>,
    table: &Table,
    config: &Config,
) -> Vec<(String, u64)> {
    let ok = |w: &str| {
        table.is_native(w) && table.plausible(w) && w.chars().count() <= config.max_native_chars
    };
    let mut vocab: HashMap<String, u64> = counts
        .iter()
        .filter(|(w, c)| ok(w) && (**c >= config.min_count || listed.contains(*w)))
        .map(|(w, c)| (w.clone(), *c))
        .collect();
    for word in listed.iter().filter(|w| ok(w)) {
        vocab.entry(word.clone()).or_insert(1);
    }
    let mut vocab: Vec<(String, u64)> = vocab.into_iter().collect();
    sort_and_cap(&mut vocab, config.malayalam_max);
    vocab
}

/// A lexicon file's words and counts, most frequent first (ties in key order). A pack's English
/// vocabulary is the base's `en.fst` itself, so its `next.fst` is built against exactly the
/// lexicon the keyboard ships.
pub fn read_lexicon(path: &Path) -> io::Result<Vec<(String, u64)>> {
    let lex = mlengine_core::lexicon::Lexicon::open(path)
        .map_err(|e| io::Error::other(format!("{}: {e}", path.display())))?;
    let mut words = Vec::with_capacity(lex.len());
    lex.words(|key, count| {
        if let Ok(word) = std::str::from_utf8(key) {
            words.push((word.to_owned(), count));
        }
    });
    words.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(words)
}

/// Writes a native pack's lexicons into `out`: `<lang>.fst`, `<romanised>.fst` (keyed by the
/// table's skeletons), `next.fst`, plus `stats.txt`.
pub fn write_native_bundle(
    out: &Path,
    lang: &str,
    romanised: &str,
    table: &Table,
    native: &[(String, u64)],
    next: &[(Vec<u8>, u64)],
) -> io::Result<Stats> {
    fs::create_dir_all(out)?;
    let mut report = String::new();
    let (keys, keys_len) = table.build(native)?;
    let files: [(String, Vec<u8>, usize); 3] = [
        (
            format!("{lang}.fst"),
            build(to_entries(native)).map_err(io::Error::other)?,
            native.len(),
        ),
        (format!("{romanised}.fst"), keys, keys_len),
        (
            "next.fst".to_string(),
            build(next.to_vec()).map_err(io::Error::other)?,
            next.len(),
        ),
    ];
    for (name, bytes, n) in files {
        fs::write(out.join(&name), &bytes)?;
        let _ = writeln!(report, "{name}: {n} keys, {} bytes", bytes.len());
    }
    let top: Vec<&str> = native.iter().take(20).map(|(w, _)| w.as_str()).collect();
    let _ = writeln!(report, "{lang} top 20: {}", top.join(" "));
    fs::write(out.join("stats.txt"), &report)?;
    Ok(Stats { report })
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
    fn native_words_keep_virama_and_nukta_and_split_at_dandas() {
        let t = hindi();
        let line = "क्या ज़िंदगी है। २०२० में ok, 42";
        assert_eq!(
            native_words(&t, line),
            ["क्या", "ज़िंदगी", "है", "में", "ok", "42"]
        );
        let c = count_native_words(line.as_bytes(), &t, 30, None).unwrap();
        let mut got: Vec<_> = c.keys().map(String::as_str).collect();
        got.sort();
        assert_eq!(got, ["क्या", "ज़िंदगी", "में", "है"]);
        assert!(native_phrase_break(&t, '।'));
        assert!(!native_phrase_break(&t, '्'));
    }

    #[test]
    fn native_vocab_keeps_frequent_plausible_words() {
        let t = hindi();
        let c = count_native_words("है है है ा ा ा न् न् न् भारत".as_bytes(), &t, 30, None).unwrap();
        let config = Config {
            min_count: 2,
            ..Config::default()
        };
        let vocab = native_vocab(&c, &set(&["दिल्ली"]), &t, &config);
        assert_eq!(vocab, [("है".to_string(), 3), ("दिल्ली".to_string(), 1)]);
    }

    #[test]
    fn native_next_words_pair_within_phrases() {
        let t = hindi();
        let vocab = vocab_of(&["क्या", "हाल", "है", "ठीक"]);
        let mut next = NextWords::new(&vocab, &Config::default());
        next.add_corpus_with(
            "क्या हाल है। ठीक है\nक्या हाल".as_bytes(),
            None,
            |c| native_phrase_break(&t, c),
            |p| native_words(&t, p),
        )
        .unwrap();
        let mut entries = next.entries(3);
        entries.sort();
        let want: Vec<(Vec<u8>, u64)> = [("क्या\0हाल", 2), ("ठीक\0है", 1), ("हाल\0है", 1)]
            .iter()
            .map(|(k, c)| (k.as_bytes().to_vec(), *c))
            .collect();
        assert_eq!(entries, want);
    }

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
        let m = counts("എന്റെ വീട് എന്റെ cat", Script::MALAYALAM);
        assert_eq!(m.get("എന്റെ"), Some(&2));
    }

    #[test]
    fn counting_normalises_old_chillus() {
        let m = counts("അവന\u{0D4D}\u{200D} അവൻ", Script::MALAYALAM);
        assert_eq!(m.get("അവൻ"), Some(&2));
    }

    #[test]
    fn counting_ignores_joiners_at_word_edges() {
        // Malayalam Wikipedia often ends a word in virama + ZWNJ ("ആണ്‌").
        let m = counts("ആണ്\u{200C} ആണ് \u{200C}തന്നെ തന്നെ", Script::MALAYALAM);
        assert_eq!(m.get("ആണ്"), Some(&2));
        assert_eq!(m.get("തന്നെ"), Some(&2));
        assert_eq!(m.len(), 2);
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
        let vocab = english_vocab(
            &c,
            &set(&["the", "London", "I", "i", "hello"]),
            &HashSet::new(),
            &config,
        );
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
        assert!(english_vocab(&c, &HashSet::new(), &HashSet::new(), &rare).is_empty());
        let common = Config {
            english_unlisted_min: 5,
            ..rare
        };
        assert_eq!(
            english_vocab(&c, &HashSet::new(), &HashSet::new(), &common).len(),
            1
        );
    }

    #[test]
    fn listed_indian_english_is_kept_in_its_own_spelling() {
        // Vishu is too rare for an unlisted word, PNR is in no corpus here, and
        // biryani's capitalised uses (menus, titles) outnumber its lowercase ones.
        let config = Config {
            min_count: 3,
            english_unlisted_min: 100,
            ..Config::default()
        };
        let c = counts(
            &format!(
                "Vishu vishu Biryani Biryani biryani zork {}",
                "pan ".repeat(5)
            ),
            Script::Latin,
        );
        let scowl = set(&["pan"]);
        let extra = set(&["Vishu", "biryani", "PNR", "PAN"]);
        let mut vocab = english_vocab(&c, &scowl, &extra, &config);
        vocab.sort();
        assert_eq!(
            vocab,
            [
                ("PNR".to_string(), 1),
                ("Vishu".to_string(), 2),
                ("biryani".to_string(), 3),
                ("pan".to_string(), 5),
            ]
        );
    }

    #[test]
    fn malayalam_keeps_frequent_and_listed_words() {
        let c = counts("എന്റെ എന്റെ എന്റെ വീട് ി ൽ ൽ ൽ ന് ന് ന്", Script::MALAYALAM);
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
        let vocab = english_vocab(&c, &set(&["a", "b", "c"]), &HashSet::new(), &config);
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
    fn hand_lists_skip_comments() {
        let list = read_hand_list("# festivals\nOnam\n\n  PNR \n#biryani\n".as_bytes()).unwrap();
        assert_eq!(list, set(&["Onam", "PNR"]));
    }

    #[test]
    fn word_lists_trim_bom_and_blank_lines() {
        let list = read_word_list("\u{FEFF}അക്കാമ്മ\n\n  കൊച്ചി \n".as_bytes()).unwrap();
        assert_eq!(list, set(&["അക്കാമ്മ", "കൊച്ചി"]));
    }

    fn vocab_of(words: &[&str]) -> Vec<(String, u64)> {
        words.iter().map(|w| (w.to_string(), 1)).collect()
    }

    #[test]
    fn display_form_prefers_lowercase_from_five_percent() {
        let scowl = set(&["foo", "Foo"]);
        let at = display_form("foo", &[("foo", 1), ("Foo", 19)], 20, &scowl);
        assert_eq!(at, "foo");
        let below = display_form("foo", &[("foo", 1), ("Foo", 20)], 21, &scowl);
        assert_eq!(below, "Foo");
    }

    #[test]
    fn mixed_script_and_digit_tokens_are_dropped() {
        let c = counts("abc123 x1 caf\u{e9}猫 ok 2020 a\u{0D15}", Script::Latin);
        let mut keys: Vec<_> = c.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["ok"]);
        let m = counts(
            "\u{0D15}\u{0D3E}1 \u{0D15}\u{0D3E} ab\u{0D15}",
            Script::MALAYALAM,
        );
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn overlong_tokens_are_rejected_while_counting() {
        let ok = "a".repeat(MAX_ENGLISH_LEN);
        let long = "a".repeat(MAX_ENGLISH_LEN + 1);
        let c = counts(&format!("{ok} {long}"), Script::Latin);
        assert_eq!(c.len(), 1);
        assert!(c.contains_key(&ok));
        let ok_ml = "\u{0D15}".repeat(MAX_NATIVE_CHARS);
        let long_ml = "\u{0D15}".repeat(MAX_NATIVE_CHARS + 1);
        let m = counts(&format!("{ok_ml} {long_ml}"), Script::MALAYALAM);
        assert_eq!(m.len(), 1);
        assert!(m.contains_key(&ok_ml));
    }

    #[test]
    fn invalid_utf8_lines_are_skipped() {
        let mut data = b"cat dog\n".to_vec();
        data.extend_from_slice(b"bad \xFF\xFE line\n");
        data.extend_from_slice(b"cat bird\n");
        let c = count_words(&data[..], Script::Latin, None).unwrap();
        assert_eq!(c.get("cat"), Some(&2));
        assert_eq!(c.get("bird"), Some(&1));
        assert_eq!(c.get("bad"), None);
        let mut next = NextWords::new(&vocab_of(&["cat", "dog", "bird"]), &Config::default());
        next.add_corpus(&data[..], None).unwrap();
        let keys: Vec<_> = next.entries(3).into_iter().map(|(k, _)| k).collect();
        assert!(keys.contains(&b"cat\0bird".to_vec()));
    }

    #[test]
    fn roman_numeral_i_is_not_the_pronoun() {
        // "World War I of", "Elizabeth I and", "part i of": not what follows "I".
        let vocab = vocab_of(&[
            "I",
            "War",
            "Elizabeth",
            "part",
            "of",
            "and",
            "am",
            "If",
            "then",
        ]);
        let text = "War I of\nElizabeth I and\npart i of\nI am\nthen I am\nIf I am";
        let mut next = NextWords::new(&vocab, &Config::default());
        next.add_corpus(text.as_bytes(), None).unwrap();
        let mut i_entries: Vec<_> = next
            .entries(5)
            .into_iter()
            .filter(|(k, _)| k.starts_with(b"i\0"))
            .collect();
        i_entries.sort();
        assert_eq!(i_entries, [(b"i\0am".to_vec(), 2)]);
    }

    #[test]
    fn a_next_word_keeps_the_capital_it_mostly_has_after_that_word() {
        // "united" is shown lowercase on its own, but after "the" it is "United".
        let vocab = vocab_of(&["the", "united", "states", "we"]);
        let text = "the United States\nthe United States\nthe united front\nwe united";
        let mut next = NextWords::new(&vocab, &Config::default());
        next.add_corpus(text.as_bytes(), None).unwrap();
        let mut keys: Vec<_> = next.entries(3).into_iter().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                (b"the\0United".to_vec(), 3),
                (b"united\0States".to_vec(), 2),
                (b"we\0united".to_vec(), 1),
            ]
        );
    }

    #[test]
    fn pairs_do_not_cross_punctuation_or_lines() {
        let vocab = vocab_of(&["a", "b", "c", "d", "e", "f", "g"]);
        let text = "a b. c\nd e\nf\u{2026} g";
        let mut next = NextWords::new(&vocab, &Config::default());
        next.add_corpus(text.as_bytes(), None).unwrap();
        let mut keys: Vec<_> = next.entries(3).into_iter().map(|(k, _)| k).collect();
        keys.sort();
        assert_eq!(keys, [b"a\0b".to_vec(), b"d\0e".to_vec()]);
    }

    #[test]
    fn newly_listed_breaks_split_phrases() {
        for sep in [
            "/", "|", "\u{2018}", "\u{201C}", "\u{201D}", "{", "}", "<", ">", "*",
        ] {
            let text = format!("a {sep} b");
            let mut next = NextWords::new(&vocab_of(&["a", "b"]), &Config::default());
            next.add_corpus(text.as_bytes(), None).unwrap();
            assert!(next.entries(3).is_empty(), "{sep:?} should break a phrase");
        }
    }

    #[test]
    fn pruning_keeps_the_strongest_pairs() {
        let vocab = vocab_of(&["a", "b", "c", "d", "e", "f", "g", "h"]);
        let text = "a b\na b\na b\nc d\nc d\ne f\ng h\nh g\nf e\nd c\n";
        let config = Config::default();
        let mut pruned = NextWords::new(&vocab, &config).with_prune_threshold(3);
        pruned.add_corpus(text.as_bytes(), None).unwrap();
        let mut keys: Vec<_> = pruned.entries(3).into_iter().collect();
        keys.sort();
        assert!(keys.contains(&(b"a\0b".to_vec(), 3)));
        assert!(keys.contains(&(b"c\0d".to_vec(), 2)));
        assert!(pruned.pairs.len() <= 6);

        let mut full = NextWords::new(&vocab, &config);
        full.add_corpus(text.as_bytes(), None).unwrap();
        let mut never = NextWords::new(&vocab, &config).with_prune_threshold(1_000);
        never.add_corpus(text.as_bytes(), None).unwrap();
        let (mut x, mut y) = (full.entries(3), never.entries(3));
        x.sort();
        y.sort();
        assert_eq!(x, y);
    }

    #[test]
    fn bundle_bytes_are_deterministic() {
        let c = counts("the cat the dog the cat a b c d e f g", Script::Latin);
        let config = Config {
            min_count: 1,
            ..Config::default()
        };
        let en = english_vocab(&c, &set(&["the", "cat", "dog"]), &HashSet::new(), &config);
        let build_once = || {
            let dir = tempfile::tempdir().unwrap();
            let mut next = NextWords::new(&en, &config);
            next.add_corpus("the cat the dog the cat".as_bytes(), None)
                .unwrap();
            let tables = mlengine_core::romanisation::Romanisation::load(
                &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/ml.neopack/romanisation.json"),
            )
            .unwrap();
            let entries = next.entries(3);
            write_bundle(dir.path(), &tables, &en, &[], &entries, &entries).unwrap();
            ["en.fst", "en-next.fst", "ml.fst", "manglish.fst", "next.fst"]
                .map(|n| fs::read(dir.path().join(n)).unwrap())
        };
        assert_eq!(build_once(), build_once());
    }
}

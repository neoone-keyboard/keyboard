//! A word list stored as a finite-state transducer (`fst` crate): key bytes →
//! u64 count. Bundle files are memory-mapped, so a lexicon costs almost no
//! resident memory and nothing proportional to its size is ever allocated.

use std::fmt;
use std::fs::File;
use std::path::Path;

use fst::{Automaton, IntoStreamer, Map, MapBuilder, Streamer};
use memmap2::Mmap;

/// Key holding the sum of every count, so counts can become probabilities.
/// It starts with NUL, which never appears in typed text, so no lookup meets it.
pub const TOTAL_KEY: &[u8] = b"\x00total";

/// Separates the two parts of a compound key (`skeleton\0word`, `prev\0next`).
pub const SEPARATOR: u8 = 0;

#[derive(Debug)]
pub enum LexiconError {
    Io(std::io::Error),
    Fst(fst::Error),
}

impl fmt::Display for LexiconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "lexicon I/O error: {e}"),
            Self::Fst(e) => write!(f, "lexicon format error: {e}"),
        }
    }
}

impl std::error::Error for LexiconError {}

impl From<std::io::Error> for LexiconError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<fst::Error> for LexiconError {
    fn from(e: fst::Error) -> Self {
        Self::Fst(e)
    }
}

/// Where the FST bytes live: a read-only file mapping (the app) or memory (tests, lexbuild).
pub enum Bytes {
    Mapped(Mmap),
    Owned(Vec<u8>),
}

impl AsRef<[u8]> for Bytes {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Mapped(m) => m,
            Self::Owned(v) => v,
        }
    }
}

pub struct Lexicon {
    map: Map<Bytes>,
    total: u64,
}

impl fmt::Debug for Lexicon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lexicon")
            .field("keys", &self.map.len())
            .field("total", &self.total)
            .finish()
    }
}

impl Lexicon {
    /// Maps `path` read-only and checks the FST's checksum, so a corrupt file
    /// is rejected here instead of misbehaving during a lookup.
    pub fn open(path: &Path) -> Result<Self, LexiconError> {
        let file = File::open(path)?;
        // SAFETY: bundle files are read-only resources inside the signed app
        // bundle; nothing truncates or writes them while they are mapped.
        let mmap = unsafe { Mmap::map(&file)? };
        Self::from_backing(Bytes::Mapped(mmap))
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, LexiconError> {
        Self::from_backing(Bytes::Owned(bytes))
    }

    fn from_backing(bytes: Bytes) -> Result<Self, LexiconError> {
        let map = Map::new(bytes)?;
        map.as_fst().verify()?;
        let total = map.get(TOTAL_KEY).unwrap_or(0).max(1);
        Ok(Self { map, total })
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn get(&self, key: &[u8]) -> Option<u64> {
        if key.first() == Some(&SEPARATOR) {
            return None;
        }
        self.map.get(key)
    }

    /// Natural log of the relative frequency of a count.
    pub fn log_prob(&self, count: u64) -> f32 {
        (count.max(1) as f64 / self.total as f64).ln() as f32
    }

    /// Calls `visit` with every key the automaton accepts, with the automaton's
    /// final state, stopping after `limit` keys so a walk is always bounded.
    pub fn search<A>(&self, automaton: A, limit: usize, mut visit: impl FnMut(&[u8], u64, A::State))
    where
        A: Automaton,
        A::State: Clone,
    {
        let mut stream = self.map.search_with_state(automaton).into_stream();
        let mut seen = 0;
        while let Some((key, value, state)) = stream.next() {
            if key.first() == Some(&SEPARATOR) {
                continue;
            }
            visit(key, value, state);
            seen += 1;
            if seen >= limit {
                break;
            }
        }
    }

    /// Every key starting with `prefix`, compared ASCII case-insensitively
    /// (`lon` finds `London`), at most `limit` of them in key order.
    pub fn scan_prefix(&self, prefix: &[u8], limit: usize, mut visit: impl FnMut(&[u8], u64)) {
        self.search(CaseFoldPrefix::new(prefix), limit, |key, value, _| {
            visit(key, value)
        });
    }
}

/// Builds FST bytes from (key, count) pairs in any order. Duplicate keys are
/// summed, and the total is stored under `TOTAL_KEY`.
pub fn build(entries: impl IntoIterator<Item = (Vec<u8>, u64)>) -> Result<Vec<u8>, LexiconError> {
    let mut entries: Vec<(Vec<u8>, u64)> = entries
        .into_iter()
        .filter(|(k, _)| !k.is_empty() && k.first() != Some(&SEPARATOR))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut merged: Vec<(Vec<u8>, u64)> = Vec::with_capacity(entries.len());
    for (key, value) in entries {
        match merged.last_mut() {
            Some((last, total)) if *last == key => *total = total.saturating_add(value),
            _ => merged.push((key, value)),
        }
    }
    let total = merged
        .iter()
        .fold(0u64, |sum, (_, v)| sum.saturating_add(*v));
    let mut builder = MapBuilder::memory();
    builder.insert(TOTAL_KEY, total)?;
    for (key, value) in merged {
        builder.insert(key, value)?;
    }
    Ok(builder.into_inner()?)
}

/// Matches keys that start with `prefix`, folding ASCII case on both sides.
/// Non-ASCII bytes (Malayalam) must match exactly.
#[derive(Debug, Clone)]
pub struct CaseFoldPrefix {
    prefix: Vec<u8>,
}

impl CaseFoldPrefix {
    pub fn new(prefix: &[u8]) -> Self {
        Self {
            prefix: prefix.to_ascii_lowercase(),
        }
    }
}

impl Automaton for CaseFoldPrefix {
    /// How many prefix bytes matched so far; None once a byte differed.
    type State = Option<usize>;

    fn start(&self) -> Option<usize> {
        Some(0)
    }

    fn is_match(&self, state: &Option<usize>) -> bool {
        *state == Some(self.prefix.len())
    }

    fn can_match(&self, state: &Option<usize>) -> bool {
        state.is_some()
    }

    fn will_always_match(&self, state: &Option<usize>) -> bool {
        self.is_match(state)
    }

    fn accept(&self, state: &Option<usize>, byte: u8) -> Option<usize> {
        let matched = (*state)?;
        match self.prefix.get(matched) {
            None => Some(matched),
            Some(&want) if want == byte.to_ascii_lowercase() => Some(matched + 1),
            Some(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn lexicon(words: &[(&str, u64)]) -> Lexicon {
        let bytes = build(words.iter().map(|(w, c)| (w.as_bytes().to_vec(), *c))).unwrap();
        Lexicon::from_bytes(bytes).unwrap()
    }

    #[test]
    fn counts_and_total() {
        let lex = lexicon(&[("the", 60), ("cat", 30), ("the", 10)]);
        assert_eq!(lex.get(b"the"), Some(70));
        assert_eq!(lex.get(b"dog"), None);
        assert_eq!(lex.total(), 100);
        assert_eq!(lex.get(TOTAL_KEY), None, "the total is not a word");
        assert!((lex.log_prob(70) - (0.7f32).ln()).abs() < 1e-6);
    }

    #[test]
    fn prefix_scan_folds_ascii_case() {
        let lex = lexicon(&[("London", 5), ("lonely", 3), ("long", 9), ("lost", 1)]);
        let mut got = Vec::new();
        lex.scan_prefix(b"LON", 10, |k, v| {
            got.push((String::from_utf8(k.to_vec()).unwrap(), v))
        });
        assert_eq!(
            got,
            [
                ("London".to_string(), 5),
                ("lonely".to_string(), 3),
                ("long".to_string(), 9)
            ]
        );
    }

    #[test]
    fn prefix_scan_is_bounded() {
        let lex = lexicon(&[("aa", 1), ("ab", 1), ("ac", 1), ("ad", 1)]);
        let mut n = 0;
        lex.scan_prefix(b"a", 2, |_, _| n += 1);
        assert_eq!(n, 2);
    }

    #[test]
    fn malayalam_prefix_scan() {
        let lex = lexicon(&[("എന്റെ", 50), ("എന്ത്", 40), ("അവൻ", 9)]);
        let mut got = Vec::new();
        lex.scan_prefix("എന്".as_bytes(), 10, |k, _| {
            got.push(String::from_utf8(k.to_vec()).unwrap())
        });
        assert_eq!(got, ["എന്ത്", "എന്റെ"]);
    }

    #[test]
    fn opens_a_mapped_file() {
        let bytes = build([(b"hello".to_vec(), 3)]).unwrap();
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&bytes).unwrap();
        let lex = Lexicon::open(file.path()).unwrap();
        assert_eq!(lex.get(b"hello"), Some(3));
    }

    #[test]
    fn corrupt_or_missing_files_are_errors_not_panics() {
        let mut bytes = build([(b"hello".to_vec(), 3), (b"help".to_vec(), 4)]).unwrap();
        let middle = bytes.len() / 2;
        bytes[middle] ^= 0xFF;
        assert!(Lexicon::from_bytes(bytes).is_err());
        assert!(Lexicon::from_bytes(b"not an fst".to_vec()).is_err());
        assert!(Lexicon::from_bytes(Vec::new()).is_err());
        assert!(Lexicon::open(Path::new("/nonexistent/en.fst")).is_err());
    }
}

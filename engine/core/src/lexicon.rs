//! A word list stored as a finite-state transducer (`fst` crate): key bytes →
//! u64 count. Bundle files are memory-mapped, so a lexicon costs almost no
//! resident memory and nothing proportional to its size is ever allocated.
//!
//! Scans stop after a bounded number of keys, and FST keys come in byte order,
//! not frequency order. So `build` also writes, for every prefix whose subtree
//! (compared ASCII case-insensitively) has more than `SCAN_LIMIT` keys:
//! - its `TOP_K` most frequent keys, under `TOP_PREFIX` (`top_completions`);
//! - its total count, under `COUNT_PREFIX` (`subtree_count`).
//!
//! Any other prefix has at most `SCAN_LIMIT` keys, so scanning it is exact
//! (prefixes longer than 255 bytes are never precomputed and may be cut off).
//! Reserved keys start with a byte below `FIRST_WORD_BYTE`, which no word
//! starts with; `get`, `search` and `scan_prefix` never return them.

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

/// Most keys a subtree may have before `build` precomputes its top keys and
/// total. Smaller subtrees are scanned whole, so every result is exact.
pub const SCAN_LIMIT: usize = 500;

/// How many keys `top_completions` can return: the precomputed list's length.
pub const TOP_K: usize = 16;

/// Reserved key `\x01`, prefix length (one byte), ASCII-lowercased prefix, then
/// one of the subtree's top keys as stored → that key's count. The length byte
/// keeps prefixes apart when a prefix itself contains `\0` (`next.fst`'s
/// `prev\0next`): the lists for `th` and `th\0` can never be confused.
pub const TOP_PREFIX: u8 = 0x01;

/// Reserved key `\x02` + ASCII-lowercased prefix → the subtree's total count.
pub const COUNT_PREFIX: u8 = 0x02;

/// Keys starting with a byte below this are reserved (`TOTAL_KEY`, `TOP_PREFIX`,
/// `COUNT_PREFIX`); `build` drops input keys that start with one.
pub const FIRST_WORD_BYTE: u8 = 0x03;

/// Longest prefix that gets precomputed entries (its length must fit the length byte).
const MAX_PRECOMPUTED_PREFIX: usize = u8::MAX as usize;

fn is_reserved(key: &[u8]) -> bool {
    key.first().is_none_or(|&b| b < FIRST_WORD_BYTE)
}

fn top_key_start(prefix: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(prefix.len() + 2);
    key.push(TOP_PREFIX);
    key.push(prefix.len() as u8);
    key.extend_from_slice(prefix);
    key
}

fn count_key(prefix: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(prefix.len() + 1);
    key.push(COUNT_PREFIX);
    key.extend_from_slice(prefix);
    key
}

/// Most frequent first; equal counts in key order, so results are deterministic.
fn by_frequency(a: &(Vec<u8>, u64), b: &(Vec<u8>, u64)) -> std::cmp::Ordering {
    b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))
}

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

    /// FNV-1a of the file's bytes, little-endian: what a language model built against this
    /// lexicon records (`nn::Tables::fingerprint`). Reads the whole file; call it once.
    pub fn fingerprint(&self) -> [u8; 8] {
        crate::nn::file::fnv1a(self.map.as_fst().as_bytes()).to_le_bytes()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn get(&self, key: &[u8]) -> Option<u64> {
        if is_reserved(key) {
            return None;
        }
        self.map.get(key)
    }

    /// Every key equal to `key` ignoring ASCII case (`us` → `US`, `us`), with
    /// its count, in key order. The walk only follows matching bytes, so it
    /// visits no more than the variants that exist.
    pub fn case_variants(&self, key: &[u8]) -> Vec<(Vec<u8>, u64)> {
        let mut found = Vec::new();
        self.search(CaseFoldExact::new(key), usize::MAX, |k, v, _| {
            found.push((k.to_vec(), v))
        });
        found
    }

    /// The `min(k, TOP_K)` most frequent keys starting with `prefix` (ASCII
    /// case-insensitively), the prefix itself included, most frequent first and
    /// equal counts in key order. Exact for any prefix: it reads the
    /// precomputed list when `build` wrote one, and otherwise scans a subtree
    /// that by construction has at most `SCAN_LIMIT` keys.
    pub fn top_completions(&self, prefix: &[u8], k: usize) -> Vec<(Vec<u8>, u64)> {
        let k = k.min(TOP_K);
        let lower = prefix.to_ascii_lowercase();
        let mut found = Vec::new();
        if self.is_precomputed(&lower) {
            let start = top_key_start(&lower);
            let mut stream = self.map.range().ge(&start).into_stream();
            while let Some((key, value)) = stream.next() {
                let Some(word) = key.strip_prefix(start.as_slice()) else {
                    break;
                };
                found.push((word.to_vec(), value));
            }
        } else {
            self.scan_prefix(&lower, SCAN_LIMIT, |key, value| {
                found.push((key.to_vec(), value))
            });
        }
        found.sort_by(by_frequency);
        found.truncate(k);
        found
    }

    /// The sum of the counts of every key starting with `prefix` (ASCII
    /// case-insensitively), the prefix itself included. Exact, like `top_completions`.
    pub fn subtree_count(&self, prefix: &[u8]) -> u64 {
        let lower = prefix.to_ascii_lowercase();
        if let Some(total) = self.precomputed_count(&lower) {
            return total;
        }
        let mut total = 0u64;
        self.scan_prefix(&lower, SCAN_LIMIT, |_, value| {
            total = total.saturating_add(value)
        });
        total
    }

    /// Whether some word starts with exactly the bytes `prefix` (no case folding, unlike
    /// `subtree_count`): a walk down the FST, `prefix.len()` transitions and no scan, so it is
    /// cheap enough to ask at every beam-search step. True for an empty prefix unless there are
    /// no words.
    pub fn has_prefix(&self, prefix: &[u8]) -> bool {
        let Some(&first) = prefix.first() else {
            return self.map.len() > usize::from(self.map.contains_key(TOTAL_KEY));
        };
        if first < FIRST_WORD_BYTE {
            return false;
        }
        // Every node is on the path to some key, so reaching the end of `prefix` means a key
        // starts with it; that key starts with `first`, so it is a word.
        let fst = self.map.as_fst();
        let mut node = fst.root();
        for &b in prefix {
            match node.find_input(b) {
                Some(i) => node = fst.node(node.transition(i).addr),
                None => return false,
            }
        }
        true
    }

    fn precomputed_count(&self, lower: &[u8]) -> Option<u64> {
        if lower.len() > MAX_PRECOMPUTED_PREFIX {
            return None;
        }
        self.map.get(count_key(lower))
    }

    fn is_precomputed(&self, lower: &[u8]) -> bool {
        self.precomputed_count(lower).is_some()
    }

    /// Natural log of the relative frequency of a count.
    pub fn log_prob(&self, count: u64) -> f32 {
        (count.max(1) as f64 / self.total as f64).ln() as f32
    }

    /// Calls `visit` with every key the automaton accepts, with the automaton's
    /// final state, stopping after `limit` keys so a walk is always bounded.
    /// Reserved keys are never visited: the walk doesn't enter them at all.
    pub fn search<A>(&self, automaton: A, limit: usize, mut visit: impl FnMut(&[u8], u64, A::State))
    where
        A: Automaton,
        A::State: Clone,
    {
        let mut stream = self
            .map
            .search_with_state(WordsOnly(automaton))
            .into_stream();
        let mut seen = 0;
        while let Some((key, value, state)) = stream.next() {
            let Some((_, state)) = state else {
                continue;
            };
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

    /// Every word with its count, in key order; reserved keys are skipped.
    /// Unbounded, so it is for offline tools (the eval harness), never typing.
    pub fn words(&self, mut visit: impl FnMut(&[u8], u64)) {
        self.search(fst::automaton::AlwaysMatch, usize::MAX, |key, value, _| {
            visit(key, value)
        });
    }
}

/// Builds FST bytes from (key, count) pairs in any order. Duplicate keys are
/// summed, and the total is stored under `TOTAL_KEY`. Keys that are empty or
/// start with a reserved byte are dropped. For every prefix with more than
/// `SCAN_LIMIT` keys it also writes the reserved top-keys and subtree-count
/// entries (see the module comment). The output depends only on the input
/// multiset, never on its order.
pub fn build(entries: impl IntoIterator<Item = (Vec<u8>, u64)>) -> Result<Vec<u8>, LexiconError> {
    build_with(entries, SCAN_LIMIT)
}

fn build_with(
    entries: impl IntoIterator<Item = (Vec<u8>, u64)>,
    scan_limit: usize,
) -> Result<Vec<u8>, LexiconError> {
    let mut entries: Vec<(Vec<u8>, u64)> = entries
        .into_iter()
        .filter(|(k, _)| !is_reserved(k))
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
    let mut reserved = precomputed_entries(&merged, scan_limit);
    reserved.sort_by(|a, b| a.0.cmp(&b.0));
    let mut builder = MapBuilder::memory();
    builder.insert(TOTAL_KEY, total)?;
    // Reserved keys start with 0x01 or 0x02, so they sort after TOTAL_KEY and before every word.
    for (key, value) in reserved {
        builder.insert(key, value)?;
    }
    for (key, value) in merged {
        builder.insert(key, value)?;
    }
    Ok(builder.into_inner()?)
}

/// The top-keys and subtree-count entries for every prefix (of the
/// ASCII-lowercased keys) whose subtree has more than `scan_limit` keys.
fn precomputed_entries(merged: &[(Vec<u8>, u64)], scan_limit: usize) -> Vec<(Vec<u8>, u64)> {
    // In lowercased order every subtree is one contiguous run.
    let mut order: Vec<(Vec<u8>, usize)> = merged
        .iter()
        .enumerate()
        .map(|(i, (k, _))| (k.to_ascii_lowercase(), i))
        .collect();
    order.sort();
    let mut out = Vec::new();
    let mut emit = |prefix: &[u8], run: &[(Vec<u8>, usize)]| {
        if run.len() <= scan_limit || prefix.len() > MAX_PRECOMPUTED_PREFIX {
            return;
        }
        let mut sum = 0u64;
        let mut top: Vec<&(Vec<u8>, u64)> = Vec::with_capacity(TOP_K + 1);
        for (_, i) in run {
            let entry = &merged[*i];
            sum = sum.saturating_add(entry.1);
            if top.len() < TOP_K || by_frequency(entry, top[TOP_K - 1]).is_lt() {
                let at = top.partition_point(|t| by_frequency(t, entry).is_lt());
                top.insert(at, entry);
                top.truncate(TOP_K);
            }
        }
        out.push((count_key(prefix), sum));
        let start = top_key_start(prefix);
        for (key, count) in top {
            let mut k = start.clone();
            k.extend_from_slice(key);
            out.push((k, *count));
        }
    };
    // The empty prefix is the whole list.
    emit(&[], &order);
    // Open prefixes, one per depth: (length, index of the first key in the run).
    let mut open: Vec<usize> = Vec::new();
    for i in 0..=order.len() {
        let shared = match (i.checked_sub(1).map(|j| &order[j].0), order.get(i)) {
            (Some(prev), Some((key, _))) => {
                prev.iter().zip(key).take_while(|(a, b)| a == b).count()
            }
            _ => 0,
        };
        // Close every prefix deeper than what this key shares with the previous one.
        while open.len() > shared {
            let depth = open.len();
            let first = open.pop().unwrap_or(0);
            emit(&order[first].0[..depth], &order[first..i]);
        }
        if let Some((key, _)) = order.get(i) {
            while open.len() < key.len() {
                open.push(i);
            }
        }
    }
    out
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

/// Matches keys equal to `key`, folding ASCII case on both sides.
#[derive(Debug, Clone)]
struct CaseFoldExact {
    key: Vec<u8>,
}

impl CaseFoldExact {
    fn new(key: &[u8]) -> Self {
        Self {
            key: key.to_ascii_lowercase(),
        }
    }
}

impl Automaton for CaseFoldExact {
    /// How many key bytes matched so far; None once a byte differed or the key ran past.
    type State = Option<usize>;

    fn start(&self) -> Option<usize> {
        Some(0)
    }

    fn is_match(&self, state: &Option<usize>) -> bool {
        *state == Some(self.key.len())
    }

    fn can_match(&self, state: &Option<usize>) -> bool {
        state.is_some()
    }

    fn accept(&self, state: &Option<usize>, byte: u8) -> Option<usize> {
        let matched = (*state)?;
        match self.key.get(matched) {
            Some(&want) if want == byte.to_ascii_lowercase() => Some(matched + 1),
            _ => None,
        }
    }
}

/// Wraps an automaton so it never enters the reserved key space. The state is
/// None once dead, otherwise (whether a byte was read, the inner state).
#[derive(Debug, Clone)]
struct WordsOnly<A>(A);

impl<A: Automaton> Automaton for WordsOnly<A>
where
    A::State: Clone,
{
    type State = Option<(bool, A::State)>;

    fn start(&self) -> Self::State {
        Some((false, self.0.start()))
    }

    fn is_match(&self, state: &Self::State) -> bool {
        matches!(state, Some((true, inner)) if self.0.is_match(inner))
    }

    fn can_match(&self, state: &Self::State) -> bool {
        matches!(state, Some((_, inner)) if self.0.can_match(inner))
    }

    fn will_always_match(&self, state: &Self::State) -> bool {
        matches!(state, Some((true, inner)) if self.0.will_always_match(inner))
    }

    fn accept(&self, state: &Self::State, byte: u8) -> Self::State {
        let (started, inner) = state.as_ref()?;
        if !started && byte < FIRST_WORD_BYTE {
            return None;
        }
        Some((true, self.0.accept(inner, byte)))
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
    fn fingerprint_is_the_fnv1a_of_the_file() {
        let bytes = build([(b"the".to_vec(), 60)]).unwrap();
        let want = crate::nn::file::fnv1a(&bytes).to_le_bytes();
        assert_eq!(Lexicon::from_bytes(bytes).unwrap().fingerprint(), want);
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

    fn owned(entries: &[(Vec<u8>, u64)]) -> Vec<(Vec<u8>, u64)> {
        entries.to_vec()
    }

    /// A mixed lexicon: case variants, Malayalam, `prev\0next` keys whose
    /// prefixes contain the separator, and ties.
    fn mixed() -> Vec<(Vec<u8>, u64)> {
        let mut entries: Vec<(Vec<u8>, u64)> = Vec::new();
        for (i, a) in ('a'..='h').enumerate() {
            for (j, b) in ('a'..='f').enumerate() {
                let count = ((i * 7 + j * 3) % 5) as u64 + 1;
                entries.push((format!("t{a}{b}").into_bytes(), count));
                entries.push((format!("T{a}{b}x").into_bytes(), count + 1));
                entries.push((format!("th\0{a}{b}").into_bytes(), count));
                entries.push((format!("th\0\0{a}").into_bytes(), count));
                entries.push((format!("ക{a}{b}").into_bytes(), count));
            }
        }
        entries.extend(owned(&[
            (b"US".to_vec(), 4),
            (b"us".to_vec(), 9),
            (b"use".to_vec(), 2),
            ("കാര്യം".as_bytes().to_vec(), 50),
        ]));
        entries
    }

    /// Every prefix of every key, lowercased, plus a few that match nothing.
    fn prefixes(entries: &[(Vec<u8>, u64)]) -> Vec<Vec<u8>> {
        let mut out: Vec<Vec<u8>> = vec![Vec::new(), b"zz".to_vec(), b"th\0\0\0".to_vec()];
        for (key, _) in entries {
            let lower = key.to_ascii_lowercase();
            for n in 1..=lower.len() {
                out.push(lower[..n].to_vec());
            }
        }
        out.sort();
        out.dedup();
        out
    }

    #[test]
    fn precomputed_and_scanned_results_are_identical() {
        let entries = mixed();
        let scanned =
            Lexicon::from_bytes(build_with(entries.clone(), usize::MAX).unwrap()).unwrap();
        // A tiny limit precomputes almost every prefix.
        let precomputed = Lexicon::from_bytes(build_with(entries.clone(), 2).unwrap()).unwrap();
        let mut heavy = 0;
        for prefix in prefixes(&entries) {
            if precomputed.is_precomputed(&prefix) {
                heavy += 1;
            }
            assert!(!scanned.is_precomputed(&prefix));
            for k in [1, 3, TOP_K, 100] {
                assert_eq!(
                    precomputed.top_completions(&prefix, k),
                    scanned.top_completions(&prefix, k),
                    "{:?} k={k}",
                    String::from_utf8_lossy(&prefix)
                );
            }
            assert_eq!(
                precomputed.subtree_count(&prefix),
                scanned.subtree_count(&prefix)
            );
            let upper = prefix.to_ascii_uppercase();
            assert_eq!(
                precomputed.top_completions(&upper, 5),
                scanned.top_completions(&prefix, 5)
            );
        }
        assert!(
            heavy > 20,
            "the test must exercise the precomputed path ({heavy})"
        );
        // The reserved entries are invisible to everything else.
        assert!(precomputed.len() > scanned.len());
        let mut words = Vec::new();
        precomputed.scan_prefix(b"", usize::MAX, |k, v| words.push((k.to_vec(), v)));
        let mut plain = Vec::new();
        scanned.scan_prefix(b"", usize::MAX, |k, v| plain.push((k.to_vec(), v)));
        assert_eq!(words, plain);
        assert_eq!(precomputed.total(), scanned.total());
    }

    #[test]
    fn top_completions_rank_by_count_then_key() {
        let lex = lexicon(&[("tea", 5), ("Ten", 9), ("ten", 9), ("the", 50), ("to", 70)]);
        let got = lex.top_completions(b"TE", 10);
        assert_eq!(
            got,
            [
                (b"Ten".to_vec(), 9),
                (b"ten".to_vec(), 9),
                (b"tea".to_vec(), 5)
            ]
        );
        assert_eq!(
            lex.top_completions(b"t", 2),
            [(b"to".to_vec(), 70), (b"the".to_vec(), 50)]
        );
        assert_eq!(lex.subtree_count(b"t"), 143);
        assert_eq!(lex.subtree_count(b"x"), 0);
        assert!(lex.top_completions(b"x", 3).is_empty());
    }

    #[test]
    fn a_big_subtree_still_gives_its_most_frequent_keys() {
        // 3000 names sort before "could" in key order.
        let mut entries: Vec<(Vec<u8>, u64)> = (0..3000)
            .map(|i| (format!("Co{i:04}").into_bytes(), 1))
            .collect();
        entries.push((b"could".to_vec(), 1000));
        entries.push((b"cold".to_vec(), 900));
        let lex = Lexicon::from_bytes(build(entries).unwrap()).unwrap();
        assert!(lex.is_precomputed(b"co"));
        assert_eq!(
            lex.top_completions(b"co", 2),
            [(b"could".to_vec(), 1000), (b"cold".to_vec(), 900)]
        );
        assert_eq!(lex.subtree_count(b"co"), 3000 + 1900);
        assert_eq!(lex.subtree_count(b"Co0"), 1000);
        assert_eq!(lex.top_completions(b"cou", 5), [(b"could".to_vec(), 1000)]);
    }

    #[test]
    fn separator_prefixes_do_not_bleed_into_each_other() {
        // `th` and `th\0` are both big; the length byte keeps their lists apart.
        let mut entries: Vec<(Vec<u8>, u64)> = (0..600)
            .map(|i| (format!("th\0w{i:03}").into_bytes(), 1))
            .collect();
        entries.extend((0..600).map(|i| (format!("th{i:03}").into_bytes(), 2)));
        entries.push((b"th\0\0odd".to_vec(), 7));
        entries.push((b"th\0big".to_vec(), 500));
        let lex = Lexicon::from_bytes(build(entries).unwrap()).unwrap();
        assert!(lex.is_precomputed(b"th\0"));
        assert!(lex.is_precomputed(b"th"));
        assert_eq!(
            lex.top_completions(b"th\0", 2),
            [(b"th\0big".to_vec(), 500), (b"th\0\0odd".to_vec(), 7)]
        );
        assert_eq!(
            lex.top_completions(b"th\0\0", 5),
            [(b"th\0\0odd".to_vec(), 7)]
        );
        assert_eq!(lex.subtree_count(b"th\0"), 600 + 7 + 500);
        assert_eq!(lex.subtree_count(b"th"), 600 * 2 + 600 + 7 + 500);
    }

    #[test]
    fn build_is_deterministic() {
        let entries = mixed();
        let mut reversed = entries.clone();
        reversed.reverse();
        let a = build(entries.clone()).unwrap();
        assert_eq!(a, build(reversed).unwrap());
        assert_eq!(a, build(entries).unwrap());
        let mut big: Vec<(Vec<u8>, u64)> = (0..1200)
            .map(|i| (format!("w{i}").into_bytes(), (i % 7) as u64))
            .collect();
        let first = build(big.clone()).unwrap();
        big.reverse();
        assert_eq!(first, build(big).unwrap());
    }

    #[test]
    fn reserved_keys_are_dropped_and_never_seen() {
        let lex = Lexicon::from_bytes(
            build([
                (b"\x01sneaky".to_vec(), 5),
                (b"\x02sneaky".to_vec(), 5),
                (b"\x00x".to_vec(), 5),
                (b"ok".to_vec(), 1),
            ])
            .unwrap(),
        )
        .unwrap();
        assert_eq!(lex.len(), 2, "TOTAL_KEY and ok");
        assert_eq!(lex.get(b"\x01sneaky"), None);
        assert_eq!(lex.total(), 1);
        let mut many: Vec<(Vec<u8>, u64)> = (0..800)
            .map(|i| (format!("a{i}").into_bytes(), 1))
            .collect();
        many.push((b"b".to_vec(), 1));
        let lex = Lexicon::from_bytes(build(many).unwrap()).unwrap();
        assert!(lex.is_precomputed(b"a"));
        let mut seen = 0;
        lex.search(fst::automaton::AlwaysMatch, usize::MAX, |k, _, _| {
            assert!(!is_reserved(k));
            seen += 1;
        });
        assert_eq!(seen, 801);
        assert_eq!(lex.get(&count_key(b"a")), None);
    }

    #[test]
    fn words_visits_every_word_and_no_reserved_key() {
        let mut many: Vec<(Vec<u8>, u64)> = (0..800)
            .map(|i| (format!("a{i}").into_bytes(), i + 1))
            .collect();
        many.push(("ഒരു".as_bytes().to_vec(), 9000));
        let lex = Lexicon::from_bytes(build(many.clone()).unwrap()).unwrap();
        assert!(lex.is_precomputed(b"a"), "reserved entries exist to be skipped");
        let mut seen = Vec::new();
        lex.words(|k, v| seen.push((k.to_vec(), v)));
        many.sort();
        assert_eq!(seen, many, "every word once, in key order, nothing reserved");
    }

    #[test]
    fn has_prefix_is_exact_and_never_sees_reserved_keys() {
        let lex = lexicon(&[("എന്തു", 5), ("എന്ത്", 3), ("cat", 1)]);
        assert!(lex.has_prefix("എ".as_bytes()));
        assert!(lex.has_prefix("എന്ത".as_bytes()));
        assert!(lex.has_prefix("എന്തു".as_bytes()), "a whole word is its own prefix");
        assert!(!lex.has_prefix("എന്തുക".as_bytes()));
        assert!(!lex.has_prefix("ന".as_bytes()));
        assert!(lex.has_prefix(b"ca"));
        assert!(!lex.has_prefix(b"CA"), "no case folding");
        assert!(!lex.has_prefix(b"\x00"), "the total key is reserved");
        assert!(!lex.has_prefix(&[COUNT_PREFIX]));
        assert!(lex.has_prefix(b""));
        assert!(!lexicon(&[]).has_prefix(b""), "only the total key");
    }

    #[test]
    fn case_variants_are_exact_and_fold_ascii() {
        let lex = lexicon(&[("US", 4), ("us", 9), ("use", 2), ("Us", 1)]);
        assert_eq!(
            lex.case_variants(b"uS"),
            [
                (b"US".to_vec(), 4),
                (b"Us".to_vec(), 1),
                (b"us".to_vec(), 9)
            ]
        );
        assert!(lex.case_variants(b"u").is_empty());
    }
}

//! Words the user taught the keyboard. An append-only log in the extension's
//! own container, replayed into a small map at load. Learned words are never
//! auto-replaced. I/O failures never stop typing: the store keeps working in
//! memory and simply doesn't persist.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// Most words kept; the least recently used are evicted beyond this.
pub const CAPACITY: usize = 5_000;
/// A word typed this many times (and not in any lexicon) is learned.
pub const TIMES_TO_LEARN: u32 = 2;
/// Longest word stored, in chars.
pub const MAX_WORD_CHARS: usize = 48;

const LOG_NAME: &str = "learned-words.log";

#[derive(Debug, Clone)]
struct Entry {
    display: String,
    seen: u32,
    learned: bool,
    last_used: u64,
}

#[derive(Debug, Default)]
pub struct UserStore {
    path: Option<PathBuf>,
    entries: HashMap<String, Entry>,
    clock: u64,
    log_lines: usize,
}

/// Lookups ignore ASCII case, so "Thom" typed at a sentence start is still the learned "Thom".
fn key(word: &str) -> String {
    word.to_ascii_lowercase()
}

fn acceptable(word: &str) -> bool {
    !word.is_empty()
        && word.chars().count() <= MAX_WORD_CHARS
        && !word.chars().any(|c| c.is_control() || c.is_whitespace())
}

impl UserStore {
    /// A store that is never written to disk (tests, or no container available).
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Loads `dir/learned-words.log`, creating `dir` if needed. Malformed lines are skipped.
    pub fn open(dir: &Path) -> Self {
        let mut store = Self {
            path: Some(dir.join(LOG_NAME)),
            ..Self::default()
        };
        let _ = fs::create_dir_all(dir);
        if let Some(path) = &store.path
            && let Ok(file) = File::open(path)
        {
            let lines: Vec<String> = BufReader::new(file).lines().map_while(Result::ok).collect();
            for line in &lines {
                match line.split_once('\t') {
                    Some(("L", word)) => store.apply_learn(word),
                    Some(("S", word)) => store.apply_seen(word),
                    _ => {}
                }
            }
            store.log_lines = lines.len();
        }
        store
    }

    pub fn is_learned(&self, word: &str) -> bool {
        self.entries.get(&key(word)).is_some_and(|e| e.learned)
    }

    pub fn learned_count(&self) -> usize {
        self.entries.values().filter(|e| e.learned).count()
    }

    /// Learns a word at once (the user undid an autocorrect, or tapped what they typed).
    pub fn learn(&mut self, word: &str) {
        if !acceptable(word) {
            return;
        }
        self.apply_learn(word);
        self.append("L", word);
    }

    /// Records that a word not in any lexicon was typed; learns it the second time.
    pub fn note_typed(&mut self, word: &str) {
        if !acceptable(word) || self.is_learned(word) {
            return;
        }
        self.apply_seen(word);
        self.append("S", word);
    }

    /// Learned words starting with `prefix` (ASCII case-insensitive), most recently used first.
    pub fn completions(&self, prefix: &str, limit: usize) -> Vec<String> {
        let prefix = key(prefix);
        let mut found: Vec<&Entry> = self
            .entries
            .iter()
            .filter(|(k, e)| e.learned && k.starts_with(&prefix) && k.len() > prefix.len())
            .map(|(_, e)| e)
            .collect();
        found.sort_by_key(|e| std::cmp::Reverse(e.last_used));
        found
            .into_iter()
            .take(limit)
            .map(|e| e.display.clone())
            .collect()
    }

    pub fn forget_all(&mut self) {
        self.entries.clear();
        self.log_lines = 0;
        if let Some(path) = &self.path {
            let _ = File::create(path);
        }
    }

    fn apply_learn(&mut self, word: &str) {
        let entry = self.touch(word);
        entry.learned = true;
    }

    fn apply_seen(&mut self, word: &str) {
        let entry = self.touch(word);
        entry.seen = entry.seen.saturating_add(1);
        if entry.seen >= TIMES_TO_LEARN {
            entry.learned = true;
        }
    }

    fn touch(&mut self, word: &str) -> &mut Entry {
        self.clock += 1;
        let clock = self.clock;
        let k = key(word);
        if !self.entries.contains_key(&k) && self.entries.len() >= CAPACITY {
            self.evict_oldest();
        }
        let entry = self.entries.entry(k).or_insert_with(|| Entry {
            display: word.to_string(),
            seen: 0,
            learned: false,
            last_used: clock,
        });
        entry.last_used = clock;
        entry
    }

    fn evict_oldest(&mut self) {
        if let Some(oldest) = self
            .entries
            .iter()
            .min_by_key(|(_, e)| e.last_used)
            .map(|(k, _)| k.clone())
        {
            self.entries.remove(&oldest);
        }
    }

    fn append(&mut self, tag: &str, word: &str) {
        let Some(path) = self.path.clone() else {
            return;
        };
        self.log_lines += 1;
        if self.log_lines > CAPACITY * 4 {
            self.compact(&path);
            return;
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(file, "{tag}\t{word}");
        }
    }

    /// Rewrites the log from the map so it never grows without bound.
    fn compact(&mut self, path: &Path) {
        let mut entries: Vec<&Entry> = self.entries.values().collect();
        entries.sort_by_key(|e| e.last_used);
        let mut text = String::new();
        let mut lines = 0;
        for e in entries {
            if e.learned {
                text.push_str(&format!("L\t{}\n", e.display));
                lines += 1;
            } else {
                for _ in 0..e.seen {
                    text.push_str(&format!("S\t{}\n", e.display));
                    lines += 1;
                }
            }
        }
        let tmp = path.with_extension("tmp");
        if fs::write(&tmp, text).is_ok() && fs::rename(&tmp, path).is_ok() {
            self.log_lines = lines;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns_on_request() {
        let mut store = UserStore::in_memory();
        store.learn("Thom");
        assert!(store.is_learned("Thom"));
        assert!(store.is_learned("thom"), "lookups ignore case");
        assert!(!store.is_learned("Tom"));
    }

    #[test]
    fn learns_a_word_typed_twice() {
        let mut store = UserStore::in_memory();
        store.note_typed("Jishnu");
        assert!(!store.is_learned("Jishnu"));
        store.note_typed("Jishnu");
        assert!(store.is_learned("Jishnu"));
    }

    #[test]
    fn rejects_empty_long_and_multiline_words() {
        let mut store = UserStore::in_memory();
        store.learn("");
        store.learn(&"a".repeat(MAX_WORD_CHARS + 1));
        store.learn("two\nlines");
        store.learn("a\tb");
        assert_eq!(store.learned_count(), 0);
    }

    #[test]
    fn completes_learned_words() {
        let mut store = UserStore::in_memory();
        store.learn("Jishnu");
        store.learn("Jitin");
        store.learn("Kochi");
        assert_eq!(store.completions("ji", 5), ["Jitin", "Jishnu"]);
        assert!(
            store.completions("jishnu", 5).is_empty(),
            "the word itself is not a completion"
        );
    }

    #[test]
    fn evicts_the_oldest_beyond_capacity() {
        let mut store = UserStore::in_memory();
        for i in 0..CAPACITY {
            store.learn(&format!("word{i}"));
        }
        store.learn("word0"); // used again, so word1 is now the oldest
        store.learn("newest");
        assert!(store.is_learned("word0"));
        assert!(!store.is_learned("word1"));
        assert!(store.is_learned("newest"));
        assert_eq!(store.learned_count(), CAPACITY);
    }

    #[test]
    fn persists_and_forgets() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = UserStore::open(dir.path());
            store.learn("Thom");
            store.note_typed("Jishnu");
        }
        let mut store = UserStore::open(dir.path());
        assert!(store.is_learned("Thom"));
        assert!(!store.is_learned("Jishnu"));
        store.note_typed("Jishnu");
        assert!(
            store.is_learned("Jishnu"),
            "the first sighting survived the reload"
        );
        store.forget_all();
        assert_eq!(store.learned_count(), 0);
        assert_eq!(UserStore::open(dir.path()).learned_count(), 0);
    }

    #[test]
    fn compaction_keeps_what_was_learned() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = UserStore::open(dir.path());
        store.learn("Thom");
        for _ in 0..(CAPACITY * 4 + 10) {
            store.learn("Thom");
        }
        let text = fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
        assert!(text.lines().count() < 100, "log was compacted");
        assert!(UserStore::open(dir.path()).is_learned("Thom"));
    }

    #[test]
    fn unreadable_lines_and_missing_dirs_do_not_fail() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(LOG_NAME), "garbage\nL\tok\n\u{FFFD}\n").unwrap();
        assert!(UserStore::open(dir.path()).is_learned("ok"));
        let mut store = UserStore::open(Path::new("/nonexistent-root/x"));
        store.learn("still");
        assert!(store.is_learned("still"));
    }
}

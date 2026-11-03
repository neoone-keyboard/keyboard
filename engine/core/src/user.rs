//! Words the user taught the keyboard. An append-only log in the extension's
//! own container, replayed into a small map at load. Learned words are never
//! auto-replaced. Words the user removed from the bar are kept too, so they
//! are never suggested again until learned anew. I/O failures never stop typing: the store keeps working in
//! memory and simply doesn't persist.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
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
    /// Keys (`key`) of words removed from the bar: never suggested until learned again.
    removed: HashSet<String>,
    clock: u64,
    log_lines: usize,
    /// The log ended mid-line and could not be truncated; the next append starts a fresh line.
    dirty_tail: bool,
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
        let Some(path) = store.path.clone() else {
            return store;
        };
        let Ok(bytes) = fs::read(&path) else {
            return store;
        };
        // Only complete (newline-terminated) lines count; a kill mid-write leaves a partial tail.
        let end = bytes.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        for raw in bytes[..end].split(|&b| b == b'\n') {
            let Ok(line) = std::str::from_utf8(raw) else {
                continue;
            };
            match line.split_once('\t') {
                Some(("L", word)) if acceptable(word) => store.apply_learn(word),
                Some(("S", word)) if acceptable(word) => store.apply_seen(word),
                Some(("R", word)) if acceptable(word) => store.apply_remove(word),
                _ => {}
            }
        }
        store.log_lines = bytes[..end].iter().filter(|&&b| b == b'\n').count();
        if end < bytes.len() {
            let truncated = OpenOptions::new()
                .write(true)
                .open(&path)
                .and_then(|f| f.set_len(end as u64));
            store.dirty_tail = truncated.is_err();
        }
        store
    }

    pub fn is_learned(&self, word: &str) -> bool {
        self.entries.get(&key(word)).is_some_and(|e| e.learned)
    }

    pub fn learned_count(&self) -> usize {
        self.entries.values().filter(|e| e.learned).count()
    }

    /// Whether the user removed this word from the bar (ASCII case-insensitive).
    pub fn is_removed(&self, word: &str) -> bool {
        self.removed.contains(&key(word))
    }

    /// Forgets a word and keeps it from being suggested again, until it is learned.
    pub fn remove(&mut self, word: &str) {
        if !acceptable(word) {
            return;
        }
        self.apply_remove(word);
        self.append("R", word);
    }

    /// Learns a word at once (the user undid an autocorrect, or tapped what they typed);
    /// a removed word is restored.
    pub fn learn(&mut self, word: &str) {
        if !acceptable(word) {
            return;
        }
        self.apply_learn(word);
        self.append("L", word);
    }

    /// Records that a word not in any lexicon was typed; learns it the second time.
    /// A removed word is not recorded: only an explicit `learn` restores it.
    pub fn note_typed(&mut self, word: &str) {
        if !acceptable(word) || self.is_learned(word) || self.is_removed(word) {
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

    /// Every learned word, as shown, in no particular order.
    pub fn learned(&self) -> impl Iterator<Item = &str> {
        self.entries
            .values()
            .filter(|e| e.learned)
            .map(|e| e.display.as_str())
    }

    pub fn forget_all(&mut self) {
        self.entries.clear();
        self.removed.clear();
        self.log_lines = 0;
        self.dirty_tail = false;
        if let Some(path) = &self.path {
            let _ = fs::remove_file(path.with_extension("tmp"));
            if fs::remove_file(path).is_err() {
                let _ = File::create(path);
            }
        }
    }

    fn apply_remove(&mut self, word: &str) {
        let k = key(word);
        self.entries.remove(&k);
        self.removed.insert(k);
    }

    fn apply_learn(&mut self, word: &str) {
        self.removed.remove(&key(word));
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
        // Unlearned (seen-once) entries go first; learned ones only when none remain.
        if let Some(oldest) = self
            .entries
            .iter()
            .min_by_key(|(_, e)| (e.learned, e.last_used))
            .map(|(k, _)| k.clone())
        {
            self.entries.remove(&oldest);
        }
    }

    fn append(&mut self, tag: &str, word: &str) {
        let Some(path) = self.path.clone() else {
            return;
        };
        // One write call per event, so a kill can leave at worst a partial tail.
        let mut line = String::new();
        if self.dirty_tail {
            line.push('\n');
        }
        line.push_str(&format!("{tag}\t{word}\n"));
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            if file.write_all(line.as_bytes()).is_ok() {
                self.dirty_tail = false;
                self.log_lines += 1;
            } else {
                // A failed write (e.g. a full disk) may have left part of the line.
                self.dirty_tail = true;
            }
        }
        if self.log_lines > CAPACITY * 4 {
            self.compact(&path);
        }
    }

    /// Rewrites the log from the map so it never grows without bound.
    fn compact(&mut self, path: &Path) {
        let mut entries: Vec<&Entry> = self.entries.values().collect();
        entries.sort_by_key(|e| e.last_used);
        let mut text = String::new();
        let mut lines = 0;
        // A removed word has no entry (and a learned one is not removed), so order is free.
        for k in &self.removed {
            text.push_str(&format!("R\t{k}\n"));
            lines += 1;
        }
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
            self.dirty_tail = false;
        } else {
            // Back off instead of retrying on every keystroke.
            let _ = fs::remove_file(&tmp);
            self.log_lines = 0;
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
    fn partial_last_line_is_ignored_and_not_glued_to_the_next_append() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(LOG_NAME), "L\tok\nL\tpart").unwrap();
        let mut store = UserStore::open(dir.path());
        assert!(store.is_learned("ok"));
        assert!(!store.is_learned("part"));
        store.learn("next");
        let reopened = UserStore::open(dir.path());
        assert!(reopened.is_learned("ok"));
        assert!(reopened.is_learned("next"));
        assert!(!reopened.is_learned("part"));
        assert!(!reopened.is_learned("partnext"));
    }

    #[test]
    fn invalid_utf8_line_does_not_stop_replay() {
        let dir = tempfile::tempdir().unwrap();
        let mut bytes = b"L\tbefore\nL\t".to_vec();
        bytes.extend_from_slice(&[0xFF, 0xFE, 0xC0]);
        bytes.extend_from_slice(b"\nL\tafter\n");
        fs::write(dir.path().join(LOG_NAME), bytes).unwrap();
        let store = UserStore::open(dir.path());
        assert!(store.is_learned("before"));
        assert!(store.is_learned("after"));
        assert_eq!(store.learned_count(), 2);
    }

    #[test]
    fn replay_applies_the_same_validation_as_live_input() {
        let dir = tempfile::tempdir().unwrap();
        let long = "a".repeat(MAX_WORD_CHARS + 1);
        let log = format!("L\ttwo words\nL\t{long}\nL\t\nL\tfine\n");
        fs::write(dir.path().join(LOG_NAME), log).unwrap();
        let store = UserStore::open(dir.path());
        assert_eq!(store.learned_count(), 1);
        assert!(store.is_learned("fine"));
    }

    #[cfg(unix)]
    #[test]
    fn failed_compaction_backs_off_and_leaves_no_tmp_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mut store = UserStore::open(dir.path());
        store.learn("Thom");
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let can_still_write = fs::write(dir.path().join("probe"), "x").is_ok();
        if !can_still_write {
            for _ in 0..(CAPACITY * 4 + 10) {
                store.learn("Thom");
            }
            assert!(store.log_lines <= CAPACITY * 4, "backed off after failing");
            assert!(!dir.path().join(LOG_NAME).with_extension("tmp").exists());
        }
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(store.is_learned("Thom"));
    }

    #[test]
    fn forget_all_removes_a_leftover_tmp_file() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path().join(LOG_NAME).with_extension("tmp");
        let mut store = UserStore::open(dir.path());
        store.learn("Thom");
        fs::write(&tmp, "L\tstale\n").unwrap();
        store.forget_all();
        assert!(!tmp.exists());
        assert_eq!(store.learned_count(), 0);
        assert_eq!(UserStore::open(dir.path()).learned_count(), 0);
    }

    #[test]
    fn one_off_words_do_not_evict_learned_ones() {
        let mut store = UserStore::in_memory();
        for i in 0..10 {
            store.learn(&format!("keep{i}"));
        }
        for i in 0..(CAPACITY - 10) {
            store.note_typed(&format!("once{i}"));
        }
        for i in 0..200 {
            store.note_typed(&format!("fresh{i}"));
        }
        for i in 0..10 {
            assert!(store.is_learned(&format!("keep{i}")));
        }
        assert_eq!(store.learned_count(), 10);
    }

    #[test]
    fn removed_words_are_forgotten_and_blocked() {
        let mut store = UserStore::in_memory();
        store.learn("Thom");
        store.remove("thom");
        assert!(store.is_removed("Thom"), "lookups ignore case");
        assert!(!store.is_learned("Thom"));
        assert!(store.completions("th", 5).is_empty());
        store.remove("കിളി");
        assert!(store.is_removed("കിളി"));
        assert!(!store.is_removed("Tom"));
    }

    #[test]
    fn typing_a_removed_word_does_not_learn_or_restore_it() {
        let mut store = UserStore::in_memory();
        store.remove("Jishnu");
        store.note_typed("Jishnu");
        store.note_typed("Jishnu");
        assert!(store.is_removed("Jishnu"));
        assert!(!store.is_learned("Jishnu"));
    }

    #[test]
    fn learning_a_removed_word_restores_it() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = UserStore::open(dir.path());
            store.remove("Thom");
            store.learn("thom");
            assert!(!store.is_removed("Thom"));
            assert!(store.is_learned("Thom"));
        }
        let store = UserStore::open(dir.path());
        assert!(!store.is_removed("Thom"), "the log replays in order");
        assert!(store.is_learned("Thom"));
    }

    #[test]
    fn removals_persist_and_survive_compaction() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = UserStore::open(dir.path());
            store.learn("Thom");
            store.remove("Thom");
            store.remove("hello");
        }
        let mut store = UserStore::open(dir.path());
        assert!(store.is_removed("thom"));
        assert!(store.is_removed("hello"));
        assert!(
            !store.is_learned("Thom"),
            "the removal came after the learning"
        );
        for _ in 0..(CAPACITY * 4 + 10) {
            store.learn("Anu");
        }
        let text = fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
        assert!(text.lines().count() < 100, "log was compacted");
        let reopened = UserStore::open(dir.path());
        assert!(reopened.is_removed("Thom"));
        assert!(reopened.is_removed("hello"));
        assert!(reopened.is_learned("Anu"));
    }

    #[test]
    fn forget_all_clears_removals() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = UserStore::open(dir.path());
        store.remove("hello");
        store.forget_all();
        assert!(!store.is_removed("hello"));
        assert!(!UserStore::open(dir.path()).is_removed("hello"));
    }

    #[test]
    fn rejects_unacceptable_removals() {
        let mut store = UserStore::in_memory();
        store.remove("");
        store.remove("two words");
        assert!(!store.is_removed(""));
        assert!(!store.is_removed("two words"));
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

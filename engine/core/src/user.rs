//! Words the user taught the keyboard. An append-only log in the extension's
//! own container, replayed into a small map at load. Learned words are never
//! auto-replaced. Words the user removed from the bar are kept too, so they
//! are never suggested again until learned anew. I/O failures never stop typing: the store keeps working in
//! memory and simply doesn't persist.
//!
//! Log lines: `L` learned, `S` seen (typed once), `R` removed from the bar, `F` one
//! word forgotten (not blocked), `U` a removed word restored (not learned).
//!
//! The store also keeps what `personal` learns from use, in lines of tab-separated fields,
//! each with an optional count at the end (1 when absent; compaction writes it): `N word`
//! a use of a known or learned word, `P previous word` a word after another in one
//! sentence, `T before previous word` after two, `C typed chosen` a suggestion chosen
//! over the default for a typed form, `H m` or `H e` a word from the Latin keyboard that
//! ended up Malayalam or English. Removing or forgetting a word (`R`, `F`) drops all of
//! these that hold it. A log from before these lines loads as it did; an older build
//! skips them.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::personal::{Followed, Habit, Personal};

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
    /// Keys (`key`) of words removed from the bar, with the clock when removed: never
    /// suggested until learned again.
    removed: HashMap<String, u64>,
    clock: u64,
    log_lines: usize,
    /// The log ended mid-line and could not be truncated; the next append starts a fresh line.
    dirty_tail: bool,
    /// Phrases, uses, choices and the language habit (`personal`).
    personal: Personal,
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
            store.replay(line);
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

    /// Applies one log line; a malformed one is skipped.
    fn replay(&mut self, line: &str) {
        let fields: Vec<&str> = line.split('\t').collect();
        let ok = |words: &[&str]| words.iter().all(|w| acceptable(w));
        match fields.as_slice() {
            ["L", word] if acceptable(word) => self.apply_learn(word),
            ["S", word] if acceptable(word) => self.apply_seen(word),
            ["R", word] if acceptable(word) => self.apply_remove(word),
            ["F", word] if acceptable(word) => self.apply_forget(word),
            ["U", word] if acceptable(word) => self.apply_restore(word),
            ["N", word, n @ ..] if ok(&[word]) => {
                if let Some(n) = count(n) {
                    self.personal.apply_use(word, n);
                }
            }
            ["P", previous, word, n @ ..] if ok(&[previous, word]) => {
                if let Some(n) = count(n) {
                    self.personal.apply_pair(previous, word, n);
                }
            }
            ["T", before, previous, word, n @ ..] if ok(&[before, previous, word]) => {
                if let Some(n) = count(n) {
                    self.personal.apply_trigram(before, previous, word, n);
                }
            }
            ["C", typed, chosen, n @ ..] if ok(&[typed, chosen]) => {
                if let Some(n) = count(n) {
                    self.personal.apply_choice(typed, chosen, n);
                }
            }
            ["H", language, n @ ..] => {
                let habit = match *language {
                    "m" => Some(Habit::Malayalam),
                    "e" => Some(Habit::English),
                    _ => None,
                };
                if let (Some(habit), Some(n)) = (habit, count(n)) {
                    self.personal.apply_habit(habit, n);
                }
            }
            _ => {}
        }
    }

    pub fn is_learned(&self, word: &str) -> bool {
        self.entries.get(&key(word)).is_some_and(|e| e.learned)
    }

    pub fn learned_count(&self) -> usize {
        self.entries.values().filter(|e| e.learned).count()
    }

    /// Whether the user removed this word from the bar (ASCII case-insensitive).
    pub fn is_removed(&self, word: &str) -> bool {
        self.removed.contains_key(&key(word))
    }

    /// Forgets a word and keeps it from being suggested again, until it is learned.
    pub fn remove(&mut self, word: &str) {
        if !acceptable(word) {
            return;
        }
        self.apply_remove(word);
        self.append("R", word);
    }

    /// A word was used: committed after `previous` (and `before_previous` before that), in
    /// the same sentence. `counted`: count this use (a word known or learned). `habit`: the
    /// language a word from the Latin keyboard ended up in. One write for all of it.
    pub fn note_used(
        &mut self,
        word: &str,
        previous: Option<&str>,
        before_previous: Option<&str>,
        counted: bool,
        habit: Option<Habit>,
    ) {
        if !acceptable(word) {
            return;
        }
        let previous = previous.filter(|w| acceptable(w));
        let before_previous = before_previous.filter(|w| acceptable(w) && previous.is_some());
        let mut lines = Vec::new();
        if counted {
            self.personal.apply_use(word, 1);
            lines.push(format!("N\t{word}"));
        }
        if let Some(previous) = previous {
            self.personal.apply_pair(previous, word, 1);
            lines.push(format!("P\t{previous}\t{word}"));
            if let Some(before) = before_previous {
                self.personal.apply_trigram(before, previous, word, 1);
                lines.push(format!("T\t{before}\t{previous}\t{word}"));
            }
        }
        if let Some(habit) = habit {
            self.personal.apply_habit(habit, 1);
            let tag = if habit == Habit::Malayalam { "m" } else { "e" };
            lines.push(format!("H\t{tag}"));
        }
        self.append_lines(&lines);
    }

    /// The user chose `chosen` over the default suggestion for `typed`: it is offered first
    /// for `typed` from now on, until another is chosen.
    pub fn note_choice(&mut self, typed: &str, chosen: &str) {
        if !acceptable(typed) || !acceptable(chosen) || typed.eq_ignore_ascii_case(chosen) {
            return;
        }
        self.personal.apply_choice(typed, chosen, 1);
        self.append_lines(&[format!("C\t{typed}\t{chosen}")]);
    }

    /// See `Personal::followers`.
    pub fn followers(
        &self,
        before_previous: Option<&str>,
        previous: &str,
        limit: usize,
    ) -> Vec<Followed> {
        self.personal.followers(before_previous, previous, limit)
    }

    /// How many times `word` was used (`note_used` with `counted`).
    pub fn uses(&self, word: &str) -> u32 {
        self.personal.uses(word)
    }

    /// Whether any use is counted: when none is, `uses` need not be asked.
    pub fn has_uses(&self) -> bool {
        self.personal.has_uses()
    }

    /// The suggestion last chosen for `typed` (`note_choice`).
    pub fn choice(&self, typed: &str) -> Option<&str> {
        self.personal.choice(typed)
    }

    /// Recent Latin-keyboard words that ended up Malayalam, and English (`Personal::habit`).
    pub fn habit(&self) -> (u32, u32) {
        self.personal.habit()
    }

    /// Forgets one word (learned or seen once) without blocking it: typed again, it is
    /// learned again. Writes nothing for a word the store doesn't hold.
    pub fn forget(&mut self, word: &str) {
        if !acceptable(word) || !self.entries.contains_key(&key(word)) {
            return;
        }
        self.apply_forget(word);
        self.append("F", word);
    }

    /// Un-removes a word, so it may be suggested again. It is not learned by this.
    pub fn restore(&mut self, word: &str) {
        if !acceptable(word) || !self.is_removed(word) {
            return;
        }
        self.apply_restore(word);
        self.append("U", word);
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
    /// A removed word starts counting again from zero: typed `TIMES_TO_LEARN` more times,
    /// it is learned and no longer removed (the user re-adds it by using it).
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

    /// Every learned word, as shown, in no particular order.
    pub fn learned(&self) -> impl Iterator<Item = &str> {
        self.entries
            .values()
            .filter(|e| e.learned)
            .map(|e| e.display.as_str())
    }

    /// Every learned word, as shown, most recently used first.
    pub fn learned_words(&self) -> Vec<String> {
        let mut found: Vec<&Entry> = self.entries.values().filter(|e| e.learned).collect();
        found.sort_by_key(|e| std::cmp::Reverse(e.last_used));
        found.into_iter().map(|e| e.display.clone()).collect()
    }

    /// Every word removed from the bar, most recently removed first. As keyed: ASCII
    /// letters lowercased.
    pub fn removed_words(&self) -> Vec<String> {
        let mut found: Vec<(&String, &u64)> = self.removed.iter().collect();
        found.sort_by_key(|&(_, &at)| std::cmp::Reverse(at));
        found.into_iter().map(|(k, _)| k.clone()).collect()
    }

    pub fn forget_all(&mut self) {
        self.entries.clear();
        self.removed.clear();
        self.personal.clear();
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
        self.clock += 1;
        let k = key(word);
        self.entries.remove(&k);
        self.removed.insert(k, self.clock);
        self.personal.purge(word);
    }

    fn apply_forget(&mut self, word: &str) {
        self.entries.remove(&key(word));
        self.personal.purge(word);
    }

    fn apply_restore(&mut self, word: &str) {
        self.removed.remove(&key(word));
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
            self.removed.remove(&key(word));
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
        self.append_lines(&[format!("{tag}\t{word}")]);
    }

    /// Appends one event's lines (given without their newlines).
    fn append_lines(&mut self, lines: &[String]) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if lines.is_empty() {
            return;
        }
        // One write call per event, so a kill can leave at worst a partial tail.
        let mut text = String::new();
        if self.dirty_tail {
            text.push('\n');
        }
        for line in lines {
            text.push_str(line);
            text.push('\n');
        }
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            if file.write_all(text.as_bytes()).is_ok() {
                self.dirty_tail = false;
                self.log_lines += lines.len();
            } else {
                // A failed write (e.g. a full disk) may have left part of the line.
                self.dirty_tail = true;
            }
        }
        // Past what compaction would write by a margin, so that it runs once in many events.
        if self.log_lines > self.state_lines() + CAPACITY * 4 {
            self.compact(&path);
        }
    }

    /// About as many lines as `compact` writes.
    fn state_lines(&self) -> usize {
        self.entries.len() + self.removed.len() + self.personal.len()
    }

    /// Rewrites the log from the map so it never grows without bound.
    fn compact(&mut self, path: &Path) {
        let mut entries: Vec<&Entry> = self.entries.values().collect();
        entries.sort_by_key(|e| e.last_used);
        let mut text = String::new();
        let mut lines = 0;
        // A removed word has no entry (and a learned one is not removed), so the two need
        // not interleave; removals keep their own order for `removed_words`. Forgotten and
        // restored words are simply absent, so no `F` or `U` line is written.
        let mut removed: Vec<(&String, &u64)> = self.removed.iter().collect();
        removed.sort_by_key(|&(_, &at)| at);
        for (k, _) in removed {
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
        // After the words: replaying an `R` or `F` drops the phrases of its word.
        let personal = self.personal.lines();
        lines += personal.lines().count();
        text.push_str(&personal);
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

/// The count at the end of a `personal` line: 1 when absent, and never 0.
fn count(field: &[&str]) -> Option<u32> {
    match field {
        [] => Some(1),
        [n] => n.parse().ok().filter(|&n| n > 0),
        _ => None,
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
    fn a_removed_word_typed_twice_more_is_learned_again() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = UserStore::open(dir.path());
            store.note_typed("Sam.2024@example.com");
            store.note_typed("Sam.2024@example.com");
            store.remove("sam.2024@example.com");
            store.note_typed("sam.2024@example.com");
            assert!(store.is_removed("sam.2024@example.com"), "once is not enough");
            store.note_typed("sam.2024@example.com");
            assert!(!store.is_removed("sam.2024@example.com"));
            assert!(store.is_learned("sam.2024@example.com"));
            assert_eq!(store.completions("neo", 1), ["sam.2024@example.com"], "as re-typed");
        }
        let store = UserStore::open(dir.path());
        assert!(!store.is_removed("sam.2024@example.com"), "the log replays in order");
        assert!(store.is_learned("sam.2024@example.com"));
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
    fn learned_words_are_listed_most_recently_used_first() {
        let mut store = UserStore::in_memory();
        store.learn("Thom");
        store.learn("Anu");
        store.note_typed("once");
        store.learn("Kochi");
        store.remove("Kochi");
        store.learn("thom"); // used again; shown as first learned
        assert_eq!(store.learned_words(), ["Thom", "Anu"]);
    }

    #[test]
    fn removed_words_are_listed_most_recently_removed_first_across_reopen_and_compaction() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = UserStore::open(dir.path());
            store.remove("hello");
            store.remove("Thom");
            store.remove("കിളി");
            assert_eq!(store.removed_words(), ["കിളി", "thom", "hello"]);
        }
        let mut store = UserStore::open(dir.path());
        assert_eq!(store.removed_words(), ["കിളി", "thom", "hello"]);
        for _ in 0..(CAPACITY * 4 + 10) {
            store.learn("Anu");
        }
        let text = fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
        assert!(text.lines().count() < 100, "log was compacted");
        assert_eq!(
            UserStore::open(dir.path()).removed_words(),
            ["കിളി", "thom", "hello"]
        );
    }

    #[test]
    fn forgetting_one_word_does_not_block_it() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = UserStore::open(dir.path());
            store.learn("Thom");
            store.learn("Anu");
            store.forget("thom");
            assert!(!store.is_learned("Thom"));
            assert!(!store.is_removed("Thom"), "forgotten, not removed");
            assert!(store.is_learned("Anu"));
            assert_eq!(store.learned_words(), ["Anu"]);
        }
        let text = fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
        assert!(text.ends_with("F\tthom\n"));
        let mut store = UserStore::open(dir.path());
        assert!(!store.is_learned("Thom"), "the log replays the forget");
        assert!(!store.is_removed("Thom"));
        assert!(store.is_learned("Anu"));
        store.note_typed("Thom");
        assert!(!store.is_learned("Thom"), "counting starts again");
        store.note_typed("Thom");
        assert!(store.is_learned("Thom"));
    }

    #[test]
    fn a_word_learned_after_being_forgotten_replays_as_learned() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = UserStore::open(dir.path());
            store.learn("Thom");
            store.forget("Thom");
            store.learn("Thom");
        }
        assert!(UserStore::open(dir.path()).is_learned("Thom"));
    }

    #[test]
    fn forgetting_an_unknown_word_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = UserStore::open(dir.path());
        store.learn("Thom");
        store.forget("Anu");
        store.forget("");
        store.forget("two words");
        let text = fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
        assert_eq!(text, "L\tThom\n");
    }

    #[test]
    fn restoring_a_removed_word_unblocks_it_without_learning_it() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = UserStore::open(dir.path());
            store.learn("Thom");
            store.remove("Thom");
            store.remove("hello");
            store.restore("thom");
            assert!(!store.is_removed("Thom"));
            assert!(!store.is_learned("Thom"), "restored, not learned");
            assert_eq!(store.removed_words(), ["hello"]);
            store.restore("never-removed");
            store.restore("");
        }
        let text = fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
        assert!(
            text.ends_with("U\tthom\n"),
            "nothing written for a word not removed"
        );
        let store = UserStore::open(dir.path());
        assert!(!store.is_removed("Thom"), "the log replays the restore");
        assert!(store.is_removed("hello"));
        assert!(!store.is_learned("Thom"));
    }

    #[test]
    fn forgets_and_restores_survive_compaction() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = UserStore::open(dir.path());
        store.learn("Thom");
        store.learn("Kochi");
        store.remove("hello");
        store.remove("world");
        store.forget("Thom");
        store.restore("hello");
        for _ in 0..(CAPACITY * 4 + 10) {
            store.learn("Anu");
        }
        let text = fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
        assert!(text.lines().count() < 100, "log was compacted");
        assert!(
            !text.contains("F\t") && !text.contains("U\t"),
            "compaction writes only state"
        );
        let reopened = UserStore::open(dir.path());
        assert_eq!(reopened.learned_words(), ["Anu", "Kochi"]);
        assert_eq!(reopened.removed_words(), ["world"]);
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

    fn followers(store: &UserStore, before: Option<&str>, previous: &str) -> Vec<String> {
        store
            .followers(before, previous, 5)
            .into_iter()
            .map(|f| f.word)
            .collect()
    }

    /// "hello how are you", committed word by word as the engine notes it.
    fn type_hello_how_are_you(store: &mut UserStore) {
        store.note_used("hello", None, None, true, Some(Habit::English));
        store.note_used("how", Some("hello"), None, true, Some(Habit::English));
        store.note_used(
            "are",
            Some("how"),
            Some("hello"),
            true,
            Some(Habit::English),
        );
        store.note_used("you", Some("are"), Some("how"), true, Some(Habit::English));
    }

    #[test]
    fn phrases_uses_choices_and_habit_persist() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = UserStore::open(dir.path());
            type_hello_how_are_you(&mut store);
            store.note_used("ഇന്ന്", Some("you"), None, true, Some(Habit::Malayalam));
            store.note_choice("innu", "ഇന്ന്");
        }
        let text = fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
        assert!(text.starts_with("N\thello\nH\te\nN\thow\nP\thello\thow\nH\te\n"));
        assert!(text.contains("T\thow\tare\tyou\n"));
        assert!(text.ends_with("C\tinnu\tഇന്ന്\n"));
        let store = UserStore::open(dir.path());
        assert_eq!(followers(&store, None, "Hello"), ["how"]);
        assert_eq!(followers(&store, Some("how"), "are"), ["you"]);
        assert_eq!(store.uses("HOW"), 1);
        assert_eq!(store.choice("Innu"), Some("ഇന്ന്"));
        assert_eq!(store.habit(), (1, 4));
        assert_eq!(store.learned_count(), 0, "using known words learns no word");
    }

    #[test]
    fn nothing_is_noted_for_unacceptable_words() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = UserStore::open(dir.path());
        store.note_used("two words", Some("hello"), None, true, None);
        store.note_used("hello", None, None, false, None);
        store.note_choice("same", "Same");
        store.note_choice("", "x");
        assert!(!dir.path().join(LOG_NAME).exists());
        // An unacceptable previous word only loses the phrase.
        store.note_used("how", Some("a\tb"), Some("x"), false, None);
        assert!(store.followers(None, "a\tb", 5).is_empty());
        assert!(store.personal.is_empty());
    }

    #[test]
    fn counts_in_lines_are_optional_and_malformed_ones_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let log = "N\thow\t3\nN\tbad\t0\nN\tbad\tx\nP\thello\thow\t2\nP\thello\n\
                   P\thello\tkitty\t1\t9\nT\thow\tare\tyou\nC\tinnu\tഇന്ന്\t4\nH\tm\t7\nH\tq\nH\te\n";
        fs::write(dir.path().join(LOG_NAME), log).unwrap();
        let store = UserStore::open(dir.path());
        assert_eq!(store.uses("how"), 3);
        assert_eq!(store.uses("bad"), 0);
        assert_eq!(
            store.followers(None, "hello", 5),
            [Followed {
                word: "how".into(),
                score: 2
            }]
        );
        assert_eq!(followers(&store, Some("how"), "are"), ["you"]);
        assert_eq!(store.choice("innu"), Some("ഇന്ന്"));
        assert_eq!(store.habit(), (7, 1));
    }

    #[test]
    fn a_log_from_before_phrases_loads_as_before() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(LOG_NAME),
            "L\tThom\nS\tAnu\nR\thello\nL\tKochi\nF\tkochi\nU\thello\n",
        )
        .unwrap();
        let mut store = UserStore::open(dir.path());
        assert_eq!(store.learned_words(), ["Thom"]);
        assert!(!store.is_removed("hello"));
        store.note_typed("Anu");
        assert!(store.is_learned("Anu"));
        assert!(store.personal.is_empty());
    }

    #[test]
    fn removing_or_forgetting_a_word_drops_its_phrases_and_choices_for_good() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = UserStore::open(dir.path());
            type_hello_how_are_you(&mut store);
            store.note_used("kitty", Some("hello"), None, true, None);
            store.note_choice("kity", "kitty");
            store.note_choice("hw", "how");
            store.remove("kitty");
            store.learn("Thom");
            store.note_used("Thom", Some("hello"), None, true, None);
            store.forget("thom");
            assert_eq!(followers(&store, None, "hello"), ["how"]);
            assert_eq!(store.choice("kity"), None);
            assert_eq!(store.choice("hw"), Some("how"));
            assert_eq!(store.uses("kitty"), 0);
        }
        let store = UserStore::open(dir.path());
        assert_eq!(
            followers(&store, None, "hello"),
            ["how"],
            "the log replays the cascade"
        );
        assert_eq!(store.choice("kity"), None);
        assert_eq!(store.uses("thom"), 0);
        assert_eq!(store.uses("hello"), 1);
    }

    #[test]
    fn forget_all_clears_phrases_uses_choices_and_habit() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = UserStore::open(dir.path());
        type_hello_how_are_you(&mut store);
        store.note_choice("innu", "ഇന്ന്");
        store.forget_all();
        assert!(store.personal.is_empty());
        assert_eq!(store.habit(), (0, 0));
        let reopened = UserStore::open(dir.path());
        assert!(reopened.personal.is_empty());
        assert!(reopened.followers(None, "hello", 5).is_empty());
    }

    #[test]
    fn compaction_keeps_phrases_counts_recency_and_cascades() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = UserStore::open(dir.path());
        type_hello_how_are_you(&mut store);
        type_hello_how_are_you(&mut store);
        store.note_used("kitty", Some("hello"), None, true, None);
        store.note_used("there", Some("hello"), None, true, None);
        store.note_choice("innu", "ഇന്ന്");
        store.remove("there");
        for _ in 0..(CAPACITY * 4 + 10) {
            store.note_used("ok", None, None, true, None);
        }
        let text = fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
        assert!(text.lines().count() < 100, "log was compacted");
        assert!(text.contains("P\thello\thow\t2\n"));
        let reopened = UserStore::open(dir.path());
        assert_eq!(followers(&reopened, None, "hello"), ["how", "kitty"]);
        assert_eq!(followers(&reopened, Some("how"), "are"), ["you"]);
        assert_eq!(reopened.uses("how"), 2);
        assert!(reopened.uses("ok") > CAPACITY as u32 * 4);
        assert_eq!(reopened.choice("innu"), Some("ഇന്ന്"));
        assert_eq!(reopened.habit(), (0, 8));
        assert!(reopened.is_removed("there"));
    }

    #[test]
    fn a_full_store_stays_small_on_disk() {
        use crate::personal::{CHOICE_CAPACITY, PAIR_CAPACITY};
        let dir = tempfile::tempdir().unwrap();
        let mut store = UserStore::open(dir.path());
        let size = |dir: &Path| fs::metadata(dir.join(LOG_NAME)).unwrap().len();
        let mut largest = 0;
        // Every table past its cap, with words of a typical length, through compactions.
        for i in 0..CAPACITY + 100 {
            store.learn(&format!("learned{i}"));
        }
        for round in 0..3 {
            for i in 0..PAIR_CAPACITY + 100 {
                let (a, b) = (format!("first{round}x{i}"), format!("second{i}"));
                let c = format!("third{i}");
                store.note_used(&c, Some(&b), Some(&a), true, Some(Habit::English));
                largest = largest.max(size(dir.path()));
            }
        }
        for i in 0..CHOICE_CAPACITY + 10 {
            store.note_choice(&format!("typed{i}"), &format!("chosen{i}"));
        }
        assert!(largest < 1_000_000, "{largest} bytes");
        let reopened = UserStore::open(dir.path());
        assert_eq!(reopened.learned_count(), CAPACITY);
        assert_eq!(reopened.personal.len(), store.personal.len());
    }
}

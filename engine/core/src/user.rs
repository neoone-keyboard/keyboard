//! Words the user taught the keyboard. An append-only log in the extension's
//! own container, replayed into a small map at load. Learned words are never
//! auto-replaced. Words the user removed from the bar are kept too, so they
//! are never suggested again until learned anew. I/O failures never stop typing: the store keeps working in
//! memory and simply doesn't persist.
//!
//! Log lines: `L` learned, `S` seen (typed once), `R` removed from the bar, `F` one
//! word forgotten (not blocked), `U` a removed word restored (not learned). A word holds no
//! whitespace, except a phone number typed in groups (`+91 55501 12345`), with single spaces;
//! an older build skips its lines.
//!
//! The store also keeps what `personal` learns from use, in lines of tab-separated fields,
//! each with an optional count at the end (1 when absent; compaction writes it): `N word`
//! a use of a known or learned word, `P previous word` a word after another in one
//! sentence, `T before previous word` after two, `C typed chosen` a suggestion chosen
//! over the default for a typed form, `H ml` or `H en` a word from the Latin keyboard that
//! ended up in that language, by its id. Removing or forgetting a word (`R`, `F`) drops all of
//! these that hold it. A log from before these lines loads as it did; an older build
//! skips them. A log from before languages were ids tags the habit `H m` and `H e`: those
//! read as `ml` and `en`, and opening such a log rewrites it (`compact`) in the new tags.
//!
//! Learned words stay in one log for every language; a word's language follows from its
//! script and the packs, except a word picked from the bar, which says its own: `W lang word`
//! or `W lang word typed`, a native-script word of the language `lang` (a pack's id) that the
//! user tapped in the bar though the pack's lexicon doesn't hold it, and the Latin letters it
//! was picked for, lowercased (left out when they were only the word's beginning). It is a
//! learned word like any other (the list, uses, phrases, `R` and `F`), and the engine also
//! finds it from Latin letters as it finds the pack's own words (`own::OwnWords`). A word keeps
//! the last `MAX_TYPED_FORMS` runs of letters. Each pick writes `L word` before its `W` line
//! the first time, so an older build, which skips `W`, still has the word learned; a log
//! without `W` lines loads as it did. Splitting the log by language later is one more
//! migration.

use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::lang::LangId;
use crate::personal::{self, Followed, Personal};
use crate::text;

/// Most words kept; the least recently used are evicted beyond this.
pub const CAPACITY: usize = 5_000;
/// A word typed this many times (and not in any lexicon) is learned.
pub const TIMES_TO_LEARN: u32 = 2;
/// Longest word stored, in chars.
pub const MAX_WORD_CHARS: usize = 48;
/// Most runs of Latin letters kept for a picked word (`UserStore::pick`); the oldest goes.
pub const MAX_TYPED_FORMS: usize = 4;
/// Longest language id in a `W` line, in bytes.
const MAX_LANG_BYTES: usize = 32;

const LOG_NAME: &str = "learned-words.log";

#[derive(Debug, Clone)]
struct Entry {
    display: String,
    seen: u32,
    learned: bool,
    last_used: u64,
    /// Set for a word picked from the bar (`UserStore::pick`).
    picked: Option<Picked>,
}

/// What a `W` line says of a word.
#[derive(Debug, Clone, PartialEq)]
struct Picked {
    lang: LangId,
    /// The Latin letters it was picked for, lowercased, the latest last.
    typed: Vec<String>,
}

impl Picked {
    /// The `W` lines that record it: one for each run of letters, or one without any.
    fn lines(&self) -> usize {
        self.typed.len().max(1)
    }
}

/// A word picked from the bar (`UserStore::picked`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickedWord<'a> {
    pub word: &'a str,
    /// The language it was offered as a word of: a pack's id.
    pub lang: &'a LangId,
    /// The Latin letters it was picked for, lowercased, the latest last.
    pub typed: &'a [String],
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
    /// A habit line with a tag from before languages were ids was replayed.
    old_habit_tags: bool,
    /// Counts every change to the picked words (`picked_generation`).
    picked_generation: u64,
    /// The `W` lines `compact` would write.
    picked_lines: usize,
}

/// Lookups ignore ASCII case, so "Thom" typed at a sentence start is still the learned "Thom".
fn key(word: &str) -> String {
    word.to_ascii_lowercase()
}

/// No word holds whitespace but a phone number typed in groups (`+91 55501 12345`), whose
/// single spaces are kept as typed.
fn acceptable(word: &str) -> bool {
    !word.is_empty()
        && word.chars().count() <= MAX_WORD_CHARS
        && !word.chars().any(|c| c.is_control() || c.is_whitespace() && c != ' ')
        && (!word.contains(' ') || text::is_phone_number(word))
}

/// A word that may be in a phrase, a use or a choice (`personal`): one without a space, as
/// a phrase's context joins its words with one. A phone number never is.
fn in_phrase(word: &str) -> bool {
    acceptable(word) && !word.contains(' ')
}

/// A language id a `W` line may carry: a pack's manifest id (`hi`), short and without
/// whitespace.
fn lang_tag(lang: &str) -> bool {
    !lang.is_empty()
        && lang.len() <= MAX_LANG_BYTES
        && !lang.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The Latin letters a word may be picked for: ASCII letters only.
fn latin_letters(typed: &str) -> bool {
    !typed.is_empty()
        && typed.len() <= MAX_WORD_CHARS
        && typed.bytes().all(|b| b.is_ascii_alphabetic())
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
        if store.old_habit_tags {
            store.compact(&path);
        }
        store
    }

    /// Applies one log line; a malformed one is skipped.
    fn replay(&mut self, line: &str) {
        let fields: Vec<&str> = line.split('\t').collect();
        let ok = |words: &[&str]| words.iter().all(|w| in_phrase(w));
        match fields.as_slice() {
            ["L", word] if acceptable(word) => self.apply_learn(word),
            ["S", word] if acceptable(word) => self.apply_seen(word),
            ["R", word] if acceptable(word) => self.apply_remove(word),
            ["F", word] if acceptable(word) => self.apply_forget(word),
            ["U", word] if acceptable(word) => self.apply_restore(word),
            ["W", lang, word] if lang_tag(lang) && in_phrase(word) => {
                self.apply_pick(&LangId(lang.to_string()), word, None);
            }
            ["W", lang, word, typed]
                if lang_tag(lang) && in_phrase(word) && latin_letters(typed) =>
            {
                self.apply_pick(&LangId(lang.to_string()), word, Some(typed));
            }
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
            ["H", language, n @ ..] if !language.is_empty() => {
                if let Some(n) = count(n) {
                    self.old_habit_tags |= personal::is_old_habit_tag(language);
                    self.personal
                        .apply_habit(&personal::habit_lang(language), n);
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
        habit: Option<&LangId>,
    ) {
        if !in_phrase(word) {
            return;
        }
        let previous = previous.filter(|w| in_phrase(w));
        let before_previous = before_previous.filter(|w| in_phrase(w) && previous.is_some());
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
        if let Some(lang) = habit {
            self.personal.apply_habit(lang, 1);
            lines.push(format!("H\t{}", lang.0));
        }
        self.append_lines(&lines);
    }

    /// The user chose `chosen` over the default suggestion for `typed`: it is offered first
    /// for `typed` from now on, until another is chosen.
    pub fn note_choice(&mut self, typed: &str, chosen: &str) {
        if !in_phrase(typed) || !in_phrase(chosen) || typed.eq_ignore_ascii_case(chosen) {
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

    /// Used words (`uses`) longer than `prefix` that start with it, ASCII letters lowercased
    /// (`Personal::used_starting_with`). A removed word has no uses.
    pub fn used_completions(&self, prefix: &str) -> Vec<String> {
        self.personal
            .used_starting_with(prefix)
            .map(str::to_string)
            .collect()
    }

    /// Whether any use is counted: when none is, `uses` need not be asked.
    pub fn has_uses(&self) -> bool {
        self.personal.has_uses()
    }

    /// The suggestion last chosen for `typed` (`note_choice`).
    pub fn choice(&self, typed: &str) -> Option<&str> {
        self.personal.choice(typed)
    }

    /// Recent Latin-keyboard words by the language they ended up in (`Personal::habit`).
    pub fn habit(&self) -> &BTreeMap<LangId, u32> {
        self.personal.habit()
    }

    /// Forgets one word (learned or seen once) without blocking it: typed again, it is
    /// learned again. Its phrases, uses and choices go too. Writes nothing for a word the
    /// store doesn't hold.
    pub fn forget(&mut self, word: &str) {
        let held = self.entries.contains_key(&key(word)) || self.personal.mentions(word);
        if !acceptable(word) || !held {
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

    /// The user tapped `word` in the bar, offered as a word of `lang` (a pack's id) though
    /// the pack's lexicon doesn't hold it: it is learned at once, with its language and the
    /// Latin letters it was picked for (`typed`; None, or letters that are no Latin word, add
    /// none). A removed word is restored.
    pub fn pick(&mut self, lang: &LangId, word: &str, typed: Option<&str>) {
        if !lang_tag(&lang.0) || !in_phrase(word) {
            return;
        }
        let typed = typed.filter(|t| latin_letters(t)).map(str::to_ascii_lowercase);
        let mut lines = Vec::new();
        if !self.is_learned(word) {
            lines.push(format!("L\t{word}"));
        }
        lines.push(match &typed {
            Some(typed) => format!("W\t{}\t{word}\t{typed}", lang.0),
            None => format!("W\t{}\t{word}", lang.0),
        });
        self.apply_pick(lang, word, typed.as_deref());
        self.append_lines(&lines);
    }

    /// The words picked from the bar (`pick`), in no particular order.
    pub fn picked(&self) -> impl Iterator<Item = PickedWord<'_>> {
        self.entries.values().filter_map(|e| {
            let picked = e.picked.as_ref()?;
            Some(PickedWord {
                word: &e.display,
                lang: &picked.lang,
                typed: &picked.typed,
            })
        })
    }

    /// What `word` was picked as (`pick`); None for a word that wasn't.
    pub fn picked_word(&self, word: &str) -> Option<PickedWord<'_>> {
        let entry = self.entries.get(&key(word))?;
        let picked = entry.picked.as_ref()?;
        Some(PickedWord {
            word: &entry.display,
            lang: &picked.lang,
            typed: &picked.typed,
        })
    }

    /// Changes whenever the picked words do (one is picked, picked for other letters,
    /// removed, forgotten or evicted): what is built from `picked` is rebuilt then. 0 until
    /// the first.
    pub fn picked_generation(&self) -> u64 {
        self.picked_generation
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
        if self.picked_lines > 0 {
            self.picked_generation += 1;
            self.picked_lines = 0;
        }
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
        self.drop_entry(&k);
        self.removed.insert(k, self.clock);
        self.personal.purge(word);
    }

    fn apply_forget(&mut self, word: &str) {
        self.drop_entry(&key(word));
        self.personal.purge(word);
    }

    /// Takes the entry keyed `k` out, with what it was picked for.
    fn drop_entry(&mut self, k: &str) {
        if let Some(picked) = self.entries.remove(k).and_then(|e| e.picked) {
            self.picked_generation += 1;
            self.picked_lines -= picked.lines();
        }
    }

    /// Learns `word` as a picked word of `lang`, and adds `typed` (lowercase Latin letters) as
    /// the latest letters it was picked for.
    fn apply_pick(&mut self, lang: &LangId, word: &str, typed: Option<&str>) {
        self.apply_learn(word);
        let Some(entry) = self.entries.get_mut(&key(word)) else {
            return;
        };
        let before = entry.picked.clone();
        let lines_before = before.as_ref().map_or(0, Picked::lines);
        let picked = entry.picked.get_or_insert_with(|| Picked {
            lang: lang.clone(),
            typed: Vec::new(),
        });
        picked.lang = lang.clone();
        if let Some(typed) = typed {
            let typed = typed.to_ascii_lowercase();
            picked.typed.retain(|t| *t != typed);
            picked.typed.push(typed);
            if picked.typed.len() > MAX_TYPED_FORMS {
                picked.typed.remove(0);
            }
        }
        self.picked_lines = self.picked_lines - lines_before + picked.lines();
        if before.as_ref() != Some(&*picked) {
            self.picked_generation += 1;
        }
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
            picked: None,
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
            self.drop_entry(&oldest);
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
        self.entries.len() + self.picked_lines + self.removed.len() + self.personal.len()
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
                if let Some(picked) = &e.picked {
                    let lang = &picked.lang.0;
                    if picked.typed.is_empty() {
                        text.push_str(&format!("W\t{lang}\t{}\n", e.display));
                        lines += 1;
                    }
                    for typed in &picked.typed {
                        text.push_str(&format!("W\t{lang}\t{}\t{typed}\n", e.display));
                        lines += 1;
                    }
                }
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
    fn a_phone_number_keeps_its_spaces_and_no_other_word_has_one() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(LOG_NAME), "L\tok\nL\ttwo words\nS\t55501 1234\n").unwrap();
        let mut store = UserStore::open(dir.path());
        assert!(store.is_learned("ok"), "an older log loads as before");
        assert_eq!(store.learned_count(), 1);
        store.note_typed("+91 55501 12345");
        store.note_typed("+91 55501 12345");
        store.learn("55501 12345");
        store.learn("two words");
        assert_eq!(store.completions("+91 98", 5), ["+91 55501 12345"]);
        assert_eq!(store.completions("55501", 5), ["55501 12345"]);
        let reopened = UserStore::open(dir.path());
        assert!(reopened.is_learned("+91 55501 12345"));
        assert!(!reopened.is_learned("two words"));
        assert!(!reopened.is_learned("55501 1234"));
        store.remove("55501 12345");
        store.forget("+91 55501 12345");
        let mut reopened = UserStore::open(dir.path());
        assert_eq!(reopened.learned_words(), ["ok"]);
        assert_eq!(reopened.removed_words(), ["55501 12345"]);
        for _ in 0..(CAPACITY * 4 + 10) {
            reopened.learn("+44 20 7946 0958");
        }
        let text = fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
        assert!(text.lines().count() < 100, "log was compacted");
        let compacted = UserStore::open(dir.path());
        assert!(compacted.is_learned("+44 20 7946 0958"));
        assert!(compacted.is_removed("55501 12345"));
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

    /// The habit's Malayalam and English counts.
    fn ml_en(store: &UserStore) -> (u32, u32) {
        let h = store.habit();
        let n = |id: LangId| h.get(&id).copied().unwrap_or(0);
        (n(LangId("ml".into())), n(LangId::english()))
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
        store.note_used("hello", None, None, true, Some(&LangId::english()));
        store.note_used("how", Some("hello"), None, true, Some(&LangId::english()));
        store.note_used(
            "are",
            Some("how"),
            Some("hello"),
            true,
            Some(&LangId::english()),
        );
        store.note_used(
            "you",
            Some("are"),
            Some("how"),
            true,
            Some(&LangId::english()),
        );
    }

    #[test]
    fn phrases_uses_choices_and_habit_persist() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = UserStore::open(dir.path());
            type_hello_how_are_you(&mut store);
            store.note_used("ഇന്ന്", Some("you"), None, true, Some(&LangId("ml".into())));
            store.note_choice("innu", "ഇന്ന്");
        }
        let text = fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
        assert!(text.starts_with("N\thello\nH\ten\nN\thow\nP\thello\thow\nH\ten\n"));
        assert!(text.contains("T\thow\tare\tyou\n"));
        assert!(text.ends_with("C\tinnu\tഇന്ന്\n"));
        let store = UserStore::open(dir.path());
        assert_eq!(followers(&store, None, "Hello"), ["how"]);
        assert_eq!(followers(&store, Some("how"), "are"), ["you"]);
        assert_eq!(store.uses("HOW"), 1);
        assert_eq!(store.choice("Innu"), Some("ഇന്ന്"));
        assert_eq!(ml_en(&store), (1, 4));
        assert_eq!(store.learned_count(), 0, "using known words learns no word");
    }

    #[test]
    fn nothing_is_noted_for_unacceptable_words() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = UserStore::open(dir.path());
        store.note_used("two words", Some("hello"), None, true, None);
        store.note_used("+91 55501 12345", Some("call"), None, true, None);
        store.note_choice("55501 12345", "x");
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
        assert_eq!(ml_en(&store), (7, 1));
    }

    #[test]
    fn habit_tags_from_before_language_ids_load_and_are_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        // Old tags, a new tag for the same language, and enough lines to halve the counts.
        let mut log = String::from("L\tThom\nH\tm\t5\nH\te\nH\tml\t2\nN\thow\t3\n");
        for _ in 0..150 {
            log.push_str("H\te\nH\tm\n");
        }
        log.push_str("H\ten\t4\nH\thi\n");
        fs::write(dir.path().join(LOG_NAME), &log).unwrap();
        // The counts as the old log gives them, replayed without the migration.
        let mut expected = Personal::default();
        for line in log.lines().filter(|l| l.starts_with("H\t")) {
            let fields: Vec<&str> = line.split('\t').collect();
            let n = fields.get(2).map_or(1, |n| n.parse().unwrap());
            expected.apply_habit(&personal::habit_lang(fields[1]), n);
        }
        let store = UserStore::open(dir.path());
        assert_eq!(store.habit(), expected.habit());
        assert!(store.habit().contains_key(&LangId("hi".into())));
        let text = fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
        let tags: Vec<&str> = text
            .lines()
            .filter_map(|l| l.strip_prefix("H\t"))
            .map(|l| l.split('\t').next().unwrap())
            .collect();
        assert_eq!(tags, ["en", "hi", "ml"], "rewritten in the new tags");
        let reopened = UserStore::open(dir.path());
        assert_eq!(reopened.habit(), expected.habit());
        assert!(reopened.is_learned("Thom"));
        assert_eq!(reopened.uses("how"), 3);
        // A log already in the new tags is left as it is.
        let before = fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
        drop(UserStore::open(dir.path()));
        assert_eq!(
            fs::read_to_string(dir.path().join(LOG_NAME)).unwrap(),
            before
        );
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
        assert_eq!(ml_en(&store), (0, 0));
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
        assert_eq!(ml_en(&reopened), (0, 8));
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
                store.note_used(&c, Some(&b), Some(&a), true, Some(&LangId::english()));
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

    fn hi() -> LangId {
        LangId("hi".into())
    }

    fn picked(store: &UserStore) -> Vec<(String, String, Vec<String>)> {
        let mut found: Vec<_> = store
            .picked()
            .map(|p| (p.lang.0.clone(), p.word.to_string(), p.typed.to_vec()))
            .collect();
        found.sort();
        found
    }

    #[test]
    fn a_picked_word_keeps_its_language_and_letters_across_restarts() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = UserStore::open(dir.path());
            store.pick(&hi(), "रौबदार", Some("Raubdar"));
            store.pick(&hi(), "रौबदार", Some("raubdaar"));
            store.pick(&hi(), "रौबदार", Some("raubdar"));
            store.pick(&LangId("as".into()), "প্ৰাপ্তিতকৈ", None);
        }
        let store = UserStore::open(dir.path());
        assert!(store.is_learned("रौबदार"), "a picked word is a learned one");
        assert_eq!(
            picked(&store),
            [
                ("as".to_string(), "প্ৰাপ্তিতকৈ".to_string(), vec![]),
                (
                    "hi".to_string(),
                    "रौबदार".to_string(),
                    vec!["raubdaar".to_string(), "raubdar".to_string()]
                ),
            ]
        );
        assert_eq!(store.learned_words(), ["প্ৰাপ্তিতকৈ", "रौबदार"]);
        let log = fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
        assert!(
            log.lines().any(|l| l == "L\tरौबदार"),
            "an older build, which skips `W`, still has the word learned: {log}"
        );
    }

    #[test]
    fn a_word_keeps_only_its_latest_typed_letters() {
        let mut store = UserStore::in_memory();
        for i in 0..MAX_TYPED_FORMS + 2 {
            let typed = format!("raub{}", "a".repeat(i));
            store.pick(&hi(), "रौबदार", Some(&typed));
        }
        let forms = &picked(&store)[0].2;
        assert_eq!(forms.len(), MAX_TYPED_FORMS);
        assert_eq!(forms.last().unwrap(), &format!("raub{}", "a".repeat(MAX_TYPED_FORMS + 1)));
        assert!(!forms.contains(&"raub".to_string()), "the oldest went: {forms:?}");
    }

    #[test]
    fn unacceptable_picks_are_refused_live_and_on_replay() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(LOG_NAME),
            "W\thi\nW\t\tरौब\traub\nW\thi\tदो शब्द\tdo\nW\thi\tरौब\tra ub\nW\thi\tरौब\tराउब\n\
             W\thi\tरौब\traub\textra\nW\thi\tठीक\nW\thi\tरौबदार\traubdar\n",
        )
        .unwrap();
        let mut store = UserStore::open(dir.path());
        assert_eq!(
            picked(&store),
            [
                ("hi".to_string(), "ठीक".to_string(), vec![]),
                ("hi".to_string(), "रौबदार".to_string(), vec!["raubdar".to_string()]),
            ],
            "a `W` line alone learns its word"
        );
        store.pick(&LangId("h i".into()), "रौब", Some("raub"));
        store.pick(&LangId(String::new()), "रौब", Some("raub"));
        store.pick(&hi(), "दो शब्द", Some("do"));
        store.pick(&hi(), "", Some("raub"));
        assert_eq!(picked(&store).len(), 2);
        // Letters that are no Latin word are left out; the word is still picked.
        store.pick(&hi(), "रौब", Some("r4ub"));
        assert_eq!(picked(&store)[1], ("hi".to_string(), "रौब".to_string(), vec![]));
    }

    #[test]
    fn forgetting_or_removing_a_picked_word_drops_the_pick() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = UserStore::open(dir.path());
        let start = store.picked_generation();
        store.pick(&hi(), "रौबदार", Some("raubdar"));
        store.pick(&hi(), "ठीकठाक", Some("thikthak"));
        let both = store.picked_generation();
        assert_ne!(start, both, "the generation says the picked words changed");
        store.learn("Thom");
        store.note_used("रौबदार", None, None, true, None);
        assert_eq!(store.picked_generation(), both, "other events leave it alone");
        store.forget("रौबदार");
        let one = store.picked_generation();
        assert_ne!(both, one);
        store.remove("ठीकठाक");
        assert_ne!(one, store.picked_generation());
        assert!(picked(&store).is_empty());
        assert!(picked(&UserStore::open(dir.path())).is_empty());
        // Learned again by typing, it is a plain learned word: no language, no letters.
        store.learn("रौबदार");
        assert!(picked(&store).is_empty());
        store.pick(&hi(), "रौबदार", Some("raubdar"));
        let before = store.picked_generation();
        store.forget_all();
        assert_ne!(before, store.picked_generation());
        assert!(picked(&store).is_empty());
    }

    #[test]
    fn an_evicted_picked_word_changes_the_generation() {
        let mut store = UserStore::in_memory();
        store.pick(&hi(), "रौबदार", Some("raubdar"));
        let before = store.picked_generation();
        for i in 0..CAPACITY {
            store.learn(&format!("word{i}"));
        }
        assert!(picked(&store).is_empty());
        assert_ne!(before, store.picked_generation());
    }

    #[test]
    fn compaction_keeps_picked_words_with_their_language_and_letters() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = UserStore::open(dir.path());
        store.pick(&hi(), "रौबदार", Some("raubdar"));
        store.pick(&hi(), "रौबदार", Some("robdar"));
        store.note_used("रौबदार", None, None, true, None);
        for _ in 0..(CAPACITY * 4 + 10) {
            store.learn("Thom");
        }
        let text = fs::read_to_string(dir.path().join(LOG_NAME)).unwrap();
        assert!(text.lines().count() < 100, "log was compacted");
        let reopened = UserStore::open(dir.path());
        assert_eq!(
            picked(&reopened),
            [(
                "hi".to_string(),
                "रौबदार".to_string(),
                vec!["raubdar".to_string(), "robdar".to_string()]
            )]
        );
        assert_eq!(reopened.uses("रौबदार"), 1);
        assert_eq!(reopened.learned_words(), ["Thom", "रौबदार"]);
    }

    #[test]
    fn a_log_without_picked_words_loads_as_before_and_has_none() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(LOG_NAME),
            "L\tThom\nS\tAnu\nL\tरौबदार\nN\tरौबदार\t3\nC\traubdar\tरौबदार\nH\thi\t2\n",
        )
        .unwrap();
        let store = UserStore::open(dir.path());
        assert!(store.is_learned("Thom") && store.is_learned("रौबदार"));
        assert_eq!(store.uses("रौबदार"), 3);
        assert_eq!(store.choice("raubdar"), Some("रौबदार"));
        assert_eq!(store.picked().count(), 0);
        assert_eq!(store.picked_generation(), UserStore::in_memory().picked_generation());
    }
}

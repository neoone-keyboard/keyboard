//! What the user's typing says beyond new words (`user::UserStore` keeps those): which words
//! follow which (phrases), how often each word is used, which suggestion was chosen for a
//! typed form, and how often Latin typing ends in Malayalam. The store replays all of it from
//! its log; this module keeps the maps, their caps and their queries. Keys ignore ASCII case,
//! as the store's do.
//!
//! Every table is bounded. Beyond its cap the entries seen once go first, least recently used
//! first, and frequent ones only when none remain (as the store evicts words), a sixteenth at a
//! time so that eviction scans the table rarely. Clocks are per apply, so replaying the log (or
//! its compacted form, written oldest first) gives the same tables.

use std::collections::{BTreeMap, HashMap};

use crate::lang::LangId;

/// Most word pairs (previous word → word) kept.
pub const PAIR_CAPACITY: usize = 6_000;
/// Most word triples (two previous words → word) kept.
pub const TRIGRAM_CAPACITY: usize = 3_000;
/// Most words kept after one context; the least used is dropped for a new one.
pub const FOLLOWERS_PER_CONTEXT: usize = 8;
/// Most words whose uses are counted.
pub const USAGE_CAPACITY: usize = 3_000;
/// Most typed forms with a chosen suggestion.
pub const CHOICE_CAPACITY: usize = 500;
/// The language habit counts at most this many recent words: past it every count is halved,
/// so old habits fade.
pub const HABIT_WINDOW: u32 = 200;

/// The language of an `H` line's tag, its id. A log from before languages were ids tags
/// Malayalam `m` and English `e`; those read as `ml` and `en` (`is_old_habit_tag`).
pub fn habit_lang(tag: &str) -> LangId {
    match tag {
        "m" => LangId("ml".into()),
        "e" => LangId::english(),
        other => LangId(other.into()),
    }
}

/// Whether an `H` line's tag is one from before languages were ids (`m` or `e`).
pub fn is_old_habit_tag(tag: &str) -> bool {
    matches!(tag, "m" | "e")
}

fn key(word: &str) -> String {
    word.to_ascii_lowercase()
}

/// What eviction keeps longest: frequent entries, then recent ones.
fn worth(count: u32, last_used: u32) -> (bool, u32) {
    (count > 1, last_used)
}

/// How many entries an eviction leaves of `cap`.
fn after_eviction(cap: usize) -> usize {
    cap - cap / 16
}

/// A table's least worthy entries past `after_eviction(cap)` are those at or below the
/// returned cutoff; None when within `cap`.
fn cutoff(mut worths: Vec<(bool, u32)>, cap: usize) -> Option<(bool, u32)> {
    if worths.len() <= cap {
        return None;
    }
    let drop = worths.len() - after_eviction(cap);
    let (_, nth, _) = worths.select_nth_unstable(drop - 1);
    Some(*nth)
}

#[derive(Debug, Clone)]
struct Follower {
    /// As last committed (`Kochi`, `I`, `വീട്`).
    word: Box<str>,
    count: u32,
    last_used: u32,
}

/// Followers by context: one word (`hello`) for pairs, two joined by a space (`how are`) for
/// triples. Words never hold whitespace (`user::in_phrase`).
#[derive(Debug)]
struct Phrases {
    contexts: HashMap<Box<str>, Vec<Follower>>,
    len: usize,
    cap: usize,
}

impl Phrases {
    fn new(cap: usize) -> Self {
        Self {
            contexts: HashMap::new(),
            len: 0,
            cap,
        }
    }

    fn add(&mut self, context: &str, word: &str, n: u32, clock: u32) {
        let followers = self.contexts.entry(context.into()).or_default();
        if let Some(f) = followers
            .iter_mut()
            .find(|f| f.word.eq_ignore_ascii_case(word))
        {
            f.count = f.count.saturating_add(n);
            f.last_used = clock;
            f.word = word.into();
            return;
        }
        if followers.len() >= FOLLOWERS_PER_CONTEXT
            && let Some(i) = (0..followers.len())
                .min_by_key(|&i| worth(followers[i].count, followers[i].last_used))
        {
            followers.swap_remove(i);
            self.len -= 1;
        }
        // Most contexts keep one or two followers: no room for four (`Vec`'s first growth).
        followers.reserve_exact(1);
        followers.push(Follower {
            word: word.into(),
            count: n,
            last_used: clock,
        });
        self.len += 1;
        if self.len > self.cap {
            self.evict();
        }
    }

    fn evict(&mut self) {
        let worths = self
            .contexts
            .values()
            .flatten()
            .map(|f| worth(f.count, f.last_used))
            .collect();
        let Some(cut) = cutoff(worths, self.cap) else {
            return;
        };
        for followers in self.contexts.values_mut() {
            followers.retain(|f| worth(f.count, f.last_used) > cut);
        }
        self.contexts.retain(|_, f| !f.is_empty());
        self.len = self.contexts.values().map(Vec::len).sum();
    }

    fn followers(&self, context: &str) -> &[Follower] {
        self.contexts.get(context).map_or(&[], Vec::as_slice)
    }

    /// Drops `word` (keyed) as a follower and every context holding it.
    fn purge(&mut self, word: &str) {
        self.contexts
            .retain(|context, _| !context.split(' ').any(|w| w == word));
        for followers in self.contexts.values_mut() {
            followers.retain(|f| !f.word.eq_ignore_ascii_case(word));
        }
        self.contexts.retain(|_, f| !f.is_empty());
        self.len = self.contexts.values().map(Vec::len).sum();
    }

    /// Every (context, follower), least recently used first.
    fn oldest_first(&self) -> Vec<(&str, &Follower)> {
        let mut all: Vec<(&str, &Follower)> = self
            .contexts
            .iter()
            .flat_map(|(c, fs)| fs.iter().map(move |f| (&**c, f)))
            .collect();
        all.sort_by_key(|(_, f)| f.last_used);
        all
    }
}

#[derive(Debug, Clone, Copy)]
struct Uses {
    count: u32,
    last_used: u32,
}

#[derive(Debug, Clone)]
struct Choice {
    chosen: Box<str>,
    count: u32,
    last_used: u32,
}

#[derive(Debug)]
pub struct Personal {
    pairs: Phrases,
    trigrams: Phrases,
    uses: HashMap<Box<str>, Uses>,
    choices: HashMap<Box<str>, Choice>,
    /// Recent words from the Latin keyboard, by the language they ended up in.
    habit: BTreeMap<LangId, u32>,
    clock: u32,
}

impl Default for Personal {
    fn default() -> Self {
        Self {
            pairs: Phrases::new(PAIR_CAPACITY),
            trigrams: Phrases::new(TRIGRAM_CAPACITY),
            uses: HashMap::new(),
            choices: HashMap::new(),
            habit: BTreeMap::new(),
            clock: 0,
        }
    }
}

/// A word the user went on with, after a context (`Personal::followers`).
#[derive(Debug, Clone, PartialEq)]
pub struct Followed {
    /// As last committed.
    pub word: String,
    /// Times after the previous word, plus twice the times after the two previous words.
    pub score: u32,
}

impl Personal {
    fn tick(&mut self) -> u32 {
        self.clock = self.clock.wrapping_add(1);
        self.clock
    }

    pub fn apply_use(&mut self, word: &str, n: u32) {
        let clock = self.tick();
        let k = key(word);
        let entry = self.uses.entry(k.into()).or_insert(Uses {
            count: 0,
            last_used: clock,
        });
        entry.count = entry.count.saturating_add(n);
        entry.last_used = clock;
        if self.uses.len() > USAGE_CAPACITY {
            let worths = self
                .uses
                .values()
                .map(|u| worth(u.count, u.last_used))
                .collect();
            if let Some(cut) = cutoff(worths, USAGE_CAPACITY) {
                self.uses.retain(|_, u| worth(u.count, u.last_used) > cut);
            }
        }
    }

    pub fn apply_pair(&mut self, previous: &str, word: &str, n: u32) {
        let clock = self.tick();
        self.pairs.add(&key(previous), word, n, clock);
    }

    pub fn apply_trigram(&mut self, before_previous: &str, previous: &str, word: &str, n: u32) {
        let clock = self.tick();
        let context = format!("{} {}", key(before_previous), key(previous));
        self.trigrams.add(&context, word, n, clock);
    }

    /// The latest choice for a typed form replaces an earlier, different one.
    pub fn apply_choice(&mut self, typed: &str, chosen: &str, n: u32) {
        let clock = self.tick();
        let entry = self.choices.entry(key(typed).into()).or_insert(Choice {
            chosen: chosen.into(),
            count: 0,
            last_used: clock,
        });
        if !entry.chosen.eq_ignore_ascii_case(chosen) {
            entry.count = 0;
        }
        entry.chosen = chosen.into();
        entry.count = entry.count.saturating_add(n);
        entry.last_used = clock;
        if self.choices.len() > CHOICE_CAPACITY {
            let worths = self
                .choices
                .values()
                .map(|c| worth(c.count, c.last_used))
                .collect();
            if let Some(cut) = cutoff(worths, CHOICE_CAPACITY) {
                self.choices
                    .retain(|_, c| worth(c.count, c.last_used) > cut);
            }
        }
    }

    /// `n` more words from the Latin keyboard ended up in `lang`.
    pub fn apply_habit(&mut self, lang: &LangId, n: u32) {
        match self.habit.get_mut(lang) {
            Some(count) => *count = count.saturating_add(n),
            None => {
                self.habit.insert(lang.clone(), n);
            }
        }
        let total = |h: &BTreeMap<LangId, u32>| h.values().fold(0u32, |a, &n| a.saturating_add(n));
        while total(&self.habit) > HABIT_WINDOW {
            self.habit.values_mut().for_each(|n| *n /= 2);
        }
        self.habit.retain(|_, n| *n > 0);
    }

    /// Forgets everything about `word`: its uses, the phrases it is in, and the choices it
    /// was typed or chosen in.
    pub fn purge(&mut self, word: &str) {
        let k = key(word);
        self.uses.remove(k.as_str());
        self.pairs.purge(&k);
        self.trigrams.purge(&k);
        self.choices
            .retain(|typed, c| **typed != *k && !c.chosen.eq_ignore_ascii_case(&k));
    }

    /// Whether `purge(word)` would drop anything.
    pub fn mentions(&self, word: &str) -> bool {
        let k = key(word);
        let in_phrases = |p: &Phrases| {
            p.contexts.iter().any(|(context, followers)| {
                context.split(' ').any(|w| w == k)
                    || followers.iter().any(|f| f.word.eq_ignore_ascii_case(&k))
            })
        };
        self.uses.contains_key(k.as_str())
            || in_phrases(&self.pairs)
            || in_phrases(&self.trigrams)
            || self
                .choices
                .iter()
                .any(|(typed, c)| **typed == *k || c.chosen.eq_ignore_ascii_case(&k))
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The words that followed `previous` (and `before_previous` before it), best first: by
    /// `Followed::score`, then the most recent. At most `limit`.
    pub fn followers(
        &self,
        before_previous: Option<&str>,
        previous: &str,
        limit: usize,
    ) -> Vec<Followed> {
        let previous = key(previous);
        let mut found: Vec<(Followed, u32)> = self
            .pairs
            .followers(&previous)
            .iter()
            .map(|f| {
                let followed = Followed {
                    word: f.word.to_string(),
                    score: f.count,
                };
                (followed, f.last_used)
            })
            .collect();
        if let Some(before) = before_previous {
            let context = format!("{} {previous}", key(before));
            for f in self.trigrams.followers(&context) {
                let extra = f.count.saturating_mul(2);
                match found
                    .iter_mut()
                    .find(|(p, _)| p.word.eq_ignore_ascii_case(&f.word))
                {
                    Some((p, _)) => p.score = p.score.saturating_add(extra),
                    None => found.push((
                        Followed {
                            word: f.word.to_string(),
                            score: extra,
                        },
                        f.last_used,
                    )),
                }
            }
        }
        found.sort_by_key(|(f, last)| std::cmp::Reverse((f.score, *last)));
        found.into_iter().take(limit).map(|(f, _)| f).collect()
    }

    /// How many times `word` was used (ASCII case-insensitive).
    pub fn uses(&self, word: &str) -> u32 {
        self.uses.get(key(word).as_str()).map_or(0, |u| u.count)
    }

    /// Whether any word's uses are counted: when none are, nothing need be looked up.
    pub fn has_uses(&self) -> bool {
        !self.uses.is_empty()
    }

    /// The suggestion last chosen for `typed` (ASCII case-insensitive), as chosen.
    pub fn choice(&self, typed: &str) -> Option<&str> {
        self.choices.get(key(typed).as_str()).map(|c| &*c.chosen)
    }

    /// Recent words from the Latin keyboard, by the language they ended up in. A language
    /// with none is left out.
    pub fn habit(&self) -> &BTreeMap<LangId, u32> {
        &self.habit
    }

    /// Entries in every table: as many lines as `lines` writes.
    pub fn len(&self) -> usize {
        let habit = self.habit.len();
        self.uses.len() + self.pairs.len + self.trigrams.len + self.choices.len() + habit
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The tables as log lines (`user.rs`), each table least recently used first, so that
    /// replaying them gives the same tables.
    pub fn lines(&self) -> String {
        let mut text = String::new();
        let mut uses: Vec<(&Box<str>, &Uses)> = self.uses.iter().collect();
        uses.sort_by_key(|(_, u)| u.last_used);
        for (word, u) in uses {
            text.push_str(&format!("N\t{word}\t{}\n", u.count));
        }
        for (context, f) in self.pairs.oldest_first() {
            text.push_str(&format!("P\t{context}\t{}\t{}\n", f.word, f.count));
        }
        for (context, f) in self.trigrams.oldest_first() {
            let (first, second) = context.split_once(' ').unwrap_or((context, ""));
            text.push_str(&format!("T\t{first}\t{second}\t{}\t{}\n", f.word, f.count));
        }
        let mut choices: Vec<(&Box<str>, &Choice)> = self.choices.iter().collect();
        choices.sort_by_key(|(_, c)| c.last_used);
        for (typed, c) in choices {
            text.push_str(&format!("C\t{typed}\t{}\t{}\n", c.chosen, c.count));
        }
        for (id, n) in &self.habit {
            text.push_str(&format!("H\t{}\t{n}\n", id.0));
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(found: &[Followed]) -> Vec<&str> {
        found.iter().map(|f| f.word.as_str()).collect()
    }

    #[test]
    fn followers_rank_by_count_then_recency() {
        let mut p = Personal::default();
        p.apply_pair("hello", "kitty", 1);
        p.apply_pair("hello", "how", 1);
        assert_eq!(words(&p.followers(None, "Hello", 5)), ["how", "kitty"]);
        p.apply_pair("hello", "kitty", 1);
        assert_eq!(words(&p.followers(None, "hello", 5)), ["kitty", "how"]);
        assert_eq!(words(&p.followers(None, "hello", 1)), ["kitty"]);
        assert!(p.followers(None, "kitty", 5).is_empty());
    }

    #[test]
    fn the_two_previous_words_outweigh_one() {
        let mut p = Personal::default();
        for _ in 0..2 {
            p.apply_pair("are", "we", 1);
        }
        p.apply_pair("are", "you", 1);
        p.apply_trigram("how", "are", "you", 1);
        assert_eq!(words(&p.followers(Some("how"), "are", 5)), ["you", "we"]);
        assert_eq!(words(&p.followers(None, "are", 5)), ["we", "you"]);
        assert_eq!(words(&p.followers(Some("so"), "are", 5)), ["we", "you"]);
    }

    #[test]
    fn a_follower_keeps_its_latest_case() {
        let mut p = Personal::default();
        p.apply_pair("to", "kochi", 1);
        p.apply_pair("to", "Kochi", 1);
        let found = p.followers(None, "to", 5);
        assert_eq!(
            found,
            [Followed {
                word: "Kochi".into(),
                score: 2
            }]
        );
    }

    #[test]
    fn one_context_keeps_at_most_its_cap() {
        let mut p = Personal::default();
        p.apply_pair("the", "best", 1);
        p.apply_pair("the", "best", 1);
        for i in 0..FOLLOWERS_PER_CONTEXT + 3 {
            p.apply_pair("the", &format!("w{i}"), 1);
        }
        let found = p.followers(None, "the", 100);
        assert_eq!(found.len(), FOLLOWERS_PER_CONTEXT);
        assert_eq!(found[0].word, "best", "the frequent one stays");
        assert_eq!(found[1].word, format!("w{}", FOLLOWERS_PER_CONTEXT + 2));
    }

    #[test]
    fn tables_stay_within_their_caps_keeping_the_frequent_and_recent() {
        let mut p = Personal::default();
        p.apply_pair("keep", "this", 2);
        p.apply_use("often", 5);
        for i in 0..PAIR_CAPACITY + 100 {
            p.apply_pair(&format!("p{i}"), "x", 1);
        }
        for i in 0..USAGE_CAPACITY + 100 {
            p.apply_use(&format!("u{i}"), 1);
        }
        assert!(p.pairs.len <= PAIR_CAPACITY);
        assert!(p.uses.len() <= USAGE_CAPACITY);
        assert_eq!(words(&p.followers(None, "keep", 1)), ["this"]);
        assert_eq!(p.uses("often"), 5);
        let newest = format!("p{}", PAIR_CAPACITY + 99);
        assert_eq!(words(&p.followers(None, &newest, 1)), ["x"]);
        assert!(
            p.followers(None, "p0", 1).is_empty(),
            "the oldest went first"
        );
        assert_eq!(p.uses("u0"), 0);
    }

    #[test]
    fn purging_a_word_drops_its_phrases_uses_and_choices() {
        let mut p = Personal::default();
        p.apply_pair("hello", "kitty", 1);
        p.apply_pair("kitty", "cat", 1);
        p.apply_pair("hello", "how", 1);
        p.apply_trigram("hello", "kitty", "cat", 1);
        p.apply_trigram("say", "hello", "kitty", 1);
        p.apply_use("Kitty", 3);
        p.apply_choice("kity", "kitty", 1);
        p.apply_choice("innu", "ഇന്ന്", 1);
        p.purge("Kitty");
        assert_eq!(words(&p.followers(Some("say"), "hello", 5)), ["how"]);
        assert!(p.followers(None, "kitty", 5).is_empty());
        assert!(p.followers(Some("hello"), "kitty", 5).is_empty());
        assert_eq!(p.uses("kitty"), 0);
        assert_eq!(p.choice("kity"), None);
        assert_eq!(p.choice("innu"), Some("ഇന്ന്"));
        p.purge("ഇന്ന്");
        assert_eq!(p.choice("innu"), None);
        assert_eq!(p.len(), 1, "only hello → how is left");
    }

    #[test]
    fn the_latest_choice_wins() {
        let mut p = Personal::default();
        p.apply_choice("Innu", "ഇന്നു", 1);
        assert_eq!(p.choice("innu"), Some("ഇന്നു"));
        p.apply_choice("innu", "ഇന്ന്", 1);
        assert_eq!(p.choice("INNU"), Some("ഇന്ന്"));
    }

    #[test]
    fn the_habit_fades() {
        let (ml, en) = (LangId("ml".into()), LangId::english());
        let mut p = Personal::default();
        p.apply_habit(&en, HABIT_WINDOW);
        assert_eq!(p.habit().get(&en), Some(&HABIT_WINDOW));
        p.apply_habit(&ml, 1);
        assert_eq!(p.habit().get(&en), Some(&(HABIT_WINDOW / 2)));
        assert_eq!(p.habit().get(&ml), None, "halved to nothing");
    }
}

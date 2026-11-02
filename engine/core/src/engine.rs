//! Turns the text before the cursor into ranked suggestions. Each source
//! (English, Malayalam, next words) is optional: a missing or corrupt file
//! only disables that source. Every walk over a lexicon is bounded.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use crate::fuzzy::{self, WeightedEdit};
use crate::lexicon::{Lexicon, SEPARATOR, TOP_K};
use crate::malayalam;
use crate::normalize::normalize;
use crate::rank::{self, apply_case, capitalize};
use crate::text::{self, Script};
use crate::user::UserStore;

/// Most keys visited by one correction walk. Prefix lookups need no limit
/// here: `Lexicon::top_completions` and `subtree_count` are exact and bounded.
const CORRECTION_LIMIT: usize = 4_000;
/// Candidates returned: the middle slot plus two side slots.
const SHOWN: usize = 3;

/// The vowel signs the strip offers, in aksharamala order (KeyboardCore's `Malayalam.stripSigns`).
pub const STRIP_SIGNS: [char; 15] = [
    '\u{0D3E}', '\u{0D3F}', '\u{0D40}', '\u{0D41}', '\u{0D42}', '\u{0D43}', '\u{0D46}', '\u{0D47}',
    '\u{0D48}', '\u{0D4A}', '\u{0D4B}', '\u{0D57}', '\u{0D02}', '\u{0D03}', '\u{0D4D}',
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// QWERTY: English and Manglish.
    Latin,
    /// The native Malayalam layout: what was typed always stays in the middle.
    Malayalam,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Verbatim,
    Completion,
    Correction,
    Manglish,
    NextWord,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub text: String,
    pub kind: Kind,
    pub score: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Suggestions {
    /// The word being typed, as typed ("" after a space).
    pub typed: String,
    /// `candidates[0]` goes in the middle. When it isn't the typed word, the
    /// verbatim candidate is `candidates[1]`.
    pub candidates: Vec<Candidate>,
    /// Vowel signs ordered by how often they follow the typed prefix; empty
    /// unless the text ends in a Malayalam consonant.
    pub vowel_signs: Vec<char>,
}

#[derive(Debug, Default)]
pub struct Sources {
    pub english: Option<Lexicon>,
    pub malayalam: Option<Lexicon>,
    pub next_words: Option<Lexicon>,
}

/// File paths handed over by the platform. Any of them may be missing.
#[derive(Debug, Clone, Default)]
pub struct Paths {
    pub english: Option<PathBuf>,
    pub malayalam: Option<PathBuf>,
    pub next_words: Option<PathBuf>,
    /// Directory for learned words; None keeps them in memory only.
    pub user_dir: Option<PathBuf>,
}

impl Sources {
    pub fn open(paths: &Paths) -> Self {
        let open = |p: &Option<PathBuf>| p.as_deref().and_then(|p| Lexicon::open(p).ok());
        Self {
            english: open(&paths.english),
            malayalam: open(&paths.malayalam),
            next_words: open(&paths.next_words),
        }
    }

    /// Names of the sources that loaded, for diagnostics.
    pub fn loaded(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.english.is_some() {
            names.push("english");
        }
        if self.malayalam.is_some() {
            names.push("malayalam");
        }
        if self.next_words.is_some() {
            names.push("next_words");
        }
        names
    }
}

#[derive(Debug)]
pub struct Engine {
    sources: Sources,
    user: Mutex<UserStore>,
}

impl Engine {
    pub fn open(paths: &Paths) -> Self {
        let user = match &paths.user_dir {
            Some(dir) => UserStore::open(dir),
            None => UserStore::in_memory(),
        };
        Self::new(Sources::open(paths), user)
    }

    pub fn new(sources: Sources, user: UserStore) -> Self {
        Self {
            sources,
            user: Mutex::new(user),
        }
    }

    pub fn loaded_sources(&self) -> Vec<&'static str> {
        self.sources.loaded()
    }

    /// A poisoned lock still holds usable data; never panic over it.
    fn user(&self) -> MutexGuard<'_, UserStore> {
        self.user
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn learn(&self, word: &str) {
        self.user().learn(&normalize(word));
    }

    /// A word was committed as typed. Unknown words are learned the second time.
    pub fn note_committed(&self, word: &str) {
        let word = normalize(word);
        if !self.is_known(&word) {
            self.user().note_typed(&word);
        }
    }

    pub fn forget_learned(&self) {
        self.user().forget_all();
    }

    pub fn suggest(&self, context_before: &str, layout: Layout, autocorrect: bool) -> Suggestions {
        let typed = text::current_word(context_before);
        if typed.is_empty() {
            return Suggestions {
                candidates: self.next_words(context_before),
                ..Suggestions::default()
            };
        }
        // Classify and search the normal form (`don’` → `don'`); show the word as typed.
        let typed_n = normalize(typed);
        let mut suggestions = match (layout, text::script(&typed_n)) {
            (Layout::Latin, Script::Latin) => {
                let before = &context_before[..context_before.len() - typed.len()];
                let sentence_start = text::previous_word(before).is_some_and(|(_, end)| end);
                self.suggest_latin(typed, &typed_n, autocorrect, sentence_start)
            }
            (_, Script::Malayalam) | (Layout::Malayalam, _) => self.suggest_malayalam(typed),
            _ => vec![verbatim(typed, 0.0)],
        };
        suggestions.truncate(SHOWN);
        Suggestions {
            typed: typed.to_string(),
            candidates: suggestions,
            vowel_signs: self.vowel_signs(context_before, typed),
        }
    }

    fn is_known(&self, word: &str) -> bool {
        match text::script(word) {
            Script::Latin => self.english_count(word).is_some(),
            Script::Malayalam => self
                .sources
                .malayalam
                .as_ref()
                .is_some_and(|l| l.get(word.as_bytes()).is_some()),
            Script::Other => true, // numbers and the like are never learned
        }
    }

    /// The count of an English word, matching case the way `apply_case` would show it.
    fn english_count(&self, word: &str) -> Option<u64> {
        let lex = self.sources.english.as_ref()?;
        lex.case_variants(word.as_bytes())
            .into_iter()
            .filter(|(key, _)| apply_case(word, &String::from_utf8_lossy(key)) == word)
            .map(|(_, count)| count)
            .max()
    }

    /// `typed` is shown as typed; `typed_n` (its normal form) is what is looked up.
    fn suggest_latin(
        &self,
        typed: &str,
        typed_n: &str,
        autocorrect: bool,
        sentence_start: bool,
    ) -> Vec<Candidate> {
        let learned = self.user().is_learned(typed_n);
        // How a stored word is shown: in the typed case, capitalised at a sentence start.
        let show = |word: &str| {
            let shown = apply_case(typed_n, word);
            if sentence_start {
                capitalize(&shown)
            } else {
                shown
            }
        };
        let mut known_score = None;
        // Another case of the typed word (`tom` → `Tom`): the best correction there is.
        let mut case_fix: Option<Candidate> = None;
        let mut corrections = Vec::new();
        let mut completions = Vec::new();

        if let Some(lex) = &self.sources.english {
            let score_of = |count: u64| lex.log_prob(count) + rank::KNOWN_BONUS;
            let variants = lex.case_variants(typed_n.as_bytes());
            for (key, count) in &variants {
                let Ok(word) = std::str::from_utf8(key) else {
                    continue;
                };
                if apply_case(typed_n, word) == typed_n {
                    let score = score_of(*count);
                    known_score = Some(known_score.map_or(score, |s: f32| s.max(score)));
                }
            }
            // Other cases of a word that is itself in the lexicon aren't offered (`us` hides `US`).
            if known_score.is_none() {
                for (key, count) in &variants {
                    let Ok(word) = std::str::from_utf8(key) else {
                        continue;
                    };
                    let candidate = Candidate {
                        text: show(word),
                        kind: Kind::Correction,
                        score: score_of(*count),
                    };
                    if case_fix.as_ref().is_none_or(|c| candidate.score > c.score) {
                        case_fix = Some(candidate);
                    }
                }
            }

            // Short words get fewer edits: more would match half the lexicon.
            let max_cost = match typed_n.len() {
                1 => 0,
                2 | 3 => fuzzy::EDIT,
                4..=6 => fuzzy::EDIT + fuzzy::CHEAP_EDIT,
                _ => 2 * fuzzy::EDIT,
            };
            if let Some(automaton) = WeightedEdit::new(typed_n, max_cost) {
                let probe = automaton.clone();
                lex.search(automaton, CORRECTION_LIMIT, |key, count, state| {
                    let cost = state.cost(&probe);
                    // Cost 0 is a case variant, handled above.
                    let (Ok(word), true) = (std::str::from_utf8(key), cost > 0) else {
                        return;
                    };
                    let candidate = Candidate {
                        text: show(word),
                        kind: Kind::Correction,
                        score: score_of(count) - rank::EDIT_PENALTY * f32::from(cost),
                    };
                    // A correction that only extends the typed word (`hel` → `help`,
                    // `don'` → `don't`) is a completion: it never takes the middle.
                    if starts_with_ignoring_case(word, typed_n) {
                        completions.push(Candidate {
                            kind: Kind::Completion,
                            ..candidate
                        });
                    } else {
                        corrections.push(candidate);
                    }
                });
            }
            if typed_n.len() >= 2 {
                for (key, count) in lex.top_completions(typed_n.as_bytes(), TOP_K) {
                    let Ok(word) = std::str::from_utf8(&key) else {
                        continue;
                    };
                    if word.len() <= typed_n.len() {
                        continue;
                    }
                    let extra = (word.len() - typed_n.len()) as f32;
                    completions.push(Candidate {
                        text: show(word),
                        kind: Kind::Completion,
                        score: lex.log_prob(count)
                            - rank::COMPLETION_PENALTY
                            - rank::COMPLETION_PER_CHAR * extra,
                    });
                }
            }
        }
        for word in self.user().completions(typed_n, 2) {
            let extra = word.len().saturating_sub(typed_n.len()) as f32;
            completions.push(Candidate {
                text: show(&word),
                kind: Kind::Completion,
                score: rank::LEARNED_COMPLETION - rank::COMPLETION_PER_CHAR * extra,
            });
        }

        let typed_c = verbatim(typed, known_score.unwrap_or(rank::UNKNOWN));
        // A real word (or a learned one) is never replaced, and only a
        // correction may replace: space never commits an unfinished word.
        let replacement = if autocorrect && !learned && known_score.is_none() {
            let best_completion = completions
                .iter()
                .map(|c| c.score)
                .fold(f32::NEG_INFINITY, f32::max);
            match &case_fix {
                Some(fix) => Some(fix.clone()),
                None => corrections
                    .iter()
                    .max_by(|a, b| a.score.total_cmp(&b.score))
                    // A typed prefix more likely to be finished than mistyped stays as typed.
                    .filter(|best| best.score > best_completion)
                    .cloned(),
            }
            .filter(|best| {
                best.score >= typed_c.score + rank::MARGIN && best.score >= rank::MIN_REPLACE
            })
        } else {
            None
        };
        let mut others = corrections;
        others.extend(completions);
        others.extend(case_fix);
        arrange(typed_c, typed_n, others, replacement)
    }

    fn suggest_malayalam(&self, typed: &str) -> Vec<Candidate> {
        let typed_n = normalize(typed);
        let mut others = Vec::new();
        if let Some(lex) = &self.sources.malayalam {
            for (key, count) in lex.top_completions(typed_n.as_bytes(), TOP_K) {
                if key.len() <= typed_n.len() {
                    continue;
                }
                let Ok(word) = std::str::from_utf8(&key) else {
                    continue;
                };
                let extra = (word.chars().count() - typed_n.chars().count()) as f32;
                let score = lex.log_prob(count)
                    - rank::COMPLETION_PENALTY
                    - rank::COMPLETION_PER_CHAR * extra;
                others.push(Candidate {
                    text: word.to_string(),
                    kind: Kind::Completion,
                    score,
                });
            }
        }
        for word in self.user().completions(&typed_n, 2) {
            let score = rank::LEARNED_COMPLETION;
            others.push(Candidate {
                text: word,
                kind: Kind::Completion,
                score,
            });
        }
        // Native Malayalam is never replaced: the typed word always stays in the middle.
        arrange(verbatim(typed, 0.0), &typed_n, others, None)
    }

    fn next_words(&self, context_before: &str) -> Vec<Candidate> {
        let (Some(lex), Some((previous, sentence_end))) = (
            &self.sources.next_words,
            text::previous_word(context_before),
        ) else {
            return Vec::new();
        };
        let mut key = normalize(previous).to_ascii_lowercase().into_bytes();
        key.push(SEPARATOR);
        let mut found: Vec<Candidate> = Vec::new();
        for (k, count) in lex.top_completions(&key, TOP_K) {
            let Ok(next) = std::str::from_utf8(&k[key.len()..]) else {
                continue;
            };
            let text = if sentence_end {
                capitalize(next)
            } else {
                next.to_string()
            };
            found.push(Candidate {
                text,
                kind: Kind::NextWord,
                score: count as f32,
            });
        }
        sort_and_dedup(&mut found);
        found.truncate(SHOWN);
        found
    }

    /// Orders the strip's vowel signs by how often each follows the typed
    /// prefix in the Malayalam lexicon. Signs never seen are left out; the
    /// keyboard appends them in aksharamala order.
    fn vowel_signs(&self, context_before: &str, typed: &str) -> Vec<char> {
        let (Some(lex), Some(_)) = (
            &self.sources.malayalam,
            malayalam::vowel_sign_base(context_before),
        ) else {
            return Vec::new();
        };
        let prefix = normalize(typed);
        let mut totals: Vec<(char, u64)> = STRIP_SIGNS
            .iter()
            .map(|&sign| {
                let mut probe = prefix.clone();
                probe.push(sign);
                let mut total = lex.subtree_count(probe.as_bytes());
                if sign == '\u{0D57}' {
                    // NFC writes െ + ൗ as ൌ (U+0D4C); both spell "au".
                    probe.pop();
                    probe.push('\u{0D4C}');
                    total = total.saturating_add(lex.subtree_count(probe.as_bytes()));
                }
                (sign, total)
            })
            .filter(|&(_, total)| total > 0)
            .collect();
        totals.sort_by_key(|&(_, total)| std::cmp::Reverse(total));
        totals.into_iter().map(|(sign, _)| sign).collect()
    }
}

fn starts_with_ignoring_case(word: &str, prefix: &str) -> bool {
    word.len() >= prefix.len()
        && word.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

fn verbatim(typed: &str, score: f32) -> Candidate {
    Candidate {
        text: typed.to_string(),
        kind: Kind::Verbatim,
        score,
    }
}

/// Highest score first; of two candidates with the same text, the better one stays.
fn sort_and_dedup(candidates: &mut Vec<Candidate>) {
    candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
    let mut seen = std::collections::HashSet::new();
    candidates.retain(|c| seen.insert(c.text.clone()));
}

/// Puts `replacement` (chosen by the caller, already checked against the
/// margin and floor) in the middle with the typed word second; otherwise the
/// typed word is in the middle. The best of the rest fill the side slots.
/// Nothing equal to the typed word, as typed or normalised, is repeated.
fn arrange(
    typed: Candidate,
    typed_n: &str,
    mut others: Vec<Candidate>,
    replacement: Option<Candidate>,
) -> Vec<Candidate> {
    others.retain(|c| c.text != typed.text && c.text != typed_n);
    let mut out = Vec::with_capacity(SHOWN);
    if let Some(best) = replacement.filter(|r| r.text != typed.text && r.text != typed_n) {
        others.retain(|c| c.text != best.text);
        out.push(best);
    }
    sort_and_dedup(&mut others);
    out.push(typed);
    out.extend(others.into_iter().take(SHOWN));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexicon::build;

    fn lex(words: &[(&str, u64)]) -> Option<Lexicon> {
        Some(
            Lexicon::from_bytes(
                build(words.iter().map(|(w, c)| (w.as_bytes().to_vec(), *c))).unwrap(),
            )
            .unwrap(),
        )
    }

    fn engine() -> Engine {
        let sources = Sources {
            english: lex(&[
                ("the", 1000),
                ("then", 50),
                ("hello", 40),
                ("help", 60),
                ("London", 30),
                ("I", 200),
            ]),
            malayalam: lex(&[
                ("എന്റെ", 100),
                ("എന്ത്", 80),
                ("കാര്യം", 10),
                ("കിളി", 5),
                ("കീ", 1),
            ]),
            next_words: lex(&[
                ("i\0am", 30),
                ("i\0have", 20),
                ("i\0was", 10),
                ("i\0will", 5),
            ]),
        };
        Engine::new(sources, UserStore::in_memory())
    }

    fn texts(s: &Suggestions) -> Vec<&str> {
        s.candidates.iter().map(|c| c.text.as_str()).collect()
    }

    #[test]
    fn nothing_to_suggest_from_empty_text() {
        assert_eq!(
            engine().suggest("", Layout::Latin, true),
            Suggestions::default()
        );
    }

    #[test]
    fn missing_sources_still_give_the_typed_word() {
        let engine = Engine::new(Sources::default(), UserStore::in_memory());
        let s = engine.suggest("hel", Layout::Latin, true);
        assert_eq!(texts(&s), ["hel"]);
        assert_eq!(s.candidates[0].kind, Kind::Verbatim);
        assert!(engine.loaded_sources().is_empty());
    }

    #[test]
    fn known_word_stays_in_the_middle() {
        let s = engine().suggest("hello", Layout::Latin, true);
        assert_eq!(s.candidates[0].text, "hello");
        assert_eq!(s.candidates[0].kind, Kind::Verbatim);
    }

    #[test]
    fn typo_is_corrected_and_typed_word_is_second() {
        let s = engine().suggest("teh", Layout::Latin, true);
        assert_eq!(s.typed, "teh");
        assert_eq!(s.candidates[0].text, "the");
        assert_eq!(s.candidates[0].kind, Kind::Correction);
        assert_eq!(s.candidates[1].text, "teh");
        assert_eq!(s.candidates[1].kind, Kind::Verbatim);
    }

    #[test]
    fn autocorrect_off_keeps_the_typed_word_in_the_middle() {
        let s = engine().suggest("teh", Layout::Latin, false);
        assert_eq!(s.candidates[0].text, "teh");
        assert!(texts(&s).contains(&"the"));
    }

    #[test]
    fn completions_follow_case() {
        let s = engine().suggest("Hel", Layout::Latin, true);
        assert!(texts(&s).contains(&"Help"));
        assert!(texts(&s).contains(&"Hello"));
    }

    #[test]
    fn stored_capitals_are_kept() {
        let s = engine().suggest("i", Layout::Latin, true);
        assert_eq!(s.candidates[0].text, "I");
        let s = engine().suggest("londo", Layout::Latin, true);
        assert!(texts(&s).contains(&"London"));
    }

    #[test]
    fn next_words_after_a_space() {
        let s = engine().suggest("I ", Layout::Latin, true);
        assert_eq!(s.typed, "");
        assert_eq!(texts(&s), ["am", "have", "was"]);
        assert!(s.candidates.iter().all(|c| c.kind == Kind::NextWord));
    }

    #[test]
    fn next_words_are_capitalised_after_a_full_stop() {
        let engine = Engine::new(
            Sources {
                next_words: lex(&[("rains\0it", 3)]),
                ..Sources::default()
            },
            UserStore::in_memory(),
        );
        assert_eq!(
            texts(&engine.suggest("It rains. ", Layout::Latin, true)),
            ["It"]
        );
    }

    #[test]
    fn malayalam_layout_keeps_the_typed_word_in_the_middle() {
        let s = engine().suggest("എന്", Layout::Malayalam, true);
        assert_eq!(texts(&s), ["എന്", "എന്റെ", "എന്ത്"]);
        assert_eq!(s.candidates[1].kind, Kind::Completion);
    }

    #[test]
    fn vowel_signs_follow_the_lexicon() {
        let s = engine().suggest("ക", Layout::Malayalam, true);
        assert_eq!(s.vowel_signs, ['\u{0D3E}', '\u{0D3F}', '\u{0D40}']);
        assert!(
            engine()
                .suggest("കി", Layout::Malayalam, true)
                .vowel_signs
                .is_empty()
        );
    }

    #[test]
    fn learned_words_are_never_replaced_and_are_completed() {
        let engine = engine();
        assert_eq!(
            engine.suggest("thw", Layout::Latin, true).candidates[0].text,
            "the"
        );
        engine.learn("thw");
        assert_eq!(
            engine.suggest("thw", Layout::Latin, true).candidates[0].text,
            "thw"
        );
        engine.learn("Thomasina");
        assert!(texts(&engine.suggest("thoma", Layout::Latin, true)).contains(&"Thomasina"));
    }

    #[test]
    fn unknown_words_typed_twice_are_learned_known_words_are_not_recorded() {
        let engine = engine();
        engine.note_committed("thw");
        assert_eq!(
            engine.suggest("thw", Layout::Latin, true).candidates[0].text,
            "the"
        );
        engine.note_committed("thw");
        assert_eq!(
            engine.suggest("thw", Layout::Latin, true).candidates[0].text,
            "thw"
        );
        engine.note_committed("hello");
        assert!(!engine.user().is_learned("hello"));
    }

    #[test]
    fn forgetting_learned_words() {
        let engine = engine();
        engine.learn("thw");
        engine.forget_learned();
        assert_eq!(
            engine.suggest("thw", Layout::Latin, true).candidates[0].text,
            "the"
        );
    }
}

//! Turns the text before the cursor into ranked suggestions. Each source
//! (English, Malayalam, Manglish, next words) is optional: a missing or corrupt file
//! only disables that source. Every walk over a lexicon is bounded.

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use crate::chat_forms;
use crate::fuzzy::{self, WeightedEdit};
use crate::lexicon::{Lexicon, SEPARATOR, TOP_K};
use crate::malayalam;
use crate::manglish;
use crate::nn::{Deadline, Search, Translit};
use crate::normalize::normalize;
use crate::rank::{self, apply_case, capitalize};
use crate::suffix;
use crate::text::{self, Script};
use crate::translit::transliterate;
use crate::user::UserStore;

/// Most keys visited by one correction walk. Prefix lookups need no limit
/// here: `Lexicon::top_completions` and `subtree_count` are exact and bounded.
const CORRECTION_LIMIT: usize = 4_000;
/// Candidates returned: the middle slot plus two side slots.
const SHOWN: usize = 3;
/// Lexicon spellings asked of the transliterator per word. On an iPhone 15 Pro Max, encode
/// plus 10 decoder steps at this width takes about 2.5 ms (p50) run back to back; a whole
/// suggest that starts from an idle CPU, as when typing, runs 5–7× slower, hence the
/// keyboard's deadline (`suggest_until`). Width 3 saved a quarter of the decoder rows but moved
/// the eval both ways (chat 0.7950 → 0.7919 top-1, human 0.5665 → 0.5715).
const MODEL_WIDTH: usize = 4;

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
    pub manglish: Option<Lexicon>,
    pub next_words: Option<Lexicon>,
    /// The neural transliterator (`nn::Translit`). Without it, Manglish ranking is M2's:
    /// skeleton lookup, suffix splits and the rule-based spelling (`translit::transliterate`).
    pub translit: Option<Translit>,
}

/// File paths handed over by the platform. Any of them may be missing.
#[derive(Debug, Clone, Default)]
pub struct Paths {
    pub english: Option<PathBuf>,
    pub malayalam: Option<PathBuf>,
    pub manglish: Option<PathBuf>,
    pub next_words: Option<PathBuf>,
    /// The transliterator model (`.mlkbnn`).
    pub translit: Option<PathBuf>,
    /// Directory for learned words; None keeps them in memory only.
    pub user_dir: Option<PathBuf>,
}

impl Sources {
    pub fn open(paths: &Paths) -> Self {
        let open = |p: &Option<PathBuf>| p.as_deref().and_then(|p| Lexicon::open(p).ok());
        Self {
            english: open(&paths.english),
            malayalam: open(&paths.malayalam),
            manglish: open(&paths.manglish),
            next_words: open(&paths.next_words),
            translit: paths
                .translit
                .as_deref()
                .and_then(|p| Translit::open(p).ok()),
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
        if self.manglish.is_some() {
            names.push("manglish");
        }
        if self.next_words.is_some() {
            names.push("next_words");
        }
        if self.translit.is_some() {
            names.push("translit");
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
        let word = normalize(word);
        let word = text::learnable_address(&word).unwrap_or(&word);
        self.user().learn(word);
    }

    /// A word was committed as typed. Unknown words are learned the second
    /// time, unless autocorrect would replace them: a typo committed where
    /// autocorrect couldn't run (`thnaks` in a search field) stays a typo.
    /// An address (`text::is_address`) is learned the second time, without the
    /// punctuation typed after it.
    pub fn note_committed(&self, word: &str) {
        let word = normalize(word);
        if let Some(address) = text::learnable_address(&word) {
            self.user().note_typed(address);
        } else if !self.is_known(&word) && !self.would_replace(&word) {
            self.user().note_typed(&word);
        }
    }

    /// Whether autocorrect would put something else in the middle for `word`.
    /// The previous word isn't known here, so the context bias is neutral.
    fn would_replace(&self, word: &str) -> bool {
        text::script(word) == Script::Latin
            && self
                .suggest_latin(word, word, true, false, false, 0.0, None)
                .first()
                .is_some_and(|c| c.kind != Kind::Verbatim)
    }

    /// The learned addresses (`text::is_address`), as shown, sorted. The keyboard reads
    /// letters that begin one as one word too, as `current_word` does.
    pub fn learned_addresses(&self) -> Vec<String> {
        let mut found: Vec<String> = self
            .user()
            .learned()
            .filter(|w| text::is_address(w))
            .map(str::to_string)
            .collect();
        found.sort();
        found
    }

    /// Forgets learned words and words removed from the bar.
    pub fn forget_learned(&self) {
        self.user().forget_all();
    }

    /// The user removed `word` from the bar: it is forgotten if learned and never
    /// suggested again (as typed, it still shows), until it is learned anew.
    pub fn remove_word(&self, word: &str) {
        self.user().remove(&normalize(word));
    }

    /// Every learned word, as shown, most recently used first (the "Your words" screen).
    pub fn learned_words(&self) -> Vec<String> {
        self.user().learned_words()
    }

    /// The words removed from the bar, most recently removed first, ASCII lowercased.
    pub fn removed_words(&self) -> Vec<String> {
        self.user().removed_words()
    }

    /// Forgets one learned word without blocking it: suggested again only once learned anew.
    pub fn forget_word(&self, word: &str) {
        self.user().forget(&normalize(word));
    }

    /// Un-removes a word removed from the bar; it is not learned by this.
    pub fn restore_word(&self, word: &str) {
        self.user().restore(&normalize(word));
    }

    /// Takes out words the user removed from the bar. Pools are filtered before the
    /// middle is chosen, so a removed word never replaces one and the next best fills its
    /// slot. Never given the typed word itself.
    fn drop_removed(&self, candidates: &mut Vec<Candidate>) {
        let user = self.user();
        candidates.retain(|c| !user.is_removed(&c.text));
    }

    fn is_removed(&self, candidate: &Candidate) -> bool {
        self.user().is_removed(&candidate.text)
    }

    pub fn suggest(&self, context_before: &str, layout: Layout, autocorrect: bool) -> Suggestions {
        self.suggest_until(context_before, layout, autocorrect, None)
    }

    /// `suggest`, with the transliterator held to `deadline` (`nn::Deadline`): when time runs
    /// out it stops and its finished spellings are used, so a slow CPU costs Manglish
    /// spellings rather than the whole bar. The keyboard passes a clock started with the
    /// request; the eval and tests pass none, so their results don't depend on the machine.
    pub fn suggest_until(
        &self,
        context_before: &str,
        layout: Layout,
        autocorrect: bool,
        deadline: Option<Deadline<'_>>,
    ) -> Suggestions {
        let (typed, address) = self.current_word(context_before);
        if typed.is_empty() {
            return Suggestions {
                candidates: self.next_words(context_before),
                ..Suggestions::default()
            };
        }
        // Classify and search the normal form (`don’` → `don'`); show the word as typed.
        let typed_n = normalize(typed);
        if address {
            return Suggestions {
                typed: typed.to_string(),
                candidates: self.suggest_address(typed, &typed_n),
                vowel_signs: Vec::new(),
            };
        }
        let mut suggestions = match (layout, text::script(&typed_n)) {
            (Layout::Latin, Script::Latin) => {
                let before = &context_before[..context_before.len() - typed.len()];
                let previous = text::previous_word(before);
                let sentence_start = previous.is_some_and(|(_, end)| end);
                let mid_sentence = previous.is_some_and(|(_, end)| !end);
                let bias = self.manglish_bias(previous.map(|(word, _)| word));
                self.suggest_latin(
                    typed,
                    &typed_n,
                    autocorrect,
                    sentence_start,
                    mid_sentence,
                    bias,
                    deadline,
                )
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

    /// The word being typed (`text::current_word`), and whether it is an address. Letters
    /// and punctuation that begin a learned address are one too (`neo.9` of
    /// sam.2024@example.com), back to the whitespace: their full stop is no word break.
    fn current_word<'a>(&self, context_before: &'a str) -> (&'a str, bool) {
        let word = text::current_word(context_before);
        if text::is_address(word) {
            return (word, true);
        }
        let token = text::last_token(context_before);
        if token != word && self.begins_learned_address(token) {
            (token, true)
        } else {
            (word, false)
        }
    }

    /// Whether `token`, two characters or more, begins a learned address (ASCII
    /// case-insensitive, as the store is).
    fn begins_learned_address(&self, token: &str) -> bool {
        token.chars().nth(1).is_some()
            && self
                .user()
                .learned()
                .any(|w| text::is_address(w) && starts_with_ignoring_case(w, token))
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

    /// How much the previous word's language favours Manglish over English.
    fn manglish_bias(&self, previous: Option<&str>) -> f32 {
        let Some(word) = previous else {
            return rank::CONTEXT_AT_START;
        };
        let word = normalize(word);
        match text::script(&word) {
            Script::Malayalam => rank::CONTEXT_AFTER_MALAYALAM,
            Script::Latin if self.english_count(&word).is_some() => rank::CONTEXT_AFTER_ENGLISH,
            _ => 0.0,
        }
    }

    /// `typed` is shown as typed; `typed_n` (its normal form) is what is looked up.
    /// `sentence_start`: a word before it ended a sentence. `mid_sentence`: a word
    /// before it didn't (neither at the very start of the text).
    /// `manglish_bias` comes from the previous word's language (`manglish_bias`).
    #[allow(clippy::too_many_arguments)]
    fn suggest_latin(
        &self,
        typed: &str,
        typed_n: &str,
        autocorrect: bool,
        sentence_start: bool,
        mid_sentence: bool,
        manglish_bias: f32,
        deadline: Option<Deadline<'_>>,
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
        // Corrections, each with its edit cost (`fuzzy` half-edits).
        let mut corrections: Vec<(Candidate, u8)> = Vec::new();
        // The best word that is the typed letters with apostrophes put back
        // (`rank::CONTRACTION_LOOKALIKES`).
        let mut contraction: Option<Candidate> = None;
        // A long typed word's best near match plus up to `fuzzy::MAX_TAIL` letters, and
        // that word if it dominates its matched letters (`rank::COMPLETION_DOMINANT`).
        let mut near_completion: Option<NearCompletion> = None;
        let mut dominant: Option<Candidate> = None;
        // Corrections to a name: the lexicon stores them capitalised (London, Kochi).
        let mut names: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut completions = Vec::new();
        let mut manglish: Option<ManglishCandidates> = None;

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

            let found = self.manglish(typed_n, manglish_bias, known_score.is_some(), deadline);
            // Letters that spell a Malayalam word are not a mistyped English one: the
            // long-word search's extra reach (`rank::LONG_WORD`) is not used for them.
            let long = typed_n.len() >= rank::LONG_WORD && found.exact.is_empty();
            manglish = Some(found);

            // Short words get fewer edits: more would match half the lexicon.
            let max_cost = match typed_n.len() {
                1 => 0,
                2 | 3 => fuzzy::EDIT,
                4..=6 => fuzzy::EDIT + fuzzy::CHEAP_EDIT,
                _ => 2 * fuzzy::EDIT,
            };
            let automaton = WeightedEdit::new(typed_n, max_cost).map(|a| {
                if long {
                    a.with_cheap_vowels().with_tail()
                } else {
                    a
                }
            });
            // Corrections that are contractions (index, score before edits), and the edit
            // cost of each bare correction (`whats`), for the step after the search.
            let mut contractions_found: Vec<(usize, f32)> = Vec::new();
            let mut bare_costs: std::collections::HashMap<String, u8> =
                std::collections::HashMap::new();
            if let Some(automaton) = automaton {
                let probe = automaton.clone();
                lex.search(automaton, CORRECTION_LIMIT, |key, count, state| {
                    let Ok(word) = std::str::from_utf8(key) else {
                        return;
                    };
                    // A long word read as a near match plus a few letters (`apostrap` →
                    // apostrophe, `preferenc` → preference): a candidate for the middle
                    // when it dominates (`rank::COMPLETION_DOMINANT`).
                    if let Some((cost, extra)) = state
                        .tail(&probe)
                        .filter(|&(cost, _)| cost <= rank::NEAR_COMPLETION_COST)
                    {
                        let middle = score_of(count) - rank::EDIT_PENALTY * f32::from(cost);
                        if near_completion
                            .as_ref()
                            .is_none_or(|n| middle > n.candidate.score)
                        {
                            near_completion = Some(NearCompletion {
                                candidate: Candidate {
                                    text: show(word),
                                    kind: Kind::Correction,
                                    score: middle,
                                },
                                matched: key[..key.len() - extra].to_vec(),
                                count,
                            });
                        }
                        completions.push(Candidate {
                            text: show(word),
                            kind: Kind::Completion,
                            score: lex.log_prob(count)
                                - rank::EDIT_PENALTY * f32::from(cost)
                                - rank::COMPLETION_PENALTY
                                - rank::COMPLETION_PER_CHAR * extra as f32,
                        });
                    }
                    let cost = state.cost(&probe);
                    // Cost 0 is a case variant, handled above.
                    if cost == 0 || cost > max_cost {
                        return;
                    }
                    // Only leaving out apostrophes (`dont` → `don't`) costs nothing:
                    // it is the commonest slip, and no letter is wrong.
                    let apostrophes_only = only_apostrophes_missing(word, typed_n);
                    let penalty = if apostrophes_only { 0.0 } else { slip_penalty(cost) };
                    let candidate = Candidate {
                        text: show(word),
                        kind: Kind::Correction,
                        score: score_of(count) - penalty,
                    };
                    if apostrophes_only
                        && contraction
                            .as_ref()
                            .is_none_or(|c: &Candidate| candidate.score > c.score)
                    {
                        contraction = Some(candidate.clone());
                    }
                    // A correction that only extends the typed word (`hel` → `help`,
                    // `don'` → `don't`) is a completion: it never takes the middle.
                    if starts_with_ignoring_case(word, typed_n) {
                        completions.push(Candidate {
                            kind: Kind::Completion,
                            ..candidate
                        });
                    } else {
                        if word.starts_with(|c: char| c.is_ascii_uppercase()) {
                            names.insert(candidate.text.clone());
                        }
                        if word.contains('\'') {
                            contractions_found.push((corrections.len(), score_of(count)));
                        } else {
                            bare_costs.insert(word.to_ascii_lowercase(), cost);
                        }
                        corrections.push((candidate, cost));
                    }
                });
            }
            // A contraction reached through its bare form costs only the slips to that
            // form (`whsts` → whats → what's): the apostrophe was never typed.
            for (index, base) in contractions_found {
                let (candidate, cost) = &mut corrections[index];
                let bare = candidate.text.replace('\'', "").to_ascii_lowercase();
                if rank::CONTRACTION_LOOKALIKES.contains(&bare.as_str()) {
                    continue;
                }
                if let Some(&bare_cost) = bare_costs.get(&bare).filter(|&&c| c < *cost) {
                    *cost = bare_cost;
                    candidate.score = candidate.score.max(base - slip_penalty(bare_cost));
                }
            }
            // The best near completion takes part only if its matched letters are no word
            // themselves (`recieve` is receive, not received) and it holds most of what
            // they begin; otherwise the typed letters are just a prefix.
            dominant = near_completion
                .filter(|n| lex.case_variants(&n.matched).is_empty())
                .filter(|n| {
                    let mass = lex.subtree_count(&n.matched);
                    n.count as f64 >= rank::COMPLETION_DOMINANT * mass as f64
                })
                .map(|n| n.candidate);
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
        // The latest learned address the letters begin takes the first side slot, shown as
        // learned (`Neo` → sam.2024@example.com): it is one thing typed before, not letters to
        // finish, so its length costs nothing. It never replaces, nor holds back a correction.
        let (addresses, words): (Vec<String>, Vec<String>) = self
            .user()
            .completions(typed_n, usize::MAX)
            .into_iter()
            .partition(|w| text::is_address(w));
        let mut address: Vec<Candidate> = addresses
            .into_iter()
            .take(1)
            .map(|word| Candidate {
                text: word,
                kind: Kind::Completion,
                score: rank::LEARNED_COMPLETION,
            })
            .collect();
        for word in words.into_iter().take(2) {
            let extra = word.len().saturating_sub(typed_n.len()) as f32;
            completions.push(Candidate {
                text: show(&word),
                kind: Kind::Completion,
                score: rank::LEARNED_COMPLETION - rank::COMPLETION_PER_CHAR * extra,
            });
        }
        // Removed words take no part (Manglish ones are taken out in `manglish`).
        self.drop_removed(&mut completions);
        self.drop_removed(&mut address);
        {
            let user = self.user();
            corrections.retain(|(c, _)| !user.is_removed(&c.text));
        }
        let case_fix = case_fix.filter(|c| !self.is_removed(c));
        let contraction = contraction.filter(|c| !self.is_removed(c));
        let dominant = dominant.filter(|c| !self.is_removed(c));

        let manglish =
            manglish.unwrap_or_else(|| self.manglish(typed_n, manglish_bias, false, deadline));

        let typed_c = verbatim(typed, known_score.unwrap_or(rank::UNKNOWN));
        // A real word (or a learned one) is never replaced, and only a
        // correction may replace: space never commits an unfinished word.
        let capitals = Capitals::of(typed_n, mid_sentence);
        let may_replace = autocorrect && !learned && !capitals.acronym;
        // Candidates that take the first side slots whatever their scores.
        let mut first_sides: Vec<Candidate> = Vec::new();
        let english_completions: Vec<Candidate> = {
            let mut found: Vec<Candidate> = completions
                .iter()
                .filter(|c| c.kind == Kind::Completion)
                .cloned()
                .collect();
            sort_and_dedup(&mut found);
            found
        };
        completions.extend(manglish.completions);
        let replacement = if known_score.is_some() {
            // A typed word in the lexicon is replaced only by its contraction, and only
            // when that is likelier and the word is not a real one spelled the same
            // (`rank::CONTRACTION_LOOKALIKES`); otherwise the contraction is offered.
            let lookalike =
                rank::CONTRACTION_LOOKALIKES.contains(&typed_n.to_ascii_lowercase().as_str());
            match contraction {
                Some(c) if may_replace && !lookalike && c.score > typed_c.score => Some(c),
                Some(c) => {
                    first_sides.push(c);
                    None
                }
                None => None,
            }
        } else if may_replace {
            let best_completion = completions
                .iter()
                .map(|c| c.score)
                .fold(f32::NEG_INFINITY, f32::max);
            // All English words that start with the typed letters, as one candidate.
            let prefix = self.english_prefix_log_prob(typed_n);
            // Typed letters that begin English words, four or more of them, are likelier
            // right so far than mistyped twice (`rank::PREFIX_KEEPS_LETTERS`).
            let keeps_prefix = typed_n.len() >= rank::PREFIX_KEEPS_LETTERS && prefix.is_finite();
            let best_correction = corrections
                .iter()
                .filter(|(c, _)| {
                    !capitals.deliberate
                        || (names.contains(&c.text) && same_first_letter(&c.text, typed_n))
                })
                .max_by(|a, b| a.0.score.total_cmp(&b.0.score))
                .cloned();
            // The English candidate, with its edit cost for the prefix checks, or None to
            // skip them: a contraction (no letter is wrong) or a dominant completion (it
            // is the likely prefix).
            let english = match (&case_fix, &contraction) {
                (Some(fix), _) => Some((fix.clone(), Some(0))),
                (None, Some(c)) => Some((c.clone(), None)),
                (None, None) => best_correction.map(|(c, cost)| (c, Some(cost))),
            };
            let english = match (english, dominant) {
                (Some(e), Some(d)) => Some(if d.score > e.0.score { (d, None) } else { e }),
                (e, d) => e.or(d.map(|d| (d, None))),
            };
            // A correction of two slips or more to typed letters that begin words.
            let held_back = keeps_prefix
                && english
                    .as_ref()
                    .is_some_and(|(_, cost)| cost.is_some_and(|c| c > fuzzy::CHEAP_EDIT));
            // A Malayalam word with exactly the typed skeleton competes as a
            // correction; one the typed letters spell out counts for more
            // against English, unless an English word comes before it.
            let spelled_bonus = if manglish_bias <= rank::CONTEXT_AFTER_ENGLISH {
                0.0
            } else {
                rank::MANGLISH_SPELLED_OVER_CORRECTION
            };
            let malayalam = manglish
                .exact
                .iter()
                .map(|(c, spelled)| {
                    let against_english = c.score + if *spelled { spelled_bonus } else { 0.0 };
                    (c.clone(), against_english)
                })
                .max_by(|a, b| a.1.total_cmp(&b.1))
                // A capitalised word after an English word is a name in English text.
                .filter(|_| !(capitals.deliberate && manglish_bias <= rank::CONTEXT_AFTER_ENGLISH));
            match (english, malayalam) {
                // Too close to call: keep what was typed and offer both.
                (Some((e, _)), Some((m, m_score)))
                    if (e.score - m_score).abs() < rank::AMBIGUOUS =>
                {
                    first_sides = vec![e, m];
                    None
                }
                (english, malayalam) => {
                    // A typed prefix more likely to be finished than mistyped stays as typed.
                    let english = english
                        .filter(|(e, cost)| match cost {
                            None => true,
                            Some(_) => {
                                e.score > best_completion
                                    && e.score > prefix + rank::PREFIX_VS_CORRECTION
                                    && !held_back
                            }
                        })
                        .map(|(e, _)| e);
                    let malayalam = malayalam.filter(|(c, against_english)| {
                        *against_english > best_completion
                            && c.score > prefix + rank::PREFIX_VS_MANGLISH
                    });
                    match (english, malayalam) {
                        (Some(e), Some((m, m_score))) => {
                            Some(if e.score > m_score { e } else { m })
                        }
                        (e, m) => e.or(m.map(|(m, _)| m)),
                    }
                }
            }
            .filter(|best| {
                best.score >= typed_c.score + rank::MARGIN && best.score >= rank::MIN_REPLACE
            })
            .or_else(|| {
                // Kept as a prefix where no Malayalam word fits the letters: the best
                // word they begin takes a side slot (Malayalam alternatives keep theirs).
                if held_back && manglish.exact.is_empty() {
                    first_sides.extend(english_completions.first().cloned());
                }
                None
            })
        } else {
            None
        };
        let mut others: Vec<Candidate> = corrections.into_iter().map(|(c, _)| c).collect();
        others.extend(completions);
        others.extend(case_fix);
        others.extend(manglish.exact.into_iter().map(|(c, _)| c));
        others.extend(manglish.side);
        address.extend(first_sides);
        arrange(typed_c, typed_n, others, replacement, address)
    }

    /// Log-probability of all English words that start with `typed` and are
    /// longer than it; minus infinity for none, or for a single letter.
    fn english_prefix_log_prob(&self, typed: &str) -> f32 {
        let Some(lex) = self.sources.english.as_ref().filter(|_| typed.len() >= 2) else {
            return f32::NEG_INFINITY;
        };
        let own: u64 = lex
            .case_variants(typed.as_bytes())
            .iter()
            .map(|(_, count)| count)
            .sum();
        match lex.subtree_count(typed.as_bytes()).saturating_sub(own) {
            0 => f32::NEG_INFINITY,
            mass => lex.log_prob(mass),
        }
    }

    /// Malayalam candidates for a Latin word, scored to compete with English.
    /// `english_word`: the typed word is in the English lexicon, so it isn't
    /// split into stem and suffix.
    /// `deadline`: as in `suggest_until`.
    fn manglish(
        &self,
        typed_n: &str,
        bias: f32,
        english_word: bool,
        deadline: Option<Deadline<'_>>,
    ) -> ManglishCandidates {
        let mut found = ManglishCandidates::default();
        let Some(lex) = &self.sources.manglish else {
            return found;
        };
        // One letter matches half the lexicon; an apostrophe is English.
        if typed_n.len() < 2 || typed_n.contains('\'') {
            return found;
        }
        let fit_bonus = |missing: usize| {
            if missing == 0 {
                rank::MANGLISH_EXACT_BONUS
            } else {
                -rank::MANGLISH_MISSING_PER_CHAR * missing as f32
            }
        };
        for m in manglish::lookup(lex, typed_n) {
            // Without the fit bonus: it ranks a word against English, but
            // must not lift a rare word (a name like ജിഷ്ണു) over the floor.
            let unfitted = m.log_prob - rank::MANGLISH_DISTANCE * m.distance as f32 + bias;
            found.push(
                m.word,
                unfitted,
                fit_bonus(m.missing),
                m.missing,
                m.spelled,
                rank::MIN_REPLACE,
            );
        }
        self.add_chat_form(typed_n, bias, &mut found);
        // No common whole form fits: read it as stem + suffixes (സുഖം + ആണോ).
        let splits = if found.exact.is_empty() && !english_word {
            self.scored_splits(lex, typed_n)
        } else {
            Vec::new()
        };
        if !splits.is_empty() {
            Self::add_splits(&splits, bias, &mut found);
        }
        // The model's spellings, held to the lexicon and the splits. Not for a word that is
        // English: it would never be replaced, and the model is the costliest step. Its free
        // spelling is only a fallback for when nothing else is found (below), and nothing is
        // ever taken out of `found`: with a candidate already, the free beam isn't run.
        let free = if english_word {
            None
        } else {
            let want_free = found.exact.is_empty() && found.side.is_empty();
            self.add_model(typed_n, &splits, bias, want_free, deadline, &mut found)
        };
        // A rare spelling one vowel sign from a far commoner word (ആവിശ്യം, ആവശ്യം)
        // is a variant of it: offer the common one beside it.
        self.add_standard_neighbour(&mut found);
        // Names and rare words: the model's own spelling (or, without the model, the
        // rule-based one), as a side-slot guess.
        if found.exact.is_empty() && found.side.is_empty() {
            let guess = match free {
                Some(text) => Some((text, rank::MANGLISH_MODEL_FREE)),
                None => transliterate(typed_n).map(|text| (text, rank::UNKNOWN)),
            };
            if let Some((text, score)) = guess {
                found.side.push(Candidate {
                    text,
                    kind: Kind::Manglish,
                    score,
                });
            }
        }
        {
            let user = self.user();
            found.exact.retain(|(c, _)| !user.is_removed(&c.text));
        }
        self.drop_removed(&mut found.completions);
        self.drop_removed(&mut found.side);
        found
    }

    /// A chat spelling in `data/chat-forms.tsv` (`nanni` → നന്ദി) offers its word as if the
    /// typed letters spelled it out, scored like a skeleton match with no distance. The
    /// table is curated evidence, not a guess, so the word may take the middle a little
    /// below `MIN_REPLACE` (`rank::MANGLISH_CHAT_FORM_RELIEF`), except where English is
    /// likely (after an English word, or a capital), as for the model's confident readings.
    fn add_chat_form(&self, typed_n: &str, bias: f32, found: &mut ManglishCandidates) {
        let (Some(word), Some(ml)) = (chat_forms::lookup(typed_n), &self.sources.malayalam) else {
            return;
        };
        let Some(count) = ml.get(word.as_bytes()) else {
            return;
        };
        let english_likely = bias <= rank::CONTEXT_AFTER_ENGLISH
            || typed_n.starts_with(|c: char| c.is_ascii_uppercase());
        let floor = if english_likely {
            rank::MIN_REPLACE
        } else {
            rank::MIN_REPLACE - rank::MANGLISH_CHAT_FORM_RELIEF
        };
        found.push(
            word.to_string(),
            ml.log_prob(count) + bias,
            rank::MANGLISH_EXACT_BONUS,
            0,
            true,
            floor,
        );
    }

    /// Offers the commonest Malayalam word one medial vowel sign (dropped) away
    /// from the best whole-word candidate, when it is far more common
    /// (`rank::MANGLISH_NEIGHBOUR_GAIN`): in its place for the middle when that
    /// candidate may take it (`rank::MANGLISH_NEIGHBOUR_PROMOTION`), else as a
    /// side slot (`rank::MANGLISH_NEIGHBOUR_PENALTY`).
    fn add_standard_neighbour(&self, found: &mut ManglishCandidates) {
        let Some(ml) = &self.sources.malayalam else {
            return;
        };
        let best = found
            .exact
            .iter()
            .map(|(c, _)| c)
            .chain(&found.side)
            .max_by(|a, b| a.score.total_cmp(&b.score));
        let Some(best) = best else {
            return;
        };
        // Its `spelled` flag, if it may take the middle (the same word may be
        // filed twice, by the skeleton lookup and by the model).
        let middle_spelled = found
            .exact
            .iter()
            .find(|(c, _)| c.text == best.text)
            .map(|(_, spelled)| *spelled);
        let Some(own) = ml.get(best.text.as_bytes()).map(|c| ml.log_prob(c)) else {
            return;
        };
        let mut neighbour: Option<(String, f32)> = None;
        for variant in malayalam::vowel_sign_drops(&best.text) {
            let Some(log_prob) = ml.get(variant.as_bytes()).map(|c| ml.log_prob(c)) else {
                continue;
            };
            if log_prob >= own + rank::MANGLISH_NEIGHBOUR_GAIN
                && neighbour.as_ref().is_none_or(|(_, lp)| log_prob > *lp)
            {
                neighbour = Some((variant, log_prob));
            }
        }
        let Some((text, _)) = neighbour else {
            return;
        };
        match middle_spelled {
            // The rare form would take the middle: the standard one takes it
            // instead, on the rare form's evidence, and the rare form moves aside.
            Some(spelled) => {
                let candidate = Candidate {
                    text,
                    kind: Kind::Manglish,
                    score: best.score + rank::MANGLISH_NEIGHBOUR_PROMOTION,
                };
                found.exact.push((candidate, spelled));
            }
            None => {
                let candidate = Candidate {
                    text,
                    kind: Kind::Manglish,
                    score: best.score - rank::MANGLISH_NEIGHBOUR_PENALTY,
                };
                found.side.push(candidate);
            }
        }
    }

    /// Suffix splits (`suffix::splits`) of `typed_n`, best first, each word once.
    fn scored_splits(&self, lex: &Lexicon, typed_n: &str) -> Vec<ScoredSplit> {
        let mut splits: Vec<ScoredSplit> =
            suffix::splits(lex, typed_n, rank::MANGLISH_SPLIT_STEM_MIN)
                .into_iter()
                .map(|split| {
                    let (spelled, distance) = manglish::fit(typed_n, &split.word, false);
                    let log_prob =
                        split.log_prob - rank::MANGLISH_SPLIT_PER_SUFFIX * split.suffixes as f32;
                    ScoredSplit {
                        word: split.word,
                        log_prob,
                        distance,
                        spelled,
                    }
                })
                .collect();
        splits.sort_by(|a, b| {
            let score = |s: &ScoredSplit| s.log_prob - rank::MANGLISH_DISTANCE * s.distance as f32;
            score(b).total_cmp(&score(a))
        });
        let mut seen = std::collections::HashSet::new();
        splits.retain(|s| seen.insert(s.word.clone()));
        splits
    }

    /// Suffix splits of a word with no common whole form, scored as their
    /// stem. A whole form always beats a split: a split that
    /// spells a rare whole form already found lifts it to the split's score
    /// (both readings agree), and any other split stays in a side slot, below
    /// the rare whole forms.
    fn add_splits(splits: &[ScoredSplit], bias: f32, found: &mut ManglishCandidates) {
        let fit = rank::MANGLISH_EXACT_BONUS;
        let rare_whole = found
            .side
            .iter()
            .map(|c| c.score)
            .fold(f32::NEG_INFINITY, f32::max);
        for split in splits {
            let word = split.word.clone();
            let spelled = split.spelled;
            let unfitted = split.log_prob - rank::MANGLISH_DISTANCE * split.distance as f32 + bias;
            if let Some(i) = found.side.iter().position(|c| c.text == word) {
                let whole = found.side.remove(i);
                found.push(
                    word,
                    unfitted.max(whole.score - fit),
                    fit,
                    0,
                    spelled,
                    rank::MIN_REPLACE,
                );
            } else if rare_whole.is_finite() {
                found.side.push(Candidate {
                    text: word,
                    kind: Kind::Manglish,
                    score: (unfitted + fit).min(rare_whole - 0.01),
                });
            } else {
                found.push(word, unfitted, fit, 0, spelled, rank::MIN_REPLACE);
            }
        }
    }

    /// The transliterator's spellings of `typed_n` that are lexicon words or `splits`, added
    /// as whole words; returns its free spelling (off the lexicon), if any. A lexicon word
    /// scores its log probability, a split its stem's (as in `add_splits`), plus
    /// `MANGLISH_MODEL_WEIGHT` × the model's per-token score, in place of the skeleton
    /// lookup's romanisation-distance penalty, except where English is likely
    /// (see `rank::MANGLISH_MODEL_WEIGHT`). One the skeleton lookup or a split found too
    /// appears twice, and the better score wins (`arrange`, and the max over `exact`).
    /// Nothing without the model or the Malayalam lexicon it is held to. `free`: run the
    /// free beam too. `deadline`: as in `suggest_until`.
    fn add_model(
        &self,
        typed_n: &str,
        splits: &[ScoredSplit],
        bias: f32,
        free: bool,
        deadline: Option<Deadline<'_>>,
        found: &mut ManglishCandidates,
    ) -> Option<String> {
        let (Some(model), Some(ml)) = (&self.sources.translit, &self.sources.malayalam) else {
            return None;
        };
        let split_words: Vec<String> = splits.iter().map(|s| s.word.clone()).collect();
        let latin = typed_n.to_ascii_lowercase();
        let keeps_distance = bias <= rank::CONTEXT_AFTER_ENGLISH
            || typed_n.starts_with(|c: char| c.is_ascii_uppercase());
        let search = Search {
            width: MODEL_WIDTH,
            free,
            deadline,
        };
        let mut free = None;
        for t in model.search(&latin, ml, &split_words, &search).0 {
            if !t.in_lexicon {
                free = Some(t.text);
                continue;
            }
            if t.score < rank::MANGLISH_MODEL_FLOOR {
                continue;
            }
            let prior = match ml.get(t.text.as_bytes()) {
                Some(count) => ml.log_prob(count),
                None => match splits.iter().find(|s| s.word == t.text) {
                    Some(split) => split.log_prob,
                    None => continue,
                },
            };
            let (spelled, distance) = manglish::fit(typed_n, &t.text, false);
            let distance_penalty = if keeps_distance {
                rank::MANGLISH_DISTANCE * distance as f32
            } else {
                0.0
            };
            let unfitted = prior - distance_penalty + rank::MANGLISH_MODEL_WEIGHT * t.score + bias;
            // A confident model reading is evidence the rare word is meant, not a guess.
            let floor = if !keeps_distance && t.score >= rank::MANGLISH_MODEL_CONFIDENT {
                rank::MIN_REPLACE - rank::MANGLISH_MODEL_CONFIDENT_RELIEF
            } else {
                rank::MIN_REPLACE
            };
            found.push(
                t.text,
                unfitted,
                rank::MANGLISH_EXACT_BONUS,
                0,
                spelled,
                floor,
            );
        }
        free
    }

    /// An address is never corrected or read as words: what was typed stays in the
    /// middle, beside learned words it begins (`priya@e` → priya@example.com), shown as learned.
    fn suggest_address(&self, typed: &str, typed_n: &str) -> Vec<Candidate> {
        let mut others: Vec<Candidate> = self
            .user()
            .completions(typed_n, 2)
            .into_iter()
            .map(|word| Candidate {
                // One thing typed before: its length costs nothing (`suggest_latin`).
                score: rank::LEARNED_COMPLETION,
                text: word,
                kind: Kind::Completion,
            })
            .collect();
        self.drop_removed(&mut others);
        arrange(verbatim(typed, 0.0), typed_n, others, None, Vec::new())
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
        self.drop_removed(&mut others);
        // Native Malayalam is never replaced: the typed word always stays in the middle.
        arrange(verbatim(typed, 0.0), &typed_n, others, None, Vec::new())
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
        self.drop_removed(&mut found);
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

/// A suffix split (`suffix::Split`) with how closely the typed letters fit it.
struct ScoredSplit {
    word: String,
    /// The stem's log probability, less `MANGLISH_SPLIT_PER_SUFFIX` per suffix.
    log_prob: f32,
    /// As in `manglish::fit`.
    distance: usize,
    spelled: bool,
}

/// Manglish candidates for one typed word, by the role each may play.
#[derive(Default)]
struct ManglishCandidates {
    /// Same skeleton as typed and common enough: may take the middle, like a
    /// correction. The flag is `manglish::Match::spelled`.
    exact: Vec<(Candidate, bool)>,
    /// Longer words: completions, side slots only.
    completions: Vec<Candidate>,
    /// Side slots only, and they don't stop a correction: rare words with the
    /// typed skeleton, or else the transliteration (names, words not in the list).
    side: Vec<Candidate>,
}

/// A long typed word read as a near match of a key's first letters (`matched`) plus a
/// few more (`fuzzy::WeightedEdit::with_tail`), scored for the middle.
struct NearCompletion {
    candidate: Candidate,
    matched: Vec<u8>,
    count: u64,
}

impl ManglishCandidates {
    /// Files a Malayalam word by role. `unfitted` is its score before the
    /// `fit` bonus for how much of its skeleton was typed; `missing` and
    /// `spelled` are as in `manglish::Match`. `floor` is the least `unfitted`
    /// that may take the middle (`rank::MIN_REPLACE`, or below it for a
    /// confident model reading: `rank::MANGLISH_MODEL_CONFIDENT`).
    fn push(
        &mut self,
        text: String,
        unfitted: f32,
        fit: f32,
        missing: usize,
        spelled: bool,
        floor: f32,
    ) {
        let candidate = Candidate {
            text,
            kind: Kind::Manglish,
            score: unfitted + fit,
        };
        if missing > 0 {
            self.completions.push(candidate);
        } else if unfitted >= floor {
            self.exact.push((candidate, spelled));
        } else {
            self.side.push(candidate);
        }
    }
}

/// Capitals the user typed on purpose, which autocorrect respects. The keyboard
/// capitalises the first word of the text and of each sentence on its own, so only a
/// capital elsewhere, or a run of capitals, says anything.
#[derive(Debug, Clone, Copy)]
struct Capitals {
    /// Two to four letters, all capitals (`PNR`, `OTP`): an abbreviation. Never replaced:
    /// every short string is one edit from some word or other abbreviation (`PNR` → `ONE`).
    acronym: bool,
    /// An acronym, or a capitalised word mid-sentence (`for Vishu`, `to Munnar`): a
    /// name. Corrected only to another name with its first letter (`Lodnon` → `London`,
    /// not `Fish`, `Lunar` or `Rail`), and not replaced by Malayalam after an English
    /// word (`Add Rahul`).
    deliberate: bool,
}

impl Capitals {
    fn of(typed: &str, mid_sentence: bool) -> Self {
        let letters = typed.chars().filter(char::is_ascii_alphabetic).count();
        let acronym = (2..=4).contains(&letters)
            && letters == typed.chars().count()
            && typed.chars().all(|c| c.is_ascii_uppercase());
        let capital = typed.starts_with(|c: char| c.is_ascii_uppercase());
        Self {
            acronym,
            deliberate: acronym || (capital && mid_sentence),
        }
    }
}

fn same_first_letter(a: &str, b: &str) -> bool {
    match (a.chars().next(), b.chars().next()) {
        (Some(x), Some(y)) => x.eq_ignore_ascii_case(&y),
        _ => false,
    }
}

/// Whether `word` is `typed` with apostrophes put back, ignoring case.
/// What a correction's edits cost (`fuzzy` half-edits) against its frequency.
fn slip_penalty(cost: u8) -> f32 {
    rank::EDIT_PENALTY * f32::from(cost)
        + rank::EXTRA_SLIP_PENALTY * f32::from(cost.saturating_sub(fuzzy::EDIT))
}

fn only_apostrophes_missing(word: &str, typed: &str) -> bool {
    let letters = word.bytes().filter(|&b| b != b'\'');
    word.contains('\'')
        && letters
            .map(|b| b.to_ascii_lowercase())
            .eq(typed.bytes().map(|b| b.to_ascii_lowercase()))
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
/// `first_sides` take the first side slots whatever their scores (an
/// ambiguous pair). Nothing equal to the typed word, as typed or normalised, is repeated.
fn arrange(
    typed: Candidate,
    typed_n: &str,
    mut others: Vec<Candidate>,
    replacement: Option<Candidate>,
    first_sides: Vec<Candidate>,
) -> Vec<Candidate> {
    others.retain(|c| c.text != typed.text && c.text != typed_n);
    let mut out = Vec::with_capacity(SHOWN);
    if let Some(best) = replacement.filter(|r| r.text != typed.text && r.text != typed_n) {
        others.retain(|c| c.text != best.text);
        out.push(best);
    }
    sort_and_dedup(&mut others);
    out.push(typed);
    let mut sides: Vec<Candidate> = first_sides
        .into_iter()
        .filter(|c| c.text != typed_n && !out.iter().any(|o| o.text == c.text))
        .collect();
    others.retain(|c| !sides.iter().any(|s| s.text == c.text));
    sides.extend(others);
    out.extend(sides.into_iter().take(SHOWN));
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
            manglish: None,
            translit: None,
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
    fn a_missing_apostrophe_beats_a_commoner_word_two_slips_away() {
        // On Wikipedia "song" is five times as common as "don't", and d→s, t→g
        // are neighbouring keys; leaving out the apostrophe is still the likelier slip.
        let sources = Sources {
            english: lex(&[("song", 5000), ("don't", 1000), ("the", 100_000)]),
            ..Sources::default()
        };
        let engine = Engine::new(sources, UserStore::in_memory());
        let s = engine.suggest("dont", Layout::Latin, true);
        assert_eq!(s.candidates[0].text, "don't");
        assert_eq!(s.candidates[0].kind, Kind::Correction);
        let s = engine.suggest("Dont", Layout::Latin, true);
        assert_eq!(s.candidates[0].text, "Don't");
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
        engine.note_committed("Thomasina");
        assert!(!texts(&engine.suggest("thoma", Layout::Latin, true)).contains(&"Thomasina"));
        engine.note_committed("Thomasina");
        assert!(texts(&engine.suggest("thoma", Layout::Latin, true)).contains(&"Thomasina"));
        engine.note_committed("hello");
        assert!(!engine.user().is_learned("hello"));
    }

    #[test]
    fn a_word_with_a_confident_correction_is_not_learned_from_commits() {
        // Committed twice where autocorrect couldn't run (a search field): still a typo.
        let sources = Sources {
            english: lex(&[("thanks", 500), ("the", 1000)]),
            ..Sources::default()
        };
        let engine = Engine::new(sources, UserStore::in_memory());
        assert_eq!(
            engine.suggest("thnaks", Layout::Latin, true).candidates[0].text,
            "thanks"
        );
        engine.note_committed("thnaks");
        engine.note_committed("thnaks");
        assert!(!engine.user().is_learned("thnaks"));
        assert_eq!(
            engine.suggest("thnaks", Layout::Latin, true).candidates[0].text,
            "thanks"
        );
        // An explicit choice (undo, or a tap on the quoted word) still learns it.
        engine.learn("thnaks");
        assert_eq!(
            engine.suggest("thnaks", Layout::Latin, true).candidates[0].text,
            "thnaks"
        );
    }

    #[test]
    fn removed_words_leave_next_words_and_malayalam_completions() {
        let engine = engine();
        engine.remove_word("am");
        assert_eq!(
            texts(&engine.suggest("I ", Layout::Latin, true)),
            ["have", "was", "will"]
        );
        engine.remove_word("എന്റെ");
        assert_eq!(
            texts(&engine.suggest("എന്", Layout::Malayalam, true)),
            ["എന്", "എന്ത്"]
        );
        // The verbatim slot is what was typed, removed or not.
        assert_eq!(
            texts(&engine.suggest("എന്റെ", Layout::Malayalam, true))[0],
            "എന്റെ"
        );
    }

    #[test]
    fn your_words_lists_forgets_one_and_restores_one() {
        let engine = engine();
        engine.learn("Thomasina");
        engine.learn("Jishnu");
        assert_eq!(engine.learned_words(), ["Jishnu", "Thomasina"]);
        engine.forget_word("Thomasina");
        assert_eq!(engine.learned_words(), ["Jishnu"]);
        assert!(!texts(&engine.suggest("thoma", Layout::Latin, true)).contains(&"Thomasina"));
        assert!(engine.removed_words().is_empty(), "forgotten, not removed");

        engine.remove_word("am");
        assert_eq!(engine.removed_words(), ["am"]);
        engine.restore_word("am");
        assert!(engine.removed_words().is_empty());
        assert!(texts(&engine.suggest("I ", Layout::Latin, true)).contains(&"am"));
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

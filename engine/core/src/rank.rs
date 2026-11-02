//! Every ranking weight in one place. Scores are natural-log probabilities
//! plus bonuses and penalties; higher is better. The golden tests in
//! `tests/golden.rs` pin the behaviour these numbers produce.

/// Added to every English word found in the lexicon. It sets how English
/// candidates weigh against Manglish ones.
pub const KNOWN_BONUS: f32 = 2.0;
/// Score of a typed word in no lexicon. Anything plausible beats it.
pub const UNKNOWN: f32 = -18.0;
/// Penalty per half-edit (`fuzzy::CHEAP_EDIT`) of a correction.
pub const EDIT_PENALTY: f32 = 1.6;
/// Penalty for a completion, plus a little for every letter it adds.
pub const COMPLETION_PENALTY: f32 = 1.0;
pub const COMPLETION_PER_CHAR: f32 = 0.3;
/// Score of a learned word offered as a completion, before the per-letter penalty.
pub const LEARNED_COMPLETION: f32 = -4.0;
/// A candidate must beat the typed word by this much to take the middle slot.
/// A typed word that is in the English lexicon is never replaced at all.
pub const MARGIN: f32 = 2.0;
/// Nothing scoring below this replaces the typed word, however unknown it is:
/// a rare word is a guess, not a correction.
pub const MIN_REPLACE: f32 = -13.0;

/// The English words that start with the typed letters (`ent` → enter,
/// entire, entry …) compete with any replacement as one candidate, scored by
/// their summed log-probability plus a bonus. Space never commits an
/// unfinished word, and a common prefix is more likely unfinished than mistyped.
///
/// Tuned on the real bundle (Task 17). Against an English correction the sum
/// counts as an English word (`KNOWN_BONUS`): `thi` stays (this, think …: −3.6)
/// rather than becoming `the` (−3.75); `wor` and `bec` likewise. `teh` (−9.9)
/// still becomes `the`.
pub const PREFIX_VS_CORRECTION: f32 = KNOWN_BONUS;
/// Against a Manglish word with the typed skeleton, the sum gets less: the
/// skeleton match is itself evidence the word is finished. `ent` stays (−6.6
/// against എന്റെ at −7.3); `ente` (−7.5 against −6.5) and `ende` (−8.3) convert.
pub const PREFIX_VS_MANGLISH: f32 = 0.5;

/// Manglish: when nothing points to English (no English word before it), a
/// Malayalam word the typed letters spell out (up to vowel length) gets this
/// much over an English *correction*: a correction needs a typo, a spelled
/// word needs none. `nale` → നാളെ (−9.7 + 4.5) rather than `name` (−6.9), clear of `AMBIGUOUS`; `teh`
/// spells no Malayalam word and still becomes `the`. Not counted against the
/// English prefix sum above, or `thi` would become തി.
pub const MANGLISH_SPELLED_OVER_CORRECTION: f32 = 4.5;
/// When the best English replacement and the best Malayalam one score within
/// this of each other, neither takes the middle: the typed word stays, and
/// both go in the side slots (`I feel pani` → pani, with pain and പണി). Half
/// of `MARGIN`: on the small golden lexicon `pain` is 15 times as common as
/// പണി and leads by 1.6 after an English word, which must still convert.
pub const AMBIGUOUS: f32 = 1.0;

/// Manglish: bonus when the typed skeleton is the word's whole skeleton.
pub const MANGLISH_EXACT_BONUS: f32 = 1.5;
/// Manglish: penalty per skeleton letter the word has beyond what was typed.
pub const MANGLISH_MISSING_PER_CHAR: f32 = 0.4;
/// Manglish: penalty per edit between the typed letters and the word's romanisation.
pub const MANGLISH_DISTANCE: f32 = 0.8;
/// Manglish suffix splitting (`suffix.rs`): a word read as stem + suffixes
/// scores as its stem, less this per suffix: the joined form is rarer than
/// the stem, and the reading is a guess. `sukhamano` → സുഖം (−9.8) + ആണോ.
pub const MANGLISH_SPLIT_PER_SUFFIX: f32 = 1.0;
/// Stems rarer than this are not split from: a rare stem plus a suffix is
/// more often nonsense than a word.
pub const MANGLISH_SPLIT_STEM_MIN: f32 = -12.5;
/// Manglish bias from the previous word: after Malayalam, after English, at the start.
pub const CONTEXT_AFTER_MALAYALAM: f32 = 2.0;
pub const CONTEXT_AFTER_ENGLISH: f32 = -2.0;
pub const CONTEXT_AT_START: f32 = -1.0;

/// Makes a suggestion follow the case of what was typed: `Hel` → `Hello`,
/// `HEL` → `HELLO`. Words stored with capitals (London, I, NASA) keep them.
pub fn apply_case(typed: &str, candidate: &str) -> String {
    if !candidate.is_ascii() {
        return candidate.to_string();
    }
    let letters = typed.chars().filter(char::is_ascii_alphabetic).count();
    if letters >= 2 && typed.chars().all(|c| !c.is_ascii_lowercase()) {
        return candidate.to_ascii_uppercase();
    }
    if typed.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        return capitalize(candidate);
    }
    candidate.to_string()
}

pub fn capitalize(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_follows_typing() {
        assert_eq!(apply_case("Hel", "hello"), "Hello");
        assert_eq!(apply_case("hel", "hello"), "hello");
        assert_eq!(apply_case("HEL", "hello"), "HELLO");
        assert_eq!(
            apply_case("H", "hello"),
            "Hello",
            "one capital letter is a capitalised word"
        );
        assert_eq!(apply_case("lon", "London"), "London");
        assert_eq!(apply_case("i", "I"), "I");
        assert_eq!(apply_case("Ent", "എന്റെ"), "എന്റെ");
    }
}

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

/// Manglish: bonus when the typed skeleton is the word's whole skeleton.
pub const MANGLISH_EXACT_BONUS: f32 = 1.5;
/// Manglish: penalty per skeleton letter the word has beyond what was typed.
pub const MANGLISH_MISSING_PER_CHAR: f32 = 0.4;
/// Manglish: penalty per edit between the typed letters and the word's romanisation.
pub const MANGLISH_DISTANCE: f32 = 0.8;
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

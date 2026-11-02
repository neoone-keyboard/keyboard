//! Text helpers that must be correct for both English and Malayalam.

use unicode_segmentation::UnicodeSegmentation;

/// Returns the word being typed at the end of `context_before`, or "" if the
/// text ends in a space or punctuation.
pub fn current_word(context_before: &str) -> &str {
    // Walk grapheme clusters backwards so combining marks, virama, ZWJ and ZWNJ
    // stay attached to their base character.
    let start = context_before
        .grapheme_indices(true)
        .rev()
        .take_while(|&(_, g)| is_word_grapheme(g))
        .last()
        .map(|(i, _)| i);
    let word = match start {
        Some(i) => context_before[i..].trim_start_matches(is_apostrophe),
        None => return "",
    };
    if word.chars().any(char::is_alphanumeric) { word } else { "" }
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}')
}

/// A grapheme belongs to a word if it starts with a letter, digit or apostrophe.
fn is_word_grapheme(g: &str) -> bool {
    g.chars().next().is_some_and(|c| c.is_alphanumeric() || is_apostrophe(c))
}

#[cfg(test)]
mod tests {
    use super::current_word;

    #[test]
    fn english_word_at_end() {
        assert_eq!(current_word("hello wor"), "wor");
    }

    #[test]
    fn empty_after_space_or_punctuation() {
        assert_eq!(current_word("hello "), "");
        assert_eq!(current_word("hello,"), "");
        assert_eq!(current_word(""), "");
    }

    #[test]
    fn apostrophes_stay_inside_words() {
        assert_eq!(current_word("I don't"), "don't");
        assert_eq!(current_word("I don\u{2019}t"), "don\u{2019}t");
    }

    #[test]
    fn malayalam_with_virama_and_vowel_signs() {
        // "എന്റ" contains a virama (U+0D4D), which is not alphabetic in Unicode.
        assert_eq!(current_word("ഞാൻ എന്റ"), "എന്റ");
        assert_eq!(current_word("കീബോ"), "കീബോ");
    }

    #[test]
    fn malayalam_chillu_and_zero_width_joiners() {
        assert_eq!(current_word("അവൻ"), "അവൻ");
        assert_eq!(current_word("x ന\u{0D4D}\u{200D}"), "ന\u{0D4D}\u{200D}");
    }

    #[test]
    fn digits_count_as_word_characters() {
        assert_eq!(current_word("room 42"), "42");
    }

    #[test]
    fn combining_marks_attach_to_their_base() {
        assert_eq!(current_word("cafe\u{301}"), "cafe\u{301}");
        assert_eq!(current_word("e\u{301}x"), "e\u{301}x");
    }

    #[test]
    fn malayalam_sign_variants_stay_in_word() {
        assert_eq!(current_word("ക\u{0D3B}"), "ക\u{0D3B}");
        assert_eq!(current_word("ക\u{0D3C}"), "ക\u{0D3C}");
        assert_eq!(current_word("x ക\u{0D3B}"), "ക\u{0D3B}");
    }

    #[test]
    fn emoji_and_whitespace_are_not_word_characters() {
        assert_eq!(current_word("👍"), "");
        assert_eq!(current_word("a\nb"), "b");
        assert_eq!(current_word("a\tb"), "b");
    }

    #[test]
    fn malayalam_signs_and_digits_are_kept_whole() {
        assert_eq!(current_word("കം"), "കം");
        assert_eq!(current_word("ദുഃ"), "ദുഃ");
        assert_eq!(current_word("൧൨"), "൧൨");
    }

    #[test]
    fn hyphen_splits_words() {
        assert_eq!(current_word("well-known"), "known");
    }

    #[test]
    fn never_returns_a_word_without_letters_or_digits() {
        assert_eq!(current_word("x \u{200C}"), "");
        assert_eq!(current_word("👨\u{200D}"), "");
        assert_eq!(current_word("''"), "");
    }

    #[test]
    fn leading_apostrophes_are_dropped() {
        assert_eq!(current_word("'hi"), "hi");
        assert_eq!(current_word("don't"), "don't");
    }

    proptest::proptest! {
        #[test]
        fn current_word_is_always_a_suffix(s in "\\PC{0,30}") {
            proptest::prop_assert!(s.ends_with(current_word(&s)));
        }
    }
}

//! Text helpers that must be correct for both English and Malayalam.

/// Returns the word being typed at the end of `context_before`, or "" if the
/// text ends in a space or punctuation.
pub fn current_word(context_before: &str) -> &str {
    let start = context_before
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is_word_char(c))
        .last()
        .map(|(i, _)| i);
    match start {
        Some(i) => &context_before[i..],
        None => "",
    }
}

/// Letters, digits, Malayalam vowel signs (Unicode-alphabetic), plus the
/// characters that join a word but aren't alphabetic: apostrophes, virama, ZWNJ, ZWJ.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '\'' | '\u{2019}' | '\u{0D4D}' | '\u{200C}' | '\u{200D}')
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
}

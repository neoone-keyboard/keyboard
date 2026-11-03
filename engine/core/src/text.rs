//! Text helpers that must be correct for both English and Malayalam.

use unicode_segmentation::UnicodeSegmentation;

/// Returns the word being typed at the end of `context_before`, or "" if the
/// text ends in a space or punctuation. An address (`is_address`) is one word back
/// to the whitespace before it, punctuation and all.
pub fn current_word(context_before: &str) -> &str {
    if let Some(address) = address_at_end(context_before) {
        return address;
    }
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

/// The address being typed at the end of `context_before`, if `last_token` is one.
fn address_at_end(context_before: &str) -> Option<&str> {
    let token = last_token(context_before);
    is_address(token).then_some(token)
}

/// The text after the last whitespace in `context_before`, less opening brackets and
/// quotes: what an address being typed would be (`neo.9` of a learned sam.2024@example.com).
pub fn last_token(context_before: &str) -> &str {
    let start = context_before
        .trim_end_matches(|c: char| !c.is_whitespace())
        .len();
    context_before[start..].trim_start_matches(is_opening)
}

fn is_opening(c: char) -> bool {
    matches!(
        c,
        '(' | '[' | '{' | '<' | '"' | '\'' | '\u{201C}' | '\u{2018}'
    )
}

fn is_closing(c: char) -> bool {
    matches!(
        c,
        ')' | ']' | '}' | '>' | '"' | '\'' | '\u{201D}' | '\u{2019}'
    )
}

/// An email or handle (it has an `@`) or a URL (it starts with `http://`, `https://` or
/// `www.`, in any case), with a letter or digit. Typed as one word: never corrected,
/// and learned whole.
pub fn is_address(token: &str) -> bool {
    let starts = |prefix: &str| {
        token
            .get(..prefix.len())
            .is_some_and(|s| s.eq_ignore_ascii_case(prefix))
    };
    (token.contains('@')
        || starts("http://")
        || starts("https://")
        || starts("www.")
        || is_domain(token))
        && token.chars().any(char::is_alphanumeric)
}

/// Top-level domains that make `name.tld` (and `name.tld/path`) an address. Not ones that
/// are English words (`it`, `is`, `to`, `no` …): `end.it` is a missed space, not a site.
pub const DOMAIN_ENDINGS: [&str; 34] = [
    "com", "net", "org", "edu", "gov", "mil", "int", "io", "dev", "app", "ai", "co", "uk", "in",
    "me", "ca", "de", "fr", "jp", "au", "nz", "sg", "ae", "ly", "gg", "tv", "info", "biz", "xyz",
    "tech", "site", "online", "store", "blog",
];

/// `example.com`, `example.com/blog`, `gov.in`: labels of letters, digits and hyphens joined by
/// dots, the last one in `DOMAIN_ENDINGS`, then an optional `/path`.
fn is_domain(token: &str) -> bool {
    let host = token.split('/').next().unwrap_or(token);
    let mut labels = host.split('.');
    let Some(ending) = host.rsplit('.').next() else {
        return false;
    };
    host.contains('.')
        && labels.all(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        && DOMAIN_ENDINGS.iter().any(|e| e.eq_ignore_ascii_case(ending))
}

/// An address as it is learned, without the brackets, quotes and punctuation typed
/// around it (`(priya@example.com),`); None for a word that is no address.
pub fn learnable_address(word: &str) -> Option<&str> {
    let trimmed = word
        .trim_start_matches(is_opening)
        .trim_end_matches(|c: char| {
            matches!(c, '.' | ',' | ';' | ':' | '?' | '!') || is_closing(c)
        });
    is_address(trimmed).then_some(trimmed)
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}')
}

/// A grapheme belongs to a word if it starts with a letter, digit or apostrophe.
fn is_word_grapheme(g: &str) -> bool {
    g.chars().next().is_some_and(|c| c.is_alphanumeric() || is_apostrophe(c))
}

/// Which writing system a word is in. Only `Latin` and `Malayalam` words go into a lexicon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    /// ASCII letters, optionally with apostrophes inside (don't).
    Latin,
    /// Malayalam letters and signs (U+0D00–U+0D63, U+0D7A–U+0D7F), ZWJ and ZWNJ.
    Malayalam,
    /// Anything else: digits, mixed scripts, other alphabets.
    Other,
}

pub fn script(word: &str) -> Script {
    if word.is_empty() {
        return Script::Other;
    }
    if word.bytes().all(|b| b.is_ascii_alphabetic() || b == b'\'')
        && word.bytes().any(|b| b.is_ascii_alphabetic())
    {
        return Script::Latin;
    }
    let malayalam_letter = |c: char| ('\u{0D00}'..='\u{0D63}').contains(&c) || ('\u{0D7A}'..='\u{0D7F}').contains(&c);
    if word.chars().all(|c| malayalam_letter(c) || c == '\u{200C}' || c == '\u{200D}')
        && word.chars().any(malayalam_letter)
    {
        return Script::Malayalam;
    }
    Script::Other
}

/// A char that can be part of a word when tokenising a corpus line. Cheaper
/// than grapheme segmentation, which matters for 500M words.
fn is_token_char(c: char) -> bool {
    c.is_alphanumeric() || ('\u{0D00}'..='\u{0D7F}').contains(&c) || matches!(c, '\u{200C}' | '\u{200D}') || is_apostrophe(c)
}

/// Splits a line of corpus text into words. Apostrophes are kept inside words
/// (don't) but trimmed from their ends ('quoted').
pub fn words(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| !is_token_char(c))
        .map(|w| w.trim_matches(is_apostrophe))
        .filter(|w| !w.is_empty())
}

/// The last complete word before the cursor when the text ends in a space,
/// plus whether a sentence ended right after it (". ", "? ", "! ").
/// Returns None while a word is being typed or when there is no word.
pub fn previous_word(context_before: &str) -> Option<(&str, bool)> {
    if !context_before.ends_with(char::is_whitespace) {
        return None;
    }
    let trimmed = context_before.trim_end();
    // A line break ends a sentence too (a new line in a chat is a new message).
    let sentence_end =
        trimmed.ends_with(['.', '?', '!']) || context_before[trimmed.len()..].contains('\n');
    let without_punct = trimmed.trim_end_matches(|c: char| !is_token_char(c) || is_apostrophe(c));
    match current_word(without_punct) {
        "" => None,
        word => Some((word, sentence_end)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn scripts() {
        assert_eq!(script("hello"), Script::Latin);
        assert_eq!(script("don't"), Script::Latin);
        assert_eq!(script("എന്റെ"), Script::Malayalam);
        assert_eq!(script("അവൻ"), Script::Malayalam);
        assert_eq!(script("abcഎ"), Script::Other);
        assert_eq!(script("42"), Script::Other);
        assert_eq!(script("൧൨"), Script::Other);
        assert_eq!(script("café"), Script::Other);
        assert_eq!(script("'"), Script::Other);
        assert_eq!(script(""), Script::Other);
    }

    #[test]
    fn words_split_on_spaces_and_punctuation() {
        let got: Vec<&str> = words("Hello, world! I don't 'know' എന്റെ വീട്.").collect();
        assert_eq!(got, ["Hello", "world", "I", "don't", "know", "എന്റെ", "വീട്"]);
    }

    #[test]
    fn previous_word_after_a_space() {
        assert_eq!(previous_word("I am "), Some(("am", false)));
        assert_eq!(previous_word("It rains. "), Some(("rains", true)));
        assert_eq!(previous_word("Really? "), Some(("Really", true)));
        assert_eq!(previous_word("ഞാൻ "), Some(("ഞാൻ", false)));
        assert_eq!(previous_word("ok\n"), Some(("ok", true)));
        assert_eq!(previous_word("ok \n "), Some(("ok", true)));
        assert_eq!(previous_word("I am"), None);
        assert_eq!(previous_word(" "), None);
        assert_eq!(previous_word(""), None);
    }

    #[test]
    fn an_address_is_one_word() {
        assert_eq!(current_word("mail priya@example.com"), "priya@example.com");
        assert_eq!(current_word("mail priya@example."), "priya@example.");
        assert_eq!(current_word("mail jishnu@"), "jishnu@");
        assert_eq!(current_word("see https://x.com/a-b"), "https://x.com/a-b");
        assert_eq!(current_word("see http://x"), "http://x");
        assert_eq!(current_word("Www.x.com"), "Www.x.com");
        assert_eq!(current_word("(priya@example.com"), "priya@example.com");
        assert_eq!(current_word("a\npriya@example.com"), "priya@example.com");
    }

    #[test]
    fn other_punctuation_still_ends_a_word() {
        assert_eq!(current_word("x.com"), "x.com");
        assert_eq!(current_word("see example.com/blog"), "example.com/blog");
        assert_eq!(current_word("end.it"), "it", "a missed space, not a site");
        assert_eq!(current_word("x.co"), "x.co");
        assert_eq!(current_word("x.c"), "c", "not a domain yet");
        assert_eq!(current_word("wwwx.com"), "wwwx.com");
        assert_eq!(current_word("hello."), "");
        assert_eq!(current_word("@"), "");
        assert_eq!(current_word("me @"), "");
    }

    #[test]
    fn last_token_runs_to_the_whitespace() {
        assert_eq!(last_token("hi neo.9"), "neo.9");
        assert_eq!(last_token("(neo."), "neo.");
        assert_eq!(last_token("a\nb.c"), "b.c");
        assert_eq!(last_token("hi "), "");
    }

    #[test]
    fn addresses() {
        assert!(is_address("priya@example.com"));
        assert!(is_address("@jishnu"));
        assert!(is_address("HTTPS://x"));
        assert!(is_address("www.x"));
        assert!(!is_address("@"));
        assert!(is_address("x.com"));
        assert!(is_address("Google.COM"));
        assert!(is_address("mail.gov.in"));
        assert!(!is_address("end.it"));
        assert!(!is_address("hello."));
        assert!(!is_address("a..com"));
        assert!(!is_address("hello"));
    }

    #[test]
    fn previous_word_after_an_address() {
        assert_eq!(
            previous_word("mail priya@example.com "),
            Some(("priya@example.com", false))
        );
        assert_eq!(previous_word("at x@y.com. "), Some(("x@y.com", true)));
        assert_eq!(
            previous_word("(see www.x.com) "),
            Some(("www.x.com", false))
        );
    }

    proptest::proptest! {
        #[test]
        fn current_word_is_always_a_suffix(s in "\\PC{0,30}") {
            proptest::prop_assert!(s.ends_with(current_word(&s)));
        }
    }
}

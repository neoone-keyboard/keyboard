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

/// An email or handle (it has an `@`), a URL (it starts with `http://`, `https://` or
/// `www.`, in any case) or a dotted word (`is_dotted`), with a letter or digit. Typed as
/// one word: never corrected, and learned whole.
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
        || is_dotted(token))
        && token.chars().any(char::is_alphanumeric)
}

/// `a.b.c`, `e.g.`, `3.14`, `googel.com`: a full stop with more typed after it. A full
/// stop ends a word only when whitespace follows it (`hello.`, `hello.)`).
fn is_dotted(token: &str) -> bool {
    without_trailing_punctuation(token).contains('.')
}

/// `word` less the punctuation, brackets and quotes typed after it (`(e.g.),` → `(e.g`).
fn without_trailing_punctuation(word: &str) -> &str {
    word.trim_end_matches(|c: char| matches!(c, '.' | ',' | ';' | ':' | '?' | '!') || is_closing(c))
}

/// An address as it is learned, without the brackets, quotes and punctuation typed
/// around it (`(priya@example.com),`); None for a word that is no address, or one without
/// a letter (`3.14`: a number is learned only as a phone number, `phone_number_at_end`).
pub fn learnable_address(word: &str) -> Option<&str> {
    let trimmed = without_trailing_punctuation(word.trim_start_matches(is_opening));
    (is_address(trimmed) && trimmed.chars().any(char::is_alphabetic)).then_some(trimmed)
}

/// Most digit groups a phone number is typed in with a space between each
/// (`+44 20 7946 0958`).
pub const MAX_NUMBER_GROUPS: usize = 4;

/// A word that may be (part of) a phone number: digits, `+`, `-`, `.`, `(` and `)`, with a
/// digit (`55501`, `+91`, `(0484)`, `55501-1`).
fn is_number_word(word: &str) -> bool {
    word.bytes().any(|b| b.is_ascii_digit())
        && word
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | '(' | ')'))
}

/// The ends of `context_before` that may be a phone number or its start, longest first: its
/// last `is_number_word`s back over single spaces, at most `MAX_NUMBER_GROUPS` (`call +91 98`
/// → `+91 98`, `98`). The space is the word break, so a number typed in groups arrives as
/// several words. An opening quote before the first is no part of it, as in `last_token`.
/// None when the text ends in whitespace.
pub fn number_tails(context_before: &str) -> impl Iterator<Item = &str> {
    let mut starts = Vec::new();
    let mut end = context_before.len();
    while starts.len() < MAX_NUMBER_GROUPS {
        let start = context_before[..end]
            .trim_end_matches(|c: char| !c.is_whitespace())
            .len();
        let token = &context_before[start..end];
        let word = token.trim_start_matches(|c| is_opening(c) && c != '(');
        if !is_number_word(word) {
            break;
        }
        starts.push(end - word.len());
        // One space before it, and a word before that.
        let before = &context_before[..start];
        let Some(rest) = before.strip_suffix(' ') else {
            break;
        };
        if word.len() != token.len() || rest.is_empty() || rest.ends_with(char::is_whitespace) {
            break;
        }
        end = rest.len();
    }
    starts.into_iter().rev().map(|s| &context_before[s..])
}

/// The phone number (`is_phone_number`) that ends `text`, less the punctuation, brackets
/// and quotes around it, over as many of its `number_tails` as make one: `call +91 55501
/// 12345.` → `+91 55501 12345`. None when the text ends in whitespace.
pub fn phone_number_at_end(text: &str) -> Option<&str> {
    number_tails(without_trailing_punctuation(text)).find_map(|tail| {
        [tail, tail.trim_start_matches('(')]
            .into_iter()
            .find(|t| is_phone_number(t))
    })
}

/// A phone number as it is learned, typed as one word or in groups with a single space
/// between them. E.164 allows at most 15 digits; with fewer than 7 it is a year, an amount
/// or a code, not a number to call.
///
/// One word: an optional `+`, an optional area code in brackets, then digits with single
/// `-` or `.` between groups (`+91-55501-12345`, `(0484)5550123`, `98.470.12345`), 7 to 15
/// digits. Bare digits need 10 (a mobile number with its prefix), as 7 digits alone are as
/// likely an amount (`1000000`). One `.` is a decimal point (`1234567.89`).
///
/// In groups (`+91 55501 12345`, `(415) 555-0123`): 2 to `MAX_NUMBER_GROUPS` words of 2 to 5
/// digits (a `+` country code first has 1 to 3, and the national number after it any), 10 to
/// 15 digits in all. A lone digit begins no number (`I have 2 kids 3 dogs`), and the total
/// keeps two ordinary numbers apart (`in 2024 2025`).
///
/// Without a `+`, what reads as another kind of number is none: thousands (`1.234.567`,
/// `123 456 789 012`), dates (`12-05-2026`, `2026.05.12`) and two short groups (`2020-2024`).
pub fn is_phone_number(text: &str) -> bool {
    let words: Vec<&str> = text.split(' ').collect();
    if words.len() > MAX_NUMBER_GROUPS {
        return false;
    }
    let plus = text.starts_with('+');
    let mut groups: Vec<&str> = Vec::new();
    for (i, word) in words.iter().enumerate() {
        let Some(found) = number_groups(word, i == 0) else {
            return false;
        };
        if words.len() > 1 {
            let short = |g: &&str| (2..=5).contains(&g.len());
            let fits = match i {
                0 => match found.groups.as_slice() {
                    [code] if plus => code.len() <= 3,
                    [group] => short(group),
                    _ => false,
                },
                _ => {
                    found.separator.is_none_or(|s| s == '-')
                        && (plus || found.groups.iter().all(short))
                }
            };
            if !fits {
                return false;
            }
        } else {
            let bare = !plus && !found.bracketed && found.separator.is_none();
            let dots = found.separator == Some('.');
            if bare && digits(&found.groups) < 10 || dots && found.groups.len() < 3 {
                return false;
            }
        }
        groups.extend(found.groups);
    }
    let range = if words.len() > 1 { 10..=15 } else { 7..=15 };
    range.contains(&digits(&groups)) && (plus || !reads_as_another_number(&groups))
}

fn digits(groups: &[&str]) -> usize {
    groups.iter().map(|g| g.len()).sum()
}

/// Thousands, a date, or two short groups (`is_phone_number`).
fn reads_as_another_number(groups: &[&str]) -> bool {
    let lens: Vec<usize> = groups.iter().map(|g| g.len()).collect();
    let thousands = lens.len() >= 2 && lens[0] <= 3 && lens[1..].iter().all(|&n| n == 3);
    let date = matches!(lens.as_slice(), [a, b, 2 | 4] if *a <= 2 && *b <= 2)
        || matches!(lens.as_slice(), [4, b, c] if *b <= 2 && *c <= 2);
    let two_short = matches!(lens.as_slice(), [a, b] if *a <= 4 && *b <= 4);
    thousands || date || two_short
}

/// The digit groups of one word of a phone number, and what separates them.
struct NumberGroups<'a> {
    groups: Vec<&'a str>,
    separator: Option<char>,
    bracketed: bool,
}

/// `word` as digit groups: a `+` and a bracketed area code may begin the first word
/// (`+1(415)555-0123`), then digits with one kind of separator, `-` or `.`, single.
fn number_groups(word: &str, first: bool) -> Option<NumberGroups<'_>> {
    let all_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let mut rest = if first {
        word.strip_prefix('+').unwrap_or(word)
    } else {
        word
    };
    let mut groups = Vec::new();
    let mut bracketed = false;
    // A country code straight before the brackets: `+1(415)`.
    if rest.len() < word.len()
        && let Some((code, _)) = rest.split_once('(')
        && (1..=3).contains(&code.len())
        && all_digits(code)
    {
        groups.push(code);
        rest = &rest[code.len()..];
    }
    if first && let Some(inner) = rest.strip_prefix('(') {
        let (code, after) = inner.split_once(')')?;
        if !all_digits(code) || code.len() > 5 {
            return None;
        }
        groups.push(code);
        bracketed = true;
        rest = after;
    }
    let separator = rest.chars().find(|&c| c == '-' || c == '.');
    if !rest.is_empty() {
        for group in rest.split(separator.unwrap_or(' ')) {
            if !all_digits(group) {
                return None;
            }
            groups.push(group);
        }
    }
    (!groups.is_empty()).then_some(NumberGroups {
        groups,
        separator,
        bracketed,
    })
}

fn is_apostrophe(c: char) -> bool {
    matches!(c, '\'' | '\u{2019}')
}

/// A grapheme belongs to a word if it starts with a letter, digit or apostrophe, and not with
/// a script's phrase end (Bengali ৷ is a numeric sign typed for a danda).
fn is_word_grapheme(g: &str) -> bool {
    g.chars().next().is_some_and(|c| {
        (c.is_alphanumeric() || is_apostrophe(c)) && !crate::script::ends_phrase(c)
    })
}

/// Which writing system a word is in. Only `Latin` and `Native` words go into a lexicon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    /// ASCII letters, optionally with apostrophes inside (don't).
    Latin,
    /// Letters and signs of one native script in use (`script::all`, `is_word_of`), by its ISO
    /// 15924 code, plus ZWJ and ZWNJ, and apostrophes inside the word where the script has them
    /// (`ScriptRules::inner_apostrophe`: Assamese হ'ল). For Malayalam: U+0D00–U+0D63 and
    /// U+0D7A–U+0D7F.
    Native(&'static str),
    /// Anything else: digits, mixed scripts, other alphabets.
    Other,
}

impl Script {
    /// Malayalam, the pack every build has had so far.
    pub const MALAYALAM: Script = Script::Native("mlym");
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
    // Any rule set in use: with two languages of one script, a word of either is in it.
    crate::script::all()
        .find(|rules| is_word_of(*rules, word))
        .map_or(Script::Other, |rules| Script::Native(rules.code()))
}

/// Whether `word` is a word as `rules` make one: its letters and signs, ZWJ and ZWNJ, and
/// apostrophes inside it where the rules have them. With two languages of one script this
/// tells whose word a text can be (Assamese হ'ল and ৰ are no Bengali words).
pub fn is_word_of(rules: &dyn crate::script::ScriptRules, word: &str) -> bool {
    let letter = |c: char| rules.is_letter(c);
    // An apostrophe inside the word, never first or last, where the script has them.
    let inner = rules.inner_apostrophe() && !word.starts_with('\'') && !word.ends_with('\'');
    word.chars()
        .all(|c| letter(c) || c == '\u{200C}' || c == '\u{200D}' || (inner && c == '\''))
        && word.chars().any(letter)
}

/// A char that can be part of a word when tokenising a corpus line. Cheaper
/// than grapheme segmentation, which matters for 500M words.
fn is_token_char(c: char) -> bool {
    (c.is_alphanumeric()
        || crate::script::is_word_char(c)
        || matches!(c, '\u{200C}' | '\u{200D}')
        || is_apostrophe(c))
        && !crate::script::ends_phrase(c)
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
    let sentence_end = trimmed.ends_with(['.', '?', '!'])
        || trimmed.ends_with(crate::script::ends_phrase)
        || context_before[trimmed.len()..].contains('\n');
    let without_punct = trimmed.trim_end_matches(|c: char| !is_token_char(c) || is_apostrophe(c));
    match current_word(without_punct) {
        "" => None,
        word => Some((word, sentence_end)),
    }
}

/// The words a phrase continues when the text ends in a space: the previous word, and the
/// one before it. Phrases never run across a sentence end (`previous_word`), so after
/// "Hi. " there is neither, and after "Hi. how " only "how". A comma doesn't end one.
pub fn phrase_words(context_before: &str) -> (Option<&str>, Option<&str>) {
    let Some((previous, false)) = previous_word(context_before) else {
        return (None, None);
    };
    // `previous` is a slice of `context_before`.
    let start = previous.as_ptr() as usize - context_before.as_ptr() as usize;
    let earlier = previous_word(&context_before[..start]).and_then(|(w, end)| (!end).then_some(w));
    (Some(previous), earlier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrase_words_stop_at_a_sentence_end() {
        assert_eq!(phrase_words("hello how "), (Some("how"), Some("hello")));
        assert_eq!(phrase_words("hello, how "), (Some("how"), Some("hello")));
        assert_eq!(phrase_words("hello "), (Some("hello"), None));
        assert_eq!(phrase_words("Hi. how "), (Some("how"), None));
        assert_eq!(phrase_words("Hi\nhow "), (Some("how"), None));
        assert_eq!(phrase_words("Hi how. "), (None, None));
        assert_eq!(phrase_words("hello\n"), (None, None));
        assert_eq!(phrase_words("hello h"), (None, None), "a word is being typed");
        assert_eq!(phrase_words(""), (None, None));
        assert_eq!(phrase_words("എന്റെ വീട് "), (Some("വീട്"), Some("എന്റെ")));
        assert_eq!(
            phrase_words("mail priya@example.com "),
            (Some("priya@example.com"), Some("mail"))
        );
    }

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
        assert_eq!(script("എന്റെ"), Script::MALAYALAM);
        assert_eq!(script("അവൻ"), Script::MALAYALAM);
        assert_eq!(script("abcഎ"), Script::Other);
        assert_eq!(script("42"), Script::Other);
        assert_eq!(script("൧൨"), Script::Other);
        assert_eq!(script("café"), Script::Other);
        assert_eq!(script("'"), Script::Other);
        assert_eq!(script(""), Script::Other);
    }

    #[test]
    fn devanagari_and_bengali_scripts() {
        assert_eq!(script("नमस्ते"), Script::Native("deva"));
        assert_eq!(script("ज\u{093C}िंदगी"), Script::Native("deva"));
        assert_eq!(script("नमस्ते।"), Script::Other, "a danda is no letter");
        assert_eq!(script("२०२०"), Script::Other);
        assert_eq!(script("क'"), Script::Other, "Devanagari has no apostrophe words");
        assert_eq!(script("অসমীয\u{09BC}া"), Script::Native("beng"));
        assert_eq!(script("হ'ল"), Script::Native("beng"));
        assert_eq!(script("'হল"), Script::Other);
        assert_eq!(script("হল'"), Script::Other);
        assert_eq!(script("০৯"), Script::Other);
        assert_eq!(script("नमস"), Script::Other, "two scripts");
        assert_eq!(script("എന്റെ"), Script::MALAYALAM);
    }

    #[test]
    fn devanagari_and_bengali_words_stay_whole() {
        assert_eq!(current_word("मैं समझ"), "समझ");
        assert_eq!(current_word("क्\u{200D}ष"), "क्\u{200D}ष");
        assert_eq!(current_word("ज\u{093C}"), "ज\u{093C}");
        assert_eq!(current_word("नमस्ते।"), "");
        assert_eq!(current_word("মই হ'ল"), "হ'ল");
        assert_eq!(current_word("মই ৷"), "");
        let got: Vec<&str> = words("क्या हाल है। ठीक॥ ক্ষমা হ'ল৷ মই").collect();
        assert_eq!(got, ["क्या", "हाल", "है", "ठीक", "ক্ষমা", "হ'ল", "মই"]);
        assert_eq!(previous_word("नमस्ते। "), Some(("नमस्ते", true)));
        assert_eq!(previous_word("हाल है "), Some(("है", false)));
        assert_eq!(previous_word("মই হ'ল৷ "), Some(("হ'ল", true)));
        assert_eq!(phrase_words("क्या। हाल "), (Some("हाल"), None));
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
    fn a_full_stop_with_more_typed_after_it_is_inside_the_word() {
        assert_eq!(current_word("x.com"), "x.com");
        assert_eq!(current_word("see example.com/blog"), "example.com/blog");
        assert_eq!(current_word("end.it"), "end.it");
        assert_eq!(current_word("googel.c"), "googel.c");
        assert_eq!(current_word("so a.b.c"), "a.b.c");
        assert_eq!(current_word("(e.g"), "e.g");
        assert_eq!(current_word("pi 3.14"), "3.14");
        assert_eq!(current_word("e.g.,"), "e.g.,");
    }

    #[test]
    fn a_full_stop_before_whitespace_still_ends_a_word() {
        assert_eq!(current_word("hello."), "");
        assert_eq!(current_word("hello..."), "");
        assert_eq!(current_word("(hello.)"), "");
        assert_eq!(current_word("Hello. Wor"), "Wor");
        assert_eq!(current_word("well-known"), "known");
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
        assert!(is_address("end.it"));
        assert!(is_address("a..com"));
        assert!(is_address("e.g."));
        assert!(!is_address("hello."));
        assert!(!is_address("hello.\u{201D}"));
        assert!(!is_address("..."));
        assert!(!is_address("hello"));
    }

    #[test]
    fn dotted_words_are_learned_without_the_punctuation_after_them() {
        assert_eq!(learnable_address("(e.g.),"), Some("e.g"));
        assert_eq!(learnable_address("a.b.c"), Some("a.b.c"));
        assert_eq!(learnable_address("hello."), None);
        assert_eq!(learnable_address("3.14"), None, "numbers are never learned");
    }

    #[test]
    fn previous_word_after_a_dotted_word() {
        assert_eq!(previous_word("so a.b.c "), Some(("a.b.c", false)));
        assert_eq!(previous_word("see e.g. "), Some(("e.g", true)));
        assert_eq!(previous_word("teh. "), Some(("teh", true)));
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

    #[test]
    fn phone_numbers() {
        for number in [
            "5550112345",
            "+915550112345",
            "55501-12345",
            "+91-55501-12345",
            "98.470.12345",
            "(0484)5550123",
            "+1(415)555-0123",
            "1-800-555-0199",
            "55501 12345",
            "+91 55501 12345",
            "+91 5550112345",
            "+44 20 7946 0958",
            "020 7946 0958",
            "(415) 555-0123",
            "+1 415-555-0123",
        ] {
            assert!(is_phone_number(number), "{number}");
        }
    }

    #[test]
    fn other_numbers_are_no_phone_numbers() {
        for number in [
            "",
            "100",
            "2024",
            "100000",
            "1000000", // a bare number needs the 10 digits of a mobile
            "3.14",
            "1234567.89", // one full stop is a decimal point
            "1.234.567",  // thousands
            "123 456 789 012",
            "1,000,000",
            "12/05/2026",
            "12.05.2026", // dates
            "12-05-2026",
            "2026-05-12",
            "2020-2024", // a range: two short groups
            "555-0123",
            "10:30",
            "$5550112345",
            "5550112345%",
            "+91",
            "+91 98",
            "55501  12345",
            " 55501 12345",
            "55501 12345 ",
            "2 5550112345", // a group of one digit begins no spaced number
            "0484 5550123", // spaced groups are short, but after a country code
            "55501 12345 67890 12345 1",
            "5550112345678901", // past E.164's 15
            "++915550112345",
            "55501--12345",
            "55501-12345.",
            "-5550112345",
            "55501-123.45",
            "()5550112345",
            "(55501)",
            "55501 +12345",
        ] {
            assert!(!is_phone_number(number), "{number}");
        }
    }

    #[test]
    fn the_phone_number_at_the_end_runs_back_over_its_groups() {
        assert_eq!(phone_number_at_end("call 55501 12345"), Some("55501 12345"));
        assert_eq!(
            phone_number_at_end("call +91 55501 12345."),
            Some("+91 55501 12345")
        );
        assert_eq!(phone_number_at_end("5550112345"), Some("5550112345"));
        assert_eq!(phone_number_at_end("(55501-12345),"), Some("55501-12345"));
        assert_eq!(phone_number_at_end("\"55501 12345\""), Some("55501 12345"));
        assert_eq!(phone_number_at_end("(0484)5550123"), Some("(0484)5550123"));
        assert_eq!(phone_number_at_end("room 2 5550112345"), Some("5550112345"));
        assert_eq!(phone_number_at_end("call +91 55501"), None);
        assert_eq!(phone_number_at_end("I have 2 kids 3 dogs"), None);
        assert_eq!(phone_number_at_end("in 2024 2025"), None);
        assert_eq!(phone_number_at_end("call 55501\n12345"), None);
        assert_eq!(phone_number_at_end("call 55501  12345"), None);
        assert_eq!(phone_number_at_end("pay $5550112345"), None);
        assert_eq!(phone_number_at_end("call 55501 12345 "), None);
    }

    #[test]
    fn number_tails_are_longest_first() {
        let tails: Vec<&str> = number_tails("call +91 98").collect();
        assert_eq!(tails, ["+91 98", "98"]);
        let tails: Vec<&str> = number_tails("a 1 2 3 4 5").collect();
        assert_eq!(tails, ["2 3 4 5", "3 4 5", "4 5", "5"], "four groups at most");
        let tails: Vec<&str> = number_tails("x  55501-1").collect();
        assert_eq!(tails, ["55501-1"]);
        let tails: Vec<&str> = number_tails("(0484) 23").collect();
        assert_eq!(tails, ["(0484) 23", "23"]);
        assert_eq!(number_tails("hi 98a").count(), 0);
        assert_eq!(number_tails("98 ").count(), 0);
        let tails: Vec<&str> = number_tails("\"98 76").collect();
        assert_eq!(tails, ["98 76", "76"], "an opening quote is no part of it");
        assert_eq!(number_tails("+").count(), 0);
        assert_eq!(number_tails("").count(), 0);
    }

    proptest::proptest! {
        #[test]
        fn current_word_is_always_a_suffix(s in "\\PC{0,30}") {
            proptest::prop_assert!(s.ends_with(current_word(&s)));
        }

        #[test]
        fn number_tails_are_suffixes(s in "[0-9+() .a-]{0,30}") {
            for tail in number_tails(&s) {
                proptest::prop_assert!(s.ends_with(tail));
            }
        }
    }
}

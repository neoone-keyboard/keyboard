//! The keyboard turns Shift on by itself at the start of the text and of each sentence, so
//! a word typed there arrives capitalised (`Shukriya`) whatever language it is in. That
//! capital says nothing about the language: with a pack beside English, the bar for a
//! capitalised sentence start is the bar for the same letters in lowercase. A capital the
//! user typed (mid-sentence, a run of capitals) is still a hint of English or a name.

use std::path::{Path, PathBuf};

use mlengine_core::pack::Pack;
use mlengine_core::{Engine, Layout, Paths};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// English and the pack `id`, as the keyboard opens them.
fn engine(id: &str) -> Engine {
    let pack = Pack::open(&root().join(format!("packs/{id}.neopack"))).unwrap();
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    Engine::open(&Paths::with_packs(Some(en), &[pack], None))
}

/// The bar's words, Latin ones lowercased (a sentence start shows English capitalised).
fn bar(e: &Engine, context: &str) -> Vec<String> {
    let s = e.suggest(context, Layout::Latin, true);
    let words: Vec<String> = s.candidates.iter().map(|c| c.text.to_lowercase()).collect();
    eprintln!("{context:?}: {words:?}");
    words
}

/// Each word, capitalised at the text start and after a full stop, gets the lowercase bar.
fn capital_is_ignored_at_sentence_starts(id: &str, words: &[&str]) {
    let e = engine(id);
    for word in words {
        let capitalised = {
            let mut c = word.chars();
            let first = c.next().unwrap().to_ascii_uppercase();
            format!("{first}{}", c.as_str())
        };
        for before in ["", "Ok. "] {
            assert_eq!(
                bar(&e, &format!("{before}{capitalised}")),
                bar(&e, &format!("{before}{word}")),
                "{id}: {before:?}{capitalised} against {word}"
            );
        }
    }
}

#[test]
fn hindi_sentence_start_capitals_are_ignored() {
    capital_is_ignored_at_sentence_starts("hi", &["shukriya", "mera", "kya", "aap", "kaise"]);
}

#[test]
fn urdu_sentence_start_capitals_are_ignored() {
    capital_is_ignored_at_sentence_starts("ur", &["shukriya", "mera", "kya", "aap", "kaise"]);
}

/// The first word of a message has no word before it to say which language it is in. With a
/// pack beside English, a common word of the pack that the letters spell (and that is not an
/// English word) leads there as it does after a sentence end: neither the typed letters nor a
/// weak English correction (`near` for `mera`) keeps the middle.
#[test]
fn the_first_word_of_a_message_is_the_packs() {
    for (id, words) in [
        ("ur", &[("mera", "میرا"), ("kya", "کیا"), ("kaise", "کیسے")][..]),
        ("hi", &[("mera", "मेरा"), ("kya", "क्या"), ("kaise", "कैसे")][..]),
    ] {
        let e = engine(id);
        for (typed, word) in words {
            for context in [typed.to_string(), format!("Ok. {typed}")] {
                assert_eq!(bar(&e, &context)[0], *word, "{id}: {context:?}");
            }
        }
    }
}

/// English words typed at the start of a message stay English.
#[test]
fn english_words_at_the_start_stay_english() {
    for id in ["ur", "hi"] {
        let e = engine(id);
        for word in ["hello", "the", "main", "Hello", "Thanks", "thanks"] {
            assert_eq!(bar(&e, word)[0], word.to_lowercase(), "{id}: {word}");
        }
    }
}

#[test]
fn urdu_shukriya_at_a_sentence_start_is_urdu() {
    let e = engine("ur");
    for context in ["Shukriya", "Ok. Shukriya", "ٹھیک ہے۔ Shukriya"] {
        assert_eq!(bar(&e, context)[0], "شکریہ", "{context}");
    }
}

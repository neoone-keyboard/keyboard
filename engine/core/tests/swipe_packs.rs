//! Swipe typing on the Hindi, Assamese and Tamil packs: a path through the keys of a common
//! romanised word gives the native word, drawn through the skeleton keys of the pack's own
//! `romanisation.json`. Paths have no wobble here; `swipe_eval_packs` (`make eval-swipe`)
//! measures noisy ones.

use std::path::{Path, PathBuf};

use mlengine_core::lang::LangId;
use mlengine_core::pack::Pack;
use mlengine_core::swipe::{KeyMap, ideal_path, qwerty};
use mlengine_core::{Engine, Kind, Paths};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// English and the pack `id`, without its transliterator (a swipe never asks it).
fn engine(id: &str) -> Engine {
    let pack = Pack::open(&root().join(format!("packs/{id}.neopack"))).unwrap();
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    let mut paths = Paths::with_packs(Some(en), &[pack], None);
    paths.packs[0].translit = None;
    Engine::open(&paths)
}

/// What a swipe through `drawn`'s keys gives: `(text, kind, language id)`, best first.
fn swipe(engine: &Engine, drawn: &str) -> Vec<(String, Kind, String)> {
    let keys = qwerty();
    let path = ideal_path(&KeyMap::new(&keys).unwrap(), drawn, 4.0);
    engine
        .swipe(&path, &keys, "", None)
        .into_iter()
        .map(|c| {
            let lang = engine.lang_id(c.lang).map_or("?".into(), |l| l.0);
            (c.text, c.kind, lang)
        })
        .collect()
}

/// Each `(drawn, word)`: the swipe's candidates hold `word` as a romanised word of pack `id`.
fn assert_native(id: &str, words: &[(&str, &str)]) {
    let engine = engine(id);
    for (drawn, word) in words {
        let found = swipe(&engine, drawn);
        assert!(
            found
                .iter()
                .any(|(text, kind, lang)| text == word && *kind == Kind::Romanised && lang == id),
            "{id} {drawn}: no {word} in {found:?}"
        );
    }
}

/// The same with English switched off: the word is in the first three, and nothing is English.
fn assert_native_alone(id: &str, words: &[(&str, &str)]) {
    let mut engine = engine(id);
    engine.set_active(&[LangId(id.into())]).unwrap();
    for (drawn, word) in words {
        let found = swipe(&engine, drawn);
        assert!(
            found.iter().all(|(_, kind, lang)| *kind == Kind::Romanised && lang == id),
            "{id} {drawn}: {found:?}"
        );
        assert!(
            found.iter().take(3).any(|(text, _, _)| text == word),
            "{id} {drawn}: no {word} in the first three of {found:?}"
        );
    }
}

/// Hindi words whose skeleton keeps a letter for every key drawn (no `a` between consonants).
const HINDI: &[(&str, &str)] = &[
    ("hindi", "हिंदी"),
    ("dost", "दोस्त"),
    ("dilli", "दिल्ली"),
    ("aur", "और"),
];

/// Hindi words spelt with an `a` between consonants, which Hindi's skeleton drops
/// (`dropBetweenConsonants`): namaste is `nmste`, sarkar `srkr`, ghar `gr`.
const HINDI_INHERENT_A: &[(&str, &str)] = &[
    ("namaste", "नमस्ते"),
    ("sarkar", "सरकार"),
    ("nahin", "नहीं"),
    ("pyar", "प्यार"),
    ("ghar", "घर"),
    ("dhanyavad", "धन्यवाद"),
    ("karne", "करने"),
    ("kab", "कब"),
    ("samajh", "समझ"),
    ("matlab", "मतलब"),
];

const ASSAMESE: &[(&str, &str)] = &[
    ("tumi", "তুমি"),
    ("manuh", "মানুহ"),
    ("bhal", "ভাল"),
    ("guwahati", "গুৱাহাটী"),
];

const TAMIL: &[(&str, &str)] = &[
    ("vanakkam", "வணக்கம்"),
    ("nandri", "நன்றி"),
    ("tamil", "தமிழ்"),
    ("amma", "அம்மா"),
];

#[test]
fn a_hindi_swipe_gives_the_hindi_word() {
    assert_native("hi", HINDI);
}

/// The finger passes through `a` where the skeleton has no letter: the swipe rules built from
/// the pack's tables (`romanisation::swipe_rules`) let a path draw a letter of
/// `dropBetweenConsonants` between two consonants of a skeleton.
#[test]
fn a_hindi_swipe_through_a_dropped_a_gives_the_hindi_word() {
    assert_native("hi", HINDI_INHERENT_A);
}

#[test]
fn an_assamese_swipe_gives_the_assamese_word() {
    assert_native("as", ASSAMESE);
}

#[test]
fn a_tamil_swipe_gives_the_tamil_word() {
    assert_native("ta", TAMIL);
}

#[test]
fn with_english_off_a_pack_swipe_gives_only_its_words() {
    assert_native_alone("hi", HINDI);
    assert_native_alone("hi", HINDI_INHERENT_A);
    assert_native_alone("as", ASSAMESE);
    assert_native_alone("ta", TAMIL);
}

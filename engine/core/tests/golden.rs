//! Golden behaviour for ranking, on a small lexicon with realistic counts
//! (per billion words). If a weight in `rank.rs` changes, these must still pass.

use mlengine_core::lexicon::{Lexicon, build};
use mlengine_core::user::UserStore;
use mlengine_core::{Engine, Kind, Layout, Sources};

#[rustfmt::skip]
const ENGLISH: &[(&str, u64)] = &[
    ("the", 50_000_000), ("of", 30_000_000), ("and", 25_000_000), ("to", 24_000_000),
    ("a", 20_000_000), ("in", 18_000_000), ("is", 10_000_000), ("I", 8_000_000),
    ("that", 9_000_000), ("for", 7_000_000), ("it", 7_000_000), ("was", 6_000_000),
    ("on", 6_000_000), ("with", 5_000_000), ("he", 5_000_000), ("as", 5_000_000),
    ("them", 1_500_000), ("then", 1_400_000), ("there", 2_000_000), ("their", 2_200_000),
    ("they", 3_000_000), ("ten", 300_000), ("tea", 80_000), ("thee", 40_000),
    ("help", 300_000), ("hello", 30_000), ("hell", 60_000), ("jello", 1_000),
    ("helped", 90_000), ("helping", 70_000), ("London", 400_000), ("don't", 900_000),
    ("enter", 120_000), ("entry", 100_000), ("Tom", 100_000), ("an", 4_000_000),
    ("ant", 20_000), ("any", 1_000_000), ("am", 900_000), ("have", 4_000_000),
    ("could", 1_500_000), ("us", 900_000), ("US", 200_000), ("will", 2_000_000),
    ("Will", 50_000), ("ENT", 3_000),
];

fn english() -> Option<Lexicon> {
    let bytes = build(ENGLISH.iter().map(|(w, c)| (w.as_bytes().to_vec(), *c))).unwrap();
    Some(Lexicon::from_bytes(bytes).unwrap())
}

fn engine() -> Engine {
    Engine::new(
        Sources {
            english: english(),
            ..Sources::default()
        },
        UserStore::in_memory(),
    )
}

fn middle(engine: &Engine, context: &str) -> (String, Kind) {
    let s = engine.suggest(context, Layout::Latin, true);
    let c = &s.candidates[0];
    (c.text.clone(), c.kind)
}

#[test]
fn teh_becomes_the() {
    assert_eq!(middle(&engine(), "teh"), ("the".into(), Kind::Correction));
}

#[test]
fn hello_stays_hello() {
    assert_eq!(middle(&engine(), "hello"), ("hello".into(), Kind::Verbatim));
    assert_eq!(middle(&engine(), "Hello"), ("Hello".into(), Kind::Verbatim));
}

#[test]
fn common_typos() {
    let engine = engine();
    assert_eq!(middle(&engine, "dont").0, "don't");
    assert_eq!(middle(&engine, "thw").0, "the");
    assert_eq!(middle(&engine, "helllo").0, "hello");
    assert_eq!(middle(&engine, "Teh").0, "The");
    assert_eq!(middle(&engine, "i").0, "I");
}

#[test]
fn real_words_are_left_alone() {
    let engine = engine();
    for word in [
        "the", "them", "then", "ten", "tea", "hell", "help", "an", "any", "am", "enter",
    ] {
        assert_eq!(
            middle(&engine, word),
            (word.to_string(), Kind::Verbatim),
            "{word}"
        );
    }
}

#[test]
fn a_learned_name_stays() {
    let engine = engine();
    assert_eq!(
        middle(&engine, "Anu").0,
        "And",
        "before learning, Anu looks like a typo"
    );
    engine.learn("Anu");
    assert_eq!(middle(&engine, "Anu"), ("Anu".into(), Kind::Verbatim));
    assert_eq!(middle(&engine, "anu").0, "anu");
}

fn texts(engine: &Engine, context: &str) -> Vec<String> {
    let s = engine.suggest(context, Layout::Latin, true);
    s.candidates.into_iter().map(|c| c.text).collect()
}

#[test]
fn space_never_commits_an_unfinished_word() {
    let engine = engine();
    assert_eq!(middle(&engine, "cou"), ("cou".into(), Kind::Verbatim));
    assert!(texts(&engine, "cou").contains(&"could".to_string()));
    assert_eq!(middle(&engine, "ent"), ("ent".into(), Kind::Verbatim));
    assert!(texts(&engine, "ent").contains(&"enter".to_string()));
}

#[test]
fn curly_apostrophe_gets_suggestions() {
    let engine = engine();
    let s = engine.suggest("I don\u{2019}", Layout::Latin, true);
    assert_eq!(
        s.candidates[0].text, "don\u{2019}",
        "the typed word is kept as typed"
    );
    assert!(texts(&engine, "I don\u{2019}").contains(&"don't".to_string()));
    assert_eq!(
        middle(&engine, "don\u{2019}t"),
        ("don\u{2019}t".into(), Kind::Verbatim)
    );
}

#[test]
fn a_known_word_shows_no_other_case_of_itself() {
    let engine = engine();
    assert!(!texts(&engine, "us").contains(&"US".to_string()));
    assert!(!texts(&engine, "will").contains(&"Will".to_string()));
    assert_eq!(middle(&engine, "us"), ("us".into(), Kind::Verbatim));
}

#[test]
fn a_case_variant_is_the_best_correction() {
    assert_eq!(middle(&engine(), "tom"), ("Tom".into(), Kind::Correction));
}

#[test]
fn a_case_variant_does_not_replace_an_unfinished_word() {
    // ENT is a rarer word than the completions of "ent", so space keeps "ent".
    assert_eq!(middle(&engine(), "ent"), ("ent".into(), Kind::Verbatim));
}

#[test]
fn sentence_starts_are_capitalised() {
    let engine = engine();
    assert_eq!(
        middle(&engine, "It rains. teh"),
        ("The".into(), Kind::Correction)
    );
    assert!(texts(&engine, "It rains. hel").contains(&"Help".to_string()));
    assert_eq!(middle(&engine, "it rains teh").0, "the");
}

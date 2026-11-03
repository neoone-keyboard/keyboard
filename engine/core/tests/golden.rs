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
    ("Will", 50_000), ("ENT", 3_000), ("fish", 200_000), ("one", 3_000_000),
    // Contractions, and bare forms counted from text that dropped the apostrophe.
    ("what", 2_000_000), ("what's", 300_000), ("whats", 3_000), ("I'm", 1_000_000),
    ("im", 200_000), ("it's", 2_000_000), ("its", 1_500_000), ("let's", 200_000),
    ("lets", 150_000), ("well", 1_000_000), ("we'll", 100_000), ("he'll", 30_000),
    ("they're", 300_000),
    // Long words and their neighbours.
    ("preference", 20_000), ("preferences", 8_000), ("definitely", 50_000),
    ("also", 1_500_000), ("apostle", 5_000), ("apostrophe", 2_000), ("apostrophes", 500),
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

/// A contraction typed without its apostrophe becomes the contraction, even where the
/// bare form is in the lexicon (`whats`, `im`), and even against a commoner word one
/// letter away (`theyre` → they're, not there). Real words spelled like one stay, with
/// the contraction offered beside them (`rank::CONTRACTION_LOOKALIKES`).
#[test]
fn contractions_typed_without_the_apostrophe() {
    let engine = engine();
    for (typed, want) in [("whats", "what's"), ("im", "I'm"), ("theyre", "they're")] {
        assert_eq!(
            middle(&engine, typed),
            (want.to_string(), Kind::Correction),
            "{typed}: {:?}",
            texts(&engine, typed)
        );
    }
    for (typed, side) in [
        ("its", "it's"),
        ("lets", "let's"),
        ("well", "we'll"),
        ("hell", "he'll"),
    ] {
        let bar = texts(&engine, typed);
        assert_eq!(bar[0], typed, "{typed}: {bar:?}");
        assert_eq!(bar[1], side, "{typed}: {bar:?}");
    }
}

/// Long words: a near match with a few letters to go takes the middle when it
/// dominates (`rank::COMPLETION_DOMINANT`), and one vowel for another is a cheap slip.
#[test]
fn long_words_are_completed_and_corrected() {
    let engine = engine();
    for (typed, want) in [
        ("preferenc", "preference"),
        ("prefarance", "preference"),
        ("definately", "definitely"),
        ("apostrap", "apostrophe"),
    ] {
        assert_eq!(
            middle(&engine, typed),
            (want.to_string(), Kind::Correction),
            "{typed}: {:?}",
            texts(&engine, typed)
        );
    }
}

/// Four letters that begin words are not corrected two slips away (`apos` is not
/// `also`); what they begin is offered instead.
#[test]
fn a_long_prefix_is_not_corrected_into_a_short_word() {
    let engine = engine();
    let bar = texts(&engine, "apos");
    assert_eq!(bar[0], "apos", "{bar:?}");
    assert_eq!(bar[1], "apostle", "{bar:?}");
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

#[test]
fn capitals_typed_on_purpose_are_respected() {
    let engine = engine();
    // Lowercase, these are typos.
    assert_eq!(middle(&engine, "home for vish").0, "fish");
    assert_eq!(middle(&engine, "check the onr").0, "one");
    // A capital mid-sentence is a name (`Vishu`), corrected only to a name with its
    // first letter; all capitals is an abbreviation (`PNR`), never replaced.
    assert_eq!(
        middle(&engine, "home for Vish"),
        ("Vish".into(), Kind::Verbatim)
    );
    assert!(texts(&engine, "home for Vish").contains(&"Fish".to_string()));
    assert_eq!(
        middle(&engine, "check the ONR"),
        ("ONR".into(), Kind::Verbatim)
    );
    assert_eq!(middle(&engine, "ONR"), ("ONR".into(), Kind::Verbatim));
    assert_eq!(middle(&engine, "we went to Lodnon").0, "London");
    // The first word of a sentence is capitalised by the keyboard: no evidence.
    assert_eq!(middle(&engine, "Teh").0, "The");
    assert_eq!(middle(&engine, "It rains. Teh").0, "The");
}

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
    // Words that begin like a learned email (`sam.2024@example.com`).
    ("new", 2_000_000), ("need", 900_000), ("never", 800_000), ("next", 700_000),
    ("neo", 5_000), ("neon", 10_000), ("neoliberal", 2_000),
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

/// An engine whose learned and removed words live in `dir`, as on the phone.
fn engine_in(dir: &std::path::Path) -> Engine {
    Engine::new(
        Sources {
            english: english(),
            ..Sources::default()
        },
        UserStore::open(dir),
    )
}

#[test]
fn a_removed_correction_gives_way_to_the_next_best() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    assert_eq!(middle(&engine, "teh"), ("the".into(), Kind::Correction));
    engine.remove_word("the");
    let bar = texts(&engine, "teh");
    assert!(!bar.contains(&"the".to_string()), "{bar:?}");
    assert_eq!(middle(&engine, "teh"), ("ten".into(), Kind::Correction));
    assert!(
        !texts(&engine, "Teh").contains(&"The".to_string()),
        "removals ignore case"
    );
    // What the user types is still shown as typed.
    assert_eq!(middle(&engine, "the"), ("the".into(), Kind::Verbatim));
    drop(engine);
    let engine = engine_in(dir.path());
    assert_eq!(middle(&engine, "teh").0, "ten", "the removal was kept");
    // Learning it again (the user tapped the quoted word) brings it back.
    engine.learn("the");
    assert_eq!(middle(&engine, "teh"), ("the".into(), Kind::Correction));
}

#[test]
fn a_removed_completion_gives_way_to_another() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    let before = texts(&engine, "hel");
    assert!(before.contains(&"help".to_string()), "{before:?}");
    engine.remove_word("help");
    let after = texts(&engine, "hel");
    assert!(!after.contains(&"help".to_string()), "{after:?}");
    assert_eq!(
        after.len(),
        before.len(),
        "another word took the slot: {after:?}"
    );
    engine.forget_learned();
    assert!(texts(&engine, "hel").contains(&"help".to_string()));
}

#[test]
fn a_removed_learned_word_is_forgotten() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    engine.learn("Anu");
    engine.remove_word("Anu");
    assert_eq!(middle(&engine, "Anu").0, "And", "no longer learned");
    engine.note_committed("Anu");
    engine.note_committed("Anu");
    assert_eq!(
        middle(&engine, "Anu").0,
        "And",
        "typing it does not bring it back while autocorrect replaces it"
    );
    assert!(!texts(&engine, "An").contains(&"Anu".to_string()));
}

#[test]
fn an_email_committed_twice_is_learned_whole_and_completed() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    engine.note_committed("priya@example.com");
    assert!(!texts(&engine, "jis").contains(&"priya@example.com".to_string()));
    engine.note_committed("priya@example.com");
    drop(engine);
    let engine = engine_in(dir.path());
    for typed in [
        "jis",
        "jishnu",
        "jishnu@",
        "mail me at priya@e",
        "It rains. jis",
    ] {
        let s = engine.suggest(typed, Layout::Latin, true);
        if typed.contains('@') {
            assert_eq!(s.candidates[0].kind, Kind::Verbatim, "{typed}: {s:?}");
        }
        assert!(
            s.candidates
                .iter()
                .any(|c| c.text == "priya@example.com" && c.kind == Kind::Completion),
            "{typed}: {s:?}"
        );
    }
    // Shown as learned: an address is not recased after a capital or at a sentence start.
    assert!(texts(&engine, "Jishnu@").contains(&"priya@example.com".to_string()));
}

#[test]
fn punctuation_around_an_address_is_not_learned() {
    let engine = engine();
    engine.note_committed("priya@example.com.");
    engine.note_committed("priya@example.com,");
    assert!(texts(&engine, "jis").contains(&"priya@example.com".to_string()));
    engine.learn("(www.example.com)");
    assert!(texts(&engine, "www.ex").contains(&"www.example.com".to_string()));
}

#[test]
fn an_address_is_never_corrected() {
    let engine = engine();
    for typed in [
        "priya@exmaple.com",
        "teh@teh",
        "https://teh",
        "www.teh",
        "me@hellp.",
    ] {
        let s = engine.suggest(typed, Layout::Latin, true);
        assert_eq!(s.typed, typed);
        let bar: Vec<(&str, Kind)> = s
            .candidates
            .iter()
            .map(|c| (c.text.as_str(), c.kind))
            .collect();
        assert_eq!(bar, [(typed, Kind::Verbatim)], "only what was typed");
    }
}

#[test]
fn a_full_stop_still_ends_an_ordinary_word() {
    let engine = engine();
    assert_eq!(engine.suggest("hello.", Layout::Latin, true).typed, "");
    assert_eq!(
        middle(&engine, "hello. teh"),
        ("The".into(), Kind::Correction)
    );
}

#[test]
fn a_learned_email_is_offered_from_its_first_letters() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    engine.note_committed("sam.2024@example.con");
    engine.note_committed("sam.2024@example.con");
    engine.note_committed("sam.2024@example.com");
    engine.note_committed("sam.2024@example.com");
    for typed in [
        "n",
        "ne",
        "neo",
        "Neo",
        "neo.",
        "neo.9",
        "sam.2024@g",
        "hi neo.98",
        "It rains. Neo.9",
    ] {
        let s = engine.suggest(typed, Layout::Latin, true);
        assert!(
            s.candidates
                .iter()
                .any(|c| c.text == "sam.2024@example.com" && c.kind == Kind::Completion),
            "{typed}: {s:?}"
        );
    }
    // What was typed so far is one word: the full stop in it is no word break.
    assert_eq!(
        engine.suggest("hi neo.9", Layout::Latin, true).typed,
        "neo.9"
    );
    assert_eq!(engine.suggest("Neo.", Layout::Latin, true).typed, "Neo.");
    // It is offered, never put in for what was typed.
    for typed in ["neo", "neo.", "neo.9"] {
        assert_eq!(middle(&engine, typed), (typed.into(), Kind::Verbatim));
    }
    // Letters it doesn't begin are ordinary words again.
    assert_eq!(engine.suggest("neon.", Layout::Latin, true).typed, "");
    assert_eq!(engine.suggest("n.", Layout::Latin, true).typed, "");
    assert_eq!(
        engine.learned_addresses(),
        ["sam.2024@example.com", "sam.2024@example.con"]
    );
}

#[test]
fn without_a_learned_address_punctuation_ends_a_word() {
    let engine = engine();
    assert_eq!(engine.suggest("neo.", Layout::Latin, true).typed, "");
    assert_eq!(engine.suggest("x.com", Layout::Latin, true).typed, "x.com");
    assert!(engine.learned_addresses().is_empty());
}

#[test]
fn a_full_stop_with_more_typed_after_it_is_inside_the_word() {
    let engine = engine();
    for typed in ["a.b.c", "e.g", "googel.com", "end.it", "teh.teh"] {
        let s = engine.suggest(&format!("so {typed}"), Layout::Latin, true);
        assert_eq!(s.typed, typed);
        assert_eq!(middle(&engine, typed), (typed.into(), Kind::Verbatim));
    }
}

#[test]
fn a_full_stop_before_whitespace_ends_the_sentence() {
    let engine = engine();
    let s = engine.suggest("teh.", Layout::Latin, true);
    assert_eq!(s.typed, "");
    assert!(
        s.candidates.iter().all(|c| c.kind == Kind::NextWord),
        "{s:?}"
    );
    assert_eq!(middle(&engine, "hello. teh").0, "The");
}

#[test]
fn a_dotted_word_committed_twice_is_learned_and_completed() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine_in(dir.path());
    engine.note_committed("a.b.c.");
    assert!(!texts(&engine, "a.b").contains(&"a.b.c".to_string()));
    engine.note_committed("a.b.c");
    for typed in ["a.", "a.b", "so a.b"] {
        assert!(
            texts(&engine, typed).contains(&"a.b.c".to_string()),
            "{typed}"
        );
    }
    assert_eq!(engine.learned_addresses(), ["a.b.c"]);
}

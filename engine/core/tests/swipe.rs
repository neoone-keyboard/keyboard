//! Swipe typing on the real, committed lexicon bundle and LM (`make lexicons`, `make models`).
//! Paths are drawn without wobble here; `swipe_eval` measures noisy ones.

use std::path::PathBuf;
use std::time::Instant;

use mlengine_core::swipe::{KeyMap, ideal_path, qwerty};
use mlengine_core::{Engine, Kind, Paths};

fn bundle() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/ios/KeyboardExtension")
}

fn engine() -> Engine {
    let dir = bundle();
    let pack = mlengine_core::pack::Pack::open(&dir.join("../../../packs/ml.neopack")).unwrap();
    Engine::open(&{
        let mut paths = Paths::with_packs(Some(dir.join("Lexicon/en.fst")), &[pack], None);
        paths.packs[0].translit = None;
        paths
    })
}

fn swipe(engine: &Engine, word: &str, before: &str) -> Vec<(String, Kind)> {
    let keys = qwerty();
    let path = ideal_path(&KeyMap::new(&keys).unwrap(), word, 4.0);
    engine
        .swipe(&path, &keys, before, None)
        .into_iter()
        .map(|c| (c.text, c.kind))
        .collect()
}

#[test]
fn english_words_come_first() {
    let engine = engine();
    for (word, before) in [
        ("the", "I saw "),
        ("hello", ""),
        ("because", "I came "),
        ("going", "I am "),
    ] {
        let found = swipe(&engine, word, before);
        assert_eq!(found[0].0, word, "{word}: {found:?}");
    }
}

/// With English switched off, a swipe gives only Malayalam words.
#[test]
fn english_switched_off_swipes_no_english() {
    let mut engine = engine();
    engine
        .set_active(&[mlengine_core::lang::LangId("ml".into())])
        .unwrap();
    for (drawn, before) in [("hello", ""), ("ningal", "ഇത് "), ("the", "I saw ")] {
        let found = swipe(&engine, drawn, before);
        assert!(!found.is_empty(), "{drawn}");
        assert!(
            found.iter().all(|(_, k)| *k == Kind::Romanised),
            "{drawn}: {found:?}"
        );
    }
}

#[test]
fn manglish_skeletons_give_malayalam_words() {
    let engine = engine();
    // Drawn as people spell them: ningal for നിങ്ങൾ, enthanu for എന്താണ് (skeleton entan,
    // the h and the final u drawn but not in it).
    for (drawn, word, before) in [("ningal", "നിങ്ങൾ", "ഇത് "), ("enthanu", "എന്താണ്", "ഇത് ")]
    {
        let found = swipe(&engine, drawn, before);
        assert!(
            found
                .iter()
                .take(3)
                .any(|(w, k)| w == word && *k == Kind::Romanised),
            "{drawn}: {found:?}"
        );
    }
}

/// A word the user taught the keyboard is in no lexicon the beam walks, so it is fitted to
/// the path on its own (`swipe::fit`): offered for a swipe through its letters, in the case
/// it was learned in, and for no other swipe.
#[test]
fn a_learned_word_can_be_swiped() {
    let engine = engine();
    let has = |found: &[(String, Kind)], word: &str| found.iter().any(|(w, _)| w == word);
    assert!(!has(&swipe(&engine, "thomasina", ""), "Thomasina"));
    engine.learn("Thomasina");
    engine.learn("Jo-Anne");
    let found = swipe(&engine, "thomasina", "");
    assert!(
        found
            .iter()
            .any(|(w, k)| w == "Thomasina" && *k == Kind::Completion),
        "{found:?}"
    );
    // A hyphen is in the word but not drawn.
    assert!(has(&swipe(&engine, "joanne", ""), "Jo-Anne"));
    for other in ["hello", "because"] {
        let found = swipe(&engine, other, "");
        assert!(!has(&found, "Thomasina") && !has(&found, "Jo-Anne"), "{other}: {found:?}");
    }
}

/// `swipe::fit`, the measure a learned word gets: lowest for the word drawn, infinite for a
/// word with a letter far from the path, for one that starts with a mark, and for no letters.
#[test]
fn fit_measures_a_word_against_the_path() {
    use mlengine_core::swipe::{MAX_POINTS, STEP, fit, resample};
    let keys = qwerty();
    let map = KeyMap::new(&keys).unwrap();
    let path = resample(&map.scale(&ideal_path(&map, "hello", 4.0)), STEP, MAX_POINTS);
    let drawn = fit(&map, &path, "hello");
    assert!(drawn.is_finite());
    assert_eq!(fit(&map, &path, "Hello"), drawn, "case doesn't matter");
    assert_eq!(fit(&map, &path, "hell-o"), drawn, "a hyphen is not drawn");
    assert!(fit(&map, &path, "help") > drawn);
    // q and z are nowhere near a path from h to o.
    assert_eq!(fit(&map, &path, "hezlo"), f32::INFINITY);
    assert_eq!(fit(&map, &path, "'hello"), f32::INFINITY);
    assert_eq!(fit(&map, &path, "hello2"), f32::INFINITY);
    assert_eq!(fit(&map, &path, ""), f32::INFINITY);
}

#[test]
fn a_swipe_is_quick() {
    let engine = engine();
    let _ = swipe(&engine, "the", "");
    let start = Instant::now();
    for word in ["something", "because", "people", "sukamanu", "enthanu"] {
        let _ = swipe(&engine, word, "and then ");
    }
    // Debug build on the Mac; the release build on the phone is what the design's 25 ms is for.
    assert!(start.elapsed().as_millis() < 2_000, "{:?}", start.elapsed());
}

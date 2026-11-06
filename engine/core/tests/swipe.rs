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

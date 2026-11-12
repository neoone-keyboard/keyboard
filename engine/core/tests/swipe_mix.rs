//! Swiping with English and native packs active together: a common English word the path
//! fits stays first over a pack's word that fits it only as loosely or worse, even after a
//! word of the pack; a pack's word the path really spells still comes first.

use std::path::{Path, PathBuf};

use mlengine_core::lang::{EN, LangId};
use mlengine_core::pack::Pack;
use mlengine_core::swipe::{KeyMap, ideal_path, qwerty};
use mlengine_core::{Candidate, Engine, Paths};

mod paths;
use paths::{NOISE, Rng, noisy_path, seed};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// English and the packs `ids`, all active in that order, without their transliterators.
fn engine(ids: &[&str]) -> Engine {
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    let packs: Vec<Pack> = ids
        .iter()
        .map(|id| Pack::open(&root().join(format!("packs/{id}.neopack"))).unwrap())
        .collect();
    let mut paths = Paths::with_packs(Some(en), &packs, None);
    for p in &mut paths.packs {
        p.translit = None;
    }
    let mut engine = Engine::open(&paths);
    let active: Vec<LangId> = std::iter::once("en")
        .chain(ids.iter().copied())
        .map(|id| LangId(id.into()))
        .collect();
    engine.set_active(&active).unwrap();
    engine
}

/// The first candidate of a swipe along `path`, after `before`.
fn first(engine: &Engine, path: &[(f32, f32)], before: &str) -> Option<Candidate> {
    engine.swipe(path, &qwerty(), before, None).into_iter().next()
}

/// Twenty wobbly swipes of each word (the swipe eval's paths) with English, Hindi and
/// Assamese active, at the start and after an Assamese word: how often a pack's word comes
/// first. An English word that beats the one drawn (`help` for a hurried `hello`) is the
/// English ranking's call, not counted here. Before `Weights::english_common`, after আমি a
/// pack's word took `tomorrow` 19 times (তোমাৰ, টুকুৰাৰে …), `hello` 20 (হেলা), `please` 19
/// (পাচে) and `water` 15 (ভৰিৰ).
#[test]
fn a_common_english_swipe_stays_first_beside_packs() {
    let engine = engine(&["hi", "as"]);
    let map = KeyMap::new(&qwerty()).unwrap();
    let mut over = Vec::new();
    for before in ["", "আমি "] {
        for (word, at_most) in [("tomorrow", 7), ("hello", 6), ("please", 2), ("water", 3)] {
            let mut rng = Rng(seed(word));
            let mut native = Vec::new();
            for _ in 0..20 {
                let path = noisy_path(&map, word, &mut rng, NOISE);
                if let Some(c) = first(&engine, &path, before).filter(|c| c.lang != EN) {
                    native.push(c.text);
                }
            }
            if native.len() > at_most {
                over.push(format!("{before:?} {word}: {} of 20 to {native:?}", native.len()));
            }
        }
    }
    assert!(over.is_empty(), "{over:#?}");
}

/// A native word the path spells comes first on the same engines, after a word of its own
/// language: कभी, ഞാൻ, তুমি.
#[test]
fn a_native_swipe_that_spells_the_word_stays_first() {
    let map = KeyMap::new(&qwerty()).unwrap();
    for (ids, before, drawn, word) in [
        (&["hi", "ml"][..], "मैं ", "kabhi", "कभी"),
        (&["hi", "ml"][..], "", "njan", "ഞാൻ"),
        (&["hi", "as"][..], "मैं ", "kabhi", "कभी"),
        (&["hi", "as"][..], "আমি ", "tumi", "তুমি"),
    ] {
        let engine = engine(ids);
        let path = ideal_path(&map, drawn, 4.0);
        let got = first(&engine, &path, before).map(|c| c.text);
        assert_eq!(got.as_deref(), Some(word), "{ids:?} {before:?} {drawn}");
    }
}

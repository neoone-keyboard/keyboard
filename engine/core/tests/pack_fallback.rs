//! M6 phase 3, fallback: a pack that is missing or broken when the engine opens is left out
//! with a reported problem (`Engine::pack_problems`), and English and every other pack still
//! work.

use std::fs;
use std::path::{Path, PathBuf};

use mlengine_core::pack::Pack;
use mlengine_core::{Engine, Layout, Paths};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn en_fst() -> PathBuf {
    root().join("apps/ios/KeyboardExtension/Lexicon/en.fst")
}

/// A pack folder `<id>.neopack` in `dir` whose manifest is fine but whose lexicon is not an FST.
fn broken_pack(dir: &Path, id: &str) -> PathBuf {
    let pack = dir.join(format!("{id}.neopack"));
    fs::create_dir_all(&pack).unwrap();
    let manifest = format!(
        r#"{{"format": 1, "id": "{id}", "version": "1", "name": "X", "nativeName": "X",
            "script": "mlym", "minEngine": 1, "files": {{"lexicon": "x.fst"}}}}"#
    );
    fs::write(pack.join("manifest.json"), manifest).unwrap();
    fs::write(pack.join("x.fst"), b"not an fst").unwrap();
    pack
}

fn texts(engine: &Engine, context: &str) -> Vec<String> {
    engine
        .suggest(context, Layout::Latin, true)
        .candidates
        .into_iter()
        .map(|c| c.text)
        .collect()
}

#[test]
fn a_pack_whose_lexicon_does_not_open_is_reported_and_english_still_works() {
    let dir = tempfile::tempdir().unwrap();
    let pack = Pack::open(&broken_pack(dir.path(), "xx")).expect("the manifest is fine");
    let engine = Engine::open(&Paths::with_packs(Some(en_fst()), &[pack], None));
    assert_eq!(engine.loaded_sources(), ["english"]);
    let problems = engine.pack_problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].starts_with("xx.neopack: lexicon unreadable"), "{problems:?}");
    assert_eq!(texts(&engine, "teh")[0], "the");
}

#[test]
fn a_broken_pack_leaves_the_others_working() {
    let dir = tempfile::tempdir().unwrap();
    let broken = Pack::open(&broken_pack(dir.path(), "xx")).unwrap();
    let ml = Pack::open(&root().join("packs/ml.neopack")).unwrap();
    let engine = Engine::open(&Paths::with_packs(Some(en_fst()), &[broken, ml], None));
    assert!(engine.lang_of_id("ml").is_some());
    assert_eq!(engine.pack_problems().len(), 1);
    assert_eq!(texts(&engine, "ente")[0], "എന്റെ");
}

#[test]
fn a_second_pack_with_the_same_id_and_a_pack_called_en_are_left_out() {
    let dir = tempfile::tempdir().unwrap();
    let ml = Pack::open(&root().join("packs/ml.neopack")).unwrap();
    let mut en = ml.clone();
    en.manifest.id = "en".into();
    let engine = Engine::open(&Paths::with_packs(
        Some(en_fst()),
        &[ml.clone(), ml, en],
        None,
    ));
    assert_eq!(
        engine.active_languages().iter().map(|l| l.0.as_str()).collect::<Vec<_>>(),
        ["en", "ml"]
    );
    assert!(engine.lang_of_id("ml").is_some());
    let problems = engine.pack_problems();
    assert_eq!(problems.len(), 2, "{problems:?}");
    assert!(problems[0].starts_with("ml.neopack: another pack has id ml"), "{problems:?}");
    assert!(problems[1].starts_with("ml.neopack: id en is the English base"), "{problems:?}");
    drop(dir);
}

#[test]
fn opening_without_packs_reports_nothing() {
    let engine = Engine::open(&Paths {
        english: Some(en_fst()),
        ..Paths::default()
    });
    assert!(engine.pack_problems().is_empty());
}

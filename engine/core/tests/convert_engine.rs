//! A phrase-conversion pack through the engine's API, as the apps will call it: the pack opens
//! as a converter beside the word packs, the keyboard composes typed letters and converts the
//! kana, and the word engine doesn't see the pack.

use std::fs;
use std::path::{Path, PathBuf};

use mlengine_core::convert::{connection, dict};
use mlengine_core::pack::Pack;
use mlengine_core::{Engine, Paths};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A small Japanese pack in `dir`: ids 0 BOS/EOS, 1 noun, 2 particle, 3 verb, 4 unknown.
fn write_pack(dir: &Path) {
    let row = |reading: &str, id: u16, cost: u16, surface: &str| dict::Row {
        reading: reading.into(),
        lid: id,
        rid: id,
        cost,
        surface: surface.into(),
    };
    let rows = [
        row("きょう", 1, 1000, "今日"),
        row("きょう", 1, 1500, "京"),
        row("は", 2, 100, "は"),
        row("あめ", 1, 1000, "雨"),
        row("が", 2, 100, "が"),
        row("ふる", 3, 1000, "降る"),
    ];
    fs::write(
        dir.join("conversion.dict"),
        dict::build(&rows, 5, 4, &[2]).unwrap(),
    )
    .unwrap();
    let mut costs = vec![500; 25];
    costs[5 + 2] = 0;
    costs[2 * 5 + 3] = 0;
    fs::write(
        dir.join("connection.bin"),
        connection::build(5, &costs).unwrap(),
    )
    .unwrap();
    fs::copy(
        root().join("data/langs/ja/kana.json"),
        dir.join("kana.json"),
    )
    .unwrap();
    fs::write(
        dir.join("manifest.json"),
        r#"{"format": 1, "id": "ja", "version": "1", "name": "Japanese", "nativeName": "日本語",
            "script": "jpan", "minEngine": 9, "conversion": "phrase",
            "files": {"kana": "kana.json", "conversionLexicon": "conversion.dict",
                      "connection": "connection.bin"}}"#,
    )
    .unwrap();
}

fn engine_with(dirs: &[&Path]) -> Engine {
    let packs: Vec<Pack> = dirs.iter().map(|d| Pack::open(d).unwrap()).collect();
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    Engine::open(&Paths::with_packs(Some(en), &packs, None))
}

#[test]
fn composes_and_converts_through_the_engine() {
    let ja = tempfile::tempdir().unwrap();
    write_pack(ja.path());
    let hi = root().join("packs/hi.neopack");
    let engine = engine_with(&[&hi, ja.path()]);
    assert_eq!(engine.pack_problems(), Vec::<String>::new());
    assert_eq!(
        engine
            .conversion_languages()
            .iter()
            .map(|l| l.0.as_str())
            .collect::<Vec<_>>(),
        ["ja"]
    );
    // Not a language of the word engine.
    assert!(engine.lang_of_id("ja").is_none());
    assert!(engine.lang_of_id("hi").is_some());

    // While typing: kana and the letters waiting; then finished and converted.
    assert_eq!(
        engine.compose("ja", "kyouhaamegafur", false),
        Some(("きょうはあめがふ".into(), "r".into()))
    );
    let (kana, waiting) = engine.compose("ja", "kyouhaamegafuru", true).unwrap();
    assert_eq!(waiting, "");
    let conversion = engine.convert("ja", &kana, 5).unwrap();
    assert_eq!(conversion.candidates[0], "今日は雨が降る");
    assert_eq!(conversion.segments[0].reading, "きょうは");
    assert_eq!(conversion.first[0], "今日は");
    assert!(conversion.first.contains(&"京は".to_string()));

    assert_eq!(engine.compose("hi", "a", true), None);
    assert_eq!(engine.convert("xx", "あ", 5), None);
}

#[test]
fn a_broken_conversion_pack_is_reported() {
    let ja = tempfile::tempdir().unwrap();
    write_pack(ja.path());
    fs::write(ja.path().join("connection.bin"), b"not a matrix").unwrap();
    let engine = engine_with(&[ja.path()]);
    assert!(engine.conversion_languages().is_empty());
    let problems = engine.pack_problems();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems[0].contains("not a connection matrix"),
        "{problems:?}"
    );
}

#[test]
fn an_id_twice_keeps_the_first() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    write_pack(a.path());
    write_pack(b.path());
    let engine = engine_with(&[a.path(), b.path()]);
    assert_eq!(engine.conversion_languages().len(), 1);
    assert_eq!(
        engine.pack_problems().len(),
        1,
        "{:?}",
        engine.pack_problems()
    );
}

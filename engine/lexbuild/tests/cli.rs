//! Runs the real `lexbuild` binary on tiny fixture corpora and types against the result.

use std::path::Path;
use std::process::Command;

use mlengine_core::lexicon::Lexicon;
use mlengine_core::{Engine, Layout, Paths};

#[test]
fn builds_a_bundle_the_engine_can_use() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let out = tempfile::tempdir().unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_lexbuild"))
        .args(["--english".as_ref(), fixtures.join("en.txt").as_os_str()])
        .args(["--malayalam".as_ref(), fixtures.join("ml.txt").as_os_str()])
        .args(["--romanisation".as_ref(), fixtures.join("../../../../packs/ml.neopack/romanisation.json").as_os_str()])
        .args(["--scowl".as_ref(), fixtures.join("scowl.txt").as_os_str()])
        .args(["--smc".as_ref(), fixtures.join("smc.txt").as_os_str()])
        .args(["--out".as_ref(), out.path().as_os_str()])
        .status()
        .unwrap();
    assert!(status.success());
    for name in ["en.fst", "ml.fst", "manglish.fst", "next.fst", "stats.txt"] {
        assert!(out.path().join(name).exists(), "{name} missing");
    }

    let paths = Paths {
        english: Some(out.path().join("en.fst")),
        malayalam: Some(out.path().join("ml.fst")),
        manglish: Some(out.path().join("manglish.fst")),
        romanisation: Some(fixtures.join("../../../../packs/ml.neopack/romanisation.json")),
        next_words: Some(out.path().join("next.fst")),
        translit: None,
        lm: None,
        user_dir: None,
    };
    let engine = Engine::open(&paths);
    assert_eq!(
        engine.loaded_sources(),
        ["english", "malayalam", "manglish", "next_words"]
    );

    let first = |context: &str, layout| {
        engine.suggest(context, layout, true).candidates[0]
            .text
            .clone()
    };
    assert_eq!(first("teh", Layout::Latin), "the");
    assert_eq!(first("londn", Layout::Latin), "London");
    assert_eq!(first("I ", Layout::Latin), "am");
    assert_eq!(first("ende", Layout::Latin), "എന്റെ");
    let ml = engine.suggest("എന്", Layout::Malayalam, true);
    assert_eq!(ml.candidates[1].text, "എന്റെ");
    let names = engine.suggest("കൊ", Layout::Malayalam, true);
    assert!(
        names.candidates.iter().any(|c| c.text == "കൊച്ചി"),
        "SMC list words are included"
    );
}

#[test]
fn builds_a_pack_language_from_its_config() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let config = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/langs/as/lang.json");
    // A Malayalam bundle first, for its English next words.
    let english = tempfile::tempdir().unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_lexbuild"))
        .args(["--english".as_ref(), fixtures.join("en.txt").as_os_str()])
        .args(["--malayalam".as_ref(), fixtures.join("ml.txt").as_os_str()])
        .args(["--romanisation".as_ref(), fixtures.join("../../../../packs/ml.neopack/romanisation.json").as_os_str()])
        .args(["--scowl".as_ref(), fixtures.join("scowl.txt").as_os_str()])
        .args(["--out".as_ref(), english.path().as_os_str()])
        .status()
        .unwrap();
    assert!(status.success());
    let out = tempfile::tempdir().unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_lexbuild-native"))
        .args(["--lang-config".as_ref(), config.as_os_str()])
        .args(["--native".as_ref(), fixtures.join("as.txt").as_os_str()])
        .args([
            "--english-next".as_ref(),
            english.path().join("next.fst").as_os_str(),
        ])
        .args([
            "--romanised-keys".as_ref(),
            fixtures.join("as-romanised.tsv").as_os_str(),
        ])
        .args(["--out".as_ref(), out.path().as_os_str()])
        .status()
        .unwrap();
    assert!(status.success());

    let lexicon = Lexicon::open(&out.path().join("as.fst")).unwrap();
    assert_eq!(lexicon.get("মই".as_bytes()), Some(4));
    // তোমার is counted as তোমাৰ: the Bengali ra folds to the Assamese one.
    assert_eq!(lexicon.get("তোমাৰ".as_bytes()), Some(3));
    assert_eq!(lexicon.get("তোমার".as_bytes()), None);
    assert_eq!(lexicon.get("হ'ল".as_bytes()), Some(3));

    let next = Lexicon::open(&out.path().join("next.fst")).unwrap();
    assert_eq!(next.get("মই\0ভাত".as_bytes()), Some(3));
    assert!(next.get(b"i\0am").is_some(), "English pairs are copied");
    // A danda ends a phrase: "খাওঁ। মই" is no pair.
    assert_eq!(next.get("খাওঁ\0মই".as_bytes()), None);

    let romanised = Lexicon::open(&out.path().join("romanised.fst")).unwrap();
    let mut keys = 0;
    romanised.words(|_, _| keys += 1);
    assert_eq!(keys, 2);
}

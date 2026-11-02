//! Runs the real `lexbuild` binary on tiny fixture corpora and types against the result.

use std::path::Path;
use std::process::Command;

use mlengine_core::{Engine, Layout, Paths};

#[test]
fn builds_a_bundle_the_engine_can_use() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let out = tempfile::tempdir().unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_lexbuild"))
        .args(["--english".as_ref(), fixtures.join("en.txt").as_os_str()])
        .args(["--malayalam".as_ref(), fixtures.join("ml.txt").as_os_str()])
        .args(["--scowl".as_ref(), fixtures.join("scowl.txt").as_os_str()])
        .args(["--smc".as_ref(), fixtures.join("smc.txt").as_os_str()])
        .args(["--out".as_ref(), out.path().as_os_str()])
        .status()
        .unwrap();
    assert!(status.success());
    for name in ["en.fst", "ml.fst", "next.fst", "stats.txt"] {
        assert!(out.path().join(name).exists(), "{name} missing");
    }

    let paths = Paths {
        english: Some(out.path().join("en.fst")),
        malayalam: Some(out.path().join("ml.fst")),
        next_words: Some(out.path().join("next.fst")),
        user_dir: None,
    };
    let engine = Engine::open(&paths);
    assert_eq!(
        engine.loaded_sources(),
        ["english", "malayalam", "next_words"]
    );

    let first = |context: &str, layout| {
        engine.suggest(context, layout, true).candidates[0]
            .text
            .clone()
    };
    assert_eq!(first("teh", Layout::Latin), "the");
    assert_eq!(first("londn", Layout::Latin), "London");
    assert_eq!(first("I ", Layout::Latin), "am");
    let ml = engine.suggest("എന്", Layout::Malayalam, true);
    assert_eq!(ml.candidates[1].text, "എന്റെ");
    let names = engine.suggest("കൊ", Layout::Malayalam, true);
    assert!(
        names.candidates.iter().any(|c| c.text == "കൊച്ചി"),
        "SMC list words are included"
    );
}

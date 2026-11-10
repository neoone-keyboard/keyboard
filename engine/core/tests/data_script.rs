//! A pack in a script the engine has no code for, defined only by its `lang` file (engine 4):
//! the Gujarati test pack `fixtures/gu.neopack`. It opens, its words are words of its script,
//! and it suggests Gujarati words for romanised and for native typing.
//!
//! The pack is test-sized and built from `fixtures/gu-corpus.txt` with its own `lang.json` and
//! `romanisation.json` (Hindi's tables moved to the Gujarati block; not a checked
//! romanisation). To rebuild it, from `engine/`:
//!
//!     F=core/tests/fixtures
//!     cargo run -p lexbuild --bin lexbuild-native -- --lang-config $F/gu.neopack/lang.json \
//!         --native $F/gu-corpus.txt --romanisation $F/gu.neopack/romanisation.json --out $F/gu.neopack
//!     rm $F/gu.neopack/stats.txt $F/gu.neopack/words.tsv
//!     python3 -I ../scripts/pack.py $F/gu.neopack --write

use std::fs;
use std::path::{Path, PathBuf};

use mlengine_core::pack::{Pack, PackError};
use mlengine_core::text::{self, Script};
use mlengine_core::{Engine, Kind, Layout, Paths, Suggestions, script};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gu.neopack")
}

fn en_fst() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/ios/KeyboardExtension/Lexicon/en.fst")
}

fn engine() -> Engine {
    let pack = Pack::open(&fixture()).unwrap();
    Engine::open(&Paths::with_packs(Some(en_fst()), &[pack], None))
}

fn texts(s: &Suggestions) -> Vec<&str> {
    s.candidates.iter().map(|c| c.text.as_str()).collect()
}

fn lang_of(e: &Engine, s: &Suggestions, text: &str) -> Option<String> {
    s.candidates
        .iter()
        .find(|c| c.text == text)
        .and_then(|c| e.lang_id(c.lang))
        .map(|l| l.0)
}

/// A copy of the fixture in a temporary folder, with `edit` applied to the manifest text and
/// `lang` (if given) as its `lang.json`.
fn edited(edit: impl Fn(&str) -> String, lang: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for entry in fs::read_dir(fixture()).unwrap() {
        let path = entry.unwrap().path();
        fs::copy(&path, dir.path().join(path.file_name().unwrap())).unwrap();
    }
    let manifest = fs::read_to_string(dir.path().join("manifest.json")).unwrap();
    fs::write(dir.path().join("manifest.json"), edit(&manifest)).unwrap();
    if let Some(lang) = lang {
        fs::write(dir.path().join("lang.json"), lang).unwrap();
    }
    dir
}

#[test]
fn the_pack_opens_with_its_own_script() {
    let pack = Pack::open(&fixture()).unwrap();
    assert_eq!(pack.manifest.script, "gujr");
    assert_eq!(pack.manifest.min_engine, 4);
    assert!(pack.file("lang").is_some());
    assert!(!script::supported().contains(&"gujr"), "no code for Gujarati");

    let e = engine();
    let loaded = e.loaded_sources();
    for part in ["gu:lexicon", "gu:romanised", "gu:next_words"] {
        assert!(loaded.iter().any(|s| s == part), "{part} in {loaded:?}");
    }
    assert!(e.pack_problems().is_empty(), "{:?}", e.pack_problems());
    // Once the engine has opened the pack, its words are words of the script.
    assert_eq!(text::script("ગુજરાતી"), Script::Native("gujr"));
    assert_eq!(text::script("ગુજરાત૧"), Script::Other, "a digit is no letter");
    let rules = script::rules("gujr").unwrap();
    assert_eq!(rules.mark_base("ગુજર"), Some('ર'));
    assert_eq!(rules.mark_base("ગુ"), None);
    assert!(rules.ends_phrase('।'));
}

#[test]
fn romanised_typing_suggests_gujarati() {
    let e = engine();
    for (typed, want) in [("gujarat", "ગુજરાત"), ("gujarati", "ગુજરાતી"), ("namaste", "નમસ્તે")] {
        let s = e.suggest(typed, Layout::Latin, false);
        assert!(texts(&s).contains(&want), "{typed}: {:?}", texts(&s));
        assert_eq!(lang_of(&e, &s, want).as_deref(), Some("gu"));
    }
}

#[test]
fn native_typing_completes_and_predicts() {
    let e = engine();
    let gu = Layout::Native(e.lang_of_id("gu").unwrap());
    let s = e.suggest("ગુજ", gu, false);
    assert_eq!(s.typed, "ગુજ");
    assert!(
        s.candidates
            .iter()
            .any(|c| c.kind == Kind::Completion && c.text.starts_with("ગુજરાત")),
        "{:?}",
        texts(&s)
    );
    let s = e.suggest("ગુજરાતી ", gu, false);
    assert!(
        s.candidates.iter().any(|c| c.kind == Kind::NextWord),
        "next words after ગુજરાતી: {:?}",
        texts(&s)
    );
    // The danda ends the word.
    let s = e.suggest("નમસ્તે।", gu, false);
    assert_eq!(s.typed, "");
}

#[test]
fn a_new_script_without_rules_is_refused_in_words() {
    let without = edited(|m| m.replace(",\n    \"lang\": \"lang.json\"", ""), None);
    fs::remove_file(without.path().join("lang.json")).unwrap();
    let err = Pack::open(without.path()).unwrap_err();
    assert!(matches!(err, PackError::Script(_)), "{err:?}");
    assert!(err.to_string().contains("no lang file"), "{err}");

    let missing = edited(|m| m.to_string(), None);
    fs::remove_file(missing.path().join("lang.json")).unwrap();
    let err = Pack::open(missing.path()).unwrap_err();
    assert!(err.to_string().contains("isn't in the pack"), "{err}");

    for (lang, why) in [
        (r#"{"script": "taml", "block": ["0B80", "0BFF"]}"#, "the manifest says gujr"),
        (r#"{"script": "gujr", "block": ["0D00", "0D7F"]}"#, "overlap script mlym"),
        (r#"{"script": "gujr"}"#, "no characters"),
        ("{", "lang.json"),
    ] {
        let dir = edited(|m| m.to_string(), Some(lang));
        let err = Pack::open(dir.path()).unwrap_err();
        assert!(matches!(err, PackError::Script(_)), "{lang}: {err:?}");
        assert!(err.to_string().contains(why), "{lang}: {err}");
    }
}

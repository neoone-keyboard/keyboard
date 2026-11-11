//! Runs the real `lexbuild` binary on tiny fixture corpora and types against the result.

use std::path::Path;
use std::process::Command;

use mlengine_core::lexicon::Lexicon;
use mlengine_core::{Engine, Layout, PackPaths, Paths};

/// The Malayalam pack's native layout: it is the first pack loaded.
const ML: Layout = Layout::Native(mlengine_core::lang::Lang(1));

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
    for name in ["en.fst", "en-next.fst", "ml.fst", "manglish.fst", "next.fst", "stats.txt"] {
        assert!(out.path().join(name).exists(), "{name} missing");
    }
    // The English base's own next words: next.fst's English pairs, and only those.
    let next = Lexicon::open(&out.path().join("next.fst")).unwrap();
    let english_next = Lexicon::open(&out.path().join("en-next.fst")).unwrap();
    assert!(english_next.get(b"i\0am").is_some());
    let mut all = Vec::new();
    next.words(|k, c| all.push((k.to_vec(), c)));
    let mut english = Vec::new();
    english_next.words(|k, c| english.push((k.to_vec(), c)));
    let latin: Vec<_> = all
        .iter()
        .filter(|(k, _)| k.split(|&b| b == 0).next().unwrap().is_ascii())
        .cloned()
        .collect();
    assert!(latin.len() < all.len(), "the fixture has Malayalam pairs too");
    assert_eq!(english, latin);

    let paths = Paths {
        english: Some(out.path().join("en.fst")),
        english_next: None,
        packs: vec![PackPaths {
            lexicon: Some(out.path().join("ml.fst")),
            romanised: Some(out.path().join("manglish.fst")),
            romanisation: Some(fixtures.join("../../../../packs/ml.neopack/romanisation.json")),
            next_words: Some(out.path().join("next.fst")),
            ..PackPaths::new("ml", "mlym")
        }],
        user_dir: None,
    };
    let engine = Engine::open(&paths);
    assert_eq!(
        engine.loaded_sources(),
        ["english", "ml:lexicon", "ml:romanised", "ml:next_words"]
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
    let ml = engine.suggest("എന്", ML, true);
    assert_eq!(ml.candidates[1].text, "എന്റെ");
    let names = engine.suggest("കൊ", ML, true);
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

    // The stats say how many tokens of the script were dropped as not words.
    let stats = std::fs::read_to_string(out.path().join("stats.txt")).unwrap();
    assert!(
        stats.contains("as dropped (a letter of the script, but not a word): "),
        "{stats}"
    );
}

/// A tiny `en.fst` for `--english-fst`, from the Malayalam mode.
fn english_bundle(fixtures: &Path) -> tempfile::TempDir {
    let out = tempfile::tempdir().unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_lexbuild"))
        .args(["--english".as_ref(), fixtures.join("en.txt").as_os_str()])
        .args(["--malayalam".as_ref(), fixtures.join("ml.txt").as_os_str()])
        .args(["--romanisation".as_ref(), fixtures.join("../../../../packs/ml.neopack/romanisation.json").as_os_str()])
        .args(["--scowl".as_ref(), fixtures.join("scowl.txt").as_os_str()])
        .args(["--out".as_ref(), out.path().as_os_str()])
        .status()
        .unwrap();
    assert!(status.success());
    out
}

/// `lexbuild --lang hi` on the Hindi fixture, with `extra` arguments, into a new folder.
fn hindi_bundle(fixtures: &Path, english: &Path, extra: &[&str]) -> tempfile::TempDir {
    let out = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lexbuild"))
        .args(["--english".as_ref(), fixtures.join("en.txt").as_os_str()])
        .args(["--lang", "hi"])
        .args(["--native".as_ref(), fixtures.join("hi.txt").as_os_str()])
        .args(["--romanisation".as_ref(), fixtures.join("../../../../packs/hi.neopack/romanisation.json").as_os_str()])
        .args(["--romanised-name", "hinglish"])
        .args(["--english-fst".as_ref(), english.join("en.fst").as_os_str()])
        .args(["--out".as_ref(), out.path().as_os_str()])
        .args(extra)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    // The report goes to stdout and to stats.txt, the same text.
    let stats = std::fs::read_to_string(out.path().join("stats.txt")).unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout), stats);
    out
}

/// The `--lang` mode (Hindi's build, `data/build-pack-lexicons.sh`): the pack's lexicon, its
/// romanised lexicon keyed by the pack's own tables and named by `--romanised-name`, and next
/// words for the pack's words and the base's English.
#[test]
fn builds_a_lang_pack_the_engine_can_use() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let tables = fixtures.join("../../../../packs/hi.neopack/romanisation.json");
    let english = english_bundle(&fixtures);
    let out = hindi_bundle(&fixtures, english.path(), &[]);
    for name in ["hi.fst", "hinglish.fst", "next.fst", "stats.txt"] {
        assert!(out.path().join(name).exists(), "{name} missing");
    }
    assert!(!out.path().join("en.fst").exists(), "the base's lexicon is not rebuilt");

    let lexicon = Lexicon::open(&out.path().join("hi.fst")).unwrap();
    assert_eq!(lexicon.get("मेरा".as_bytes()), Some(6));
    assert_eq!(lexicon.get("है".as_bytes()), Some(8));
    assert_eq!(lexicon.get("घर".as_bytes()), Some(4));
    assert_eq!(lexicon.get("में".as_bytes()), Some(3));
    // Twice in the corpus: under the least count kept.
    assert_eq!(lexicon.get("नाम".as_bytes()), None);
    // Latin words and digits in the native corpus are not the pack's words.
    assert_eq!(lexicon.get(b"hello"), None);

    let next = Lexicon::open(&out.path().join("next.fst")).unwrap();
    assert_eq!(next.get("मेरा\0घर".as_bytes()), Some(3));
    assert!(next.get(b"i\0am").is_some(), "English pairs are counted against en.fst");
    // A danda ends a phrase: "है। मेरा" is no pair.
    assert_eq!(next.get("है\0मेरा".as_bytes()), None);

    // Every lexicon word has its key in the romanised lexicon, as the pack's tables spell it.
    let romanisation = mlengine_core::romanisation::Romanisation::load(&tables).unwrap();
    let romanised = Lexicon::open(&out.path().join("hinglish.fst")).unwrap();
    let mut words = Vec::new();
    lexicon.words(|k, _| words.push(String::from_utf8(k.to_vec()).unwrap()));
    assert!(words.len() >= 4, "{words:?}");
    for word in &words {
        let key = mlengine_core::manglish::key(&romanisation, word).unwrap();
        assert!(romanised.get(&key).is_some(), "{word} has no romanised key");
    }

    let paths = Paths {
        english: Some(english.path().join("en.fst")),
        english_next: None,
        packs: vec![PackPaths {
            lexicon: Some(out.path().join("hi.fst")),
            romanised: Some(out.path().join("hinglish.fst")),
            romanisation: Some(tables),
            next_words: Some(out.path().join("next.fst")),
            ..PackPaths::new("hi", "deva")
        }],
        user_dir: None,
    };
    let engine = Engine::open(&paths);
    assert_eq!(
        engine.loaded_sources(),
        ["english", "hi:lexicon", "hi:romanised", "hi:next_words"]
    );
    let texts = |context: &str| -> Vec<String> {
        engine
            .suggest(context, Layout::Latin, true)
            .candidates
            .into_iter()
            .map(|c| c.text)
            .collect()
    };
    assert!(texts("mera").contains(&"मेरा".to_string()), "{:?}", texts("mera"));
    assert_eq!(texts("मेरा ")[0], "घर");
}

/// `lexbuild-rekey` writes the romanised lexicon `lexbuild --lang` wrote, from the native
/// lexicon and the tables alone: the same file, byte for byte.
#[test]
fn rekey_rebuilds_the_romanised_lexicon_lang_mode_wrote() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let tables = fixtures.join("../../../../packs/hi.neopack/romanisation.json");
    let english = english_bundle(&fixtures);
    let built = hindi_bundle(&fixtures, english.path(), &[]);
    let out = tempfile::tempdir().unwrap();
    let rekeyed = out.path().join("hinglish.fst");
    let status = Command::new(env!("CARGO_BIN_EXE_lexbuild-rekey"))
        .arg(&tables)
        .arg(built.path().join("hi.fst"))
        .arg(&rekeyed)
        .status()
        .unwrap();
    assert!(status.success());
    let was = std::fs::read(built.path().join("hinglish.fst")).unwrap();
    assert!(Lexicon::from_bytes(was.clone()).unwrap().len() >= 4);
    assert_eq!(std::fs::read(&rekeyed).unwrap(), was);

    // Too few arguments: the usage line, exit code 2, nothing written.
    let output = Command::new(env!("CARGO_BIN_EXE_lexbuild-rekey"))
        .arg(&tables)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage: lexbuild-rekey"));
}

/// `--max-chars` drops longer native words; `--max-lines` stops each corpus early;
/// `--native-list` keeps a word whatever its count.
#[test]
fn lang_mode_options_change_what_is_kept() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let english = english_bundle(&fixtures);
    let get = |dir: &tempfile::TempDir, word: &str| {
        Lexicon::open(&dir.path().join("hi.fst"))
            .unwrap()
            .get(word.as_bytes())
    };

    let all = hindi_bundle(&fixtures, english.path(), &[]);
    assert_eq!(get(&all, "ज़िंदगी"), Some(3));

    // मेरा is four chars, ज़िंदगी seven.
    let short = hindi_bundle(&fixtures, english.path(), &["--max-chars", "4"]);
    assert_eq!(get(&short, "मेरा"), Some(6));
    assert_eq!(get(&short, "ज़िंदगी"), None);

    // The first two lines only: मेरा four times, में once (dropped), no ज़िंदगी.
    let two = hindi_bundle(&fixtures, english.path(), &["--max-lines", "2"]);
    assert_eq!(get(&two, "मेरा"), Some(4));
    assert_eq!(get(&two, "में"), None);
    assert_eq!(get(&two, "ज़िंदगी"), None);

    // नाम is in the corpus twice: kept, with that count, once it is listed. A listed word the
    // corpus never has is kept with a count of one.
    let list = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(list.path(), "नाम\nभारत\n").unwrap();
    let listed = hindi_bundle(
        &fixtures,
        english.path(),
        &["--native-list", list.path().to_str().unwrap()],
    );
    assert_eq!(get(&all, "नाम"), None);
    assert_eq!(get(&listed, "नाम"), Some(2));
    assert_eq!(get(&listed, "भारत"), Some(1));
}

/// What the command line refuses, and what it says: each must fail and name the problem.
#[test]
fn refuses_arguments_that_do_not_make_a_bundle() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let out = tempfile::tempdir().unwrap();
    let en = fixtures.join("en.txt");
    let ml = fixtures.join("ml.txt");
    let hi = fixtures.join("hi.txt");
    let scowl = fixtures.join("scowl.txt");
    let ml_tables = fixtures.join("../../../../packs/ml.neopack/romanisation.json");
    let hi_tables = fixtures.join("../../../../packs/hi.neopack/romanisation.json");
    let missing = fixtures.join("no-such-corpus.txt");
    let s = |p: &std::path::PathBuf| p.to_str().unwrap().to_string();
    let o = out.path().to_str().unwrap().to_string();
    let cases: Vec<(&str, Vec<String>, &str)> = vec![
        (
            "neither --malayalam nor --native",
            vec!["--english".into(), s(&en), "--scowl".into(), s(&scowl), "--out".into(), o.clone()],
            "--malayalam",
        ),
        (
            "--native without --lang",
            vec![
                "--english".into(), s(&en), "--native".into(), s(&hi),
                "--romanisation".into(), s(&hi_tables), "--english-fst".into(), s(&en),
                "--out".into(), o.clone(),
            ],
            "--lang",
        ),
        (
            "--native without --english-fst",
            vec![
                "--english".into(), s(&en), "--lang".into(), "hi".into(), "--native".into(), s(&hi),
                "--romanisation".into(), s(&hi_tables), "--out".into(), o.clone(),
            ],
            "--english-fst",
        ),
        (
            "--malayalam with --native",
            vec![
                "--english".into(), s(&en), "--malayalam".into(), s(&ml), "--lang".into(), "hi".into(),
                "--native".into(), s(&hi), "--romanisation".into(), s(&hi_tables),
                "--english-fst".into(), s(&en), "--out".into(), o.clone(),
            ],
            "cannot be used with",
        ),
        (
            "the Malayalam bundle without --romanisation",
            vec![
                "--english".into(), s(&en), "--malayalam".into(), s(&ml), "--scowl".into(), s(&scowl),
                "--out".into(), o.clone(),
            ],
            "--romanisation is required",
        ),
        (
            "a corpus file that isn't there",
            vec![
                "--english".into(), s(&missing), "--malayalam".into(), s(&ml), "--scowl".into(), s(&scowl),
                "--romanisation".into(), s(&ml_tables), "--out".into(), o.clone(),
            ],
            "no-such-corpus.txt",
        ),
    ];
    for (what, args, says) in cases {
        let output = Command::new(env!("CARGO_BIN_EXE_lexbuild"))
            .args(&args)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{what}: built a bundle");
        assert!(stderr.contains(says), "{what}: {stderr}");
    }
    // Nothing was written by any of them.
    assert_eq!(std::fs::read_dir(out.path()).unwrap().count(), 0);
}

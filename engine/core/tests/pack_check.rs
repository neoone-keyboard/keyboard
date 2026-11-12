//! The checks every pack must pass, read from its manifest, so a new pack is covered without
//! naming it here. `NEOPACK` picks one pack (an id in `packs/`, or a folder); without it every
//! pack in `packs/` is checked. Every word of each lexicon, so `#[ignore]`: `make test-pack
//! PACK_LANG=xx` runs it for one pack and `make test-data` for all.
//!
//! For each pack:
//! - it opens (`Pack::open`), and the engine loads every source its manifest lists: a
//!   transliterator or LM that doesn't fit the pack is otherwise left out without a word;
//! - every word of its lexicon is in its script's normal form and is a word of that script, so
//!   a typed word normalised at run time meets the stored one;
//! - every key of its romanised lexicon is the skeleton its `romanisation.json` gives the word,
//!   so what is typed folds to the key the word is stored under;
//! - its `layout.json` parses as the keyboard parses it.
//!
//! A phrase-conversion pack (Japanese) has no lexicon or script rules: it opens as the engine's
//! converter, under its id, and converts its own name's reading.

use std::path::{Path, PathBuf};

use fst::{IntoStreamer, Streamer};
use mlengine_core::lexicon::Lexicon;
use mlengine_core::pack::Pack;
use mlengine_core::pack_layout::PackLayout;
use mlengine_core::romanisation::Romanisation;
use mlengine_core::script::{self, ScriptRules};
use mlengine_core::text::{self, Script};
use mlengine_core::{Engine, Paths};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The pack folders to check: `NEOPACK`'s, else all of `packs/`.
fn packs() -> Vec<PathBuf> {
    if let Some(named) = std::env::var("NEOPACK").ok().filter(|n| !n.is_empty()) {
        let as_path = PathBuf::from(&named);
        return vec![if as_path.is_dir() {
            as_path
        } else {
            root().join(format!("packs/{named}.neopack"))
        }];
    }
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root().join("packs"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "neopack") && p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// The pack's `minEngine` is at least the level its contents need (`Pack::needs`): an app on
/// a lower engine would accept it and ignore or misread what needs more. The error names
/// each thing in the pack that needs more than it declares.
fn declares_the_engine_it_needs(pack: &Pack) -> Result<(), String> {
    let id = &pack.manifest.id;
    let declared = pack.manifest.min_engine;
    let needs = pack.needs().map_err(|e| format!("{id}: {e}"))?;
    let above: Vec<String> = needs
        .iter()
        .filter(|n| n.level > declared)
        .map(ToString::to_string)
        .collect();
    if above.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{id}: minEngine is {declared}, but the pack needs {} (`make pack-needs PACK_LANG={id}`); \
         raise minEngine and the version. Needs above {declared}:\n  {}",
        needs[0].level,
        above.join("\n  ")
    ))
}

/// The engine loads what the manifest lists, under the pack's id.
fn sources_load(pack: &Pack) {
    let id = &pack.manifest.id;
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    let engine = Engine::open(&Paths::with_packs(
        Some(en),
        std::slice::from_ref(pack),
        None,
    ));
    let problems = engine.pack_problems();
    assert!(problems.is_empty(), "{id}: {problems:?}");
    let loaded = engine.loaded_sources();
    let roles = [
        ("lexicon", "lexicon"),
        ("romanisedLexicon", "romanised"),
        ("nextWords", "next_words"),
        ("translit", "translit"),
        ("lm", "lm"),
    ];
    for (role, source) in roles {
        if pack.manifest.files.contains_key(role) {
            assert!(
                loaded.contains(&format!("{id}:{source}")),
                "{id}: the manifest lists {role} but the engine didn't load it: {loaded:?}"
            );
        }
    }
}

/// Every lexicon word is in normal form and of the pack's script. Returns the word count.
fn words_are_in_normal_form(pack: &Pack, rules: &'static dyn ScriptRules) -> usize {
    let id = &pack.manifest.id;
    let lexicon = Lexicon::open(&pack.file("lexicon").expect("a lexicon role")).unwrap();
    let code = rules.code();
    let (mut n, mut bad) = (0, Vec::new());
    lexicon.words(|key, _| {
        let word = std::str::from_utf8(key).unwrap();
        n += 1;
        let normal = rules.normalize(word);
        let whole = script::normalize_word(word);
        let class = text::script(word);
        if (normal != word || whole != word || class != Script::Native(code)) && bad.len() < 20 {
            bad.push(format!(
                "{word:?}: normal {normal:?}, normalize_word {whole:?}, {class:?}"
            ));
        }
    });
    assert!(
        bad.is_empty(),
        "{id}: lexicon words not in normal form, first:\n{}",
        bad.join("\n")
    );
    n
}

/// Every key `skeleton\0word` of the romanised lexicon is the tables' skeleton of the word.
/// Returns the key count, or None for a pack without a romanised lexicon.
fn keys_match_the_tables(pack: &Pack, rules: &'static dyn ScriptRules) -> Option<usize> {
    let id = &pack.manifest.id;
    let lexicon = pack.file("romanisedLexicon")?;
    let tables = pack
        .file("romanisation")
        .unwrap_or_else(|| panic!("{id}: a romanisedLexicon needs the romanisation role"));
    let r = Romanisation::load(&tables).unwrap();
    // As the engine unpacks them: by the pack's rules (`ScriptRules::packing`), the 128 code
    // points from the script's first or its own character table.
    let packing = rules.packing();
    let map = fst::Map::new(std::fs::read(lexicon).unwrap()).unwrap();
    let mut stream = map.keys().into_stream();
    let (mut n, mut bad) = (0, Vec::new());
    while let Some(key) = stream.next() {
        if key[0] < mlengine_core::lexicon::FIRST_WORD_BYTE {
            continue; // reserved: totals and precomputed top lists
        }
        n += 1;
        let parts = key.iter().position(|&b| b == 0).and_then(|sep| {
            Some((
                std::str::from_utf8(&key[..sep]).ok()?,
                mlengine_core::manglish::unpack(&key[sep + 1..], packing)?,
            ))
        });
        let Some((stored, word)) = parts else {
            if bad.len() < 20 {
                bad.push(format!(
                    "{:?}: not skeleton, NUL, packed word",
                    String::from_utf8_lossy(key)
                ));
            }
            continue;
        };
        // One of the word's skeletons: a word with a shorter spelling has a second key.
        let got = r.word_skeletons(&word);
        if !got.iter().any(|s| s == stored) && bad.len() < 20 {
            bad.push(format!("{word}: stored {stored}, tables {got:?}"));
        }
    }
    assert!(
        bad.is_empty(),
        "{id}: romanised keys that the tables don't give, first:\n{}",
        bad.join("\n")
    );
    Some(n)
}

/// The engine opens a phrase-conversion pack as a converter under its id, and it converts.
fn converter_loads(pack: &Pack) {
    let id = &pack.manifest.id;
    let engine = Engine::open(&Paths::with_packs(None, std::slice::from_ref(pack), None));
    let problems = engine.pack_problems();
    assert!(problems.is_empty(), "{id}: {problems:?}");
    assert_eq!(engine.conversion_languages().iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), [id.as_str()]);
    // Typed letters compose, and the composed text converts to at least itself.
    let (kana, pending) = engine.compose(id, "a", true).unwrap();
    assert!(pending.is_empty() && !kana.is_empty(), "{id}: a composes to {kana:?} {pending:?}");
    let conversion = engine.convert(id, &kana, 5).unwrap();
    assert!(conversion.candidates.contains(&kana), "{id}: {conversion:?}");
    let c = engine.converter(id).unwrap();
    eprintln!("{id}: a converter, {} readings, {} part-of-speech ids", c.dictionary().readings(), c.dictionary().ids());
}

#[test]
#[ignore]
fn every_pack_passes_its_checks() {
    let dirs = packs();
    assert!(!dirs.is_empty(), "no packs to check");
    for dir in dirs {
        let pack = Pack::open(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        let id = pack.manifest.id.clone();
        if pack.manifest.conversion.is_some() {
            declares_the_engine_it_needs(&pack).unwrap_or_else(|e| panic!("{e}"));
            converter_loads(&pack);
            continue;
        }
        // The rules the engine uses for this pack, in use while it is checked as they are
        // while its engine is open, and no longer: the next pack may share its script
        // (Assamese and Bengali), and a word is read without an engine by the first in use.
        let rules = script::for_pack(&pack.manifest.script, pack.file("lang").as_deref())
            .unwrap_or_else(|e| panic!("{id}: {e}"));
        let _in_use = script::in_use(rules).unwrap_or_else(|e| panic!("{id}: {e}"));

        declares_the_engine_it_needs(&pack).unwrap_or_else(|e| panic!("{e}"));
        sources_load(&pack);
        let words = words_are_in_normal_form(&pack, rules);
        assert!(words > 0, "{id}: an empty lexicon");
        let keys = keys_match_the_tables(&pack, rules);
        let layout = pack.file("layout").map(|path| {
            PackLayout::load(&path).unwrap_or_else(|e| panic!("{id}: {e}"));
        });
        eprintln!(
            "{id}: {words} words in normal form; {}; {}",
            keys.map_or("no romanised lexicon".to_string(), |n| format!(
                "{n} romanised keys match the tables"
            )),
            if layout.is_some() {
                "the layout parses"
            } else {
                "no layout"
            },
        );
    }
}

/// The `minEngine` check alone, for every pack in `packs/` (or `NEOPACK`'s): quick, so
/// `make test-engine` runs it, not only `make test-data`.
#[test]
fn every_pack_declares_the_engine_it_needs() {
    let problems: Vec<String> = packs()
        .iter()
        .map(|dir| Pack::open(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())))
        .filter_map(|pack| declares_the_engine_it_needs(&pack).err())
        .collect();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// Hindi's files under a manifest that declares engine 3, as Hindi 2026.11.14 did: the check
/// fails and names what needs more.
#[test]
fn a_pack_declaring_too_low_an_engine_fails() {
    let hi = root().join("packs/hi.neopack");
    let dir = tempfile::tempdir().unwrap();
    for name in ["romanisation.json", "lang.json"] {
        std::fs::copy(hi.join(name), dir.path().join(name)).unwrap();
    }
    std::fs::write(dir.path().join("hi.fst"), b"x").unwrap();
    let manifest = r#"{"format": 1, "id": "hi", "version": "2026.11.14", "name": "Hindi",
        "nativeName": "हिन्दी", "script": "deva", "minEngine": 3,
        "files": {"lexicon": "hi.fst", "romanisation": "romanisation.json", "lang": "lang.json"}}"#;
    std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();
    let pack = Pack::open(dir.path()).unwrap();
    let err = declares_the_engine_it_needs(&pack).unwrap_err();
    assert!(err.contains("minEngine is 3, but the pack needs 7"), "{err}");
    let named = [
        "optionalFinals",
        "skeleton.alsoBetweenConsonants",
        "skeleton.alsoBesideConsonant",
        "skeleton.dropTrailing.alsoKept",
        "looseSpelling",
    ];
    for field in named {
        assert!(err.contains(field), "{field}: {err}");
    }
    assert!(!err.contains("script deva"), "a need at or below 3 isn't named: {err}");
    let raised = manifest.replace(r#""minEngine": 3"#, r#""minEngine": 7"#);
    std::fs::write(dir.path().join("manifest.json"), raised).unwrap();
    assert_eq!(declares_the_engine_it_needs(&Pack::open(dir.path()).unwrap()), Ok(()));
}

/// A pack whose script lists its characters (`lang.json`'s `charTable`, Urdu's) under a
/// manifest that declares engine 7: the check fails and names the table, as an engine 7 app
/// would unpack its romanised keys from the block's first code point.
#[test]
fn a_pack_with_a_character_table_and_a_low_engine_fails() {
    let dir = tempfile::tempdir().unwrap();
    let lang = r#"{"script": "arab", "block": ["0600", "06FF"],
        "charTable": [["0621", "063A"], ["0641", "064A"], ["06A9", "06A9"], ["06CC", "06CC"]]}"#;
    std::fs::write(dir.path().join("lang.json"), lang).unwrap();
    std::fs::write(dir.path().join("ur.fst"), b"x").unwrap();
    let manifest = r#"{"format": 1, "id": "ur", "version": "2026.11.12", "name": "Urdu",
        "nativeName": "اردو", "script": "arab", "minEngine": 7,
        "files": {"lexicon": "ur.fst", "lang": "lang.json"}}"#;
    std::fs::write(dir.path().join("manifest.json"), manifest).unwrap();
    let err = declares_the_engine_it_needs(&Pack::open(dir.path()).unwrap()).unwrap_err();
    assert!(err.contains("minEngine is 7, but the pack needs 8"), "{err}");
    assert!(err.contains("charTable"), "{err}");
    let raised = manifest.replace(r#""minEngine": 7"#, r#""minEngine": 8"#);
    std::fs::write(dir.path().join("manifest.json"), raised).unwrap();
    assert_eq!(declares_the_engine_it_needs(&Pack::open(dir.path()).unwrap()), Ok(()));
}

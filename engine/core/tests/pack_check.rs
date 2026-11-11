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

/// A romanised-lexicon key's packed native word back to text: block offsets 0x80–0xFF from
/// `base`, ZWNJ 0x01, ZWJ 0x02, the apostrophe 0x27.
fn unpack(bytes: &[u8], base: u32) -> Option<String> {
    bytes
        .iter()
        .map(|&b| match b {
            0x01 => Some('\u{200C}'),
            0x02 => Some('\u{200D}'),
            0x27 => Some('\''),
            0x80.. => char::from_u32(base + u32::from(b - 0x80)),
            _ => None,
        })
        .collect()
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
    let base = *rules.block().start() as u32;
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
                unpack(&key[sep + 1..], base)?,
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

#[test]
#[ignore]
fn every_pack_passes_its_checks() {
    let dirs = packs();
    assert!(!dirs.is_empty(), "no packs to check");
    for dir in dirs {
        let pack = Pack::open(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        let id = pack.manifest.id.clone();
        // The rules the engine uses for this pack, in use while it is checked as they are
        // while its engine is open, and no longer: the next pack may share its script
        // (Assamese and Bengali), and a word is read without an engine by the first in use.
        let rules = script::for_pack(&pack.manifest.script, pack.file("lang").as_deref())
            .unwrap_or_else(|e| panic!("{id}: {e}"));
        let _in_use = script::in_use(rules).unwrap_or_else(|e| panic!("{id}: {e}"));

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

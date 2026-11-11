//! Two packs of one script whose `lang.json` differ, both loaded and active: Assamese and
//! Bengali, both `beng`. Assamese folds র to ৰ, writes an apostrophe inside a word (হ'ল) and
//! takes a vowel sign on ৰ; Bengali keeps র, and ৰ is no letter of its words. Each pack's own
//! rules read its own words, whichever pack was loaded first.
//!
//! The packs are built here from a few words and the two languages' real `lang.json` and
//! `romanisation.json` (`fixtures/same-script/`: copies of the Assamese pack's and of the
//! Bengali trial pack's).

use std::fs;
use std::path::{Path, PathBuf};

use mlengine_core::lang::LangId;
use mlengine_core::lexicon::{SEPARATOR, build};
use mlengine_core::manglish;
use mlengine_core::pack::Pack;
use mlengine_core::romanisation::Romanisation;
use mlengine_core::script;
use mlengine_core::swipe::{KeyMap, ideal_path, qwerty};
use mlengine_core::{Engine, Layout, Paths};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/same-script")
}

/// Assamese words, in Assamese's normal form, with their counts.
const ASSAMESE: [(&str, u64); 8] = [
    ("তোমাৰ", 500),
    ("আমাৰ", 400),
    ("প্ৰথম", 300),
    ("কৰা", 200),
    ("কৰি", 150),
    ("হ'ল", 100),
    ("মই", 90),
    ("ঘৰ", 80),
];
/// Bengali words. প্রথম is left out, so the text প্রথম is in no lexicon as written.
const BENGALI: [(&str, u64); 6] = [
    ("তোমার", 500),
    ("আমার", 400),
    ("করা", 200),
    ("বাংলা", 100),
    ("আমি", 90),
    ("ঘর", 80),
];
const ASSAMESE_NEXT: [(&str, &str, u64); 2] = [("আমাৰ", "ঘৰ", 50), ("প্ৰথম", "কৰি", 40)];
const BENGALI_NEXT: [(&str, &str, u64); 1] = [("আমার", "বাংলা", 50)];

/// The pack folder `<id>.neopack` in `dir`: `words` as its lexicon and, keyed by its own
/// tables, its romanised lexicon; `next` as its next words; and the language's `lang.json`.
fn pack(dir: &Path, id: &str, words: &[(&str, u64)], next: &[(&str, &str, u64)]) -> Pack {
    let folder = dir.join(format!("{id}.neopack"));
    fs::create_dir_all(&folder).unwrap();
    for file in ["lang.json", "romanisation.json"] {
        fs::copy(fixtures().join(id).join(file), folder.join(file)).unwrap();
    }
    let rules = script::for_pack("beng", Some(&folder.join("lang.json"))).unwrap();
    for (word, _) in words {
        assert_eq!(rules.normalize(word), *word, "{id}: {word} is not in its normal form");
    }
    let lexicon = build(words.iter().map(|(w, n)| (w.as_bytes().to_vec(), *n))).unwrap();
    fs::write(folder.join("words.fst"), lexicon).unwrap();
    let tables = Romanisation::load(&folder.join("romanisation.json")).unwrap();
    let vocab: Vec<(String, u64)> = words.iter().map(|(w, n)| (w.to_string(), *n)).collect();
    let romanised = manglish::entries(&tables, &vocab);
    assert_eq!(romanised.len(), words.len(), "{id}: every word has a romanised key");
    fs::write(folder.join("romanised.fst"), build(romanised).unwrap()).unwrap();
    let pairs = next.iter().map(|(previous, word, n)| {
        let mut key = previous.as_bytes().to_vec();
        key.push(SEPARATOR);
        key.extend(word.as_bytes());
        (key, *n)
    });
    fs::write(folder.join("next.fst"), build(pairs).unwrap()).unwrap();
    fs::write(
        folder.join("layout.json"),
        r#"{"format": 1, "spaceBar": "x", "languageKey": "x",
            "pages": [{"id": "letters", "label": "x", "rows": [["ক"]]}],
            "vowelSignStrip": ["া", "ি", "ু"]}"#,
    )
    .unwrap();
    let manifest = format!(
        r#"{{"format": 1, "id": "{id}", "version": "1", "name": "{id}", "nativeName": "{id}",
            "script": "beng", "romanised": {{"name": "{id}"}}, "minEngine": 4,
            "files": {{"lexicon": "words.fst", "romanisedLexicon": "romanised.fst",
                       "nextWords": "next.fst", "romanisation": "romanisation.json",
                       "layout": "layout.json", "lang": "lang.json"}}}}"#
    );
    fs::write(folder.join("manifest.json"), manifest).unwrap();
    Pack::open(&folder).unwrap()
}

struct Setup {
    engine: Engine,
    /// Keeps the pack folders.
    _dir: tempfile::TempDir,
}

/// An engine with the two packs loaded in the order `load` and active in the order `active`,
/// without English: every word is one of the two languages'.
fn engine(load: [&str; 2], active: &[&str]) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let packs: Vec<Pack> = load
        .iter()
        .map(|id| match *id {
            "as" => pack(dir.path(), "as", &ASSAMESE, &ASSAMESE_NEXT),
            _ => pack(dir.path(), "bn", &BENGALI, &BENGALI_NEXT),
        })
        .collect();
    let mut engine = Engine::open(&Paths::with_packs(None, &packs, None));
    assert!(engine.pack_problems().is_empty(), "{:?}", engine.pack_problems());
    let ids: Vec<LangId> = active.iter().map(|id| LangId((*id).into())).collect();
    engine.set_active(&ids).unwrap();
    engine.set_max_suggestions(10);
    Setup { engine, _dir: dir }
}

const LOAD_ORDERS: [[&str; 2]; 2] = [["as", "bn"], ["bn", "as"]];
const ACTIVE_ORDERS: [[&str; 2]; 2] = [["as", "bn"], ["bn", "as"]];

/// `f` on an engine of every load order and every active order, named in a failure.
fn every_order(f: impl Fn(&Engine, &str)) {
    for load in LOAD_ORDERS {
        for active in ACTIVE_ORDERS {
            let setup = engine(load, &active);
            f(&setup.engine, &format!("loaded {load:?}, active {active:?}"));
        }
    }
}

/// The candidates as `(text, language id)`.
fn bar(e: &Engine, context: &str, layout: Layout) -> Vec<(String, String)> {
    e.suggest(context, layout, true)
        .candidates
        .into_iter()
        .map(|c| (c.text, e.lang_id(c.lang).map_or("?".into(), |l| l.0)))
        .collect()
}

fn native(e: &Engine, id: &str) -> Layout {
    Layout::Native(e.lang_of_id(id).unwrap())
}

fn has(bar: &[(String, String)], text: &str, lang: &str) -> bool {
    bar.iter().any(|(t, l)| t == text && l == lang)
}

#[test]
fn a_native_layout_completes_from_its_own_pack_by_its_own_rules() {
    every_order(|e, order| {
        // Each layout's pack reads what is typed on it.
        let found = bar(e, "তোমা", native(e, "as"));
        assert!(has(&found, "তোমাৰ", "as"), "{order}: {found:?}");
        assert!(!has(&found, "তোমার", "bn"), "{order}: {found:?}");
        let found = bar(e, "তোমা", native(e, "bn"));
        assert!(has(&found, "তোমার", "bn"), "{order}: {found:?}");
        assert!(!has(&found, "তোমাৰ", "as"), "{order}: {found:?}");
        // Assamese folds a Bengali ra typed on its layout (কর is কৰ), Bengali keeps it.
        let found = bar(e, "কর", native(e, "as"));
        assert!(has(&found, "কৰা", "as") && has(&found, "কৰি", "as"), "{order}: {found:?}");
        let found = bar(e, "কর", native(e, "bn"));
        assert!(has(&found, "করা", "bn"), "{order}: {found:?}");
        assert!(!found.iter().any(|(_, lang)| lang == "as"), "{order}: {found:?}");
    });
}

#[test]
fn the_vowel_signs_after_a_letter_are_the_layouts_packs() {
    every_order(|e, order| {
        // Assamese's lexicon has কৰা and কৰি, Bengali's only করা.
        let signs = |layout| e.suggest("কর", layout, true).vowel_signs;
        assert_eq!(signs(native(e, "as")), ['া', 'ি'], "{order}");
        assert_eq!(signs(native(e, "bn")), ['া'], "{order}");
        // ৰ takes a vowel sign in Assamese; it is no letter of a Bengali word.
        let signs = |layout| e.suggest("কৰ", layout, true).vowel_signs;
        assert_eq!(signs(native(e, "as")), ['া', 'ি'], "{order}");
    });
}

#[test]
fn romanised_words_carry_their_own_language() {
    every_order(|e, order| {
        // Bengali's tables write ো as `o`, Assamese's as `u`.
        for typed in ["tomar", "tumar"] {
            let found = bar(e, typed, Layout::Latin);
            let (word, lang) = if typed == "tomar" { ("তোমার", "bn") } else { ("তোমাৰ", "as") };
            assert!(has(&found, word, lang), "{order} {typed}: {found:?}");
            assert!(
                !has(&found, "তোমার", "as") && !has(&found, "তোমাৰ", "bn"),
                "{order} {typed}: {found:?}"
            );
        }
    });
}

#[test]
fn a_swipe_gives_each_packs_word_as_its_own() {
    every_order(|e, order| {
        let keys = qwerty();
        let path = ideal_path(&KeyMap::new(&keys).unwrap(), "tomar", 4.0);
        let found: Vec<(String, String)> = e
            .swipe(&path, &keys, "", None)
            .into_iter()
            .map(|c| (c.text, e.lang_id(c.lang).unwrap().0))
            .collect();
        assert!(has(&found, "তোমাৰ", "as"), "{order}: {found:?}");
        assert!(has(&found, "তোমার", "bn"), "{order}: {found:?}");
    });
}

#[test]
fn text_before_the_cursor_is_read_by_the_pack_that_holds_it_as_written() {
    every_order(|e, order| {
        // আমার is a Bengali word as written; Assamese would fold it to আমাৰ, its own word.
        let found = bar(e, "আমার ", Layout::Latin);
        assert!(has(&found, "বাংলা", "bn"), "{order}: {found:?}");
        assert!(!has(&found, "ঘৰ", "as"), "{order}: {found:?}");
        // আমাৰ has a letter no Bengali word has.
        let found = bar(e, "আমাৰ ", Layout::Latin);
        assert!(has(&found, "ঘৰ", "as"), "{order}: {found:?}");
        assert!(!has(&found, "বাংলা", "bn"), "{order}: {found:?}");
        // প্রথম is in neither lexicon as written; folded, it is Assamese's প্ৰথম.
        let found = bar(e, "প্রথম ", Layout::Latin);
        assert!(has(&found, "কৰি", "as"), "{order}: {found:?}");
    });
}

#[test]
fn a_word_in_no_lexicon_is_read_by_the_first_active_pack_of_its_script() {
    for load in LOAD_ORDERS {
        // সাগর is in neither lexicon, with or without the fold.
        let first_assamese = engine(load, &["as", "bn"]);
        first_assamese.engine.learn("সাগর");
        assert_eq!(first_assamese.engine.learned_words(), ["সাগৰ"], "loaded {load:?}");
        let first_bengali = engine(load, &["bn", "as"]);
        first_bengali.engine.learn("সাগর");
        assert_eq!(first_bengali.engine.learned_words(), ["সাগর"], "loaded {load:?}");
        // Only one of them active: its rules, though the other is loaded (and loaded last).
        let only_bengali = engine(load, &["bn"]);
        only_bengali.engine.learn("সাগর");
        assert_eq!(only_bengali.engine.learned_words(), ["সাগর"], "loaded {load:?}");
        let only_assamese = engine(load, &["as"]);
        only_assamese.engine.learn("সাগর");
        assert_eq!(only_assamese.engine.learned_words(), ["সাগৰ"], "loaded {load:?}");
    }
}

#[test]
fn a_word_typed_on_a_layout_is_noted_in_that_packs_normal_form() {
    every_order(|e, order| {
        // করা committed on the Assamese layout is Assamese's কৰা, though Bengali holds it as
        // written: it follows আমাৰ from then on.
        for _ in 0..3 {
            e.note_used("আমাৰ ", "করা", native(e, "as"));
        }
        let found = bar(e, "আমাৰ ", Layout::Latin);
        assert!(has(&found, "কৰা", "as"), "{order}: {found:?}");
        assert!(!found.iter().any(|(text, _)| text == "করা"), "{order}: {found:?}");
    });
}

#[test]
fn a_picked_word_is_kept_in_the_normal_form_of_the_language_it_was_offered_as() {
    every_order(|e, order| {
        // প্রথম is not in Bengali's lexicon: picked as a Bengali word, it is Bengali's own
        // word as written, never folded to Assamese's প্ৰথম.
        e.note_choice("prothom", "প্রথম", Some(&LangId("bn".into())));
        let found = bar(e, "prothom", Layout::Latin);
        assert!(has(&found, "প্রথম", "bn"), "{order}: {found:?}");
        assert_eq!(found[0].0, "প্রথম", "{order}: what was chosen leads");
    });
    every_order(|e, order| {
        // A word picked as Assamese's is folded as Assamese folds.
        e.note_choice("sagor", "সাগর", Some(&LangId("as".into())));
        let found = bar(e, "sagor", Layout::Latin);
        assert!(has(&found, "সাগৰ", "as"), "{order}: {found:?}");
        assert!(!found.iter().any(|(text, _)| text == "সাগর"), "{order}: {found:?}");
    });
}

#[test]
fn each_languages_rules_are_found_by_its_id() {
    for load in LOAD_ORDERS {
        let _setup = engine(load, &["as", "bn"]);
        let rules = |id: &str| script::of_language(id).unwrap();
        // অ takes the -ya sign in Bengali (অ্যাপ); in Assamese no sign attaches to a vowel.
        assert_eq!(rules("bn").mark_base("অ"), Some('অ'), "loaded {load:?}");
        assert_eq!(rules("as").mark_base("অ"), None, "loaded {load:?}");
        assert_eq!(rules("as").mark_base("কৰ"), Some('ৰ'), "loaded {load:?}");
        assert_eq!(rules("bn").mark_base("কৰ"), None, "loaded {load:?}");
        assert_eq!(rules("as").normalize("তোমার"), "তোমাৰ", "loaded {load:?}");
        assert_eq!(rules("bn").normalize("তোমার"), "তোমার", "loaded {load:?}");
        assert!(rules("as").inner_apostrophe() && !rules("bn").inner_apostrophe());
    }
}

#[test]
fn one_pack_alone_reads_as_before_whichever_was_opened_before_it() {
    // A Bengali engine after an Assamese one in the same process, and the other way round:
    // neither leaves its rules behind for the next.
    for first in ["as", "bn", "as"] {
        let dir = tempfile::tempdir().unwrap();
        let pack = match first {
            "as" => pack(dir.path(), "as", &ASSAMESE, &ASSAMESE_NEXT),
            _ => pack(dir.path(), "bn", &BENGALI, &BENGALI_NEXT),
        };
        let mut e = Engine::open(&Paths::with_packs(None, &[pack], None));
        e.set_active(&[LangId(first.into())]).unwrap();
        e.learn("সাগর");
        let want = if first == "as" { "সাগৰ" } else { "সাগর" };
        assert_eq!(e.learned_words(), [want], "{first}");
    }
}

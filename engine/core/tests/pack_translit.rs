//! Each pack's transliterator spells in the pack's own script. The models were trained with
//! the script's block as their target vocabulary (`training/mlkb/translit/vocab.py::native`),
//! so the Hindi model's tokens are Devanagari and the Assamese one's Bengali-Assamese (with the
//! apostrophe of হ'ল): read as Malayalam, the model can never match the pack's lexicon, and
//! its free spelling comes out in the wrong script.

use std::path::{Path, PathBuf};

use mlengine_core::lexicon::Lexicon;
use mlengine_core::pack::Pack;
use mlengine_core::script;
use mlengine_core::{Engine, Kind, Layout, Paths};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// English and the pack `id`, with its transliterator or without it.
fn engine(id: &str, model: bool) -> Engine {
    let pack = Pack::open(&root().join(format!("packs/{id}.neopack"))).unwrap();
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    let mut paths = Paths::with_packs(Some(en), &[pack], None);
    if !model {
        paths.packs[0].translit = None;
    }
    Engine::open(&paths)
}

/// The pack's script: its block, ZWJ and ZWNJ, and the apostrophe where words carry one.
fn in_script(id: &str, word: &str) -> bool {
    let script = Pack::open(&root().join(format!("packs/{id}.neopack")))
        .unwrap()
        .manifest
        .script;
    let rules = script::rules(&script).unwrap();
    word.chars().all(|c| {
        rules.block().contains(&c)
            || c == '\u{200D}'
            || c == '\u{200C}'
            || (c == '\'' && rules.inner_apostrophe())
    })
}

/// The romanised candidates for `typed` on the Latin layout.
fn romanised(e: &Engine, typed: &str) -> Vec<String> {
    e.suggest(typed, Layout::Latin, true)
        .candidates
        .into_iter()
        .filter(|c| c.kind == Kind::Romanised)
        .map(|c| c.text)
        .collect()
}

#[test]
fn every_pack_loads_its_transliterator() {
    for id in ["ml", "hi", "as"] {
        let e = engine(id, true);
        assert!(
            e.loaded_sources().contains(&format!("{id}:translit")),
            "{id}: {:?}",
            e.loaded_sources()
        );
    }
}

/// Words off the lexicon (names, long compounds, letters typed at random) get only the model's
/// free spelling, in the side slot: in the pack's script.
#[test]
fn free_spellings_are_in_the_pack_script() {
    for id in ["hi", "as"] {
        let e = engine(id, true);
        let mut guessed = 0;
        for typed in [
            "jhumritalaiya",
            "kshitijavardhan",
            "brindavanam",
            "qwrtpl",
            "kolkatar",
        ] {
            let got = romanised(&e, typed);
            eprintln!("{id} {typed}: {got:?}");
            for w in &got {
                assert!(
                    in_script(id, w),
                    "{id} {typed}: {w} is not in the pack's script"
                );
            }
            guessed += usize::from(!got.is_empty());
        }
        assert!(guessed >= 4, "{id}: {guessed} of 5 words got a spelling");
    }
}

/// The rule-based spelling is Malayalam's only, so without its transliterator a pack's word
/// off the lexicon gets nothing in its script: the Assamese model, whose vocabulary has the
/// apostrophe (134 tokens), must load for `kolkatar` to get one.
#[test]
fn an_assamese_word_off_the_lexicon_gets_a_spelling() {
    assert_eq!(
        romanised(&engine("as", false), "kolkatar"),
        Vec::<String>::new()
    );
    let got = romanised(&engine("as", true), "kolkatar");
    assert!(!got.is_empty(), "kolkatar: no Assamese spelling");
    assert!(got.iter().all(|w| in_script("as", w)), "{got:?}");
}

/// The model held to the lexicon adds words the skeleton lookup alone doesn't offer: over the
/// first rows of the pack's human transliteration set, more targets are in the bar with it.
#[test]
fn the_model_finds_lexicon_words() {
    for id in ["hi", "as"] {
        let path = root().join(format!("data/eval/{id}/translit-human.tsv"));
        let rows: Vec<(String, String)> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
            .take(300)
            .map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                (f[1].to_string(), f[2].to_string())
            })
            .collect();
        let hits = |e: &Engine| {
            rows.iter()
                .filter(|(latin, want)| {
                    e.suggest(latin, Layout::Latin, true)
                        .candidates
                        .iter()
                        .take(3)
                        .any(|c| &c.text == want)
                })
                .count()
        };
        let without = hits(&engine(id, false));
        let with = hits(&engine(id, true));
        eprintln!(
            "{id}: top-3 {without} without the model, {with} with it, of {}",
            rows.len()
        );
        assert!(
            with >= without + rows.len() / 50,
            "{id}: {with} with the model, {without} without"
        );
    }
}

/// A lexicon word the model reads far behind its best lexicon reading doesn't lead it, however
/// common it is (`rank::MANGLISH_MODEL_TRAILS`): the constrained beam fills its width with
/// what the lexicon lets it reach, a shorter word the letters begin (पाठशाला for
/// `pathshalayen`) or a commoner inflection (देनेवाले for `denewaali`).
#[test]
fn a_reading_far_behind_the_models_best_does_not_lead_it() {
    let hi = engine("hi", true);
    for (typed, want) in [("denewaali", "देनेवाली"), ("pathshalayen", "पाठशालाएं")] {
        let got = romanised(&hi, typed);
        assert_eq!(got.first().map(String::as_str), Some(want), "{typed}: {got:?}");
    }
    let assamese = engine("as", true);
    for (typed, want) in [("kokrajharat", "কোকৰাঝাৰত"), ("bangladexeer", "বাংলাদেশীৰ")] {
        let got = romanised(&assamese, typed);
        assert_eq!(got.first().map(String::as_str), Some(want), "{typed}: {got:?}");
    }
}

/// Readings the model finds about as good are still ranked by how common the words are:
/// गलगण्ड, 0.17 per token behind गलगंड for `galgand`, is the commoner spelling and leads.
#[test]
fn close_readings_are_ranked_by_frequency() {
    let got = romanised(&engine("hi", true), "galgand");
    assert_eq!(got.first().map(String::as_str), Some("गलगण्ड"), "{got:?}");
}

/// A word the lexicon doesn't have gets the model's free spelling beside the rare lexicon
/// words the letters loosely fit, when the model prefers it to every lexicon reading: second
/// among the native words, never first. প্ৰাপ্তিতকৈ (`praptitkoi`) is not in the Assamese
/// lexicon; প্ৰাপ্তিত is, and was the only word offered.
#[test]
fn the_free_spelling_is_offered_beside_rare_lexicon_words() {
    let e = engine("as", true);
    for (typed, lexicon_word, free) in [
        ("praptitkoi", "প্ৰাপ্তিত", "প্ৰাপ্তিতকৈ"),
        ("owosthitire", "অৱস্থিতিৰ", "অৱস্থিতিৰে"),
    ] {
        let got = romanised(&e, typed);
        assert_eq!(got, [lexicon_word, free], "{typed}");
    }
}

/// The free spelling stays out where the model prefers a lexicon word to it: every Assamese
/// word offered for `biswabidyaloy` (বিশ্ববিদ্যালয়) is a lexicon word, with no near-duplicate
/// spelled beside it.
#[test]
fn no_free_spelling_where_the_model_prefers_a_lexicon_word() {
    let pack = Pack::open(&root().join("packs/as.neopack")).unwrap();
    let lexicon = Lexicon::open(&pack.file("lexicon").unwrap()).unwrap();
    let got = romanised(&engine("as", true), "biswabidyaloy");
    assert!(!got.is_empty(), "biswabidyaloy: nothing offered");
    for w in &got {
        assert!(
            lexicon.get(w.as_bytes()).is_some(),
            "{w} is not a lexicon word: {got:?}"
        );
    }
}

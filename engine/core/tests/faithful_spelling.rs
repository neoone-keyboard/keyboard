//! A Manglish word the Malayalam lexicon doesn't hold is still offered as typed: when the
//! transliterator spells the letters better off the lexicon than as any lexicon word, its
//! spelling is in the bar beside the rare words the letters loosely fit. `thookkal` (തൂക്കൽ,
//! hanging or weighing; three times in the Wikipedia text, not in `ml.fst`) had only
//! തൂക്കിൽ, തുകൽ and ടുക്കൽ, though the model reads തൂക്കൽ at −0.20 per token and തൂക്കിൽ at
//! −0.90.

use std::path::{Path, PathBuf};

use mlengine_core::lexicon::Lexicon;
use mlengine_core::nn::translit::{Search, Translit};
use mlengine_core::nn::vocab::Target;
use mlengine_core::pack::Pack;
use mlengine_core::{Engine, Layout, Paths};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// English and Malayalam, as the keyboard opens them.
fn engine() -> Engine {
    engine_with(&["ml"])
}

/// English and the packs `ids`, in that load order.
fn engine_with(ids: &[&str]) -> Engine {
    let packs: Vec<Pack> = ids
        .iter()
        .map(|id| Pack::open(&root().join(format!("packs/{id}.neopack"))).unwrap())
        .collect();
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    Engine::open(&Paths::with_packs(Some(en), &packs, None))
}

/// The bar's first three words for `context`.
fn bar(e: &Engine, context: &str) -> Vec<String> {
    let s = e.suggest(context, Layout::Latin, true);
    let words: Vec<String> = s.candidates.iter().take(3).map(|c| c.text.clone()).collect();
    eprintln!("{context:?}: {words:?}");
    words
}

#[test]
fn thookkal_offers_its_own_spelling() {
    let e = engine();
    for context in ["thookkal", "Thookkal"] {
        assert!(
            bar(&e, context).iter().any(|w| w == "തൂക്കൽ"),
            "{context:?} has no തൂക്കൽ in its first three"
        );
    }
}

/// Every word here is off the lexicon and the model's own spelling of it leads every lexicon
/// reading (checked first, so the case stays one of those): it is in the first three.
#[test]
fn a_leading_spelling_off_the_lexicon_is_offered() {
    let pack = root().join("packs/ml.neopack");
    let model = Translit::open(&pack.join("translit.mlkbnn"), Target::MALAYALAM).unwrap();
    let lexicon = Lexicon::open(&pack.join("ml.fst")).unwrap();
    let e = engine();
    for (typed, want) in [
        ("thookkal", "തൂക്കൽ"),
        ("nagararaashtrangalkk", "നഗരരാഷ്ട്രങ്ങൾക്ക്"),
    ] {
        assert!(lexicon.get(want.as_bytes()).is_none(), "{want} is a lexicon word");
        let search = Search {
            width: 4,
            free: true,
            deadline: None,
            own: &[],
        };
        let read = model.search(typed, &lexicon, &[], &search).0;
        assert_eq!(
            read.first().map(|t| (t.text.as_str(), t.in_lexicon)),
            Some((want, false)),
            "the model's first reading of {typed}"
        );
        assert!(
            bar(&e, typed).iter().any(|w| w == want),
            "{typed}: {want} not in the first three"
        );
    }
}

/// The same after a sentence's end, after an English word and after a Malayalam word, with
/// Malayalam alone and with Hindi and Assamese beside it. After `Ok.` the capital kept the
/// distance penalty on തൂക്കിൽ and the LM put തുകൽ over it, and തൂക്കൽ, held just under
/// തൂക്കിൽ, went with it to fourth. After എന്റെ the context lifted തൂക്കിൽ and തുകൽ over the
/// floor of words that may replace, and the model's free spelling was never read beside them.
#[test]
fn thookkal_offers_its_own_spelling_in_every_context() {
    for packs in [&["ml"][..], &["ml", "hi", "as"]] {
        let e = engine_with(packs);
        for context in [
            "Ok. Thookkal",
            "ok. thookkal",
            "ok thookkal",
            "Ok thookkal",
            "ente thookkal",
            "Ente thookkal",
            "എന്റെ thookkal",
            "ente veettil thookkal",
        ] {
            assert!(
                bar(&e, context).iter().any(|w| w == "തൂക്കൽ"),
                "{packs:?}, {context:?}: no തൂക്കൽ in the first three"
            );
        }
    }
}

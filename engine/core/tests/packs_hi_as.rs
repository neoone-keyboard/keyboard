//! The Hindi and Assamese packs (packs/hi.neopack, packs/as.neopack) loaded beside English and
//! Malayalam, as the keyboard would open them: romanised typing on the Latin layout, native
//! typing on a pack's own layout, the active list, and the manifests' English fingerprint.

use std::path::{Path, PathBuf};

use mlengine_core::lang::{ActiveError, LangId};
use mlengine_core::lexicon::Lexicon;
use mlengine_core::pack::Pack;
use mlengine_core::{Engine, Kind, Layout, Paths, Suggestions};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn en_fst() -> PathBuf {
    root().join("apps/ios/KeyboardExtension/Lexicon/en.fst")
}

fn pack(id: &str) -> Pack {
    Pack::open(&root().join(format!("packs/{id}.neopack"))).unwrap()
}

/// English and the packs `ids`, in that load order.
fn engine(ids: &[&str]) -> Engine {
    let packs: Vec<Pack> = ids.iter().map(|id| pack(id)).collect();
    Engine::open(&Paths::with_packs(Some(en_fst()), &packs, None))
}

fn ids(list: &[&str]) -> Vec<LangId> {
    list.iter().map(|id| LangId((*id).into())).collect()
}

fn show(e: &Engine, s: &Suggestions) -> String {
    s.candidates
        .iter()
        .map(|c| {
            format!(
                "{} [{:?} {}]",
                c.text,
                c.kind,
                e.lang_id(c.lang).map_or("?".into(), |l| l.0)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn suggest(e: &Engine, context: &str, layout: Layout) -> Suggestions {
    let s = e.suggest(context, layout, false);
    eprintln!("{context:?} {layout:?}: {}", show(e, &s));
    s
}

fn texts(s: &Suggestions) -> Vec<&str> {
    s.candidates.iter().map(|c| c.text.as_str()).collect()
}

/// The candidate `text`'s language id, if offered.
fn lang_of(e: &Engine, s: &Suggestions, text: &str) -> Option<String> {
    s.candidates
        .iter()
        .find(|c| c.text == text)
        .and_then(|c| e.lang_id(c.lang))
        .map(|l| l.0)
}

#[test]
fn hindi_loads_with_every_role_it_has() {
    let e = engine(&["hi"]);
    let loaded = e.loaded_sources();
    for part in [
        "english",
        "hi:lexicon",
        "hi:romanised",
        "hi:next_words",
        "hi:translit",
    ] {
        assert!(loaded.iter().any(|s| s == part), "{part} in {loaded:?}");
    }
    assert_eq!(e.active_languages(), ids(&["en", "hi"]));
    let hi = e.lang_of_id("hi").unwrap();
    assert_eq!(e.layouts(), [Layout::Latin, Layout::Native(hi)]);
}

#[test]
fn hinglish_on_the_latin_layout() {
    let e = engine(&["hi"]);
    for (typed, want) in [("kya", "क्या"), ("samjhna", "समझना"), ("nahi", "नहीं")]
    {
        let s = suggest(&e, typed, Layout::Latin);
        assert!(texts(&s).contains(&want), "{typed}: {}", show(&e, &s));
        assert_eq!(lang_of(&e, &s, want).as_deref(), Some("hi"));
    }
}

#[test]
fn devanagari_on_the_native_layout_completes_words() {
    let e = engine(&["hi"]);
    let hi = Layout::Native(e.lang_of_id("hi").unwrap());
    let s = suggest(&e, "नम", hi);
    assert_eq!(s.typed, "नम");
    assert!(
        s.candidates
            .iter()
            .any(|c| c.kind == Kind::Completion && c.text.starts_with("नम") && c.text != "नम"),
        "{}",
        show(&e, &s)
    );
    let s = suggest(&e, "क्य", hi);
    assert!(texts(&s).contains(&"क्या"), "{}", show(&e, &s));
    // A virama mid-word keeps the word whole.
    let s = suggest(&e, "मैं समझ", hi);
    assert_eq!(s.typed, "समझ");
    // Next words after a Hindi word and a space. A danda ends the word (and the phrase, so
    // no pair is learned across it: `text::phrase_words`).
    let s = suggest(&e, "क्या ", hi);
    assert!(
        s.candidates.iter().any(|c| c.kind == Kind::NextWord),
        "next words after क्या"
    );
    let s = suggest(&e, "नमस्ते।", hi);
    assert_eq!(s.typed, "");
}

#[test]
fn hindi_alone_offers_no_english() {
    let mut e = engine(&["hi"]);
    e.set_active(&ids(&["hi"])).unwrap();
    for typed in ["kya", "the", "hello", "samjhna"] {
        let s = suggest(&e, typed, Layout::Latin);
        assert!(
            s.candidates
                .iter()
                .all(|c| e.lang_id(c.lang).unwrap().0 == "hi"),
            "{typed}: {}",
            show(&e, &s)
        );
    }
}

#[test]
fn malayalam_and_hindi_without_english() {
    let mut e = engine(&["ml", "hi"]);
    e.set_active(&ids(&["ml", "hi"])).unwrap();
    let (ml, hi) = (e.lang_of_id("ml").unwrap(), e.lang_of_id("hi").unwrap());
    assert_eq!(
        e.layouts(),
        [Layout::Latin, Layout::Native(ml), Layout::Native(hi)]
    );
    let s = suggest(&e, "kya", Layout::Latin);
    assert_eq!(
        lang_of(&e, &s, "क्या").as_deref(),
        Some("hi"),
        "{}",
        show(&e, &s)
    );
    let s = suggest(&e, "ente", Layout::Latin);
    assert_eq!(
        lang_of(&e, &s, "എന്റെ").as_deref(),
        Some("ml"),
        "{}",
        show(&e, &s)
    );
    for typed in ["nahi", "samjhna", "veedu", "the"] {
        let s = suggest(&e, typed, Layout::Latin);
        assert!(
            s.candidates
                .iter()
                .all(|c| matches!(e.lang_id(c.lang).unwrap().0.as_str(), "ml" | "hi")),
            "{typed}: {}",
            show(&e, &s)
        );
    }
    // Each native layout types its own script.
    let s = suggest(&e, "क्य", Layout::Native(hi));
    assert_eq!(lang_of(&e, &s, "क्या").as_deref(), Some("hi"));
    let s = suggest(&e, "എന്റ", Layout::Native(ml));
    assert_eq!(lang_of(&e, &s, "എന്റെ").as_deref(), Some("ml"));
}

#[test]
fn three_languages_are_refused() {
    let mut e = engine(&["ml", "hi"]);
    assert_eq!(
        e.set_active(&ids(&["en", "ml", "hi"])),
        Err(ActiveError::TooMany(3))
    );
    assert_eq!(e.active_languages(), ids(&["en", "ml"]));
}

/// Common Assamese words, romanised as Aksharantar's human test pairs spell them
/// (data/eval/as/translit-human.tsv), and words with the apostrophe.
#[test]
fn assamese_on_the_latin_layout() {
    let e = engine(&["as"]);
    let mut found = 0;
    let pairs = [
        ("moi", "মই"),
        ("ami", "আমি"),
        ("tumi", "তুমি"),
        ("asom", "অসম"),
        ("oxom", "অসম"),
        ("manuh", "মানুহ"),
        ("bhal", "ভাল"),
        ("kotha", "কথা"),
        ("kori", "কৰি"),
        ("ghor", "ঘৰ"),
        ("hol", "হ'ল"),
        ("kot", "ক'ত"),
    ];
    for (typed, want) in pairs {
        let s = suggest(&e, typed, Layout::Latin);
        if texts(&s).contains(&want) {
            found += 1;
            assert_eq!(lang_of(&e, &s, want).as_deref(), Some("as"));
        } else {
            eprintln!("  missed {want} for {typed}");
        }
    }
    eprintln!("as: {found} of {} offered", pairs.len());
    assert!(found >= pairs.len() / 2, "{found} of {}", pairs.len());
}

#[test]
fn assamese_words_with_an_apostrophe_are_native_words() {
    let e = engine(&["as"]);
    let as_ = Layout::Native(e.lang_of_id("as").unwrap());
    let s = suggest(&e, "মই হ'", as_);
    assert_eq!(s.typed, "হ'");
    let s = suggest(&e, "তাই হ'ল ", as_);
    assert!(!s.candidates.is_empty(), "next words after হ'ল");
}

/// A pack's `englishFingerprint` names the `en.fst` its next words were built against; the
/// committed packs must match the committed bundle.
#[test]
fn english_fingerprints_match_the_bundle() {
    let en = Lexicon::open(&en_fst()).unwrap();
    let hex: String = en
        .fingerprint()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    for id in ["ml", "hi", "as"] {
        if let Some(fp) = pack(id).manifest.english_fingerprint {
            assert_eq!(fp, hex, "{id}");
        }
    }
}

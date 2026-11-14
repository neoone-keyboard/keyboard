//! A pack's ranking choices (`manifest.json`'s `ranking`, `pack::Ranking`) are its own data:
//! a pack that sets one is ranked as Malayalam is for that behaviour, and one that doesn't as
//! Hindi is, whatever its language. Each test runs a committed pack as shipped beside a copy
//! whose manifest sets or clears one choice (the copy's other files are the pack's own).

use std::path::{Path, PathBuf};

use mlengine_core::lang::LangId;
use mlengine_core::pack::Pack;
use mlengine_core::{Engine, Layout, Paths};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A copy of `packs/<id>.neopack` with its manifest's `ranking.<field>` set to `on` (each
/// other file linked to the pack's own).
fn with_choice(id: &str, field: &str, on: bool) -> (tempfile::TempDir, Pack) {
    let from = root().join(format!("packs/{id}.neopack"));
    let dir = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(&from).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap();
        if name != "manifest.json" {
            std::os::unix::fs::symlink(&path, dir.path().join(name)).unwrap();
        }
    }
    let text = std::fs::read_to_string(from.join("manifest.json")).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_str(&text).unwrap();
    if !manifest["ranking"].is_object() {
        manifest["ranking"] = serde_json::json!({});
    }
    manifest["ranking"][field] = on.into();
    std::fs::write(dir.path().join("manifest.json"), manifest.to_string()).unwrap();
    let pack = Pack::open(dir.path()).unwrap();
    (dir, pack)
}

fn shipped(id: &str) -> Pack {
    Pack::open(&root().join(format!("packs/{id}.neopack"))).unwrap()
}

/// English and `packs`, all active, in that order.
fn engine(packs: &[Pack]) -> Engine {
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    let mut e = Engine::open(&Paths::with_packs(Some(en), packs, None));
    let ids: Vec<LangId> = std::iter::once("en".to_string())
        .chain(packs.iter().map(|p| p.manifest.id.clone()))
        .map(LangId)
        .collect();
    e.set_active(&ids).unwrap();
    e
}

/// The bar's first three for `context`, typed.
fn bar(e: &Engine, context: &str) -> Vec<String> {
    let s = e.suggest(context, Layout::Latin, true);
    let words: Vec<String> = s.candidates.iter().take(3).map(|c| c.text.clone()).collect();
    eprintln!("{context:?}: {words:?}");
    words
}

/// The bar's first word for `context`.
fn middle(e: &Engine, context: &str) -> String {
    bar(e, context).swap_remove(0)
}

/// Malayalam as shipped, and a copy with `field` off: the copy ranks that behaviour as Hindi.
fn malayalam_without(field: &str) -> (Engine, Engine, tempfile::TempDir) {
    let (dir, copy) = with_choice("ml", field, false);
    (engine(&[shipped("ml")]), engine(&[copy]), dir)
}

/// Hindi as shipped, and a copy with `field` on: the copy ranks that behaviour as Malayalam.
fn hindi_with(field: &str) -> (Engine, Engine, tempfile::TempDir) {
    let (dir, copy) = with_choice("hi", field, true);
    (engine(&[shipped("hi")]), engine(&[copy]), dir)
}

#[test]
fn malayalam_sets_every_choice_and_hindi_none() {
    assert!(shipped("ml").manifest.ranking.fields().iter().all(|(_, on)| *on));
    for id in ["hi", "as", "ta", "ur"] {
        assert!(
            shipped(id).manifest.ranking.fields().iter().all(|(_, on)| !*on),
            "{id} sets a ranking choice"
        );
    }
}

/// `startBelowEnglish`: at the start of a message the pack's words start a step behind
/// English, so a word typed in Latin letters that the pack reads only loosely stays as typed.
#[test]
fn start_below_english_keeps_the_typed_word_first_at_the_start() {
    let (ml, ml_off, _dir) = malayalam_without("startBelowEnglish");
    assert_eq!(middle(&ml, "athbuthamaanu"), "athbuthamaanu");
    assert_eq!(middle(&ml_off, "athbuthamaanu"), "അത്ഭുതമാണ്");
    let (hi, hi_on, _dir) = hindi_with("startBelowEnglish");
    assert_eq!(middle(&hi, "angraji"), "अंग्रजी");
    assert_eq!(middle(&hi_on, "angraji"), "angraji");
    // After a word the choice says nothing: the previous word sets the prior.
    assert_eq!(bar(&hi, "mera angraji"), bar(&hi_on, "mera angraji"));
}

/// Each pack starts at its own prior: Malayalam's choice no longer puts Hindi a step behind
/// English at the start, so a Hindi name gets what it gets without Malayalam. Malayalam's own
/// start is unchanged.
#[test]
fn another_packs_start_prior_does_not_move_hindi() {
    let en_hi = engine(&[shipped("hi")]);
    let en_ml_hi = engine(&[shipped("ml"), shipped("hi")]);
    for typed in ["reyansh", "Reyansh"] {
        let alone = bar(&en_hi, typed);
        assert_eq!(alone[0], "रेयांश", "{typed} with English and Hindi");
        assert_eq!(middle(&en_ml_hi, typed), "रेयांश", "{typed} with Malayalam active too");
    }
    // Malayalam's own word at the start is still a step behind English.
    assert_eq!(middle(&en_ml_hi, "athbuthamaanu"), "athbuthamaanu");
}

/// `readingsByFrequency`: the transliterator's readings are ordered by frequency, however far
/// behind its best the model reads them; without it, a far reading trails the best one.
#[test]
fn readings_by_frequency_lets_a_common_far_reading_lead() {
    let (ml, ml_off, _dir) = malayalam_without("readingsByFrequency");
    assert_eq!(middle(&ml, "ramesh"), "രമേശ്");
    assert_eq!(middle(&ml_off, "ramesh"), "രമേഷ്");
    let (hi, hi_on, _dir) = hindi_with("readingsByFrequency");
    assert_eq!(middle(&hi, "avsthayen"), "अवस्थाएं");
    assert_eq!(middle(&hi_on, "avsthayen"), "अवस्थाएँ");
}

/// `keepLexiconBeam`: the lexicon search keeps all its rows beside the free spelling's.
#[test]
fn keep_lexicon_beam_keeps_a_reading_the_free_spelling_would_take() {
    let (ml, ml_off, _dir) = malayalam_without("keepLexiconBeam");
    assert_eq!(middle(&ml, "sheri"), "ശരി");
    assert!(!bar(&ml_off, "sheri").iter().any(|w| w == "ശരി"));
    let (hi, hi_on, _dir) = hindi_with("keepLexiconBeam");
    assert_eq!(middle(&hi, "agusta"), "Augusta");
    assert_eq!(middle(&hi_on, "agusta"), "अगस्त");
}

/// `liftedWordsStayRare`: a rare word the previous word lifted is still rare, and the
/// transliterator's own spelling is offered beside it (തൂക്കൽ after എന്റെ, off the lexicon).
#[test]
fn lifted_words_stay_rare_offers_the_own_spelling_after_context() {
    let (ml, ml_off, _dir) = malayalam_without("liftedWordsStayRare");
    for context in ["ente thookkal", "എന്റെ thookkal"] {
        assert!(bar(&ml, context).iter().any(|w| w == "തൂക്കൽ"), "{context}");
        assert!(!bar(&ml_off, context).iter().any(|w| w == "തൂക്കൽ"), "{context}");
    }
    // At the start nothing is lifted: the same either way.
    assert_eq!(bar(&ml, "thookkal"), bar(&ml_off, "thookkal"));
}

/// `holdOwnSpelling`: the transliterator's own spelling beside rare words is held under the
/// pack's best word and a likely English word, where without it it keeps its place.
#[test]
fn hold_own_spelling_keeps_the_own_spelling_under_the_words_found() {
    let (ml, ml_off, _dir) = malayalam_without("holdOwnSpelling");
    // Third, ജെറി: `jersi` less its `s`, a slip (the typo search, `Engine::add_slips`).
    assert_eq!(bar(&ml, "jersi"), ["jersi", "ജെർസി", "ജെറി"]);
    assert_eq!(bar(&ml_off, "jersi"), ["jersi", "ജെർസി", "ജേർസി"]);
    let (hi, hi_on, _dir) = hindi_with("holdOwnSpelling");
    assert_eq!(bar(&hi, "aryama"), ["aryama", "अर्यमा", "आर्यम"]);
    // Third, आर्या: `aryama` less its `m`, a slip (the typo search).
    assert_eq!(bar(&hi_on, "aryama"), ["aryama", "अर्यमा", "आर्या"]);
}

/// `swipeIgnoresEnglishMargin`: a swiped word of the pack competes with a common English word
/// on its score alone. Assamese without it keeps `hello` first over হেলা after আমি (the path
/// fits both as well); with it, as Malayalam's words would, হেলা takes most of them.
#[test]
fn swipe_ignores_english_margin_lets_a_loose_pack_word_beat_english() {
    use mlengine_core::lang::EN;
    use mlengine_core::swipe::{KeyMap, qwerty};
    let map = KeyMap::new(&qwerty()).unwrap();
    let natives = |e: &Engine| {
        let mut rng = paths::Rng(paths::seed("hello"));
        (0..20)
            .filter(|_| {
                let path = paths::noisy_path(&map, "hello", &mut rng, paths::NOISE);
                let first = e.swipe(&path, &qwerty(), "আমি ", None).into_iter().next();
                first.is_some_and(|c| c.lang != EN)
            })
            .count()
    };
    let (_dir, on) = with_choice("as", "swipeIgnoresEnglishMargin", true);
    let (held, unheld) = (natives(&engine(&[shipped("as")])), natives(&engine(&[on])));
    eprintln!("hello after আমি: {held} of 20 to Assamese held, {unheld} unheld");
    assert!(held <= 6 && unheld >= held + 6, "{held} held, {unheld} unheld");
}

mod paths;

const FIELDS: [&str; 6] = [
    "startBelowEnglish",
    "swipeIgnoresEnglishMargin",
    "readingsByFrequency",
    "keepLexiconBeam",
    "liftedWordsStayRare",
    "holdOwnSpelling",
];

#[test]
#[ignore = "exploration: inputs whose bar one ranking choice changes, for each choice and pack"]
fn print_inputs_each_choice_changes() {
    let sets = [
        ("ml", root().join("data/eval/chat-golden.tsv")),
        ("ml", root().join("data/eval/translit-human.tsv")),
        ("hi", root().join("data/eval/hi/translit-human.tsv")),
    ];
    for field in FIELDS {
        for (id, set) in &sets {
            let base = engine(&[shipped(id)]);
            let flipped = !shipped(id).manifest.ranking.fields().iter().any(|(f, on)| *f == field && *on);
            let (_dir, copy) = with_choice(id, field, flipped);
            let other = engine(&[copy]);
            let text = std::fs::read_to_string(set).unwrap();
            let mut shown = 0;
            for line in text.lines().filter(|l| !l.starts_with('#')) {
                let f: Vec<&str> = line.split('\t').collect();
                if f.len() < 3 {
                    continue;
                }
                let before = if f[0].is_empty() { f[1].to_string() } else { format!("{} {}", f[0], f[1]) };
                let (a, b) = (bar(&base, &before), bar(&other, &before));
                if a != b && shown < 8 {
                    println!("{field} {id} {flipped}: {before:?} want {}: {a:?} -> {b:?}", f[2]);
                    shown += 1;
                }
            }
        }
    }
}

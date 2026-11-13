//! The Hindi pack with its LM (docs/design/hindi-lm.md), opened through
//! the pack's manifest as the keyboard opens it, with English from the bundle.
//!
//! The LM is MLENGINE_HI_LM, else `packs/hi.neopack/lm.mlkbnn`, else the training output
//! `training/build/lm-hi.mlkbnn`; every test is skipped without one. `hindi_latency` is the
//! suggestion latency gate (p99 under 8 ms on the Mac, release build: `make bench-hi`).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mlengine_core::lang::Lang;
use mlengine_core::pack::Pack;
use mlengine_core::{Engine, Kind, Layout, Paths, Timing};

/// The Hindi pack's native layout: it is the only pack loaded.
const HI: Layout = Layout::Native(Lang(1));

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn lm_path() -> Option<PathBuf> {
    let path = std::env::var_os("MLENGINE_HI_LM")
        .map(PathBuf::from)
        .into_iter()
        .chain([
            root().join("packs/hi.neopack/lm.mlkbnn"),
            root().join("training/build/lm-hi.mlkbnn"),
        ])
        .find(|p| p.exists());
    if path.is_none() {
        println!("skipped: no Hindi lm.mlkbnn (`make lm-data lm LM_LANG=hi` builds it)");
    }
    path
}

/// English and the Hindi pack, with `lm` in place of the pack's own (None: no LM).
fn engine(lm: Option<PathBuf>) -> Engine {
    let pack = Pack::open(&root().join("packs/hi.neopack")).expect("the Hindi pack");
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    let mut paths = Paths::with_packs(Some(en), &[pack], None);
    paths.packs[0].lm = lm;
    Engine::open(&paths)
}

fn devanagari(w: &str) -> bool {
    w.chars().any(|c| ('\u{0900}'..='\u{097F}').contains(&c))
}

fn next_words(engine: &Engine, context: &str, layout: Layout) -> Vec<String> {
    engine
        .suggest(context, layout, true)
        .candidates
        .into_iter()
        .filter(|c| c.kind == Kind::NextWord)
        .map(|c| c.text)
        .collect()
}

#[test]
fn the_hindi_lm_loads_with_the_pack() {
    let Some(lm) = lm_path() else { return };
    let engine = engine(Some(lm));
    assert!(
        engine.loaded_sources().contains(&"hi:lm".to_string()),
        "{:?}",
        engine.loaded_sources()
    );
}

/// Its native pieces are Devanagari (`Lm::set_native`): after Hindi, in either script, the
/// next words are Hindi; after English, English.
#[test]
fn hindi_next_words_follow_the_sentence() {
    let Some(lm) = lm_path() else { return };
    let engine = engine(Some(lm));
    for (context, layout) in [("मैं घर ", HI), ("main ghar ", Layout::Latin), ("मुझे ", HI)]
    {
        let got = next_words(&engine, context, layout);
        println!("{context:?}: {got:?}");
        assert!(
            !got.is_empty() && got.iter().all(|w| devanagari(w)),
            "{context:?}: {got:?}"
        );
    }
    let english = next_words(&engine, "I want to ", Layout::Latin);
    println!("\"I want to \": {english:?}");
    assert!(
        !english.is_empty() && !english.iter().any(|w| devanagari(w)),
        "{english:?}"
    );
}

/// Words typed letter by letter, then a space: English after "I said ", Hinglish, and Hindi on
/// the native layout.
fn inputs() -> Vec<(String, Layout)> {
    let mut out = Vec::new();
    for w in ["thanks", "tomorrow", "because"] {
        for end in 1..=w.len() {
            out.push((format!("I said {}", &w[..end]), Layout::Latin));
        }
        out.push((format!("I said {w} "), Layout::Latin));
    }
    for w in ["kya", "kaise", "hoga", "samajh", "zindagi", "bahut"] {
        for end in 1..=w.len() {
            out.push((format!("main {}", &w[..end]), Layout::Latin));
        }
        out.push((format!("main {w} "), Layout::Latin));
    }
    for w in ["मैं", "आज", "घर", "जाऊंगा", "क्योंकि", "बारिश"]
    {
        let chars: Vec<char> = w.chars().collect();
        for end in 1..=chars.len() {
            out.push((chars[..end].iter().collect(), HI));
        }
        out.push((format!("{w} "), HI));
    }
    out
}

fn report(label: &str, mut samples: Vec<(Duration, &str)>) -> Duration {
    samples.sort();
    let p50 = samples[samples.len() / 2].0;
    let p99 = samples[samples.len() * 99 / 100];
    let worst = samples[samples.len() - 1];
    println!(
        "{label}: {} requests, p50 {p50:?}, p99 {:?} ({:?}), max {:?} ({:?})",
        samples.len(),
        p99.0,
        p99.1,
        worst.0,
        worst.1
    );
    p99.0
}

/// Every input 20 times in order (the LM context moves as when typing); the p99 must stay
/// under 8 ms.
fn time(label: &str, engine: &Engine, inputs: &[(String, Layout)], suggest: &dyn Fn(&str, Layout)) {
    println!("{label}: sources {:?}", engine.loaded_sources());
    for (context, layout) in inputs {
        suggest(context, *layout);
    }
    let mut samples = Vec::new();
    for _ in 0..20 {
        for (context, layout) in inputs {
            let start = Instant::now();
            suggest(context, *layout);
            samples.push((start.elapsed(), context.as_str()));
        }
    }
    let after: Vec<_> = samples
        .iter()
        .filter(|s| s.1.ends_with(' '))
        .copied()
        .collect();
    let mid: Vec<_> = samples
        .iter()
        .filter(|s| !s.1.ends_with(' '))
        .copied()
        .collect();
    report(&format!("{label}, after a space"), after);
    report(&format!("{label}, mid-word"), mid);
    let p99 = report(&format!("{label}, all"), samples);
    assert!(p99 < Duration::from_millis(8), "{label}: p99 {p99:?}");
}

/// The latency gate with 2,000 of the user's own Hindi words in the store (`own::OwnWords`).
#[test]
#[ignore = "timing: run with `make bench-hi` (release build)"]
fn hindi_latency_with_own_words() {
    let Some(lm) = lm_path() else { return };
    let inputs = inputs();
    let dir = root().join("packs/hi.neopack");
    let without = engine(None);
    pick_own_words(&without, &dir, 2_000);
    time("Hindi pack, 2,000 own words, without the LM", &without, &inputs, &|c, l| {
        let _ = without.suggest(c, l, true);
    });
    let with = engine(Some(lm));
    pick_own_words(&with, &dir, 2_000);
    time("Hindi pack, 2,000 own words, with the LM", &with, &inputs, &|c, l| {
        let _ = with.suggest(c, l, true);
    });
}

/// Picks `n` native words the pack in `dir` doesn't hold as the user's own
/// (`Engine::note_choice`, as a tap in the bar does): its most frequent words with their first
/// letter added at the end, each for its romanisation, so that the letters of the latency
/// inputs find many of them, whole and begun. Every third is used twice more, which lets it
/// take the middle (`rank::OWN_WORD_REPLACES_AFTER`). Prints how long it takes to forget one
/// (the engine's index of them is rebuilt from the store, as it is when the engine opens), and
/// a suggest after a pick (the word is put into the index) and after neither.
fn pick_own_words(engine: &Engine, dir: &std::path::Path, n: usize) {
    use mlengine_core::romanisation::Romanisation;
    let pack = Pack::open(dir).expect("the pack");
    let lang = mlengine_core::lang::LangId(pack.manifest.id.clone());
    let r = Romanisation::load(&pack.file("romanisation").unwrap()).unwrap();
    let lexicon = mlengine_core::lexicon::Lexicon::open(&pack.file("lexicon").unwrap()).unwrap();
    let mut words: Vec<(u64, String)> = Vec::new();
    lexicon.words(|key, count| words.push((count, String::from_utf8_lossy(key).into_owned())));
    words.sort_by(|a, b| b.cmp(a));
    let before = engine.learned_words().len();
    let mut picked = 0;
    let mut last = String::new();
    for (_, word) in &words {
        if picked == n {
            break;
        }
        let own = format!("{word}{}", word.chars().next().unwrap());
        if lexicon.get(own.as_bytes()).is_some() {
            continue;
        }
        last = own.clone();
        engine.note_choice(&r.romanize(&own), &own, Some(&lang));
        engine.note_used("", &own, Layout::Latin);
        if picked % 3 == 0 {
            engine.note_used("", &own, Layout::Latin);
            engine.note_used("", &own, Layout::Latin);
        }
        picked = engine.learned_words().len() - before;
    }
    assert_eq!(picked, n, "own words picked");
    let suggest = || {
        let start = Instant::now();
        let _ = engine.suggest("I said the", Layout::Latin, true);
        start.elapsed()
    };
    let _ = suggest();
    let start = Instant::now();
    engine.forget_word(&last);
    let rebuilt = start.elapsed();
    engine.note_choice(&r.romanize(&last), &last, Some(&lang));
    engine.note_used("", &last, Layout::Latin);
    let after_pick = suggest();
    println!(
        "{n} own words: forgetting one {rebuilt:?} (the index is rebuilt), a suggest after a pick \
         {after_pick:?}, after neither {:?}",
        suggest()
    );
}

#[test]
#[ignore = "timing: run with `make bench-hi` (release build)"]
fn hindi_latency() {
    let Some(lm) = lm_path() else { return };
    let inputs = inputs();
    let without = engine(None);
    time("Hindi pack without the LM", &without, &inputs, &|c, l| {
        let _ = without.suggest(c, l, true);
    });
    let with = engine(Some(lm));
    time("Hindi pack with the LM", &with, &inputs, &|c, l| {
        let _ = with.suggest(c, l, true);
    });
    time(
        "Hindi pack with the LM, keyboard cutoffs",
        &with,
        &inputs,
        &|c, l| {
            let start = Instant::now();
            let elapsed = || start.elapsed();
            let _ = with.suggest_timed(c, l, true, Some(&Timing::keyboard(&elapsed)));
        },
    );
}

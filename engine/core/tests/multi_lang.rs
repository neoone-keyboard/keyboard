//! Three and four languages at once on the real packs: English, Malayalam, Hindi and Assamese,
//! each romanised pack typing on the Latin layout. Which language's words a Latin word gets,
//! and where on the bar, and the latency gate with every transliterator running.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mlengine_core::lang::LangId;
use mlengine_core::pack::Pack;
use mlengine_core::{Engine, Layout, Paths, Suggestions, Timing};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// English, the packs `installed` in that load order, and `active` switched on, with the
/// keyboard's 10 suggestions.
fn engine(installed: &[&str], active: &[&str]) -> Engine {
    let packs: Vec<Pack> = installed
        .iter()
        .map(|id| Pack::open(&root().join(format!("packs/{id}.neopack"))).unwrap())
        .collect();
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    let mut e = Engine::open(&Paths::with_packs(Some(en), &packs, None));
    let ids: Vec<LangId> = active.iter().map(|id| LangId((*id).into())).collect();
    e.set_active(&ids).unwrap();
    e.set_max_suggestions(10);
    e
}

fn show(e: &Engine, s: &Suggestions) -> String {
    s.candidates
        .iter()
        .map(|c| {
            format!(
                "{} [{} {:.1}]",
                c.text,
                e.lang_id(c.lang).map_or("?".into(), |l| l.0),
                c.score
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn suggest(e: &Engine, context: &str) -> Suggestions {
    let s = e.suggest(context, Layout::Latin, true);
    eprintln!("{context:?}: {}", show(e, &s));
    s
}

const ALL: [&str; 3] = ["ml", "hi", "as"];

#[test]
fn four_languages_can_be_active_and_each_has_its_layout() {
    let e = engine(&ALL, &["en", "ml", "hi", "as"]);
    let lang = |id: &str| e.lang_of_id(id).unwrap();
    assert_eq!(
        e.layouts(),
        [
            Layout::Latin,
            Layout::Native(lang("ml")),
            Layout::Native(lang("hi")),
            Layout::Native(lang("as"))
        ]
    );
    let ids: Vec<LangId> = ["en", "ml", "hi", "as", "en"]
        .iter()
        .map(|id| LangId((*id).into()))
        .collect();
    let mut e = e;
    assert!(e.set_active(&ids).is_err(), "five, one a repeat");
    // Until the keyboard chooses, English and the first pack, as with two languages.
    let packs: Vec<Pack> = ALL
        .iter()
        .map(|id| Pack::open(&root().join(format!("packs/{id}.neopack"))).unwrap())
        .collect();
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    let fresh = Engine::open(&Paths::with_packs(Some(en), &packs, None));
    assert_eq!(
        fresh.active_languages(),
        [LangId("en".into()), LangId("ml".into())]
    );
}

#[test]
#[ignore = "exploration: WORDS=a,b cargo test ... print_bars -- --ignored --nocapture"]
fn print_bars() {
    let sets: [&[&str]; 4] = [
        &["en", "hi"],
        &["en", "ml"],
        &["en", "ml", "hi"],
        &["en", "ml", "hi", "as"],
    ];
    let words = std::env::var("WORDS").unwrap_or_default();
    for set in sets {
        eprintln!("== {set:?}");
        let e = engine(&ALL, set);
        for ctx in words.split(',').filter(|w| !w.is_empty()) {
            suggest(&e, ctx);
        }
    }
}

/// Words of every language typed letter by letter after "I said ", each then followed by a
/// space, and native words letter by letter on their layouts: `bundle.rs`'s latency inputs
/// with Hinglish and Assamese added.
fn latency_inputs(e: &Engine) -> Vec<(String, Layout)> {
    let latin = [
        "the",
        "because",
        "tomorrow",
        "hello",
        "keyboard",
        "beautiful",
        "together",
        "teh",
        "becuase",
        "definitely",
        "something",
        "Jishnu", // English
        "ente",
        "njan",
        "veettil",
        "sukhamano",
        "varunnundo",
        "kazhichille",
        "enikku",
        "avishyam",
        "mera",
        "kya",
        "nahi",
        "samjhna",
        "accha",
        "bahut",
        "theek",
        "kal", // Hinglish
        "moi",
        "tumi",
        "manuh",
        "kotha",
        "bhal",
        "ghor", // Assamese
    ];
    let mut inputs: Vec<(String, Layout)> = Vec::new();
    for w in latin {
        for end in 1..=w.len() {
            inputs.push((format!("I said {}", &w[..end]), Layout::Latin));
        }
        inputs.push((format!("{w} "), Layout::Latin));
    }
    let native = [
        ("ml", "എന്റെ"),
        ("ml", "സുഖമാണോ"),
        ("hi", "मेरा"),
        ("hi", "नमस्ते"),
        ("as", "অসমীয়া"),
    ];
    for (id, w) in native {
        let Some(lang) = e
            .lang_of_id(id)
            .filter(|_| e.active_languages().contains(&LangId(id.into())))
        else {
            continue;
        };
        let chars: Vec<char> = w.chars().collect();
        for end in 1..=chars.len() {
            inputs.push((chars[..end].iter().collect(), Layout::Native(lang)));
        }
    }
    inputs
}

/// p99 over 20 rounds of `inputs`, with `suggest`.
fn p99(
    inputs: &[(String, Layout)],
    suggest: &dyn Fn(&str, Layout),
) -> (Duration, Duration, String) {
    for (context, layout) in inputs {
        suggest(context, *layout);
    }
    let mut samples: Vec<(Duration, &str)> = Vec::new();
    for _ in 0..20 {
        for (context, layout) in inputs {
            let start = Instant::now();
            suggest(context, *layout);
            samples.push((start.elapsed(), context));
        }
    }
    samples.sort();
    let p50 = samples[samples.len() / 2].0;
    let p99 = samples[samples.len() * 99 / 100];
    (p50, p99.0, p99.1.to_string())
}

/// The latency gate (p99 under 8 ms on the Mac, as `bundle.rs`'s) with 2, 3 and 4 languages
/// active: every active romanised pack runs its transliterator on each Latin keystroke.
/// Run in release: `make bench`.
#[test]
#[ignore = "timing: run with `make bench` (release build)"]
fn latency_with_more_languages() {
    let cases: [&[&str]; 5] = [
        &["en", "ml"],
        &["en", "hi"],
        &["en", "ml", "hi"],
        &["en", "ml", "hi", "as"],
        &["ml", "hi", "as"],
    ];
    let mut worst = Duration::ZERO;
    for active in cases {
        if active.len() > mlengine_core::lang::MAX_ACTIVE {
            continue;
        }
        let e = engine(&["ml", "hi", "as"], active);
        let inputs = latency_inputs(&e);
        let plain = p99(&inputs, &|c, l| {
            let _ = e.suggest(c, l, true);
        });
        let keyboard = p99(&inputs, &|c, l| {
            let start = Instant::now();
            let elapsed = || start.elapsed();
            let _ = e.suggest_timed(c, l, true, Some(&Timing::keyboard(&elapsed)));
        });
        println!(
            "{active:?}: {} requests; p50 {:?}, p99 {:?} ({:?}); keyboard cutoffs p50 {:?}, p99 {:?} ({:?})",
            inputs.len() * 20,
            plain.0,
            plain.1,
            plain.2,
            keyboard.0,
            keyboard.1,
            keyboard.2
        );
        worst = worst.max(plain.1);
    }
    assert!(worst < Duration::from_millis(8), "p99 {worst:?}");
}

//! Three and four languages at once on the real packs: English, Malayalam, Hindi and Assamese,
//! each romanised pack typing on the Latin layout. Which language's words a Latin word gets,
//! and where on the bar, and the latency gate with every transliterator running.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mlengine_core::lang::LangId;
use mlengine_core::pack::Pack;
use mlengine_core::{Engine, Kind, Layout, Paths, Suggestions, Timing};

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

/// The bar's first slots, those shown without scrolling: the middle and two side slots.
const SHOWN: usize = 3;

fn texts(s: &Suggestions) -> Vec<&str> {
    s.candidates.iter().map(|c| c.text.as_str()).collect()
}

fn middle(s: &Suggestions) -> &str {
    &s.candidates[0].text
}

/// The language ids of the candidates in the first `SHOWN` slots, but for the word as typed
/// (which is the first pack's without English).
fn shown_langs(e: &Engine, s: &Suggestions) -> Vec<String> {
    s.candidates
        .iter()
        .take(SHOWN)
        .filter(|c| c.kind != Kind::Verbatim)
        .map(|c| e.lang_id(c.lang).unwrap().0)
        .collect()
}

/// Where `text` is on the bar, if offered.
fn place(s: &Suggestions, text: &str) -> Option<usize> {
    s.candidates.iter().position(|c| c.text == text)
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

/// With no context, each romanised word takes its own language's spelling.
#[test]
fn manglish_and_hinglish_words_find_their_language() {
    for active in [&["en", "ml", "hi"][..], &["en", "ml", "hi", "as"]] {
        let e = engine(&ALL, active);
        for (typed, want) in [
            ("ente", "എന്റെ"),
            ("njan", "ഞാൻ"),
            ("ningal", "നിങ്ങൾ"),
            ("mera", "मेरा"),
            ("kya", "क्या"),
            ("bahut", "बहुत"),
            ("theek", "ठीक"),
        ] {
            let s = suggest(&e, typed);
            assert_eq!(middle(&s), want, "{active:?} {typed}: {}", show(&e, &s));
        }
    }
}

/// After a word in a pack's script, that pack's words take the middle and the side slots
/// shown with it; another pack's are further along the bar.
#[test]
fn the_previous_words_language_keeps_the_shown_slots() {
    for active in [&["en", "ml", "hi"][..], &["en", "ml", "hi", "as"]] {
        let e = engine(&ALL, active);
        for (context, want, lang) in [
            ("എന്റെ pani", "പണി", "ml"),
            ("ഞാൻ നാളെ veettil", "വീട്ടിൽ", "ml"),
            ("മേരാ ente", "എന്റെ", "ml"),
            ("मेरा pani", "पानी", "hi"),
            ("मेरा naam", "नाम", "hi"),
            ("मैं कल ghar", "घर", "hi"),
        ] {
            let s = suggest(&e, context);
            assert_eq!(middle(&s), want, "{active:?} {context}: {}", show(&e, &s));
            for got in shown_langs(&e, &s) {
                assert!(
                    got == "en" || got == lang,
                    "{active:?} {context}: {got} shown, {}",
                    show(&e, &s)
                );
            }
        }
    }
}

/// A Malayalam context with a word Malayalam has no good spelling for: the middle may be
/// another pack's, but Malayalam's best is shown beside it, and the bar still holds the
/// others further along.
#[test]
fn the_favoured_languages_best_is_shown_beside_another_packs_middle() {
    let e = engine(&ALL, &["en", "ml", "hi", "as"]);
    let s = suggest(&e, "എന്റെ mera");
    assert_eq!(middle(&s), "मेरा", "{}", show(&e, &s));
    let ml = place(&s, "മേരാ").expect("Malayalam's spelling offered");
    assert!(ml < SHOWN, "{}", show(&e, &s));
    assert!(
        s.candidates
            .iter()
            .skip(SHOWN)
            .any(|c| e.lang_id(c.lang).unwrap().0 == "as"),
        "Assamese further along: {}",
        show(&e, &s)
    );
}

/// No Assamese word swamps the shown slots of a Malayalam sentence.
#[test]
fn another_pack_does_not_swamp_the_shown_slots() {
    let e = engine(&ALL, &["en", "ml", "hi", "as"]);
    let s = suggest(&e, "ഞാൻ നാളെ acha");
    let langs = shown_langs(&e, &s);
    assert!(
        langs.iter().all(|l| l == "en" || l == "ml"),
        "{}",
        show(&e, &s)
    );
    assert!(
        texts(&s).contains(&"अच्छा"),
        "Hindi further along: {}",
        show(&e, &s)
    );
}

/// After a Hindi word the next words are Hindi, though Malayalam's LM is loaded: it knows
/// nothing of Devanagari.
#[test]
fn next_words_follow_the_previous_words_pack() {
    for active in [&["en", "ml", "hi"][..], &["en", "ml", "hi", "as"]] {
        let e = engine(&ALL, active);
        for (context, lang) in [("मेरा ", "hi"), ("मैं कल ", "hi"), ("എന്റെ ", "ml")]
        {
            // Twice: the second time the LM's context has caught up.
            let _ = e.suggest(context, Layout::Latin, true);
            let s = suggest(&e, context);
            assert!(!s.candidates.is_empty(), "{active:?} {context}");
            for c in &s.candidates {
                let got = e.lang_id(c.lang).unwrap().0;
                assert!(
                    got == lang || got == "en",
                    "{active:?} {context}: {}",
                    show(&e, &s)
                );
            }
        }
    }
}

/// The language habit tells a Malayalam user from a Hindi one when nothing else does: the
/// same letters, with no context, keep the user's language in the shown slots. With English
/// and without.
#[test]
fn the_habit_favours_the_users_language() {
    let users = [
        ("ml", ["എന്റെ", "വീട്", "ഞാൻ", "പണി", "നല്ല"]),
        ("hi", ["मेरा", "घर", "मैं", "पानी", "अच्छा"]),
    ];
    for ((lang, words), active) in users.into_iter().flat_map(|u| {
        [
            (u, &["en", "ml", "hi", "as"][..]),
            (u, &["ml", "hi", "as"][..]),
        ]
    }) {
        let e = engine(&ALL, active);
        for _ in 0..6 {
            for w in words {
                e.note_used("", w, Layout::Latin);
            }
            e.note_used("", "the", Layout::Latin);
        }
        for typed in ["pani", "acha"] {
            let s = suggest(&e, typed);
            let langs = shown_langs(&e, &s);
            assert!(
                langs.iter().all(|l| l == "en" || l == lang),
                "{lang} user, {active:?}, {typed}: {}",
                show(&e, &s)
            );
            assert!(
                langs.iter().any(|l| l == lang),
                "{lang} user, {active:?}, {typed}: {}",
                show(&e, &s)
            );
        }
    }
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

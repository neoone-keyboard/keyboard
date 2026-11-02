//! Golden checks and the latency gate on the real, committed lexicon bundle.
//! `make lexicons` rebuilds it. Set MLENGINE_BUNDLE to test another directory.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mlengine_core::{Engine, Kind, Layout, Paths};

fn bundle_dir() -> PathBuf {
    std::env::var_os("MLENGINE_BUNDLE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../apps/ios/KeyboardExtension/Lexicon")
        })
}

fn engine() -> Engine {
    let dir = bundle_dir();
    assert!(
        dir.join("en.fst").exists(),
        "no lexicon bundle in {}: run `make lexicons`",
        dir.display()
    );
    Engine::open(&Paths {
        english: Some(dir.join("en.fst")),
        malayalam: Some(dir.join("ml.fst")),
        next_words: Some(dir.join("next.fst")),
        user_dir: None,
    })
}

fn middle(engine: &Engine, context: &str) -> (String, Kind) {
    let c = &engine.suggest(context, Layout::Latin, true).candidates[0];
    (c.text.clone(), c.kind)
}

#[test]
fn every_bundle_file_loads() {
    assert_eq!(
        engine().loaded_sources(),
        ["english", "malayalam", "next_words"]
    );
}

#[test]
fn english_golden() {
    let engine = engine();
    assert_eq!(middle(&engine, "teh").0, "the");
    assert_eq!(middle(&engine, "dont").0, "don't");
    assert_eq!(middle(&engine, "becuase").0, "because");
    assert_eq!(middle(&engine, "i").0, "I");
    for word in [
        "hello", "the", "them", "then", "help", "enter", "man", "London",
    ] {
        assert_eq!(
            middle(&engine, word),
            (word.to_string(), Kind::Verbatim),
            "{word}"
        );
    }
    let next = engine.suggest("I ", Layout::Latin, true);
    assert_eq!(next.candidates.len(), 3, "next words after I: {next:?}");
}

#[test]
fn malayalam_layout_golden() {
    let engine = engine();
    let s = engine.suggest("എന്റ", Layout::Malayalam, true);
    assert_eq!(
        s.candidates[0].text, "എന്റ",
        "typed text stays in the middle"
    );
    assert!(s.candidates.iter().any(|c| c.text == "എന്റെ"), "{s:?}");
    let k = engine.suggest("ക", Layout::Malayalam, true);
    assert!(!k.vowel_signs.is_empty());
}

/// p99 of a suggestion request must stay under 8 ms on the Mac (spec M2 §6).
/// Run in release: `make bench`.
#[test]
#[ignore = "timing: run with `make bench` (release build)"]
fn latency() {
    let engine = engine();
    let words = [
        "the",
        "because",
        "tomorrow",
        "hello",
        "keyboard",
        "beautiful",
        "wonderful",
        "together",
        "ente",
        "njan",
        "veettil",
        "malayalam",
        "sukhamano",
        "enikku",
        "thank",
        "something",
        "Jishnu",
        "teh",
        "becuase",
        "helo",
    ];
    let mut inputs: Vec<(String, Layout)> = Vec::new();
    for w in words {
        for end in 1..=w.len() {
            inputs.push((format!("I said {}", &w[..end]), Layout::Latin));
        }
        inputs.push((format!("{w} "), Layout::Latin));
    }
    for w in ["എന്റെ", "വീട്ടിൽ", "കേരളം", "മലയാളം", "സുഖമാണോ"]
    {
        let chars: Vec<char> = w.chars().collect();
        for end in 1..=chars.len() {
            inputs.push((chars[..end].iter().collect(), Layout::Malayalam));
        }
    }
    for (context, layout) in &inputs {
        let _ = engine.suggest(context, *layout, true); // warm the page cache
    }
    let mut samples: Vec<(Duration, &str)> = Vec::new();
    for _ in 0..20 {
        for (context, layout) in &inputs {
            let start = Instant::now();
            let _ = engine.suggest(context, *layout, true);
            samples.push((start.elapsed(), context.as_str()));
        }
    }
    samples.sort();
    let p50 = samples[samples.len() / 2].0;
    let p99 = samples[samples.len() * 99 / 100];
    let worst = samples[samples.len() - 1];
    println!(
        "suggest latency over {} requests: p50 {p50:?}, p99 {:?} ({:?}), max {:?} ({:?})",
        samples.len(),
        p99.0,
        p99.1,
        worst.0,
        worst.1
    );
    assert!(
        p99.0 < Duration::from_millis(8),
        "p99 {:?} on {:?}",
        p99.0,
        p99.1
    );
}

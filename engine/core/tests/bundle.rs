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
        manglish: Some(dir.join("manglish.fst")),
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
        ["english", "malayalam", "manglish", "next_words"]
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

fn texts(engine: &Engine, context: &str) -> Vec<String> {
    let s = engine.suggest(context, Layout::Latin, true);
    s.candidates.into_iter().map(|c| c.text).collect()
}

#[test]
fn manglish_golden() {
    let engine = engine();
    for (typed, want) in [
        ("ente", "എന്റെ"),
        ("ende", "എന്റെ"),
        ("entey", "എന്റെ"),
        ("njan", "ഞാൻ"),
        ("illa", "ഇല്ല"),
    ] {
        assert_eq!(
            middle(&engine, typed),
            (want.to_string(), Kind::Manglish),
            "{typed}"
        );
    }
}

#[test]
fn unfinished_english_prefixes_stay_as_typed() {
    // `ent` has എന്റെ's skeleton, and `thi` is one slip from `the`, but far
    // more English words start with them (enter, entire …; this, think …).
    let engine = engine();
    for typed in ["ent", "wor", "bec", "thi"] {
        assert_eq!(
            middle(&engine, typed),
            (typed.to_string(), Kind::Verbatim),
            "{typed}: {:?}",
            texts(&engine, typed)
        );
    }
    assert!(texts(&engine, "ent").contains(&"എന്റെ".to_string()));
}

#[test]
fn manglish_beats_english_corrections_when_nothing_points_to_english() {
    let engine = engine();
    for (typed, want) in [
        ("nale", "നാളെ"),
        ("ente", "എന്റെ"),
        ("ende", "എന്റെ"),
        ("njan", "ഞാൻ"),
        ("sugham", "സുഖം"),
        ("veedu", "വീട്"),
        ("ഞാൻ veedu", "വീട്"),
    ] {
        assert_eq!(
            middle(&engine, typed),
            (want.to_string(), Kind::Manglish),
            "{typed}: {:?}",
            texts(&engine, typed)
        );
    }
    // Known gap: എന്തു (the older spelling, five times as common in the
    // corpus) wins over എന്ത്; both romanise as `enthu`. Either is Malayalam.
    let (word, kind) = middle(&engine, "enthu");
    assert_eq!(kind, Kind::Manglish);
    assert!(["എന്ത്", "എന്തു"].contains(&word.as_str()), "{word}");
}

#[test]
fn a_close_call_between_languages_keeps_the_typed_word() {
    let engine = engine();
    let s = texts(&engine, "I feel pani");
    assert_eq!(s[0], "pani", "{s:?}");
    assert!(s[1..].contains(&"pain".to_string()), "{s:?}");
    assert!(
        s[1..].iter().any(|t| t == "പനി" || t == "പണി"),
        "{s:?}"
    );
}

#[test]
fn english_stays_english_with_manglish_on() {
    let engine = engine();
    for word in ["hello", "the", "end", "then"] {
        assert_eq!(
            middle(&engine, word),
            (word.to_string(), Kind::Verbatim),
            "{word}"
        );
    }
    assert_eq!(middle(&engine, "teh"), ("the".into(), Kind::Correction));
    let sentence = "I am going home";
    for (end, _) in sentence.match_indices(' ').chain([(sentence.len(), "")]) {
        let context = &sentence[..end];
        let word = context.rsplit(' ').next().unwrap();
        assert_eq!(
            middle(&engine, context),
            (word.to_string(), Kind::Verbatim),
            "{context}"
        );
    }
}

#[test]
fn code_mixed_english_words_stay_english() {
    let engine = engine();
    for context in [
        "njan innu office",
        "ഞാൻ ഇന്ന് office",
        "ente phone",
        "എന്റെ phone",
    ] {
        let word = context.rsplit(' ').next().unwrap();
        assert_eq!(
            middle(&engine, context),
            (word.to_string(), Kind::Verbatim),
            "{context}"
        );
    }
    assert_eq!(middle(&engine, "njan").0, "ഞാൻ");
    assert_eq!(middle(&engine, "ഞാൻ innu").0, "ഇന്ന്");
    assert_eq!(middle(&engine, "ഞാൻ ഇന്ന് office il poyi").0, "പോയി");
}

/// Chat forms missing from the corpus (or too rare to convert on their own),
/// read as a frequent stem plus suffixes.
#[test]
fn agglutinated_manglish_splits_into_stem_and_suffix() {
    let engine = engine();
    for (typed, want) in [
        ("sukhamano", "സുഖമാണോ"),         // സുഖം + ആണോ
        ("kazhicho", "കഴിച്ചോ"),         // കഴിച്ചു + ഓ
        ("varunnundo", "വരുന്നുണ്ടോ"),      // വരുന്നുണ്ട് + ഓ
        ("ishtamano", "ഇഷ്ടമാണോ"),        // ഇഷ്ടം + ആണോ
        ("sukhamalle", "സുഖമല്ലേ"),       // സുഖം + അല്ലേ
        ("veettilano", "വീട്ടിലാണോ"),     // വീട്ടിൽ + ആണോ: chillu ൽ → ല
        ("veettilundo", "വീട്ടിലുണ്ടോ"),   // വീട്ടിൽ + ഉണ്ടോ
        ("sukhamayirunnu", "സുഖമായിരുന്നു"), // സുഖം + ആയിരുന്നു
        ("kazhichille", "കഴിച്ചില്ലേ"),   // കഴിച്ചു + ഇല്ലേ
        ("kazhichilla", "കഴിച്ചില്ല"),    // കഴിച്ചു + ഇല്ല
        ("ethiyo", "എത്തിയോ"),           // എത്തി + ഓ: glide യ
        ("paranjo", "പറഞ്ഞോ"),           // പറഞ്ഞു + ഓ
        ("paranjennu", "പറഞ്ഞെന്ന്"),      // പറഞ്ഞു + എന്ന്
        ("ammayodu", "അമ്മയോട്"),         // അമ്മ + ഓട്: typed glide y
        ("makkalodu", "മക്കളോട്"),        // മക്കൾ + ഓട്: chillu ൾ → ള
        ("ammakku", "അമ്മക്ക്"),           // അമ്മ + ക്ക്
        // Rarer stems convert after a Malayalam word.
        ("അവൻ poyirunno", "പോയിരുന്നോ"),  // പോയിരുന്നു + ഓ
        ("കുട്ടി urangiyo", "ഉറങ്ങിയോ"),   // ഉറങ്ങി + ഓ
        ("അവൻ chettanodu", "ചേട്ടനോട്"),  // ചേട്ടൻ + ഓട്
        ("ഞാൻ kazhikkatte", "കഴിക്കട്ടെ"), // കഴിക്ക + ട്ടെ
        ("ഞാൻ vaanganam", "വാങ്ങണം"),      // വാങ്ങ + ണം
    ] {
        assert_eq!(
            middle(&engine, typed),
            (want.to_string(), Kind::Manglish),
            "{typed}: {:?}",
            texts(&engine, typed)
        );
    }
    // Without Malayalam context, a rarer stem's split waits in a side slot.
    let s = texts(&engine, "poyirunno");
    assert_eq!(s[0], "poyirunno");
    assert!(s.contains(&"പോയിരുന്നോ".to_string()), "{s:?}");
}

#[test]
fn suffix_splitting_leaves_english_and_whole_forms_alone() {
    let engine = engine();
    // English words ending in a suffix spelling (-o, -il, -ude, -anam …).
    for word in [
        "radio", "piano", "tomato", "studio", "pencil", "until", "attitude", "potato",
    ] {
        assert_eq!(
            middle(&engine, word),
            (word.to_string(), Kind::Verbatim),
            "{word}: {:?}",
            texts(&engine, word)
        );
    }
    // Whole forms in the corpus keep their own spelling.
    for (typed, want) in [
        ("veettil", "വീട്ടിൽ"),
        ("kandilla", "കണ്ടില്ല"),
        ("varumo", "വരുമോ"),
    ] {
        assert_eq!(
            middle(&engine, typed).0,
            want,
            "{typed}: {:?}",
            texts(&engine, typed)
        );
    }
    // A rare whole form (പഠിക്കണം) outranks a split of a common stem
    // (പട്ടിക + ണം), and neither is frequent enough to convert.
    let s = texts(&engine, "padikkanam");
    assert_eq!(s[0], "padikkanam", "{s:?}");
    let whole = s.iter().position(|t| t == "പഠിക്കണം");
    let split = s.iter().position(|t| t == "പട്ടികണം");
    assert!(
        whole.is_some() && (split.is_none() || split > whole),
        "{s:?}"
    );
    // After an English word, a split with a misspelled stem stays on the side.
    let s = texts(&engine, "I kazhicho");
    assert_eq!(s[0], "kazhicho", "{s:?}");
    assert!(s.contains(&"കഴിച്ചോ".to_string()), "{s:?}");
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
        "varunnundo",
        "kazhichille",
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

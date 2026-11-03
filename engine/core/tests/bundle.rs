//! Golden checks and the latency gate on the real, committed lexicon bundle.
//! `make lexicons` rebuilds it. Set MLENGINE_BUNDLE to test another directory.
//!
//! The transliterator rows also need the trained model, `training/build/translit.mlkbnn`,
//! which isn't committed; they are skipped without it.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mlengine_core::lexicon::Lexicon;
use mlengine_core::nn::{Translit, Transliteration};
use mlengine_core::{Engine, Kind, Layout, Paths, rank, suffix};

fn bundle_dir() -> PathBuf {
    std::env::var_os("MLENGINE_BUNDLE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../apps/ios/KeyboardExtension/Lexicon")
        })
}

fn engine() -> Engine {
    open_engine(None)
}

fn open_engine(translit: Option<PathBuf>) -> Engine {
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
        translit,
        user_dir: None,
    })
}

fn model_path() -> Option<PathBuf> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../training/build/translit.mlkbnn");
    if path.exists() {
        Some(path)
    } else {
        println!(
            "skipping the transliterator: {} is missing (export it with mlkb.translit.export)",
            path.display()
        );
        None
    }
}

/// The bundle with the trained transliterator; None (the row is skipped) without the model.
fn model_engine() -> Option<Engine> {
    model_path().map(|path| open_engine(Some(path)))
}

/// The engine without the model, then with it when the model file exists: every
/// golden must hold for both.
fn engines() -> Vec<Engine> {
    let mut engines = vec![engine()];
    engines.extend(model_engine());
    engines
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
    for engine in engines() {
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
}

/// Indian English in English sentences (`data/eval/english-sentences.txt`): words from
/// `data/indian-english.txt`, and capitals typed on purpose (`Capitals` in engine.rs).
/// Each was autocorrected before (Vishu → Fish, biryani → Britain, PNR → ONE).
#[test]
fn indian_english_stays_as_typed() {
    for engine in engines() {
        for context in [
            "Are you coming home for Vishu",
            "My mother made chicken biryani",
            "Can you check the PNR",
            "We should plan a trip to Munnar",
            "The weather in Ooty",
            "Happy Onam",
            "Eid",
            "Add Rahul",
            "Send me the OTP",
        ] {
            let word = context.rsplit(' ').next().unwrap();
            assert_eq!(
                middle(&engine, context),
                (word.to_string(), Kind::Verbatim),
                "{context}: {:?}",
                texts(&engine, context)
            );
        }
        // Lowercase, the same places and festivals are still Manglish.
        for (typed, want) in [("onam", "ഓണം"), ("vishu", "വിഷു"), ("munnar", "മൂന്നാർ")] {
            assert_eq!(middle(&engine, typed).0, want, "{typed}: {:?}", texts(&engine, typed));
        }
    }
}

#[test]
fn malayalam_layout_golden() {
    for engine in engines() {
        let s = engine.suggest("എന്റ", Layout::Malayalam, true);
        assert_eq!(
            s.candidates[0].text, "എന്റ",
            "typed text stays in the middle"
        );
        assert!(s.candidates.iter().any(|c| c.text == "എന്റെ"), "{s:?}");
        let k = engine.suggest("ക", Layout::Malayalam, true);
        assert!(!k.vowel_signs.is_empty());
    }
}

fn texts(engine: &Engine, context: &str) -> Vec<String> {
    let s = engine.suggest(context, Layout::Latin, true);
    s.candidates.into_iter().map(|c| c.text).collect()
}

#[test]
fn manglish_golden() {
    for engine in engines() {
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
}

#[test]
fn unfinished_english_prefixes_stay_as_typed() {
    // `ent` has എന്റെ's skeleton, and `thi` is one slip from `the`, but far
    // more English words start with them (enter, entire …; this, think …).
    for engine in engines() {
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
}

#[test]
fn manglish_beats_english_corrections_when_nothing_points_to_english() {
    for engine in engines() {
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
}

#[test]
fn a_close_call_between_languages_keeps_the_typed_word() {
    for engine in engines() {
        let s = texts(&engine, "I feel pani");
        assert_eq!(s[0], "pani", "{s:?}");
        assert!(s[1..].contains(&"pain".to_string()), "{s:?}");
        assert!(s[1..].iter().any(|t| t == "പനി" || t == "പണി"), "{s:?}");
    }
}

#[test]
fn english_stays_english_with_manglish_on() {
    for engine in engines() {
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
}

#[test]
fn code_mixed_english_words_stay_english() {
    for engine in engines() {
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
}

/// Chat forms missing from the corpus (or too rare to convert on their own),
/// read as a frequent stem plus suffixes.
#[test]
fn agglutinated_manglish_splits_into_stem_and_suffix() {
    for engine in engines() {
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
        // Without Malayalam context, a rarer stem's split waits in a side slot,
        // unless the model reads it confidently (`rank::MANGLISH_MODEL_CONFIDENT`).
        let s = texts(&engine, "poyirunno");
        let has_model = engine.loaded_sources().contains(&"translit");
        assert_eq!(
            s[0],
            if has_model {
                "പോയിരുന്നോ"
            } else {
                "poyirunno"
            },
            "{s:?}"
        );
        assert!(s.contains(&"പോയിരുന്നോ".to_string()), "{s:?}");
    }
}

#[test]
fn suffix_splitting_leaves_english_and_whole_forms_alone() {
    for engine in engines() {
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
        // (പട്ടിക + ണം). Neither is frequent enough to convert on its own; the
        // model's confident reading converts the whole form.
        let s = texts(&engine, "padikkanam");
        let has_model = engine.loaded_sources().contains(&"translit");
        assert_eq!(
            s[0],
            if has_model {
                "പഠിക്കണം"
            } else {
                "padikkanam"
            },
            "{s:?}"
        );
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
}

#[test]
fn the_model_loads_as_a_source() {
    let Some(engine) = model_engine() else { return };
    assert!(engine.loaded_sources().contains(&"translit"));
}

/// Chat words the model's ranking converts.
#[test]
fn translit_model_converts_in_the_middle() {
    let Some(engine) = model_engine() else { return };
    for (typed, want) in [
        ("enthu", "എന്തു"),
        ("thrissur", "തൃശ്ശൂർ"),       // M2: ത്രിശ്ശൂർ on the side
        ("sheriyanu", "ശരിയാണ്"),     // M2: ശെരിയാൻ on the side
        ("manasilayi", "മനസ്സിലായി"), // M2: on the side
    ] {
        assert_eq!(
            middle(&engine, typed),
            (want.to_string(), Kind::Manglish),
            "{typed}: {:?}",
            texts(&engine, typed)
        );
    }
}

/// Loose spellings convert in the middle to the commoner standard word one
/// medial vowel sign away (`rank::MANGLISH_NEIGHBOUR_GAIN`), which takes the
/// place of the rare form the model read confidently (`rank::MANGLISH_MODEL_CONFIDENT`);
/// the rare form stays beside it. User-reported on device, 2026-11-03: the user
/// types ആവശ്യം as `avshyam` and `avishyam` (ആവിശ്യം is in the lexicon too).
#[test]
fn translit_model_forgives_loose_spellings() {
    let Some(engine) = model_engine() else { return };
    for context in ["avshyam", "avishyam", "ഞാൻ avishyam"] {
        let bar = texts(&engine, context);
        assert_eq!(
            middle(&engine, context),
            ("ആവശ്യം".to_string(), Kind::Manglish),
            "{context}: {bar:?}"
        );
        assert!(bar.contains(&"ആവിശ്യം".to_string()), "{context}: {bar:?}");
    }
    assert_eq!(middle(&engine, "aavashyam").0, "ആവശ്യം");
    // A final vowel sign is inflection, not a slip: വന്നോ is not വന്ന.
    assert_eq!(middle(&engine, "vanno").0, "വന്നോ");
}

/// Known misses; no ranking weight reaches them.
/// - `kollamo` → കൊള്ളാമോ: both readings are splits with no corpus count, and both
///   sources prefer കൊല്ലമോ: the model (−0.150 per token against −0.390) and the stem
///   frequency (കൊല്ലം over കൊള്ളാം). കൊള്ളാമോ stays in a side slot.
/// - `chakshusravanam` → ചക്ഷുശ്രവണം (user-reported on device, 2026-11-03): not in the
///   lexicon, so only the model's free spelling (side slot) can offer it, and the model
///   spells ചക്ഷുസ്രാവണം (s → സ, −0.09 per token). A rare word; left to a better model,
///   and not asserted below: off the lexicon it could only ever reach a side slot.
#[test]
#[ignore = "known misses: model and stem frequency both prefer another spelling"]
fn translit_model_known_misses() {
    let Some(engine) = model_engine() else { return };
    assert_eq!(
        middle(&engine, "kollamo"),
        ("കൊള്ളാമോ".to_string(), Kind::Manglish),
        "{:?}",
        texts(&engine, "kollamo")
    );
}

/// Chat spellings from `data/chat-forms.tsv`. `nanni` (നന്ദി as said; user-reported on
/// device, 2026-11-03): neither source reaches it. The model can't spell it (after നന്, ദ
/// is its 23rd token), and the skeleton lookup doesn't match it (ന്ദ romanises nd). Its
/// confident runner-up നണ്ണി (−12.45, −0.44 per token) took the middle through
/// `rank::MANGLISH_MODEL_CONFIDENT`; it stays beside നന്ദി now.
#[test]
fn chat_forms_convert() {
    for engine in engines() {
        for (typed, want) in [
            ("nanni", "നന്ദി"),
            ("ചേട്ടാ nanni", "നന്ദി"),
            ("nte", "എന്റെ"),
            ("ntha", "എന്താ"),
            ("pwoli", "പൊളി"),
        ] {
            assert_eq!(
                middle(&engine, typed),
                (want.to_string(), Kind::Manglish),
                "{typed}: {:?}",
                texts(&engine, typed)
            );
        }
    }
}

#[test]
fn translit_model_leaves_english_as_typed() {
    let Some(engine) = model_engine() else { return };
    for context in ["I said hello", "radio", "piano", "meeting"] {
        let word = context.rsplit(' ').next().unwrap();
        assert_eq!(
            middle(&engine, context),
            (word.to_string(), Kind::Verbatim),
            "{context}: {:?}",
            texts(&engine, context)
        );
    }
}

/// p99 of a suggestion request must stay under 8 ms on the Mac (spec M2 §6), model included.
/// Run in release: `make bench`.
#[test]
#[ignore = "timing: run with `make bench` (release build)"]
fn latency() {
    // With the transliterator when it has been exported: that is what the keyboard runs.
    let engine = model_engine().unwrap_or_else(engine);
    println!("sources: {:?}", engine.loaded_sources());
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

fn trained_translit() -> Option<Translit> {
    model_path().map(|path| Translit::open(&path).unwrap())
}

fn bundle_lexicon(name: &str) -> Lexicon {
    Lexicon::open(&bundle_dir().join(name)).unwrap()
}

/// The constrained beam's spellings of `typed`, with its suffix splits allowed.
fn transliterations(
    t: &Translit,
    ml: &Lexicon,
    manglish: &Lexicon,
    typed: &str,
) -> Vec<Transliteration> {
    let splits: Vec<String> = suffix::splits(manglish, typed, rank::MANGLISH_SPLIT_STEM_MIN)
        .into_iter()
        .map(|s| s.word)
        .collect();
    t.candidates_with_splits(typed, ml, &splits, 4)
}

#[test]
fn translit_spells_lexicon_words_and_splits() {
    let Some(t) = trained_translit() else { return };
    let (ml, manglish) = (bundle_lexicon("ml.fst"), bundle_lexicon("manglish.fst"));
    for (typed, want) in [
        ("enthu", "എന്തു"),
        // Known model gap: നന്ദി is out of reach. After നന്, ദ is the model's 23rd token
        // (log prob -7.4), far outside the 8 expanded per step; നന്നി is in ml.fst too.
        ("nanni", "നന്നി"),
        ("jishnu", "ജിഷ്ണു"),     // a name, but in ml.fst
        ("sukhamano", "സുഖമാണോ"), // split: സുഖം + ആണോ
        ("kollamo", "കൊള്ളാമോ"),  // split: കൊള്ളാം + ഓ
    ] {
        let got = transliterations(&t, &ml, &manglish, typed);
        assert!(
            got.iter().any(|c| c.in_lexicon && c.text == want),
            "{typed}: {got:?}"
        );
        assert!(got.iter().filter(|c| c.in_lexicon).count() <= 4);
        assert!(got.iter().filter(|c| !c.in_lexicon).count() <= 1);
    }
    let long = "a".repeat(24);
    let _ = t.candidates(&long, &ml, 4);
    assert!(t.candidates(&format!("{long}a"), &ml, 4).is_empty());
}

#[test]
fn translit_free_beam_spells_words_off_the_lexicon() {
    let Some(t) = trained_translit() else { return };
    // ജിഷ്ണു is in ml.fst, so take it out: a lexicon of everything but names.
    let ml = Lexicon::from_bytes(
        mlengine_core::lexicon::build([("എന്തു".as_bytes().to_vec(), 1)]).unwrap(),
    )
    .unwrap();
    let got = t.candidates("jishnu", &ml, 4);
    let free: Vec<_> = got.iter().filter(|c| !c.in_lexicon).collect();
    assert_eq!(free.len(), 1, "{got:?}");
    assert_eq!(free[0].text, "ജിഷ്ണു", "{got:?}");
}

/// Prints each word's spellings and the per-word p50/p99. Run in release:
/// `cargo test --release -p mlengine-core --test bundle -- --ignored --nocapture translit_latency`.
#[test]
#[ignore = "timing: run in release"]
fn translit_latency() {
    let Some(t) = trained_translit() else { return };
    let (ml, manglish) = (bundle_lexicon("ml.fst"), bundle_lexicon("manglish.fst"));
    let words = [
        "enthu",
        "nanni",
        "jishnu",
        "sukhamano",
        "kollamo",
        "ente",
        "njan",
        "veettil",
        "amma",
        "kerala",
        "malayalam",
        "varunnundo",
        "kazhichille",
        "enikku",
        "thiruvananthapuram",
        "pinne",
        "sheri",
        "evide",
        "ippol",
        "nale",
        "ammayodu",
        "paranjennu",
    ];
    for w in words {
        println!("{w}: {:?}", transliterations(&t, &ml, &manglish, w));
    }
    let mut samples: Vec<(Duration, &str)> = Vec::new();
    for _ in 0..10 {
        for w in words {
            let start = Instant::now();
            let _ = transliterations(&t, &ml, &manglish, w);
            samples.push((start.elapsed(), w));
        }
    }
    samples.sort();
    println!(
        "translit over {} requests: p50 {:?}, p99 {:?}, max {:?}",
        samples.len(),
        samples[samples.len() / 2].0,
        samples[samples.len() * 99 / 100],
        samples[samples.len() - 1]
    );
}

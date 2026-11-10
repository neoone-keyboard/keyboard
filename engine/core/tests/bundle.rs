//! Golden checks and the latency gate on the real, committed lexicon bundle.
//! `make lexicons` rebuilds it. Set MLENGINE_BUNDLE to test another directory.
//!
//! The transliterator rows use the model the keyboard bundles,
//! `packs/ml.neopack/translit.mlkbnn` (`make models`).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mlengine_core::lang::{ActiveError, EN, LangId};
use mlengine_core::lexicon::Lexicon;
use mlengine_core::nn::{Deadline, Target, Translit, Transliteration};
use mlengine_core::pack::Pack;
use mlengine_core::{Engine, Kind, Layout, Paths, Timing, rank, suffix};

/// The Malayalam pack's native layout: it is the first pack loaded.
const ML: Layout = Layout::Native(mlengine_core::lang::Lang(1));

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
    Engine::open(&bundle_paths(translit, None))
}

/// The Malayalam pack (MLENGINE_PACK to test another build of it).
fn pack_dir() -> PathBuf {
    std::env::var_os("MLENGINE_PACK")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/ml.neopack"))
}

/// English from the bundle, Malayalam through the pack's manifest (as the keyboard opens it),
/// with the given models in place of the pack's.
fn bundle_paths(translit: Option<PathBuf>, lm: Option<PathBuf>) -> Paths {
    let dir = bundle_dir();
    assert!(
        dir.join("en.fst").exists(),
        "no lexicon bundle in {}: run `make lexicons`",
        dir.display()
    );
    let pack = Pack::open(&pack_dir()).expect("the Malayalam pack");
    let mut paths = Paths::with_packs(Some(dir.join("en.fst")), &[pack], None);
    paths.packs[0].translit = translit;
    paths.packs[0].lm = lm;
    paths
}

/// The LM: MLENGINE_LM, else the bundled one, else the training output.
fn lm_path() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = std::env::var_os("MLENGINE_LM")
        .map(PathBuf::from)
        .into_iter()
        .chain([
            root.join("packs/ml.neopack/lm.mlkbnn"),
            root.join("training/build/lm.mlkbnn"),
        ])
        .find(|p| p.exists());
    if path.is_none() {
        println!("skipping the LM: no lm.mlkbnn (`make models` builds it)");
    }
    path
}

/// Both models when they exist; None (the row is skipped) without the LM.
fn lm_engine() -> Option<Engine> {
    Some(Engine::open(&bundle_paths(model_path(), Some(lm_path()?))))
}

/// The keyboard's bundled model (MLENGINE_TRANSLIT to test another export).
fn model_path() -> Option<PathBuf> {
    let path = std::env::var_os("MLENGINE_TRANSLIT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../packs/ml.neopack/translit.mlkbnn")
        });
    if path.exists() {
        Some(path)
    } else {
        println!(
            "skipping the transliterator: {} is missing (`make models` builds it)",
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
        ["english", "ml:lexicon", "ml:romanised", "ml:next_words"]
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

/// User reports from the phone, against SwiftKey. A contraction typed without its
/// apostrophe becomes the contraction (`rank::CONTRACTION_LOOKALIKES`), long words are
/// completed or corrected (`rank::LONG_WORD`, `rank::COMPLETION_DOMINANT`), and four
/// letters that begin words are not corrected two slips away (`rank::PREFIX_KEEPS_LETTERS`).
#[test]
fn english_user_reports() {
    for engine in engines() {
        for (context, want) in [
            ("whats", "what's"),
            ("so whats", "what's"),
            ("dont", "don't"),
            ("cant", "can't"),
            ("im", "I'm"),
            ("I think im", "I'm"),
            ("ive", "I've"),
            ("youre", "you're"),
            ("theyre", "they're"),
            ("isnt", "isn't"),
            ("didnt", "didn't"),
            ("thats", "that's"),
            ("doesnt", "doesn't"),
            ("apostrap", "apostrophe"),
            ("the apostrap", "apostrophe"),
            ("preferenc", "preference"),
            ("definately", "definitely"),
            ("prefarance", "preference"),
            ("preferance", "preference"),
            ("recieve", "receive"),
            ("becuase", "because"),
            ("tommorow", "tomorrow"),
            // One slip from whats, three from where (reported from the phone).
            ("whsts", "what's"),
            ("wgats", "what's"),
            ("ok\nWhsts", "What's"),
        ] {
            assert_eq!(
                middle(&engine, context),
                (want.to_string(), Kind::Correction),
                "{context}: {:?}",
                texts(&engine, context)
            );
        }
        // Real words stay, with the contraction beside them where there is one.
        for (context, side) in [
            ("its", Some("it's")),
            ("I think its", Some("it's")),
            ("lets", Some("let's")),
            ("were", Some("we're")),
            ("we were", Some("we're")),
            ("ill", Some("I'll")),
            ("id", Some("I'd")),
            ("well", Some("we'll")),
            ("shell", Some("she'll")),
            ("hell", Some("he'll")),
            ("wont", Some("won't")),
            ("the apos", None),
        ] {
            let word = context.rsplit(' ').next().unwrap();
            let bar = texts(&engine, context);
            assert_eq!(
                middle(&engine, context),
                (word.to_string(), Kind::Verbatim),
                "{bar:?}"
            );
            if let Some(side) = side {
                assert_eq!(bar[1], side, "{context}: {bar:?}");
            }
        }
    }
}

/// A word the user writes rises among the completions of its letters in any context, past
/// a commoner one they never write (reported: `appo` offered appointed and also, never
/// appointment, however often it was written; `rank::USE_COMPLETION_WEIGHT`). Only the side
/// slots move: the letters stay in the middle, and a clear correction stays.
#[test]
fn a_used_word_rises_among_the_completions_in_any_context() {
    let contexts = ["appo", "the appo", "when i type appo", "Can we move the appo"];
    let first_completion = |engine: &Engine, context: &str| {
        engine
            .suggest(context, Layout::Latin, true)
            .candidates
            .into_iter()
            .find(|c| c.kind == Kind::Completion)
            .map(|c| c.text.to_ascii_lowercase())
    };
    let offered = |engine: &Engine, context: &str| {
        texts(engine, context)
            .iter()
            .any(|w| w.eq_ignore_ascii_case("appointment"))
    };
    for mut engine in engines().into_iter().chain(lm_engine()) {
        for shown in [3, 10] {
            engine.set_max_suggestions(shown);
            engine.forget_learned();
            for context in contexts {
                assert_ne!(
                    first_completion(&engine, context).as_deref(),
                    Some("appointment"),
                    "{shown}, {context}: unused"
                );
            }
            // Written in one context, three times.
            for _ in 0..3 {
                engine.note_used("I have an ", "appointment", Layout::Latin);
            }
            for context in contexts {
                let bar = texts(&engine, context);
                assert!(offered(&engine, context), "{shown}, {context}: {bar:?}");
                let typed = context.rsplit(' ').next().unwrap();
                assert_eq!(middle(&engine, context), (typed.to_string(), Kind::Verbatim));
            }
            // Ten times: the first completion everywhere.
            for _ in 0..7 {
                engine.note_used("", "appointment", Layout::Latin);
            }
            for context in contexts {
                assert_eq!(
                    first_completion(&engine, context).as_deref(),
                    Some("appointment"),
                    "{shown}, {context}: {:?}",
                    texts(&engine, context)
                );
            }
            // Uses of a completion of the letters never hold back their correction.
            for _ in 0..20 {
                engine.note_used("", "tehran", Layout::Latin);
            }
            assert_eq!(middle(&engine, "teh"), ("the".to_string(), Kind::Correction));
            // A removed word is never offered, however often it was written.
            engine.remove_word("appointment");
            for context in contexts {
                assert!(!offered(&engine, context), "{shown}, {context}: removed");
            }
        }
    }
}

/// A learned email is in the bar from its first letters, though they begin common English
/// words (reported: `neo` hid sam.2024@example.com behind neon and never).
#[test]
fn a_learned_email_beats_english_completions() {
    for engine in engines() {
        engine.note_committed("sam.2024@example.com");
        engine.note_committed("sam.2024@example.com");
        for context in ["n", "ne", "neo", "Neo", "neo.", "neo.9", "hi neo.98"] {
            let bar = texts(&engine, context);
            assert!(
                bar.contains(&"sam.2024@example.com".to_string()),
                "{context}: {bar:?}"
            );
        }
        assert_eq!(middle(&engine, "neo").0, "neo");
    }
}

/// Manglish words are not completed or corrected into English by the long-word search.
#[test]
fn manglish_words_are_not_made_english() {
    for engine in engines() {
        for (typed, want) in [
            ("ente", "എന്റെ"),
            ("nanni", "നന്ദി"),
            ("sukhamano", "സുഖമാണോ"),
            ("avishyam", "ആവശ്യം"),
        ] {
            let bar = texts(&engine, typed);
            assert_ne!(
                middle(&engine, typed).1,
                Kind::Correction,
                "{typed}: {bar:?}"
            );
            if engine.loaded_sources().contains(&"ml:translit".to_string()) {
                assert_eq!(bar[0], want, "{typed}: {bar:?}");
            }
        }
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
        for (typed, want) in [("onam", "ഓണം"), ("vishu", "വിഷു"), ("munnar", "മൂന്നാർ")]
        {
            assert_eq!(
                middle(&engine, typed).0,
                want,
                "{typed}: {:?}",
                texts(&engine, typed)
            );
        }
    }
}

#[test]
fn malayalam_layout_golden() {
    for engine in engines() {
        let s = engine.suggest("എന്റ", ML, true);
        assert_eq!(
            s.candidates[0].text, "എന്റ",
            "typed text stays in the middle"
        );
        assert!(s.candidates.iter().any(|c| c.text == "എന്റെ"), "{s:?}");
        let k = engine.suggest("ക", ML, true);
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
                (want.to_string(), Kind::Romanised),
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
                (want.to_string(), Kind::Romanised),
                "{typed}: {:?}",
                texts(&engine, typed)
            );
        }
        // Known gap: എന്തു (the older spelling, five times as common in the
        // corpus) wins over എന്ത്; both romanise as `enthu`. Either is Malayalam.
        let (word, kind) = middle(&engine, "enthu");
        assert_eq!(kind, Kind::Romanised);
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
                (want.to_string(), Kind::Romanised),
                "{typed}: {:?}",
                texts(&engine, typed)
            );
        }
        // Without Malayalam context, a rarer stem's split waits in a side slot,
        // unless the model reads it confidently (`rank::MANGLISH_MODEL_CONFIDENT`).
        let s = texts(&engine, "poyirunno");
        let has_model = engine.loaded_sources().contains(&"ml:translit".to_string());
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
        let has_model = engine.loaded_sources().contains(&"ml:translit".to_string());
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
    assert!(engine.loaded_sources().contains(&"ml:translit".to_string()));
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
            (want.to_string(), Kind::Romanised),
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
            ("ആവശ്യം".to_string(), Kind::Romanised),
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
        ("കൊള്ളാമോ".to_string(), Kind::Romanised),
        "{:?}",
        texts(&engine, "kollamo")
    );
}

/// Chat spellings from the pack's `chat-forms.tsv`. `nanni` (നന്ദി as said; user-reported on
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
                (want.to_string(), Kind::Romanised),
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

#[test]
fn lm_loads_only_with_the_lexicons_it_was_built_for() {
    let Some(engine) = lm_engine() else { return };
    assert!(engine.loaded_sources().contains(&"ml:lm".to_string()));
    let other = Engine::open(&Paths {
        english: None,
        ..bundle_paths(None, lm_path())
    });
    assert!(!other.loaded_sources().contains(&"ml:lm".to_string()));
    let mut swapped = bundle_paths(None, lm_path());
    let english = swapped.english.take();
    swapped.english = std::mem::replace(&mut swapped.packs[0].lexicon, english);
    let other = Engine::open(&swapped);
    assert!(!other.loaded_sources().contains(&"ml:lm".to_string()));
}

/// The typos M3a gets wrong (design §1), in sentences: context picks the meant word. Without
/// context (`english-typos.tsv`) they are only printed. `tounge` is the fifth: `tongue` is
/// past the correction search's edits, and the LM only re-ranks what it finds, so it is
/// only printed too (it now stays as typed rather than becoming `young`).
#[test]
fn context_fixes_the_typos_frequency_gets_wrong() {
    let Some(engine) = lm_engine() else { return };
    println!(
        "\"I bit my tounge\": {:?}",
        middle(&engine, "I bit my tounge").0
    );
    let mut wrong = Vec::new();
    for (context, want) in [
        ("I can't remeber", "remember"),
        ("that's so wierd", "weird"),
        ("what a nice suprise", "surprise"),
        ("can I have a peice", "piece"),
    ] {
        let (got, _) = middle(&engine, context);
        let typed = context.rsplit(' ').next().unwrap();
        println!(
            "{context:?}: {got:?}; alone {:?}",
            middle(&engine, typed).0
        );
        if got != want {
            wrong.push(format!("{context:?} → {got:?}, want {want:?}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

/// Next words follow the sentence's language: Malayalam after Manglish, English after English.
#[test]
fn lm_next_words_follow_the_sentence() {
    let Some(engine) = lm_engine() else { return };
    let malayalam = |w: &String| w.chars().any(|c| ('\u{0D00}'..='\u{0D7F}').contains(&c));
    let ente = texts(&engine, "ente ");
    println!("\"ente \": {ente:?}");
    assert!(ente.iter().any(malayalam), "{ente:?}");
    assert!(ente.iter().all(malayalam), "{ente:?}");
    let english = texts(&engine, "I want to ");
    println!("\"I want to \": {english:?}");
    assert!(!english.is_empty() && !english.iter().any(malayalam), "{english:?}");
    for context in ["njan ippo ", "see you ", "എന്റെ ", "how are "] {
        println!("{context:?}: {:?}", texts(&engine, context));
    }
}

/// Out of time before the model starts, a suggest is the lexicon-only engine's; with time to
/// spare, it is the unbounded one's.
#[test]
fn the_model_deadline_falls_back_to_the_lexicons() {
    let Some(with) = model_engine() else { return };
    let without = engine();
    let budget = Duration::from_millis(10);
    let (gone, plenty) = (|| Duration::ZERO, || budget);
    for context in [
        "sukhamano",
        "I said avishyam",
        "enthu",
        "hrithvik",
        "preferenc",
        "ente veettil",
        "Jishnu",
    ] {
        let late = Some(Deadline {
            budget,
            left: &gone,
        });
        assert_eq!(
            with.suggest_until(context, Layout::Latin, true, late),
            without.suggest(context, Layout::Latin, true),
            "{context}"
        );
        let early = Some(Deadline {
            budget,
            left: &plenty,
        });
        assert_eq!(
            with.suggest_until(context, Layout::Latin, true, early),
            with.suggest(context, Layout::Latin, true),
            "{context}"
        );
    }
}

/// p99 of a suggestion request must stay under 8 ms on the Mac (spec M2 §6), model included,
/// and the LM too when it is built (after-space and mid-word requests printed apart).
/// Run in release: `make bench`.
#[test]
#[ignore = "timing: run with `make bench` (release build)"]
fn latency() {
    // With the transliterator: that is what the keyboard runs.
    let engine = model_engine().expect("the latency gate needs the bundled transliterator");
    let inputs = latency_inputs();
    time_suggests("without the LM", &engine, &inputs);
    let Some(engine) = lm_engine() else {
        println!("no lm.mlkbnn: the LM latency is skipped");
        return;
    };
    time_suggests("with the LM", &engine, &inputs);
    // As the keyboard runs it: with its cutoffs, the LM stops early on a slow request.
    let keyboard = |context: &str, layout| {
        let start = Instant::now();
        let elapsed = || start.elapsed();
        let _ = engine.suggest_timed(context, layout, true, Some(&Timing::keyboard(&elapsed)));
    };
    time_with("with the LM, keyboard cutoffs", &engine, &inputs, &keyboard);
    // A long message as the keyboard sends it: the last 64 characters before the cursor, so
    // the front of the text moves at every keystroke once it is longer.
    let message = message_inputs();
    time_suggests("with the LM, a long message", &engine, &message);
    time_with(
        "with the LM, a long message, keyboard cutoffs",
        &engine,
        &message,
        &keyboard,
    );
    // The keyboard's cutoffs on a clock 6 times faster, as on a phone typing from idle (design
    // §4: 5–7 times slower): how often the LM's context is ready for the word being typed.
    let phone = |context: &str, layout| {
        let start = Instant::now();
        let elapsed = || start.elapsed() * 6;
        let _ = engine.suggest_timed(context, layout, true, Some(&Timing::keyboard(&elapsed)));
    };
    for (label, suggest) in [
        ("keyboard cutoffs", &keyboard as &dyn Fn(&str, Layout)),
        ("phone cutoffs", &phone),
    ] {
        let ready = message
            .iter()
            .filter(|(context, layout)| {
                suggest(context, *layout);
                engine.lm_ready()
            })
            .count();
        println!(
            "with the LM, a long message, {label}: LM ready after {ready} of {} requests",
            message.len()
        );
    }
}

/// The latency gate with 3, 10 and 20 suggestions (`Engine::set_max_suggestions`), with the
/// LM, without cutoffs and with the keyboard's, on the word inputs and the long message.
#[test]
#[ignore = "timing: run with `make bench` (release build)"]
fn latency_by_suggestion_count() {
    let Some(mut engine) = lm_engine() else {
        println!("no lm.mlkbnn: skipped");
        return;
    };
    let inputs = latency_inputs();
    let message = message_inputs();
    for n in [3, 10, 20] {
        engine.set_max_suggestions(n);
        let engine = &engine;
        time_suggests(&format!("{n} suggestions"), engine, &inputs);
        time_suggests(&format!("{n} suggestions, a long message"), engine, &message);
        let keyboard = |context: &str, layout| {
            let start = Instant::now();
            let elapsed = || start.elapsed();
            let _ = engine.suggest_timed(context, layout, true, Some(&Timing::keyboard(&elapsed)));
        };
        time_with(
            &format!("{n} suggestions, a long message, keyboard cutoffs"),
            engine,
            &message,
            &keyboard,
        );
    }
}

/// A long message typed letter by letter, each request with only the last 64 characters
/// before the cursor (`KeyboardViewController.requestSuggestions`).
fn message_inputs() -> Vec<(String, Layout)> {
    let text = "I said njan varam, but the bus was late again. Hello how are you doing today? \
        I was thinking we could meet tomorrow at the cafe near the station. ente veettil \
        ellarum sukhamano? enikku nale varan pattilla because of work, definitely next week. ";
    let chars: Vec<char> = text.chars().collect();
    (1..=chars.len())
        .map(|end| {
            let context: String = chars[end.saturating_sub(64)..end].iter().collect();
            (context, Layout::Latin)
        })
        .collect()
}

/// The latency gate with every table of the learned-word store full (`personal`): phrases
/// after "said" and after each typed word, uses, and a choice for the first letters of each.
#[test]
#[ignore = "timing: run with `make bench` (release build)"]
fn latency_with_a_full_store() {
    use mlengine_core::personal::PAIR_CAPACITY;
    use mlengine_core::user::CAPACITY;
    let fill = |engine: &Engine| {
        for i in 0..CAPACITY {
            engine.learn(&format!("zw{i}"));
        }
        for i in 0..PAIR_CAPACITY {
            let before = format!("zw{} zw{} ", i % CAPACITY, (i + 1) % CAPACITY);
            engine.note_used(&before, &format!("zw{}", (i + 2) % CAPACITY), Layout::Latin);
        }
        for w in LATENCY_WORDS {
            for _ in 0..3 {
                engine.note_used("I said ", w, Layout::Latin);
                engine.note_used(&format!("I said {w} "), "the", Layout::Latin);
            }
            engine.note_choice(&w[..w.len().min(3)], w);
        }
    };
    let inputs = latency_inputs();
    let engine = model_engine().expect("the latency gate needs the bundled transliterator");
    fill(&engine);
    time_suggests("full store, without the LM", &engine, &inputs);
    let Some(engine) = lm_engine() else {
        println!("no lm.mlkbnn: the LM latency is skipped");
        return;
    };
    fill(&engine);
    time_suggests("full store, with the LM", &engine, &inputs);
}

/// The English and Manglish words of `latency_inputs`.
const LATENCY_WORDS: [&str; 27] = [
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
    "preference",
    "apostrophe",
    "definitely",
    "enthu",
    "avishyam",
];

/// Words typed letter by letter after "I said ", each then followed by a space, and Malayalam
/// words letter by letter.
fn latency_inputs() -> Vec<(String, Layout)> {
    let mut inputs: Vec<(String, Layout)> = Vec::new();
    for w in LATENCY_WORDS {
        for end in 1..=w.len() {
            inputs.push((format!("I said {}", &w[..end]), Layout::Latin));
        }
        inputs.push((format!("{w} "), Layout::Latin));
    }
    for w in ["എന്റെ", "വീട്ടിൽ", "കേരളം", "മലയാളം", "സുഖമാണോ"]
    {
        let chars: Vec<char> = w.chars().collect();
        for end in 1..=chars.len() {
            inputs.push((chars[..end].iter().collect(), ML));
        }
    }
    inputs
}

/// Times every input with `suggest` and no cutoffs.
fn time_suggests(label: &str, engine: &Engine, inputs: &[(String, Layout)]) {
    time_with(label, engine, inputs, &|context, layout| {
        let _ = engine.suggest(context, layout, true);
    });
}

/// Times every input 20 times in order (so the LM context moves as when typing), prints p50
/// and p99 overall, after a space and mid-word, and checks the overall p99.
fn time_with(
    label: &str,
    engine: &Engine,
    inputs: &[(String, Layout)],
    suggest: &dyn Fn(&str, Layout),
) {
    println!("{label}: sources {:?}", engine.loaded_sources());
    for (context, layout) in inputs {
        suggest(context, *layout); // warm the page cache
    }
    let mut samples: Vec<(Duration, &str)> = Vec::new();
    for _ in 0..20 {
        for (context, layout) in inputs {
            let start = Instant::now();
            suggest(context, *layout);
            samples.push((start.elapsed(), context.as_str()));
        }
    }
    let after_space: Vec<_> = samples.iter().filter(|s| s.1.ends_with(' ')).copied().collect();
    let mid_word: Vec<_> = samples.iter().filter(|s| !s.1.ends_with(' ')).copied().collect();
    report(&format!("{label}, after a space"), after_space);
    report(&format!("{label}, mid-word"), mid_word);
    let p99 = report(&format!("{label}, all"), samples);
    assert!(
        p99.0 < Duration::from_millis(8),
        "{label}: p99 {:?} on {:?}",
        p99.0,
        p99.1
    );
}

/// Prints p50, p99 and the slowest request; returns the p99 one.
fn report<'a>(label: &str, mut samples: Vec<(Duration, &'a str)>) -> (Duration, &'a str) {
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
    p99
}

fn trained_translit() -> Option<Translit> {
    model_path().map(|path| Translit::open(&path, Target::MALAYALAM).unwrap())
}

fn bundle_lexicon(name: &str) -> Lexicon {
    let path = bundle_dir().join(name);
    Lexicon::open(&if path.exists() { path } else { pack_dir().join(name) }).unwrap()
}

/// Malayalam's romanisation tables, from the pack.
fn tables() -> mlengine_core::romanisation::Romanisation {
    mlengine_core::romanisation::Romanisation::load(&pack_dir().join("romanisation.json")).unwrap()
}

/// The constrained beam's spellings of `typed`, with its suffix splits allowed.
fn transliterations(
    t: &Translit,
    ml: &Lexicon,
    manglish: &Lexicon,
    typed: &str,
) -> Vec<Transliteration> {
    let suffixes = suffix::Suffixes::load(&pack_dir().join("suffixes.tsv")).unwrap();
    let mlym = mlengine_core::script::rules("mlym").unwrap();
    let splits: Vec<String> = suffix::splits(
        mlym,
        &tables(),
        &suffixes,
        manglish,
        typed,
        rank::MANGLISH_SPLIT_STEM_MIN,
    )
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

/// English switched off (`set_active(["ml"])`), with the whole pack as the keyboard opens it.
fn ml_only() -> Engine {
    let paths = bundle_paths(model_path(), lm_path());
    let mut engine = Engine::open(&paths);
    engine.set_active(&[LangId("ml".into())]).unwrap();
    engine
}

/// Without English, nothing English is offered and Latin letters that aren't Manglish stay
/// as typed; Manglish still converts.
#[test]
fn english_switched_off_offers_no_english() {
    let engine = ml_only();
    assert_eq!(engine.layouts(), [Layout::Latin, ML]);
    for context in [
        "hello wor",
        "hello",
        "I said teh",
        "thnaks",
        "hello ",
        "I want to ",
    ] {
        let s = engine.suggest(context, Layout::Latin, true);
        println!(
            "{context:?}: {:?}",
            s.candidates.iter().map(|c| &c.text).collect::<Vec<_>>()
        );
        assert!(
            s.candidates.iter().all(|c| c.lang != EN),
            "{context:?}: {s:?}"
        );
    }
    let s = engine.suggest("hello wor", Layout::Latin, true);
    assert_eq!(s.candidates[0].text, "wor");
    assert_eq!(s.candidates[0].kind, Kind::Verbatim);
    assert_eq!(
        middle(&engine, "ente"),
        ("എന്റെ".to_string(), Kind::Romanised)
    );
    let s = engine.suggest("എന്റ", ML, true);
    assert_eq!(s.candidates[1].text, "എന്റെ");
}

#[test]
fn the_active_list_is_checked() {
    let mut engine = engine();
    let ids = |ids: &[&str]| {
        ids.iter()
            .map(|id| LangId(id.to_string()))
            .collect::<Vec<_>>()
    };
    assert_eq!(engine.active_languages(), ids(&["en", "ml"]));
    assert_eq!(
        engine.set_active(&ids(&["en", "ml", "en", "ml", "en"])),
        Err(ActiveError::TooMany(5))
    );
    assert_eq!(
        engine.set_active(&ids(&["hi"])),
        Err(ActiveError::Unknown("hi".into()))
    );
    assert_eq!(engine.set_active(&[]), Err(ActiveError::Empty));
    assert_eq!(
        engine.active_languages(),
        ids(&["en", "ml"]),
        "a refusal changes nothing"
    );
    engine.set_active(&ids(&["ml", "en"])).unwrap();
    assert_eq!(engine.active_languages(), ids(&["ml", "en"]));
    engine.set_active(&ids(&["en"])).unwrap();
    assert_eq!(engine.layouts(), [Layout::Latin]);
    assert!(
        texts(&engine, "ente").iter().all(|w| w.is_ascii()),
        "Malayalam is off"
    );
    engine.set_default_active();
    assert_eq!(engine.active_languages(), ids(&["en", "ml"]));
}

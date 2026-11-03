//! Golden behaviour for Manglish in Auto mode: English and Malayalam
//! candidates compete in one bar. Counts are per billion words.

use mlengine_core::lexicon::{Lexicon, build};
use mlengine_core::user::UserStore;
use mlengine_core::{Engine, Kind, Layout, Sources, manglish};

#[rustfmt::skip]
const ENGLISH: &[(&str, u64)] = &[
    ("the", 50_000_000), ("I", 8_000_000), ("my", 2_000_000), ("feel", 200_000),
    ("hello", 30_000), ("man", 500_000), ("enter", 120_000), ("pain", 300_000),
    ("panic", 40_000), ("end", 400_000), ("and", 25_000_000), ("then", 1_400_000),
    ("potato", 30_000),
];

#[rustfmt::skip]
const MALAYALAM: &[(&str, u64)] = &[
    ("എന്റെ", 3_000_000), ("എന്ത്", 1_500_000), ("ഞാൻ", 2_500_000), ("വീട്", 400_000),
    ("വീട്ടിൽ", 300_000), ("അവൻ", 900_000), ("മലയാളം", 600_000), ("പണി", 20_000),
    ("ഹലോ", 5_000), ("മാൻ", 20_000), ("ഇല്ല", 2_000_000), ("അമ്മ", 700_000),
    ("എന്റേത്", 50_000), ("തേ", 3_000), ("ഉറങ്ങി", 22), ("നന്ദി", 300_000),
    ("നണ്ണി", 40_000),
    // Stems for suffix splitting; none of the joined forms is listed.
    ("സുഖം", 300_000), ("കഴിച്ചു", 200_000), ("വരുന്നു", 400_000), ("വന്നു", 800_000),
    ("പൊട്ടാട്ട്", 50_000),
];

fn lexicon(entries: Vec<(Vec<u8>, u64)>) -> Option<Lexicon> {
    Some(Lexicon::from_bytes(build(entries).unwrap()).unwrap())
}

fn engine() -> Engine {
    let en = ENGLISH
        .iter()
        .map(|(w, c)| (w.as_bytes().to_vec(), *c))
        .collect();
    let ml: Vec<(String, u64)> = MALAYALAM.iter().map(|(w, c)| (w.to_string(), *c)).collect();
    let sources = Sources {
        english: lexicon(en),
        malayalam: lexicon(
            ml.iter()
                .map(|(w, c)| (w.as_bytes().to_vec(), *c))
                .collect(),
        ),
        manglish: lexicon(manglish::entries(&ml)),
        next_words: None,
        translit: None,
        lm: None,
    };
    Engine::new(sources, UserStore::in_memory())
}

fn middle(engine: &Engine, context: &str) -> (String, Kind) {
    let c = &engine.suggest(context, Layout::Latin, true).candidates[0];
    (c.text.clone(), c.kind)
}

fn has(engine: &Engine, context: &str, text: &str) -> bool {
    engine
        .suggest(context, Layout::Latin, true)
        .candidates
        .iter()
        .any(|c| c.text == text && c.kind == Kind::Manglish)
}

#[test]
fn ente_becomes_malayalam() {
    let engine = engine();
    for typed in ["ente", "ende", "entey", "അവൻ ente", "ഞാൻ ende"] {
        assert_eq!(
            middle(&engine, typed),
            ("എന്റെ".into(), Kind::Manglish),
            "{typed}"
        );
    }
}

#[test]
fn manglish_words_people_type() {
    let engine = engine();
    assert_eq!(middle(&engine, "njan"), ("ഞാൻ".into(), Kind::Manglish));
    assert_eq!(middle(&engine, "veedu").0, "വീട്");
    assert_eq!(middle(&engine, "avan").0, "അവൻ");
    assert_eq!(middle(&engine, "illa").0, "ഇല്ല");
    assert_eq!(middle(&engine, "ഞാൻ veettil").0, "വീട്ടിൽ");
}

#[test]
fn english_words_stay_english() {
    let engine = engine();
    for word in ["hello", "the", "man", "enter", "my", "end", "then"] {
        assert_eq!(
            middle(&engine, word),
            (word.to_string(), Kind::Verbatim),
            "{word}"
        );
    }
}

#[test]
fn english_corrections_still_work() {
    let engine = engine();
    assert_eq!(middle(&engine, "teh"), ("the".into(), Kind::Correction));
    assert_eq!(middle(&engine, "helo").0, "hello");
}

#[test]
fn malayalam_shows_in_a_side_slot_for_english_words() {
    let s = engine().suggest("man", Layout::Latin, true);
    assert_eq!(s.candidates[0].text, "man");
    assert!(
        s.candidates
            .iter()
            .any(|c| c.text == "മാൻ" && c.kind == Kind::Manglish)
    );
}

#[test]
fn completes_long_words_from_a_prefix() {
    let engine = engine();
    assert!(has(&engine, "malaya", "മലയാളം"));
    // A prefix match is a completion: it never takes the middle.
    assert_eq!(middle(&engine, "malaya"), ("malaya".into(), Kind::Verbatim));
}

#[test]
fn previous_word_language_biases_the_choice() {
    let engine = engine();
    assert_eq!(middle(&engine, "ഞാൻ pani").0, "പണി");
    assert_eq!(middle(&engine, "I feel pani").0, "pain");
}

#[test]
fn unknown_names_get_a_transliteration_in_a_side_slot() {
    let engine = engine();
    for typed in ["Jishnu", "jishnu", "ഞാൻ jishnu"] {
        let s = engine.suggest(typed, Layout::Latin, true);
        let word = typed.rsplit(' ').next().unwrap();
        assert_eq!(s.candidates[0].text, word, "{typed}");
        assert_eq!(s.candidates[0].kind, Kind::Verbatim, "{typed}");
        assert!(
            s.candidates[1..]
                .iter()
                .any(|c| c.kind == Kind::Manglish && c.text == "ജിഷ്ണു"),
            "{typed}: {s:?}"
        );
    }
}

#[test]
fn a_rare_malayalam_word_needs_malayalam_context_to_replace() {
    // The exact-skeleton bonus ranks a word against English; it doesn't lift
    // a rare word over the `MIN_REPLACE` floor. Only the context can.
    let engine = engine();
    assert_eq!(middle(&engine, "urangi"), ("urangi".into(), Kind::Verbatim));
    assert!(has(&engine, "urangi", "ഉറങ്ങി"), "offered on the side");
    assert_eq!(middle(&engine, "അമ്മ urangi").0, "ഉറങ്ങി");
}

#[test]
fn a_learned_latin_word_is_not_converted() {
    let engine = engine();
    engine.learn("ente");
    assert_eq!(middle(&engine, "ente"), ("ente".into(), Kind::Verbatim));
    assert!(has(&engine, "ente", "എന്റെ"), "still offered on the side");
}

#[test]
fn autocorrect_off_keeps_manglish_out_of_the_middle() {
    let s = engine().suggest("ente", Layout::Latin, false);
    assert_eq!(s.candidates[0].text, "ente");
    assert_eq!(s.candidates[1].text, "എന്റെ");
}

#[test]
fn without_the_manglish_source_latin_stays_english() {
    let engine = Engine::new(
        Sources {
            english: lexicon(
                ENGLISH
                    .iter()
                    .map(|(w, c)| (w.as_bytes().to_vec(), *c))
                    .collect(),
            ),
            ..Sources::default()
        },
        UserStore::in_memory(),
    );
    let s = engine.suggest("ente", Layout::Latin, true);
    assert_eq!(s.candidates[0].text, "ente");
    assert!(s.candidates.iter().all(|c| c.kind != Kind::Manglish));
}

#[test]
fn agglutinated_forms_split_into_stem_and_suffix() {
    let engine = engine();
    for (typed, want) in [
        ("sukhamano", "സുഖമാണോ"),    // സുഖം + ആണോ: ം becomes മ + the vowel sign
        ("kazhicho", "കഴിച്ചോ"),    // കഴിച്ചു + ഓ: ു gives way to the vowel sign
        ("varunnundo", "വരുന്നുണ്ടോ"), // വരുന്നു + ഉണ്ട് + ഓ: two suffixes
        ("vanno", "വന്നോ"),
    ] {
        assert_eq!(
            middle(&engine, typed),
            (want.to_string(), Kind::Manglish),
            "{typed}: {:?}",
            engine.suggest(typed, Layout::Latin, true)
        );
    }
}

#[test]
fn english_words_are_not_split() {
    // potato = potat + o, and പൊട്ടാട്ട് has the skeleton of potat.
    let engine = engine();
    let s = engine.suggest("potato", Layout::Latin, true);
    assert_eq!(s.candidates[0].text, "potato");
    assert!(s.candidates.iter().all(|c| c.text != "പൊട്ടാട്ടോ"), "{s:?}");
}

#[test]
fn a_whole_form_beats_a_split() {
    // veettil is വീട്ടിൽ; the split വീട് + ഇൽ would give വീടിൽ.
    let engine = engine();
    let s = engine.suggest("ഞാൻ veettil", Layout::Latin, true);
    assert_eq!(s.candidates[0].text, "വീട്ടിൽ");
    assert!(s.candidates.iter().all(|c| c.text != "വീടിൽ"), "{s:?}");
}

#[test]
fn short_words_are_not_split() {
    // vano (4 letters) could be വന്നു + ഓ, like vanno.
    let engine = engine();
    assert!(!has(&engine, "vano", "വന്നോ"));
    assert!(has(&engine, "vanno", "വന്നോ"));
}

#[test]
fn chat_forms_reach_their_word() {
    // `data/chat-forms.tsv`: nanni is നന്ദി as said, though ന്ദ romanises nd and the
    // skeleton lookup alone finds only നണ്ണി. nte is എന്റെ with the vowel dropped.
    let engine = engine();
    for (typed, want) in [("nanni", "നന്ദി"), ("Nanni", "നന്ദി"), ("nte", "എന്റെ")]
    {
        assert_eq!(
            middle(&engine, typed),
            (want.into(), Kind::Manglish),
            "{typed}"
        );
    }
    assert_eq!(middle(&engine, "nandi").0, "നന്ദി");
}

#[test]
fn a_capitalised_word_after_english_stays_a_name() {
    let engine = engine();
    assert_eq!(middle(&engine, "and ente").0, "എന്റെ");
    assert_eq!(middle(&engine, "and Ente"), ("Ente".into(), Kind::Verbatim));
    assert!(has(&engine, "and Ente", "എന്റെ"));
    // Not after Malayalam, and not at the start, where the keyboard capitalises.
    assert_eq!(middle(&engine, "Ente").0, "എന്റെ");
    assert_eq!(middle(&engine, "ഞാൻ Ente").0, "എന്റെ");
}

#[test]
fn a_removed_manglish_word_is_not_suggested() {
    let engine = engine();
    assert_eq!(middle(&engine, "ente"), ("എന്റെ".into(), Kind::Manglish));
    engine.remove_word("എന്റെ");
    assert!(!has(&engine, "ente", "എന്റെ"));
    assert_ne!(middle(&engine, "ente").0, "എന്റെ");
}

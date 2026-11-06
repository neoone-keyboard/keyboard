//! Ranking on lexicons with thousands of keys under one prefix. A real
//! lexicon has them (`Co…` names, Malayalam words under `ക`), and a scan that
//! stops after N keys in key order would miss the common words.

use mlengine_core::lexicon::{Lexicon, build};
use mlengine_core::user::UserStore;
use mlengine_core::lang::LangId;
use mlengine_core::{Engine, Layout, PackSources, Sources};

/// The Malayalam pack's native layout: it is the first pack loaded.
const ML: Layout = Layout::Native(mlengine_core::lang::Lang(1));

/// More decoys than any bounded scan visits.
const DECOYS: usize = 3_500;

fn lex(entries: Vec<(String, u64)>) -> Option<Lexicon> {
    let bytes = build(entries.into_iter().map(|(w, c)| (w.into_bytes(), c))).unwrap();
    Some(Lexicon::from_bytes(bytes).unwrap())
}

/// A Malayalam pack over `entries` with the pack's vowel-sign strip, nothing else loaded.
fn ml_pack(entries: Vec<(String, u64)>) -> PackSources {
    let rules = mlengine_core::script::rules("mlym").unwrap();
    let layout =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/ml.neopack/layout.json");
    PackSources {
        vowel_sign_strip: mlengine_core::pack_layout::PackLayout::load(&layout)
            .unwrap()
            .vowel_sign_strip,
        ..PackSources::new(LangId("ml".into()), rules, lex(entries).unwrap())
    }
}

/// `n` distinct strings, each `len` characters from `alphabet`.
fn tails(alphabet: &[char], len: usize, n: usize) -> Vec<String> {
    (0..n)
        .map(|mut i| {
            (0..len)
                .map(|_| {
                    let c = alphabet[i % alphabet.len()];
                    i /= alphabet.len();
                    c
                })
                .collect()
        })
        .collect()
}

fn texts(engine: &Engine, context: &str, layout: Layout) -> Vec<String> {
    engine
        .suggest(context, layout, true)
        .candidates
        .into_iter()
        .map(|c| c.text)
        .collect()
}

#[test]
fn co_finds_could_among_thousands_of_names() {
    let letters: Vec<char> = ('a'..='z').collect();
    // "Co…" sorts before "could", so a key-order scan sees only names.
    let mut entries: Vec<(String, u64)> = tails(&letters, 4, DECOYS)
        .into_iter()
        .map(|t| (format!("Co{t}"), 3))
        .collect();
    entries.push(("could".into(), 1_500_000));
    let engine = Engine::new(
        Sources {
            english: lex(entries),
            ..Sources::default()
        },
        UserStore::in_memory(),
    );
    let got = texts(&engine, "co", Layout::Latin);
    assert_eq!(got[0], "co", "a completion never takes the middle");
    assert_eq!(got[1], "could");
}

#[test]
fn next_words_are_the_most_frequent_followers() {
    let letters: Vec<char> = ('a'..='v').collect();
    // Followers that sort before "world", "year" and "zebra".
    let mut entries: Vec<(String, u64)> = tails(&letters, 4, DECOYS)
        .into_iter()
        .map(|t| (format!("the\0{t}"), 1))
        .collect();
    entries.extend([
        ("the\0zebra".into(), 10),
        ("the\0year".into(), 30),
        ("the\0world".into(), 50),
        ("then\0we".into(), 900),
        ("th\0x".into(), 800),
    ]);
    let engine = Engine::new(
        Sources {
            packs: vec![PackSources {
                next_words: lex(entries),
                ..ml_pack(Vec::new())
            }],
            ..Sources::default()
        },
        UserStore::in_memory(),
    );
    assert_eq!(
        texts(&engine, "the ", Layout::Latin),
        ["world", "year", "zebra"]
    );
    assert_eq!(texts(&engine, "then ", Layout::Latin), ["we"]);
    assert_eq!(texts(&engine, "th ", Layout::Latin), ["x"]);
}

#[test]
fn ka_finds_karyam_among_thousands_of_words() {
    // Consonants sort before the vowel sign ാ, so "കക…" keys come first.
    let consonants: Vec<char> = ('\u{0D15}'..='\u{0D28}').collect();
    let mut entries: Vec<(String, u64)> = tails(&consonants, 3, DECOYS)
        .into_iter()
        .map(|t| (format!("ക{t}"), 2))
        .collect();
    entries.push(("കാര്യം".into(), 90_000));
    let engine = Engine::new(
        Sources {
            packs: vec![ml_pack(entries)],
            ..Sources::default()
        },
        UserStore::in_memory(),
    );
    let got = texts(&engine, "ക", ML);
    assert_eq!(got[0], "ക");
    assert_eq!(got[1], "കാര്യം");
}

#[test]
fn vowel_strip_is_ordered_by_whole_subtree_counts() {
    let consonants: Vec<char> = ('\u{0D15}'..='\u{0D28}').collect();
    // ാ: 400 words of 10 (4000 in all); ി: one word of 3500.
    let mut entries: Vec<(String, u64)> = tails(&consonants, 2, 400)
        .into_iter()
        .map(|t| (format!("ഖാ{t}"), 10))
        .collect();
    entries.push(("ഖിളി".into(), 3_500));
    let engine = Engine::new(
        Sources {
            packs: vec![ml_pack(entries)],
            ..Sources::default()
        },
        UserStore::in_memory(),
    );
    let s = engine.suggest("ഖ", ML, true);
    assert_eq!(s.vowel_signs, ['\u{0D3E}', '\u{0D3F}']);
}

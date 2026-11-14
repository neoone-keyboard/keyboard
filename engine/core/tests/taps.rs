//! Where the finger landed (`Engine::suggest_tapped`): a letter tapped near another key is cheap
//! to correct to it, one tapped in the middle of its key is not; without taps nothing changes.
//! English and one pack, as the keyboard opens them.

use std::path::{Path, PathBuf};

use mlengine_core::pack::Pack;
use mlengine_core::{Candidate, Engine, Layout, Paths, Suggestions, Tap};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn engine(id: &str) -> Engine {
    let pack = Pack::open(&root().join(format!("packs/{id}.neopack"))).unwrap();
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    Engine::open(&Paths::with_packs(Some(en), &[pack], None))
}

/// A letter tapped in the middle of its key.
fn centre(letter: char) -> Tap {
    Tap { letter, probability: 0.97, others: Vec::new() }
}

fn near(letter: char, probability: f32, other: char, other_probability: f32) -> Tap {
    Tap { letter, probability, others: vec![(other, other_probability)] }
}

/// Every letter of `word` tapped in the middle of its key, but those `at` replaces.
fn taps(word: &str, at: &[(usize, Tap)]) -> Vec<Tap> {
    word.chars()
        .enumerate()
        .map(|(i, c)| at.iter().find(|(j, _)| *j == i).map_or_else(|| centre(c), |(_, t)| t.clone()))
        .collect()
}

fn bar(e: &Engine, context: &str, taps: &[Tap]) -> Suggestions {
    let s = e.suggest_tapped(context, Layout::Latin, true, None, taps);
    let shown: Vec<String> = s.candidates.iter().map(|c| format!("{} {:.2}", c.text, c.score)).collect();
    eprintln!("{context:?} {} taps: {shown:?}", taps.len());
    s
}

fn first(s: &Suggestions) -> &str {
    s.candidates.first().map_or("", |c| c.text.as_str())
}

fn score_of(s: &Suggestions, word: &str) -> Option<f32> {
    s.candidates.iter().find(|c: &&Candidate| c.text == word).map(|c| c.score)
}

#[test]
fn a_tap_near_j_makes_nkan_njan() {
    let mut ml = engine("ml");
    ml.set_max_suggestions(10);
    let today = bar(&ml, "nkan", &[]);
    // The second tap between k and j.
    let between = bar(&ml, "nkan", &taps("nkan", &[(1, near('k', 0.55, 'j', 0.45))]));
    assert_eq!(first(&between), "ഞാൻ");
    let (Some(was), Some(now)) = (score_of(&today, "ഞാൻ"), score_of(&between, "ഞാൻ")) else {
        panic!("ഞാൻ should be offered both ways");
    };
    assert!(now > was, "{now} should beat {was}");
    // In the middle of k: no stronger than without taps.
    let middle = bar(&ml, "nkan", &taps("nkan", &[(1, centre('k'))]));
    assert!(score_of(&middle, "ഞാൻ").is_none_or(|s| s <= was), "{middle:?}");
}

#[test]
fn a_tap_near_h_makes_tge_the() {
    let mut ml = engine("ml");
    ml.set_max_suggestions(10);
    let today = bar(&ml, "I saw tge", &[]);
    let between = bar(&ml, "I saw tge", &taps("tge", &[(1, near('g', 0.6, 'h', 0.4))]));
    assert_eq!(first(&between), "the");
    assert!(score_of(&between, "the") > score_of(&today, "the"));
    let middle = bar(&ml, "I saw tge", &taps("tge", &[]));
    assert!(score_of(&middle, "the") <= score_of(&today, "the"));
}

/// A word typed right keeps its place however close each tap came to another key.
#[test]
fn a_word_typed_right_stays_first() {
    let mut ml = engine("ml");
    ml.set_max_suggestions(10);
    let unsure = |word: &str, others: &str| -> Vec<Tap> {
        word.chars().zip(others.chars()).map(|(c, o)| near(c, 0.52, o, 0.48)).collect()
    };
    for (context, word, others, want) in [
        ("", "njan", "hkam", "ഞാൻ"),
        ("", "veettil", "cwwrryk", "വീട്ടിൽ"),
        ("I said ", "hello", "jwkki", "hello"),
        ("see you ", "tomorrow", "rinieeiw", "tomorrow"),
        ("ente ", "amma", "snns", "അമ്മ"),
    ] {
        let without = bar(&ml, &format!("{context}{word}"), &[]);
        let with = bar(&ml, &format!("{context}{word}"), &unsure(word, others));
        assert_eq!(first(&without), want, "{word}");
        assert_eq!(first(&with), want, "{word}");
    }
}

/// Taps that aren't the word's letters (the word was edited, or pasted) are ignored: the bar
/// is the one without taps.
#[test]
fn taps_that_dont_match_the_word_change_nothing() {
    let ml = engine("ml");
    let today = bar(&ml, "nkan", &[]);
    let other = bar(&ml, "nkan", &taps("xyzq", &[(1, near('y', 0.55, 'u', 0.45))]));
    assert_eq!(today, other);
    // Nor do taps for a word on a native layout.
    let native = ml.suggest("എന്", Layout::Native(ml.lang_of_id("ml").unwrap()), true);
    let tapped = ml.suggest_tapped(
        "എന്",
        Layout::Native(ml.lang_of_id("ml").unwrap()),
        true,
        None,
        &taps("abc", &[]),
    );
    assert_eq!(native, tapped);
}

#[test]
fn hindi_tap_near_e_makes_mwra_mera() {
    let hi = engine("hi");
    let between = bar(&hi, "mwra", &taps("mwra", &[(1, near('w', 0.55, 'e', 0.45))]));
    let native: Vec<&str> = between.candidates.iter().map(|c| c.text.as_str()).filter(|t| !t.is_ascii()).collect();
    assert_eq!(native.first(), Some(&"मेरा"), "{between:?}");
}

/// How long a suggest takes with every letter tapped off-centre (each naming the key beside
/// it), against the same requests without taps: `cargo test --release --test taps --
/// --ignored --nocapture tapped_latency`.
#[test]
#[ignore = "timing: run in release on a quiet machine"]
fn tapped_latency() {
    use std::time::{Duration, Instant};
    const ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
    let beside = |c: char| -> char {
        let row = ROWS.iter().find(|r| r.contains(c)).unwrap();
        let i = row.find(c).unwrap();
        row.chars().nth(if i + 1 < row.len() { i + 1 } else { i - 1 }).unwrap()
    };
    let words = [
        "enikku", "varunnundo", "kazhichille", "sukhamano", "thiruvananthapuram", "pattilla", "veettil",
        "ellarum", "nale", "njan", "definitely", "apostrophe", "beautiful", "keyboard", "something",
        "wonderful", "preference", "station", "because", "malayalam",
    ];
    let ml = engine("ml");
    let mut plain = Vec::new();
    let mut tapped = Vec::new();
    for round in 0..3 {
        for word in words {
            for end in 1..=word.len() {
                let typed = &word[..end];
                let context = format!("I said {typed}");
                let t: Vec<Tap> = typed.chars().map(|c| near(c, 0.7, beside(c), 0.25)).collect();
                let start = Instant::now();
                ml.suggest_tapped(&context, Layout::Latin, true, None, &[]);
                let a = start.elapsed();
                let start = Instant::now();
                ml.suggest_tapped(&context, Layout::Latin, true, None, &t);
                let b = start.elapsed();
                if round > 0 {
                    plain.push(a);
                    tapped.push(b);
                }
            }
        }
    }
    let line = |name: &str, times: &mut Vec<Duration>| {
        times.sort();
        let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
        eprintln!("{name}: {} requests, p50 {:?}, p99 {:?}, max {:?}", times.len(), at(0.5), at(0.99), at(1.0));
    };
    line("without taps", &mut plain);
    line("every letter tapped off-centre", &mut tapped);
}

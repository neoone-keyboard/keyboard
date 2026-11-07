//! Per-keystroke latency breakdown on the real bundle and the bundled transliterator:
//! the full suggest with and without the model, and the model's beam alone.
//! `cargo test --release -p mlengine-core --test latency_breakdown -- --ignored --nocapture`

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mlengine_core::lexicon::Lexicon;
use mlengine_core::nn::{Search, Translit};
use mlengine_core::{Engine, Layout, PackPaths, Paths};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn open(model: bool) -> Engine {
    let dir = root().join("apps/ios/KeyboardExtension/Lexicon");
    let pack = root().join("packs/ml.neopack");
    Engine::open(&Paths {
        english: Some(dir.join("en.fst")),
        english_next: None,
        packs: vec![PackPaths {
            lexicon: Some(pack.join("ml.fst")),
            romanised: Some(pack.join("manglish.fst")),
            romanisation: Some(pack.join("romanisation.json")),
            suffixes: Some(pack.join("suffixes.tsv")),
            chat_forms: Some(pack.join("chat-forms.tsv")),
            layout: Some(pack.join("layout.json")),
            next_words: Some(pack.join("next.fst")),
            lm: None,
            translit: model.then(|| root().join("packs/ml.neopack/translit.mlkbnn")),
            ..PackPaths::new("ml", "mlym")
        }],
        user_dir: None,
    })
}

fn median(mut f: impl FnMut(), runs: usize) -> Duration {
    f();
    let mut t: Vec<Duration> = (0..runs)
        .map(|_| {
            let s = Instant::now();
            f();
            s.elapsed()
        })
        .collect();
    t.sort();
    t[t.len() / 2]
}

#[test]
#[ignore]
fn breakdown() {
    let with = open(true);
    let without = open(false);
    let model =
        Translit::open(&root().join("packs/ml.neopack/translit.mlkbnn")).unwrap();
    let ml = Lexicon::open(&root().join("packs/ml.neopack/ml.fst")).unwrap();
    let en = Lexicon::open(&root().join("apps/ios/KeyboardExtension/Lexicon/en.fst")).unwrap();
    for word in [
        "preferenc",
        "apostrophe",
        "definitely",
        "enthu",
        "sukhamano",
        "avishyam",
    ] {
        println!("{word}");
        for end in 1..=word.len() {
            let typed = &word[..end];
            let ctx = format!("I said {typed}");
            let full = median(|| drop(with.suggest(&ctx, Layout::Latin, true)), 9);
            let base = median(|| drop(without.suggest(&ctx, Layout::Latin, true)), 9);
            let beam = median(|| drop(model.candidates(typed, &ml, 4)), 9);
            let search = Search {
                width: 4,
                ..Search::default()
            };
            let (_, tr) = model.search(typed, &ml, &[], &search);
            let own: u64 = en
                .case_variants(typed.as_bytes())
                .iter()
                .map(|(_, c)| c)
                .sum();
            let prefix = en.log_prob(en.subtree_count(typed.as_bytes()).saturating_sub(own));
            let top = with.suggest(&ctx, Layout::Latin, true);
            let shown: Vec<&str> = top.candidates.iter().map(|c| c.text.as_str()).collect();
            println!(
                "  {typed:<12} full {:>5.2} no-model {:>5.2} beam {:>5.2} ms | steps {:>2} rows {:>3} prefix {:>4} clones {:>3} enc {:>4.2} dec {:>4.2} search {:>4.2} ms en-prefix {prefix:>6.1} {shown:?}",
                full.as_secs_f64() * 1e3,
                base.as_secs_f64() * 1e3,
                beam.as_secs_f64() * 1e3,
                tr.steps,
                tr.rows,
                tr.prefix_checks,
                tr.clones,
                tr.encode.as_secs_f64() * 1e3,
                tr.decode.as_secs_f64() * 1e3,
                tr.search.as_secs_f64() * 1e3,
            );
        }
    }
}

//! The M6 phase 2 safety net: every suggestion the bundled engine makes over the eval inputs,
//! written to a file so a refactor can be checked for identical output (`make golden-check`).
//!
//! Each line is `section \t layout \t input \t candidates \t vowel signs`, with candidates as
//! `text|kind|score` (score to 3 places). Lines in Malayalam script are typed on the native
//! layout, every other line on QWERTY. Every prefix that ends at a character boundary is asked.
//! The `learn` section commits each word as it goes, so learned words and the language habit
//! are covered too.
//!
//! OUT sets the output file (default `build/golden/now.tsv`); MAXLINES caps the lines read
//! from each input (default 200).

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;

use mlengine_core::pack::Pack;
use mlengine_core::{Engine, Layout, Paths};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn engine() -> Engine {
    let pack = Pack::open(&root().join("packs/ml.neopack")).expect("the Malayalam pack");
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    Engine::open(&Paths::with_pack(Some(en), &pack, None))
}

const INPUTS: &[&str] = &[
    "english-sentences.txt",
    "english-typos.tsv",
    "english-typos-context.tsv",
    "language-choice.tsv",
    "chat-golden.tsv",
    "next-ml.txt",
    "next-manglish.tsv",
    "translit-human.tsv",
];

fn lines(name: &str, max: usize) -> Vec<String> {
    let text = std::fs::read_to_string(root().join("data/eval").join(name)).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|l| l.split('\t').next().unwrap_or("").to_string())
        .take(max)
        .collect()
}

fn layout_for(line: &str) -> Layout {
    if line.chars().any(|c| ('\u{0d00}'..='\u{0d7f}').contains(&c)) {
        Layout::Malayalam
    } else {
        Layout::Latin
    }
}

fn row(out: &mut String, section: &str, e: &Engine, ctx: &str, layout: Layout) {
    let s = e.suggest(ctx, layout, true);
    let shown: Vec<String> = s
        .candidates
        .iter()
        .map(|c| format!("{}|{:?}|{:.3}", c.text, c.kind, c.score))
        .collect();
    let signs: String = s.vowel_signs.iter().collect();
    writeln!(out, "{section}\t{layout:?}\t{ctx}\t{}\t{signs}", shown.join(" ")).unwrap();
}

#[test]
#[ignore]
fn golden_dump() {
    let max: usize = std::env::var("MAXLINES").ok().and_then(|s| s.parse().ok()).unwrap_or(200);
    let out_path = std::env::var_os("OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("build/golden/now.tsv"));
    std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
    let mut out = String::new();

    let e = engine();
    for name in INPUTS {
        for line in lines(name, max) {
            let layout = layout_for(&line);
            for (i, c) in line.char_indices() {
                row(&mut out, name, &e, &line[..i + c.len_utf8()], layout);
            }
        }
    }

    // Learning: a fresh engine commits each word, then is asked at every space.
    let e = engine();
    for name in ["chat-golden.tsv", "language-choice.tsv"] {
        for line in lines(name, max) {
            let layout = layout_for(&line);
            let mut ctx = String::new();
            for word in line.split(' ').filter(|w| !w.is_empty()) {
                e.note_used(&ctx, word, layout);
                e.note_committed(word);
                ctx.push_str(word);
                ctx.push(' ');
                row(&mut out, "learn", &e, &ctx, layout);
            }
        }
    }

    std::fs::File::create(&out_path).unwrap().write_all(out.as_bytes()).unwrap();
    println!("{}: {} lines", out_path.display(), out.lines().count());
}

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
//! from each input (default 200). `golden_dump_ml_only` writes the same rows with English
//! switched off. `more_suggestions_keep_the_first` asks the same inputs of engines that return
//! 3, 10 and 20 candidates and checks the longer lists only add to the shorter ones.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use mlengine_core::lang::LangId;
use mlengine_core::pack::Pack;
use mlengine_core::{Candidate, DEFAULT_SUGGESTIONS, Engine, Kind, Layout, MAX_SUGGESTIONS, Paths};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn engine() -> Engine {
    let pack = Pack::open(&root().join("packs/ml.neopack")).expect("the Malayalam pack");
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    Engine::open(&Paths::with_packs(Some(en), &[pack], None))
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

fn layout_for(e: &Engine, line: &str) -> Layout {
    if line.chars().any(|c| ('\u{0d00}'..='\u{0d7f}').contains(&c)) {
        Layout::Native(e.lang_of_id("ml").expect("the Malayalam pack"))
    } else {
        Layout::Latin
    }
}

/// `layout` as the file has always shown it: Malayalam's native layout is `Malayalam`.
fn layout_name(e: &Engine, layout: Layout) -> String {
    match layout {
        Layout::Latin => "Latin".into(),
        Layout::Native(lang) if e.lang_id(lang).is_some_and(|id| id.0 == "ml") => {
            "Malayalam".into()
        }
        Layout::Native(lang) => format!("Native({})", lang.0),
    }
}

/// `kind` as the file has always shown it: a romanised Malayalam word is `Manglish`.
fn kind_name(e: &Engine, c: &Candidate) -> String {
    match c.kind {
        Kind::Romanised if e.lang_id(c.lang).is_some_and(|id| id.0 == "ml") => "Manglish".into(),
        kind => format!("{kind:?}"),
    }
}

fn row(out: &mut String, section: &str, e: &Engine, ctx: &str, layout: Layout) {
    let s = e.suggest(ctx, layout, true);
    let shown: Vec<String> = s
        .candidates
        .iter()
        .map(|c| format!("{}|{}|{:.3}", c.text, kind_name(e, c), c.score))
        .collect();
    let signs: String = s.vowel_signs.iter().collect();
    let layout = layout_name(e, layout);
    writeln!(out, "{section}\t{layout}\t{ctx}\t{}\t{signs}", shown.join(" ")).unwrap();
}

#[test]
#[ignore]
fn golden_dump() {
    let out_path = std::env::var_os("OUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("build/golden/now.tsv"));
    dump(&[], &out_path);
}

/// The same inputs with English switched off (`set_active(["ml"])`): no English candidate,
/// Manglish and native Malayalam as with English on. OUT_ML_ONLY sets the output file
/// (default `build/golden/ml-only.tsv`).
#[test]
#[ignore]
fn golden_dump_ml_only() {
    let out_path = std::env::var_os("OUT_ML_ONLY")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("build/golden/ml-only.tsv"));
    dump(&["ml"], &out_path);
}

/// An engine with `active` as its active list (none: the default).
fn engine_with(active: &[&str]) -> Engine {
    let mut e = engine();
    if !active.is_empty() {
        let ids: Vec<LangId> = active.iter().map(|id| LangId(id.to_string())).collect();
        e.set_active(&ids).unwrap();
    }
    e
}

fn dump(active: &[&str], out_path: &Path) {
    let max: usize = std::env::var("MAXLINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
    let mut out = String::new();

    let e = engine_with(active);
    for name in INPUTS {
        for line in lines(name, max) {
            let layout = layout_for(&e, &line);
            for (i, c) in line.char_indices() {
                row(&mut out, name, &e, &line[..i + c.len_utf8()], layout);
            }
        }
    }

    // Learning: a fresh engine commits each word, then is asked at every space.
    let e = engine_with(active);
    for name in ["chat-golden.tsv", "language-choice.tsv"] {
        for line in lines(name, max) {
            let layout = layout_for(&e, &line);
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

    std::fs::File::create(out_path)
        .unwrap()
        .write_all(out.as_bytes())
        .unwrap();
    println!("{}: {} lines", out_path.display(), out.lines().count());
}

/// More suggestions (`Engine::set_max_suggestions`) only add to the end: over the golden inputs
/// (MAXLINES of each, default 200) and the learning rows, with English on and off, the first
/// three of 10 or 20 are the three of the default, same text, kind and score, in order, and
/// the first ten of 20 are the ten. Vowel signs don't change.
#[test]
#[ignore]
fn more_suggestions_keep_the_first() {
    let max: usize = std::env::var("MAXLINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    for active in [&[][..], &["ml"][..]] {
        let engines: Vec<Engine> = [DEFAULT_SUGGESTIONS, 10, MAX_SUGGESTIONS]
            .into_iter()
            .map(|n| {
                let mut e = engine_with(active);
                e.set_max_suggestions(n);
                e
            })
            .collect();
        let (mut asked, mut longer) = (0, 0);
        let mut check = |ctx: &str, layout: Layout, engines: &[Engine]| {
            let lists: Vec<_> = engines.iter().map(|e| e.suggest(ctx, layout, true)).collect();
            for pair in lists.windows(2) {
                let (short, long) = (&pair[0], &pair[1]);
                let n = short.candidates.len();
                assert!(long.candidates.len() >= n, "{ctx:?}: fewer with more asked");
                assert_eq!(short.candidates[..], long.candidates[..n], "{ctx:?} ({active:?})");
                assert_eq!(short.vowel_signs, long.vowel_signs, "{ctx:?}");
                assert_eq!(short.typed, long.typed, "{ctx:?}");
            }
            asked += 1;
            if lists[2].candidates.len() > lists[0].candidates.len() {
                longer += 1;
            }
            for (list, e) in lists.iter().zip(engines) {
                assert!(list.candidates.len() <= e.max_suggestions());
            }
        };
        let e = &engines[0];
        for name in INPUTS {
            for line in lines(name, max) {
                let layout = layout_for(e, &line);
                for (i, c) in line.char_indices() {
                    check(&line[..i + c.len_utf8()], layout, &engines);
                }
            }
        }
        // Learning, as in `dump`: every engine commits the same words.
        let engines: Vec<Engine> = engines
            .iter()
            .map(|e| {
                let mut fresh = engine_with(active);
                fresh.set_max_suggestions(e.max_suggestions());
                fresh
            })
            .collect();
        for name in ["chat-golden.tsv", "language-choice.tsv"] {
            for line in lines(name, max) {
                let layout = layout_for(&engines[0], &line);
                let mut ctx = String::new();
                for word in line.split(' ').filter(|w| !w.is_empty()) {
                    for e in &engines {
                        e.note_used(&ctx, word, layout);
                        e.note_committed(word);
                    }
                    ctx.push_str(word);
                    ctx.push(' ');
                    check(&ctx, layout, &engines);
                }
            }
        }
        println!("active {active:?}: {asked} asked, {longer} with more than the default");
        assert!(longer * 2 > asked, "more candidates for most inputs");
    }
}

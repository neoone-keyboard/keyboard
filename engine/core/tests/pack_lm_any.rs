//! Any pack with an LM, opened through its manifest as the keyboard opens it, with English from
//! the bundle: `pack_lm.rs`'s checks for Hindi, for the pack named by `NEOPACK_LM` (`as`, `ta`).
//!
//!     NEOPACK_LM=as cargo test --release -p mlengine-core --test pack_lm_any -- --include-ignored --nocapture
//!
//! The LM is MLENGINE_PACK_LM, else `packs/<id>.neopack/lm.mlkbnn`, else the training output
//! `training/build/lm-<id>.mlkbnn`; every test is skipped without `NEOPACK_LM` or without a
//! model. `pack_latency` is the suggestion latency gate (p99 under 8 ms on the Mac, release
//! build), without the LM and with it: the first is the pack's latency before it had one.
//!
//! The words typed are the pack's own: Latin spellings and their native words from its held-out
//! set, `data/eval/<id>/translit-human.tsv`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mlengine_core::lang::Lang;
use mlengine_core::pack::Pack;
use mlengine_core::script;
use mlengine_core::{Engine, Kind, Layout, Paths, Timing};

/// The pack's native layout: it is the only pack loaded.
const NATIVE: Layout = Layout::Native(Lang(1));
/// Words typed in each script.
const WORDS: usize = 6;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn pack_id() -> Option<String> {
    let id = std::env::var("NEOPACK_LM").ok().filter(|id| !id.is_empty());
    if id.is_none() {
        println!("skipped: name a pack with NEOPACK_LM");
    }
    id
}

fn pack_dir(id: &str) -> PathBuf {
    root().join(format!("packs/{id}.neopack"))
}

fn lm_path(id: &str) -> Option<PathBuf> {
    let path = std::env::var_os("MLENGINE_PACK_LM")
        .map(PathBuf::from)
        .into_iter()
        .chain([
            pack_dir(id).join("lm.mlkbnn"),
            root().join(format!("training/build/lm-{id}.mlkbnn")),
        ])
        .find(|p| p.exists());
    if path.is_none() {
        println!("skipped: no lm.mlkbnn for {id} (`make lm-data lm LM_LANG={id}` builds it)");
    }
    path
}

/// English and the pack, with `lm` in place of the pack's own (None: no LM).
fn engine(id: &str, lm: Option<PathBuf>) -> Engine {
    let pack = Pack::open(&pack_dir(id)).expect("the pack");
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    let mut paths = Paths::with_packs(Some(en), &[pack], None);
    paths.packs[0].lm = lm;
    Engine::open(&paths)
}

/// Whether the word has a letter of the pack's script. Call after the engine is open: a pack
/// in a script the engine has no rules of its own for brings them in its `lang.json`.
fn native(id: &str, w: &str) -> bool {
    let code = Pack::open(&pack_dir(id)).unwrap().manifest.script;
    let block = script::rules(&code).expect("the pack's script").block();
    w.chars().any(|c| block.contains(&c))
}

/// `WORDS` (latin, native) pairs spread over the first section of the pack's held-out set,
/// with 4 to 8 Latin letters.
fn held_out(id: &str) -> Vec<(String, String)> {
    let path = root().join(format!("data/eval/{id}/translit-human.tsv"));
    let text = std::fs::read_to_string(&path).expect("the pack's held-out set");
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut sections = 0;
    for line in text.lines() {
        if line.starts_with("# section:") {
            sections += 1;
            if sections > 1 {
                break;
            }
        }
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() == 3 && (4..=8).contains(&cols[1].len()) {
            rows.push((cols[1].to_string(), cols[2].to_string()));
        }
    }
    let step = (rows.len() / WORDS).max(1);
    rows.into_iter().step_by(step).take(WORDS).collect()
}

fn next_words(engine: &Engine, context: &str, layout: Layout) -> Vec<String> {
    engine
        .suggest(context, layout, true)
        .candidates
        .into_iter()
        .filter(|c| c.kind == Kind::NextWord)
        .map(|c| c.text)
        .collect()
}

#[test]
fn the_lm_loads_with_the_pack() {
    let Some(id) = pack_id() else { return };
    let Some(lm) = lm_path(&id) else { return };
    let engine = engine(&id, Some(lm));
    assert!(
        engine.loaded_sources().contains(&format!("{id}:lm")),
        "{:?}",
        engine.loaded_sources()
    );
}

/// Its native pieces are the pack's script (`Lm::set_native`): after native words the next
/// words are native; after English, English.
#[test]
fn next_words_follow_the_sentence() {
    let Some(id) = pack_id() else { return };
    let Some(lm) = lm_path(&id) else { return };
    let engine = engine(&id, Some(lm));
    let words = held_out(&id);
    let context = format!("{} {} ", words[0].1, words[1].1);
    let got = next_words(&engine, &context, NATIVE);
    println!("{context:?}: {got:?}");
    assert!(
        !got.is_empty() && got.iter().all(|w| native(&id, w)),
        "{context:?}: {got:?}"
    );
    let english = next_words(&engine, "I want to ", Layout::Latin);
    println!("\"I want to \": {english:?}");
    assert!(
        !english.is_empty() && !english.iter().any(|w| native(&id, w)),
        "{english:?}"
    );
}

/// Words typed letter by letter, then a space: English after "I said ", the pack's romanised
/// words after one of them, and its native words on the native layout.
fn inputs(id: &str) -> Vec<(String, Layout)> {
    let mut out = Vec::new();
    for w in ["thanks", "tomorrow", "because"] {
        for end in 1..=w.len() {
            out.push((format!("I said {}", &w[..end]), Layout::Latin));
        }
        out.push((format!("I said {w} "), Layout::Latin));
    }
    let words = held_out(id);
    let first = &words[0].0;
    for (latin, _) in &words {
        for end in 1..=latin.len() {
            out.push((format!("{first} {}", &latin[..end]), Layout::Latin));
        }
        out.push((format!("{first} {latin} "), Layout::Latin));
    }
    for (_, word) in &words {
        let chars: Vec<char> = word.chars().collect();
        for end in 1..=chars.len() {
            out.push((chars[..end].iter().collect(), NATIVE));
        }
        out.push((format!("{word} "), NATIVE));
    }
    out
}

fn report(label: &str, mut samples: Vec<(Duration, &str)>) -> Duration {
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
    p99.0
}

/// Every input 20 times in order (the LM context moves as when typing); the p99 must stay
/// under 8 ms.
fn time(label: &str, engine: &Engine, inputs: &[(String, Layout)], suggest: &dyn Fn(&str, Layout)) {
    println!("{label}: sources {:?}", engine.loaded_sources());
    for (context, layout) in inputs {
        suggest(context, *layout);
    }
    let mut samples = Vec::new();
    for _ in 0..20 {
        for (context, layout) in inputs {
            let start = Instant::now();
            suggest(context, *layout);
            samples.push((start.elapsed(), context.as_str()));
        }
    }
    let after: Vec<_> = samples
        .iter()
        .filter(|s| s.1.ends_with(' '))
        .copied()
        .collect();
    let mid: Vec<_> = samples
        .iter()
        .filter(|s| !s.1.ends_with(' '))
        .copied()
        .collect();
    report(&format!("{label}, after a space"), after);
    report(&format!("{label}, mid-word"), mid);
    let p99 = report(&format!("{label}, all"), samples);
    assert!(p99 < Duration::from_millis(8), "{label}: p99 {p99:?}");
}

#[test]
#[ignore = "timing: release build, NEOPACK_LM=<id>"]
fn pack_latency() {
    let Some(id) = pack_id() else { return };
    let Some(lm) = lm_path(&id) else { return };
    let inputs = inputs(&id);
    let without = engine(&id, None);
    time(&format!("{id} pack without the LM"), &without, &inputs, &|c, l| {
        let _ = without.suggest(c, l, true);
    });
    let with = engine(&id, Some(lm));
    time(&format!("{id} pack with the LM"), &with, &inputs, &|c, l| {
        let _ = with.suggest(c, l, true);
    });
    time(
        &format!("{id} pack with the LM, keyboard cutoffs"),
        &with,
        &inputs,
        &|c, l| {
            let start = Instant::now();
            let elapsed = || start.elapsed();
            let _ = with.suggest_timed(c, l, true, Some(&Timing::keyboard(&elapsed)));
        },
    );
}

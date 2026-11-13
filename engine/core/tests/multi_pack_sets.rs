//! The held-out sets of several packs typed with more than one pack active: what each pack's
//! own words get when another pack is on beside it. `make eval` runs each pack with English
//! alone; this measures the mixes (English + Malayalam + Hindi, and with Assamese too).
//!
//!     cd engine && cargo test --release -p mlengine-core --test multi_pack_sets -- --ignored --nocapture
//!
//! For each set (`data/eval/translit-human.tsv`, `data/eval/<id>/translit-human.tsv` and
//! Malayalam's `chat-golden.tsv`), with each mix: top-1, the first suggestion in the target's
//! script is the target; first slot, the bar's first suggestion is (what space commits); in bar,
//! the target is one of the three shown.

use std::path::{Path, PathBuf};

use mlengine_core::lang::LangId;
use mlengine_core::pack::Pack;
use mlengine_core::text::{self, Script};
use mlengine_core::{Engine, Layout, Paths};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn engine(ids: &[&str]) -> Engine {
    let packs: Vec<Pack> = ids
        .iter()
        .map(|id| Pack::open(&root().join(format!("packs/{id}.neopack"))).unwrap())
        .collect();
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    let mut e = Engine::open(&Paths::with_packs(Some(en), &packs, None));
    let active: Vec<LangId> = std::iter::once(&"en")
        .chain(ids)
        .map(|id| LangId((*id).into()))
        .collect();
    e.set_active(&active).unwrap();
    e
}

/// `context<TAB>typed<TAB>intended` rows, `#` lines skipped.
fn rows(path: &Path) -> Vec<(String, String, String)> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .filter_map(|l| {
            let mut f = l.split('\t');
            Some((f.next()?.into(), f.next()?.into(), f.next()?.into()))
        })
        .collect()
}

fn score(e: &Engine, rows: &[(String, String, String)]) -> (f64, f64, f64) {
    let (mut top1, mut first, mut bar) = (0, 0, 0);
    for (context, typed, want) in rows {
        let before = if context.is_empty() { typed.clone() } else { format!("{context} {typed}") };
        let s = e.suggest(&before, Layout::Latin, true);
        let texts: Vec<&str> = s.candidates.iter().map(|c| c.text.as_str()).collect();
        let script = text::script(want);
        let native = texts.iter().find(|t| match script {
            Script::Native(_) => text::script(t) == script,
            _ => true,
        });
        top1 += usize::from(native == Some(&want.as_str()));
        first += usize::from(texts.first() == Some(&want.as_str()));
        bar += usize::from(texts.iter().take(3).any(|t| t == want));
    }
    let n = rows.len() as f64 / 100.0;
    (top1 as f64 / n, first as f64 / n, bar as f64 / n)
}

#[test]
#[ignore = "measurement: cargo test --release --test multi_pack_sets -- --ignored --nocapture"]
fn multi_pack_sets() {
    let e = root().join("data/eval");
    let sets = [
        ("ml translit-human", e.join("translit-human.tsv")),
        ("ml chat-golden", e.join("chat-golden.tsv")),
        ("hi translit-human", e.join("hi/translit-human.tsv")),
        ("as translit-human", e.join("as/translit-human.tsv")),
    ];
    let mixes: [&[&str]; 5] = [&["ml"], &["hi"], &["ml", "hi"], &["hi", "ml"], &["ml", "hi", "as"]];
    for mix in mixes {
        let engine = engine(mix);
        for (name, path) in &sets {
            let lang = &name[..2];
            if !mix.contains(&lang) {
                continue;
            }
            let (top1, first, bar) = score(&engine, &rows(path));
            println!(
                "en+{:<9} {name:<18} top-1 {top1:6.2}%  first slot {first:6.2}%  in bar {bar:6.2}%",
                mix.join("+")
            );
        }
    }
}

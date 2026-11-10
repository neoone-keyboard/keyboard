//! How long the engine takes to open, with each pack of `packs/` beside English and with all
//! of them: the keyboard opens it on its first key. Also the part a pack's script rules take
//! (`script::for_pack`, which reads the pack's `lang.json` or the engine's own copy).
//! Prints; no gate. Release build:
//!
//!     cargo test --release -p mlengine-core --test open_time -- --ignored --nocapture

use std::path::PathBuf;
use std::time::{Duration, Instant};

use mlengine_core::pack::Pack;
use mlengine_core::{Engine, Paths};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn median(mut times: Vec<Duration>) -> Duration {
    times.sort();
    times[times.len() / 2]
}

fn open(ids: &[&str]) -> Duration {
    let en = root().join("apps/ios/KeyboardExtension/Lexicon/en.fst");
    let times = (0..30)
        .map(|_| {
            let start = Instant::now();
            let packs: Vec<Pack> = ids
                .iter()
                .map(|id| Pack::open(&root().join(format!("packs/{id}.neopack"))).unwrap())
                .collect();
            let engine = Engine::open(&Paths::with_packs(Some(en.clone()), &packs, None));
            let took = start.elapsed();
            assert!(engine.loaded_sources().len() > ids.len());
            took
        })
        .collect();
    median(times)
}

#[test]
#[ignore]
fn open_time() {
    let first = Instant::now();
    let pack = Pack::open(&root().join("packs/ml.neopack")).unwrap();
    println!("the first Pack::open in the process (ml): {:?}", first.elapsed());
    drop(pack);
    for ids in [&["ml"][..], &["hi"], &["as"], &["ta"], &["as", "hi", "ml", "ta"]] {
        println!("Pack::open + Engine::open, English and {ids:?}: median {:?}", open(ids));
    }
    for id in ["ml", "hi", "as", "ta"] {
        let dir = root().join(format!("packs/{id}.neopack"));
        let times = (0..200)
            .map(|_| {
                let start = Instant::now();
                Pack::open(&dir).unwrap();
                start.elapsed()
            })
            .collect();
        println!("Pack::open alone (manifest and script rules), {id}: median {:?}", median(times));
    }
}

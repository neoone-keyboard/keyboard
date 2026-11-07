//! The committed `en-next.fst` (the English base's next words) is exactly the English pairs of
//! `packs/ml.neopack/next.fst`, as `lexbuild` writes it beside `en.fst`. Rebuild it with
//! `lexbuild-english-next` (or `make lexicons`) when `next.fst` changes. In `make test-data`.

use std::path::Path;

use mlengine_core::lexicon::{Lexicon, build};

#[test]
#[ignore = "reads the committed lexicons; run by make test-data"]
fn the_committed_english_next_words_are_the_packs_english_pairs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let next = Lexicon::open(&root.join("packs/ml.neopack/next.fst")).unwrap();
    let want = build(lexbuild::english_pairs(&next)).unwrap();
    let have = std::fs::read(root.join("apps/ios/KeyboardExtension/Lexicon/en-next.fst")).unwrap();
    assert!(
        have == want,
        "apps/ios/KeyboardExtension/Lexicon/en-next.fst is stale: run lexbuild-english-next"
    );
}

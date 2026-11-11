//! Every pack's `romanisation.json`, loaded by the engine, gives the keys its romanised lexicon
//! was built with, Malayalam's included (its keys came from the original `romanize.rs`). Slow (every
//! word of every pack), so `#[ignore]`: `make test-data` runs it.

use std::path::{Path, PathBuf};

use fst::{IntoStreamer, Streamer};
use mlengine_core::romanisation::Romanisation;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn map(path: &Path) -> fst::Map<Vec<u8>> {
    fst::Map::new(std::fs::read(path).unwrap()).unwrap()
}

fn pack(id: &str) -> Romanisation {
    Romanisation::load(&root().join(format!("packs/{id}.neopack/romanisation.json"))).unwrap()
}

/// The packed native word of a romanised-lexicon key back to text: block offsets 0x80–0xFF from
/// `base`, ZWNJ 0x01, ZWJ 0x02, the apostrophe 0x27.
fn unpack(bytes: &[u8], base: u32) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x01 => '\u{200C}',
            0x02 => '\u{200D}',
            0x27 => '\'',
            _ => char::from_u32(base + u32::from(b - 0x80)).unwrap(),
        })
        .collect()
}

/// Every key `skeleton\0word` of a romanised lexicon equals the loaded tables' skeleton of the
/// word. Returns how many keys were checked; panics listing the first mismatches.
fn keys_match(id: &str, file: &str, base: u32) -> usize {
    let r = pack(id);
    let m = map(&root().join(format!("packs/{id}.neopack/{file}")));
    let mut stream = m.keys().into_stream();
    let (mut n, mut bad, mut odd) = (0, Vec::new(), Vec::new());
    while let Some(key) = stream.next() {
        if key[0] < mlengine_core::lexicon::FIRST_WORD_BYTE {
            continue; // reserved: totals and precomputed top lists
        }
        let Some(sep) = key.iter().position(|&b| b == 0) else {
            if odd.len() < 5 {
                odd.push(format!("{:?}", String::from_utf8_lossy(key)));
            }
            continue;
        };
        let word = unpack(&key[sep + 1..], base);
        let stored = std::str::from_utf8(&key[..sep]).unwrap();
        // One of the word's skeletons: a word with a shorter spelling has a second key.
        let got = r.word_skeletons(&word);
        if !got.iter().any(|s| s == stored) && bad.len() < 20 {
            bad.push(format!("{word}: stored {stored}, tables {got:?}"));
        }
        n += 1;
    }
    assert!(odd.is_empty(), "{id}: keys without a separator: {odd:?}");
    assert!(
        bad.is_empty(),
        "{id}: {} mismatches, first:\n{}",
        bad.len(),
        bad.join("\n")
    );
    n
}

/// `manglish.fst` was built by the engine's original Malayalam romaniser (`romanize.rs`, removed
/// in M6 phase 2), so its keys hold that romaniser's skeleton of every word: the pack's tables
/// must give the same. (Before removal, this test also compared `romanize` and typed-text
/// skeletons word for word over all of `ml.fst`, and they were identical.)
#[test]
#[ignore]
fn malayalam_tables_reproduce_the_original_keys() {
    let n = keys_match("ml", "manglish.fst", 0x0D00);
    assert!(n > 100_000, "{n} keys");
    eprintln!("ml: {n} manglish.fst keys");
}

#[test]
#[ignore]
fn hindi_and_assamese_keys_match_their_tables() {
    eprintln!("hi: {} keys", keys_match("hi", "hinglish.fst", 0x0900));
    eprintln!("as: {} keys", keys_match("as", "romanised.fst", 0x0980));
}

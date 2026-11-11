//! Rebuilds a pack's romanised lexicon from its native lexicon after `romanisation.json`
//! changed: every word of `<lang>.fst`, with its count, keyed by the tables' skeleton. The
//! same keys `lexbuild --lang` writes, without re-reading the corpus.
//!
//!     lexbuild-rekey packs/hi.neopack/romanisation.json packs/hi.neopack/hi.fst packs/hi.neopack/hinglish.fst

use std::path::PathBuf;

use fst::{IntoStreamer, Streamer};
use lexbuild::romanisation::Table;
use mlengine_core::lexicon::FIRST_WORD_BYTE;

fn main() -> std::io::Result<()> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [romanisation, native, out] = args.as_slice() else {
        eprintln!("usage: lexbuild-rekey <romanisation.json> <native.fst> <out.fst>");
        std::process::exit(2);
    };
    let table = Table::load(romanisation)?;
    let map = fst::Map::new(std::fs::read(native)?).map_err(std::io::Error::other)?;
    let mut vocab = Vec::new();
    let mut stream = map.into_stream();
    while let Some((key, count)) = stream.next() {
        if key[0] >= FIRST_WORD_BYTE {
            let word = String::from_utf8(key.to_vec()).map_err(std::io::Error::other)?;
            vocab.push((word, count));
        }
    }
    let (bytes, n) = table.build(&vocab)?;
    std::fs::write(out, bytes)?;
    eprintln!("{}: {n} keys from {} words", out.display(), vocab.len());
    Ok(())
}

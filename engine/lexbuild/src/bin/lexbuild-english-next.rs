//! The English base's `en-next.fst` from a pack's `next.fst`: its English pairs alone
//! (`lexbuild::english_pairs`). `lexicon::build` depends only on its entries, so this is the
//! file `lexbuild` writes beside `en.fst` in the same run, without re-reading the corpus.
//!
//!     lexbuild-english-next packs/ml.neopack/next.fst apps/ios/KeyboardExtension/Lexicon/en-next.fst

use std::path::PathBuf;

use mlengine_core::lexicon::{Lexicon, build};

fn main() -> std::io::Result<()> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [next, out] = args.as_slice() else {
        eprintln!("usage: lexbuild-english-next <next.fst> <out.fst>");
        std::process::exit(2);
    };
    let lex = Lexicon::open(next)
        .map_err(|e| std::io::Error::other(format!("{}: {e}", next.display())))?;
    let entries = lexbuild::english_pairs(&lex);
    let n = entries.len();
    std::fs::write(out, build(entries).map_err(std::io::Error::other)?)?;
    eprintln!("{}: {n} English pairs of {}", out.display(), lex.len());
    Ok(())
}

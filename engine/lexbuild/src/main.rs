//! `lexbuild`: plain-text corpora in, lexicon bundle out. See `make lexicons`.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use lexbuild::{
    Config, NextWords, count_words, english_vocab, malayalam_vocab, read_hand_list, read_word_list,
    write_bundle,
};
use mlengine_core::text::Script;

#[derive(Parser)]
#[command(about = "Build the keyboard's lexicon bundle from plain-text corpora")]
struct Args {
    /// English corpus files, one paragraph per line.
    #[arg(long = "english", required = true)]
    english: Vec<PathBuf>,
    /// Malayalam corpus files, one paragraph per line.
    #[arg(long = "malayalam", required = true)]
    malayalam: Vec<PathBuf>,
    /// SCOWL word list, one word per line.
    #[arg(long)]
    scowl: PathBuf,
    /// Hand-written English word lists (`data/indian-english.txt`), one word per
    /// line, `#` comments: always kept, in the listed spelling.
    #[arg(long = "english-list")]
    english_list: Vec<PathBuf>,
    /// Malayalam word lists (SMC), one word per line.
    #[arg(long = "smc")]
    smc: Vec<PathBuf>,
    /// Output directory for the bundle files.
    #[arg(long)]
    out: PathBuf,
    /// Stop reading each corpus file after this many lines (for quick runs).
    #[arg(long)]
    max_lines: Option<usize>,
}

fn open(path: &PathBuf) -> io::Result<BufReader<File>> {
    File::open(path)
        .map(BufReader::new)
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}

fn merge(into: &mut lexbuild::Counts, from: lexbuild::Counts) {
    for (word, count) in from {
        *into.entry(word).or_default() += count;
    }
}

fn run(args: Args) -> io::Result<()> {
    let config = Config::default();
    let mut en_counts = lexbuild::Counts::new();
    for path in &args.english {
        eprintln!("counting {}", path.display());
        merge(
            &mut en_counts,
            count_words(open(path)?, Script::Latin, args.max_lines)?,
        );
    }
    let mut ml_counts = lexbuild::Counts::new();
    for path in &args.malayalam {
        eprintln!("counting {}", path.display());
        merge(
            &mut ml_counts,
            count_words(open(path)?, Script::Malayalam, args.max_lines)?,
        );
    }
    let scowl = read_word_list(open(&args.scowl)?)?;
    let mut smc = HashSet::new();
    for path in &args.smc {
        smc.extend(read_word_list(open(path)?)?);
    }

    let mut extra = HashSet::new();
    for path in &args.english_list {
        extra.extend(read_hand_list(open(path)?)?);
    }
    let english = english_vocab(&en_counts, &scowl, &extra, &config);
    let malayalam = malayalam_vocab(&ml_counts, &smc, &config);
    drop((en_counts, ml_counts));

    let mut next = Vec::new();
    for (vocab, paths) in [(&english, &args.english), (&malayalam, &args.malayalam)] {
        let mut pairs = NextWords::new(vocab, &config);
        for path in paths {
            eprintln!("counting word pairs in {}", path.display());
            pairs.add_corpus(open(path)?, args.max_lines)?;
        }
        next.extend(pairs.entries(config.next_per_word));
    }

    let stats = write_bundle(&args.out, &english, &malayalam, &next)?;
    print!("{}", stats.report);
    Ok(())
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lexbuild: {e}");
            ExitCode::FAILURE
        }
    }
}

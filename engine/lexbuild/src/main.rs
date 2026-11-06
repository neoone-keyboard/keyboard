//! `lexbuild`: plain-text corpora in, lexicon bundle out. See `make lexicons`.
//!
//! Two modes. With `--malayalam`, the Malayalam bundle (`en.fst`, `ml.fst`, `manglish.fst`,
//! `next.fst`) as always. With `--lang`, `--native` and `--romanisation`, any other language's
//! pack lexicons (`<lang>.fst`, `<romanised>.fst`, `next.fst`), the romanised keys built from
//! the pack's `romanisation.json`, and English counted against the base's `en.fst`.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use lexbuild::romanisation::Table;
use mlengine_core::romanisation::Romanisation;
use lexbuild::{
    Config, NextWords, count_native_words, count_words, english_vocab, malayalam_vocab,
    native_phrase_break, native_vocab, native_words, read_hand_list, read_lexicon, read_word_list,
    write_bundle, write_native_bundle,
};
use mlengine_core::text::Script;

#[derive(Parser)]
#[command(about = "Build the keyboard's lexicon bundle from plain-text corpora")]
struct Args {
    /// English corpus files, one paragraph per line.
    #[arg(long = "english", required = true)]
    english: Vec<PathBuf>,
    /// Malayalam corpus files, one paragraph per line (the Malayalam bundle).
    #[arg(
        long = "malayalam",
        required_unless_present = "native",
        conflicts_with = "native"
    )]
    malayalam: Vec<PathBuf>,
    /// SCOWL word list, one word per line (the Malayalam bundle, which also builds en.fst).
    #[arg(long, required_unless_present = "english_fst")]
    scowl: Option<PathBuf>,
    /// Hand-written English word lists (`data/indian-english.txt`), one word per
    /// line, `#` comments: always kept, in the listed spelling.
    #[arg(long = "english-list")]
    english_list: Vec<PathBuf>,
    /// Malayalam word lists (SMC), one word per line.
    #[arg(long = "smc")]
    smc: Vec<PathBuf>,
    /// A pack's language id (`hi`): names `<lang>.fst`.
    #[arg(long, requires = "native")]
    lang: Option<String>,
    /// The pack's native corpus files, one paragraph per line.
    #[arg(long = "native", requires_all = ["lang", "romanisation", "english_fst"])]
    native: Vec<PathBuf>,
    /// The pack's `romanisation.json`: its script and the romanised-lexicon keys.
    #[arg(long)]
    romanisation: Option<PathBuf>,
    /// The romanised lexicon's file name, without `.fst` (`hinglish`).
    #[arg(long, default_value = "romanised")]
    romanised_name: String,
    /// Native word lists kept whatever their count, one word per line.
    #[arg(long = "native-list")]
    native_list: Vec<PathBuf>,
    /// The English base's lexicon (`en.fst`): its words are the English vocabulary whose
    /// next words a pack's `next.fst` stores.
    #[arg(long)]
    english_fst: Option<PathBuf>,
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

fn english_next(
    english: &[(String, u64)],
    args: &Args,
    config: &Config,
) -> io::Result<Vec<(Vec<u8>, u64)>> {
    let mut pairs = NextWords::new(english, config);
    for path in &args.english {
        eprintln!("counting word pairs in {}", path.display());
        pairs.add_corpus(open(path)?, args.max_lines)?;
    }
    Ok(pairs.entries(config.next_per_word))
}

fn run_native(args: Args) -> io::Result<()> {
    let config = Config::default();
    let (Some(lang), Some(table_path), Some(english_fst)) =
        (&args.lang, &args.romanisation, &args.english_fst)
    else {
        unreachable!("clap requires --lang, --romanisation and --english-fst with --native");
    };
    let table = Table::load(table_path)?;
    let mut counts = lexbuild::Counts::new();
    for path in &args.native {
        eprintln!("counting {}", path.display());
        merge(
            &mut counts,
            count_native_words(
                open(path)?,
                &table,
                config.max_malayalam_chars,
                args.max_lines,
            )?,
        );
    }
    let mut listed = HashSet::new();
    for path in &args.native_list {
        listed.extend(read_word_list(open(path)?)?);
    }
    let native = native_vocab(&counts, &listed, &table, &config);
    drop(counts);

    let english = read_lexicon(english_fst)?;
    let mut next = english_next(&english, &args, &config)?;
    let mut pairs = NextWords::new(&native, &config);
    for path in &args.native {
        eprintln!("counting word pairs in {}", path.display());
        pairs.add_corpus_with(
            open(path)?,
            args.max_lines,
            |c| native_phrase_break(&table, c),
            |phrase| native_words(&table, phrase),
        )?;
    }
    next.extend(pairs.entries(config.next_per_word));

    let stats = write_native_bundle(
        &args.out,
        lang,
        &args.romanised_name,
        &table,
        &native,
        &next,
    )?;
    print!("{}", stats.report);
    Ok(())
}

fn run(args: Args) -> io::Result<()> {
    if !args.native.is_empty() {
        return run_native(args);
    }
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
            count_words(open(path)?, Script::MALAYALAM, args.max_lines)?,
        );
    }
    let scowl_path = args
        .scowl
        .as_ref()
        .ok_or_else(|| io::Error::other("--scowl is needed to build en.fst"))?;
    let scowl = read_word_list(open(scowl_path)?)?;
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

    let mut next = english_next(&english, &args, &config)?;
    let mut pairs = NextWords::new(&malayalam, &config);
    for path in &args.malayalam {
        eprintln!("counting word pairs in {}", path.display());
        pairs.add_corpus(open(path)?, args.max_lines)?;
    }
    next.extend(pairs.entries(config.next_per_word));

    let Some(romanisation) = &args.romanisation else {
        return Err(io::Error::other(
            "--romanisation is required: Malayalam's manglish.fst is keyed by packs/ml.neopack/romanisation.json",
        ));
    };
    let tables = Romanisation::load(romanisation).map_err(io::Error::other)?;
    let stats = write_bundle(&args.out, &tables, &english, &malayalam, &next)?;
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

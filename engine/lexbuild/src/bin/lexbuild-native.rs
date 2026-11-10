//! `lexbuild-native`: a pack language's lexicons from its language config and corpus (see
//! `lexbuild::native`). The config defines the script, as a pack's `lang` file does for the
//! engine, so any script works without code.
//!
//!     lexbuild-native --lang-config data/langs/as/lang.json --native data/raw/text/as.txt \
//!         --english-next packs/ml.neopack/next.fst --out data/raw/bundle-as
//!
//! writes `<id>.fst` (word frequencies), `next.fst` (the English pairs of `--english-next`,
//! which were counted against the same `en.fst`, plus the native ones), `words.tsv` (the
//! vocabulary, for the romanised keys) and `stats.txt`, which also lists the most frequent
//! tokens dropped as not words of the script (mis-encodings a `replace` pair in the config
//! would mend). With `--romanised-keys` (rows of
//! `skeleton<TAB>word<TAB>count`) it writes `romanised.fst`; `--native` is then optional. With
//! `--romanisation` (a pack's `romanisation.json`) instead, `romanised.fst` is keyed by the
//! tables' walk and skeleton of every word in the vocabulary, as `lexbuild --lang` keys it.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use lexbuild::native::{self, Lang};
use lexbuild::romanisation::Table;
use lexbuild::{Config, Counts, Entries, NextWords};
use mlengine_core::lexicon::{Lexicon, build};

#[derive(Parser)]
#[command(about = "Build a pack language's lexicons from its language config")]
struct Args {
    /// The language config (`data/langs/<id>/lang.json`).
    #[arg(long)]
    lang_config: PathBuf,
    /// Corpus files in the language, one paragraph per line.
    #[arg(long = "native")]
    native: Vec<PathBuf>,
    /// Word lists, one word per line: always kept.
    #[arg(long = "native-list")]
    native_list: Vec<PathBuf>,
    /// A `next.fst` whose English pairs (an ASCII previous word) are copied into this one.
    #[arg(long)]
    english_next: Option<PathBuf>,
    /// `skeleton<TAB>word<TAB>count` rows: writes `romanised.fst`.
    #[arg(long, conflicts_with = "romanisation")]
    romanised_keys: Option<PathBuf>,
    /// A pack's `romanisation.json`: writes `romanised.fst` from the vocabulary (needs
    /// `--native`).
    #[arg(long, requires = "native")]
    romanisation: Option<PathBuf>,
    /// Output directory.
    #[arg(long)]
    out: PathBuf,
    /// Stop reading each corpus file after this many lines (for quick runs).
    #[arg(long)]
    max_lines: Option<usize>,
}

/// How many of the most frequent dropped tokens `stats.txt` lists.
const DROPPED_SHOWN: usize = 20;

fn open(path: &PathBuf) -> io::Result<BufReader<File>> {
    File::open(path)
        .map(BufReader::new)
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}

/// The pairs of a `next.fst` whose previous word is ASCII: the English half.
fn english_pairs(path: &Path) -> io::Result<Entries> {
    let lex =
        Lexicon::open(path).map_err(|e| io::Error::other(format!("{}: {e}", path.display())))?;
    Ok(lexbuild::english_pairs(&lex))
}

fn write_fst(out: &Path, name: &str, entries: Entries, report: &mut String) -> io::Result<()> {
    let n = entries.len();
    let bytes = build(entries).map_err(io::Error::other)?;
    fs::write(out.join(name), &bytes)?;
    let _ = writeln!(report, "{name}: {n} keys, {} bytes", bytes.len());
    Ok(())
}

fn run(args: Args) -> io::Result<()> {
    let lang = Lang::open(&args.lang_config)?;
    // The script as the engine will read it from the pack: the romanisation tables find their
    // script's rules by its code.
    let rules = mlengine_core::script::data::DataScript::intern(lang.def.clone());
    mlengine_core::script::install(rules).map_err(io::Error::other)?;
    fs::create_dir_all(&args.out)?;
    let mut report = String::new();

    if !args.native.is_empty() {
        let mut counts = Counts::new();
        let mut dropped = Counts::new();
        for path in &args.native {
            eprintln!("counting {}", path.display());
            let (words, not_words) = native::count_tokens(&lang, open(path)?, args.max_lines)?;
            for (word, count) in words {
                *counts.entry(word).or_default() += count;
            }
            for (word, count) in not_words {
                *dropped.entry(word).or_default() += count;
            }
        }
        let mut listed = HashSet::new();
        for path in &args.native_list {
            listed.extend(native::read_word_list(&lang, open(path)?)?);
        }
        let tokens: u64 = counts.values().sum();
        let _ = writeln!(
            report,
            "{} tokens: {tokens}, distinct forms: {}",
            lang.id,
            counts.len()
        );
        // Tokens with a letter of the script that the config's rules refuse. A frequent one
        // is usually a mis-encoding to mend with `replace` in the config.
        let dropped_tokens: u64 = dropped.values().sum();
        let _ = writeln!(
            report,
            "{} dropped (a letter of the script, but not a word): {dropped_tokens} tokens, {} forms",
            lang.id,
            dropped.len()
        );
        let mut worst: Vec<(String, u64)> = dropped.into_iter().collect();
        worst.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        if !worst.is_empty() {
            let top: Vec<String> = worst
                .iter()
                .take(DROPPED_SHOWN)
                .map(|(w, c)| format!("{w} {c}"))
                .collect();
            let _ = writeln!(
                report,
                "{} dropped top {DROPPED_SHOWN}: {}",
                lang.id,
                top.join(", ")
            );
        }
        drop(worst);
        let vocab = native::vocab(&lang, &counts, &listed);
        drop(counts);

        let config = Config::default();
        let mut pairs = NextWords::new(&vocab, &config).with_lang(lang.clone());
        for path in &args.native {
            eprintln!("counting word pairs in {}", path.display());
            pairs.add_corpus(open(path)?, args.max_lines)?;
        }
        let mut next = pairs.entries(config.next_per_word);
        let native_pairs = next.len();
        if let Some(path) = &args.english_next {
            next.extend(english_pairs(path)?);
        }
        let _ = writeln!(
            report,
            "next.fst pairs: {native_pairs} {}, {} English",
            lang.id,
            next.len() - native_pairs
        );

        let entries: Entries = vocab
            .iter()
            .map(|(w, c)| (w.as_bytes().to_vec(), *c))
            .collect();
        write_fst(&args.out, &format!("{}.fst", lang.id), entries, &mut report)?;
        write_fst(&args.out, "next.fst", next, &mut report)?;
        let mut tsv = io::BufWriter::new(File::create(args.out.join("words.tsv"))?);
        for (word, count) in &vocab {
            writeln!(tsv, "{word}\t{count}")?;
        }
        tsv.flush()?;
        let top: Vec<&str> = vocab.iter().take(20).map(|(w, _)| w.as_str()).collect();
        let _ = writeln!(report, "{} top 20: {}", lang.id, top.join(" "));
        if let Some(path) = &args.romanisation {
            let table = Table::load(path)?;
            let entries = table.entries(&vocab);
            let _ = writeln!(report, "romanised keys skipped: {}", vocab.len() - entries.len());
            write_fst(&args.out, "romanised.fst", entries, &mut report)?;
        }
    }

    if let Some(path) = &args.romanised_keys {
        let (entries, skipped) = native::romanised_entries(&lang, open(path)?)?;
        let _ = writeln!(report, "romanised keys skipped: {skipped}");
        write_fst(&args.out, "romanised.fst", entries, &mut report)?;
    }

    let stats = args.out.join("stats.txt");
    let mut previous = fs::read_to_string(&stats).unwrap_or_default();
    if args.native.is_empty() {
        previous.push_str(&report); // a romanised-only run adds to the earlier stats
    } else {
        previous = report.clone();
    }
    fs::write(stats, previous)?;
    print!("{report}");
    Ok(())
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lexbuild-native: {e}");
            ExitCode::FAILURE
        }
    }
}

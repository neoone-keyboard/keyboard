//! Conversion accuracy of the Japanese pack (packs/ja.neopack) on held-out sentences:
//! data/eval/ja/tatoeba-convert.tsv (Tatoeba sentences with reviewed readings, written by
//! scripts/ja-eval-set.py). Nothing in the pack was built or tuned from them.
//!
//! Each sentence's reading is converted in one go, as a typist converts a whole sentence, and
//! each phrase (the text between 、。！？) on its own, as one converted phrase by phrase:
//!
//! - exact: the first candidate is the sentence (phrase) as written;
//! - in top 10: it is among the first ten candidates;
//! - characters: 1 − edit distance / length of the first candidate, over all sentences.
//!
//! `cargo test --release -p mlengine-core --test ja_eval -- --ignored --nocapture`
//! `JA_EVAL_SHOW=20` prints the first 20 sentences converted wrong. `JA_EVAL_MIN` (a
//! percentage) fails the run when sentence exact falls under it.

use std::path::{Path, PathBuf};
use std::time::Instant;

use mlengine_core::convert::Converter;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut prev = row[0];
        row[0] = i;
        for j in 1..=b.len() {
            let cur = row[j];
            row[j] = (prev + usize::from(a[i - 1] != b[j - 1]))
                .min(row[j] + 1)
                .min(row[j - 1] + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// `text` cut after each 、。！？, the marks kept with the phrase before them.
fn phrases(text: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    for c in text.chars() {
        out.last_mut().unwrap().push(c);
        if matches!(c, '、' | '。' | '！' | '？') {
            out.push(String::new());
        }
    }
    out.retain(|p| !p.is_empty());
    out
}

#[derive(Default)]
struct Score {
    n: usize,
    exact: usize,
    top10: usize,
}

impl Score {
    fn add(&mut self, candidates: &[String], want: &str) {
        self.n += 1;
        self.exact += usize::from(candidates.first().is_some_and(|c| c == want));
        self.top10 += usize::from(candidates.iter().take(10).any(|c| c == want));
    }

    fn pct(part: usize, n: usize) -> f64 {
        100.0 * part as f64 / n.max(1) as f64
    }

    fn line(&self, what: &str) -> String {
        format!(
            "{what}: {} | exact {:.1}% | in top 10 {:.1}%",
            self.n,
            Self::pct(self.exact, self.n),
            Self::pct(self.top10, self.n)
        )
    }
}

#[test]
#[ignore = "real pack data: run in release with --nocapture"]
fn conversion_accuracy() {
    let pack = root().join("packs/ja.neopack");
    let connection = std::env::var_os("JA_CONNECTION")
        .map_or_else(|| pack.join("connection.bin"), PathBuf::from);
    let c = Converter::open(
        &pack.join("kana.json"),
        &pack.join("conversion.dict"),
        &connection,
    )
    .unwrap();
    let text = std::fs::read_to_string(root().join("data/eval/ja/tatoeba-convert.tsv")).unwrap();
    let show: usize = std::env::var("JA_EVAL_SHOW")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let mut sentences = Score::default();
    let mut phrase_score = Score::default();
    let (mut chars, mut errors) = (0usize, 0usize);
    let mut shown = 0;
    let started = Instant::now();
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let [_, reading, want] = line.split('\t').collect::<Vec<_>>()[..] else {
            panic!("bad line {line}");
        };
        let conv = c.convert(reading, 10);
        sentences.add(&conv.candidates, want);
        let got = conv.candidates.first().map_or("", String::as_str);
        chars += want.chars().count();
        errors += edit_distance(got, want);
        if got != want && shown < show {
            shown += 1;
            println!("{reading}\n  got  {got}\n  want {want}");
        }
        let (readings, wants) = (phrases(reading), phrases(want));
        if readings.len() == wants.len() {
            for (r, w) in readings.iter().zip(&wants) {
                phrase_score.add(&c.convert(r, 10).candidates, w);
            }
        }
    }
    println!("{}", sentences.line("sentences"));
    println!("{}", phrase_score.line("phrases"));
    println!(
        "characters: {:.1}% right ({errors} edits in {chars})",
        100.0 - Score::pct(errors, chars)
    );
    println!("time: {:?}", started.elapsed());
    if let Some(min) = std::env::var("JA_EVAL_MIN")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
    {
        let exact = Score::pct(sentences.exact, sentences.n);
        assert!(exact >= min, "sentence exact {exact:.1}% under {min}%");
    }
}

#[test]
fn phrases_end_at_marks() {
    assert_eq!(phrases("あ、い。う"), ["あ、", "い。", "う"]);
    assert_eq!(phrases("あ"), ["あ"]);
    assert_eq!(edit_distance("今日は", "京は"), 2);
    assert_eq!(edit_distance("", "あ"), 1);
}

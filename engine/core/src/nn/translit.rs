//! Latin → native-script spellings from the transformer, by beam search held to the lexicon.
//! The script is the pack's: its output tokens are a `vocab::Target`.
//!
//! The constrained beam keeps a hypothesis only while its text is a prefix of some word of the
//! pack's lexicon (`Lexicon::has_prefix`) or of a suffix split of the typed word, and finishes
//! one only on a whole word or split. Splits come in from the caller as finished native-script
//! words (`suffix::splits` reads the typed Latin, never the beam's text, so they are the same
//! at every step): "could become a split form" is then a byte-prefix test against a handful of
//! strings, with no suffix tables walked per step.
//!
//! Beside it runs a free beam of one: the best hypothesis that has left the lexicon, so a name
//! or a new word still gets the model's own spelling.
//!
//! Scoring follows `training/mlkb/translit/decode.py::beam`: hypotheses are ranked by log
//! probability per generated token, eos included, and a step's candidates fill the beam in that
//! order, finished ones counting toward its width.

use std::path::Path;
use std::time::{Duration, Instant};

use super::vocab::{self, BOS, EOS, PAD, Target};
use super::{DecoderState, ModelFile, NnError, Transformer};
use crate::lexicon::Lexicon;

/// Longest typed word transliterated, in letters.
pub const MAX_LETTERS: usize = 24;
/// Most decoder steps per word (eos included).
pub const MAX_STEPS: usize = 24;
/// Most results per word: the constrained beam's width is capped one below it, for the free one.
pub const MAX_CANDIDATES: usize = 15;
/// Tokens expanded per hypothesis per step.
const EXPAND: usize = 8;

/// One spelling of the typed word.
#[derive(Debug, Clone, PartialEq)]
pub struct Transliteration {
    pub text: String,
    /// The model's log probability of the spelling, eos included.
    pub log_prob: f32,
    /// `log_prob` per generated token (the characters and eos): what results are ranked by.
    pub score: f32,
    /// A lexicon word or split; false for the free beam's spelling.
    pub in_lexicon: bool,
}

/// The transliterator model, ready to search.
#[derive(Debug)]
pub struct Translit {
    model: Transformer,
    /// The script the model spells in: its output tokens are this vocabulary's.
    target: Target,
}

impl Translit {
    /// Opens a `.mlkbnn` transliterator trained to spell in `target` (a pack's is
    /// `Target::for_script` of the pack's script).
    pub fn open(path: &Path, target: Target) -> Result<Self, NnError> {
        Self::new(Transformer::new(&ModelFile::open(path)?)?, target)
    }

    /// Checks that `model` reads Latin and writes `target`'s tokens.
    pub fn new(model: Transformer, target: Target) -> Result<Self, NnError> {
        let c = model.config();
        if c.src_vocab != vocab::LATIN_SIZE || c.tgt_vocab != target.size() {
            return Err(NnError::Shape(format!(
                "vocabularies {} → {}, want {} → {}",
                c.src_vocab,
                c.tgt_vocab,
                vocab::LATIN_SIZE,
                target.size()
            )));
        }
        Ok(Self { model, target })
    }

    /// `candidates_with_splits` with no splits.
    pub fn candidates(&self, latin: &str, lexicon: &Lexicon, width: usize) -> Vec<Transliteration> {
        self.candidates_with_splits(latin, lexicon, &[], width)
    }

    /// Up to `width` (at most `MAX_CANDIDATES - 1`) spellings of `latin` that are `lexicon`
    /// words or among `splits`, plus at most one free-beam spelling that is neither; best
    /// `score` first. Nothing unless `latin` is 1–`MAX_LETTERS` letters `a`–`z`.
    pub fn candidates_with_splits(
        &self,
        latin: &str,
        lexicon: &Lexicon,
        splits: &[String],
        width: usize,
    ) -> Vec<Transliteration> {
        let search = Search {
            width,
            ..Search::default()
        };
        self.search(latin, lexicon, splits, &search).0
    }

    /// `candidates_with_splits` as `search` asks (without the free beam, or until a
    /// deadline), with a count of the work it did.
    pub fn search(
        &self,
        latin: &str,
        lexicon: &Lexicon,
        splits: &[String],
        search: &Search<'_>,
    ) -> (Vec<Transliteration>, BeamTrace) {
        let mut trace = BeamTrace::default();
        let t0 = Instant::now();
        let max_len = self.model.config().max_len;
        if latin.is_empty() || latin.len() > MAX_LETTERS || latin.len() + 2 > max_len {
            return (Vec::new(), trace);
        }
        let Some(src) = vocab::encode_latin(latin) else {
            return (Vec::new(), trace);
        };
        let width = search.width.min(MAX_CANDIDATES - 1);
        if width == 0 && !search.free {
            return (Vec::new(), trace);
        }
        // The encoder can't stop part way: it starts only with half the budget left.
        if search
            .deadline
            .as_ref()
            .is_some_and(|d| (d.left)() < d.budget / 2)
        {
            trace.expired = true;
            return (Vec::new(), trace);
        }
        let allowed = Allowed { lexicon, splits };
        let encoded = self.model.encode(&src);
        trace.encode = t0.elapsed();
        let mut decoder = self.model.decoder(&encoded);
        // `states[i]` is `live[i]`'s decoder history.
        let mut states = vec![decoder.start()];
        let mut live = vec![Hyp {
            next: BOS,
            text: String::new(),
            log_prob: 0.0,
            constrained: width > 0,
        }];
        let mut known: Vec<Transliteration> = Vec::new();
        let mut free: Option<Transliteration> = None;
        let mut expansions: Vec<Expansion> = Vec::new();
        let mut buf = String::new();
        // A beam holds `max_len` tokens, bos included, and each step feeds one.
        for step in 0..MAX_STEPS.min(max_len) {
            let open = |h: &Hyp| {
                if h.constrained {
                    known.len() < width
                } else {
                    free.is_none()
                }
            };
            let mut keep = live.iter().map(open);
            states.retain(|_| keep.next().unwrap());
            live.retain(open);
            if live.is_empty() {
                break;
            }
            // Out of time: what has finished is the answer.
            if search
                .deadline
                .as_ref()
                .is_some_and(|d| (d.left)().is_zero())
            {
                trace.expired = true;
                break;
            }
            let tokens: Vec<u32> = live.iter().map(|h| h.next).collect();
            let ts = Instant::now();
            let logits = decoder.step_batch(&mut states, &tokens);
            trace.decode += ts.elapsed();
            trace.steps += 1;
            trace.rows += live.len();
            let tsearch = Instant::now();

            expansions.clear();
            let rows = logits.chunks_exact(self.target.size());
            for (parent, (h, row)) in live.iter().zip(rows).enumerate() {
                let lse = log_sum_exp(row);
                for token in top_tokens(row) {
                    let valid = h.constrained
                        && if token == EOS {
                            allowed.accepts(&h.text)
                        } else {
                            buf.clear();
                            buf.push_str(&h.text);
                            buf.extend(self.target.char(token));
                            trace.prefix_checks += 1;
                            allowed.is_prefix(&buf)
                        };
                    expansions.push(Expansion {
                        parent,
                        token,
                        log_prob: h.log_prob + row[token as usize] - lse,
                        valid,
                    });
                }
            }
            // Every expansion has generated `step + 1` tokens, so ranking by total log
            // probability is ranking by `score`.
            expansions.sort_by(|a, b| b.log_prob.total_cmp(&a.log_prob));
            let generated = (step + 1) as f32;
            let finish = |h: &Hyp, e: &Expansion, in_lexicon: bool| Transliteration {
                text: h.text.clone(),
                log_prob: e.log_prob,
                score: e.log_prob / generated,
                in_lexicon,
            };

            let mut chosen: Vec<(&Expansion, bool)> = Vec::new();
            if known.len() < width {
                let mut kept = 0;
                for e in expansions.iter().filter(|e| e.valid) {
                    if e.token == EOS {
                        known.push(finish(&live[e.parent], e, true));
                    } else {
                        chosen.push((e, true));
                        kept += 1;
                    }
                    if kept + known.len() >= width {
                        break;
                    }
                }
                chosen.truncate(width.saturating_sub(known.len()));
            }
            if search.free && free.is_none() {
                // The best hypothesis off the lexicon; ending on nothing doesn't count.
                let best = expansions
                    .iter()
                    .filter(|e| !e.valid)
                    .find(|e| e.token != EOS || !live[e.parent].text.is_empty());
                match best {
                    Some(e) if e.token == EOS => free = Some(finish(&live[e.parent], e, false)),
                    Some(e) => chosen.push((e, false)),
                    None => {}
                }
            }

            // Fork the parents: each child but a parent's last takes a copy of its state.
            let mut uses = vec![0usize; live.len()];
            for (e, _) in &chosen {
                uses[e.parent] += 1;
            }
            let mut parents: Vec<Option<DecoderState>> = states.drain(..).map(Some).collect();
            let mut next = Vec::with_capacity(chosen.len());
            for (e, constrained) in chosen {
                uses[e.parent] -= 1;
                let state = if uses[e.parent] == 0 {
                    parents[e.parent].take()
                } else {
                    trace.clones += 1;
                    parents[e.parent].clone()
                };
                states.push(state.expect("a parent's state moves to its last child"));
                let mut text = live[e.parent].text.clone();
                text.extend(self.target.char(e.token));
                next.push(Hyp {
                    next: e.token,
                    text,
                    log_prob: e.log_prob,
                    constrained,
                });
            }
            live = next;
            trace.search += tsearch.elapsed();
        }
        trace.total = t0.elapsed();
        // Hypotheses still open at the step limit (or the deadline) never finished: they
        // are dropped.
        known.extend(free);
        known.sort_by(|a, b| b.score.total_cmp(&a.score));
        (known, trace)
    }
}

/// How `Translit::search` runs.
pub struct Search<'a> {
    /// Lexicon spellings wanted (at most `MAX_CANDIDATES - 1`).
    pub width: usize,
    /// Run the free beam of one beside the constrained one.
    pub free: bool,
    /// None: search to the end.
    pub deadline: Option<Deadline<'a>>,
}

impl Default for Search<'_> {
    fn default() -> Self {
        Self {
            width: 0,
            free: true,
            deadline: None,
        }
    }
}

/// A time limit on a search. The encoder runs only while at least half of `budget` is
/// `left`; a decoder step only while some is. Once out of time the search returns the
/// spellings that have finished. `left` is a clock in the keyboard, a script in tests.
#[derive(Clone, Copy)]
pub struct Deadline<'a> {
    /// The whole allowance, of which `left` is what remains.
    pub budget: Duration,
    pub left: &'a dyn Fn() -> Duration,
}

/// What one beam search did: for benches and tests, never logged on the keyboard.
#[derive(Debug, Clone, Default)]
pub struct BeamTrace {
    /// The search stopped (or never started) for its deadline.
    pub expired: bool,
    /// Decoder steps run.
    pub steps: usize,
    /// Hypotheses fed to the decoder, summed over the steps.
    pub rows: usize,
    /// `Allowed::is_prefix` calls.
    pub prefix_checks: usize,
    /// Decoder states copied to fork a hypothesis.
    pub clones: usize,
    pub encode: Duration,
    pub decode: Duration,
    /// Expansion, pruning and forking, after each step's decode.
    pub search: Duration,
    pub total: Duration,
}

/// What the constrained beam may spell: lexicon words and the typed word's splits.
struct Allowed<'a> {
    lexicon: &'a Lexicon,
    splits: &'a [String],
}

impl Allowed<'_> {
    fn accepts(&self, text: &str) -> bool {
        !text.is_empty()
            && (self.lexicon.get(text.as_bytes()).is_some()
                || self.splits.iter().any(|s| s == text))
    }

    fn is_prefix(&self, text: &str) -> bool {
        self.lexicon.has_prefix(text.as_bytes()) || self.splits.iter().any(|s| s.starts_with(text))
    }
}

/// One open hypothesis. Its decoder state has consumed every token but `next`, which is fed
/// at the next step (bos, for the first).
struct Hyp {
    next: u32,
    /// The characters generated so far, `next` included.
    text: String,
    log_prob: f32,
    /// In the constrained beam; false for the free beam's hypothesis.
    constrained: bool,
}

/// A hypothesis extended by one token.
struct Expansion {
    parent: usize,
    token: u32,
    log_prob: f32,
    /// Still a lexicon prefix (or, for eos, a whole word), from a constrained parent.
    valid: bool,
}

fn log_sum_exp(row: &[f32]) -> f32 {
    let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    max + row.iter().map(|l| (l - max).exp()).sum::<f32>().ln()
}

/// The `EXPAND` most likely tokens of `row`, best first, leaving out pad and bos.
fn top_tokens(row: &[f32]) -> Vec<u32> {
    let mut top: Vec<u32> = Vec::with_capacity(EXPAND + 1);
    for t in (0..row.len() as u32).filter(|&t| t != PAD && t != BOS) {
        let at = top.partition_point(|&u| row[u as usize] >= row[t as usize]);
        if at < EXPAND {
            top.insert(at, t);
            top.truncate(EXPAND);
        }
    }
    top
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexicon::build;

    fn tiny() -> Translit {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/translit-tiny.mlkbnn"
        );
        Translit::open(Path::new(path), Target::MALAYALAM).unwrap()
    }

    fn lexicon(words: &[&str]) -> Lexicon {
        Lexicon::from_bytes(build(words.iter().map(|w| (w.as_bytes().to_vec(), 1))).unwrap())
            .unwrap()
    }

    /// The model's log probability of `text` then eos, by teacher forcing.
    fn forced_log_prob(t: &Translit, latin: &str, text: &str) -> f32 {
        let enc = t.model.encode(&vocab::encode_latin(latin).unwrap());
        let mut dec = t.model.decoder(&enc);
        let mut state = dec.start();
        let mut prev = BOS;
        let mut total = 0.0;
        for next in text
            .chars()
            .map(|c| t.target.token(c).unwrap())
            .chain([EOS])
        {
            let logits = dec.step(&mut state, prev);
            total += log_softmax_at(logits, next);
            prev = next;
        }
        total
    }

    fn log_softmax_at(logits: &[f32], i: u32) -> f32 {
        let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let sum: f32 = logits.iter().map(|l| (l - max).exp()).sum();
        logits[i as usize] - max - sum.ln()
    }

    /// Words the tiny fixture's random weights can spell for `ab`: each ends where eos is among
    /// a hypothesis's top tokens. Most branches never end, the free beam's included, so
    /// `tests/bundle.rs` checks the free beam on the trained model.
    const AB_WORDS: &[&str] = &[
        "ഛഛ\u{d53}",
        "ഛഛഘ",
        "ഛഛ൮",
        "ഛഛഛ\u{d53}",
        "ഛഛഛഘ",
        "ഛഛഓ\u{d53}",
        "ഛഛഘഅ",
        "ഛൔഛ\u{d53}",
    ];

    fn check_scores(t: &Translit, latin: &str, got: &[Transliteration]) {
        for c in got {
            let want = forced_log_prob(t, latin, &c.text);
            assert!((c.log_prob - want).abs() < 1e-3, "{c:?}: forced {want}");
            let tokens = c.text.chars().count() + 1;
            assert!((c.score - c.log_prob / tokens as f32).abs() < 1e-6, "{c:?}");
        }
        assert!(got.windows(2).all(|w| w[0].score >= w[1].score), "{got:?}");
    }

    #[test]
    fn at_most_width_lexicon_words_and_one_free_spelling() {
        let t = tiny();
        let lex = lexicon(AB_WORDS);
        let got = t.candidates("ab", &lex, 4);
        let (known, free): (Vec<_>, Vec<_>) = got.iter().partition(|c| c.in_lexicon);
        assert_eq!(known.len(), 4, "{got:?}");
        assert!(
            known.iter().all(|c| lex.get(c.text.as_bytes()).is_some()),
            "{got:?}"
        );
        assert!(free.len() <= 1, "{got:?}");
        assert!(
            free.iter().all(|c| lex.get(c.text.as_bytes()).is_none()),
            "{got:?}"
        );
        check_scores(&t, "ab", &got);

        let wide = t.candidates("ab", &lex, 100);
        assert!(wide.iter().filter(|c| c.in_lexicon).count() > 4, "{wide:?}");
        assert!(wide.len() <= MAX_CANDIDATES);
        assert!(t.candidates("ab", &lex, 0).iter().all(|c| !c.in_lexicon));
    }

    #[test]
    fn branches_off_the_lexicon_are_pruned_except_in_the_free_beam() {
        let t = tiny();
        // One word: every other branch leaves the lexicon at some step, though the model
        // likes several of them better (ഛഛ\u{d53} and ഛഛഘ above).
        let lex = lexicon(&["ഛഛ൮"]);
        let got = t.candidates("ab", &lex, 4);
        let known: Vec<&str> = got
            .iter()
            .filter(|c| c.in_lexicon)
            .map(|c| c.text.as_str())
            .collect();
        assert_eq!(known, ["ഛഛ൮"], "{got:?}");
        assert!(got.len() <= 2, "{got:?}");
        check_scores(&t, "ab", &got);

        // Words the model never starts with: nothing but (at most) the free spelling.
        let far = lexicon(&["അമ്മ", "കേരളം"]);
        let got = t.candidates("ab", &far, 4);
        assert!(
            got.len() <= 1 && got.iter().all(|c| !c.in_lexicon),
            "{got:?}"
        );
    }

    /// A clock that has `budget` left for its first `calls` readings, then none.
    fn clock(budget: Duration, calls: usize) -> impl Fn() -> Duration {
        let read = std::cell::Cell::new(0);
        move || {
            read.set(read.get() + 1);
            if read.get() <= calls {
                budget
            } else {
                Duration::ZERO
            }
        }
    }

    #[test]
    fn a_deadline_stops_the_search_with_what_has_finished() {
        let t = tiny();
        let lex = lexicon(AB_WORDS);
        let budget = Duration::from_millis(10);
        let search = |deadline| Search {
            width: 4,
            deadline,
            ..Search::default()
        };
        let (all, unbounded) = t.search("ab", &lex, &[], &search(None));
        assert!(!unbounded.expired && unbounded.steps > 3, "{unbounded:?}");

        // Less than half the budget left: the encoder doesn't start.
        let half_gone = || budget / 2 - Duration::from_nanos(1);
        let (got, trace) = t.search(
            "ab",
            &lex,
            &[],
            &search(Some(Deadline {
                budget,
                left: &half_gone,
            })),
        );
        assert!(
            got.is_empty() && trace.expired && trace.steps == 0,
            "{trace:?}"
        );

        // Time for the encoder and three steps: three steps run, and what finished in them is
        // what the unbounded search found by then.
        let left = clock(budget, 4);
        let (got, trace) = t.search(
            "ab",
            &lex,
            &[],
            &search(Some(Deadline {
                budget,
                left: &left,
            })),
        );
        assert!(trace.expired && trace.steps == 3, "{trace:?}");
        assert!(got.len() < all.len(), "{got:?}");
        assert!(
            got.iter().all(|c| all.contains(c)),
            "{got:?} not in {all:?}"
        );
        check_scores(&t, "ab", &got);

        // A budget never used up changes nothing.
        let plenty = || budget;
        let (got, _) = t.search(
            "ab",
            &lex,
            &[],
            &search(Some(Deadline {
                budget,
                left: &plenty,
            })),
        );
        assert_eq!(got, all);
    }

    #[test]
    fn without_the_free_beam_the_lexicon_spellings_are_unchanged() {
        let t = tiny();
        let lex = lexicon(AB_WORDS);
        let with = t.candidates("ab", &lex, 4);
        let search = Search {
            width: 4,
            free: false,
            ..Search::default()
        };
        let (without, _) = t.search("ab", &lex, &[], &search);
        let known: Vec<_> = with.into_iter().filter(|c| c.in_lexicon).collect();
        assert_eq!(without, known);
        let none = Search {
            free: false,
            ..Search::default()
        };
        assert!(t.search("ab", &lex, &[], &none).0.is_empty());
    }

    #[test]
    fn splits_count_as_lexicon_words() {
        let t = tiny();
        let empty = lexicon(&[]);
        let splits = vec!["ഛഛഘ".to_string()];
        let got = t.candidates_with_splits("ab", &empty, &splits, 4);
        let known: Vec<&str> = got
            .iter()
            .filter(|c| c.in_lexicon)
            .map(|c| c.text.as_str())
            .collect();
        assert_eq!(known, ["ഛഛഘ"], "{got:?}");
        check_scores(&t, "ab", &got);
    }

    #[test]
    fn only_short_lowercase_latin_words() {
        let t = tiny();
        let lex = lexicon(AB_WORDS);
        for bad in [
            "",
            "Kerala",
            "ker ala",
            "kerala1",
            "കേരളം",
            "abcdefghijklmnopqrstuvwxy", // 25 letters
        ] {
            assert!(t.candidates(bad, &lex, 4).is_empty(), "{bad}");
        }
        // The fixture's 16 positions hold 14 letters with bos and eos; longer words are skipped
        // rather than overflowing the encoder.
        assert!(t.candidates("abcdefghijklmno", &lex, 4).is_empty());
        let _ = t.candidates("abcdefghijklmn", &lex, 4);
    }
}

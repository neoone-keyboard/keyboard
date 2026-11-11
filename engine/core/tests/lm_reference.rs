//! The LM's int8 forward pass and context cache against PyTorch, on a tiny seeded model
//! (vocab 300, d=64, 2 layers).
//!
//! `uv run --group train python -m mlkb.lm.make_fixture` (from `training/`) regenerates
//! `fixtures/lm-tiny.mlkbnn` and `lm-tiny-reference.json`: the log-softmax after the last token
//! of a few token sequences, from PyTorch on the dequantised int8 weights, and the same after
//! feeding 20 tokens, dropping the oldest 8 rows and feeding 4 more.
//!
//! Ignored (`make bench`), on `training/build/lm.mlkbnn` when it exists: `lm_latency` times a
//! commit, one token and a fresh prefill; `ring_matches_fresh_windows` checks that a cache
//! that drops its oldest words predicts as well as one computed afresh (design §9).
//! `MLENGINE_LM_LANG=hi` runs them on Hindi's (`training/build/lm-hi.mlkbnn`, its held-out
//! English and Hindi sentences in `training/build/lm-hi/`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mlengine_core::nn::tokenizer::BOS;
use mlengine_core::nn::{Lm, LmScratch};

fn fixture() -> Lm {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    Lm::open(&dir.join("lm-tiny.mlkbnn")).unwrap()
}

fn reference() -> serde_json::Value {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    serde_json::from_str(&std::fs::read_to_string(dir.join("lm-tiny-reference.json")).unwrap())
        .unwrap()
}

fn ids(v: &serde_json::Value) -> Vec<u32> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_u64().unwrap() as u32)
        .collect()
}

fn close(got: &[f32], want: &serde_json::Value) {
    let want: Vec<f32> = want
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap() as f32)
        .collect();
    assert_eq!(got.len(), want.len());
    let worst = got
        .iter()
        .zip(&want)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(worst < 2e-2, "max |Δ| = {worst}");
}

#[test]
fn next_log_probs_match_pytorch() {
    let (lm, r) = (fixture(), reference());
    let mut s = LmScratch::default();
    for case in r["cases"].as_array().unwrap() {
        let tokens = ids(&case["tokens"]);
        let mut cache = lm.cache();
        lm.feed(&mut cache, &tokens, &mut s);
        close(lm.next_log_probs(&cache, &mut s), &case["log_probs"]);
    }
}

#[test]
fn feeding_one_token_at_a_time_equals_one_batch() {
    let lm = fixture();
    let mut s = LmScratch::default();
    let tokens = [2, 40, 41, 42, 43, 44];
    let (mut a, mut b) = (lm.cache(), lm.cache());
    lm.feed(&mut a, &tokens, &mut s);
    for &t in &tokens {
        lm.feed(&mut b, &[t], &mut s);
    }
    let x = lm.next_log_probs(&a, &mut s).to_vec();
    let y = lm.next_log_probs(&b, &mut s).to_vec();
    assert!(x.iter().zip(&y).all(|(p, q)| (p - q).abs() < 1e-4));
}

#[test]
fn evicting_old_rows_matches_pytorch_on_the_kept_rows() {
    let (lm, r) = (fixture(), reference());
    let e = &r["eviction"];
    let tokens = ids(&e["tokens"]);
    let (fed, evict) = (
        e["feed"].as_u64().unwrap() as usize,
        e["evict"].as_u64().unwrap() as usize,
    );
    assert_eq!((fed, evict, tokens.len()), (20, 8, 24));
    let mut s = LmScratch::default();
    let mut cache = lm.cache();
    lm.feed(&mut cache, &tokens[..20], &mut s);
    lm.evict_front(&mut cache, 8);
    lm.feed(&mut cache, &tokens[20..24], &mut s);
    assert_eq!(cache.rows(), 16);
    assert_eq!(cache.next_pos(), 24);
    close(lm.next_log_probs(&cache, &mut s), &e["log_probs"]);
}

#[test]
fn truncating_restores_the_earlier_state() {
    let lm = fixture();
    let mut s = LmScratch::default();
    let mut cache = lm.cache();
    lm.feed(&mut cache, &[2, 40, 41], &mut s);
    let before = lm.next_log_probs(&cache, &mut s).to_vec();
    lm.feed(&mut cache, &[42, 43], &mut s);
    lm.truncate(&mut cache, 3);
    assert_eq!(lm.next_log_probs(&cache, &mut s), &before[..]);
}

#[test]
fn an_empty_cache_has_no_distribution_and_restarts_positions() {
    let lm = fixture();
    let mut s = LmScratch::default();
    let mut cache = lm.cache();
    assert!(lm.next_log_probs(&cache, &mut s).is_empty());
    lm.feed(&mut cache, &[2, 40], &mut s);
    lm.evict_front(&mut cache, 2);
    assert_eq!((cache.rows(), cache.next_pos()), (0, 2));
    lm.feed(&mut cache, &[2], &mut s);
    assert_eq!(
        cache.next_pos(),
        1,
        "an empty cache starts again at position 0"
    );
}

/// The language of the trained LM to test (`MLENGINE_LM_LANG`), Malayalam by default.
fn lm_lang() -> String {
    std::env::var("MLENGINE_LM_LANG").unwrap_or_else(|_| "ml".into())
}

/// `training/build/lm` for Malayalam, `training/build/lm-<lang>` for another language.
fn build_stem() -> PathBuf {
    let lang = lm_lang();
    let stem = if lang == "ml" {
        "lm".to_string()
    } else {
        format!("lm-{lang}")
    };
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../training/build")
        .join(stem)
}

fn trained() -> Option<Lm> {
    let path = build_stem().with_extension("mlkbnn");
    if !path.exists() {
        println!("skipped: {} is missing", path.display());
        return None;
    }
    let mut lm = Lm::open(&path).unwrap();
    // Its native pieces are its pack's script, as the engine sets them when the pack opens.
    let script = match lm_lang().as_str() {
        "ml" => Some("mlym"),
        "hi" => Some("deva"),
        "as" => Some("beng"),
        _ => None,
    };
    if let Some(rules) = script.and_then(mlengine_core::script::embedded) {
        lm.set_native(rules.block());
    }
    Some(lm)
}

fn percentiles(mut times: Vec<Duration>) -> (Duration, Duration) {
    times.sort();
    (times[times.len() / 2], times[times.len() * 99 / 100])
}

#[test]
#[ignore = "benchmark: make bench"]
fn lm_latency() {
    let Some(lm) = trained() else { return };
    let tok = lm.tokenizer();
    let context = tok.encode(
        "i was thinking we could meet at the station tomorrow morning before the train leaves \
         and then walk to the office together if you have time",
    );
    let mut ctx: Vec<u32> = vec![BOS];
    ctx.extend(&context);
    ctx.truncate(31);
    let word = tok.word("together");
    let two = [word[0], *tok.word("anyway").last().unwrap()];
    let mut s = LmScratch::default();
    let runs = 500;
    let time = |label: &str, run: &mut dyn FnMut()| {
        run();
        let times = (0..runs)
            .map(|_| {
                let start = Instant::now();
                run();
                start.elapsed()
            })
            .collect();
        let (p50, p99) = percentiles(times);
        println!("lm {label}: p50 {p50:?}, p99 {p99:?} over {runs} runs");
        p99
    };
    let mut base = lm.cache();
    lm.feed(&mut base, &ctx[..24], &mut s);
    let commit = time("commit (2-token word + next_log_probs)", &mut || {
        let rows = base.rows();
        lm.feed(&mut base, &two, &mut s);
        std::hint::black_box(lm.next_log_probs(&base, &mut s));
        lm.truncate(&mut base, rows);
    });
    time("feed one token", &mut || {
        let rows = base.rows();
        lm.feed(&mut base, &two[..1], &mut s);
        lm.truncate(&mut base, rows);
    });
    let mut prefill = vec![BOS];
    prefill.extend(tok.encode(&"the cat sat on the mat and ".repeat(6)));
    prefill.truncate(32);
    time("fresh 32-token prefill + next_log_probs", &mut || {
        let mut cache = lm.cache();
        lm.feed(&mut cache, &prefill, &mut s);
        std::hint::black_box(lm.next_log_probs(&cache, &mut s));
    });
    assert!(
        commit <= Duration::from_millis(1),
        "LM commit p99 {commit:?} over 1 ms"
    );
}

/// Mean next-token log loss over the tokens of `lines` (one stream), predicted from a cache that
/// drops its oldest whole words to stay within 32 tokens (as `LmContext` does), and from the
/// same tokens computed afresh at positions from 0.
fn ring_and_fresh(lm: &Lm, lines: &[&str]) -> (f64, f64, usize) {
    const WINDOW: usize = 32;
    let tok = lm.tokenizer();
    let mut s = LmScratch::default();
    let mut ring = lm.cache();
    lm.feed(&mut ring, &[BOS], &mut s);
    // Token counts of the cached words, oldest first; bos counts as a word.
    let mut words: Vec<usize> = vec![1];
    let mut held: Vec<u32> = vec![BOS];
    let (mut ring_loss, mut fresh_loss, mut n) = (0f64, 0f64, 0usize);
    for line in lines {
        for word in line.split_whitespace() {
            let tokens = tok.word(word);
            for &t in &tokens {
                let lp = lm.next_log_probs(&ring, &mut s)[t as usize];
                ring_loss -= f64::from(lp);
                let mut fresh = lm.cache();
                lm.feed(&mut fresh, &held, &mut s);
                fresh_loss -= f64::from(lm.next_log_probs(&fresh, &mut s)[t as usize]);
                n += 1;
                lm.feed(&mut ring, &[t], &mut s);
                held.push(t);
            }
            words.push(tokens.len());
            while ring.rows() > WINDOW && words.len() > 1 {
                let drop = words.remove(0);
                lm.evict_front(&mut ring, drop);
                held.drain(..drop);
            }
        }
    }
    (ring_loss / n as f64, fresh_loss / n as f64, n)
}

#[test]
#[ignore = "slow, needs the trained model: make bench"]
fn ring_matches_fresh_windows() {
    let Some(lm) = trained() else { return };
    let dir = build_stem();
    for name in [
        "heldout-en.txt".to_string(),
        format!("heldout-{}.txt", lm_lang()),
    ] {
        let text = std::fs::read_to_string(dir.join(&name)).unwrap();
        let lines: Vec<&str> = text.lines().take(150).collect();
        let (ring, fresh, n) = ring_and_fresh(&lm, &lines);
        let gap = (ring - fresh) / fresh;
        println!(
            "{name}: {n} tokens, ring {ring:.4} vs fresh {fresh:.4} nats/token ({:+.2}%)",
            gap * 100.0
        );
        assert!(
            gap.abs() < 0.02,
            "{name}: ring differs by {:.2}%",
            gap * 100.0
        );
    }
}

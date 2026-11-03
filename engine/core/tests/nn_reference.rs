//! The int8 forward pass against PyTorch, on a tiny seeded model (d=64, 2+2 layers).
//!
//! `uv run --group train python -m mlkb.translit.make_fixture` (from `training/`) regenerates
//! `fixtures/translit-tiny.mlkbnn` and `translit-tiny-reference.json`. The reference holds, per
//! input, the encoder output and the decoder logits after every fed token, from PyTorch on the
//! dequantised int8 weights (which the engine must match to float rounding) and on the original
//! float weights (int8 error included, so a looser bound).
//!
//! `nn_latency` (ignored; `make bench`) times the forward pass at full size.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mlengine_core::nn::{Config, DecoderState, ModelFile, Transformer};
use serde_json::Value;

/// Engine vs PyTorch on the same (dequantised) weights: only float rounding differs.
const DEQUANTISED_TOLERANCE: f32 = 1e-4;
/// Engine vs PyTorch on the float weights. The fixture's random weights are much rougher than
/// a trained model's, so int8 error is large here; this bound only catches gross bugs.
const FLOAT_TOLERANCE: f32 = 0.25;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn tiny() -> Transformer {
    let file = ModelFile::open(&fixtures().join("translit-tiny.mlkbnn")).unwrap();
    Transformer::new(&file).unwrap()
}

struct Case {
    latin: String,
    src: Vec<u32>,
    tgt: Vec<u32>,
    /// `[kind]["encoder" | "logits"]`, flattened row-major, with the row count.
    outputs: Value,
}

fn cases() -> Vec<Case> {
    let text = std::fs::read_to_string(fixtures().join("translit-tiny-reference.json")).unwrap();
    let json: Value = serde_json::from_str(&text).unwrap();
    let ids = |v: &Value| -> Vec<u32> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|t| t.as_u64().unwrap() as u32)
            .collect()
    };
    json["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| Case {
            latin: c["latin"].as_str().unwrap().to_string(),
            src: ids(&c["src"]),
            tgt: ids(&c["tgt"]),
            outputs: c.clone(),
        })
        .collect()
}

impl Case {
    fn rows(&self, kind: &str, what: &str) -> Vec<Vec<f32>> {
        self.outputs[kind][what]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                row.as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_f64().unwrap() as f32)
                    .collect()
            })
            .collect()
    }
}

fn max_diff(got: &[f32], want: &[f32]) -> f32 {
    assert_eq!(got.len(), want.len());
    got.iter()
        .zip(want)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max)
}

#[test]
fn fixture_has_the_tiny_shape() {
    let t = tiny();
    let c = t.config();
    assert_eq!(
        (c.d, c.heads, c.enc_layers, c.dec_layers),
        (64, 4, 2, 2),
        "{c:?}"
    );
    assert!(cases().len() >= 3);
    assert!(cases().iter().all(|c| c.tgt.len() >= 3));
}

#[test]
fn encoder_matches_pytorch() {
    let t = tiny();
    let d = t.config().d;
    for case in cases() {
        let enc = t.encode(&case.src);
        assert_eq!(enc.len(), case.src.len());
        for (kind, tolerance) in [
            ("dequantised", DEQUANTISED_TOLERANCE),
            ("float", FLOAT_TOLERANCE),
        ] {
            let want: Vec<f32> = case.rows(kind, "encoder").concat();
            assert_eq!(want.len(), case.src.len() * d);
            let diff = max_diff(enc.output(), &want);
            println!("{} encoder vs {kind}: max abs diff {diff:.2e}", case.latin);
            assert!(
                diff <= tolerance,
                "{} encoder vs {kind}: {diff}",
                case.latin
            );
        }
    }
}

#[test]
fn every_decoder_step_matches_pytorch() {
    let t = tiny();
    for case in cases() {
        let enc = t.encode(&case.src);
        let mut dec = t.decoder(&enc);
        let mut state = dec.start();
        let dequantised = case.rows("dequantised", "logits");
        let float = case.rows("float", "logits");
        for (i, &token) in case.tgt.iter().enumerate() {
            let logits = dec.step(&mut state, token);
            assert_eq!(state.len(), i + 1);
            let diff = max_diff(logits, &dequantised[i]);
            let diff_float = max_diff(logits, &float[i]);
            println!(
                "{} step {i}: max abs diff {diff:.2e} (dequantised), {diff_float:.2e} (float)",
                case.latin
            );
            assert!(
                diff <= DEQUANTISED_TOLERANCE,
                "{} step {i}: {diff}",
                case.latin
            );
            assert!(
                diff_float <= FLOAT_TOLERANCE,
                "{} step {i}: {diff_float}",
                case.latin
            );
        }
    }
}

#[test]
fn forked_and_batched_beams_match_single_steps() {
    let t = tiny();
    let case = cases().into_iter().max_by_key(|c| c.tgt.len()).unwrap();
    let want = case.rows("dequantised", "logits");
    let vocab = t.config().tgt_vocab;
    let enc = t.encode(&case.src);
    let mut dec = t.decoder(&enc);

    // Two steps on one beam, then fork it; the fork must not see the parent's later tokens.
    let mut parent = dec.start();
    dec.step(&mut parent, case.tgt[0]);
    dec.step(&mut parent, case.tgt[1]);
    let fork = parent.clone();
    dec.step(&mut parent, 0); // a different token on the parent only

    // One batch: the fork continues the reference, a fresh beam starts it, and the parent
    // (one token ahead, with a different history) rides along.
    let mut beams: Vec<DecoderState> = vec![fork, dec.start(), parent];
    let logits = dec
        .step_batch(&mut beams, &[case.tgt[2], case.tgt[0], case.tgt[3]])
        .to_vec();
    assert_eq!(logits.len(), 3 * vocab);
    assert!(max_diff(&logits[..vocab], &want[2]) <= DEQUANTISED_TOLERANCE);
    assert!(max_diff(&logits[vocab..2 * vocab], &want[0]) <= DEQUANTISED_TOLERANCE);
    assert!(max_diff(&logits[2 * vocab..], &want[3]) > 1e-3); // its history differs
    assert_eq!(
        beams.iter().map(DecoderState::len).collect::<Vec<_>>(),
        [3, 1, 4]
    );

    // Batched steps keep matching the reference one step at a time.
    for (i, (&token, want)) in case.tgt.iter().zip(&want).enumerate().skip(3) {
        let logits = dec.step_batch(&mut beams[..1], &[token]);
        assert!(max_diff(logits, want) <= DEQUANTISED_TOLERANCE, "step {i}");
    }
}

// ---- Latency ----

/// A 64-bit LCG; enough randomness for weights whose values don't matter.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn unit(&mut self) -> f32 {
        (self.next() % 2001) as f32 / 1000.0 - 1.0
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The `state_dict` names and shapes of `mlkb.translit.model.Translit` for `c`.
fn translit_tensors(c: &Config) -> Vec<(String, Vec<usize>)> {
    let (d, ff) = (c.d, c.ff);
    let mut out: Vec<(String, Vec<usize>)> = vec![
        ("src_emb.weight".into(), vec![c.src_vocab, d]),
        ("src_pos.weight".into(), vec![c.max_len, d]),
        ("tgt_emb.weight".into(), vec![c.tgt_vocab, d]),
        ("tgt_pos.weight".into(), vec![c.max_len, d]),
    ];
    let attn = |p: &str| {
        vec![
            (format!("{p}.in_proj_weight"), vec![3 * d, d]),
            (format!("{p}.in_proj_bias"), vec![3 * d]),
            (format!("{p}.out_proj.weight"), vec![d, d]),
            (format!("{p}.out_proj.bias"), vec![d]),
        ]
    };
    let rest = |p: &str, norms: usize| {
        let mut v = vec![
            (format!("{p}.linear1.weight"), vec![ff, d]),
            (format!("{p}.linear1.bias"), vec![ff]),
            (format!("{p}.linear2.weight"), vec![d, ff]),
            (format!("{p}.linear2.bias"), vec![d]),
        ];
        for n in 1..=norms {
            v.push((format!("{p}.norm{n}.weight"), vec![d]));
            v.push((format!("{p}.norm{n}.bias"), vec![d]));
        }
        v
    };
    for i in 0..c.enc_layers {
        out.extend(attn(&format!("enc.{i}.self_attn")));
        out.extend(rest(&format!("enc.{i}"), 2));
    }
    out.push(("enc_norm.weight".into(), vec![d]));
    out.push(("enc_norm.bias".into(), vec![d]));
    for i in 0..c.dec_layers {
        out.extend(attn(&format!("dec.{i}.self_attn")));
        out.extend(attn(&format!("dec.{i}.multihead_attn")));
        out.extend(rest(&format!("dec.{i}"), 3));
    }
    out.push(("dec_norm.weight".into(), vec![d]));
    out.push(("dec_norm.bias".into(), vec![d]));
    out
}

/// Writes a random model of shape `c` in the `.mlkbnn` format (as `export.py` does).
fn write_random_model(path: &Path, c: &Config, seed: u64) {
    let mut rng = Lcg(seed);
    let mut out = b"MLKBNN01".to_vec();
    let tensors = translit_tensors(c);
    let header = [
        1,
        c.kind,
        c.d as u32,
        c.heads as u32,
        c.enc_layers as u32,
        c.dec_layers as u32,
        c.ff as u32,
        c.max_len as u32,
        c.src_vocab as u32,
        c.tgt_vocab as u32,
        tensors.len() as u32,
    ];
    for v in header {
        out.extend(v.to_le_bytes());
    }
    for (name, dims) in &tensors {
        out.extend((name.len() as u16).to_le_bytes());
        out.extend(name.as_bytes());
        out.push(if dims.len() == 2 { 1 } else { 0 });
        out.push(dims.len() as u8);
        for &d in dims {
            out.extend((d as u32).to_le_bytes());
        }
        out.resize(out.len().next_multiple_of(16), 0);
        if let [rows, cols] = dims[..] {
            let scale = 1.0 / (127.0 * (cols as f32).sqrt());
            for _ in 0..rows {
                out.extend(scale.to_le_bytes());
            }
            out.extend((0..rows * cols).map(|_| (rng.next() % 255) as u8 ^ 0x80));
        } else {
            for _ in 0..dims[0] {
                out.extend((1.0 + 0.1 * rng.unit()).to_le_bytes());
            }
        }
    }
    let sum = fnv1a(&out);
    out.extend(sum.to_le_bytes());
    std::fs::write(path, out).unwrap();
}

fn percentile(sorted: &[Duration], p: f64) -> Duration {
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

/// 200 Latin words of 3–12 letters, as token ids with bos/eos (Latin vocabulary: a = 3).
fn words() -> Vec<Vec<u32>> {
    let mut rng = Lcg(7);
    (0..200)
        .map(|_| {
            let len = 3 + (rng.next() % 10) as usize;
            let mut w = vec![1];
            w.extend((0..len).map(|_| 3 + (rng.next() % 26) as u32));
            w.push(2);
            w
        })
        .collect()
}

/// One encode plus `steps` decoder steps over `beam` beams, forking the beams after every
/// step the way a beam search does. Returns (batched, one-beam-at-a-time) timings per word.
fn time_model(t: &Transformer, beam: usize, steps: usize) -> (Vec<Duration>, Vec<Duration>) {
    let vocab = t.config().tgt_vocab;
    let argmax = |l: &[f32]| (3..vocab).max_by(|&a, &b| l[a].total_cmp(&l[b])).unwrap() as u32;
    let mut batched = Vec::new();
    let mut single = Vec::new();
    for src in words() {
        let start = Instant::now();
        let enc = t.encode(&src);
        let mut dec = t.decoder(&enc);
        let mut beams: Vec<DecoderState> = vec![dec.start(); beam];
        let mut tokens = vec![1; beam];
        for _ in 0..steps {
            let logits = dec.step_batch(&mut beams, &tokens);
            for (b, token) in tokens.iter_mut().enumerate() {
                *token = argmax(&logits[b * vocab..(b + 1) * vocab]);
            }
            beams = (0..beam).map(|b| beams[(b + 1) % beam].clone()).collect();
        }
        batched.push(start.elapsed());

        let start = Instant::now();
        let enc = t.encode(&src);
        let mut dec = t.decoder(&enc);
        let mut beams: Vec<DecoderState> = vec![dec.start(); beam];
        let mut tokens = vec![1; beam];
        for _ in 0..steps {
            for (state, token) in beams.iter_mut().zip(tokens.iter_mut()) {
                *token = argmax(dec.step(state, *token));
            }
            beams = (0..beam).map(|b| beams[(b + 1) % beam].clone()).collect();
        }
        single.push(start.elapsed());
    }
    batched.sort();
    single.sort();
    (batched, single)
}

fn report(label: &str, t: &Transformer) {
    // Warm up caches and the mapping first.
    time_model(t, 4, 10);
    let (batched, single) = time_model(t, 4, 10);
    for (how, times) in [("batched", &batched), ("one beam at a time", &single)] {
        println!(
            "{label}: encode + 10 steps x beam 4, {how}: p50 {:?}, p99 {:?}",
            percentile(times, 0.5),
            percentile(times, 0.99)
        );
    }
}

#[test]
#[ignore = "timing; run with `make bench` (release build)"]
fn nn_latency() {
    let config = Config {
        kind: 1,
        d: 256,
        heads: 4,
        enc_layers: 4,
        dec_layers: 3,
        ff: 512,
        max_len: 32,
        src_vocab: 29,
        tgt_vocab: 133,
    };
    let dir = tempfile::tempdir().unwrap();
    let random = dir.path().join("random.mlkbnn");
    write_random_model(&random, &config, 1);
    let start = Instant::now();
    let file = ModelFile::open(&random).unwrap();
    let t = Transformer::new(&file).unwrap();
    println!(
        "random full-size model: {} bytes, load {:?}",
        std::fs::metadata(&random).unwrap().len(),
        start.elapsed()
    );
    report("random full-size model", &t);

    let trained =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../training/build/translit.mlkbnn");
    if trained.exists() {
        let t = Transformer::new(&ModelFile::open(&trained).unwrap()).unwrap();
        report("training/build/translit.mlkbnn", &t);
    } else {
        println!(
            "skipping the trained model: {} is missing (export it with mlkb.translit.export)",
            trained.display()
        );
    }
}

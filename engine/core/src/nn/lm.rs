//! The bilingual LM's forward pass: the int8 re-implementation of `training/mlkb/lm/model.py`
//! (`LM`, in eval mode), with a context cache.
//!
//! - `x = emb[t]`, then pre-norm layers `x += out(Attn(rope(q), rope(k), v))` over
//!   `qkv(norm1(x))`, causal, and `x += ff2(relu(ff1(norm2(x))))`; logits are
//!   `norm(x) · embᵀ`, tied and without bias.
//! - Rotary positions turn q and k of each head by their absolute position, so an attention
//!   score depends only on how far apart two rows are. The cache can therefore drop its oldest
//!   rows (`evict_front`) and keep going without recomputing the rest.
//!
//! The cache holds, per layer, the rotated keys and the values of every context row, and the
//! final-normed hidden state of each row (the next-token distribution reads the newest).

use std::path::Path;

use super::file::{Config, KIND_LM, ModelFile, NnError, TensorRef};
use super::ops::{QMatrix, QuantBuf, add_assign, attend, matmul_i8, relu, rope};
use super::tokenizer::Tokenizer;
use super::weights::{Linear, Loader, Mat, Norm, norm_rows};

/// Positions past this are not reused: the caller (`LmContext`) prefills afresh, from 0.
pub const MAX_POSITION: usize = 4096;

#[derive(Debug)]
struct Layer {
    norm1: Norm,
    /// q, k, v projections packed as `[3d, d]`.
    qkv: Linear,
    out: Linear,
    norm2: Norm,
    ff1: Linear,
    ff2: Linear,
}

/// Per-piece tables written by `training/mlkb/lm/export.py` (design §3).
#[derive(Debug)]
pub struct Tables {
    /// For a Latin piece, the share of its frequency that is English rather than Manglish.
    pub p_en: Vec<f32>,
    /// ln of the share of `en.fst` word counts held by words whose first token is the piece;
    /// -inf when no word starts with it.
    pub first_en: Vec<f32>,
    /// The same for `ml.fst`.
    pub first_ml: Vec<f32>,
    /// FNV-1a (little-endian) of the `en.fst` and `ml.fst` the tables were built from.
    pub fingerprint: [u8; 16],
}

/// The bilingual LM with its tokeniser and lookup tables, read from one `.mlkbnn` of kind
/// `KIND_LM`. It shares the file's mapping: the int8 values are read in place.
#[derive(Debug)]
pub struct Lm {
    file: ModelFile,
    config: Config,
    emb: Mat,
    layers: Vec<Layer>,
    norm: Norm,
    inv_freq: Vec<f32>,
    tokenizer: Tokenizer,
    tables: Tables,
}

/// The context: per layer, the keys (rotated) and values of its rows, `[k (d), v (d)]` each;
/// the final-normed hidden state of each row; and the absolute position of the first row.
#[derive(Debug, Clone, Default)]
pub struct Cache {
    first_pos: usize,
    rows: usize,
    kv: Vec<Vec<f32>>,
    hidden: Vec<f32>,
}

impl Cache {
    /// Context rows held.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Absolute position the next fed token gets.
    pub fn next_pos(&self) -> usize {
        self.first_pos + self.rows
    }
}

/// Work buffers for `feed` and `next_log_probs`; grows to the largest call, then reused.
#[derive(Debug, Default)]
pub struct LmScratch {
    n: usize,
    x: Vec<f32>,
    h: Vec<f32>,
    qkv: Vec<f32>,
    attn: Vec<f32>,
    proj: Vec<f32>,
    ff: Vec<f32>,
    logits: Vec<f32>,
    quant: QuantBuf,
    scores: Vec<f32>,
}

impl LmScratch {
    /// Room for `n` new rows attending over `t` rows.
    fn ensure(&mut self, c: &Config, n: usize, t: usize) {
        if self.scores.len() < t {
            self.scores.resize(t, 0.0);
        }
        if n <= self.n {
            return;
        }
        self.n = n;
        self.x.resize(n * c.d, 0.0);
        self.h.resize(n * c.d, 0.0);
        self.qkv.resize(n * 3 * c.d, 0.0);
        self.attn.resize(n * c.d, 0.0);
        self.proj.resize(n * c.d, 0.0);
        self.ff.resize(n * c.ff, 0.0);
    }
}

fn f32s(file: &ModelFile, name: &str, len: usize) -> Result<Vec<f32>, NnError> {
    Loader(file).vector(name, len)
}

fn bytes<'a>(file: &'a ModelFile, name: &str) -> Result<&'a [u8], NnError> {
    match file.tensor(name) {
        Some(TensorRef::Bytes(b)) => Ok(b),
        Some(t) => Err(NnError::Shape(format!(
            "{name}: expected bytes, found {:?}",
            t.dims()
        ))),
        None => Err(NnError::Missing(name.into())),
    }
}

impl Lm {
    /// Opens a `.mlkbnn` language model.
    pub fn open(path: &Path) -> Result<Self, NnError> {
        Self::new(&ModelFile::open(path)?)
    }

    /// Resolves every tensor of `file` once, checking each shape against the header, and
    /// builds the tokeniser.
    pub fn new(file: &ModelFile) -> Result<Self, NnError> {
        let c = file.config();
        if c.kind != KIND_LM || c.enc_layers != 0 {
            return Err(NnError::Malformed(format!(
                "model kind {} with {} encoder layers is not a language model",
                c.kind, c.enc_layers
            )));
        }
        if c.heads == 0
            || c.d == 0
            || !c.d.is_multiple_of(c.heads)
            || !(c.d / c.heads).is_multiple_of(2)
        {
            return Err(NnError::Shape(format!(
                "d = {} must be a positive multiple of heads = {}, with an even head width",
                c.d, c.heads
            )));
        }
        let vocab = c.tgt_vocab;
        let pieces = std::str::from_utf8(bytes(file, "tok.pieces")?)
            .map_err(|e| NnError::Malformed(format!("tok.pieces: {e}")))?
            .split('\0')
            .map(String::from)
            .collect::<Vec<_>>();
        let scores = file
            .tensor("tok.scores")
            .ok_or_else(|| NnError::Missing("tok.scores".into()))?
            .to_f32();
        if scores.len() != vocab || pieces.len() != vocab {
            return Err(NnError::Shape(format!(
                "vocabulary {vocab}, but {} pieces and {} scores",
                pieces.len(),
                scores.len()
            )));
        }
        let fingerprint = bytes(file, "lex.fingerprint")?
            .try_into()
            .map_err(|_| NnError::Shape("lex.fingerprint: expected 16 bytes".into()))?;
        let tables = Tables {
            p_en: f32s(file, "tok.p_en", vocab)?,
            first_en: f32s(file, "lex.first_en", vocab)?,
            first_ml: f32s(file, "lex.first_ml", vocab)?,
            fingerprint,
        };
        let theta = f32s(file, "rope.theta", 1)?[0];
        let dh = c.d / c.heads;
        let inv_freq = (0..dh / 2)
            .map(|i| theta.powf(-2.0 * i as f32 / dh as f32))
            .collect();
        let l = Loader(file);
        let layers = (0..c.dec_layers)
            .map(|i| {
                let p = format!("layers.{i}");
                Ok(Layer {
                    norm1: l.norm(&format!("{p}.norm1"), c.d)?,
                    qkv: l.linear(&format!("{p}.qkv"), 3 * c.d, c.d)?,
                    out: l.linear(&format!("{p}.out"), c.d, c.d)?,
                    norm2: l.norm(&format!("{p}.norm2"), c.d)?,
                    ff1: l.linear(&format!("{p}.ff1"), c.ff, c.d)?,
                    ff2: l.linear(&format!("{p}.ff2"), c.d, c.ff)?,
                })
            })
            .collect::<Result<_, NnError>>()?;
        Ok(Self {
            emb: l.mat("emb.weight", vocab, c.d)?,
            layers,
            norm: l.norm("norm", c.d)?,
            inv_freq,
            tokenizer: Tokenizer::new(pieces, scores)?,
            tables,
            file: file.clone(),
            config: c,
        })
    }

    /// The tokeniser shipped with the model.
    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }

    /// Which script's pieces the tokeniser flags `NATIVE` (`Tokenizer::set_native`): the
    /// block of the pack the model belongs to. Malayalam's until set.
    pub fn set_native(&mut self, block: std::ops::RangeInclusive<char>) {
        self.tokenizer.set_native(block);
    }

    /// `set_native` with the block of `rules`, the rules of the pack whose LM this is; its
    /// text is normalised by them from now on (`Tokenizer::set_rules`).
    pub fn set_rules(&mut self, rules: &'static dyn crate::script::ScriptRules) {
        self.tokenizer.set_rules(rules);
    }

    /// The per-piece tables shipped with the model.
    pub fn tables(&self) -> &Tables {
        &self.tables
    }

    /// The model's shape.
    pub fn config(&self) -> Config {
        self.config
    }

    /// An empty context, positions from 0.
    pub fn cache(&self) -> Cache {
        Cache {
            kv: vec![Vec::new(); self.config.dec_layers],
            ..Cache::default()
        }
    }

    fn q<'a>(&'a self, m: &'a Mat) -> QMatrix<'a> {
        m.q(&self.file)
    }

    fn linear(&self, l: &Linear, x: &[f32], n: usize, out: &mut [f32], quant: &mut QuantBuf) {
        matmul_i8(self.q(&l.weight), Some(&l.bias), x, n, out, quant);
    }

    /// Runs `tokens` after the cache's rows as one batch (each weight read once) and appends
    /// their rows. An empty cache starts again at position 0.
    ///
    /// # Panics
    /// If a token is outside the vocabulary.
    pub fn feed(&self, cache: &mut Cache, tokens: &[u32], s: &mut LmScratch) {
        let c = &self.config;
        let (n, d, f) = (tokens.len(), c.d, c.ff);
        if n == 0 {
            return;
        }
        if cache.rows == 0 {
            cache.first_pos = 0;
        }
        s.ensure(c, n, cache.rows + n);
        let emb = self.q(&self.emb);
        for (i, &t) in tokens.iter().enumerate() {
            assert!((t as usize) < c.tgt_vocab, "token {t} out of range");
            emb.row_into(t as usize, &mut s.x[i * d..(i + 1) * d]);
        }
        let base = cache.next_pos();
        for (l, layer) in self.layers.iter().enumerate() {
            norm_rows(&layer.norm1, &s.x[..n * d], &mut s.h[..n * d], d);
            self.linear(
                &layer.qkv,
                &s.h[..n * d],
                n,
                &mut s.qkv[..n * 3 * d],
                &mut s.quant,
            );
            let kv = &mut cache.kv[l];
            for i in 0..n {
                let row = &mut s.qkv[i * 3 * d..(i + 1) * 3 * d];
                rope(&mut row[..d], base + i, &self.inv_freq);
                rope(&mut row[d..2 * d], base + i, &self.inv_freq);
                kv.extend_from_slice(&row[d..]);
            }
            for i in 0..n {
                // Causal: row i sees every cached row and the new rows up to itself.
                attend(
                    &s.qkv[i * 3 * d..i * 3 * d + d],
                    kv,
                    (0, d, 2 * d),
                    cache.rows + i + 1,
                    c.heads,
                    &mut s.scores,
                    &mut s.attn[i * d..(i + 1) * d],
                );
            }
            self.linear(
                &layer.out,
                &s.attn[..n * d],
                n,
                &mut s.proj[..n * d],
                &mut s.quant,
            );
            add_assign(&mut s.x[..n * d], &s.proj[..n * d]);
            norm_rows(&layer.norm2, &s.x[..n * d], &mut s.h[..n * d], d);
            self.linear(
                &layer.ff1,
                &s.h[..n * d],
                n,
                &mut s.ff[..n * f],
                &mut s.quant,
            );
            relu(&mut s.ff[..n * f]);
            self.linear(
                &layer.ff2,
                &s.ff[..n * f],
                n,
                &mut s.proj[..n * d],
                &mut s.quant,
            );
            add_assign(&mut s.x[..n * d], &s.proj[..n * d]);
        }
        cache.rows += n;
        let start = cache.hidden.len();
        cache.hidden.resize(start + n * d, 0.0);
        norm_rows(&self.norm, &s.x[..n * d], &mut cache.hidden[start..], d);
    }

    /// Drops the oldest `rows` rows (all of them, if fewer); the rest keep their positions,
    /// as rotary scores are relative.
    pub fn evict_front(&self, cache: &mut Cache, rows: usize) {
        let (rows, d) = (rows.min(cache.rows), self.config.d);
        for kv in &mut cache.kv {
            kv.drain(..rows * 2 * d);
        }
        cache.hidden.drain(..rows * d);
        cache.rows -= rows;
        cache.first_pos += rows;
    }

    /// Keeps the first `rows` rows.
    pub fn truncate(&self, cache: &mut Cache, rows: usize) {
        let (rows, d) = (rows.min(cache.rows), self.config.d);
        for kv in &mut cache.kv {
            kv.truncate(rows * 2 * d);
        }
        cache.hidden.truncate(rows * d);
        cache.rows = rows;
    }

    /// Log-softmax over the vocabulary after the newest row; empty for an empty cache.
    pub fn next_log_probs<'s>(&self, cache: &Cache, s: &'s mut LmScratch) -> &'s [f32] {
        let (d, vocab) = (self.config.d, self.config.tgt_vocab);
        if cache.rows == 0 {
            return &[];
        }
        s.logits.resize(vocab, 0.0);
        let last = &cache.hidden[(cache.rows - 1) * d..cache.rows * d];
        matmul_i8(
            self.q(&self.emb),
            None,
            last,
            1,
            &mut s.logits,
            &mut s.quant,
        );
        let max = s.logits.iter().fold(f32::NEG_INFINITY, |m, &v| m.max(v));
        let sum: f32 = s.logits.iter().map(|&v| (v - max).exp()).sum();
        let shift = max + sum.ln();
        for v in &mut s.logits {
            *v -= shift;
        }
        &s.logits
    }
}

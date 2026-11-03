//! The transliterator's forward pass: the int8 re-implementation of
//! `training/mlkb/translit/model.py` (`Translit`, in eval mode).
//!
//! - Encoder: `x = src_emb[t] + src_pos[i]` (unscaled), then pre-norm layers
//!   `x += SelfAttn(norm1(x)); x += W2·relu(W1·norm2(x) + b1) + b2`, then `enc_norm`.
//! - Decoder: `x = tgt_emb[t] + tgt_pos[i]`, then pre-norm layers with causal self-attention
//!   (`norm1`), cross-attention over the encoder output (`norm2`; the memory itself is not
//!   normed again) and the feed-forward block (`norm3`), then `dec_norm`. Logits are
//!   `dec_norm(x) · tgt_embᵀ`, tied and without bias.
//! - Attention is PyTorch's `nn.MultiheadAttention`: `in_proj_weight` packs the q, k and v
//!   projections as rows `[0, d)`, `[d, 2d)`, `[2d, 3d)`; scores are `q·k / sqrt(d / heads)`.
//!
//! One word at a time and no padding, so the padding masks of the PyTorch model never apply.
//! The decoder runs one token per call with a per-beam cache of self-attention keys and values;
//! the cross-attention keys and values are computed once per encoded word.

use std::ops::Range;

use super::file::{Config, KIND_TRANSLIT, ModelFile, NnError, TensorRef};
use super::ops::{QMatrix, QuantBuf, add_assign, dot, layer_norm, matmul_i8, relu, softmax};

/// An int8 matrix resolved from the file: its scales (copied) and where its values are.
#[derive(Debug)]
struct Mat {
    scales: Vec<f32>,
    values: Range<usize>,
    rows: usize,
    cols: usize,
}

#[derive(Debug)]
struct Linear {
    weight: Mat,
    bias: Vec<f32>,
}

#[derive(Debug)]
struct Norm {
    weight: Vec<f32>,
    bias: Vec<f32>,
}

#[derive(Debug)]
struct Attention {
    /// q, k, v projections packed as `[3d, d]`.
    in_proj: Linear,
    out_proj: Linear,
}

#[derive(Debug)]
struct FeedForward {
    linear1: Linear,
    linear2: Linear,
}

#[derive(Debug)]
struct EncoderLayer {
    self_attn: Attention,
    ff: FeedForward,
    norm1: Norm,
    norm2: Norm,
}

#[derive(Debug)]
struct DecoderLayer {
    self_attn: Attention,
    cross_attn: Attention,
    ff: FeedForward,
    norm1: Norm,
    norm2: Norm,
    norm3: Norm,
}

/// Resolves tensors by name, checking dtype and shape.
struct Loader<'a>(&'a ModelFile);

impl Loader<'_> {
    fn mat(&self, name: &str, rows: usize, cols: usize) -> Result<Mat, NnError> {
        match self.0.tensor(name) {
            Some(TensorRef::I8 {
                scales,
                rows: r,
                cols: c,
                ..
            }) if (r, c) == (rows, cols) => Ok(Mat {
                scales: scales.to_vec(),
                values: self.0.int8_range(name).expect("int8 tensor has a range"),
                rows,
                cols,
            }),
            Some(t) => Err(NnError::Shape(format!(
                "{name}: expected int8 [{rows}, {cols}], found {:?} {}",
                t.dims(),
                if matches!(t, TensorRef::F32 { .. }) {
                    "f32"
                } else {
                    "int8"
                }
            ))),
            None => Err(NnError::Missing(name.into())),
        }
    }

    fn vector(&self, name: &str, len: usize) -> Result<Vec<f32>, NnError> {
        match self.0.tensor(name) {
            Some(TensorRef::F32 { dims, values }) if dims == [len] => Ok(values.to_vec()),
            Some(t) => Err(NnError::Shape(format!(
                "{name}: expected f32 [{len}], found {:?}",
                t.dims()
            ))),
            None => Err(NnError::Missing(name.into())),
        }
    }

    fn linear(&self, prefix: &str, rows: usize, cols: usize) -> Result<Linear, NnError> {
        Ok(Linear {
            weight: self.mat(&format!("{prefix}.weight"), rows, cols)?,
            bias: self.vector(&format!("{prefix}.bias"), rows)?,
        })
    }

    fn norm(&self, prefix: &str, d: usize) -> Result<Norm, NnError> {
        Ok(Norm {
            weight: self.vector(&format!("{prefix}.weight"), d)?,
            bias: self.vector(&format!("{prefix}.bias"), d)?,
        })
    }

    fn attention(&self, prefix: &str, d: usize) -> Result<Attention, NnError> {
        Ok(Attention {
            in_proj: Linear {
                weight: self.mat(&format!("{prefix}.in_proj_weight"), 3 * d, d)?,
                bias: self.vector(&format!("{prefix}.in_proj_bias"), 3 * d)?,
            },
            out_proj: self.linear(&format!("{prefix}.out_proj"), d, d)?,
        })
    }

    fn feed_forward(&self, prefix: &str, c: &Config) -> Result<FeedForward, NnError> {
        Ok(FeedForward {
            linear1: self.linear(&format!("{prefix}.linear1"), c.ff, c.d)?,
            linear2: self.linear(&format!("{prefix}.linear2"), c.d, c.ff)?,
        })
    }
}

/// The transliterator, ready to run. It shares the file's mapping: the int8 values are read
/// in place, and only the f32 tensors and the scales are copied.
#[derive(Debug)]
pub struct Transformer {
    file: ModelFile,
    config: Config,
    src_emb: Mat,
    src_pos: Mat,
    tgt_emb: Mat,
    tgt_pos: Mat,
    enc: Vec<EncoderLayer>,
    enc_norm: Norm,
    dec: Vec<DecoderLayer>,
    dec_norm: Norm,
}

/// One encoded source word: the encoder output, and each decoder layer's cross-attention
/// keys and values over it.
#[derive(Debug, Clone)]
pub struct Encoded {
    len: usize,
    output: Vec<f32>,
    /// Per decoder layer, `len` rows of `[k (d), v (d)]`.
    cross_kv: Vec<Vec<f32>>,
}

impl Encoded {
    /// Source length in tokens (bos and eos included).
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the source is empty (never, for an `Encoded` from `Transformer::encode`).
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The encoder output, `len × d` row-major (after the final layer norm).
    pub fn output(&self) -> &[f32] {
        &self.output
    }
}

/// One beam's decoder history: the self-attention keys and values of every token it has
/// consumed, per layer. `clone` forks the beam (a copy of the cache, a few tens of KB at most).
#[derive(Debug)]
pub struct DecoderState {
    len: usize,
    /// Per decoder layer, `len` rows of `[k (d), v (d)]`, with room for `max_len` rows.
    kv: Vec<Vec<f32>>,
}

impl Clone for DecoderState {
    fn clone(&self) -> Self {
        Self {
            len: self.len,
            kv: self
                .kv
                .iter()
                .map(|layer| {
                    let mut copy = Vec::with_capacity(layer.capacity());
                    copy.extend_from_slice(layer);
                    copy
                })
                .collect(),
        }
    }
}

impl DecoderState {
    /// Tokens consumed so far.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether no token has been consumed yet.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// Runs decoder steps for one encoded word. It owns the scratch buffers, so steps don't
/// allocate once the buffers have grown to the largest batch.
#[derive(Debug)]
pub struct Decoder<'a> {
    model: &'a Transformer,
    encoded: &'a Encoded,
    scratch: Scratch,
}

/// Work buffers, sized for `n` rows at a time.
#[derive(Debug, Default)]
struct Scratch {
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

impl Scratch {
    fn new(c: &Config, n: usize) -> Self {
        let mut s = Self {
            scores: vec![0.0; c.max_len],
            ..Self::default()
        };
        s.ensure(c, n);
        s
    }

    fn ensure(&mut self, c: &Config, n: usize) {
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
        self.logits.resize(n * c.tgt_vocab, 0.0);
    }
}

/// Multi-head attention of one query `q` (`d`) over `t` rows of `kv`, where row `j`'s key is
/// `kv[j*stride + k_off ..][..d]` and its value `kv[j*stride + v_off ..][..d]`. Writes `d`
/// values to `out`. `scores` needs `t` entries.
#[allow(clippy::too_many_arguments)]
fn attend(
    q: &[f32],
    kv: &[f32],
    (k_off, v_off, stride): (usize, usize, usize),
    t: usize,
    heads: usize,
    scores: &mut [f32],
    out: &mut [f32],
) {
    let d = q.len();
    let dh = d / heads;
    let scale = 1.0 / (dh as f32).sqrt();
    let scores = &mut scores[..t];
    out.fill(0.0);
    for h in 0..heads {
        let cols = h * dh..(h + 1) * dh;
        let qh = &q[cols.clone()];
        for (j, s) in scores.iter_mut().enumerate() {
            let k = &kv[j * stride + k_off + cols.start..][..dh];
            *s = dot(qh, k) * scale;
        }
        softmax(scores);
        let oh = &mut out[cols.clone()];
        for (j, &p) in scores.iter().enumerate() {
            let v = &kv[j * stride + v_off + cols.start..][..dh];
            for (o, &vv) in oh.iter_mut().zip(v) {
                *o += p * vv;
            }
        }
    }
}

impl Transformer {
    /// Resolves every tensor of `file` once, checking each shape against the header.
    pub fn new(file: &ModelFile) -> Result<Self, NnError> {
        let c = file.config();
        if c.kind != KIND_TRANSLIT {
            return Err(NnError::Malformed(format!(
                "model kind {} is not a transliterator",
                c.kind
            )));
        }
        if c.heads == 0 || c.d == 0 || !c.d.is_multiple_of(c.heads) || c.max_len == 0 {
            return Err(NnError::Shape(format!(
                "d = {} must be a positive multiple of heads = {}, max_len = {} positive",
                c.d, c.heads, c.max_len
            )));
        }
        let l = Loader(file);
        let enc = (0..c.enc_layers)
            .map(|i| {
                let p = format!("enc.{i}");
                Ok(EncoderLayer {
                    self_attn: l.attention(&format!("{p}.self_attn"), c.d)?,
                    ff: l.feed_forward(&p, &c)?,
                    norm1: l.norm(&format!("{p}.norm1"), c.d)?,
                    norm2: l.norm(&format!("{p}.norm2"), c.d)?,
                })
            })
            .collect::<Result<_, NnError>>()?;
        let dec = (0..c.dec_layers)
            .map(|i| {
                let p = format!("dec.{i}");
                Ok(DecoderLayer {
                    self_attn: l.attention(&format!("{p}.self_attn"), c.d)?,
                    cross_attn: l.attention(&format!("{p}.multihead_attn"), c.d)?,
                    ff: l.feed_forward(&p, &c)?,
                    norm1: l.norm(&format!("{p}.norm1"), c.d)?,
                    norm2: l.norm(&format!("{p}.norm2"), c.d)?,
                    norm3: l.norm(&format!("{p}.norm3"), c.d)?,
                })
            })
            .collect::<Result<_, NnError>>()?;
        Ok(Self {
            src_emb: l.mat("src_emb.weight", c.src_vocab, c.d)?,
            src_pos: l.mat("src_pos.weight", c.max_len, c.d)?,
            tgt_emb: l.mat("tgt_emb.weight", c.tgt_vocab, c.d)?,
            tgt_pos: l.mat("tgt_pos.weight", c.max_len, c.d)?,
            enc,
            enc_norm: l.norm("enc_norm", c.d)?,
            dec,
            dec_norm: l.norm("dec_norm", c.d)?,
            file: file.clone(),
            config: c,
        })
    }

    /// The model's shape.
    pub fn config(&self) -> Config {
        self.config
    }

    fn q<'a>(&'a self, m: &'a Mat) -> QMatrix<'a> {
        QMatrix {
            data: &self.file.bytes()[m.values.clone()],
            scales: &m.scales,
            rows: m.rows,
            cols: m.cols,
        }
    }

    /// `out = linear(x)` for `n` rows.
    fn linear(&self, l: &Linear, x: &[f32], n: usize, out: &mut [f32], quant: &mut QuantBuf) {
        matmul_i8(self.q(&l.weight), Some(&l.bias), x, n, out, quant);
    }

    /// `x += W2·relu(W1·norm(x) + b1) + b2` for `n` rows of `x`.
    fn feed_forward(&self, ff: &FeedForward, norm: &Norm, n: usize, s: &mut Scratch) {
        let (d, f) = (self.config.d, self.config.ff);
        norm_rows(norm, &s.x[..n * d], &mut s.h[..n * d], d);
        self.linear(
            &ff.linear1,
            &s.h[..n * d],
            n,
            &mut s.ff[..n * f],
            &mut s.quant,
        );
        relu(&mut s.ff[..n * f]);
        self.linear(
            &ff.linear2,
            &s.ff[..n * f],
            n,
            &mut s.proj[..n * d],
            &mut s.quant,
        );
        add_assign(&mut s.x[..n * d], &s.proj[..n * d]);
    }

    /// Runs the encoder over `src` (token ids with bos and eos) and precomputes the decoder's
    /// cross-attention keys and values.
    ///
    /// # Panics
    /// If `src` is empty, longer than `max_len`, or holds a token outside the source vocabulary.
    pub fn encode(&self, src: &[u32]) -> Encoded {
        let c = &self.config;
        let (n, d) = (src.len(), c.d);
        assert!(
            (1..=c.max_len).contains(&n),
            "source length {n} not in 1..={}",
            c.max_len
        );
        let mut s = Scratch::new(c, n);
        let (emb, pos) = (self.q(&self.src_emb), self.q(&self.src_pos));
        for (i, &t) in src.iter().enumerate() {
            assert!((t as usize) < c.src_vocab, "source token {t} out of range");
            let x = &mut s.x[i * d..(i + 1) * d];
            emb.row_into(t as usize, x);
            pos.add_row(i, x);
        }
        for layer in &self.enc {
            let a = &layer.self_attn;
            norm_rows(&layer.norm1, &s.x[..n * d], &mut s.h[..n * d], d);
            self.linear(
                &a.in_proj,
                &s.h[..n * d],
                n,
                &mut s.qkv[..n * 3 * d],
                &mut s.quant,
            );
            for i in 0..n {
                attend(
                    &s.qkv[i * 3 * d..i * 3 * d + d],
                    &s.qkv[..n * 3 * d],
                    (d, 2 * d, 3 * d),
                    n,
                    c.heads,
                    &mut s.scores,
                    &mut s.attn[i * d..(i + 1) * d],
                );
            }
            self.linear(
                &a.out_proj,
                &s.attn[..n * d],
                n,
                &mut s.proj[..n * d],
                &mut s.quant,
            );
            add_assign(&mut s.x[..n * d], &s.proj[..n * d]);
            self.feed_forward(&layer.ff, &layer.norm2, n, &mut s);
        }
        let mut output = vec![0.0; n * d];
        norm_rows(&self.enc_norm, &s.x[..n * d], &mut output, d);
        let cross_kv = self
            .dec
            .iter()
            .map(|layer| {
                let kv_rows = self.q(&layer.cross_attn.in_proj.weight).rows(d..3 * d);
                let mut kv = vec![0.0; n * 2 * d];
                let bias = &layer.cross_attn.in_proj.bias[d..3 * d];
                matmul_i8(kv_rows, Some(bias), &output, n, &mut kv, &mut s.quant);
                kv
            })
            .collect();
        Encoded {
            len: n,
            output,
            cross_kv,
        }
    }

    /// A decoder for `encoded`; start beams with `Decoder::start`.
    pub fn decoder<'a>(&'a self, encoded: &'a Encoded) -> Decoder<'a> {
        Decoder {
            model: self,
            encoded,
            scratch: Scratch::new(&self.config, 1),
        }
    }
}

/// `nn.LayerNorm` on each `d`-long row of `x`.
fn norm_rows(norm: &Norm, x: &[f32], out: &mut [f32], d: usize) {
    for (xr, or) in x.chunks_exact(d).zip(out.chunks_exact_mut(d)) {
        layer_norm(xr, &norm.weight, &norm.bias, or);
    }
}

impl Decoder<'_> {
    /// A beam that has consumed nothing; feed it bos first.
    pub fn start(&self) -> DecoderState {
        let c = &self.model.config;
        DecoderState {
            len: 0,
            kv: (0..c.dec_layers)
                .map(|_| Vec::with_capacity(c.max_len * 2 * c.d))
                .collect(),
        }
    }

    /// Feeds `token` to one beam and returns the next-token logits (`tgt_vocab` long).
    ///
    /// # Panics
    /// As `step_batch`.
    pub fn step(&mut self, state: &mut DecoderState, token: u32) -> &[f32] {
        self.step_batch(std::slice::from_mut(state), &[token])
    }

    /// Feeds `tokens[i]` to `states[i]` for every beam at once (beams may be at different
    /// lengths) and returns their next-token logits, `states.len() × tgt_vocab` row-major.
    /// Batching shares the weight reads across beams.
    ///
    /// # Panics
    /// If the lengths differ, a beam already holds `max_len` tokens, or a token is outside the
    /// target vocabulary.
    pub fn step_batch(&mut self, states: &mut [DecoderState], tokens: &[u32]) -> &[f32] {
        let m = self.model;
        let c = &m.config;
        let (n, d) = (states.len(), c.d);
        assert_eq!(n, tokens.len(), "one token per beam");
        self.scratch.ensure(c, n);
        let s = &mut self.scratch;
        let (emb, pos) = (m.q(&m.tgt_emb), m.q(&m.tgt_pos));
        for (i, (state, &t)) in states.iter().zip(tokens).enumerate() {
            assert!(state.len < c.max_len, "beam is full ({} tokens)", c.max_len);
            assert!((t as usize) < c.tgt_vocab, "target token {t} out of range");
            let x = &mut s.x[i * d..(i + 1) * d];
            emb.row_into(t as usize, x);
            pos.add_row(state.len, x);
        }
        for (l, layer) in m.dec.iter().enumerate() {
            // Causal self-attention over the beam's own history, this token included.
            let a = &layer.self_attn;
            norm_rows(&layer.norm1, &s.x[..n * d], &mut s.h[..n * d], d);
            m.linear(
                &a.in_proj,
                &s.h[..n * d],
                n,
                &mut s.qkv[..n * 3 * d],
                &mut s.quant,
            );
            for (i, state) in states.iter_mut().enumerate() {
                let qkv = &s.qkv[i * 3 * d..(i + 1) * 3 * d];
                let cache = &mut state.kv[l];
                cache.extend_from_slice(&qkv[d..]);
                attend(
                    &qkv[..d],
                    cache,
                    (0, d, 2 * d),
                    state.len + 1,
                    c.heads,
                    &mut s.scores,
                    &mut s.attn[i * d..(i + 1) * d],
                );
            }
            m.linear(
                &a.out_proj,
                &s.attn[..n * d],
                n,
                &mut s.proj[..n * d],
                &mut s.quant,
            );
            add_assign(&mut s.x[..n * d], &s.proj[..n * d]);

            // Cross-attention over the encoder output.
            let a = &layer.cross_attn;
            norm_rows(&layer.norm2, &s.x[..n * d], &mut s.h[..n * d], d);
            let q_rows = m.q(&a.in_proj.weight).rows(0..d);
            let q = &mut s.qkv[..n * d];
            matmul_i8(
                q_rows,
                Some(&a.in_proj.bias[..d]),
                &s.h[..n * d],
                n,
                q,
                &mut s.quant,
            );
            let kv = &self.encoded.cross_kv[l];
            for i in 0..n {
                attend(
                    &s.qkv[i * d..(i + 1) * d],
                    kv,
                    (0, d, 2 * d),
                    self.encoded.len,
                    c.heads,
                    &mut s.scores,
                    &mut s.attn[i * d..(i + 1) * d],
                );
            }
            m.linear(
                &a.out_proj,
                &s.attn[..n * d],
                n,
                &mut s.proj[..n * d],
                &mut s.quant,
            );
            add_assign(&mut s.x[..n * d], &s.proj[..n * d]);

            m.feed_forward(&layer.ff, &layer.norm3, n, s);
        }
        for state in states.iter_mut() {
            state.len += 1;
        }
        norm_rows(&m.dec_norm, &s.x[..n * d], &mut s.h[..n * d], d);
        let logits = &mut s.logits[..n * c.tgt_vocab];
        matmul_i8(
            m.q(&m.tgt_emb),
            None,
            &s.h[..n * d],
            n,
            logits,
            &mut s.quant,
        );
        logits
    }
}

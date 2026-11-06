//! Python access to the production engine, for the evaluation harness only.
//! Never shipped: what the harness measures is exactly what the keyboard runs.

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;

use mlengine_core::lexicon::Lexicon;
use mlengine_core::lm_context::LmWeights;
use mlengine_core::nn::tokenizer::{BOS, flag};
use mlengine_core::nn::{Cache, LmScratch};
use mlengine_core::{Engine as Core, Kind, Layout, PackPaths, Paths, rank};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// The engine over the given lexicon files, transliterator model (`translit`, a `.mlkbnn`)
/// and language model (`lm`); a missing or unreadable file is left out, and an LM built
/// against other lexicons is not loaded, exactly as on the device.
#[pyclass]
struct Engine(Core);

#[pymethods]
impl Engine {
    #[new]
    #[pyo3(signature = (english=None, malayalam=None, manglish=None, next_words=None, translit=None, lm=None, romanisation=None, suffixes=None, chat_forms=None, layout=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        english: Option<PathBuf>,
        malayalam: Option<PathBuf>,
        manglish: Option<PathBuf>,
        next_words: Option<PathBuf>,
        translit: Option<PathBuf>,
        lm: Option<PathBuf>,
        romanisation: Option<PathBuf>,
        suffixes: Option<PathBuf>,
        chat_forms: Option<PathBuf>,
        layout: Option<PathBuf>,
    ) -> Self {
        let ml = PackPaths {
            lexicon: malayalam,
            romanised: manglish,
            romanisation,
            suffixes,
            chat_forms,
            layout,
            next_words,
            translit,
            lm,
            ..PackPaths::new("ml", "mlym")
        };
        Engine(Core::open(&Paths {
            english,
            packs: vec![ml],
            user_dir: None,
        }))
    }

    /// Suggestions for the text before the cursor, in bar order (the middle
    /// slot first). Each item is (text, kind, score); `layout` is "latin", or "malayalam"
    /// for the Malayalam pack's native layout (Latin when that pack isn't loaded).
    fn suggest(
        &self,
        context: &str,
        layout: &str,
        autocorrect: bool,
    ) -> PyResult<Vec<(String, &'static str, f32)>> {
        let layout = match layout {
            "latin" => Layout::Latin,
            "malayalam" => self
                .0
                .lang_of_id("ml")
                .map_or(Layout::Latin, Layout::Native),
            other => return Err(PyValueError::new_err(format!("unknown layout {other:?}"))),
        };
        Ok(self
            .0
            .suggest(context, layout, autocorrect)
            .candidates
            .into_iter()
            .map(|c| (c.text, kind_name(c.kind), c.score))
            .collect())
    }

    /// Names of the sources that loaded, e.g. ["english", "ml:lexicon", "ml:lm"].
    fn loaded_sources(&self) -> Vec<String> {
        self.0.loaded_sources()
    }

    /// Sets the LM weights for sweeps (`mlengine_core::lm_context::LmWeights`); a field left
    /// out keeps `rank`'s value.
    #[pyo3(signature = (en=None, ml=None, clip=None, lang_weight=None, lang_offset=None, next_bigram=None))]
    fn set_lm_weights(
        &mut self,
        en: Option<f32>,
        ml: Option<f32>,
        clip: Option<f32>,
        lang_weight: Option<f32>,
        lang_offset: Option<f32>,
        next_bigram: Option<f32>,
    ) {
        let d = LmWeights::default();
        self.0.set_lm_weights(LmWeights {
            en: en.unwrap_or(d.en),
            ml: ml.unwrap_or(d.ml),
            clip: clip.unwrap_or(d.clip),
            lang_weight: lang_weight.unwrap_or(d.lang_weight),
            lang_offset: lang_offset.unwrap_or(d.lang_offset),
            next_bigram: next_bigram.unwrap_or(d.next_bigram),
        });
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Verbatim => "verbatim",
        Kind::Completion => "completion",
        Kind::Correction => "correction",
        Kind::Romanised => "manglish",
        Kind::NextWord => "next_word",
    }
}

/// The `limit` most frequent words of a lexicon file with their counts, most
/// frequent first (ties in key order). Keys that are not UTF-8 are skipped.
#[pyfunction]
fn malayalam_words(path: PathBuf, limit: usize) -> PyResult<Vec<(String, u64)>> {
    let lex = Lexicon::open(&path).map_err(|e| PyValueError::new_err(e.to_string()))?;
    let mut words = Vec::with_capacity(lex.len());
    lex.words(|key, count| {
        if let Ok(word) = std::str::from_utf8(key) {
            words.push((word.to_owned(), count));
        }
    });
    words.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    words.truncate(limit);
    Ok(words)
}

/// The LM's tokeniser over SentencePiece pieces and scores (in id order), exactly as the
/// engine runs it. Training text is tokenised through this, so the model sees the ids the
/// phone produces.
#[pyclass(frozen)]
struct Tokenizer(mlengine_core::nn::Tokenizer);

#[pymethods]
impl Tokenizer {
    #[new]
    fn new(pieces: Vec<String>, scores: Vec<f32>) -> PyResult<Self> {
        mlengine_core::nn::Tokenizer::new(pieces, scores)
            .map(Tokenizer)
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Token ids of `text` (after `lm_normalize`).
    fn encode(&self, text: &str) -> Vec<u32> {
        self.0.encode(text)
    }

    /// The text of piece `id`.
    fn piece(&self, id: u32) -> String {
        self.0.piece(id).to_owned()
    }

    /// Writes, for each non-empty line of `src`, `BOS` then the line's tokens to `dst` as
    /// little-endian `u16`, and returns the number of ids written. Runs without the GIL.
    fn encode_file(&self, py: Python<'_>, src: PathBuf, dst: PathBuf) -> PyResult<u64> {
        if self.0.len() > 65_536 {
            return Err(PyValueError::new_err("vocabulary too large for u16 ids"));
        }
        py.detach(|| -> std::io::Result<u64> {
            let reader = BufReader::with_capacity(1 << 20, File::open(&src)?);
            let mut out = BufWriter::with_capacity(1 << 20, File::create(&dst)?);
            let mut count = 0u64;
            for line in reader.lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                let ids = self.0.encode(&line);
                for id in std::iter::once(BOS).chain(ids) {
                    out.write_all(&(id as u16).to_le_bytes())?;
                    count += 1;
                }
            }
            out.flush()?;
            Ok(count)
        })
        .map_err(|e| PyValueError::new_err(format!("{}: {e}", src.display())))
    }
}

/// The LM through the production forward pass and cache, for the memorisation check (design
/// §10).
#[pyclass(frozen)]
struct Lm(mlengine_core::nn::Lm);

#[pymethods]
impl Lm {
    #[new]
    fn new(path: PathBuf) -> PyResult<Self> {
        mlengine_core::nn::Lm::open(&path)
            .map(Lm)
            .map_err(|e| PyValueError::new_err(format!("{}: {e}", path.display())))
    }

    /// Feeds `tokens`, then generates `n` tokens: the likeliest at `temperature` 0, else
    /// sampled from the softmax at that temperature (seeded by `seed`), each fed in turn. The
    /// cache slides as on the phone (`LmContext`): while it holds more than `rank::LM_WINDOW`
    /// tokens, its oldest whole word goes (a word starts at a word-start piece; a token that
    /// isn't one belongs to the word before it). Runs without the GIL.
    fn generate(
        &self,
        py: Python<'_>,
        tokens: Vec<u32>,
        n: usize,
        temperature: f32,
        seed: u64,
    ) -> PyResult<Vec<u32>> {
        let vocab = self.0.config().tgt_vocab;
        if let Some(t) = tokens.iter().find(|&&t| t as usize >= vocab) {
            return Err(PyValueError::new_err(format!("token {t} out of range")));
        }
        Ok(py.detach(|| {
            let (lm, tok) = (&self.0, self.0.tokenizer());
            let mut s = LmScratch::default();
            let mut cache = lm.cache();
            // Token counts of the cached words, oldest first.
            let mut words: Vec<usize> = Vec::new();
            let mut rng = seed;
            let mut out = Vec::with_capacity(n);
            let mut feed = |cache: &mut Cache, words: &mut Vec<usize>, t: u32| {
                lm.feed(cache, &[t], &mut s);
                match words.last_mut() {
                    Some(last) if tok.flags(t) & flag::WORD_START == 0 && t != BOS => *last += 1,
                    _ => words.push(1),
                }
                while cache.rows() > rank::LM_WINDOW && words.len() > 1 {
                    lm.evict_front(cache, words.remove(0));
                }
                lm.next_log_probs(cache, &mut s).to_vec()
            };
            let mut next = Vec::new();
            for &t in &tokens {
                next = feed(&mut cache, &mut words, t);
            }
            for _ in 0..n {
                if next.is_empty() {
                    break;
                }
                let t = if temperature <= 0.0 {
                    argmax(&next)
                } else {
                    sample(&next, temperature, &mut rng)
                };
                out.push(t);
                next = feed(&mut cache, &mut words, t);
            }
            out
        }))
    }
}

fn argmax(log_probs: &[f32]) -> u32 {
    let best = log_probs
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map_or(0, |(i, _)| i);
    best as u32
}

/// A token drawn from `softmax(log_probs / temperature)` with a SplitMix64 step of `state`.
fn sample(log_probs: &[f32], temperature: f32, state: &mut u64) -> u32 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let u = (z >> 11) as f64 / (1u64 << 53) as f64;
    let max = log_probs.iter().fold(f32::NEG_INFINITY, |m, &v| m.max(v));
    let weights: Vec<f64> = log_probs
        .iter()
        .map(|&v| f64::from((v - max) / temperature).exp())
        .collect();
    let mut left = u * weights.iter().sum::<f64>();
    for (i, w) in weights.iter().enumerate() {
        left -= w;
        if left <= 0.0 {
            return i as u32;
        }
    }
    (weights.len() - 1) as u32
}

/// The text form the LM is trained and run on (see `mlengine_core::nn::lm_normalize`).
#[pyfunction]
fn lm_normalize(text: &str) -> String {
    mlengine_core::nn::lm_normalize(text)
}

#[pymodule]
fn mlengine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Engine>()?;
    m.add_class::<Tokenizer>()?;
    m.add_class::<Lm>()?;
    m.add_function(wrap_pyfunction!(lm_normalize, m)?)?;
    m.add_function(wrap_pyfunction!(malayalam_words, m)?)
}

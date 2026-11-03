//! The only door between platform code and the engine. Every exported call
//! goes through `guarded`, so a Rust panic becomes a typed `EngineError`
//! instead of killing the keyboard. UniFFI's own `catch_unwind` is the
//! backstop; `guarded` is what gives callers a typed error. Swift callers must
//! use `try` with do/catch, never `try!`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum EngineError {
    #[error("internal engine failure: {message}")]
    Internal { message: String },
}

/// Which keyboard the text came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum InputLayout {
    Latin,
    Malayalam,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CandidateKind {
    Verbatim,
    Completion,
    Correction,
    Manglish,
    NextWord,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Candidate {
    pub text: String,
    pub kind: CandidateKind,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Suggestions {
    /// The word being typed ("" after a space).
    pub typed: String,
    /// `candidates[0]` is the middle slot; when it isn't the typed word, `candidates[1]` is.
    pub candidates: Vec<Candidate>,
    /// Vowel signs, most likely first; empty unless the text ends in a Malayalam consonant.
    pub vowel_signs: Vec<String>,
}

/// File paths of the bundled lexicons and the directory for learned words.
/// Any of them may be nil: that source is simply switched off.
#[derive(Debug, Clone, Default, PartialEq, uniffi::Record)]
pub struct EngineConfig {
    #[uniffi(default = None)]
    pub english: Option<String>,
    #[uniffi(default = None)]
    pub malayalam: Option<String>,
    #[uniffi(default = None)]
    pub manglish: Option<String>,
    #[uniffi(default = None)]
    pub next_words: Option<String>,
    /// The transliterator model (`.mlkbnn`); nil keeps Manglish on the lexicons alone.
    #[uniffi(default = None)]
    pub translit: Option<String>,
    #[uniffi(default = None)]
    pub user_dir: Option<String>,
}

impl From<&EngineConfig> for mlengine_core::Paths {
    fn from(c: &EngineConfig) -> Self {
        let path = |s: &Option<String>| s.as_ref().map(std::path::PathBuf::from);
        Self {
            english: path(&c.english),
            malayalam: path(&c.malayalam),
            manglish: path(&c.manglish),
            next_words: path(&c.next_words),
            translit: path(&c.translit),
            user_dir: path(&c.user_dir),
        }
    }
}

impl From<mlengine_core::Kind> for CandidateKind {
    fn from(kind: mlengine_core::Kind) -> Self {
        use mlengine_core::Kind;
        match kind {
            Kind::Verbatim => Self::Verbatim,
            Kind::Completion => Self::Completion,
            Kind::Correction => Self::Correction,
            Kind::Manglish => Self::Manglish,
            Kind::NextWord => Self::NextWord,
        }
    }
}

#[derive(uniffi::Object)]
pub struct Engine {
    inner: mlengine_core::Engine,
}

#[uniffi::export]
impl Engine {
    /// Maps the lexicon files and loads learned words. Never fails because a
    /// file is missing or corrupt: that source is just disabled.
    #[uniffi::constructor]
    pub fn new(config: EngineConfig) -> Result<Arc<Self>, EngineError> {
        guarded(|| Arc::new(Self { inner: mlengine_core::Engine::open(&(&config).into()) }))
    }

    pub fn suggest(
        &self,
        context_before: String,
        layout: InputLayout,
        autocorrect: bool,
    ) -> Result<Suggestions, EngineError> {
        guarded(|| {
            let layout = match layout {
                InputLayout::Latin => mlengine_core::Layout::Latin,
                InputLayout::Malayalam => mlengine_core::Layout::Malayalam,
            };
            let s = self.inner.suggest(&context_before, layout, autocorrect);
            Suggestions {
                typed: s.typed,
                candidates: s
                    .candidates
                    .into_iter()
                    .map(|c| Candidate { text: c.text, kind: c.kind.into() })
                    .collect(),
                vowel_signs: s.vowel_signs.into_iter().map(String::from).collect(),
            }
        })
    }

    /// The user undid an autocorrect or tapped what they typed: never replace this word.
    pub fn learn(&self, word: String) -> Result<(), EngineError> {
        guarded(|| self.inner.learn(&word))
    }

    /// A word was committed as typed; an unknown word is learned the second time.
    pub fn note_committed(&self, word: String) -> Result<(), EngineError> {
        guarded(|| self.inner.note_committed(&word))
    }

    pub fn forget_learned(&self) -> Result<(), EngineError> {
        guarded(|| self.inner.forget_learned())
    }

    /// Which lexicons loaded, for diagnostics ("english", "malayalam", ...).
    pub fn loaded_sources(&self) -> Result<Vec<String>, EngineError> {
        guarded(|| self.inner.loaded_sources().into_iter().map(String::from).collect())
    }
}

#[uniffi::export]
pub fn engine_version() -> Result<String, EngineError> {
    guarded(|| env!("CARGO_PKG_VERSION").to_string())
}

/// The transliterator's int8 product kernel on this CPU, "sdot" or "scalar": for diagnostics.
#[uniffi::export]
pub fn nn_kernel() -> String {
    mlengine_core::nn::ops::kernel().to_string()
}

/// The consonant a vowel sign would attach to, if the text ends in one.
#[uniffi::export]
pub fn malayalam_vowel_sign_base(context_before: String) -> Result<Option<String>, EngineError> {
    guarded(|| mlengine_core::malayalam::vowel_sign_base(&context_before).map(String::from))
}

/// Timings from `NnBenchmark::run`, for the runtime spike's debug screen.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct NnBench {
    /// Mapping the file and resolving every tensor.
    pub load_ms: f64,
    /// Per word: one encode plus 10 decoder steps over 4 beams.
    pub p50_us: u64,
    pub p99_us: u64,
    /// Per word: the encode alone.
    pub encode_p50_us: u64,
}

/// A loaded `.mlkbnn` transliterator, timed the way a beam search would use it. Loading and
/// running are separate calls so the app can read its memory footprint in between.
#[derive(uniffi::Object)]
pub struct NnBenchmark {
    model: mlengine_core::nn::Transformer,
    load_ms: f64,
}

#[uniffi::export]
impl NnBenchmark {
    /// Maps `path` and resolves the model's tensors.
    #[uniffi::constructor]
    pub fn new(path: String) -> Result<Arc<Self>, EngineError> {
        guarded(|| {
            let start = std::time::Instant::now();
            let file = mlengine_core::nn::ModelFile::open(std::path::Path::new(&path))?;
            let model = mlengine_core::nn::Transformer::new(&file)?;
            Ok(Arc::new(Self {
                model,
                load_ms: start.elapsed().as_secs_f64() * 1e3,
            }))
        })?
    }

    /// Milliseconds the constructor took.
    pub fn load_ms(&self) -> f64 {
        self.load_ms
    }

    /// For each of `words` synthetic Latin words (4–12 letters): one encode, then 10 batched
    /// decoder steps over 4 beams, forking the beams by clone after every step.
    pub fn run(&self, words: u32) -> Result<NnBench, EngineError> {
        guarded(|| {
            let (mut total, mut encode) = bench_words(&self.model, words as usize);
            total.sort_unstable();
            encode.sort_unstable();
            NnBench {
                load_ms: self.load_ms,
                p50_us: percentile_us(&total, 0.5),
                p99_us: percentile_us(&total, 0.99),
                encode_p50_us: percentile_us(&encode, 0.5),
            }
        })
    }
}

/// Loads `path` and runs `NnBenchmark::run(words)` on it.
#[uniffi::export]
pub fn nn_benchmark(path: String, words: u32) -> Result<NnBench, EngineError> {
    NnBenchmark::new(path)?.run(words)
}

impl From<mlengine_core::nn::NnError> for EngineError {
    fn from(e: mlengine_core::nn::NnError) -> Self {
        Self::Internal {
            message: e.to_string(),
        }
    }
}

const BENCH_BEAMS: usize = 4;
const BENCH_STEPS: usize = 10;

/// (encode + steps, encode alone) per word.
fn bench_words(
    model: &mlengine_core::nn::Transformer,
    words: usize,
) -> (Vec<std::time::Duration>, Vec<std::time::Duration>) {
    use mlengine_core::nn::DecoderState;
    use std::time::Instant;
    let vocab = model.config().tgt_vocab;
    let argmax = |l: &[f32]| {
        (3..vocab)
            .max_by(|&a, &b| l[a].total_cmp(&l[b]))
            .unwrap_or(3) as u32
    };
    // A 64-bit LCG: Latin tokens are bos = 1, eos = 2, a..z = 3..28.
    let mut seed: u64 = 7;
    let mut next = move || {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        seed >> 33
    };
    let (mut total, mut encode) = (Vec::with_capacity(words), Vec::with_capacity(words));
    for _ in 0..words {
        let len = 4 + (next() % 9) as usize;
        let mut src = vec![1];
        src.extend((0..len).map(|_| 3 + (next() % 26) as u32));
        src.push(2);

        let start = Instant::now();
        let enc = model.encode(&src);
        encode.push(start.elapsed());
        let mut dec = model.decoder(&enc);
        let mut beams: Vec<DecoderState> = vec![dec.start(); BENCH_BEAMS];
        let mut tokens = vec![1; BENCH_BEAMS];
        for _ in 0..BENCH_STEPS {
            let logits = dec.step_batch(&mut beams, &tokens);
            for (b, token) in tokens.iter_mut().enumerate() {
                *token = argmax(&logits[b * vocab..(b + 1) * vocab]);
            }
            beams = (0..BENCH_BEAMS)
                .map(|b| beams[(b + 1) % BENCH_BEAMS].clone())
                .collect();
        }
        total.push(start.elapsed());
    }
    (total, encode)
}

fn percentile_us(sorted: &[std::time::Duration], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize].as_micros() as u64
}

fn guarded<T>(f: impl FnOnce() -> T) -> Result<T, EngineError> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_string());
        EngineError::Internal { message }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_passes_values_through() {
        assert_eq!(guarded(|| 7).unwrap(), 7);
    }

    #[test]
    fn guarded_turns_panics_into_errors() {
        let err = guarded(|| -> u8 { panic!("boom") }).unwrap_err();
        let EngineError::Internal { message } = err;
        assert!(message.contains("boom"), "message was {message:?}");
    }

    #[test]
    fn guarded_reports_formatted_string_payloads() {
        let err = guarded(|| -> u8 { panic!("x {}", 1) }).unwrap_err();
        let EngineError::Internal { message } = err;
        assert!(message.contains("x 1"), "message was {message:?}");
    }

    #[test]
    fn guarded_reports_unknown_payloads() {
        let err = guarded(|| -> u8 { std::panic::panic_any(5_u8) }).unwrap_err();
        let EngineError::Internal { message } = err;
        assert_eq!(message, "unknown panic");
    }

    #[test]
    fn engine_version_is_reported() {
        assert_eq!(engine_version().unwrap(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn engine_without_lexicons_suggests_the_typed_word() {
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let s = engine.suggest("hello wor".into(), InputLayout::Latin, true).unwrap();
        assert_eq!(s.typed, "wor");
        assert_eq!(s.candidates, vec![Candidate { text: "wor".into(), kind: CandidateKind::Verbatim }]);
        assert!(engine.loaded_sources().unwrap().is_empty());
    }

    #[test]
    fn missing_files_disable_sources_instead_of_failing() {
        let config = EngineConfig { english: Some("/nonexistent/en.fst".into()), ..EngineConfig::default() };
        let engine = Engine::new(config).unwrap();
        assert!(engine.loaded_sources().unwrap().is_empty());
    }

    #[test]
    fn learning_crosses_ffi() {
        let dir = std::env::temp_dir().join(format!("mlengine-ffi-test-{}", std::process::id()));
        let config = EngineConfig { user_dir: Some(dir.to_string_lossy().into_owned()), ..EngineConfig::default() };
        let engine = Engine::new(config.clone()).unwrap();
        engine.learn("Thom".into()).unwrap();
        engine.note_committed("Anu".into()).unwrap();
        engine.forget_learned().unwrap();
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn nn_benchmark_times_the_tiny_fixture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../core/tests/fixtures/translit-tiny.mlkbnn"
        );
        let bench = nn_benchmark(path.into(), 5).unwrap();
        assert!(bench.load_ms >= 0.0);
        assert!(bench.p50_us <= bench.p99_us);
        assert!(bench.encode_p50_us <= bench.p99_us);
    }

    #[test]
    fn nn_benchmark_reports_a_missing_model_as_an_error() {
        assert!(nn_benchmark("/nonexistent/model.mlkbnn".into(), 1).is_err());
    }

    #[test]
    fn malayalam_vowel_sign_base_crosses_ffi() {
        assert_eq!(malayalam_vowel_sign_base("ന്റ".into()).unwrap(), Some("റ".to_string()));
        assert_eq!(malayalam_vowel_sign_base("കി".into()).unwrap(), None);
    }
}

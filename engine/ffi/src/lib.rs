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

/// A point of a swipe's path, in the keyboard view's points.
#[derive(Debug, Clone, Copy, PartialEq, uniffi::Record)]
pub struct SwipePoint {
    pub x: f32,
    pub y: f32,
}

/// A letter key as drawn, in the same points as the path.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct SwipeKey {
    /// One ASCII letter.
    pub letter: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
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
    /// The language model (`.mlkbnn`); nil, or one built against other lexicons, keeps
    /// ranking on the previous word and `next.fst`.
    #[uniffi(default = None)]
    pub lm: Option<String>,
    #[uniffi(default = None)]
    pub user_dir: Option<String>,
    /// A language pack folder (`<id>.neopack`, M6). When it opens, its files replace
    /// `malayalam`, `manglish`, `next_words`, `translit` and `lm`; when it is refused, those
    /// fields are used and `Engine::pack_problem` says why.
    #[uniffi(default = None)]
    pub pack_dir: Option<String>,
    /// Milliseconds from the start of a suggest after which the transliterator stops and its
    /// finished spellings are used (`mlengine_core::nn::Deadline`); nil lets it run to the end.
    #[uniffi(default = None)]
    pub model_deadline_ms: Option<u32>,
    /// LM cutoffs in milliseconds from the start of a suggest (`mlengine_core::Timing`): the
    /// context prefill while a word is typed, everything the LM does after a space, and
    /// tier-1 rows. Nil lets that stage run to the end.
    #[uniffi(default = None)]
    pub lm_prefill_ms: Option<u32>,
    #[uniffi(default = None)]
    pub lm_after_space_ms: Option<u32>,
    #[uniffi(default = None)]
    pub lm_deadline_ms: Option<u32>,
    /// How long `prepare` may fill the LM's context; nil, until it is done.
    #[uniffi(default = None)]
    pub lm_prepare_ms: Option<u32>,
}

fn millis(ms: Option<u32>) -> Option<std::time::Duration> {
    ms.map(|ms| std::time::Duration::from_millis(ms.into()))
}

/// The engine's paths for `c`, and why its pack was refused, if it was.
fn paths(c: &EngineConfig) -> (mlengine_core::Paths, Option<String>) {
    let path = |s: &Option<String>| s.as_ref().map(std::path::PathBuf::from);
    let Some(dir) = path(&c.pack_dir) else {
        return (c.into(), None);
    };
    match mlengine_core::pack::Pack::open(&dir) {
        Ok(pack) => (
            mlengine_core::Paths::with_packs(path(&c.english), &[pack], path(&c.user_dir)),
            None,
        ),
        Err(e) => (c.into(), Some(e.to_string())),
    }
}

impl From<&EngineConfig> for mlengine_core::Paths {
    fn from(c: &EngineConfig) -> Self {
        let path = |s: &Option<String>| s.as_ref().map(std::path::PathBuf::from);
        // Without a pack folder, the explicit fields describe one Malayalam pack.
        let ml = mlengine_core::PackPaths {
            lexicon: path(&c.malayalam),
            romanised: path(&c.manglish),
            // The romanisation tables sit beside the romanised lexicon, as they do in
            // packs/ml.neopack.
            romanisation: path(&c.manglish)
                .map(|m| m.with_file_name("romanisation.json"))
                .filter(|p| p.exists()),
            next_words: path(&c.next_words),
            translit: path(&c.translit),
            lm: path(&c.lm),
            ..mlengine_core::PackPaths::new("ml", "mlym")
        };
        Self {
            english: path(&c.english),
            packs: vec![ml],
            user_dir: path(&c.user_dir),
        }
    }
}

impl From<InputLayout> for mlengine_core::Layout {
    fn from(layout: InputLayout) -> Self {
        match layout {
            InputLayout::Latin => Self::Latin,
            InputLayout::Malayalam => Self::Malayalam,
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
            // Swift still calls a romanised word `manglish`.
            Kind::Romanised => Self::Manglish,
            Kind::NextWord => Self::NextWord,
        }
    }
}

#[derive(uniffi::Object)]
pub struct Engine {
    inner: mlengine_core::Engine,
    model_deadline: Option<std::time::Duration>,
    lm_prefill: Option<std::time::Duration>,
    lm_after_space: Option<std::time::Duration>,
    lm_deadline: Option<std::time::Duration>,
    lm_prepare: Option<std::time::Duration>,
    pack_problem: Option<String>,
}

#[uniffi::export]
impl Engine {
    /// Maps the lexicon files and loads learned words. Never fails because a
    /// file is missing or corrupt: that source is just disabled.
    #[uniffi::constructor]
    pub fn new(config: EngineConfig) -> Result<Arc<Self>, EngineError> {
        guarded(|| {
            let (paths, pack_problem) = paths(&config);
            Arc::new(Self {
                inner: mlengine_core::Engine::open(&paths),
                pack_problem,
                model_deadline: millis(config.model_deadline_ms),
                lm_prefill: millis(config.lm_prefill_ms),
                lm_after_space: millis(config.lm_after_space_ms),
                lm_deadline: millis(config.lm_deadline_ms),
                lm_prepare: millis(config.lm_prepare_ms),
            })
        })
    }

    pub fn suggest(
        &self,
        context_before: String,
        layout: InputLayout,
        autocorrect: bool,
    ) -> Result<Suggestions, EngineError> {
        guarded(|| {
            let layout = layout.into();
            let start = std::time::Instant::now();
            let elapsed = || start.elapsed();
            let timing = mlengine_core::Timing {
                elapsed: &elapsed,
                translit: self.model_deadline,
                lm_prefill: self.lm_prefill,
                lm_after_space: self.lm_after_space,
                lm_deadline: self.lm_deadline,
            };
            let s = self.inner.suggest_timed(&context_before, layout, autocorrect, Some(&timing));
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

    /// The words a swipe across the letter keys could have drawn, best first (design
    /// `2026-11-06-m4-swipe-design.md`): English words and Manglish words in Malayalam script.
    /// The LM's context is filled within `lm_after_space_ms`, as after a space.
    pub fn swipe(
        &self,
        path: Vec<SwipePoint>,
        keys: Vec<SwipeKey>,
        context_before: String,
    ) -> Result<Vec<Candidate>, EngineError> {
        guarded(|| {
            let path: Vec<(f32, f32)> = path.iter().map(|p| (p.x, p.y)).collect();
            let keys: Vec<mlengine_core::swipe::Key> = keys
                .iter()
                .filter_map(|k| {
                    let &[letter] = k.letter.as_bytes() else { return None };
                    letter.is_ascii_alphabetic().then(|| mlengine_core::swipe::Key {
                        letter: letter.to_ascii_lowercase(),
                        x: k.x,
                        y: k.y,
                        width: k.width,
                        height: k.height,
                    })
                })
                .collect();
            self.inner
                .swipe(&path, &keys, &context_before, self.lm_after_space)
                .into_iter()
                .map(|c| Candidate { text: c.text, kind: c.kind.into() })
                .collect()
        })
    }

    /// Fills the LM's context for the text before the cursor, within `lm_prepare_ms`, so the
    /// first suggest in a field already has it. Call when the keyboard appears.
    pub fn prepare(&self, context_before: String) -> Result<(), EngineError> {
        guarded(|| self.inner.prepare(&context_before, self.lm_prepare))
    }

    /// The user undid an autocorrect or tapped what they typed: never replace this word.
    pub fn learn(&self, word: String) -> Result<(), EngineError> {
        guarded(|| self.inner.learn(&word))
    }

    /// A word was committed as typed; an unknown word is learned the second time.
    pub fn note_committed(&self, word: String) -> Result<(), EngineError> {
        guarded(|| self.inner.note_committed(&word))
    }

    /// A word was committed after `context_before` (typed, corrected or tapped), from
    /// `layout`: learns its use, the phrase it continues and the language habit. Never call
    /// it for a password field.
    pub fn note_used(
        &self,
        context_before: String,
        word: String,
        layout: InputLayout,
    ) -> Result<(), EngineError> {
        guarded(|| self.inner.note_used(&context_before, &word, layout.into()))
    }

    /// The user tapped `chosen` for `typed` though the bar's middle was something else.
    pub fn note_choice(&self, typed: String, chosen: String) -> Result<(), EngineError> {
        guarded(|| self.inner.note_choice(&typed, &chosen))
    }

    /// Forgets learned words and words removed from the bar.
    pub fn forget_learned(&self) -> Result<(), EngineError> {
        guarded(|| self.inner.forget_learned())
    }

    /// The user held a suggestion and chose Remove: never suggest it again until learned.
    pub fn remove_word(&self, word: String) -> Result<(), EngineError> {
        guarded(|| self.inner.remove_word(&word))
    }

    /// Every learned word, as shown, most recently used first: for the "Your words" screen.
    pub fn learned_words(&self) -> Result<Vec<String>, EngineError> {
        guarded(|| self.inner.learned_words())
    }

    /// The words removed from the bar, most recently removed first (ASCII lowercased).
    pub fn removed_words(&self) -> Result<Vec<String>, EngineError> {
        guarded(|| self.inner.removed_words())
    }

    /// Forgets one learned word without blocking it, unlike `remove_word`.
    pub fn forget_word(&self, word: String) -> Result<(), EngineError> {
        guarded(|| self.inner.forget_word(&word))
    }

    /// Un-removes a word removed from the bar; it is not learned by this.
    pub fn restore_word(&self, word: String) -> Result<(), EngineError> {
        guarded(|| self.inner.restore_word(&word))
    }

    /// The learned emails and URLs: the keyboard treats letters that begin one as one word.
    pub fn learned_addresses(&self) -> Result<Vec<String>, EngineError> {
        guarded(|| self.inner.learned_addresses())
    }

    /// Which sources loaded, for diagnostics ("english", "ml:lexicon", "ml:lm", ...).
    /// Why the configured pack was refused; nil when it opened or none was given.
    pub fn pack_problem(&self) -> Option<String> {
        self.pack_problem.clone()
    }

    pub fn loaded_sources(&self) -> Result<Vec<String>, EngineError> {
        guarded(|| self.inner.loaded_sources())
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
    fn the_lm_and_its_cutoffs_cross_ffi() {
        let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/tests/fixtures");
        let config = EngineConfig {
            lm: Some(format!("{fixtures}/lm-tiny.mlkbnn")),
            model_deadline_ms: Some(8),
            lm_prefill_ms: Some(3),
            lm_after_space_ms: Some(10),
            lm_deadline_ms: Some(11),
            lm_prepare_ms: Some(6),
            ..EngineConfig::default()
        };
        let engine = Engine::new(config).unwrap();
        // No lexicons: the LM was built against others, so it is not loaded.
        assert!(!engine.loaded_sources().unwrap().contains(&"ml:lm".to_string()));
        engine.prepare("hello there ".into()).unwrap();
        let s = engine.suggest("hello th".into(), InputLayout::Latin, true).unwrap();
        assert_eq!(s.typed, "th");
    }

    #[test]
    fn a_pack_folder_fills_the_malayalam_sources() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let config = EngineConfig {
            english: Some(format!("{root}/apps/ios/KeyboardExtension/Lexicon/en.fst")),
            pack_dir: Some(format!("{root}/packs/ml.neopack")),
            ..EngineConfig::default()
        };
        let engine = Engine::new(config).unwrap();
        assert_eq!(engine.pack_problem(), None);
        assert_eq!(
            engine.loaded_sources().unwrap(),
            ["english", "ml:lexicon", "ml:romanised", "ml:next_words", "ml:translit", "ml:lm"]
        );
    }

    #[test]
    fn a_refused_pack_says_why_and_uses_the_explicit_paths() {
        let config = EngineConfig {
            pack_dir: Some("/nonexistent/xx.neopack".into()),
            ..EngineConfig::default()
        };
        let engine = Engine::new(config).unwrap();
        assert!(engine.pack_problem().unwrap().starts_with("manifest unreadable"));
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
        engine.learn("sam@example.com".into()).unwrap();
        assert_eq!(engine.learned_addresses().unwrap(), ["sam@example.com"]);
        engine.forget_learned().unwrap();
        assert!(engine.learned_addresses().unwrap().is_empty());
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn phrases_and_choices_cross_ffi_and_persist() {
        let dir = std::env::temp_dir().join(format!("mlengine-ffi-phrase-test-{}", std::process::id()));
        let config = EngineConfig { user_dir: Some(dir.to_string_lossy().into_owned()), ..EngineConfig::default() };
        let texts = |engine: &Engine, context: &str| -> Vec<String> {
            let s = engine.suggest(context.into(), InputLayout::Latin, true).unwrap();
            s.candidates.into_iter().map(|c| c.text).collect()
        };
        let engine = Engine::new(config.clone()).unwrap();
        engine.learn("Thom".into()).unwrap();
        engine.learn("Anu".into()).unwrap();
        engine.note_used("".into(), "Thom".into(), InputLayout::Latin).unwrap();
        engine.note_used("Thom ".into(), "Anu".into(), InputLayout::Latin).unwrap();
        engine.note_choice("thmo".into(), "Thom".into()).unwrap();
        drop(engine);
        let engine = Engine::new(config).unwrap();
        assert_eq!(texts(&engine, "Thom "), ["Anu"]);
        assert_eq!(texts(&engine, "thmo")[0], "Thom");
        engine.forget_learned().unwrap();
        assert!(texts(&engine, "Thom ").is_empty());
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn phone_numbers_cross_ffi_and_persist() {
        let dir = std::env::temp_dir().join(format!("mlengine-ffi-number-test-{}", std::process::id()));
        let config = EngineConfig { user_dir: Some(dir.to_string_lossy().into_owned()), ..EngineConfig::default() };
        let engine = Engine::new(config.clone()).unwrap();
        for _ in 0..2 {
            engine.note_used("call +91 55501 ".into(), "12345".into(), InputLayout::Latin).unwrap();
        }
        drop(engine);
        let engine = Engine::new(config).unwrap();
        assert_eq!(engine.learned_addresses().unwrap(), ["+91 55501 12345"]);
        let s = engine.suggest("call +91 98".into(), InputLayout::Latin, true).unwrap();
        assert_eq!(s.typed, "+91 98");
        assert_eq!(s.candidates[1].text, "+91 55501 12345");
        engine.remove_word("+91 55501 12345".into()).unwrap();
        assert!(engine.learned_addresses().unwrap().is_empty());
        assert_eq!(engine.removed_words().unwrap(), ["+91 55501 12345"]);
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn removal_crosses_ffi_and_persists() {
        let dir = std::env::temp_dir().join(format!("mlengine-ffi-remove-test-{}", std::process::id()));
        let config = EngineConfig { user_dir: Some(dir.to_string_lossy().into_owned()), ..EngineConfig::default() };
        let texts = |engine: &Engine| -> Vec<String> {
            let s = engine.suggest("thoma".into(), InputLayout::Latin, true).unwrap();
            s.candidates.into_iter().map(|c| c.text).collect()
        };
        let engine = Engine::new(config.clone()).unwrap();
        engine.learn("Thomasina".into()).unwrap();
        assert!(texts(&engine).contains(&"Thomasina".to_string()));
        engine.remove_word("Thomasina".into()).unwrap();
        assert!(!texts(&engine).contains(&"Thomasina".to_string()));
        drop(engine);
        let engine = Engine::new(config).unwrap();
        assert!(!texts(&engine).contains(&"Thomasina".to_string()), "the removal was kept");
        drop(engine);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn your_words_cross_ffi_and_persist() {
        let dir = std::env::temp_dir().join(format!(
            "mlengine-ffi-your-words-test-{}",
            std::process::id()
        ));
        let config = EngineConfig {
            user_dir: Some(dir.to_string_lossy().into_owned()),
            ..EngineConfig::default()
        };
        let engine = Engine::new(config.clone()).unwrap();
        engine.learn("Thom".into()).unwrap();
        engine.learn("Anu".into()).unwrap();
        engine.remove_word("hello".into()).unwrap();
        engine.remove_word("world".into()).unwrap();
        assert_eq!(engine.learned_words().unwrap(), ["Anu", "Thom"]);
        assert_eq!(engine.removed_words().unwrap(), ["world", "hello"]);
        engine.forget_word("Thom".into()).unwrap();
        engine.restore_word("hello".into()).unwrap();
        drop(engine);
        let engine = Engine::new(config).unwrap();
        assert_eq!(
            engine.learned_words().unwrap(),
            ["Anu"],
            "the forget was kept"
        );
        assert_eq!(
            engine.removed_words().unwrap(),
            ["world"],
            "the restore was kept"
        );
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

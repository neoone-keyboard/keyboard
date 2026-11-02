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
    pub next_words: Option<String>,
    #[uniffi(default = None)]
    pub user_dir: Option<String>,
}

impl From<&EngineConfig> for mlengine_core::Paths {
    fn from(c: &EngineConfig) -> Self {
        let path = |s: &Option<String>| s.as_ref().map(std::path::PathBuf::from);
        Self {
            english: path(&c.english),
            malayalam: path(&c.malayalam),
            next_words: path(&c.next_words),
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

/// The consonant a vowel sign would attach to, if the text ends in one.
#[uniffi::export]
pub fn malayalam_vowel_sign_base(context_before: String) -> Result<Option<String>, EngineError> {
    guarded(|| mlengine_core::malayalam::vowel_sign_base(&context_before).map(String::from))
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
    fn malayalam_vowel_sign_base_crosses_ffi() {
        assert_eq!(malayalam_vowel_sign_base("ന്റ".into()).unwrap(), Some("റ".to_string()));
        assert_eq!(malayalam_vowel_sign_base("കി".into()).unwrap(), None);
    }
}

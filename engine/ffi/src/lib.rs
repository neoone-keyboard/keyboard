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

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Suggestion {
    pub text: String,
}

#[derive(uniffi::Object)]
pub struct Engine {
    inner: mlengine_core::Engine,
}

#[uniffi::export]
impl Engine {
    #[uniffi::constructor]
    pub fn new() -> Result<Arc<Self>, EngineError> {
        guarded(|| Arc::new(Self { inner: mlengine_core::Engine::new() }))
    }

    pub fn suggest(&self, context_before: String) -> Result<Vec<Suggestion>, EngineError> {
        guarded(|| {
            self.inner
                .suggest(&context_before)
                .into_iter()
                .map(|text| Suggestion { text })
                .collect()
        })
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
    fn engine_suggest_returns_records() {
        let engine = Engine::new().unwrap();
        assert_eq!(
            engine.suggest("hello wor".into()).unwrap(),
            vec![Suggestion { text: "wor".into() }]
        );
    }

    #[test]
    fn malayalam_vowel_sign_base_crosses_ffi() {
        assert_eq!(malayalam_vowel_sign_base("ന്റ".into()).unwrap(), Some("റ".to_string()));
        assert_eq!(malayalam_vowel_sign_base("കി".into()).unwrap(), None);
    }
}

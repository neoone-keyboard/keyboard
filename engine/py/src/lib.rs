//! Python access to the production engine, for the evaluation harness only.
//! Never shipped: what the harness measures is exactly what the keyboard runs.

use std::path::PathBuf;

use mlengine_core::lexicon::Lexicon;
use mlengine_core::{Engine as Core, Kind, Layout, Paths};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// The engine over the given lexicon files; a missing or unreadable file is
/// left out, exactly as on the device.
#[pyclass(frozen)]
struct Engine(Core);

#[pymethods]
impl Engine {
    #[new]
    #[pyo3(signature = (english=None, malayalam=None, manglish=None, next_words=None, translit=None))]
    fn new(
        english: Option<PathBuf>,
        malayalam: Option<PathBuf>,
        manglish: Option<PathBuf>,
        next_words: Option<PathBuf>,
        translit: Option<PathBuf>,
    ) -> Self {
        let _ = translit; // wired in Task 10
        Engine(Core::open(&Paths {
            english,
            malayalam,
            manglish,
            next_words,
            user_dir: None,
        }))
    }

    /// Suggestions for the text before the cursor, in bar order (the middle
    /// slot first). Each item is (text, kind, score); `layout` is "latin" or
    /// "malayalam".
    fn suggest(
        &self,
        context: &str,
        layout: &str,
        autocorrect: bool,
    ) -> PyResult<Vec<(String, &'static str, f32)>> {
        let layout = match layout {
            "latin" => Layout::Latin,
            "malayalam" => Layout::Malayalam,
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

    /// Names of the sources that loaded, e.g. ["english", "malayalam"].
    fn loaded_sources(&self) -> Vec<&'static str> {
        self.0.loaded_sources()
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Verbatim => "verbatim",
        Kind::Completion => "completion",
        Kind::Correction => "correction",
        Kind::Manglish => "manglish",
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

#[pymodule]
fn mlengine(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Engine>()?;
    m.add_function(wrap_pyfunction!(malayalam_words, m)?)
}

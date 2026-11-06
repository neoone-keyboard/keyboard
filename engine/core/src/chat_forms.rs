//! Chat spellings no general rule reaches (`nanni` for നന്ദി, `nte` for എന്റെ), from a pack's
//! hand-written `chat-forms.tsv` (manifest role `chatForms`). See Malayalam's file for what
//! belongs there and why; `Engine::manglish` offers a row's word as if the typed letters
//! spelled it.
//!
//! The format: one row per line, `latin<TAB>native`; lines starting with `#` and blank lines
//! are skipped, as is a line without a tab.

use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatForms {
    rows: Vec<(String, String)>,
}

impl ChatForms {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self::parse(&text))
    }

    pub fn parse(text: &str) -> Self {
        let rows = text
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
            .filter_map(|line| line.split_once('\t'))
            .map(|(latin, word)| (latin.to_string(), word.to_string()))
            .collect();
        Self { rows }
    }

    /// The native word for a typed chat spelling, ignoring ASCII case.
    pub fn lookup(&self, typed: &str) -> Option<&str> {
        self.rows
            .iter()
            .find_map(|(latin, word)| latin.eq_ignore_ascii_case(typed).then_some(word.as_str()))
    }

    /// The table's `(latin, native)` rows, in file order.
    pub fn rows(&self) -> impl Iterator<Item = (&str, &str)> {
        self.rows.iter().map(|(l, w)| (l.as_str(), w.as_str()))
    }
}

/// Malayalam's table from `packs/ml.neopack`, for the engine's unit tests.
#[cfg(test)]
pub(crate) fn ml() -> &'static ChatForms {
    static ML: std::sync::OnceLock<ChatForms> = std::sync::OnceLock::new();
    ML.get_or_init(load_ml)
}

/// An owned copy of `ml()`, for a test's `Sources`.
#[cfg(test)]
pub(crate) fn load_ml() -> ChatForms {
    ChatForms::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/ml.neopack/chat-forms.tsv"),
    )
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::normalize;
    use crate::text::{Script, script};

    #[test]
    fn finds_rows_ignoring_case() {
        assert_eq!(ml().lookup("nanni"), Some("നന്ദി"));
        assert_eq!(ml().lookup("Nanni"), Some("നന്ദി"));
        assert_eq!(ml().lookup("nandi"), None);
        assert_eq!(ml().lookup(""), None);
    }

    #[test]
    fn parses_rows_skipping_comments_blanks_and_lines_without_a_tab() {
        let forms = ChatForms::parse("# note\n\nnte\tഎന്റെ\nbroken\nnanni\tനന്ദി\n");
        assert_eq!(
            forms.rows().collect::<Vec<_>>(),
            [("nte", "എന്റെ"), ("nanni", "നന്ദി")]
        );
        assert!(ChatForms::load(Path::new("/nonexistent/chat-forms.tsv")).is_err());
    }

    #[test]
    fn rows_are_lowercase_latin_to_normalised_malayalam() {
        let rows: Vec<_> = ml().rows().collect();
        assert!(rows.len() >= 5, "{rows:?}");
        let mut seen = std::collections::HashSet::new();
        for (latin, word) in rows {
            assert!(latin.bytes().all(|b| b.is_ascii_lowercase()), "{latin:?}");
            assert_eq!(script(word), Script::MALAYALAM, "{word:?}");
            assert_eq!(normalize(word), word, "{word:?} is not in normal form");
            assert!(seen.insert(latin), "{latin} is listed twice");
        }
    }
}

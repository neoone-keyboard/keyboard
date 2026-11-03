//! Chat spellings no general rule reaches (`nanni` for നന്ദി, `nte` for എന്റെ), from the
//! hand-written table `data/chat-forms.tsv`, compiled in. See that file for what belongs
//! there and why; `Engine::manglish` offers a row's word as if the typed letters spelled it.

const TABLE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/chat-forms.tsv"
));

/// The Malayalam word for a typed chat spelling, ignoring ASCII case.
pub fn lookup(typed: &str) -> Option<&'static str> {
    rows().find_map(|(latin, word)| latin.eq_ignore_ascii_case(typed).then_some(word))
}

/// The table's `(latin, malayalam)` rows; `#` lines and blank lines skipped.
pub fn rows() -> impl Iterator<Item = (&'static str, &'static str)> {
    TABLE
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('\t'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::normalize::normalize;
    use crate::text::{Script, script};

    #[test]
    fn finds_rows_ignoring_case() {
        assert_eq!(lookup("nanni"), Some("നന്ദി"));
        assert_eq!(lookup("Nanni"), Some("നന്ദി"));
        assert_eq!(lookup("nandi"), None);
        assert_eq!(lookup(""), None);
    }

    #[test]
    fn rows_are_lowercase_latin_to_normalised_malayalam() {
        let rows: Vec<_> = rows().collect();
        assert!(rows.len() >= 5, "{rows:?}");
        let mut seen = std::collections::HashSet::new();
        for (latin, word) in rows {
            assert!(latin.bytes().all(|b| b.is_ascii_lowercase()), "{latin:?}");
            assert_eq!(script(word), Script::Malayalam, "{word:?}");
            assert_eq!(normalize(word), word, "{word:?} is not in normal form");
            assert!(seen.insert(latin), "{latin} is listed twice");
        }
    }
}

//! A pack's `layout.json` (manifest role `layout`, format 1): the native-script keyboard. The
//! format is specified in `docs/design/pack-format.md`
//! ("layout.json"). The engine uses only `vowel_sign_strip`, to order the strip
//! (`Engine::suggest`'s `vowel_signs`); the keyboard reads the rest.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackLayout {
    pub format: u32,
    /// The space bar's name while this layout is up: "മലയാളം".
    pub space_bar: String,
    /// The language key's label on another layout, to switch to this one: "മ".
    pub language_key: String,
    /// The native pages, the first being the one the layout opens on.
    pub pages: Vec<Page>,
    /// Long-press alternates by key: a consonant's chillu.
    #[serde(default)]
    pub long_press: BTreeMap<String, Vec<String>>,
    /// What the strip offers after a letter a mark can attach to (`ScriptRules::mark_base`),
    /// in the order shown when the engine has no preference.
    #[serde(default, deserialize_with = "chars")]
    pub vowel_sign_strip: Vec<char>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Page {
    pub id: String,
    /// The label of the key that shows this page: "കഖ".
    pub label: String,
    /// The text each key types, row by row. The keyboard adds its own keys (shift, backspace,
    /// the bottom row).
    pub rows: Vec<Vec<String>>,
}

/// A list of one-char strings.
fn chars<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<char>, D::Error> {
    let strings: Vec<String> = Vec::deserialize(d)?;
    strings
        .iter()
        .map(|s| {
            let mut it = s.chars();
            match (it.next(), it.next()) {
                (Some(c), None) => Ok(c),
                _ => Err(serde::de::Error::custom(format!(
                    "strip sign {s:?} is not one char"
                ))),
            }
        })
        .collect()
}

impl PackLayout {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn parse(json: &str) -> Result<Self, String> {
        let layout: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if layout.format != 1 {
            return Err(format!(
                "layout format {} (this engine reads 1)",
                layout.format
            ));
        }
        if layout.pages.is_empty() {
            return Err("a layout has at least one page".into());
        }
        for page in &layout.pages {
            if page.rows.iter().flatten().any(String::is_empty) {
                return Err(format!("page {:?} has an empty key", page.id));
            }
        }
        Ok(layout)
    }
}

/// Malayalam's layout from `packs/ml.neopack`, for the engine's unit tests.
#[cfg(test)]
pub(crate) fn load_ml() -> PackLayout {
    PackLayout::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/ml.neopack/layout.json"),
    )
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malayalams_layout_loads() {
        let ml = load_ml();
        assert_eq!(ml.space_bar, "മലയാളം");
        assert_eq!(ml.language_key, "മ");
        let labels: Vec<&str> = ml.pages.iter().map(|p| p.label.as_str()).collect();
        assert_eq!(labels, ["കഖ", "ശഷ", "അആ"]);
        assert_eq!(ml.long_press["ന"], ["ൻ"]);
        // Every strip sign can be typed from the vowel page.
        let vowels: Vec<&String> = ml.pages[2].rows.iter().flatten().collect();
        for sign in &ml.vowel_sign_strip {
            assert!(vowels.iter().any(|k| k.chars().eq([*sign])), "{sign:?}");
        }
    }

    #[test]
    fn refuses_what_it_cannot_read() {
        let good = r#"{"format": 1, "spaceBar": "x", "languageKey": "x",
                       "pages": [{"id": "a", "label": "a", "rows": [["a"]]}]}"#;
        assert!(PackLayout::parse(good).is_ok());
        for bad in [
            good.replace("\"format\": 1", "\"format\": 2"),
            good.replace(
                "[{\"id\": \"a\", \"label\": \"a\", \"rows\": [[\"a\"]]}]",
                "[]",
            ),
            good.replace("[[\"a\"]]", "[[\"\"]]"),
            good.replace(
                "\"format\": 1",
                "\"format\": 1, \"vowelSignStrip\": [\"ab\"]",
            ),
            "{".to_string(),
        ] {
            assert!(PackLayout::parse(&bad).is_err(), "{bad}");
        }
    }
}

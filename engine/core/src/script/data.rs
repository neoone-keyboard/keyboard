//! A script defined by data: the `lang` file of a pack (`lang.json`, the same file as
//! `data/langs/<id>/lang.json`, which `lexbuild-native` builds the lexicons with). One parser for
//! the builder and the engine, so the two read one definition of what a word is and how it is
//! normalised.
//!
//! Every field but `script` is optional, and the Brahmic ones are features a script may or may
//! not use:
//!
//! - `script`: the code the manifest names (ISO 15924 in lower case; a private-use code such as
//!   `qaaa` for an invented script).
//! - `block` (`["0A80", "0AFF"]`) and `letters` (`[["A000", "A48F"], …]`): the script's
//!   characters, as hex code point ranges. At least one range between them. No size limit here
//!   (the transliterator's vocabulary and the romanised lexicon's packing have their own).
//! - `notWord`: ranges inside those that are not part of a word (digits, currency signs).
//! - `bases`: letters a combining sign attaches to. A vowel sign typed after one is shown on
//!   the vowel-sign strip (`mark_base`). Without `bases`, there is no strip.
//! - `signs`, `virama`: combining signs that must follow a base, and the virama. Only the
//!   builder reads them (which corpus tokens are well-formed words).
//! - `replace`: `[from, to]` text replacements that make one spelling, applied before and after
//!   the engine's `normalize::normalize` (NFC, `’` to `'`, joiners dropped).
//! - `phraseBreaks`: characters that end a phrase as a full stop does (the danda).
//! - `apostrophe`: an apostrophe may stand inside a word.
//!
//! Other keys (the builder's `firstLetter`, `maxWords`, notes) are ignored here.

use std::ops::RangeInclusive;
use std::sync::Mutex;

use serde::Deserialize;

use super::ScriptRules;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Raw {
    script: String,
    #[serde(default)]
    block: Option<[String; 2]>,
    #[serde(default)]
    letters: Vec<[String; 2]>,
    #[serde(default)]
    not_word: Vec<[String; 2]>,
    #[serde(default)]
    virama: Option<String>,
    #[serde(default)]
    signs: Vec<[String; 2]>,
    #[serde(default)]
    bases: Vec<[String; 2]>,
    #[serde(default)]
    apostrophe: bool,
    #[serde(default)]
    replace: Vec<[String; 2]>,
    #[serde(default)]
    phrase_breaks: Vec<String>,
}

/// A script definition, parsed and checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptDef {
    /// The script code: "gujr".
    pub code: String,
    /// The character ranges (`block`, then `letters`).
    pub ranges: Vec<(char, char)>,
    pub not_word: Vec<(char, char)>,
    pub virama: Option<char>,
    /// Combining signs that must follow a `bases` letter.
    pub signs: Vec<(char, char)>,
    pub bases: Vec<(char, char)>,
    /// Apostrophes may stand inside a word (Assamese হ'ল).
    pub apostrophe: bool,
    pub replace: Vec<(String, String)>,
    pub phrase_breaks: Vec<char>,
}

fn in_ranges(ranges: &[(char, char)], c: char) -> bool {
    ranges.iter().any(|&(a, b)| (a..=b).contains(&c))
}

fn code_point(hex: &str) -> Result<char, String> {
    u32::from_str_radix(hex, 16)
        .ok()
        .and_then(char::from_u32)
        .ok_or_else(|| format!("not a code point: {hex:?}"))
}

fn range(pair: &[String; 2]) -> Result<(char, char), String> {
    let (a, b) = (code_point(&pair[0])?, code_point(&pair[1])?);
    if b < a {
        return Err(format!("range {}–{} ends before it starts", pair[0], pair[1]));
    }
    Ok((a, b))
}

fn ranges(pairs: &[[String; 2]]) -> Result<Vec<(char, char)>, String> {
    pairs.iter().map(range).collect()
}

fn single_char(s: &str, what: &str) -> Result<char, String> {
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(c),
        _ => Err(format!("{what} must be one character: {s:?}")),
    }
}

/// A script code: 1–16 of a-z, 0-9 and "-", starting with a letter.
fn valid_code(code: &str) -> bool {
    (1..=16).contains(&code.len())
        && code.starts_with(|c: char| c.is_ascii_lowercase())
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl ScriptDef {
    /// Parses `lang.json` text; the error says what is wrong in words.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let raw: Raw = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if !valid_code(&raw.script) {
            return Err(format!("script code {:?} is not allowed", raw.script));
        }
        let mut chars = Vec::new();
        if let Some(block) = &raw.block {
            chars.push(range(block)?);
        }
        chars.extend(ranges(&raw.letters)?);
        if chars.is_empty() {
            return Err("no characters: give `block` or `letters`".into());
        }
        let ascii = chars.iter().any(|&(a, _)| a.is_ascii());
        if ascii {
            return Err("a script's characters can't be ASCII (that is English's)".into());
        }
        let replace = raw
            .replace
            .into_iter()
            .map(|[a, b]| {
                if a.is_empty() {
                    Err("a replacement can't replace the empty string".to_string())
                } else {
                    Ok((a, b))
                }
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            code: raw.script,
            ranges: chars,
            not_word: ranges(&raw.not_word)?,
            virama: raw.virama.as_deref().map(code_point).transpose()?,
            signs: ranges(&raw.signs)?,
            bases: ranges(&raw.bases)?,
            apostrophe: raw.apostrophe,
            replace,
            phrase_breaks: raw
                .phrase_breaks
                .iter()
                .map(|s| single_char(s, "a phrase break"))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Reads and parses a `lang.json`.
    pub fn open(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// In the script's ranges.
    pub fn in_script(&self, c: char) -> bool {
        in_ranges(&self.ranges, c)
    }

    /// A letter or sign of the script: in its ranges and not `notWord`.
    pub fn is_word_char(&self, c: char) -> bool {
        self.in_script(c) && !in_ranges(&self.not_word, c)
    }

    pub fn is_sign(&self, c: char) -> bool {
        in_ranges(&self.signs, c)
    }

    pub fn is_base(&self, c: char) -> bool {
        in_ranges(&self.bases, c)
    }

    /// Every sign follows a base letter, as in well-formed text.
    pub fn signs_follow_bases(&self, chars: &[char]) -> bool {
        chars
            .iter()
            .enumerate()
            .all(|(i, &c)| !self.is_sign(c) || (i > 0 && self.is_base(chars[i - 1])))
    }

    pub fn is_phrase_break(&self, c: char) -> bool {
        self.phrase_breaks.contains(&c)
    }

    fn apply_replace(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (from, to) in &self.replace {
            if out.contains(from.as_str()) {
                out = out.replace(from.as_str(), to);
            }
        }
        out
    }

    /// The script's normal form of `text`: `replace`, the engine's `normalize`, `replace` again,
    /// and one more round when a replacement made text that NFC or the joiner rules change.
    pub fn normalize(&self, text: &str) -> String {
        let before = self.apply_replace(&crate::normalize::normalize(&self.apply_replace(text)));
        let after = self.apply_replace(&crate::normalize::normalize(&before));
        if after == before { before } else { after }
    }

    /// The span from the first character of the script to the last.
    pub fn span(&self) -> RangeInclusive<char> {
        let start = self.ranges.iter().map(|r| r.0).min().unwrap_or('\0');
        let end = self.ranges.iter().map(|r| r.1).max().unwrap_or('\0');
        start..=end
    }
}

/// The script plug-in a `ScriptDef` makes.
#[derive(Debug)]
pub struct DataScript {
    code: &'static str,
    def: ScriptDef,
}

/// Every `DataScript` made so far: one per distinct definition, so opening the same pack
/// again reuses it (plug-ins are `'static`, and live as long as the process).
static MADE: Mutex<Vec<&'static DataScript>> = Mutex::new(Vec::new());

impl DataScript {
    /// The plug-in for `def`, made once per distinct definition.
    pub fn intern(def: ScriptDef) -> &'static DataScript {
        let mut made = MADE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(found) = made.iter().find(|d| d.def == def) {
            return found;
        }
        let code: &'static str = Box::leak(def.code.clone().into_boxed_str());
        let script: &'static DataScript = Box::leak(Box::new(DataScript { code, def }));
        made.push(script);
        script
    }

    pub fn def(&self) -> &ScriptDef {
        &self.def
    }
}

impl ScriptRules for DataScript {
    fn code(&self) -> &'static str {
        self.code
    }
    fn block(&self) -> RangeInclusive<char> {
        self.def.span()
    }
    fn ranges(&self) -> Vec<RangeInclusive<char>> {
        self.def.ranges.iter().map(|&(a, b)| a..=b).collect()
    }
    fn is_letter(&self, c: char) -> bool {
        self.def.is_word_char(c)
    }
    fn normalize(&self, word: &str) -> String {
        self.def.normalize(word)
    }
    /// The last character, when it is a base letter; a base that is itself a combining mark
    /// (a nukta) stands for the base before it.
    fn mark_base(&self, context_before: &str) -> Option<char> {
        use unicode_normalization::char::is_combining_mark;
        let mut chars = context_before.chars().rev();
        let last = chars.next().filter(|&c| self.def.is_base(c))?;
        if !is_combining_mark(last) {
            return Some(last);
        }
        chars
            .next()
            .filter(|&c| self.def.is_base(c) && !is_combining_mark(c))
    }
    fn ends_phrase(&self, c: char) -> bool {
        self.def.is_phrase_break(c)
    }
    fn inner_apostrophe(&self) -> bool {
        self.def.apostrophe
    }
}

/// Whether two scripts share a character.
pub(super) fn overlaps(a: &dyn ScriptRules, b: &dyn ScriptRules) -> bool {
    let bs = b.ranges();
    a.ranges().iter().any(|ra| {
        bs.iter()
            .any(|rb| ra.start() <= rb.end() && rb.start() <= ra.end())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUJARATI: &str = r#"{
      "script": "gujr", "block": ["0A80", "0AFF"],
      "notWord": [["0AE6", "0AEF"], ["0AF0", "0AF1"]],
      "bases": [["0A95", "0AB9"], ["0ABC", "0ABC"]],
      "signs": [["0ABE", "0ACD"]], "virama": "0ACD",
      "phraseBreaks": ["।"], "maxWords": 10
    }"#;

    #[test]
    fn parses_and_classifies() {
        let def = ScriptDef::from_json(GUJARATI).unwrap();
        assert_eq!(def.code, "gujr");
        assert_eq!(def.virama, Some('\u{0ACD}'));
        assert!(def.is_word_char('ક') && def.is_word_char('ા'));
        assert!(!def.is_word_char('૧'), "a digit");
        assert!(!def.is_word_char('क'));
        assert!(def.is_phrase_break('।'));
    }

    #[test]
    fn only_script_and_characters_are_required() {
        let def = ScriptDef::from_json(r#"{"script": "qaaa", "letters": [["E000", "E0FF"], ["E200", "E201"]]}"#)
            .unwrap();
        assert_eq!(def.span(), '\u{E000}'..='\u{E201}');
        assert!(def.is_word_char('\u{E201}') && !def.is_word_char('\u{E1FF}'));
        let rules = DataScript::intern(def);
        assert_eq!(rules.mark_base("\u{E000}"), None, "no bases, no strip");
        assert!(!rules.inner_apostrophe());
        // Wider than 128 code points is fine for a definition.
        ScriptDef::from_json(r#"{"script": "qaab", "block": ["4E00", "9FFF"]}"#).unwrap();
    }

    #[test]
    fn refuses_what_is_wrong() {
        for (json, want) in [
            (r#"{"block": ["0A80", "0AFF"]}"#, "missing field `script`"),
            (r#"{"script": "gujr"}"#, "no characters"),
            (r#"{"script": "Gujr", "block": ["0A80", "0AFF"]}"#, "not allowed"),
            (r#"{"script": "gujr", "block": ["0AFF", "0A80"]}"#, "ends before"),
            (r#"{"script": "gujr", "block": ["zz", "0A80"]}"#, "not a code point"),
            (r#"{"script": "gujr", "block": ["0041", "005A"]}"#, "ASCII"),
            (r#"{"script": "gujr", "block": ["0A80", "0AFF"], "phraseBreaks": ["ab"]}"#, "one character"),
            (r#"{"script": "gujr", "block": ["0A80", "0AFF"], "replace": [["", "x"]]}"#, "empty"),
        ] {
            let err = ScriptDef::from_json(json).unwrap_err();
            assert!(err.contains(want), "{json}: {err}");
        }
    }

    #[test]
    fn mark_base_follows_bases() {
        let rules = DataScript::intern(ScriptDef::from_json(GUJARATI).unwrap());
        assert_eq!(rules.mark_base("ગુજર"), Some('ર'));
        assert_eq!(rules.mark_base("જ\u{0ABC}"), Some('જ'), "base + nukta");
        assert_eq!(rules.mark_base("કિ"), None);
        assert_eq!(rules.mark_base("અ"), None);
        assert_eq!(rules.mark_base(""), None);
    }

    #[test]
    fn interning_reuses_a_definition() {
        let a = DataScript::intern(ScriptDef::from_json(GUJARATI).unwrap());
        let b = DataScript::intern(ScriptDef::from_json(GUJARATI).unwrap());
        assert!(std::ptr::eq(a, b));
    }

    #[test]
    fn replacements_apply_around_the_engine_normal_form() {
        let def = ScriptDef::from_json(
            r#"{"script": "beng", "block": ["0980", "09FF"], "replace": [["ত্‍", "ৎ"], ["র", "ৰ"]]}"#,
        )
        .unwrap();
        assert_eq!(def.normalize("উত\u{09CD}\u{200D}সৱ"), "উৎসৱ");
        assert_eq!(def.normalize("তোমার"), "তোমাৰ");
        assert_eq!(def.normalize("ক্\u{200C}ষ"), "ক্ষ");
    }
}

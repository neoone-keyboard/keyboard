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
//!   the engine's `normalize::normalize` (NFC, `’` to `'`, the joiner rules).
//! - `joiners`: `"afterVirama"` keeps a ZWJ or ZWNJ that stands between the `virama` and a
//!   character of the script (it changes how the cluster is drawn: Malayalam ഗ്‍ക, Devanagari's
//!   half forms and explicit halant); `"drop"`, the default, drops every joiner.
//! - `zwjForms`: `[consonant, letter]` code points. The consonant + `virama` + ZWJ is an older
//!   encoding of the letter and becomes it (Malayalam's chillus: ന + ് + ZWJ is ൻ).
//!   Both need `virama`. A file that has neither field, in a script the engine has its own
//!   rules for, gets that script's (`script::for_pack`): files were written before the fields.
//! - `phraseBreaks`: characters that end a phrase as a full stop does (the danda).
//! - `apostrophe`: an apostrophe may stand inside a word.
//! - `notLetter`: ranges that stay inside a run when text is split into words, but that a word
//!   is not made of (Malayalam's digits and number signs: its text has always been split so).
//! - `looseSigns`: vowel signs that loose spelling inserts between consonants. A word is also
//!   tried without one that stands before a consonant (`vowel_sign_drops`).
//! - `finalPairs`: `[a, b]` code points; a word that ends in a consonant and one of them is
//!   also written with the other (Malayalam's final ു and ്; `other_final_u`).
//! - `suffixJoins`: how a stem and a suffix of the pack's `suffixes.tsv` fuse ([`Joins`]).
//!   Without it the pack's words are never split into a stem and suffixes.
//! - `charTable` (engine 8): hex code point ranges, in order, listing the characters the
//!   script's words are made of: the romanised lexicon packs them one byte each and the
//!   transliterator spells in them (`packing`), up to `CharTable::MAX`. For a script whose
//!   letters span more than 128 code points (Urdu's, across the Arabic block). Without it
//!   both use the 128 code points from the script's first, as every pack before 8 does.
//!
//! A consonant, for the last three, is a `bases` letter that is not a combining mark.
//!
//! Other keys (the builder's `firstLetter`, `maxWords`, notes) are ignored here.

use std::ops::RangeInclusive;
use std::sync::Mutex;

use serde::Deserialize;

use super::ScriptRules;
use crate::packing::{CharTable, Packing};

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
    #[serde(default)]
    not_letter: Vec<[String; 2]>,
    #[serde(default)]
    loose_signs: Vec<[String; 2]>,
    #[serde(default)]
    final_pairs: Vec<[String; 2]>,
    #[serde(default)]
    suffix_joins: Option<RawJoins>,
    #[serde(default)]
    joiners: Option<String>,
    #[serde(default)]
    zwj_forms: Option<Vec<[String; 2]>>,
    #[serde(default)]
    char_table: Option<Vec<[String; 2]>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawJoins {
    #[serde(default)]
    vowel_signs: Vec<[String; 2]>,
    #[serde(default)]
    nasal: Option<[String; 2]>,
    #[serde(default)]
    final_forms: Vec<[String; 2]>,
    #[serde(default)]
    elide: Vec<String>,
    #[serde(default)]
    glides: Vec<RawGlide>,
    #[serde(default)]
    inherent_glide: Option<String>,
    #[serde(default)]
    consonant_after: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawGlide {
    glide: String,
    after: Vec<[String; 2]>,
}

/// `suffixJoins`: the sandhi between a stem and a suffix (`suffix::Join` says which kind a
/// suffix is). Every part is optional; code points are hex.
///
/// - `vowelSigns`: `[vowel, sign]`, the sign an independent vowel becomes on a consonant
///   (`""` for the vowel a consonant carries by itself). A suffix joined as a vowel or a case
///   ending must start with one of these.
/// - `nasal`: `[ending, consonant]`. A stem ending in `ending` takes a vowel suffix as the
///   consonant + the vowel's sign, takes no case ending, and loses the ending to an oblique
///   one, which only such a stem takes. `ending` is one code point (Malayalam's ം) or several
///   separated by spaces (Tamil's ம் is `"0BAE 0BCD"`; engine 6).
/// - `finalForms`: `[letter, consonant]`, a word-final form of a consonant (a chillu), which
///   becomes the consonant before a vowel.
/// - `elide`: final signs that give way to the suffix's vowel, after a consonant.
/// - `glides`: the consonant put between a stem ending in one of `after` (ranges) and a
///   vowel; `inherentGlide`: the one after a bare consonant.
/// - `consonantAfter`: finals, besides vowels and consonants, that take a suffix starting
///   with a consonant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Joins {
    pub vowel_signs: Vec<(char, Option<char>)>,
    pub nasal: Option<(String, char)>,
    pub final_forms: Vec<(char, char)>,
    pub elide: Vec<char>,
    pub glides: Vec<(char, Vec<(char, char)>)>,
    pub inherent_glide: Option<char>,
    pub consonant_after: Vec<char>,
}

fn pair(p: &[String; 2]) -> Result<(char, char), String> {
    Ok((code_point(&p[0])?, code_point(&p[1])?))
}

fn points(list: &[String]) -> Result<Vec<char>, String> {
    list.iter().map(|s| code_point(s)).collect()
}

impl Joins {
    fn from_raw(raw: &RawJoins) -> Result<Self, String> {
        Ok(Self {
            vowel_signs: raw
                .vowel_signs
                .iter()
                .map(|[vowel, sign]| {
                    let sign = if sign.is_empty() {
                        None
                    } else {
                        Some(code_point(sign)?)
                    };
                    Ok((code_point(vowel)?, sign))
                })
                .collect::<Result<_, String>>()?,
            nasal: raw
                .nasal
                .as_ref()
                .map(|[ending, consonant]| {
                    let ending: String = ending
                        .split_whitespace()
                        .map(code_point)
                        .collect::<Result<_, _>>()?;
                    if ending.is_empty() {
                        return Err("suffixJoins nasal has no ending".to_string());
                    }
                    Ok((ending, code_point(consonant)?))
                })
                .transpose()?,
            final_forms: raw
                .final_forms
                .iter()
                .map(pair)
                .collect::<Result<_, _>>()?,
            elide: points(&raw.elide)?,
            glides: raw
                .glides
                .iter()
                .map(|g| Ok((code_point(&g.glide)?, ranges(&g.after)?)))
                .collect::<Result<_, String>>()?,
            inherent_glide: raw
                .inherent_glide
                .as_deref()
                .map(code_point)
                .transpose()?,
            consonant_after: points(&raw.consonant_after)?,
        })
    }
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
    /// In a run when text is split, but not letters of a word.
    pub not_letter: Vec<(char, char)>,
    /// Vowel signs loose spelling inserts between consonants.
    pub loose_signs: Vec<(char, char)>,
    /// Two endings, after a consonant, of one word.
    pub final_pairs: Vec<(char, char)>,
    pub joins: Option<Joins>,
    /// What the normal form does with ZWJ and ZWNJ (`joiners`, `zwjForms`); None drops all.
    pub joiners: Option<crate::normalize::Joiners>,
    /// The file has `joiners` or `zwjForms` (see `script::for_pack`).
    pub says_joiners: bool,
    /// `charTable`: the characters words are packed and spelled in (`packing`).
    pub char_table: Option<&'static CharTable>,
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
        let virama = raw.virama.as_deref().map(code_point).transpose()?;
        let says_joiners = raw.joiners.is_some() || raw.zwj_forms.is_some();
        let keep = match raw.joiners.as_deref() {
            None | Some("drop") => false,
            Some("afterVirama") => true,
            Some(other) => return Err(format!("joiners is \"afterVirama\" or \"drop\", not {other:?}")),
        };
        let forms: Vec<(char, char)> = raw
            .zwj_forms
            .iter()
            .flatten()
            .map(pair)
            .collect::<Result<_, _>>()?;
        let joiners = match virama {
            Some(virama) if keep || !forms.is_empty() => Some(crate::normalize::Joiners {
                virama,
                ranges: chars.clone(),
                keep,
                forms,
            }),
            None if keep || !forms.is_empty() => {
                return Err("joiners and zwjForms need the script's virama".into());
            }
            _ => None,
        };
        let char_table = raw
            .char_table
            .as_ref()
            .map(|pairs| {
                let mut table = Vec::new();
                for &(a, b) in &ranges(pairs)? {
                    table.extend(a..=b);
                }
                if let Some(c) = table.iter().find(|&&c| !in_ranges(&chars, c)) {
                    return Err(format!(
                        "charTable has U+{:04X}, which is not a character of the script",
                        *c as u32
                    ));
                }
                CharTable::intern(table)
            })
            .transpose()?;
        Ok(Self {
            char_table,
            joiners,
            says_joiners,
            code: raw.script,
            ranges: chars,
            not_word: ranges(&raw.not_word)?,
            virama,
            signs: ranges(&raw.signs)?,
            bases: ranges(&raw.bases)?,
            apostrophe: raw.apostrophe,
            replace,
            phrase_breaks: raw
                .phrase_breaks
                .iter()
                .map(|s| single_char(s, "a phrase break"))
                .collect::<Result<_, _>>()?,
            not_letter: ranges(&raw.not_letter)?,
            loose_signs: ranges(&raw.loose_signs)?,
            final_pairs: raw
                .final_pairs
                .iter()
                .map(pair)
                .collect::<Result<_, _>>()?,
            joins: raw
                .suffix_joins
                .as_ref()
                .map(Joins::from_raw)
                .transpose()?,
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

    /// What a word of the script is made of: `is_word_char`, less `notLetter`.
    pub fn is_letter(&self, c: char) -> bool {
        self.is_word_char(c) && !in_ranges(&self.not_letter, c)
    }

    /// A base letter that is not a combining mark (a nukta is a base too).
    pub fn is_consonant(&self, c: char) -> bool {
        self.is_base(c) && !unicode_normalization::char::is_combining_mark(c)
    }

    /// Every spelling of `word` with one `looseSigns` sign dropped from before a consonant.
    pub fn vowel_sign_drops(&self, word: &str) -> Vec<String> {
        if self.loose_signs.is_empty() {
            return Vec::new();
        }
        let chars: Vec<char> = word.chars().collect();
        (0..chars.len())
            .filter(|&i| {
                in_ranges(&self.loose_signs, chars[i])
                    && chars.get(i + 1).is_some_and(|&c| self.is_consonant(c))
            })
            .map(|i| chars[..i].iter().chain(&chars[i + 1..]).collect())
            .collect()
    }

    /// `word` with the other ending of a `finalPairs` pair, when it ends in a consonant and
    /// one of the two.
    pub fn other_final(&self, word: &str) -> Option<String> {
        for &(a, b) in &self.final_pairs {
            for (from, to) in [(a, b), (b, a)] {
                if let Some(stem) = word.strip_suffix(from) {
                    return stem
                        .chars()
                        .last()
                        .is_some_and(|c| self.is_consonant(c))
                        .then(|| format!("{stem}{to}"));
                }
            }
        }
        None
    }

    /// The glide between a stem ending in `last` and a suffix starting with a vowel.
    fn glide(&self, joins: &Joins, last: char) -> Option<char> {
        joins
            .glides
            .iter()
            .find(|(_, after)| in_ranges(after, last))
            .map(|&(glide, _)| glide)
            .or_else(|| joins.inherent_glide.filter(|_| self.is_consonant(last)))
    }

    /// `stem` and `suffix` joined the way `how` says (`suffixJoins`), or None when they don't
    /// join or the script has no joins.
    pub fn join_suffix(
        &self,
        stem: &str,
        suffix: &str,
        how: crate::suffix::Join,
    ) -> Option<String> {
        use crate::suffix::Join;
        let joins = self.joins.as_ref()?;
        let last = stem.chars().last()?;
        let body = &stem[..stem.len() - last.len_utf8()];
        // The stem without its nasal ending, and the consonant the ending stands for.
        let nasal = joins.nasal.as_ref().and_then(|(ending, consonant)| {
            Some((stem.strip_suffix(ending.as_str())?, *consonant))
        });
        match how {
            Join::Consonant => {
                let first = suffix.chars().next()?;
                let joins_on = self.is_consonant(last)
                    || self.glide(joins, last).is_some()
                    || joins.consonant_after.contains(&last);
                (self.is_consonant(first) && joins_on).then(|| format!("{stem}{suffix}"))
            }
            Join::Oblique => nasal.map(|(body, _)| format!("{body}{suffix}")),
            Join::Vowel | Join::Case => {
                let mut chars = suffix.chars();
                let first = chars.next()?;
                let sign = joins.vowel_signs.iter().find(|&&(v, _)| v == first)?.1;
                let rest = chars.as_str();
                let sign: String = sign.into_iter().collect();
                if let Some((body, consonant)) = nasal {
                    return (how == Join::Vowel).then(|| format!("{body}{consonant}{sign}{rest}"));
                }
                if let Some(&(_, base)) = joins.final_forms.iter().find(|&&(f, _)| f == last) {
                    return Some(format!("{body}{base}{sign}{rest}"));
                }
                if joins.elide.contains(&last) {
                    return body
                        .chars()
                        .last()
                        .filter(|&c| self.is_consonant(c))
                        .map(|_| format!("{body}{sign}{rest}"));
                }
                let glide = self.glide(joins, last)?;
                Some(format!("{stem}{glide}{sign}{rest}"))
            }
        }
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

    /// The engine's normal form with this script's joiner rules in place of whichever are in
    /// use for its code, so a definition normalises the same installed or not (the builder
    /// never installs one).
    fn engine_normalize(&self, text: &str) -> String {
        super::plain_with(text, &self.code, self.joiners.as_ref())
    }

    /// The script's normal form of `text`: `replace`, the engine's `normalize`, `replace` again,
    /// and one more round when a replacement made text that NFC or the joiner rules change.
    pub fn normalize(&self, text: &str) -> String {
        if self.replace.is_empty() {
            // The engine's normal form is its own fixed point.
            return self.engine_normalize(text);
        }
        let before = self.apply_replace(&self.engine_normalize(&self.apply_replace(text)));
        let after = self.apply_replace(&self.engine_normalize(&before));
        if after == before { before } else { after }
    }

    /// How the script's words are packed and spelled: its `charTable`, else the 128 code
    /// points from its first.
    pub fn packing(&self) -> Packing {
        match self.char_table {
            Some(table) => Packing::Table(table),
            None => Packing::Block(*self.span().start()),
        }
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
        self.def.is_letter(c)
    }
    fn is_word_char(&self, c: char) -> bool {
        self.def.is_word_char(c)
    }
    fn normalize(&self, word: &str) -> String {
        self.def.normalize(word)
    }
    fn vowel_sign_drops(&self, word: &str) -> Vec<String> {
        self.def.vowel_sign_drops(word)
    }
    fn other_final_u(&self, word: &str) -> Option<String> {
        self.def.other_final(word)
    }
    fn join_suffix(&self, stem: &str, suffix: &str, how: crate::suffix::Join) -> Option<String> {
        self.def.join_suffix(stem, suffix, how)
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
    fn joiners(&self) -> Option<&crate::normalize::Joiners> {
        self.def.joiners.as_ref()
    }
    fn joins(&self) -> Option<&Joins> {
        self.def.joins.as_ref()
    }
    fn packing(&self) -> Packing {
        self.def.packing()
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

/// A script other than Malayalam's with its own sandhi: Tamil's, as the guide gives it.
#[cfg(test)]
pub(crate) const TAMIL: &str = r#"{
  "script": "taml", "block": ["0B80", "0BFF"], "virama": "0BCD", "bases": [["0B95", "0BB9"]],
  "suffixJoins": {
    "vowelSigns": [["0B85", ""], ["0B86", "0BBE"], ["0B87", "0BBF"], ["0B88", "0BC0"],
                   ["0B89", "0BC1"], ["0B8A", "0BC2"], ["0B8E", "0BC6"], ["0B8F", "0BC7"],
                   ["0B90", "0BC8"], ["0B92", "0BCA"], ["0B93", "0BCB"]],
    "nasal": ["0BAE 0BCD", "0BAE"],
    "elide": ["0BCD"],
    "glides": [
      {"glide": "0BB5", "after": [["0BBE", "0BBE"], ["0BC1", "0BC2"], ["0BCA", "0BCC"]]},
      {"glide": "0BAF", "after": [["0BBF", "0BC0"], ["0BC6", "0BC8"]]}
    ],
    "inherentGlide": "0BB5"
  }
}"#;

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
            (r#"{"script": "gujr", "block": ["0A80", "0AFF"], "joiners": "afterVirama"}"#, "need the script's virama"),
            (r#"{"script": "gujr", "block": ["0A80", "0AFF"], "virama": "0ACD", "joiners": "keep"}"#, "afterVirama"),
            (r#"{"script": "gujr", "block": ["0A80", "0AFF"], "zwjForms": [["0A95", "0A96"]]}"#, "need the script's virama"),
        ] {
            let err = ScriptDef::from_json(json).unwrap_err();
            assert!(err.contains(want), "{json}: {err}");
        }
    }

    #[test]
    fn another_scripts_suffixes_join_by_its_own_rules() {
        use crate::suffix::Join;
        let ta = ScriptDef::from_json(TAMIL).unwrap();
        let join = |stem, suffix, how| ta.join_suffix(stem, suffix, how);
        // A final pulli gives way to the suffix's vowel; a vowel takes its glide.
        assert_eq!(join("அவன்", "இடம்", Join::Case).as_deref(), Some("அவனிடம்"));
        assert_eq!(join("பள்ளி", "இல்", Join::Case).as_deref(), Some("பள்ளியில்"));
        assert_eq!(join("பூ", "இல்", Join::Case).as_deref(), Some("பூவில்"));
        assert_eq!(join("அம்மா", "ஆ", Join::Vowel).as_deref(), Some("அம்மாவா"));
        // ம் is two code points: it becomes ம before a clitic, takes no case ending, and
        // gives way to the oblique one.
        assert_eq!(join("காலம்", "ஆ", Join::Vowel).as_deref(), Some("காலமா"));
        assert_eq!(join("காலம்", "இல்", Join::Case), None);
        assert_eq!(join("காலம்", "த்தில்", Join::Oblique).as_deref(), Some("காலத்தில்"));
        assert_eq!(join("பள்ளி", "த்தில்", Join::Oblique), None);
        assert_eq!(join("பள்ளி", "க்கு", Join::Consonant).as_deref(), Some("பள்ளிக்கு"));
        assert_eq!(join("அவன்", "க்கு", Join::Consonant), None, "அவனுக்கு, not அவன்க்கு");
        let bad = TAMIL.replace("0BAE 0BCD", " ");
        assert!(ScriptDef::from_json(&bad).unwrap_err().contains("no ending"));
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

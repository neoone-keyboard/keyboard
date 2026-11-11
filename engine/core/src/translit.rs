//! Rule-based Latin → native script, Mozhi-style, for names and words that aren't in the
//! lexicon when the pack has no transliterator model to ask. Deterministic and approximate: it
//! can't know vowel length the user didn't type, so it only ever fills a side slot.
//!
//! The rules are a pack's data: `romanisation.json`'s optional `fromLatin` section
//! ([`RawFromLatin`]). Malayalam's file holds the tables and habits this module had as code
//! for Malayalam alone. A pack without the section gets a speller derived from its other
//! tables ([`Speller::derived`]): each Latin spelling of a consonant or vowel, turned round.

use std::collections::{BTreeMap, HashMap};

use serde::Deserialize;

/// `fromLatin`: how typed Latin letters are spelled in the script, read left to right. At each
/// position the first `vowels` entry that matches is taken, else the first `consonants` entry,
/// so longer spellings go first (`chh` before `ch` before `c`). A consonant is followed by a
/// virama unless a vowel comes next. Everything but the two tables is optional:
///
/// - `consonants`: `[latin, native]`; `native` may be a cluster (`nt` → ന്റ).
/// - `vowels`: `[latin, independent, sign]`: the letter at the start of a word or after a
///   vowel, and the sign on a consonant (`""` for the vowel a consonant carries by itself).
/// - `finalForms`: `[native, final]`: a word-final consonant's own form in place of the
///   consonant and its virama (Malayalam's chillus and anusvara: ന → ൻ, മ → ം).
/// - `bareFinal`: a word-final consonant with no final form takes no virama (Hindi कमल).
/// - `longBeforeOpenSyllable`: `signs` are `[latin, sign]`, the sign a short vowel takes
///   instead when one single-letter consonant and then a vowel or the word's end follow
///   (names spell long ഏ and ഓ so: sneha, mohan); never before a `notBefore` spelling.
/// - `finalVowels`: `[latin, sign]`: a consonant spelling that, last in the word and after a
///   consonant, is this vowel sign (revathy → രേവതി).
/// - `fused`: a consonant spelling and the vowel after it that become one sign on the
///   consonant before (`r` + `i` after `k` is ൃ: krishna → കൃഷ്ണ), when that consonant's
///   spelling is in `after` and the text doesn't go on with `unless` (`ii`).
/// - `afterConsonant`: a consonant spelled otherwise straight after one of `after`
///   (`n` after `sh` or `r` is ണ: jishnu, karnan).
/// - `betweenVowels`: `[latin, native]`: a consonant spelled otherwise between two vowels
///   (a lone `ch` is ച്ച: kochi).
/// - `beforeConsonant`: a consonant that takes `native` in place of itself and a virama when
///   another consonant follows, unless that one starts with a `notBefore` letter
///   (`r` is the chillu ർ: arjun; not before യ or ര).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawFromLatin {
    consonants: Vec<(String, String)>,
    vowels: Vec<(String, String, String)>,
    #[serde(default)]
    final_forms: Vec<(String, String)>,
    #[serde(default)]
    bare_final: bool,
    #[serde(default)]
    long_before_open_syllable: Option<RawOpen>,
    #[serde(default)]
    final_vowels: Vec<(String, String)>,
    #[serde(default)]
    fused: Vec<Fused>,
    #[serde(default)]
    after_consonant: Vec<AfterConsonant>,
    #[serde(default)]
    between_vowels: Vec<(String, String)>,
    #[serde(default)]
    before_consonant: Vec<BeforeConsonant>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOpen {
    signs: Vec<(String, String)>,
    #[serde(default)]
    not_before: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Fused {
    consonant: String,
    vowel: String,
    #[serde(default)]
    unless: String,
    after: Vec<String>,
    sign: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct AfterConsonant {
    consonant: String,
    after: Vec<String>,
    native: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BeforeConsonant {
    consonant: String,
    native: String,
    #[serde(default)]
    not_before: Vec<String>,
}

/// A pack's Latin → native speller.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Speller {
    virama: String,
    consonants: Vec<(String, String)>,
    vowels: Vec<(String, String, String)>,
    final_forms: Vec<(String, String)>,
    bare_final: bool,
    open_signs: Vec<(String, String)>,
    open_not_before: Vec<String>,
    final_vowels: Vec<(String, String)>,
    fused: Vec<Fused>,
    after_consonant: Vec<AfterConsonant>,
    between_vowels: Vec<(String, String)>,
    before_consonant: Vec<BeforeConsonant>,
}

fn is_latin(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase())
}

impl Speller {
    /// The speller a `fromLatin` section describes, for a script whose virama is `virama`.
    pub fn from_raw(raw: RawFromLatin, virama: &str) -> Result<Self, String> {
        let open = raw.long_before_open_syllable;
        let (open_signs, open_not_before) = open.map_or_else(Default::default, |o| (o.signs, o.not_before));
        let latin = raw
            .consonants
            .iter()
            .map(|(l, _)| l)
            .chain(raw.vowels.iter().map(|(l, _, _)| l))
            .chain(open_signs.iter().map(|(l, _)| l))
            .chain(raw.final_vowels.iter().map(|(l, _)| l))
            .chain(raw.between_vowels.iter().map(|(l, _)| l));
        if let Some(bad) = latin.into_iter().find(|l| !is_latin(l)) {
            return Err(format!("fromLatin spelling {bad:?}: lower-case a–z"));
        }
        if raw.consonants.iter().any(|(_, native)| native.is_empty()) {
            return Err("a fromLatin consonant has no native letter".into());
        }
        if raw.fused.iter().any(|f| f.vowel.is_empty()) {
            return Err("a fromLatin fused rule has no vowel".into());
        }
        Ok(Self {
            virama: virama.to_string(),
            consonants: raw.consonants,
            vowels: raw.vowels,
            final_forms: raw.final_forms,
            bare_final: raw.bare_final,
            open_signs,
            open_not_before,
            final_vowels: raw.final_vowels,
            fused: raw.fused,
            after_consonant: raw.after_consonant,
            between_vowels: raw.between_vowels,
            before_consonant: raw.before_consonant,
        })
    }

    /// A speller from a pack's walk tables turned round, for a pack with no `fromLatin`.
    ///
    /// `consonants` are the pack's `clusters` and `consonants` tables, `independent` and
    /// `signs` its vowel tables, each native key → canonical Latin; `inherent` is the Latin
    /// of the vowel a consonant carries. Several letters share a Latin spelling (Hindi त and
    /// ट are both `t`), and the tables don't say which is commoner: the shortest key, then the
    /// lowest code point, is taken. A Latin letter that is no spelling by itself borrows a letter
    /// the skeleton calls the same (`singles`): the one it folds to (`c` as `k`), else one
    /// that folds to it (Tamil's `t` from `d`, as ட is spelled `d`). `bare_final`: the script
    /// writes a word-final consonant with nothing after it.
    pub fn derived(
        virama: &str,
        consonants: &[(String, String)],
        independent: &[(String, String)],
        signs: &[(String, String)],
        inherent: &str,
        singles: &[(u8, Vec<u8>)],
        bare_final: bool,
    ) -> Self {
        // Latin → the native key to spell it with: the shortest, then the lowest.
        let pick = |table: &[(String, String)]| -> BTreeMap<String, String> {
            let mut out: BTreeMap<String, String> = BTreeMap::new();
            for (native, latin) in table {
                if !is_latin(latin) {
                    continue;
                }
                let better = out.get(latin).is_none_or(|have| {
                    (native.chars().count(), native.as_str()) < (have.chars().count(), have.as_str())
                });
                if better {
                    out.insert(latin.clone(), native.clone());
                }
            }
            out
        };
        let longest_first = |mut rows: Vec<(String, String)>| {
            rows.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
            rows
        };
        let independent = pick(independent);
        let mut sign_of = pick(signs);
        if is_latin(inherent) {
            // The consonant's own vowel needs no sign.
            sign_of.insert(inherent.to_string(), String::new());
        }
        // Every Latin spelling of a vowel, as a letter or as a sign. One that only a sign has
        // (Assamese া is `a`, আ is `aa`) takes the letter whose spelling begins with it; one
        // that only a letter has takes the sign of its longest beginning, else the letter.
        let mut vowels: Vec<(String, String, String)> = Vec::new();
        let spellings: std::collections::BTreeSet<&String> = independent.keys().chain(sign_of.keys()).collect();
        for latin in spellings {
            let letter = independent.get(latin).or_else(|| {
                independent
                    .iter()
                    .find(|(l, _)| l.starts_with(latin.as_str()))
                    .map(|(_, native)| native)
            });
            let Some(letter) = letter else { continue };
            let sign = sign_of
                .get(latin)
                .or_else(|| {
                    sign_of
                        .iter()
                        .filter(|(l, _)| latin.starts_with(l.as_str()))
                        .max_by_key(|(l, _)| l.len())
                        .map(|(_, sign)| sign)
                })
                .unwrap_or(letter);
            vowels.push((latin.clone(), letter.clone(), sign.clone()));
        }
        vowels.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        let vowel_at = |s: &str| vowels.iter().any(|(v, _, _)| s.starts_with(v.as_str()));
        let mut picked = pick(consonants);
        // A spelling a vowel would always take first is never reached; drop it.
        picked.retain(|latin, _| !vowel_at(latin));
        let singles: Vec<(String, String)> = singles
            .iter()
            .map(|(l, s)| ((*l as char).to_string(), String::from_utf8_lossy(s).into_owned()))
            .collect();
        let folds = singles.iter().map(|(a, b)| (a, b));
        let unfolds = singles.iter().map(|(a, b)| (b, a));
        for (from, to) in folds.chain(unfolds) {
            if from.len() == 1
                && !picked.contains_key(from)
                && !vowel_at(from)
                && let Some(native) = picked.get(to).cloned()
            {
                picked.insert(from.clone(), native);
            }
        }
        Self {
            virama: virama.to_string(),
            consonants: longest_first(picked.into_iter().collect()),
            vowels,
            bare_final,
            ..Self::default()
        }
    }

    fn vowel_at(&self, s: &str) -> Option<&(String, String, String)> {
        self.vowels.iter().find(|(v, _, _)| s.starts_with(v.as_str()))
    }

    fn consonant_at(&self, s: &str) -> Option<&(String, String)> {
        self.consonants.iter().find(|(c, _)| s.starts_with(c.as_str()))
    }

    /// True when `rest` (what follows a typed vowel) is one plain consonant and then a vowel
    /// or the end of the word.
    fn open_syllable_follows(&self, rest: &str) -> bool {
        let Some((spelling, letter)) = self.consonant_at(rest) else {
            return false;
        };
        let after = &rest[spelling.len()..];
        !self.open_not_before.contains(spelling)
            && letter.chars().count() == 1
            && (after.is_empty() || self.vowel_at(after).is_some())
    }

    /// `latin` spelled in the script. None unless it is all ASCII letters and every one of
    /// them starts a spelling the tables have.
    pub fn spell(&self, latin: &str) -> Option<String> {
        if latin.is_empty() || !latin.bytes().all(|b| b.is_ascii_alphabetic()) {
            return None;
        }
        let lower = latin.to_ascii_lowercase();
        let mut rest = lower.as_str();
        let mut out = String::new();
        // The Latin spelling and output offset of a consonant still waiting for its vowel.
        let mut pending: Option<(&str, usize)> = None;
        while !rest.is_empty() {
            if let Some((spelling, independent, sign)) = self.vowel_at(rest) {
                let after = &rest[spelling.len()..];
                let sign = self
                    .open_signs
                    .iter()
                    .find(|(v, _)| v == spelling)
                    .filter(|_| self.open_syllable_follows(after))
                    .map_or(sign, |(_, long)| long);
                out.push_str(if pending.is_some() { sign } else { independent });
                pending = None;
                rest = after;
                continue;
            }
            let (spelling, letter) = self.consonant_at(rest)?;
            let mut letter = letter.as_str();
            let after = &rest[spelling.len()..];
            let previous = pending.map(|(s, _)| s);
            if after.is_empty()
                && pending.is_some()
                && let Some((_, sign)) = self.final_vowels.iter().find(|(c, _)| c == spelling)
            {
                out.push_str(sign);
                pending = None;
                break;
            }
            if let Some(f) = self.fused.iter().find(|f| {
                f.consonant == *spelling
                    && after.starts_with(f.vowel.as_str())
                    && (f.unless.is_empty() || !after.starts_with(f.unless.as_str()))
                    && previous.is_some_and(|p| f.after.iter().any(|a| a == p))
            }) {
                out.push_str(&f.sign);
                pending = None;
                rest = &after[f.vowel.len()..];
                continue;
            }
            if let Some(a) = self.after_consonant.iter().find(|a| {
                a.consonant == *spelling && previous.is_some_and(|p| a.after.iter().any(|x| x == p))
            }) {
                letter = &a.native;
            } else if pending.is_none()
                && !out.is_empty()
                && self.vowel_at(after).is_some()
                && let Some((_, native)) = self.between_vowels.iter().find(|(c, _)| c == spelling)
            {
                letter = native;
            }
            if let Some((p, start)) = pending {
                let form = self.before_consonant.iter().find(|b| {
                    b.consonant == p && !b.not_before.iter().any(|x| letter.starts_with(x.as_str()))
                });
                match form {
                    Some(b) => out.replace_range(start.., &b.native),
                    None => out.push_str(&self.virama),
                }
            }
            let start = out.len();
            out.push_str(letter);
            pending = Some((spelling.as_str(), start));
            rest = after;
        }
        if let Some((_, start)) = pending {
            match self.final_forms.iter().find(|(native, _)| *native == out[start..]) {
                Some((_, form)) => out.replace_range(start.., form),
                None if !self.bare_final => out.push_str(&self.virama),
                None => {}
            }
        }
        Some(out)
    }
}

/// The `fromLatin` of the packs published before the section, by script: Malayalam's, from its
/// pack's file, for a `mlym` pack whose `romanisation.json` has none (its fallback spelling was
/// this module's code then, and is the same now).
const EMBEDDED: [(&str, &str); 1] = [(
    "mlym",
    include_str!("../../../packs/ml.neopack/romanisation.json"),
)];

/// The engine's own speller for `script`, if it has one.
pub fn embedded(script: &str) -> Option<Speller> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct File {
        virama: String,
        from_latin: RawFromLatin,
    }
    let (_, json) = EMBEDDED.iter().find(|(code, _)| *code == script)?;
    let file: File = serde_json::from_str(json)
        .unwrap_or_else(|e| panic!("the engine's fromLatin for {script}: {e}"));
    let virama: String = unicode_normalization::UnicodeNormalization::nfc(file.virama.as_str()).collect();
    Some(
        Speller::from_raw(file.from_latin, &virama)
            .unwrap_or_else(|e| panic!("the engine's fromLatin for {script}: {e}")),
    )
}

/// Rows of a `HashMap` table in a fixed order (the map's is not).
pub(crate) fn rows(table: &HashMap<Vec<char>, String>) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = table
        .iter()
        .map(|(native, latin)| (native.iter().collect(), latin.clone()))
        .collect();
    rows.sort();
    rows
}

#[cfg(test)]
mod tests {
    use crate::romanisation::{Romanisation, ml};

    fn t(latin: &str) -> String {
        ml().spell(latin).unwrap()
    }

    #[test]
    fn common_words() {
        assert_eq!(t("amma"), "അമ്മ");
        assert_eq!(t("avan"), "അവൻ");
        assert_eq!(t("kutti"), "കുട്ടി");
        assert_eq!(t("ente"), "എന്റെ");
        assert_eq!(t("maram"), "മരം");
        assert_eq!(t("paal"), "പാൽ");
        // Really വീട്; the lexicon has it. `d` is ദ because names want it.
        assert_eq!(t("veedu"), "വീദു");
        assert_eq!(t("vannu"), "വന്നു");
    }

    #[test]
    fn names() {
        assert_eq!(t("Anu"), "അനു");
        assert_eq!(t("Kochi"), "കൊച്ചി");
        assert_eq!(t("Thomas"), "തോമസ്");
    }

    /// What a Malayali expects from common names typed Mozhi-style.
    #[test]
    fn names_as_malayalis_spell_them() {
        assert_eq!(t("jishnu"), "ജിഷ്ണു");
        assert_eq!(t("anu"), "അനു");
        assert_eq!(t("kochi"), "കൊച്ചി");
        assert_eq!(t("thiruvananthapuram"), "തിരുവനന്തപുരം");
        assert_eq!(t("meera"), "മീര");
        assert_eq!(t("krishna"), "കൃഷ്ണ");
        assert_eq!(t("arjun"), "അർജുൻ");
        assert_eq!(t("deepa"), "ദീപ");
        assert_eq!(t("sneha"), "സ്നേഹ");
        assert_eq!(t("suresh"), "സുരേഷ്");
        assert_eq!(t("santhosh"), "സന്തോഷ്");
        assert_eq!(t("mohan"), "മോഹൻ");
        assert_eq!(t("revathy"), "രേവതി");
        assert_eq!(t("priya"), "പ്രിയ");
        assert_eq!(t("lakshmi"), "ലക്ഷ്മി");
        assert_eq!(t("sindhu"), "സിന്ധു");
        assert_eq!(t("panchami"), "പഞ്ചമി");
        assert_eq!(t("karnan"), "കർണൻ");
        assert_eq!(t("radha"), "രധ"); // vowel length: see below
        // Known misses. Simple rules can't tell these apart, and they are left
        // to the lexicon:
        // - untyped vowel length: rahul → രഹുൽ (രാഹുൽ), radha → രധ (രാധ);
        // - ന/ണ and ല/ള: venu → വേനു (വേണു), kannan → കന്നൻ (കണ്ണൻ),
        //   keralam → കേരലം (കേരളം);
        // - ശ vs ഷ (`sh` is ഷ): shankar → ഷങ്കർ (ശങ്കർ);
        // - short e/o before one consonant: koduthu → കോദുതു (കൊടുത്തു),
        //   chechi → ചെച്ചി (ചേച്ചി);
        // - ട vs ദ, and a spoken final u: veedu → വീദു (വീട്);
        // - ശ്ശ and long vowels: thrissur → തൃസ്സുർ (തൃശ്ശൂർ).
        assert_eq!(t("rahul"), "രഹുൽ");
        assert_eq!(t("shankar"), "ഷങ്കർ");
    }

    #[test]
    fn rejects_non_letters() {
        assert_eq!(ml().spell(""), None);
        assert_eq!(ml().spell("don't"), None);
        assert_eq!(ml().spell("abc1"), None);
        assert_eq!(ml().spell("എ"), None);
    }

    #[test]
    fn a_malayalam_pack_without_the_section_gets_the_engines() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packs/ml.neopack/romanisation.json");
        let mut file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(file.as_object_mut().unwrap().remove("fromLatin").is_some());
        let old = Romanisation::parse(&file.to_string()).unwrap();
        for word in ["jishnu", "krishna", "kochi", "revathy", "arjun", "thiruvananthapuram"] {
            assert_eq!(old.spell(word), ml().spell(word), "{word}");
        }
        assert_eq!(old.spell("mohan").as_deref(), Some("മോഹൻ"));
    }

    fn pack(id: &str) -> Romanisation {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../packs/{id}.neopack/romanisation.json"));
        Romanisation::load(&path).unwrap()
    }

    /// A pack with no `fromLatin` spells from its own tables turned round: rough, but in its
    /// own script and by its own letters.
    #[test]
    fn a_pack_without_the_section_spells_from_its_tables() {
        let ta = pack("ta");
        assert_eq!(ta.spell("amma").as_deref(), Some("அம்ம"));
        assert_eq!(ta.spell("kal").as_deref(), Some("கல்"), "a final consonant takes the pulli");
        assert_eq!(ta.spell("paadam").as_deref(), Some("பாடம்"));
        // ட is spelled `d`, and the skeleton says t and d are one: `t` borrows it. The tables
        // can't say that த is the commoner t (தமிழ்).
        assert_eq!(ta.spell("tamil").as_deref(), Some("டமில்"));
        let hi = pack("hi");
        assert_eq!(hi.spell("kamal").as_deref(), Some("कमल"), "Hindi's final consonant is bare");
        // Rough: त and ट are both `t` and ट is the lower code point; ा and the inherent
        // vowel are both `a`.
        assert_eq!(hi.spell("kitab").as_deref(), Some("किटब"));
        let as_ = pack("as");
        let spelled = as_.spell("manuh").unwrap();
        assert!(spelled.chars().all(|c| ('\u{0980}'..='\u{09FF}').contains(&c)), "{spelled}");
        for r in [&ta, &hi, &as_] {
            assert_eq!(r.spell("don't"), None);
        }
    }

    #[test]
    fn a_section_is_checked() {
        let with = |section: &str| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../packs/hi.neopack/romanisation.json");
            let mut file: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            file.as_object_mut()
                .unwrap()
                .insert("fromLatin".into(), serde_json::from_str(section).unwrap());
            Romanisation::parse(&file.to_string())
        };
        let hi = with(
            r#"{"consonants": [["kh", "ख"], ["k", "क"], ["t", "त"], ["b", "ब"], ["m", "म"], ["l", "ल"]],
                "vowels": [["aa", "आ", "ा"], ["a", "अ", ""], ["i", "इ", "ि"]], "bareFinal": true}"#,
        )
        .unwrap();
        assert_eq!(hi.spell("kitaab").as_deref(), Some("किताब"));
        assert_eq!(hi.spell("kamal").as_deref(), Some("कमल"));
        assert_eq!(hi.spell("khat").as_deref(), Some("खत"));
        assert_eq!(hi.spell("zoo"), None, "no spelling starts with z");
        assert!(with(r#"{"consonants": [["K", "क"]], "vowels": []}"#).is_err());
        assert!(with(r#"{"consonants": [["k", ""]], "vowels": []}"#).is_err());
        assert!(with(r#"{"vowels": []}"#).is_err());
    }

    /// The Malayalam speller as it was when it was code: the tables and the habits, word for
    /// word. Malayalam's `fromLatin` must spell every word as this does.
    mod coded {
        const VIRAMA: char = '\u{0D4D}';
        const VOCALIC_R_SIGN: char = '\u{0D43}';
        const I_SIGN: &str = "\u{0D3F}";

        const CONSONANTS: &[(&str, &str)] = &[
            ("chh", "ഛ"),
            ("nth", "ന്ത"),
            ("ndh", "ന്ധ"),
            ("nch", "ഞ്ച"),
            ("nj", "ഞ"),
            ("ng", "ങ"),
            ("nk", "ങ്ക"),
            ("nt", "ന്റ"),
            ("nd", "ന്ദ"),
            ("ch", "ച"),
            ("kh", "ഖ"),
            ("gh", "ഘ"),
            ("jh", "ഝ"),
            ("th", "ത"),
            ("dh", "ധ"),
            ("ph", "ഫ"),
            ("bh", "ഭ"),
            ("sh", "ഷ"),
            ("zh", "ഴ"),
            ("k", "ക"),
            ("g", "ഗ"),
            ("c", "ക"),
            ("q", "ക"),
            ("j", "ജ"),
            ("t", "ട"),
            ("d", "ദ"),
            ("n", "ന"),
            ("p", "പ"),
            ("f", "ഫ"),
            ("b", "ബ"),
            ("m", "മ"),
            ("y", "യ"),
            ("r", "ര"),
            ("l", "ല"),
            ("v", "വ"),
            ("w", "വ"),
            ("s", "സ"),
            ("z", "സ"),
            ("h", "ഹ"),
            ("x", "ക്സ"),
        ];

        const VOWELS: &[(&str, &str, &str)] = &[
            ("aa", "ആ", "\u{0D3E}"),
            ("ai", "ഐ", "\u{0D48}"),
            ("au", "ഔ", "\u{0D57}"),
            ("ee", "ഈ", "\u{0D40}"),
            ("ii", "ഈ", "\u{0D40}"),
            ("oo", "ഊ", "\u{0D42}"),
            ("uu", "ഊ", "\u{0D42}"),
            ("a", "അ", ""),
            ("i", "ഇ", "\u{0D3F}"),
            ("u", "ഉ", "\u{0D41}"),
            ("e", "എ", "\u{0D46}"),
            ("o", "ഒ", "\u{0D4A}"),
        ];

        const VOCALIC_R_AFTER: &[&str] = &["k", "h", "m", "v", "d", "th"];

        type Vowel = (&'static str, &'static str, &'static str);
        type Consonant = (&'static str, &'static str);

        fn vowel_at(s: &str) -> Option<&'static Vowel> {
            VOWELS.iter().find(|(v, _, _)| s.starts_with(v))
        }

        fn consonant_at(s: &str) -> Option<&'static Consonant> {
            CONSONANTS.iter().find(|(c, _)| s.starts_with(c))
        }

        fn final_form(letter: &str) -> Option<&'static str> {
            match letter {
                "ന" => Some("ൻ"),
                "ണ" => Some("ൺ"),
                "ല" => Some("ൽ"),
                "ര" => Some("ർ"),
                "മ" => Some("ം"),
                _ => None,
            }
        }

        fn open_syllable_follows(rest: &str) -> bool {
            let Some(&(spelling, letter)) = consonant_at(rest) else {
                return false;
            };
            let after = &rest[spelling.len()..];
            spelling != "ch"
                && letter.chars().count() == 1
                && (after.is_empty() || vowel_at(after).is_some())
        }

        pub fn transliterate(latin: &str) -> Option<String> {
            if latin.is_empty() || !latin.bytes().all(|b| b.is_ascii_alphabetic()) {
                return None;
            }
            let lower = latin.to_ascii_lowercase();
            let mut rest = lower.as_str();
            let mut out = String::new();
            let mut pending: Option<(&str, usize)> = None;
            while !rest.is_empty() {
                if let Some(&(spelling, independent, sign)) = vowel_at(rest) {
                    let after = &rest[spelling.len()..];
                    let sign = match spelling {
                        "e" if open_syllable_follows(after) => "\u{0D47}",
                        "o" if open_syllable_follows(after) => "\u{0D4B}",
                        _ => sign,
                    };
                    out.push_str(if pending.is_some() { sign } else { independent });
                    pending = None;
                    rest = after;
                    continue;
                }
                let &(spelling, mut letter) = consonant_at(rest)?;
                let after = &rest[spelling.len()..];
                let previous = pending.map(|(s, _)| s);
                if spelling == "y" && after.is_empty() && pending.is_some() {
                    out.push_str(I_SIGN);
                    pending = None;
                    break;
                }
                if spelling == "r"
                    && after.starts_with('i')
                    && !after.starts_with("ii")
                    && previous.is_some_and(|p| VOCALIC_R_AFTER.contains(&p))
                {
                    out.push(VOCALIC_R_SIGN);
                    pending = None;
                    rest = &after[1..];
                    continue;
                }
                match spelling {
                    "n" if matches!(previous, Some("sh" | "r")) => letter = "ണ",
                    "ch" if pending.is_none() && !out.is_empty() && vowel_at(after).is_some() => {
                        letter = "ച്ച"
                    }
                    _ => {}
                }
                if let Some((p, start)) = pending {
                    if p == "r" && !letter.starts_with(['ര', 'യ']) {
                        out.replace_range(start.., "ർ");
                    } else {
                        out.push(VIRAMA);
                    }
                }
                let start = out.len();
                out.push_str(letter);
                pending = Some((spelling, start));
                rest = after;
            }
            if let Some((_, start)) = pending {
                match final_form(&out[start..]) {
                    Some(chillu) => out.replace_range(start.., chillu),
                    None => out.push(VIRAMA),
                }
            }
            Some(out)
        }
    }

    /// Every typed spelling the eval sets hold, spelled by the pack's data as the code did.
    #[test]
    fn malayalams_section_spells_the_eval_words_as_the_code_did() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/eval");
        let mut words = 0;
        for name in ["translit-human.tsv", "chat-golden.tsv", "next-manglish.tsv", "english-sentences.txt"] {
            let text = std::fs::read_to_string(root.join(name)).unwrap();
            for word in text.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| !w.is_empty()) {
                assert_eq!(ml().spell(word), coded::transliterate(word), "{word}");
                words += 1;
            }
        }
        assert!(words > 10_000, "{words}");
    }

    proptest::proptest! {
        #[test]
        fn letters_always_transliterate(s in "[a-zA-Z]{1,20}") {
            let out = ml().spell(&s).unwrap();
            let all_malayalam = out.chars().all(|c| ('\u{0D00}'..='\u{0D7F}').contains(&c));
            proptest::prop_assert!(all_malayalam, "{}", out);
        }

        #[test]
        fn malayalams_section_spells_as_the_code_did(s in "[a-zA-Z]{1,24}") {
            proptest::prop_assert_eq!(ml().spell(&s), coded::transliterate(&s));
        }

        /// Syllable-shaped strings reach the habits (kri, sh + n, r + consonant, a final y)
        /// far more often than random letters do.
        #[test]
        fn malayalams_section_spells_syllables_as_the_code_did(
            s in "((ch|sh|th|kh|nt|nd|nj|ng|nk|n|r|k|m|v|d|h|y|l|p|s|c|x|q|w|z|f|b|g|j|t)?(aa|ai|au|ee|ii|oo|uu|a|i|u|e|o)?){1,8}"
        ) {
            proptest::prop_assume!(!s.is_empty());
            proptest::prop_assert_eq!(ml().spell(&s), coded::transliterate(&s));
        }
    }
}

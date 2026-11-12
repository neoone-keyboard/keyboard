//! Romaji to kana: the composer of a phrase-conversion pack (Japanese), driven by its
//! `kana.json` (the `kana` role). The table maps typed Latin to kana; the composer reads typed
//! letters one at a time as input methods do:
//!
//! - it waits while the letters typed so far could still become a longer rule (`k`, `ky`,
//!   `n`, which could be `na`);
//! - a rule that nothing longer can follow is written at once, and its `left` letters stay
//!   typed (`kk` gives っ and leaves `k`, so `kka` is っか);
//! - letters that no rule continues are written by the longest rule they hold (`n` then `k`
//!   gives ん and starts again at `k`), else kept as typed;
//! - at the end (`finish`), letters still waiting are written by their rule (`n` → ん) or
//!   kept as typed.
//!
//! Upper case letters are read as lower case. Characters no rule starts with (digits, `@`)
//! pass through as typed.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::Deserialize;

/// A `kana.json`: rules of typed Latin → kana.
#[derive(Debug, Clone)]
pub struct KanaTable {
    /// typed → (kana, letters left typed).
    rules: HashMap<String, (String, String)>,
    /// Every proper prefix of a rule's typed letters.
    prefixes: HashSet<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    format: u32,
    #[serde(default)]
    #[allow(dead_code)]
    notes: Option<String>,
    rules: Vec<Vec<String>>,
}

impl KanaTable {
    /// The table in `kana.json` at `path`.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text)
    }

    /// The table in `text`, a `kana.json`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: Raw = serde_json::from_str(text).map_err(|e| format!("kana.json: {e}"))?;
        if raw.format != 1 {
            return Err(format!(
                "kana.json: format {}, this engine reads 1",
                raw.format
            ));
        }
        let mut rules = HashMap::new();
        let mut prefixes = HashSet::new();
        for rule in raw.rules {
            let (typed, kana, left) = match rule.as_slice() {
                [t, k] => (t.clone(), k.clone(), String::new()),
                [t, k, l] => (t.clone(), k.clone(), l.clone()),
                _ => {
                    return Err(format!(
                        "kana.json: a rule is [typed, kana] or [typed, kana, left]: {rule:?}"
                    ));
                }
            };
            if typed.is_empty() {
                return Err("kana.json: a rule with nothing typed".into());
            }
            if typed != typed.to_lowercase() {
                return Err(format!(
                    "kana.json: {typed:?} isn't lower case (typed letters are read as lower case)"
                ));
            }
            if left.chars().count() >= typed.chars().count() {
                return Err(format!(
                    "kana.json: {typed:?} leaves {left:?}, which is not shorter"
                ));
            }
            let chars: Vec<char> = typed.chars().collect();
            for end in 1..chars.len() {
                prefixes.insert(chars[..end].iter().collect());
            }
            if rules.insert(typed.clone(), (kana, left)).is_some() {
                return Err(format!("kana.json: {typed:?} has two rules"));
            }
        }
        Ok(Self { rules, prefixes })
    }

    /// The kana of `typed` read in one go, and the letters still waiting at the end (which
    /// the app shows as typed): `kyouha` → (きょうは, ""), `nihonn` → (にほん, ""), `kak` →
    /// (か, "k"). `finish` writes what waits too (a final `n` as ん).
    pub fn compose(&self, typed: &str, finish: bool) -> (String, String) {
        let mut composer = Composer::new(self);
        for c in typed.chars() {
            composer.push(c);
        }
        if finish {
            composer.finish();
        }
        (composer.kana().to_string(), composer.pending().to_string())
    }
}

/// Composing text being typed: kana written so far and the letters waiting for more.
#[derive(Debug, Clone)]
pub struct Composer<'t> {
    table: &'t KanaTable,
    kana: String,
    pending: String,
}

impl<'t> Composer<'t> {
    pub fn new(table: &'t KanaTable) -> Self {
        Self {
            table,
            kana: String::new(),
            pending: String::new(),
        }
    }

    /// The kana written so far.
    pub fn kana(&self) -> &str {
        &self.kana
    }

    /// The letters waiting for more (shown as typed, after the kana).
    pub fn pending(&self) -> &str {
        &self.pending
    }

    /// What the app shows as composing text: the kana, then the waiting letters.
    pub fn text(&self) -> String {
        format!("{}{}", self.kana, self.pending)
    }

    pub fn is_empty(&self) -> bool {
        self.kana.is_empty() && self.pending.is_empty()
    }

    /// Clears the composition.
    pub fn clear(&mut self) {
        self.kana.clear();
        self.pending.clear();
    }

    /// Types `c`.
    pub fn push(&mut self, c: char) {
        let c = c.to_ascii_lowercase();
        let t = self.table;
        // Each turn either keeps `c` waiting, writes it, or shortens `pending`; so this ends.
        loop {
            let mut typed = self.pending.clone();
            typed.push(c);
            if t.prefixes.contains(&typed) {
                self.pending = typed;
                return;
            }
            if let Some((kana, left)) = t.rules.get(&typed) {
                self.kana.push_str(kana);
                self.pending = left.clone();
                return;
            }
            if self.pending.is_empty() {
                // No rule starts with c.
                self.kana.push(c);
                return;
            }
            // Nothing continues pending with c: write pending, then read c again.
            self.flush_pending();
        }
    }

    /// Writes the waiting letters by the longest rule they start with (all of them, `n` → ん,
    /// or fewer) and reads the rest again; with no rule, the first letter is written as typed.
    fn flush_pending(&mut self) {
        let chars: Vec<char> = std::mem::take(&mut self.pending).chars().collect();
        for end in (1..=chars.len()).rev() {
            let prefix: String = chars[..end].iter().collect();
            if let Some((kana, left)) = self.table.rules.get(&prefix) {
                self.kana.push_str(kana);
                for c in left.chars().chain(chars[end..].iter().copied()) {
                    self.push(c);
                }
                return;
            }
        }
        self.kana.push(chars[0]);
        for &c in &chars[1..] {
            self.push(c);
        }
    }

    /// Ends typing: letters still waiting are written by their rule (`n` → ん), else kept
    /// as typed.
    pub fn finish(&mut self) {
        while !self.pending.is_empty() {
            let before = self.pending.clone();
            self.flush_pending();
            if self.pending == before {
                // Can't happen (flush always writes at least a letter), but never spin.
                let p = std::mem::take(&mut self.pending);
                self.kana.push_str(&p);
            }
        }
    }

    /// Deletes the last waiting letter, else the last kana. False when there was nothing.
    pub fn backspace(&mut self) -> bool {
        if self.pending.pop().is_some() {
            return true;
        }
        self.kana.pop().is_some()
    }
}

/// `text` with hiragana turned into katakana (あ → ア, ゝ → ヽ); everything else unchanged.
pub fn to_katakana(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{3041}'..='\u{3096}' | '\u{309D}' | '\u{309E}' => {
                char::from_u32(c as u32 + 0x60).unwrap_or(c)
            }
            _ => c,
        })
        .collect()
}

/// `text` with katakana turned into hiragana (ア → あ); ー and katakana with no hiragana
/// (ヷ, ヺ) unchanged.
pub fn to_hiragana(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\u{30A1}'..='\u{30F6}' | '\u{30FD}' | '\u{30FE}' => {
                char::from_u32(c as u32 - 0x60).unwrap_or(c)
            }
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> KanaTable {
        let text = include_str!("../../../../data/langs/ja/kana.json");
        KanaTable::parse(text).unwrap()
    }

    /// `typed` composed and finished.
    fn kana(typed: &str) -> String {
        let (kana, pending) = table().compose(typed, true);
        assert_eq!(pending, "", "{typed}");
        kana
    }

    #[test]
    fn plain_syllables() {
        let cases = [
            ("aiueo", "あいうえお"),
            ("kakikukeko", "かきくけこ"),
            ("sasisuseso", "さしすせそ"),
            ("shi", "し"),
            ("tatituteto", "たちつてと"),
            ("chitsu", "ちつ"),
            ("hahihuheho", "はひふへほ"),
            ("fu", "ふ"),
            ("ji", "じ"),
            ("zi", "じ"),
            ("wawo", "わを"),
            ("yayuyo", "やゆよ"),
            ("ra", "ら"),
            ("pa", "ぱ"),
            ("nihongo", "にほんご"),
            ("kyouhaamegafuru", "きょうはあめがふる"),
            ("watashi", "わたし"),
        ];
        for (typed, want) in cases {
            assert_eq!(kana(typed), want, "{typed}");
        }
    }

    #[test]
    fn contracted_sounds() {
        let cases = [
            ("kya", "きゃ"),
            ("kyu", "きゅ"),
            ("sha", "しゃ"),
            ("sya", "しゃ"),
            ("cha", "ちゃ"),
            ("tya", "ちゃ"),
            ("ja", "じゃ"),
            ("jya", "じゃ"),
            ("zya", "じゃ"),
            ("nya", "にゃ"),
            ("ryo", "りょ"),
            ("byu", "びゅ"),
            ("pyo", "ぴょ"),
            ("myu", "みゅ"),
            ("hyaku", "ひゃく"),
            ("fa", "ふぁ"),
            ("thi", "てぃ"),
            ("dhi", "でぃ"),
            ("vu", "ゔ"),
            ("tsa", "つぁ"),
            ("che", "ちぇ"),
            ("she", "しぇ"),
            ("je", "じぇ"),
            ("kwa", "くぁ"),
        ];
        for (typed, want) in cases {
            assert_eq!(kana(typed), want, "{typed}");
        }
    }

    #[test]
    fn n_and_nn() {
        let cases = [
            ("nn", "ん"),
            ("n'", "ん"),
            ("xn", "ん"),
            ("nihon", "にほん"),
            ("nihonn", "にほん"),
            ("kanji", "かんじ"),
            ("sannpo", "さんぽ"),
            ("sanpo", "さんぽ"),
            ("konnnichiha", "こんにちは"),
            // nn is ん, so the next vowel starts its own kana: strict, as input methods are.
            ("konnichiha", "こんいちは"),
            ("kon'ya", "こんや"),
            ("konya", "こにゃ"),
            ("kin'en", "きんえん"),
            ("kinen", "きねん"),
            ("shinbun", "しんぶん"),
            ("ninja", "にんじゃ"),
            ("nnn", "んん"),
        ];
        for (typed, want) in cases {
            assert_eq!(kana(typed), want, "{typed}");
        }
        // nn is ん at once; a third n waits, and finish writes it as ん.
        assert_eq!(table().compose("nnn", false), ("ん".into(), "n".into()));
    }

    #[test]
    fn small_tsu_from_doubled_consonants() {
        let cases = [
            ("kitte", "きって"),
            ("gakkou", "がっこう"),
            ("zasshi", "ざっし"),
            ("matcha", "まっちゃ"),
            ("macchi", "まっち"),
            ("kappa", "かっぱ"),
            ("motto", "もっと"),
            ("issho", "いっしょ"),
            ("yatta", "やった"),
            ("jjo", "っじょ"),
            ("ffa", "っふぁ"),
        ];
        for (typed, want) in cases {
            assert_eq!(kana(typed), want, "{typed}");
        }
        assert_eq!(table().compose("bbb", false), ("っっ".into(), "b".into()));
    }

    #[test]
    fn small_kana_with_x_or_l() {
        let cases = [
            ("xtu", "っ"),
            ("ltu", "っ"),
            ("xtsu", "っ"),
            ("ltsu", "っ"),
            ("xa", "ぁ"),
            ("li", "ぃ"),
            ("xya", "ゃ"),
            ("lyo", "ょ"),
            ("xwa", "ゎ"),
            ("xka", "ヵ"),
            ("lke", "ヶ"),
            ("axtu", "あっ"),
        ];
        for (typed, want) in cases {
            assert_eq!(kana(typed), want, "{typed}");
        }
    }

    #[test]
    fn long_vowels_and_marks() {
        let cases = [
            ("ra-men", "らーめん"),
            ("ko-hi-", "こーひー"),
            ("toukyou", "とうきょう"),
            ("oo", "おお"),
            ("ne.", "ね。"),
            ("a,i", "あ、い"),
            ("[a]", "「あ」"),
            ("e?", "え？"),
        ];
        for (typed, want) in cases {
            assert_eq!(kana(typed), want, "{typed}");
        }
    }

    #[test]
    fn letters_waiting_show_as_typed() {
        let t = table();
        assert_eq!(t.compose("k", false), ("".into(), "k".into()));
        assert_eq!(t.compose("ky", false), ("".into(), "ky".into()));
        assert_eq!(t.compose("kak", false), ("か".into(), "k".into()));
        assert_eq!(t.compose("n", false), ("".into(), "n".into()));
        assert_eq!(t.compose("nihon", false), ("にほ".into(), "n".into()));
        assert_eq!(t.compose("kk", false), ("っ".into(), "k".into()));
        // At the end, letters without a rule stay as typed.
        assert_eq!(t.compose("k", true), ("k".into(), "".into()));
        assert_eq!(t.compose("kak", true), ("かk".into(), "".into()));
    }

    #[test]
    fn letters_no_rule_continues() {
        // q then k: q starts rules (qa), k doesn't continue it, so q is written as typed.
        assert_eq!(kana("qka"), "qか");
        // Digits and other characters pass through.
        assert_eq!(kana("2ji"), "2じ");
        assert_eq!(kana("a@b"), "あ@b");
        // ky then a consonant: no rule, ky written as typed letters.
        assert_eq!(kana("kyka"), "kyか");
        // ts waits for tsu; k doesn't continue it and neither ts nor t is a rule.
        assert_eq!(kana("tsk"), "tsk");
    }

    #[test]
    fn upper_case_reads_as_lower() {
        assert_eq!(kana("Nihongo"), "にほんご");
        assert_eq!(kana("KYOU"), "きょう");
    }

    #[test]
    fn backspace_deletes_a_letter_then_a_kana() {
        let t = table();
        let mut c = Composer::new(&t);
        for ch in "kyok".chars() {
            c.push(ch);
        }
        assert_eq!(c.text(), "きょk");
        assert!(c.backspace());
        assert_eq!(c.text(), "きょ");
        assert!(c.backspace());
        assert_eq!(c.text(), "き");
        assert!(c.backspace());
        assert!(c.is_empty());
        assert!(!c.backspace());
        c.push('a');
        assert_eq!(c.text(), "あ");
        c.clear();
        assert!(c.is_empty());
    }

    #[test]
    fn katakana_toggle() {
        assert_eq!(to_katakana("らーめん"), "ラーメン");
        assert_eq!(to_katakana("ゔぁいおりん"), "ヴァイオリン");
        assert_eq!(to_katakana("ゝ漢a"), "ヽ漢a");
        assert_eq!(to_hiragana("ラーメン"), "らーめん");
        assert_eq!(to_hiragana(&to_katakana("きょうはあめ")), "きょうはあめ");
        assert_eq!(to_katakana(&kana("ko-hi-")), "コーヒー");
    }

    #[test]
    fn every_rule_types_its_own_kana() {
        let t = table();
        for (typed, (kana, left)) in &t.rules {
            let (got, pending) = t.compose(typed, false);
            // A rule others continue waits (n, which could be na): finish writes it.
            if t.prefixes.contains(typed) {
                assert_eq!(pending, *typed, "{typed}");
                assert_eq!(t.compose(typed, true).0, *kana, "{typed}");
            } else {
                assert_eq!((&got, &pending), (kana, left), "{typed}");
            }
        }
    }

    #[test]
    fn bad_tables_are_refused() {
        let bad = [
            r#"{"format": 2, "rules": []}"#,
            r#"{"format": 1, "rules": [["ka"]]}"#,
            r#"{"format": 1, "rules": [["", "か"]]}"#,
            r#"{"format": 1, "rules": [["Ka", "か"]]}"#,
            r#"{"format": 1, "rules": [["kk", "っ", "kk"]]}"#,
            r#"{"format": 1, "rules": [["ka", "か"], ["ka", "が"]]}"#,
            r#"{"format": 1, "rules": [], "other": 1}"#,
        ];
        for text in bad {
            assert!(KanaTable::parse(text).is_err(), "{text}");
        }
    }
}

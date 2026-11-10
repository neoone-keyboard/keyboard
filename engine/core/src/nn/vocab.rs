//! The transliterator's token ids, a mirror of `training/mlkb/translit/vocab.py`: the model
//! file is only meaningful with these exact tables, so never reorder them.
//!
//! Latin: `pad bos eos`, then `a`–`z`. A native script (`Target`): `pad bos eos`, then every
//! code point of its 128-code-point block (U+0D00–U+0D7F for Malayalam, U+0900–U+097F for
//! Devanagari, U+0980–U+09FF for Bengali-Assamese), then ZWJ and ZWNJ, then any characters the
//! script writes inside words besides (the apostrophe of Assamese হ'ল).

use crate::script::ScriptRules;

pub const PAD: u32 = 0;
pub const BOS: u32 = 1;
pub const EOS: u32 = 2;

/// Tokens before the first character token.
const SPECIALS: u32 = 3;

/// Latin vocabulary size: the specials and 26 letters.
pub const LATIN_SIZE: usize = SPECIALS as usize + 26;

/// Code points in a script's block.
const BLOCK: u32 = 128;

const ZWJ: char = '\u{200D}';
const ZWNJ: char = '\u{200C}';

/// The extra character `vocab.py` adds for a language that writes an apostrophe inside words.
const APOSTROPHE: &[char] = &['\''];

/// `bos`, one token per letter, `eos`; None unless every byte is `a`–`z`. Unlike `vocab.py`
/// it doesn't lowercase: callers decide what typed text counts as Latin.
pub fn encode_latin(text: &str) -> Option<Vec<u32>> {
    let mut out = Vec::with_capacity(text.len() + 2);
    out.push(BOS);
    for b in text.bytes() {
        if !b.is_ascii_lowercase() {
            return None;
        }
        out.push(SPECIALS + u32::from(b - b'a'));
    }
    out.push(EOS);
    Some(out)
}

/// The target vocabulary of one native script: `vocab.py`'s `block(start, extra)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    /// The first code point of the script's block.
    start: u32,
    /// The characters after ZWJ and ZWNJ, in token order.
    extra: &'static [char],
}

impl Target {
    /// Malayalam's: `vocab.py`'s `MALAYALAM`.
    pub const MALAYALAM: Target = Target {
        start: 0x0D00,
        extra: &[],
    };

    /// The vocabulary a pack written in `rules`' script was trained with: the script's block,
    /// and the apostrophe where its words carry one (`ScriptRules::inner_apostrophe`; `vocab.py`
    /// adds it for a language whose config says `"apostrophe": true`).
    pub fn for_script(rules: &dyn ScriptRules) -> Self {
        Self {
            start: *rules.block().start() as u32,
            extra: if rules.inner_apostrophe() {
                APOSTROPHE
            } else {
                &[]
            },
        }
    }

    /// Tokens in the vocabulary: the specials, the block, ZWJ, ZWNJ and the extras.
    pub fn size(&self) -> usize {
        (SPECIALS + BLOCK) as usize + 2 + self.extra.len()
    }

    /// The token of a character; None outside the vocabulary.
    pub fn token(&self, c: char) -> Option<u32> {
        let cp = c as u32;
        if (self.start..self.start + BLOCK).contains(&cp) {
            return Some(SPECIALS + cp - self.start);
        }
        match c {
            ZWJ => Some(SPECIALS + BLOCK),
            ZWNJ => Some(SPECIALS + BLOCK + 1),
            _ => self
                .extra
                .iter()
                .position(|&e| e == c)
                .map(|i| SPECIALS + BLOCK + 2 + i as u32),
        }
    }

    /// The character of a token; None for the specials and out-of-range ids.
    pub fn char(&self, token: u32) -> Option<char> {
        match token.checked_sub(SPECIALS)? {
            i if i < BLOCK => char::from_u32(self.start + i),
            BLOCK => Some(ZWJ),
            i if i == BLOCK + 1 => Some(ZWNJ),
            i => self.extra.get((i - BLOCK - 2) as usize).copied(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::script;
    use serde_json::Value;

    #[test]
    fn matches_the_fixture_reference() {
        // make_fixture.py writes each case's tokens with vocab.py.
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/translit-tiny-reference.json"
        );
        let json: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let ml = Target::MALAYALAM;
        assert_eq!(json["config"]["src_vocab"], LATIN_SIZE);
        assert_eq!(json["config"]["tgt_vocab"], ml.size());
        let ids = |v: &Value| -> Vec<u32> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|t| t.as_u64().unwrap() as u32)
                .collect()
        };
        for case in json["cases"].as_array().unwrap() {
            let latin = case["latin"].as_str().unwrap();
            assert_eq!(encode_latin(latin).unwrap(), ids(&case["src"]), "{latin}");
            // `tgt` is the decoder input: bos and the word, without eos.
            let tgt = ids(&case["tgt"]);
            assert_eq!(tgt[0], BOS);
            let word: String = tgt[1..].iter().map(|&t| ml.char(t).unwrap()).collect();
            assert_eq!(word, case["malayalam"].as_str().unwrap());
        }
    }

    fn round_trips(t: Target) {
        for token in 0..t.size() as u32 + 2 {
            match t.char(token) {
                Some(c) => assert_eq!(t.token(c), Some(token), "{t:?} {c:?}"),
                None => assert!(
                    token < SPECIALS || token >= t.size() as u32,
                    "{t:?} {token}"
                ),
            }
        }
    }

    #[test]
    fn tables_round_trip() {
        assert_eq!(encode_latin("az"), Some(vec![BOS, 3, 28, EOS]));
        assert_eq!(encode_latin(""), Some(vec![BOS, EOS]));
        for bad in ["A", "a b", "é", "a1"] {
            assert_eq!(encode_latin(bad), None, "{bad}");
        }
        let ml = Target::MALAYALAM;
        round_trips(ml);
        assert_eq!(ml.size(), 133);
        assert_eq!(ml.token('എ'), Some(17));
        assert_eq!(ml.char(ml.size() as u32 - 2), Some(ZWJ));
        assert_eq!(ml.token('a'), None);
        assert_eq!(ml.token('क'), None);
        assert_eq!(ml.token('\''), None);
    }

    /// `vocab.py`'s `native(lang)` per script: `block(0x0D00)` for ml, `block(0x0900)` for hi,
    /// `block(0x0980, ("'",))` for as (134 tokens, the apostrophe last).
    #[test]
    fn each_script_has_its_own_block() {
        let target = |code| Target::for_script(script::rules(code).unwrap());
        assert_eq!(target("mlym"), Target::MALAYALAM);

        let hi = target("deva");
        round_trips(hi);
        assert_eq!(hi.size(), 133);
        assert_eq!(hi.char(3), Some('\u{0900}'));
        assert_eq!(hi.token('क'), Some(3 + 0x15));
        assert_eq!(hi.char(130), Some('\u{097F}'));
        assert_eq!((hi.char(131), hi.char(132)), (Some(ZWJ), Some(ZWNJ)));
        assert_eq!(hi.char(133), None);
        assert_eq!(hi.token('ക'), None);
        assert_eq!(hi.token('\''), None);

        let as_ = target("beng");
        round_trips(as_);
        assert_eq!(as_.size(), 134);
        assert_eq!(as_.char(3), Some('\u{0980}'));
        assert_eq!(as_.token('ৰ'), Some(3 + 0x70));
        assert_eq!((as_.char(131), as_.char(132)), (Some(ZWJ), Some(ZWNJ)));
        assert_eq!(as_.char(133), Some('\''));
        assert_eq!(as_.token('\''), Some(133));
        assert_eq!(as_.token('क'), None);
    }
}

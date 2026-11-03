//! The transliterator's token ids, a mirror of `training/mlkb/translit/vocab.py`: the model
//! file is only meaningful with these exact tables, so never reorder them.
//!
//! Latin: `pad bos eos`, then `a`–`z`. Malayalam: `pad bos eos`, then every code point in
//! U+0D00–U+0D7F, then ZWJ and ZWNJ.

pub const PAD: u32 = 0;
pub const BOS: u32 = 1;
pub const EOS: u32 = 2;

/// Tokens before the first character token.
const SPECIALS: u32 = 3;

/// Latin vocabulary size: the specials and 26 letters.
pub const LATIN_SIZE: usize = SPECIALS as usize + 26;

/// Malayalam vocabulary size: the specials, the 128-code-point block, ZWJ and ZWNJ.
pub const MALAYALAM_SIZE: usize = SPECIALS as usize + 128 + 2;

const ZWJ: char = '\u{200D}';
const ZWNJ: char = '\u{200C}';

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

/// The token of a Malayalam-side character; None outside the vocabulary.
pub fn malayalam_token(c: char) -> Option<u32> {
    match c {
        '\u{0D00}'..='\u{0D7F}' => Some(SPECIALS + (c as u32 - 0x0D00)),
        ZWJ => Some(SPECIALS + 128),
        ZWNJ => Some(SPECIALS + 129),
        _ => None,
    }
}

/// The character of a Malayalam-side token; None for the specials and out-of-range ids.
pub fn malayalam_char(token: u32) -> Option<char> {
    match token.checked_sub(SPECIALS)? {
        i @ 0..128 => char::from_u32(0x0D00 + i),
        128 => Some(ZWJ),
        129 => Some(ZWNJ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn matches_the_fixture_reference() {
        // make_fixture.py writes each case's tokens with vocab.py.
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/translit-tiny-reference.json"
        );
        let json: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(json["config"]["src_vocab"], LATIN_SIZE);
        assert_eq!(json["config"]["tgt_vocab"], MALAYALAM_SIZE);
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
            let word: String = tgt[1..]
                .iter()
                .map(|&t| malayalam_char(t).unwrap())
                .collect();
            assert_eq!(word, case["malayalam"].as_str().unwrap());
        }
    }

    #[test]
    fn tables_round_trip() {
        assert_eq!(encode_latin("az"), Some(vec![BOS, 3, 28, EOS]));
        assert_eq!(encode_latin(""), Some(vec![BOS, EOS]));
        for bad in ["A", "a b", "é", "a1"] {
            assert_eq!(encode_latin(bad), None, "{bad}");
        }
        for t in 0..MALAYALAM_SIZE as u32 + 2 {
            match malayalam_char(t) {
                Some(c) => assert_eq!(malayalam_token(c), Some(t)),
                None => assert!(t < SPECIALS || t >= MALAYALAM_SIZE as u32, "{t}"),
            }
        }
        assert_eq!(malayalam_token('എ'), Some(17));
        assert_eq!(malayalam_char(MALAYALAM_SIZE as u32 - 2), Some(ZWJ));
        assert_eq!(malayalam_token('a'), None);
    }
}

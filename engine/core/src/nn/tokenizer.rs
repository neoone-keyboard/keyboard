//! The LM's tokeniser: SentencePiece unigram pieces (trained by `training/mlkb/lm/tokenizer.py`)
//! applied by Viterbi over piece scores, as `sentencepiece` encodes. A character no piece
//! covers becomes its UTF-8 byte pieces, scored `min_score - UNK_PENALTY` as SentencePiece's
//! byte fallback does. Ids 0–3 are pad, unk, bos and eos; ids 4–259 the byte pieces.

use std::ops::RangeInclusive;

use fst::raw::Output;
use fst::{IntoStreamer, Map, MapBuilder, Streamer};

use super::NnError;
use crate::normalize::normalize;

/// Padding.
pub const PAD: u32 = 0;
/// SentencePiece's unknown piece (never produced: unknown characters fall back to bytes).
pub const UNK: u32 = 1;
/// Start of a text.
pub const BOS: u32 = 2;
/// End of a text.
pub const EOS: u32 = 3;
/// SentencePiece's mark for the start of a word.
pub const WORD_START: char = '\u{2581}';
/// SentencePiece's penalty for an unknown character, below the lowest piece score.
const UNK_PENALTY: f32 = 10.0;
/// A step of the Viterbi path that took a character's byte pieces.
const BYTES: u32 = u32::MAX;

/// What a piece is, from its text.
pub mod flag {
    /// Begins with `WORD_START`.
    pub const WORD_START: u8 = 1;
    /// Has an ASCII letter.
    pub const LATIN: u8 = 2;
    /// Has a character of the model's native script block (`Tokenizer::set_native`;
    /// Malayalam's, U+0D00–U+0D7F, unless set).
    pub const NATIVE: u8 = 4;
    /// Neither: punctuation, digits, a bare `WORD_START`.
    pub const OTHER: u8 = 8;
    /// A byte-fallback piece (`<0x41>`).
    pub const BYTE: u8 = 16;
    /// pad, unk, bos or eos.
    pub const CONTROL: u8 = 32;
}

/// The text form the LM is trained and run on: the engine's `normalize` (NFC, quote
/// folding), ASCII lowercase, single spaces. Training text goes through this same function.
pub fn lm_normalize(text: &str) -> String {
    let normal = normalize(text);
    let mut out = String::with_capacity(normal.len());
    for word in normal.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.extend(word.chars().map(|c| c.to_ascii_lowercase()));
    }
    out
}

/// Pieces, their scores, and an index from piece text to id.
#[derive(Debug)]
pub struct Tokenizer {
    pieces: Vec<String>,
    scores: Vec<f32>,
    flags: Vec<u8>,
    index: Map<Vec<u8>>,
    bytes: [u32; 256],
    unk_score: f32,
}

fn byte_of(piece: &str) -> Option<u8> {
    let hex = piece.strip_prefix("<0x")?.strip_suffix('>')?;
    (hex.len() == 2)
        .then(|| u8::from_str_radix(hex, 16).ok())
        .flatten()
}

/// The native block a tokeniser flags until told otherwise (`Tokenizer::set_native`).
pub const MALAYALAM_BLOCK: RangeInclusive<char> = '\u{0D00}'..='\u{0D7F}';

fn flags_of(id: u32, piece: &str, native: &RangeInclusive<char>) -> u8 {
    if id <= EOS {
        return flag::CONTROL;
    }
    if byte_of(piece).is_some() {
        return flag::BYTE;
    }
    let mut f = 0;
    if piece.starts_with(WORD_START) {
        f |= flag::WORD_START;
    }
    if piece.chars().any(|c| native.contains(&c)) {
        f |= flag::NATIVE;
    } else if piece.chars().any(|c| c.is_ascii_alphabetic()) {
        f |= flag::LATIN;
    } else {
        f |= flag::OTHER;
    }
    f
}

impl Tokenizer {
    /// Pieces in id order with their scores (log probabilities), as SentencePiece writes them.
    pub fn new(pieces: Vec<String>, scores: Vec<f32>) -> Result<Self, NnError> {
        if pieces.len() != scores.len() || pieces.len() <= EOS as usize {
            return Err(NnError::Malformed(
                "tokeniser: pieces and scores don't match".into(),
            ));
        }
        let mut bytes = [UNK; 256];
        let mut entries: Vec<(&[u8], u64)> = Vec::new();
        let mut flags = Vec::with_capacity(pieces.len());
        let mut min_score = f32::INFINITY;
        for (id, piece) in pieces.iter().enumerate() {
            let f = flags_of(id as u32, piece, &MALAYALAM_BLOCK);
            if f & flag::BYTE != 0 {
                bytes[usize::from(byte_of(piece).unwrap())] = id as u32;
            } else if f & flag::CONTROL == 0 {
                entries.push((piece.as_bytes(), id as u64));
                min_score = min_score.min(scores[id]);
            }
            flags.push(f);
        }
        entries.sort();
        entries.dedup_by(|a, b| a.0 == b.0);
        let mut builder = MapBuilder::memory();
        for (key, id) in &entries {
            builder
                .insert(key, *id)
                .map_err(|e| NnError::Malformed(e.to_string()))?;
        }
        let bytes_fst = builder
            .into_inner()
            .map_err(|e| NnError::Malformed(e.to_string()))?;
        let index = Map::new(bytes_fst).map_err(|e| NnError::Malformed(e.to_string()))?;
        Ok(Self {
            unk_score: min_score - UNK_PENALTY,
            pieces,
            scores,
            flags,
            index,
            bytes,
        })
    }

    /// Number of pieces.
    pub fn len(&self) -> usize {
        self.pieces.len()
    }

    /// Never true for a tokeniser `new` accepted.
    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// The piece's text.
    pub fn piece(&self, id: u32) -> &str {
        &self.pieces[id as usize]
    }

    /// Flags the pieces with a character of `block` `NATIVE` instead of Malayalam's: the
    /// script of the pack whose LM this is (`Engine` sets it from the pack's script rules).
    pub fn set_native(&mut self, block: RangeInclusive<char>) {
        for (id, piece) in self.pieces.iter().enumerate() {
            self.flags[id] = flags_of(id as u32, piece, &block);
        }
    }

    /// The piece's `flag` bits.
    pub fn flags(&self, id: u32) -> u8 {
        self.flags[id as usize]
    }

    /// Tokens of `text` after `lm_normalize`; every word starts with a `WORD_START` piece.
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let normal = lm_normalize(text);
        if normal.is_empty() {
            return Vec::new();
        }
        let mut s = String::with_capacity(normal.len() + 8);
        for word in normal.split(' ') {
            s.push(WORD_START);
            s.push_str(word);
        }
        self.viterbi(&s)
    }

    /// The tokens of one word, as the LM sees it after a space.
    pub fn word(&self, word: &str) -> Vec<u32> {
        self.encode(word)
    }

    /// Pieces whose text begins with `prefix`, in byte order.
    pub fn starting_with<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = u32> + 'a {
        let mut stream = self.index.range().ge(prefix).into_stream();
        let mut out = Vec::new();
        while let Some((key, id)) = stream.next() {
            if !key.starts_with(prefix.as_bytes()) {
                break;
            }
            out.push(id as u32);
        }
        out.into_iter()
    }

    fn viterbi(&self, s: &str) -> Vec<u32> {
        let bytes = s.as_bytes();
        let n = bytes.len();
        let starts: Vec<usize> = s.char_indices().map(|(i, _)| i).chain([n]).collect();
        let mut best = vec![f32::NEG_INFINITY; n + 1];
        let mut back = vec![(0usize, BYTES); n + 1];
        best[0] = 0.0;
        for (k, &i) in starts.iter().enumerate() {
            if i == n || best[i] == f32::NEG_INFINITY {
                continue;
            }
            let next_char = starts[k + 1];
            let mut covered = false;
            // One walk down the index from `i` finds every piece that starts here.
            let fst = self.index.as_fst();
            let mut node = fst.root();
            let mut output = Output::zero();
            for (j, &b) in bytes.iter().enumerate().skip(i) {
                let Some(t) = node.find_input(b) else { break };
                let t = node.transition(t);
                output = output.cat(t.out);
                node = fst.node(t.addr);
                let end = j + 1;
                if node.is_final() && s.is_char_boundary(end) {
                    let id = output.cat(node.final_output()).value() as u32;
                    let score = best[i] + self.scores[id as usize];
                    if score > best[end] {
                        best[end] = score;
                        back[end] = (i, id);
                    }
                    covered |= end == next_char;
                }
            }
            if !covered {
                let score = best[i] + self.unk_score;
                if score > best[next_char] {
                    best[next_char] = score;
                    back[next_char] = (i, BYTES);
                }
            }
        }
        let mut out = Vec::new();
        let mut j = n;
        while j > 0 {
            let (i, id) = back[j];
            if id == BYTES {
                out.extend(
                    bytes[i..j]
                        .iter()
                        .rev()
                        .map(|&b| self.bytes[usize::from(b)]),
                );
            } else {
                out.push(id);
            }
            j = i;
        }
        out.reverse();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Control pieces, 256 byte pieces, then a few words, as SentencePiece orders them.
    fn tiny() -> Tokenizer {
        let mut pieces: Vec<String> = ["<pad>", "<unk>", "<s>", "</s>"].map(String::from).to_vec();
        pieces.extend((0..=255u8).map(|b| format!("<0x{b:02X}>")));
        let mut scores = vec![0.0; pieces.len()];
        for (p, s) in [
            ("▁", -3.0),
            ("▁the", -2.0),
            ("▁then", -4.0),
            ("n", -3.5),
            ("▁t", -5.0),
            ("he", -5.0),
            ("▁എന്റെ", -3.0),
            ("▁എ", -6.0),
            (".", -2.5),
        ] {
            pieces.push(p.to_string());
            scores.push(s);
        }
        Tokenizer::new(pieces, scores).unwrap()
    }

    fn texts(t: &Tokenizer, s: &str) -> Vec<String> {
        t.encode(s)
            .iter()
            .map(|&id| t.piece(id).to_string())
            .collect()
    }

    #[test]
    fn lm_normalize_lowercases_latin_and_collapses_space() {
        assert_eq!(lm_normalize("  Hello\tWORLD  "), "hello world");
        assert_eq!(lm_normalize("എന്റെ  Phone"), "എന്റെ phone");
        assert_eq!(lm_normalize("don’t"), "don't");
    }

    #[test]
    fn viterbi_takes_the_likelier_segmentation() {
        // ▁the + ▁then (−6.0) beats ▁the + ▁the + n (−7.5).
        assert_eq!(texts(&tiny(), "the then"), ["▁the", "▁then"]);
        assert_eq!(texts(&tiny(), "The."), ["▁the", "."]);
        assert_eq!(texts(&tiny(), "എന്റെ"), ["▁എന്റെ"]);
    }

    #[test]
    fn a_character_no_piece_covers_falls_back_to_its_bytes() {
        assert_eq!(
            texts(&tiny(), "the ☃"),
            ["▁the", "▁", "<0xE2>", "<0x98>", "<0x83>"]
        );
    }

    #[test]
    fn words_start_with_the_word_mark() {
        let t = tiny();
        assert_eq!(t.word("Then"), t.encode("then"));
        assert_eq!(t.piece(t.word("Then")[0]), "▁then");
    }

    #[test]
    fn flags_follow_the_piece_text() {
        let t = tiny();
        let id = |p: &str| (0..t.len() as u32).find(|&i| t.piece(i) == p).unwrap();
        assert_eq!(t.flags(BOS), flag::CONTROL);
        assert_eq!(t.flags(id("<0x41>")), flag::BYTE);
        assert_eq!(t.flags(id("▁the")), flag::WORD_START | flag::LATIN);
        assert_eq!(t.flags(id("▁എന്റെ")), flag::WORD_START | flag::NATIVE);
        assert_eq!(t.flags(id("n")), flag::LATIN);
        assert_eq!(t.flags(id(".")), flag::OTHER);
    }

    #[test]
    fn the_native_flag_follows_the_block_it_is_given() {
        let mut t = tiny();
        let id = |t: &Tokenizer, p: &str| (0..t.len() as u32).find(|&i| t.piece(i) == p).unwrap();
        t.set_native('\u{0900}'..='\u{097F}');
        let (ml, the) = (id(&t, "▁എന്റെ"), id(&t, "▁the"));
        assert_eq!(t.flags(ml), flag::WORD_START | flag::OTHER);
        assert_eq!(t.flags(the), flag::WORD_START | flag::LATIN);
        let mut pieces: Vec<String> = (0..t.len() as u32)
            .map(|i| t.piece(i).to_string())
            .collect();
        pieces.push("▁हम".into());
        let mut deva = Tokenizer::new(pieces, vec![-1.0; t.len() + 1]).unwrap();
        let hum = id(&deva, "▁हम");
        assert_eq!(
            deva.flags(hum),
            flag::WORD_START | flag::OTHER,
            "Malayalam's block by default"
        );
        deva.set_native('\u{0900}'..='\u{097F}');
        assert_eq!(deva.flags(hum), flag::WORD_START | flag::NATIVE);
        assert_eq!(
            deva.encode("हम"),
            [hum],
            "the block changes flags, not tokens"
        );
    }

    #[test]
    fn pieces_that_begin_with_a_prefix() {
        let t = tiny();
        let mut found: Vec<&str> = t.starting_with("▁th").map(|id| t.piece(id)).collect();
        found.sort();
        assert_eq!(found, ["▁the", "▁then"]);
    }
}

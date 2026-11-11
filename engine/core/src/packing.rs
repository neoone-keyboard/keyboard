//! How a native word is written one byte per character in a romanised-lexicon key
//! (`skeleton\0packed-word`, `manglish`), and which characters a transliterator spells in
//! (`nn::vocab::Target`).
//!
//! Two ways, chosen by the pack's `lang.json`:
//!
//! - [`Packing::Block`], every pack before engine 8: the 128 code points from the first one of
//!   the script (U+0D00 for Malayalam) become 0x80–0xFF. A script whose letters span more
//!   than 128 code points can't be packed so.
//! - [`Packing::Table`] (engine 8): the script's own character table, `lang.json`'s
//!   `charTable`, lists the characters its words are made of, in order, up to
//!   [`CharTable::MAX`]. The first 128 become 0x80–0xFF, the rest the free bytes below 0x80
//!   ([`TABLE_BYTES`]). Urdu's letters lie across the whole Arabic block (ک is U+06A9, ے
//!   U+06D2), which no 128-code-point window holds.
//!
//! Either way ZWNJ is 0x01, ZWJ 0x02 and the apostrophe 0x27 (Assamese হ'ল), and no packed
//! byte is 0x00 (`lexicon::SEPARATOR`) or an ASCII capital, as the lexicon compares keys ASCII
//! case-insensitively (`lexicon::build`'s precomputed lists).

use std::sync::Mutex;

const ZWNJ: char = '\u{200C}';
const ZWJ: char = '\u{200D}';

/// The bytes a table's characters are packed as, in order: 0x80–0xFF, then 0x03–0x7E less the
/// apostrophe and the ASCII capitals. Below 0x03 are the separator and the joiners.
pub const TABLE_BYTES: [u8; CharTable::MAX] = table_bytes();

const fn table_bytes() -> [u8; CharTable::MAX] {
    let mut out = [0u8; CharTable::MAX];
    let mut i = 0;
    let mut b: u16 = 0x80;
    while b <= 0xFF {
        out[i] = b as u8;
        i += 1;
        b += 1;
    }
    let mut b: u8 = 0x03;
    while b <= 0x7E {
        if b != b'\'' && !b.is_ascii_uppercase() {
            out[i] = b;
            i += 1;
        }
        b += 1;
    }
    assert!(i == CharTable::MAX);
    out
}

/// A script's character table (`lang.json`'s `charTable`): the characters its words are made
/// of, each with its byte (`TABLE_BYTES`, by position) and its transliterator token.
#[derive(Debug, PartialEq, Eq)]
pub struct CharTable {
    chars: Vec<char>,
    /// `chars`, sorted, each with its position.
    sorted: Vec<(char, u8)>,
    /// The position of the character each byte stands for, `u8::MAX` for none.
    by_byte: [u8; 256],
}

impl CharTable {
    /// Most characters a table may list: one byte each.
    pub const MAX: usize = 128 + (0x7F - 0x03) - 1 - 26;

    /// The table of `chars`, in order, made once per distinct list (tables live as long as
    /// the process, as the script rules holding them do). An error, in words, for an empty or
    /// too long list, a repeated character, or one the packing writes otherwise (ASCII, the
    /// joiners).
    pub fn intern(chars: Vec<char>) -> Result<&'static CharTable, String> {
        if chars.is_empty() {
            return Err("charTable lists no characters".into());
        }
        if chars.len() > Self::MAX {
            return Err(format!(
                "charTable lists {} characters, at most {} fit one byte each",
                chars.len(),
                Self::MAX
            ));
        }
        let mut sorted: Vec<(char, u8)> =
            chars.iter().enumerate().map(|(i, &c)| (c, i as u8)).collect();
        sorted.sort_unstable();
        if let Some(w) = sorted.windows(2).find(|w| w[0].0 == w[1].0) {
            return Err(format!("charTable lists U+{:04X} twice", w[0].0 as u32));
        }
        if let Some(&(c, _)) = sorted.iter().find(|(c, _)| c.is_ascii() || matches!(*c, ZWNJ | ZWJ)) {
            return Err(format!(
                "charTable can't list U+{:04X}: ASCII and the joiners are packed by themselves",
                c as u32
            ));
        }
        let mut by_byte = [u8::MAX; 256];
        for (i, &b) in TABLE_BYTES.iter().take(chars.len()).enumerate() {
            by_byte[b as usize] = i as u8;
        }
        let table = CharTable { chars, sorted, by_byte };
        static MADE: Mutex<Vec<&'static CharTable>> = Mutex::new(Vec::new());
        let mut made = MADE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(&found) = made.iter().find(|t| ***t == table) {
            return Ok(found);
        }
        let leaked: &'static CharTable = Box::leak(Box::new(table));
        made.push(leaked);
        Ok(leaked)
    }

    /// The characters, in table order.
    pub fn chars(&self) -> &[char] {
        &self.chars
    }

    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// A character's position in the table.
    pub fn position(&self, c: char) -> Option<usize> {
        self.sorted
            .binary_search_by_key(&c, |&(c, _)| c)
            .ok()
            .map(|i| self.sorted[i].1 as usize)
    }

    fn byte(&self, c: char) -> Option<u8> {
        self.position(c).map(|i| TABLE_BYTES[i])
    }

    fn char_of(&self, b: u8) -> Option<char> {
        match self.by_byte[b as usize] {
            u8::MAX => None,
            i => Some(self.chars[i as usize]),
        }
    }
}

/// How a pack's native words are packed (see the module comment). Copy: a table is interned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packing {
    /// The 128 code points from this one become 0x80–0xFF.
    Block(char),
    /// The script's character table (engine 8).
    Table(&'static CharTable),
}

impl Packing {
    /// One byte per character; None if the word has a character the packing has no byte for.
    pub fn pack(&self, word: &str) -> Option<Vec<u8>> {
        word.chars()
            .map(|c| match c {
                ZWNJ => Some(0x01),
                ZWJ => Some(0x02),
                '\'' => Some(b'\''),
                _ => match self {
                    Packing::Block(base) => {
                        let offset = (c as u32).checked_sub(*base as u32)?;
                        (offset < 0x80).then(|| 0x80 + offset as u8)
                    }
                    Packing::Table(table) => table.byte(c),
                },
            })
            .collect()
    }

    /// The inverse of `pack`; None for bytes `pack` never writes.
    pub fn unpack(&self, bytes: &[u8]) -> Option<String> {
        bytes
            .iter()
            .map(|&b| match b {
                0x01 => Some(ZWNJ),
                0x02 => Some(ZWJ),
                b'\'' => Some('\''),
                _ => match self {
                    Packing::Block(base) => match b {
                        0x80..=0xFF => char::from_u32(*base as u32 + u32::from(b - 0x80)),
                        _ => None,
                    },
                    Packing::Table(table) => table.char_of(b),
                },
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Urdu's letters, which span more than 128 code points of the Arabic block.
    fn urdu() -> &'static CharTable {
        let mut chars: Vec<char> = ('\u{0621}'..='\u{063A}').chain('\u{0641}'..='\u{064A}').collect();
        chars.extend("ٹپچڈڑژکگںھہۂۃیےۓ".chars());
        CharTable::intern(chars).unwrap()
    }

    #[test]
    fn table_bytes_are_distinct_and_never_reserved_or_capitals() {
        let mut seen = std::collections::HashSet::new();
        for &b in &TABLE_BYTES {
            assert!(seen.insert(b), "{b:#x} twice");
            assert!(b >= 0x03 && b != b'\'' && !b.is_ascii_uppercase(), "{b:#x}");
        }
        assert_eq!(CharTable::MAX, 225);
        assert_eq!(&TABLE_BYTES[..2], &[0x80, 0x81]);
        assert_eq!(TABLE_BYTES[128], 0x03);
    }

    #[test]
    fn a_wide_table_round_trips() {
        let p = Packing::Table(urdu());
        for word in ["میں", "ہے", "کہ", "گھر", "پاکستان", "بڑا", "ڈاکٹر", "ہوں\u{200C}گے", "ک'"] {
            let packed = p.pack(word).unwrap_or_else(|| panic!("{word}"));
            assert_eq!(packed.len(), word.chars().count(), "{word}");
            assert!(!packed.contains(&0), "{word}");
            assert!(!packed.iter().any(u8::is_ascii_uppercase), "{word}");
            assert_eq!(p.unpack(&packed).as_deref(), Some(word));
        }
        assert_eq!(p.pack("کa"), None, "a Latin letter");
        assert_eq!(p.pack("\u{064B}"), None, "a sign the table leaves out");
        assert_eq!(p.unpack(&[0xFF]), None, "a byte past the table");
    }

    #[test]
    fn every_byte_of_a_full_table_round_trips() {
        let chars: Vec<char> = ('\u{4E00}'..).take(CharTable::MAX).collect();
        let table = CharTable::intern(chars.clone()).unwrap();
        let p = Packing::Table(table);
        let word: String = chars.iter().collect();
        let packed = p.pack(&word).unwrap();
        assert_eq!(packed, TABLE_BYTES);
        assert_eq!(p.unpack(&packed), Some(word));
        let mut one_more = chars;
        one_more.push('\u{9FFF}');
        assert!(CharTable::intern(one_more).unwrap_err().contains("at most 225"));
    }

    #[test]
    fn a_block_packs_as_before() {
        let p = Packing::Block('\u{0D00}');
        assert_eq!(p.pack("എന്റെ"), Some(b"\x8E\xA8\xCD\xB1\xC6".to_vec()));
        assert_eq!(p.pack("क"), None);
        assert_eq!(p.unpack(b"\x8E\xA8\xCD\xB1\xC6").as_deref(), Some("എന്റെ"));
        assert_eq!(p.unpack(b"\x03"), None);
    }

    #[test]
    fn tables_refuse_what_cant_be_packed() {
        for (chars, want) in [
            (vec![], "no characters"),
            (vec!['ک', 'ک'], "twice"),
            (vec!['ک', 'a'], "ASCII"),
            (vec!['ک', ZWNJ], "joiners"),
        ] {
            let err = CharTable::intern(chars).unwrap_err();
            assert!(err.contains(want), "{err}");
        }
    }

    #[test]
    fn interning_reuses_a_table() {
        assert!(std::ptr::eq(urdu(), urdu()));
    }
}

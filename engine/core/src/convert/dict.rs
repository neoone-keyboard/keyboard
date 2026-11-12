//! The conversion dictionary (the `conversionLexicon` role): reading → entries of (surface,
//! left id, right id, cost), as a phrase converter's lattice needs them. One file, memory-mapped
//! and read in place:
//!
//! ```text
//! 0   "NEOJDIC1"
//! 8   u32 ids         how many part-of-speech ids the connection matrix has
//! 12  u32 unknown     the id given to a kana no entry covers
//! 16  u64 fst         length of the FST section
//! 24  u64 entries     length of the entries section
//! 32  u64 strings     length of the strings section
//! 40  [u8; (ids+7)/8] ids that join the segment before them (particles, auxiliaries,
//!                     suffixes), bit i of byte i/8
//! ..  FST: reading (UTF-8) → offset of its entries
//! ..  entries: per reading, varint count, then per entry varint (lid << 1 | rid == lid),
//!     varint rid (when it differs), varint cost, varint surface: 0 the reading itself,
//!     1 the reading in katakana, n + 2 the string at offset n
//! ..  strings: varint length, UTF-8
//! ```
//!
//! Little-endian. Entries of a reading are in order of cost, cheapest first.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use fst::raw::{Fst, Output};
use memmap2::Mmap;

use super::romaji::to_katakana;
use crate::lexicon::Bytes;

const MAGIC: &[u8; 8] = b"NEOJDIC1";
const HEADER: usize = 40;

/// What an entry's surface is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// The reading itself (hiragana).
    Reading,
    /// The reading in katakana.
    Katakana,
    /// A string of the strings section, by offset.
    Pooled(u32),
}

/// One dictionary entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub lid: u16,
    pub rid: u16,
    pub cost: u16,
    pub surface: Surface,
}

/// One row of the dictionary's source, for `build`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub reading: String,
    pub lid: u16,
    pub rid: u16,
    pub cost: u16,
    pub surface: String,
}

/// A slice of the file, for the FST.
struct Section {
    bytes: Arc<Bytes>,
    start: usize,
    end: usize,
}

impl AsRef<[u8]> for Section {
    fn as_ref(&self) -> &[u8] {
        &(*self.bytes).as_ref()[self.start..self.end]
    }
}

pub struct Dictionary {
    bytes: Arc<Bytes>,
    fst: Fst<Section>,
    ids: u32,
    unknown: u16,
    entries: (usize, usize),
    strings: (usize, usize),
}

impl std::fmt::Debug for Dictionary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dictionary")
            .field("readings", &self.fst.len())
            .field("ids", &self.ids)
            .finish()
    }
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], at: usize) -> usize {
    usize::try_from(u64::from_le_bytes(b[at..at + 8].try_into().unwrap())).unwrap_or(usize::MAX)
}

/// The varint at `b[*at..]`, moving `at` past it; None past the end or over 32 bits.
fn varint(b: &[u8], at: &mut usize) -> Option<u32> {
    let mut value: u32 = 0;
    for shift in (0..35).step_by(7) {
        let byte = *b.get(*at)?;
        *at += 1;
        value |= u32::from(byte & 0x7F).checked_shl(shift)?;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn put_varint(out: &mut Vec<u8>, mut v: u32) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

impl Dictionary {
    /// Maps the file at `path` read-only.
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        // SAFETY: pack files are read-only once installed; nothing writes them while mapped.
        let mmap = unsafe { Mmap::map(&file) }.map_err(|e| format!("{}: {e}", path.display()))?;
        Self::from_backing(Bytes::Mapped(mmap))
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        Self::from_backing(Bytes::Owned(bytes))
    }

    fn from_backing(bytes: Bytes) -> Result<Self, String> {
        let b = bytes.as_ref();
        if b.len() < HEADER || &b[..8] != MAGIC {
            return Err("not a conversion dictionary".into());
        }
        let ids = u32_at(b, 8);
        let unknown = u32_at(b, 12);
        if ids == 0 || ids > u32::from(u16::MAX) || unknown >= ids {
            return Err(format!(
                "conversion dictionary: {ids} ids, unknown {unknown}"
            ));
        }
        let flags = (ids as usize).div_ceil(8);
        let lens = [u64_at(b, 16), u64_at(b, 24), u64_at(b, 32)];
        let fst_start = HEADER + flags;
        let total = lens
            .iter()
            .try_fold(fst_start, |sum, len| sum.checked_add(*len))
            .ok_or("conversion dictionary: sections overflow")?;
        if total != b.len() {
            return Err(format!(
                "conversion dictionary: sections end at {total}, the file at {}",
                b.len()
            ));
        }
        let entries = (fst_start + lens[0], fst_start + lens[0] + lens[1]);
        let strings = (entries.1, total);
        let bytes = Arc::new(bytes);
        let section = Section {
            bytes: Arc::clone(&bytes),
            start: fst_start,
            end: entries.0,
        };
        let fst = Fst::new(section).map_err(|e| format!("conversion dictionary: {e}"))?;
        Ok(Self {
            bytes,
            fst,
            ids,
            unknown: unknown as u16,
            entries,
            strings,
        })
    }

    fn data(&self) -> &[u8] {
        (*self.bytes).as_ref()
    }

    /// How many part-of-speech ids there are (the connection matrix's size).
    pub fn ids(&self) -> u32 {
        self.ids
    }

    /// The id of a kana no entry covers.
    pub fn unknown_id(&self) -> u16 {
        self.unknown
    }

    /// How many readings there are.
    pub fn readings(&self) -> usize {
        self.fst.len()
    }

    /// Whether words of part-of-speech `id` join the segment before them.
    pub fn joins_before(&self, id: u16) -> bool {
        let b = self.data();
        let i = usize::from(id);
        i < self.ids as usize && b[HEADER + i / 8] & (1 << (i % 8)) != 0
    }

    /// Every reading that `kana` starts with: (its length in bytes, offset of its entries),
    /// shortest first.
    pub fn prefixes(&self, kana: &str) -> Vec<(usize, u32)> {
        let mut found = Vec::new();
        let mut node = self.fst.root();
        let mut out = Output::zero();
        for (i, &byte) in kana.as_bytes().iter().enumerate() {
            let Some(t) = node.find_input(byte) else {
                break;
            };
            let t = node.transition(t);
            out = out.cat(t.out);
            node = self.fst.node(t.addr);
            if node.is_final() && kana.is_char_boundary(i + 1) {
                let value = out.cat(node.final_output()).value();
                found.push((i + 1, value as u32));
            }
        }
        found
    }

    /// The entries of `reading`, cheapest first; empty when it has none.
    pub fn lookup(&self, reading: &str) -> Vec<Entry> {
        let Some(offset) = self.fst.get(reading).map(|o| o.value() as u32) else {
            return Vec::new();
        };
        self.entries_at(offset)
    }

    /// The entries at `offset` (from `prefixes`).
    pub fn entries_at(&self, offset: u32) -> Vec<Entry> {
        let mut out = Vec::new();
        self.each_entry(offset, |e| out.push(e));
        out
    }

    /// Calls `f` on each entry at `offset`, cheapest first.
    pub fn each_entry(&self, offset: u32, mut f: impl FnMut(Entry)) {
        let b = &self.data()[self.entries.0..self.entries.1];
        let mut at = offset as usize;
        let Some(count) = varint(b, &mut at) else {
            return;
        };
        for _ in 0..count {
            let Some(head) = varint(b, &mut at) else {
                return;
            };
            let lid = (head >> 1) as u16;
            let rid = if head & 1 == 1 {
                lid
            } else {
                match varint(b, &mut at) {
                    Some(r) => r as u16,
                    None => return,
                }
            };
            let (Some(cost), Some(code)) = (varint(b, &mut at), varint(b, &mut at)) else {
                return;
            };
            let surface = match code {
                0 => Surface::Reading,
                1 => Surface::Katakana,
                n => Surface::Pooled(n - 2),
            };
            f(Entry {
                lid,
                rid,
                cost: cost.min(u32::from(u16::MAX)) as u16,
                surface,
            });
        }
    }

    /// The surface `s` of an entry whose reading is `reading`.
    pub fn surface(&self, reading: &str, s: Surface) -> String {
        match s {
            Surface::Reading => reading.to_string(),
            Surface::Katakana => to_katakana(reading),
            Surface::Pooled(offset) => {
                let b = &self.data()[self.strings.0..self.strings.1];
                let mut at = offset as usize;
                let Some(len) = varint(b, &mut at) else {
                    return String::new();
                };
                b.get(at..at + len as usize)
                    .and_then(|s| std::str::from_utf8(s).ok())
                    .unwrap_or_default()
                    .to_string()
            }
        }
    }
}

/// The dictionary file for `rows`, with `ids` part-of-speech ids, `unknown` the id of an
/// uncovered kana and `joins` the ids that join the segment before them. Rows with the same
/// reading, surface and ids keep the lowest cost.
pub fn build(rows: &[Row], ids: u32, unknown: u16, joins: &[u16]) -> Result<Vec<u8>, String> {
    if ids == 0 || ids > u32::from(u16::MAX) || u32::from(unknown) >= ids {
        return Err(format!("{ids} ids, unknown {unknown}"));
    }
    let mut by_reading: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for row in rows {
        if u32::from(row.lid) >= ids || u32::from(row.rid) >= ids {
            return Err(format!("{row:?}: an id past {ids}"));
        }
        if row.reading.is_empty() || row.surface.is_empty() {
            return Err(format!("{row:?}: empty"));
        }
        by_reading.entry(&row.reading).or_default().push(row);
    }
    let mut strings: Vec<u8> = Vec::new();
    let mut pooled: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    let mut entries: Vec<u8> = Vec::new();
    let mut fst = fst::MapBuilder::memory();
    for (reading, mut rows) in by_reading {
        rows.sort_by(|a, b| {
            (a.cost, &a.surface, a.lid, a.rid).cmp(&(b.cost, &b.surface, b.lid, b.rid))
        });
        let mut seen = std::collections::HashSet::new();
        rows.retain(|r| seen.insert((&r.surface, r.lid, r.rid)));
        fst.insert(reading, entries.len() as u64)
            .map_err(|e| e.to_string())?;
        put_varint(&mut entries, rows.len() as u32);
        let katakana = to_katakana(reading);
        for row in rows {
            let same = row.lid == row.rid;
            put_varint(&mut entries, u32::from(row.lid) << 1 | u32::from(same));
            if !same {
                put_varint(&mut entries, u32::from(row.rid));
            }
            put_varint(&mut entries, u32::from(row.cost));
            let code = if row.surface == reading {
                0
            } else if row.surface == katakana {
                1
            } else {
                let offset = *pooled.entry(&row.surface).or_insert_with(|| {
                    let at = strings.len() as u32;
                    put_varint(&mut strings, row.surface.len() as u32);
                    strings.extend_from_slice(row.surface.as_bytes());
                    at
                });
                offset + 2
            };
            put_varint(&mut entries, code);
        }
    }
    let fst = fst.into_inner().map_err(|e| e.to_string())?;
    let mut flags = vec![0u8; (ids as usize).div_ceil(8)];
    for &id in joins {
        if u32::from(id) >= ids {
            return Err(format!("joining id {id} past {ids}"));
        }
        flags[usize::from(id) / 8] |= 1 << (id % 8);
    }
    let mut out =
        Vec::with_capacity(HEADER + flags.len() + fst.len() + entries.len() + strings.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&ids.to_le_bytes());
    out.extend_from_slice(&u32::from(unknown).to_le_bytes());
    for len in [fst.len(), entries.len(), strings.len()] {
        out.extend_from_slice(&(len as u64).to_le_bytes());
    }
    out.extend_from_slice(&flags);
    out.extend_from_slice(&fst);
    out.extend_from_slice(&entries);
    out.extend_from_slice(&strings);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(reading: &str, lid: u16, rid: u16, cost: u16, surface: &str) -> Row {
        Row {
            reading: reading.into(),
            lid,
            rid,
            cost,
            surface: surface.into(),
        }
    }

    fn small() -> Dictionary {
        let rows = [
            row("こう", 3, 3, 500, "公"),
            row("こう", 4, 5, 300, "高"),
            row("こうえん", 3, 3, 2000, "公園"),
            row("こうえん", 3, 3, 2500, "講演"),
            row("こうえん", 3, 3, 2400, "講演"),
            row("こうえん", 3, 3, 9000, "こうえん"),
            row("こうえん", 3, 3, 9100, "コウエン"),
            row("に", 7, 7, 100, "に"),
            row("に", 3, 3, 4000, "二"),
            row("ら", 3, 3, 9000, "ら"),
        ];
        Dictionary::from_bytes(build(&rows, 10, 3, &[7]).unwrap()).unwrap()
    }

    #[test]
    fn looks_up_a_reading_cheapest_first() {
        let d = small();
        assert_eq!(d.readings(), 4);
        assert_eq!(d.ids(), 10);
        assert_eq!(d.unknown_id(), 3);
        let e = d.lookup("こうえん");
        let surfaces: Vec<String> = e.iter().map(|e| d.surface("こうえん", e.surface)).collect();
        // 講演 twice with the same ids keeps the cheaper.
        assert_eq!(surfaces, ["公園", "講演", "こうえん", "コウエン"]);
        assert_eq!(e[1].cost, 2400);
        assert_eq!(e[2].surface, Surface::Reading);
        assert_eq!(e[3].surface, Surface::Katakana);
        let e = d.lookup("こう");
        assert_eq!((e[0].lid, e[0].rid, e[0].cost), (4, 5, 300));
        assert_eq!(d.surface("こう", e[0].surface), "高");
        assert!(d.lookup("こ").is_empty());
        assert!(d.lookup("ない").is_empty());
    }

    #[test]
    fn finds_every_reading_a_text_starts_with() {
        let d = small();
        let found: Vec<usize> = d.prefixes("こうえんに").iter().map(|p| p.0).collect();
        assert_eq!(found, ["こう".len(), "こうえん".len()]);
        let (_, offset) = d.prefixes("こうえんに")[1];
        assert_eq!(d.entries_at(offset), d.lookup("こうえん"));
        assert!(d.prefixes("ぬ").is_empty());
        assert!(d.prefixes("").is_empty());
    }

    #[test]
    fn knows_which_ids_join_the_segment_before() {
        let d = small();
        assert!(d.joins_before(7));
        assert!(!d.joins_before(3));
        assert!(!d.joins_before(99));
    }

    #[test]
    fn refuses_bad_files_and_rows() {
        assert!(Dictionary::from_bytes(b"nope".to_vec()).is_err());
        let mut good = build(&[row("あ", 1, 1, 1, "亜")], 4, 0, &[]).unwrap();
        good.push(0);
        assert!(Dictionary::from_bytes(good).is_err(), "trailing byte");
        assert!(build(&[row("あ", 9, 1, 1, "亜")], 4, 0, &[]).is_err());
        assert!(build(&[row("", 1, 1, 1, "亜")], 4, 0, &[]).is_err());
        assert!(build(&[], 4, 4, &[]).is_err());
        assert!(build(&[], 4, 0, &[5]).is_err());
    }

    #[test]
    fn varints_round_trip() {
        for v in [0u32, 1, 127, 128, 300, 16_383, 16_384, u32::MAX] {
            let mut b = Vec::new();
            put_varint(&mut b, v);
            let mut at = 0;
            assert_eq!(varint(&b, &mut at), Some(v));
            assert_eq!(at, b.len());
        }
        let mut at = 0;
        assert_eq!(varint(&[0x80], &mut at), None);
    }
}

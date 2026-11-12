//! The connection matrix (the `connection` role): the cost of a word whose right id is `rid`
//! followed by one whose left id is `lid`. One file, memory-mapped and read in place:
//!
//! ```text
//! 0   "NEOJCON1"
//! 8   u16 ids          the matrix is ids × ids
//! 10  u16 resolution   a stored byte b is the cost b × resolution
//! 12  u32 0
//! 16  u8 × ids × ids   by rid, then lid
//! ```
//!
//! One byte a cost: the source's costs (Mozc's, at most about 15,000) are rounded to a multiple
//! of `resolution`, as Mozc's own compact matrix does, which loses nothing a ranking notices
//! (the steps are well under one word's cost difference) and makes the matrix 7 MB, not 14.
//!
//! Mozc's `connection_single_column.txt` lists its costs in the same order, by rid then lid.
//! (Read the other way, the eval's sentences converted exactly fall from 66% to 10%:
//! `tests/ja_eval.rs`.)

use std::fs::File;
use std::path::Path;

use memmap2::Mmap;

use crate::lexicon::Bytes;

const MAGIC: &[u8; 8] = b"NEOJCON1";
const HEADER: usize = 16;

pub struct Connection {
    bytes: Bytes,
    ids: u16,
    resolution: u16,
}

impl std::fmt::Debug for Connection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Connection")
            .field("ids", &self.ids)
            .finish()
    }
}

impl Connection {
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
            return Err("not a connection matrix".into());
        }
        let ids = u16::from_le_bytes([b[8], b[9]]);
        let resolution = u16::from_le_bytes([b[10], b[11]]);
        let want = HEADER + usize::from(ids) * usize::from(ids);
        if ids == 0 || resolution == 0 || b.len() != want {
            return Err(format!(
                "connection matrix: {ids} ids, {} bytes, {want} wanted",
                b.len()
            ));
        }
        Ok(Self {
            bytes,
            ids,
            resolution,
        })
    }

    pub fn ids(&self) -> u16 {
        self.ids
    }

    /// The cost of `rid` followed by `lid`. Ids past the matrix cost the most it holds.
    pub fn cost(&self, rid: u16, lid: u16) -> i32 {
        if rid >= self.ids || lid >= self.ids {
            return 255 * i32::from(self.resolution);
        }
        let at = HEADER + usize::from(rid) * usize::from(self.ids) + usize::from(lid);
        i32::from(self.bytes.as_ref()[at]) * i32::from(self.resolution)
    }
}

/// The matrix file for `costs` (ids × ids, by rid then lid), each rounded to a multiple of the
/// smallest resolution that keeps the largest under 256 steps.
pub fn build(ids: u16, costs: &[i32]) -> Result<Vec<u8>, String> {
    let n = usize::from(ids);
    if ids == 0 || costs.len() != n * n {
        return Err(format!("{} costs for {ids} ids", costs.len()));
    }
    if let Some(c) = costs.iter().find(|c| **c < 0) {
        return Err(format!("negative cost {c}"));
    }
    let max = costs.iter().copied().max().unwrap_or(0);
    let resolution =
        u16::try_from((max / 255 + 1).max(1)).map_err(|_| format!("cost {max} too large"))?;
    let r = i32::from(resolution);
    let mut out = Vec::with_capacity(HEADER + costs.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&ids.to_le_bytes());
    out.extend_from_slice(&resolution.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend(costs.iter().map(|c| ((c + r / 2) / r).min(255) as u8));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn costs_round_trip_to_the_resolution() {
        let costs = [0, 100, 5000, 15_281, 64, 63, 7, 2000, 9];
        let m = Connection::from_bytes(build(3, &costs).unwrap()).unwrap();
        assert_eq!(m.ids(), 3);
        // 15,281 / 255 + 1 = 60.
        assert_eq!(m.resolution, 60);
        for rid in 0..3u16 {
            for lid in 0..3u16 {
                let want = costs[usize::from(rid) * 3 + usize::from(lid)];
                let got = m.cost(rid, lid);
                assert!((got - want).abs() <= 30, "{rid} {lid}: {got} for {want}");
            }
        }
        assert_eq!(m.cost(0, 0), 0);
        assert_eq!(m.cost(1, 0), 15_300, "by rid, then lid: costs[3]");
        assert_eq!(m.cost(9, 0), 255 * 60);
    }

    #[test]
    fn refuses_bad_files() {
        assert!(Connection::from_bytes(b"NEOJCON1".to_vec()).is_err());
        let mut b = build(2, &[1, 2, 3, 4]).unwrap();
        b.pop();
        assert!(Connection::from_bytes(b).is_err());
        assert!(build(2, &[1, 2, 3]).is_err());
        assert!(build(1, &[-1]).is_err());
    }
}

//! The int8 model file (`.mlkbnn`), written by `training/mlkb/translit/export.py`.
//!
//! All integers and floats are little-endian. Layout:
//!
//! | Bytes | Field |
//! |---|---|
//! | 8 | magic `MLKBNN01` |
//! | 4 | `u32` version, currently 1 |
//! | 4 | `u32` kind: 1 = encoder–decoder transliterator |
//! | 32 | `u32` × 8: `d`, `heads`, `enc_layers`, `dec_layers`, `ff`, `max_len`, `src_vocab`, `tgt_vocab` |
//! | 4 | `u32` tensor count |
//! | … | the tensors, one after another (below) |
//! | 8 | `u64` FNV-1a checksum of every byte before it |
//!
//! Each tensor:
//! - `u16` name length, then the name in UTF-8: PyTorch's `state_dict` key, exactly;
//! - `u8` dtype: 0 = f32, 1 = int8 with one f32 scale per row (2-D only);
//! - `u8` ndim, then `u32` dims × ndim;
//! - zero bytes up to the next offset that is a multiple of 16 (from the start of the file);
//! - the data. f32: the values, row-major. int8: `rows` f32 scales, then `rows × cols` int8
//!   values, row-major; element `[r, c]` is `scales[r] * values[r * cols + c]`.
//!
//! Matrices (linear weights, embeddings) are int8, quantised symmetrically per row
//! (`scale = max |row| / 127`). Biases and layer-norm parameters are f32.
//!
//! The file is memory-mapped and checked once on open. The f32 tensors and the scales, which
//! are small, are copied out then; the int8 values are read in place from the mapping.

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use memmap2::Mmap;

use crate::lexicon::Bytes;

/// The first 8 bytes of every model file.
pub const MAGIC: &[u8; 8] = b"MLKBNN01";

/// The only format version this reader accepts.
pub const VERSION: u32 = 1;

/// `kind` of the encoder–decoder transliterator.
pub const KIND_TRANSLIT: u32 = 1;

/// Every tensor's data starts at a multiple of this many bytes.
pub const ALIGN: usize = 16;

const DTYPE_F32: u8 = 0;
const DTYPE_I8: u8 = 1;
const HEADER_LEN: usize = 8 + 4 * 11;
const CHECKSUM_LEN: usize = 8;

/// The model's shape, from the file header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// What the tensors describe; `KIND_TRANSLIT` is the only kind so far.
    pub kind: u32,
    /// Model width.
    pub d: usize,
    /// Attention heads; `d` is a multiple of it.
    pub heads: usize,
    /// Encoder layers.
    pub enc_layers: usize,
    /// Decoder layers.
    pub dec_layers: usize,
    /// Feed-forward width.
    pub ff: usize,
    /// Positions per sequence (encoder and decoder each).
    pub max_len: usize,
    /// Source (Latin) vocabulary size.
    pub src_vocab: usize,
    /// Target (Malayalam) vocabulary size.
    pub tgt_vocab: usize,
}

/// Why a model file was rejected.
#[derive(Debug)]
pub enum NnError {
    /// The file couldn't be read or mapped.
    Io(std::io::Error),
    /// The file doesn't start with `MAGIC`.
    BadMagic,
    /// A format version this reader doesn't know.
    Version(u32),
    /// The file ends before its tensors and checksum do.
    Truncated,
    /// The checksum doesn't match the contents.
    Checksum,
    /// The contents are structurally wrong (bad dtype, odd dims, bytes after the checksum…).
    Malformed(String),
    /// A tensor the model needs isn't in the file.
    Missing(String),
    /// A tensor has the wrong shape or dtype for the header's config.
    Shape(String),
}

impl fmt::Display for NnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "model I/O error: {e}"),
            Self::BadMagic => write!(f, "not a model file (bad magic)"),
            Self::Version(v) => write!(f, "unsupported model file version {v}"),
            Self::Truncated => write!(f, "model file is truncated"),
            Self::Checksum => write!(f, "model file checksum mismatch"),
            Self::Malformed(why) => write!(f, "malformed model file: {why}"),
            Self::Missing(name) => write!(f, "model file has no tensor {name}"),
            Self::Shape(why) => write!(f, "model tensor has the wrong shape: {why}"),
        }
    }
}

impl std::error::Error for NnError {}

impl From<std::io::Error> for NnError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// One tensor, borrowed from a `ModelFile`.
#[derive(Debug, Clone, Copy)]
pub enum TensorRef<'a> {
    /// f32 values, row-major.
    F32 {
        dims: &'a [usize],
        values: &'a [f32],
    },
    /// A `rows × cols` int8 matrix. `data` holds the int8 values as two's-complement bytes
    /// (read each with `as i8`), row-major; element `[r, c]` is `scales[r] * data[r*cols+c]`.
    I8 {
        scales: &'a [f32],
        data: &'a [u8],
        rows: usize,
        cols: usize,
    },
}

impl TensorRef<'_> {
    /// The tensor's dimensions.
    pub fn dims(&self) -> Vec<usize> {
        match *self {
            Self::F32 { dims, .. } => dims.to_vec(),
            Self::I8 { rows, cols, .. } => vec![rows, cols],
        }
    }

    /// Every value as f32, row-major (int8 matrices dequantised).
    pub fn to_f32(&self) -> Vec<f32> {
        match *self {
            Self::F32 { values, .. } => values.to_vec(),
            Self::I8 {
                scales, data, cols, ..
            } => data
                .chunks_exact(cols.max(1))
                .zip(scales)
                .flat_map(|(row, &s)| row.iter().map(move |&b| s * f32::from(b as i8)))
                .collect(),
        }
    }
}

#[derive(Debug)]
enum Data {
    F32(Vec<f32>),
    I8 {
        scales: Vec<f32>,
        values: Range<usize>,
    },
}

#[derive(Debug)]
struct Entry {
    dims: Vec<usize>,
    data: Data,
}

struct Inner {
    config: Config,
    bytes: Bytes,
    names: Vec<String>,
    tensors: HashMap<String, Entry>,
}

/// An open, verified model file. Cloning is cheap: clones share the mapping.
#[derive(Clone)]
pub struct ModelFile(Arc<Inner>);

impl fmt::Debug for ModelFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelFile")
            .field("config", &self.0.config)
            .field("tensors", &self.0.names.len())
            .field("bytes", &self.0.bytes.as_ref().len())
            .finish()
    }
}

impl ModelFile {
    /// Maps `path` read-only, checks the magic, version, layout and checksum, and indexes the
    /// tensors by name.
    pub fn open(path: &Path) -> Result<Self, NnError> {
        let file = File::open(path)?;
        // SAFETY: model files are read-only resources inside the signed app bundle (or build
        // outputs in tests); nothing truncates or writes them while they are mapped.
        let mmap = unsafe { Mmap::map(&file)? };
        Self::from_backing(Bytes::Mapped(mmap))
    }

    /// Like `open`, from bytes in memory.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, NnError> {
        Self::from_backing(Bytes::Owned(bytes))
    }

    fn from_backing(bytes: Bytes) -> Result<Self, NnError> {
        let (config, names, tensors) = parse(bytes.as_ref())?;
        Ok(Self(Arc::new(Inner {
            config,
            bytes,
            names,
            tensors,
        })))
    }

    /// The model's shape.
    pub fn config(&self) -> Config {
        self.0.config
    }

    /// Tensor names, in file order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.0.names.iter().map(String::as_str)
    }

    /// The tensor called `name` (a PyTorch `state_dict` key).
    pub fn tensor(&self, name: &str) -> Option<TensorRef<'_>> {
        let entry = self.0.tensors.get(name)?;
        Some(match &entry.data {
            Data::F32(values) => TensorRef::F32 {
                dims: &entry.dims,
                values,
            },
            Data::I8 { scales, values } => TensorRef::I8 {
                scales,
                data: &self.0.bytes.as_ref()[values.clone()],
                rows: entry.dims[0],
                cols: entry.dims[1],
            },
        })
    }
}

/// 64-bit FNV-1a.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h = (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Reads little-endian fields with bounds checks; running past the end is `Truncated`.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], NnError> {
        let end = self.pos.checked_add(n).ok_or(NnError::Truncated)?;
        let out = self.bytes.get(self.pos..end).ok_or(NnError::Truncated)?;
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, NnError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, NnError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, NnError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn usize(&mut self) -> Result<usize, NnError> {
        Ok(self.u32()? as usize)
    }

    fn f32s(&mut self, n: usize) -> Result<Vec<f32>, NnError> {
        let len = n.checked_mul(4).ok_or(NnError::Truncated)?;
        Ok(self
            .take(len)?
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
            .collect())
    }
}

type Parsed = (Config, Vec<String>, HashMap<String, Entry>);

fn parse(bytes: &[u8]) -> Result<Parsed, NnError> {
    if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != MAGIC {
        return Err(if bytes.len() < MAGIC.len() {
            NnError::Truncated
        } else {
            NnError::BadMagic
        });
    }
    if bytes.len() < HEADER_LEN + CHECKSUM_LEN {
        return Err(NnError::Truncated);
    }
    let body_len = bytes.len() - CHECKSUM_LEN;
    let mut r = Reader {
        bytes: &bytes[..body_len],
        pos: MAGIC.len(),
    };
    let version = r.u32()?;
    if version != VERSION {
        return Err(NnError::Version(version));
    }
    let config = Config {
        kind: r.u32()?,
        d: r.usize()?,
        heads: r.usize()?,
        enc_layers: r.usize()?,
        dec_layers: r.usize()?,
        ff: r.usize()?,
        max_len: r.usize()?,
        src_vocab: r.usize()?,
        tgt_vocab: r.usize()?,
    };
    let count = r.usize()?;
    let mut names = Vec::new();
    let mut tensors = HashMap::new();
    for _ in 0..count {
        let name_len = usize::from(r.u16()?);
        let name = std::str::from_utf8(r.take(name_len)?)
            .map_err(|_| NnError::Malformed("tensor name is not UTF-8".into()))?
            .to_string();
        let dtype = r.u8()?;
        let ndim = usize::from(r.u8()?);
        let dims = (0..ndim)
            .map(|_| r.usize())
            .collect::<Result<Vec<_>, _>>()?;
        r.take(r.pos.next_multiple_of(ALIGN) - r.pos)?;
        let size = dims
            .iter()
            .try_fold(1usize, |acc, &d| acc.checked_mul(d))
            .ok_or_else(|| NnError::Malformed(format!("{name}: dims overflow")))?;
        let data = match dtype {
            DTYPE_F32 => Data::F32(r.f32s(size)?),
            DTYPE_I8 if ndim == 2 => {
                let scales = r.f32s(dims[0])?;
                let start = r.pos;
                r.take(size)?;
                Data::I8 {
                    scales,
                    values: start..r.pos,
                }
            }
            DTYPE_I8 => {
                return Err(NnError::Malformed(format!(
                    "{name}: int8 tensor is {ndim}-D"
                )));
            }
            other => return Err(NnError::Malformed(format!("{name}: unknown dtype {other}"))),
        };
        if tensors.insert(name.clone(), Entry { dims, data }).is_some() {
            return Err(NnError::Malformed(format!("{name}: duplicate tensor")));
        }
        names.push(name);
    }
    if r.pos != body_len {
        return Err(NnError::Malformed(format!(
            "{} bytes between the last tensor and the checksum",
            body_len - r.pos
        )));
    }
    let stored = u64::from_le_bytes(bytes[body_len..].try_into().unwrap());
    if stored != fnv1a(&bytes[..body_len]) {
        return Err(NnError::Checksum);
    }
    Ok((config, names, tensors))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(out: &mut Vec<u8>) {
        while !out.len().is_multiple_of(ALIGN) {
            out.push(0);
        }
    }

    /// A file written by hand: `norm.bias` = f32 [3], `emb.weight` = int8 [2, 3].
    fn tiny() -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        for v in [VERSION, KIND_TRANSLIT, 4, 2, 1, 1, 8, 16, 29, 133, 2] {
            out.extend(v.to_le_bytes());
        }
        out.extend(9u16.to_le_bytes());
        out.extend(b"norm.bias");
        out.extend([DTYPE_F32, 1]);
        out.extend(3u32.to_le_bytes());
        pad(&mut out);
        for v in [0.5f32, -1.0, 2.25] {
            out.extend(v.to_le_bytes());
        }
        out.extend(10u16.to_le_bytes());
        out.extend(b"emb.weight");
        out.extend([DTYPE_I8, 2]);
        out.extend(2u32.to_le_bytes());
        out.extend(3u32.to_le_bytes());
        pad(&mut out);
        for s in [0.5f32, 0.25] {
            out.extend(s.to_le_bytes());
        }
        out.extend([1i8, -2, 127, -127, 0, 4].map(|v| v as u8));
        let sum = fnv1a(&out);
        out.extend(sum.to_le_bytes());
        out
    }

    #[test]
    fn reads_config_names_shapes_and_values() {
        let file = ModelFile::from_bytes(tiny()).unwrap();
        assert_eq!(
            file.config(),
            Config {
                kind: KIND_TRANSLIT,
                d: 4,
                heads: 2,
                enc_layers: 1,
                dec_layers: 1,
                ff: 8,
                max_len: 16,
                src_vocab: 29,
                tgt_vocab: 133,
            }
        );
        assert_eq!(
            file.names().collect::<Vec<_>>(),
            ["norm.bias", "emb.weight"]
        );
        let bias = file.tensor("norm.bias").unwrap();
        assert_eq!(bias.dims(), [3]);
        assert_eq!(bias.to_f32(), [0.5, -1.0, 2.25]);
        let emb = file.tensor("emb.weight").unwrap();
        assert_eq!(emb.dims(), [2, 3]);
        let TensorRef::I8 { scales, data, .. } = emb else {
            panic!("emb.weight should be int8");
        };
        assert_eq!(scales, [0.5, 0.25]);
        assert_eq!(data[1] as i8, -2);
        assert_eq!(emb.to_f32(), [0.5, -1.0, 63.5, -31.75, 0.0, 1.0]);
        assert!(file.tensor("missing").is_none());
    }

    #[test]
    fn opens_a_file_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tiny.mlkbnn");
        std::fs::write(&path, tiny()).unwrap();
        let file = ModelFile::open(&path).unwrap();
        assert_eq!(
            file.tensor("norm.bias").unwrap().to_f32(),
            [0.5, -1.0, 2.25]
        );
    }

    #[test]
    fn rejects_a_flipped_byte() {
        let mut bytes = tiny();
        let i = bytes.len() - 12; // inside the int8 values
        bytes[i] ^= 0x10;
        assert!(matches!(
            ModelFile::from_bytes(bytes),
            Err(NnError::Checksum)
        ));
    }

    #[test]
    fn rejects_a_bad_magic_and_version() {
        let mut bytes = tiny();
        bytes[0] = b'X';
        assert!(matches!(
            ModelFile::from_bytes(bytes),
            Err(NnError::BadMagic)
        ));
        let mut bytes = tiny();
        bytes[8] = 2;
        assert!(matches!(
            ModelFile::from_bytes(bytes),
            Err(NnError::Version(2))
        ));
    }

    #[test]
    fn rejects_a_truncated_file() {
        let bytes = tiny();
        for len in [0, 4, 20, HEADER_LEN + 3, bytes.len() - 20, bytes.len() - 1] {
            let err = ModelFile::from_bytes(bytes[..len].to_vec()).unwrap_err();
            assert!(
                matches!(err, NnError::Truncated | NnError::Malformed(_)),
                "{len}: {err}"
            );
        }
    }

    #[test]
    fn fnv1a_matches_the_reference_values() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}

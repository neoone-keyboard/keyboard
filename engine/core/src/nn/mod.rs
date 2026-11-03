//! Small neural models on the CPU: the int8 model file (`file`), the numeric kernels (`ops`),
//! the transliterator's encoder–decoder forward pass (`transformer`), its token tables (`vocab`)
//! and the lexicon-constrained beam search over it (`translit`).
//!
//! `training/mlkb/translit/export.py` writes the files; nothing here depends on PyTorch.

pub mod file;
pub mod ops;
pub mod transformer;
pub mod translit;
pub mod vocab;

pub use file::{Config, ModelFile, NnError, TensorRef};
pub use transformer::{Decoder, DecoderState, Encoded, Transformer};
pub use translit::{BeamTrace, Deadline, Search, Translit, Transliteration};

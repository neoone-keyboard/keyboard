//! Small neural models on the CPU: the int8 model file (`file`), the numeric kernels (`ops`)
//! and the transliterator's encoder–decoder forward pass (`transformer`).
//!
//! `training/mlkb/translit/export.py` writes the files; nothing here depends on PyTorch.

pub mod file;
pub mod ops;
pub mod transformer;

pub use file::{Config, ModelFile, NnError, TensorRef};
pub use transformer::{Decoder, DecoderState, Encoded, Transformer};

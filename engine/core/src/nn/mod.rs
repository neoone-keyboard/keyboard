//! Small neural models on the CPU: the int8 model file (`file`), the numeric kernels (`ops`),
//! the transliterator's encoder–decoder forward pass (`transformer`), its token tables (`vocab`)
//! and the lexicon-constrained beam search over it (`translit`); the language model's
//! tokeniser and text form (`tokenizer`) and its forward pass with a context cache (`lm`).
//!
//! `training/mlkb/translit/export.py` writes the files; nothing here depends on PyTorch.

pub mod file;
pub mod lm;
pub mod ops;
pub mod tokenizer;
pub mod transformer;
pub mod translit;
pub mod vocab;
mod weights;

pub use file::{Config, ModelFile, NnError, TensorRef};
pub use lm::{Cache, Lm, LmScratch, Tables};
pub use tokenizer::{Tokenizer, lm_normalize};
pub use transformer::{Decoder, DecoderState, Encoded, Transformer};
pub use translit::{BeamTrace, Deadline, Search, Translit, Transliteration};

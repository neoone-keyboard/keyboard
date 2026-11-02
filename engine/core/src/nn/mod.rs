//! Small neural models on the CPU: the int8 model file (`file`).
//!
//! `training/mlkb/translit/export.py` writes the files; nothing here depends on PyTorch.

pub mod file;

pub use file::{Config, ModelFile, NnError, TensorRef};

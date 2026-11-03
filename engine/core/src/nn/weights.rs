//! Int8 matrices, linear layers and layer norms resolved from a `.mlkbnn`, shared by the
//! transliterator (`transformer`) and the language model (`lm`).

use std::ops::Range;

use super::file::{ModelFile, NnError, TensorRef};
use super::ops::{QMatrix, layer_norm};

/// An int8 matrix resolved from the file: its scales (copied) and where its values are.
#[derive(Debug)]
pub(crate) struct Mat {
    pub(crate) scales: Vec<f32>,
    pub(crate) values: Range<usize>,
    pub(crate) rows: usize,
    pub(crate) cols: usize,
}

impl Mat {
    /// The matrix, borrowed from `file` (the one it was resolved from).
    pub(crate) fn q<'a>(&'a self, file: &'a ModelFile) -> QMatrix<'a> {
        QMatrix {
            data: &file.bytes()[self.values.clone()],
            scales: &self.scales,
            rows: self.rows,
            cols: self.cols,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Linear {
    pub(crate) weight: Mat,
    pub(crate) bias: Vec<f32>,
}

#[derive(Debug)]
pub(crate) struct Norm {
    pub(crate) weight: Vec<f32>,
    pub(crate) bias: Vec<f32>,
}

/// Resolves tensors by name, checking dtype and shape.
pub(crate) struct Loader<'a>(pub(crate) &'a ModelFile);

impl Loader<'_> {
    pub(crate) fn mat(&self, name: &str, rows: usize, cols: usize) -> Result<Mat, NnError> {
        match self.0.tensor(name) {
            Some(TensorRef::I8 {
                scales,
                rows: r,
                cols: c,
                ..
            }) if (r, c) == (rows, cols) => Ok(Mat {
                scales: scales.to_vec(),
                values: self.0.int8_range(name).expect("int8 tensor has a range"),
                rows,
                cols,
            }),
            Some(t) => Err(NnError::Shape(format!(
                "{name}: expected int8 [{rows}, {cols}], found {:?} {}",
                t.dims(),
                match t {
                    TensorRef::F32 { .. } => "f32",
                    TensorRef::I8 { .. } => "int8",
                    TensorRef::Bytes(_) => "bytes",
                }
            ))),
            None => Err(NnError::Missing(name.into())),
        }
    }

    pub(crate) fn vector(&self, name: &str, len: usize) -> Result<Vec<f32>, NnError> {
        match self.0.tensor(name) {
            Some(TensorRef::F32 { dims, values }) if dims == [len] => Ok(values.to_vec()),
            Some(t) => Err(NnError::Shape(format!(
                "{name}: expected f32 [{len}], found {:?}",
                t.dims()
            ))),
            None => Err(NnError::Missing(name.into())),
        }
    }

    pub(crate) fn linear(&self, prefix: &str, rows: usize, cols: usize) -> Result<Linear, NnError> {
        Ok(Linear {
            weight: self.mat(&format!("{prefix}.weight"), rows, cols)?,
            bias: self.vector(&format!("{prefix}.bias"), rows)?,
        })
    }

    pub(crate) fn norm(&self, prefix: &str, d: usize) -> Result<Norm, NnError> {
        Ok(Norm {
            weight: self.vector(&format!("{prefix}.weight"), d)?,
            bias: self.vector(&format!("{prefix}.bias"), d)?,
        })
    }
}

/// `nn.LayerNorm` on each `d`-long row of `x`.
pub(crate) fn norm_rows(norm: &Norm, x: &[f32], out: &mut [f32], d: usize) {
    for (xr, or) in x.chunks_exact(d).zip(out.chunks_exact_mut(d)) {
        layer_norm(xr, &norm.weight, &norm.bias, or);
    }
}

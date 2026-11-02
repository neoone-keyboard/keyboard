//! The numeric kernels of the forward pass: int8 matrix products, layer norm, softmax, ReLU.
//!
//! Single-threaded plain Rust. The inner loops keep `LANES` independent accumulators, so LLVM
//! can vectorise them without reordering float additions on its own.

use std::ops::Range;

/// PyTorch's default `layer_norm_eps`, used by every norm in the model.
pub const LAYER_NORM_EPS: f32 = 1e-5;

const LANES: usize = 16;

/// A borrowed `rows × cols` int8 matrix with one f32 scale per row: element `[r, c]` is
/// `scales[r] * (data[r * cols + c] as i8)`.
#[derive(Debug, Clone, Copy)]
pub struct QMatrix<'a> {
    /// The int8 values as two's-complement bytes, row-major.
    pub data: &'a [u8],
    /// One scale per row.
    pub scales: &'a [f32],
    /// Row count.
    pub rows: usize,
    /// Column count.
    pub cols: usize,
}

impl<'a> QMatrix<'a> {
    /// The matrix made of rows `rows` of this one.
    pub fn rows(&self, rows: Range<usize>) -> QMatrix<'a> {
        QMatrix {
            data: &self.data[rows.start * self.cols..rows.end * self.cols],
            scales: &self.scales[rows.clone()],
            rows: rows.len(),
            cols: self.cols,
        }
    }

    /// Dequantises row `r` into `out` (`cols` long).
    pub fn row_into(&self, r: usize, out: &mut [f32]) {
        let s = self.scales[r];
        let row = &self.data[r * self.cols..(r + 1) * self.cols];
        for (o, &q) in out.iter_mut().zip(row) {
            *o = s * f32::from(q as i8);
        }
    }

    /// Adds dequantised row `r` to `out`.
    pub fn add_row(&self, r: usize, out: &mut [f32]) {
        let s = self.scales[r];
        let row = &self.data[r * self.cols..(r + 1) * self.cols];
        for (o, &q) in out.iter_mut().zip(row) {
            *o += s * f32::from(q as i8);
        }
    }
}

/// `a * b + c`, fused where the CPU has FMA in its base instruction set (every aarch64 CPU,
/// so every iPhone), where it also vectorises; elsewhere a plain multiply and add, because
/// without the `fma` target feature `f32::mul_add` is a slow library call.
#[inline(always)]
fn mul_add(a: f32, b: f32, c: f32) -> f32 {
    #[cfg(target_arch = "aarch64")]
    {
        a.mul_add(b, c)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        a * b + c
    }
}

/// The dot product of two equal-length vectors.
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    let mut acc = [0f32; LANES];
    let (a_chunks, b_chunks) = (a.chunks_exact(LANES), b.chunks_exact(LANES));
    let tail: f32 = a_chunks
        .remainder()
        .iter()
        .zip(b_chunks.remainder())
        .map(|(x, y)| x * y)
        .sum();
    for (x, y) in a_chunks.zip(b_chunks) {
        for i in 0..LANES {
            acc[i] = mul_add(x[i], y[i], acc[i]);
        }
    }
    acc.iter().sum::<f32>() + tail
}

/// `out[i] = w · x[i] + bias` for each of the `n` row vectors in `x` (`n × w.cols`), giving
/// `out` as `n × w.rows`. Each weight row is converted to f32 once, into `row_buf`
/// (at least `w.cols` long), and then used for all `n` inputs.
pub fn matmul_i8(
    w: QMatrix<'_>,
    bias: Option<&[f32]>,
    x: &[f32],
    n: usize,
    out: &mut [f32],
    row_buf: &mut [f32],
) {
    let (rows, cols) = (w.rows, w.cols);
    debug_assert_eq!(x.len(), n * cols);
    debug_assert_eq!(out.len(), n * rows);
    let row_buf = &mut row_buf[..cols];
    for r in 0..rows {
        for (o, &q) in row_buf.iter_mut().zip(&w.data[r * cols..(r + 1) * cols]) {
            *o = f32::from(q as i8);
        }
        let (s, b) = (w.scales[r], bias.map_or(0.0, |b| b[r]));
        for i in 0..n {
            out[i * rows + r] = s * dot(row_buf, &x[i * cols..(i + 1) * cols]) + b;
        }
    }
}

/// `out = (x - mean) / sqrt(var + eps) * weight + bias`, as `nn.LayerNorm` over one vector.
pub fn layer_norm(x: &[f32], weight: &[f32], bias: &[f32], out: &mut [f32]) {
    let n = x.len() as f32;
    let mean = x.iter().sum::<f32>() / n;
    let var = x.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n;
    let inv = 1.0 / (var + LAYER_NORM_EPS).sqrt();
    for (((o, &v), &w), &b) in out.iter_mut().zip(x).zip(weight).zip(bias) {
        *o = (v - mean) * inv * w + b;
    }
}

/// Softmax in place, shifted by the maximum so large inputs don't overflow.
pub fn softmax(x: &mut [f32]) {
    let max = x.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut sum = 0.0;
    for v in x.iter_mut() {
        *v = (*v - max).exp();
        sum += *v;
    }
    let inv = 1.0 / sum;
    for v in x.iter_mut() {
        *v *= inv;
    }
}

/// `max(x, 0)` in place.
pub fn relu(x: &mut [f32]) {
    for v in x.iter_mut() {
        *v = v.max(0.0);
    }
}

/// `dst += src`.
pub fn add_assign(dst: &mut [f32], src: &[f32]) {
    for (d, &s) in dst.iter_mut().zip(src) {
        *d += s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &[f32], b: &[f32], tol: f32) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
    }

    /// A 5 × 37 int8 matrix (37 columns exercises the remainder path of `dot`).
    fn matrix() -> (Vec<u8>, Vec<f32>, usize, usize) {
        let (rows, cols) = (5, 37);
        let data = (0..rows * cols)
            .map(|i| (((i * 37 + 11) % 255) as i32 - 127) as i8 as u8)
            .collect();
        let scales = vec![0.01, 0.02, 0.5, 0.0, 1.0];
        (data, scales, rows, cols)
    }

    #[test]
    fn matmul_i8_matches_a_naive_f32_product() {
        let (data, scales, rows, cols) = matrix();
        let w = QMatrix {
            data: &data,
            scales: &scales,
            rows,
            cols,
        };
        let bias: Vec<f32> = (0..rows).map(|r| r as f32 * 0.5 - 1.0).collect();
        let n = 3;
        let x: Vec<f32> = (0..n * cols)
            .map(|i| ((i % 13) as f32 - 6.0) / 7.0)
            .collect();
        let mut want = vec![0.0; n * rows];
        for i in 0..n {
            for r in 0..rows {
                let mut acc = bias[r] as f64;
                for c in 0..cols {
                    let wv = scales[r] as f64 * f64::from(data[r * cols + c] as i8);
                    acc += wv * x[i * cols + c] as f64;
                }
                want[i * rows + r] = acc as f32;
            }
        }
        let mut out = vec![0.0; n * rows];
        let mut buf = vec![0.0; cols];
        matmul_i8(w, Some(&bias), &x, n, &mut out, &mut buf);
        assert!(close(&out, &want, 1e-4), "{out:?} vs {want:?}");

        // One vector, no bias, a row range: the same rows of the same product.
        let mut one = vec![0.0; 2];
        matmul_i8(
            w.rows(1..3),
            None,
            &x[cols..2 * cols],
            1,
            &mut one,
            &mut buf,
        );
        let expect = [want[rows + 1] - bias[1], want[rows + 2] - bias[2]];
        assert!(close(&one, &expect, 1e-4), "{one:?} vs {expect:?}");
    }

    #[test]
    fn rows_dequantise() {
        let (data, scales, rows, cols) = matrix();
        let w = QMatrix {
            data: &data,
            scales: &scales,
            rows,
            cols,
        };
        let mut row = vec![0.0; cols];
        w.row_into(2, &mut row);
        assert_eq!(row[0], 0.5 * f32::from(data[2 * cols] as i8));
        w.add_row(2, &mut row);
        assert_eq!(row[0], 1.0 * f32::from(data[2 * cols] as i8));
    }

    #[test]
    fn dot_handles_every_length() {
        for len in [0, 1, 15, 16, 17, 64, 100] {
            let a: Vec<f32> = (0..len).map(|i| i as f32 * 0.25).collect();
            let b: Vec<f32> = (0..len).map(|i| 1.0 - i as f32 * 0.125).collect();
            let want: f32 = a.iter().zip(&b).map(|(x, y)| x * y).sum();
            assert!(
                (dot(&a, &b) - want).abs() <= 1e-3 * want.abs().max(1.0),
                "{len}"
            );
        }
    }

    #[test]
    fn layer_norm_normalises_then_scales() {
        let x = [1.0, 2.0, 3.0, 4.0];
        let mut out = [0.0; 4];
        layer_norm(&x, &[1.0; 4], &[0.0; 4], &mut out);
        let mean: f32 = out.iter().sum::<f32>() / 4.0;
        let var: f32 = out.iter().map(|v| v * v).sum::<f32>() / 4.0;
        assert!(mean.abs() < 1e-6);
        assert!((var - 1.0).abs() < 1e-4);
        // 1.25 is the variance of x: out = (x - 2.5) / sqrt(1.25 + eps).
        assert!((out[0] - (-1.5 / (1.25f32 + LAYER_NORM_EPS).sqrt())).abs() < 1e-6);
        layer_norm(&x, &[2.0, 2.0, 1.0, 1.0], &[0.5; 4], &mut out);
        assert!((out[3] - (1.5 / (1.25f32 + LAYER_NORM_EPS).sqrt() + 0.5)).abs() < 1e-6);
    }

    #[test]
    fn softmax_is_stable_with_large_inputs() {
        let mut x = [1000.0, 1001.0, 1002.0];
        softmax(&mut x);
        assert!(x.iter().all(|v| v.is_finite()));
        assert!((x.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        let mut y = [0.0, 1.0, 2.0];
        softmax(&mut y);
        assert!(close(&x, &y, 1e-6));
        let mut z = [-1e30, 0.0];
        softmax(&mut z);
        assert_eq!(z, [0.0, 1.0]);
    }

    #[test]
    fn relu_and_add() {
        let mut x = [-1.0, 0.0, 2.5];
        relu(&mut x);
        assert_eq!(x, [0.0, 0.0, 2.5]);
        add_assign(&mut x, &[1.0, 1.0, 1.0]);
        assert_eq!(x, [1.0, 1.0, 3.5]);
    }
}

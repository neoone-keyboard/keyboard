//! The numeric kernels of the forward pass: int8 matrix products, layer norm, softmax, ReLU.
//!
//! Single-threaded. The float loops are plain Rust that keep `LANES` independent accumulators,
//! so LLVM can vectorise them without reordering float additions on its own. The matrix
//! product quantises its inputs and runs on int8 dot products: a NEON kernel on aarch64
//! (`neon`), with a plain Rust fallback.

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

/// Activations are quantised per row to `round(x / scale)`, 22 bits, held as this many int8
/// limbs: balanced base-128 digits, most significant first. Products then run on int8 dot
/// products and are exact but for that rounding. Each limb costs a full int8 product, but two
/// (15 bits) miss PyTorch by several times float rounding (the `nn_reference` fixture fails);
/// three match it as the f32 path did. `quantise` and `matmul_i8` spell out the three.
const LIMBS: usize = 3;
/// The largest quantised magnitude, `127 * 128² + 63 * 128 + 63`: its top digit is 127.
const QUANT_MAX: f32 = 2_088_895.0;

/// Scratch for `matmul_i8`: its input rows, quantised, and the products. Grows to the
/// largest call and is then reused, so products don't allocate.
#[derive(Debug, Default)]
pub struct QuantBuf {
    /// "Limb rows", `cols` wide: input `j`'s limb `l` is row `j * LIMBS + l`.
    limbs: Vec<i8>,
    /// Per input row, the value of one quantisation step.
    scales: Vec<f32>,
    /// `dots[v * rows + r]`: limb row `v` · weight row `r`.
    dots: Vec<i32>,
    /// One output row before scaling.
    acc: Vec<f32>,
}

impl QuantBuf {
    /// Quantises the `n` rows of `x` (`n × cols`) into `n * LIMBS` limb rows.
    fn quantise(&mut self, x: &[f32], n: usize, cols: usize) {
        self.limbs.resize(n * LIMBS * cols, 0);
        self.scales.resize(n, 0.0);
        for ((row, limbs), scale) in x
            .chunks_exact(cols)
            .zip(self.limbs.chunks_exact_mut(LIMBS * cols))
            .zip(&mut self.scales)
        {
            let max = row.iter().fold(0f32, |m, v| m.max(v.abs()));
            *scale = max / QUANT_MAX;
            let inv = if max > 0.0 { QUANT_MAX / max } else { 0.0 };
            let (top, low) = limbs.split_at_mut(cols);
            let (mid, low) = low.split_at_mut(cols);
            for (((&v, t), m), l) in row.iter().zip(top).zip(mid).zip(low) {
                // Least significant digit first: each in -64..=63, but the top one, which
                // QUANT_MAX keeps in -127..=127.
                let q = (v * inv).round() as i32;
                let d2 = ((q + 64) & 127) - 64;
                let q = (q - d2) >> 7;
                let d1 = ((q + 64) & 127) - 64;
                (*t, *m, *l) = (((q - d1) >> 7) as i8, d1 as i8, d2 as i8);
            }
        }
    }
}

/// `out[i] = w · x[i] + bias` for each of the `n` row vectors in `x` (`n × w.cols`), giving
/// `out` as `n × w.rows`. The inputs are quantised to int8 limbs in `buf`; each output is then
/// an exact integer dot product of int8 weights and quantised inputs, scaled once.
pub fn matmul_i8(
    w: QMatrix<'_>,
    bias: Option<&[f32]>,
    x: &[f32],
    n: usize,
    out: &mut [f32],
    buf: &mut QuantBuf,
) {
    let (rows, cols) = (w.rows, w.cols);
    assert_eq!(x.len(), n * cols);
    assert_eq!(out.len(), n * rows);
    assert_eq!(w.data.len(), rows * cols);
    // Each limb's dot product then stays below 2^24, so f32 holds it exactly.
    assert!(cols < 1040, "{cols} columns");
    buf.quantise(x, n, cols);
    buf.dots.resize(n * LIMBS * rows, 0);
    buf.acc.resize(rows, 0.0);
    limb_dots(int8(w.data), cols, &buf.limbs, &mut buf.dots);
    for (j, out) in out.chunks_exact_mut(rows).enumerate() {
        let limb = |l: usize| &buf.dots[(j * LIMBS + l) * rows..][..rows];
        let (top, mid, low) = (limb(0), limb(1), limb(2));
        for (r, a) in buf.acc.iter_mut().enumerate() {
            // The digits' weights are powers of two: only the last addition rounds.
            *a = (top[r] as f32 * 16384.0 + mid[r] as f32 * 128.0) + low[r] as f32;
        }
        let sx = buf.scales[j];
        for (r, (o, &a)) in out.iter_mut().zip(&buf.acc).enumerate() {
            *o = a * (w.scales[r] * sx) + bias.map_or(0.0, |b| b[r]);
        }
    }
}

/// The file's two's-complement bytes as `i8`.
fn int8(bytes: &[u8]) -> &[i8] {
    // SAFETY: u8 and i8 have the same size and alignment, and every bit pattern is valid.
    unsafe { std::slice::from_raw_parts(bytes.as_ptr().cast(), bytes.len()) }
}

/// The int8 product kernel this CPU runs, "sdot" or "scalar": for diagnostics.
pub fn kernel() -> &'static str {
    #[cfg(target_arch = "aarch64")]
    if std::arch::is_aarch64_feature_detected!("dotprod") {
        return "sdot";
    }
    "scalar"
}

/// `dots[v * rows + r]` = limb row `v` of `limbs` · weight row `r` of `w`, both `cols` wide.
fn limb_dots(w: &[i8], cols: usize, limbs: &[i8], dots: &mut [i32]) {
    #[cfg(target_arch = "aarch64")]
    if cols.is_multiple_of(16) && std::arch::is_aarch64_feature_detected!("dotprod") {
        // SAFETY: the CPU has dotprod, checked just above, and `cols` is a multiple of 16.
        unsafe { neon::limb_dots(w, cols, limbs, dots) };
        return;
    }
    limb_dots_scalar(w, cols, limbs, 0, dots);
}

/// `limb_dots` for weight rows `from..` in plain Rust: the fallback, and the last
/// `rows % 4` rows that the NEON kernel leaves.
fn limb_dots_scalar(w: &[i8], cols: usize, limbs: &[i8], from: usize, dots: &mut [i32]) {
    let rows = w.len() / cols;
    for (v, limb) in limbs.chunks_exact(cols).enumerate() {
        for r in from..rows {
            dots[v * rows + r] = w[r * cols..(r + 1) * cols]
                .iter()
                .zip(limb)
                .map(|(&a, &b)| i32::from(a) * i32::from(b))
                .sum();
        }
    }
}

/// The product kernel on `sdot` (Armv8.2 dotprod; the A13 and later, every Apple Silicon Mac):
/// one instruction sums four products of int8 pairs into each of four i32 lanes.
#[cfg(target_arch = "aarch64")]
mod neon {
    use std::arch::aarch64::{int8x16_t, int32x4_t, vdupq_n_s32, vld1q_s8, vpaddq_s32, vst1q_s32};
    use std::arch::asm;

    use super::limb_dots_scalar;

    /// Weight rows per block; its four sums come out as one vector.
    const ROWS: usize = 4;

    /// `acc += a · b` in each 4-byte group. Inline assembly, as the `vdotq_s32` intrinsic is
    /// not stable yet; only valid inside a `dotprod` function.
    #[inline(always)]
    unsafe fn sdot(mut acc: int32x4_t, a: int8x16_t, b: int8x16_t) -> int32x4_t {
        // SAFETY: register-only arithmetic; the caller guarantees the CPU has dotprod.
        unsafe {
            asm!(
                "sdot {acc:v}.4s, {a:v}.16b, {b:v}.16b",
                acc = inout(vreg) acc,
                a = in(vreg) a,
                b = in(vreg) b,
                options(pure, nomem, nostack, preserves_flags),
            );
        }
        acc
    }

    /// `super::limb_dots` in blocks of 4 weight rows by up to 4 limb rows (16 accumulators);
    /// the last `rows % 4` rows go to the scalar kernel.
    ///
    /// # Safety
    /// The CPU must have dotprod and `cols` must be a multiple of 16.
    #[target_feature(enable = "dotprod")]
    pub unsafe fn limb_dots(w: &[i8], cols: usize, limbs: &[i8], dots: &mut [i32]) {
        let (rows, limb_rows) = (w.len() / cols, limbs.len() / cols);
        assert!(cols.is_multiple_of(16) && dots.len() == rows * limb_rows);
        for r in (0..rows - rows % ROWS).step_by(ROWS) {
            for v in (0..limb_rows).step_by(4) {
                // SAFETY (all arms): dotprod as above; rows r.. and limb rows v.. are in
                // bounds.
                unsafe {
                    match limb_rows - v {
                        1 => block::<1>(w, cols, limbs, r, v, dots),
                        2 => block::<2>(w, cols, limbs, r, v, dots),
                        3 => block::<3>(w, cols, limbs, r, v, dots),
                        _ => block::<4>(w, cols, limbs, r, v, dots),
                    }
                }
            }
        }
        limb_dots_scalar(w, cols, limbs, rows - rows % ROWS, dots);
    }

    /// Weight rows `r .. r + 4` by limb rows `v .. v + B`, 16 columns a step.
    ///
    /// # Safety
    /// The CPU must have dotprod; `cols` must be a multiple of 16 and the rows in bounds.
    #[target_feature(enable = "dotprod")]
    unsafe fn block<const B: usize>(
        w: &[i8],
        cols: usize,
        limbs: &[i8],
        r: usize,
        v: usize,
        dots: &mut [i32],
    ) {
        let rows = w.len() / cols;
        assert!(r + ROWS <= rows && (v + B) * cols <= limbs.len());
        let (wp, xp) = (w[r * cols..].as_ptr(), limbs[v * cols..].as_ptr());
        let mut acc = [[vdupq_n_s32(0); ROWS]; B];
        for c in (0..cols).step_by(16) {
            // SAFETY: every load is 16 bytes at column c < cols (a multiple of 16) of a row
            // asserted in bounds.
            unsafe {
                let mut wv = [vld1q_s8(wp.add(c)); ROWS];
                for (k, row) in wv.iter_mut().enumerate().skip(1) {
                    *row = vld1q_s8(wp.add(k * cols + c));
                }
                for (b, acc) in acc.iter_mut().enumerate() {
                    let xv = vld1q_s8(xp.add(b * cols + c));
                    for (a, &row) in acc.iter_mut().zip(&wv) {
                        *a = sdot(*a, row, xv);
                    }
                }
            }
        }
        for (b, [a0, a1, a2, a3]) in acc.into_iter().enumerate() {
            // Pairwise adds reduce the four rows' accumulators to their four sums, in order.
            let sums = vpaddq_s32(vpaddq_s32(a0, a1), vpaddq_s32(a2, a3));
            let out = &mut dots[(v + b) * rows + r..][..ROWS];
            // SAFETY: `out` holds four i32.
            unsafe { vst1q_s32(out.as_mut_ptr(), sums) };
        }
    }
}

/// Multi-head attention of one query `q` (`d`) over `t` rows of `kv`, where row `j`'s key is
/// `kv[j*stride + k_off ..][..d]` and its value `kv[j*stride + v_off ..][..d]`. Writes `d`
/// values to `out`. `scores` needs `t` entries.
#[allow(clippy::too_many_arguments)]
pub fn attend(
    q: &[f32],
    kv: &[f32],
    (k_off, v_off, stride): (usize, usize, usize),
    t: usize,
    heads: usize,
    scores: &mut [f32],
    out: &mut [f32],
) {
    let d = q.len();
    let dh = d / heads;
    let scale = 1.0 / (dh as f32).sqrt();
    let scores = &mut scores[..t];
    out.fill(0.0);
    for h in 0..heads {
        let cols = h * dh..(h + 1) * dh;
        let qh = &q[cols.clone()];
        for (j, s) in scores.iter_mut().enumerate() {
            let k = &kv[j * stride + k_off + cols.start..][..dh];
            *s = dot(qh, k) * scale;
        }
        softmax(scores);
        let oh = &mut out[cols.clone()];
        for (j, &p) in scores.iter().enumerate() {
            let v = &kv[j * stride + v_off + cols.start..][..dh];
            for (o, &vv) in oh.iter_mut().zip(v) {
                *o += p * vv;
            }
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
        let mut buf = QuantBuf::default();
        matmul_i8(w, Some(&bias), &x, n, &mut out, &mut buf);
        // The inputs' quantisation error, relative to the outputs, is well under 1e-5.
        let rel = |a: &[f32], b: &[f32]| {
            a.iter()
                .zip(b)
                .all(|(x, y)| (x - y).abs() <= 1e-5 * y.abs().max(1.0))
        };
        assert!(rel(&out, &want), "{out:?} vs {want:?}");

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
        assert!(rel(&one, &expect), "{one:?} vs {expect:?}");
    }

    #[test]
    fn quantised_inputs_keep_22_bits() {
        let x = [1.0, -0.5, 0.25, 1e-3, -1.0, 0.0, 0.7, -0.3, 0.123_456_7];
        let cols = x.len();
        let mut buf = QuantBuf::default();
        buf.quantise(&x, 1, cols);
        let limb = |l: usize, c: usize| buf.limbs[l * cols + c];
        for (c, &v) in x.iter().enumerate() {
            let q = (0..LIMBS).fold(0f32, |acc, l| acc * 128.0 + f32::from(limb(l, c)));
            // One step: v / scale itself rounds a little in f32.
            assert!((q * buf.scales[0] - v).abs() <= buf.scales[0], "{v}");
        }
        assert_eq!((limb(0, 0), limb(1, 0), limb(2, 0)), (127, 63, 63));
        assert_eq!((limb(0, 4), limb(1, 4), limb(2, 4)), (-127, -63, -63));
        buf.quantise(&[0.0; 4], 1, 4);
        assert_eq!(buf.scales[0], 0.0);
        assert!(buf.limbs[..LIMBS * 4].iter().all(|&q| q == 0));
    }

    /// Every block shape the NEON kernel has, and its tail rows, against the scalar kernel:
    /// both are exact, so they must agree to the bit. Without dotprod both are the scalar one.
    #[test]
    fn kernel_matches_the_scalar_fallback() {
        let cols = 48;
        for (rows, limb_rows) in [(1, 1), (3, 2), (4, 4), (5, 3), (8, 12), (9, 5), (16, 30)] {
            let w: Vec<i8> = (0..rows * cols)
                .map(|i| (((i * 73 + 5) % 255) as i32 - 127) as i8)
                .collect();
            let limbs: Vec<i8> = (0..limb_rows * cols)
                .map(|i| (((i * 31 + 17) % 255) as i32 - 127) as i8)
                .collect();
            let mut want = vec![0; rows * limb_rows];
            limb_dots_scalar(&w, cols, &limbs, 0, &mut want);
            let mut got = vec![0; rows * limb_rows];
            limb_dots(&w, cols, &limbs, &mut got);
            assert_eq!(got, want, "{rows} x {limb_rows}");
            assert!(want.iter().filter(|&&d| d != 0).count() > want.len() / 2);
        }
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

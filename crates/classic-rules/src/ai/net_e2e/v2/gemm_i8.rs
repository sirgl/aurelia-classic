//! Int8 matrix products of the v2 decisions (the `int8` switch of the
//! scorer, off by default): `y[m × n] ≈ x[m × k] · W + b` with the weights
//! quantized once per output column (symmetric, `max |w| / 127`) and each
//! input row per call (`max |x| / 127`), the products summed exactly in i32
//! with `sdot` (aarch64; the portable loop computes the same integers), then
//! scaled back to f32. The result is not the f32 product's: the agreement of
//! the decisions with the f32 path is measured (`v2_check --int8`).
// The kernels index rows, lanes and columns together.
#![allow(clippy::needless_range_loop)]

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

use super::weights::Lin;

/// A layer's int8 weights: blocks of 16 outputs, per block and per group of
/// 4 inputs (the inputs padded to a multiple of 16) 64 bytes, output `4q + r`
/// of the block and input `c` of the group at `16q + 4r + c`.
#[derive(Debug, Clone, Default)]
pub struct QLin {
    pub inp: usize,
    pub out: usize,
    /// Padded inputs / 4.
    groups: usize,
    w: Vec<i8>,
    /// Per output: its weights' scale.
    scale: Vec<f32>,
    pub b: Vec<f32>,
}

/// `n` rounded up to a multiple of 16.
fn pad16(n: usize) -> usize {
    n.next_multiple_of(16)
}

impl QLin {
    pub fn new(l: &Lin) -> Self {
        let (inp, out) = (l.inp, l.out);
        let groups = pad16(inp) / 4;
        let blocks = out.div_ceil(16);
        let mut scale = vec![0.0f32; out];
        for (j, s) in scale.iter_mut().enumerate() {
            let m = (0..inp).map(|k| l.wt[k * out + j].abs()).fold(0.0f32, f32::max);
            *s = if m > 0.0 { m / 127.0 } else { 1.0 };
        }
        let mut w = vec![0i8; blocks * groups * 64];
        for k in 0..inp {
            let (g, c) = (k / 4, k % 4);
            for j in 0..out {
                let q = (l.wt[k * out + j] / scale[j]).round().clamp(-127.0, 127.0) as i8;
                let (bl, qq, r) = (j / 16, (j % 16) / 4, j % 4);
                w[(bl * groups + g) * 64 + qq * 16 + r * 4 + c] = q;
            }
        }
        let b = if l.b.len() == out { l.b.clone() } else { vec![0.0; out] };
        QLin { inp, out, groups, w, scale, b }
    }

    /// `y = x · W + b` for `rows` rows (`xq`: scratch for the quantized rows).
    pub fn run(&self, x: &[f32], rows: usize, y: &mut [f32], xq: &mut Vec<i8>) {
        self.product(x, rows, y, xq, true);
    }

    /// `y += x · W` (no bias).
    pub fn add(&self, x: &[f32], rows: usize, y: &mut [f32], xq: &mut Vec<i8>) {
        self.product(x, rows, y, xq, false);
    }

    fn product(&self, x: &[f32], rows: usize, y: &mut [f32], xq: &mut Vec<i8>, set: bool) {
        let kp = self.groups * 4;
        xq.clear();
        xq.resize(rows * kp, 0);
        let mut sx = [0.0f32; 4];
        let mut r = 0;
        while r < rows {
            let rr = (rows - r).min(4);
            for i in 0..rr {
                let xr = &x[(r + i) * self.inp..(r + i + 1) * self.inp];
                let m = xr.iter().fold(0.0f32, |a, v| a.max(v.abs()));
                let s = if m > 0.0 { m / 127.0 } else { 1.0 };
                let inv = 1.0 / s;
                for (q, v) in xq[(r + i) * kp..(r + i) * kp + self.inp].iter_mut().zip(xr) {
                    *q = (v * inv).round().clamp(-127.0, 127.0) as i8;
                }
                sx[i] = s;
            }
            let mut sums = [[0i32; 16]; 4];
            for bl in 0..self.out.div_ceil(16) {
                self.block(&xq[r * kp..], kp, rr, bl, &mut sums);
                for i in 0..rr {
                    let yr = &mut y[(r + i) * self.out..(r + i + 1) * self.out];
                    for c in 0..16.min(self.out - bl * 16) {
                        let j = bl * 16 + c;
                        let v = sums[i][c] as f32 * (sx[i] * self.scale[j]);
                        yr[j] = if set { v + self.b[j] } else { yr[j] + v };
                    }
                }
            }
            r += rr;
        }
    }

    /// The integer sums of `rr` (≤ 4) quantized rows on output block `bl`.
    #[cfg(target_arch = "aarch64")]
    fn block(&self, xq: &[i8], kp: usize, rr: usize, bl: usize, sums: &mut [[i32; 16]; 4]) {
        assert!(xq.len() >= rr * kp && rr <= 4 && self.w.len() >= (bl + 1) * self.groups * 64);
        assert!(available(), "sdot on this CPU");
        // SAFETY: `dotprod` is present (checked above); the loads stay inside `xq[..rr kp]` (16 bytes at
        // multiples of 16 below kp) and the block's `groups · 64` weight bytes (checked above).
        unsafe { self.block_dot(xq, kp, rr, bl, sums) }
    }

    #[cfg(target_arch = "aarch64")]
    #[target_feature(enable = "dotprod")]
    unsafe fn block_dot(&self, xq: &[i8], kp: usize, rr: usize, bl: usize, sums: &mut [[i32; 16]; 4]) {
        let mut acc = [[vdupq_n_s32(0); 4]; 4];
        let wb = self.w.as_ptr().add(bl * self.groups * 64);
        let mut g = 0;
        while g < self.groups {
            let mut xv = [vdupq_n_s8(0); 4];
            for (i, v) in xv.iter_mut().enumerate().take(rr) {
                *v = vld1q_s8(xq.as_ptr().add(i * kp + g * 4));
            }
            macro_rules! group {
                ($l:literal) => {{
                    let p = wb.add((g + $l) * 64);
                    let w = [vld1q_s8(p), vld1q_s8(p.add(16)), vld1q_s8(p.add(32)), vld1q_s8(p.add(48))];
                    for i in 0..4 {
                        for q in 0..4 {
                            acc[i][q] = vdotq_laneq_s32::<$l>(acc[i][q], w[q], xv[i]);
                        }
                    }
                }};
            }
            group!(0);
            group!(1);
            group!(2);
            group!(3);
            g += 4;
        }
        for i in 0..rr {
            for q in 0..4 {
                vst1q_s32(sums[i].as_mut_ptr().add(4 * q), acc[i][q]);
            }
        }
    }

    #[cfg(not(target_arch = "aarch64"))]
    fn block(&self, xq: &[i8], kp: usize, rr: usize, bl: usize, sums: &mut [[i32; 16]; 4]) {
        for i in 0..rr {
            for c in 0..16 {
                let (q, r) = (c / 4, c % 4);
                let mut s = 0i32;
                for g in 0..self.groups {
                    for cc in 0..4 {
                        s += i32::from(self.w[(bl * self.groups + g) * 64 + q * 16 + r * 4 + cc]) * i32::from(xq[i * kp + g * 4 + cc]);
                    }
                }
                sums[i][c] = s;
            }
        }
    }
}

/// Whether this CPU has the int8 dot product (every Apple M-series chip).
pub fn available() -> bool {
    #[cfg(target_arch = "aarch64")]
    {
        std::arch::is_aarch64_feature_detected!("dotprod")
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_f32_product_within_quantization() {
        let (m, k, n) = (7, 70, 37);
        let x: Vec<f32> = (0..m * k).map(|i| ((i * 7 % 11) as f32 - 5.0) * 0.25).collect();
        let wt: Vec<f32> = (0..k * n).map(|i| ((i * 5 % 13) as f32 - 6.0) * 0.125).collect();
        let b: Vec<f32> = (0..n).map(|i| i as f32 * 0.5).collect();
        let l = Lin { inp: k, out: n, wt: wt.clone(), b: b.clone() };
        let q = QLin::new(&l);
        let mut y = vec![0.0; m * n];
        let mut xq = Vec::new();
        q.run(&x, m, &mut y, &mut xq);
        for r in 0..m {
            for c in 0..n {
                let want = b[c] + (0..k).map(|i| x[r * k + i] * wt[i * n + c]).sum::<f32>();
                assert!((y[r * n + c] - want).abs() < 0.05 * want.abs().max(1.0), "{r} {c}: {} {want}", y[r * n + c]);
            }
        }
    }
}

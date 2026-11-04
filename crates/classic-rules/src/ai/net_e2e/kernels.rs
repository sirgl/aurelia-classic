//! Exact integer kernels of the net's forward pass for aarch64 (NEON and
//! the int8 dot product `sdot`), used by `model.rs` when the CPU has them;
//! the portable code in `model.rs` computes the same integers and is the
//! reference (`tests/net_e2e_speed.rs` holds them equal).
//!
//! Nothing here rounds: every kernel returns exact integer sums, so the
//! result is the same bit for bit on every machine and in every order.
//!
//! - A dense layer's sums `Σ_k w[j][k] · x[k]` (int8 weights, i16 inputs):
//!   an input is split into bytes, `x = 256·hi + lo + 128` with `hi = x >> 8`
//!   and `lo = (x & 255) − 128`, both int8, so
//!   `Σ w·x = 256·Σ w·hi + Σ w·lo + 128·Σ w`; the two byte sums run on
//!   `sdot` in i32 (|Σ| ≤ 127·128·K, no overflow below 2^17 inputs), the
//!   rest in i64. The weights are packed at load time ([`Packed`]).
//! - Attention scores `q·k` (i16 × i16) in i32 lanes when the head's largest
//!   values bound the sum below 2^31, which the caller checks.
//! - Attention values `Σ_j e_j · v_j` (`e ≤ 2^15`) in i32 lanes, flushed to
//!   i64 every `chunk` keys, the chunk the caller derives from the values'
//!   bound.

/// The input of a dense layer is cut into parts (one, or the 7 tile states
/// of a hex convolution), each padded to a multiple of this (one NEON load).
pub(crate) const PAD: usize = 16;

/// `n` padded to a multiple of [`PAD`].
#[inline]
pub(crate) fn pad16(n: usize) -> usize {
    n.div_ceil(PAD) * PAD
}

/// Whether this CPU runs the kernels (aarch64 with `dotprod`; every Apple
/// M-series chip).
pub(crate) fn available() -> bool {
    #[cfg(target_arch = "aarch64")]
    {
        std::arch::is_aarch64_feature_detected!("dotprod")
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        false
    }
}

/// `x` as the high and low bytes of [`Packed`] (`x = 256·hi + lo + 128`);
/// `hi` and `lo` at least as long as `x`.
#[inline]
pub(crate) fn split(x: &[i16], hi: &mut [i8], lo: &mut [i8]) {
    for ((h, l), &v) in hi.iter_mut().zip(lo.iter_mut()).zip(x) {
        *h = (v >> 8) as i8;
        *l = (v as u8 ^ 0x80) as i8;
    }
}

/// `x` split into the padded layout of a one-part layer: `hi`, `lo` of
/// `pad16(x.len())`, the padding zero (its weights are zero).
#[inline]
pub(crate) fn split_padded(x: &[i16], hi: &mut [i8], lo: &mut [i8]) {
    let n = x.len();
    split(x, &mut hi[..n], &mut lo[..n]);
    hi[n..pad16(n)].fill(0);
    lo[n..pad16(n)].fill(0);
}

/// A dense layer's int8 weights packed for `sdot`: blocks of 16 outputs;
/// per block and per group of 4 padded inputs 64 bytes, output `4q + r` of
/// the block and input `c` of the group at `16q + 4r + c`.
#[derive(Debug, Clone)]
pub(crate) struct Packed {
    pub(crate) out: usize,
    blocks: usize,
    /// Padded inputs / 4.
    groups: usize,
    w: Vec<i8>,
    /// `128 · Σ_k w[j][k]` per output: the offset of the low bytes.
    offset: Vec<i64>,
}

impl Packed {
    /// From the input-major weights `wt[k * out + j]` of a layer of `inp`
    /// inputs read in parts of `part` (`inp` for one part), each part
    /// padded to [`pad16`].
    pub(crate) fn new(wt: &[i16], out: usize, inp: usize, part: usize) -> Self {
        let parts = inp.div_ceil(part.max(1)).max(1);
        let stride = pad16(part);
        let groups = parts * stride / 4;
        let blocks = out.div_ceil(16);
        let mut w = vec![0i8; blocks * groups * 64];
        let mut offset = vec![0i64; out];
        for k in 0..inp {
            let p = (k / part) * stride + k % part;
            let (g, c) = (p / 4, p % 4);
            for j in 0..out {
                let v = wt[k * out + j];
                let (b, q, r) = (j / 16, (j % 16) / 4, j % 4);
                w[(b * groups + g) * 64 + q * 16 + r * 4 + c] = v as i8;
                offset[j] += 128 * i64::from(v);
            }
        }
        Packed { out, blocks, groups, w, offset }
    }

    /// The padded inputs of one row (`4 · groups`).
    pub(crate) fn padded(&self) -> usize {
        self.groups * 4
    }
}

/// One input row as the kernels read it: its parts' high and low bytes in
/// order, each a multiple of [`PAD`] long, together `Packed::padded`.
pub(crate) type Parts<'a> = [(&'a [i8], &'a [i8])];

#[cfg(target_arch = "aarch64")]
pub(crate) use neon::{attention_values, scores_i32, sums};

#[cfg(not(target_arch = "aarch64"))]
pub(crate) fn sums<const R: usize>(_p: &Packed, _rows: [&Parts<'_>; R], _out: [&mut [i64]; R]) {
    unreachable!("the kernels run on aarch64 only (`available`)")
}

#[cfg(not(target_arch = "aarch64"))]
pub(crate) fn scores_i32(_q: &[i16], _kt: &[i16], _n_pad: usize, _shift: u32, _out: &mut [i32]) {
    unreachable!("the kernels run on aarch64 only (`available`)")
}

#[cfg(not(target_arch = "aarch64"))]
pub(crate) fn attention_values(_dh: usize, _e: &[i32], _rows: usize, _v: &[i32], _n: usize, _chunk: usize, _num: &mut [i64]) {
    unreachable!("the kernels run on aarch64 only (`available`)")
}

#[cfg(target_arch = "aarch64")]
mod neon {
    use super::{Packed, Parts, PAD};
    use std::arch::aarch64::*;

    /// The sums of `R` rows (`rows[r]` its parts) into `out[r]` (`p.out`
    /// values each): exact.
    pub(crate) fn sums<const R: usize>(p: &Packed, rows: [&Parts<'_>; R], out: [&mut [i64]; R]) {
        for r in 0..R {
            let total: usize = rows[r].iter().map(|(h, _)| h.len()).sum();
            assert!(total == p.padded() && rows[r].iter().all(|(h, l)| h.len() == l.len() && h.len() % PAD == 0), "rows of the packed layout");
            assert!(out[r].len() >= p.out, "a sum per output");
        }
        assert!(super::available(), "sdot on this CPU");
        // SAFETY: `dotprod` is present (checked above); every load below is
        // inside the slices checked above (parts of whole 16-byte chunks,
        // `blocks × groups × 64` packed weights).
        unsafe { sums_dot::<R>(p, rows, out) }
    }

    /// One group of 4 inputs (lane `G` of the 16 loaded) for every row.
    #[inline(always)]
    unsafe fn group<const G: i32, const R: usize>(acc: &mut [[int32x4_t; 8]; R], w: *const i8, xh: &[int8x16_t; R], xl: &[int8x16_t; R]) {
        let w0 = vld1q_s8(w);
        let w1 = vld1q_s8(w.add(16));
        let w2 = vld1q_s8(w.add(32));
        let w3 = vld1q_s8(w.add(48));
        for r in 0..R {
            acc[r][0] = vdotq_laneq_s32::<G>(acc[r][0], w0, xh[r]);
            acc[r][1] = vdotq_laneq_s32::<G>(acc[r][1], w1, xh[r]);
            acc[r][2] = vdotq_laneq_s32::<G>(acc[r][2], w2, xh[r]);
            acc[r][3] = vdotq_laneq_s32::<G>(acc[r][3], w3, xh[r]);
            acc[r][4] = vdotq_laneq_s32::<G>(acc[r][4], w0, xl[r]);
            acc[r][5] = vdotq_laneq_s32::<G>(acc[r][5], w1, xl[r]);
            acc[r][6] = vdotq_laneq_s32::<G>(acc[r][6], w2, xl[r]);
            acc[r][7] = vdotq_laneq_s32::<G>(acc[r][7], w3, xl[r]);
        }
    }

    #[target_feature(enable = "dotprod")]
    unsafe fn sums_dot<const R: usize>(p: &Packed, rows: [&Parts<'_>; R], out: [&mut [i64]; R]) {
        let n_parts = rows[0].len();
        for b in 0..p.blocks {
            let mut acc = [[vdupq_n_s32(0); 8]; R];
            let mut w = p.w.as_ptr().add(b * p.groups * 64);
            // The part indexes every row.
            #[allow(clippy::needless_range_loop)]
            for part in 0..n_parts {
                let len = rows[0][part].0.len();
                let mut at = 0;
                while at < len {
                    let mut xh = [vdupq_n_s8(0); R];
                    let mut xl = [vdupq_n_s8(0); R];
                    for r in 0..R {
                        xh[r] = vld1q_s8(rows[r][part].0.as_ptr().add(at));
                        xl[r] = vld1q_s8(rows[r][part].1.as_ptr().add(at));
                    }
                    group::<0, R>(&mut acc, w, &xh, &xl);
                    group::<1, R>(&mut acc, w.add(64), &xh, &xl);
                    group::<2, R>(&mut acc, w.add(128), &xh, &xl);
                    group::<3, R>(&mut acc, w.add(192), &xh, &xl);
                    w = w.add(256);
                    at += PAD;
                }
            }
            let first = b * 16;
            let n = (p.out - first).min(16);
            for r in 0..R {
                let mut h = [0i32; 16];
                let mut l = [0i32; 16];
                for q in 0..4 {
                    vst1q_s32(h.as_mut_ptr().add(4 * q), acc[r][q]);
                    vst1q_s32(l.as_mut_ptr().add(4 * q), acc[r][4 + q]);
                }
                let dst = &mut out[r][first..first + n];
                for (i, o) in dst.iter_mut().enumerate() {
                    *o = 256 * i64::from(h[i]) + i64::from(l[i]) + p.offset[first + i];
                }
            }
        }
    }

    /// One query's attention scores against the keys `kt` (`dh × n_pad`,
    /// key-minor; padding zero): `out[j] = round_shift(q · k_j, shift)` in
    /// i32. The caller checks `max|q| · max|k| · dh < 2^31`.
    pub(crate) fn scores_i32(q: &[i16], kt: &[i16], n_pad: usize, shift: u32, out: &mut [i32]) {
        assert!(n_pad.is_multiple_of(8) && kt.len() >= q.len() * n_pad && out.len() >= n_pad && (1..32).contains(&shift), "padded keys");
        // SAFETY: NEON is part of aarch64; loads and stores stay inside
        // `kt` (`dh × n_pad`) and `out` (`n_pad`), checked above.
        unsafe {
            let sh = vdupq_n_s32(-(shift as i32));
            let mut jb = 0;
            while jb + 32 <= n_pad {
                let mut acc = [vdupq_n_s32(0); 8];
                for (c, &qc) in q.iter().enumerate() {
                    let row = kt.as_ptr().add(c * n_pad + jb);
                    for m in 0..4 {
                        let k = vld1q_s16(row.add(8 * m));
                        acc[2 * m] = vmlal_n_s16(acc[2 * m], vget_low_s16(k), qc);
                        acc[2 * m + 1] = vmlal_high_n_s16(acc[2 * m + 1], k, qc);
                    }
                }
                for (m, a) in acc.iter().enumerate() {
                    // SRSHL by −shift: (x + 2^(shift−1)) >> shift, exact.
                    vst1q_s32(out.as_mut_ptr().add(jb + 4 * m), vrshlq_s32(*a, sh));
                }
                jb += 32;
            }
            while jb < n_pad {
                let mut lo = vdupq_n_s32(0);
                let mut hi = vdupq_n_s32(0);
                for (c, &qc) in q.iter().enumerate() {
                    let k = vld1q_s16(kt.as_ptr().add(c * n_pad + jb));
                    lo = vmlal_n_s16(lo, vget_low_s16(k), qc);
                    hi = vmlal_high_n_s16(hi, k, qc);
                }
                vst1q_s32(out.as_mut_ptr().add(jb), vrshlq_s32(lo, sh));
                vst1q_s32(out.as_mut_ptr().add(jb + 4), vrshlq_s32(hi, sh));
                jb += 8;
            }
        }
    }

    /// `num[i][c] += Σ_j e[i][j] · v[j][c]` for `rows` queries (`e`:
    /// `rows × n`, ≥ 0; `v`: `n × dh` widened to i32; `num`: `rows × dh`).
    /// The sums run in i32 lanes over at most `chunk` keys with some
    /// `e ≠ 0`, then go to i64: the caller picks `chunk` so that
    /// `chunk · max e · max|v| < 2^31`.
    pub(crate) fn attention_values(dh: usize, e: &[i32], rows: usize, v: &[i32], n: usize, chunk: usize, num: &mut [i64]) {
        assert!(e.len() >= rows * n && v.len() >= n * dh && num.len() >= rows * dh && chunk >= 1, "attention value buffers");
        // SAFETY: NEON is part of aarch64; every access is inside the
        // slices checked above.
        unsafe {
            match (dh, rows) {
                (16, 4) => values::<4, 4>(e, v, n, chunk, num),
                (16, r) => {
                    for i in 0..r {
                        values::<4, 1>(&e[i * n..], v, n, chunk, &mut num[i * 16..]);
                    }
                }
                (64, r) => {
                    for i in 0..r {
                        values::<16, 1>(&e[i * n..], v, n, chunk, &mut num[i * 64..]);
                    }
                }
                _ => values_any(dh, e, rows, v, n, chunk, num),
            }
        }
    }

    /// `D4` lanes of 4 per query (dh = 4·D4), `Q` queries at once.
    #[inline(always)]
    unsafe fn values<const D4: usize, const Q: usize>(e: &[i32], v: &[i32], n: usize, chunk: usize, num: &mut [i64]) {
        let dh = 4 * D4;
        let mut acc = [[vdupq_n_s32(0); D4]; Q];
        let mut used = 0;
        let flush = |acc: &mut [[int32x4_t; D4]; Q], num: &mut [i64]| {
            for (i, a) in acc.iter_mut().enumerate() {
                for (c, lanes) in a.iter_mut().enumerate() {
                    let mut tmp = [0i32; 4];
                    vst1q_s32(tmp.as_mut_ptr(), *lanes);
                    for (x, t) in num[i * dh + 4 * c..i * dh + 4 * c + 4].iter_mut().zip(tmp) {
                        *x += i64::from(t);
                    }
                    *lanes = vdupq_n_s32(0);
                }
            }
        };
        for j in 0..n {
            let mut es = [0i32; Q];
            let mut any = 0;
            for (i, x) in es.iter_mut().enumerate() {
                *x = *e.get_unchecked(i * n + j);
                any |= *x;
            }
            if any == 0 {
                continue;
            }
            let row = v.as_ptr().add(j * dh);
            #[allow(clippy::needless_range_loop)]
            for c in 0..D4 {
                let vc = vld1q_s32(row.add(4 * c));
                for i in 0..Q {
                    acc[i][c] = vmlaq_n_s32(acc[i][c], vc, es[i]);
                }
            }
            used += 1;
            if used == chunk {
                flush(&mut acc, num);
                used = 0;
            }
        }
        flush(&mut acc, num);
    }

    /// Other head widths: scalar i32 sums with the same flushing.
    fn values_any(dh: usize, e: &[i32], rows: usize, v: &[i32], n: usize, chunk: usize, num: &mut [i64]) {
        let mut part = vec![0i32; dh];
        for i in 0..rows {
            let mut used = 0;
            part.fill(0);
            for j in 0..n {
                let x = e[i * n + j];
                if x == 0 {
                    continue;
                }
                for (p, &vc) in part.iter_mut().zip(&v[j * dh..(j + 1) * dh]) {
                    *p += x * vc;
                }
                used += 1;
                if used == chunk {
                    for (o, p) in num[i * dh..(i + 1) * dh].iter_mut().zip(part.iter_mut()) {
                        *o += i64::from(*p);
                        *p = 0;
                    }
                    used = 0;
                }
            }
            for (o, &p) in num[i * dh..(i + 1) * dh].iter_mut().zip(&part) {
                *o += i64::from(p);
            }
        }
    }
}

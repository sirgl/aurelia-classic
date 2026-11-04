//! f32 matrix products of the v2 CPU path: `y[m × n] = x[m × k] · wt[k × n]
//! (+ b)`, the weight transposed (`weights::Lin`). Four rows and sixteen
//! columns a step (NEON on aarch64); the sum over `k` runs in ascending
//! order for every output, so a result does not depend on the batch it is
//! in or on the thread.

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

/// `y = x · wt + b` (`b` None: zero).
pub fn linear(x: &[f32], m: usize, k: usize, wt: &[f32], n: usize, b: Option<&[f32]>, y: &mut [f32]) {
    debug_assert!(x.len() >= m * k && wt.len() >= k * n && y.len() >= m * n);
    for r in 0..m {
        let row = &mut y[r * n..(r + 1) * n];
        match b {
            Some(b) => row.copy_from_slice(&b[..n]),
            None => row.fill(0.0),
        }
    }
    linear_acc(x, m, k, wt, n, y);
}

/// `y += x · wt`.
pub fn linear_acc(x: &[f32], m: usize, k: usize, wt: &[f32], n: usize, y: &mut [f32]) {
    let mut r = 0;
    while r + 4 <= m {
        block::<4>(&x[r * k..], k, wt, n, &mut y[r * n..]);
        r += 4;
    }
    match m - r {
        3 => block::<3>(&x[r * k..], k, wt, n, &mut y[r * n..]),
        2 => block::<2>(&x[r * k..], k, wt, n, &mut y[r * n..]),
        1 => block::<1>(&x[r * k..], k, wt, n, &mut y[r * n..]),
        _ => {}
    }
}

/// `R` rows, sixteen columns a step (four NEON registers a row).
#[cfg(target_arch = "aarch64")]
fn block<const R: usize>(x: &[f32], k: usize, wt: &[f32], n: usize, y: &mut [f32]) {
    assert!(x.len() >= R * k && wt.len() >= k * n && y.len() >= R * n);
    let mut c = 0;
    // SAFETY: the loads and stores stay inside `x[..R k]`, `wt[..k n]` and `y[..R n]` (asserted; c + 16 ≤ n).
    unsafe {
        while c + 16 <= n {
            let mut acc = [[vdupq_n_f32(0.0); 4]; R];
            for (r, a) in acc.iter_mut().enumerate() {
                let p = y.as_ptr().add(r * n + c);
                *a = [vld1q_f32(p), vld1q_f32(p.add(4)), vld1q_f32(p.add(8)), vld1q_f32(p.add(12))];
            }
            let xp = x.as_ptr();
            let mut wp = wt.as_ptr().add(c);
            for i in 0..k {
                let w = [vld1q_f32(wp), vld1q_f32(wp.add(4)), vld1q_f32(wp.add(8)), vld1q_f32(wp.add(12))];
                wp = wp.add(n);
                for (r, a) in acc.iter_mut().enumerate() {
                    let xv = *xp.add(r * k + i);
                    a[0] = vfmaq_n_f32(a[0], w[0], xv);
                    a[1] = vfmaq_n_f32(a[1], w[1], xv);
                    a[2] = vfmaq_n_f32(a[2], w[2], xv);
                    a[3] = vfmaq_n_f32(a[3], w[3], xv);
                }
            }
            for (r, a) in acc.iter().enumerate() {
                let p = y.as_mut_ptr().add(r * n + c);
                vst1q_f32(p, a[0]);
                vst1q_f32(p.add(4), a[1]);
                vst1q_f32(p.add(8), a[2]);
                vst1q_f32(p.add(12), a[3]);
            }
            c += 16;
        }
    }
    if c < n {
        for r in 0..R {
            tail(&x[r * k..], k, wt, n, c, &mut y[r * n..]);
        }
    }
}

#[cfg(not(target_arch = "aarch64"))]
fn block<const R: usize>(x: &[f32], k: usize, wt: &[f32], n: usize, y: &mut [f32]) {
    for r in 0..R {
        tail(&x[r * k..], k, wt, n, 0, &mut y[r * n..]);
    }
}

/// Columns `c ..` of one row: four at a time (NEON), then one at a time;
/// the sum over `k` ascending (fused multiply-add) as in the blocks.
fn tail(x: &[f32], k: usize, wt: &[f32], n: usize, c: usize, y: &mut [f32]) {
    #[cfg_attr(not(target_arch = "aarch64"), allow(unused_mut))]
    let mut c4 = c;
    #[cfg(target_arch = "aarch64")]
    while c4 + 4 <= n {
        // SAFETY: c4 + 4 ≤ n, so the loads of `wt[i n + c4 ..][..4]` and `y[c4..][..4]` are in bounds (i < k, wt.len() ≥ k n).
        unsafe {
            let mut a = vld1q_f32(y.as_ptr().add(c4));
            for (i, &xv) in x[..k].iter().enumerate() {
                a = vfmaq_n_f32(a, vld1q_f32(wt.as_ptr().add(i * n + c4)), xv);
            }
            vst1q_f32(y.as_mut_ptr().add(c4), a);
        }
        c4 += 4;
    }
    if c4 < n {
        for (i, &xv) in x[..k].iter().enumerate() {
            let w = &wt[i * n..(i + 1) * n];
            for j in c4..n {
                y[j] = xv.mul_add(w[j], y[j]);
            }
        }
    }
}

/// `Σ a[i] · b[i]`, ascending (fused).
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).fold(0.0f32, |s, (x, y)| x.mul_add(*y, s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_naive_product() {
        let (m, k, n) = (7, 13, 37);
        let x: Vec<f32> = (0..m * k).map(|i| ((i * 7 % 11) as f32 - 5.0) * 0.25).collect();
        let wt: Vec<f32> = (0..k * n).map(|i| ((i * 5 % 13) as f32 - 6.0) * 0.125).collect();
        let b: Vec<f32> = (0..n).map(|i| i as f32 * 0.5).collect();
        let mut y = vec![0.0; m * n];
        linear(&x, m, k, &wt, n, Some(&b), &mut y);
        for r in 0..m {
            for c in 0..n {
                let want = b[c] + (0..k).map(|i| x[r * k + i] * wt[i * n + c]).sum::<f32>();
                assert!((y[r * n + c] - want).abs() < 1e-3, "{r} {c}: {} {want}", y[r * n + c]);
            }
        }
    }
}

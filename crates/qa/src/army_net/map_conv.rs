//! The map encoder of variant B (DESIGN.md, «Two variants»): its kernels and its per-turn state.
//!
//! The encoder is a 1 x 1 projection of the input planes, then residual blocks of two hex convolutions
//! (`x' = x + c2(relu(c1(relu(x))))`). Layout: channel-last, `[tile][channel]`, tile `t = r * gw + q`.
//!
//! A hex convolution runs as gather + matmul: the 7 taps of every output tile (dilated, zero off the grid)
//! are gathered, with the ReLU of its input, into one row of `7 * c_in` floats (im2col), and one sgemm
//! multiplies the rows by the `[7 * c_in][c_out]` weight. On macOS the sgemm is Apple Accelerate's
//! (`cblas_sgemm`, single-threaded: the net's budget is one core); elsewhere a plain loop.
//!
//! The incremental update (`Encoder::update`) recomputes, stage by stage, only the tiles whose receptive
//! field touches a changed input plane: a stage's dirty set is the previous one spread by the taps of its
//! convolution (and, for a block's output, joined with its input's). A dirty set that covers most of the
//! grid runs as the whole stage. The kernel computes a tile the same way in both paths, so the update equals
//! the whole encode bit for bit (`tests`).
//!
//! `Encoder::reference` is the encoder as it was before these kernels (scalar, tile by tile): the exactness
//! guard of the parity tool and the tests.

use std::cell::RefCell;

/// The hex taps of a convolution (dq, dr): the tile and its six neighbours (`trainers::dump::DIRS`).
pub const TAPS: [(i32, i32); 7] = [(0, 0), (1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];
const K: usize = TAPS.len();
/// A tap off the grid (reads zero).
const OFF: u32 = u32::MAX;
/// A stage whose dirty tiles reach this share of the grid (in 1/16) is recomputed whole (no gather/scatter).
/// Measured on B1 (`ai-exam army-parity`, 4000 held-out decisions): any share from 8/16 to 17/16 (never) gives
/// the same update time within 5 %.
const WHOLE_AT_16THS: usize = 13;
/// The fewest rows of a gathered product (`stage`).
const MIN_ROWS: usize = 32;

/// A hex convolution with dilation `d`: `y[t] = b + sum over the taps of W_tap x[t + d * tap]`.
#[derive(Debug, Clone)]
struct Conv {
    n_in: usize,
    n_out: usize,
    /// `[tap * n_in + in][out]`: the im2col weight.
    w: Vec<f32>,
    b: Vec<f32>,
    /// `[tile][tap]`: the input tile each tap reads (`OFF`: off the grid).
    nb: Vec<u32>,
    /// `[tap][out][in]`: the scalar kernel's weight.
    w_old: Vec<f32>,
}

impl Conv {
    /// From MLX's `[out, 3, 3, in]` (ky = dr + 1, kx = dq + 1).
    fn new(w4: &[f32], shape: &[usize], b: &[f32], d: i32, gw: usize, gh: usize) -> Result<Conv, String> {
        let [n_out, 3, 3, n_in] = shape else { return Err(format!("a hex convolution is [out, 3, 3, in], not {shape:?}")) };
        let (n_out, n_in) = (*n_out, *n_in);
        if b.len() != n_out || w4.len() != n_out * 9 * n_in {
            return Err(format!("hex convolution {shape:?} with {} weights and bias {}", w4.len(), b.len()));
        }
        let mut w = vec![0.0; K * n_in * n_out];
        for (k, (dq, dr)) in TAPS.iter().enumerate() {
            let (ky, kx) = ((dr + 1) as usize, (dq + 1) as usize);
            for o in 0..n_out {
                for i in 0..n_in {
                    w[(k * n_in + i) * n_out + o] = w4[((o * 3 + ky) * 3 + kx) * n_in + i];
                }
            }
        }
        let w_old = (0..K * n_out * n_in).map(|j| (j / (n_out * n_in), (j / n_in) % n_out, j % n_in)).map(|(k, o, i)| w[(k * n_in + i) * n_out + o]).collect();
        Ok(Conv { n_in, n_out, w, b: b.to_vec(), nb: neighbours(gw, gh, d), w_old })
    }

    /// The scalar kernel at one tile: `y = b`, then each tap on the grid adds `W_tap . x` per output.
    fn at(&self, a: &[f32], t: usize, y: &mut [f32]) {
        y.copy_from_slice(&self.b);
        for k in 0..K {
            let n = self.nb[t * K + k];
            if n == OFF {
                continue;
            }
            let xin = &a[n as usize * self.n_in..(n as usize + 1) * self.n_in];
            let wk = &self.w_old[k * self.n_out * self.n_in..(k + 1) * self.n_out * self.n_in];
            for o in 0..self.n_out {
                y[o] += dot8(&wk[o * self.n_in..(o + 1) * self.n_in], xin);
            }
        }
    }
}

/// `[tile][tap]`: the tile each tap reads at dilation `d` on a `gw x gh` grid.
fn neighbours(gw: usize, gh: usize, d: i32) -> Vec<u32> {
    let mut nb = Vec::with_capacity(gw * gh * K);
    for t in 0..gw * gh {
        let (q, r) = ((t % gw) as i32, (t / gw) as i32);
        for (dq, dr) in TAPS {
            let (nq, nr) = (q + dq * d, r + dr * d);
            nb.push(if nq >= 0 && nr >= 0 && nq < gw as i32 && nr < gh as i32 { (nr as usize * gw + nq as usize) as u32 } else { OFF });
        }
    }
    nb
}

/// The map encoder: the input projection and the blocks' convolutions (c1, c2 of block 0, then block 1, ...).
#[derive(Debug, Clone)]
pub struct Encoder {
    tiles: usize,
    n_planes: usize,
    ch: usize,
    /// `[plane][channel]`.
    w_in: Vec<f32>,
    /// `[channel][plane]` (the scalar kernel).
    wt_in: Vec<f32>,
    b_in: Vec<f32>,
    convs: Vec<Conv>,
    kernel: Kernel,
}

/// The encoder's state for one side's turn: every stage's output, so a change recomputes only the tiles
/// it reaches (`Encoder::update`).
#[derive(Debug, Clone)]
pub struct MapState {
    pub planes: Vec<f32>,
    /// x0, then per block its inner output h and its output x; each `[tile][channel]`.
    pub stages: Vec<Vec<f32>>,
    /// The tiles recomputed by the last update, over all stages (for the timing report).
    pub last_tiles: usize,
}

impl MapState {
    /// The largest difference of the outputs of two states (the incremental check).
    pub fn max_abs_diff(&self, other: &MapState) -> f32 {
        self.out().iter().zip(other.out()).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max)
    }

    /// The largest difference of any stage relative to the largest magnitude of that stage in `reference`.
    pub fn max_rel_diff(&self, reference: &MapState) -> f32 {
        let mut worst = 0f32;
        for (a, r) in self.stages.iter().zip(&reference.stages) {
            let scale = r.iter().fold(0f32, |m, v| m.max(v.abs())).max(f32::MIN_POSITIVE);
            let diff = a.iter().zip(r).fold(0f32, |m, (x, y)| m.max((x - y).abs()));
            worst = worst.max(diff / scale);
        }
        worst
    }

    /// The encoder's output (`[tile][channel]`).
    pub fn out(&self) -> &[f32] {
        self.stages.last().map_or(&[], |v| v.as_slice())
    }
}

/// The tiles a stage computes.
enum Rows {
    All,
    /// Ascending.
    Some(Vec<u32>),
}

impl Rows {
    fn count(&self, tiles: usize) -> usize {
        match self {
            Rows::All => tiles,
            Rows::Some(v) => v.len(),
        }
    }

    fn whole_if_most(self, tiles: usize) -> Rows {
        match self {
            Rows::Some(v) if v.len() * 16 >= tiles * WHOLE_AT_16THS => Rows::All,
            r => r,
        }
    }

    /// The tiles whose convolution (taps `nb`) reads a tile of these, joined with these if `keep`.
    fn spread(&self, nb: &[u32], tiles: usize, keep: bool) -> Rows {
        let Rows::Some(v) = self else { return Rows::All };
        let mut mark = vec![false; tiles];
        for &t in v {
            if keep {
                mark[t as usize] = true;
            }
            // The taps are symmetric: the tiles that read t are the tiles t reads.
            for &n in &nb[t as usize * K..(t as usize + 1) * K] {
                if n != OFF {
                    mark[n as usize] = true;
                }
            }
        }
        Rows::Some((0..tiles as u32).filter(|t| mark[*t as usize]).collect()).whole_if_most(tiles)
    }
}

thread_local! {
    /// The im2col rows and the gathered outputs, reused across calls.
    static SCRATCH: RefCell<(Vec<f32>, Vec<f32>)> = const { RefCell::new((Vec::new(), Vec::new())) };
}

impl Encoder {
    /// The encoder's channels (MB: may be fewer than the body's d).
    pub fn channels(&self) -> usize {
        self.ch
    }

    /// The input planes per tile.
    pub fn planes(&self) -> usize {
        self.n_planes
    }

    /// `w_in` is `[planes, channels]` (MLX's `x @ W`); `convs` are (weight `[out, 3, 3, in]`, its shape, bias,
    /// dilation) in the order c1, c2 of each block; the grid is `gw x gh` tiles.
    pub fn new(w_in: &[f32], w_in_shape: &[usize], b_in: &[f32], convs: &[(&[f32], &[usize], &[f32], i32)], gw: usize, gh: usize) -> Result<Encoder, String> {
        let [n_planes, ch] = w_in_shape else { return Err(format!("the map's input projection is 2-D, not {w_in_shape:?}")) };
        let (n_planes, ch) = (*n_planes, *ch);
        if b_in.len() != ch || w_in.len() != n_planes * ch {
            return Err(format!("the map's input projection {w_in_shape:?} with bias {}", b_in.len()));
        }
        let convs = convs.iter().map(|(w, s, b, d)| Conv::new(w, s, b, *d, gw, gh)).collect::<Result<Vec<_>, _>>()?;
        if convs.len() % 2 != 0 || convs.iter().any(|c| c.n_in != ch || c.n_out != ch) {
            return Err(format!("the map's blocks are pairs of {ch} -> {ch} convolutions"));
        }
        Ok(Encoder { tiles: gw * gh, n_planes, ch, w_in: w_in.to_vec(), wt_in: transpose(w_in, n_planes, ch), b_in: b_in.to_vec(), convs, kernel: Kernel::from_env() })
    }

    /// The whole map encoded.
    pub fn full(&self, planes: Vec<f32>) -> MapState {
        if self.kernel == Kernel::Scalar {
            return self.reference(planes);
        }
        let mut st = MapState { planes, stages: vec![vec![0.0; self.tiles * self.ch]; 1 + self.convs.len()], last_tiles: 0 };
        self.run(&mut st, Rows::All);
        st
    }

    /// After a decision: only the tiles the changed planes reach, stage by stage.
    pub fn update(&self, st: &mut MapState, planes: Vec<f32>) {
        let p = self.n_planes;
        if self.kernel == Kernel::Scalar {
            let dirty: Vec<bool> = (0..self.tiles).map(|t| st.planes[t * p..(t + 1) * p] != planes[t * p..(t + 1) * p]).collect();
            st.planes = planes;
            self.run_scalar(st, &dirty);
            return;
        }
        let dirty: Vec<u32> = (0..self.tiles as u32).filter(|t| differs(&st.planes[*t as usize * p..(*t as usize + 1) * p], &planes[*t as usize * p..(*t as usize + 1) * p])).collect();
        st.planes = planes;
        if dirty.is_empty() {
            st.last_tiles = 0;
            return;
        }
        self.run(st, Rows::Some(dirty).whole_if_most(self.tiles));
    }

    fn run(&self, st: &mut MapState, rows: Rows) {
        let (tiles, ch, p) = (self.tiles, self.ch, self.n_planes);
        SCRATCH.with(|s| {
            let (a, c) = &mut *s.borrow_mut();
            let mut done = rows.count(tiles);
            // x0 = planes W_in + b_in.
            stage(self.kernel, &rows, tiles, ch, p, a, c, &mut st.stages[0], &|_, out| out.copy_from_slice(&self.b_in), &|t, row| row.copy_from_slice(&st.planes[t * p..(t + 1) * p]), &self.w_in);
            let mut rx = rows;
            for (blk, pair) in self.convs.chunks(2).enumerate() {
                let (c1, c2) = (&pair[0], &pair[1]);
                let (xi, hi, xo) = (2 * blk, 2 * blk + 1, 2 * blk + 2);
                // h = c1(relu(x)).
                let rh = rx.spread(&c1.nb, tiles, false);
                done += rh.count(tiles);
                let (before, after) = st.stages.split_at_mut(hi);
                let x = &before[xi];
                stage(self.kernel, &rh, tiles, ch, K * ch, a, c, &mut after[0], &|_, out| out.copy_from_slice(&c1.b), &|t, row| gather(x, ch, &c1.nb[t * K..(t + 1) * K], row), &c1.w);
                // x' = x + c2(relu(h)).
                let rxo = rh.spread(&c2.nb, tiles, false);
                let rxo = match (rxo, &rx) {
                    (Rows::Some(v), Rows::Some(old)) => {
                        let mut m: Vec<u32> = v.into_iter().chain(old.iter().copied()).collect();
                        m.sort_unstable();
                        m.dedup();
                        Rows::Some(m).whole_if_most(tiles)
                    }
                    _ => Rows::All,
                };
                done += rxo.count(tiles);
                let (before, after) = st.stages.split_at_mut(xo);
                let (x, h) = (&before[xi], &before[hi]);
                stage(
                    self.kernel,
                    &rxo,
                    tiles,
                    ch,
                    K * ch,
                    a,
                    c,
                    &mut after[0],
                    &|t, out| {
                        for ((o, xv), bv) in out.iter_mut().zip(&x[t * ch..(t + 1) * ch]).zip(&c2.b) {
                            *o = xv + bv;
                        }
                    },
                    &|t, row| gather(h, ch, &c2.nb[t * K..(t + 1) * K], row),
                    &c2.w,
                );
                rx = rxo;
            }
            st.last_tiles = done;
        });
    }

    /// The whole map by the encoder as it was before these kernels (the exactness guard).
    pub fn reference(&self, planes: Vec<f32>) -> MapState {
        let mut st = MapState { planes, stages: vec![vec![0.0; self.tiles * self.ch]; 1 + self.convs.len()], last_tiles: 0 };
        self.run_scalar(&mut st, &vec![true; self.tiles]);
        st
    }

    /// The encoder as it was before these kernels (`model.rs` at 91c6dee, `Net::recompute`): tile by tile, a dot
    /// product in eight lanes per output, the dirty tiles spread by each convolution's taps. Kept as the scalar
    /// kernel (`ARMY_NET_MAP_KERNEL=scalar`: the baseline of the measurement, the same play bit for bit).
    fn run_scalar(&self, st: &mut MapState, dirty0: &[bool]) {
        let (tiles, d, p) = (self.tiles, self.ch, self.n_planes);
        let mut done = 0;
        for t in 0..tiles {
            if dirty0[t] {
                let x = &st.planes[t * p..(t + 1) * p];
                for o in 0..d {
                    st.stages[0][t * d + o] = self.b_in[o] + dot8(&self.wt_in[o * p..(o + 1) * p], x);
                }
                done += 1;
            }
        }
        let spread = |dirty: &[bool], c: &Conv| -> Vec<bool> {
            let mut out = vec![false; tiles];
            for (t, on) in dirty.iter().enumerate() {
                if *on {
                    for &n in &c.nb[t * K..(t + 1) * K] {
                        if n != OFF {
                            out[n as usize] = true;
                        }
                    }
                }
            }
            out
        };
        let mut dirty = dirty0.to_vec();
        let mut a = vec![0.0; tiles * d];
        for (b, pair) in self.convs.chunks(2).enumerate() {
            let (c1, c2) = (&pair[0], &pair[1]);
            let (xi, hi, xo) = (2 * b, 2 * b + 1, 2 * b + 2);
            let dirty_h = spread(&dirty, c1);
            for (y, x) in a.iter_mut().zip(&st.stages[xi]) {
                *y = x.max(0.0);
            }
            let mut h = std::mem::take(&mut st.stages[hi]);
            for t in 0..tiles {
                if dirty_h[t] {
                    c1.at(&a, t, &mut h[t * d..(t + 1) * d]);
                    done += 1;
                }
            }
            st.stages[hi] = h;
            let mut dirty_x = spread(&dirty_h, c2);
            for t in 0..tiles {
                dirty_x[t] |= dirty[t];
            }
            for (y, x) in a.iter_mut().zip(&st.stages[hi]) {
                *y = x.max(0.0);
            }
            let mut x = std::mem::take(&mut st.stages[xo]);
            let mut y = vec![0.0; d];
            for t in 0..tiles {
                if dirty_x[t] {
                    c2.at(&a, t, &mut y);
                    for c in 0..d {
                        x[t * d + c] = st.stages[xi][t * d + c] + y[c];
                    }
                    done += 1;
                }
            }
            st.stages[xo] = x;
            dirty = dirty_x;
        }
        st.last_tiles = done;
    }
}

/// One stage over `rows`: `out[t] = init(t) + row(t) W` with `row(t)` of `k` floats and `W` `[k][n]`. The whole
/// grid writes straight into `out`; a subset is gathered, multiplied, scattered.
#[allow(clippy::too_many_arguments)]
fn stage(kernel: Kernel, rows: &Rows, tiles: usize, n: usize, k: usize, a: &mut Vec<f32>, c: &mut Vec<f32>, out: &mut [f32], init: &dyn Fn(usize, &mut [f32]), row: &dyn Fn(usize, &mut [f32]), w: &[f32]) {
    match rows {
        Rows::All => {
            a.resize(tiles * k, 0.0);
            for t in 0..tiles {
                row(t, &mut a[t * k..(t + 1) * k]);
                init(t, &mut out[t * n..(t + 1) * n]);
            }
            sgemm(kernel, tiles, n, k, a, w, out);
        }
        Rows::Some(list) => {
            let m = list.len();
            if m == 0 {
                return;
            }
            // Accelerate gives a row other bits in a product of fewer than 16 rows (`tests::row_invariance`): a
            // short list is padded with zero rows, so a tile's output is the same as in the whole stage.
            let mm = m.max(MIN_ROWS);
            a.resize(mm * k, 0.0);
            c.resize(mm * n, 0.0);
            for (i, &t) in list.iter().enumerate() {
                row(t as usize, &mut a[i * k..(i + 1) * k]);
                init(t as usize, &mut c[i * n..(i + 1) * n]);
            }
            a[m * k..mm * k].fill(0.0);
            sgemm(kernel, mm, n, k, a, w, c);
            for (i, &t) in list.iter().enumerate() {
                out[t as usize * n..(t as usize + 1) * n].copy_from_slice(&c[i * n..(i + 1) * n]);
            }
        }
    }
}

/// Whether two tiles' planes differ in any bit (a branch-free pass the compiler vectorises).
#[inline]
fn differs(a: &[f32], b: &[f32]) -> bool {
    a.iter().zip(b).fold(0u32, |acc, (x, y)| acc | (x.to_bits() ^ y.to_bits())) != 0
}

/// The im2col row of a tile: the ReLU of the input at each tap, zero off the grid.
#[inline]
fn gather(x: &[f32], ch: usize, taps: &[u32], row: &mut [f32]) {
    for (k, &n) in taps.iter().enumerate() {
        let dst = &mut row[k * ch..(k + 1) * ch];
        if n == OFF {
            dst.fill(0.0);
        } else {
            for (d, s) in dst.iter_mut().zip(&x[n as usize * ch..(n as usize + 1) * ch]) {
                *d = s.max(0.0);
            }
        }
    }
}

fn transpose(w: &[f32], rows: usize, cols: usize) -> Vec<f32> {
    let mut t = vec![0.0; rows * cols];
    for r in 0..rows {
        for c in 0..cols {
            t[c * rows + r] = w[r * cols + c];
        }
    }
    t
}

/// A dot product in eight lanes (`model::dot`: the order of the encoder before these kernels).
fn dot8(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let mut acc = [0f32; 8];
    let chunks = n / 8;
    for c in 0..chunks {
        let (x, y) = (&a[c * 8..c * 8 + 8], &b[c * 8..c * 8 + 8]);
        for l in 0..8 {
            acc[l] += x[l] * y[l];
        }
    }
    let mut s = (acc[0] + acc[4]) + (acc[1] + acc[5]) + (acc[2] + acc[6]) + (acc[3] + acc[7]);
    for i in chunks * 8..n {
        s += a[i] * b[i];
    }
    s
}

/// The matmul of the gather + matmul kernel (`ARMY_NET_MAP_KERNEL`: `scalar` is the encoder before these
/// kernels, `neon` the hand-written NEON matmul, else Accelerate's; measured in `tests::bench`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kernel {
    Scalar,
    Neon,
    Accelerate,
}

impl Kernel {
    fn from_env() -> Kernel {
        match std::env::var("ARMY_NET_MAP_KERNEL").as_deref() {
            Ok("scalar") => Kernel::Scalar,
            Ok("neon") => Kernel::Neon,
            _ => Kernel::Accelerate,
        }
    }
}

/// `c [m][n] += a [m][k] b [k][n]`, row-major, f32.
fn sgemm(kernel: Kernel, m: usize, n: usize, k: usize, a: &[f32], b: &[f32], c: &mut [f32]) {
    assert!(a.len() >= m * k && b.len() >= k * n && c.len() >= m * n, "sgemm {m}x{n}x{k} on {} {} {}", a.len(), b.len(), c.len());
    #[cfg(target_arch = "aarch64")]
    if kernel == Kernel::Neon && n % 16 == 0 {
        // SAFETY: NEON is part of every aarch64 target; the sizes are checked above.
        unsafe { neon::sgemm(m, n, k, a, b, c) };
        return;
    }
    let _ = kernel;
    accel::sgemm(m, n, k, a, b, c);
}

#[cfg(target_arch = "aarch64")]
mod neon {
    use std::arch::aarch64::*;

    /// 4 rows x 16 columns of accumulators per step of k; n a multiple of 16.
    pub(super) unsafe fn sgemm(m: usize, n: usize, k: usize, a: &[f32], b: &[f32], c: &mut [f32]) {
        let (ap, bp, cp) = (a.as_ptr(), b.as_ptr(), c.as_mut_ptr());
        let mut i = 0;
        while i < m {
            let rows = (m - i).min(4);
            for j in (0..n).step_by(16) {
                let mut acc = [[vdupq_n_f32(0.0); 4]; 4];
                for kk in 0..k {
                    let bk = bp.add(kk * n + j);
                    let bv = [vld1q_f32(bk), vld1q_f32(bk.add(4)), vld1q_f32(bk.add(8)), vld1q_f32(bk.add(12))];
                    for r in 0..rows {
                        let av = *ap.add((i + r) * k + kk);
                        for q in 0..4 {
                            acc[r][q] = vfmaq_n_f32(acc[r][q], bv[q], av);
                        }
                    }
                }
                for (r, row) in acc.iter().enumerate().take(rows) {
                    let cr = cp.add((i + r) * n + j);
                    for (q, v) in row.iter().enumerate() {
                        vst1q_f32(cr.add(4 * q), vaddq_f32(vld1q_f32(cr.add(4 * q)), *v));
                    }
                }
            }
            i += rows;
        }
    }
}

#[cfg(target_os = "macos")]
mod accel {
    use std::cell::Cell;

    const ROW_MAJOR: i32 = 101;
    const NO_TRANS: i32 = 111;
    const SINGLE_THREADED: u32 = 1;

    #[link(name = "Accelerate", kind = "framework")]
    extern "C" {
        fn cblas_sgemm(order: i32, ta: i32, tb: i32, m: i32, n: i32, k: i32, alpha: f32, a: *const f32, lda: i32, b: *const f32, ldb: i32, beta: f32, c: *mut f32, ldc: i32);
        fn BLASSetThreading(threading: u32) -> i32;
    }

    thread_local! {
        static SINGLE: Cell<bool> = const { Cell::new(false) };
    }

    pub(super) fn sgemm(m: usize, n: usize, k: usize, a: &[f32], b: &[f32], c: &mut [f32]) {
        // The budget is one core: Accelerate's own threads would take others (the setting is per thread).
        SINGLE.with(|s| {
            if !s.get() {
                // SAFETY: a plain setter of a thread-local option of Accelerate.
                unsafe { BLASSetThreading(SINGLE_THREADED) };
                s.set(true);
            }
        });
        // SAFETY: the slices hold m*k, k*n and m*n floats (checked by the caller); row-major, no aliasing.
        unsafe { cblas_sgemm(ROW_MAJOR, NO_TRANS, NO_TRANS, m as i32, n as i32, k as i32, 1.0, a.as_ptr(), k as i32, b.as_ptr(), n as i32, 1.0, c.as_mut_ptr(), n as i32) };
    }
}

#[cfg(not(target_os = "macos"))]
mod accel {
    pub(super) fn sgemm(m: usize, n: usize, k: usize, a: &[f32], b: &[f32], c: &mut [f32]) {
        for i in 0..m {
            let ci = &mut c[i * n..(i + 1) * n];
            for (kk, av) in a[i * k..(i + 1) * k].iter().enumerate() {
                for (cv, bv) in ci.iter_mut().zip(&b[kk * n..(kk + 1) * n]) {
                    *cv += av * bv;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIL: [(i32, i32); 4] = [(1, 2), (2, 4), (2, 1), (1, 1)];

    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
        }
    }

    fn encoder(seed: u64, planes: usize, ch: usize, gw: usize, gh: usize) -> Encoder {
        let mut r = Lcg(seed);
        let w_in: Vec<f32> = (0..planes * ch).map(|_| r.next() * 0.3).collect();
        let b_in: Vec<f32> = (0..ch).map(|_| r.next() * 0.1).collect();
        let s = (1.0 / (7.0 * ch as f32)).sqrt() * 1.7;
        let raw: Vec<(Vec<f32>, Vec<f32>, i32)> = DIL.iter().flat_map(|(a, b)| [*a, *b]).map(|d| ((0..ch * 9 * ch).map(|_| r.next() * s).collect(), (0..ch).map(|_| r.next() * 0.1).collect(), d)).collect();
        let shape = [ch, 3, 3, ch];
        let convs: Vec<(&[f32], &[usize], &[f32], i32)> = raw.iter().map(|(w, b, d)| (w.as_slice(), &shape[..], b.as_slice(), *d)).collect();
        Encoder::new(&w_in, &[planes, ch], &b_in, &convs, gw, gh).expect("the encoder builds")
    }

    /// Planes of a scene: sparse terrain and a few unit tiles.
    fn scene(r: &mut Lcg, tiles: usize, p: usize) -> Vec<f32> {
        let mut v = vec![0.0; tiles * p];
        for x in v.iter_mut() {
            let u = r.next();
            if u > 0.6 {
                *x = u;
            }
        }
        v
    }

    #[test]
    fn the_kernels_equal_the_scalar_encoder() {
        for (gw, gh, ch) in [(24, 24, 64), (12, 12, 32), (7, 5, 8)] {
            let enc = encoder(7, 49, ch, gw, gh);
            let mut r = Lcg(11);
            let planes = scene(&mut r, gw * gh, 49);
            let fast = enc.full(planes.clone());
            let slow = enc.reference(planes);
            let rel = fast.max_rel_diff(&slow);
            assert!(rel <= 1e-5, "{gw}x{gh}x{ch}: relative difference {rel:e}");
        }
    }

    /// A sequence of moves (a unit's facts leave its tile for another, hit points change, a unit dies): the
    /// update equals the whole encode of the same planes bit for bit, at every step.
    #[test]
    fn an_update_equals_the_whole_encode_after_every_move() {
        for (gw, gh, ch) in [(24, 24, 64), (12, 12, 32)] {
            let (tiles, p) = (gw * gh, 49);
            let enc = encoder(3, p, ch, gw, gh);
            let mut r = Lcg(5);
            let mut planes = scene(&mut r, tiles, p);
            let mut st = enc.full(planes.clone());
            let mut partial = 0;
            for step in 0..40 {
                let from = ((r.next() + 1.0) * 0.5 * tiles as f32) as usize % tiles;
                let to = ((r.next() + 1.0) * 0.5 * tiles as f32) as usize % tiles;
                for c in 16..p {
                    let v = planes[from * p + c];
                    planes[from * p + c] = 0.0;
                    planes[to * p + c] += if step % 3 == 0 { v * 0.5 } else { v };
                }
                enc.update(&mut st, planes.clone());
                if st.last_tiles < tiles * 9 {
                    partial += 1;
                }
                let whole = enc.full(planes.clone());
                for (s, (a, b)) in st.stages.iter().zip(&whole.stages).enumerate() {
                    assert!(a == b, "{gw}x{gh}: step {step}, stage {s}: the update differs from the whole encode");
                }
            }
            assert!(partial > 0, "{gw}x{gh}: some updates recompute part of the map");
        }
    }

    fn median_ms(reps: usize, mut f: impl FnMut()) -> f64 {
        for _ in 0..reps / 5 + 1 {
            f();
        }
        let mut v: Vec<f64> = (0..reps)
            .map(|_| {
                let t = std::time::Instant::now();
                f();
                t.elapsed().as_secs_f64() * 1e3
            })
            .collect();
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    }

    /// A move: a unit's facts leave one tile for its neighbour (two dirty tiles).
    fn moved(planes: &[f32], p: usize, from: usize, to: usize) -> Vec<f32> {
        let mut v = planes.to_vec();
        for c in 16..p {
            v[to * p + c] += v[from * p + c] + 0.25;
            v[from * p + c] = 0.0;
        }
        v
    }

    /// `cargo test --release -p aurelia-qa --lib map_conv::tests::bench -- --ignored --nocapture`: median ms of
    /// the whole encode and of a one-move update per kernel at B1's shape and at MB's, and the static / dynamic
    /// split (a deep stack over the static planes once, the units through one shallow block per move).
    #[test]
    #[ignore]
    fn bench() {
        for (gw, ch, reps) in [(24usize, 64usize, 200usize), (12, 32, 2000)] {
            let (tiles, p) = (gw * gw, 49);
            let mut enc = encoder(1, p, ch, gw, gw);
            let mut r = Lcg(2);
            let planes = scene(&mut r, tiles, p);
            let centre = tiles / 2 + gw / 2;
            let next = moved(&planes, p, centre, centre + 1);
            for kernel in [Kernel::Scalar, Kernel::Neon, Kernel::Accelerate] {
                enc.kernel = kernel;
                let n = if kernel == Kernel::Scalar { reps / 10 } else { reps };
                let full = median_ms(n, || {
                    std::hint::black_box(enc.full(planes.clone()));
                });
                let base = enc.full(planes.clone());
                let mut tiles_done = 0;
                let upd = median_ms(n, || {
                    let mut st = base.clone();
                    enc.update(&mut st, next.clone());
                    tiles_done = st.last_tiles;
                    std::hint::black_box(st);
                });
                let clone = median_ms(n, || {
                    std::hint::black_box(base.clone());
                });
                println!("{gw}x{gw}x{ch} {kernel:?}: whole {full:.3} ms, one-move update {:.3} ms ({tiles_done} of {} stage tiles)", upd - clone, tiles * 9);
            }
        }
        // Static / dynamic: the deep stack (4 blocks) on the 16 static planes once; the 33 unit planes through
        // one block of two d = 1 convolutions at 64 channels, recomputed per move.
        let (gw, ch) = (24usize, 64usize);
        let tiles = gw * gw;
        let mut r = Lcg(9);
        let w_in: Vec<f32> = (0..33 * ch).map(|_| r.next() * 0.3).collect();
        let b_in: Vec<f32> = (0..ch).map(|_| r.next() * 0.1).collect();
        let raw: Vec<Vec<f32>> = (0..2).map(|_| (0..ch * 9 * ch).map(|_| r.next() * 0.05).collect()).collect();
        let bias = vec![0.0; ch];
        let shape = [ch, 3, 3, ch];
        let convs: Vec<(&[f32], &[usize], &[f32], i32)> = raw.iter().map(|w| (w.as_slice(), &shape[..], bias.as_slice(), 1)).collect();
        let dynamic = Encoder::new(&w_in, &[33, ch], &b_in, &convs, gw, gw).expect("the dynamic branch builds");
        let units = scene(&mut r, tiles, 33);
        let centre = tiles / 2 + gw / 2;
        let mut next = units.clone();
        for c in 0..33 {
            next[(centre + 1) * 33 + c] += next[centre * 33 + c] + 0.25;
            next[centre * 33 + c] = 0.0;
        }
        let base = dynamic.full(units.clone());
        let mut done = 0;
        let upd = median_ms(2000, || {
            let mut st = base.clone();
            dynamic.update(&mut st, next.clone());
            done = st.last_tiles;
            std::hint::black_box(st);
        });
        let clone = median_ms(2000, || {
            std::hint::black_box(base.clone());
        });
        let whole = median_ms(2000, || {
            std::hint::black_box(dynamic.full(units.clone()));
        });
        println!("static/dynamic at 24x24x64: the unit branch (1 block, d 1) whole {whole:.3} ms, one-move update {:.3} ms ({done} stage tiles)", upd - clone);
    }

    /// The guard of the bit-equal update: Accelerate's sgemm gives a row the same bits in a product of
    /// `MIN_ROWS` or more rows, wherever the row sits (fewer rows take another path with other bits).
    #[test]
    fn a_row_has_the_same_bits_in_any_product_of_min_rows_or_more() {
        let mut r = Lcg(4);
        for (k, n) in [(49usize, 64usize), (64, 64), (448, 64), (49, 32), (32, 32), (224, 32)] {
            let a: Vec<f32> = (0..576 * k).map(|_| r.next()).collect();
            let b: Vec<f32> = (0..k * n).map(|_| r.next()).collect();
            let mut full = vec![0.1f32; 576 * n];
            sgemm(Kernel::Accelerate, 576, n, k, &a, &b, &mut full);
            for m in [MIN_ROWS, MIN_ROWS + 1, 49, 77, 100, 143, 256, 333, 575] {
                for start in [0usize, 1, 100] {
                    if start + m > 576 {
                        continue;
                    }
                    let mut c = vec![0.1f32; m * n];
                    sgemm(Kernel::Accelerate, m, n, k, &a[start * k..], &b, &mut c);
                    assert!(c[..] == full[start * n..(start + m) * n], "k {k} n {n}: rows {start}.. of a product of {m} rows differ from the 576-row product");
                }
            }
        }
    }
}

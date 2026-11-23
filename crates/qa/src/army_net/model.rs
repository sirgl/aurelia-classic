//! The army net's forward pass in f32 on one CPU core (DESIGN.md, «Body»), the same arithmetic as
//! `nn/aurelia_nn/army/model.py` (`tests/army_net.rs` and `army/parity.py` hold the logits equal).
//!
//! Weights: `"ARMW" u32 | json_len u32 | json {config, tensors: [{name, shape, offset}]} | f32 data`
//! (`army/export.py`). A matrix is stored `[in, out]` (MLX's `x @ W`).

use std::collections::BTreeMap;

use super::observe::{Candidate, Tokens, D_MAX, F_ACT, F_TOK};
use super::{S_CH, W};

pub const WEIGHTS_MAGIC: u32 = u32::from_le_bytes(*b"ARMW");

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Config {
    pub d: usize,
    pub heads: usize,
    pub layers: usize,
    pub ff: usize,
    pub f_tok: usize,
    pub f_act: usize,
    pub promos: usize,
    pub dmax: usize,
}

#[derive(Debug, serde::Deserialize)]
struct Entry {
    name: String,
    shape: Vec<usize>,
    offset: usize,
}

#[derive(Debug, serde::Deserialize)]
struct Header {
    config: Config,
    tensors: Vec<Entry>,
}

/// The token numbers that are facts of the tile a token stands on (variant B scatters them onto the map):
/// kind, hp, moves, can attack, strengths, range, arm, stances, level, xp, promotable, selectable, the
/// city's objective / population / ours.
pub const LOCAL: [usize; 33] = [1, 2, 3, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 42, 43, 44, 45];
/// Input planes of the map encoder.
pub const M_IN: usize = S_CH + LOCAL.len();
/// The hex taps of a convolution (dq, dr): the tile and its six neighbours (`trainers::dump::DIRS`).
const TAPS: [(i32, i32); 7] = [(0, 0), (1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];
/// Dilations of the two convolutions of each block: a tile sees 1 + 2 + 2 + 4 + 2 + 1 + 1 + 1 = 14 hexes.
pub const DIL: [(i32, i32); 4] = [(1, 2), (2, 4), (2, 1), (1, 1)];
const TILES: usize = W * W;

/// The map planes of a moment: the static planes and the tokens' local facts on their tiles (summed).
pub fn planes(statics: &[f32], tokens: &Tokens) -> Vec<f32> {
    let mut p = vec![0f32; TILES * M_IN];
    for i in 0..TILES {
        p[i * M_IN..i * M_IN + S_CH].copy_from_slice(&statics[i * S_CH..(i + 1) * S_CH]);
    }
    for (t, f) in tokens.feat.iter().enumerate() {
        let Ok(i) = usize::try_from(tokens.pos[t]) else { continue };
        for (k, src) in LOCAL.iter().enumerate() {
            p[i * M_IN + S_CH + k] += f[*src];
        }
    }
    p
}

/// A hex convolution with dilation `d`: `y[t] = b + Σ_tap W_tap · x[t + d · tap]` (zero off the window).
#[derive(Debug, Clone)]
struct HexConv {
    d: i32,
    n_in: usize,
    n_out: usize,
    /// `[tap][out][in]`.
    w: Vec<f32>,
    b: Vec<f32>,
}

impl HexConv {
    /// From MLX's `[out, 3, 3, in]` (ky = dr + 1, kx = dq + 1).
    fn new(w4: &[f32], shape: &[usize], b: &[f32], d: i32) -> Result<HexConv, String> {
        let [n_out, 3, 3, n_in] = shape else { return Err(format!("a hex convolution is [out, 3, 3, in], not {shape:?}")) };
        let (n_out, n_in) = (*n_out, *n_in);
        let mut w = vec![0.0; 7 * n_out * n_in];
        for (k, (dq, dr)) in TAPS.iter().enumerate() {
            let (ky, kx) = ((dr + 1) as usize, (dq + 1) as usize);
            for o in 0..n_out {
                for i in 0..n_in {
                    w[(k * n_out + o) * n_in + i] = w4[((o * 3 + ky) * 3 + kx) * n_in + i];
                }
            }
        }
        Ok(HexConv { d, n_in, n_out, w, b: b.to_vec() })
    }

    fn at(&self, x: &[f32], tile: usize, y: &mut [f32]) {
        y.copy_from_slice(&self.b);
        let (q, r) = ((tile % W) as i32, (tile / W) as i32);
        for (k, (dq, dr)) in TAPS.iter().enumerate() {
            let (nq, nr) = (q + dq * self.d, r + dr * self.d);
            if nq < 0 || nr < 0 || nq >= W as i32 || nr >= W as i32 {
                continue;
            }
            let n = (nr as usize) * W + nq as usize;
            let xin = &x[n * self.n_in..(n + 1) * self.n_in];
            let wk = &self.w[k * self.n_out * self.n_in..(k + 1) * self.n_out * self.n_in];
            for o in 0..self.n_out {
                y[o] += dot(&wk[o * self.n_in..(o + 1) * self.n_in], xin);
            }
        }
    }
}

/// The tiles whose convolution output reads a tile of `dirty` (the taps are symmetric).
fn spread(dirty: &[bool], d: i32) -> Vec<bool> {
    let mut out = vec![false; TILES];
    for (t, on) in dirty.iter().enumerate() {
        if !on {
            continue;
        }
        let (q, r) = ((t % W) as i32, (t / W) as i32);
        for (dq, dr) in TAPS {
            let (nq, nr) = (q + dq * d, r + dr * d);
            if nq >= 0 && nr >= 0 && nq < W as i32 && nr < W as i32 {
                out[(nr as usize) * W + nq as usize] = true;
            }
        }
    }
    out
}

#[derive(Debug, Clone)]
struct Block {
    c1: HexConv,
    c2: HexConv,
}

/// The map encoder of variant B: a 1 x 1 projection, then four residual blocks of two hex convolutions.
#[derive(Debug, Clone)]
struct Encoder {
    input: Dense,
    blocks: Vec<Block>,
    tok: Dense,
    cand: Dense,
    pool: Dense,
}

/// The encoder's state for one side's turn: every stage's output, so a change recomputes only the tiles
/// it reaches (`Net::map_update`).
#[derive(Debug, Clone)]
pub struct MapState {
    planes: Vec<f32>,
    /// x0, then per block its inner output h and its output x.
    stages: Vec<Vec<f32>>,
    /// The tiles recomputed by the last update, over all stages (for the timing report).
    pub last_tiles: usize,
}

impl MapState {
    /// The largest difference of the outputs of two states (the incremental check).
    pub fn max_abs_diff(&self, other: &MapState) -> f32 {
        self.out().iter().zip(other.out()).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max)
    }

    /// The encoder's output (`[tile][d]`).
    fn out(&self) -> &[f32] {
        self.stages.last().map_or(&[], |v| v.as_slice())
    }
}

/// A dense layer `y = x W + b`, W stored transposed (`[out][in]`) for contiguous dot products.
#[derive(Debug, Clone)]
struct Dense {
    n_in: usize,
    n_out: usize,
    wt: Vec<f32>,
    b: Vec<f32>,
}

impl Dense {
    fn new(w: &[f32], shape: &[usize], b: &[f32]) -> Result<Dense, String> {
        let [n_in, n_out] = shape else { return Err(format!("a dense weight is 2-D, not {shape:?}")) };
        let (n_in, n_out) = (*n_in, *n_out);
        if b.len() != n_out {
            return Err(format!("bias {} for {n_out} outputs", b.len()));
        }
        let mut wt = vec![0.0; n_in * n_out];
        for i in 0..n_in {
            for o in 0..n_out {
                wt[o * n_in + i] = w[i * n_out + o];
            }
        }
        Ok(Dense { n_in, n_out, wt, b: b.to_vec() })
    }

    fn apply(&self, x: &[f32], y: &mut [f32]) {
        for o in 0..self.n_out {
            y[o] = self.b[o] + dot(&self.wt[o * self.n_in..(o + 1) * self.n_in], x);
        }
    }

    fn rows(&self, x: &[f32], n: usize) -> Vec<f32> {
        let mut y = vec![0.0; n * self.n_out];
        for r in 0..n {
            self.apply(&x[r * self.n_in..(r + 1) * self.n_in], &mut y[r * self.n_out..(r + 1) * self.n_out]);
        }
        y
    }
}

/// A dot product in eight lanes (the order MLX does not fix either; parity is within 1e-4).
#[inline]
fn dot(a: &[f32], b: &[f32]) -> f32 {
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

#[derive(Debug, Clone)]
struct Norm {
    g: Vec<f32>,
    b: Vec<f32>,
}

impl Norm {
    fn apply(&self, x: &[f32], y: &mut [f32]) {
        let n = x.len() as f32;
        let mean = x.iter().sum::<f32>() / n;
        let var = x.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n;
        let inv = 1.0 / (var + 1e-5).sqrt();
        for i in 0..x.len() {
            y[i] = (x[i] - mean) * inv * self.g[i] + self.b[i];
        }
    }
}

#[derive(Debug, Clone)]
struct Layer {
    norm1: Norm,
    q: Dense,
    k: Dense,
    v: Dense,
    o: Dense,
    /// `[heads][dmax + 1]`.
    bias: Vec<f32>,
    norm2: Norm,
    f1: Dense,
    f2: Dense,
}

/// The loaded net.
#[derive(Debug, Clone)]
pub struct Net {
    pub config: Config,
    e1: Dense,
    e2: Dense,
    layers: Vec<Layer>,
    norm: Norm,
    pointer: Dense,
    end: Dense,
    c1: Dense,
    promo: Vec<f32>,
    c2: Dense,
    s1: Dense,
    s2: Dense,
    v1: Dense,
    v2: Dense,
    /// Variant B (None: variant A, no map).
    map: Option<Encoder>,
}

/// The token states after the body (`[T][d]`).
pub struct Body {
    pub h: Vec<f32>,
    pub n: usize,
}

fn relu_into(x: &[f32], y: &mut [f32]) {
    for (a, b) in y.iter_mut().zip(x) {
        *a = b.max(0.0);
    }
}

fn relu(v: &mut [f32]) {
    for x in v.iter_mut() {
        *x = x.max(0.0);
    }
}

impl Net {
    pub fn load(path: &std::path::Path) -> Result<Net, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Net::from_bytes(&bytes)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Net, String> {
        let u32_at = |i: usize| bytes.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        if u32_at(0) != Some(WEIGHTS_MAGIC) {
            return Err("not army-net weights".into());
        }
        let len = u32_at(4).ok_or("short header")? as usize;
        let header: Header = serde_json::from_slice(bytes.get(8..8 + len).ok_or("short header")?).map_err(|e| e.to_string())?;
        let data_at = 8 + len;
        let floats: Vec<f32> = bytes[data_at..].chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        let mut t: BTreeMap<String, (Vec<usize>, Vec<f32>)> = BTreeMap::new();
        for e in header.tensors {
            let n: usize = e.shape.iter().product();
            let v = floats.get(e.offset..e.offset + n).ok_or(format!("tensor {} out of the data", e.name))?.to_vec();
            t.insert(e.name, (e.shape, v));
        }
        let c = header.config;
        if c.f_tok != F_TOK || c.f_act != F_ACT || c.dmax != usize::from(D_MAX) {
            return Err(format!("weights for f_tok {} f_act {} dmax {}, the code has {F_TOK} {F_ACT} {D_MAX}", c.f_tok, c.f_act, c.dmax));
        }
        let get = |name: &str| t.get(name).ok_or(format!("no tensor {name}"));
        let dense = |name: &str| -> Result<Dense, String> {
            let (shape, w) = get(&format!("{name}.weight"))?;
            let (_, b) = get(&format!("{name}.bias"))?;
            Dense::new(w, shape, b)
        };
        let norm = |name: &str| -> Result<Norm, String> { Ok(Norm { g: get(&format!("{name}.weight"))?.1.clone(), b: get(&format!("{name}.bias"))?.1.clone() }) };
        let mut layers = Vec::new();
        for l in 0..c.layers {
            let p = format!("layers.{l}");
            layers.push(Layer {
                norm1: norm(&format!("{p}.norm1"))?,
                q: dense(&format!("{p}.q"))?,
                k: dense(&format!("{p}.k"))?,
                v: dense(&format!("{p}.v"))?,
                o: dense(&format!("{p}.o"))?,
                bias: get(&format!("{p}.dist_bias"))?.1.clone(),
                norm2: norm(&format!("{p}.norm2"))?,
                f1: dense(&format!("{p}.f1"))?,
                f2: dense(&format!("{p}.f2"))?,
            });
        }
        let map = if t.contains_key("m_in.weight") {
            let mut blocks = Vec::new();
            for (b, (d1, d2)) in DIL.iter().enumerate() {
                let conv = |name: &str, d: i32| -> Result<HexConv, String> {
                    let (shape, w) = get(&format!("blocks.{b}.{name}.weight"))?;
                    let (_, bias) = get(&format!("blocks.{b}.{name}.bias"))?;
                    HexConv::new(w, shape, bias, d)
                };
                blocks.push(Block { c1: conv("c1", *d1)?, c2: conv("c2", *d2)? });
            }
            Some(Encoder { input: dense("m_in")?, blocks, tok: dense("m_tok")?, cand: dense("m_cand")?, pool: dense("m_pool")? })
        } else {
            None
        };
        Ok(Net {
            map,
            e1: dense("e1")?,
            e2: dense("e2")?,
            layers,
            norm: norm("norm")?,
            pointer: dense("pointer")?,
            end: dense("end")?,
            c1: dense("c1")?,
            promo: get("promo")?.1.clone(),
            c2: dense("c2")?,
            s1: dense("s1")?,
            s2: dense("s2")?,
            v1: dense("v1")?,
            v2: dense("v2")?,
            config: c,
        })
    }

    pub fn has_map(&self) -> bool {
        self.map.is_some()
    }

    /// Variant B: the whole map encoded (at the start of the side's turn).
    pub fn map_full(&self, planes: Vec<f32>) -> Option<MapState> {
        let enc = self.map.as_ref()?;
        let all = vec![true; TILES];
        let mut st = MapState { planes, stages: Vec::new(), last_tiles: 0 };
        st.stages.push(vec![0.0; TILES * enc.input.n_out]);
        for b in &enc.blocks {
            st.stages.push(vec![0.0; TILES * b.c1.n_out]);
            st.stages.push(vec![0.0; TILES * b.c2.n_out]);
        }
        self.recompute(enc, &mut st, &all);
        Some(st)
    }

    /// Variant B after a decision: only the tiles the changed planes reach, stage by stage.
    pub fn map_update(&self, st: &mut MapState, planes: Vec<f32>) {
        let Some(enc) = self.map.as_ref() else { return };
        let mut dirty = vec![false; TILES];
        for (t, flag) in dirty.iter_mut().enumerate() {
            *flag = st.planes[t * M_IN..(t + 1) * M_IN] != planes[t * M_IN..(t + 1) * M_IN];
        }
        st.planes = planes;
        self.recompute(enc, st, &dirty);
    }

    fn recompute(&self, enc: &Encoder, st: &mut MapState, dirty0: &[bool]) {
        let d = self.config.d;
        let mut tiles = 0;
        for t in 0..TILES {
            if dirty0[t] {
                enc.input.apply(&st.planes[t * M_IN..(t + 1) * M_IN], &mut st.stages[0][t * d..(t + 1) * d]);
                tiles += 1;
            }
        }
        let mut dirty = dirty0.to_vec();
        let mut a = vec![0.0; TILES * d];
        for (b, block) in enc.blocks.iter().enumerate() {
            let (xi, hi, xo) = (1 + 2 * b - 1, 1 + 2 * b, 2 + 2 * b);
            // h = c1(relu(x)) on the tiles that read a changed x.
            let dirty_h = spread(&dirty, block.c1.d);
            relu_into(&st.stages[xi], &mut a);
            let mut h = std::mem::take(&mut st.stages[hi]);
            for t in 0..TILES {
                if dirty_h[t] {
                    block.c1.at(&a, t, &mut h[t * d..(t + 1) * d]);
                    tiles += 1;
                }
            }
            st.stages[hi] = h;
            // x' = x + c2(relu(h)) on the tiles that read a changed h, or whose x changed.
            let mut dirty_x = spread(&dirty_h, block.c2.d);
            for t in 0..TILES {
                dirty_x[t] |= dirty[t];
            }
            relu_into(&st.stages[hi], &mut a);
            let mut x = std::mem::take(&mut st.stages[xo]);
            let mut y = vec![0.0; d];
            for t in 0..TILES {
                if dirty_x[t] {
                    block.c2.at(&a, t, &mut y);
                    for c in 0..d {
                        x[t * d + c] = st.stages[xi][t * d + c] + y[c];
                    }
                    tiles += 1;
                }
            }
            st.stages[xo] = x;
            dirty = dirty_x;
        }
        st.last_tiles = tiles;
    }

    /// The token states: embedding (plus, in variant B, the map at each token's tile and the map's mean on
    /// the objective token), then the transformer layers with the distance bias.
    pub fn body(&self, tokens: &Tokens, map: Option<&MapState>) -> Body {
        let n = tokens.len();
        let d = self.config.d;
        let flat: Vec<f32> = tokens.feat.iter().flat_map(|f| f.iter().copied()).collect();
        let mut e = self.e1.rows(&flat, n);
        relu(&mut e);
        let mut h = self.e2.rows(&e, n);
        if let (Some(enc), Some(st)) = (self.map.as_ref(), map) {
            let m = st.out();
            let mut y = vec![0.0; d];
            for i in 0..n {
                let Ok(p) = usize::try_from(tokens.pos[i]) else { continue };
                enc.tok.apply(&m[p * d..(p + 1) * d], &mut y);
                for c in 0..d {
                    h[i * d + c] += y[c];
                }
            }
            let mut mean = vec![0.0; d];
            for t in 0..TILES {
                for c in 0..d {
                    mean[c] += m[t * d + c];
                }
            }
            for v in mean.iter_mut() {
                *v /= TILES as f32;
            }
            enc.pool.apply(&mean, &mut y);
            for c in 0..d {
                h[c] += y[c];
            }
        }
        let mut a = vec![0.0; n * d];
        for layer in &self.layers {
            self.attend(layer, &tokens.dist, n, &mut h, &mut a);
            self.feed(layer, n, &mut h, &mut a);
        }
        let mut out = vec![0.0; n * d];
        for i in 0..n {
            self.norm.apply(&h[i * d..(i + 1) * d], &mut out[i * d..(i + 1) * d]);
        }
        Body { h: out, n }
    }

    fn attend(&self, layer: &Layer, dist: &[u8], n: usize, h: &mut [f32], a: &mut [f32]) {
        let d = self.config.d;
        let heads = self.config.heads;
        let dh = d / heads;
        let nb = self.config.dmax + 1;
        for i in 0..n {
            layer.norm1.apply(&h[i * d..(i + 1) * d], &mut a[i * d..(i + 1) * d]);
        }
        let q = layer.q.rows(a, n);
        let k = layer.k.rows(a, n);
        let v = layer.v.rows(a, n);
        let scale = 1.0 / (dh as f32).sqrt();
        let mut ctx = vec![0.0; n * d];
        let mut s = vec![0.0; n];
        for hd in 0..heads {
            for i in 0..n {
                let qi = &q[i * d + hd * dh..i * d + hd * dh + dh];
                let mut max = f32::NEG_INFINITY;
                for j in 0..n {
                    let kj = &k[j * d + hd * dh..j * d + hd * dh + dh];
                    s[j] = dot(qi, kj) * scale + layer.bias[hd * nb + usize::from(dist[i * n + j])];
                    max = max.max(s[j]);
                }
                let mut sum = 0.0;
                for x in s.iter_mut().take(n) {
                    *x = (*x - max).exp();
                    sum += *x;
                }
                let out = &mut ctx[i * d + hd * dh..i * d + hd * dh + dh];
                for j in 0..n {
                    let w = s[j] / sum;
                    let vj = &v[j * d + hd * dh..j * d + hd * dh + dh];
                    for c in 0..dh {
                        out[c] += w * vj[c];
                    }
                }
            }
        }
        let o = layer.o.rows(&ctx, n);
        for (x, y) in h.iter_mut().zip(o) {
            *x += y;
        }
    }

    fn feed(&self, layer: &Layer, n: usize, h: &mut [f32], a: &mut [f32]) {
        let d = self.config.d;
        for i in 0..n {
            layer.norm2.apply(&h[i * d..(i + 1) * d], &mut a[i * d..(i + 1) * d]);
        }
        let mut f = layer.f1.rows(a, n);
        relu(&mut f);
        let g = layer.f2.rows(&f, n);
        for (x, y) in h.iter_mut().zip(g) {
            *x += y;
        }
    }

    /// The pointer's logits: one per token (None where not selectable), and the end's.
    pub fn pointer(&self, body: &Body, tokens: &Tokens) -> (Vec<Option<f32>>, f32) {
        let d = self.config.d;
        let mut y = [0f32; 1];
        let mut out = Vec::with_capacity(body.n);
        for i in 0..body.n {
            if tokens.selectable[i] {
                self.pointer.apply(&body.h[i * d..(i + 1) * d], &mut y);
                out.push(Some(y[0]));
            } else {
                out.push(None);
            }
        }
        self.end.apply(&body.h[0..d], &mut y);
        (out, y[0])
    }

    /// The action scorer's logits for the unit of token `unit` over its candidates.
    pub fn actions(&self, body: &Body, unit: usize, candidates: &[Candidate], map: Option<&MapState>) -> Vec<f32> {
        let d = self.config.d;
        let hu = &body.h[unit * d..(unit + 1) * d];
        let h0 = &body.h[0..d];
        let mut c1 = vec![0.0; d];
        let mut c = vec![0.0; d];
        let mut joint = vec![0.0; 4 * d];
        let mut s1 = vec![0.0; d];
        let mut y = [0f32; 1];
        let mut out = Vec::with_capacity(candidates.len());
        for cand in candidates {
            self.c1.apply(&cand.feat, &mut c1);
            if let (Some(enc), Some(st), Ok(pos)) = (self.map.as_ref(), map, usize::try_from(cand.pos)) {
                let m = st.out();
                let mut y = vec![0.0; d];
                enc.cand.apply(&m[pos * d..(pos + 1) * d], &mut y);
                for i in 0..d {
                    c1[i] += y[i];
                }
            }
            let p = usize::from(cand.promo);
            for i in 0..d {
                c1[i] = (c1[i] + self.promo[p * d + i]).max(0.0);
            }
            self.c2.apply(&c1, &mut c);
            joint[..d].copy_from_slice(hu);
            joint[d..2 * d].copy_from_slice(h0);
            joint[2 * d..3 * d].copy_from_slice(&c);
            for i in 0..d {
                joint[3 * d + i] = hu[i] * c[i];
            }
            self.s1.apply(&joint, &mut s1);
            relu(&mut s1);
            self.s2.apply(&s1, &mut y);
            out.push(y[0]);
        }
        out
    }

    /// The value head: (the objective's win logit, the expected share of the army lost).
    pub fn value(&self, body: &Body) -> (f32, f32) {
        let d = self.config.d;
        let mut v = vec![0.0; d];
        self.v1.apply(&body.h[0..d], &mut v);
        relu(&mut v);
        let mut y = [0f32; 2];
        self.v2.apply(&v, &mut y);
        (y[0], y[1])
    }
}

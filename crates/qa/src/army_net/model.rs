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
    /// Variant M (None: no memory).
    mem: Option<Memory>,
    /// Variant MH (None: no history).
    hist: Option<HistW>,
}

/// Variant MH's history weights (`army/model_mh.py`).
#[derive(Debug, Clone)]
struct HistW {
    h1: Dense,
    h2: Dense,
    hp: Dense,
    /// `[layer][head]`: a history key's bias, and the bias of a key of the query's own unit.
    hist_bias: Vec<f32>,
    same_bias: Vec<f32>,
    /// `[plan][theta_p, w_p]`.
    stall: Vec<f32>,
}

/// The plan's named dimensions MH reads (`model_m.py`, `model_mh.py`).
const P_ENGAGED: usize = 10;
const P_CITY0: usize = 12;

/// The token states after the body (`[T + plans][d]`: the tokens, then variant M's plan tokens).
pub struct Body {
    pub h: Vec<f32>,
    /// Tokens (without the plan tokens).
    pub n: usize,
    pub plans: usize,
}

/// Unit slots of variant M's memory (a unit's index in `Orders::army`), its width, the plan tokens.
pub const SLOTS: usize = 16;
pub const U_DIM: usize = 16;
pub const PLANS: usize = 4;

/// One GRU cell (PyTorch's convention, gates r, z, n): `wx` from the input, `wh` from the state, biases inside.
#[derive(Debug, Clone)]
struct Gru {
    wx: Dense,
    wh: Dense,
    h: usize,
}

impl Gru {
    fn step(&self, x: &[f32], h: &[f32]) -> Vec<f32> {
        let n = self.h;
        let mut gx = vec![0.0; 3 * n];
        let mut gh = vec![0.0; 3 * n];
        self.wx.apply(x, &mut gx);
        self.wh.apply(h, &mut gh);
        let sig = |v: f32| 1.0 / (1.0 + (-v).exp());
        (0..n)
            .map(|k| {
                let r = sig(gx[k] + gh[k]);
                let z = sig(gx[n + k] + gh[n + k]);
                let c = (gx[2 * n + k] + r * gh[2 * n + k]).tanh();
                (1.0 - z) * c + z * h[k]
            })
            .collect()
    }
}

/// Variant M's memory weights (`army/model_m.py`).
#[derive(Debug, Clone)]
struct Memory {
    um: Dense,
    plan0: Vec<f32>,
    /// `[layer][head]`: the attention bias of any pair with a plan token.
    plan_bias: Vec<f32>,
    gu: Gru,
    pq: Dense,
    pk: Dense,
    gp: Gru,
    /// `[plan][theta, kappa, w_event]`.
    hyst: Vec<f32>,
}

/// The memory of one play (variant M): every army slot's u, the plan tokens, the slots seen at the last update.
#[derive(Debug, Clone)]
pub struct MemoryState {
    pub u: Vec<f32>,
    pub p: Vec<f32>,
    pub alive: [f32; SLOTS],
    /// The operation plan token P_0 in force in each turn so far (MH: a history token adds the plan of its turn).
    pub p0_turns: Vec<Vec<f32>>,
}

impl MemoryState {
    /// P_0 of the turn `age` turns before this one (this turn's: the plan now), clamped at the play's start.
    fn p0_at(&self, age: u8, d: usize) -> &[f32] {
        let k = self.p0_turns.len();
        let j = k.saturating_sub(usize::from(age));
        if j >= k {
            &self.p[..d]
        } else {
            &self.p0_turns[j]
        }
    }
}

/// What variant M's body reads besides the tokens: the memory and each token's slot (-1 none).
pub struct MemIn<'a> {
    pub state: &'a MemoryState,
    pub slots: &'a [i16],
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
        let mem = if t.contains_key("plan0") {
            let raw = |name: &str| -> Result<Vec<f32>, String> { Ok(get(name)?.1.clone()) };
            let mat = |w: &str, b: &str| -> Result<Dense, String> {
                let (shape, wv) = get(w)?;
                Dense::new(wv, shape, &raw(b)?)
            };
            let gu = Gru { wx: mat("gu_wx", "gu_bx")?, wh: mat("gu_wh", "gu_bh")?, h: U_DIM };
            let gp = Gru { wx: mat("gp_wx", "gp_bx")?, wh: mat("gp_wh", "gp_bh")?, h: c.d };
            Some(Memory { um: dense("um")?, plan0: raw("plan0")?, plan_bias: raw("plan_bias")?, gu, pq: dense("pq")?, pk: dense("pk")?, gp, hyst: raw("hyst")? })
        } else {
            None
        };
        let hist = if t.contains_key("h1.weight") {
            let raw = |name: &str| -> Result<Vec<f32>, String> { Ok(get(name)?.1.clone()) };
            Some(HistW { h1: dense("h1")?, h2: dense("h2")?, hp: dense("hp")?, hist_bias: raw("hist_bias")?, same_bias: raw("same_bias")?, stall: raw("stall")? })
        } else {
            None
        };
        Ok(Net {
            hist,
            mem,
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

    pub fn has_memory(&self) -> bool {
        self.mem.is_some()
    }

    /// Variant M: the memory at a play's start (None: no memory).
    pub fn initial_memory(&self) -> Option<MemoryState> {
        let m = self.mem.as_ref()?;
        Some(MemoryState { u: vec![0.0; SLOTS * U_DIM], p: m.plan0.clone(), alive: [0.0; SLOTS], p0_turns: Vec::new() })
    }

    /// Variant M at our turn's end: `body` is this moment's body (read with `state`); every army token updates its
    /// slot, the plan tokens attend to the tokens and update with hysteresis (`army/model_m.py`, `MemNet.step`).
    pub fn memory_step(&self, body: &Body, tokens: &Tokens, slots: &[i16], state: &mut MemoryState) {
        let Some(m) = self.mem.as_ref() else { return };
        let d = self.config.d;
        let n = body.n;
        let mut cover = [0f32; SLOTS];
        let mut x = vec![0.0; d + F_TOK];
        let mut news: Vec<(usize, Vec<f32>)> = Vec::new();
        for i in 0..n {
            let Ok(s) = usize::try_from(slots[i]) else { continue };
            if s >= SLOTS {
                continue;
            }
            x[..d].copy_from_slice(&body.h[i * d..(i + 1) * d]);
            x[d..].copy_from_slice(&tokens.feat[i]);
            news.push((s, m.gu.step(&x, &state.u[s * U_DIM..(s + 1) * U_DIM])));
            cover[s] = 1.0;
        }
        for (s, u) in news {
            state.u[s * U_DIM..(s + 1) * U_DIM].copy_from_slice(&u);
        }
        let event = cover.iter().zip(&state.alive).map(|(c, a)| a * (1.0 - c)).sum::<f32>().min(1.0);
        // MH: the progress numbers (the objective city's share now, dealt to it, taken, lost over the window).
        let progress = self.hist.as_ref().map(|_| {
            let city = (0..n).find(|i| tokens.feat[*i][43] > 0.5 && tokens.feat[*i][3] > 0.5).map_or(1.0, |i| tokens.feat[i][7]);
            let (mut dealt, mut taken, mut lost) = (0f32, 0f32, 0f32);
            for f in &tokens.hist {
                dealt += f[14] * f[19];
                taken += f[15];
                lost += f[17];
            }
            [city, dealt, taken, lost]
        });
        state.p0_turns.push(state.p[..d].to_vec());
        let mut x = vec![0.0; d + F_TOK + progress.map_or(0, |p| p.len())];
        let keys = m.pk.rows(&body.h[..n * d], n);
        let scale = 1.0 / (d as f32).sqrt();
        let mut q = vec![0.0; d];
        let mut w = vec![0.0; n];
        for k in 0..PLANS {
            let pk = state.p[k * d..(k + 1) * d].to_vec();
            m.pq.apply(&pk, &mut q);
            let mut max = f32::NEG_INFINITY;
            for j in 0..n {
                w[j] = dot(&q, &keys[j * d..(j + 1) * d]) * scale;
                max = max.max(w[j]);
            }
            let mut sum = 0.0;
            for v in w.iter_mut().take(n) {
                *v = (*v - max).exp();
                sum += *v;
            }
            let mut pooled = vec![0.0; d];
            for j in 0..n {
                let a = w[j] / sum;
                for c in 0..d {
                    pooled[c] += a * body.h[j * d + c];
                }
            }
            x[..d].copy_from_slice(&pooled);
            x[d..d + F_TOK].copy_from_slice(&tokens.feat[0]);
            if let Some(p) = progress {
                x[d + F_TOK..].copy_from_slice(&p);
            }
            let pn = m.gp.step(&x, &pk);
            let diff = pn.iter().zip(&pk).map(|(a, b)| (a - b) * (a - b)).sum::<f32>();
            let norm = pk.iter().map(|a| a * a).sum::<f32>();
            let delta = (diff + 1e-12).sqrt() / ((norm + 1e-12).sqrt() + 1e-3);
            let (theta, kappa, we) = (m.hyst[k * 3], m.hyst[k * 3 + 1], m.hyst[k * 3 + 2]);
            let stall = match (self.hist.as_ref(), progress) {
                (Some(hw), Some(p)) => {
                    let (tp, wp) = (hw.stall[k * 2], hw.stall[k * 2 + 1]);
                    let eng = 1.0 / (1.0 + (-3.0 * pk[P_ENGAGED]).exp());
                    wp * eng * (tp - (pk[P_CITY0] - p[0].tanh())).max(0.0)
                }
                _ => 0.0,
            };
            let c = 1.0 / (1.0 + (-(kappa * (delta - theta) + we * event + stall)).exp());
            for i in 0..d {
                state.p[k * d + i] = pk[i] + c * (pn[i] - pk[i]);
            }
        }
        state.alive = cover;
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
    pub fn body(&self, tokens: &Tokens, map: Option<&MapState>, mem: Option<&MemIn>) -> Body {
        let n = tokens.len();
        let d = self.config.d;
        let flat: Vec<f32> = tokens.feat.iter().flat_map(|f| f.iter().copied()).collect();
        let mut e = self.e1.rows(&flat, n);
        relu(&mut e);
        let mut h = self.e2.rows(&e, n);
        // Variant M: each unit's slot memory enters its token; the plan tokens follow the tokens.
        let mut plans = 0;
        if let (Some(m), Some(mi)) = (self.mem.as_ref(), mem) {
            let mut y = vec![0.0; d];
            for i in 0..n {
                let Ok(s) = usize::try_from(mi.slots[i]) else { continue };
                if s >= SLOTS {
                    continue;
                }
                m.um.apply(&mi.state.u[s * U_DIM..(s + 1) * U_DIM], &mut y);
                for c in 0..d {
                    h[i * d + c] += y[c];
                }
            }
            h.extend_from_slice(&mi.state.p);
            plans = PLANS;
        }
        // Variant MH: the history's embedding (with the plan of each fact's turn), read as keys and values.
        let mut he: Vec<f32> = Vec::new();
        let mut qslots: Vec<i16> = Vec::new();
        if let (Some(hw), Some(mi)) = (self.hist.as_ref(), mem) {
            let mut e1 = vec![0.0; d];
            let mut y = vec![0.0; d];
            let mut z = vec![0.0; d];
            for (k, f) in tokens.hist.iter().enumerate() {
                hw.h1.apply(f, &mut e1);
                relu(&mut e1);
                hw.h2.apply(&e1, &mut y);
                hw.hp.apply(mi.state.p0_at(tokens.hist_age[k], d), &mut z);
                he.extend(y.iter().zip(&z).map(|(a, b)| a + b));
            }
            qslots = (0..n).map(|i| mi.slots[i]).chain(std::iter::repeat(-1).take(plans)).collect();
        }
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
        let total = n + plans;
        let mut a = vec![0.0; total * d];
        let hist_in = if he.is_empty() { None } else { Some((he.as_slice(), tokens.hist_slot.as_slice(), qslots.as_slice())) };
        for (li, layer) in self.layers.iter().enumerate() {
            self.attend(layer, li, &tokens.dist, n, total, &mut h, &mut a, hist_in);
            self.feed(layer, total, &mut h, &mut a);
        }
        let mut out = vec![0.0; total * d];
        for i in 0..total {
            self.norm.apply(&h[i * d..(i + 1) * d], &mut out[i * d..(i + 1) * d]);
        }
        Body { h: out, n, plans }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    fn attend(&self, layer: &Layer, li: usize, dist: &[u8], n: usize, total: usize, h: &mut [f32], a: &mut [f32], hist: Option<(&[f32], &[i16], &[i16])>) {
        let d = self.config.d;
        let heads = self.config.heads;
        let dh = d / heads;
        let nb = self.config.dmax + 1;
        let plan_bias = self.mem.as_ref().map(|m| &m.plan_bias[li * heads..(li + 1) * heads]);
        let n_real = n;
        let n = total;
        for i in 0..n {
            layer.norm1.apply(&h[i * d..(i + 1) * d], &mut a[i * d..(i + 1) * d]);
        }
        let q = layer.q.rows(a, n);
        let k = layer.k.rows(a, n);
        let v = layer.v.rows(a, n);
        // MH: the history's keys and values (the layer's norm1, k and v on the history's embedding).
        let (nh, kh, vh, hb, sb) = match (hist, self.hist.as_ref()) {
            (Some((he, _, _)), Some(hw)) => {
                let nh = he.len() / d;
                let mut ah = vec![0.0; he.len()];
                for j in 0..nh {
                    layer.norm1.apply(&he[j * d..(j + 1) * d], &mut ah[j * d..(j + 1) * d]);
                }
                (nh, layer.k.rows(&ah, nh), layer.v.rows(&ah, nh), &hw.hist_bias[li * heads..(li + 1) * heads], &hw.same_bias[li * heads..(li + 1) * heads])
            }
            _ => (0, Vec::new(), Vec::new(), &[][..], &[][..]),
        };
        let scale = 1.0 / (dh as f32).sqrt();
        let mut ctx = vec![0.0; n * d];
        let mut s = vec![0.0; n + nh];
        for hd in 0..heads {
            for i in 0..n {
                let qi = &q[i * d + hd * dh..i * d + hd * dh + dh];
                let mut max = f32::NEG_INFINITY;
                for j in 0..n {
                    let kj = &k[j * d + hd * dh..j * d + hd * dh + dh];
                    let bias = if i < n_real && j < n_real { layer.bias[hd * nb + usize::from(dist[i * n_real + j])] } else { plan_bias.map_or(0.0, |p| p[hd]) };
                    s[j] = dot(qi, kj) * scale + bias;
                    max = max.max(s[j]);
                }
                if let Some((_, hslot, qslot)) = hist {
                    for j in 0..nh {
                        let kj = &kh[j * d + hd * dh..j * d + hd * dh + dh];
                        let same = hslot[j] >= 0 && qslot[i] == hslot[j];
                        s[n + j] = dot(qi, kj) * scale + hb[hd] + if same { sb[hd] } else { 0.0 };
                        max = max.max(s[n + j]);
                    }
                }
                let mut sum = 0.0;
                for x in s.iter_mut().take(n + nh) {
                    *x = (*x - max).exp();
                    sum += *x;
                }
                let out = &mut ctx[i * d + hd * dh..i * d + hd * dh + dh];
                for j in 0..n + nh {
                    let w = s[j] / sum;
                    let vj = if j < n { &v[j * d + hd * dh..j * d + hd * dh + dh] } else { &vh[(j - n) * d + hd * dh..(j - n) * d + hd * dh + dh] };
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

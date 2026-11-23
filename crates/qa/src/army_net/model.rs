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
/// Dilations of the two convolutions of each block: a tile sees 1 + 2 + 2 + 4 + 2 + 1 + 1 + 1 = 14 hexes.
pub const DIL: [(i32, i32); 4] = [(1, 2), (2, 4), (2, 1), (1, 1)];
const TILES: usize = W * W;

pub use super::map_conv::MapState;

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

/// The map encoder of variant B (`map_conv`) and the heads' readouts of its output.
#[derive(Debug, Clone)]
struct Encoder {
    conv: super::map_conv::Encoder,
    tok: Dense,
    cand: Dense,
    pool: Dense,
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
    /// Variant MN (None: no notes).
    notes: Option<NoteW>,
}

/// Variant MN's note weights (`army/model_mn.py`).
#[derive(Debug, Clone)]
struct NoteW {
    n1: Dense,
    n2: Dense,
    pull: Dense,
    ret: Dense,
    reason: Dense,
    free: Dense,
    /// The plan's stage readout (scale, shift), `aux_p[0]`.
    stage_scale: f32,
    stage_shift: f32,
}

pub const NOTE_KINDS: [&str; 5] = ["PLAN", "PLAN_CHANGE", "WAIT_STAGE", "PULL_BACK_HEAL", "RETURN"];
pub const REASONS: [&str; 4] = ["losses", "city stronger", "advance", "other"];
pub const STAGE_NAMES: [&str; 6] = ["gather", "march", "stage", "assault", "heal-rotate", "withdraw"];
const STAGES: usize = 6;
const FREE: usize = 8;
const F_NOTE: usize = 5 + STAGES + 4 + 2 + FREE;
const STAGE_WAIT: usize = 2;
const STAGE_HEAL: usize = 4;
const CHANGE_SPAN: f32 = 2.0;
const AGE_NONE: f32 = 99.0;
const P_WAITED: usize = 11;

/// Variant MN's note stream of a play (`MNNet.initial_notes`).
#[derive(Debug, Clone)]
pub struct NoteState {
    pub stage: [f32; STAGES],
    pub waited: f32,
    pub free: [f32; FREE],
    pub change_age: f32,
    pub reason: [f32; 4],
    pub pull: [f32; SLOTS],
    pub pull_age: [f32; SLOTS],
    /// The notes written at the last turn's end, for the trace (`PLAN(city, assault)`, ...).
    pub written: Vec<String>,
}

impl NoteState {
    fn new() -> NoteState {
        NoteState { stage: [0.0; STAGES], waited: 0.0, free: [0.0; FREE], change_age: AGE_NONE, reason: [0.0; 4], pull: [0.0; SLOTS], pull_age: [0.0; SLOTS], written: Vec::new() }
    }

    /// The note tokens in force (`MNNet.note_tokens`, the masked ones left out): numbers and the unit's slot.
    fn tokens(&self) -> Vec<([f32; F_NOTE], i16)> {
        let tok = |kind: usize, stage: Option<&[f32; STAGES]>, reason: Option<&[f32; 4]>, waited: f32, age: f32, free: Option<&[f32; FREE]>| {
            let mut f = [0f32; F_NOTE];
            f[kind] = 1.0;
            if let Some(st) = stage {
                f[5..5 + STAGES].copy_from_slice(st);
            }
            if let Some(r) = reason {
                f[11..15].copy_from_slice(r);
            }
            f[15] = waited * 0.1;
            f[16] = age / 3.0;
            if let Some(fr) = free {
                f[17..].copy_from_slice(fr);
            }
            f
        };
        let has_plan = self.stage.iter().sum::<f32>() > 0.0;
        let mut out = Vec::new();
        if has_plan {
            out.push((tok(0, Some(&self.stage), None, 0.0, 0.0, Some(&self.free)), -1));
        }
        if self.change_age < CHANGE_SPAN {
            out.push((tok(1, Some(&self.stage), Some(&self.reason), 0.0, self.change_age.min(9.0), None), -1));
        }
        if has_plan && self.stage[STAGE_WAIT] > 0.5 {
            out.push((tok(2, None, None, self.waited, 0.0, None), -1));
        }
        for s in 0..SLOTS {
            if self.pull[s] > 0.5 {
                out.push((tok(3, None, None, 0.0, self.pull_age[s].min(9.0), None), s as i16));
            }
        }
        out
    }
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
    /// Variant MN: the note stream.
    pub notes: Option<NoteState>,
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
            let mut convs = Vec::new();
            for (b, (d1, d2)) in DIL.iter().enumerate() {
                for (name, d) in [("c1", *d1), ("c2", *d2)] {
                    let (shape, w) = get(&format!("blocks.{b}.{name}.weight"))?;
                    let (_, bias) = get(&format!("blocks.{b}.{name}.bias"))?;
                    convs.push((w.as_slice(), shape.as_slice(), bias.as_slice(), d));
                }
            }
            let (in_shape, w_in) = get("m_in.weight")?;
            let (_, b_in) = get("m_in.bias")?;
            let conv = super::map_conv::Encoder::new(w_in, in_shape, b_in, &convs, W, W)?;
            Some(Encoder { conv, tok: dense("m_tok")?, cand: dense("m_cand")?, pool: dense("m_pool")? })
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
        let notes = if t.contains_key("n1.weight") {
            let aux = get("aux_p")?.1.clone();
            Some(NoteW { n1: dense("n1")?, n2: dense("n2")?, pull: dense("pull")?, ret: dense("ret")?, reason: dense("reason")?, free: dense("free")?, stage_scale: aux[0], stage_shift: aux[1] })
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
            notes,
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

    /// MN at our turn's end (after the memory's update): the heads and the stream (`MNNet.note_heads`, `write_notes`).
    #[allow(clippy::too_many_arguments)]
    fn write_notes(&self, nw: &NoteW, mut ns: NoteState, body: &Body, tokens: &Tokens, slots: &[i16], p_old0: &[f32], state: &MemoryState, event: f32) -> NoteState {
        let d = self.config.d;
        let argmax = |v: &[f32]| v.iter().enumerate().fold((0usize, f32::NEG_INFINITY), |b, (i, x)| if *x > b.1 { (i, *x) } else { b }).0;
        let stage_of = |p: &[f32]| -> Vec<f32> { (0..STAGES).map(|k| nw.stage_scale * p[k] + nw.stage_shift).collect() };
        let new = argmax(&stage_of(&state.p[..d]));
        let old = argmax(&stage_of(p_old0));
        let ind = [event, if new == STAGE_HEAL { 1.0 } else { 0.0 }, if new > old { 1.0 } else { 0.0 }, 0.0];
        let mut rl = [0f32; 4];
        nw.reason.apply(&ind, &mut rl);
        let reason = argmax(&rl);
        let mut pull = [0f32; SLOTS];
        let mut ret = [0f32; SLOTS];
        let mut x = vec![0.0; d + F_TOK + U_DIM];
        let mut y = [0f32; 1];
        let mut written = Vec::new();
        for i in 0..body.n {
            let Ok(s) = usize::try_from(slots[i]) else { continue };
            if s >= SLOTS {
                continue;
            }
            x[..d].copy_from_slice(&body.h[i * d..(i + 1) * d]);
            x[d..d + F_TOK].copy_from_slice(&tokens.feat[i]);
            x[d + F_TOK..].copy_from_slice(&state.u[s * U_DIM..(s + 1) * U_DIM]);
            nw.pull.apply(&x, &mut y);
            if y[0] > 0.0 {
                pull[s] = 1.0;
            }
            nw.ret.apply(&x, &mut y);
            if y[0] > 0.0 {
                ret[s] = 1.0;
            }
        }
        let mut stage = [0f32; STAGES];
        stage[new] = 1.0;
        let had = ns.stage.iter().sum::<f32>() > 0.0;
        let changed = had && argmax(&ns.stage) != new;
        written.push(format!("PLAN(objective, {})", STAGE_NAMES[new]));
        ns.change_age = if changed { 0.0 } else { ns.change_age + 1.0 };
        if changed {
            ns.reason = [0.0; 4];
            ns.reason[reason] = 1.0;
            written.push(format!("PLAN_CHANGE({})", REASONS[reason]));
        }
        for s in 0..SLOTS {
            let was = ns.pull[s] > 0.5;
            let now = (ns.pull[s].max(pull[s])) * (1.0 - ret[s]);
            ns.pull_age[s] = if now > 0.5 { if was { ns.pull_age[s] + 1.0 } else { 0.0 } } else { 0.0 };
            if now > 0.5 && !was {
                written.push(format!("PULL_BACK_HEAL(slot {s})"));
            }
            if was && now < 0.5 {
                written.push(format!("RETURN(slot {s})"));
            }
            ns.pull[s] = now;
        }
        ns.stage = stage;
        ns.waited = (state.p[P_WAITED] * 10.0).max(0.0);
        if new == STAGE_WAIT {
            written.push(format!("WAIT_STAGE({:.0})", ns.waited));
        }
        let mut fr = [0f32; FREE];
        nw.free.apply(&state.p[..d], &mut fr);
        for v in fr.iter_mut() {
            *v = v.tanh();
        }
        ns.free = fr;
        ns.written = written;
        ns
    }

    pub fn has_memory(&self) -> bool {
        self.mem.is_some()
    }

    /// Variant M: the memory at a play's start (None: no memory).
    pub fn initial_memory(&self) -> Option<MemoryState> {
        let m = self.mem.as_ref()?;
        Some(MemoryState { u: vec![0.0; SLOTS * U_DIM], p: m.plan0.clone(), alive: [0.0; SLOTS], p0_turns: Vec::new(), notes: self.notes.as_ref().map(|_| NoteState::new()) })
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
        let p_old0: Vec<f32> = state.p[..d].to_vec();
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
        if let (Some(nw), Some(ns)) = (self.notes.as_ref(), state.notes.clone()) {
            state.notes = Some(self.write_notes(nw, ns, body, tokens, slots, &p_old0, state, event));
        }
        state.alive = cover;
    }

    /// Variant B: the whole map encoded (at the start of the side's turn).
    pub fn map_full(&self, planes: Vec<f32>) -> Option<MapState> {
        Some(self.map.as_ref()?.conv.full(planes))
    }

    /// Variant B after a decision: only the tiles the changed planes reach, stage by stage.
    pub fn map_update(&self, st: &mut MapState, planes: Vec<f32>) {
        if let Some(enc) = self.map.as_ref() {
            enc.conv.update(st, planes);
        }
    }

    /// Variant B: the whole map encoded by the scalar encoder these kernels replaced (the exactness guard).
    pub fn map_reference(&self, planes: Vec<f32>) -> Option<MapState> {
        Some(self.map.as_ref()?.conv.reference(planes))
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
        let mut hslots: Vec<i16> = tokens.hist_slot.clone();
        if let (Some(nw), Some(mi)) = (self.notes.as_ref(), mem) {
            let mut e1 = vec![0.0; d];
            let mut y = vec![0.0; d];
            for (f, s) in mi.state.notes.as_ref().map(|n| n.tokens()).unwrap_or_default() {
                nw.n1.apply(&f, &mut e1);
                relu(&mut e1);
                nw.n2.apply(&e1, &mut y);
                he.extend_from_slice(&y);
                hslots.push(s);
            }
        }
        if let (Some(enc), Some(st)) = (self.map.as_ref(), map) {
            let m = st.out();
            let mc = enc.conv.channels();
            let mut y = vec![0.0; d];
            for i in 0..n {
                let Ok(p) = usize::try_from(tokens.pos[i]) else { continue };
                enc.tok.apply(&m[p * mc..(p + 1) * mc], &mut y);
                for c in 0..d {
                    h[i * d + c] += y[c];
                }
            }
            let mut mean = vec![0.0; mc];
            for t in 0..TILES {
                for c in 0..mc {
                    mean[c] += m[t * mc + c];
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
        let hist_in = if he.is_empty() { None } else { Some((he.as_slice(), hslots.as_slice(), qslots.as_slice())) };
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
                let mc = enc.conv.channels();
                enc.cand.apply(&m[pos * mc..(pos + 1) * mc], &mut y);
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

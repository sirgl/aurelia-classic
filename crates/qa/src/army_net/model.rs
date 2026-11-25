//! The army net's forward pass in f32 on one CPU core (DESIGN.md, «Body»), the same arithmetic as
//! `nn/aurelia_nn/army/model.py` (`tests/army_net.rs` and `army/parity.py` hold the logits equal).
//!
//! Weights: `"ARMW" u32 | json_len u32 | json {config, tensors: [{name, shape, offset}]} | f32 data`
//! (`army/export.py`). A matrix is stored `[in, out]` (MLX's `x @ W`).

use std::collections::BTreeMap;

use super::observe::{Candidate, Tokens, D_MAX, F_ACT, F_TOK, X_ACT, X_DECLARES, X_TOK};
use super::{S_CH, S_REL, W};

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
    /// One char per layer: 'c' classic, 'm' modern (`army/model.py`, `ModernLayer`). None: all classic.
    #[serde(default)]
    pub kinds: Option<String>,
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
    planes_with(statics, None, tokens)
}

/// `planes`, and (typed tokens: a map encoder of `M_IN + S_REL` planes) the relation planes after the tokens' facts.
fn planes_with(statics: &[f32], rel: Option<&[f32]>, tokens: &Tokens) -> Vec<f32> {
    let np = M_IN + if rel.is_some() { S_REL } else { 0 };
    let mut p = vec![0f32; TILES * np];
    for i in 0..TILES {
        p[i * np..i * np + S_CH].copy_from_slice(&statics[i * S_CH..(i + 1) * S_CH]);
        if let Some(r) = rel {
            p[i * np + M_IN..(i + 1) * np].copy_from_slice(&r[i * S_REL..(i + 1) * S_REL]);
        }
    }
    for (t, f) in tokens.feat.iter().enumerate() {
        let Ok(i) = usize::try_from(tokens.pos[t]) else { continue };
        for (k, src) in LOCAL.iter().enumerate() {
            p[i * np + S_CH + k] += f[*src];
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

/// The cache of the key / value tokens (plan 11.5, step 1.5: "history, notes, summaries encoded once per turn"): the
/// embedding of a fact, a note or a third token and its key and value at each layer are kept by CONTENT (the input
/// numbers' bits, the net's id), so a token unchanged since an earlier decision is not encoded again. The numbers are
/// those the same code computed then, bit for bit (`ARMY_NET_NO_CACHE=1` recomputes them: the guard of the equality).
/// A fact's keys live while the fact lives; a note's and a summary's for the turn; a third token's while its numbers
/// stand. Per thread (a play runs on one thread); emptied when it holds `KV_MAX` rows.
#[derive(Default)]
struct KvCache {
    rows: std::collections::HashMap<Vec<u32>, std::rc::Rc<Vec<f32>>>,
    hits: u64,
    misses: u64,
}

const KV_MAX: usize = 1 << 14;
const KV_FACT: u32 = 1;
const KV_NOTE: u32 = 2;
const KV_THIRD: u32 = 3;
const KV_LAYER: u32 = 16;

thread_local! {
    static KV: std::cell::RefCell<KvCache> = std::cell::RefCell::new(KvCache::default());
}

fn kv_off() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var_os("ARMY_NET_NO_CACHE").is_some())
}

/// The cache's hits and misses on this thread so far.
pub fn cache_stats() -> (u64, u64) {
    KV.with(|c| {
        let c = c.borrow();
        (c.hits, c.misses)
    })
}

/// The cache key of numbers `parts` of kind `tag` for net `id`.
fn kv_key(id: u64, tag: u32, parts: &[&[f32]]) -> Vec<u32> {
    let mut k = Vec::with_capacity(3 + parts.iter().map(|p| p.len() + 1).sum::<usize>());
    k.extend([id as u32, (id >> 32) as u32, tag]);
    for p in parts {
        k.push(p.len() as u32);
        k.extend(p.iter().map(|x| x.to_bits()));
    }
    k
}

/// The row of `key`: cached, or `make` (then cached).
fn kv_get(key: Vec<u32>, make: impl FnOnce() -> Vec<f32>) -> std::rc::Rc<Vec<f32>> {
    if kv_off() {
        return std::rc::Rc::new(make());
    }
    if let Some(v) = KV.with(|c| {
        let mut c = c.borrow_mut();
        let v = c.rows.get(&key).cloned();
        if v.is_some() {
            c.hits += 1;
        }
        v
    }) {
        return v;
    }
    let v = std::rc::Rc::new(make());
    KV.with(|c| {
        let mut c = c.borrow_mut();
        if c.rows.len() >= KV_MAX {
            c.rows.clear();
        }
        c.misses += 1;
        c.rows.insert(key, v.clone());
    });
    v
}

static NET_IDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A modern layer (`army/model.py`, `ModernLayer`): RMSNorm, QK-norm, gated attention with the distance bias, SwiGLU.
#[derive(Debug, Clone)]
struct Modern {
    rms1: Vec<f32>,
    q: Dense,
    k: Dense,
    v: Dense,
    g: Dense,
    o: Dense,
    /// `[dh]`, shared by the heads.
    qn: Vec<f32>,
    kn: Vec<f32>,
    /// `[heads][dmax + 1]`.
    bias: Vec<f32>,
    rms2: Vec<f32>,
    w1: Dense,
    w3: Dense,
    w2: Dense,
}

#[derive(Debug, Clone)]
enum Block {
    Classic(Layer),
    Modern(Modern),
}

/// `y = x / sqrt(mean(x^2) + 1e-5) * w` (MLX `nn.RMSNorm`).
fn rms_norm(x: &[f32], w: &[f32], y: &mut [f32]) {
    let ms = x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32;
    let inv = 1.0 / (ms + 1e-5).sqrt();
    for i in 0..x.len() {
        y[i] = x[i] * inv * w[i];
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// The loaded net.
#[derive(Debug, Clone)]
pub struct Net {
    /// This load's id (the cache's keys).
    id: u64,
    pub config: Config,
    e1: Dense,
    e2: Dense,
    layers: Vec<Block>,
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
    /// Variant MBA: the stake's projection `[3][d]` (None: no price input).
    price: Option<Vec<f32>>,
    /// Step 1.8 (MBP): the stake's projection `w_price_all` `[3][d]` added to EVERY army token (None: none).
    price_all: Option<Vec<f32>>,
    /// Variant MBH: the history's separate cross-attention (None: none).
    xhist: Option<XHistW>,
    /// Variant MBN: the notes' separate cross-attention and heads (None: none).
    xnotes: Option<XNoteW>,
    /// MBe: the hand-set term "a wounded unit does not end its turn in the city's strike range" (`ww`, None: none).
    ww: Option<[f32; WW_LEN]>,
    /// MBT: typed tokens and the third parties' path (None: none; the third tokens are not read).
    typed: Option<TypedW>,
}

/// Token types of MBT (`army/model_mb.py` `MBTNet`): the objective, a unit of the main stream, a city of it, a third
/// party's unit, a third party's city.
pub const TYPES: usize = 5;
const TY_OBJ: usize = 0;
const TY_UNIT: usize = 1;
const TY_CITY: usize = 2;
const TY_TUNIT: usize = 3;
const TY_TCITY: usize = 4;

/// MBT's weights (`work/nets/net-types/GATES.md` "gate T1"): the objective's and the city's encoders (`ty_obj1/2`,
/// `ty_city1/2`; the unit's is `e1`/`e2`), the type embedding `ty_emb` [TYPES][d], the extras' projection `ty_x`
/// [X_TOK][d]; the third tokens' encoders (`tp_u1/2` units, `tp_c1/2` cities), their extras' projection `tp_x`, and per
/// layer a separate cross-attention of the stream to them (the layer's q / k / v, norm1 on their embedding, a null key
/// `tp_null[l][h]`, the distance bias `tp_dist[l][h][d]` for a main-token query, 0 for a plan token's) whose context
/// enters through `tp_o[l]`; the candidates' extras' projection `cx` [X_ACT][d] into the scorer's first layer and the
/// shift `c_new` of the new candidate kind ("declares war").
#[derive(Debug, Clone)]
struct TypedW {
    obj1: Dense,
    obj2: Dense,
    city1: Dense,
    city2: Dense,
    emb: Vec<f32>,
    x: Vec<f32>,
    u1: Dense,
    u2: Dense,
    c1: Dense,
    c2: Dense,
    tx: Vec<f32>,
    null: Vec<f32>,
    dist: Vec<f32>,
    o: Vec<Dense>,
    cx: Vec<f32>,
    c_new: f32,
}

/// The type of main token `i` (by its kind numbers).
fn main_type(f: &[f32; F_TOK]) -> usize {
    if f[super::observe::TOK_OBJECTIVE] > 0.5 {
        TY_OBJ
    } else if f[super::observe::TOK_CITY] > 0.5 {
        TY_CITY
    } else {
        TY_UNIT
    }
}

/// `y += x M` for a row `x` and a matrix `M` stored `[in][out]`.
fn add_proj(x: &[f32], m: &[f32], y: &mut [f32]) {
    let d = y.len();
    for (k, v) in x.iter().enumerate() {
        if *v != 0.0 {
            for c in 0..d {
                y[c] += v * m[k * d + c];
            }
        }
    }
}

/// The numbers of the hand-set term `ww` (`army/model_mb.py`, `MBENet`; M-HANDOFF.md, "Hand edit: wounded out of range"):
/// [scale, hp threshold, hp slope, ranged hp threshold, resting weight, city-low threshold, city-low slope, resting slope,
/// stay scale, healing weight, back threshold, strike scale, ranged strike threshold, stay escape, ranged engaged threshold,
/// foe city only].
pub const WW_LEN: usize = 16;
/// Half the smallest step of a token's hit points (1 / 100): "gained since our last turn" is number 46 above it.
const HEALING_STEP: f32 = 0.005;
/// Half the step of the threat numbers (a count / 4): a candidate's threat (19) is below the unit's (39) by more than it.
const THREAT_STEP: f32 = 0.125;

/// Variant MBN's note weights (`army/model_mb.py`, `MBNNet`): a note's embedding `n2(relu(n1(note)))`; per layer a
/// cross-attention like MBH's (`null`, `same`, `slope` per layer and head; its context through `no[layer]`); the heads
/// written at our turn's end: `rest` on [H_i, token, u_s, resting before], `reason` on its indicators, `free` on P_0,
/// the plan token's stage and engaged readouts (`aux_p`).
#[derive(Debug, Clone)]
struct XNoteW {
    n1: Dense,
    n2: Dense,
    null: Vec<f32>,
    same: Vec<f32>,
    slope: Vec<f32>,
    no: Vec<Dense>,
    rest: Dense,
    reason: Dense,
    free: Dense,
    /// `aux_p` [[stage scale, shift], [engaged scale, shift]].
    aux_p: [f32; 4],
}

/// MBN's note numbers: MN's and PLAN's engaged.
const F_XNOTE: usize = F_NOTE + 1;
const P_ENGAGED_N: usize = 10;

/// The ablation of MBN's proof: the notes masked in every body (`ARMY_NET_NO_NOTES=1`).
fn notes_masked() -> bool {
    static NO: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *NO.get_or_init(|| std::env::var_os("ARMY_NET_NO_NOTES").is_some())
}

/// Variant MBH's history weights (`army/model_mb.py`, `MBHNet`): a fact's embedding `hx2(relu(hx1(fact, its age
/// number zeroed))) + hxp(P_0 of its turn)`; per layer a cross-attention of the tokens and plan tokens to the facts
/// with its own softmax over a null key (logit `null`, value 0) and the facts (the layer's q / k / v, minus
/// `slope` x age, plus `same` for a fact of the query's unit), its context through `ho[layer]` into the residual.
#[derive(Debug, Clone)]
struct XHistW {
    h1: Dense,
    h2: Dense,
    hp: Dense,
    /// `[layer][head]`.
    null: Vec<f32>,
    same: Vec<f32>,
    slope: Vec<f32>,
    ho: Vec<Dense>,
}

/// The fact's own age number (history column 10): MBH's embedding reads it as zero (age enters as a bias).
const HX_AGE_COL: usize = 10;

/// MBH's facts for one body: their embedding `[nh][d]`, slots, ages.
struct XHistIn {
    he: Vec<f32>,
    slots: Vec<i16>,
    ages: Vec<f32>,
    qslots: Vec<i16>,
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
    /// MBN: turns in the stage, the plan engaged (MBN keeps its resting units in `pull` / `pull_age`: a state).
    pub stage_age: f32,
    pub engaged: f32,
}

impl NoteState {
    fn new() -> NoteState {
        NoteState { stage: [0.0; STAGES], waited: 0.0, free: [0.0; FREE], change_age: AGE_NONE, reason: [0.0; 4], pull: [0.0; SLOTS], pull_age: [0.0; SLOTS], written: Vec::new(), stage_age: 0.0, engaged: 0.0 }
    }

    /// MBN's note tokens in force (`MBNNet.note_tokens`, the masked ones left out): numbers, the unit's slot, age.
    fn tokens_x(&self) -> Vec<([f32; F_XNOTE], i16, f32)> {
        let tok = |kind: usize, stage: Option<&[f32; STAGES]>, reason: Option<&[f32; 4]>, waited: f32, age: f32, free: Option<&[f32; FREE]>, engaged: f32| {
            let mut f = [0f32; F_XNOTE];
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
                f[17..17 + FREE].copy_from_slice(fr);
            }
            f[F_NOTE] = engaged;
            f
        };
        let has_plan = self.stage.iter().sum::<f32>() > 0.0;
        let (sa, ca) = (self.stage_age.min(9.0), self.change_age.min(9.0));
        let mut out = Vec::new();
        if has_plan {
            out.push((tok(0, Some(&self.stage), None, 0.0, sa, Some(&self.free), self.engaged), -1, sa));
        }
        if self.change_age < CHANGE_SPAN {
            out.push((tok(1, Some(&self.stage), Some(&self.reason), 0.0, ca, None, 0.0), -1, ca));
        }
        if has_plan && self.engaged < 0.5 {
            out.push((tok(2, None, None, self.waited, 0.0, None, 0.0), -1, 0.0));
        }
        for s in 0..SLOTS {
            if self.pull[s] > 0.5 {
                let ra = self.pull_age[s].min(9.0);
                out.push((tok(3, None, None, 0.0, ra, None, 0.0), s as i16, ra));
            }
        }
        out
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
/// The unit slot's "resting" number (`model_m.U_RESTING`).
const U_RESTING_IDX: usize = 1;
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
    /// The plan token's readouts `aux_p` [[stage scale, shift], [engaged scale, shift]] (None: not exported).
    aux_p: Option<[f32; 4]>,
    /// The unit readout of "resting" (`aux_u[1]`: scale, shift on `u[U_RESTING]`; None: not exported).
    aux_rest: Option<[f32; 2]>,
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
        let kinds: Vec<char> = match &c.kinds {
            Some(k) => k.chars().collect(),
            None => vec!['c'; c.layers],
        };
        if kinds.len() != c.layers || kinds.iter().any(|k| !matches!(k, 'c' | 'm')) {
            return Err(format!("kinds {:?} for {} layers", c.kinds, c.layers));
        }
        let plain = |name: &str, n_in: usize, n_out: usize| -> Result<Dense, String> {
            let (shape, w) = get(&format!("{name}.weight"))?;
            if shape.as_slice() != [n_in, n_out] {
                return Err(format!("{name} is {shape:?}, not [{n_in}, {n_out}]"));
            }
            Dense::new(w, shape, &vec![0.0; n_out])
        };
        let mut layers = Vec::new();
        for (l, kind) in kinds.iter().enumerate() {
            let p = format!("layers.{l}");
            if *kind == 'm' {
                let ff = get(&format!("{p}.w1.weight"))?.0.get(1).copied().ok_or("w1 is not 2-D")?;
                layers.push(Block::Modern(Modern {
                    rms1: get(&format!("{p}.rms1.weight"))?.1.clone(),
                    q: dense(&format!("{p}.q"))?,
                    k: dense(&format!("{p}.k"))?,
                    v: dense(&format!("{p}.v"))?,
                    g: dense(&format!("{p}.g"))?,
                    o: dense(&format!("{p}.o"))?,
                    qn: get(&format!("{p}.qn.weight"))?.1.clone(),
                    kn: get(&format!("{p}.kn.weight"))?.1.clone(),
                    bias: get(&format!("{p}.dist_bias"))?.1.clone(),
                    rms2: get(&format!("{p}.rms2.weight"))?.1.clone(),
                    w1: plain(&format!("{p}.w1"), c.d, ff)?,
                    w3: plain(&format!("{p}.w3"), c.d, ff)?,
                    w2: dense(&format!("{p}.w2"))?,
                }));
                continue;
            }
            layers.push(Block::Classic(Layer {
                norm1: norm(&format!("{p}.norm1"))?,
                q: dense(&format!("{p}.q"))?,
                k: dense(&format!("{p}.k"))?,
                v: dense(&format!("{p}.v"))?,
                o: dense(&format!("{p}.o"))?,
                bias: get(&format!("{p}.dist_bias"))?.1.clone(),
                norm2: norm(&format!("{p}.norm2"))?,
                f1: dense(&format!("{p}.f1"))?,
                f2: dense(&format!("{p}.f2"))?,
            }));
        }
        if kinds.contains(&'m') && ["h1.weight", "hx_slope", "nx_slope", "n1.weight", "ty_emb"].iter().any(|n| t.contains_key(*n)) {
            return Err("a net with modern layers has no history, notes or typed tokens (hist, xhist, xnotes, notes, ty_emb)".into());
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
            let aux_p = t.get("aux_p").filter(|(s, _)| s.as_slice() == [2, 2]).map(|(_, v)| [v[0], v[1], v[2], v[3]]);
            let aux_rest = t.get("aux_u").filter(|(s, _)| s.as_slice() == [4, 2]).map(|(_, v)| [v[2], v[3]]);
            Some(Memory { um: dense("um")?, plan0: raw("plan0")?, plan_bias: raw("plan_bias")?, gu, pq: dense("pq")?, pk: dense("pk")?, gp, hyst: raw("hyst")?, aux_p, aux_rest })
        } else {
            None
        };
        let notes = if t.contains_key("n1.weight") && !t.contains_key("nx_slope") {
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
        let xhist = if t.contains_key("hx_slope") {
            let raw = |name: &str| -> Result<Vec<f32>, String> { Ok(get(name)?.1.clone()) };
            let ho = (0..c.layers).map(|l| dense(&format!("ho.{l}"))).collect::<Result<Vec<_>, _>>()?;
            Some(XHistW { h1: dense("hx1")?, h2: dense("hx2")?, hp: dense("hxp")?, null: raw("hx_null")?, same: raw("hx_same")?, slope: raw("hx_slope")?, ho })
        } else {
            None
        };
        let xnotes = if t.contains_key("nx_slope") {
            let raw = |name: &str| -> Result<Vec<f32>, String> { Ok(get(name)?.1.clone()) };
            let no = (0..c.layers).map(|l| dense(&format!("no.{l}"))).collect::<Result<Vec<_>, _>>()?;
            let aux = raw("aux_p")?;
            if get("n1.weight")?.0.first() != Some(&F_XNOTE) {
                return Err(format!("MBN notes of {:?} numbers, the code has {F_XNOTE}", get("n1.weight")?.0));
            }
            Some(XNoteW {
                n1: dense("n1")?,
                n2: dense("n2")?,
                null: raw("nx_null")?,
                same: raw("nx_same")?,
                slope: raw("nx_slope")?,
                no,
                rest: dense("rest")?,
                reason: dense("reason")?,
                free: dense("free")?,
                aux_p: [aux[0], aux[1], aux[2], aux[3]],
            })
        } else {
            None
        };
        let ww = match t.get("ww") {
            Some((shape, v)) if shape.as_slice() == [WW_LEN] => Some(std::array::from_fn(|i| v[i])),
            Some((shape, _)) => return Err(format!("ww is {shape:?}, not [{WW_LEN}]")),
            None => None,
        };
        let typed = if t.contains_key("ty_emb") {
            let raw = |name: &str, shape: &[usize]| -> Result<Vec<f32>, String> {
                let (s, v) = get(name)?;
                if s.as_slice() != shape {
                    return Err(format!("{name} is {s:?}, not {shape:?}"));
                }
                Ok(v.clone())
            };
            let nb = c.dmax + 1;
            Some(TypedW {
                obj1: dense("ty_obj1")?,
                obj2: dense("ty_obj2")?,
                city1: dense("ty_city1")?,
                city2: dense("ty_city2")?,
                emb: raw("ty_emb", &[TYPES, c.d])?,
                x: raw("ty_x", &[X_TOK, c.d])?,
                u1: dense("tp_u1")?,
                u2: dense("tp_u2")?,
                c1: dense("tp_c1")?,
                c2: dense("tp_c2")?,
                tx: raw("tp_x", &[X_TOK, c.d])?,
                null: raw("tp_null", &[c.layers, c.heads])?,
                dist: raw("tp_dist", &[c.layers, c.heads, nb])?,
                o: (0..c.layers).map(|l| dense(&format!("tp_o.{l}"))).collect::<Result<Vec<_>, _>>()?,
                cx: raw("cx", &[X_ACT, c.d])?,
                c_new: raw("c_new", &[1])?[0],
            })
        } else {
            None
        };
        let price = if t.contains_key("w_price") {
            let (shape, w) = get("w_price")?;
            if shape.as_slice() != [3, c.d] {
                return Err(format!("w_price is {shape:?}, not [3, {}]", c.d));
            }
            Some(w.clone())
        } else {
            None
        };
        let price_all = if t.contains_key("w_price_all") {
            let (shape, w) = get("w_price_all")?;
            if shape.as_slice() != [3, c.d] {
                return Err(format!("w_price_all is {shape:?}, not [3, {}]", c.d));
            }
            Some(w.clone())
        } else {
            None
        };
        Ok(Net {
            id: NET_IDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            typed,
            ww,
            xnotes,
            xhist,
            price,
            price_all,
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

    /// MBT: typed tokens and the third parties' path.
    pub fn has_typed(&self) -> bool {
        self.typed.is_some()
    }

    /// The map planes this net's encoder reads: the old `planes`, plus the relation planes `rel` when the encoder has
    /// `M_IN + S_REL` inputs (MBT).
    pub fn planes(&self, statics: &[f32], rel: &[f32], tokens: &Tokens) -> Vec<f32> {
        let wide = self.map.as_ref().is_some_and(|e| e.conv.planes() == M_IN + S_REL);
        planes_with(statics, wide.then_some(rel), tokens)
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

    /// MBN at our turn's end (after the memory's update): the net's own notes (`train_m.unroll_one_n`'s own path,
    /// `MBNNet.note_heads`, `reason_logits`, `write_notes`).
    #[allow(clippy::too_many_arguments)]
    fn write_notes_x(&self, xw: &XNoteW, mut ns: NoteState, body: &Body, tokens: &Tokens, slots: &[i16], state: &MemoryState, event: f32) -> NoteState {
        let d = self.config.d;
        let argmax = |v: &[f32]| v.iter().enumerate().fold((0usize, f32::NEG_INFINITY), |b, (i, x)| if *x > b.1 { (i, *x) } else { b }).0;
        let p0 = &state.p[..d];
        let st: Vec<f32> = (0..STAGES).map(|k| xw.aux_p[0] * p0[k] + xw.aux_p[1]).collect();
        let new = argmax(&st);
        let engaged = if xw.aux_p[2] * p0[P_ENGAGED_N] + xw.aux_p[3] > 0.0 { 1.0 } else { 0.0 };
        let mut rest = [0f32; SLOTS];
        let mut x = vec![0.0; d + F_TOK + U_DIM + 1];
        let mut y = [0f32; 1];
        for i in 0..body.n {
            let Ok(s) = usize::try_from(slots[i]) else { continue };
            if s >= SLOTS {
                continue;
            }
            x[..d].copy_from_slice(&body.h[i * d..(i + 1) * d]);
            x[d..d + F_TOK].copy_from_slice(&tokens.feat[i]);
            x[d + F_TOK..d + F_TOK + U_DIM].copy_from_slice(&state.u[s * U_DIM..(s + 1) * U_DIM]);
            x[d + F_TOK + U_DIM] = ns.pull[s];
            xw.rest.apply(&x, &mut y);
            if y[0] > 0.0 {
                rest[s] = 1.0;
            }
        }
        let had = ns.stage.iter().sum::<f32>() > 0.0;
        let old = argmax(&ns.stage);
        let ind = [event, if new == STAGE_HEAL { 1.0 } else { 0.0 }, if had && new > old { 1.0 } else { 0.0 }, 0.0];
        let mut rl = [0f32; 4];
        xw.reason.apply(&ind, &mut rl);
        let reason = argmax(&rl);
        let changed = had && old != new;
        let mut written = vec![format!("PLAN(objective, {}{})", STAGE_NAMES[new], if engaged > 0.5 { ", engaged" } else { "" })];
        ns.stage_age = if had && !changed { ns.stage_age + 1.0 } else { 0.0 };
        ns.change_age = if changed { 0.0 } else { ns.change_age + 1.0 };
        if changed {
            ns.reason = [0.0; 4];
            ns.reason[reason] = 1.0;
            written.push(format!("PLAN_CHANGE({})", REASONS[reason]));
        }
        for s in 0..SLOTS {
            let was = ns.pull[s] > 0.5;
            let now = rest[s] > 0.5;
            ns.pull_age[s] = if now { if was { ns.pull_age[s] + 1.0 } else { 0.0 } } else { 0.0 };
            if now && !was {
                written.push(format!("PULL_BACK_HEAL(slot {s})"));
            }
            if was && !now {
                written.push(format!("RETURN(slot {s})"));
            }
            ns.pull[s] = rest[s];
        }
        let mut stage = [0f32; STAGES];
        stage[new] = 1.0;
        ns.stage = stage;
        ns.engaged = engaged;
        ns.waited = (p0[P_WAITED] * 10.0).max(0.0);
        if engaged < 0.5 {
            written.push(format!("WAIT_STAGE({:.0})", ns.waited));
        }
        let mut fr = [0f32; FREE];
        xw.free.apply(p0, &mut fr);
        for v in fr.iter_mut() {
            *v = v.tanh();
        }
        ns.free = fr;
        ns.written = written;
        ns
    }

    /// The plan's own stage and engaged (the aux readouts on P_0; the diagnosis of the stage per round), None
    /// without memory or readouts.
    pub fn plan_stage(&self, state: &MemoryState) -> Option<(usize, bool)> {
        let a = self.mem.as_ref()?.aux_p?;
        let p0 = &state.p[..self.config.d];
        let st: Vec<f32> = (0..STAGES).map(|k| a[0] * p0[k] + a[1]).collect();
        let s = st.iter().enumerate().fold((0usize, f32::NEG_INFINITY), |b, (i, x)| if *x > b.1 { (i, *x) } else { b }).0;
        Some((s, a[2] * p0[P_ENGAGED_N] + a[3] > 0.0))
    }

    /// The memory's own "resting" per army slot (`aux_u[1]` on `u[U_RESTING]`, the unit readout the auxiliary loss trains): info for
    /// the unit diagnosis, never read by the play. None without memory or without the readout.
    pub fn resting_readout(&self, state: &MemoryState) -> Option<[bool; SLOTS]> {
        let a = self.mem.as_ref()?.aux_rest?;
        let mut out = [false; SLOTS];
        for (s, o) in out.iter_mut().enumerate() {
            *o = a[0] * state.u[s * U_DIM + U_RESTING_IDX] + a[1] > 0.0;
        }
        Some(out)
    }

    pub fn has_memory(&self) -> bool {
        self.mem.is_some()
    }

    /// Variant M: the memory at a play's start (None: no memory).
    pub fn initial_memory(&self) -> Option<MemoryState> {
        let m = self.mem.as_ref()?;
        let notes = (self.notes.is_some() || self.xnotes.is_some()).then(NoteState::new);
        Some(MemoryState { u: vec![0.0; SLOTS * U_DIM], p: m.plan0.clone(), alive: [0.0; SLOTS], p0_turns: Vec::new(), notes })
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
        if let (Some(xw), Some(ns)) = (self.xnotes.as_ref(), state.notes.clone()) {
            state.notes = Some(self.write_notes_x(xw, ns, body, tokens, slots, state, event));
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
        let mut h = match self.typed.as_ref() {
            None => {
                let mut e = self.e1.rows(&flat, n);
                relu(&mut e);
                self.e2.rows(&e, n)
            }
            // MBT: each type its encoder, then its type embedding and its extras' projection.
            Some(tw) => {
                let mut h = vec![0.0; n * d];
                let mut e = vec![0.0; d];
                for i in 0..n {
                    let ty = main_type(&tokens.feat[i]);
                    let (a, b) = match ty {
                        TY_OBJ => (&tw.obj1, &tw.obj2),
                        TY_CITY => (&tw.city1, &tw.city2),
                        _ => (&self.e1, &self.e2),
                    };
                    a.apply(&tokens.feat[i], &mut e);
                    relu(&mut e);
                    let row = &mut h[i * d..(i + 1) * d];
                    b.apply(&e, row);
                    for c in 0..d {
                        row[c] += tw.emb[ty * d + c];
                    }
                    if let Some(x) = tokens.xfeat.get(i) {
                        add_proj(x, &tw.x, row);
                    }
                }
                h
            }
        };
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
        // MBP (step 1.8): the stake's projection on every army token (`w_price_all` [3, d]); the plan tokens are not army tokens.
        if let Some(wa) = self.price_all.as_ref() {
            for row in h.chunks_mut(d).take(n) {
                for (k, p) in tokens.price.iter().enumerate() {
                    for c in 0..d {
                        row[c] += p * wa[k * d + c];
                    }
                }
            }
        }
        // MBA: the stake's projection on the objective token (`w_price` [3, d]).
        if let Some(wp) = self.price.as_ref() {
            for (k, p) in tokens.price.iter().enumerate() {
                for c in 0..d {
                    h[c] += p * wp[k * d + c];
                }
            }
        }
        let total = n + plans;
        // MBH: the facts' embedding (fixed while a fact lives: its turn's plan, its age only as a bias).
        let xin = match (self.xhist.as_ref(), mem) {
            (Some(xw), Some(mi)) => {
                let mut f = [0f32; super::observe::F_HIST];
                let mut e1 = vec![0.0; d];
                let mut y = vec![0.0; d];
                let mut z = vec![0.0; d];
                let mut he = Vec::with_capacity(tokens.hist.len() * d);
                for (k, fact) in tokens.hist.iter().enumerate() {
                    f.copy_from_slice(fact);
                    f[HX_AGE_COL] = 0.0;
                    let p0 = mi.state.p0_at(tokens.hist_age[k], d);
                    let row = kv_get(kv_key(self.id, KV_FACT, &[&f, p0]), || {
                        xw.h1.apply(&f, &mut e1);
                        relu(&mut e1);
                        xw.h2.apply(&e1, &mut y);
                        xw.hp.apply(p0, &mut z);
                        y.iter().zip(&z).map(|(a, b)| a + b).collect()
                    });
                    he.extend_from_slice(&row);
                }
                let qslots = (0..n).map(|i| mi.slots[i]).chain(std::iter::repeat(-1).take(plans)).collect();
                Some(XHistIn { he, slots: tokens.hist_slot.clone(), ages: tokens.hist_age.iter().map(|a| f32::from(*a)).collect(), qslots })
            }
            _ => None,
        };
        // MBN: the notes in force (their embedding, slots, ages); none when masked (the ablation).
        let nin = match (self.xnotes.as_ref(), mem) {
            (Some(xw), Some(mi)) => {
                let notes = if notes_masked() { Vec::new() } else { mi.state.notes.as_ref().map(|n| n.tokens_x()).unwrap_or_default() };
                let mut e1 = vec![0.0; d];
                let mut y = vec![0.0; d];
                let mut he = Vec::with_capacity(notes.len() * d);
                for (f, _, _) in &notes {
                    let row = kv_get(kv_key(self.id, KV_NOTE, &[f]), || {
                        xw.n1.apply(f, &mut e1);
                        relu(&mut e1);
                        xw.n2.apply(&e1, &mut y);
                        y.clone()
                    });
                    he.extend_from_slice(&row);
                }
                let qslots = (0..n).map(|i| mi.slots[i]).chain(std::iter::repeat(-1).take(plans)).collect();
                Some(XHistIn { he, slots: notes.iter().map(|x| x.1).collect(), ages: notes.iter().map(|x| x.2).collect(), qslots })
            }
            _ => None,
        };
        // MBT: the third tokens' embedding (their encoder, type, extras, the map at their tile).
        let te: Option<Vec<f32>> = self.typed.as_ref().map(|tw| {
            let th = &tokens.third;
            let m = th.len();
            let mut te = vec![0.0; m * d];
            let mut e = vec![0.0; d];
            let mut y = vec![0.0; d];
            for j in 0..m {
                let city = th.is_city(j);
                let (a, b, ty) = if city { (&tw.c1, &tw.c2, TY_TCITY) } else { (&tw.u1, &tw.u2, TY_TUNIT) };
                let at = match (self.map.as_ref(), map, usize::try_from(th.pos[j])) {
                    (Some(enc), Some(st), Ok(p)) => {
                        let mc = enc.conv.channels();
                        Some(&st.out()[p * mc..(p + 1) * mc])
                    }
                    _ => None,
                };
                let cached = kv_get(kv_key(self.id, KV_THIRD, &[&th.feat[j], &th.x[j], at.unwrap_or(&[])]), || {
                    let mut row = vec![0.0; d];
                    a.apply(&th.feat[j], &mut e);
                    relu(&mut e);
                    b.apply(&e, &mut row);
                    for c in 0..d {
                        row[c] += tw.emb[ty * d + c];
                    }
                    add_proj(&th.x[j], &tw.tx, &mut row);
                    if let (Some(enc), Some(mrow)) = (self.map.as_ref(), at) {
                        enc.tok.apply(mrow, &mut y);
                        for c in 0..d {
                            row[c] += y[c];
                        }
                    }
                    row
                });
                te[j * d..(j + 1) * d].copy_from_slice(&cached);
            }
            te
        });
        let mut a = vec![0.0; total * d];
        let hist_in = if he.is_empty() { None } else { Some((he.as_slice(), hslots.as_slice(), qslots.as_slice())) };
        for (li, block) in self.layers.iter().enumerate() {
            let layer = match block {
                Block::Classic(layer) => layer,
                Block::Modern(m) => {
                    self.modern(m, li, &tokens.dist, n, total, &mut h);
                    continue;
                }
            };
            let q = self.attend(layer, li, &tokens.dist, n, total, &mut h, &mut a, hist_in);
            if let (Some(xw), Some(xi)) = (self.xhist.as_ref(), xin.as_ref()) {
                let heads = self.config.heads;
                let r = li * heads..(li + 1) * heads;
                self.cross(&xw.null[r.clone()], &xw.same[r.clone()], &xw.slope[r], &xw.ho[li], layer, total, &q, &mut h, xi);
            }
            if let (Some(xw), Some(xi)) = (self.xnotes.as_ref(), nin.as_ref()) {
                let heads = self.config.heads;
                let r = li * heads..(li + 1) * heads;
                self.cross(&xw.null[r.clone()], &xw.same[r.clone()], &xw.slope[r], &xw.no[li], layer, total, &q, &mut h, xi);
            }
            if let (Some(tw), Some(te)) = (self.typed.as_ref(), te.as_ref()) {
                self.cross_third(tw, li, layer, n, total, &q, &mut h, te, &tokens.third.dist);
            }
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
    /// The layer's self-attention (MH: with the history in its softmax) added to `h`; leaves norm1 of the old
    /// stream in `a` and returns the queries (MBH's cross-attention reads both).
    fn attend(&self, layer: &Layer, li: usize, dist: &[u8], n: usize, total: usize, h: &mut [f32], a: &mut [f32], hist: Option<(&[f32], &[i16], &[i16])>) -> Vec<f32> {
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
        q
    }

    /// A modern layer (`ModernLayer.forward`): gated QK-normed attention with the distance bias (and the plan
    /// pairs' bias, as `attend`), then the SwiGLU MLP, both added to `h`.
    fn modern(&self, m: &Modern, li: usize, dist: &[u8], n_real: usize, n: usize, h: &mut [f32]) {
        let d = self.config.d;
        let heads = self.config.heads;
        let dh = d / heads;
        let nb = self.config.dmax + 1;
        let plan_bias = self.mem.as_ref().map(|mem| &mem.plan_bias[li * heads..(li + 1) * heads]);
        let mut a = vec![0.0; n * d];
        for i in 0..n {
            rms_norm(&h[i * d..(i + 1) * d], &m.rms1, &mut a[i * d..(i + 1) * d]);
        }
        let mut q = m.q.rows(&a, n);
        let mut k = m.k.rows(&a, n);
        let v = m.v.rows(&a, n);
        let gate = m.g.rows(&a, n);
        let mut tmp = vec![0.0; dh];
        for i in 0..n * heads {
            rms_norm(&q[i * dh..(i + 1) * dh], &m.qn, &mut tmp);
            q[i * dh..(i + 1) * dh].copy_from_slice(&tmp);
            rms_norm(&k[i * dh..(i + 1) * dh], &m.kn, &mut tmp);
            k[i * dh..(i + 1) * dh].copy_from_slice(&tmp);
        }
        let scale = 1.0 / (dh as f32).sqrt();
        let mut ctx = vec![0.0; n * d];
        let mut s = vec![0.0; n];
        for hd in 0..heads {
            for i in 0..n {
                let qi = &q[i * d + hd * dh..i * d + hd * dh + dh];
                let mut max = f32::NEG_INFINITY;
                for j in 0..n {
                    let kj = &k[j * d + hd * dh..j * d + hd * dh + dh];
                    let bias = if i < n_real && j < n_real { m.bias[hd * nb + usize::from(dist[i * n_real + j])] } else { plan_bias.map_or(0.0, |p| p[hd]) };
                    s[j] = dot(qi, kj) * scale + bias;
                    max = max.max(s[j]);
                }
                let mut sum = 0.0;
                for x in s.iter_mut() {
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
        for (c, g) in ctx.iter_mut().zip(&gate) {
            *c *= sigmoid(*g);
        }
        for (x, y) in h.iter_mut().zip(m.o.rows(&ctx, n)) {
            *x += y;
        }
        for i in 0..n {
            rms_norm(&h[i * d..(i + 1) * d], &m.rms2, &mut a[i * d..(i + 1) * d]);
        }
        let mut f = m.w1.rows(&a, n);
        let u = m.w3.rows(&a, n);
        for (x, y) in f.iter_mut().zip(&u) {
            *x = *x * sigmoid(*x) * y;
        }
        for (x, y) in h.iter_mut().zip(m.w2.rows(&f, n)) {
            *x += y;
        }
    }

    /// MBH / MBN: the cross-attention of the `n` stream tokens (queries `q`) to the facts or the notes, its own
    /// softmax over the null key and them (`null`, `same`, `slope`: this layer's per head), through `out` into `h`.
    #[allow(clippy::too_many_arguments)]
    fn cross(&self, nulls: &[f32], sames: &[f32], slopes: &[f32], out_proj: &Dense, layer: &Layer, n: usize, q: &[f32], h: &mut [f32], xi: &XHistIn) {
        let d = self.config.d;
        let heads = self.config.heads;
        let dh = d / heads;
        let nh = xi.he.len() / d;
        let li = self.layers.iter().position(|b| matches!(b, Block::Classic(l) if std::ptr::eq(l, layer))).unwrap_or(usize::MAX);
        let (kh, vh) = self.key_values(li, layer, &xi.he);
        let scale = 1.0 / (dh as f32).sqrt();
        let mut ctx = vec![0.0; n * d];
        let mut s = vec![0.0; nh];
        for hd in 0..heads {
            let (null, same, slope) = (nulls[hd], sames[hd], slopes[hd]);
            for i in 0..n {
                let qi = &q[i * d + hd * dh..i * d + hd * dh + dh];
                let mut max = null;
                for j in 0..nh {
                    let kj = &kh[j * d + hd * dh..j * d + hd * dh + dh];
                    let own = xi.slots[j] >= 0 && xi.qslots[i] == xi.slots[j];
                    s[j] = dot(qi, kj) * scale - slope * xi.ages[j] + if own { same } else { 0.0 };
                    max = max.max(s[j]);
                }
                let mut sum = (null - max).exp();
                for x in s.iter_mut() {
                    *x = (*x - max).exp();
                    sum += *x;
                }
                let out = &mut ctx[i * d + hd * dh..i * d + hd * dh + dh];
                for j in 0..nh {
                    let w = s[j] / sum;
                    let vj = &vh[j * d + hd * dh..j * d + hd * dh + dh];
                    for c in 0..dh {
                        out[c] += w * vj[c];
                    }
                }
            }
        }
        let o = out_proj.rows(&ctx, n);
        for (x, y) in h.iter_mut().zip(o) {
            *x += y;
        }
    }

    /// MBT: the cross-attention of the `total` stream tokens (queries `q`; the first `n` are the main tokens) to the third
    /// tokens (embedding `te`, distances `dist` [n][m]): its own softmax over the null key and them, the distance bias for
    /// a main token's query, through `tp_o[li]` into `h`.
    #[allow(clippy::too_many_arguments)]
    fn cross_third(&self, tw: &TypedW, li: usize, layer: &Layer, n: usize, total: usize, q: &[f32], h: &mut [f32], te: &[f32], dist: &[u8]) {
        let d = self.config.d;
        let heads = self.config.heads;
        let dh = d / heads;
        let nb = self.config.dmax + 1;
        let m = te.len() / d;
        let mut ctx = vec![0.0; total * d];
        if m > 0 {
            let (kh, vh) = self.key_values(li, layer, te);
            let scale = 1.0 / (dh as f32).sqrt();
            let mut s = vec![0.0; m];
            for hd in 0..heads {
                let null = tw.null[li * heads + hd];
                let bias = &tw.dist[(li * heads + hd) * nb..(li * heads + hd + 1) * nb];
                for i in 0..total {
                    let qi = &q[i * d + hd * dh..i * d + hd * dh + dh];
                    let mut max = null;
                    for j in 0..m {
                        let kj = &kh[j * d + hd * dh..j * d + hd * dh + dh];
                        s[j] = dot(qi, kj) * scale + if i < n { bias[usize::from(dist[i * m + j])] } else { 0.0 };
                        max = max.max(s[j]);
                    }
                    let mut sum = (null - max).exp();
                    for x in s.iter_mut() {
                        *x = (*x - max).exp();
                        sum += *x;
                    }
                    let out = &mut ctx[i * d + hd * dh..i * d + hd * dh + dh];
                    for j in 0..m {
                        let w = s[j] / sum;
                        let vj = &vh[j * d + hd * dh..j * d + hd * dh + dh];
                        for c in 0..dh {
                            out[c] += w * vj[c];
                        }
                    }
                }
            }
        }
        if m == 0 {
            // No third token: the context is zero and the output its bias (the same numbers as the product).
            for i in 0..total {
                for (x, b) in h[i * d..(i + 1) * d].iter_mut().zip(&tw.o[li].b) {
                    *x += b;
                }
            }
            return;
        }
        let o = tw.o[li].rows(&ctx, total);
        for (x, y) in h.iter_mut().zip(o) {
            *x += y;
        }
    }

    /// The keys and values `[rows][d]` at layer `li` of the key / value tokens `e` (`[rows][d]`, a fact's, a note's, a third
    /// token's embedding): the layer's norm1, then its k and v, each row through the cache.
    fn key_values(&self, li: usize, layer: &Layer, e: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let d = self.config.d;
        let rows = e.len() / d;
        let mut kh = Vec::with_capacity(e.len());
        let mut vh = Vec::with_capacity(e.len());
        let mut a = vec![0.0; d];
        for j in 0..rows {
            let ej = &e[j * d..(j + 1) * d];
            let kv = kv_get(kv_key(self.id, KV_LAYER + li as u32, &[ej]), || {
                layer.norm1.apply(ej, &mut a);
                let mut out = vec![0.0; 2 * d];
                layer.k.apply(&a, &mut out[..d]);
                layer.v.apply(&a, &mut out[d..]);
                out
            });
            kh.extend_from_slice(&kv[..d]);
            vh.extend_from_slice(&kv[d..]);
        }
        (kh, vh)
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
            if let Some(tw) = self.typed.as_ref() {
                add_proj(&cand.x, &tw.cx, &mut c1);
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
            match self.typed.as_ref() {
                Some(tw) => out.push(y[0] + tw.c_new * cand.x[X_DECLARES]),
                None => out.push(y[0]),
            }
        }
        out
    }

    /// `actions` plus MBe's hand-set terms (`ww`; the same logits as `actions` without it), both times the unit's gate
    /// g = wounded x (1 - city_low): (1) a non-strike candidate (done, move, fortify, set up) whose tile is in a foe city's strike
    /// range (candidate number 16) loses `scale x g`; (2) a move of a unit that stands out of that range (its token's number 36)
    /// loses `stay scale x g` (a unit that moves does not heal). wounded = max(sigmoid(hp slope (threshold - hp)), resting weight x
    /// sigmoid(resting slope x the memory's resting readout), healing weight x sigmoid(hp slope (back threshold - hp)) x
    /// sigmoid(hp slope (gained since our last turn - HEALING_STEP))), the threshold the ranged one for a unit with range; city_low =
    /// sigmoid(slope (threshold - the objective city's hp share)); (3) a strike (kinds 2-4) loses `strike scale x sigmoid(hp slope
    /// (strike threshold - hp)) x (1 - city_low)`, the strike threshold the hp threshold, for a unit with range the ranged strike
    /// threshold. A unit with range reads the ranged engaged threshold instead of the ranged one while the memory's plan says
    /// engaged (`plan_stage`'s readout: the teacher's shooters fire down to `ROTATE_BURST` once the assault went in). With
    /// `foe city only` = 1 both gates are 0 unless the objective is a foe city (a city token with number 43, not 45: the terms
    /// are the city assault's rotation, not a rule of the field). With
    /// `stay escape` = 1 the stay term spares a move to a tile less threatened than the unit's own (candidate
    /// number 19 below the token's 39: the teacher's rest moves only to a safer or better healing tile). `MBENet.wounded_term` is
    /// the same arithmetic.
    pub fn actions_x(&self, body: &Body, unit: usize, candidates: &[Candidate], map: Option<&MapState>, tokens: &Tokens, mem: Option<&MemIn>) -> Vec<f32> {
        let mut out = self.actions(body, unit, candidates, map);
        let Some(w) = self.ww else { return out };
        let (g, gs) = self.wounded_gate(&w, unit, tokens, mem);
        let out_of_fire = 1.0 - tokens.feat[unit][36];
        let here_threat = tokens.feat[unit][39];
        for (o, c) in out.iter_mut().zip(candidates) {
            let f = c.feat[16] * (c.feat[0] + c.feat[1] + c.feat[5] + c.feat[6]);
            let strike = c.feat[2] + c.feat[3] + c.feat[4];
            let escape = if c.feat[19] < here_threat - THREAT_STEP { w[13] } else { 0.0 };
            *o -= (w[0] * f + w[8] * c.feat[1] * out_of_fire * (1.0 - escape)) * g + w[11] * strike * gs;
        }
        out
    }

    /// The unit's factors of the `ww` terms: (wounded x (1 - city low), its strike gate x (1 - city low)).
    fn wounded_gate(&self, w: &[f32; WW_LEN], unit: usize, tokens: &Tokens, mem: Option<&MemIn>) -> (f32, f32) {
        let sig = |v: f32| 1.0 / (1.0 + (-v).exp());
        let f = &tokens.feat[unit];
        let engaged = match (mem, self.mem.as_ref().and_then(|m| m.aux_p)) {
            (Some(m), Some(a)) => a[2] * m.state.p[P_ENGAGED_N] + a[3] > 0.0,
            _ => false,
        };
        let thr = if f[13] > 0.0 {
            if engaged {
                w[14]
            } else {
                w[3]
            }
        } else {
            w[1]
        };
        let hp = sig(w[2] * (thr - f[7]));
        let rest = match (mem, self.mem.as_ref().and_then(|m| m.aux_rest)) {
            (Some(m), Some(a)) => match usize::try_from(m.slots.get(unit).copied().unwrap_or(-1)) {
                Ok(s) if s < SLOTS => w[4] * sig(w[7] * (a[0] * m.state.u[s * U_DIM + U_RESTING_IDX] + a[1])),
                _ => 0.0,
            },
            _ => 0.0,
        };
        let healing = w[9] * sig(w[2] * (w[10] - f[7])) * sig(w[2] * (f[46] - HEALING_STEP));
        let objective = (0..tokens.len()).find(|i| tokens.feat[*i][3] > 0.5 && tokens.feat[*i][43] > 0.5);
        let city = objective.map_or(1.0, |i| tokens.feat[i][7]);
        let foe_city = objective.is_some_and(|i| tokens.feat[i][45] < 0.5);
        if w[15] > 0.5 && !foe_city {
            return (0.0, 0.0);
        }
        let low = sig(w[6] * (w[5] - city));
        let strike = sig(w[2] * ((if f[13] > 0.0 { w[12] } else { w[1] }) - f[7]));
        (hp.max(rest).max(healing) * (1.0 - low), strike * (1.0 - low))
    }

    pub fn has_ww(&self) -> bool {
        self.ww.is_some()
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

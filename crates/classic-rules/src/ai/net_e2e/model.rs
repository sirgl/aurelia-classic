//! The integer forward pass of the end-to-end net
//! (classic/docs/ai-neural-architecture.md, classic/docs/ai-net-model.md).
//!
//! Integer only, so a game gets the same answer on every platform (no
//! floats): it computes bit for bit what the trainer's NumPy reference
//! (`nn/aurelia_nn/quantize.py`) computes, which a test holds on fixed
//! vectors (`tests/net_e2e_model.rs`).
//!
//! - activations: Q8 (value × 256) in i16, saturated;
//! - a dense layer: int8 weights with a scale per output (`mult / 2^shift`),
//!   the exact integer sum `Σ w·x` (i32 over chunks of 512 inputs, added in
//!   i64), then `round_shift(sum × mult, shift) + bias`, ReLU if any,
//!   saturation;
//! - LayerNorm without affine parameters: integer mean and variance and
//!   `u64::isqrt`;
//! - softmax: a table of `exp(−t/256)` at 2^15 (read from the weights file)
//!   and one rounded division per output;
//! - memory: a GRU per slot kind with sigmoid and tanh tables (2^15, from the
//!   weights file).
//!
//! Speed ([`Speed`]) never changes a value: every sum is an exact integer,
//! so its order, its lanes and its threads do not matter. On aarch64 the
//! sums run on NEON and the int8 dot product (`kernels.rs`), the portable
//! loops here being the reference; `begin_turn` may split its rows over
//! threads; and a [`TurnCache`] kept per player recomputes only the tile
//! states whose inputs changed since that player's last turn
//! (`tests/net_e2e_speed.rs` holds all of them equal to the plain path).
//!
//! Use: [`Model::from_bytes`] once; per player-turn [`Model::begin_turn`]
//! with the player's [`Memory`] into a reusable [`TurnCache`] (tile states
//! and token embeddings, the costly part); per decision [`Model::query`] once
//! and [`Model::score`] per candidate (a few thousand multiply-adds), or
//! [`Model::score_all`]; after the decision, [`Model::note_choice`] with
//! the chosen candidate (the turn trace, weights version 3: each decision's
//! context adds what the turn's earlier choices were); [`Model::end_turn`]
//! gives the memory for the player's next turn.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};

use super::kernels::{self, pad16, Packed};

/// Profiling span names per layer (`prof::span` takes static names).
const CONV_SPANS: [&str; 6] = ["net/model/conv0", "net/model/conv1", "net/model/conv2", "net/model/conv3", "net/model/conv4", "net/model/conv5+"];
const ATTN_SPANS: [&str; 6] = ["net/model/attn0", "net/model/attn1", "net/model/attn2", "net/model/attn3", "net/model/attn4", "net/model/attn5+"];

/// The newest weights file version this reader knows: 5 carries the defer
/// choice (format 9: `Config::defer` and its logit bias); 4 carries the event
/// table (format 7) and, events on, the event cross-attention; 3 carries the
/// turn trace; a version-2, 3 or 4 file reads as before (no defer).
pub const FORMAT_VERSION: u32 = 5;
/// The oldest version it reads.
pub const MIN_FORMAT_VERSION: u32 = 2;

const MAGIC: &[u8; 4] = b"AUNE";
const END: &[u8; 4] = b"END\0";

/// Fraction bits of the activations (Q8).
pub const ACT_BITS: u32 = 8;
const ACT_ONE: i32 = 1 << ACT_BITS;
/// Input normalisation: `x_q = round_shift(stored × mult, INPUT_SHIFT)`.
pub const INPUT_SHIFT: u32 = 16;
/// Inputs summed in i32 at a time: `CHUNK × 127 × 32768 < 2^31`.
const CHUNK: usize = 512;
/// The widest dense layer output (stack buffers).
pub const MAX_OUT: usize = 1024;
/// The longest input row of a table (stack buffers).
pub const MAX_ROW: usize = 1024;

/// GRU gate tables: values at 2^GATE_BITS over Q8 pre-activations.
pub const GATE_BITS: u32 = 15;

/// Entity kinds with a unit slot (an own unit) and with a nation slot (a
/// city-state, another nation).
pub const UNIT_SLOT_KINDS: [i32; 1] = [0];
pub const NATION_SLOT_KINDS: [i32; 2] = [4, 5];

/// Decision heads, in the order of the data's head column (the file's
/// config says how many; these are the ones with an actor's context).
pub const HEAD_UNIT: usize = 0;
pub const HEAD_RESEARCH: usize = 1;
pub const HEAD_PRODUCTION: usize = 2;
pub const HEAD_POLICY: usize = 3;

/// Rows of a `begin_turn` step below which it does not start threads.
const MIN_PARALLEL_ROWS: usize = 64;

/// The net's shape, read from the weights file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub c_tile: usize,
    pub c_global: usize,
    pub c_entity: usize,
    pub c_candidate: usize,
    pub d_tile: usize,
    pub d_model: usize,
    pub n_attn_heads: usize,
    pub n_conv: usize,
    pub n_attn: usize,
    pub d_ff: usize,
    pub d_head: usize,
    pub n_kinds: usize,
    pub n_relations: usize,
    pub n_decision_heads: usize,
    pub tag_buckets: usize,
    pub tag_dim: usize,
    pub param_buckets: usize,
    pub param_dim: usize,
    pub mem_player: usize,
    pub mem_nation: usize,
    pub mem_unit: usize,
    pub id_buckets: usize,
    pub id_dim: usize,
    /// Flags (docs/ai-net-model.md, Flags): hashed lookup embeddings.
    pub engram: bool,
    pub engram_buckets: usize,
    pub engram_dim: usize,
    /// A goal token from the aim weights.
    pub aim: bool,
    pub n_aims: usize,
    /// A second region level of 16×16-tile blocks.
    pub coarse: bool,
    /// The turn trace (version 3; docs/ai-net-model.md, The turn trace):
    /// each decision's context adds `trace · sat(round_shift(Σ,
    /// trace_shift))`, `Σ` the candidate encodings of the turn's earlier
    /// choices of the heads in the `trace_heads` bit mask (a wait excepted).
    pub trace: bool,
    pub trace_heads: u32,
    pub trace_shift: u32,
    /// Version 4: the event rows' width (format 7), and whether the net
    /// reads them (docs/ai-net-model.md, Events): each token attends over
    /// the turn's event tokens and adds the result through `xattn.out`.
    pub events: bool,
    pub c_event: usize,
    /// Version 5 (docs/ai-net-model.md, Defer): the unit head scores the
    /// defer candidate (`encode::tag::DEFER`), its logit plus the file's
    /// `defer_bias`; off (every older file), the net player never offers it.
    pub defer: bool,
}

impl Config {
    /// Whether a choice of `head` joins the turn trace.
    pub fn traces(&self, head: usize) -> bool {
        self.trace && head < 32 && (self.trace_heads >> head) & 1 == 1
    }
}

/// The config fields of a version-2 file, of version 3 (`trace`,
/// `trace_heads`, `trace_shift` appended) and of version 4 (`events`,
/// `c_event` appended).
const CONFIG_FIELDS_V2: usize = 29;
const CONFIG_FIELDS_V3: usize = 32;
const CONFIG_FIELDS_V4: usize = 34;
const CONFIG_FIELDS_V5: usize = 35;

/// The largest |defer bias| a weights file may carry (Q8): the biased logit
/// stays far inside i32.
pub const MAX_DEFER_BIAS: i32 = 1 << 24;

/// How the forward pass runs. Every setting computes the same integers
/// (the sums are exact), so a game is the same with any of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Speed {
    /// The aarch64 kernels (NEON and `sdot`, `kernels.rs`) where the CPU
    /// has them; off, or elsewhere: the portable loops.
    pub simd: bool,
    /// Threads of `begin_turn` (1: the calling thread only). Its rows of
    /// tiles and tokens are split between them.
    pub threads: usize,
    /// Recompute only the tile states whose inputs changed since the
    /// cache's previous turn (and their neighbourhood, layer by layer);
    /// needs a [`TurnCache`] kept per player.
    pub incremental: bool,
}

impl Speed {
    /// The portable loops on one thread, every tile recomputed: the
    /// reference the others are tested against.
    pub const PLAIN: Speed = Speed { simd: false, threads: 1, incremental: false };
}

impl Default for Speed {
    /// The kernels, one thread, incremental tile states.
    fn default() -> Self {
        Speed { simd: true, threads: 1, incremental: true }
    }
}

/// Engram sites (the first word of a key).
const ENGRAM_TILE: u32 = 1;
const ENGRAM_ENTITY: u32 = 2;
const ENGRAM_CANDIDATE: u32 = 3;

/// An engram key's bucket: FNV-1a over the words, `(h ^ (h >> 29)) &
/// (buckets − 1)` (the same as `engram_hash` of the trainer).
fn engram_bucket(words: &[u32], buckets: usize) -> usize {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &w in words {
        h = (h ^ u64::from(w)).wrapping_mul(0x0000_0100_0000_01B3);
    }
    ((h ^ (h >> 29)) & (buckets as u64 - 1)) as usize
}

/// The hashed lookup tables (engram on): one per site, and the entity
/// columns that join the id columns in an entity's key.
#[derive(Debug, Clone, Default)]
struct Engram {
    buckets: usize,
    dim: usize,
    entity_small: Vec<usize>,
    tile: Vec<i16>,
    entity: Vec<i16>,
    candidate: Vec<i16>,
}

impl Engram {
    fn row<'a>(&self, table: &'a [i16], words: &[u32]) -> &'a [i16] {
        let b = engram_bucket(words, self.buckets);
        &table[b * self.dim..(b + 1) * self.dim]
    }
}

/// One dense layer; the weights transposed (`wt[i * out + j]`, input-major)
/// and widened to i16 so the portable sums run over contiguous outputs;
/// packed for the kernels too where the CPU has them.
#[derive(Debug, Clone)]
struct Dense {
    out: usize,
    inp: usize,
    wt: Vec<i16>,
    mult: Vec<i64>,
    shift: Vec<u32>,
    bias: Vec<i64>,
    packed: Option<Packed>,
}

impl Dense {
    /// `acc[j] += Σ_k w[j][start + k] · x[k]`: i32 sums over chunks of at
    /// most `CHUNK` inputs (they cannot overflow), added to the i64 sums.
    #[inline]
    fn acc_part(&self, acc: &mut [i64], start: usize, x: &[i16]) {
        // Fixed widths let the compiler unroll and vectorise the sums.
        match self.out {
            32 => self.acc_part_n::<32>(acc, start, x),
            64 => self.acc_part_n::<64>(acc, start, x),
            48 => self.acc_part_n::<48>(acc, start, x),
            96 => self.acc_part_n::<96>(acc, start, x),
            128 => self.acc_part_n::<128>(acc, start, x),
            192 => self.acc_part_n::<192>(acc, start, x),
            256 => self.acc_part_n::<256>(acc, start, x),
            384 => self.acc_part_n::<384>(acc, start, x),
            _ => self.acc_part_any(acc, start, x),
        }
    }

    #[inline]
    fn acc_part_n<const N: usize>(&self, acc: &mut [i64], start: usize, x: &[i16]) {
        let acc: &mut [i64; N] = acc.try_into().expect("the sums of N outputs");
        for (c, xs) in x.chunks(CHUNK).enumerate() {
            let first = start + c * CHUNK;
            let rows = &self.wt[first * N..(first + xs.len()) * N];
            let mut part = [0i32; N];
            for (&xi, row) in xs.iter().zip(rows.as_chunks::<N>().0) {
                if xi == 0 {
                    continue;
                }
                let xi = i32::from(xi);
                // An indexed loop vectorises better here than a zip.
                #[allow(clippy::needless_range_loop)]
                for j in 0..N {
                    part[j] += i32::from(row[j]) * xi;
                }
            }
            for (a, &p) in acc.iter_mut().zip(&part) {
                *a += i64::from(p);
            }
        }
    }

    /// Other widths: each product (an i32) added to the i64 sums directly,
    /// which gives the same sums without a buffer.
    fn acc_part_any(&self, acc: &mut [i64], start: usize, x: &[i16]) {
        let out = self.out;
        for (k, &xi) in x.iter().enumerate() {
            if xi == 0 {
                continue;
            }
            let xi = i32::from(xi);
            let i = start + k;
            for (a, &w) in acc.iter_mut().zip(&self.wt[i * out..(i + 1) * out]) {
                *a += i64::from(i32::from(w) * xi);
            }
        }
    }

    /// A hex convolution's sums (`acc = Σ` over the 7 parts of N inputs
    /// each), in i32 at once: 7·N ≤ CHUNK inputs cannot overflow.
    #[inline]
    fn conv_sum<'a, const N: usize>(&self, acc: &mut [i64], part: impl Fn(usize) -> &'a [i16]) {
        debug_assert!(7 * N <= CHUNK && self.out == N);
        let mut sum = [0i32; N];
        for k in 0..7 {
            let x = part(k);
            let rows = &self.wt[k * N * N..(k + 1) * N * N];
            for (&xi, row) in x.iter().zip(rows.as_chunks::<N>().0) {
                let xi = i32::from(xi);
                #[allow(clippy::needless_range_loop)]
                for j in 0..N {
                    sum[j] += i32::from(row[j]) * xi;
                }
            }
        }
        for (a, &v) in acc.iter_mut().zip(&sum) {
            *a = i64::from(v);
        }
    }

    /// One output's value from its sum: rescale, bias, ReLU if any,
    /// saturation.
    #[inline]
    fn value(&self, j: usize, acc: i64, relu: bool) -> i16 {
        let mut y = round_shift(acc * self.mult[j], self.shift[j]) + self.bias[j];
        if relu && y < 0 {
            y = 0;
        }
        sat16(y)
    }

    /// The sums brought to Q8: rescale, bias, ReLU if any, saturation.
    #[inline]
    fn finish(&self, acc: &[i64], out: &mut [i16], relu: bool) {
        for (j, (o, &a)) in out[..self.out].iter_mut().zip(acc).enumerate() {
            *o = self.value(j, a, relu);
        }
    }

    /// A residual step: `out = sat(prev + layer(…))` (ReLU in the layer).
    #[inline]
    fn finish_residual(&self, acc: &[i64], prev: &[i16], out: &mut [i16]) {
        for (j, ((o, &a), &p)) in out[..self.out].iter_mut().zip(acc).zip(prev).enumerate() {
            *o = sat16(i64::from(p) + i64::from(self.value(j, a, true)));
        }
    }

    /// The layer on `x` into `out` with the portable sums; `acc` is scratch
    /// of at least `out` values (only that prefix is touched).
    fn apply(&self, x: &[i16], out: &mut [i16], relu: bool, acc: &mut [i64]) {
        debug_assert!(x.len() == self.inp && out.len() == self.out);
        let acc = &mut acc[..self.out];
        acc.fill(0);
        self.acc_part(acc, 0, x);
        self.finish(acc, out, relu);
    }

    /// The layer on the rows of `x` (`m × inp`) into `out` (`m × out`):
    /// with `simd` on the kernels two rows at a time, else row by row.
    fn rows(&self, simd: bool, x: &[i16], out: &mut [i16], relu: bool, w: &mut Scratch) {
        let (k, o) = (self.inp, self.out);
        let m = out.len() / o;
        debug_assert!(x.len() == m * k && out.len() == m * o);
        let Some(p) = self.packed.as_ref().filter(|_| simd) else {
            for i in 0..m {
                self.apply(&x[i * k..(i + 1) * k], &mut out[i * o..(i + 1) * o], relu, &mut w.acc);
            }
            return;
        };
        let pk = p.padded();
        let Scratch { acc, hi, lo, .. } = w;
        let (acc_a, acc_b) = acc.split_at_mut(o);
        let (ha, hb) = hi.split_at_mut(pk);
        let (la, lb) = lo.split_at_mut(pk);
        let mut i = 0;
        while i + 2 <= m {
            kernels::split_padded(&x[i * k..(i + 1) * k], ha, la);
            kernels::split_padded(&x[(i + 1) * k..(i + 2) * k], hb, lb);
            kernels::sums::<2>(p, [&[(&ha[..pk], &la[..pk])], &[(&hb[..pk], &lb[..pk])]], [&mut acc_a[..o], &mut acc_b[..o]]);
            self.finish(acc_a, &mut out[i * o..(i + 1) * o], relu);
            self.finish(acc_b, &mut out[(i + 1) * o..(i + 2) * o], relu);
            i += 2;
        }
        if i < m {
            kernels::split_padded(&x[i * k..(i + 1) * k], ha, la);
            kernels::sums::<1>(p, [&[(&ha[..pk], &la[..pk])]], [&mut acc_a[..o]]);
            self.finish(acc_a, &mut out[i * o..(i + 1) * o], relu);
        }
    }
}

/// Scratch buffers of the forward pass, kept in the [`TurnCache`] (one per
/// thread of `begin_turn`, one more for `query`, `score` and `end_turn`)
/// and reused across tiles, tokens, candidates and turns: sized once, then
/// only the prefix a step needs is written (and zeroed only where it
/// accumulates).
#[derive(Debug, Clone, Default)]
struct Scratch {
    /// Two rows of sums.
    acc: Vec<i64>,
    num: Vec<i64>,
    a: Vec<i16>,
    b: Vec<i16>,
    c: Vec<i16>,
    d: Vec<i16>,
    /// Two rows of split inputs (`kernels::split`).
    hi: Vec<i8>,
    lo: Vec<i8>,
    words: Vec<u32>,
    /// A thread's rows of output (tile states).
    out: Vec<i16>,
    /// Attention: one query's scores, and the weights of up to 4 queries.
    s32: Vec<i32>,
    s64: Vec<i64>,
    e: Vec<i32>,
}

impl Scratch {
    /// Sized for rows of up to `width` values.
    fn ready(&mut self, width: usize) {
        if self.a.len() < width {
            self.acc = vec![0; 2 * width];
            self.num = vec![0; 4 * width];
            self.a = vec![0; width];
            self.b = vec![0; width];
            self.c = vec![0; width];
            self.d = vec![0; width];
            self.hi = vec![0; 2 * pad16(width)];
            self.lo = vec![0; 2 * pad16(width)];
            self.words = vec![0; width + 8];
        }
    }
}

#[derive(Debug, Clone)]
struct Block {
    qkv: Dense,
    out: Dense,
    ff1: Dense,
    ff2: Dense,
}

#[derive(Debug, Clone)]
struct Head {
    query: Dense,
    hidden: Dense,
    logit: Dense,
}

/// The event layers (version 4, events on): the event token, then a
/// cross-attention of every token over the event tokens (pre-LayerNorm on
/// both sides, the heads of the self-attention) whose output layer starts
/// at zero, so a net warm-started from one without events computes what it
/// did.
#[derive(Debug, Clone)]
struct EventLayers {
    /// [event row, region token of its tile, has a tile, actor's entity
    /// token, has an actor] → d_model.
    token: Dense,
    q: Dense,
    kv: Dense,
    out: Dense,
}

/// The rows of one feature table as layer input: the numeric columns
/// (stored × multiplier, Q8), then an embedding row per id column.
#[derive(Debug, Clone)]
struct TableInput {
    /// The numeric columns, in order, with their multipliers.
    numeric: Vec<(usize, i64)>,
    /// The id columns, in order.
    ids: Vec<usize>,
    /// `ids.len() × id_buckets × id_dim`, Q8.
    emb: Vec<i16>,
    id_buckets: usize,
    id_dim: usize,
}

impl TableInput {
    fn width(&self) -> usize {
        self.numeric.len() + self.ids.len() * self.id_dim
    }

    /// Writes a stored row as layer input into `out` (`width()` values).
    fn build(&self, stored: &[i16], out: &mut [i16]) {
        for (o, &(c, m)) in out.iter_mut().zip(&self.numeric) {
            *o = sat16(round_shift(i64::from(stored[c]) * m, INPUT_SHIFT));
        }
        let mut at = self.numeric.len();
        let b = self.id_buckets;
        for (k, &c) in self.ids.iter().enumerate() {
            let v = i64::from(stored[c]);
            let bucket = if v <= 0 { 0 } else { 1 + ((v - 1) as usize) % (b - 1) };
            let row = (k * b + bucket) * self.id_dim;
            out[at..at + self.id_dim].copy_from_slice(&self.emb[row..row + self.id_dim]);
            at += self.id_dim;
        }
    }
}

/// One GRU (PyTorch's GRUCell): input and hidden layers of 3 × width (gates
/// r, z, n).
#[derive(Debug, Clone)]
struct Gru {
    ih: Dense,
    hh: Dense,
}

/// The quantised net.
#[derive(Debug, Clone)]
pub struct Model {
    cfg: Config,
    layout_hash: u64,
    /// This model's weights (a clone shares it): the key of the tile states
    /// a [`TurnCache`] keeps.
    id: u64,
    speed: Speed,
    /// The widest layer input or output (scratch rows).
    width: usize,
    tile_in: TableInput,
    global_in: TableInput,
    entity_in: TableInput,
    candidate_in: TableInput,
    exp_lut: Vec<i64>,
    /// The largest |value| of the exp table (attention bounds).
    exp_max: i64,
    /// The exp table in i32 with a 0 after its end (the kernels' softmax
    /// weights: an index clamped to the end reads 0, as the table's end
    /// means).
    exp_lut32: Vec<i32>,
    sig_lut: Vec<i64>,
    tanh_lut: Vec<i64>,
    gate_half: i64,
    ln_eps: u64,
    score_shift: u32,
    edges: Vec<i16>,
    /// The edge rows split for the kernels (`kernels::split`), per layer.
    edge_split: Vec<(Vec<i8>, Vec<i8>)>,
    tag_emb: Vec<i16>,
    param_emb: Vec<i16>,
    /// Version 5: added to the defer candidate's logit (Q8; 0 without).
    defer_bias: i32,
    tile_embed: Dense,
    convs: Vec<Dense>,
    region: Dense,
    entity: Dense,
    global: Dense,
    slot_player: Dense,
    slot_nation: Dense,
    slot_unit: Dense,
    coarse: Option<Dense>,
    aim: Option<Dense>,
    engram: Engram,
    blocks: Vec<Block>,
    candidate: Dense,
    heads: Vec<Head>,
    value_hidden: Dense,
    value_out: Dense,
    gru_player: Gru,
    gru_nation: Gru,
    gru_unit: Gru,
    /// The turn trace's layer (`d_model × d_model`; trace on).
    trace: Option<Dense>,
    /// Version 4: the event table's input (always) and layers (events on).
    event_in: Option<TableInput>,
    events: Option<EventLayers>,
}

/// A turn's inputs as the encoder writes them (classic/docs/ai-net-data.md),
/// row-major, entity indices local to the turn.
#[derive(Debug, Clone, Copy)]
pub struct TurnInput<'a> {
    /// `tiles × c_tile`.
    pub tiles: &'a [i16],
    /// `tiles × 6`, −1 off the map.
    pub neighbors: &'a [i32],
    /// `tiles`: the region of each tile.
    pub region: &'a [i32],
    /// `c_global`.
    pub global: &'a [i16],
    /// `entities × c_entity`.
    pub entities: &'a [i16],
    /// `entities × 3`: kind, tile (−1 none), relation.
    pub entity_meta: &'a [i32],
    /// `entities`: the persistent id keying the entity's memory slot (a
    /// unit's id, a nation's player id), −1 none.
    pub entity_ids: &'a [i32],
    /// `tiles`: the 16×16 block of each tile (coarse regions on; else empty).
    pub coarse_region: &'a [i32],
    /// `n_aims` aim weights, Q8 (aim on; empty means all zero: imitation).
    pub aim: &'a [i16],
    /// Format 7: `events × c_event`, the turn's event rows (read with
    /// events on; may be empty).
    pub events: &'a [i16],
    /// `events × 4`: kind, tile (−1 none), the actor's entity (local index,
    /// −1 none), parameter.
    pub event_meta: &'a [i32],
}

/// Which memory slot an entity kind has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Unit,
    Nation,
}

fn slot_of(kind: i32) -> Option<Slot> {
    if UNIT_SLOT_KINDS.contains(&kind) {
        Some(Slot::Unit)
    } else if NATION_SLOT_KINDS.contains(&kind) {
        Some(Slot::Nation)
    } else {
        None
    }
}

/// A player's memory between its turns (classic/docs/ai-neural-architecture.md,
/// Memory): plain i16 vectors (Q8), so the playing mind can keep it in the
/// saved AI state. Slots are sorted by id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Memory {
    /// `mem_player` values.
    pub player: Vec<i16>,
    /// Ids of the nation slots, ascending.
    pub nation_ids: Vec<i32>,
    /// `nation_ids.len() × mem_nation`.
    pub nations: Vec<i16>,
    /// Ids of the unit slots, ascending.
    pub unit_ids: Vec<i32>,
    /// `unit_ids.len() × mem_unit`.
    pub units: Vec<i16>,
}

impl Memory {
    /// The memory at the start of a game: all zero, no slots.
    pub fn new(cfg: &Config) -> Self {
        Memory { player: vec![0; cfg.mem_player], ..Default::default() }
    }

    fn nation(&self, id: i32, width: usize) -> Option<&[i16]> {
        self.nation_ids.binary_search(&id).ok().map(|i| &self.nations[i * width..(i + 1) * width])
    }

    fn unit(&self, id: i32, width: usize) -> Option<&[i16]> {
        self.unit_ids.binary_search(&id).ok().map(|i| &self.units[i * width..(i + 1) * width])
    }
}

/// Puts `slot` under `id` in sorted `ids` / `values` (replacing).
fn put_slot(ids: &mut Vec<i32>, values: &mut Vec<i16>, width: usize, id: i32, slot: &[i16]) {
    match ids.binary_search(&id) {
        Ok(i) => values[i * width..(i + 1) * width].copy_from_slice(slot),
        Err(i) => {
            ids.insert(i, id);
            values.splice(i * width..i * width, slot.iter().copied());
        }
    }
}

/// The tile states of every layer of the previous turn, for the
/// incremental hex convolutions: valid for the model, the kernels' choice,
/// the tile rows and the neighbour table they were computed from.
#[derive(Debug, Clone, Default)]
struct TileStates {
    valid: bool,
    model: u64,
    simd: bool,
    /// The tile rows and neighbours of the previous turn.
    tiles: Vec<i16>,
    neighbors: Vec<i32>,
    /// Who has a tile as a neighbour (`rev[rev_at[t]..rev_at[t + 1]]`).
    rev_at: Vec<u32>,
    rev: Vec<u32>,
    /// `n_conv + 1` layers of `tiles × d_tile`: the embedding, then after
    /// each convolution.
    layers: Vec<Vec<i16>>,
    /// The convolutions' inputs split for the kernels (`tiles ×
    /// pad16(d_tile)` each; simd only).
    hi: Vec<Vec<i8>>,
    lo: Vec<Vec<i8>>,
    /// The tiles to compute at a layer, and those whose state changed.
    work: Vec<u32>,
    changed: Vec<u32>,
    mark: Vec<u32>,
    stamp: u32,
    /// Tile evaluations of the last turn: embeddings, convolutions.
    embedded: usize,
    convolved: usize,
}

/// A turn's encoding: the cached tile states and token embeddings the
/// heads read, plus scratch buffers kept between turns. Kept per player,
/// it also keeps that player's tile states of every layer for the
/// incremental convolutions ([`Speed::incremental`]).
#[derive(Debug, Clone, Default)]
pub struct TurnCache {
    n_tiles: usize,
    n_regions: usize,
    n_coarse: usize,
    n_entities: usize,
    /// Token offsets: the first entity, the global token, the player slot,
    /// the first entity slot.
    off_ent: usize,
    off_global: usize,
    off_player: usize,
    off_slots: usize,
    /// `n_tiles × d_tile`, Q8.
    tile_h: Vec<i16>,
    /// `(n_regions + n_entities + 2 + slots) × d_model`, Q8, after the final
    /// LayerNorm: regions, entities, global, player slot, entity slots.
    tokens: Vec<i16>,
    value: i32,
    /// The entity of each slot token, in entity order.
    slot_entity: Vec<usize>,
    /// Per entity, its slot token's index among the slots, `usize::MAX` none.
    slot_of_entity: Vec<usize>,
    entity_ids: Vec<i32>,
    entity_kinds: Vec<i32>,
    /// The turn trace: the sum of the candidate encodings of the turn's
    /// traced choices so far (`d_model`, Q8, exact; zero at `begin_turn`).
    trace: Vec<i64>,
    states: TileStates,
    // Scratch.
    rows: Vec<i16>,
    region_sum: Vec<i64>,
    region_sum32: Vec<i32>,
    region_count: Vec<i64>,
    region_max: Vec<i16>,
    norm: Vec<i16>,
    qkv: Vec<i16>,
    attn: Vec<i16>,
    ff: Vec<i16>,
    delta: Vec<i16>,
    /// Keys and values per head: the portable layout (`(head × n + j) ×
    /// dh`), or for the kernels the keys transposed (`head × dh × n_pad`)
    /// and the values widened (`v32`).
    kt: Vec<i16>,
    vt: Vec<i16>,
    v32: Vec<i32>,
    bounds: Vec<HeadBounds>,
    /// The event cross-attention: the event layer's input rows, the event
    /// tokens and their LayerNorm, keys and values, the queries, the output.
    ev_rows: Vec<i16>,
    ev_tok: Vec<i16>,
    ev_norm: Vec<i16>,
    ev_kv: Vec<i16>,
    ev_q: Vec<i16>,
    ev_o: Vec<i16>,
    /// One per thread of `begin_turn`.
    workers: Vec<Scratch>,
    /// A remote scorer's session of this player (`remote.rs`; 0: none yet):
    /// its memory and turn live on the server, not here.
    pub session: u64,
    /// Scratch of the forward pass (in a cell: `query`, `score` and
    /// `end_turn` take the cache shared).
    scratch: RefCell<Scratch>,
}

impl TurnCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tiles(&self) -> usize {
        self.n_tiles
    }

    pub fn regions(&self) -> usize {
        self.n_regions
    }

    pub fn entities(&self) -> usize {
        self.n_entities
    }

    /// Slot tokens (own units, nations, city-states) this turn.
    pub fn slots(&self) -> usize {
        self.slot_entity.len()
    }

    /// The tile states (`tiles × d_tile`, Q8).
    pub fn tile_states(&self) -> &[i16] {
        &self.tile_h
    }

    /// The tokens: regions, entities, global, player slot, entity slots
    /// (`× d_model`, Q8).
    pub fn tokens(&self) -> &[i16] {
        &self.tokens
    }

    /// The value head's output: the expected final points share, Q8.
    pub fn value(&self) -> i32 {
        self.value
    }

    /// Sets the value a remote scorer got for this turn (`remote.rs`).
    pub fn set_value(&mut self, value: i32) {
        self.value = value;
    }

    /// The turn trace so far (`d_model` sums, Q8; empty before the first
    /// turn): what [`Model::note_choice`] added since `begin_turn`.
    pub fn trace(&self) -> &[i64] {
        &self.trace
    }

    /// Tile evaluations of the last `begin_turn`: the embeddings and the
    /// convolutions computed (all of them: `tiles` and `n_conv × tiles`;
    /// fewer when the incremental convolutions reused the previous turn's).
    pub fn recomputed(&self) -> (usize, usize) {
        (self.states.embedded, self.states.convolved)
    }

    fn global_token(&self) -> usize {
        self.off_global
    }
}

/// Per attention head: whether the scores fit i32 lanes, and how many keys
/// the values may sum in i32 (0: not at all).
#[derive(Debug, Clone, Copy, Default)]
struct HeadBounds {
    scores_i32: bool,
    chunk: usize,
}

/// Sets a buffer's length without clearing it: for buffers every element of
/// which is written before it is read (grows zero-filled once).
fn size<T: Copy + Default>(v: &mut Vec<T>, n: usize) {
    if v.len() != n {
        v.resize(n, T::default());
    }
}

/// `value / 2^shift`, rounded half up (`shift ≥ 1`).
#[inline]
fn round_shift(value: i64, shift: u32) -> i64 {
    (value + (1i64 << (shift - 1))) >> shift
}

/// `a / b` rounded half up, `b > 0`.
#[inline]
fn div_round(a: i64, b: i64) -> i64 {
    (2 * a + b).div_euclid(2 * b)
}

/// `Σ a·b` of two i16 rows (each product fits an i32, the sum an i64).
#[inline]
fn dot_any(a: &[i16], b: &[i16]) -> i64 {
    a.iter().zip(b).map(|(&x, &y)| i64::from(i32::from(x) * i32::from(y))).sum()
}

#[inline]
fn sat16(v: i64) -> i16 {
    v.clamp(i64::from(i16::MIN), i64::from(i16::MAX)) as i16
}

/// FNV-1a 64 of `data`: the layout hash of the weights file is this of the
/// layout's canonical text (classic/docs/ai-net-model.md).
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// The row of a per-head embedding for an id (bucket 0: none).
#[inline]
fn bucket_row(head: usize, id: i32, buckets: usize) -> usize {
    let bucket = if id < 0 { 0 } else { 1 + (id as usize) % (buckets - 1) };
    head * buckets + bucket
}

/// LayerNorm without affine parameters over rows of `d` (a power of two).
fn layer_norm(x: &[i16], out: &mut [i16], d: usize, eps: u64) {
    let lg = d.trailing_zeros();
    for (row, dst) in x.chunks_exact(d).zip(out.chunks_exact_mut(d)) {
        let sum: i64 = row.iter().map(|&v| i64::from(v)).sum();
        let dd = d as i64;
        let mut var: i64 = 0;
        for &v in row {
            let c = dd * i64::from(v) - sum;
            var += c * c;
        }
        let v = ((var << (16 - lg)) as u64) + eps;
        let sigma = v.isqrt() as i64;
        for (o, &v) in dst.iter_mut().zip(row) {
            let c = dd * i64::from(v) - sum;
            *o = sat16(div_round(c << 16, sigma));
        }
    }
}

/// The softmax weights `e_j = EXP_LUT[t_j]` (0 beyond the table) of the
/// distances `t_j = max − s_j ≥ 0` into `e`; their sum. `lut` is the table
/// with a 0 after its end, where every distance beyond it reads.
#[inline]
fn softmax_weights(lut: &[i32], t: impl Iterator<Item = i64>, e: &mut [i32]) -> i64 {
    let end = lut.len() as i64 - 1;
    let mut den = 0i64;
    for (x, t) in e.iter_mut().zip(t) {
        *x = lut[t.min(end) as usize];
        den += i64::from(*x);
    }
    den
}

/// Runs `f` on every job: on this thread, or (`parallel`) one thread per
/// job, the last one here. The jobs write disjoint buffers, so the result
/// is the same either way.
fn run_jobs<T: Send>(parallel: bool, jobs: impl Iterator<Item = T>, f: impl Fn(T) + Sync) {
    if !parallel {
        jobs.for_each(f);
        return;
    }
    let f = &f;
    std::thread::scope(|s| {
        let mut jobs = jobs.peekable();
        while let Some(job) = jobs.next() {
            if jobs.peek().is_none() {
                f(job);
            } else {
                s.spawn(move || f(job));
            }
        }
    });
}

/// Rows per thread: `n` split in `threads` (a multiple of 4: the
/// attention takes queries 4 at a time); all of them below
/// [`MIN_PARALLEL_ROWS`].
fn rows_per_job(n: usize, threads: usize) -> usize {
    if threads <= 1 || n < MIN_PARALLEL_ROWS {
        n.max(1)
    } else {
        n.div_ceil(threads).next_multiple_of(4)
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len()).ok_or_else(|| format!("the weights file ends early (at byte {})", self.pos))?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64, String> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_le_bytes(a))
    }

    fn i32s(&mut self, n: usize) -> Result<Vec<i32>, String> {
        let b = self.take(n.checked_mul(4).ok_or("size overflow")?)?;
        Ok(b.as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes(*c)).collect())
    }

    fn i16s(&mut self, n: usize) -> Result<Vec<i16>, String> {
        let b = self.take(n.checked_mul(2).ok_or("size overflow")?)?;
        Ok(b.as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)).collect())
    }

    fn dense(&mut self, name: &str, out: usize, inp: usize) -> Result<Dense, String> {
        self.dense_parts(name, out, inp, inp)
    }

    /// A dense layer whose input the kernels read in parts of `part`
    /// values (the 7 tile states of a hex convolution).
    fn dense_parts(&mut self, name: &str, out: usize, inp: usize, part: usize) -> Result<Dense, String> {
        let (o, i) = (self.u32()? as usize, self.u32()? as usize);
        if (o, i) != (out, inp) {
            return Err(format!("layer {name}: {o}×{i}, expected {out}×{inp}"));
        }
        if out > MAX_OUT {
            return Err(format!("layer {name}: {out} outputs, at most {MAX_OUT}"));
        }
        let w = self.take(out * inp)?;
        let mut wt = vec![0i16; out * inp];
        for j in 0..out {
            for k in 0..inp {
                let v = i16::from(w[j * inp + k] as i8);
                if !(-127..=127).contains(&v) {
                    return Err(format!("layer {name}: a weight is −128"));
                }
                wt[k * out + j] = v;
            }
        }
        let mult = self.i32s(out)?;
        let shift = self.i32s(out)?;
        let bias = self.i32s(out)?;
        if mult.iter().any(|&m| m < 0) || shift.iter().any(|&s| !(1..=62).contains(&s)) {
            return Err(format!("layer {name}: a scale is out of range"));
        }
        let packed = (kernels::available() && out > 0 && inp > 0).then(|| Packed::new(&wt, out, inp, part));
        Ok(Dense {
            out,
            inp,
            wt,
            mult: mult.into_iter().map(i64::from).collect(),
            shift: shift.into_iter().map(|s| s as u32).collect(),
            bias: bias.into_iter().map(i64::from).collect(),
            packed,
        })
    }

    fn gru(&mut self, name: &str, input: usize, width: usize) -> Result<Gru, String> {
        Ok(Gru { ih: self.dense(&format!("gru.{name}.ih"), 3 * width, input)?, hh: self.dense(&format!("gru.{name}.hh"), 3 * width, width)? })
    }

    /// A table's id columns and their embeddings.
    fn table_input(&mut self, columns: usize, mult: Vec<i32>, id_buckets: usize, id_dim: usize) -> Result<TableInput, String> {
        let k = self.u32()? as usize;
        let mut ids = Vec::with_capacity(k);
        for _ in 0..k {
            let c = self.u32()? as usize;
            if c >= columns || ids.last().is_some_and(|&p| p >= c) {
                return Err(format!("id column {c} out of order or range"));
            }
            ids.push(c);
        }
        let emb = if k > 0 { self.i16s(k * id_buckets * id_dim)? } else { Vec::new() };
        let numeric = (0..columns).filter(|c| !ids.contains(c)).map(|c| (c, i64::from(mult[c]))).collect();
        Ok(TableInput { numeric, ids, emb, id_buckets, id_dim })
    }
}

/// The keys and values of one attention layer, as the rows read them.
struct Keys<'a> {
    qkv: &'a [i16],
    kt: &'a [i16],
    vt: &'a [i16],
    v32: &'a [i32],
    bounds: &'a [HeadBounds],
    n: usize,
    n_pad: usize,
}

impl Model {
    /// Reads a weights file (format in classic/docs/ai-net-model.md).
    pub fn from_bytes(data: &[u8]) -> Result<Model, String> {
        let mut r = Reader { data, pos: 0 };
        if r.take(4)? != MAGIC {
            return Err("not an AUNE weights file".into());
        }
        let version = r.u32()?;
        if !(MIN_FORMAT_VERSION..=FORMAT_VERSION).contains(&version) {
            return Err(format!("weights version {version}, the reader knows {MIN_FORMAT_VERSION} … {FORMAT_VERSION}"));
        }
        let fields = match version {
            5.. => CONFIG_FIELDS_V5,
            4 => CONFIG_FIELDS_V4,
            3 => CONFIG_FIELDS_V3,
            _ => CONFIG_FIELDS_V2,
        };
        let n = r.u32()? as usize;
        if n < fields + 6 {
            return Err(format!("config block of {n} values"));
        }
        let mut conf = Vec::with_capacity(n);
        for _ in 0..n {
            conf.push(r.u32()? as usize);
        }
        let cfg = Config {
            c_tile: conf[0],
            c_global: conf[1],
            c_entity: conf[2],
            c_candidate: conf[3],
            d_tile: conf[4],
            d_model: conf[5],
            n_attn_heads: conf[6],
            n_conv: conf[7],
            n_attn: conf[8],
            d_ff: conf[9],
            d_head: conf[10],
            n_kinds: conf[11],
            n_relations: conf[12],
            n_decision_heads: conf[13],
            tag_buckets: conf[14],
            tag_dim: conf[15],
            param_buckets: conf[16],
            param_dim: conf[17],
            mem_player: conf[18],
            mem_nation: conf[19],
            mem_unit: conf[20],
            id_buckets: conf[21],
            id_dim: conf[22],
            engram: conf[23] != 0,
            engram_buckets: conf[24],
            engram_dim: conf[25],
            aim: conf[26] != 0,
            n_aims: conf[27],
            coarse: conf[28] != 0,
            trace: version >= 3 && conf[29] != 0,
            trace_heads: if version >= 3 { conf[30] as u32 } else { 0 },
            trace_shift: if version >= 3 { conf[31] as u32 } else { 0 },
            events: version >= 4 && conf[32] != 0,
            c_event: if version >= 4 { conf[33] } else { 0 },
            defer: version >= 5 && conf[34] != 0,
        };
        if cfg.trace_shift > 30 {
            return Err(format!("trace shift {}", cfg.trace_shift));
        }
        if cfg.engram && (!cfg.engram_buckets.is_power_of_two() || cfg.engram_dim == 0) {
            return Err("engram buckets must be a power of two".into());
        }
        let k = fields;
        let (act_bits, input_shift, lut_len, ln_eps, gate_half, gate_bits) = (conf[k], conf[k + 1], conf[k + 2], conf[k + 3], conf[k + 4], conf[k + 5]);
        if act_bits != ACT_BITS as usize || input_shift != INPUT_SHIFT as usize || gate_bits != GATE_BITS as usize || gate_half == 0 {
            return Err("the fixed-point constants differ".into());
        }
        let d = cfg.d_model;
        if !d.is_power_of_two() || d > 1 << 16 || cfg.n_attn_heads == 0 || !d.is_multiple_of(cfg.n_attn_heads) {
            return Err(format!("d_model {d} with {} heads", cfg.n_attn_heads));
        }
        let head_dim = d / cfg.n_attn_heads;
        let root = head_dim.isqrt();
        if root * root != head_dim || !root.is_power_of_two() {
            return Err(format!("the attention head width {head_dim} is not a power of 4"));
        }
        if [cfg.c_tile, cfg.c_global, cfg.c_entity, cfg.c_candidate, cfg.c_event].iter().any(|&c| c > MAX_ROW) {
            return Err(format!("a feature row is longer than {MAX_ROW}"));
        }
        if cfg.tag_buckets < 2 || cfg.param_buckets < 2 || cfg.id_buckets < 2 || cfg.d_tile == 0 || cfg.n_decision_heads == 0 {
            return Err("degenerate config".into());
        }
        let layout_hash = r.u64()?;
        let mult_tile = r.i32s(cfg.c_tile)?;
        let mult_global = r.i32s(cfg.c_global)?;
        let mult_entity = r.i32s(cfg.c_entity)?;
        let mult_candidate = r.i32s(cfg.c_candidate)?;
        let mult_event = if version >= 4 { r.i32s(cfg.c_event)? } else { Vec::new() };
        let widen = |v: Vec<i32>| v.into_iter().map(i64::from).collect::<Vec<i64>>();
        let exp_lut = widen(r.i32s(lut_len)?);
        let sig_lut = widen(r.i32s(2 * gate_half + 1)?);
        let tanh_lut = widen(r.i32s(2 * gate_half + 1)?);
        let edges = r.i16s(cfg.n_conv * cfg.d_tile)?;
        let tag_emb = r.i16s(cfg.n_decision_heads * cfg.tag_buckets * cfg.tag_dim)?;
        let param_emb = r.i16s(cfg.n_decision_heads * cfg.param_buckets * cfg.param_dim)?;
        let defer_bias = if version >= 5 { r.i32s(1)?[0] } else { 0 };
        if defer_bias.unsigned_abs() > MAX_DEFER_BIAS as u32 {
            return Err(format!("a defer bias of {defer_bias}"));
        }
        let tile_in = r.table_input(cfg.c_tile, mult_tile, cfg.id_buckets, cfg.id_dim)?;
        let global_in = r.table_input(cfg.c_global, mult_global, cfg.id_buckets, cfg.id_dim)?;
        let entity_in = r.table_input(cfg.c_entity, mult_entity, cfg.id_buckets, cfg.id_dim)?;
        let candidate_in = r.table_input(cfg.c_candidate, mult_candidate, cfg.id_buckets, cfg.id_dim)?;
        let event_in = if version >= 4 { Some(r.table_input(cfg.c_event, mult_event, cfg.id_buckets, cfg.id_dim)?) } else { None };
        if cfg.events && (cfg.c_event == 0 || event_in.as_ref().is_some_and(|t| t.width() + 2 * d + 2 > MAX_OUT.max(MAX_ROW))) {
            return Err(format!("an event row of {} values", cfg.c_event));
        }
        let mut engram = Engram::default();
        if cfg.engram {
            let k = r.u32()? as usize;
            for _ in 0..k {
                let c = r.u32()? as usize;
                if c >= cfg.c_entity {
                    return Err(format!("engram entity column {c} out of range"));
                }
                engram.entity_small.push(c);
            }
            let n = cfg.engram_buckets * cfg.engram_dim;
            engram = Engram { buckets: cfg.engram_buckets, dim: cfg.engram_dim, tile: r.i16s(n)?, entity: r.i16s(n)?, candidate: r.i16s(n)?, ..engram };
        }
        let ed = if cfg.engram { cfg.engram_dim } else { 0 };
        if [&tile_in, &global_in, &entity_in, &candidate_in].iter().any(|t| t.width() + ed > MAX_ROW) {
            return Err(format!("a table's input is wider than {MAX_ROW}"));
        }
        let layers = r.u32()? as usize;
        let expected = 1 + cfg.n_conv + 3 + 3 + usize::from(cfg.coarse) + usize::from(cfg.aim) + 4 * cfg.n_attn + 1 + 3 * cfg.n_decision_heads + 2 + 6 + usize::from(cfg.trace) + 4 * usize::from(cfg.events);
        if layers != expected {
            return Err(format!("{layers} layers, expected {expected}"));
        }
        let tile_embed = r.dense("tile_embed", cfg.d_tile, tile_in.width() + ed)?;
        let mut convs = Vec::with_capacity(cfg.n_conv);
        for i in 0..cfg.n_conv {
            convs.push(r.dense_parts(&format!("conv{i}"), cfg.d_tile, 7 * cfg.d_tile, cfg.d_tile)?);
        }
        let region = r.dense("region", d, 2 * cfg.d_tile)?;
        let entity = r.dense("entity", d, entity_in.width() + ed + cfg.n_kinds + cfg.n_relations + cfg.d_tile + 1)?;
        let global = r.dense("global", d, global_in.width())?;
        let slot_player = r.dense("slot.player", d, cfg.mem_player)?;
        let slot_nation = r.dense("slot.nation", d, cfg.mem_nation)?;
        let slot_unit = r.dense("slot.unit", d, cfg.mem_unit)?;
        let coarse = if cfg.coarse { Some(r.dense("coarse", d, 2 * cfg.d_tile)?) } else { None };
        let aim = if cfg.aim { Some(r.dense("aim", d, cfg.n_aims)?) } else { None };
        let mut blocks = Vec::with_capacity(cfg.n_attn);
        for i in 0..cfg.n_attn {
            blocks.push(Block {
                qkv: r.dense(&format!("attn{i}.qkv"), 3 * d, d)?,
                out: r.dense(&format!("attn{i}.out"), d, d)?,
                ff1: r.dense(&format!("attn{i}.ff1"), cfg.d_ff, d)?,
                ff2: r.dense(&format!("attn{i}.ff2"), d, cfg.d_ff)?,
            });
        }
        let candidate = r.dense("candidate", d, candidate_in.width() + ed + cfg.tag_dim + cfg.param_dim + cfg.d_tile + 1 + d + 1)?;
        let mut heads = Vec::with_capacity(cfg.n_decision_heads);
        for h in 0..cfg.n_decision_heads {
            heads.push(Head {
                query: r.dense(&format!("head{h}.query"), d, d)?,
                hidden: r.dense(&format!("head{h}.hidden"), cfg.d_head, d)?,
                logit: r.dense(&format!("head{h}.logit"), 1, cfg.d_head)?,
            });
        }
        let value_hidden = r.dense("value.hidden", cfg.d_head, d)?;
        let value_out = r.dense("value.out", 1, cfg.d_head)?;
        let gru_player = r.gru("player", d, cfg.mem_player)?;
        let gru_nation = r.gru("nation", d, cfg.mem_nation)?;
        let gru_unit = r.gru("unit", d, cfg.mem_unit)?;
        let trace = if cfg.trace { Some(r.dense("trace", d, d)?) } else { None };
        let events = match (&event_in, cfg.events) {
            (Some(ein), true) => Some(EventLayers {
                token: r.dense("event", d, ein.width() + 2 * d + 2)?,
                q: r.dense("xattn.q", d, d)?,
                kv: r.dense("xattn.kv", 2 * d, d)?,
                out: r.dense("xattn.out", d, d)?,
            }),
            _ => None,
        };
        if r.take(4)? != END || r.pos != data.len() {
            return Err("bad end of the weights file".into());
        }
        let mut all: Vec<&Dense> = vec![&tile_embed, &region, &entity, &global, &slot_player, &slot_nation, &slot_unit, &candidate, &value_hidden, &value_out];
        all.extend(convs.iter().chain(coarse.as_ref()).chain(aim.as_ref()).chain(trace.as_ref()));
        all.extend(events.iter().flat_map(|e| [&e.token, &e.q, &e.kv, &e.out]));
        all.extend(blocks.iter().flat_map(|b| [&b.qkv, &b.out, &b.ff1, &b.ff2]));
        all.extend(heads.iter().flat_map(|h| [&h.query, &h.hidden, &h.logit]));
        all.extend([&gru_player, &gru_nation, &gru_unit].iter().flat_map(|g| [&g.ih, &g.hh]));
        let width = all.iter().map(|l| l.inp.max(l.out)).max().unwrap_or(0).max(MAX_ROW + 8).max(MAX_OUT);
        let s = pad16(cfg.d_tile);
        let edge_split = edges
            .chunks_exact(cfg.d_tile)
            .map(|e| {
                let (mut hi, mut lo) = (vec![0i8; s], vec![0i8; s]);
                kernels::split_padded(e, &mut hi, &mut lo);
                (hi, lo)
            })
            .collect();
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Ok(Model {
            cfg,
            layout_hash,
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            speed: Speed::default(),
            width,
            tile_in,
            global_in,
            entity_in,
            candidate_in,
            exp_max: exp_lut.iter().map(|v| v.abs()).max().unwrap_or(0),
            exp_lut32: exp_lut.iter().map(|&v| v as i32).chain([0]).collect(),
            exp_lut,
            sig_lut,
            tanh_lut,
            gate_half: gate_half as i64,
            ln_eps: ln_eps as u64,
            score_shift: ACT_BITS + root.trailing_zeros(),
            edges,
            edge_split,
            tag_emb,
            param_emb,
            defer_bias,
            tile_embed,
            convs,
            region,
            entity,
            global,
            slot_player,
            slot_nation,
            slot_unit,
            coarse,
            aim,
            engram,
            blocks,
            candidate,
            heads,
            value_hidden,
            value_out,
            gru_player,
            gru_nation,
            gru_unit,
            trace,
            event_in,
            events,
        })
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// FNV-1a 64 of the feature layout the net was trained on; the encoder
    /// compares it with its own before playing.
    pub fn layout_hash(&self) -> u64 {
        self.layout_hash
    }

    /// How the forward pass runs (the values do not depend on it).
    pub fn speed(&self) -> Speed {
        self.speed
    }

    pub fn set_speed(&mut self, speed: Speed) {
        self.speed = Speed { threads: speed.threads.max(1), ..speed };
    }

    /// The same model running with `speed`.
    pub fn with_speed(mut self, speed: Speed) -> Self {
        self.set_speed(speed);
        self
    }

    /// Whether the kernels run (asked for, and this CPU has them).
    pub fn simd(&self) -> bool {
        self.speed.simd && self.tile_embed.packed.is_some()
    }

    /// Starts a player's turn: encodes its inputs with its memory into
    /// `cache` (buffers reused): tile states, region, entity, global and slot
    /// tokens through the attention, the value.
    pub fn begin_turn(&self, input: &TurnInput<'_>, memory: &Memory, cache: &mut TurnCache) {
        let cfg = &self.cfg;
        let d = cfg.d_model;
        assert!(input.tiles.len().is_multiple_of(cfg.c_tile.max(1)), "tile rows of c_tile values");
        let n_tiles = input.tiles.len().checked_div(cfg.c_tile).unwrap_or(input.region.len());
        assert!(input.neighbors.len() == 6 * n_tiles && input.region.len() == n_tiles, "neighbours and regions per tile");
        assert!(input.global.len() == cfg.c_global, "a global row of c_global values");
        assert!(input.entities.len().is_multiple_of(cfg.c_entity.max(1)), "entity rows of c_entity values");
        let n_ent = input.entity_meta.len() / 3;
        assert!(input.entity_meta.len() == 3 * n_ent && input.entities.len() == n_ent * cfg.c_entity, "an entity meta row per entity");
        assert!(input.entity_ids.len() == n_ent, "an id per entity");
        assert!(memory.player.len() == cfg.mem_player, "a player slot of mem_player values");
        let simd = self.simd();
        let threads = self.speed.threads.max(1);
        if cache.workers.len() < threads {
            cache.workers.resize_with(threads, Scratch::default);
        }
        for w in &mut cache.workers[..threads] {
            w.ready(self.width);
        }
        let n_regions = input.region.iter().map(|&r| r + 1).max().unwrap_or(0).max(0) as usize;
        cache.n_tiles = n_tiles;
        cache.n_regions = n_regions;
        cache.n_entities = n_ent;
        cache.entity_ids.clear();
        cache.entity_ids.extend_from_slice(input.entity_ids);
        cache.entity_kinds.clear();
        cache.entity_kinds.extend((0..n_ent).map(|e| input.entity_meta[e * 3]));
        cache.trace.clear();
        cache.trace.resize(d, 0);
        cache.slot_entity.clear();
        cache.slot_of_entity.clear();
        for e in 0..n_ent {
            if slot_of(input.entity_meta[e * 3]).is_some() {
                cache.slot_of_entity.push(cache.slot_entity.len());
                cache.slot_entity.push(e);
            } else {
                cache.slot_of_entity.push(usize::MAX);
            }
        }

        self.tile_states(input, n_tiles, cache, simd, threads);

        // Regions (and coarse regions): mean and max of the tile states.
        let span = prof::span("net/model/regions");
        let n_coarse = if cfg.coarse {
            assert!(input.coarse_region.len() == n_tiles, "a coarse region per tile");
            input.coarse_region.iter().map(|&r| r + 1).max().unwrap_or(0).max(0) as usize
        } else {
            0
        };
        let n_slots = cache.slot_entity.len();
        let ent0 = n_regions + n_coarse;
        let g = ent0 + n_ent;
        let aim_at = g + 2;
        let slots0 = g + 2 + usize::from(cfg.aim);
        let n_tok = slots0 + n_slots;
        cache.n_coarse = n_coarse;
        (cache.off_ent, cache.off_global, cache.off_player, cache.off_slots) = (ent0, g, g + 1, slots0);
        // Every token is written below.
        size(&mut cache.tokens, n_tok * d);
        self.pool_regions(input, cache, n_regions, n_coarse, simd);
        drop(span);

        // Entity tokens: their rows, then the layer on them all.
        let span = prof::span("net/model/entities");
        {
            let k = self.entity.inp;
            let TurnCache { rows, tile_h, tokens, workers, .. } = cache;
            size(rows, n_ent * k);
            let w = &mut workers[0];
            for (e, row) in rows.chunks_exact_mut(k.max(1)).enumerate().take(n_ent) {
                self.entity_row(input, e, tile_h, n_tiles, row, &mut w.words);
            }
            self.entity.rows(simd, &rows[..n_ent * k], &mut tokens[ent0 * d..(ent0 + n_ent) * d], false, w);
        }
        drop(span);

        self.global_and_slots(input, memory, cache, simd, g, aim_at, slots0);

        if let Some(ev) = &self.events {
            let _span = prof::span("net/model/events");
            self.event_attention(ev, input, cache, n_tok, simd);
        }

        for (i, block) in self.blocks.iter().enumerate() {
            let _span = prof::span(ATTN_SPANS[i.min(ATTN_SPANS.len() - 1)]);
            self.attention_block(block, cache, n_tok, simd, threads);
        }
        let _span = prof::span("net/model/final");
        size(&mut cache.norm, n_tok * d);
        layer_norm(&cache.tokens, &mut cache.norm, d, self.ln_eps);
        std::mem::swap(&mut cache.tokens, &mut cache.norm);

        let w = &mut cache.workers[0];
        let gt = &cache.tokens[g * d..(g + 1) * d];
        let mut hidden = std::mem::take(&mut w.d);
        self.value_hidden.rows(simd, gt, &mut hidden[..cfg.d_head], true, w);
        let mut v = [0i16; 1];
        self.value_out.rows(simd, &hidden[..cfg.d_head], &mut v, false, w);
        w.d = hidden;
        cache.value = i32::from(v[0]);
    }

    /// The global token; the memory: the player slot's token, the aim
    /// token, then each entity slot's token (its slot projected, plus the
    /// entity's token).
    #[allow(clippy::too_many_arguments)]
    fn global_and_slots(&self, input: &TurnInput<'_>, memory: &Memory, cache: &mut TurnCache, simd: bool, g: usize, aim_at: usize, slots0: usize) {
        let _span = prof::span("net/model/global_slots");
        let cfg = &self.cfg;
        let d = cfg.d_model;
        let TurnCache { tokens, workers, slot_entity, entity_kinds, off_ent, .. } = cache;
        let w = &mut workers[0];
        let mut a = std::mem::take(&mut w.a);
        let mut tmp = std::mem::take(&mut w.b);
        let mut zeros = std::mem::take(&mut w.c);
        let gw = self.global_in.width();
        self.global_in.build(input.global, &mut a[..gw]);
        self.global.rows(simd, &a[..gw], &mut tokens[g * d..(g + 1) * d], false, w);
        self.slot_player.rows(simd, &memory.player, &mut tokens[(g + 1) * d..(g + 2) * d], false, w);
        if let Some(aim) = &self.aim {
            let x = &mut a[..cfg.n_aims];
            if input.aim.is_empty() {
                x.fill(0);
            } else {
                assert!(input.aim.len() == cfg.n_aims, "n_aims aim weights");
                x.copy_from_slice(input.aim);
            }
            aim.rows(simd, x, &mut tokens[aim_at * d..(aim_at + 1) * d], false, w);
        }
        let wz = cfg.mem_unit.max(cfg.mem_nation);
        zeros[..wz].fill(0);
        for (s, &e) in slot_entity.iter().enumerate() {
            let (layer, slot) = match slot_of(entity_kinds[e]) {
                Some(Slot::Unit) => (&self.slot_unit, if input.entity_ids[e] >= 0 { memory.unit(input.entity_ids[e], cfg.mem_unit) } else { None }.unwrap_or(&zeros[..cfg.mem_unit])),
                _ => (&self.slot_nation, if input.entity_ids[e] >= 0 { memory.nation(input.entity_ids[e], cfg.mem_nation) } else { None }.unwrap_or(&zeros[..cfg.mem_nation])),
            };
            let at = slots0 + s;
            let out = &mut tmp[..d];
            layer.rows(simd, slot, out, false, w);
            let (head, tail) = tokens.split_at_mut(at * d);
            let ent = &head[(*off_ent + e) * d..(*off_ent + e + 1) * d];
            for ((t, &o), &x) in tail[..d].iter_mut().zip(out.iter()).zip(ent) {
                *t = sat16(i64::from(o) + i64::from(x));
            }
        }
        (w.a, w.b, w.c) = (a, tmp, zeros);
    }

    /// The region and coarse region tokens: per group the mean and the max
    /// of its tiles' states, through the layer.
    fn pool_regions(&self, input: &TurnInput<'_>, cache: &mut TurnCache, n_regions: usize, n_coarse: usize, simd: bool) {
        let dt = self.cfg.d_tile;
        let d = self.cfg.d_model;
        let pools: [(&Dense, &[i32], usize, usize); 2] =
            [(&self.region, input.region, n_regions, 0), (self.coarse.as_ref().unwrap_or(&self.region), input.coarse_region, n_coarse, n_regions)];
        let TurnCache { tile_h, tokens, region_sum, region_sum32, region_count, region_max, rows, workers, .. } = cache;
        for &(layer, groups, n, first) in &pools {
            if n == 0 {
                continue;
            }
            // Sums, counts and maxima accumulate: zeroed. The sums of fewer
            // than 2^16 tiles fit i32 (whose loop vectorises), else i64.
            region_count.clear();
            region_count.resize(n, 0);
            region_max.clear();
            region_max.resize(n * dt, 0);
            region_sum.clear();
            region_sum.resize(n * dt, 0);
            let small = groups.len() < 1 << 16;
            if small {
                region_sum32.clear();
                region_sum32.resize(n * dt, 0);
            }
            for (t, &r) in groups.iter().enumerate() {
                if r < 0 {
                    continue;
                }
                let r = r as usize;
                region_count[r] += 1;
                let h = &tile_h[t * dt..(t + 1) * dt];
                for (m, &v) in region_max[r * dt..(r + 1) * dt].iter_mut().zip(h) {
                    *m = (*m).max(v);
                }
                if small {
                    for (s, &v) in region_sum32[r * dt..(r + 1) * dt].iter_mut().zip(h) {
                        *s += i32::from(v);
                    }
                } else {
                    for (s, &v) in region_sum[r * dt..(r + 1) * dt].iter_mut().zip(h) {
                        *s += i64::from(v);
                    }
                }
            }
            if small {
                for (s, &v) in region_sum.iter_mut().zip(region_sum32.iter()) {
                    *s = i64::from(v);
                }
            }
            size(rows, n * 2 * dt);
            for (r, row) in rows.chunks_exact_mut(2 * dt).enumerate() {
                let count = region_count[r];
                let (mean, max) = row.split_at_mut(dt);
                for (m, &sum) in mean.iter_mut().zip(&region_sum[r * dt..(r + 1) * dt]) {
                    *m = if count > 0 { sat16(div_round(sum, count)) } else { 0 };
                }
                max.copy_from_slice(&region_max[r * dt..(r + 1) * dt]);
            }
            layer.rows(simd, rows, &mut tokens[first * d..(first + n) * d], false, &mut workers[0]);
        }
    }

    /// The entity layer's input row of entity `e`: its table input, the
    /// engram vector, kind and relation one-hot, its tile's state and
    /// whether it has one (zeros where it has none).
    fn entity_row(&self, input: &TurnInput<'_>, e: usize, tile_h: &[i16], n_tiles: usize, row: &mut [i16], words: &mut [u32]) {
        let cfg = &self.cfg;
        let (ce, nk, nr, dt) = (cfg.c_entity, cfg.n_kinds, cfg.n_relations, cfg.d_tile);
        let ew = self.entity_in.width();
        let ed = if cfg.engram { cfg.engram_dim } else { 0 };
        let stored = &input.entities[e * ce..(e + 1) * ce];
        self.entity_in.build(stored, &mut row[..ew]);
        if cfg.engram {
            words[0] = ENGRAM_ENTITY;
            words[1] = input.entity_meta[e * 3] as u32;
            let mut k = 2;
            for &c in &self.entity_in.ids {
                words[k] = stored[c] as i32 as u32;
                k += 1;
            }
            for &c in &self.engram.entity_small {
                words[k] = stored[c].clamp(0, 15) as u32;
                k += 1;
            }
            row[ew..ew + ed].copy_from_slice(self.engram.row(&self.engram.entity, &words[..k]));
        }
        let base = ew + ed;
        let rest = &mut row[base..self.entity.inp];
        rest.fill(0);
        let (kind, tile, rel) = (input.entity_meta[e * 3], input.entity_meta[e * 3 + 1], input.entity_meta[e * 3 + 2]);
        if kind >= 0 && (kind as usize) < nk {
            rest[kind as usize] = ACT_ONE as i16;
        }
        if rel >= 0 && (rel as usize) < nr {
            rest[nk + rel as usize] = ACT_ONE as i16;
        }
        if tile >= 0 && (tile as usize) < n_tiles {
            let t = tile as usize;
            rest[nk + nr..nk + nr + dt].copy_from_slice(&tile_h[t * dt..(t + 1) * dt]);
            rest[nk + nr + dt] = ACT_ONE as i16;
        }
    }

    /// The tile embedding's input row of tile `t` into `row`; its length.
    fn tile_row(&self, input: &TurnInput<'_>, t: usize, row: &mut [i16], words: &mut [u32]) -> usize {
        let cfg = &self.cfg;
        let tw = self.tile_in.width();
        let stored = &input.tiles[t * cfg.c_tile..(t + 1) * cfg.c_tile];
        self.tile_in.build(stored, &mut row[..tw]);
        if !cfg.engram {
            return tw;
        }
        let ed = cfg.engram_dim;
        words[0] = ENGRAM_TILE;
        for (w, &c) in words[1..].iter_mut().zip(&self.tile_in.ids) {
            *w = stored[c] as i32 as u32;
        }
        row[tw..tw + ed].copy_from_slice(self.engram.row(&self.engram.tile, &words[..1 + self.tile_in.ids.len()]));
        tw + ed
    }

    /// The embedding of `tiles` into the worker's `out` (`tiles × d_tile`).
    fn embed_tiles(&self, simd: bool, input: &TurnInput<'_>, tiles: &[u32], w: &mut Scratch) {
        let dt = self.cfg.d_tile;
        let layer = &self.tile_embed;
        if w.out.len() < tiles.len() * dt {
            w.out.resize(tiles.len() * dt, 0);
        }
        let Scratch { a, b, hi, lo, acc, out, words, .. } = w;
        let Some(p) = layer.packed.as_ref().filter(|_| simd) else {
            for (&t, o) in tiles.iter().zip(out.chunks_exact_mut(dt)) {
                let n = self.tile_row(input, t as usize, a, words);
                layer.apply(&a[..n], o, true, acc);
            }
            return;
        };
        let pk = p.padded();
        let (acc_a, acc_b) = acc.split_at_mut(dt);
        let (ha, hb) = hi.split_at_mut(pk);
        let (la, lb) = lo.split_at_mut(pk);
        let (pairs, rest) = tiles.as_chunks::<2>();
        for (pair, o) in pairs.iter().zip(out.chunks_exact_mut(2 * dt)) {
            let na = self.tile_row(input, pair[0] as usize, a, words);
            let nb = self.tile_row(input, pair[1] as usize, b, words);
            kernels::split_padded(&a[..na], ha, la);
            kernels::split_padded(&b[..nb], hb, lb);
            kernels::sums::<2>(p, [&[(&ha[..pk], &la[..pk])], &[(&hb[..pk], &lb[..pk])]], [&mut acc_a[..dt], &mut acc_b[..dt]]);
            let (oa, ob) = o.split_at_mut(dt);
            layer.finish(acc_a, oa, true);
            layer.finish(acc_b, ob, true);
        }
        if let [t] = rest {
            let n = self.tile_row(input, *t as usize, a, words);
            kernels::split_padded(&a[..n], ha, la);
            kernels::sums::<1>(p, [&[(&ha[..pk], &la[..pk])]], [&mut acc_a[..dt]]);
            let at = (tiles.len() - 1) * dt;
            layer.finish(acc_a, &mut out[at..at + dt], true);
        }
    }

    /// Hex convolution `l` of `tiles` into the worker's `out` (`tiles ×
    /// d_tile`): `prev` the layer's input states, `split` the same split for
    /// the kernels (simd).
    #[allow(clippy::too_many_arguments)]
    fn conv_tiles(&self, simd: bool, l: usize, neighbors: &[i32], prev: &[i16], split: Option<(&[i8], &[i8])>, tiles: &[u32], w: &mut Scratch) {
        let dt = self.cfg.d_tile;
        let n_tiles = prev.len() / dt;
        let conv = &self.convs[l];
        if w.out.len() < tiles.len() * dt {
            w.out.resize(tiles.len() * dt, 0);
        }
        let Scratch { acc, out, .. } = w;
        match (conv.packed.as_ref().filter(|_| simd), split) {
            (Some(p), Some((hi, lo))) => {
                let s = pad16(dt);
                let (eh, el) = &self.edge_split[l];
                let parts = |t: usize| -> [(&[i8], &[i8]); 7] {
                    std::array::from_fn(|k| {
                        let src = if k == 0 { t as i32 } else { neighbors[t * 6 + k - 1] };
                        if src >= 0 && (src as usize) < n_tiles {
                            let at = src as usize * s;
                            (&hi[at..at + s], &lo[at..at + s])
                        } else {
                            (&eh[..], &el[..])
                        }
                    })
                };
                let (acc_a, acc_b) = acc.split_at_mut(dt);
                let (pairs, rest) = tiles.as_chunks::<2>();
                for (pair, o) in pairs.iter().zip(out.chunks_exact_mut(2 * dt)) {
                    let (ta, tb) = (pair[0] as usize, pair[1] as usize);
                    kernels::sums::<2>(p, [&parts(ta), &parts(tb)], [&mut acc_a[..dt], &mut acc_b[..dt]]);
                    let (oa, ob) = o.split_at_mut(dt);
                    conv.finish_residual(acc_a, &prev[ta * dt..(ta + 1) * dt], oa);
                    conv.finish_residual(acc_b, &prev[tb * dt..(tb + 1) * dt], ob);
                }
                if let [t] = rest {
                    let t = *t as usize;
                    kernels::sums::<1>(p, [&parts(t)], [&mut acc_a[..dt]]);
                    let at = (tiles.len() - 1) * dt;
                    conv.finish_residual(acc_a, &prev[t * dt..(t + 1) * dt], &mut out[at..at + dt]);
                }
            }
            _ => {
                let edge = &self.edges[l * dt..(l + 1) * dt];
                for (&t, o) in tiles.iter().zip(out.chunks_exact_mut(dt)) {
                    let t = t as usize;
                    let acc = &mut acc[..dt];
                    let part = |k: usize| -> &[i16] {
                        let src = if k == 0 { t as i32 } else { neighbors[t * 6 + k - 1] };
                        if src >= 0 && (src as usize) < n_tiles {
                            &prev[src as usize * dt..(src as usize + 1) * dt]
                        } else {
                            edge
                        }
                    };
                    match dt {
                        // The whole fan-in (7 × 32 < CHUNK) summed in i32 at once.
                        32 => conv.conv_sum::<32>(acc, part),
                        64 => conv.conv_sum::<64>(acc, part),
                        _ => {
                            acc.fill(0);
                            for k in 0..7 {
                                conv.acc_part(acc, k * dt, part(k));
                            }
                        }
                    }
                    conv.finish_residual(acc, &prev[t * dt..(t + 1) * dt], o);
                }
            }
        }
    }

    /// The tile states: the embedding and the hex convolutions (residual)
    /// into `cache.tile_h`. Incremental ([`Speed::incremental`]): only the
    /// tiles whose rows changed since the cache's previous turn are
    /// embedded, and a convolution recomputes only the tiles next to a
    /// state that changed in the layer before (a state recomputed to the
    /// same value stops the spread).
    fn tile_states(&self, input: &TurnInput<'_>, n_tiles: usize, cache: &mut TurnCache, simd: bool, threads: usize) {
        let cfg = &self.cfg;
        let (dt, ct) = (cfg.d_tile, cfg.c_tile);
        let s = pad16(dt);
        let n_conv = self.convs.len();
        let TurnCache { states: st, workers, tile_h, .. } = cache;
        let workers = &mut workers[..threads];
        let reuse = self.speed.incremental
            && st.valid
            && st.model == self.id
            && st.simd == simd
            && st.tiles.len() == input.tiles.len()
            && st.layers.len() == n_conv + 1
            && st.layers[0].len() == n_tiles * dt
            && st.neighbors == input.neighbors;
        st.valid = false;
        st.work.clear();
        if reuse {
            for t in 0..n_tiles {
                let rows = t * ct..(t + 1) * ct;
                if st.tiles[rows.clone()] != input.tiles[rows.clone()] {
                    st.tiles[rows.clone()].copy_from_slice(&input.tiles[rows]);
                    st.work.push(t as u32);
                }
            }
        } else {
            st.tiles.clear();
            st.tiles.extend_from_slice(input.tiles);
            st.neighbors.clear();
            st.neighbors.extend_from_slice(input.neighbors);
            st.layers.resize_with(n_conv + 1, Vec::new);
            for layer in &mut st.layers {
                size(layer, n_tiles * dt);
            }
            let splits = if simd { n_conv } else { 0 };
            st.hi.resize_with(splits, Vec::new);
            st.lo.resize_with(splits, Vec::new);
            for v in st.hi.iter_mut().chain(st.lo.iter_mut()) {
                size(v, n_tiles * s);
            }
            st.work.extend(0..n_tiles as u32);
            // Who has each tile as a neighbour.
            st.rev_at.clear();
            st.rev_at.resize(n_tiles + 1, 0);
            for &nb in input.neighbors {
                if nb >= 0 && (nb as usize) < n_tiles {
                    st.rev_at[nb as usize + 1] += 1;
                }
            }
            for t in 0..n_tiles {
                st.rev_at[t + 1] += st.rev_at[t];
            }
            st.rev.clear();
            st.rev.resize(st.rev_at[n_tiles] as usize, 0);
            let mut fill = st.rev_at.clone();
            for (i, &nb) in input.neighbors.iter().enumerate() {
                if nb >= 0 && (nb as usize) < n_tiles {
                    st.rev[fill[nb as usize] as usize] = (i / 6) as u32;
                    fill[nb as usize] += 1;
                }
            }
            st.mark.clear();
            st.mark.resize(n_tiles, 0);
            st.stamp = 0;
        }
        let full = !reuse;
        st.embedded = st.work.len();
        st.convolved = 0;
        for l in 0..=n_conv {
            let _span = prof::span(if l == 0 { "net/model/tile_embed" } else { CONV_SPANS[(l - 1).min(CONV_SPANS.len() - 1)] });
            if l > 0 && !full {
                // The tiles next to a changed state (or changed themselves).
                st.stamp = st.stamp.wrapping_add(1);
                if st.stamp == 0 {
                    st.mark.fill(0);
                    st.stamp = 1;
                }
                st.work.clear();
                for &c in &st.changed {
                    let c = c as usize;
                    let around = &st.rev[st.rev_at[c] as usize..st.rev_at[c + 1] as usize];
                    for &t in std::iter::once(&(c as u32)).chain(around) {
                        if st.mark[t as usize] != st.stamp {
                            st.mark[t as usize] = st.stamp;
                            st.work.push(t);
                        }
                    }
                }
                st.work.sort_unstable();
            }
            if l > 0 {
                st.convolved += st.work.len();
            }
            let per = rows_per_job(st.work.len(), threads);
            {
                let jobs = st.work.chunks(per).zip(workers.iter_mut());
                let parallel = st.work.len() > per;
                if l == 0 {
                    run_jobs(parallel, jobs, |(tiles, w)| self.embed_tiles(simd, input, tiles, w));
                } else {
                    let prev = &st.layers[l - 1];
                    let split = (simd).then(|| (&st.hi[l - 1][..], &st.lo[l - 1][..]));
                    run_jobs(parallel, jobs, |(tiles, w)| self.conv_tiles(simd, l - 1, input.neighbors, prev, split, tiles, w));
                }
            }
            // The new states into the layer; those that changed go on.
            st.changed.clear();
            let layer = &mut st.layers[l];
            for (tiles, w) in st.work.chunks(per).zip(workers.iter()) {
                for (&t, new) in tiles.iter().zip(w.out.chunks_exact(dt)) {
                    let dst = &mut layer[t as usize * dt..(t as usize + 1) * dt];
                    if full || dst != new {
                        dst.copy_from_slice(new);
                        st.changed.push(t);
                    }
                }
            }
            if simd && l < n_conv {
                let (hi, lo) = (&mut st.hi[l], &mut st.lo[l]);
                for &t in &st.changed {
                    let t = t as usize;
                    kernels::split_padded(&layer[t * dt..(t + 1) * dt], &mut hi[t * s..(t + 1) * s], &mut lo[t * s..(t + 1) * s]);
                }
            }
        }
        size(tile_h, n_tiles * dt);
        tile_h.copy_from_slice(&st.layers[n_conv]);
        st.valid = true;
        st.model = self.id;
        st.simd = simd;
    }

    /// The event cross-attention (events on; docs/ai-net-model.md, Events),
    /// before the first attention block: each event's token is the event
    /// layer on [its row, the region token of its tile, has a tile, its
    /// actor's entity token, has an actor] (the tokens before any
    /// attention); then every token `x` attends over the event tokens —
    /// `q = xattn.q(LN(x))`, `k, v = xattn.kv(LN(event token))`, the heads,
    /// scores and softmax of the self-attention — and `x += xattn.out(o)`.
    /// No event: nothing changes.
    fn event_attention(&self, ev: &EventLayers, input: &TurnInput<'_>, cache: &mut TurnCache, n: usize, simd: bool) {
        let cfg = &self.cfg;
        let d = cfg.d_model;
        let ce = cfg.c_event;
        let n_ev = input.events.len().checked_div(ce).unwrap_or(0);
        assert!(input.events.len() == n_ev * ce && input.event_meta.len() == 4 * n_ev, "an event meta row of 4 per event row of c_event");
        if n_ev == 0 {
            return;
        }
        let event_in = self.event_in.as_ref().expect("a version-4 file has the event table");
        let (n_tiles, n_ent, n_regions, ent0) = (cache.n_tiles, cache.n_entities, cache.n_regions, cache.off_ent);
        let TurnCache { tokens, ev_rows, ev_tok, ev_norm, ev_kv, ev_q, ev_o, norm, delta, workers, .. } = cache;
        let k = ev.token.inp;
        let ew = event_in.width();
        size(ev_rows, n_ev * k);
        for (e, row) in ev_rows.chunks_exact_mut(k).enumerate() {
            event_in.build(&input.events[e * ce..(e + 1) * ce], &mut row[..ew]);
            let meta = &input.event_meta[e * 4..(e + 1) * 4];
            let region = usize::try_from(meta[1]).ok().filter(|&t| t < n_tiles).and_then(|t| usize::try_from(input.region[t]).ok()).filter(|&r| r < n_regions);
            let at = ew;
            match region {
                Some(r) => {
                    row[at..at + d].copy_from_slice(&tokens[r * d..(r + 1) * d]);
                    row[at + d] = ACT_ONE as i16;
                }
                None => row[at..at + d + 1].fill(0),
            }
            let at = ew + d + 1;
            match usize::try_from(meta[2]).ok().filter(|&a| a < n_ent) {
                Some(a) => {
                    row[at..at + d].copy_from_slice(&tokens[(ent0 + a) * d..(ent0 + a + 1) * d]);
                    row[at + d] = ACT_ONE as i16;
                }
                None => row[at..at + d + 1].fill(0),
            }
        }
        let w = &mut workers[0];
        size(ev_tok, n_ev * d);
        ev.token.rows(simd, ev_rows, ev_tok, false, w);
        size(ev_norm, n_ev * d);
        layer_norm(ev_tok, ev_norm, d, self.ln_eps);
        size(ev_kv, n_ev * 2 * d);
        ev.kv.rows(simd, ev_norm, ev_kv, false, w);
        size(norm, n * d);
        layer_norm(&tokens[..n * d], norm, d, self.ln_eps);
        size(ev_q, n * d);
        ev.q.rows(simd, &norm[..n * d], ev_q, false, w);
        // Every token over the events, head by head (exact integer sums).
        let heads = cfg.n_attn_heads;
        let dh = d / heads;
        let lut = &self.exp_lut;
        let n_lut = lut.len() as i64;
        size(ev_o, n * d);
        size(&mut w.s64, n_ev);
        let Scratch { s64: scores, num, .. } = &mut *w;
        for i in 0..n {
            for h in 0..heads {
                let q = &ev_q[i * d + h * dh..i * d + (h + 1) * dh];
                let mut max = i64::MIN;
                for (j, s) in scores.iter_mut().enumerate().take(n_ev) {
                    let key = &ev_kv[j * 2 * d + h * dh..j * 2 * d + (h + 1) * dh];
                    *s = round_shift(dot_any(q, key), self.score_shift);
                    max = max.max(*s);
                }
                let num = &mut num[..dh];
                num.fill(0);
                let mut den: i64 = 0;
                for (j, &s) in scores.iter().enumerate().take(n_ev) {
                    let t = max - s;
                    if t >= n_lut {
                        continue;
                    }
                    let e = lut[t as usize];
                    den += e;
                    let value = &ev_kv[j * 2 * d + d + h * dh..j * 2 * d + d + (h + 1) * dh];
                    for (a, &x) in num.iter_mut().zip(value) {
                        *a += e * i64::from(x);
                    }
                }
                for (dst, &a) in ev_o[i * d + h * dh..i * d + (h + 1) * dh].iter_mut().zip(num.iter()) {
                    *dst = sat16(div_round(a, den));
                }
            }
        }
        size(delta, n * d);
        ev.out.rows(simd, ev_o, &mut delta[..n * d], false, w);
        for (x, &dx) in tokens[..n * d].iter_mut().zip(delta.iter()) {
            *x = sat16(i64::from(*x) + i64::from(dx));
        }
    }

    fn attention_block(&self, block: &Block, cache: &mut TurnCache, n: usize, simd: bool, threads: usize) {
        let d = self.cfg.d_model;
        let heads = self.cfg.n_attn_heads;
        let dh = d / heads;
        let dff = self.cfg.d_ff;
        let TurnCache { tokens, norm, qkv, attn, ff, delta, kt, vt, v32, bounds, workers, .. } = cache;
        let workers = &mut workers[..threads];
        // Every buffer below is fully written before it is read.
        size(norm, n * d);
        size(qkv, n * 3 * d);
        size(attn, n * d);
        size(ff, n * dff);
        size(delta, n * d);
        let per = rows_per_job(n, threads);
        let parallel = n > per;
        {
            let _span = prof::span("net/model/attn/qkv");
            let jobs = tokens.chunks(per * d).zip(norm.chunks_mut(per * d)).zip(qkv.chunks_mut(per * 3 * d)).zip(workers.iter_mut());
            run_jobs(parallel, jobs, |(((x, y), q), w)| {
                layer_norm(x, y, d, self.ln_eps);
                block.qkv.rows(simd, y, q, false, w);
            });
        }
        // Keys and values head by head.
        let n_pad = n.next_multiple_of(8);
        bounds.clear();
        if simd {
            size(kt, heads * dh * n_pad);
            size(v32, heads * n * dh);
            for h in 0..heads {
                let (mut mq, mut mk, mut mv) = (0i64, 0i64, 0i64);
                let keys = &mut kt[h * dh * n_pad..(h + 1) * dh * n_pad];
                for j in 0..n {
                    let src = j * 3 * d + h * dh;
                    for c in 0..dh {
                        let (q, k, v) = (qkv[src + c], qkv[src + d + c], qkv[src + 2 * d + c]);
                        mq = mq.max(i64::from(q).abs());
                        mk = mk.max(i64::from(k).abs());
                        mv = mv.max(i64::from(v).abs());
                        keys[c * n_pad + j] = k;
                        v32[(h * n + j) * dh + c] = i32::from(v);
                    }
                }
                for c in 0..dh {
                    keys[c * n_pad + n..(c + 1) * n_pad].fill(0);
                }
                let max = i64::from(i32::MAX);
                let ev = self.exp_max * mv.max(1);
                bounds.push(HeadBounds { scores_i32: mq * mk * dh as i64 <= max, chunk: if ev <= max { (max / ev) as usize } else { 0 } });
            }
        } else {
            // Each token's slice of a head contiguous.
            size(kt, n * d);
            size(vt, n * d);
            for j in 0..n {
                for h in 0..heads {
                    let src = j * 3 * d + h * dh;
                    let dst = (h * n + j) * dh;
                    kt[dst..dst + dh].copy_from_slice(&qkv[src + d..src + d + dh]);
                    vt[dst..dst + dh].copy_from_slice(&qkv[src + 2 * d..src + 2 * d + dh]);
                }
            }
        }
        let keys = Keys { qkv, kt, vt, v32, bounds, n, n_pad };
        let _span = prof::span("net/model/attn/rows");
        let jobs = tokens
            .chunks_mut(per * d)
            .zip(attn.chunks_mut(per * d))
            .zip(norm.chunks_mut(per * d))
            .zip(ff.chunks_mut(per * dff))
            .zip(delta.chunks_mut(per * d))
            .zip(workers.iter_mut())
            .enumerate();
        run_jobs(parallel, jobs, |(i, (((((x, a), y), f), dx), w))| {
            let first = i * per;
            let rows = x.len() / d;
            let span = prof::span("net/model/attn/heads");
            if simd {
                self.attend_simd(&keys, first, rows, a, w);
            } else {
                self.attend_plain(&keys, first, rows, a, w);
            }
            drop(span);
            let _span = prof::span("net/model/attn/out_ff");
            block.out.rows(simd, a, dx, false, w);
            for (x, &dx) in x.iter_mut().zip(dx.iter()) {
                *x = sat16(i64::from(*x) + i64::from(dx));
            }
            layer_norm(x, y, d, self.ln_eps);
            block.ff1.rows(simd, y, f, true, w);
            block.ff2.rows(simd, f, dx, false, w);
            for (x, &dx) in x.iter_mut().zip(dx.iter()) {
                *x = sat16(i64::from(*x) + i64::from(dx));
            }
        });
    }

    /// The attention of queries `first..first + rows` over every key, into
    /// `out` (`rows × d_model`): the portable loops.
    fn attend_plain(&self, keys: &Keys<'_>, first: usize, rows: usize, out: &mut [i16], w: &mut Scratch) {
        let d = self.cfg.d_model;
        let heads = self.cfg.n_attn_heads;
        let dh = d / heads;
        let n = keys.n;
        let lut = &self.exp_lut;
        let n_lut = lut.len() as i64;
        size(&mut w.s64, n);
        let Scratch { s64: scores, num, .. } = w;
        for r in 0..rows {
            let i = first + r;
            for h in 0..heads {
                let q = &keys.qkv[i * 3 * d + h * dh..i * 3 * d + (h + 1) * dh];
                let kh = &keys.kt[h * n * dh..(h + 1) * n * dh];
                let values = &keys.vt[h * n * dh..(h + 1) * n * dh];
                let mut max = i64::MIN;
                for (s, k) in scores.iter_mut().zip(kh.chunks_exact(dh)) {
                    let v = round_shift(dot_any(q, k), self.score_shift);
                    *s = v;
                    if v > max {
                        max = v;
                    }
                }
                let num = &mut num[..dh];
                num.fill(0);
                let mut den: i64 = 0;
                for (&s, v) in scores.iter().zip(values.chunks_exact(dh)) {
                    let t = max - s;
                    if t >= n_lut {
                        continue;
                    }
                    let e = lut[t as usize];
                    den += e;
                    for (a, &x) in num.iter_mut().zip(v) {
                        *a += e * i64::from(x);
                    }
                }
                let o = &mut out[r * d + h * dh..r * d + (h + 1) * dh];
                for (dst, &a) in o.iter_mut().zip(num.iter()) {
                    *dst = sat16(div_round(a, den));
                }
            }
        }
    }

    /// The same with the kernels: per head, up to 4 queries at a time, the
    /// scores in i32 lanes when the head's bound allows (else i64), the
    /// values in i32 lanes flushed to i64 (`kernels::attention_values`).
    fn attend_simd(&self, keys: &Keys<'_>, first: usize, rows: usize, out: &mut [i16], w: &mut Scratch) {
        let d = self.cfg.d_model;
        let heads = self.cfg.n_attn_heads;
        let dh = d / heads;
        let (n, n_pad) = (keys.n, keys.n_pad);
        let lut = &self.exp_lut32;
        size(&mut w.s32, n_pad);
        size(&mut w.s64, n);
        size(&mut w.e, 4 * n);
        let Scratch { s32, s64, e, num, .. } = w;
        for h in 0..heads {
            let kh = &keys.kt[h * dh * n_pad..(h + 1) * dh * n_pad];
            let vh = &keys.v32[h * n * dh..(h + 1) * n * dh];
            let bound = keys.bounds[h];
            let mut r0 = 0;
            while r0 < rows {
                let qb = (rows - r0).min(4);
                let mut den = [0i64; 4];
                for (qi, den) in den.iter_mut().enumerate().take(qb) {
                    let i = first + r0 + qi;
                    let q = &keys.qkv[i * 3 * d + h * dh..i * 3 * d + (h + 1) * dh];
                    let ei = &mut e[qi * n..(qi + 1) * n];
                    *den = if bound.scores_i32 {
                        kernels::scores_i32(q, kh, n_pad, self.score_shift, s32);
                        let max = s32[..n].iter().copied().max().unwrap_or(i32::MIN);
                        softmax_weights(lut, s32[..n].iter().map(|&s| i64::from(max) - i64::from(s)), ei)
                    } else {
                        for (j, s) in s64.iter_mut().enumerate() {
                            let dot: i64 = q.iter().enumerate().map(|(c, &x)| i64::from(i32::from(x) * i32::from(kh[c * n_pad + j]))).sum();
                            *s = round_shift(dot, self.score_shift);
                        }
                        let max = s64.iter().copied().max().unwrap_or(i64::MIN);
                        softmax_weights(lut, s64.iter().map(|&s| max - s), ei)
                    };
                }
                let num = &mut num[..qb * dh];
                num.fill(0);
                if bound.chunk > 0 {
                    kernels::attention_values(dh, &e[..qb * n], qb, vh, n, bound.chunk, num);
                } else {
                    for qi in 0..qb {
                        for j in 0..n {
                            let x = i64::from(e[qi * n + j]);
                            if x != 0 {
                                for (a, &v) in num[qi * dh..(qi + 1) * dh].iter_mut().zip(&vh[j * dh..(j + 1) * dh]) {
                                    *a += x * i64::from(v);
                                }
                            }
                        }
                    }
                }
                for qi in 0..qb {
                    let r = r0 + qi;
                    let o = &mut out[r * d + h * dh..r * d + (h + 1) * dh];
                    for (dst, &a) in o.iter_mut().zip(&num[qi * dh..(qi + 1) * dh]) {
                        *dst = sat16(div_round(a, den[qi]));
                    }
                }
                r0 += qb;
            }
        }
    }

    /// One integer GRU step: `hidden` (Q8) from `x` (Q8) into `out`.
    #[allow(clippy::too_many_arguments)]
    fn gru_step(&self, gru: &Gru, x: &[i16], hidden: &[i16], out: &mut [i16], w: &mut Scratch, gi: &mut [i16], gh: &mut [i16]) {
        let m = hidden.len();
        let (gi, gh) = (&mut gi[..3 * m], &mut gh[..3 * m]);
        let simd = self.simd();
        gru.ih.rows(simd, x, gi, false, w);
        gru.hh.rows(simd, hidden, gh, false, w);
        let half = self.gate_half;
        let idx = |v: i64| (v.clamp(-half, half) + half) as usize;
        let one = 1i64 << GATE_BITS;
        for j in 0..m {
            let r = self.sig_lut[idx(i64::from(gi[j]) + i64::from(gh[j]))];
            let z = self.sig_lut[idx(i64::from(gi[m + j]) + i64::from(gh[m + j]))];
            let rn = round_shift(r * i64::from(gh[2 * m + j]), GATE_BITS);
            let n = self.tanh_lut[idx(i64::from(gi[2 * m + j]) + rn)];
            let h = i64::from(hidden[j]) << (GATE_BITS - ACT_BITS);
            out[j] = sat16(round_shift((one - z) * n + z * h, 2 * GATE_BITS - ACT_BITS));
        }
    }

    /// Ends a player's turn: the memory after it. The player slot and the
    /// slots of this turn's nations and units are updated by the GRUs from
    /// their tokens; other nations' slots are kept, slots of units not in
    /// this turn are dropped; a repeated id keeps the last in entity order.
    pub fn end_turn(&self, cache: &TurnCache, memory: &Memory) -> Memory {
        let _span = prof::span("net/model/memory_gru");
        let cfg = &self.cfg;
        let d = cfg.d_model;
        let mut new = Memory {
            player: vec![0; cfg.mem_player],
            nation_ids: memory.nation_ids.clone(),
            nations: memory.nations.clone(),
            unit_ids: Vec::new(),
            units: Vec::new(),
        };
        let (p, slots0) = (cache.off_player, cache.off_slots);
        let mut sc = cache.scratch.borrow_mut();
        sc.ready(self.width);
        let sc = &mut *sc;
        let (mut gi, mut gh, mut zeros, mut out) = (std::mem::take(&mut sc.a), std::mem::take(&mut sc.b), std::mem::take(&mut sc.c), std::mem::take(&mut sc.d));
        self.gru_step(&self.gru_player, &cache.tokens[p * d..(p + 1) * d], &memory.player, &mut new.player, sc, &mut gi, &mut gh);
        zeros[..cfg.mem_unit.max(cfg.mem_nation)].fill(0);
        for (s, &e) in cache.slot_entity.iter().enumerate() {
            let id = cache.entity_ids[e];
            if id < 0 {
                continue;
            }
            let x = &cache.tokens[(slots0 + s) * d..(slots0 + s + 1) * d];
            match slot_of(cache.entity_kinds[e]) {
                Some(Slot::Unit) => {
                    let w = cfg.mem_unit;
                    let old = memory.unit(id, w).unwrap_or(&zeros[..w]);
                    self.gru_step(&self.gru_unit, x, old, &mut out[..w], sc, &mut gi, &mut gh);
                    put_slot(&mut new.unit_ids, &mut new.units, w, id, &out[..w]);
                }
                _ => {
                    let w = cfg.mem_nation;
                    let old = memory.nation(id, w).unwrap_or(&zeros[..w]);
                    self.gru_step(&self.gru_nation, x, old, &mut out[..w], sc, &mut gi, &mut gh);
                    put_slot(&mut new.nation_ids, &mut new.nations, w, id, &out[..w]);
                }
            }
        }
        (sc.a, sc.b, sc.c, sc.d) = (gi, gh, zeros, out);
        new
    }

    /// The head's query of a decision's context (`d_model` values into
    /// `out`): for unit commands and production the actor's token plus its
    /// slot's token (`actor` a local entity index); otherwise, or without an
    /// actor, the global token plus the player slot's token; with the turn
    /// trace on, plus the trace layer on the turn's choices so far.
    pub fn query(&self, cache: &TurnCache, head: usize, actor: i32, out: &mut [i16]) {
        let d = self.cfg.d_model;
        assert!(head < self.heads.len() && out.len() == d, "a known head and a d_model buffer");
        let g = cache.global_token();
        let uses_actor = (head == HEAD_UNIT || head == HEAD_PRODUCTION) && actor >= 0 && (actor as usize) < cache.n_entities;
        let (a, b) = if uses_actor {
            let a = cache.off_ent + actor as usize;
            let s = cache.slot_of_entity[actor as usize];
            (a, if s == usize::MAX { None } else { Some(cache.off_slots + s) })
        } else {
            (g, Some(cache.off_player))
        };
        let mut sc = cache.scratch.borrow_mut();
        sc.ready(self.width);
        let sc = &mut *sc;
        let mut ctx = std::mem::take(&mut sc.a);
        let c = &mut ctx[..d];
        c.copy_from_slice(&cache.tokens[a * d..(a + 1) * d]);
        if let Some(b) = b {
            for (c, &x) in c.iter_mut().zip(&cache.tokens[b * d..(b + 1) * d]) {
                *c = sat16(i64::from(*c) + i64::from(x));
            }
        }
        if let Some(trace) = &self.trace {
            // t = trace · sat(round_shift(Σ, shift)); the context adds it.
            assert!(cache.trace.len() == d, "the turn trace starts at begin_turn");
            let (mut sums, mut t) = (std::mem::take(&mut sc.b), std::mem::take(&mut sc.c));
            let shift = self.cfg.trace_shift;
            for (o, &v) in sums[..d].iter_mut().zip(&cache.trace) {
                *o = sat16(if shift > 0 { round_shift(v, shift) } else { v });
            }
            trace.rows(self.simd(), &sums[..d], &mut t[..d], false, sc);
            for (c, &x) in c.iter_mut().zip(&t[..d]) {
                *c = sat16(i64::from(*c) + i64::from(x));
            }
            (sc.b, sc.c) = (sums, t);
        }
        self.heads[head].query.rows(self.simd(), c, out, false, sc);
        sc.a = ctx;
    }

    /// A decision of the turn was made: with the turn trace on, the chosen
    /// candidate's encoding (the candidate layer on its row, as `score`
    /// computes it) joins the trace when `head` is traced and the choice is
    /// no wait. `row`, `meta` as for [`Model::score`]. Call it once per
    /// decision, after scoring it, with the candidate taken (none taken:
    /// no call).
    pub fn note_choice(&self, cache: &mut TurnCache, head: usize, actor: i32, row: &[i16], meta: &[i32]) {
        if !self.cfg.traces(head) || matches!(meta.first(), Some(&super::encode::tag::WAIT | &super::encode::tag::DEFER)) {
            return;
        }
        let d = self.cfg.d_model;
        assert!(cache.trace.len() == d, "the turn trace starts at begin_turn");
        let mut u = [0i16; MAX_OUT];
        {
            let mut sc = cache.scratch.borrow_mut();
            sc.ready(self.width);
            let sc = &mut *sc;
            let mut x = std::mem::take(&mut sc.a);
            self.candidate_encoding(cache, head, actor, row, meta, &mut x, &mut u[..d], sc);
            sc.a = x;
        }
        for (t, &v) in cache.trace.iter_mut().zip(&u[..d]) {
            *t += i64::from(v);
        }
    }

    /// One candidate's logit (Q8): `row` its `c_candidate` features, `meta`
    /// its tag, target tile (−1), target entity (local index, −1) and
    /// parameter id (−1); `query` from [`Model::query`] for this decision
    /// (`actor` as given to it).
    pub fn score(&self, cache: &TurnCache, head: usize, actor: i32, query: &[i16], row: &[i16], meta: &[i32]) -> i32 {
        let cfg = &self.cfg;
        let d = cfg.d_model;
        assert!(query.len() == d, "a query of d_model values");
        let bias = self.defer_offset(head, meta);
        let mut sc = cache.scratch.borrow_mut();
        sc.ready(self.width);
        let sc = &mut *sc;
        let simd = self.simd();
        let mut x = std::mem::take(&mut sc.a);
        let mut enc = std::mem::take(&mut sc.b);
        let mut z = std::mem::take(&mut sc.c);
        let enc_d = &mut enc[..d];
        self.candidate_encoding(cache, head, actor, row, meta, &mut x, enc_d, sc);
        for (h, &q) in enc_d.iter_mut().zip(query) {
            *h = sat16(i64::from(*h) + i64::from(q)).max(0);
        }
        let head = &self.heads[head];
        let zh = &mut z[..cfg.d_head];
        head.hidden.rows(simd, enc_d, zh, true, sc);
        let mut logit = [0i16; 1];
        head.logit.rows(simd, zh, &mut logit, false, sc);
        (sc.a, sc.b, sc.c) = (x, enc, z);
        i32::from(logit[0]) + bias
    }

    /// What a candidate's logit adds (Q8): the defer bias on the defer
    /// candidate of the unit head (defer on), else 0.
    fn defer_offset(&self, head: usize, meta: &[i32]) -> i32 {
        if self.cfg.defer && head == HEAD_UNIT && meta.first() == Some(&super::encode::tag::DEFER) {
            self.defer_bias
        } else {
            0
        }
    }

    /// The defer bias (Q8; 0 without defer).
    pub fn defer_bias(&self) -> i32 {
        self.defer_bias
    }

    /// The same net with the defer choice on (`bias`, Q8, within
    /// ±[`MAX_DEFER_BIAS`]) or off (None): an older file's net made to offer
    /// it (tests, the bench's A/B), or a file's defer turned off.
    pub fn with_defer(mut self, bias: Option<i32>) -> Self {
        self.cfg.defer = bias.is_some();
        self.defer_bias = bias.unwrap_or(0).clamp(-MAX_DEFER_BIAS, MAX_DEFER_BIAS);
        self
    }

    /// The candidate layer on one candidate (`d_model` values into `enc`):
    /// its input row built in `x` (zeros where the candidate has no target
    /// tile or entity).
    #[allow(clippy::too_many_arguments)]
    fn candidate_encoding(&self, cache: &TurnCache, head: usize, actor: i32, row: &[i16], meta: &[i32], x: &mut [i16], enc: &mut [i16], sc: &mut Scratch) {
        let cfg = &self.cfg;
        let (d, dt) = (cfg.d_model, cfg.d_tile);
        assert!(row.len() == cfg.c_candidate && meta.len() == 4 && enc.len() == d && head < self.heads.len(), "a candidate row and its meta");
        let cw = self.candidate_in.width();
        self.candidate_in.build(row, &mut x[..cw]);
        let mut at = cw;
        if cfg.engram {
            let n_ent = cache.n_entities;
            let kind = |e: i32| if e >= 0 && (e as usize) < n_ent { cache.entity_kinds[e as usize] + 1 } else { 0 };
            let target = if meta[2] >= 0 && (meta[2] as usize) < n_ent {
                kind(meta[2])
            } else if meta[1] >= 0 && (meta[1] as usize) < cache.n_tiles {
                8
            } else {
                0
            };
            let words = [ENGRAM_CANDIDATE, head as u32, meta[0] as u32, target as u32, kind(actor) as u32, meta[3] as u32];
            x[cw..cw + cfg.engram_dim].copy_from_slice(self.engram.row(&self.engram.candidate, &words));
            at += cfg.engram_dim;
        }
        let tag = bucket_row(head, meta[0], cfg.tag_buckets);
        x[at..at + cfg.tag_dim].copy_from_slice(&self.tag_emb[tag * cfg.tag_dim..(tag + 1) * cfg.tag_dim]);
        at += cfg.tag_dim;
        let param = bucket_row(head, meta[3], cfg.param_buckets);
        x[at..at + cfg.param_dim].copy_from_slice(&self.param_emb[param * cfg.param_dim..(param + 1) * cfg.param_dim]);
        at += cfg.param_dim;
        let tile = meta[1];
        if tile >= 0 && (tile as usize) < cache.n_tiles {
            let t = tile as usize;
            x[at..at + dt].copy_from_slice(&cache.tile_h[t * dt..(t + 1) * dt]);
            x[at + dt] = ACT_ONE as i16;
        } else {
            x[at..at + dt + 1].fill(0);
        }
        at += dt + 1;
        let ent = meta[2];
        if ent >= 0 && (ent as usize) < cache.n_entities {
            let e = cache.off_ent + ent as usize;
            x[at..at + d].copy_from_slice(&cache.tokens[e * d..(e + 1) * d]);
            x[at + d] = ACT_ONE as i16;
        } else {
            x[at..at + d + 1].fill(0);
        }
        at += d + 1;
        debug_assert_eq!(at, self.candidate.inp);
        self.candidate.rows(self.simd(), &x[..at], enc, false, sc);
    }

    /// The logits of a decision's candidates (`rows`: `k × c_candidate`,
    /// `metas`: `k × 4`) into `out` (`k`).
    pub fn score_all(&self, cache: &TurnCache, head: usize, actor: i32, rows: &[i16], metas: &[i32], out: &mut [i32]) {
        let cfg = &self.cfg;
        let k = out.len();
        assert!(rows.len() == k * cfg.c_candidate && metas.len() == k * 4, "k candidate rows and metas");
        let mut q = [0i16; MAX_OUT];
        let q = &mut q[..cfg.d_model];
        self.query(cache, head, actor, q);
        for (i, o) in out.iter_mut().enumerate() {
            *o = self.score(cache, head, actor, q, &rows[i * cfg.c_candidate..(i + 1) * cfg.c_candidate], &metas[i * 4..(i + 1) * 4]);
        }
    }
}

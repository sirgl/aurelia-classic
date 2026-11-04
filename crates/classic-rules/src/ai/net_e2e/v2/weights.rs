//! The v2 weights file (`nn/aurelia_nn/export_v2.py` `weights`): named f32
//! tensors, the input buffers and the config as named integers. Loaded once;
//! the layers are kept transposed (`[in × out]`, see [`Lin`]) for the
//! kernels of `gemm.rs` and the GPU.

use std::collections::HashMap;
use std::path::Path;

const MAGIC: &[u8; 4] = b"AV2W";
const VERSION: u32 = 1;

/// A tensor of the file.
#[derive(Debug, Clone)]
pub enum Tensor {
    F32(Vec<usize>, Vec<f32>),
    I64(Vec<usize>, Vec<i64>),
    U8(Vec<usize>, Vec<u8>),
}

/// The file's tensors by name.
#[derive(Debug, Default)]
pub struct File {
    pub tensors: HashMap<String, Tensor>,
}

impl File {
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn parse(b: &[u8]) -> Result<Self, String> {
        let mut at = 0usize;
        let take = |at: &mut usize, n: usize| -> Result<&[u8], String> {
            let s = b.get(*at..*at + n).ok_or("a short v2 weights file")?;
            *at += n;
            Ok(s)
        };
        if take(&mut at, 4)? != MAGIC {
            return Err("not a v2 weights file (export_v2.py weights)".into());
        }
        let u32_ = |at: &mut usize| -> Result<u32, String> { Ok(u32::from_le_bytes(take(at, 4)?.try_into().expect("4"))) };
        let version = u32_(&mut at)?;
        if version != VERSION {
            return Err(format!("v2 weights version {version}, this build reads {VERSION}"));
        }
        let count = u32_(&mut at)? as usize;
        let mut tensors = HashMap::with_capacity(count);
        for _ in 0..count {
            let n = u16::from_le_bytes(take(&mut at, 2)?.try_into().expect("2")) as usize;
            let name = String::from_utf8(take(&mut at, n)?.to_vec()).map_err(|_| "a tensor name not in UTF-8")?;
            let head = take(&mut at, 2)?;
            let (dtype, rank) = (head[0], head[1] as usize);
            let mut dims = Vec::with_capacity(rank);
            for _ in 0..rank {
                dims.push(u32_(&mut at)? as usize);
            }
            let off = u64::from_le_bytes(take(&mut at, 8)?.try_into().expect("8")) as usize;
            let len: usize = dims.iter().product();
            let data = |w: usize| b.get(off..off + len * w).ok_or_else(|| format!("{name}: data past the file's end"));
            let t = match dtype {
                0 => Tensor::F32(dims, data(4)?.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().expect("4"))).collect()),
                1 => Tensor::I64(dims, data(8)?.chunks_exact(8).map(|c| i64::from_le_bytes(c.try_into().expect("8"))).collect()),
                2 => Tensor::U8(dims, data(1)?.to_vec()),
                _ => return Err(format!("{name}: dtype {dtype}")),
            };
            tensors.insert(name, t);
        }
        Ok(Self { tensors })
    }

    pub fn f32(&self, name: &str) -> Result<(&[usize], &[f32]), String> {
        match self.tensors.get(name) {
            Some(Tensor::F32(d, v)) => Ok((d, v)),
            Some(_) => Err(format!("v2 weights: {name} is not f32")),
            None => Err(format!("v2 weights: no {name}")),
        }
    }

    pub fn i64s(&self, name: &str) -> Result<&[i64], String> {
        match self.tensors.get(name) {
            Some(Tensor::I64(_, v)) => Ok(v),
            Some(_) => Err(format!("v2 weights: {name} is not i64")),
            None => Err(format!("v2 weights: no {name}")),
        }
    }

    pub fn int(&self, name: &str) -> Result<usize, String> {
        let v = self.i64s(name)?;
        v.first().map(|x| *x as usize).ok_or_else(|| format!("v2 weights: {name} is empty"))
    }

    pub fn ints(&self, name: &str) -> Result<Vec<usize>, String> {
        Ok(self.i64s(name)?.iter().map(|x| *x as usize).collect())
    }

    /// A vector of `n` floats.
    pub fn vec(&self, name: &str, n: usize) -> Result<Vec<f32>, String> {
        let (_, v) = self.f32(name)?;
        if v.len() != n {
            return Err(format!("v2 weights: {name} has {} values, the config wants {n}", v.len()));
        }
        Ok(v.to_vec())
    }

    /// A `[out × in]` matrix, checked.
    pub fn mat(&self, name: &str, out: usize, inp: usize) -> Result<&[f32], String> {
        let (d, v) = self.f32(name)?;
        if d != [out, inp] {
            return Err(format!("v2 weights: {name} is {d:?}, the config wants [{out}, {inp}]"));
        }
        Ok(v)
    }

    /// A Linear layer `name.weight` `[out × in]` (and `name.bias` when `bias`).
    pub fn lin(&self, name: &str, inp: usize, out: usize, bias: bool) -> Result<Lin, String> {
        let w = self.mat(&format!("{name}.weight"), out, inp)?;
        let b = if bias { self.vec(&format!("{name}.bias"), out)? } else { vec![0.0; out] };
        Ok(Lin::from_rows(w, b, inp, out))
    }
}

/// A Linear layer, `y = x · W + b`, the weight kept transposed: `wt[i × out
/// + o]` is PyTorch's `weight[o][i]`.
#[derive(Debug, Clone, Default)]
pub struct Lin {
    pub inp: usize,
    pub out: usize,
    pub wt: Vec<f32>,
    pub b: Vec<f32>,
}

impl Lin {
    /// From PyTorch's `[out × in]` weight.
    pub fn from_rows(w: &[f32], b: Vec<f32>, inp: usize, out: usize) -> Self {
        let mut wt = vec![0.0; inp * out];
        for o in 0..out {
            for i in 0..inp {
                wt[i * out + o] = w[o * inp + i];
            }
        }
        Self { inp, out, wt, b }
    }

    /// The rows `from .. to` of the input as a layer of their own (no bias).
    pub fn input_slice(&self, from: usize, to: usize) -> Lin {
        Lin { inp: to - from, out: self.out, wt: self.wt[from * self.out..to * self.out].to_vec(), b: vec![0.0; self.out] }
    }

    /// `y = x · W + b` for `rows` rows.
    pub fn run(&self, x: &[f32], rows: usize, y: &mut [f32]) {
        super::gemm::linear(x, rows, self.inp, &self.wt, self.out, Some(&self.b), y);
    }

    /// `y += x · W` (no bias).
    pub fn add(&self, x: &[f32], rows: usize, y: &mut [f32]) {
        super::gemm::linear_acc(x, rows, self.inp, &self.wt, self.out, y);
    }
}

/// A GRU cell (PyTorch `GRUCell`): gates r, z, n.
#[derive(Debug, Clone, Default)]
pub struct Gru {
    pub ih: Lin,
    pub hh: Lin,
}

/// One map block (`model_v2.HexBlock`).
#[derive(Debug, Clone, Default)]
pub struct HexBlock {
    pub dilation: usize,
    /// `[7 × d_tile]`: the tile, then the 6 directions.
    pub dw: Vec<f32>,
    pub dw_bias: Vec<f32>,
    pub edge: Vec<f32>,
    pub norm: Vec<f32>,
    pub pw1: Lin,
    pub pw2: Lin,
}

/// One transformer block (`model_v2.Block`).
#[derive(Debug, Clone, Default)]
pub struct Block {
    pub norm1: Vec<f32>,
    pub qkv: Lin,
    pub qn: Vec<f32>,
    pub kn: Vec<f32>,
    pub out: Lin,
    pub norm2: Vec<f32>,
    pub ff1: Lin,
    pub ff2: Lin,
    /// `[heads × 14]`.
    pub dist_bias: Vec<f32>,
}

/// A table's input normalisation (`Net.norm`): numeric columns × multiplier,
/// clipped; id columns embedded.
#[derive(Debug, Clone, Default)]
pub struct Norm {
    pub columns: usize,
    pub num: Vec<usize>,
    pub mult: Vec<f32>,
    /// The candidate table's kept columns (`Measured.cand_keep`), else empty.
    pub keep: Vec<f32>,
    pub ids: Vec<usize>,
    /// `[ids × buckets × id_dim]`.
    pub emb: Vec<f32>,
    pub buckets: usize,
    pub id_dim: usize,
}

pub const CLIP: f32 = 128.0;

impl Norm {
    pub fn width(&self) -> usize {
        self.num.len() + self.ids.len() * self.id_dim
    }

    /// The layer input of one stored row into `out` (`width()` values).
    pub fn row(&self, x: &[i16], out: &mut [f32]) {
        let keep = |c: usize| if self.keep.is_empty() { f32::from(x[c]) } else { f32::from(x[c]) * self.keep[c] };
        for (j, &c) in self.num.iter().enumerate() {
            out[j] = (keep(c) * self.mult[c]).clamp(-CLIP, CLIP);
        }
        let base = self.num.len();
        for (r, &c) in self.ids.iter().enumerate() {
            let v = i64::from(x[c]);
            let bucket = if v <= 0 { 0 } else { 1 + (v - 1).rem_euclid(self.buckets as i64 - 1) } as usize;
            let row = &self.emb[(r * self.buckets + bucket) * self.id_dim..][..self.id_dim];
            for (k, e) in row.iter().enumerate() {
                out[base + r * self.id_dim + k] = e.clamp(-CLIP, CLIP);
            }
        }
    }
}

/// The config of the net (`common.Config`, the fields v2 reads).
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub c_tile: usize,
    pub c_global: usize,
    pub c_entity: usize,
    pub c_candidate: usize,
    pub c_event: usize,
    pub d_tile: usize,
    pub d_model: usize,
    pub heads: usize,
    pub d_ff: usize,
    pub d_head: usize,
    pub n_kinds: usize,
    pub n_relations: usize,
    pub n_decision_heads: usize,
    pub tag_buckets: usize,
    pub param_buckets: usize,
    pub mem_player: usize,
    pub mem_nation: usize,
    pub mem_unit: usize,
    pub trace: bool,
    pub trace_heads: u32,
    pub trace_shift: u32,
    pub events: bool,
    pub defer: bool,
}

impl Config {
    pub fn head_dim(&self) -> usize {
        self.d_model / self.heads
    }
}

/// The combat module (`model_v2.Combat`).
#[derive(Debug, Clone, Default)]
pub struct Combat {
    pub cols_entity: Vec<usize>,
    pub inv_entity: Vec<f32>,
    pub cols_tile: Vec<usize>,
    pub inv_tile: Vec<f32>,
    pub att: Lin,
    pub dfd: Lin,
    pub l2: Lin,
    pub head: Lin,
}

impl Combat {
    /// `side` of an entity row and its tile's row (`[ne + nt]` features).
    pub fn side(&self, ent: &[i16], tile: &[i16], out: &mut [f32]) {
        // sign(x) · log(1 + |x|), sign(0) = 0.
        let f = |x: f32| if x == 0.0 { 0.0 } else { x.signum() * x.abs().ln_1p() };
        for (j, (&c, &inv)) in self.cols_entity.iter().zip(&self.inv_entity).enumerate() {
            out[j] = f(f32::from(ent[c]) * inv);
        }
        let ne = self.cols_entity.len();
        for (j, (&c, &inv)) in self.cols_tile.iter().zip(&self.inv_tile).enumerate() {
            out[ne + j] = f(f32::from(tile[c]) * inv);
        }
    }

    /// `run`: from the first hidden layer's pre-activation `h1` (`[hidden]`)
    /// to the outputs (`[4]`) and the last hidden layer (`[width]`).
    pub fn run(&self, h1: &[f32], out: &mut [f32; 4], hid: &mut [f32]) {
        let mut a = [0.0f32; 256];
        let a = &mut a[..h1.len()];
        for (x, y) in a.iter_mut().zip(h1) {
            *x = y.max(0.0);
        }
        self.l2.run(a, 1, hid);
        for x in hid.iter_mut() {
            *x = x.max(0.0);
        }
        self.head.run(hid, 1, out);
    }
}

/// The whole v2 net.
#[derive(Debug, Clone, Default)]
pub struct Net {
    pub cfg: Config,
    pub label: String,
    pub layout_hash: u64,
    pub norm_tile: Norm,
    pub norm_global: Norm,
    pub norm_entity: Norm,
    pub norm_candidate: Norm,
    pub norm_event: Norm,
    pub tile_embed: Lin,
    pub map: Vec<HexBlock>,
    pub map_norm: Vec<f32>,
    pub region: Lin,
    pub entity: Lin,
    pub glob: Lin,
    pub blocks: Vec<Block>,
    pub final_norm: Vec<f32>,
    pub value_hidden: Lin,
    pub value_out: Lin,
    pub slot_player: Lin,
    pub slot_nation: Lin,
    pub slot_unit: Lin,
    pub gru_player: Gru,
    pub gru_nation: Gru,
    pub gru_unit: Gru,
    pub event: Lin,
    pub xq: Lin,
    pub xkv: Lin,
    pub xout: Lin,
    // The candidate encoding (`cand`), split by its input's blocks: the row,
    // the tag and parameter tables (the embeddings, clipped, through their
    // columns), the target tile's state, has a tile, the target entity's
    // token, has an entity.
    pub cand_row: Lin,
    pub cand_tag: Vec<f32>,
    pub cand_param: Vec<f32>,
    pub cand_tile: Lin,
    pub cand_has_tile: Vec<f32>,
    pub cand_ent: Lin,
    pub cand_has_ent: Vec<f32>,
    pub cand_bias: Vec<f32>,
    pub head_query: Vec<Lin>,
    pub trace: Lin,
    pub combat: Combat,
    pub fields_in: Lin,
    pub ca_norm: Vec<f32>,
    pub ca_q: Lin,
    pub ca_kv: Lin,
    pub ca_pair_kv: Lin,
    pub ca_qn: Vec<f32>,
    pub ca_kn: Vec<f32>,
    /// `[heads × 14 × dh]`.
    pub ca_dist_k: Vec<f32>,
    pub ca_dist_v: Vec<f32>,
    /// `ca_dist_k` transposed per head, its columns padded with zeros to
    /// [`N_BUCKETS_PAD`]: `[heads × dh × 16]` (whole vector blocks in `gemm`).
    pub ca_dist_kt: Vec<f32>,
    /// `[heads × 14]`.
    pub ca_dist_bias: Vec<f32>,
    /// `[heads × dh]`.
    pub ca_null_k: Vec<f32>,
    pub ca_null_v: Vec<f32>,
    pub ca_out: Lin,
    pub cand_out_norm: Vec<f32>,
    pub head_hidden: Vec<Lin>,
    pub head_logit: Vec<Lin>,
    pub defer_bias: f32,
    /// The entity columns of the sum heads (`export_v2.SUM_COLUMNS`).
    pub sum_columns: Vec<usize>,
}

/// Hex distance buckets (`model_v2._BUCKET_OF`, `N_BUCKETS`, `NO_POSITION`).
pub const N_BUCKETS: usize = 14;
/// [`N_BUCKETS`] rounded up to whole vector blocks.
pub const N_BUCKETS_PAD: usize = 16;
pub const NO_POSITION: usize = 13;

pub fn bucket_of(d: i64) -> usize {
    let d = d.clamp(0, 63);
    match d {
        0..=7 => d as usize,
        8..=11 => 8,
        12..=15 => 9,
        16..=23 => 10,
        24..=31 => 11,
        _ => 12,
    }
}

/// `model_v2.hex_distance` in f32 (the region centroids are fractional),
/// rounded half to even as `torch.round`.
pub fn hex_distance(q1: f32, r1: f32, q2: f32, r2: f32) -> i64 {
    let (dq, dr) = (q1 - q2, r1 - r2);
    ((dq.abs() + dr.abs() + (dq + dr).abs()) * 0.5).round_ties_even() as i64
}

impl Net {
    pub fn load(path: &Path) -> Result<Self, String> {
        Self::from_file(&File::load(path)?).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_file(f: &File) -> Result<Self, String> {
        let int = |n: &str| f.int(&format!("cfg.{n}"));
        if int("arch")? != 2 {
            return Err("not a v2 net".into());
        }
        let cfg = Config {
            c_tile: int("c_tile")?,
            c_global: int("c_global")?,
            c_entity: int("c_entity")?,
            c_candidate: int("c_candidate")?,
            c_event: int("c_event")?,
            d_tile: int("d_tile")?,
            d_model: int("d_model")?,
            heads: int("n_attn_heads")?,
            d_ff: int("d_ff")?,
            d_head: int("d_head")?,
            n_kinds: int("n_kinds")?,
            n_relations: int("n_relations")?,
            n_decision_heads: int("n_decision_heads")?,
            tag_buckets: int("tag_buckets")?,
            param_buckets: int("param_buckets")?,
            mem_player: int("mem_player")?,
            mem_nation: int("mem_nation")?,
            mem_unit: int("mem_unit")?,
            trace: int("trace")? != 0,
            trace_heads: int("trace_heads")? as u32,
            trace_shift: int("trace_shift")? as u32,
            events: int("events")? != 0,
            defer: int("defer")? != 0,
        };
        let (dt, d, h) = (cfg.d_tile, cfg.d_model, cfg.heads);
        let dh = d / h;
        let id_dim = int("id_dim")?;
        let id_buckets = int("id_buckets")?;
        let norm = |table: &str, columns: usize| -> Result<Norm, String> {
            let mult = f.vec(&format!("in_{table}"), columns)?;
            let num = f.ints(&format!("num_{table}"))?;
            let ids = f.ints(&format!("idc_{table}"))?;
            let emb = if ids.is_empty() { Vec::new() } else { f.vec(&format!("id_emb.{table}.weight"), ids.len() * id_buckets * id_dim)? };
            let keep = if table == "candidate" && int("drop_move_strike")? != 0 { f.vec("cand_keep", columns)? } else { Vec::new() };
            Ok(Norm { columns, num, mult, keep, ids, emb, buckets: id_buckets, id_dim })
        };
        let norm_tile = norm("tile", cfg.c_tile)?;
        let norm_global = norm("global", cfg.c_global)?;
        let norm_entity = norm("entity", cfg.c_entity)?;
        let norm_candidate = norm("candidate", cfg.c_candidate)?;
        let norm_event = if cfg.events { norm("event", cfg.c_event)? } else { Norm::default() };
        let ratio = int("map_ratio")?;
        let dilations = f.ints("cfg.map_dilations")?;
        let mut map = Vec::new();
        for (i, &k) in dilations.iter().enumerate() {
            let p = format!("map_blocks.{i}");
            map.push(HexBlock {
                dilation: k,
                dw: f.vec(&format!("{p}.dw"), 7 * dt)?,
                dw_bias: f.vec(&format!("{p}.dw_bias"), dt)?,
                edge: f.vec(&format!("{p}.edge"), dt)?,
                norm: f.vec(&format!("{p}.norm.weight"), dt)?,
                pw1: f.lin(&format!("{p}.pw1"), dt, ratio * dt, true)?,
                pw2: f.lin(&format!("{p}.pw2"), ratio * dt, dt, true)?,
            });
        }
        let mut blocks = Vec::new();
        for i in 0..int("n_attn")? {
            let p = format!("blocks.{i}");
            blocks.push(Block {
                norm1: f.vec(&format!("{p}.norm1.weight"), d)?,
                qkv: f.lin(&format!("{p}.qkv"), d, 3 * d, true)?,
                qn: f.vec(&format!("{p}.qn.weight"), dh)?,
                kn: f.vec(&format!("{p}.kn.weight"), dh)?,
                out: f.lin(&format!("{p}.out"), d, d, true)?,
                norm2: f.vec(&format!("{p}.norm2.weight"), d)?,
                ff1: f.lin(&format!("{p}.ff1"), d, cfg.d_ff, true)?,
                ff2: f.lin(&format!("{p}.ff2"), cfg.d_ff, d, true)?,
                dist_bias: f.vec(&format!("{p}.dist_bias"), h * N_BUCKETS)?,
            });
        }
        let gru = |name: &str, inp: usize, hid: usize| -> Result<Gru, String> {
            let ih = Lin::from_rows(f.mat(&format!("{name}.weight_ih"), 3 * hid, inp)?, f.vec(&format!("{name}.bias_ih"), 3 * hid)?, inp, 3 * hid);
            let hh = Lin::from_rows(f.mat(&format!("{name}.weight_hh"), 3 * hid, hid)?, f.vec(&format!("{name}.bias_hh"), 3 * hid)?, hid, 3 * hid);
            Ok(Gru { ih, hh })
        };
        // The candidate encoding, split (see the fields).
        let cin = int("candidate_in")?;
        let cand = f.lin("cand", cin, d, true)?;
        let rn = norm_candidate.width();
        let tag_dim = int("tag_dim")?;
        let param_dim = int("param_dim")?;
        let table = |emb: &str, rows: usize, dim: usize, from: usize| -> Result<Vec<f32>, String> {
            let e = f.vec(emb, rows * dim)?;
            let mut out = vec![0.0; rows * d];
            for r in 0..rows {
                for k in 0..dim {
                    let x = e[r * dim + k].clamp(-CLIP, CLIP);
                    let w = &cand.wt[(from + k) * d..][..d];
                    for (o, wv) in out[r * d..][..d].iter_mut().zip(w) {
                        *o += x * wv;
                    }
                }
            }
            Ok(out)
        };
        let nh = cfg.n_decision_heads;
        let cand_tag = table("tag_emb.weight", nh * cfg.tag_buckets, tag_dim, rn)?;
        let cand_param = table("param_emb.weight", nh * cfg.param_buckets, param_dim, rn + tag_dim)?;
        let t0 = rn + tag_dim + param_dim;
        let cand_tile = cand.input_slice(t0, t0 + dt);
        let cand_has_tile = cand.wt[(t0 + dt) * d..][..d].to_vec();
        let e0 = t0 + dt + 1;
        let cand_ent = cand.input_slice(e0, e0 + d);
        let cand_has_ent = cand.wt[(e0 + d) * d..][..d].to_vec();
        if e0 + d + 1 != cin {
            return Err(format!("candidate input {cin}, the blocks sum to {}", e0 + d + 1));
        }
        let ce = f.ints("combat.cols_entity")?;
        let ct = f.ints("combat.cols_tile")?;
        let ncomb = ce.len() + ct.len();
        let hidden = f.f32("combat.att.weight")?.0[0];
        let width = f.f32("combat.l2.weight")?.0[0];
        let combat = Combat {
            inv_entity: f.vec("combat.inv_entity", ce.len())?,
            inv_tile: f.vec("combat.inv_tile", ct.len())?,
            cols_entity: ce,
            cols_tile: ct,
            att: f.lin("combat.att", ncomb, hidden, true)?,
            dfd: f.lin("combat.dfd", ncomb, hidden, false)?,
            l2: f.lin("combat.l2", hidden, width, true)?,
            head: f.lin("combat.head", width, 4, true)?,
        };
        let lh = f.i64s("layout_hash")?;
        let label = match f.tensors.get("label") {
            Some(Tensor::U8(_, v)) => String::from_utf8_lossy(v).into_owned(),
            _ => String::new(),
        };
        let ev_in = int("event_in")?;
        Ok(Net {
            label,
            layout_hash: lh.first().map_or(0, |x| *x as u64),
            tile_embed: f.lin("tile_embed", norm_tile.width(), dt, true)?,
            norm_tile,
            region: f.lin("region", 2 * dt, d, true)?,
            entity: f.lin("entity", int("entity_in")?, d, true)?,
            glob: f.lin("glob", norm_global.width(), d, true)?,
            norm_global,
            norm_entity,
            map,
            map_norm: f.vec("map_norm.weight", dt)?,
            blocks,
            final_norm: f.vec("final_norm.weight", d)?,
            value_hidden: f.lin("value_hidden", d, cfg.d_head, true)?,
            value_out: f.lin("value_out", cfg.d_head, 1, true)?,
            slot_player: f.lin("slot_player", cfg.mem_player, d, true)?,
            slot_nation: f.lin("slot_nation", cfg.mem_nation, d, true)?,
            slot_unit: f.lin("slot_unit", cfg.mem_unit, d, true)?,
            gru_player: gru("gru_player", d, cfg.mem_player)?,
            gru_nation: gru("gru_nation", d, cfg.mem_nation)?,
            gru_unit: gru("gru_unit", d, cfg.mem_unit)?,
            event: if cfg.events { f.lin("event", ev_in, d, true)? } else { Lin::default() },
            xq: if cfg.events { f.lin("xattn.q", d, d, true)? } else { Lin::default() },
            xkv: if cfg.events { f.lin("xattn.kv", d, 2 * d, true)? } else { Lin::default() },
            xout: if cfg.events { f.lin("xattn.out", d, d, true)? } else { Lin::default() },
            norm_event,
            cand_row: cand.input_slice(0, rn),
            norm_candidate,
            cand_tag,
            cand_param,
            cand_tile,
            cand_has_tile,
            cand_ent,
            cand_has_ent,
            cand_bias: cand.b.clone(),
            head_query: (0..nh).map(|i| f.lin(&format!("head_query.{i}"), d, d, true)).collect::<Result<_, _>>()?,
            trace: if cfg.trace { f.lin("trace", d, d, false)? } else { Lin::default() },
            fields_in: f.lin("fields_in", 4, d, true)?,
            ca_norm: f.vec("cand_attn.norm.weight", d)?,
            ca_q: f.lin("cand_attn.q", d, d, true)?,
            ca_kv: f.lin("cand_attn.kv", d, 2 * d, true)?,
            ca_pair_kv: f.lin("cand_attn.pair_kv", 4 + width, 2 * d, true)?,
            ca_qn: f.vec("cand_attn.qn.weight", dh)?,
            ca_kn: f.vec("cand_attn.kn.weight", dh)?,
            ca_dist_kt: {
                let k = f.vec("cand_attn.dist_k", h * N_BUCKETS * dh)?;
                let mut t = vec![0.0; h * dh * N_BUCKETS_PAD];
                for hh in 0..h {
                    for b in 0..N_BUCKETS {
                        for c in 0..dh {
                            t[(hh * dh + c) * N_BUCKETS_PAD + b] = k[(hh * N_BUCKETS + b) * dh + c];
                        }
                    }
                }
                t
            },
            ca_dist_k: f.vec("cand_attn.dist_k", h * N_BUCKETS * dh)?,
            ca_dist_v: f.vec("cand_attn.dist_v", h * N_BUCKETS * dh)?,
            ca_dist_bias: f.vec("cand_attn.dist_bias", h * N_BUCKETS)?,
            ca_null_k: f.vec("cand_attn.null_k", h * dh)?,
            ca_null_v: f.vec("cand_attn.null_v", h * dh)?,
            ca_out: f.lin("cand_attn.out", d, d, true)?,
            cand_out_norm: f.vec("cand_out_norm.weight", d)?,
            head_hidden: (0..nh).map(|i| f.lin(&format!("head_hidden.{i}"), d, cfg.d_head, true)).collect::<Result<_, _>>()?,
            head_logit: (0..nh).map(|i| f.lin(&format!("head_logit.{i}"), cfg.d_head, 1, true)).collect::<Result<_, _>>()?,
            defer_bias: if cfg.defer { f.f32("defer_bias")?.1[0] } else { 0.0 },
            sum_columns: f.ints("sum.columns")?,
            combat,
            cfg,
        })
    }
}

/// `x · rsqrt(mean(x²) + 1e-6) · w` in place (`model_v2.RMSNorm`).
pub fn rms_norm(x: &mut [f32], w: &[f32]) {
    let ms = x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32;
    let r = 1.0 / (ms + 1e-6).sqrt();
    for (v, g) in x.iter_mut().zip(w) {
        *v = *v * r * g;
    }
}

/// LayerNorm without affine parameters (`F.layer_norm`, eps 1e-5).
pub fn layer_norm(x: &[f32], out: &mut [f32]) {
    let n = x.len() as f32;
    let mean = x.iter().sum::<f32>() / n;
    let var = x.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n;
    let r = 1.0 / (var + 1e-5).sqrt();
    for (o, v) in out.iter_mut().zip(x) {
        *o = (v - mean) * r;
    }
}

pub fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// The GRU cell on one row: `h' = (1 − z) n + z h`.
pub fn gru_row(g: &Gru, x: &[f32], h: &[f32], out: &mut [f32]) {
    let n = h.len();
    let mut gi = vec![0.0; 3 * n];
    let mut gh = vec![0.0; 3 * n];
    g.ih.run(x, 1, &mut gi);
    g.hh.run(h, 1, &mut gh);
    for j in 0..n {
        let r = sigmoid(gi[j] + gh[j]);
        let z = sigmoid(gi[n + j] + gh[n + j]);
        let c = (gi[2 * n + j] + r * gh[2 * n + j]).tanh();
        out[j] = (1.0 - z) * c + z * h[j];
    }
}

//! The trunk of v2, once per player-turn (`model_v2.NetV2.embed`, `attend`,
//! `value`, `update_memory`, and the parts of `score` that do not depend on
//! the decision): [`Job::prepare`] turns a turn's tables and the session's
//! memory into the layer inputs on the CPU; [`run_cpu`] is the f32
//! reference forward (and the fallback without a GPU); `gpu.rs` computes the
//! same [`TrunkOut`] on the GPU.
//!
//! Token order: regions, entities, global, player slot, then a slot token
//! per entity that has a slot (an own unit, a city-state, a nation), in
//! entity order. (The training's tokens have a slot position per entity,
//! masked where it has none; those rows are no key and nothing reads them.)

use std::sync::Arc;

use super::weights::{bucket_of, hex_distance, layer_norm, rms_norm, Net, NO_POSITION};

/// What a map's neighbour and region tables give: tile coordinates, the
/// dilated neighbour indices, the regions' tiles and centroids.
#[derive(Debug)]
pub struct MapPrep {
    pub n_tiles: usize,
    pub neighbors: Vec<i32>,
    pub region: Vec<i32>,
    pub n_regions: usize,
    pub q: Vec<f32>,
    pub r: Vec<f32>,
    /// Per region, its tiles (ascending): `region_tiles[region_start[g] ..
    /// region_start[g + 1]]`.
    pub region_start: Vec<u32>,
    pub region_tiles: Vec<u32>,
    pub region_q: Vec<f32>,
    pub region_r: Vec<f32>,
    /// Per map block's dilation: `[tiles × 7]` the tile, then the tile `k`
    /// steps along each direction (`n_tiles`: off the map, the edge row).
    pub dilated: Vec<(usize, Vec<u32>)>,
}

impl MapPrep {
    pub fn new(neighbors: &[i32], region: &[i32], dilations: &[usize]) -> Self {
        let t = region.len();
        // model_v2.map_width: TileId = r × width + q; tile 0's sixth
        // neighbour (q, r+1) is `width` (the synthetic odd-q tables put it fourth).
        let w = if t == 0 {
            1
        } else {
            let kernel = neighbors[0] == 1;
            let w = if kernel { neighbors[5] } else { neighbors[3] };
            if w > 0 {
                w as usize
            } else {
                t
            }
        };
        let q: Vec<f32> = (0..t).map(|i| (i % w) as f32).collect();
        let r: Vec<f32> = (0..t).map(|i| (i / w) as f32).collect();
        let n_regions = region.iter().copied().max().map_or(0, |m| m as usize + 1);
        let mut count = vec![0u32; n_regions];
        for &g in region {
            count[g as usize] += 1;
        }
        let mut region_start = vec![0u32; n_regions + 1];
        for g in 0..n_regions {
            region_start[g + 1] = region_start[g] + count[g];
        }
        let mut fill = region_start.clone();
        let mut region_tiles = vec![0u32; t];
        let (mut sq, mut sr) = (vec![0.0f32; n_regions], vec![0.0f32; n_regions]);
        for (i, &g) in region.iter().enumerate() {
            let g = g as usize;
            region_tiles[fill[g] as usize] = i as u32;
            fill[g] += 1;
            sq[g] += q[i];
            sr[g] += r[i];
        }
        let cnt = |g: usize| (count[g].max(1)) as f32;
        let region_q = (0..n_regions).map(|g| sq[g] / cnt(g)).collect();
        let region_r = (0..n_regions).map(|g| sr[g] / cnt(g)).collect();
        let mut dilated: Vec<(usize, Vec<u32>)> = Vec::new();
        for &k in dilations {
            if dilated.iter().any(|(d, _)| *d == k) {
                continue;
            }
            let mut idx = vec![0u32; t * 7];
            for i in 0..t {
                idx[i * 7] = i as u32;
                for j in 0..6 {
                    let mut cur = i;
                    for _ in 0..k {
                        if cur == t {
                            break;
                        }
                        let nb = neighbors[cur * 6 + j];
                        cur = if nb < 0 || nb as usize >= t { t } else { nb as usize };
                    }
                    idx[i * 7 + 1 + j] = cur as u32;
                }
            }
            dilated.push((k, idx));
        }
        Self { n_tiles: t, neighbors: neighbors.to_vec(), region: region.to_vec(), n_regions, q, r, region_start, region_tiles, region_q, region_r, dilated }
    }

    pub fn same(&self, neighbors: &[i32], region: &[i32]) -> bool {
        self.neighbors == neighbors && self.region == region
    }

    pub fn dilated(&self, k: usize) -> &[u32] {
        &self.dilated.iter().find(|(d, _)| *d == k).expect("a block's dilation").1
    }
}

/// A slot token: its entity and kind.
#[derive(Debug, Clone, Copy)]
pub struct Slot {
    pub entity: usize,
    pub unit: bool,
}

/// One player-turn's trunk input, prepared on the CPU.
#[derive(Debug, Default)]
pub struct Job {
    pub map: Option<Arc<MapPrep>>,
    /// `[tiles × row_in(tile)]`.
    pub tiles: Vec<f32>,
    /// `[row_in(global)]`.
    pub glob: Vec<f32>,
    pub n_ent: usize,
    /// `[E × entity_in]`, the tile-state columns and `has` left for the trunk.
    pub ent_in: Vec<f32>,
    /// The entities' tiles (−1 none).
    pub ent_tile: Vec<i32>,
    pub slots: Vec<Slot>,
    pub mem_player: Vec<f32>,
    /// The unit slots' memory `[units × mem_unit]` and the nation slots'
    /// `[nations × mem_nation]`, each in slot order.
    pub mem_unit: Vec<f32>,
    pub mem_nation: Vec<f32>,
    pub n_ev: usize,
    /// `[V × event_in]`, the region and actor token columns left for the trunk.
    pub ev_in: Vec<f32>,
    pub ev_region: Vec<i32>,
    pub ev_actor: Vec<i32>,
    /// The combat module's features `[E × (ne + nt)]` (`Combat.side`).
    pub side: Vec<f32>,
    /// The sum heads' entity masks and reach (`NetV2.sum_fields`).
    pub ready: Vec<bool>,
    pub hostile: Vec<bool>,
    pub reach: Vec<f32>,
}

/// The turn's tables (as `model::TurnInput`) and the memory the session
/// reads for it.
pub struct Tables<'a> {
    pub tiles: &'a [i16],
    pub global: &'a [i16],
    pub entities: &'a [i16],
    pub entity_meta: &'a [i32],
    pub events: &'a [i16],
    pub event_meta: &'a [i32],
}

/// Memory slot kinds (`common.UNIT_SLOT_KINDS`, `NATION_SLOT_KINDS`).
pub fn slot_kind(kind: i32) -> Option<bool> {
    match kind {
        0 => Some(true),
        4 | 5 => Some(false),
        _ => None,
    }
}

impl Job {
    /// The layer inputs of a turn; `memory(e, unit)` the slot memory of
    /// entity `e` (None: zero).
    pub fn prepare(net: &Net, map: Arc<MapPrep>, t: &Tables<'_>, mem_player: &[f32], memory: &dyn Fn(usize, bool) -> Option<Vec<f32>>) -> Job {
        let cfg = &net.cfg;
        let nt = map.n_tiles;
        let wt = net.norm_tile.width();
        let mut tiles = vec![0.0; nt * wt];
        for i in 0..nt {
            net.norm_tile.row(&t.tiles[i * cfg.c_tile..(i + 1) * cfg.c_tile], &mut tiles[i * wt..(i + 1) * wt]);
        }
        let mut glob = vec![0.0; net.norm_global.width()];
        net.norm_global.row(t.global, &mut glob);
        let e = t.entity_meta.len() / 3;
        let ein = net.entity.inp;
        let we = net.norm_entity.width();
        let mut ent_in = vec![0.0; e * ein];
        let mut ent_tile = vec![-1; e];
        let mut slots = Vec::new();
        let (mut mem_unit, mut mem_nation) = (Vec::new(), Vec::new());
        let comb = &net.combat;
        let nside = comb.att.inp;
        let mut side = vec![0.0; e * nside];
        let sc = &net.sum_columns;
        let (mut ready, mut hostile, mut reach) = (vec![false; e], vec![false; e], vec![0.0; e]);
        for i in 0..e {
            let row = &t.entities[i * cfg.c_entity..(i + 1) * cfg.c_entity];
            let (kind, tile, rel) = (t.entity_meta[i * 3], t.entity_meta[i * 3 + 1], t.entity_meta[i * 3 + 2]);
            let x = &mut ent_in[i * ein..(i + 1) * ein];
            net.norm_entity.row(row, &mut x[..we]);
            if kind >= 0 && (kind as usize) < cfg.n_kinds {
                x[we + kind as usize] = 1.0;
            }
            if rel >= 0 && (rel as usize) < cfg.n_relations {
                x[we + cfg.n_kinds + rel as usize] = 1.0;
            }
            let et = if tile >= 0 && (tile as usize) < nt { tile } else { -1 };
            ent_tile[i] = et;
            if let Some(unit) = slot_kind(kind) {
                slots.push(Slot { entity: i, unit });
                let width = if unit { cfg.mem_unit } else { cfg.mem_nation };
                let m = memory(i, unit).unwrap_or_else(|| vec![0.0; width]);
                if unit { &mut mem_unit } else { &mut mem_nation }.extend_from_slice(&m);
            }
            // The combat side reads the tile row of the entity's tile (tile 0 without one, as the training's clamp).
            let tr = et.max(0) as usize;
            comb.side(row, &t.tiles[tr * cfg.c_tile..(tr + 1) * cfg.c_tile], &mut side[i * nside..(i + 1) * nside]);
            let col = |j: usize| f32::from(row[sc[j]]);
            let ok = et >= 0;
            let fights = (col(0) > 0.0 || col(1) > 0.0) && col(2) == 0.0 && ok;
            ready[i] = fights && col(3) > 0.0 && col(4) > 0.0 && col(5) > 0.0;
            hostile[i] = fights && col(6) > 0.0 && (col(7) > 0.0 || col(8) > 0.0);
            reach[i] = col(9) + col(10).max(1.0);
        }
        let (mut n_ev, mut ev_in, mut ev_region, mut ev_actor) = (0, Vec::new(), Vec::new(), Vec::new());
        if cfg.events {
            n_ev = t.event_meta.len() / 4;
            let vin = net.event.inp;
            let wv = net.norm_event.width();
            ev_in = vec![0.0; n_ev * vin];
            for v in 0..n_ev {
                net.norm_event.row(&t.events[v * cfg.c_event..(v + 1) * cfg.c_event], &mut ev_in[v * vin..v * vin + wv]);
                let tile = t.event_meta[v * 4 + 1];
                ev_region.push(if tile >= 0 && (tile as usize) < nt { map.region[tile as usize] } else { -1 });
                let a = t.event_meta[v * 4 + 2];
                ev_actor.push(if a >= 0 && (a as usize) < e { a } else { -1 });
            }
        }
        Job {
            map: Some(map),
            tiles,
            glob,
            n_ent: e,
            ent_in,
            ent_tile,
            slots,
            mem_player: mem_player.to_vec(),
            mem_unit,
            mem_nation,
            n_ev,
            ev_in,
            ev_region,
            ev_actor,
            side,
            ready,
            hostile,
            reach,
        }
    }

    pub fn map(&self) -> &MapPrep {
        self.map.as_deref().expect("a prepared job")
    }

    /// Token count: regions, entities, global, player, slots.
    pub fn tokens(&self) -> usize {
        self.map().n_regions + self.n_ent + 2 + self.slots.len()
    }

    /// Each token's position (axial q, r) and whether it has one.
    pub fn positions(&self) -> Vec<(f32, f32, bool)> {
        let m = self.map();
        let ent = |e: usize| {
            let t = self.ent_tile[e];
            let i = t.max(0) as usize;
            (m.q.get(i).copied().unwrap_or(0.0), m.r.get(i).copied().unwrap_or(0.0), t >= 0)
        };
        let mut p = Vec::with_capacity(self.tokens());
        p.extend((0..m.n_regions).map(|g| (m.region_q[g], m.region_r[g], true)));
        p.extend((0..self.n_ent).map(ent));
        p.push((0.0, 0.0, false));
        p.push((0.0, 0.0, false));
        p.extend(self.slots.iter().map(|s| ent(s.entity)));
        p
    }
}

/// The distance bucket of two positions.
pub fn pair_bucket(a: (f32, f32, bool), b: (f32, f32, bool)) -> usize {
    if a.2 && b.2 {
        bucket_of(hex_distance(a.0, a.1, b.0, b.1))
    } else {
        NO_POSITION
    }
}

/// What the trunk gives a turn's decisions.
#[derive(Debug, Default, Clone)]
pub struct TrunkOut {
    /// `[tiles × d_tile]` after the final map RMSNorm.
    pub tile_h: Vec<f32>,
    /// `[tokens × d_model]` after the final RMSNorm.
    pub tokens: Vec<f32>,
    pub value: f32,
    pub new_player: Vec<f32>,
    /// The memory after the turn of the unit and the nation slots, in slot order.
    pub new_unit: Vec<f32>,
    pub new_nation: Vec<f32>,
    /// `cand_attn.kv` of the entity tokens `[E × 2d]`.
    pub ent_kv: Vec<f32>,
    /// The candidate encoding's entity-token columns times the tokens `[E × d]`.
    pub p_ent: Vec<f32>,
    /// `combat.att(side)` and `combat.dfd(side)` `[E × hidden]`.
    pub att_side: Vec<f32>,
    pub dfd_side: Vec<f32>,
    /// `[E × E]`: the share of the defender's (column) hit points the
    /// attacker's (row) blow deals, clamped to 0 … 1; computed for the
    /// ready and hostile rows only (others 0).
    pub dealt: Vec<f32>,
    /// The focus sum head per entity.
    pub focus: Vec<f32>,
}

fn relu2(x: &mut [f32]) {
    for v in x {
        let r = v.max(0.0);
        *v = r * r;
    }
}

/// The f32 reference trunk on the CPU.
pub fn run_cpu(net: &Net, job: &Job) -> TrunkOut {
    let cfg = &net.cfg;
    let (dt, d, nh) = (cfg.d_tile, cfg.d_model, cfg.heads);
    let dh = d / nh;
    let m = job.map();
    let nt = m.n_tiles;
    // Map.
    let mut h = vec![0.0; nt * dt];
    net.tile_embed.run(&job.tiles, nt, &mut h);
    let mut y = vec![0.0; nt * dt];
    let mut a = vec![0.0; nt * net.map.first().map_or(0, |b| b.pw1.out)];
    let mut b2 = vec![0.0; nt * dt];
    for blk in &net.map {
        let idx = m.dilated(blk.dilation);
        for t in 0..nt {
            let yr = &mut y[t * dt..(t + 1) * dt];
            yr.fill(0.0);
            for tap in 0..7 {
                let src = idx[t * 7 + tap] as usize;
                let g = if src == nt { &blk.edge[..] } else { &h[src * dt..(src + 1) * dt] };
                let w = &blk.dw[tap * dt..(tap + 1) * dt];
                for c in 0..dt {
                    yr[c] = g[c].mul_add(w[c], yr[c]);
                }
            }
            for c in 0..dt {
                yr[c] += blk.dw_bias[c];
            }
            rms_norm(yr, &blk.norm);
        }
        blk.pw1.run(&y, nt, &mut a);
        relu2(&mut a);
        blk.pw2.run(&a, nt, &mut b2);
        for (x, v) in h.iter_mut().zip(&b2) {
            *x += v;
        }
    }
    for t in 0..nt {
        rms_norm(&mut h[t * dt..(t + 1) * dt], &net.map_norm);
    }
    let tile_h = h;
    // Regions: mean and max (with a zero) of the tile states.
    let r = m.n_regions;
    let mut pool = vec![0.0; r * 2 * dt];
    for g in 0..r {
        let tiles = &m.region_tiles[m.region_start[g] as usize..m.region_start[g + 1] as usize];
        let p = &mut pool[g * 2 * dt..(g + 1) * 2 * dt];
        p[dt..].fill(0.0);
        for &t in tiles {
            let row = &tile_h[t as usize * dt..(t as usize + 1) * dt];
            for c in 0..dt {
                p[c] += row[c];
                p[dt + c] = p[dt + c].max(row[c]);
            }
        }
        let cnt = tiles.len().max(1) as f32;
        for c in 0..dt {
            p[c] /= cnt;
        }
    }
    let n = job.tokens();
    let e = job.n_ent;
    let (o_ent, o_glob) = (r, r + e);
    let o_slots = o_glob + 2;
    let mut x = vec![0.0; n * d];
    net.region.run(&pool, r, &mut x[..r * d]);
    // Entities.
    let ein = net.entity.inp;
    let mut ent_in = job.ent_in.clone();
    let c0 = ein - dt - 1;
    for i in 0..e {
        let et = job.ent_tile[i];
        if et >= 0 {
            ent_in[i * ein + c0..i * ein + c0 + dt].copy_from_slice(&tile_h[et as usize * dt..(et as usize + 1) * dt]);
            ent_in[i * ein + ein - 1] = 1.0;
        }
    }
    net.entity.run(&ent_in, e, &mut x[o_ent * d..(o_ent + e) * d]);
    net.glob.run(&job.glob, 1, &mut x[o_glob * d..(o_glob + 1) * d]);
    net.slot_player.run(&job.mem_player, 1, &mut x[(o_glob + 1) * d..(o_glob + 2) * d]);
    let (mut iu, mut inn) = (0, 0);
    for (s, slot) in job.slots.iter().enumerate() {
        let row = &mut x[(o_slots + s) * d..(o_slots + s + 1) * d];
        if slot.unit {
            net.slot_unit.run(&job.mem_unit[iu * cfg.mem_unit..(iu + 1) * cfg.mem_unit], 1, row);
            iu += 1;
        } else {
            net.slot_nation.run(&job.mem_nation[inn * cfg.mem_nation..(inn + 1) * cfg.mem_nation], 1, row);
            inn += 1;
        }
        let et = &x[(o_ent + slot.entity) * d..(o_ent + slot.entity + 1) * d].to_vec();
        for (a, b) in x[(o_slots + s) * d..(o_slots + s + 1) * d].iter_mut().zip(et) {
            *a += b;
        }
    }
    // Events: every token attends over the event tokens.
    if cfg.events && job.n_ev > 0 {
        let v = job.n_ev;
        let vin = net.event.inp;
        let mut ev_in = job.ev_in.clone();
        let wv = net.norm_event.width();
        for j in 0..v {
            let row = &mut ev_in[j * vin..(j + 1) * vin];
            let g = job.ev_region[j];
            if g >= 0 {
                row[wv..wv + d].copy_from_slice(&x[g as usize * d..(g as usize + 1) * d]);
                row[wv + d] = 1.0;
            }
            let ac = job.ev_actor[j];
            if ac >= 0 {
                let t = o_ent + ac as usize;
                row[wv + d + 1..wv + 2 * d + 1].copy_from_slice(&x[t * d..(t + 1) * d]);
                row[wv + 2 * d + 1] = 1.0;
            }
        }
        let mut ev = vec![0.0; v * d];
        net.event.run(&ev_in, v, &mut ev);
        let mut ln = vec![0.0; v * d];
        for j in 0..v {
            layer_norm(&ev[j * d..(j + 1) * d], &mut ln[j * d..(j + 1) * d]);
        }
        let mut kv = vec![0.0; v * 2 * d];
        net.xkv.run(&ln, v, &mut kv);
        let mut lx = vec![0.0; n * d];
        for i in 0..n {
            layer_norm(&x[i * d..(i + 1) * d], &mut lx[i * d..(i + 1) * d]);
        }
        let mut q = vec![0.0; n * d];
        net.xq.run(&lx, n, &mut q);
        let mut o = vec![0.0; n * d];
        let scale = 1.0 / (dh as f32).sqrt();
        let mut s = vec![0.0; v];
        for i in 0..n {
            for hh in 0..nh {
                let qi = &q[i * d + hh * dh..][..dh];
                for j in 0..v {
                    s[j] = super::gemm::dot(qi, &kv[j * 2 * d + hh * dh..][..dh]) * scale;
                }
                softmax(&mut s);
                let oi = &mut o[i * d + hh * dh..][..dh];
                for j in 0..v {
                    let vv = &kv[j * 2 * d + d + hh * dh..][..dh];
                    for c in 0..dh {
                        oi[c] = s[j].mul_add(vv[c], oi[c]);
                    }
                }
            }
        }
        let mut add = vec![0.0; n * d];
        net.xout.run(&o, n, &mut add);
        for (a, b) in x.iter_mut().zip(&add) {
            *a += b;
        }
    }
    // The transformer.
    let pos = job.positions();
    // Every token here is a key (the training's slot positions without a slot are not built).
    let key = vec![true; n];
    let mut bucket = vec![0u8; n * n];
    for i in 0..n {
        for j in 0..n {
            bucket[i * n + j] = pair_bucket(pos[i], pos[j]) as u8;
        }
    }
    let mut nx = vec![0.0; n * d];
    let mut qkv = vec![0.0; n * 3 * d];
    let mut o = vec![0.0; n * d];
    let mut add = vec![0.0; n * d];
    let mut ff = vec![0.0; n * cfg.d_ff];
    let scale = 1.0 / (dh as f32).sqrt();
    let mut s = vec![0.0; n];
    for blk in &net.blocks {
        nx.copy_from_slice(&x);
        for i in 0..n {
            rms_norm(&mut nx[i * d..(i + 1) * d], &blk.norm1);
        }
        blk.qkv.run(&nx, n, &mut qkv);
        for i in 0..n {
            for hh in 0..nh {
                rms_norm(&mut qkv[i * 3 * d + hh * dh..][..dh], &blk.qn);
                rms_norm(&mut qkv[i * 3 * d + d + hh * dh..][..dh], &blk.kn);
            }
        }
        for i in 0..n {
            for hh in 0..nh {
                let qi = &qkv[i * 3 * d + hh * dh..][..dh];
                for j in 0..n {
                    s[j] = if key[j] { super::gemm::dot(qi, &qkv[j * 3 * d + d + hh * dh..][..dh]) * scale + blk.dist_bias[hh * 14 + bucket[i * n + j] as usize] } else { f32::NEG_INFINITY };
                }
                softmax(&mut s);
                let oi = &mut o[i * d + hh * dh..][..dh];
                oi.fill(0.0);
                for j in 0..n {
                    if s[j] == 0.0 {
                        continue;
                    }
                    let vv = &qkv[j * 3 * d + 2 * d + hh * dh..][..dh];
                    for c in 0..dh {
                        oi[c] = s[j].mul_add(vv[c], oi[c]);
                    }
                }
            }
        }
        blk.out.run(&o, n, &mut add);
        for (a, b) in x.iter_mut().zip(&add) {
            *a += b;
        }
        nx.copy_from_slice(&x);
        for i in 0..n {
            rms_norm(&mut nx[i * d..(i + 1) * d], &blk.norm2);
        }
        blk.ff1.run(&nx, n, &mut ff);
        relu2(&mut ff);
        blk.ff2.run(&ff, n, &mut add);
        for (a, b) in x.iter_mut().zip(&add) {
            *a += b;
        }
    }
    for i in 0..n {
        rms_norm(&mut x[i * d..(i + 1) * d], &net.final_norm);
    }
    let tokens = x;
    finish(net, job, tile_h, tokens)
}

/// The per-turn outputs after the tokens (value, memory, the decision
/// path's tables), shared by the CPU and GPU trunks.
pub fn finish(net: &Net, job: &Job, tile_h: Vec<f32>, tokens: Vec<f32>) -> TrunkOut {
    let mut out = TrunkOut { tile_h, tokens, ..Default::default() };
    heads(net, job, &mut out);
    combat(net, job, &mut out);
    out
}

/// Value, memory, the entities' candidate-attention keys and values and
/// the candidate encoding's entity columns.
pub fn heads(net: &Net, job: &Job, out: &mut TrunkOut) {
    let cfg = &net.cfg;
    let d = cfg.d_model;
    let r = job.map().n_regions;
    let e = job.n_ent;
    let (o_ent, o_glob) = (r, r + e);
    let o_slots = o_glob + 2;
    let x = &out.tokens;
    let mut vh = vec![0.0; cfg.d_head];
    net.value_hidden.run(&x[o_glob * d..(o_glob + 1) * d], 1, &mut vh);
    for v in &mut vh {
        *v = v.max(0.0);
    }
    let mut v = [0.0f32];
    net.value_out.run(&vh, 1, &mut v);
    out.value = v[0];
    out.new_player = vec![0.0; cfg.mem_player];
    super::weights::gru_row(&net.gru_player, &x[(o_glob + 1) * d..(o_glob + 2) * d], &job.mem_player, &mut out.new_player);
    let (mut iu, mut inn) = (0, 0);
    out.new_unit = vec![0.0; job.mem_unit.len()];
    out.new_nation = vec![0.0; job.mem_nation.len()];
    for (s, slot) in job.slots.iter().enumerate() {
        let tok = &x[(o_slots + s) * d..(o_slots + s + 1) * d];
        if slot.unit {
            let w = cfg.mem_unit;
            super::weights::gru_row(&net.gru_unit, tok, &job.mem_unit[iu * w..(iu + 1) * w], &mut out.new_unit[iu * w..(iu + 1) * w]);
            iu += 1;
        } else {
            let w = cfg.mem_nation;
            super::weights::gru_row(&net.gru_nation, tok, &job.mem_nation[inn * w..(inn + 1) * w], &mut out.new_nation[inn * w..(inn + 1) * w]);
            inn += 1;
        }
    }
    let ents = &x[o_ent * d..(o_ent + e) * d];
    out.ent_kv = vec![0.0; e * 2 * d];
    net.ca_kv.run(ents, e, &mut out.ent_kv);
    out.p_ent = vec![0.0; e * d];
    super::gemm::linear(ents, e, d, &net.cand_ent.wt, d, None, &mut out.p_ent);
}

/// The combat module's per-entity layers, the blows of the ready and
/// hostile entities on every entity, and the focus sum head.
pub fn combat(net: &Net, job: &Job, out: &mut TrunkOut) {
    let e = job.n_ent;
    let c = &net.combat;
    let hid = c.att.out;
    out.att_side = vec![0.0; e * hid];
    c.att.run(&job.side, e, &mut out.att_side);
    out.dfd_side = vec![0.0; e * hid];
    c.dfd.run(&job.side, e, &mut out.dfd_side);
    out.dealt = vec![0.0; e * e];
    let mut h1 = vec![0.0; hid];
    let mut o4 = [0.0f32; 4];
    let mut h2 = vec![0.0; c.l2.out];
    for i in 0..e {
        if !(job.ready[i] || job.hostile[i]) {
            continue;
        }
        for j in 0..e {
            for k in 0..hid {
                h1[k] = out.att_side[i * hid + k] + out.dfd_side[j * hid + k];
            }
            c.run(&h1, &mut o4, &mut h2);
            out.dealt[i * e + j] = o4[0].clamp(0.0, 1.0);
        }
    }
    focus(job, out);
}

/// `focus[j] = Σ_i dealt[i][j]` over the ready `i` within reach of `j`.
pub fn focus(job: &Job, out: &mut TrunkOut) {
    let e = job.n_ent;
    let m = job.map();
    let at = |i: usize| {
        let t = job.ent_tile[i].max(0) as usize;
        (m.q.get(t).copied().unwrap_or(0.0), m.r.get(t).copied().unwrap_or(0.0))
    };
    out.focus = vec![0.0; e];
    for j in 0..e {
        if job.ent_tile[j] < 0 {
            continue;
        }
        let (qj, rj) = at(j);
        let mut s = 0.0f32;
        for i in 0..e {
            if !job.ready[i] {
                continue;
            }
            let (qi, ri) = at(i);
            if hex_distance(qi, ri, qj, rj) as f32 <= job.reach[i] {
                s += out.dealt[i * e + j];
            }
        }
        out.focus[j] = s;
    }
}

/// Softmax in place; −∞ entries get 0.
pub fn softmax(s: &mut [f32]) {
    let m = s.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if m == f32::NEG_INFINITY {
        s.fill(0.0);
        return;
    }
    let mut sum = 0.0;
    for v in s.iter_mut() {
        *v = (*v - m).exp();
        sum += *v;
    }
    let inv = 1.0 / sum;
    for v in s.iter_mut() {
        *v *= inv;
    }
}

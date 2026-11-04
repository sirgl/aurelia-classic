//! A decision of v2 on the CPU (`model_v2.NetV2.score` for one decision):
//! the candidates' encoding, the head's query of the context and the turn
//! trace, the sum heads, the candidate attention to the entity tokens with
//! the combat module's pair features, the heads. It reads the trunk's
//! [`TrunkOut`]; what several decisions of a turn share (a tile's encoding
//! columns, an actor's keys and values) is computed once, when first needed.

use std::collections::HashMap;
use std::sync::Arc;

use super::gemm::linear;
use super::trunk::{pair_bucket, softmax, MapPrep, TrunkOut};
use super::gemm_i8::QLin;
use super::weights::{rms_norm, Lin, Net, CLIP, N_BUCKETS, N_BUCKETS_PAD};

pub const HEAD_UNIT: usize = 0;
pub const HEAD_PRODUCTION: usize = 2;
pub const WAIT_TAG: i32 = 250;
pub const DEFER_TAG: i32 = 251;

/// The int8 layers of a decision (the scorer's `int8` switch): the products
/// over the model width; the small ones (the fields, the combat head, the
/// logit) stay f32.
#[derive(Debug)]
pub struct Int8 {
    head_query: Vec<QLin>,
    trace: QLin,
    cand_row: QLin,
    ca_q: QLin,
    ca_out: QLin,
    head_hidden: Vec<QLin>,
    pair_kv: QLin,
    l2: QLin,
}

impl Int8 {
    pub fn new(net: &Net) -> Self {
        Int8 {
            head_query: net.head_query.iter().map(QLin::new).collect(),
            trace: QLin::new(&net.trace),
            cand_row: QLin::new(&net.cand_row),
            ca_q: QLin::new(&net.ca_q),
            ca_out: QLin::new(&net.ca_out),
            head_hidden: net.head_hidden.iter().map(QLin::new).collect(),
            pair_kv: QLin::new(&net.ca_pair_kv),
            l2: QLin::new(&net.combat.l2),
        }
    }
}

/// `y = x · W + b` in f32, or with the int8 layer when there is one.
fn run(l: &Lin, q: Option<&QLin>, x: &[f32], rows: usize, y: &mut [f32], xq: &mut Vec<i8>) {
    match q {
        Some(q) => q.run(x, rows, y, xq),
        None => l.run(x, rows, y),
    }
}

/// `y += x · W`, f32 or int8.
fn add(l: &Lin, q: Option<&QLin>, x: &[f32], rows: usize, y: &mut [f32], xq: &mut Vec<i8>) {
    match q {
        Some(q) => q.add(x, rows, y, xq),
        None => l.add(x, rows, y),
    }
}

/// An actor's keys and values over the turn's entities, per head: the keys
/// QK-normed and transposed (`[heads × dh × ep]`: column `e` the null key,
/// the columns past it zero, `ep` a multiple of 16 so the score products run
/// in whole vector blocks), the values `[heads × E × dh]`.
#[derive(Debug)]
struct ActorKv {
    k: Vec<f32>,
    v: Vec<f32>,
}

/// The key columns of an actor's keys: E entities and the null key, rounded up to 16.
fn key_cols(e: usize) -> usize {
    (e + 1).next_multiple_of(16)
}

/// A decision's working buffers, kept between the decisions of a session
/// (no allocation per decision once they have grown).
#[derive(Debug, Default)]
struct Scratch {
    ctx: Vec<f32>,
    tr: Vec<f32>,
    q: Vec<f32>,
    rin: Vec<f32>,
    enc: Vec<f32>,
    x: Vec<f32>,
    xn: Vec<f32>,
    qq: Vec<f32>,
    ctile: Vec<i32>,
    cent: Vec<i32>,
    bucket: Vec<u8>,
    ent_pos: Vec<(f32, f32, bool)>,
    danger: Vec<f32>,
    qh: Vec<f32>,
    sc: Vec<f32>,
    p: Vec<f32>,
    qd: Vec<f32>,
    pb: Vec<f32>,
    oh: Vec<f32>,
    row: Vec<f32>,
    o: Vec<f32>,
    add: Vec<f32>,
    hid: Vec<f32>,
    lg: Vec<f32>,
    xq: Vec<i8>,
}

/// `v` resized to `n` (its contents left as they are: each user overwrites them).
fn sized(v: &mut Vec<f32>, n: usize) -> &mut [f32] {
    v.resize(n, 0.0);
    &mut v[..n]
}

/// A turn of a session after its trunk.
#[derive(Debug)]
pub struct TurnState {
    pub out: TrunkOut,
    pub map: Arc<MapPrep>,
    pub n_ent: usize,
    pub ent_tile: Vec<i32>,
    /// Per entity, its slot token (`usize::MAX`: none).
    pub slot_of: Vec<usize>,
    pub hostile: Vec<bool>,
    pub reach: Vec<f32>,
    /// The turn trace: the sum of the chosen candidates' encodings (clamped).
    pub trace: Vec<f32>,
    /// The last decision's encodings (clamped), head and tags, for the trace.
    pub last: Option<(Vec<f32>, usize, Vec<i32>)>,
    tile_enc: HashMap<u32, Vec<f32>>,
    actors: HashMap<i32, ActorKv>,
    scratch: Scratch,
}

impl TurnState {
    pub fn new(net: &Net, out: TrunkOut, job: &super::trunk::Job) -> Self {
        let mut slot_of = vec![usize::MAX; job.n_ent];
        for (s, slot) in job.slots.iter().enumerate() {
            slot_of[slot.entity] = s;
        }
        TurnState {
            out,
            map: job.map.clone().expect("a prepared job"),
            n_ent: job.n_ent,
            ent_tile: job.ent_tile.clone(),
            slot_of,
            hostile: job.hostile.clone(),
            reach: job.reach.clone(),
            trace: vec![0.0; net.cfg.d_model],
            last: None,
            tile_enc: HashMap::new(),
            actors: HashMap::new(),
            scratch: Scratch::default(),
        }
    }

    /// The previous decision took candidate `at` (−1: none): the turn trace.
    pub fn note(&mut self, net: &Net, at: i32) {
        let Some((enc, head, tags)) = &self.last else { return };
        let d = net.cfg.d_model;
        if at < 0 || at as usize >= tags.len() || !net.cfg.trace || (net.cfg.trace_heads >> head) & 1 == 0 {
            return;
        }
        let tag = tags[at as usize];
        if tag == WAIT_TAG || tag == DEFER_TAG {
            return;
        }
        for (s, v) in self.trace.iter_mut().zip(&enc[at as usize * d..(at as usize + 1) * d]) {
            *s += v;
        }
    }

    fn coords(&self, tile: i32) -> (f32, f32, bool) {
        let t = tile.max(0) as usize;
        (self.map.q.get(t).copied().unwrap_or(0.0), self.map.r.get(t).copied().unwrap_or(0.0), tile >= 0)
    }

    fn make_actor_kv(&self, net: &Net, q8: Option<&Int8>, actor: i32, xq: &mut Vec<i8>) -> ActorKv {
        let d = net.cfg.d_model;
        let nh = net.cfg.heads;
        let dh = d / nh;
        let e = self.n_ent;
        let ep = key_cols(e);
        let c = &net.combat;
        let hid = c.att.out;
        let w = c.l2.out;
        let np = 4 + w;
        let mut pair = vec![0.0; e * np];
        if actor >= 0 && e > 0 {
            // The combat module on (the actor, each entity), all rows at once.
            let a = actor as usize;
            let mut h1 = vec![0.0; e * hid];
            let att = &self.out.att_side[a * hid..(a + 1) * hid];
            for (j, h) in h1.chunks_exact_mut(hid).enumerate() {
                for ((v, x), y) in h.iter_mut().zip(att).zip(&self.out.dfd_side[j * hid..(j + 1) * hid]) {
                    *v = (x + y).max(0.0);
                }
            }
            let mut h2 = vec![0.0; e * w];
            run(&c.l2, q8.map(|q| &q.l2), &h1, e, &mut h2, xq);
            for v in &mut h2 {
                *v = v.max(0.0);
            }
            let mut o4 = vec![0.0; e * 4];
            c.head.run(&h2, e, &mut o4);
            for j in 0..e {
                pair[j * np..j * np + 4].copy_from_slice(&o4[j * 4..j * 4 + 4]);
                pair[j * np + 4..(j + 1) * np].copy_from_slice(&h2[j * w..(j + 1) * w]);
            }
        }
        let mut kv = vec![0.0; e * 2 * d];
        run(&net.ca_pair_kv, q8.map(|q| &q.pair_kv), &pair, e, &mut kv, xq);
        for (x, y) in kv.iter_mut().zip(&self.out.ent_kv) {
            *x += y;
        }
        // Per head: the keys QK-normed and transposed [dh × ep] with the null key, the values [E × dh].
        let mut kt = vec![0.0; nh * dh * ep];
        let mut v = vec![0.0; nh * e * dh];
        let mut kr = vec![0.0; dh];
        for j in 0..e {
            for hh in 0..nh {
                kr.copy_from_slice(&kv[j * 2 * d + hh * dh..j * 2 * d + (hh + 1) * dh]);
                rms_norm(&mut kr, &net.ca_kn);
                for (c_, x) in kr.iter().enumerate() {
                    kt[(hh * dh + c_) * ep + j] = *x;
                }
                v[(hh * e + j) * dh..(hh * e + j + 1) * dh].copy_from_slice(&kv[j * 2 * d + d + hh * dh..j * 2 * d + d + (hh + 1) * dh]);
            }
        }
        for hh in 0..nh {
            for c_ in 0..dh {
                kt[(hh * dh + c_) * ep + e] = net.ca_null_k[hh * dh + c_];
            }
        }
        ActorKv { k: kt, v }
    }

    /// A tile's encoding columns (`cand_tile` and the has-tile bias), made when first named.
    fn ensure_tile_enc(&mut self, net: &Net, tile: u32) {
        let dt = net.cfg.d_tile;
        let d = net.cfg.d_model;
        let out = &self.out;
        self.tile_enc.entry(tile).or_insert_with(|| {
            let mut y = vec![0.0; d];
            linear(&out.tile_h[tile as usize * dt..(tile as usize + 1) * dt], 1, dt, &net.cand_tile.wt, d, None, &mut y);
            for (a, b) in y.iter_mut().zip(&net.cand_has_tile) {
                *a += b;
            }
            y
        });
    }

    /// The logits of a decision's candidates (`rows` `[k × c_candidate]`
    /// stored, `meta` `[k]` tag, tile, entity, parameter).
    ///
    /// Every float operation is the one of the straightforward version (each
    /// matrix product's output a fused sum over its input in ascending order,
    /// the same additions in the same order), only the buffers are kept and
    /// the score products padded to whole vector blocks: the same bits.
    /// `q8`: the int8 layers (None: f32).
    #[allow(clippy::too_many_arguments)]
    pub fn score(&mut self, net: &Net, q8: Option<&Int8>, head: usize, actor: i32, rows: &[i16], meta: &[[i32; 4]], logits: &mut Vec<f32>) {
        let cfg = &net.cfg;
        let d = cfg.d_model;
        let nh = cfg.heads;
        let dh = d / nh;
        let k = meta.len();
        let e = self.n_ent;
        let nt = self.map.n_tiles;
        let r = self.map.n_regions;
        let (o_ent, o_glob) = (r, r + e);
        let o_slots = o_glob + 2;
        logits.clear();
        prof::start();
        let mut s = std::mem::take(&mut self.scratch);
        let actor = if actor >= 0 && (actor as usize) < e { actor } else { -1 };
        let a = actor.max(0) as usize;
        // The context: the actor's token and slot, else the global and player tokens.
        let uses_actor = (head == HEAD_UNIT || head == HEAD_PRODUCTION) && actor >= 0;
        let tok = |i: usize| &self.out.tokens[i * d..(i + 1) * d];
        let ctx = sized(&mut s.ctx, d);
        if uses_actor {
            ctx.copy_from_slice(tok(o_ent + a));
            if self.slot_of[a] != usize::MAX {
                for (c, v) in ctx.iter_mut().zip(tok(o_slots + self.slot_of[a])) {
                    *c += v;
                }
            }
        } else {
            for ((c, g), p) in ctx.iter_mut().zip(tok(o_glob)).zip(tok(o_glob + 1)) {
                *c = g + p;
            }
        }
        if cfg.trace {
            let sh = 1.0 / (1u32 << cfg.trace_shift) as f32;
            let tr = sized(&mut s.tr, d);
            for (t, v) in tr.iter_mut().zip(&self.trace) {
                *t = (v * sh).clamp(-CLIP, CLIP);
            }
            add(&net.trace, q8.map(|q| &q.trace), tr, 1, ctx, &mut s.xq);
        }
        let q = sized(&mut s.q, d);
        run(&net.head_query[head], q8.map(|q| &q.head_query[head]), ctx, 1, q, &mut s.xq);
        prof::mark(0);
        // The candidates' encodings.
        let cc = cfg.c_candidate;
        let rn = net.norm_candidate.width();
        let rin = sized(&mut s.rin, k * rn);
        for i in 0..k {
            net.norm_candidate.row(&rows[i * cc..(i + 1) * cc], &mut rin[i * rn..(i + 1) * rn]);
        }
        let enc = sized(&mut s.enc, k * d);
        match q8 {
            Some(q) => {
                for row in enc.chunks_exact_mut(d) {
                    row.copy_from_slice(&net.cand_bias[..d]);
                }
                q.cand_row.add(rin, k, enc, &mut s.xq);
            }
            None => linear(rin, k, rn, &net.cand_row.wt, d, Some(&net.cand_bias), enc),
        }
        let emb_head = head.min(cfg.n_decision_heads - 1);
        s.ctile.clear();
        s.cent.clear();
        for m in meta {
            let tile = m[1] >= 0 && (m[1] as usize) < nt;
            s.ctile.push(if tile { m[1] } else { -1 });
            s.cent.push(if m[2] >= 0 && (m[2] as usize) < e { m[2] } else { -1 });
            if tile {
                self.ensure_tile_enc(net, m[1] as u32);
            }
        }
        for (i, m) in meta.iter().enumerate() {
            let tb = if m[0] < 0 { 0 } else { 1 + m[0].rem_euclid(cfg.tag_buckets as i32 - 1) as usize };
            let pb = if m[3] < 0 { 0 } else { 1 + m[3].rem_euclid(cfg.param_buckets as i32 - 1) as usize };
            let row = &mut enc[i * d..(i + 1) * d];
            let tt = &net.cand_tag[(emb_head * cfg.tag_buckets + tb) * d..][..d];
            let pt = &net.cand_param[(emb_head * cfg.param_buckets + pb) * d..][..d];
            for c in 0..d {
                row[c] += tt[c] + pt[c];
            }
            if s.ctile[i] >= 0 {
                let te = &self.tile_enc[&(s.ctile[i] as u32)];
                for c in 0..d {
                    row[c] += te[c];
                }
            }
            if s.cent[i] >= 0 {
                let pe = &self.out.p_ent[s.cent[i] as usize * d..][..d];
                for c in 0..d {
                    row[c] += pe[c] + net.cand_has_ent[c];
                }
            }
        }
        prof::mark(1);
        // x = enc + the head's query.
        let x = sized(&mut s.x, k * d);
        for (xr, er) in x.chunks_exact_mut(d).zip(enc.chunks_exact(d)) {
            for ((xv, ev), qv) in xr.iter_mut().zip(er).zip(q.iter()) {
                *xv = ev + qv;
            }
        }
        prof::mark(2);
        // The candidate's tile (its target tile, else the actor's) and its distance buckets to the entities.
        let actor_tile = if actor >= 0 { self.ent_tile[a] } else { -1 };
        s.bucket.clear();
        s.bucket.resize(k * e, 0);
        s.ent_pos.clear();
        s.ent_pos.extend((0..e).map(|j| self.coords(self.ent_tile[j])));
        let danger = sized(&mut s.danger, k);
        for i in 0..k {
            let p = if s.ctile[i] >= 0 { s.ctile[i] } else { actor_tile };
            let pp = self.coords(p);
            for (b, ep_) in s.bucket[i * e..(i + 1) * e].iter_mut().zip(&s.ent_pos) {
                *b = pair_bucket(pp, *ep_) as u8;
            }
            // Danger: the hostile entities within their reach of the tile, their blows on the actor.
            danger[i] = 0.0;
            if p >= 0 && actor >= 0 {
                let mut sum = 0.0f32;
                for (j, ep_) in s.ent_pos.iter().enumerate() {
                    if self.hostile[j] && (super::weights::hex_distance(pp.0, pp.1, ep_.0, ep_.1) as f32) <= self.reach[j] {
                        sum += self.out.dealt[j * e + a];
                    }
                }
                danger[i] = sum;
            }
        }
        for i in 0..k {
            let has_e = s.cent[i] >= 0;
            let f = if has_e { self.out.focus[s.cent[i] as usize] } else { 0.0 };
            let fin = [f.min(4.0), if f >= 1.0 && has_e { 1.0 } else { 0.0 }, danger[i].min(4.0), if danger[i] >= 1.0 { 1.0 } else { 0.0 }];
            net.fields_in.add(&fin, 1, &mut x[i * d..(i + 1) * d]);
            for c in 0..d {
                x[i * d + c] += net.fields_in.b[c];
            }
        }
        prof::mark(3);
        // The candidate attention.
        let xn = sized(&mut s.xn, k * d);
        xn.copy_from_slice(x);
        for xr in xn.chunks_exact_mut(d) {
            rms_norm(xr, &net.ca_norm);
        }
        let qq = sized(&mut s.qq, k * d);
        run(&net.ca_q, q8.map(|q| &q.ca_q), xn, k, qq, &mut s.xq);
        for hr in qq.chunks_exact_mut(dh) {
            rms_norm(hr, &net.ca_qn);
        }
        prof::mark(4);
        if !self.actors.contains_key(&actor) {
            let kv = self.make_actor_kv(net, q8, actor, &mut s.xq);
            self.actors.insert(actor, kv);
        }
        let akv = &self.actors[&actor];
        prof::mark(5);
        let scale = 1.0 / (dh as f32).sqrt();
        let ep = key_cols(e);
        let o = sized(&mut s.o, k * d);
        // Per head, as matrix products: the scores [K × ep] (column e: the null
        // key), the distance keys [K × 16], the softmax, the values and the distance values.
        let qh = sized(&mut s.qh, k * dh);
        let sc = sized(&mut s.sc, k * ep);
        let pr = sized(&mut s.p, k * e);
        let qd = sized(&mut s.qd, k * N_BUCKETS_PAD);
        let pbk = sized(&mut s.pb, k * N_BUCKETS);
        let oh = sized(&mut s.oh, k * dh);
        let row = sized(&mut s.row, e + 1);
        for hh in 0..nh {
            for i in 0..k {
                qh[i * dh..(i + 1) * dh].copy_from_slice(&qq[i * d + hh * dh..i * d + (hh + 1) * dh]);
            }
            linear(qh, k, dh, &akv.k[hh * dh * ep..(hh + 1) * dh * ep], ep, None, sc);
            linear(qh, k, dh, &net.ca_dist_kt[hh * dh * N_BUCKETS_PAD..(hh + 1) * dh * N_BUCKETS_PAD], N_BUCKETS_PAD, None, qd);
            let bias = &net.ca_dist_bias[hh * N_BUCKETS..(hh + 1) * N_BUCKETS];
            pbk.fill(0.0);
            let null_v = &net.ca_null_v[hh * dh..(hh + 1) * dh];
            for i in 0..k {
                let si = &sc[i * ep..(i + 1) * ep];
                let qdi = &qd[i * N_BUCKETS_PAD..(i + 1) * N_BUCKETS_PAD];
                let bi = &s.bucket[i * e..(i + 1) * e];
                row[0] = si[e] * scale;
                for j in 0..e {
                    let b = bi[j] as usize;
                    row[1 + j] = si[j] * scale + qdi[b] * scale + bias[b];
                }
                softmax(row);
                for (oc, nv) in oh[i * dh..(i + 1) * dh].iter_mut().zip(null_v) {
                    *oc = row[0] * nv;
                }
                let pbi = &mut pbk[i * N_BUCKETS..(i + 1) * N_BUCKETS];
                for j in 0..e {
                    pr[i * e + j] = row[1 + j];
                    pbi[bi[j] as usize] += row[1 + j];
                }
            }
            super::gemm::linear_acc(pr, k, e, &akv.v[hh * e * dh..(hh + 1) * e * dh], dh, oh);
            super::gemm::linear_acc(pbk, k, N_BUCKETS, &net.ca_dist_v[hh * N_BUCKETS * dh..(hh + 1) * N_BUCKETS * dh], dh, oh);
            for i in 0..k {
                o[i * d + hh * dh..i * d + (hh + 1) * dh].copy_from_slice(&oh[i * dh..(i + 1) * dh]);
            }
        }
        prof::mark(6);
        let att = sized(&mut s.add, k * d);
        run(&net.ca_out, q8.map(|q| &q.ca_out), o, k, att, &mut s.xq);
        for (xv, av) in x.iter_mut().zip(att.iter()) {
            *xv += av;
        }
        for xr in x.chunks_exact_mut(d) {
            rms_norm(xr, &net.cand_out_norm);
        }
        prof::mark(7);
        let hdim = cfg.d_head;
        let hid = sized(&mut s.hid, k * hdim);
        run(&net.head_hidden[head], q8.map(|q| &q.head_hidden[head]), x, k, hid, &mut s.xq);
        for v in hid.iter_mut() {
            *v = v.max(0.0);
        }
        let lg = sized(&mut s.lg, k);
        net.head_logit[head].run(hid, k, lg);
        for i in 0..k {
            let mut l = lg[i];
            if cfg.defer && head == HEAD_UNIT && meta[i][0] == DEFER_TAG {
                l += net.defer_bias;
            }
            logits.push(l);
        }
        // The encodings (clamped) go to the trace; the previous ones' buffer comes back for the next decision.
        let mut enc = std::mem::take(&mut s.enc);
        enc.truncate(k * d);
        for v in &mut enc {
            *v = v.clamp(-CLIP, CLIP);
        }
        let mut tags = self.last.take().map(|(old, _, mut tags)| {
            s.enc = old;
            tags.clear();
            tags
        }).unwrap_or_default();
        tags.extend(meta.iter().map(|m| m[0]));
        self.last = Some((enc, head, tags));
        self.scratch = s;
        prof::mark(8);
    }
}

/// `AURELIA_V2_DECIDE_PROFILE`: time per section of `score` (a measurement).
pub mod prof {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::OnceLock;
    use std::time::Instant;

    pub static NS: [AtomicU64; 9] = [const { AtomicU64::new(0) }; 9];
    thread_local! {
        static LAST: std::cell::Cell<Option<Instant>> = const { std::cell::Cell::new(None) };
    }

    fn on() -> bool {
        static ON: OnceLock<bool> = OnceLock::new();
        *ON.get_or_init(|| std::env::var_os("AURELIA_V2_DECIDE_PROFILE").is_some())
    }

    pub fn start() {
        if on() {
            LAST.with(|l| l.set(Some(Instant::now())));
        }
    }

    pub fn mark(i: usize) {
        if on() {
            LAST.with(|l| {
                let now = Instant::now();
                if let Some(t) = l.get() {
                    NS[i].fetch_add((now - t).as_nanos() as u64, Ordering::Relaxed);
                }
                l.set(Some(now));
            });
        }
    }

    /// The sections' milliseconds: context, encodings, query, buckets and
    /// fields, attention query, actor keys, scores, output, heads.
    pub fn report() -> Vec<f64> {
        NS.iter().map(|n| n.load(Ordering::Relaxed) as f64 / 1e6).collect()
    }
}

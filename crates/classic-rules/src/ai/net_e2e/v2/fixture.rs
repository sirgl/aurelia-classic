//! Recorded positions with PyTorch's logits (`python -m aurelia_nn.export_v2
//! fixture`), replayed through a [`Scorer`]: the parity check of v2 in
//! process against the offline evaluation (`v2_bench eval`) and the remote
//! player's server.

use std::path::Path;

use crate::ai::net_e2e::encode::Candidate;
use crate::ai::net_e2e::mind::Scorer;
use crate::ai::net_e2e::model::{TurnCache, TurnInput};

const MAGIC: &[u8; 4] = b"AV2X";

/// A recorded decision.
#[derive(Debug)]
pub struct Decision {
    pub head: usize,
    pub actor: i32,
    pub candidates: Vec<Candidate>,
    /// The offline evaluation's logits.
    pub offline: Vec<f32>,
    /// The remote server's (Q8).
    pub server: Vec<i32>,
    pub choice: i32,
}

/// A recorded turn: the BEGIN request's tables.
#[derive(Debug, Default)]
pub struct Turn {
    pub neighbors: Vec<i32>,
    pub region: Vec<i32>,
    pub tiles: Vec<i16>,
    pub global: Vec<i16>,
    pub entities: Vec<i16>,
    pub entity_meta: Vec<i32>,
    pub entity_ids: Vec<i32>,
    pub events: Vec<i16>,
    pub event_meta: Vec<i32>,
    pub server_value: i32,
    pub decisions: Vec<Decision>,
}

impl Turn {
    pub fn input(&self) -> TurnInput<'_> {
        TurnInput {
            tiles: &self.tiles,
            neighbors: &self.neighbors,
            region: &self.region,
            global: &self.global,
            entities: &self.entities,
            entity_meta: &self.entity_meta,
            entity_ids: &self.entity_ids,
            coarse_region: &[],
            aim: &[],
            events: &self.events,
            event_meta: &self.event_meta,
        }
    }
}

struct R<'a>(&'a [u8]);

impl R<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        if self.0.len() < n {
            return Err("a short fixture".into());
        }
        let (a, b) = self.0.split_at(n);
        self.0 = b;
        Ok(a)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("4")))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().expect("4")))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("8")))
    }
    fn i16s(&mut self, n: usize) -> Result<Vec<i16>, String> {
        Ok(self.take(2 * n)?.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect())
    }
    fn i32s(&mut self, n: usize) -> Result<Vec<i32>, String> {
        Ok(self.take(4 * n)?.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().expect("4"))).collect())
    }
    fn f32s(&mut self, n: usize) -> Result<Vec<f32>, String> {
        Ok(self.take(4 * n)?.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().expect("4"))).collect())
    }
}

/// The column counts of the net the fixture was made for.
#[derive(Debug, Clone, Copy)]
pub struct Widths {
    pub c_tile: usize,
    pub c_global: usize,
    pub c_entity: usize,
    pub c_candidate: usize,
    pub c_event: usize,
}

/// Windows of turns.
pub fn load(path: &Path, w: Widths) -> Result<Vec<Vec<Turn>>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut r = R(&bytes);
    if r.take(4)? != MAGIC {
        return Err(format!("{}: not a v2 fixture", path.display()));
    }
    let _version = r.u32()?;
    let n_windows = r.u32()? as usize;
    let mut windows = Vec::with_capacity(n_windows);
    for _ in 0..n_windows {
        let n_turns = r.u32()? as usize;
        let mut turns = Vec::with_capacity(n_turns);
        for _ in 0..n_turns {
            let len = r.u32()? as usize;
            let mut b = R(r.take(len)?);
            let _kind = b.u8()?;
            let _session = b.u64()?;
            let _key = b.u64()?;
            let nt = b.u32()? as usize;
            let mut t = Turn::default();
            if b.u8()? != 0 {
                t.neighbors = b.i32s(nt * 6)?;
                t.region = b.i32s(nt)?;
            }
            t.tiles = b.i16s(nt * w.c_tile)?;
            t.global = b.i16s(w.c_global)?;
            let e = b.u32()? as usize;
            t.entities = b.i16s(e * w.c_entity)?;
            t.entity_meta = b.i32s(e * 3)?;
            t.entity_ids = b.i32s(e)?;
            let v = b.u32()? as usize;
            t.events = b.i16s(v * w.c_event)?;
            t.event_meta = b.i32s(v * 4)?;
            t.server_value = r.i32()?;
            let n_dec = r.u32()? as usize;
            for _ in 0..n_dec {
                let len = r.u32()? as usize;
                let mut b = R(r.take(len)?);
                let _kind = b.u8()?;
                let _session = b.u64()?;
                let _note = b.i32()?;
                let head = b.u32()? as usize;
                let actor = b.i32()?;
                let k = b.u32()? as usize;
                let rows = b.i16s(k * w.c_candidate)?;
                let meta = b.i32s(k * 4)?;
                let candidates = (0..k)
                    .map(|i| Candidate {
                        row: rows[i * w.c_candidate..(i + 1) * w.c_candidate].to_vec(),
                        meta: [meta[i * 4], meta[i * 4 + 1], meta[i * 4 + 2], meta[i * 4 + 3]],
                        command: None,
                        targets: Vec::new(),
                    })
                    .collect();
                let k2 = r.u32()? as usize;
                let offline = r.f32s(k2)?;
                let server = r.i32s(k2)?;
                let choice = r.i32()?;
                t.decisions.push(Decision { head, actor, candidates, offline, server, choice });
            }
            turns.push(t);
        }
        windows.push(turns);
    }
    Ok(windows)
}

fn argmax_f(v: &[f32]) -> usize {
    (0..v.len()).fold(0, |b, i| if v[i] > v[b] { i } else { b })
}

fn argmax_i(v: &[i32]) -> usize {
    (0..v.len()).fold(0, |b, i| if v[i] > v[b] { i } else { b })
}

/// What a replay found.
#[derive(Debug, Default, Clone)]
pub struct Report {
    pub windows: usize,
    pub turns: usize,
    pub decisions: usize,
    /// The same candidate as the offline evaluation's argmax.
    pub agree_offline: usize,
    /// The same candidate as the remote server's.
    pub agree_server: usize,
    /// Of the disagreements with the offline argmax, those where the
    /// offline logits' top two lie within 2 Q8 steps (a near tie).
    pub near_ties: usize,
    /// max |logit − offline logit| (the scorer's raw logits when it gives
    /// them, else Q8 / 256).
    pub max_abs_diff: f32,
    pub mean_abs_diff: f64,
    /// max |value − the server's value| (Q8).
    pub max_value_diff: i32,
    /// Per head: decisions, agreeing with the offline argmax.
    pub by_head: Vec<[usize; 2]>,
    pub begin_ms: f64,
    pub score_ms: f64,
}

/// Replays the windows through `scorer`, the turn trace forced by the
/// recorded choices; `raw` gives the last scored decision's float logits.
pub fn replay(windows: &[Vec<Turn>], scorer: &dyn Scorer, raw: &dyn Fn(&TurnCache) -> Option<Vec<f32>>) -> Report {
    let mut rep = Report { windows: windows.len(), by_head: vec![[0; 2]; 8], ..Default::default() };
    let mut sum_abs = 0.0f64;
    let mut n_abs = 0usize;
    let mut out = Vec::new();
    for w in windows {
        let mut cache = TurnCache::new();
        let first = &w[0];
        for t in w {
            // A turn's map tables: the window's first (the fixture sends them every turn).
            let input = if t.neighbors.is_empty() { TurnInput { neighbors: &first.neighbors, region: &first.region, ..t.input() } } else { t.input() };
            let s = super::now_ns();
            scorer.begin_turn(&input, &crate::ai::net_e2e::model::Memory::default(), &mut cache);
            rep.begin_ms += (super::now_ns() - s) as f64 / 1e6;
            rep.max_value_diff = rep.max_value_diff.max((cache.value() - t.server_value).abs());
            rep.turns += 1;
            let mut prev: Option<&Decision> = None;
            for d in &t.decisions {
                if let Some(p) = prev {
                    if p.choice >= 0 && (p.choice as usize) < p.candidates.len() {
                        scorer.note_choice(&mut cache, p.head, p.actor, &p.candidates[p.choice as usize]);
                    }
                }
                let s = super::now_ns();
                scorer.score(&cache, d.head, d.actor, &d.candidates, &mut out);
                rep.score_ms += (super::now_ns() - s) as f64 / 1e6;
                let k = d.candidates.len();
                let got = raw(&cache).unwrap_or_else(|| out.iter().map(|&v| v as f32 / 256.0).collect());
                let a_ref = argmax_f(&d.offline[..k]);
                let a_got = argmax_i(&out);
                rep.decisions += 1;
                rep.agree_offline += usize::from(a_ref == a_got);
                rep.agree_server += usize::from(argmax_i(&d.server) == a_got);
                let h = &mut rep.by_head[d.head.min(7)];
                h[0] += 1;
                h[1] += usize::from(a_ref == a_got);
                if a_ref != a_got {
                    let mut srt = d.offline[..k].to_vec();
                    srt.sort_by(|a, b| b.total_cmp(a));
                    if k > 1 && srt[0] - srt[1] < 2.0 / 256.0 {
                        rep.near_ties += 1;
                    }
                }
                for (g, o) in got.iter().zip(&d.offline) {
                    let diff = (g - o).abs();
                    rep.max_abs_diff = rep.max_abs_diff.max(diff);
                    sum_abs += f64::from(diff);
                    n_abs += 1;
                }
                prev = Some(d);
            }
        }
    }
    rep.mean_abs_diff = sum_abs / n_abs.max(1) as f64;
    rep
}

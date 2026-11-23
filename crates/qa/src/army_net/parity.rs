//! `ai-exam army-parity --plays FILE --weights FILE --out FILE [--limit N]`: the Rust forward on recorded
//! decisions (`record` format), one JSON line each: the pointer's logits (null where not selectable, the
//! end's last) and, when the decision names a unit, the action logits of that unit. `army/parity.py`
//! compares them with MLX's. Also the forward time per decision on this one thread.

use std::io::Write;
use std::time::Instant;

use classic_rules::ids::TileId;

use super::model::Net;
use super::observe::{f16_to_f32, Action, Candidate, Tokens, F_ACT, F_TOK};
use super::record::{DECISION_MAGIC, MEMORY_STATE, PLAY_MAGIC, PLAY_MAGIC3, PLAY_MAGIC4, TRACK};

/// One recorded decision, as the net reads it.
pub struct Recorded {
    /// The play's static planes (shared by its decisions).
    pub statics: std::rc::Rc<Vec<f32>>,
    pub play: usize,
    pub round: u16,
    pub tokens: Tokens,
    pub candidates: Vec<Candidate>,
    pub pointer: i16,
    pub action: i16,
    /// Record v3: the decision's flags (`MEMORY_STATE`), each token's army slot (-1 none), the teacher's state.
    pub flags: u16,
    pub slots: Vec<i16>,
    pub track: [i16; TRACK],
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let s = self.b.get(self.at..self.at + n).ok_or("the file ends inside a record")?;
        self.at += n;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16, String> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, String> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn f16s(&mut self, n: usize) -> Result<Vec<f32>, String> {
        (0..n).map(|_| self.u16().map(f16_to_f32)).collect()
    }
}

/// The decisions of a recorded file, at most `limit`.
pub fn read(bytes: &[u8], limit: usize) -> Result<Vec<Recorded>, String> {
    let mut r = Reader { b: bytes, at: 0 };
    let mut out = Vec::new();
    while r.at < bytes.len() && out.len() < limit {
        let magic = r.u32()?;
        if magic != PLAY_MAGIC && magic != PLAY_MAGIC3 && magic != PLAY_MAGIC4 {
            return Err(format!("no play at {}", r.at - 4));
        }
        let v4 = magic == PLAY_MAGIC4;
        let v3 = magic == PLAY_MAGIC3 || v4;
        let jl = r.u32()? as usize;
        r.take(jl)?;
        let statics = std::rc::Rc::new(r.f16s(super::W * super::W * super::S_CH)?);
        let play = out.last().map_or(0, |d: &Recorded| d.play + 1);
        let n = r.u32()?;
        for _ in 0..n {
            if r.u32()? != DECISION_MAGIC {
                return Err(format!("no decision at {}", r.at - 4));
            }
            let t = r.u16()? as usize;
            let c = r.u16()? as usize;
            let pointer = r.u16()? as i16;
            let action = r.u16()? as i16;
            let round = r.u16()?;
            let flags = r.u16()?;
            let tf = r.f16s(t * F_TOK)?;
            let dist = r.take(t * t)?.to_vec();
            let selectable: Vec<bool> = r.take(t)?.iter().map(|x| *x != 0).collect();
            let _danger = r.f16s(t)?;
            let cf = r.f16s(c * F_ACT)?;
            let promos: Vec<u16> = (0..c).map(|_| r.u16()).collect::<Result<_, _>>()?;
            let pos: Vec<i16> = (0..t).map(|_| r.u16().map(|x| x as i16)).collect::<Result<_, _>>()?;
            let cpos: Vec<i16> = (0..c).map(|_| r.u16().map(|x| x as i16)).collect::<Result<_, _>>()?;
            let (slots, track) = if v3 {
                let slots: Vec<i16> = (0..t).map(|_| r.u16().map(|x| x as i16)).collect::<Result<_, _>>()?;
                let mut track = [0i16; TRACK];
                for x in track.iter_mut() {
                    *x = r.u16()? as i16;
                }
                (slots, track)
            } else {
                (vec![-1; t], [-1; TRACK])
            };
            let flags = if v3 { flags } else { 0 };
            let (mut hist, mut hist_slot, mut hist_age) = (Vec::new(), Vec::new(), Vec::new());
            if v4 {
                let h = r.u16()? as usize;
                let hf = r.f16s(h * super::observe::F_HIST)?;
                hist = hf.chunks_exact(super::observe::F_HIST).map(|x| x.try_into().expect("chunks of F_HIST")).collect();
                hist_slot = (0..h).map(|_| r.u16().map(|x| x as i16)).collect::<Result<_, _>>()?;
                hist_age = r.take(h)?.to_vec();
            }
            let feat: Vec<[f32; F_TOK]> = tf.chunks_exact(F_TOK).map(|x| x.try_into().expect("chunks of F_TOK")).collect();
            let tokens = Tokens { feat, dist, selectable, unit: vec![None; t], tile: vec![TileId(0); t], pos, hist, hist_slot, hist_age };
            let candidates = cf.chunks_exact(F_ACT).zip(promos).zip(cpos).map(|((x, promo), pos)| Candidate { feat: x.try_into().expect("chunks of F_ACT"), promo, pos, action: Action::Done }).collect();
            if out.len() < limit {
                out.push(Recorded { statics: statics.clone(), play, round, tokens, candidates, pointer, action, flags, slots, track });
            }
        }
    }
    Ok(out)
}

pub fn main(args: &[String]) -> Result<(), String> {
    let mut plays = None;
    let mut weights = None;
    let mut out = None;
    let mut limit = usize::MAX;
    // Variant B: also encode every decision with the scalar encoder the kernels replaced, and compare.
    let mut map_reference = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut v = || it.next().cloned().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--plays" => plays = Some(v()?),
            "--weights" => weights = Some(v()?),
            "--out" => out = Some(v()?),
            "--limit" => limit = v()?.parse().map_err(|e| format!("{e}"))?,
            "--map-reference" => map_reference = true,
            other => return Err(format!("unknown option {other}")),
        }
    }
    let net = Net::load(std::path::Path::new(&weights.ok_or("--weights")?))?;
    let bytes = std::fs::read(plays.ok_or("--plays")?).map_err(|e| e.to_string())?;
    let decisions = read(&bytes, limit)?;
    let mut file = std::io::BufWriter::new(std::fs::File::create(out.ok_or("--out")?).map_err(|e| e.to_string())?);
    let mut forward_ns = 0u128;
    let mut encode_ns = 0u128;
    let mut update_ns = 0u128;
    let mut updates = 0u64;
    let mut update_worst = 0f32;
    let mut update_rel = 0f32;
    let mut update_exact = 0u64;
    let mut reference_rel = 0f32;
    let mut encode_each: Vec<u64> = Vec::new();
    let mut update_each: Vec<u64> = Vec::new();
    let mut unit_decisions = 0u64;
    // Variant B: the map of the previous decision of the same play and round, updated incrementally, is
    // checked against the map encoded whole.
    let mut prev: Option<(usize, u16, super::model::MapState)> = None;
    // Variant M: the memory of each play is carried in the file's order and updated at its memory states.
    let mut memory: Option<(usize, super::model::MemoryState)> = None;
    let mut memory_ns = 0u128;
    let mut memory_steps = 0u64;
    for d in &decisions {
        if net.has_memory() && memory.as_ref().is_none_or(|(p, _)| *p != d.play) {
            memory = net.initial_memory().map(|m| (d.play, m));
        }
        let map = if net.has_map() {
            let planes = super::model::planes(&d.statics, &d.tokens);
            let te = Instant::now();
            let full = net.map_full(planes.clone());
            let ns = te.elapsed().as_nanos();
            encode_ns += ns;
            encode_each.push(ns as u64);
            if let (true, Some(f)) = (map_reference, full.as_ref()) {
                if let Some(r) = net.map_reference(planes.clone()) {
                    reference_rel = reference_rel.max(f.max_rel_diff(&r));
                }
            }
            if let (Some((p, r, mut st)), Some(f)) = (prev.take(), full.as_ref()) {
                if p == d.play && r == d.round {
                    let tu = Instant::now();
                    net.map_update(&mut st, planes);
                    let ns = tu.elapsed().as_nanos();
                    update_ns += ns;
                    update_each.push(ns as u64);
                    updates += 1;
                    update_worst = update_worst.max(st.max_abs_diff(f));
                    update_rel = update_rel.max(st.max_rel_diff(f));
                    update_exact += u64::from(st.stages == f.stages);
                }
            }
            prev = full.clone().map(|f| (d.play, d.round, f));
            full
        } else {
            None
        };
        let t0 = Instant::now();
        let mem_in = memory.as_ref().map(|(_, m)| super::model::MemIn { state: m, slots: &d.slots });
        let body = net.body(&d.tokens, map.as_ref(), mem_in.as_ref());
        let (ptr, end) = net.pointer(&body, &d.tokens);
        let acts = if d.pointer >= 0 { net.actions(&body, d.pointer as usize, &d.candidates, map.as_ref()) } else { Vec::new() };
        forward_ns += t0.elapsed().as_nanos();
        if d.flags & MEMORY_STATE != 0 {
            if let Some((_, m)) = memory.as_mut() {
                let tm = Instant::now();
                net.memory_step(&body, &d.tokens, &d.slots, m);
                memory_ns += tm.elapsed().as_nanos();
                memory_steps += 1;
            }
        }
        if d.pointer >= 0 {
            unit_decisions += 1;
        }
        let ptr: Vec<serde_json::Value> = ptr.iter().map(|x| x.map_or(serde_json::Value::Null, |v| serde_json::json!(v))).chain(std::iter::once(serde_json::json!(end))).collect();
        writeln!(file, "{}", serde_json::json!({"ptr": ptr, "act": acts, "label_ptr": d.pointer, "label_act": d.action})).map_err(|e| e.to_string())?;
    }
    let n = decisions.len().max(1) as f64;
    println!("{} decisions ({} with a unit), forward {:.1} us per decision (body + pointer + its unit's scorer), one thread", decisions.len(), unit_decisions, forward_ns as f64 / 1000.0 / n);
    if net.has_memory() {
        println!("memory: {memory_steps} updates, {:.1} us each (one thread)", memory_ns as f64 / 1000.0 / memory_steps.max(1) as f64);
    }
    if net.has_map() {
        println!("map: whole encode {:.2} ms; incremental update {:.2} ms mean over {updates} consecutive decisions, max |incremental - whole| {update_worst:e}", encode_ns as f64 / 1e6 / n, update_ns as f64 / 1e6 / updates.max(1) as f64);
        let q = |v: &mut Vec<u64>, f: f64| -> f64 {
            v.sort_unstable();
            v.get(((v.len() as f64 - 1.0) * f).round() as usize).map_or(0.0, |x| *x as f64 / 1e6)
        };
        println!("map: whole encode ms p50 {:.3} p95 {:.3}; update ms p50 {:.3} p95 {:.3}; update relative to whole {update_rel:e}, bit-equal {update_exact} of {updates}", q(&mut encode_each, 0.5), q(&mut encode_each, 0.95), q(&mut update_each, 0.5), q(&mut update_each, 0.95));
        if map_reference {
            println!("map: whole encode relative to the scalar encoder (max over stages and decisions) {reference_rel:e}");
        }
    }
    Ok(())
}

//! `ai-exam army-parity --plays FILE --weights FILE --out FILE [--limit N]`: the Rust forward on recorded
//! decisions (`record` format), one JSON line each: the pointer's logits (null where not selectable, the
//! end's last) and, when the decision names a unit, the action logits of that unit. `army/parity.py`
//! compares them with MLX's. Also the forward time per decision on this one thread.

use std::io::Write;
use std::time::Instant;

use classic_rules::ids::TileId;

use super::model::Net;
use super::observe::{f16_to_f32, Action, Candidate, Tokens, F_ACT, F_TOK};
use super::record::{DECISION_MAGIC, PLAY_MAGIC};

/// One recorded decision, as the net reads it.
pub struct Recorded {
    pub tokens: Tokens,
    pub candidates: Vec<Candidate>,
    pub pointer: i16,
    pub action: i16,
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
        if r.u32()? != PLAY_MAGIC {
            return Err(format!("no play at {}", r.at - 4));
        }
        let jl = r.u32()? as usize;
        r.take(jl)?;
        let n = r.u32()?;
        for _ in 0..n {
            if r.u32()? != DECISION_MAGIC {
                return Err(format!("no decision at {}", r.at - 4));
            }
            let t = r.u16()? as usize;
            let c = r.u16()? as usize;
            let pointer = r.u16()? as i16;
            let action = r.u16()? as i16;
            let _round = r.u16()?;
            let _zero = r.u16()?;
            let tf = r.f16s(t * F_TOK)?;
            let dist = r.take(t * t)?.to_vec();
            let selectable: Vec<bool> = r.take(t)?.iter().map(|x| *x != 0).collect();
            let _danger = r.f16s(t)?;
            let cf = r.f16s(c * F_ACT)?;
            let promos: Vec<u16> = (0..c).map(|_| r.u16()).collect::<Result<_, _>>()?;
            let feat: Vec<[f32; F_TOK]> = tf.chunks_exact(F_TOK).map(|x| x.try_into().expect("chunks of F_TOK")).collect();
            let tokens = Tokens { feat, dist, selectable, unit: vec![None; t], tile: vec![TileId(0); t] };
            let candidates = cf.chunks_exact(F_ACT).zip(promos).map(|(x, promo)| Candidate { feat: x.try_into().expect("chunks of F_ACT"), promo, action: Action::Done }).collect();
            if out.len() < limit {
                out.push(Recorded { tokens, candidates, pointer, action });
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
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut v = || it.next().cloned().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--plays" => plays = Some(v()?),
            "--weights" => weights = Some(v()?),
            "--out" => out = Some(v()?),
            "--limit" => limit = v()?.parse().map_err(|e| format!("{e}"))?,
            other => return Err(format!("unknown option {other}")),
        }
    }
    let net = Net::load(std::path::Path::new(&weights.ok_or("--weights")?))?;
    let bytes = std::fs::read(plays.ok_or("--plays")?).map_err(|e| e.to_string())?;
    let decisions = read(&bytes, limit)?;
    let mut file = std::io::BufWriter::new(std::fs::File::create(out.ok_or("--out")?).map_err(|e| e.to_string())?);
    let mut forward_ns = 0u128;
    let mut unit_decisions = 0u64;
    for d in &decisions {
        let t0 = Instant::now();
        let body = net.body(&d.tokens);
        let (ptr, end) = net.pointer(&body, &d.tokens);
        let acts = if d.pointer >= 0 { net.actions(&body, d.pointer as usize, &d.candidates) } else { Vec::new() };
        forward_ns += t0.elapsed().as_nanos();
        if d.pointer >= 0 {
            unit_decisions += 1;
        }
        let ptr: Vec<serde_json::Value> = ptr.iter().map(|x| x.map_or(serde_json::Value::Null, |v| serde_json::json!(v))).chain(std::iter::once(serde_json::json!(end))).collect();
        writeln!(file, "{}", serde_json::json!({"ptr": ptr, "act": acts, "label_ptr": d.pointer, "label_act": d.action})).map_err(|e| e.to_string())?;
    }
    let n = decisions.len().max(1) as f64;
    println!("{} decisions ({} with a unit), forward {:.1} us per decision (body + pointer + its unit's scorer), one thread", decisions.len(), unit_decisions, forward_ns as f64 / 1000.0 / n);
    Ok(())
}

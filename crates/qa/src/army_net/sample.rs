//! Opt-in SAMPLING of the army net's decisions (the Rust actor of an actor-learner RL; a MEASUREMENT / training tool).
//!
//! Off by default: without `ARMY_SAMPLE_PLAN` nothing here runs and the net plays greedy as before. With
//! `ARMY_SAMPLE_PLAN=FILE` (JSON `{"seed": u64, "copies": G, "plan": {"family/era/instance/attempt": [rounds]}}`) the player
//! `army-net` of `ai-exam suite` plays each scene of the plan G times (the copies share the scene and its combat stream and
//! differ only by their draws); in the rounds the plan lists for the scene (relative to the scene's start round) the net
//! SAMPLES its pick (softmax over the selectable units and the end, temperature 1) and the unit's action (softmax over the legal
//! actions), the other rounds are greedy: the same scheme as `armyrl` (nn/aurelia_nn/tacsim/armyrl.py) in the MLX twin.
//!
//! What it records (with `--record FILE`): the play in the army-net recorder's format (`record.rs`): per turn the first moment
//! (unlabelled) and the turn-end memory state, and in a sampled turn every decision with its pick and action; the play's JSON
//! header gets `copy` and `sample` = [[decision index, log-prob of the pick, log-prob of the action or null], ...] (Rust's own
//! log-probs, f32 arithmetic of the Rust net: the learner's parity check).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub struct Config {
    pub seed: u64,
    pub copies: usize,
    pub plan: BTreeMap<String, Vec<u32>>,
}

static CONFIG: OnceLock<Option<Config>> = OnceLock::new();

pub fn config() -> Option<&'static Config> {
    CONFIG
        .get_or_init(|| {
            let path = std::env::var_os("ARMY_SAMPLE_PLAN")?;
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("ARMY_SAMPLE_PLAN {path:?}: {e}"));
            let v: serde_json::Value = serde_json::from_str(&text).expect("ARMY_SAMPLE_PLAN is JSON");
            let plan = v["plan"].as_object().expect("plan").iter().map(|(k, r)| (k.clone(), r.as_array().expect("rounds").iter().map(|x| x.as_u64().unwrap() as u32).collect())).collect();
            Some(Config { seed: v["seed"].as_u64().unwrap_or(1), copies: v["copies"].as_u64().unwrap_or(8) as usize, plan })
        })
        .as_ref()
}

struct State {
    rng: u64,
    rounds: Vec<u32>,
    copy: usize,
    log: Vec<(usize, f32, Option<f32>)>,
    fallbacks: u32,
    /// The memory (and notes) state after each of our turn ends, flattened: what the next turn's decisions read.
    memory: Vec<f32>,
    memory_len: usize,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

/// The next play of this thread is copy `copy` of the scene `key`: its draws come from (seed, key, copy).
pub fn begin(cfg: &Config, key: &str, copy: usize) -> bool {
    let Some(rounds) = cfg.plan.get(key) else { return false };
    let mut h = cfg.seed;
    for b in key.bytes() {
        h = mix(h ^ u64::from(b));
    }
    h = mix(h ^ (copy as u64 + 1).wrapping_mul(0x9e3779b97f4a7c15));
    STATE.with(|s| *s.borrow_mut() = Some(State { rng: h, rounds: rounds.clone(), copy, log: Vec::new(), fallbacks: 0, memory: Vec::new(), memory_len: 0 }));
    true
}

pub fn end() {
    STATE.with(|s| *s.borrow_mut() = None);
}

pub fn on() -> bool {
    STATE.with(|s| s.borrow().is_some())
}

/// Whether the net samples in the round `rel` (relative to the scene's start round).
pub fn sampled(rel: u32) -> bool {
    STATE.with(|s| s.borrow().as_ref().is_some_and(|s| s.rounds.contains(&rel)))
}

fn uniform(s: &mut State) -> f64 {
    s.rng = s.rng.wrapping_add(0x9e3779b97f4a7c15);
    (mix(s.rng) >> 11) as f64 / (1u64 << 53) as f64
}

/// Draws an index from softmax(logits) (None entries are not offered); returns (index, its log-prob).
fn draw(logits: &[Option<f64>]) -> (usize, f64) {
    let max = logits.iter().flatten().fold(f64::NEG_INFINITY, |a, b| a.max(*b));
    let z: f64 = logits.iter().flatten().map(|l| (l - max).exp()).sum();
    let u = STATE.with(|s| uniform(s.borrow_mut().as_mut().expect("sampling state")));
    let mut acc = 0.0;
    let mut last = 0;
    for (i, l) in logits.iter().enumerate() {
        let Some(l) = l else { continue };
        last = i;
        acc += (l - max).exp() / z;
        if u < acc {
            return (i, l - max - z.ln());
        }
    }
    let l = logits[last].unwrap();
    (last, l - max - z.ln())
}

/// The sampled pick among the selectable tokens and the end: (Some(token) | None = end, log-prob).
pub fn pointer(logits: &[Option<f32>], end: f32) -> (Option<usize>, f32) {
    let mut v: Vec<Option<f64>> = logits.iter().map(|l| l.map(f64::from)).collect();
    v.push(Some(f64::from(end)));
    let (i, lp) = draw(&v);
    (if i == logits.len() { None } else { Some(i) }, lp as f32)
}

/// The sampled action among the legal ones: (index, log-prob).
pub fn action(scores: &[f32]) -> (usize, f32) {
    let v: Vec<Option<f64>> = scores.iter().map(|s| Some(f64::from(*s))).collect();
    let (i, lp) = draw(&v);
    (i, lp as f32)
}

/// The decision `index` of the recording was sampled with these log-probs.
pub fn note(index: usize, lp_ptr: f32, lp_act: Option<f32>) {
    STATE.with(|s| {
        if let Some(s) = s.borrow_mut().as_mut() {
            s.log.push((index, lp_ptr, lp_act));
        }
    });
}

/// The memory after our turn ends (`seat::turn_end`), kept for the learner: the learner reads the memory and the notes the
/// decisions of the next turn read from here instead of recomputing them (a recomputed hidden state can differ in a
/// discrete element and move a decision by 0.1 in the log-probability: the 5 outliers of the first measurement).
/// Layout per turn: u [SLOTS * U_DIM], p [PLANS * d], alive [SLOTS], then the notes (stage 6, stage_age, engaged,
/// waited, free 8, change_age, reason 4, rest = pull 16, rest_age = pull_age 16), in the order of the net's note stream.
pub fn note_memory(state: &super::model::MemoryState) {
    STATE.with(|s| {
        if let Some(s) = s.borrow_mut().as_mut() {
            let before = s.memory.len();
            s.memory.extend_from_slice(&state.u);
            s.memory.extend_from_slice(&state.p);
            s.memory.extend_from_slice(&state.alive);
            if let Some(n) = state.notes.as_ref() {
                s.memory.extend_from_slice(&n.stage);
                s.memory.extend_from_slice(&[n.stage_age, n.engaged, n.waited]);
                s.memory.extend_from_slice(&n.free);
                s.memory.push(n.change_age);
                s.memory.extend_from_slice(&n.reason);
                s.memory.extend_from_slice(&n.pull);
                s.memory.extend_from_slice(&n.pull_age);
            }
            s.memory_len = s.memory.len() - before;
        }
    });
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn note_fallback() {
    STATE.with(|s| {
        if let Some(s) = s.borrow_mut().as_mut() {
            s.fallbacks += 1;
        }
    });
}

/// The header fields of the recorded play.
pub fn header() -> Option<serde_json::Value> {
    STATE.with(|s| {
        s.borrow().as_ref().map(|s| {
            serde_json::json!({"copy": s.copy, "fallbacks": s.fallbacks, "sample": s.log.iter().map(|(i, p, a)| serde_json::json!([i, p, a])).collect::<Vec<_>>(),
                "memory_len": s.memory_len, "memory": base64(&s.memory.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>())})
        })
    })
}

//! v2 in process (classic/docs/ai-net-model.md, «v2 in-process (GPU)»): the
//! big net (`nn/aurelia_nn/model_v2.py`) plays inside the game process from
//! a float weights file (`python -m aurelia_nn.export_v2 weights`), with no
//! Python and no socket. `NetPlayer::from_path("v2:W.v2[,cpu][,int8][,log=FILE]")`,
//! so the bench's `net:v2:W.v2`, `ai-selfplay --net v2:W.v2` and the E2 loop
//! play it.
//!
//! - The trunk (map blocks, tokens, events, the transformer, value, memory,
//!   the entities' candidate-attention keys and the combat module's blows)
//!   runs once per player-turn: on the GPU (`gpu.rs`, wgpu and WGSL, the
//!   `gpu` feature) or on the CPU (`trunk::run_cpu`, the f32 reference, and
//!   the path without a GPU or with `,cpu`).
//! - Each decision runs on the CPU (`decide.rs`) from the trunk's outputs.
//! - The memory and the turn trace live here, per session (one per nation a
//!   scorer plays, `TurnCache::session`), as the remote player's server
//!   keeps them (`nn/aurelia_nn/remote.py`): nation slots kept, unit slots of
//!   the last turn, the memory after the turn written from its start.
//! - The engine (the weights and the GPU) is shared by every scorer of the
//!   process that loads the same file; the GPU runs the trunks of the
//!   sessions that wait at the same moment as one batch.

pub mod decide;
pub mod fixture;
pub mod gemm;
pub mod gemm_i8;
#[cfg(feature = "gpu")]
pub mod gpu;
// The blocking batcher (a worker thread owns the device) is native; wasm32
// drives `gpu::Gpu` with `.await` (no threads, no blocking waits).
pub mod trunk;
pub mod weights;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::ai::net_e2e::encode::Candidate;
use crate::ai::net_e2e::mind::Scorer;
use crate::ai::net_e2e::model::{Memory, TurnCache, TurnInput};

use decide::TurnState;
use trunk::{Job, MapPrep, Tables, TrunkOut};
use weights::Net;

/// The prefix of a v2 weights spec (`v2:W.v2`).
pub const PREFIX: &str = "v2:";

/// Where a turn's trunk runs.
pub enum Backend {
    Cpu,
    #[cfg(all(feature = "gpu", not(target_arch = "wasm32")))]
    Gpu(gpu::Batcher),
}

impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Backend::Cpu => "cpu",
            #[cfg(all(feature = "gpu", not(target_arch = "wasm32")))]
            Backend::Gpu(_) => "gpu",
        })
    }
}

/// The weights and the trunk's backend, shared by the scorers of a file.
#[derive(Debug)]
pub struct Engine {
    pub net: Arc<Net>,
    pub backend: Backend,
    pub path: PathBuf,
    /// The int8 decision layers, made when a scorer first asks.
    int8: OnceLock<Arc<decide::Int8>>,
}

impl Engine {
    /// The engine of a weights file (`gpu`: on the GPU when there is one).
    pub fn new(path: &Path, gpu: bool) -> Result<Self, String> {
        let net = Arc::new(Net::load(path)?);
        let backend = if gpu { Self::gpu_backend(&net)? } else { Backend::Cpu };
        Ok(Self { net, backend, path: path.to_path_buf(), int8: OnceLock::new() })
    }

    #[cfg(all(feature = "gpu", not(target_arch = "wasm32")))]
    fn gpu_backend(net: &Arc<Net>) -> Result<Backend, String> {
        match gpu::Batcher::start(net.clone()) {
            Ok(b) => Ok(Backend::Gpu(b)),
            Err(e) if std::env::var_os("AURELIA_V2_REQUIRE_GPU").is_some() => Err(e),
            Err(e) => {
                eprintln!("v2: no GPU ({e}); the trunk runs on the CPU");
                Ok(Backend::Cpu)
            }
        }
    }

    #[cfg(not(all(feature = "gpu", not(target_arch = "wasm32"))))]
    fn gpu_backend(_net: &Arc<Net>) -> Result<Backend, String> {
        Ok(Backend::Cpu)
    }

    /// The shared engine of a file: one per (path, backend) in the process.
    pub fn shared(path: &Path, gpu: bool) -> Result<Arc<Engine>, String> {
        static ENGINES: OnceLock<Mutex<HashMap<(PathBuf, bool), Arc<Engine>>>> = OnceLock::new();
        let mut all = ENGINES.get_or_init(Default::default).lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = (path.to_path_buf(), gpu);
        if let Some(e) = all.get(&key) {
            return Ok(e.clone());
        }
        let e = Arc::new(Engine::new(path, gpu)?);
        all.insert(key, e.clone());
        Ok(e)
    }

    /// The trunk of one turn.
    pub fn trunk(&self, job: &Arc<Job>) -> TrunkOut {
        match &self.backend {
            Backend::Cpu => trunk::run_cpu(&self.net, job),
            #[cfg(all(feature = "gpu", not(target_arch = "wasm32")))]
            Backend::Gpu(b) => b.run(job),
        }
    }

    /// The GPU's counters as JSON (None on the CPU).
    pub fn gpu_stats(&self) -> Option<String> {
        match &self.backend {
            Backend::Cpu => None,
            #[cfg(all(feature = "gpu", not(target_arch = "wasm32")))]
            Backend::Gpu(b) => {
                let s = b.stats();
                let ms = |ns: u64| ns as f64 / 1e6 / s.batches.max(1) as f64;
                let kernels: Vec<String> = gpu::kernel_names()
                    .iter()
                    .zip(s.kernel_ns.iter().zip(&s.kernel_n))
                    .filter(|(_, (_, n))| **n > 0)
                    .map(|(name, (ns, n))| format!("\"{name}\":[{n},{:.3}]", *ns as f64 / 1e6 / s.batches.max(1) as f64))
                    .collect();
                Some(format!(
                    "{{\"adapter\":{:?},\"batches\":{},\"turns\":{},\"cpu_turns\":{},\"mean_batch\":{:.2},\"largest_batch\":{},\"dispatches_per_batch\":{:.1},\"encode_ms\":{:.3},\"gpu_wait_ms\":{:.3},\"read_ms\":{:.3},\"arena_mb\":{:.1},\"weights_mb\":{:.1},\"mode\":\"{:?}\",\"queue_ms_per_turn\":{:.3},\"busy_ms\":{:.1},\"span_ms\":{:.1},\"sizes\":{:?},\"kernel_ms_per_batch\":{{{}}}}}",
                    b.adapter,
                    s.batches,
                    s.turns,
                    s.cpu_turns,
                    s.turns as f64 / s.batches.max(1) as f64,
                    s.largest_batch,
                    s.dispatches as f64 / s.batches.max(1) as f64,
                    ms(s.encode_ns),
                    ms(s.wait_ns),
                    ms(s.read_ns),
                    s.arena_bytes as f64 / 1048576.0,
                    s.weight_bytes as f64 / 1048576.0,
                    b.mode,
                    s.queue_ns as f64 / 1e6 / s.turns.max(1) as f64,
                    s.busy_ns as f64 / 1e6,
                    s.last_ns.saturating_sub(s.first_ns) as f64 / 1e6,
                    s.sizes,
                    kernels.join(",")
                ))
            }
        }
    }

    /// The int8 decision layers (`v2:W.v2,int8`), made once.
    pub fn int8(&self) -> Arc<decide::Int8> {
        self.int8.get_or_init(|| Arc::new(decide::Int8::new(&self.net))).clone()
    }

    pub fn on_gpu(&self) -> bool {
        !matches!(self.backend, Backend::Cpu)
    }
}

/// A session's memory (`remote.Session`, the training's `MemoryBank`).
#[derive(Debug, Default)]
struct Bank {
    player: Vec<f32>,
    nations: BTreeMap<i32, Vec<f32>>,
    units: BTreeMap<i32, Vec<f32>>,
}

#[derive(Debug, Default)]
struct Session {
    bank: Bank,
    map: Option<Arc<MapPrep>>,
    turn: Option<TurnState>,
    /// The last decision's candidates (row prefix, meta): which one a noted choice is.
    last: Vec<(Vec<i16>, [i32; 4])>,
    /// The last decision's float logits.
    logits: Vec<f32>,
    /// The scorer's time in the turn so far (BEGIN and decisions).
    turn_ns: u64,
}

/// Time spent, for `log=`.
#[derive(Debug, Default, Clone, Copy)]
pub struct Counters {
    pub begins: u64,
    pub begin_ns: u64,
    pub trunk_ns: u64,
    pub scores: u64,
    pub score_ns: u64,
    pub candidates: u64,
}

/// The scorer of v2 in process.
#[derive(Debug)]
pub struct V2Scorer {
    engine: Arc<Engine>,
    /// The decisions' int8 layers (None: f32, the default).
    int8: Option<Arc<decide::Int8>>,
    sessions: Mutex<(u64, HashMap<u64, Session>)>,
    counters: Mutex<Counters>,
    /// Each finished turn's scorer time (µs), for `log=`.
    turns: Mutex<Vec<u32>>,
    log: Option<PathBuf>,
    /// The game thread's hold on the GPU batcher (lockstep counts the holders).
    #[cfg(all(feature = "gpu", not(target_arch = "wasm32")))]
    _hold: Option<gpu::Hold>,
}

/// Nanoseconds of a monotonic clock (0 on wasm32, where `Instant` panics).
pub fn now_ns() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static T0: OnceLock<std::time::Instant> = OnceLock::new();
        T0.get_or_init(std::time::Instant::now).elapsed().as_nanos() as u64
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

/// Q8 of a logit (`remote.py`: round(logit × 256), half to even, saturated).
pub fn q8(x: f32) -> i32 {
    (x * 256.0).round_ties_even() as i32
}

impl V2Scorer {
    pub fn new(engine: Arc<Engine>, log: Option<PathBuf>) -> Self {
        #[cfg(all(feature = "gpu", not(target_arch = "wasm32")))]
        let _hold = match &engine.backend {
            Backend::Gpu(b) => Some(b.hold()),
            Backend::Cpu => None,
        };
        Self {
            engine,
            int8: None,
            sessions: Mutex::new((1, HashMap::new())),
            counters: Mutex::default(),
            turns: Mutex::default(),
            log,
            #[cfg(all(feature = "gpu", not(target_arch = "wasm32")))]
            _hold,
        }
    }

    /// The decisions with int8 products (`sdot`; the agreement with f32 is
    /// measured, not exact). Panics where the CPU lacks the dot product.
    pub fn with_int8(mut self) -> Self {
        assert!(gemm_i8::available(), "v2 int8: this CPU has no int8 dot product");
        self.int8 = Some(self.engine.int8());
        self
    }

    pub fn engine(&self) -> &Arc<Engine> {
        &self.engine
    }

    pub fn counters(&self) -> Counters {
        *self.counters.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The float logits of the session's last decision.
    pub fn raw_logits(&self, cache: &TurnCache) -> Option<Vec<f32>> {
        self.lock().1.get(&cache.session).map(|s| s.logits.clone())
    }

    fn lock(&self) -> MutexGuard<'_, (u64, HashMap<u64, Session>)> {
        self.sessions.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The trunk input of a session's turn (its map prepared once).
    fn job(&self, s: &mut Session, input: &TurnInput<'_>) -> Job {
        let net = &self.engine.net;
        let map = match &s.map {
            Some(m) if m.same(input.neighbors, input.region) => m.clone(),
            _ => {
                let dil: Vec<usize> = net.map.iter().map(|b| b.dilation).collect();
                let m = Arc::new(MapPrep::new(input.neighbors, input.region, &dil));
                s.map = Some(m.clone());
                m
            }
        };
        let cfg = &net.cfg;
        if s.bank.player.is_empty() {
            s.bank.player = vec![0.0; cfg.mem_player];
        }
        let tables = Tables {
            tiles: input.tiles,
            global: input.global,
            entities: input.entities,
            entity_meta: input.entity_meta,
            events: if cfg.events { input.events } else { &[] },
            event_meta: if cfg.events { input.event_meta } else { &[] },
        };
        assert_eq!(input.tiles.len(), input.region.len() * cfg.c_tile, "v2: the tile columns");
        assert_eq!(input.global.len(), cfg.c_global, "v2: the global columns");
        assert_eq!(input.entities.len(), input.entity_ids.len() * cfg.c_entity, "v2: the entity columns");
        let bank = &s.bank;
        let ids = input.entity_ids;
        let memory = |e: usize, unit: bool| -> Option<Vec<f32>> {
            let id = ids[e];
            if id < 0 {
                return None;
            }
            if unit { bank.units.get(&id) } else { bank.nations.get(&id) }.cloned()
        };
        Job::prepare(net, map, &tables, &bank.player, &memory)
    }

    /// The memory after the turn (from its start, as `remote.Player.begin`).
    fn remember(&self, s: &mut Session, job: &Job, out: &TrunkOut, ids: &[i32]) {
        let cfg = &self.engine.net.cfg;
        s.bank.player = out.new_player.clone();
        let mut units = BTreeMap::new();
        let (mut iu, mut inn) = (0, 0);
        for slot in &job.slots {
            let id = ids[slot.entity];
            if slot.unit {
                if id >= 0 {
                    units.insert(id, out.new_unit[iu * cfg.mem_unit..(iu + 1) * cfg.mem_unit].to_vec());
                }
                iu += 1;
            } else {
                if id >= 0 {
                    s.bank.nations.insert(id, out.new_nation[inn * cfg.mem_nation..(inn + 1) * cfg.mem_nation].to_vec());
                }
                inn += 1;
            }
        }
        s.bank.units = units;
    }
}

impl Drop for V2Scorer {
    fn drop(&mut self) {
        let Some(path) = &self.log else { return };
        let c = self.counters();
        let mut turns = std::mem::take(&mut *self.turns.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        turns.extend(self.lock().1.values().filter(|s| s.turn_ns > 0).map(|s| (s.turn_ns / 1000) as u32));
        turns.sort_unstable();
        let pct = |q: f64| turns.get(((turns.len() as f64 - 1.0) * q).round() as usize).map_or(0.0, |&u| f64::from(u) / 1000.0);
        let line = format!(
            "{{\"v2\":{:?},\"backend\":\"{:?}\",\"begins\":{},\"begin_ms\":{:.3},\"trunk_ms\":{:.3},\"scores\":{},\"score_ms\":{:.3},\"candidates\":{},\"turns\":{},\"turn_ms_p50\":{:.3},\"turn_ms_p95\":{:.3},\"turn_us\":{:?},\"gpu\":{}}}\n",
            self.engine.path.display().to_string(),
            self.engine.backend,
            c.begins,
            c.begin_ns as f64 / 1e6,
            c.trunk_ns as f64 / 1e6,
            c.scores,
            c.score_ns as f64 / 1e6,
            c.candidates,
            turns.len(),
            pct(0.5),
            pct(0.95),
            turns,
            self.engine.gpu_stats().unwrap_or_else(|| "null".into())
        );
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            use std::io::Write;
            let _ = f.write_all(line.as_bytes());
        }
    }
}

impl Scorer for V2Scorer {
    fn label(&self) -> String {
        format!("{PREFIX}{} ({}, {:?})", self.engine.path.display(), self.engine.net.label, self.engine.backend)
    }

    fn memory(&self) -> Memory {
        Memory::default()
    }

    fn has_head(&self, head: i32) -> bool {
        usize::try_from(head).is_ok_and(|h| h < self.engine.net.cfg.n_decision_heads)
    }

    fn layout_hash(&self) -> Option<u64> {
        Some(self.engine.net.layout_hash)
    }

    fn defers(&self) -> bool {
        self.engine.net.cfg.defer
    }

    fn begin_turn(&self, input: &TurnInput<'_>, _memory: &Memory, cache: &mut TurnCache) {
        let start = now_ns();
        let mut s = {
            let mut g = self.lock();
            if cache.session == 0 {
                cache.session = g.0;
                g.0 += 1;
            }
            g.1.remove(&cache.session).unwrap_or_default()
        };
        if s.turn_ns > 0 {
            self.turns.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((s.turn_ns / 1000) as u32);
        }
        let job = Arc::new(self.job(&mut s, input));
        let t = now_ns();
        let out = self.engine.trunk(&job);
        let trunk_ns = now_ns() - t;
        self.remember(&mut s, &job, &out, input.entity_ids);
        cache.set_value(q8(out.value));
        s.turn = Some(TurnState::new(&self.engine.net, out, &job));
        s.last.clear();
        let begin_ns = now_ns() - start;
        s.turn_ns = begin_ns;
        self.lock().1.insert(cache.session, s);
        let mut c = self.counters.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        c.begins += 1;
        c.trunk_ns += trunk_ns;
        c.begin_ns += begin_ns;
    }

    fn score(&self, cache: &TurnCache, head: usize, actor: i32, candidates: &[Candidate], out: &mut Vec<i32>) {
        out.clear();
        if candidates.is_empty() {
            return;
        }
        let start = now_ns();
        let net = &self.engine.net;
        let width = net.cfg.c_candidate;
        let mut rows = vec![0i16; candidates.len() * width];
        for (i, c) in candidates.iter().enumerate() {
            let n = width.min(c.row.len());
            rows[i * width..i * width + n].copy_from_slice(&c.row[..n]);
        }
        let meta: Vec<[i32; 4]> = candidates.iter().map(|c| c.meta).collect();
        // The session out of the map while it scores: the lock is held only to
        // take it and put it back, so the games of a process score in parallel.
        let mut s = self.lock().1.remove(&cache.session).expect("v2: score before begin_turn");
        let turn = s.turn.as_mut().expect("v2: score before begin_turn");
        let mut logits = std::mem::take(&mut s.logits);
        turn.score(net, self.int8.as_deref(), head, actor, &rows, &meta, &mut logits);
        out.extend(logits.iter().map(|&l| q8(l)));
        s.logits = logits;
        s.last = candidates.iter().map(|c| (c.row[..width.min(c.row.len())].to_vec(), c.meta)).collect();
        let score_ns = now_ns() - start;
        s.turn_ns += score_ns;
        self.lock().1.insert(cache.session, s);
        let mut c = self.counters.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        c.scores += 1;
        c.candidates += candidates.len() as u64;
        c.score_ns += score_ns;
    }

    fn end_turn(&self, _cache: &TurnCache, memory: &Memory) -> Memory {
        memory.clone()
    }

    fn note_choice(&self, cache: &mut TurnCache, _head: usize, _actor: i32, candidate: &Candidate) {
        let net = &self.engine.net;
        let width = net.cfg.c_candidate;
        let mut g = self.lock();
        let Some(s) = g.1.get_mut(&cache.session) else { return };
        let row = &candidate.row[..width.min(candidate.row.len())];
        let at = s.last.iter().position(|(r, m)| *m == candidate.meta && r[..] == *row).map_or(-1, |i| i as i32);
        if let Some(t) = s.turn.as_mut() {
            t.note(net, at);
        }
    }
}

/// The scorer of a spec `W.v2[,cpu][,int8][,log=FILE]` (after [`PREFIX`]).
pub fn scorer(spec: &str) -> Result<Arc<dyn Scorer>, String> {
    let mut parts = spec.split(',');
    let path = parts.next().filter(|s| !s.is_empty()).ok_or_else(|| format!("{PREFIX}{spec}: no weights file"))?;
    let (mut gpu, mut log, mut int8) = (true, None, false);
    for part in parts {
        match part.split_once('=') {
            Some(("log", v)) => log = Some(PathBuf::from(v)),
            None if part == "cpu" => gpu = false,
            None if part == "int8" => int8 = true,
            _ => return Err(format!("{PREFIX}{spec}: `{part}` is no `cpu`, `int8` or `log=FILE`")),
        }
    }
    if int8 && !gemm_i8::available() {
        return Err(format!("{PREFIX}{spec}: no int8 dot product on this CPU"));
    }
    let s = V2Scorer::new(Engine::shared(Path::new(path), gpu)?, log);
    Ok(Arc::new(if int8 { s.with_int8() } else { s }))
}

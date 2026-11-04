//! The speed paths of the integer net (`ai::net_e2e::model::Speed`: the
//! aarch64 kernels, threads, incremental hex convolutions) compute the same
//! integers as the plain path, which `net_e2e_model.rs` holds equal to the
//! NumPy reference; and the measurement of the forward pass on real game
//! states (classic/docs/ai-net-model.md, Measured).
//!
//! Weights here are random, written in the weights file format: of a
//! synthetic layout (any shape), or of a template file's layout with the
//! layers resized (`medium` from the small `random-small.bin`), which then
//! plays a real game.
//!
//! The profile (release build, spans on):
//! `CARGO_TARGET_DIR=<dir> cargo +1.98.1 test --release -p classic-rules
//! --features prof/enabled --test net_e2e_speed -- --ignored --nocapture
//! net_profile` with `NET_PROFILE_WEIGHTS=<weights.bin>` (default the pinned
//! small weights), `NET_PROFILE_PRESET=medium` (the file's layout resized),
//! `NET_PROFILE_MAP=64x48x6`, `NET_PROFILE_ROUNDS=100`.

use std::sync::Arc;
use std::time::Instant;

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::net_e2e::encode;
use classic_rules::ai::net_e2e::encode::Candidate;
use classic_rules::ai::net_e2e::mind::{parse_heads, ModelScorer, NetPlayer, Scorer, ScriptedRoles};
use classic_rules::ai::net_e2e::model::{Memory, Model, Speed, TurnCache, TurnInput};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::classic::Classic;
use classic_rules::ids::PlayerId;
use classic_rules::scenario::{generate_new_game, ScenarioConfig};
use classic_rules::{compile_rules, ClassicGame, CompiledRuleset, GameState};
use kernel::ai::driver::AiDriver;

type Driver = AiDriver<Classic, ClassicMind>;

/// A small deterministic generator (an LCG; tests only).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    /// Uniform in `lo..=hi`.
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() as i64).rem_euclid(hi - lo + 1)
    }
}

/// Config block indices of the weights file (classic/docs/ai-net-model.md).
mod conf {
    pub const C_TILE: usize = 0;
    pub const C_GLOBAL: usize = 1;
    pub const C_ENTITY: usize = 2;
    pub const C_CANDIDATE: usize = 3;
    pub const D_TILE: usize = 4;
    pub const D_MODEL: usize = 5;
    pub const HEADS: usize = 6;
    pub const N_CONV: usize = 7;
    pub const N_ATTN: usize = 8;
    pub const D_FF: usize = 9;
    pub const D_HEAD: usize = 10;
    pub const N_KINDS: usize = 11;
    pub const N_RELATIONS: usize = 12;
    pub const N_DECISION_HEADS: usize = 13;
    pub const TAG_BUCKETS: usize = 14;
    pub const TAG_DIM: usize = 15;
    pub const PARAM_BUCKETS: usize = 16;
    pub const PARAM_DIM: usize = 17;
    pub const MEM_PLAYER: usize = 18;
    pub const MEM_NATION: usize = 19;
    pub const MEM_UNIT: usize = 20;
    pub const ID_BUCKETS: usize = 21;
    pub const ID_DIM: usize = 22;
    pub const ENGRAM: usize = 23;
    pub const ENGRAM_BUCKETS: usize = 24;
    pub const ENGRAM_DIM: usize = 25;
    pub const AIM: usize = 26;
    pub const N_AIMS: usize = 27;
    pub const COARSE: usize = 28;
    pub const LUT_LEN: usize = 31;
    pub const LN_EPS: usize = 32;
    pub const GATE_HALF: usize = 33;
    pub const COUNT: usize = 35;
}

/// Everything of a weights file but its layers, edge rows and
/// embeddings sized by the shape: what a resized file keeps.
#[derive(Clone)]
struct Header {
    conf: Vec<u32>,
    hash: u64,
    mults: [Vec<i32>; 4],
    exp: Vec<i32>,
    sig: Vec<i32>,
    tanh: Vec<i32>,
    tag: Vec<i16>,
    param: Vec<i16>,
    /// Per table: id columns and their embeddings.
    ids: [(Vec<u32>, Vec<i16>); 4],
    engram: Option<(Vec<u32>, Vec<i16>)>,
}

struct Reader<'a>(&'a [u8], usize);

impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> &[u8] {
        let s = &self.0[self.1..self.1 + n];
        self.1 += n;
        s
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.bytes(4).try_into().unwrap())
    }
    fn i32s(&mut self, n: usize) -> Vec<i32> {
        self.bytes(4 * n).as_chunks::<4>().0.iter().map(|c| i32::from_le_bytes(*c)).collect()
    }
    fn i16s(&mut self, n: usize) -> Vec<i16> {
        self.bytes(2 * n).as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)).collect()
    }
}

impl Header {
    /// The header of a weights file (version 2).
    fn read(data: &[u8]) -> Header {
        let mut r = Reader(data, 0);
        assert_eq!(r.bytes(4), b"AUNE");
        assert_eq!(r.u32(), 2);
        let n = r.u32() as usize;
        let conf: Vec<u32> = (0..n).map(|_| r.u32()).collect();
        let c = |i: usize| conf[i] as usize;
        let hash = u64::from_le_bytes(r.bytes(8).try_into().unwrap());
        let mults = [r.i32s(c(conf::C_TILE)), r.i32s(c(conf::C_GLOBAL)), r.i32s(c(conf::C_ENTITY)), r.i32s(c(conf::C_CANDIDATE))];
        let exp = r.i32s(c(conf::LUT_LEN));
        let sig = r.i32s(2 * c(conf::GATE_HALF) + 1);
        let tanh = r.i32s(2 * c(conf::GATE_HALF) + 1);
        let _edges = r.i16s(c(conf::N_CONV) * c(conf::D_TILE));
        let tag = r.i16s(c(conf::N_DECISION_HEADS) * c(conf::TAG_BUCKETS) * c(conf::TAG_DIM));
        let param = r.i16s(c(conf::N_DECISION_HEADS) * c(conf::PARAM_BUCKETS) * c(conf::PARAM_DIM));
        let mut table = || {
            let k = r.u32() as usize;
            let cols: Vec<u32> = (0..k).map(|_| r.u32()).collect();
            let emb = if k > 0 { r.i16s(k * c(conf::ID_BUCKETS) * c(conf::ID_DIM)) } else { Vec::new() };
            (cols, emb)
        };
        let ids = [table(), table(), table(), table()];
        let engram = (conf[conf::ENGRAM] != 0).then(|| {
            let k = r.u32() as usize;
            let cols: Vec<u32> = (0..k).map(|_| r.u32()).collect();
            (cols, r.i16s(3 * c(conf::ENGRAM_BUCKETS) * c(conf::ENGRAM_DIM)))
        });
        Header { conf, hash, mults, exp, sig, tanh, tag, param, ids, engram }
    }

    /// A synthetic layout: `c` columns per table (two id columns each but
    /// the global table's one), the fixed-point tables of the trainer.
    fn synthetic(rng: &mut Rng, c: [usize; 4], flags: bool) -> Header {
        let mut conf = vec![0u32; conf::COUNT];
        let set = |conf: &mut Vec<u32>, i: usize, v: usize| conf[i] = v as u32;
        for (i, &n) in c.iter().enumerate() {
            set(&mut conf, i, n);
        }
        for (i, v) in [
            (conf::N_KINDS, 8),
            (conf::N_RELATIONS, 8),
            (conf::N_DECISION_HEADS, 4),
            (conf::TAG_BUCKETS, 128),
            (conf::TAG_DIM, 8),
            (conf::PARAM_BUCKETS, 256),
            (conf::PARAM_DIM, 16),
            (conf::MEM_PLAYER, 32),
            (conf::MEM_NATION, 16),
            (conf::MEM_UNIT, 16),
            (conf::ID_BUCKETS, 256),
            (conf::ID_DIM, 4),
            (conf::ENGRAM, usize::from(flags)),
            (conf::ENGRAM_BUCKETS, 1 << 10),
            (conf::ENGRAM_DIM, 16),
            (conf::AIM, usize::from(flags)),
            (conf::N_AIMS, 3),
            (conf::COARSE, usize::from(flags)),
            (29, 8),
            (30, 16),
            (conf::LUT_LEN, 2840),
            (conf::GATE_HALF, 2048),
            (34, 15),
        ] {
            set(&mut conf, i, v);
        }
        let mults = c.map(|n| (0..n).map(|_| rng.range(1 << 20, 1 << 25) as i32).collect());
        let exp = (0..2840).map(|t| (32768.0 * (-(t as f64) / 256.0).exp()).round() as i32).collect();
        let sig = (0..=4096).map(|i| (32768.0 / (1.0 + (-(i as f64 - 2048.0) / 256.0).exp())).round() as i32).collect();
        let tanh = (0..=4096).map(|i| (32768.0 * ((i as f64 - 2048.0) / 256.0).tanh()).round() as i32).collect();
        let emb = |rng: &mut Rng, n: usize| (0..n).map(|_| rng.range(-600, 600) as i16).collect::<Vec<i16>>();
        let tag = emb(rng, 4 * 128 * 8);
        let param = emb(rng, 4 * 256 * 16);
        let ids = [0, 1, 2, 3].map(|t| {
            let cols: Vec<u32> = if t == 1 { vec![3] } else { vec![1, 4] };
            let n = cols.len() * 256 * 4;
            (cols, emb(rng, n))
        });
        let engram = flags.then(|| (vec![0, 5], emb(rng, 3 * (1 << 10) * 16)));
        Header { conf, hash: 0x1234_5678_9abc_def0, mults, exp, sig, tanh, tag, param, ids, engram }
    }

    /// A weights file of this header with the shape `shape` (d_tile,
    /// n_conv, d_model, n_attn, heads, d_ff, d_head) and random layers;
    /// `gain` bits louder than activations near 1 (saturating ones).
    fn write(&self, rng: &mut Rng, shape: [usize; 7], gain: i32) -> Vec<u8> {
        let mut conf = self.conf.clone();
        let [dt, n_conv, d, n_attn, heads, dff, dhead] = shape;
        for (i, v) in [(conf::D_TILE, dt), (conf::N_CONV, n_conv), (conf::D_MODEL, d), (conf::N_ATTN, n_attn), (conf::HEADS, heads), (conf::D_FF, dff), (conf::D_HEAD, dhead)] {
            conf[i] = v as u32;
        }
        conf[conf::LN_EPS] = (1e-5 * (d * d) as f64 * 4294967296.0).round() as u32;
        let c = |i: usize| conf[i] as usize;
        let mut out = Vec::new();
        out.extend_from_slice(b"AUNE");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(conf.len() as u32).to_le_bytes());
        conf.iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        out.extend_from_slice(&self.hash.to_le_bytes());
        let i32s = |out: &mut Vec<u8>, v: &[i32]| v.iter().for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
        let i16s = |out: &mut Vec<u8>, v: &[i16]| v.iter().for_each(|x| out.extend_from_slice(&x.to_le_bytes()));
        for m in &self.mults {
            i32s(&mut out, m);
        }
        i32s(&mut out, &self.exp);
        i32s(&mut out, &self.sig);
        i32s(&mut out, &self.tanh);
        let edges: Vec<i16> = (0..n_conv * dt).map(|_| rng.range(0, 700) as i16).collect();
        i16s(&mut out, &edges);
        i16s(&mut out, &self.tag);
        i16s(&mut out, &self.param);
        for (cols, emb) in &self.ids {
            out.extend_from_slice(&(cols.len() as u32).to_le_bytes());
            cols.iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
            i16s(&mut out, emb);
        }
        if let Some((cols, tables)) = &self.engram {
            out.extend_from_slice(&(cols.len() as u32).to_le_bytes());
            cols.iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
            i16s(&mut out, tables);
        }
        let ed = if conf[conf::ENGRAM] != 0 { c(conf::ENGRAM_DIM) } else { 0 };
        let width = |t: usize| c(t) - self.ids[t].0.len() + self.ids[t].0.len() * c(conf::ID_DIM);
        let mut layers: Vec<(usize, usize)> = vec![(dt, width(0) + ed)];
        layers.extend((0..n_conv).map(|_| (dt, 7 * dt)));
        layers.push((d, 2 * dt));
        layers.push((d, width(2) + ed + c(conf::N_KINDS) + c(conf::N_RELATIONS) + dt + 1));
        layers.push((d, width(1)));
        layers.extend([(d, c(conf::MEM_PLAYER)), (d, c(conf::MEM_NATION)), (d, c(conf::MEM_UNIT))]);
        if conf[conf::COARSE] != 0 {
            layers.push((d, 2 * dt));
        }
        if conf[conf::AIM] != 0 {
            layers.push((d, c(conf::N_AIMS)));
        }
        for _ in 0..n_attn {
            layers.extend([(3 * d, d), (d, d), (dff, d), (d, dff)]);
        }
        layers.push((d, width(3) + ed + c(conf::TAG_DIM) + c(conf::PARAM_DIM) + dt + 1 + d + 1));
        for _ in 0..c(conf::N_DECISION_HEADS) {
            layers.extend([(d, d), (dhead, d), (1, dhead)]);
        }
        layers.extend([(dhead, d), (1, dhead)]);
        for m in [c(conf::MEM_PLAYER), c(conf::MEM_NATION), c(conf::MEM_UNIT)] {
            layers.extend([(3 * m, d), (3 * m, m)]);
        }
        out.extend_from_slice(&(layers.len() as u32).to_le_bytes());
        for (o, i) in layers {
            out.extend_from_slice(&(o as u32).to_le_bytes());
            out.extend_from_slice(&(i as u32).to_le_bytes());
            // A row's largest |w| is 127 (as quantised); the scale keeps
            // activations near 1 (Q8 256), with some saturating.
            for _ in 0..o {
                let mut row: Vec<i8> = (0..i).map(|_| rng.range(-127, 127) as i8).collect();
                row[rng.range(0, i as i64 - 1) as usize] = if rng.next() & 1 == 0 { 127 } else { -127 };
                out.extend(row.iter().map(|&w| w as u8));
            }
            let shift = 20 + (127.0 * (i as f64).sqrt() / 1.7).log2().round() as i32 - gain;
            let mults: Vec<i32> = (0..o).map(|_| rng.range(1 << 20, 1 << 21) as i32).collect();
            i32s(&mut out, &mults);
            i32s(&mut out, &vec![shift.clamp(1, 62); o]);
            let bias: Vec<i32> = (0..o).map(|_| rng.range(-200, 200) as i32).collect();
            i32s(&mut out, &bias);
        }
        out.extend_from_slice(b"END\0");
        out
    }
}

/// The preset shapes: d_tile, n_conv, d_model, n_attn, heads, d_ff, d_head.
const SMALL: [usize; 7] = [32, 3, 64, 2, 4, 128, 32];
const MEDIUM: [usize; 7] = [64, 4, 128, 4, 8, 384, 64];

fn pinned_weights() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/net_mind/random-small.bin")
}

/// A turn's inputs, owned.
#[derive(Clone, Default)]
struct Owned {
    tiles: Vec<i16>,
    neighbors: Vec<i32>,
    region: Vec<i32>,
    global: Vec<i16>,
    entities: Vec<i16>,
    meta: Vec<i32>,
    ids: Vec<i32>,
    coarse: Vec<i32>,
    aim: Vec<i16>,
    candidates: Vec<i16>,
    candidate_meta: Vec<i32>,
}

impl Owned {
    fn input(&self) -> TurnInput<'_> {
        TurnInput {
            tiles: &self.tiles,
            neighbors: &self.neighbors,
            region: &self.region,
            global: &self.global,
            entities: &self.entities,
            entity_meta: &self.meta,
            entity_ids: &self.ids,
            coarse_region: &self.coarse,
            aim: &self.aim,
            events: &[],
            event_meta: &[],
        }
    }
}

/// The neighbour, region and coarse tables of a map, as the net player
/// builds them (`mind.rs`, `MapTables`).
fn tables(dims: classic_rules::math::MapDimensions) -> (Vec<i32>, Vec<i32>, Vec<i32>) {
    let neighbors = encode::neighbor_table(dims).into_iter().flatten().collect();
    let (region, _) = encode::region_table(dims);
    let across = dims.width.div_ceil(16);
    let coarse = (0..dims.tile_count() as u32).map(|n| ((n / dims.width / 16) * across + (n % dims.width) / 16) as i32).collect();
    (neighbors, region, coarse)
}

/// A player's turn input from its observation now (as the net player's
/// `begin` builds it), with candidates of its research decision, each row
/// cut to the net's `width` (a format-9 net reads the prefix of a row).
fn observe(rules: &CompiledRuleset, game: &GameState, player: PlayerId, coarse: bool, width: usize) -> Owned {
    let sight = game.ai_observation(rules, player);
    let turn = encode::Turn::encode(&sight, rules);
    let (neighbors, region, coarse_region) = tables(turn.dims);
    let candidates = encode::research_candidates(&sight, rules);
    Owned {
        tiles: turn.tiles.clone(),
        neighbors,
        region,
        global: turn.global.clone(),
        entities: turn.entities.clone(),
        meta: turn.entity_meta.iter().flat_map(|m| [m[0], m[1], m[2]]).collect(),
        ids: turn.entity_meta.iter().map(|m| m[3]).collect(),
        coarse: if coarse { coarse_region } else { Vec::new() },
        aim: Vec::new(),
        candidates: candidates.iter().flat_map(|c| c.row[..width].iter().copied()).collect(),
        candidate_meta: candidates.iter().flat_map(|c| c.meta).collect(),
    }
}

fn round(game: &GameState) -> u32 {
    game.view().auth.turn().round
}

fn driver(rules: &CompiledRuleset, mind: ClassicMind) -> Driver {
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    AiDriver::with_mind(settings, Vec::new(), mind)
}

/// The weights of the profile: `NET_PROFILE_WEIGHTS` (the pinned small
/// weights) as they are, or resized to `NET_PROFILE_PRESET=medium`.
fn profile_weights() -> (String, Vec<u8>) {
    let path = std::env::var("NET_PROFILE_WEIGHTS").unwrap_or_else(|_| pinned_weights().display().to_string());
    let data = std::fs::read(&path).expect("the weights file");
    match std::env::var("NET_PROFILE_PRESET").as_deref() {
        Ok("medium") => (format!("{path} resized to medium, random"), Header::read(&data).write(&mut Rng(7), MEDIUM, 0)),
        _ => (path, data),
    }
}

/// THE PROFILE: a game of nets on a big map (the bench's setting: heads unit
/// and research, founders and builders scripted), the `net/*` spans of the
/// game (with `--features prof/enabled`); then the forward pass alone on the
/// last rounds' real states, per phase.
#[test]
#[ignore = "a measurement"]
fn net_profile() {
    let rules = compile_rules(core_rules()).unwrap();
    let (label, data) = profile_weights();
    let model = Arc::new(Model::from_bytes(&data).expect("the weights load"));
    let cfg = *model.config();
    let map: Vec<u32> = std::env::var("NET_PROFILE_MAP").unwrap_or_else(|_| "64x48x6".into()).split('x').map(|p| p.parse().unwrap()).collect();
    let rounds: u32 = std::env::var("NET_PROFILE_ROUNDS").ok().and_then(|r| r.parse().ok()).unwrap_or(100);
    let config = ScenarioConfig { seed: 0x0_9E7A, players: map[2] as u8, width: map[0], height: map[1], cities: 1, units: 2, water_ratio: 10 };
    println!("weights {label}: d_tile {} × {} conv, d_model {} × {} attention ({} heads), d_ff {}", cfg.d_tile, cfg.n_conv, cfg.d_model, cfg.n_attn, cfg.n_attn_heads, cfg.d_ff);
    println!("map {}×{}, {} nations, {rounds} rounds; spans compiled in: {}", map[0], map[1], map[2], prof::compiled_in());
    let net = NetPlayer::new(Arc::new(ModelScorer::new(model.clone(), "profile")))
        .with_heads(parse_heads("unit,research").unwrap())
        .with_scripted(ScriptedRoles::parse("founder,builder").unwrap());
    let mut game = generate_new_game(&rules, &config);
    let mut driver = driver(&rules, ClassicMind::with_net(Variant::Net, net));
    let majors: Vec<PlayerId> = (0..map[2] as u8).map(PlayerId).collect();
    // Per player, its turn inputs of the last rounds.
    let keep = 8;
    let mut states: Vec<Vec<Owned>> = vec![Vec::new(); majors.len()];
    let _ = prof::drain();
    let started = Instant::now();
    while round(&game) < rounds && !game.is_over() {
        driver.run(&mut game, &rules, u32::MAX);
        if round(&game) + keep > rounds {
            for (p, &player) in majors.iter().enumerate() {
                if game.view().auth.players().is_major(player) {
                    states[p].push(observe(&rules, &game, player, cfg.coarse, cfg.c_candidate));
                }
            }
        }
    }
    let wall = started.elapsed();
    let records = prof::drain();
    let stats = driver.mind_mut().net().unwrap().stats();
    println!("game: {} rounds in {:.1} s; net {stats:?}", round(&game), wall.as_secs_f64());
    if !records.is_empty() {
        let turns = stats.turns.max(1) as f64;
        println!("| span | calls | total ms | ms per player-turn | µs per call |");
        println!("|---|---:|---:|---:|---:|");
        for r in records.iter().filter(|r| r.path.contains("net/")) {
            let name = r.path.split(" > ").filter(|p| p.starts_with("net/")).collect::<Vec<_>>().join(" > ");
            println!("| {name} | {} | {:.1} | {:.3} | {:.1} |", r.calls, r.incl_ns as f64 / 1e6, r.incl_ns as f64 / 1e6 / turns, r.incl_ns as f64 / 1e3 / r.calls.max(1) as f64);
        }
    }
    let last: Vec<&Owned> = states.iter().filter_map(|s| s.last()).collect();
    let tokens: Vec<usize> = last.iter().map(|o| o.meta.len() / 3).collect();
    println!("last round: entities per player {tokens:?}");
    offline(&model, &states);
}

/// Asks macOS to keep this thread (and the threads it starts) on the
/// performance cores: on a shared machine the efficiency cores otherwise
/// take turns, at another clock.
fn p_cores() {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn pthread_set_qos_class_self_np(qos: u32, relative_priority: i32) -> i32;
        }
        const QOS_CLASS_USER_INTERACTIVE: u32 = 0x21;
        // SAFETY: sets the calling thread's QoS class; no memory is touched.
        unsafe {
            pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0);
        }
    }
}

/// The forward pass alone on recorded states, per speed: each player's
/// last rounds in order through its own cache (the incremental tile states
/// reuse the turn before), the first turn a warm-up; wall clock, best of
/// several runs, per-phase spans when compiled in.
fn offline(model: &Model, states: &[Vec<Owned>]) {
    p_cores();
    let fast = Speed::default();
    let threads = std::env::var("NET_PROFILE_THREADS").ok().and_then(|t| t.parse().ok()).unwrap_or(4);
    let speeds = [
        ("plain", Speed::PLAIN),
        ("simd", Speed { incremental: false, ..fast }),
        ("simd+incremental (default)", fast),
        ("simd, threads", Speed { incremental: false, threads, ..fast }),
        ("simd+incremental, threads", Speed { threads, ..fast }),
    ];
    let players: Vec<&Vec<Owned>> = states.iter().filter(|s| s.len() >= 2).collect();
    let turns: usize = players.iter().map(|s| s.len() - 1).sum();
    println!("\noffline: {} players × {} turns (after a warm-up turn each), best of 5", players.len(), turns / players.len().max(1));
    for (label, speed) in speeds {
        // One thread: its CPU time (the machine is shared); threads: the
        // wall clock.
        let cpu = prof::set_cpu_clock(speed.threads == 1);
        let m = model.clone().with_speed(speed);
        let mut best = f64::MAX;
        let mut best_end = f64::MAX;
        let mut phases: std::collections::BTreeMap<String, u64> = Default::default();
        let mut work = (0, 0);
        for _ in 0..5 {
            let mut caches: Vec<TurnCache> = players.iter().map(|_| TurnCache::new()).collect();
            let mut memories: Vec<Memory> = players.iter().map(|_| Memory::new(m.config())).collect();
            for ((seq, cache), memory) in players.iter().zip(&mut caches).zip(&mut memories) {
                m.begin_turn(&seq[0].input(), memory, cache);
                *memory = m.end_turn(cache, memory);
            }
            let _ = prof::drain();
            let (mut spent, mut ended) = (0u64, 0u64);
            let (mut convolved, mut full) = (0, 0);
            for k in 1..players.iter().map(|s| s.len()).max().unwrap_or(0) {
                for ((seq, cache), memory) in players.iter().zip(&mut caches).zip(&mut memories) {
                    let Some(turn) = seq.get(k) else { continue };
                    let t = prof::now_ns();
                    m.begin_turn(&turn.input(), memory, cache);
                    spent += prof::now_ns() - t;
                    convolved += cache.recomputed().1;
                    full += cache.tiles() * m.config().n_conv;
                    let t = prof::now_ns();
                    *memory = m.end_turn(cache, memory);
                    ended += prof::now_ns() - t;
                }
            }
            work = (convolved, full);
            let per = spent as f64 / turns as f64 / 1e6;
            if per < best {
                best = per;
                phases = prof::drain().into_iter().filter(|r| r.path.starts_with("net/model")).map(|r| (r.path, r.incl_ns / turns as u64)).collect();
            }
            best_end = best_end.min(ended as f64 / turns as f64 / 1e6);
        }
        let clock = if cpu { "CPU" } else { "wall" };
        println!("{label} {speed:?}, {clock} clock: begin_turn {best:.3} ms, end_turn {best_end:.3} ms per player-turn; convolutions computed {:.0} %", 100.0 * work.0 as f64 / work.1.max(1) as f64);
        for (path, ns) in &phases {
            println!("    {path}: {:.3} ms", *ns as f64 / 1e6);
        }
    }
    prof::set_cpu_clock(true);
    let last: Vec<&Owned> = players.iter().filter_map(|s| s.last()).collect();
    for (label, speed) in [("plain", Speed::PLAIN), ("simd", fast)] {
        let m = model.clone().with_speed(speed);
        let mut cache = TurnCache::new();
        let mut best = f64::MAX;
        for _ in 0..3 {
            let mut out = Vec::new();
            let (mut count, mut spent) = (0, 0u64);
            for o in &last {
                m.begin_turn(&o.input(), &Memory::new(m.config()), &mut cache);
                let k = o.candidate_meta.len() / 4;
                out.resize(k, 0);
                let t = prof::now_ns();
                for _ in 0..50 {
                    m.score_all(&cache, 1, -1, &o.candidates, &o.candidate_meta, &mut out);
                }
                spent += prof::now_ns() - t;
                count += 50 * k;
            }
            best = best.min(spent as f64 / 1e3 / count.max(1) as f64);
        }
        println!("score ({label}): {best:.2} µs per candidate (CPU clock)");
    }
    prof::set_cpu_clock(false);
}

/// A synthetic turn of a `width × height` map (odd-q hex neighbours, 4×4
/// regions) with `entities` entities and 48 candidates.
fn synthetic(model: &Model, rng: &mut Rng, width: usize, height: usize, entities: usize) -> Owned {
    let cfg = *model.config();
    let n = width * height;
    let mut neighbors = vec![-1i32; n * 6];
    for y in 0..height as i64 {
        for x in 0..width as i64 {
            let odd = x & 1;
            let deltas = [(0, -1), (1, -1 + odd), (1, odd), (0, 1), (-1, odd), (-1, -1 + odd)];
            for (k, (dx, dy)) in deltas.iter().enumerate() {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && nx < width as i64 && ny >= 0 && ny < height as i64 {
                    neighbors[(y as usize * width + x as usize) * 6 + k] = (ny as usize * width + nx as usize) as i32;
                }
            }
        }
    }
    let region = (0..n).map(|t| ((t / width / 4) * width.div_ceil(4) + (t % width) / 4) as i32).collect();
    let value = |rng: &mut Rng| if rng.next().is_multiple_of(50) { rng.range(-32768, 32767) as i16 } else { rng.range(-300, 300) as i16 };
    let candidates = 48;
    Owned {
        tiles: (0..n * cfg.c_tile).map(|_| value(rng)).collect(),
        neighbors,
        region,
        global: (0..cfg.c_global).map(|_| value(rng)).collect(),
        entities: (0..entities * cfg.c_entity).map(|_| value(rng)).collect(),
        meta: (0..entities).flat_map(|_| [rng.range(-1, 8) as i32, rng.range(-1, n as i64) as i32, rng.range(-1, 8) as i32]).collect(),
        ids: (0..entities).map(|e| if e % 7 == 3 { -1 } else { e as i32 + 100 }).collect(),
        coarse: if cfg.coarse { (0..n).map(|t| ((t / width / 16) * width.div_ceil(16) + (t % width) / 16) as i32).collect() } else { Vec::new() },
        aim: if cfg.aim { vec![256, -40, 700] } else { Vec::new() },
        candidates: (0..candidates * cfg.c_candidate).map(|_| value(rng)).collect(),
        candidate_meta: (0..candidates)
            .flat_map(|_| [rng.range(-1, 300) as i32, rng.range(-1, n as i64) as i32, rng.range(-1, entities as i64) as i32, rng.range(-1, 500) as i32])
            .collect(),
    }
}

/// The next turn of a synthetic game: a few tile rows changed (whole runs
/// of them, as units move and borders grow), an entity more or less.
fn evolve(model: &Model, rng: &mut Rng, turn: &Owned) -> Owned {
    let cfg = *model.config();
    let mut next = turn.clone();
    let n = turn.region.len();
    for _ in 0..1 + n / 60 {
        let t = rng.range(0, n as i64 - 1) as usize;
        let c = rng.range(0, cfg.c_tile as i64 - 1) as usize;
        next.tiles[t * cfg.c_tile + c] = rng.range(-300, 300) as i16;
    }
    if rng.next().is_multiple_of(2) && turn.ids.len() > 2 {
        let e = turn.ids.len() - 1;
        next.entities.truncate(e * cfg.c_entity);
        next.meta.truncate(e * 3);
        next.ids.truncate(e);
    } else {
        next.entities.extend((0..cfg.c_entity).map(|_| rng.range(-300, 300) as i16));
        next.meta.extend([0, rng.range(-1, n as i64 - 1) as i32, 1]);
        next.ids.push(10_000 + turn.ids.len() as i32);
    }
    next
}

/// Everything a turn gives: the tile states, the tokens, the value, every
/// candidate's logit per head (with an actor), the memory after it.
fn outputs(model: &Model, turn: &Owned, memory: &Memory, cache: &mut TurnCache) -> (Vec<i16>, Vec<i16>, i32, Vec<i32>, Memory) {
    model.begin_turn(&turn.input(), memory, cache);
    let k = turn.candidate_meta.len() / 4;
    let mut logits = Vec::new();
    for head in 0..model.config().n_decision_heads {
        let mut out = vec![0i32; k];
        model.score_all(cache, head, (head as i32 * 5) % (turn.ids.len() as i32).max(1), &turn.candidates, &turn.candidate_meta, &mut out);
        logits.extend(out);
    }
    let next = model.end_turn(cache, memory);
    (cache.tile_states().to_vec(), cache.tokens().to_vec(), cache.value(), logits, next)
}

/// The speeds held equal to the plain path.
fn speeds() -> Vec<Speed> {
    let fast = Speed::default();
    vec![
        Speed { incremental: false, ..fast },
        fast,
        Speed { threads: 3, ..fast },
        Speed { threads: 4, incremental: false, ..fast },
        Speed { incremental: true, ..Speed::PLAIN },
        Speed { threads: 3, ..Speed::PLAIN },
    ]
}

/// One shape: a synthetic game of several turns on two maps (the caches
/// reused across the map change), every speed against the plain path, each
/// speed with its own cache kept from turn to turn.
fn check_shape(label: &str, header: &Header, shape: [usize; 7], gain: i32, seed: u64) {
    let data = header.write(&mut Rng(seed), shape, gain);
    let plain = Model::from_bytes(&data).expect("the random weights load").with_speed(Speed::PLAIN);
    let fast: Vec<Model> = speeds().into_iter().map(|s| plain.clone().with_speed(s)).collect();
    let mut caches: Vec<TurnCache> = fast.iter().map(|_| TurnCache::new()).collect();
    let mut rng = Rng(seed ^ 0xA5A5);
    let maps = [(24, 18, 40), (13, 9, 12), (24, 18, 40)];
    let mut memory = Memory::new(plain.config());
    let mut checked = 0;
    for (m, &(w, h, ents)) in maps.iter().enumerate() {
        let mut turn = synthetic(&plain, &mut rng, w, h, ents);
        for t in 0..4 {
            let expect = outputs(&plain, &turn, &memory, &mut TurnCache::new());
            for (model, cache) in fast.iter().zip(caches.iter_mut()) {
                let got = outputs(model, &turn, &memory, cache);
                let at = format!("{label}, map {m}, turn {t}, {:?}", model.speed());
                assert_eq!(got.0, expect.0, "{at}: tile states");
                assert_eq!(got.1, expect.1, "{at}: tokens");
                assert_eq!(got.2, expect.2, "{at}: value");
                assert_eq!(got.3, expect.3, "{at}: logits");
                assert_eq!(got.4, expect.4, "{at}: memory");
                checked += 1;
            }
            memory = expect.4;
            turn = evolve(&plain, &mut rng, &turn);
        }
    }
    assert_eq!(checked, 12 * fast.len());
}

/// EXACTNESS: the kernels, threads and incremental tile states give the
/// plain path's integers — tile states, tokens, value, every logit, the
/// memory — on random weights of several shapes: the small and medium
/// presets, odd widths (a tile state of 40, one attention head of 64, wide
/// feed-forward), the flags (engram, aim, coarse regions), and weights loud
/// enough to saturate (the attention's i32 bounds fail and its i64 paths
/// run).
#[test]
fn optimized_paths_equal_the_plain_path_on_random_weights() {
    let mut rng = Rng(11);
    let plain = Header::synthetic(&mut rng, [47, 30, 40, 52], false);
    let flags = Header::synthetic(&mut rng, [21, 9, 17, 33], true);
    check_shape("small", &plain, SMALL, 0, 1);
    check_shape("medium", &plain, MEDIUM, 0, 2);
    check_shape("odd widths", &plain, [40, 2, 64, 1, 1, 200, 24], 0, 3);
    check_shape("flags", &flags, SMALL, 0, 4);
    check_shape("loud", &flags, [48, 3, 64, 2, 4, 96, 32], 4, 5);
    check_shape("loud medium", &plain, MEDIUM, 3, 6);
}

/// The kernels are compiled for this machine (an Apple M-series chip, or
/// another aarch64 CPU with `dotprod`): the tests above compared them, not
/// the portable path with itself.
#[test]
fn the_kernels_run_here() {
    let data = Header::synthetic(&mut Rng(3), [10, 10, 10, 10], false).write(&mut Rng(3), SMALL, 0);
    let model = Model::from_bytes(&data).unwrap();
    assert_eq!(model.simd(), cfg!(target_arch = "aarch64"), "the kernels on aarch64");
}

/// A scorer that plays the fast model (the default speed: kernels and
/// incremental tile states, the mind's cache kept per player) and checks
/// every turn against the plain path on a fresh cache.
#[derive(Debug)]
struct Checked {
    fast: ModelScorer,
    plain: Arc<Model>,
    /// Turns checked, tile embeddings and convolutions computed, and all
    /// of them a full recompute would.
    counts: std::sync::Mutex<[usize; 4]>,
}

impl Scorer for Checked {
    fn label(&self) -> String {
        self.fast.label()
    }
    fn memory(&self) -> Memory {
        self.fast.memory()
    }
    fn coarse(&self) -> bool {
        self.fast.coarse()
    }
    fn layout_hash(&self) -> Option<u64> {
        self.fast.layout_hash()
    }
    fn memory_widths(&self) -> [usize; 3] {
        self.fast.memory_widths()
    }
    fn begin_turn(&self, input: &TurnInput<'_>, memory: &Memory, cache: &mut TurnCache) {
        self.fast.begin_turn(input, memory, cache);
        let mut fresh = TurnCache::new();
        self.plain.begin_turn(input, memory, &mut fresh);
        assert_eq!(cache.tile_states(), fresh.tile_states(), "tile states");
        assert_eq!(cache.tokens(), fresh.tokens(), "tokens");
        assert_eq!(cache.value(), fresh.value(), "value");
        let (embedded, convolved) = cache.recomputed();
        let n_conv = self.plain.config().n_conv;
        let mut counts = self.counts.lock().unwrap();
        counts[0] += 1;
        counts[1] += embedded;
        counts[2] += convolved;
        counts[3] += cache.tiles() * n_conv;
    }
    fn score(&self, cache: &TurnCache, head: usize, actor: i32, candidates: &[Candidate], out: &mut Vec<i32>) {
        self.fast.score(cache, head, actor, candidates, out);
        let mut q = vec![0i16; self.plain.config().d_model];
        self.plain.query(cache, head, actor, &mut q);
        let width = self.plain.config().c_candidate;
        let plain: Vec<i32> = candidates.iter().map(|c| self.plain.score(cache, head, actor, &q, &c.row[..width], &c.meta)).collect();
        assert_eq!(*out, plain, "logits");
    }
    fn end_turn(&self, cache: &TurnCache, memory: &Memory) -> Memory {
        let next = self.fast.end_turn(cache, memory);
        assert_eq!(next, self.plain.end_turn(cache, memory), "memory");
        next
    }
}

/// INCREMENTAL ON A REAL GAME: four nets with the pinned weights play 40
/// rounds; every turn, the fast model on the player's kept cache (its tile
/// states of the previous turn reused) equals the plain path on a fresh
/// cache, and the incremental convolutions computed fewer tiles.
#[test]
fn incremental_tile_states_equal_a_full_recompute_in_a_real_game() {
    let rules = compile_rules(core_rules()).unwrap();
    let data = std::fs::read(pinned_weights()).unwrap();
    let fast = Arc::new(Model::from_bytes(&data).unwrap().with_speed(Speed::default()));
    let plain = Arc::new(Model::from_bytes(&data).unwrap().with_speed(Speed::PLAIN));
    let checked = Arc::new(Checked { fast: ModelScorer::new(fast, "fast"), plain, counts: Default::default() });
    let config = ScenarioConfig { seed: 0x0_9E7A, players: 4, width: 32, height: 24, cities: 1, units: 2, water_ratio: 10 };
    let mut game = generate_new_game(&rules, &config);
    let mut driver = driver(&rules, ClassicMind::with_net(Variant::Net, NetPlayer::new(checked.clone())));
    while round(&game) < 40 && !game.is_over() {
        driver.run(&mut game, &rules, u32::MAX);
    }
    let [turns, embedded, convolved, full] = *checked.counts.lock().unwrap();
    eprintln!("{turns} turns checked; convolutions computed {convolved} of {full} ({:.0} %), embeddings {embedded}", 100.0 * convolved as f64 / full.max(1) as f64);
    assert!(turns >= 150, "{turns} turns");
    assert!(convolved < full, "the incremental tile states saved nothing");
}

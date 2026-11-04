//! The integer forward pass of the end-to-end net equals the trainer's
//! NumPy integer reference bit for bit (classic/docs/ai-net-model.md).
//!
//! The fixtures (`tests/fixtures/net_e2e/`) are written by
//! `python -m aurelia_nn.fixture` in `nn/`: weights files (`small`, the
//! small preset's shape; `wide`, layers wider than the fast paths assume;
//! `flags`, engram, aim and coarse regions; `trace`, the turn trace of
//! weights version 3; `events`, the event cross-attention of version 4, on
//! turns with events; `defer`, the defer choice of version 5, on turns whose
//! unit decisions end with the defer candidate) and two windows of consecutive synthetic turns on
//! maps of two sizes, with the reference's tile states, tokens, value,
//! logits (each decision's choice joining the turn trace after it) and the
//! memory after every turn (carried from turn to turn).

use std::collections::HashMap;
use std::path::PathBuf;

use classic_rules::ai::net_e2e::model::{fnv1a64, Memory, Model, Speed, TurnCache, TurnInput};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/net_e2e")
}

fn weights(name: &str) -> Vec<u8> {
    std::fs::read(fixture_dir().join(format!("{name}.bin"))).expect("the fixture's weights file")
}

/// A turn file: `name count v1 v2 …` per line.
fn read_turn(name: &str) -> Option<HashMap<String, Vec<i64>>> {
    let text = std::fs::read_to_string(fixture_dir().join(name)).ok()?;
    let mut out = HashMap::new();
    for line in text.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let mut parts = line.split_ascii_whitespace();
        let key = parts.next().unwrap().to_string();
        let count: usize = parts.next().unwrap().parse().unwrap();
        let values: Vec<i64> = parts.map(|v| v.parse().unwrap()).collect();
        assert_eq!(values.len(), count, "{key}");
        out.insert(key, values);
    }
    Some(out)
}

fn i16s(v: &[i64]) -> Vec<i16> {
    v.iter().map(|&x| i16::try_from(x).unwrap()).collect()
}

fn i32s(v: &[i64]) -> Vec<i32> {
    v.iter().map(|&x| i32::try_from(x).unwrap()).collect()
}

/// Runs a fixture turn with `memory` and checks every output; returns the
/// memory after the turn.
fn check_turn(model: &Model, file: &str, t: &HashMap<String, Vec<i64>>, memory: &Memory, cache: &mut TurnCache) -> Memory {
    let (tiles, neighbors, region, global) = (i16s(&t["tiles"]), i32s(&t["neighbors"]), i32s(&t["region"]), i16s(&t["global"]));
    let (entities, entity_meta, entity_ids) = (i16s(&t["entities"]), i32s(&t["entity_meta"]), i32s(&t["entity_ids"]));
    let coarse = t.get("coarse").map(|c| i32s(c)).unwrap_or_default();
    let aim = t.get("aim").map(|a| i16s(a)).unwrap_or_default();
    let (events, event_meta) = (t.get("events").map(|e| i16s(e)).unwrap_or_default(), t.get("event_meta").map(|e| i32s(e)).unwrap_or_default());
    let input = TurnInput {
        tiles: &tiles,
        neighbors: &neighbors,
        region: &region,
        global: &global,
        entities: &entities,
        entity_meta: &entity_meta,
        entity_ids: &entity_ids,
        coarse_region: &coarse,
        aim: &aim,
        events: if model.config().events { &events } else { &[] },
        event_meta: if model.config().events { &event_meta } else { &[] },
    };
    model.begin_turn(&input, memory, cache);
    assert_eq!(cache.tile_states(), &i16s(&t["expect_tile_h"])[..], "{file}: tile states");
    assert_eq!(cache.tokens(), &i16s(&t["expect_tokens"])[..], "{file}: tokens");
    assert_eq!(i64::from(cache.value()), t["expect_value"][0], "{file}: value");

    let cc = model.config().c_candidate;
    let (rows, metas) = (i16s(&t["candidates"]), i32s(&t["candidate_meta"]));
    let mut logits = Vec::new();
    for (d, &choice) in t["decisions"].as_chunks::<4>().0.iter().zip(&t["choices"]) {
        let (head, actor, first, count) = (d[0] as usize, d[1] as i32, d[2] as usize, d[3] as usize);
        let mut out = vec![0i32; count];
        model.score_all(cache, head, actor, &rows[first * cc..(first + count) * cc], &metas[first * 4..(first + count) * 4], &mut out);
        logits.extend(out.into_iter().map(i64::from));
        let at = first + choice as usize;
        model.note_choice(cache, head, actor, &rows[at * cc..(at + 1) * cc], &metas[at * 4..(at + 1) * 4]);
    }
    assert_eq!(logits, t["expect_logits"], "{file}: logits");
    if !model.config().trace {
        assert!(cache.trace().iter().all(|&v| v == 0), "{file}: no trace without the trace layer");
    }

    let next = model.end_turn(cache, memory);
    assert_eq!(next.player, i16s(&t["expect_mem_player"]), "{file}: player slot");
    assert_eq!(next.nation_ids, i32s(&t["expect_mem_nation_ids"]), "{file}: nation ids");
    assert_eq!(next.nations, i16s(&t["expect_mem_nations"]), "{file}: nation slots");
    assert_eq!(next.unit_ids, i32s(&t["expect_mem_unit_ids"]), "{file}: unit ids");
    assert_eq!(next.units, i16s(&t["expect_mem_units"]), "{file}: unit slots");
    next
}

/// Every window of a fixture, its turns in order with the memory carried,
/// all through one cache (the windows' maps differ in size), at every speed
/// (`Speed`: the plain path, the kernels with incremental tile states, and
/// three threads).
fn check_fixture(name: &str) -> usize {
    let speeds = [Speed::PLAIN, Speed::default(), Speed { threads: 3, ..Speed::default() }, Speed { threads: 3, ..Speed::PLAIN }];
    let mut turns = 0;
    for speed in speeds {
        let model = Model::from_bytes(&weights(name)).expect("the fixture's weights load").with_speed(speed);
        let mut cache = TurnCache::new();
        turns = 0;
        for w in 0.. {
            let mut memory = Memory::new(model.config());
            let mut t = 0;
            while let Some(turn) = read_turn(&format!("{name}_w{w}_t{t}.txt")) {
                memory = check_turn(&model, &format!("{name}_w{w}_t{t} {speed:?}"), &turn, &memory, &mut cache);
                t += 1;
            }
            if t == 0 {
                break;
            }
            turns += t;
        }
    }
    turns
}

#[test]
fn integer_forward_equals_the_numpy_reference() {
    assert_eq!(check_fixture("small"), 6);
}

#[test]
fn wide_layers_equal_the_numpy_reference() {
    assert_eq!(check_fixture("wide"), 6);
}

#[test]
fn engram_aim_and_coarse_regions_equal_the_numpy_reference() {
    assert_eq!(check_fixture("flags"), 6);
}

/// The events (weights version 4): every token attends over the turn's
/// event tokens, on windows whose turns have 0 … 6 events.
#[test]
fn the_event_attention_equals_the_numpy_reference() {
    assert_eq!(check_fixture("events"), 6);
    let model = Model::from_bytes(&weights("events")).unwrap();
    assert!(model.config().events && model.config().c_event > 0);
    let events: usize = (0..2).flat_map(|w| (0..3).map(move |t| (w, t))).map(|(w, t)| read_turn(&format!("events_w{w}_t{t}.txt")).unwrap()["event_meta"].len() / 4).sum();
    assert!(events >= 6, "{events} events in the windows");
}

/// The version-4 file of the `small` net with the events on and its output
/// layer zero (`small-zero-events.bin`, as the trainer writes a net
/// warm-started from one without events, before a step) computes bit for
/// bit what the version-2 file does without them, at every speed: on the
/// windows with events (whose other arrays are the plain windows') the
/// tokens, value, logits and memory of `small_w*_t*.txt`.
#[test]
fn zero_event_weights_play_the_net_without_events() {
    let v4 = weights("small-zero-events");
    assert_eq!(u32::from_le_bytes(v4[4..8].try_into().unwrap()), 4);
    for speed in [Speed::PLAIN, Speed::default()] {
        let model = Model::from_bytes(&v4).unwrap().with_speed(speed);
        assert!(model.config().events && !model.config().trace);
        let mut cache = TurnCache::new();
        let mut events = 0;
        for w in 0..2 {
            let mut memory = Memory::new(model.config());
            for t in 0..3 {
                let mut turn = read_turn(&format!("small_w{w}_t{t}.txt")).unwrap();
                let with = read_turn(&format!("events_w{w}_t{t}.txt")).unwrap();
                for key in ["tiles", "global", "entities", "entity_meta", "candidates", "candidate_meta"] {
                    assert_eq!(turn[key], with[key], "{key}: the windows differ only by their events");
                }
                events += with["event_meta"].len() / 4;
                turn.insert("events".into(), with["events"].clone());
                turn.insert("event_meta".into(), with["event_meta"].clone());
                memory = check_turn(&model, &format!("small-zero-events w{w} t{t} {speed:?}"), &turn, &memory, &mut cache);
            }
        }
        assert!(events >= 6);
    }
}

#[test]
fn the_turn_trace_equals_the_numpy_reference() {
    assert_eq!(check_fixture("trace"), 6);
    let model = Model::from_bytes(&weights("trace")).unwrap();
    let cfg = model.config();
    assert!(cfg.trace && cfg.traces(0) && cfg.traces(3) && !cfg.traces(4) && !cfg.traces(5));
}

/// The version-3 file of a net whose trace layer is zero
/// (`small-zero-trace.bin`: the `small` net with the trace on, as the
/// trainer writes a net warm-started from a checkpoint without the trace,
/// before a step) computes bit for bit what the version-2 file does, at
/// every speed, while its trace sums the choices.
#[test]
fn a_zero_trace_plays_the_version_2_net() {
    let (v2, v3) = (weights("small"), weights("small-zero-trace"));
    assert_eq!(u32::from_le_bytes(v2[4..8].try_into().unwrap()), 2);
    assert_eq!(u32::from_le_bytes(v3[4..8].try_into().unwrap()), 3);
    for speed in [Speed::PLAIN, Speed::default()] {
        let (plain, traced) = (Model::from_bytes(&v2).unwrap().with_speed(speed), Model::from_bytes(&v3).unwrap().with_speed(speed));
        assert!(traced.config().trace && !plain.config().trace);
        let (mut a, mut b) = (TurnCache::new(), TurnCache::new());
        for w in 0..2 {
            let (mut ma, mut mb) = (Memory::new(plain.config()), Memory::new(traced.config()));
            for t in 0..3 {
                let turn = read_turn(&format!("small_w{w}_t{t}.txt")).unwrap();
                let la = run_turn(&plain, &turn, &ma, &mut a);
                let lb = run_turn(&traced, &turn, &mb, &mut b);
                assert_eq!(la, lb, "window {w} turn {t} {speed:?}");
                assert!(b.trace().iter().any(|&v| v != 0) && a.trace().iter().all(|&v| v == 0), "only the version-3 net sums the choices");
                (ma, mb) = (plain.end_turn(&a, &ma), traced.end_turn(&b, &mb));
                assert_eq!(ma, mb);
            }
        }
    }
}

/// A fixture turn's logits, each choice noted after its decision.
fn run_turn(model: &Model, t: &HashMap<String, Vec<i64>>, memory: &Memory, cache: &mut TurnCache) -> Vec<i32> {
    let (tiles, neighbors, region, global) = (i16s(&t["tiles"]), i32s(&t["neighbors"]), i32s(&t["region"]), i16s(&t["global"]));
    let (entities, entity_meta, entity_ids) = (i16s(&t["entities"]), i32s(&t["entity_meta"]), i32s(&t["entity_ids"]));
    let input = TurnInput { tiles: &tiles, neighbors: &neighbors, region: &region, global: &global, entities: &entities, entity_meta: &entity_meta, entity_ids: &entity_ids, coarse_region: &[], aim: &[], events: &[], event_meta: &[] };
    model.begin_turn(&input, memory, cache);
    assert_eq!(cache.trace(), &vec![0i64; model.config().d_model][..], "the trace starts at zero");
    let cc = model.config().c_candidate;
    let (rows, metas) = (i16s(&t["candidates"]), i32s(&t["candidate_meta"]));
    let mut logits = Vec::new();
    for (d, &choice) in t["decisions"].as_chunks::<4>().0.iter().zip(&t["choices"]) {
        let (head, actor, first, count) = (d[0] as usize, d[1] as i32, d[2] as usize, d[3] as usize);
        let mut out = vec![0i32; count];
        model.score_all(cache, head, actor, &rows[first * cc..(first + count) * cc], &metas[first * 4..(first + count) * 4], &mut out);
        logits.extend(out);
        let at = first + choice as usize;
        model.note_choice(cache, head, actor, &rows[at * cc..(at + 1) * cc], &metas[at * 4..(at + 1) * 4]);
    }
    logits
}

#[test]
fn the_defer_choice_equals_the_numpy_reference() {
    assert_eq!(check_fixture("defer"), 6);
    let data = weights("defer");
    assert_eq!(u32::from_le_bytes(data[4..8].try_into().unwrap()), 5);
    let model = Model::from_bytes(&data).unwrap();
    assert!(model.config().defer && model.config().trace);
    assert_eq!(model.defer_bias(), 384, "1.5 logits");
}

/// The defer net (`defer.bin`: the `trace` net with the defer choice on,
/// its tag row of the defer bucket zero and a bias) scores every other
/// candidate as the trace net does: on the trace net's turns, which have no
/// defer candidate, it computes the trace fixture's every output; on its
/// own turns the bias is all the defer candidates' logits gain.
#[test]
fn the_defer_choice_leaves_the_other_candidates_alone() {
    let defer = Model::from_bytes(&weights("defer")).unwrap();
    let mut cache = TurnCache::new();
    for w in 0..2 {
        let mut memory = Memory::new(defer.config());
        for t in 0..3 {
            let turn = read_turn(&format!("trace_w{w}_t{t}.txt")).unwrap();
            memory = check_turn(&defer, &format!("trace_w{w}_t{t} on the defer net"), &turn, &memory, &mut cache);
        }
    }
    let unbiased = Model::from_bytes(&weights("defer")).unwrap().with_defer(Some(0));
    let (mut a, mut b) = (TurnCache::new(), TurnCache::new());
    let mut deferred = 0;
    for w in 0..2 {
        let (mut ma, mut mb) = (Memory::new(defer.config()), Memory::new(defer.config()));
        for t in 0..3 {
            let turn = read_turn(&format!("defer_w{w}_t{t}.txt")).unwrap();
            let (la, lb) = (run_turn(&defer, &turn, &ma, &mut a), run_turn(&unbiased, &turn, &mb, &mut b));
            let tags: Vec<i64> = turn["candidate_meta"].as_chunks::<4>().0.iter().map(|m| m[0]).collect();
            for ((x, y), tag) in la.iter().zip(&lb).zip(&tags) {
                let defer_tag = i64::from(classic_rules::ai::net_e2e::encode::tag::DEFER);
                assert_eq!(*x, y + if *tag == defer_tag { 384 } else { 0 }, "window {w} turn {t}");
                deferred += usize::from(*tag == defer_tag);
            }
            (ma, mb) = (defer.end_turn(&a, &ma), unbiased.end_turn(&b, &mb));
        }
    }
    assert!(deferred >= 6, "{deferred} defer candidates");
}

#[test]
fn the_layout_hash_is_fnv1a64() {
    // The same values as nn/tests (`fnv1a64` of the trainer).
    assert_eq!(fnv1a64(b""), 0xCBF2_9CE4_8422_2325);
    assert_eq!(fnv1a64(b"a"), 0xAF63_DC4C_8601_EC8C);
    let model = Model::from_bytes(&weights("small")).unwrap();
    assert_ne!(model.layout_hash(), 0);
}

#[test]
fn a_damaged_weights_file_is_refused() {
    let data = weights("small");
    assert!(Model::from_bytes(&data[..data.len() - 1]).is_err(), "truncated");
    let mut bad = data.clone();
    bad[0] = b'X';
    assert!(Model::from_bytes(&bad).is_err(), "bad magic");
    let mut longer = data.clone();
    longer.push(0);
    assert!(Model::from_bytes(&longer).is_err(), "trailing bytes");
    let mut version = data;
    version[4] = 99;
    assert!(Model::from_bytes(&version).is_err(), "unknown version");
}

/// A deterministic synthetic turn of a map size: odd-q hex neighbours,
/// 4×4 regions, `entities` entities.
struct Synthetic {
    tiles: Vec<i16>,
    neighbors: Vec<i32>,
    region: Vec<i32>,
    global: Vec<i16>,
    entities: Vec<i16>,
    entity_meta: Vec<i32>,
    entity_ids: Vec<i32>,
    coarse: Vec<i32>,
    candidates: Vec<i16>,
    candidate_meta: Vec<i32>,
}

fn synthetic(model: &Model, width: usize, height: usize, entities: usize, candidates: usize) -> Synthetic {
    let cfg = *model.config();
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move |m: i64| -> i64 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as i64).rem_euclid(m)
    };
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
    let rw = width.div_ceil(4);
    let region = (0..n).map(|t| ((t / width / 4) * rw + (t % width) / 4) as i32).collect();
    Synthetic {
        tiles: (0..n * cfg.c_tile).map(|_| next(200) as i16 - 100).collect(),
        neighbors,
        region,
        global: (0..cfg.c_global).map(|_| next(200) as i16 - 100).collect(),
        entities: (0..entities * cfg.c_entity).map(|_| next(200) as i16 - 100).collect(),
        entity_meta: (0..entities).flat_map(|_| [next(6) as i32, next(n as i64) as i32, next(7) as i32]).collect(),
        entity_ids: (0..entities as i32).collect(),
        coarse: if cfg.coarse { (0..n).map(|t| ((t / width / 16) * width.div_ceil(16) + (t % width) / 16) as i32).collect() } else { Vec::new() },
        candidates: (0..candidates * cfg.c_candidate).map(|_| next(200) as i16 - 100).collect(),
        candidate_meta: (0..candidates).flat_map(|_| [next(10) as i32, next(n as i64) as i32, next(entities as i64) as i32, next(100) as i32]).collect(),
    }
}

/// Timings of the release build (`cargo test --release -p classic-rules
/// --test net_e2e_model -- --ignored --nocapture`); weights from
/// `NET_E2E_WEIGHTS` (one path, or several separated by `:`) or the small
/// fixture's; maps from `NET_E2E_MAPS` (`64x48x150,104x64x300`: width,
/// height, entities).
#[test]
#[ignore]
fn timings() {
    let files: Vec<Vec<u8>> = match std::env::var("NET_E2E_WEIGHTS") {
        Ok(paths) => paths.split(':').map(|p| std::fs::read(p).unwrap()).collect(),
        Err(_) => vec![weights("small")],
    };
    let maps: Vec<(usize, usize, usize)> = std::env::var("NET_E2E_MAPS")
        .map(|m| {
            m.split(',')
                .map(|s| {
                    let v: Vec<usize> = s.split('x').map(|x| x.parse().unwrap()).collect();
                    (v[0], v[1], v[2])
                })
                .collect()
        })
        .unwrap_or_else(|_| vec![(64, 48, 150), (104, 64, 300), (128, 80, 400)]);
    // The same input every repetition: incremental tile states would reuse
    // them all, so both speeds recompute every tile.
    let speeds = [Speed::PLAIN, Speed { incremental: false, ..Speed::default() }];
    for (data, speed) in files.iter().flat_map(|d| speeds.map(|s| (d, s))) {
    let model = Model::from_bytes(data).unwrap().with_speed(speed);
    let cfg = *model.config();
    println!("{speed:?}");
    println!("weights: {} bytes, d_tile {} × {} conv, d_model {} × {} attention ({} heads), d_ff {}", data.len(), cfg.d_tile, cfg.n_conv, cfg.d_model, cfg.n_attn, cfg.n_attn_heads, cfg.d_ff);
    for &(w, h, ents) in &maps {
        let s = synthetic(&model, w, h, ents, 64);
        let input = TurnInput { tiles: &s.tiles, neighbors: &s.neighbors, region: &s.region, global: &s.global, entities: &s.entities, entity_meta: &s.entity_meta, entity_ids: &s.entity_ids, coarse_region: &s.coarse, aim: &[], events: &[], event_meta: &[] };
        let mut turn = TurnCache::new();
        let mut memory = Memory::new(model.config());
        model.begin_turn(&input, &memory, &mut turn);
        memory = model.end_turn(&turn, &memory);
        let reps = 5;
        let start = std::time::Instant::now();
        for _ in 0..reps {
            model.begin_turn(&input, &memory, &mut turn);
        }
        let encode = start.elapsed() / reps;
        let start = std::time::Instant::now();
        for _ in 0..reps {
            memory = model.end_turn(&turn, &memory);
        }
        let end = start.elapsed() / reps;
        let mut out = vec![0i32; 64];
        let reps = 2000;
        let start = std::time::Instant::now();
        let mut sink = 0i64;
        for r in 0..reps {
            model.score_all(&turn, r % 4, (r % ents) as i32, &s.candidates, &s.candidate_meta, &mut out);
            sink += i64::from(out[r % 64]);
        }
        let per_candidate = start.elapsed() / (reps as u32 * 64);
        println!(
            "{w}×{h}: {} tiles, {} regions, {ents} entities, {} tokens: begin_turn {:?}; end_turn {:?}; score {:?} per candidate (sink {sink})",
            w * h,
            turn.regions(),
            turn.tokens().len() / cfg.d_model,
            encode,
            end,
            per_candidate
        );
    }
    }
}

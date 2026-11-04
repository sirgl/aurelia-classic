//! v2 in process against PyTorch (classic/docs/ai-net-model.md, «v2
//! in-process (GPU)»): replays a fixture (`python -m aurelia_nn.export_v2
//! fixture`) through the v2 scorer and prints the agreement as JSON.
//!
//! cargo run --release -p classic-rules [--features gpu] --example v2_check -- W.v2 F.v2x [--cpu] [--int8] [--trunks] [--timing=GAP_MS]
//!
//! `--trunks` also runs every turn's trunk on the CPU reference and the
//! GPU and prints the largest differences of their outputs.

use std::path::Path;
use std::sync::Arc;

use classic_rules::ai::net_e2e::v2::{fixture, trunk, Engine, V2Scorer};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let (Some(weights), Some(fx)) = (pos.first(), pos.get(1)) else {
        eprintln!("usage: v2_check W.v2 F.v2x [--cpu] [--trunks]");
        std::process::exit(2);
    };
    let gpu = !flag("--cpu");
    let engine = Arc::new(Engine::new(Path::new(weights), gpu).unwrap_or_else(|e| panic!("{e}")));
    let c = &engine.net.cfg;
    let widths = fixture::Widths { c_tile: c.c_tile, c_global: c.c_global, c_entity: c.c_entity, c_candidate: c.c_candidate, c_event: c.c_event };
    let windows = fixture::load(Path::new(fx), widths).unwrap_or_else(|e| panic!("{e}"));
    {
        let turns: Vec<&fixture::Turn> = windows.iter().flatten().collect();
        let cands: usize = turns.iter().flat_map(|t| &t.decisions).map(|d| d.candidates.len()).sum();
        let decs: usize = turns.iter().map(|t| t.decisions.len()).sum();
        let ents: usize = turns.iter().map(|t| t.entity_ids.len()).sum();
        let tiles: usize = turns.iter().map(|t| t.tiles.len() / c.c_tile).sum();
        println!(
            "{{\"turns\":{},\"decisions_per_turn\":{:.1},\"candidates_per_decision\":{:.1},\"entities_per_turn\":{:.1},\"tiles_per_turn\":{:.0}}}",
            turns.len(),
            decs as f64 / turns.len().max(1) as f64,
            cands as f64 / decs.max(1) as f64,
            ents as f64 / turns.len().max(1) as f64,
            tiles as f64 / turns.len().max(1) as f64
        );
    }
    if flag("--trunks") {
        trunks(&engine, &windows);
    }
    #[cfg(feature = "gpu")]
    if flag("--pairs") {
        pairs(&engine, &windows);
    }
    if flag("--batch-invariance") {
        batch_invariance(&engine, &windows);
    }
    if let Some(g) = args.iter().find_map(|a| a.strip_prefix("--timing=")) {
        timing(&engine, &windows, g.parse().expect("--timing=GAP_MS"));
        return;
    }
    if let Some(n) = args.iter().find_map(|a| a.strip_prefix("--parallel=")) {
        parallel(&engine, &windows, n.parse().expect("--parallel=N"));
    }
    let scorer = V2Scorer::new(engine.clone(), None);
    // `--int8`: the decisions' int8 products (their agreement with f32 below).
    let scorer = if flag("--int8") { scorer.with_int8() } else { scorer };
    // FNV-1a of every logit's bits: the same number means the same floats.
    let hash = std::cell::Cell::new(0xcbf2_9ce4_8422_2325u64);
    let rep = fixture::replay(&windows, &scorer, &|cache| {
        let r = scorer.raw_logits(cache);
        for v in r.iter().flatten() {
            for b in v.to_bits().to_le_bytes() {
                hash.set((hash.get() ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
            }
        }
        r
    });
    println!("{{\"logits_hash\":\"{:016x}\"}}", hash.get());
    if let Some(s) = engine.gpu_stats() {
        println!("{s}");
    }
    if std::env::var_os("AURELIA_V2_DECIDE_PROFILE").is_some() {
        let names = ["context", "encodings", "query", "buckets_fields", "attn_query", "actor_kv", "scores", "output", "heads"];
        let ms = classic_rules::ai::net_e2e::v2::decide::prof::report();
        let parts: Vec<String> = names.iter().zip(&ms).map(|(n, v)| format!("\"{n}\":{v:.1}")).collect();
        println!("{{\"decide_ms_total\":{{{}}}}}", parts.join(","));
    }
    let heads: Vec<String> = rep.by_head.iter().enumerate().filter(|(_, h)| h[0] > 0).map(|(i, h)| format!("\"{i}\":[{},{}]", h[1], h[0])).collect();
    println!(
        "{{\"fixture\":{:?},\"backend\":\"{:?}\",\"windows\":{},\"turns\":{},\"decisions\":{},\"agree_offline\":{},\"agree_server\":{},\"near_ties_of_disagreements\":{},\"max_abs_logit_diff\":{:.6},\"mean_abs_logit_diff\":{:.7},\"max_value_diff_q8\":{},\"by_head\":{{{}}},\"begin_ms_mean\":{:.3},\"score_ms_mean\":{:.4}}}",
        fx,
        engine.backend,
        rep.windows,
        rep.turns,
        rep.decisions,
        rep.agree_offline,
        rep.agree_server,
        rep.near_ties,
        rep.max_abs_diff,
        rep.mean_abs_diff,
        rep.max_value_diff,
        heads.join(","),
        rep.begin_ms / rep.turns.max(1) as f64,
        rep.score_ms / rep.decisions.max(1) as f64
    );
}

/// Each turn's trunk on the CPU and on the engine's backend (zero memory: a
/// check of the kernels, not of the sessions).
fn trunks(engine: &Engine, windows: &[Vec<fixture::Turn>]) {
    let net = &engine.net;
    let dil: Vec<usize> = net.map.iter().map(|b| b.dilation).collect();
    let mut worst = [0.0f32; 6];
    let mut n = 0;
    for w in windows {
        let map = Arc::new(trunk::MapPrep::new(&w[0].neighbors, &w[0].region, &dil));
        for t in w {
            let tables = trunk::Tables { tiles: &t.tiles, global: &t.global, entities: &t.entities, entity_meta: &t.entity_meta, events: &t.events, event_meta: &t.event_meta };
            let job = Arc::new(trunk::Job::prepare(net, map.clone(), &tables, &vec![0.0; net.cfg.mem_player], &|_, _| None));
            let a = trunk::run_cpu(net, &job);
            let b = engine.trunk(&job);
            let md = |x: &[f32], y: &[f32]| x.iter().zip(y).map(|(p, q)| (p - q).abs()).fold(0.0f32, f32::max);
            for (k, v) in [md(&a.tile_h, &b.tile_h), md(&a.tokens, &b.tokens), (a.value - b.value).abs(), md(&a.ent_kv, &b.ent_kv), md(&a.dealt, &b.dealt), md(&a.new_player, &b.new_player)].into_iter().enumerate() {
                worst[k] = worst[k].max(v);
            }
            n += 1;
        }
    }
    println!(
        "{{\"trunks\":{n},\"max_diff\":{{\"tile_h\":{:.3e},\"tokens\":{:.3e},\"value\":{:.3e},\"ent_kv\":{:.3e},\"dealt\":{:.3e},\"new_player\":{:.3e}}}}}",
        worst[0], worst[1], worst[2], worst[3], worst[4], worst[5]
    );
}

/// The trunk of every fourth turn, each run 5 times with `gap` ms between
/// calls (0: the GPU kept busy; ~5: idle as between a game's turns): the
/// median and the 90th percentile of a call's wall time.
fn timing(engine: &Engine, windows: &[Vec<fixture::Turn>], gap: f64) {
    let net = &engine.net;
    let dil: Vec<usize> = net.map.iter().map(|b| b.dilation).collect();
    let mut ms = Vec::new();
    for w in windows {
        let map = Arc::new(trunk::MapPrep::new(&w[0].neighbors, &w[0].region, &dil));
        for t in w.iter().step_by(4) {
            let tables = trunk::Tables { tiles: &t.tiles, global: &t.global, entities: &t.entities, entity_meta: &t.entity_meta, events: &t.events, event_meta: &t.event_meta };
            let job = Arc::new(trunk::Job::prepare(net, map.clone(), &tables, &vec![0.0; net.cfg.mem_player], &|_, _| None));
            for _ in 0..5 {
                if gap > 0.0 {
                    // A spin, as a game's thread computing its decisions (`AURELIA_V2_TIMING_SLEEP`: a sleep).
                    let until = std::time::Instant::now() + std::time::Duration::from_secs_f64(gap / 1e3);
                    if std::env::var_os("AURELIA_V2_TIMING_SLEEP").is_some() {
                        std::thread::sleep(until - std::time::Instant::now());
                    }
                    while std::time::Instant::now() < until {
                        std::hint::spin_loop();
                    }
                }
                let t0 = std::time::Instant::now();
                let _ = engine.trunk(&job);
                ms.push(t0.elapsed().as_secs_f64() * 1e3);
            }
        }
    }
    ms.sort_by(f64::total_cmp);
    if let Some(s) = engine.gpu_stats() {
        println!("{s}");
    }
    println!("{{\"timing_gap_ms\":{gap},\"calls\":{},\"p50_ms\":{:.3},\"p90_ms\":{:.3}}}", ms.len(), ms[ms.len() / 2], ms[ms.len() * 9 / 10]);
}

/// The turns' trunks from `n` threads at once (the GPU batches them): turns per second.
fn parallel(engine: &Arc<Engine>, windows: &[Vec<fixture::Turn>], n: usize) {
    let net = &engine.net;
    let dil: Vec<usize> = net.map.iter().map(|b| b.dilation).collect();
    let mut jobs = Vec::new();
    for w in windows {
        let map = Arc::new(trunk::MapPrep::new(&w[0].neighbors, &w[0].region, &dil));
        for t in w {
            let tables = trunk::Tables { tiles: &t.tiles, global: &t.global, entities: &t.entities, entity_meta: &t.entity_meta, events: &t.events, event_meta: &t.event_meta };
            jobs.push(Arc::new(trunk::Job::prepare(net, map.clone(), &tables, &vec![0.0; net.cfg.mem_player], &|_, _| None)));
        }
    }
    let start = std::time::Instant::now();
    let rounds = 4;
    std::thread::scope(|s| {
        for k in 0..n {
            let jobs = &jobs;
            s.spawn(move || {
                for r in 0..rounds {
                    let _ = engine.trunk(&jobs[(k * 7 + r * 13) % jobs.len()]);
                }
            });
        }
    });
    let secs = start.elapsed().as_secs_f64();
    println!("{{\"parallel\":{n},\"trunks\":{},\"seconds\":{secs:.3},\"trunks_per_second\":{:.1}}}", n * rounds, (n * rounds) as f64 / secs);
    if let Some(s) = engine.gpu_stats() {
        println!("{s}");
    }
}

/// Every turn's trunk alone, then all of them from 32 threads at once (batched
/// with each other) and again alone: the outputs must be the same bytes.
fn batch_invariance(engine: &Arc<Engine>, windows: &[Vec<fixture::Turn>]) {
    let net = &engine.net;
    let dil: Vec<usize> = net.map.iter().map(|b| b.dilation).collect();
    let mut jobs = Vec::new();
    for w in windows {
        let map = Arc::new(trunk::MapPrep::new(&w[0].neighbors, &w[0].region, &dil));
        for t in w {
            let tables = trunk::Tables { tiles: &t.tiles, global: &t.global, entities: &t.entities, entity_meta: &t.entity_meta, events: &t.events, event_meta: &t.event_meta };
            jobs.push(Arc::new(trunk::Job::prepare(net, map.clone(), &tables, &vec![0.0; net.cfg.mem_player], &|_, _| None)));
        }
    }
    // Per field (tile_h, tokens, new_player, new_unit, new_nation, ent_kv, p_ent, att_side, dfd_side, dealt, focus, value): the bits.
    let flat = |o: &trunk::TrunkOut| -> Vec<Vec<u32>> {
        let mut v: Vec<Vec<u32>> = [&o.tile_h, &o.tokens, &o.new_player, &o.new_unit, &o.new_nation, &o.ent_kv, &o.p_ent, &o.att_side, &o.dfd_side, &o.dealt, &o.focus]
            .iter()
            .map(|v| v.iter().map(|x| x.to_bits()).collect())
            .collect();
        v.push(vec![o.value.to_bits()]);
        v
    };
    let alone: Vec<Vec<Vec<u32>>> = jobs.iter().map(|j| flat(&engine.trunk(j))).collect();
    let together: Vec<Vec<Vec<u32>>> = std::thread::scope(|s| {
        let hs: Vec<_> = jobs.chunks(jobs.len().div_ceil(32)).map(|c| s.spawn(move || c.iter().map(|j| flat(&engine.trunk(j))).collect::<Vec<_>>())).collect();
        hs.into_iter().flat_map(|h| h.join().expect("a thread")).collect()
    });
    let again: Vec<Vec<Vec<u32>>> = jobs.iter().rev().map(|j| flat(&engine.trunk(j))).collect::<Vec<_>>().into_iter().rev().collect();
    let same_t = alone.iter().zip(&together).filter(|(a, b)| a == b).count();
    let same_a = alone.iter().zip(&again).filter(|(a, b)| a == b).count();
    let maxd: Vec<f32> = (0..12)
        .map(|f| alone.iter().zip(&together).flat_map(|(a, b)| a[f].iter().zip(&b[f]).map(|(x, y)| (f32::from_bits(*x) - f32::from_bits(*y)).abs())).fold(0.0, f32::max))
        .collect();
    println!("{{\"max_abs_diff_per_field\":{maxd:?}}}");
    let per_field: Vec<usize> = (0..12).map(|f| alone.iter().zip(&together).filter(|(a, b)| a[f] == b[f]).count()).collect();
    println!("{{\"batch_invariance\":{{\"turns\":{},\"same_alone_and_batched\":{same_t},\"same_alone_twice\":{same_a},\"per_field\":{per_field:?}}}}}", jobs.len());
    if let Some(s) = engine.gpu_stats() {
        println!("{s}");
    }
}

/// The first turn alone, twice in one batch, and with another turn before it.
#[cfg(feature = "gpu")]
fn pairs(engine: &Arc<Engine>, windows: &[Vec<fixture::Turn>]) {
    let net = &engine.net;
    let dil: Vec<usize> = net.map.iter().map(|b| b.dilation).collect();
    let mk = |t: &fixture::Turn| {
        let map = Arc::new(trunk::MapPrep::new(&t.neighbors, &t.region, &dil));
        let tables = trunk::Tables { tiles: &t.tiles, global: &t.global, entities: &t.entities, entity_meta: &t.entity_meta, events: &t.events, event_meta: &t.event_meta };
        trunk::Job::prepare(net, map, &tables, &vec![0.0; net.cfg.mem_player], &|_, _| None)
    };
    let a = mk(&windows[0][5]);
    let b = mk(&windows[1][7]);
    let mut gpu = pollster::block_on(classic_rules::ai::net_e2e::v2::gpu::Gpu::new(net.clone())).expect("a GPU");
    let d = |x: &trunk::TrunkOut, y: &trunk::TrunkOut| {
        let f = |p: &[f32], q: &[f32]| p.iter().zip(q).map(|(u, v)| (u - v).abs()).fold(0.0f32, f32::max);
        (f(&x.tile_h, &y.tile_h), f(&x.tokens, &y.tokens))
    };
    let alone = pollster::block_on(gpu.run(&[&a])).remove(0);
    let twice = pollster::block_on(gpu.run(&[&a, &a]));
    let after = pollster::block_on(gpu.run(&[&b, &a])).remove(1);
    println!("alone vs first of two: {:?}; vs second of two: {:?}; vs after another: {:?}", d(&alone, &twice[0]), d(&alone, &twice[1]), d(&alone, &after));
    // Token rows that differ (alone vs after another).
    let dm = net.cfg.d_model;
    let rows: Vec<usize> = (0..alone.tokens.len() / dm).filter(|r| alone.tokens[r * dm..(r + 1) * dm] != after.tokens[r * dm..(r + 1) * dm]).collect();
    println!("tokens {} (regions {}, entities {}, slots {}); rows differing: {:?}", alone.tokens.len() / dm, a.map().n_regions, a.n_ent, a.slots.len(), &rows[..rows.len().min(40)]);
}

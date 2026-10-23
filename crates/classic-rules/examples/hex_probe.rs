//! Hex-grid probes for platform/docs/hex-audit.md: how our grid code compares with
//! the Red Blob Games hexagon guide (neighbours, discs, lines, sight, A*,
//! the AI's map search). Not a gate: it prints numbers (min of several
//! batches, because the machine is shared) and checks a few equalities.
//!
//! `hex_probe [all|grid|sight|astar|ai]`
use classic_rules::derived::visibility::{covered_tiles, los_clear};
use classic_rules::ids::{PlayerId, TileId};
use classic_rules::math::{hex_disc, tiles_in_hex_radius, HexCoord, MapDimensions, HEX_DIRS};
use classic_rules::navigation::{find_path, MoveProfile, PathRequest, PathScratch};
use classic_rules::rules::SourceRules;
use classic_rules::scenario::{generate, ScenarioConfig};
use classic_rules::{compile_rules, GameState};
use std::hint::black_box;
use std::time::Instant;

/// Minimum over `batches` of the time of `f` (ns), `f` returns a checksum.
fn best_ns(batches: usize, mut f: impl FnMut() -> u64) -> (f64, u64) {
    let mut best = f64::MAX;
    let mut sum = 0;
    for _ in 0..batches {
        let t0 = Instant::now();
        sum = black_box(f());
        best = best.min(t0.elapsed().as_secs_f64() * 1e9);
    }
    (best, sum)
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// The tight-range disc of the guide (loop bounds from the three
/// inequalities), in the order of `hex_disc` (q outer, r inner).
fn tight_disc(dims: MapDimensions, center: TileId, radius: i32, mut visit: impl FnMut(TileId)) {
    let Some(c) = dims.to_coord(center) else { return };
    for dq in -radius..=radius {
        for dr in (-radius).max(-dq - radius)..=radius.min(-dq + radius) {
            if let Some(t) = dims.to_tile(HexCoord { q: c.q + dq, r: c.r + dr }) {
                visit(t);
            }
        }
    }
}

/// The same with the bounds test hoisted: inside the map only index
/// arithmetic, edges fall back to the checked path.
fn interior_disc(dims: MapDimensions, center: TileId, radius: i32, mut visit: impl FnMut(TileId)) {
    let Some(c) = dims.to_coord(center) else { return };
    let w = dims.width as i32;
    let inside = c.q - radius >= 0 && c.r - radius >= 0 && c.q + radius < w && c.r + radius < dims.height as i32;
    if !inside {
        return tight_disc(dims, center, radius, visit);
    }
    for dq in -radius..=radius {
        for dr in (-radius).max(-dq - radius)..=radius.min(-dq + radius) {
            visit(TileId(((c.r + dr) * w + c.q + dq) as u32));
        }
    }
}

fn game() -> (GameState, classic_rules::CompiledRuleset) {
    let rules = compile_rules(SourceRules::default()).unwrap();
    let g = generate(&ScenarioConfig::representative_late_game(), &rules);
    (g, rules)
}

fn grid(game: &GameState) {
    let dims = game.view().auth.map().dimensions;
    let n = dims.tile_count() as u32;
    println!("## grid {}x{} = {} tiles", dims.width, dims.height, n);
    // Neighbours: current (div/mod + 6 bounds checks) vs a table.
    let table: Vec<[u32; 6]> = (0..n)
        .map(|i| {
            let mut row = [u32::MAX; 6];
            for (k, t) in dims.neighbors(TileId(i)).into_iter().enumerate() {
                row[k] = t.0;
            }
            row
        })
        .collect();
    let (cur, a) = best_ns(15, || {
        let mut s = 0u64;
        for i in 0..n {
            for t in dims.neighbors(TileId(i)) {
                s += t.0 as u64;
            }
        }
        s
    });
    let (tab, b) = best_ns(15, || {
        let mut s = 0u64;
        for i in 0..n {
            for &t in &table[i as usize] {
                if t != u32::MAX {
                    s += t as u64;
                }
            }
        }
        s
    });
    assert_eq!(a, b);
    println!("neighbours of all tiles: current {:.1} ns/tile, table {:.1} ns/tile ({:.1}x)", cur / n as f64, tab / n as f64, cur / tab);
    // Discs.
    for radius in [1, 2, 3, 5] {
        let centers: Vec<TileId> = (0..n).step_by(7).map(TileId).collect();
        let k = centers.len() as f64;
        let (vec_ns, s0) = best_ns(9, || centers.iter().map(|c| tiles_in_hex_radius(&dims, *c, radius).iter().map(|t| t.0 as u64).sum::<u64>()).sum());
        let (cur_ns, s1) = best_ns(9, || centers.iter().map(|c| hex_disc(dims, *c, radius).map(|t| t.0 as u64).sum::<u64>()).sum());
        let (tight_ns, s2) = best_ns(9, || {
            let mut s = 0;
            for c in &centers {
                tight_disc(dims, *c, radius, |t| s += t.0 as u64);
            }
            s
        });
        let (int_ns, s3) = best_ns(9, || {
            let mut s = 0;
            for c in &centers {
                interior_disc(dims, *c, radius, |t| s += t.0 as u64);
            }
            s
        });
        assert!(s0 == s1 && s1 == s2 && s2 == s3, "the discs differ");
        println!(
            "disc r={radius}: Vec {:.0} ns, hex_disc {:.0} ns, tight range {:.0} ns, interior fast path {:.0} ns per disc",
            vec_ns / k,
            cur_ns / k,
            tight_ns / k,
            int_ns / k
        );
    }
    // Same order, edge tiles included (the replacement must be exact).
    for c in [0, 1, dims.width - 1, dims.width, n / 2, n - 1] {
        for radius in 0..=6 {
            let a: Vec<TileId> = hex_disc(dims, TileId(c), radius).collect();
            let mut b = Vec::new();
            interior_disc(dims, TileId(c), radius, |t| b.push(t));
            assert_eq!(a, b, "center {c} radius {radius}");
        }
    }
    println!("disc replacement identical (same tiles, same order) at corners/edges: ok");
}

/// The guide's float line: lerp of the nudged endpoints, cube-rounded.
fn guide_line(a: HexCoord, b: HexCoord) -> Vec<HexCoord> {
    guide_line_eps(a, b, 1e-6)
}

fn guide_line_eps(a: HexCoord, b: HexCoord, eps: f64) -> Vec<HexCoord> {
    let n = a.distance(b);
    let cube = |h: HexCoord| [h.q as f64 + eps, h.r as f64 + eps, -(h.q as f64) - h.r as f64 - 2.0 * eps];
    let (ca, cb) = (cube(a), cube(b));
    let step = 1.0 / n.max(1) as f64;
    (0..=n)
        .map(|i| {
            let t = step * i as f64;
            let f = [0, 1, 2].map(|k| ca[k] * (1.0 - t) + cb[k] * t);
            let mut r = f.map(|v| v.round());
            let d = [0, 1, 2].map(|k| (r[k] - f[k]).abs());
            if d[0] > d[1] && d[0] > d[2] {
                r[0] = -r[1] - r[2];
            } else if d[1] > d[2] {
                r[1] = -r[0] - r[2];
            } else {
                r[2] = -r[0] - r[1];
            }
            HexCoord { q: r[0] as i32, r: r[1] as i32 }
        })
        .collect()
}

fn sight(game: &GameState) {
    let map = game.view().auth.map();
    let dims = map.dimensions;
    println!("## sight / lines");
    // Line equivalence with the guide's nudged float line on an open map.
    let big = MapDimensions { width: 64, height: 64 };
    let mut pairs = 0u64;
    let mut differ = 0u64;
    let (mut differ_corner, mut differ_scaled_ok) = (0u64, 0u64);
    let mut asym = 0u64;
    let mut asym_len = [0u64; 12];
    let mut tot_len = [0u64; 12];
    let from_all: Vec<TileId> = (0..big.tile_count() as u32).step_by(37).map(TileId).collect();
    for &a in &from_all {
        let ca = big.to_coord(a).unwrap();
        for dq in -9..=9 {
            for dr in -9..=9 {
                let cb = HexCoord { q: ca.q + dq, r: ca.r + dr };
                let Some(b) = big.to_tile(cb) else { continue };
                if ca.distance(cb) > 9 || a == b {
                    continue;
                }
                // Tiles strictly between, ours.
                let mut ours = Vec::new();
                classic_rules::math::hex_line_all(&big, a, b, |t| {
                    ours.push(t);
                    true
                });
                let theirs: Vec<TileId> = guide_line(ca, cb).iter().filter_map(|c| big.to_tile(*c)).collect();
                let inner: Vec<TileId> = if theirs.len() >= 2 { theirs[1..theirs.len() - 1].to_vec() } else { vec![] };
                pairs += 1;
                if ours != inner {
                    differ += 1;
                    // Is the lerp point of a differing sample a hex corner (all three fractions 1/2)?
                    let cube = |h: HexCoord| [h.q as f64, h.r as f64, -(h.q as f64) - h.r as f64];
                    let n = ca.distance(cb);
                    let eps = 1.0 / (1024.0 * n as f64);
                    let scaled: Vec<TileId> = guide_line_eps(ca, cb, eps).iter().filter_map(|c| big.to_tile(*c)).collect();
                    let scaled_inner: Vec<TileId> = if scaled.len() >= 2 { scaled[1..scaled.len() - 1].to_vec() } else { vec![] };
                    if scaled_inner == ours {
                        differ_scaled_ok += 1;
                    }
                    let (qa, qb) = (cube(ca), cube(cb));
                    let corner = (1..n).any(|i| {
                        let t = i as f64 / n as f64;
                        (0..3).all(|k| ((qa[k] * (1.0 - t) + qb[k] * t).fract().abs() - 0.5).abs() < 1e-9)
                    });
                    if corner {
                        differ_corner += 1;
                    }
                }
                let mut back = Vec::new();
                classic_rules::math::hex_line_all(&big, b, a, |t| {
                    back.push(t);
                    true
                });
                back.reverse();
                let d = ca.distance(cb) as usize;
                tot_len[d] += 1;
                if back != ours {
                    asym += 1;
                    asym_len[d] += 1;
                }
            }
        }
    }
    println!("integer line vs the guide's float line (nudge 1e-6): {pairs} pairs with distance <= 9, {differ} differ; of those {differ_corner} pass exactly through a hex corner (3-way tie), {differ_scaled_ok} agree with a float line nudged by our 1/(1024 n)");
    println!("line(A,B) != reversed line(B,A): {asym} of {pairs} pairs; by distance (asym/total): {:?}", (2..10).map(|d| format!("{d}:{}/{}", asym_len[d], tot_len[d])).collect::<Vec<_>>());
    // Sight cost on the real map.
    let land: Vec<TileId> = (0..dims.tile_count() as u32).step_by(5).map(TileId).collect();
    let k = land.len() as f64;
    for radius in [2, 3, 5] {
        let (disc, s0) = best_ns(7, || land.iter().map(|t| hex_disc(dims, *t, radius).count() as u64).sum());
        let (cov, s1) = best_ns(7, || land.iter().map(|t| covered_tiles(map, *t, radius).count() as u64).sum());
        println!(
            "sight r={radius}: disc only {:.0} ns, disc + hex-line LOS {:.0} ns per observer ({:.2} of the disc survives, LOS share {:.0} %)",
            disc / k,
            cov / k,
            s1 as f64 / s0 as f64,
            100.0 * (cov - disc) / cov
        );
    }
    // Ray-count alternative cost model: tiles on lines (sum of distances) vs disc size.
    let _ = los_clear(map, TileId(0), TileId(1));
}

fn pairs_at(game: &GameState, dist: i32, count: usize, seed: u64) -> Vec<(TileId, TileId)> {
    let map = game.view().auth.map();
    let dims = map.dimensions;
    let mut rng = Lcg(seed);
    let mut out = Vec::new();
    let passable = |t: TileId| classic_rules::navigation::cost::enter_cost_on_map(map, t).is_some();
    let mut guard = 0;
    while out.len() < count && guard < 2_000_000 {
        guard += 1;
        let a = TileId((rng.next() % dims.tile_count() as u64) as u32);
        let ca = dims.to_coord(a).unwrap();
        let k = (rng.next() % 6) as usize;
        let d = HEX_DIRS[k];
        let cb = HexCoord { q: ca.q + d.q * dist, r: ca.r + d.r * dist };
        if let Some(b) = dims.to_tile(cb) {
            if passable(a) && passable(b) && !map.is_water(a) && !map.is_water(b) {
                out.push((a, b));
            }
        }
    }
    out
}

/// The hover preview of a land unit over a goal it cannot enter (sea,
/// mountains): the search with and without the early exit.
fn blocked_goals(game: &GameState, rules: &classic_rules::CompiledRuleset) {
    use classic_rules::navigation::astar::find_path_without_early_exit;
    println!("## A* to a goal the unit cannot enter (water or impassable), 200x125, 2-move land unit");
    let map = game.view().auth.map();
    let dims = map.dimensions;
    let passable = |t: TileId| classic_rules::navigation::cost::enter_cost_on_map(map, t).is_some();
    let budget = classic_rules::navigation::MoveBudget::flat(120, 120);
    let mut scratch = PathScratch::default();
    let mut rng = Lcg(77);
    let mut pairs = Vec::new();
    while pairs.len() < 60 {
        let a = TileId((rng.next() % dims.tile_count() as u64) as u32);
        let b = TileId((rng.next() % dims.tile_count() as u64) as u32);
        if passable(a) && !map.is_water(a) && !passable(b) && dims.distance(a, b).unwrap() <= 30 {
            pairs.push((a, b));
        }
    }
    let (mut t_old, mut t_new, mut e_old, mut e_new) = (Vec::new(), Vec::new(), 0u64, 0u64);
    for (x, y) in &pairs {
        let req = || PathRequest { from: *x, to: *y, player: PlayerId(0), budget, profile: MoveProfile::LAND };
        let (old_ns, _) = best_ns(3, || {
            let p = find_path_without_early_exit(game.view(), rules, req(), &mut scratch);
            assert!(p.is_none());
            0
        });
        e_old += scratch.expanded_last;
        let (new_ns, _) = best_ns(5, || {
            let p = find_path(game.view(), rules, req(), &mut scratch);
            assert!(p.is_none());
            0
        });
        e_new += scratch.expanded_last;
        t_old.push(old_ns);
        t_new.push(new_ns);
    }
    t_old.sort_by(|a, b| a.partial_cmp(b).unwrap());
    t_new.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |v: &Vec<f64>, f: f64| v[((v.len() - 1) as f64 * f) as usize];
    println!(
        "{} queries. without the exit: median {:.2} ms, min {:.2}, max {:.2}, {:.0} expanded/query | with it: median {:.2} us, max {:.2} us, {:.0} expanded/query",
        pairs.len(), q(&t_old, 0.5) / 1e6, t_old[0] / 1e6, t_old[t_old.len() - 1] / 1e6, e_old as f64 / pairs.len() as f64,
        q(&t_new, 0.5) / 1e3, t_new[t_new.len() - 1] / 1e3, e_new as f64 / pairs.len() as f64,
    );
}

fn astar(game: &GameState, rules: &classic_rules::CompiledRuleset) {
    blocked_goals(game, rules);
    println!("## A* (kernel navigation::astar), 200x125 representative map, no roads");
    let n = game.view().auth.map().tile_count();
    let mut scratch = PathScratch::default();
    // The cost of the two O(map) fills in `search`.
    let mut a = vec![0u32; n];
    let mut b = vec![0u32; n];
    let (fill, _) = best_ns(50, || {
        a.fill(u32::MAX);
        b.fill(u32::MAX);
        black_box(&a);
        black_box(&b);
        0
    });
    println!("two fills of {n} u32: {:.1} us", fill / 1e3);
    for dist in [2, 5, 10, 30, 80] {
        let pairs = pairs_at(game, dist, 60, 7 + dist as u64);
        if pairs.is_empty() {
            continue;
        }
        let budget = classic_rules::navigation::MoveBudget::flat(120, 120);
        // Per pair: the best of 5 runs; found and not-found reported apart.
        let (mut t_found, mut t_lost, mut e_found, mut e_lost, mut nf, mut nl) = (0.0f64, 0.0f64, 0u64, 0u64, 0u32, 0u32);
        for (x, y) in &pairs {
            let mut best = f64::MAX;
            let mut ok = false;
            let mut expanded = 0;
            for _ in 0..5 {
                let t0 = Instant::now();
                let p = find_path(
                    game.view(),
                    rules,
                    PathRequest { from: *x, to: *y, player: PlayerId(0), budget, profile: MoveProfile::LAND },
                    &mut scratch,
                );
                best = best.min(t0.elapsed().as_secs_f64() * 1e9);
                ok = p.is_some();
                expanded = scratch.expanded_last;
                black_box(p);
            }
            if ok {
                t_found += best;
                e_found += expanded;
                nf += 1;
            } else {
                t_lost += best;
                e_lost += expanded;
                nl += 1;
            }
        }
        println!(
            "distance {dist:>2}: found {nf}: {:.1} us/query, {:.0} expanded, {:.0} ns/expansion | unreachable {nl}: {:.1} us/query, {:.0} expanded",
            t_found / nf.max(1) as f64 / 1e3,
            e_found as f64 / nf.max(1) as f64,
            t_found / e_found.max(1) as f64,
            t_lost / nl.max(1) as f64 / 1e3,
            e_lost as f64 / nl.max(1) as f64,
        );
    }
}

/// A* over the AI's known map with the knobs of the audit: heuristic unit
/// per hex, skipping stale queue entries, and the tie-break on the queue.
/// Returns (cost, expanded) or None.
fn probe_astar(
    map: &classic_rules::ai::planner_map::KnownMap<'_, '_>,
    dims: MapDimensions,
    from: TileId,
    to: TileId,
    unit: u32,
    skip_stale: bool,
    prefer_deep: bool,
) -> Option<(u32, u64)> {
    use classic_rules::ai::planner_map::step_points;
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let n = dims.tile_count();
    let mut cost = vec![u32::MAX; n];
    let mut heap = BinaryHeap::new();
    cost[from.0 as usize] = 0;
    heap.push(Reverse((dims.distance(from, to).unwrap() as u32 * unit, 0u32, from.0)));
    let mut expanded = 0u64;
    while let Some(Reverse((f, tie, tile))) = heap.pop() {
        let g = cost[tile as usize];
        let h = dims.distance(TileId(tile), to).unwrap() as u32 * unit;
        if skip_stale && f != g + h {
            continue;
        }
        if tile == to.0 {
            return Some((g, expanded));
        }
        expanded += 1;
        let _ = tie;
        for next in dims.neighbors(TileId(tile)) {
            let Some(points) = step_points(map, TileId(tile), next) else { continue };
            let c = g + points;
            if c >= cost[next.0 as usize] {
                continue;
            }
            cost[next.0 as usize] = c;
            let h = dims.distance(next, to).unwrap() as u32 * unit;
            // Tie-break: among equal f prefer the larger g (closer to the goal).
            let tb = if prefer_deep { u32::MAX - c } else { next.0 };
            heap.push(Reverse((c + h, tb, next.0)));
        }
    }
    None
}

fn ai(game: &mut GameState, rules: &classic_rules::CompiledRuleset) {
    use classic_rules::ai::planner_map::{explore, find_path, Blockers, KnownMap, MapScratch, PathOutcome, SearchLimit, SearchStats, Visit};
    println!("## AI map search (kernel::ai::map) vs Dijkstra, roads on every 6th row");
    let dims = game.view().auth.map().dimensions;
    let roads: Vec<TileId> = (0..dims.tile_count() as u32)
        .map(TileId)
        .filter(|t| {
            let c = dims.to_coord(*t).unwrap();
            c.r % 6 == 0 && !game.view().auth.map().is_water(*t)
        })
        .collect();
    game.debug_set_road_batch(rules, &roads);
    for dist in [12, 30] {
        let pairs = pairs_at(game, dist, 120, 99 + dist as u64);
        let auth = game.view().auth;
        let view = classic_rules::knowledge::view::PlayerView::new(auth, game.derived(), rules, PlayerId(0)).revealed();
        let map = KnownMap::land(&view);
        let mut scratch = MapScratch::default();
        let mut stats = SearchStats::default();
        let limit = SearchLimit { max_nodes: 1_000_000 };
        let (mut compared, mut worse, mut extra_points) = (0u32, 0u32, 0u64);
        let mut worst = 0.0f64;
        let (mut nodes_a, mut nodes_d, mut nodes_fix, mut nodes_stale, mut nodes_deep) = (0u64, 0u64, 0u64, 0u64, 0u64);
        let (mut fix_worse, mut stale_worse) = (0u32, 0u32);
        for (from, to) in &pairs {
            let before = stats.nodes;
            let outcome = find_path(&map, *from, *to, &Blockers::default(), &mut scratch, limit, &mut stats);
            let spent = stats.nodes - before;
            let PathOutcome::Found { points, .. } = outcome else { continue };
            nodes_a += spent;
            let before = stats.nodes;
            let mut best = u32::MAX;
            explore(&map, *from, &Blockers::default(), &mut scratch, limit, &mut stats, &mut |t, cost| {
                if t == *to {
                    best = cost;
                    Visit::Stop
                } else {
                    Visit::Continue
                }
            });
            nodes_d += stats.nodes - before;
            compared += 1;
            if points > best {
                worse += 1;
                extra_points += (points - best) as u64;
                worst = worst.max(points as f64 / best as f64);
            }
            // The kernel's own unit (60 per hex), now with the stale skip: still inadmissible with roads.
            if let Some((c, e)) = probe_astar(&map, dims, *from, *to, 60, true, false) {
                nodes_stale += e;
                stale_worse += (c > best) as u32;
            }
            // Admissible unit (the cheapest step): the road's points.
            if let Some((c, e)) = probe_astar(&map, dims, *from, *to, 30, true, false) {
                nodes_fix += e;
                fix_worse += (c > best) as u32;
            }
            if let Some((c, e)) = probe_astar(&map, dims, *from, *to, 30, true, true) {
                nodes_deep += e;
                assert!(c <= best);
            }
        }
        let q = compared.max(1) as f64;
        println!(
            "hex distance {dist}: {compared} reachable pairs. Kernel AI A* (unit 60): costlier than optimal in {worse} pairs ({:.1} %, +{:.0} points over those, worst x{:.2}), {:.0} expanded/query; Dijkstra-to-target {:.0}/query",
            100.0 * worse as f64 / q,
            extra_points as f64 / worse.max(1) as f64,
            worst,
            nodes_a as f64 / q,
            nodes_d as f64 / q,
        );
        println!(
            "   probe A*, unit 60 + stale skip: {:.0}/query, costlier {stale_worse}; unit 30 (admissible) + stale skip: {:.0}/query, costlier {fix_worse}; unit 30 + prefer larger g on ties: {:.0}/query",
            nodes_stale as f64 / q,
            nodes_fix as f64 / q,
            nodes_deep as f64 / q,
        );
    }
}

fn explore_probe(game: &GameState, rules: &classic_rules::CompiledRuleset) {
    use classic_rules::ai::planner_map::{explore, Blockers, KnownMap, MapScratch, SearchLimit, SearchStats, Visit};
    println!("## AI explore (Dijkstra over the known map), budget 3000 nodes like best_sites");
    let auth = game.view().auth;
    let view = classic_rules::knowledge::view::PlayerView::new(auth, game.derived(), rules, PlayerId(0)).revealed();
    let map = KnownMap::land(&view);
    let origins: Vec<TileId> = pairs_at(game, 2, 40, 5).into_iter().map(|p| p.0).collect();
    let mut scratch = MapScratch::default();
    let mut total = 0.0;
    let mut checksum = 0u64;
    let mut visited_total = 0u64;
    for from in &origins {
        let mut best = f64::MAX;
        for _ in 0..7 {
            let mut stats = SearchStats::default();
            let mut sum = 0u64;
            let mut count = 0u64;
            let t0 = Instant::now();
            explore(&map, *from, &Blockers::default(), &mut scratch, SearchLimit { max_nodes: 3000 }, &mut stats, &mut |t, c| {
                sum = sum.wrapping_mul(1_000_003).wrapping_add(t.0 as u64 * 7919 + c as u64);
                count += 1;
                Visit::Continue
            });
            best = best.min(t0.elapsed().as_secs_f64() * 1e9);
            checksum = checksum.wrapping_mul(31).wrapping_add(sum);
            visited_total = count;
        }
        total += best;
    }
    println!("{} origins: {:.1} us per explore (best of 7 each), last visited {}, checksum {:016x}", origins.len(), total / origins.len() as f64 / 1e3, visited_total, checksum);
}

fn main() {
    let which = std::env::args().nth(1).unwrap_or_else(|| "all".into());
    let (mut g, rules) = game();
    if which == "all" || which == "grid" {
        grid(&g);
    }
    if which == "all" || which == "sight" {
        sight(&g);
    }
    if which == "all" || which == "astar" {
        astar(&g, &rules);
    }
    if which == "all" || which == "explore" {
        explore_probe(&g, &rules);
    }
    if which == "all" || which == "ai" {
        ai(&mut g, &rules);
    }
}

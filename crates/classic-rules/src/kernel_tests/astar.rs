use crate::ids::*;
use crate::navigation::PathScratch;
use crate::navigation::MoveProfile;
use crate::CompiledRuleset;
use crate::navigation::astar::*;
use crate::ids::TerrainId;
use crate::fixtures::WATER;
use crate::rules::SourceRules;
use crate::world::state::GameConfig;
use crate::{compile_rules, GameState};

/// Ten movement points: long paths fit in one turn.
const BUDGET: MoveBudget = MoveBudget::flat(600, 600);

fn open_game(w: u32, h: u32) -> (GameState, CompiledRuleset) {
    let rules = compile_rules(SourceRules::default()).unwrap();
    let game = GameState::new(
        &rules,
        GameConfig {
            players: 1,
            width: w,
            height: h,
            rng_seed: 0,
        },
    );
    (game, rules)
}

fn paint_water(game: &mut GameState, rules: &CompiledRuleset, tiles: &[(u32, u32)], w: u32) {
    let batch: Vec<(TileId, TerrainId)> = tiles
        .iter()
        .map(|(x, y)| (TileId(y * w + x), WATER))
        .collect();
    game.debug_set_terrain_batch(rules, &batch);
}

fn request(from: TileId, to: TileId, player: PlayerId, budget: MoveBudget) -> PathRequest {
    PathRequest {
        from,
        to,
        player,
        budget,
        profile: MoveProfile::LAND,
    }
}

fn ask(game: &GameState, rules: &CompiledRuleset, from: (u32, u32), to: (u32, u32), w: u32) -> Option<Path> {
    let req = request(TileId(from.1 * w + from.0), TileId(to.1 * w + to.0), PlayerId(0), BUDGET);
    find_path(game.view(), rules, req, &mut PathScratch::default())
}

#[test]
fn open_field_optimal() {
    let (game, rules) = open_game(8, 8);
    let p = ask(&game, &rules, (0, 0), (3, 0), 8).unwrap();
    assert_eq!(p.tiles.len(), 4);
    assert_eq!(p.cost, PathCost { turns: 0, moves_left: 600 - 3 * 60 });
    let dims = game.view().auth.map().dimensions;
    for w2 in p.tiles.windows(2) {
        assert!(dims.adjacent(w2[0], w2[1]));
    }
}

#[test]
fn turns_follow_the_partial_move_rule() {
    // Two points per turn, five flat steps: 2 + 2 + 1 steps → turns 2.
    let (game, rules) = open_game(8, 8);
    let two = MoveBudget::flat(120, 120);
    let req = request(TileId(0), TileId(5), PlayerId(0), two);
    let cost = find_path_cost(game.view(), &rules, &req, &mut PathScratch::default()).unwrap();
    assert_eq!(cost, PathCost { turns: 2, moves_left: 60 });
    // A unit with no points left starts on the next turn.
    let spent = MoveBudget::flat(120, 0);
    let req = request(TileId(0), TileId(1), PlayerId(0), spent);
    let cost = find_path_cost(game.view(), &rules, &req, &mut PathScratch::default()).unwrap();
    assert_eq!(cost.turns, 1);
}

#[test]
fn wall_forces_detour_without_water_on_path() {
    let (mut game, rules) = open_game(8, 8);
    // A wall x=2, y=0..6, the gap only at y=7.
    let wall: Vec<(u32, u32)> = (0..7).map(|y| (2, y)).collect();
    paint_water(&mut game, &rules, &wall, 8);
    let p = ask(&game, &rules, (0, 0), (4, 0), 8).expect("gap at y=7 must allow passage");
    assert!(p.tiles.len() > 5, "detour must exceed 4 steps: {:?}", p.tiles);
    for t in &p.tiles {
        assert_ne!(game.view().auth.map().terrain(*t), Some(WATER));
    }
}

#[test]
fn blocked_returns_none() {
    let (mut game, rules) = open_game(8, 8);
    let wall: Vec<(u32, u32)> = (0..8).map(|y| (2, y)).collect();
    paint_water(&mut game, &rules, &wall, 8);
    assert!(ask(&game, &rules, (0, 0), (4, 0), 8).is_none());
    // A target on water is None too.
    assert!(ask(&game, &rules, (0, 0), (2, 0), 8).is_none());
}

/// The (turn, points left) label of the Dijkstra oracle: fewer turns
/// first, then more points left.
fn label_key(label: (u32, u32)) -> (u32, std::cmp::Reverse<u32>) {
    (label.0, std::cmp::Reverse(label.1))
}

/// Obviously correct Dijkstra oracle (O(V²), no heuristic, no heap, no
/// scalar): explicit (turn, points left) labels, neighbours straight from
/// the axial offsets (not `MapDimensions::neighbors`), the same step price
/// `step_cost_for` (no units, no zones of control). Rules of a step: with
/// nothing left the unit waits for the next turn and gets the allowance of
/// the tile it stands on (embarked on water); it may enter any tile while
/// it has points; it keeps at most the allowance of the entered tile.
fn dijkstra(game: &GameState, rules: &CompiledRuleset, req: &PathRequest) -> Option<PathCost> {
    use crate::navigation::cost::StepCost;
    let auth = game.view().auth;
    let map = auth.map();
    let n = map.tile_count();
    let (w, h) = (map.dimensions.width, map.dimensions.height);
    let full = |t: usize| {
        let water = map.terrain_kinds().is_water(map.terrain(TileId(t as u32)).unwrap().0);
        if req.profile.embarks() && water {
            u32::from(req.budget.embarked)
        } else {
            u32::from(req.budget.max)
        }
    };
    let mut best: Vec<Option<(u32, u32)>> = vec![None; n];
    let mut done = vec![false; n];
    let from = req.from.0 as usize;
    best[from] = Some((0, u32::from(req.budget.left).min(full(from))));
    loop {
        let u = (0..n)
            .filter(|i| !done[*i] && best[*i].is_some())
            .min_by_key(|i| label_key(best[*i].unwrap()))?;
        let (turn, left) = best[u].unwrap();
        if u == req.to.0 as usize {
            return Some(PathCost { turns: turn, moves_left: left as u16 });
        }
        done[u] = true;
        let (turn, left) = if left == 0 { (turn + 1, full(u)) } else { (turn, left) };
        let (uq, ur) = ((u as u32 % w) as i32, (u as u32 / w) as i32);
        for d in crate::math::HEX_DIRS {
            let (q, r) = (uq + d.q, ur + d.r);
            if q < 0 || r < 0 || q >= w as i32 || r >= h as i32 {
                continue;
            }
            let v = (r as u32 * w + q as u32) as usize;
            let step = crate::navigation::cost::step_cost_for(auth, rules, req.player, &req.profile, TileId(u as u32), TileId(v as u32));
            let Some(step) = step else {
                continue;
            };
            let spent = match step {
                StepCost::Points(p) => p.min(left),
                StepCost::AllMoves => left,
            };
            let label = (turn, (left - spent).min(full(v)));
            if best[v].is_none_or(|old| label_key(label) < label_key(old)) {
                best[v] = Some(label);
            }
        }
    }
}

/// Xorshift for the random maps of the differential tests.
fn xorshift(seed: u64) -> impl FnMut() -> u64 {
    let mut seed = seed;
    move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed = seed.max(1);
        seed
    }
}

fn random_terrain(game: &mut GameState, rules: &CompiledRuleset, rnd: &mut impl FnMut() -> u64, water_in_10: u64) {
    let batch: Vec<(TileId, TerrainId)> = (0..144)
        .map(|i| {
            let v = if rnd() % 10 < water_in_10 { WATER } else { TerrainId((rnd() % 3) as u16) };
            (TileId(i), v)
        })
        .collect();
    game.debug_set_terrain_batch(rules, &batch);
}

#[test]
fn matches_dijkstra_on_random_maps() {
    // A differential test in the spirit of §12: the optimised A* vs a simple oracle.
    let mut rnd = xorshift(0x1234_5678_9abc_def0);
    for round in 0..20 {
        let (mut game, rules) = open_game(12, 12);
        random_terrain(&mut game, &rules, &mut rnd, 2);
        // Half of the maps get roads: the estimate must stay admissible
        // with half-point road steps.
        if round % 2 == 1 {
            let roads: Vec<TileId> = (0..144).filter(|_| rnd() % 3 == 0).map(TileId).collect();
            game.debug_set_road_batch(&rules, &roads);
        }
        for _ in 0..5 {
            let (a, b) = ((rnd() % 144) as u32, (rnd() % 144) as u32);
            let req = request(TileId(a), TileId(b), PlayerId(0), BUDGET);
            let got = find_path(game.view(), &rules, req.clone(), &mut PathScratch::default()).map(|p| p.cost);
            assert_eq!(got, dijkstra(&game, &rules, &req), "round {}: {:?} -> {:?}", round, a, b);
        }
    }
}

/// Land and sea with embarking (§7.4–7.5, §7.1): random coasts and deep
/// water, shallow-only or all-water embarkation, one-point landings,
/// embarked allowances smaller and larger than the land one, partial moves
/// left at the start. A* on the scalar equals the explicit-label oracle,
/// and every step of its path is possible.
#[test]
fn embarking_paths_match_dijkstra_on_random_archipelagos() {
    use crate::navigation::Embark;
    let mut rnd = xorshift(0x0bad_5eed_cafe_f00d);
    let mut found = 0;
    for round in 0..40 {
        let (mut game, rules) = open_game(12, 12);
        random_terrain(&mut game, &rules, &mut rnd, 5);
        if round % 3 == 1 {
            let roads: Vec<TileId> = (0..144).filter(|_| rnd() % 4 == 0).map(TileId).collect();
            game.debug_set_road_batch(&rules, &roads);
        }
        let mut profile = MoveProfile::LAND;
        profile.embark = if round % 2 == 0 { Embark::Shallow } else { Embark::All };
        profile.disembark_flat = round % 5 == 3;
        let max = 60 * (1 + (rnd() % 4) as u16);
        let embarked = 60 * (1 + (rnd() % 5) as u16);
        for _ in 0..8 {
            let (a, b) = ((rnd() % 144) as u32, (rnd() % 144) as u32);
            let left = (rnd() % (u64::from(max) + 1)) as u16;
            let req = PathRequest {
                from: TileId(a),
                to: TileId(b),
                player: PlayerId(0),
                budget: MoveBudget { max, embarked, left },
                profile,
            };
            let path = find_path(game.view(), &rules, req.clone(), &mut PathScratch::default());
            let want = dijkstra(&game, &rules, &req);
            assert_eq!(path.as_ref().map(|p| p.cost), want, "round {round}: {a} -> {b}, {max}/{embarked}/{left}");
            let Some(path) = path else {
                continue;
            };
            found += 1;
            let auth = game.view().auth;
            for pair in path.tiles.windows(2) {
                let step = crate::navigation::cost::step_cost_for(auth, &rules, PlayerId(0), &profile, pair[0], pair[1]);
                assert!(step.is_some(), "round {round}: the path takes an impossible step");
            }
        }
    }
    assert!(found > 100, "the maps leave enough reachable pairs: {found}");
}

/// Embarking ends the move; the next turn the unit has its embarked
/// allowance on water; landing ends the move (§7.4, §7.1).
#[test]
fn a_strait_is_crossed_with_the_embarked_allowance() {
    use crate::navigation::Embark;
    let (mut game, rules) = open_game(10, 3);
    // Land x=0..=2, water x=3..=6, land x=7..=9.
    let water: Vec<(u32, u32)> = (0..3).flat_map(|y| (3..7).map(move |x| (x, y))).collect();
    paint_water(&mut game, &rules, &water, 10);
    let mut profile = MoveProfile::LAND;
    profile.embark = Embark::All;
    let cost = |profile: MoveProfile, budget: MoveBudget, to: u32| {
        let req = PathRequest {
            from: TileId(10 + 2),
            to: TileId(10 + to),
            player: PlayerId(0),
            budget,
            profile,
        };
        find_path_cost(game.view(), &rules, &req, &mut PathScratch::default())
    };
    // Embarking takes the turn; three water steps a turn at sea; the
    // landing takes the next turn.
    let three_at_sea = MoveBudget { max: 120, embarked: 180, left: 120 };
    assert_eq!(cost(profile, three_at_sea, 3), Some(PathCost { turns: 0, moves_left: 0 }));
    assert_eq!(cost(profile, three_at_sea, 6), Some(PathCost { turns: 1, moves_left: 0 }));
    assert_eq!(cost(profile, three_at_sea, 7), Some(PathCost { turns: 2, moves_left: 0 }));
    // One point at sea: three turns on the water.
    let one_at_sea = MoveBudget { max: 120, embarked: 60, left: 120 };
    assert_eq!(cost(profile, one_at_sea, 6), Some(PathCost { turns: 3, moves_left: 0 }));
    // A one-point landing: what is left is capped by the land allowance
    // (5 at sea: x=4,5,6 and the landing leave 1).
    profile.disembark_flat = true;
    let five_at_sea = MoveBudget { max: 120, embarked: 300, left: 120 };
    assert_eq!(cost(profile, five_at_sea, 8), Some(PathCost { turns: 1, moves_left: 0 }));
    assert_eq!(cost(profile, five_at_sea, 7), Some(PathCost { turns: 1, moves_left: 60 }));
    // Shallow water only: the middle of the strait (x=4,5 have no land
    // neighbour) is deep.
    profile.embark = Embark::Shallow;
    assert_eq!(cost(profile, five_at_sea, 3), Some(PathCost { turns: 0, moves_left: 0 }));
    assert!(cost(profile, five_at_sea, 4).is_none());
    // Without embarkation the strait is closed.
    profile.embark = Embark::No;
    assert!(cost(profile, five_at_sea, 7).is_none());
}

#[test]
fn deterministic_same_request_same_path() {
    let (mut game, rules) = open_game(12, 12);
    let wall: Vec<(u32, u32)> = (0..10).map(|y| (5, y)).collect();
    paint_water(&mut game, &rules, &wall, 12);
    let req = || request(TileId(0), TileId(143), PlayerId(0), BUDGET);
    let p1 = find_path(game.view(), &rules, req(), &mut PathScratch::default());
    let p2 = find_path(game.view(), &rules, req(), &mut PathScratch::default());
    assert_eq!(p1, p2);
}

#[test]
fn zones_of_control_come_from_visible_enemies_at_war() {
    use crate::ids::UnitKindId;
    // P0 goes (4,4)->(8,4) past P1's warrior at (6,4).
    let rules = compile_rules(SourceRules::default()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 12,
            height: 12,
            rng_seed: 0,
        },
    );
    let t = |x: u32| TileId(4 * 12 + x);
    game.debug_spawn_unit(&rules, PlayerId(0), t(4), UnitKindId(0));
    game.debug_spawn_unit(&rules, PlayerId(1), t(6), UnitKindId(0));
    let two = MoveBudget::flat(120, 120);
    let cost = |game: &GameState| {
        let req = request(t(4), t(8), PlayerId(0), two);
        find_path_cost(game.view(), &rules, &req, &mut PathScratch::default()).unwrap()
    };
    let peace = cost(&game);
    game.execute(
        &rules,
        crate::Command::DeclareWar {
            player: PlayerId(0),
            enemy: PlayerId(1),
        },
    )
    .unwrap();
    let war = cost(&game);
    assert!(war.turns > peace.turns, "ZOC must slow the march: {peace:?} -> {war:?}");
}

/// The early exit "no neighbour can step onto the goal" must change nothing
/// but the time: on generated worlds (cities and borders of four players,
/// units, rivers, roads, water, mountains) the result of `find_path` for
/// every kind of goal and every movement profile equals the search without
/// the exit, tile for tile and cost for cost.
#[test]
fn early_exit_for_unenterable_goals_changes_no_answer() {
    use crate::navigation::Embark;
    use crate::scenario::{generate, ScenarioConfig};
    use crate::rules::content::Domain;
    let rules = compile_rules(SourceRules::default()).unwrap();
    let (mut compared, mut exits, mut found_paths, mut reachable_nodes_equal) = (0u32, 0u32, 0u32, 0u32);
    let mut kinds = [0u32; 4]; // goal: water, impassable land, city, unit
    for seed in 1..=4u64 {
        let mut game = generate(
            &ScenarioConfig { seed, players: 4, width: 36, height: 28, cities: 10, units: 50, water_ratio: 30 },
            &rules,
        );
        let mut rnd = xorshift(0xfeed_0000 + seed);
        let n = 36 * 28;
        // Roads on a third of the tiles, rivers along random edges.
        let roads: Vec<TileId> = (0..n).filter(|_| rnd() % 3 == 0).map(TileId).collect();
        game.debug_set_road_batch(&rules, &roads);
        let dims = game.view().auth.map().dimensions;
        let mut edges = Vec::new();
        for _ in 0..60 {
            let a = TileId((rnd() % n as u64) as u32);
            let around = dims.neighbors(a);
            let b = around.as_slice()[(rnd() % around.as_slice().len() as u64) as usize];
            edges.push((a.min(b), a.max(b)));
        }
        game.debug_set_river_edges(&rules, &edges);

        let auth = game.view().auth;
        let city_tiles: Vec<TileId> = auth.cities().iter().map(|(_, c)| c.tile).collect();
        let unit_tiles: Vec<TileId> = auth.units().iter_cores().map(|u| u.tile).collect();
        let map = auth.map();
        let impassable = |t: TileId| crate::navigation::cost::enter_cost_on_map(map, t).is_none();
        let mut goals: Vec<(TileId, usize)> = Vec::new();
        for t in 0..n {
            let t = TileId(t);
            if map.is_water(t) {
                goals.push((t, 0));
            } else if impassable(t) {
                goals.push((t, 1));
            }
        }
        goals.extend(city_tiles.iter().map(|t| (*t, 2)));
        goals.extend(unit_tiles.iter().map(|t| (*t, 3)));
        for _ in 0..80 {
            goals.push((TileId((rnd() % n as u64) as u32), 4));
        }

        let mut profiles = Vec::new();
        profiles.push(MoveProfile::LAND);
        let mut shallow = MoveProfile::LAND;
        shallow.embark = Embark::Shallow;
        profiles.push(shallow);
        let mut all = MoveProfile::LAND;
        all.embark = Embark::All;
        all.disembark_flat = true;
        profiles.push(all);
        let mut sea = MoveProfile::LAND;
        sea.domain = Domain::Sea;
        profiles.push(sea);
        let mut hover = MoveProfile::LAND;
        hover.hover = true;
        profiles.push(hover);
        let mut mountains = MoveProfile::LAND;
        mountains.cross_mountains = true;
        profiles.push(mountains);
        let mut rival = MoveProfile::LAND;
        rival.rival_territory = true;
        rival.ignore_zone_of_control = true;
        profiles.push(rival);
        let mut anything = MoveProfile::LAND;
        anything.cross_impassable = true;
        profiles.push(anything);

        let mut scratch_new = PathScratch::default();
        let mut scratch_old = PathScratch::default();
        for (i, profile) in profiles.iter().enumerate() {
            for (goal, kind) in goals.iter().step_by(1 + (i % 3)) {
                let from = TileId((rnd() % n as u64) as u32);
                let player = PlayerId((rnd() % 4) as u8);
                let budget = match i % 3 {
                    0 => BUDGET,
                    1 => MoveBudget { max: 120, embarked: 240, left: 60 },
                    _ => MoveBudget { max: 60, embarked: 60, left: 60 },
                };
                let req = PathRequest { from, to: *goal, player, budget, profile: *profile };
                let new = find_path(game.view(), &rules, req.clone(), &mut scratch_new);
                let new_nodes = scratch_new.expanded_last;
                let old = find_path_without_early_exit(game.view(), &rules, req.clone(), &mut scratch_old);
                assert_eq!(new, old, "seed {seed} profile {i} {from:?} -> {goal:?}");
                let cost = find_path_cost(game.view(), &rules, &req, &mut scratch_new);
                assert_eq!(cost, old.as_ref().map(|p| p.cost));
                compared += 1;
                if *kind < 4 {
                    kinds[*kind] += 1;
                }
                if old.is_some() && from == *goal {
                    // Answered before any search; the metric keeps its old value.
                } else if old.is_some() {
                    found_paths += 1;
                    // A reachable goal is searched exactly as before.
                    reachable_nodes_equal += (new_nodes == scratch_old.expanded_last) as u32;
                } else if new_nodes == 0 && from != *goal {
                    exits += 1;
                }
            }
        }
    }
    // The test has teeth: many exits, many found paths, every goal kind.
    assert!(exits > 100, "early exits taken: {exits}");
    assert!(found_paths > 100, "paths found: {found_paths}");
    assert_eq!(reachable_nodes_equal, found_paths, "a found path was searched differently");
    assert!(kinds.iter().all(|k| *k > 20), "goal kinds: {kinds:?}");
    assert!(compared > 1000, "{compared}");
}

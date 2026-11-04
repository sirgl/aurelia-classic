//! Reusable scenario harness (§44 platform/project.md): NOT Criterion, its own format.
//!
//! Matrix §53 + torture: representative / stress / maze / warfront /
//! mass-visibility / save / AI. Each case yields work counters
//! (deterministic, so gated exactly) + wall_ms (informational, §49).
//!
//! Result line format (parsed without dependencies):
//!   m <name> c_tiles=.. c_entities=.. c_bytes=.. c_found=.. wall_ms=..
//! Modes: `run [--out file]` is a table + file; `check <baseline>` is a
//! comparison (counters exact, wall with a 3x tolerance, WARN only).
//! Exit code in check: 1 on a counter mismatch or lost coverage.

use classic_rules::ClassicGame;
use std::hint::black_box;
use std::time::Instant;

#[path = "perf_baseline/economy.rs"]
mod perf_economy;

use classic_rules::{
    compile_rules,
    derived::visibility::VisibilityState,
    derived::yields::YieldState,
    ids::{PlayerId, TileId, UnitKindId},
    metrics::WorkCounters,
    navigation::{PathRequest, PathScratch},
    rules::SourceRules,
    scenario::{generate, ScenarioConfig},
    GameConfig, GameState,
};

/// A two-move unit (the warrior): paths are measured in turns.
const NAV_BUDGET: classic_rules::navigation::MoveBudget = classic_rules::navigation::MoveBudget::flat(120, 120);

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

struct Case {
    name: &'static str,
    counters: WorkCounters,
    extra: Vec<(&'static str, u64)>,
    wall_ms: f64,
    wall_only: bool,
}

impl Case {
    fn line(&self) -> String {
        let mut s = format!(
            "m {} c_tiles={} c_entities={} c_bytes={}",
            self.name,
            self.counters.tiles_visited,
            self.counters.entities_visited,
            self.counters.bytes_allocated
        );
        for (k, v) in &self.extra {
            s.push_str(&format!(" {}={}", k, v));
        }
        s.push_str(&format!(" wall_ms={:.3}", self.wall_ms));
        if self.wall_only {
            s.push_str(" wall_only=1");
        }
        s
    }
}

/// End turns (sequential turns) until `player` is active again: the active
/// player's turn ends first, then everybody else's.
fn next_round(game: &mut GameState, rules: &classic_rules::CompiledRuleset, player: PlayerId) {
    let first = game.view().auth.active_player();
    let _ = game.execute(rules, classic_rules::Command::EndTurn { player: first });
    for _ in 0..game.view().auth.players().count() {
        let active = game.view().auth.active_player();
        if active == player {
            return;
        }
        let _ = game.execute(rules, classic_rules::Command::EndTurn { player: active });
    }
}

fn passable(game: &GameState, t: TileId) -> bool {
    classic_rules::navigation::cost::enter_cost_on_map(game.view().auth.map(), t).is_some()
}

fn land_pair(game: &GameState) -> (TileId, TileId) {
    use classic_rules::navigation::{find_path_cost, PathRequest, PathScratch};
    let auth = game.view().auth;
    let w = auth.map().dimensions.width.max(1) as i64;
    let n = auth.map().tile_count();
    // Cities keep their distance: territory radius <=3, margin 5, so borders
    // will not creep together in 200 turns (swallow -> "closed borders" in the middle of the measurement).
    let city_qr: Vec<(i64, i64)> = auth
        .cities()
        .iter()
        .map(|(_, c)| ((c.tile.0 as i64) % w, (c.tile.0 as i64) / w))
        .collect();
    let mut scratch = PathScratch::default();
    let probe_rules = compile_rules(SourceRules::default()).unwrap();
    for i in 0..n.saturating_sub(1) {
        let (a, b) = (TileId(i as u32), TileId(i as u32 + 1));
        if passable(game, a) && passable(game, b) {
            // A ford +2 kills the margin of 2 MP: dry tiles only.
            if auth.map().river(a) || auth.map().river(b) {
                continue;
            }
            // Foreign borders in peacetime are "closed borders".
            if auth.map().owner(a).unwrap_or(None).is_some()
                || auth.map().owner(b).unwrap_or(None).is_some()
            {
                continue;
            }
            let (aq, ar) = ((i as i64) % w, (i as i64) / w);
            if city_qr
                .iter()
                .any(|(cq, cr)| (cq - aq).abs() <= 5 && (cr - ar).abs() <= 5)
            {
                continue;
            }
            // The honest path cost both ways (ZOC/snow/hills are visible only here,
            // enter_cost_on_map does not show them). The margin is exactly 2 MP.
            let cheap = [(a, b), (b, a)].iter().all(|(f, t)| {
                find_path_cost(
                    game.view(),
                    &probe_rules,
                    &PathRequest {
                        from: *f,
                        to: *t,
                        player: PlayerId(0),
                        budget: NAV_BUDGET,
                        profile: classic_rules::navigation::MoveProfile::LAND,
                    },
                    &mut scratch,
                )
                .is_some_and(|c| c.this_turn())
            });
            if cheap {
                return (a, b);
            }
        }
    }
    panic!("no clean adjacent land pair");
}

fn land_list(game: &GameState) -> Vec<u32> {
    let n = game.view().auth.map().tile_count();
    (0..n as u32)
        .filter(|i| passable(game, TileId(*i)))
        .collect()
}

fn run_suite() -> Vec<Case> {
    let rules = compile_rules(SourceRules::default()).unwrap();
    let mut out = Vec::new();
    let mut acc = 0usize;

    // ---- Representative (§2.1): 25k, 200 cities, 1000 units ----
    let rep = ScenarioConfig::representative_late_game();
    let t0 = Instant::now();
    let mut game = generate(&rep, &rules);
    game.validate(&rules).expect("rep validates");
    out.push(Case {
        name: "gen.rep",
        counters: WorkCounters::default(),
        extra: vec![
            ("tiles", (rep.width * rep.height) as u64),
            ("cities", 200),
            ("units", 1000),
        ],
        wall_ms: ms(t0.elapsed()),
        wall_only: true,
    });

    // Visibility rebuild (an honest black_box).
    for (tag, g) in [("rep", &game)] {
        let t0 = Instant::now();
        let iters = 10;
        let mut c = WorkCounters::default();
        for _ in 0..iters {
            let (v, cc) = VisibilityState::rebuild_counted(black_box(g.view().auth), &rules);
            acc += black_box(v.counts.len());
            c = cc;
        }
        out.push(Case {
            name: if tag == "rep" {
                "visibility.rebuild.rep"
            } else {
                "visibility.rebuild.x"
            },
            counters: c,
            extra: vec![],
            wall_ms: ms(t0.elapsed()) / iters as f64,
            wall_only: false,
        });
    }

    // Move end-to-end: one MoveUnit per round (sequential turns: the other
    // players end their turns between moves; only the moves are timed).
    let (t0tile, t1tile) = land_pair(&game);
    let h = game.debug_spawn_unit(&rules, PlayerId(0), t0tile, UnitKindId(0));
    let mut move_time = std::time::Duration::ZERO;
    for _ in 0..200 {
        next_round(&mut game, &rules, PlayerId(0));
        let cur = game.view().auth.units().get(h).unwrap().core.tile;
        let to = if cur == t0tile { t1tile } else { t0tile };
        let t0 = Instant::now();
        black_box(
            game.execute(
                &rules,
                classic_rules::Command::MoveUnit {
                    unit: h,
                    destination: to,
                },
            )
            .unwrap(),
        );
        move_time += t0.elapsed();
    }
    out.push(Case {
        name: "exec.move",
        counters: WorkCounters::default(),
        extra: vec![],
        wall_ms: ms(move_time) / 200.0,
        wall_only: true,
    });
    let mut vis = VisibilityState::rebuild(game.view().auth, &rules);
    let mut journal = classic_rules::transaction::journal::ChangeJournal::default();
    journal
        .unit_changes
        .push(classic_rules::transaction::journal::UnitChange::Moved {
            unit: h,
            from: t0tile,
            to: t1tile,
        });
    let t0 = Instant::now();
    let mut tot = WorkCounters::default();
    for _ in 0..1000 {
        tot.add(VisibilityState::apply_changes_counted(
            black_box(&mut vis),
            &journal,
            game.view().auth,
            &rules,
        ));
    }
    out.push(Case {
        name: "visibility.move1",
        counters: WorkCounters {
            tiles_visited: tot.tiles_visited / 1000,
            entities_visited: tot.entities_visited / 1000,
            bytes_allocated: 0,
        },
        extra: vec![],
        wall_ms: ms(t0.elapsed()) / 1000.0,
        wall_only: false,
    });

    // Yields rebuild + move-noop invariant.
    let t0 = Instant::now();
    let mut yc = WorkCounters::default();
    for _ in 0..10 {
        let (y, cc) = YieldState::rebuild_counted(black_box(game.view().auth), &rules);
        acc += black_box(y.city.len());
        yc = cc;
    }
    out.push(Case {
        name: "yields.rebuild.rep",
        counters: yc,
        extra: vec![],
        wall_ms: ms(t0.elapsed()) / 10.0,
        wall_only: false,
    });
    {
        let before = game.derived().yields.clone();
        let mut ys = before.clone();
        let c = YieldState::apply_changes_counted(&mut ys, &journal, game.view().auth, &rules);
        assert_eq!(ys, before, "move must not touch yields");
        out.push(Case {
            name: "yields.move_noop",
            counters: c,
            extra: vec![],
            wall_ms: 0.0,
            wall_only: false,
        });
    }

    // Hash (full) + AI observation + save roundtrip bytes.
    let t0 = Instant::now();
    for _ in 0..200 {
        black_box(classic_rules::replay::authoritative_hash(&game));
    }
    // Counters are the honest hash complexity: tiles x layers + entities.
    let hash_tiles = game.view().auth.map().tile_count() as u64;
    let hash_entities = (game.view().auth.units().len() + game.view().auth.cities().len()) as u64;
    out.push(Case {
        name: "hash.rep",
        counters: WorkCounters {
            tiles_visited: hash_tiles,
            entities_visited: hash_entities,
            bytes_allocated: 0,
        },
        extra: vec![],
        wall_ms: ms(t0.elapsed()) / 200.0,
        wall_only: true,
    });
    let t0 = Instant::now();
    let mut oc = WorkCounters::default();
    for _ in 0..50 {
        let (obs, cc) = black_box(game.ai_observation_counted(&rules, PlayerId(0)));
        acc += black_box(obs.own_units.len());
        oc = cc;
    }
    out.push(Case {
        name: "ai.observe",
        counters: oc,
        extra: vec![],
        wall_ms: ms(t0.elapsed()) / 50.0,
        wall_only: false,
    });
    {
        let save = classic_rules::persistence::encode::encode(&game, &rules);
        let bytes = (save.world.terrain.len() * 2
            + save.world.units.len() * 16
            + save
                .world
                .cities
                .iter()
                .map(|c| 22 + c.name.len() + c.buildings.len() * 2 + c.queue.len() * 3)
                .sum::<usize>()
            + save.world.diplomacy.len()
            + save
                .world
                .knowledge_explored
                .iter()
                .map(|k| k.len())
                .sum::<usize>()) as u64;
        let t0 = Instant::now();
        for _ in 0..20 {
            black_box(classic_rules::persistence::encode::encode(&game, &rules));
        }
        out.push(Case {
            name: "save.encode",
            counters: WorkCounters::default(),
            extra: vec![("bytes", bytes)],
            wall_ms: ms(t0.elapsed()) / 20.0,
            wall_only: false,
        });
        let t0 = Instant::now();
        for _ in 0..20 {
            black_box(classic_rules::persistence::decode::decode(&save, &rules).unwrap());
        }
        out.push(Case {
            name: "save.decode",
            counters: WorkCounters::default(),
            extra: vec![("bytes", bytes)],
            wall_ms: ms(t0.elapsed()) / 20.0,
            wall_only: false,
        });
    }

    // Navigation: short / long / 100 mixed / maze (§53 + torture PathfindingMaze).
    {
        use classic_rules::navigation::find_path;
        let land = land_list(&game);
        let mut scratch = PathScratch::default();
        let t0 = Instant::now();
        for _ in 0..200 {
            black_box(find_path(
                game.view(),
                &rules,
                PathRequest {
                    from: t0tile,
                    to: t1tile,
                    player: PlayerId(0),
                    budget: NAV_BUDGET,
                    profile: classic_rules::navigation::MoveProfile::LAND,
                },
                black_box(&mut scratch),
            ));
        }
        out.push(Case {
            name: "nav.short",
            counters: WorkCounters {
                tiles_visited: scratch.expanded_last,
                entities_visited: 0,
                bytes_allocated: 0,
            },
            extra: vec![],
            wall_ms: ms(t0.elapsed()) / 200.0,
            wall_only: false,
        });
        let (a, b) = (land[0], *land.last().unwrap());
        let t0 = Instant::now();
        let p = black_box(find_path(
            game.view(),
            &rules,
            PathRequest {
                from: TileId(a),
                to: TileId(b),
                player: PlayerId(0),
                budget: NAV_BUDGET,
                profile: classic_rules::navigation::MoveProfile::LAND,
            },
            black_box(&mut scratch),
        ));
        out.push(Case {
            name: "nav.long",
            counters: WorkCounters {
                tiles_visited: scratch.expanded_last,
                entities_visited: 0,
                bytes_allocated: 0,
            },
            extra: vec![
                (
                    "turns",
                    p.as_ref().map(|p| p.cost.turns as u64).unwrap_or(u64::MAX),
                ),
                ("found", p.is_some() as u64),
            ],
            wall_ms: ms(t0.elapsed()),
            wall_only: false,
        });
        let t0 = Instant::now();
        let mut found = 0u64;
        let mut exp_tot = 0u64;
        for i in 0..100 {
            let aa = land[(i * 137) % land.len()];
            let bb = land[(i * 311 + 7) % land.len()];
            let p = find_path(
                game.view(),
                &rules,
                PathRequest {
                    from: TileId(aa),
                    to: TileId(bb),
                    player: PlayerId((i % 8) as u8),
                    budget: NAV_BUDGET,
                    profile: classic_rules::navigation::MoveProfile::LAND,
                },
                &mut scratch,
            );
            exp_tot += scratch.expanded_last;
            found += p.is_some() as u64;
            black_box(p);
        }
        out.push(Case {
            name: "nav.mixed100",
            counters: WorkCounters {
                tiles_visited: exp_tot / 100,
                entities_visited: 0,
                bytes_allocated: 0,
            },
            extra: vec![("found", found)],
            wall_ms: ms(t0.elapsed()),
            wall_only: false,
        });
        // Maze-torture: a wall with a gap at the bottom, 64x64.
        let mut maze = GameState::new(
            &rules,
            GameConfig {
                players: 1,
                width: 64,
                height: 64,
                rng_seed: 0,
            },
        );
        let wall: Vec<(TileId, classic_rules::ids::TerrainId)> = (0..63)
            .map(|y| {
                (
                    TileId(y * 64 + 32),
                    classic_rules::ids::TerrainId(rules.terrain_kinds.palette().code(classic_rules::rules::roles::Ground::Water)),
                )
            })
            .collect();
        maze.debug_set_terrain_batch(&rules, &wall);
        let t0 = Instant::now();
        let p = black_box(find_path(
            maze.view(),
            &rules,
            PathRequest {
                from: TileId(0),
                to: TileId(63),
                player: PlayerId(0),
                budget: NAV_BUDGET,
                profile: classic_rules::navigation::MoveProfile::LAND,
            },
            black_box(&mut scratch),
        ));
        out.push(Case {
            name: "nav.maze",
            counters: WorkCounters {
                tiles_visited: scratch.expanded_last,
                entities_visited: 0,
                bytes_allocated: 0,
            },
            extra: vec![("turns", p.map(|p| p.cost.turns as u64).unwrap_or(u64::MAX))],
            wall_ms: ms(t0.elapsed()),
            wall_only: false,
        });
    }

    // Torture: war (50 attacks, LargeWarFront spirit) + mass visibility (100 turns).
    {
        let mut arena = GameState::new(
            &rules,
            GameConfig {
                players: 2,
                width: 16,
                height: 16,
                rng_seed: 5,
            },
        );
        let a0 = TileId(4 * 16 + 4);
        let d0 = TileId(4 * 16 + 5);
        let atk = arena.debug_spawn_unit(&rules, PlayerId(0), a0, UnitKindId(0));
        let mut def = arena.debug_spawn_unit(&rules, PlayerId(1), d0, UnitKindId(0));
        arena
            .execute(
                &rules,
                classic_rules::Command::DeclareWar {
                    player: PlayerId(0),
                    enemy: PlayerId(1),
                },
            )
            .unwrap();
        let mut attack_time = std::time::Duration::ZERO;
        let mut kills = 0u64;
        let mut atk = atk;
        for _ in 0..50 {
            // Return fire kills both sides, so we respawn the dead. A
            // winner advances into the loser's tile: the new unit takes the
            // other of the two tiles.
            let other = |tile: Option<TileId>| if tile == Some(a0) { d0 } else { a0 };
            let tile_of = |g: &GameState, u| g.view().auth.units().get(u).map(|v| v.core.tile);
            if tile_of(&arena, atk).is_none() {
                let free = other(tile_of(&arena, def));
                atk = arena.debug_spawn_unit(&rules, PlayerId(0), free, UnitKindId(0));
                kills += 1;
            }
            if tile_of(&arena, def).is_none() {
                let free = other(tile_of(&arena, atk));
                def = arena.debug_spawn_unit(&rules, PlayerId(1), free, UnitKindId(0));
                kills += 1;
            }
            // One attack per unit and turn: a spent attacker waits a round
            // (not timed).
            let spent = arena.view().auth.units().get(atk).unwrap().core.moves_left == 0;
            if spent || arena.view().auth.active_player() != PlayerId(0) {
                next_round(&mut arena, &rules, PlayerId(0));
            }
            let t0 = Instant::now();
            black_box(
                arena
                    .execute(
                        &rules,
                        classic_rules::Command::AttackUnit {
                            attacker: atk,
                            defender: def,
                        },
                    )
                    .unwrap(),
            );
            attack_time += t0.elapsed();
        }
        arena.validate(&rules).unwrap();
        out.push(Case {
            name: "torture.warfront50",
            counters: WorkCounters::default(),
            extra: vec![("kills", kills)],
            wall_ms: ms(attack_time) / 50.0,
            wall_only: true,
        });
    }
    {
        // 100 random neighbouring moves: total work of the deltas.
        let t0 = Instant::now();
        let mut tot = WorkCounters::default();
        let units = game.view().auth.indexes().units_of(PlayerId(1)).to_vec();
        let dims = game.view().auth.map().dimensions;
        let n = dims.tile_count();
        let mut moved = 0u64;
        for (i, u) in units.iter().take(100).enumerate() {
            let cur = game.view().auth.units().get(*u).unwrap().core.tile.0;
            let dest = TileId(((cur as usize + 1) % n) as u32);
            // Moves outside the margin / into water are skipped (honest traffic, not fitting).
            if game
                .execute(
                    &rules,
                    classic_rules::Command::MoveUnit {
                        unit: *u,
                        destination: dest,
                    },
                )
                .is_ok()
            {
                moved += 1;
            }
            let _ = i;
        }
        tot.tiles_visited = moved;
        out.push(Case {
            name: "torture.massvis100",
            counters: tot,
            extra: vec![("moved", moved)],
            wall_ms: ms(t0.elapsed()),
            wall_only: true,
        });
    }

    // ---- Mechanics v1 (a content mini-world: settler/warrior/2 techs) ----
    // RON string through the real parser (no hardcoded tables).
    {
        let content = classic_content::ron::parse_ron(
            r#"(
            units: [
                (id: "core:unit/settler", movement: 2, strength: 1, founder: true, cost: 40),
                (id: "core:unit/warrior", movement: 2, strength: 10, cost: 30),
            ],
            buildings: [
                (id: "core:building/monument", cost: 40, effects: [(scope: City, selector: SourceCity, modifier: AddYield(yield_kind: Culture, amount: 2))]),
            ],
            technologies: [
                (id: "core:tech/agriculture", cost: 20, effects: [(scope: City, selector: OwnerCities, modifier: AddYieldPercent(yield_kind: Output, percent: 10))]),
                (id: "core:tech/pottery", cost: 40, effects: [(scope: City, selector: OwnerCities, modifier: AddYieldPercent(yield_kind: Output, percent: 10))]),
            ],
        )"#,
        )
        .expect("bench fixture must parse");
        let crules = compile_rules(content).unwrap();
        let mut mini = GameState::new(
            &crules,
            GameConfig {
                players: 2,
                width: 16,
                height: 16,
                rng_seed: 11,
            },
        );
        let dims = mini.view().auth.map().dimensions;
        let t = |x: i32, y: i32| {
            dims.to_tile(classic_rules::math::HexCoord { q: x, r: y })
                .unwrap()
        };
        // exec.found: a settler founds (validate+gating+commit+derived).
        // A fresh settler each time (the city absorbs the previous one);
        // the points differ so as not to pile everything onto one tile.
        let t0 = Instant::now();
        for i in 0..20 {
            // MIN_CITY_RANGE: sites three tiles apart.
            let st = t(2 + 3 * (i % 5), 2 + 3 * (i / 5));
            let s = mini.debug_spawn_unit(&crules, PlayerId(0), st, UnitKindId(0));
            black_box(
                mini.execute(&crules, classic_rules::Command::FoundCity { settler: s })
                    .unwrap(),
            );
        }
        out.push(Case {
            name: "exec.found",
            counters: WorkCounters::default(),
            extra: vec![],
            wall_ms: ms(t0.elapsed()) / 20.0,
            wall_only: true,
        });
        // territory.claim: ring claim at founding (claim_territory:
        // 7 tiles OwnerChanged + journal). The counter is the ring tiles,
        // counters gate (§49): exactly the WORK_RADIUS ring, no more.
        let t0 = Instant::now();
        let ring = classic_rules::math::tiles_in_hex_radius(
            &dims,
            t(8, 8),
            classic_rules::derived::yields::CITY_TERRITORY_RADIUS,
        )
        .len() as u64;
        for i in 0..20 {
            // One city per tile: twenty distinct tiles on the two last rows.
            let ct = t(i % 16, 15 - i / 16);
            black_box(mini.debug_found_city(&crules, PlayerId(1), ct));
        }
        let mut tc = WorkCounters::default();
        tc.tiles_visited = ring * 20;
        out.push(Case {
            name: "territory.claim",
            counters: tc,
            extra: vec![("cities", 20)],
            wall_ms: ms(t0.elapsed()) / 20.0,
            wall_only: true,
        });
        // city.produce: turns with a city (income + warrior spawns).
        let t0 = Instant::now();
        for _ in 0..10 {
            next_round(&mut mini, &crules, PlayerId(0));
        }
        out.push(Case {
            name: "city.produce",
            counters: WorkCounters::default(),
            extra: vec![("units", mini.view().auth.units().len() as u64)],
            wall_ms: ms(t0.elapsed()) / 10.0,
            wall_only: true,
        });
        // production.queue: 2 orders (settler+warrior) until the queue drains.
        // Gates the full cycle: validate + commit + growth + overflow + spawns.
        // Its own mini-world: the queue must not shift the harness's downstream cases.
        let mut mini2 = GameState::new(
            &crules,
            GameConfig {
                players: 2,
                width: 16,
                height: 16,
                rng_seed: 12,
            },
        );
        let dims2 = mini2.view().auth.map().dimensions;
        let t2 = |x: i32, y: i32| {
            dims2
                .to_tile(classic_rules::math::HexCoord { q: x, r: y })
                .unwrap()
        };
        let qc = t2(8, 8);
        let qcity = mini2.debug_found_city(&crules, PlayerId(0), qc);
        // Settlers need size 2 (SETTLER_MIN_CITY_SIZE).
        mini2.debug_grow_city(&crules, qcity, 1);
        mini2
            .execute(
                &crules,
                classic_rules::Command::EnqueueProduction {
                    player: PlayerId(0),
                    city: qcity,
                    item: classic_rules::ids::ProductionItem::Unit(UnitKindId(0)),
                },
            )
            .unwrap();
        mini2
            .execute(
                &crules,
                classic_rules::Command::EnqueueProduction {
                    player: PlayerId(0),
                    city: qcity,
                    item: classic_rules::ids::ProductionItem::Unit(UnitKindId(1)),
                },
            )
            .unwrap();
        let t0 = Instant::now();
        let mut turns = 0u64;
        for _ in 0..40 {
            next_round(&mut mini2, &crules, PlayerId(0));
            turns += 1;
            if mini2
                .view()
                .auth
                .cities()
                .get(qcity)
                .unwrap()
                .queue
                .is_empty()
            {
                break;
            }
        }
        out.push(Case {
            name: "production.queue",
            counters: WorkCounters::default(),
            extra: vec![("orders", 2), ("turns", turns)],
            wall_ms: ms(t0.elapsed()) / turns.max(1) as f64,
            wall_only: true,
        });
        // culture.expand + workforce.toggle: its own mini-world (do not shift downstream).
        let mut mini3 = GameState::new(
            &crules,
            GameConfig {
                players: 2,
                width: 16,
                height: 16,
                rng_seed: 13,
            },
        );
        let dims3 = mini3.view().auth.map().dimensions;
        let t3 = |x: i32, y: i32| {
            dims3
                .to_tile(classic_rules::math::HexCoord { q: x, r: y })
                .unwrap()
        };
        let cc = t3(8, 8);
        let ccity = mini3.debug_found_city(&crules, PlayerId(0), cc);
        // BNW: culture comes from buildings only (a Monument, 2 a turn).
        mini3.debug_add_building(&crules, ccity, classic_rules::ids::BuildingKindId(0));
        let t0 = Instant::now();
        let mut cturns = 0u64;
        for _ in 0..30 {
            next_round(&mut mini3, &crules, PlayerId(0));
            cturns += 1;
            if mini3.view().auth.cities().get(ccity).unwrap().radius >= 2 {
                break;
            }
        }
        // One level per turn : the candidate scan covers ring 5 (91
        // tiles) once; one acquisition in the case (radius 1→2).
        let ctc = WorkCounters {
            tiles_visited: 91,
            ..Default::default()
        };
        out.push(Case {
            name: "culture.expand",
            counters: ctc,
            extra: vec![("turns", cturns)],
            wall_ms: ms(t0.elapsed()) / cturns.max(1) as f64,
            wall_only: true,
        });
        // workforce.toggle: two toggles there and back (validate + commit + recompute).
        let wt = t3(9, 8);
        let t0 = Instant::now();
        for _ in 0..20 {
            black_box(
                mini3
                    .execute(
                        &crules,
                        classic_rules::Command::ToggleWorkedTile {
                            player: PlayerId(0),
                            city: ccity,
                            tile: wt,
                        },
                    )
                    .unwrap(),
            );
        }
        out.push(Case {
            name: "workforce.toggle",
            counters: WorkCounters::default(),
            extra: vec![("toggles", 20)],
            wall_ms: ms(t0.elapsed()) / 20.0,
            wall_only: true,
        });
        // economy.hurry: an order + accumulation + buyout (validate + commit +
        // gold + instant completion). The same mini3 (the city is already developed).
        mini3
            .execute(
                &crules,
                classic_rules::Command::EnqueueProduction {
                    player: PlayerId(0),
                    city: ccity,
                    item: classic_rules::ids::ProductionItem::Unit(UnitKindId(1)),
                },
            )
            .unwrap();
        let t0 = Instant::now();
        let mut hturns = 0u64;
        for _ in 0..40 {
            next_round(&mut mini3, &crules, PlayerId(0));
            hturns += 1;
            if mini3
                .execute(
                    &crules,
                    classic_rules::Command::HurryProduction {
                        player: PlayerId(0),
                        city: ccity,
                    },
                )
                .is_ok()
            {
                break;
            }
        }
        out.push(Case {
            name: "economy.hurry",
            counters: WorkCounters::default(),
            extra: vec![("turns", hturns)],
            wall_ms: ms(t0.elapsed()) / hturns.max(1) as f64,
            wall_only: true,
        });
        // diplomacy.peace: DoW + peace + grants (validate + commit + journal).
        // Its own mini-world: both alive (one city each), no contact.
        let mut mini4 = GameState::new(
            &crules,
            GameConfig {
                players: 2,
                width: 16,
                height: 16,
                rng_seed: 14,
            },
        );
        let dims4 = mini4.view().auth.map().dimensions;
        let t4 = |x: i32, y: i32| {
            dims4
                .to_tile(classic_rules::math::HexCoord { q: x, r: y })
                .unwrap()
        };
        mini4.debug_found_city(&crules, PlayerId(0), t4(3, 3));
        mini4.debug_found_city(&crules, PlayerId(1), t4(12, 12));
        // War needs contact; a peace treaty forbids a new war for 10
        // rounds (M4 diplomacy): every pass starts from the met world.
        mini4.debug_meet(PlayerId(0), PlayerId(1));
        let mut peace_time = std::time::Duration::ZERO;
        for _ in 0..20 {
            let mut world = mini4.clone();
            let t0 = Instant::now();
            let war = classic_rules::Command::DeclareWar {
                player: PlayerId(0),
                enemy: PlayerId(1),
            };
            let peace = classic_rules::Command::MakePeace {
                player: PlayerId(0),
                enemy: PlayerId(1),
            };
            let borders = classic_rules::Command::OpenBorders {
                host: PlayerId(0),
                guest: PlayerId(1),
            };
            black_box(world.execute(&crules, war).unwrap());
            black_box(world.execute(&crules, peace).unwrap());
            black_box(world.execute(&crules, borders).is_ok());
            peace_time += t0.elapsed();
        }
        out.push(Case {
            name: "diplomacy.peace",
            counters: WorkCounters::default(),
            extra: vec![("rounds", 20)],
            wall_ms: ms(peace_time) / 20.0,
            wall_only: true,
        });
        // improve.farm: a worker builds a farm in 3 turns (validate + ticks +
        // completion with the ImprovementBuilt recompute).
        // Its own mini-world: content as a RON string (no hardcoded tables).
        let imp_rules = {
            let content = classic_content::ron::parse_ron(
                r#"(
                units: [
                    (id: "core:unit/settler", movement: 2, strength: 1, founder: true, cost: 40),
                    (id: "core:unit/worker", movement: 2, strength: 1, builder: true, cost: 30),
                ],
                improvements: [
                    (id: "core:improvement/farm", work_turns: 3, food_bonus: 2, prod_bonus: 0, river_food: 1, required_tech: None),
                ],
            )"#,
            )
            .expect("bench fixture must parse");
            classic_rules::compile_rules(content).unwrap()
        };
        let mut mini5 = GameState::new(
            &imp_rules,
            GameConfig {
                players: 2,
                width: 16,
                height: 16,
                rng_seed: 15,
            },
        );
        let dims5 = mini5.view().auth.map().dimensions;
        let t5 = |x: i32, y: i32| {
            dims5
                .to_tile(classic_rules::math::HexCoord { q: x, r: y })
                .unwrap()
        };
        mini5.debug_found_city(&imp_rules, PlayerId(0), t5(8, 8));
        let w5 = mini5.debug_spawn_unit(
            &imp_rules,
            PlayerId(0),
            t5(9, 8),
            classic_rules::ids::UnitKindId(1),
        );
        mini5
            .execute(
                &imp_rules,
                classic_rules::Command::BuildImprovement {
                    unit: w5,
                    improvement: classic_rules::ids::ImprovementId(0),
                },
            )
            .unwrap();
        let t0 = Instant::now();
        for _ in 0..3 {
            next_round(&mut mini5, &imp_rules, PlayerId(0));
        }
        out.push(Case {
            name: "improve.farm",
            counters: WorkCounters::default(),
            extra: vec![("turns", 3)],
            wall_ms: ms(t0.elapsed()) / 3.0,
            wall_only: true,
        });
        // improve.road: a worker paves in 2 turns (the same work mechanism,
        // another target: RoadBuilt instead of ImprovementBuilt).
        let rw = mini5.debug_spawn_unit(
            &imp_rules,
            PlayerId(0),
            t5(10, 8),
            classic_rules::ids::UnitKindId(1),
        );
        mini5
            .execute(&imp_rules, classic_rules::Command::BuildRoad { unit: rw })
            .unwrap();
        let t0 = Instant::now();
        for _ in 0..2 {
            next_round(&mut mini5, &imp_rules, PlayerId(0));
        }
        out.push(Case {
            name: "improve.road",
            counters: WorkCounters::default(),
            extra: vec![("turns", 2)],
            wall_ms: ms(t0.elapsed()) / 2.0,
            wall_only: true,
        });
        // siege.capture: a warrior at the walls -> strikes until capture (damage + retaliation
        // + claim_territory + OwnerChanged recomputes).
        let sct = t4(5, 5);
        let sat = t4(4, 5);
        let scity = mini4.debug_found_city(&crules, PlayerId(1), sct);
        let mut satk = mini4.debug_spawn_unit(&crules, PlayerId(0), sat, UnitKindId(1));
        mini4
            .execute(
                &crules,
                classic_rules::Command::DeclareWar {
                    player: PlayerId(0),
                    enemy: PlayerId(1),
                },
            )
            .unwrap();
        let t0 = Instant::now();
        let mut shots = 0u64;
        for _ in 0..10 {
            // BNW cities strike back hard: a fallen attacker is replaced.
            if mini4.view().auth.units().get(satk).is_none() {
                satk = mini4.debug_spawn_unit(&crules, PlayerId(0), sat, UnitKindId(1));
            }
            black_box(
                mini4
                    .execute(
                        &crules,
                        classic_rules::Command::AttackCity {
                            attacker: satk,
                            city: scity,
                        },
                    )
                    .unwrap(),
            );
            shots += 1;
            if mini4.view().auth.cities().get(scity).unwrap().owner == PlayerId(0) {
                break;
            }
            next_round(&mut mini4, &crules, PlayerId(0));
        }
        out.push(Case {
            name: "siege.capture",
            counters: WorkCounters::default(),
            extra: vec![("shots", shots)],
            wall_ms: ms(t0.elapsed()) / shots.max(1) as f64,
            wall_only: true,
        });
        // research.complete: an agri order + beakers until completion (A2: queue,
        // stockpile, learn_tech + TechnologyChosen -> yields recompute).
        mini3
            .execute(
                &crules,
                classic_rules::Command::ChooseTechnology {
                    player: PlayerId(0),
                    technology: classic_rules::ids::TechId(0),
                },
            )
            .unwrap();
        let t0 = Instant::now();
        let mut rturns = 0u64;
        for _ in 0..60 {
            next_round(&mut mini3, &crules, PlayerId(0));
            rturns += 1;
            if mini3
                .view()
                .auth
                .players()
                .techs_of(PlayerId(0))
                .contains(&classic_rules::ids::TechId(0))
            {
                break;
            }
        }
        out.push(Case {
            name: "research.complete",
            counters: WorkCounters::default(),
            extra: vec![("turns", rturns)],
            wall_ms: ms(t0.elapsed()) / rturns.max(1) as f64,
            wall_only: true,
        });
        // tech.choose + yields.tech_all (exact counters of the empire recompute).
        let t0 = Instant::now();
        black_box(
            mini.execute(
                &crules,
                classic_rules::Command::ChooseTechnology {
                    player: PlayerId(0),
                    technology: classic_rules::ids::TechId(0),
                },
            )
            .unwrap(),
        );
        out.push(Case {
            name: "tech.choose",
            counters: WorkCounters::default(),
            extra: vec![],
            wall_ms: ms(t0.elapsed()),
            wall_only: true,
        });
        let mut ys = YieldState::rebuild(mini.view().auth, &crules);
        let mut journal = classic_rules::transaction::journal::ChangeJournal::default();
        journal.player_changes.push(
            classic_rules::transaction::journal::PlayerChange::TechnologyChosen {
                player: PlayerId(0),
            },
        );
        let t0 = Instant::now();
        let tc = YieldState::apply_changes_counted(
            black_box(&mut ys),
            &journal,
            mini.view().auth,
            &crules,
        );
        out.push(Case {
            name: "yields.tech_all",
            counters: tc,
            extra: vec![],
            wall_ms: ms(t0.elapsed()),
            wall_only: false,
        });
        // march.step: one march commit (A* in commit) on the rep map above;
        // here on mini for stability (rep is already covered by nav.*).
        // War P0-P1: the route goes through foreign lands (otherwise closed borders).
        // War needs contact (M4 diplomacy).
        mini.debug_meet(PlayerId(0), PlayerId(1));
        mini.execute(
            &crules,
            classic_rules::Command::DeclareWar {
                player: PlayerId(0),
                enemy: PlayerId(1),
            },
        )
        .unwrap();
        let m = mini.debug_spawn_unit(&crules, PlayerId(1), t(12, 12), UnitKindId(1));
        // P1 marches on its own turn (sequential turns).
        next_round(&mut mini, &crules, PlayerId(1));
        let t0 = Instant::now();
        black_box(
            mini.execute(
                &crules,
                classic_rules::Command::MarchTo {
                    unit: m,
                    // Next to the city at (2,2): a city tile is not entered.
                    destination: t(3, 3),
                },
            )
            .unwrap(),
        );
        out.push(Case {
            name: "march.step",
            counters: WorkCounters::default(),
            extra: vec![],
            wall_ms: ms(t0.elapsed()),
            wall_only: true,
        });
        mini.validate(&crules).unwrap();
    }

    // ---- M3.0: real content (core.ron) on the representative map ----
    core_content_cases(&mut out);
    // ---- M4: combat on the content rules ----
    combat_cases(&mut out);

    // ---- Stress (§2.2): 50k, 500 cities, 5000 units ----
    let stress = ScenarioConfig::stress();
    let t0 = Instant::now();
    let sgame = generate(&stress, &rules);
    sgame.validate(&rules).expect("stress validates");
    out.push(Case {
        name: "gen.stress",
        counters: WorkCounters::default(),
        extra: vec![
            ("tiles", (stress.width * stress.height) as u64),
            ("cities", 500),
            ("units", 5000),
        ],
        wall_ms: ms(t0.elapsed()),
        wall_only: true,
    });
    let t0 = Instant::now();
    let mut sc = WorkCounters::default();
    for _ in 0..5 {
        let (v, cc) = VisibilityState::rebuild_counted(black_box(sgame.view().auth), &rules);
        acc += black_box(v.counts.len());
        sc = cc;
    }
    out.push(Case {
        name: "visibility.rebuild.stress",
        counters: sc,
        extra: vec![],
        wall_ms: ms(t0.elapsed()) / 5.0,
        wall_only: false,
    });
    let t0 = Instant::now();
    let mut yc = WorkCounters::default();
    for _ in 0..5 {
        let (y, cc) = YieldState::rebuild_counted(black_box(sgame.view().auth), &rules);
        acc += black_box(y.city.len());
        yc = cc;
    }
    out.push(Case {
        name: "yields.rebuild.stress",
        counters: yc,
        extra: vec![],
        wall_ms: ms(t0.elapsed()) / 5.0,
        wall_only: false,
    });
    black_box(acc);
    out
}

/// Percentile of samples in microseconds (nearest-rank, sorted copy).
fn percentile_us(samples: &[f64], pct: usize) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let rank = (sorted.len() * pct).div_ceil(100).max(1) - 1;
    (sorted[rank] * 1000.0) as u64
}

/// Core content on the representative map: every city gets six ordinary
/// buildings, players research the cheapest techs during warm-up rounds.
fn core_rep_game(rules: &classic_rules::CompiledRuleset) -> GameState {
    let rep = ScenarioConfig::representative_late_game();
    let mut game = generate(&rep, rules);
    let keys = [
        "core:building/monument",
        "core:building/granary",
        "core:building/library",
        "core:building/barracks",
        "core:building/market",
        "core:building/temple",
    ];
    let cities: Vec<_> = game.view().auth.cities().iter().map(|(h, _)| h).collect();
    for city in cities {
        for key in keys {
            let index = rules.buildings.iter().position(|b| b.key == key).unwrap();
            game.debug_add_building(rules, city, classic_rules::ids::BuildingKindId(index as u16));
        }
    }
    // A treasury for the BNW maintenance of 25 cities × 6 buildings (M4-A):
    // without it every player runs a deficit that eats its science and the
    // case stops measuring a late game.
    for p in 0..game.view().auth.players().count() {
        game.debug_earn(PlayerId(p as u8), 100_000);
    }
    game
}

/// One round = EndTurn of every player; ChooseTechnology for every tech is
/// tried first (validate rejects locked and queued ones).
fn core_round(game: &mut GameState, rules: &classic_rules::CompiledRuleset) {
    let players = game.view().auth.players().count();
    for p in 0..players {
        let player = PlayerId(p as u8);
        for t in 0..rules.technologies.len() {
            let technology = classic_rules::ids::TechId(t as u16);
            let _ = game.execute(
                rules,
                classic_rules::Command::ChooseTechnology { player, technology },
            );
        }
        let _ = game.execute(rules, classic_rules::Command::EndTurn { player });
    }
}

/// Content of the content cases: `AURELIA_RULESET=legacy` is the frozen pre-BNW
/// core.ron, anything else the default BNW ruleset.
fn content_source() -> (&'static str, SourceRules) {
    match std::env::var("AURELIA_RULESET").as_deref() {
        Ok("legacy") => ("legacy", classic_content::legacy::legacy_rules()),
        _ => ("classic", classic_content::core::core_rules()),
    }
}

/// Parse + compile of the content ruleset: median and p95 of 21 runs after
/// one warm-up (parse = RON text to SourceRules, compile = compile_rules).
fn rules_compile_case(out: &mut Vec<Case>) {
    let (name, source) = content_source();
    let tables = source.units.len() + source.buildings.len() + source.technologies.len();
    drop(black_box(compile_rules(source).unwrap()));
    let mut parse_ms = Vec::new();
    let mut compile_ms = Vec::new();
    for _ in 0..21 {
        let t0 = Instant::now();
        let source = black_box(content_source().1);
        parse_ms.push(ms(t0.elapsed()));
        let t1 = Instant::now();
        let rules = black_box(compile_rules(source).unwrap());
        compile_ms.push(ms(t1.elapsed()));
        drop(rules);
    }
    let median = |v: &[f64]| percentile_us(v, 50);
    out.push(Case {
        name: if name == "legacy" {
            "rules.compile.legacy"
        } else {
            "rules.compile.classic"
        },
        counters: WorkCounters::default(),
        extra: vec![
            ("rows_units_buildings_techs", tables as u64),
            ("parse_p50_us", median(&parse_ms)),
            ("parse_p95_us", percentile_us(&parse_ms, 95)),
            ("compile_p50_us", median(&compile_ms)),
            ("compile_p95_us", percentile_us(&compile_ms, 95)),
        ],
        wall_ms: compile_ms.iter().sum::<f64>() / compile_ms.len() as f64,
        wall_only: true,
    });
}

fn core_content_cases(out: &mut Vec<Case>) {
    let rules = compile_rules(content_source().1).unwrap();
    let mut game = core_rep_game(&rules);
    for _ in 0..20 {
        core_round(&mut game, &rules);
    }
    perf_economy::cases(out, &mut game.clone(), &rules);
    let techs = game.view().auth.players().techs_of(PlayerId(0)).len() as u64;
    let mut round_ms = Vec::new();
    for _ in 0..20 {
        let t0 = Instant::now();
        core_round(&mut game, &rules);
        round_ms.push(ms(t0.elapsed()));
    }
    out.push(Case {
        name: "turn.core.rep",
        counters: WorkCounters::default(),
        extra: vec![
            ("techs_p0", techs),
            ("p50_us", percentile_us(&round_ms, 50)),
            ("p95_us", percentile_us(&round_ms, 95)),
        ],
        wall_ms: round_ms.iter().sum::<f64>() / round_ms.len() as f64,
        wall_only: true,
    });
    let mut rebuild_ms = Vec::new();
    let mut counters = WorkCounters::default();
    for _ in 0..20 {
        let t0 = Instant::now();
        let (y, c) = YieldState::rebuild_counted(black_box(game.view().auth), &rules);
        rebuild_ms.push(ms(t0.elapsed()));
        black_box(y.city.len());
        counters = c;
    }
    out.push(Case {
        name: "yields.rebuild.core.rep",
        counters,
        extra: vec![
            ("p50_us", percentile_us(&rebuild_ms, 50)),
            ("p95_us", percentile_us(&rebuild_ms, 95)),
        ],
        wall_ms: rebuild_ms.iter().sum::<f64>() / rebuild_ms.len() as f64,
        wall_only: true,
    });
    policy_cases(out, &game, &rules);
    religion_cases(out, &game, &rules);
    tourism_cases(out, &game, &rules);
}

/// Religion on the representative BNW world: five players found a religion
/// in their first city (Great Prophets placed by the harness), then rounds
/// run with religious pressure between all 200 cities; and the faith
/// overview the AI observation and the religion screen read.
fn religion_cases(out: &mut Vec<Case>, game: &GameState, rules: &classic_rules::CompiledRuleset) {
    let prophet = rules.units.iter().position(|u| u.key == "core:unit/great-prophet");
    let Some(prophet) = prophet else {
        return;
    };
    let category = |c| {
        rules
            .module.beliefs
            .iter()
            .enumerate()
            .filter(move |(_, b)| b.category == Some(c))
            .map(|(i, _)| classic_rules::ids::BeliefId(i as u16))
    };
    use classic_rules::rules::content::BeliefCategory as C;
    let mut pantheons = category(C::Pantheon);
    let mut founders = category(C::Founder);
    let mut followers = category(C::Follower);
    let mut world = game.clone();
    let mut founded = 0u64;
    for p in 0..5u8 {
        let player = PlayerId(p);
        while world.view().auth.active_player() != player {
            let active = world.view().auth.active_player();
            let _ = world.execute(rules, classic_rules::Command::EndTurn { player: active });
        }
        let Some(city) = world.view().auth.indexes().cities_of(player).first().copied() else {
            continue;
        };
        let tile = world.view().auth.cities().get(city).unwrap().tile;
        let unit = world.debug_spawn_unit(rules, player, tile, UnitKindId(prophet as u16));
        let command = classic_rules::Command::FoundReligion {
            unit,
            religion: classic_rules::rules::content::ReligionId(p as u16),
            founder: founders.next().unwrap(),
            follower: followers.next().unwrap(),
            pantheon: pantheons.next(),
            bonus: None,
        };
        if world.execute(rules, command).is_ok() {
            founded += 1;
        }
    }
    for _ in 0..5 {
        core_round(&mut world, rules);
    }
    let mut round_ms = Vec::new();
    for _ in 0..20 {
        let t0 = Instant::now();
        core_round(&mut world, rules);
        round_ms.push(ms(t0.elapsed()));
    }
    out.push(Case {
        name: "turn.religion.rep",
        counters: WorkCounters::default(),
        extra: vec![
            ("religions", founded),
            ("p50_us", percentile_us(&round_ms, 50)),
            ("p95_us", percentile_us(&round_ms, 95)),
        ],
        wall_ms: round_ms.iter().sum::<f64>() / round_ms.len() as f64,
        wall_only: true,
    });
    let player = PlayerId(0);
    let mut overview_ms = Vec::new();
    for _ in 0..21 {
        let t0 = Instant::now();
        black_box(classic_rules::derived::religion::faith_overview(world.view().auth, rules, player, 0));
        overview_ms.push(ms(t0.elapsed()));
    }
    out.push(Case {
        name: "faith.overview.rep",
        counters: WorkCounters::default(),
        extra: vec![
            ("p50_us", percentile_us(&overview_ms, 50)),
            ("p95_us", percentile_us(&overview_ms, 95)),
        ],
        wall_ms: overview_ms.iter().sum::<f64>() / overview_ms.len() as f64,
        wall_only: true,
    });
}

/// Policies on the representative BNW world: unlocking Liberty (its opener
/// reaches every city of the player) and adopting Republic, each a
/// validate + commit + yield refresh of the player's cities; and the
/// culture overview the AI observation and the Civics screen read.
fn policy_cases(out: &mut Vec<Case>, game: &GameState, rules: &classic_rules::CompiledRuleset) {
    let player = game.view().auth.active_player();
    let liberty = rules.module.policy_branches.iter().position(|b| b.key == "core:policy/liberty");
    let republic = rules.module.policies.iter().position(|p| p.key == "core:policy/liberty/republic");
    let (Some(liberty), Some(republic)) = (liberty, republic) else {
        return;
    };
    let cities = game.view().auth.indexes().cities_of(player).len() as u64;
    let mut adopt_ms = Vec::new();
    for _ in 0..21 {
        let mut copy = game.clone();
        copy.debug_grant_culture(player, 100_000);
        let t0 = Instant::now();
        copy.execute(
            rules,
            classic_rules::Command::UnlockPolicyBranch {
                player,
                branch: classic_rules::rules::content::PolicyBranchId(liberty as u16),
            },
        )
        .expect("liberty unlocks");
        copy.execute(
            rules,
            classic_rules::Command::AdoptPolicy {
                player,
                policy: classic_rules::ids::PolicyId(republic as u16),
            },
        )
        .expect("republic adopts");
        adopt_ms.push(ms(t0.elapsed()));
        black_box(copy);
    }
    out.push(Case {
        name: "policy.adopt.rep",
        counters: WorkCounters {
            entities_visited: cities * 2,
            ..Default::default()
        },
        extra: vec![
            ("cities_p", cities),
            ("p50_us", percentile_us(&adopt_ms, 50)),
            ("p95_us", percentile_us(&adopt_ms, 95)),
        ],
        wall_ms: adopt_ms.iter().sum::<f64>() / adopt_ms.len() as f64,
        wall_only: true,
    });
    let mut overview_ms = Vec::new();
    let mut adoptable = 0u64;
    for _ in 0..21 {
        let t0 = Instant::now();
        let overview = black_box(classic_rules::derived::culture::culture_overview(
            game.view().auth,
            rules,
            player,
            0,
        ));
        overview_ms.push(ms(t0.elapsed()));
        adoptable = (overview.adoptable.len() + overview.unlockable.len()) as u64;
    }
    out.push(Case {
        name: "culture.overview.rep",
        counters: WorkCounters {
            entities_visited: (rules.module.policies.len() + rules.module.policy_branches.len()) as u64,
            ..Default::default()
        },
        extra: vec![
            ("choices", adoptable),
            ("p50_us", percentile_us(&overview_ms, 50)),
            ("p95_us", percentile_us(&overview_ms, 95)),
        ],
        wall_ms: overview_ms.iter().sum::<f64>() / overview_ms.len() as f64,
        wall_only: true,
    });
}

/// A front of 2×`width` warriors facing each other on the content rules, at
/// war (combat cases).
fn combat_front(rules: &classic_rules::CompiledRuleset, width: i32) -> (GameState, Vec<classic_rules::ids::UnitHandle>, Vec<TileId>) {
    use classic_rules::math::HexCoord;
    let mut game = GameState::new(
        rules,
        GameConfig {
            players: 2,
            width: 48,
            height: 48,
            rng_seed: 11,
        },
    );
    let kind = rules
        .units
        .iter()
        .position(|u| u.key == "core:unit/warrior")
        .map(|i| UnitKindId(i as u16))
        .unwrap_or(UnitKindId(0));
    let dims = game.view().auth.map().dimensions;
    let mut attackers = Vec::new();
    let mut targets = Vec::new();
    for i in 0..width {
        let a = dims.to_tile(HexCoord { q: 10, r: 2 + i }).unwrap();
        let d = dims.to_tile(HexCoord { q: 11, r: 2 + i }).unwrap();
        attackers.push(game.debug_spawn_unit(rules, PlayerId(0), a, kind));
        game.debug_spawn_unit(rules, PlayerId(1), d, kind);
        targets.push(d);
    }
    game.execute(
        rules,
        classic_rules::Command::DeclareWar {
            player: PlayerId(0),
            enemy: PlayerId(1),
        },
    )
    .unwrap();
    (game, attackers, targets)
}

/// Combat cases on the content rules: one attack preview (the UI and AI
/// query), a round of melee attacks, the AI combat decisions.
fn combat_cases(out: &mut Vec<Case>) {
    use classic_rules::combat::exchange::AttackKind;
    let rules = compile_rules(content_source().1).unwrap();
    let (game, attackers, targets) = combat_front(&rules, 40);
    let mut preview_us: Vec<f64> = Vec::new();
    for _ in 0..9 {
        let t0 = Instant::now();
        for (a, t) in attackers.iter().zip(&targets) {
            let plan = classic_rules::combat::plan_attack(game.view(), &rules, *a, *t, AttackKind::Melee).unwrap();
            black_box(plan.preview());
        }
        preview_us.push(ms(t0.elapsed()));
    }
    out.push(Case {
        name: "combat.preview40.content",
        counters: WorkCounters::default(),
        extra: vec![("p50_us", percentile_us(&preview_us, 50))],
        wall_ms: percentile_f(&preview_us, 50),
        wall_only: true,
    });
    let mut round_ms = Vec::new();
    for _ in 0..9 {
        let mut copy = game.clone();
        let t0 = Instant::now();
        for (a, t) in attackers.iter().zip(&targets) {
            let defender = copy.view().auth.indexes().units_on(*t).first().copied();
            if let Some(defender) = defender {
                let _ = copy.execute(&rules, classic_rules::Command::AttackUnit { attacker: *a, defender });
            }
        }
        round_ms.push(ms(t0.elapsed()));
    }
    out.push(Case {
        name: "combat.melee40.content",
        counters: WorkCounters::default(),
        extra: vec![("p50_us", percentile_us(&round_ms, 50))],
        wall_ms: percentile_f(&round_ms, 50),
        wall_only: true,
    });
    let mut ai_ms = Vec::new();
    for _ in 0..9 {
        let t0 = Instant::now();
        let obs = game.ai_observation(&rules, PlayerId(0));
        black_box(classic_rules::ai::combat::ranged_orders(&obs, &rules));
        black_box(classic_rules::ai::combat::melee_orders(&obs, &rules));
        ai_ms.push(ms(t0.elapsed()));
    }
    out.push(Case {
        name: "ai.combat40.content",
        counters: WorkCounters::default(),
        extra: vec![("p50_us", percentile_us(&ai_ms, 50))],
        wall_ms: percentile_f(&ai_ms, 50),
        wall_only: true,
    });
}

/// A Standard-size BNW game with 8 AI nations, 16 city-states and
/// barbarians, played by the AI driver for `rounds` rounds.
fn minors_game(rules: &classic_rules::CompiledRuleset, rounds: u32) -> GameState {
    use classic_rules::scenario::{generate_new_game_with, NewGameOptions};
    let cfg = ScenarioConfig {
        seed: 31,
        players: 8,
        width: 80,
        height: 52,
        cities: 0,
        units: 0,
        water_ratio: 30,
    };
    let options = NewGameOptions {
        barbarians: classic_rules::world::barbarians::BarbarianMode::Normal,
        city_states: 16,
        ..Default::default()
    };
    let mut game = generate_new_game_with(rules, &cfg, None, options);
    let mut driver = new_driver(rules);
    while game.view().auth.turn().round < rounds {
        driver.run(&mut game, rules, u32::MAX);
    }
    game
}

fn new_driver(rules: &classic_rules::CompiledRuleset) -> classic_rules::ai::AiDriver {
    let settings = classic_rules::ai::AiSettings::for_rules(rules, rules.default_difficulty());
    classic_rules::ai::AiDriver::new(settings, Vec::new())
}

/// M4 track C cases on the minors game after 80 AI rounds: a round of
/// every player ending its turn (city-states, barbarians, congress,
/// espionage steps), a full AI round (a fresh driver: its memory is
/// rebuilt in that round), one nation's observation.
fn minors_cases(out: &mut Vec<Case>) {
    let rules = compile_rules(content_source().1).unwrap();
    let game = minors_game(&rules, 80);
    let mut turn_ms = Vec::new();
    let mut ai_ms = Vec::new();
    let mut observe_ms = Vec::new();
    for _ in 0..9 {
        let mut copy = game.clone();
        let t0 = Instant::now();
        let round = copy.view().auth.turn().round;
        while copy.view().auth.turn().round == round {
            let active = copy.view().auth.active_player();
            let _ = copy.execute(&rules, classic_rules::Command::EndTurn { player: active });
        }
        turn_ms.push(ms(t0.elapsed()));
        let mut copy = game.clone();
        let mut ai = new_driver(&rules);
        let t0 = Instant::now();
        ai.run(&mut copy, &rules, u32::MAX);
        ai_ms.push(ms(t0.elapsed()));
        let t0 = Instant::now();
        black_box(game.ai_observation(&rules, PlayerId(0)));
        observe_ms.push(ms(t0.elapsed()));
    }
    for (name, samples) in [
        ("turn.minors.std", &turn_ms),
        ("ai.round.minors.std", &ai_ms),
        ("ai.observe.minors.std", &observe_ms),
    ] {
        out.push(Case {
            name,
            counters: WorkCounters::default(),
            extra: vec![("p50_us", percentile_us(samples, 50)), ("p95_us", percentile_us(samples, 95))],
            wall_ms: percentile_f(samples, 50),
            wall_only: true,
        });
    }
}

/// Nearest-rank percentile of the samples (their unit).
fn percentile_f(samples: &[f64], pct: usize) -> f64 {
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let rank = (pct * sorted.len()).div_ceil(100).clamp(1, sorted.len());
    sorted[rank - 1]
}

fn print_human(cases: &[Case]) {
    println!(
        "{:<28} {:>12} {:>12} {:>12} {:>10}",
        "metric", "tiles", "entities", "extra", "wall_ms"
    );
    for c in cases {
        let extra: String = c
            .extra
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{:<28} {:>12} {:>12} {:>12} {:>10.3}{}",
            c.name,
            c.counters.tiles_visited,
            c.counters.entities_visited,
            extra,
            c.wall_ms,
            if c.wall_only { "  (wall-only)" } else { "" }
        );
    }
}

/// Parsed baseline: (metric, [(key, value)], wall_ms).
type ParsedBaseline = Vec<(String, Vec<(String, u64)>, f64)>;

/// Parser of its own format (no dependencies): m name k=v ... wall_ms=x.
fn parse_baseline(text: &str) -> ParsedBaseline {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("m ") {
            continue;
        }
        let mut parts = line[2..].split_whitespace();
        let name = parts.next().unwrap_or("?").to_string();
        let mut kvs = Vec::new();
        let mut wall = 0.0;
        for p in parts {
            if let Some((k, v)) = p.split_once('=') {
                if k == "wall_ms" {
                    wall = v.parse().unwrap_or(-1.0);
                } else if k == "wall_only" {
                    kvs.push((k.to_string(), v.parse().unwrap_or(0)));
                } else if let Ok(n) = v.parse::<u64>() {
                    kvs.push((k.to_string(), n));
                }
            }
        }
        out.push((name, kvs, wall));
    }
    out
}

fn check_mode(baseline_path: &str, cases: &[Case]) -> i32 {
    let text = std::fs::read_to_string(baseline_path).unwrap_or_else(|e| {
        eprintln!("cannot read baseline {}: {}", baseline_path, e);
        std::process::exit(2);
    });
    let base = parse_baseline(&text);
    let mut fail = 0;
    for c in cases {
        let line = c.line();
        let parsed = parse_baseline(&line);
        let (_, kvs, wall) = &parsed[0];
        match base.iter().find(|(n, _, _)| n == c.name) {
            None => {
                println!("NEW-METRIC (update baseline deliberately): {}", c.name);
            }
            Some((_, bkvs, bwall)) => {
                // Counters: exact equality (except wall_ms/wall_only and
                // the percentile times *_us: those are measurements, not counters).
                for (k, v) in kvs {
                    if k == "wall_only" || k.ends_with("_us") {
                        continue;
                    }
                    match bkvs.iter().find(|(bk, _)| bk == k) {
                        Some((_, bv)) if bv == v => {}
                        Some((_, bv)) => {
                            println!("FAIL {}: {} baseline={} actual={}", c.name, k, bv, v);
                            fail += 1;
                        }
                        None => {
                            println!("NEW-COUNTER {}: {}={} (update baseline)", c.name, k, v);
                        }
                    }
                }
                // Lost coverage: a baseline key vanished from the run.
                for (bk, _) in bkvs {
                    if bk != "wall_only" && !kvs.iter().any(|(k, _)| k == bk) {
                        println!("FAIL {}: lost counter {}", c.name, bk);
                        fail += 1;
                    }
                }
                // Wall: only a WARN at >3x (machine noise, §49).
                if *bwall > 0.0 && *wall > bwall * 3.0 {
                    println!(
                        "WARN {}: wall {:.3}ms vs baseline {:.3}ms (>3x, informational)",
                        c.name, wall, bwall
                    );
                }
            }
        }
    }
    // Metrics from the baseline that are not in the run.
    // Foreign components (render.*) are skipped with a note, not a fail:
    // their own check verifies them (a single file, two harnesses).
    for (n, _, _) in &base {
        if n.starts_with("render.") {
            if !cases.iter().any(|c| c.name == n) {
                println!("SKIP-EXTERNAL (render harness): {}", n);
            }
            continue;
        }
        if !cases.iter().any(|c| c.name == n) {
            println!("FAIL: baseline metric missing from run: {}", n);
            fail += 1;
        }
    }
    if fail == 0 {
        println!("CHECK-OK: all counters match");
    } else {
        println!("CHECK-FAIL: {} mismatches", fail);
    }
    if fail > 0 {
        1
    } else {
        0
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("run");
    if mode == "content" {
        // Content cases only (rules compile, turn.core.rep, yields): the
        // BNW vs pre-BNW comparison, AURELIA_RULESET=legacy|classic.
        let mut cases = Vec::new();
        rules_compile_case(&mut cases);
        core_content_cases(&mut cases);
        combat_cases(&mut cases);
        if std::env::var("AURELIA_BENCH_MINORS").as_deref() != Ok("0") {
            minors_cases(&mut cases);
        }
        println!("# aurelia-bench v1 content ruleset={}", content_source().0);
        for c in &cases {
            println!("{}", c.line());
        }
        return;
    }
    // Does the harness always measure in release? No: the profile is written honestly into the header.
    let cases = run_suite();
    match mode {
        "check" => {
            let path = args
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("benches/results/baseline.txt");
            std::process::exit(check_mode(path, &cases));
        }
        _ => {
            println!("# aurelia-bench v1");
            println!(
                "# profile {}",
                if cfg!(debug_assertions) {
                    "debug"
                } else {
                    "release"
                }
            );
            for c in &cases {
                println!("{}", c.line());
            }
            println!();
            print_human(&cases);
            if let Some(out) = args
                .iter()
                .position(|a| a == "--out")
                .and_then(|i| args.get(i + 1))
            {
                let mut text = String::from("# aurelia-bench v1\n");
                for c in &cases {
                    text.push_str(&c.line());
                    text.push('\n');
                }
                std::fs::write(out, text).unwrap();
                eprintln!("wrote {}", out);
            }
        }
    }
}

/// Tourism on the representative BNW world: every player gets an
/// Amphitheater and a Museum in its first city and fills them (writer,
/// artists placed by the harness), then rounds run with influence of every
/// player on every player it met; and the tourism observation the AI and
/// the Culture overview read.
fn tourism_cases(out: &mut Vec<Case>, game: &GameState, rules: &classic_rules::CompiledRuleset) {
    let key = |k: &str| rules.units.iter().position(|u| u.key == k).map(|i| UnitKindId(i as u16));
    let building = |k: &str| {
        let i = rules.buildings.iter().position(|b| b.key == k);
        i.map(|i| classic_rules::ids::BuildingKindId(i as u16))
    };
    let (Some(writer), Some(artist)) = (key("core:unit/great-writer"), key("core:unit/great-artist")) else {
        return;
    };
    let slotted = [building("core:building/amphitheater"), building("core:building/museum")];
    let mut world = game.clone();
    let mut works = 0u64;
    let players = world.view().auth.players().count() as u8;
    for p in 0..players {
        let player = PlayerId(p);
        while world.view().auth.active_player() != player {
            let active = world.view().auth.active_player();
            let _ = world.execute(rules, classic_rules::Command::EndTurn { player: active });
        }
        let Some(city) = world.view().auth.indexes().cities_of(player).first().copied() else {
            continue;
        };
        for b in slotted.into_iter().flatten() {
            world.debug_add_building(rules, city, b);
        }
        let tile = world.view().auth.cities().get(city).unwrap().tile;
        for kind in [writer, artist, artist, artist] {
            let unit = world.debug_spawn_unit(rules, player, tile, kind);
            if world.execute(rules, classic_rules::Command::CreateGreatWork { unit }).is_ok() {
                works += 1;
            }
        }
    }
    for _ in 0..5 {
        core_round(&mut world, rules);
    }
    let mut round_ms = Vec::new();
    for _ in 0..20 {
        let t0 = Instant::now();
        core_round(&mut world, rules);
        round_ms.push(ms(t0.elapsed()));
    }
    out.push(Case {
        name: "turn.tourism.rep",
        counters: WorkCounters::default(),
        extra: vec![
            ("works", works),
            ("p50_us", percentile_us(&round_ms, 50)),
            ("p95_us", percentile_us(&round_ms, 95)),
        ],
        wall_ms: round_ms.iter().sum::<f64>() / round_ms.len() as f64,
        wall_only: true,
    });
    archaeology_case(out, &world, rules);
    let mut observe_ms = Vec::new();
    for _ in 0..21 {
        let t0 = Instant::now();
        black_box(classic_rules::derived::tourism::tourism_observation(world.view().auth, rules, PlayerId(0)));
        observe_ms.push(ms(t0.elapsed()));
    }
    out.push(Case {
        name: "tourism.observation.rep",
        counters: WorkCounters::default(),
        extra: vec![
            ("p50_us", percentile_us(&observe_ms, 50)),
            ("p95_us", percentile_us(&observe_ms, 95)),
        ],
        wall_ms: observe_ms.iter().sum::<f64>() / observe_ms.len() as f64,
        wall_only: true,
    });
}

/// The antiquity sites appear: the end turn of the first player to learn
/// Archaeology on the representative world (tile weights over the whole map,
/// then the heaviest tile until the ideal count, crowding around each).
fn archaeology_case(out: &mut Vec<Case>, world: &GameState, rules: &classic_rules::CompiledRuleset) {
    let Some(tech) = classic_rules::derived::archaeology::trigger_tech(rules) else {
        return;
    };
    let mut turn_ms = Vec::new();
    let mut sites = 0u64;
    for _ in 0..9 {
        let mut game = world.clone();
        let player = game.view().auth.active_player();
        game.debug_learn(rules, player, tech);
        let t0 = Instant::now();
        let _ = game.execute(rules, classic_rules::Command::EndTurn { player });
        turn_ms.push(ms(t0.elapsed()));
        sites = game.view().auth.module().tourism().archaeology().sites.len() as u64;
    }
    out.push(Case {
        name: "archaeology.spawn.rep",
        counters: WorkCounters::default(),
        extra: vec![
            ("sites", sites),
            ("p50_us", percentile_us(&turn_ms, 50)),
            ("p95_us", percentile_us(&turn_ms, 95)),
        ],
        wall_ms: turn_ms.iter().sum::<f64>() / turn_ms.len() as f64,
        wall_only: true,
    });
}

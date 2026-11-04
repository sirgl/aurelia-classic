use crate::ids::*;
use crate::GameState;
use crate::CompiledRuleset;
use crate::persistence::decode::*;
use crate::replay::authoritative_hash;
use crate::rules::SourceRules;
use crate::scenario::ScenarioConfig;
use crate::{compile_rules, Command};

fn lively_world() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(SourceRules::default()).unwrap();
    let cfg = ScenarioConfig {
        seed: 21,
        players: 2,
        width: 16,
        height: 16,
        cities: 3,
        units: 6,
        water_ratio: 0,
    };
    let mut game = crate::scenario::generate(&cfg, &rules);
    // Moving; founding via the settler-consume path is checked elsewhere;
    // here: a couple of moves + a building, to cover the journal with variety.
    let units = game.view().auth.indexes().units_of(PlayerId(0)).to_vec();
    if let Some(u) = units.first() {
        let _ = game.execute(
            &rules,
            Command::MoveUnit {
                unit: *u,
                destination: TileId(40),
            },
        );
    }
    let _ = game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    );
    (game, rules)
}

#[test]
fn save_load_roundtrip_preserves_truth() {
    // DoD §54: save -> load = same authoritative state (by hash + validate).
    let (game, rules) = lively_world();
    let save = crate::persistence::encode::encode(&game, &rules);
    let loaded = decode(&save, &rules).unwrap();
    assert_eq!(authoritative_hash(&game), authoritative_hash(&loaded));
    loaded.validate(&rules).unwrap();
    // Derived converged too (restore builds by the rebuild path).
    assert_eq!(game.derived().yields, loaded.derived().yields);
    assert_eq!(
        game.derived().visibility.counts,
        loaded.derived().visibility.counts
    );
}

#[test]
fn save_load_preserves_city_state() {
    // The name, food and queue are part of the auth truth (§24): they survive save/load
    // pixel-exactly, not only via the hash.
    use crate::ids::{ProductionItem, UnitKindId};
    use crate::math::HexCoord;
    use crate::world::state::GameConfig;
    // A fixture from RON: settler(0)/warrior(1) as in the game.
    let rules = crate::fixtures::arena_content();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 5,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), t0);
    game.execute(
        &rules,
        Command::EnqueueProduction {
            player: PlayerId(0),
            city,
            item: ProductionItem::Unit(UnitKindId(1)),
        },
    )
    .unwrap();
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    // A manual tile is part of the truth too: we took a neighbour off before the save.
    let off = dims.to_tile(HexCoord { q: 9, r: 8 }).unwrap();
    game.execute(
        &rules,
        Command::ToggleWorkedTile {
            player: PlayerId(0),
            city,
            tile: off,
        },
    )
    .unwrap();
    let save = crate::persistence::encode::encode(&game, &rules);
    let loaded = decode(&save, &rules).unwrap();
    let a = game.view().auth.cities().get(city).unwrap();
    let (h2, b) = loaded.view().auth.cities().iter().next().unwrap();
    let _ = h2;
    assert_eq!(a.name, b.name);
    assert_eq!(a.population, b.population);
    assert_eq!(a.food_times100, b.food_times100);
    assert_eq!(a.economy, b.economy);
    assert_eq!(a.citizens, b.citizens);
    assert_eq!(a.production, b.production);
    assert_eq!(a.queue, b.queue);
    assert_eq!(a.unworked, b.unworked);
    assert_eq!(a.culture, b.culture);
    assert_eq!(a.acquired, b.acquired);
    assert_eq!(a.radius, b.radius);
    assert_eq!(a.damage, b.damage);
    assert_eq!(authoritative_hash(&game), authoritative_hash(&loaded));
}

/// A save written before the fingerprint named the rules module has the
/// same world bytes: the snapshot loader takes it, strict replay does not.
#[test]
fn a_save_before_the_module_id_loads_as_a_snapshot_only() {
    let (game, rules) = lively_world();
    let mut save = crate::persistence::encode::encode(&game, &rules);
    assert_ne!(rules.hash(), rules.hash_before_module_id());
    save.header.ruleset_hash = rules.hash_before_module_id();
    assert_eq!(decode(&save, &rules).unwrap_err(), DecodeError::RulesetMismatch);
    let loaded = decode_snapshot(&save, &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game));
    let resaved = crate::persistence::encode::encode(&loaded, &rules);
    assert_eq!(resaved.header.ruleset_hash, rules.hash());
}

#[test]
fn decode_rejects_garbage() {
    let (game, rules) = lively_world();
    let mut save = crate::persistence::encode::encode(&game, &rules);
    // A foreign ruleset.
    let other = compile_rules(SourceRules::default()).unwrap();
    let mut save2 = save.clone();
    save2.header.ruleset_hash ^= 0xdead;
    assert_eq!(
        decode(&save2, &other).unwrap_err(),
        DecodeError::RulesetMismatch
    );
    // A broken unit tile.
    save.world.units[0].tile = 999_999;
    assert!(matches!(
        decode(&save, &rules),
        Err(DecodeError::Corrupt(_))
    ));
    // A broken phase discriminant.
    let (game, rules) = lively_world();
    let mut save = crate::persistence::encode::encode(&game, &rules);
    save.world.turn_phase = 9;
    assert!(matches!(
        decode(&save, &rules),
        Err(DecodeError::Corrupt(_))
    ));
    // A foreign sim version.
    let mut save = crate::persistence::encode::encode(&game, &rules);
    save.header.simulation_version += 1;
    assert!(matches!(
        decode(&save, &rules),
        Err(DecodeError::SimMismatch(_))
    ));
}

#[test]
fn save_load_preserves_research_state() {
    // Beakers, queue and learned techs are part of the truth (§24): they survive the cycle.
    // A fixture from RON (agri=TechId(0) as in the game).
    use crate::math::HexCoord;
    use crate::world::state::GameConfig;
    let rules = crate::fixtures::arena_content();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: 16,
            height: 16,
            rng_seed: 9,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let t0 = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    game.debug_found_city(&rules, PlayerId(0), t0);
    game.execute(
        &rules,
        Command::ChooseTechnology {
            player: PlayerId(0),
            technology: crate::ids::TechId(0),
        },
    )
    .unwrap();
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    let save = crate::persistence::encode::encode(&game, &rules);
    let loaded = decode(&save, &rules).unwrap();
    let (a, b) = (game.view().auth.players(), loaded.view().auth.players());
    assert_eq!(a.science_of(PlayerId(0)), b.science_of(PlayerId(0)));
    assert!(a.science_of(PlayerId(0)) > 0);
    assert_eq!(
        a.research_queue_of(PlayerId(0)),
        b.research_queue_of(PlayerId(0))
    );
    assert_eq!(a.research_queue_of(PlayerId(0)), [crate::ids::TechId(0)]);
    assert_eq!(authoritative_hash(&game), authoritative_hash(&loaded));
}

#[test]
fn river_mouth_on_water_survives_save_load() {
    // Regression: decode marked both ends of every river edge as river,
    // the live map only the land end — a river mouth turned the water
    // tile into a river after load (hash and fording changed).
    let rules = compile_rules(SourceRules::default()).unwrap();
    let config = crate::world::state::GameConfig {
        players: 2,
        width: 8,
        height: 8,
        rng_seed: 5,
    };
    let mut game = GameState::new(&rules, config);
    let (land, water) = (TileId(9), TileId(10));
    let water_terrain = crate::fixtures::WATER;
    game.debug_set_terrain_batch(&rules, &[(water, water_terrain)]);
    game.debug_set_river_edges(&rules, &[(land, water)]);
    assert!(game.view().auth.map().river(land));
    assert!(!game.view().auth.map().river(water));
    let save = crate::persistence::encode::encode(&game, &rules);
    let loaded = decode(&save, &rules).unwrap();
    assert!(!loaded.view().auth.map().river(water), "water stays water");
    assert_eq!(authoritative_hash(&game), authoritative_hash(&loaded));
}

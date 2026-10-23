//! Ships on the classic ruleset (classic/docs/classic/rules.md): triremes keep to the coast, caravels cross the
//! ocean; naval melee units take coastal cities; ships are built only in
//! coastal cities and pillage water improvements; carriers carry fighters
//! up to their cargo space, move them along and sink with them.

use classic_content::core::core_rules;
use classic_rules::ids::{ImprovementId, PlayerId, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::{HexCoord, MapDimensions};
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::replay::authoritative_hash;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const WATER: TerrainId = TerrainId(4);
const DIMS: MapDimensions = MapDimensions { width: 24, height: 14 };

fn setup() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let game = GameState::new(
        &rules,
        GameConfig {
            players: 2,
            width: DIMS.width,
            height: DIMS.height,
            rng_seed: 5,
        },
    );
    (game, rules)
}

fn tile(q: i32, r: i32) -> TileId {
    DIMS.to_tile(HexCoord { q, r }).unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

/// Water on the columns `from..to`, every row.
fn sea(game: &mut GameState, rules: &CompiledRuleset, from: i32, to: i32) {
    let batch: Vec<(TileId, TerrainId)> = (0..DIMS.height as i32)
        .flat_map(|r| (from..to).map(move |q| (tile(q, r), WATER)))
        .collect();
    game.debug_set_terrain_batch(rules, &batch);
}

fn moved(game: &mut GameState, rules: &CompiledRuleset, unit: UnitHandle, to: TileId) -> bool {
    game.execute(rules, Command::MoveUnit { unit, destination: to }).is_ok()
}

fn war(game: &mut GameState, rules: &CompiledRuleset) {
    game.debug_meet(P0, P1);
    game.execute(rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
}

/// Triremes cannot enter the ocean (water with no land next to it);
/// caravels can; the work boat waits for Astronomy.
#[test]
fn triremes_keep_to_the_coast_and_caravels_cross_the_ocean() {
    let (mut game, rules) = setup();
    sea(&mut game, &rules, 6, 24);
    let coast = tile(6, 5);
    let ocean = tile(8, 5);
    let trireme = game.debug_spawn_unit(&rules, P0, coast, unit(&rules, "core:unit/trireme"));
    assert!(!moved(&mut game, &rules, trireme, ocean), "a trireme stays on the coast");
    let caravel = game.debug_spawn_unit(&rules, P0, tile(6, 8), unit(&rules, "core:unit/caravel"));
    assert!(moved(&mut game, &rules, caravel, tile(8, 8)), "a caravel sails the ocean");
    let boat = game.debug_spawn_unit(&rules, P0, tile(6, 11), unit(&rules, "core:unit/work-boat"));
    assert!(!moved(&mut game, &rules, boat, tile(7, 11)), "the ocean needs Astronomy");
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/astronomy"));
    assert!(moved(&mut game, &rules, boat, tile(7, 11)));
    game.validate(&rules).unwrap();
}

/// A naval melee unit takes a coastal city at 1 HP and sails into it.
#[test]
fn a_trireme_captures_a_coastal_city() {
    let (mut game, rules) = setup();
    sea(&mut game, &rules, 8, 24);
    game.debug_found_city(&rules, P1, tile(3, 10));
    let city = game.debug_found_city(&rules, P1, tile(7, 5));
    game.debug_found_city(&rules, P0, tile(2, 2));
    game.debug_set_city_damage(city, 199);
    let trireme = game.debug_spawn_unit(&rules, P0, tile(8, 5), unit(&rules, "core:unit/trireme"));
    war(&mut game, &rules);
    game.execute(&rules, Command::AttackCity { attacker: trireme, city }).unwrap();
    let auth = game.view().auth;
    assert_eq!(auth.cities().get(city).map(|c| c.owner), Some(P0), "the city is taken");
    assert_eq!(auth.units().get(trireme).unwrap().core.tile, tile(7, 5), "the ship sails in");
    game.validate(&rules).unwrap();
}

/// Ships are built in coastal cities only (a lake is not the sea).
#[test]
fn ships_are_built_only_in_coastal_cities() {
    let (mut game, rules) = setup();
    sea(&mut game, &rules, 10, 24);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/sailing"));
    let inland = game.debug_found_city(&rules, P0, tile(3, 5));
    let port = game.debug_found_city(&rules, P0, tile(9, 8));
    let item = classic_rules::ids::ProductionItem::Unit(unit(&rules, "core:unit/trireme"));
    let refused = game.execute(&rules, Command::EnqueueProduction { player: P0, city: inland, item });
    assert!(format!("{refused:?}").contains("not a coastal city"), "{refused:?}");
    game.execute(&rules, Command::EnqueueProduction { player: P0, city: port, item }).unwrap();
}

/// A ship pillages an enemy's fishing boats; an embarked unit cannot.
#[test]
fn ships_pillage_water_improvements() {
    let (mut game, rules) = setup();
    sea(&mut game, &rules, 8, 24);
    let boats = ImprovementId(rules.improvements.iter().position(|i| i.key == "core:improvement/fishing-boats").unwrap() as u16);
    let spot = tile(8, 5);
    game.debug_found_city(&rules, P1, tile(7, 5));
    game.debug_set_improvement_batch(&rules, &[(spot, boats)]);
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/optics"));
    let warrior = game.debug_spawn_unit(&rules, P0, spot, unit(&rules, "core:unit/warrior"));
    war(&mut game, &rules);
    assert!(game.execute(&rules, Command::Pillage { unit: warrior }).is_err(), "embarked units pillage nothing");
    let (mut game, rules) = setup();
    sea(&mut game, &rules, 8, 24);
    game.debug_found_city(&rules, P1, tile(7, 5));
    game.debug_set_improvement_batch(&rules, &[(spot, boats)]);
    let trireme = game.debug_spawn_unit(&rules, P0, spot, unit(&rules, "core:unit/trireme"));
    war(&mut game, &rules);
    game.execute(&rules, Command::Pillage { unit: trireme }).unwrap();
    game.validate(&rules).unwrap();
}

/// A carrier takes two fighters (Cargo II), not three; they sail with it,
/// survive a save and load, and sink with it.
#[test]
fn carriers_carry_fighters_move_them_and_sink_with_them() {
    let (mut game, rules) = setup();
    sea(&mut game, &rules, 8, 24);
    game.debug_found_city(&rules, P0, tile(6, 6));
    let carrier = game.debug_spawn_unit(&rules, P0, tile(10, 6), unit(&rules, "core:unit/carrier"));
    let fighters: Vec<UnitHandle> = (0..3)
        .map(|_| game.debug_spawn_unit(&rules, P0, tile(6, 6), unit(&rules, "core:unit/fighter")))
        .collect();
    for fighter in &fighters[..2] {
        game.execute(&rules, Command::RebaseToCarrier { unit: *fighter, carrier }).unwrap();
    }
    let full = game.execute(&rules, Command::RebaseToCarrier { unit: fighters[2], carrier });
    assert!(format!("{full:?}").contains("no room"), "{full:?}");
    assert!(moved(&mut game, &rules, carrier, tile(12, 6)));
    for fighter in &fighters[..2] {
        assert_eq!(game.view().auth.units().get(*fighter).unwrap().core.tile, tile(12, 6), "the fighters sail along");
    }
    game.validate(&rules).unwrap();
    let saved = encode(&game, &rules);
    let loaded = decode(&saved, &rules).unwrap();
    assert_eq!(authoritative_hash(&loaded), authoritative_hash(&game), "carried units survive a save");
    // A battleship of P1 sinks the carrier: its fighters go down with it.
    let battleship = game.debug_spawn_unit(&rules, P1, tile(13, 6), unit(&rules, "core:unit/battleship"));
    game.debug_set_unit_hp(carrier, 1);
    war(&mut game, &rules);
    while game.view().auth.active_player() != P1 {
        let active = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player: active }).unwrap();
    }
    game.execute(&rules, Command::RangedAttack { unit: battleship, target: tile(12, 6) }).unwrap();
    let auth = game.view().auth;
    assert!(auth.units().get(carrier).is_none(), "the carrier sinks");
    assert!(fighters[..2].iter().all(|f| auth.units().get(*f).is_none()), "its fighters with it");
    assert!(auth.units().get(fighters[2]).is_some(), "the one in the city stays");
    game.validate(&rules).unwrap();
}

/// A ship with Withdraw Before Melee (the caravel) steps back over water,
/// never onto land, when a melee ship attacks it.
#[test]
fn a_ship_withdraws_over_water() {
    let mut withdrawn = 0;
    for seed in 1..=40u64 {
        let rules = compile_rules(core_rules()).unwrap();
        let mut game = GameState::new(&rules, GameConfig { players: 2, width: DIMS.width, height: DIMS.height, rng_seed: seed });
        sea(&mut game, &rules, 8, 24);
        let caravel = game.debug_spawn_unit(&rules, P1, tile(12, 6), unit(&rules, "core:unit/caravel"));
        let trireme = game.debug_spawn_unit(&rules, P0, tile(11, 6), unit(&rules, "core:unit/trireme"));
        war(&mut game, &rules);
        let _ = game.execute(&rules, Command::AttackUnit { attacker: trireme, defender: caravel });
        let auth = game.view().auth;
        if let Some(view) = auth.units().get(caravel) {
            if view.core.tile != tile(12, 6) {
                withdrawn += 1;
                assert!(auth.map().is_water(view.core.tile), "the ship stays on water");
            }
        }
        game.validate(&rules).unwrap();
    }
    assert!(withdrawn > 0, "some caravel withdrew");
}

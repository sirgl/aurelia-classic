//! The oracle of the test harness (`kernel/src/world/harness.rs`): after EVERY
//! helper that changes a running game outside `execute`, derived state equals a
//! rebuild from the authoritative world (`validate`) and the world equals the one
//! loaded from its own save bytes. A helper that forgot its journal fails here.

use crate::fixtures::{compile_ron, ARENA_RON};
use crate::ids::*;
use crate::math::HexCoord;
use crate::persistence::decode::decode;
use crate::persistence::encode::encode;
use crate::replay::authoritative_hash;
use crate::world::city::CityStatus;
use crate::world::map::Route;
use crate::world::state::GameConfig;
use crate::{CompiledRuleset, GameState};

fn check(step: &str, game: &GameState, rules: &CompiledRuleset) {
    game.validate(rules).unwrap_or_else(|e| panic!("{step}: derived state differs from a rebuild: {e}"));
    let loaded = decode(&encode(game, rules), rules).unwrap_or_else(|e| panic!("{step}: the save does not load: {e:?}"));
    assert_eq!(authoritative_hash(game), authoritative_hash(&loaded), "{step}: the world differs from its own save");
    loaded.validate(rules).unwrap_or_else(|e| panic!("{step}: loaded world: {e}"));
    assert_eq!(game.derived().yields, loaded.derived().yields, "{step}: yields differ after a load");
}

#[test]
fn every_harness_helper_leaves_derived_state_and_save_consistent() {
    // The arena (forest, farm, mine, wheat, gold, three techs) plus one building.
    let building = "buildings: [(id: \"core:building/granary\", cost: 40, effects: [(scope: City, selector: SourceCity, modifier: AddYield(yield_kind: Output, amount: 2))])],\n    units: [";
    let rules = compile_ron(&ARENA_RON.replacen("units: [", building, 1));
    let mut game = GameState::new(&rules, GameConfig { players: 3, width: 16, height: 16, rng_seed: 7 });
    let tile = |game: &GameState, q, r| game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap();
    let p0 = PlayerId(0);
    let p1 = PlayerId(1);
    let (a, b, c, d) = (tile(&game, 4, 4), tile(&game, 5, 4), tile(&game, 4, 5), tile(&game, 5, 5));
    let far = tile(&game, 11, 11);

    game.debug_set_terrain_batch(&rules, &[(a, TerrainId(1)), (b, TerrainId(1)), (c, TerrainId(2)), (d, TerrainId(3)), (far, TerrainId(1))]);
    check("set_terrain_batch", &game, &rules);
    game.debug_set_feature_batch(&rules, &[(b, FeatureId(0))]);
    check("set_feature_batch", &game, &rules);
    game.debug_clear_features(&rules, &[b]);
    check("clear_features", &game, &rules);
    game.debug_set_river_edges(&rules, &[(a, b)]);
    check("set_river_edges", &game, &rules);
    game.debug_set_resource_batch(&rules, &[(c, ResourceId(0))]);
    check("set_resource_batch", &game, &rules);
    game.debug_set_deposit(&rules, d, ResourceId(0), 3);
    check("set_deposit", &game, &rules);
    game.debug_set_improvement_batch(&rules, &[(a, ImprovementId(0))]);
    check("set_improvement_batch", &game, &rules);
    game.debug_set_road_batch(&rules, &[a, b]);
    check("set_road_batch", &game, &rules);
    game.debug_set_route_batch(&rules, &[(c, Route::Road), (d, Route::Road)]);
    check("set_route_batch", &game, &rules);

    game.debug_set_player_civ(p0, 0);
    game.debug_sources_changed(&rules, p0);
    check("set_player_civ", &game, &rules);
    let city = game.debug_found_city(&rules, p0, a);
    check("found_city", &game, &rules);
    let other = game.debug_found_city(&rules, p1, far);
    check("found_city (second player)", &game, &rules);
    let unit = game.debug_spawn_unit(&rules, p0, b, UnitKindId(1));
    check("spawn_unit", &game, &rules);
    let _foe = game.debug_spawn_unit(&rules, p1, tile(&game, 10, 11), UnitKindId(1));
    check("spawn_unit (second player)", &game, &rules);

    game.debug_meet(p0, p1);
    check("meet", &game, &rules);
    game.debug_meet_all();
    check("meet_all", &game, &rules);
    game.debug_reveal(p0, &[c, d, far]);
    check("reveal", &game, &rules);
    game.debug_learn(&rules, p0, TechId(0));
    check("learn", &game, &rules);
    game.debug_learn_tech(&rules, p0, TechId(1));
    check("learn_tech", &game, &rules);
    game.debug_add_building(&rules, city, BuildingKindId(0));
    check("add_building", &game, &rules);
    game.debug_set_population(&rules, city, 4);
    check("set_population", &game, &rules);
    game.debug_grow_city(&rules, city, 2);
    check("grow_city", &game, &rules);
    game.debug_earn(p0, 120);
    check("earn", &game, &rules);
    game.debug_set_unit_hp(unit, 3);
    check("set_unit_hp", &game, &rules);
    game.debug_set_experience(unit, 15);
    check("set_experience", &game, &rules);
    game.debug_set_city_damage(city, 5);
    check("set_city_damage", &game, &rules);
    game.debug_set_city_name(other, "Elsewhere".to_string());
    check("set_city_name", &game, &rules);
    game.debug_set_city_status(other, CityStatus::Own);
    check("set_city_status", &game, &rules);

    // River flags WITHOUT beds are not saved (a save keeps the edges and the flags follow
    // from them), so this helper is checked against a rebuild only, and last.
    game.debug_set_river_batch(&rules, &[c]);
    game.validate(&rules).unwrap_or_else(|e| panic!("set_river_batch: derived state differs from a rebuild: {e}"));
}

//! Per-count modifiers on the classic ruleset (`Modifier::AddHappinessPerCount`,
//! `AddYieldPerCount`): Aristocracy's +1 happiness per 10 citizens of a
//! city, the Prora's +1 happiness per 2 policies, Divine Inspiration's +2
//! faith per world wonder of a following city.

use classic_rules::ClassicGame;
use classic_rules::classic::effects::ClassicYield;
use classic_content::core::core_rules;
use classic_rules::derived::happiness::{city_local_happiness, player_happiness};
use classic_rules::effects::city::city_modifiers;
use classic_rules::ids::{BeliefId, BuildingKindId, CityHandle, PlayerId, PolicyId, TileId, UnitKindId};
use classic_rules::rules::content::ReligionId;
use classic_rules::Command;
use classic_rules::rules::effects::YieldKind;
use classic_rules::{compile_rules, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn world(rules: &CompiledRuleset) -> GameState {
    GameState::new(rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 5 })
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(classic_rules::math::HexCoord { q, r }).unwrap()
}

fn policy(rules: &CompiledRuleset, key: &str) -> PolicyId {
    PolicyId(rules.module.policies.iter().position(|p| p.key == key).unwrap() as u16)
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap() as u16)
}

fn city(game: &mut GameState, rules: &CompiledRuleset, q: i32, r: i32) -> CityHandle {
    let t = tile(game, q, r);
    game.debug_found_city(rules, P0, t)
}

#[test]
fn aristocracy_gives_a_city_one_happiness_per_ten_citizens() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, 5, 5);
    game.debug_set_population(&rules, c, 21);
    let local = |game: &GameState| {
        let auth = game.view().auth;
        city_local_happiness(auth, &rules, auth.cities().get(c).unwrap()).0
    };
    let before = local(&game);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/tradition/aristocracy"));
    assert_eq!(local(&game) - before, 2, "21 citizens: two steps of 10");
    game.validate(&rules).unwrap();
}

#[test]
fn prora_gives_one_happiness_per_two_policies() {
    let rules = rules();
    let mut game = world(&rules);
    let c = city(&mut game, &rules, 5, 5);
    let happiness = |game: &GameState| player_happiness(game.view().auth, &rules, P0).buildings;
    for key in ["core:policy/tradition/opener", "core:policy/tradition/aristocracy", "core:policy/tradition/oligarchy"] {
        game.debug_adopt_policy(&rules, P0, policy(&rules, key));
    }
    let before = happiness(&game);
    game.debug_complete_building(&rules, c, building(&rules, "core:building/prora"));
    // The Prora's own +2, and 3 policies: one step of 2.
    assert_eq!(happiness(&game) - before, 2 + 1);
    game.debug_adopt_policy(&rules, P0, policy(&rules, "core:policy/tradition/legalism"));
    assert_eq!(happiness(&game) - before, 2 + 2, "4 policies: two steps");
    game.validate(&rules).unwrap();
}

#[test]
fn divine_inspiration_gives_two_faith_per_world_wonder_of_a_following_city() {
    let rules = rules();
    let mut game = world(&rules);
    let holy = city(&mut game, &rules, 5, 5);
    let at = game.view().auth.cities().get(holy).unwrap().tile;
    let prophet = rules.units.iter().position(|u| u.key == "core:unit/great-prophet").unwrap();
    let prophet = game.debug_spawn_unit(&rules, P0, at, UnitKindId(prophet as u16));
    let belief = |key: &str| BeliefId(rules.module.beliefs.iter().position(|b| b.key == key).unwrap() as u16);
    game.execute(
        &rules,
        Command::FoundReligion {
            unit: prophet,
            religion: ReligionId(0),
            founder: belief("core:belief/founder/tithe"),
            follower: belief("core:belief/follower/divine-inspiration"),
            pantheon: Some(belief("core:belief/pantheon/ancestor-worship")),
            bonus: None,
        },
    )
    .unwrap();
    let faith = |game: &GameState| {
        let auth = game.view().auth;
        city_modifiers(auth, &rules, auth.cities().get(holy).unwrap()).yields[YieldKind::Module(ClassicYield::Faith).index()].flat
    };
    let before = faith(&game);
    game.debug_add_building(&rules, holy, building(&rules, "core:building/pyramids"));
    game.debug_add_building(&rules, holy, building(&rules, "core:building/great_library"));
    assert_eq!(faith(&game) - before, 4, "two wonders, +2 each");
    game.validate(&rules).unwrap();
}

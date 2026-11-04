//! The stops of its own the AI kept on top of the rules, priced instead
//! (`planner_cities::PricedStops`, `Variant::NoOwnStops`, classic/docs/ai-own-stops.md):
//! a building with upkeep while the treasury carries the deficit, a reserve of
//! the deficit only, missionaries to any city they reach and bought only for
//! one. The buildings are picked from the rules' data, never by name.

use classic_content::core::core_rules;
use classic_rules::ai::allocator::probe;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::planner::TurnBrief;
use classic_rules::ai::planner_cities::{emergency_reserve, PricedStops, DEFICIT_RESERVE_TURNS};
use classic_rules::ai::planner_value::building_gold;
use classic_rules::ai::religion::religion_commands_with;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::derived::religion::city_majority;
use classic_rules::ids::{BeliefId, BuildingKindId, CityHandle, PlayerId, ProductionItem, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::ReligionId;
use classic_rules::world::religion::Creed;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::mind::AiMind;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn unit_kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

fn belief(rules: &CompiledRuleset, key: &str) -> BeliefId {
    BeliefId(rules.module.beliefs.iter().position(|b| b.key == key).unwrap() as u16)
}

/// A plain building with upkeep that earns no gold: no wonder, no other
/// building, resource, policy or nation needed; the cheapest of them.
fn building_with_upkeep(rules: &CompiledRuleset) -> BuildingKindId {
    let index = (0..rules.buildings.len())
        .filter(|&i| {
            let def = &rules.buildings[i];
            def.maintenance > 0
                && def.cost > 0
                && !def.wonder
                && !def.unbuildable
                && def.kind == classic_rules::rules::content::BuildingKind::Regular
                && def.requires_buildings.is_empty()
                && def.consumes_resources.is_empty()
                && def.module.requires_policy_branch.is_none()
                && def.module.faith_cost.is_none()
                && def.nation.is_none()
                && building_gold(rules, BuildingKindId(i as u16)) == 0
        })
        .min_by_key(|&i| (rules.buildings[i].cost, i))
        .expect("a building with upkeep");
    BuildingKindId(index as u16)
}

/// One city of P0 grown to 4 that knows the technology of `building`, a
/// gold deficit from an army and `treasury` gold.
fn deficit_scene(rules: &CompiledRuleset, building: BuildingKindId, treasury: u32) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 3 });
    let city = game.debug_found_city(rules, P0, tile(&game, 6, 6));
    game.debug_found_city(rules, P1, tile(&game, 24, 14));
    game.debug_grow_city(rules, city, 3);
    if let Some(tech) = rules.buildings[building.0 as usize].required_tech {
        game.debug_learn(rules, P0, tech);
    }
    let warrior = unit_kind(rules, "core:unit/warrior");
    for _ in 0..12 {
        game.debug_spawn_unit(rules, P0, tile(&game, 7, 7), warrior);
    }
    game.debug_earn(P0, treasury);
    game
}

/// The orders the allocator of `variant` considered for P0's cities at the
/// end of its turn.
fn considered(rules: &CompiledRuleset, game: &GameState, variant: Variant) -> Vec<ProductionItem> {
    let mind = ClassicMind::new(variant);
    let sight = mind.sense(game, rules, P0);
    let mut state = classic_rules::ai::AiState::new(P0, game.view().auth.turn().round);
    let started = mind.start_turn(&sight, rules, P0, &mut state);
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let mut scratch = Default::default();
    let mut counters = Default::default();
    let mut tools = kernel::ai::plan::PlanTools { rules, settings: &settings, scratch: &mut scratch, counters: &mut counters };
    probe::enable(true);
    mind.plan_end_of_turn(&sight, P0, &started.brief, &state, &mut tools);
    let choices = probe::take();
    probe::enable(false);
    choices.into_iter().flat_map(|c| c.options.into_iter().map(|o| o.item)).collect()
}

/// At a gold deficit the game's AI considers no building with upkeep, however
/// full its treasury; with the upkeep priced it does while the treasury carries
/// the deficit for `GOLD_FOCUS_TURNS`, and not on an empty treasury (the rules
/// disband units below zero).
#[test]
fn a_building_with_upkeep_is_considered_while_the_treasury_carries_the_deficit() {
    let rules = compile_rules(core_rules()).unwrap();
    let building = building_with_upkeep(&rules);
    let item = ProductionItem::Building(building);
    let full = deficit_scene(&rules, building, 5_000);
    let obs = full.ai_observation(&rules, P0);
    let maintenance = rules.buildings[building.0 as usize].maintenance as i32;
    assert!(obs.economy.income.gold < maintenance, "{:?}", obs.economy.income);
    assert!(!considered(&rules, &full, Variant::Standard).contains(&item));
    assert!(considered(&rules, &full, Variant::NoOwnStopsUpkeep).contains(&item));
    assert!(considered(&rules, &full, Variant::NoOwnStops).contains(&item));
    let empty = deficit_scene(&rules, building, 0);
    assert!(!considered(&rules, &empty, Variant::NoOwnStopsUpkeep).contains(&item));
}

/// The reserve priced is the deficit for `DEFICIT_RESERVE_TURNS` and nothing
/// at a positive income; the game's AI keeps a sum a city on top.
#[test]
fn the_priced_reserve_is_the_deficit_only() {
    let rules = compile_rules(core_rules()).unwrap();
    let building = building_with_upkeep(&rules);
    let losing = deficit_scene(&rules, building, 500);
    let obs = losing.ai_observation(&rules, P0);
    let mut brief = TurnBrief::from_observation(&obs, &rules);
    let deficit = (-obs.economy.income.gold) as u32;
    assert!(deficit > 0);
    let standard = emergency_reserve(&obs, &brief);
    brief.priced_stops = PricedStops { reserve: true, ..PricedStops::NONE };
    assert_eq!(emergency_reserve(&obs, &brief), deficit * DEFICIT_RESERVE_TURNS);
    assert!(standard > deficit * DEFICIT_RESERVE_TURNS);

    let mut earning = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 3 });
    earning.debug_found_city(&rules, P0, tile(&earning, 6, 6));
    earning.debug_earn(P0, 500);
    let obs = earning.ai_observation(&rules, P0);
    assert!(obs.economy.income.gold >= 0);
    let mut brief = TurnBrief::from_observation(&obs, &rules);
    assert!(emergency_reserve(&obs, &brief) > 0);
    brief.priced_stops = PricedStops::ALL;
    assert_eq!(emergency_reserve(&obs, &brief), 0);
}

/// P0 founded a religion in its capital; P1's city, seen by a scout of P0
/// next to it, follows nothing and grants P0 no entry.
fn mission_scene(rules: &CompiledRuleset) -> (GameState, CityHandle) {
    let mut game = GameState::new(rules, GameConfig { players: 2, width: 40, height: 24, rng_seed: 11 });
    let holy = game.debug_found_city(rules, P0, tile(&game, 5, 5));
    let foreign = game.debug_found_city(rules, P1, tile(&game, 9, 5));
    let at = game.view().auth.cities().get(holy).unwrap().tile;
    let prophet = game.debug_spawn_unit(rules, P0, at, unit_kind(rules, "core:unit/great-prophet"));
    game.execute(
        rules,
        Command::FoundReligion {
            unit: prophet,
            religion: ReligionId(0),
            founder: belief(rules, "core:belief/founder/tithe"),
            follower: belief(rules, "core:belief/follower/pagodas"),
            pantheon: Some(belief(rules, "core:belief/pantheon/ancestor-worship")),
            bonus: None,
        },
    )
    .unwrap();
    game.debug_meet(P0, P1);
    game.debug_spawn_unit(rules, P0, tile(&game, 10, 4), unit_kind(rules, "core:unit/scout"));
    (game, foreign)
}

/// The game's AI keeps its missionary off P1's land without open borders,
/// though the rules let a missionary pass closed borders; with the stop priced
/// it walks next to P1's city, the rules accept every order, and the city
/// comes to follow P0's religion.
#[test]
fn a_missionary_goes_to_a_foreign_city_the_rules_let_it_reach() {
    let rules = compile_rules(core_rules()).unwrap();
    let (mut game, foreign) = mission_scene(&rules);
    let holy = game.view().auth.cities().iter().find(|(_, c)| c.owner == P0).map(|(h, _)| h).unwrap();
    game.debug_grant_faith(P0, 400);
    let missionary = ProductionItem::Unit(unit_kind(&rules, "core:unit/missionary"));
    game.execute(&rules, Command::PurchaseWithFaith { player: P0, city: holy, item: missionary }).unwrap();
    let obs = game.ai_observation(&rules, P0);
    assert!(obs.faith.cities.iter().any(|c| c.owner == P1), "P1's city is seen");
    assert!(!obs.relations.iter().any(|r| r.other == P1 && r.they_grant_entry));
    let unit = obs.faith.units[0].0;
    let ordered = |commands: &[Command]| commands.iter().any(|c| matches!(c, Command::MarchTo { unit: u, .. } | Command::SpreadReligion { unit: u } if *u == unit));
    assert!(!ordered(&religion_commands_with(&obs, &rules, false, None)), "the game's AI keeps it home");

    let religion = Some(Creed::Religion(ReligionId(0)));
    let mut spread = false;
    for _ in 0..12 {
        let obs = game.ai_observation(&rules, P0);
        let mine: Vec<Command> = religion_commands_with(&obs, &rules, true, None)
            .into_iter()
            .filter(|c| matches!(c, Command::MarchTo { unit: u, .. } | Command::SpreadReligion { unit: u } if *u == unit))
            .collect();
        for command in mine {
            spread |= matches!(command, Command::SpreadReligion { .. });
            game.execute(&rules, command.clone()).unwrap_or_else(|e| panic!("{command:?}: {e:?}"));
        }
        if spread {
            break;
        }
        for _ in 0..2 {
            let player = game.view().auth.active_player();
            game.execute(&rules, Command::EndTurn { player }).unwrap();
        }
    }
    assert!(spread, "the missionary spread its religion next to P1's city");
    assert_eq!(city_majority(game.view().auth, foreign), religion);
}

/// With the stop priced a missionary is bought only while a city it could
/// spread to is seen; the game's AI buys one anyway.
#[test]
fn a_missionary_is_bought_only_for_a_city_to_spread_to() {
    let rules = compile_rules(core_rules()).unwrap();
    let (mut game, _) = mission_scene(&rules);
    game.debug_grant_faith(P0, 400);
    let bought = |commands: &[Command]| commands.iter().any(|c| matches!(c, Command::PurchaseWithFaith { .. }));
    let obs = game.ai_observation(&rules, P0);
    assert!(bought(&religion_commands_with(&obs, &rules, true, None)), "P1's city is a target");

    // P1's city converted: no city left to spread to.
    let foreign = game.view().auth.cities().iter().find(|(_, c)| c.owner == P1).map(|(h, _)| h).unwrap();
    let next_to = tile(&game, 8, 5);
    let preacher = game.debug_spawn_unit(&rules, P0, next_to, unit_kind(&rules, "core:unit/great-prophet"));
    game.execute(&rules, Command::SpreadReligion { unit: preacher }).unwrap();
    assert_eq!(city_majority(game.view().auth, foreign), Some(Creed::Religion(ReligionId(0))));
    let obs = game.ai_observation(&rules, P0);
    assert!(!bought(&religion_commands_with(&obs, &rules, true, None)), "no target, no missionary");
    assert!(bought(&religion_commands_with(&obs, &rules, false, None)), "the game's AI buys one anyway");
}

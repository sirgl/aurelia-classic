//! Where the spaceship is built (variant `space-finish`,
//! classic/docs/ai-science-victory.md): each space item in the city that
//! finishes it soonest, before the items the allocator chose there; a part
//! waiting in a weak city moved to a strong one; a production percent
//! toward the parts priced by what it speeds in the city.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::value::{PriceModel, Prices, SpaceDemand};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::victory::{space_plan, SpaceQueue};
use classic_rules::ai::{AiDriver, AiSettings};
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, ProductionItem, TechId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::ProjectId;
use classic_rules::rules::effects::{Condition, Modifier, ProductionFilter, YieldKind};
use classic_rules::world::state::GameConfig;
use classic_rules::knowledge::public::PlayerQueries;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

/// The prerequisite project and the parts, from the rules' data.
fn apollo(rules: &CompiledRuleset) -> ProjectId {
    classic_rules::ai::victory::prerequisite_projects(rules)[0]
}

fn part(rules: &CompiledRuleset, at: usize) -> UnitKindId {
    classic_rules::ai::victory::part_kinds(rules)[at].0
}

/// Regular buildings that add production to their city unconditionally.
fn production_buildings(rules: &CompiledRuleset) -> Vec<BuildingKindId> {
    rules
        .buildings
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.wonder)
        .filter(|(_, b)| {
            b.effects.passive_city.iter().any(|e| {
                matches!(e.condition, Condition::Always)
                    && matches!(e.modifier, Modifier::AddYield { yield_kind: YieldKind::Production, amount } if amount > 0)
            })
        })
        .map(|(i, _)| BuildingKindId(i as u16))
        .collect()
}

/// The buildings with a production percent toward the parts.
fn part_speeders(rules: &CompiledRuleset) -> Vec<BuildingKindId> {
    rules
        .buildings
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.wonder)
        .filter(|(_, b)| b.effects.passive_city.iter().any(|e| matches!(e.condition, Condition::Producing(ProductionFilter::SpaceshipParts))))
        .map(|(i, _)| BuildingKindId(i as u16))
        .collect()
}

/// P0 knows every technology; its capital is strong (every building that
/// adds production), its second city weak; P1 has one city far away. The
/// rules' strategic resource of the parts lies under the capital
/// (`aluminum` copies).
fn space_age(aluminum: u8) -> (GameState, CompiledRuleset, CityHandle, CityHandle) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 30, height: 20, rng_seed: 5 });
    let capital = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    let weak = game.debug_found_city(&rules, P0, tile(&game, 12, 5));
    game.debug_found_city(&rules, P1, tile(&game, 25, 15));
    for t in 0..rules.technologies.len() {
        game.debug_learn(&rules, P0, TechId(t as u16));
    }
    let needs = &rules.units[part(&rules, 0).0 as usize].required_resources;
    for (resource, _) in needs {
        game.debug_set_deposit(&rules, tile(&game, 5, 5), *resource, aluminum);
    }
    for building in production_buildings(&rules) {
        game.debug_complete_building(&rules, capital, building);
    }
    (game, rules, capital, weak)
}

fn production(game: &GameState, rules: &CompiledRuleset, city: CityHandle) -> i32 {
    let obs = game.ai_observation(rules, P0);
    obs.own_cities.iter().find(|c| c.handle == city).unwrap().income.production
}

fn plan(game: &GameState, rules: &CompiledRuleset) -> Vec<SpaceQueue> {
    space_plan(&game.ai_observation(rules, P0), rules)
}

fn queue_of(plan: &[SpaceQueue], city: CityHandle) -> Option<&[ProductionItem]> {
    plan.iter().find(|q| q.city == city).map(|q| q.queue.as_slice())
}

fn enqueue(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle, item: ProductionItem) {
    game.execute(rules, Command::EnqueueProduction { player: P0, city, item }).unwrap();
}

/// A building the rules let `city` build now that is no wonder and no
/// speeder of the parts.
fn plain_building(game: &GameState, rules: &CompiledRuleset, city: CityHandle) -> ProductionItem {
    let obs = game.ai_observation(rules, P0);
    (0..rules.buildings.len())
        .map(|i| ProductionItem::Building(BuildingKindId(i as u16)))
        .find(|item| {
            let ProductionItem::Building(kind) = item else { return false };
            !rules.buildings[kind.0 as usize].wonder && obs.view.accepts(&Command::EnqueueProduction { player: P0, city, item: *item })
        })
        .unwrap()
}

#[test]
fn the_project_goes_to_the_city_that_finishes_it_first() {
    let (game, rules, capital, weak) = space_age(12);
    assert!(production(&game, &rules, capital) >= 4 * production(&game, &rules, weak).max(1), "the capital is strong");
    let plan = plan(&game, &rules);
    assert_eq!(queue_of(&plan, capital), Some(&[ProductionItem::Project(apollo(&rules))][..]));
    assert_eq!(queue_of(&plan, weak), None, "the weak city keeps its empty queue");
}

/// A building the allocator chose goes after the space item; the city's
/// queue is rewritten, the building kept.
#[test]
fn a_space_item_goes_before_a_plain_building() {
    let (mut game, rules, capital, weak) = space_age(12);
    let building = plain_building(&game, &rules, capital);
    enqueue(&mut game, &rules, capital, building);
    let plan = plan(&game, &rules);
    assert_eq!(queue_of(&plan, capital), Some(&[ProductionItem::Project(apollo(&rules)), building][..]));
    assert_eq!(queue_of(&plan, weak), None);
}

/// A unit of the army keeps its place at the head; the space item comes
/// after it.
#[test]
fn a_unit_of_the_army_keeps_its_place() {
    let (mut game, rules, capital, _) = space_age(12);
    let obs = game.ai_observation(&rules, P0);
    let soldier = (0..rules.units.len())
        .map(|i| UnitKindId(i as u16))
        .find(|k| {
            classic_rules::ai::planner::role_of(&rules, *k) == classic_rules::ai::planner::AiRole::Military
                && obs.view.accepts(&Command::EnqueueProduction { player: P0, city: capital, item: ProductionItem::Unit(*k) })
        })
        .unwrap();
    drop(obs);
    enqueue(&mut game, &rules, capital, ProductionItem::Unit(soldier));
    let plan = plan(&game, &rules);
    let queue = queue_of(&plan, capital).expect("the project still goes to the capital");
    assert_eq!(queue[0], ProductionItem::Unit(soldier));
    assert_eq!(queue[1], ProductionItem::Project(apollo(&rules)));
}

/// A part ordered in the weak city, where it would take many times longer,
/// moves to the capital; the missing parts are spread by the turns each
/// city needs.
#[test]
fn a_part_stuck_in_a_weak_city_moves() {
    let (mut game, rules, capital, weak) = space_age(12);
    game.debug_add_project(P0, apollo(&rules));
    let cockpit = ProductionItem::Unit(part(&rules, 1));
    enqueue(&mut game, &rules, weak, cockpit);
    let plan = plan(&game, &rules);
    assert!(queue_of(&plan, capital).is_some_and(|q| q.contains(&cockpit)), "{plan:?}");
    assert!(!queue_of(&plan, weak).unwrap_or(&[]).contains(&cockpit), "{plan:?}");
    let parts: usize = classic_rules::ai::victory::part_kinds(&rules).iter().map(|(_, _, n)| usize::from(*n)).sum();
    let planned: usize = plan.iter().map(|q| q.queue.iter().filter(|i| matches!(i, ProductionItem::Unit(_))).count()).sum();
    assert_eq!(planned, parts, "every part ordered once");
}

/// Science off: no plan.
#[test]
fn no_plan_without_the_science_victory() {
    let (mut game, rules, _, _) = space_age(12);
    let options = game.view().auth.module().victory().options().with(classic_rules::world::victory::VictoryKind::Science, false);
    game.debug_set_victory_options(options);
    assert!(plan(&game, &rules).is_empty());
}

/// The parts' speeder is worth more in the city that will build them when
/// priced by the parts left, and its percent is worth nothing where it
/// would take the last copy of the resource the parts need.
#[test]
fn the_parts_speeder_is_priced_by_the_parts_it_speeds() {
    let speeder = *part_speeders(&rules_only()).first().expect("the rules speed the parts");
    let value = |aluminum: u8| {
        let (game, rules, capital, _) = space_age(aluminum);
        let obs = game.ai_observation(&rules, P0);
        let city = obs.own_cities.iter().find(|c| c.handle == capital).unwrap();
        let plain = Prices::of(&obs, &rules, PriceModel::Uses);
        let mut priced = plain;
        priced.space = Some(SpaceDemand::of(&obs, &rules));
        (plain.building(&obs, &rules, city, speeder, 2, 0), priced.building(&obs, &rules, city, speeder, 2, 0))
    };
    let (plain, priced) = value(12);
    assert!(priced > plain, "priced {priced} against the flat {plain}");
    let (_, starved) = value(1);
    assert!(starved < priced, "starved {starved} against {priced}");
}

fn rules_only() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

/// The game's AI with the space plan: the capital busy with a building
/// takes the project (the plain AI gives it to the idle weak city).
#[test]
fn the_space_finish_ai_builds_the_project_in_the_strong_city() {
    let head = |variant: Variant| {
        let (mut game, rules, capital, weak) = space_age(12);
        let building = plain_building(&game, &rules, capital);
        enqueue(&mut game, &rules, capital, building);
        let mut driver = AiDriver::with_mind(AiSettings::for_rules(&rules, rules.default_difficulty()), vec![P1], ClassicMind::new(variant));
        driver.run(&mut game, &rules, u32::MAX);
        let auth = game.view().auth;
        let first = |city: CityHandle| auth.cities().get(city).unwrap().queue.first().copied();
        (first(capital), first(weak), ProductionItem::Project(apollo(&rules)))
    };
    let (capital, _, project) = head(Variant::SpaceFinish);
    assert_eq!(capital, Some(project));
    let (capital, weak, project) = head(Variant::Standard);
    assert_ne!(capital, Some(project));
    assert_eq!(weak, Some(project));
}

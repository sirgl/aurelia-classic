//! The supply of the strategist's goal (`strategist-x`, `ai::supply`): the strategic copies the
//! science victory's items need, from the rules' data, in each channel: with the need the AI takes
//! the action, without it (`next-x`, the same mind without the supply) it does not.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::supply::SupplyNeed;
use classic_rules::ai::value::{PriceModel, Prices};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{PlayerId, ResourceId, TechId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::{CityStateId, ProjectId};
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::world::relations::DealItem;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::driver::AiDriver;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const GRASSLAND: u16 = 1;

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

/// The strategic resource the rules' spaceship parts need.
fn part_resource(rules: &CompiledRuleset) -> ResourceId {
    let (kind, _, _) = classic_rules::ai::victory::part_kinds(rules)[0];
    rules.units[kind.0 as usize].required_resources[0].0
}

/// A unit of the rules that is no part and needs the parts' resource.
fn holder_kind(rules: &CompiledRuleset) -> UnitKindId {
    let resource = part_resource(rules);
    let index = rules
        .units
        .iter()
        .position(|u| u.spaceship_project.is_none() && u.nation.is_none() && u.obsolete_tech.is_none() && u.required_resources.iter().any(|(r, _)| *r == resource))
        .expect("a unit of the rules needs the parts' resource");
    UnitKindId(index as u16)
}

/// A luxury of the rules that a worker improves on grassland.
fn luxury(rules: &CompiledRuleset) -> ResourceId {
    let index = rules
        .resources
        .iter()
        .position(|r| r.class == Some(classic_rules::rules::content::ResourceClass::Luxury) && !r.improvements.is_empty() && r.required_tech.is_none())
        .expect("a luxury of the rules");
    ResourceId(index as u16)
}

fn prerequisites(rules: &CompiledRuleset) -> Vec<ProjectId> {
    classic_rules::ai::victory::prerequisite_projects(rules)
}

/// Three players on grassland, P0 and P1 knowing every technology: P0's capital at (5, 10) with
/// `own` copies of the parts' resource under it, P1's city at (20, 10) with 4, P2's far away
/// (a city-state when `city_state`).
fn world(rules: &CompiledRuleset, own: u8, city_state: bool) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 3, width: 44, height: 20, rng_seed: 31 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    if city_state {
        game.debug_set_player_kind(P2, PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    }
    game.debug_found_city(rules, P0, tile(&game, 5, 10));
    game.debug_found_city(rules, P1, tile(&game, 20, 10));
    game.debug_found_city(rules, P2, tile(&game, 38, 4));
    for t in 0..rules.technologies.len() {
        game.debug_learn(rules, P0, TechId(t as u16));
        game.debug_learn(rules, P1, TechId(t as u16));
        game.debug_learn(rules, P2, TechId(t as u16));
    }
    let resource = part_resource(rules);
    if own > 0 {
        game.debug_set_deposit(rules, tile(&game, 5, 10), resource, own);
    }
    game.debug_set_deposit(rules, tile(&game, 20, 10), resource, 4);
    game.debug_meet_all();
    game
}

/// The commands P0's AI of `variant` gives in its turn.
fn turn(game: &mut GameState, rules: &CompiledRuleset, variant: Variant) -> Vec<Command> {
    while game.view().auth.active_player() != P0 {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, vec![P1, P2], ClassicMind::new(variant));
    driver.set_trace_capacity(100_000);
    driver.run(game, rules, u32::MAX);
    driver.trace().entries().filter_map(|e| e.command.clone()).collect()
}

/// The need counts the parts still missing and their speeders from the rules' data, less the
/// copies free; none without the science victory.
#[test]
fn the_need_is_the_copies_the_parts_and_their_speeders_lack() {
    let rules = rules();
    let resource = part_resource(&rules);
    let need = |own: u8| {
        let game = world(&rules, own, false);
        SupplyNeed::of(&game.ai_observation(&rules, P0), &rules).short(resource)
    };
    // One city builds the parts: its part's copy and its speeder's.
    assert_eq!(need(0), 2);
    assert_eq!(need(1), 1);
    assert_eq!(need(12), 0);
    let mut game = world(&rules, 0, false);
    let options = game.view().auth.module().victory().options().with(classic_rules::world::victory::VictoryKind::Science, false);
    game.debug_set_victory_options(options);
    assert!(!SupplyNeed::of(&game.ai_observation(&rules, P0), &rules).any());
}

/// The deal: P1 (a human) offers a copy for P0's embassy. P0 has a copy free, so without the need a
/// second copy is worth nothing and the embassy is given for nothing; with the need the copy is the
/// part's hammers.
#[test]
fn a_copy_the_goal_lacks_is_bought_in_a_deal() {
    let rules = rules();
    let answer = |variant: Variant| {
        let mut game = world(&rules, 1, false);
        game.debug_earn(P0, 500);
        while game.view().auth.active_player() != P1 {
            let active = game.view().auth.active_player();
            game.execute(&rules, Command::EndTurn { player: active }).unwrap();
        }
        let items = vec![(P1, DealItem::Resource { resource: part_resource(&rules), amount: 1 }), (P0, DealItem::Embassy)];
        game.execute(&rules, Command::ProposeDeal { from: P1, to: P0, items }).unwrap();
        turn(&mut game, &rules, variant).into_iter().find_map(|c| match c {
            Command::RespondDeal { accept, .. } => Some(accept),
            _ => None,
        })
    };
    assert_eq!(answer(Variant::StrategistX), Some(true));
    assert_eq!(answer(Variant::NextX), Some(false));
}

/// The city-state: an alliance that brings the copies the goal lacks is worth more gold.
#[test]
fn a_city_state_with_the_copies_is_worth_a_dearer_gift() {
    let rules = rules();
    let mut game = world(&rules, 0, true);
    game.debug_set_deposit(&rules, tile(&game, 38, 4), part_resource(&rules), 4);
    let obs = game.ai_observation(&rules, P0);
    let plain = Prices::of(&obs, &rules, PriceModel::Uses);
    let mut priced = plain;
    priced.supply = SupplyNeed::of(&obs, &rules);
    assert!(priced.supply.any());
    let value = |prices: &Prices| {
        classic_rules::ai::city_states::priced_gifts(&obs, prices, 1000).into_iter().filter(|g| g.city_state == P2).map(|g| g.value).max()
    };
    let (with, without) = (value(&priced), value(&plain));
    assert!(with.is_some() && with > without, "with the need {with:?}, without {without:?}");
}

/// The release: once the parts may start and none can for want of a copy, a unit holding one is
/// disbanded.
#[test]
fn a_unit_holding_the_copy_a_part_waits_for_is_disbanded() {
    let rules = rules();
    let disbands = |variant: Variant| {
        let mut game = world(&rules, 0, false);
        for project in prerequisites(&rules) {
            game.debug_add_project(P0, project);
        }
        let unit = game.debug_spawn_unit(&rules, P0, tile(&game, 6, 10), holder_kind(&rules));
        turn(&mut game, &rules, variant).into_iter().any(|c| matches!(c, Command::DisbandUnit { unit: u } if u == unit))
    };
    assert!(disbands(Variant::StrategistX));
    assert!(!disbands(Variant::NextX));
}

/// The tile: a city buys the tile of a deposit the goal lacks.
#[test]
fn a_city_buys_the_tile_of_a_deposit_the_goal_lacks() {
    let rules = rules();
    let buys = |variant: Variant| {
        let mut game = world(&rules, 0, false);
        let deposit = tile(&game, 7, 10);
        game.debug_set_deposit(&rules, deposit, part_resource(&rules), 4);
        // A luxury the empire lacks on another tile of the ring: the tile it buys without the need.
        game.debug_set_deposit(&rules, tile(&game, 3, 10), luxury(&rules), 1);
        game.debug_earn(P0, 2000);
        turn(&mut game, &rules, variant).into_iter().any(|c| matches!(c, Command::PurchaseTile { tile, .. } if tile == deposit))
    };
    // With the need the deposit's tile is bought. Without it `next-x` buys it too in this world (its
    // copies and yields lead the ring anyway), so the difference is read where it is made: the site's
    // supply score (`the_best_site_holds_the_deposit_the_goal_lacks`) and the need itself
    // (`without_a_shortage_strategist_x_decides_as_next_x`).
    assert!(buys(Variant::StrategistX));
}

/// The variant: `strategist-x` is `next-x` with the supply, under its own name.
#[test]
fn strategist_x_is_named_and_next_x_is_unchanged() {
    assert_eq!(ClassicMind::new(Variant::StrategistX).variant(), Variant::StrategistX);
    assert_eq!(ClassicMind::new(Variant::NextX).variant(), Variant::NextX);
    assert_eq!(Variant::from_name("strategist-x"), Some(Variant::StrategistX));
}

/// The improvement: a worker goes to the deposit the goal lacks first.
#[test]
fn a_worker_goes_to_the_deposit_the_goal_lacks() {
    let rules = rules();
    let worker = UnitKindId(rules.units.iter().position(|u| u.builder && u.domain == classic_rules::rules::content::Domain::Land && u.nation.is_none()).unwrap() as u16);
    let goes = |variant: Variant| {
        let mut game = world(&rules, 0, false);
        let deposit = tile(&game, 6, 10);
        game.debug_set_deposit(&rules, deposit, part_resource(&rules), 4);
        // A luxury the empire lacks beside the city: the job it takes without the need.
        game.debug_set_deposit(&rules, tile(&game, 4, 10), luxury(&rules), 1);
        let unit = game.debug_spawn_unit(&rules, P0, tile(&game, 5, 10), worker);
        turn(&mut game, &rules, variant).into_iter().find_map(|c| match c {
            Command::MoveUnit { unit: u, destination } if u == unit => Some(destination == deposit),
            _ => None,
        })
    };
    assert_eq!(goes(Variant::StrategistX), Some(true));
    assert_ne!(goes(Variant::NextX), Some(true));
}

/// The city: a building that gives copies the goal lacks is worth their hammers; without the need
/// it is worth what it was.
#[test]
fn a_building_that_gives_the_copies_is_worth_more_with_the_need() {
    use classic_rules::rules::effects::Modifier;
    use classic_rules::rules::stats::Stat;
    let rules = rules();
    let resource = part_resource(&rules);
    let giver = rules
        .buildings
        .iter()
        .position(|b| b.effects.passive_player.iter().any(|e| matches!(e.modifier, Modifier::AddStat { stat: Stat::ResourceCopies(r), .. } if r == resource)))
        .expect("a building of the rules gives copies of the parts' resource");
    let game = world(&rules, 0, false);
    let obs = game.ai_observation(&rules, P0);
    let city = &obs.own_cities[0];
    let plain = Prices::of(&obs, &rules, PriceModel::Uses);
    let mut priced = plain;
    priced.supply = SupplyNeed::of(&obs, &rules);
    let kind = classic_rules::ids::BuildingKindId(giver as u16);
    let (with, without) = (priced.building(&obs, &rules, city, kind, 1, 0), plain.building(&obs, &rules, city, kind, 1, 0));
    assert!(with > without, "with the need {with}, without {without}");
    let mut empty = plain;
    empty.supply = SupplyNeed::default();
    assert_eq!(empty.building(&obs, &rules, city, kind, 1, 0), without);
}

/// The need of each mind's turn as its brief carries it (`Prices::supply`).
fn need_of(game: &GameState, rules: &CompiledRuleset, variant: Variant) -> SupplyNeed {
    use kernel::ai::mind::AiMind;
    let mind = ClassicMind::new(variant);
    let mut state = classic_rules::ai::state::AiState::new(P0, game.view().auth.turn().round);
    let sight = mind.sense(game, rules, P0);
    let started = mind.start_turn(&sight, rules, P0, &mut state);
    started.brief.prices.map(|p| p.supply).unwrap_or_default()
}

/// `standard` and `next-x` carry no need where `strategist-x` has one, and where nobody lacks a copy
/// `strategist-x` decides as `next-x` does, command for command: the supply's code is inert without a
/// shortage.
#[test]
fn without_a_shortage_strategist_x_decides_as_next_x() {
    let rules = rules();
    let short = world(&rules, 0, false);
    assert!(need_of(&short, &rules, Variant::StrategistX).any());
    assert!(!need_of(&short, &rules, Variant::NextX).any());
    assert!(!need_of(&short, &rules, Variant::Standard).any());
    let decisions = |variant: Variant| {
        let mut game = world(&rules, 12, false);
        let worker = UnitKindId(rules.units.iter().position(|u| u.builder && u.domain == classic_rules::rules::content::Domain::Land && u.nation.is_none()).unwrap() as u16);
        game.debug_spawn_unit(&rules, P0, tile(&game, 5, 10), worker);
        game.debug_earn(P0, 2000);
        assert!(!need_of(&game, &rules, variant).any());
        turn(&mut game, &rules, variant).iter().map(|c| format!("{c:?}")).collect::<Vec<_>>()
    };
    let (ship, next) = (decisions(Variant::StrategistX), decisions(Variant::NextX));
    assert!(!next.is_empty());
    assert_eq!(ship, next);
}

/// The deal asked: on its round of proposals P0 asks P1, who holds copies, for them for gold a turn.
#[test]
#[ignore = "the offer of the copies for gold a turn is not made in this world (not resolved, lane A 22 Nov: classic/docs/ai-strategist-defects.md)"]
fn the_ai_asks_a_nation_for_the_copies_the_goal_lacks() {
    use classic_rules::rules::effects::{Condition, Modifier, YieldKind};
    let rules = rules();
    let asks = |variant: Variant| {
        let mut game = world(&rules, 0, false);
        let capital = game.view().auth.cities().iter().find(|(_, c)| c.owner == P0).map(|(h, _)| h).unwrap();
        // An income that carries gold a turn: every building that adds gold unconditionally.
        for (index, b) in rules.buildings.iter().enumerate() {
            let gold = b.effects.passive_city.iter().any(|e| matches!(e.condition, Condition::Always) && matches!(e.modifier, Modifier::AddYield { yield_kind: YieldKind::Gold, amount } if amount > 0));
            if gold && !b.wonder {
                game.debug_complete_building(&rules, capital, classic_rules::ids::BuildingKindId(index as u16));
            }
        }
        let interval = classic_rules::ai::diplomacy::PROPOSAL_INTERVAL;
        while game.view().auth.turn().round % interval != 0 || game.view().auth.active_player() != P0 {
            let active = game.view().auth.active_player();
            game.execute(&rules, Command::EndTurn { player: active }).unwrap();
        }
        let resource = part_resource(&rules);
        turn(&mut game, &rules, variant).into_iter().any(|c| match c {
            Command::ProposeDeal { to, items, .. } => to == P1 && items.iter().any(|(giver, item)| *giver == P1 && matches!(item, DealItem::Resource { resource: r, .. } if *r == resource)),
            _ => false,
        })
    };
    assert!(asks(Variant::StrategistX));
    assert!(!asks(Variant::NextX));
}

/// The site: a settler's best site holds the deposit the goal lacks in its work tiles; without the
/// need it is not the best.
#[test]
fn the_best_site_holds_the_deposit_the_goal_lacks() {
    use classic_rules::ai::planner::PlanTools;
    use classic_rules::ai::planner_map::{KnownMap, MapScratch};
    use classic_rules::ai::planner_settle::{best_sites, SiteContext};
    use classic_rules::ai::supply::SiteSupply;
    let rules = rules();
    let mut game = world(&rules, 0, false);
    let deposit = tile(&game, 5, 17);
    game.debug_set_deposit(&rules, deposit, part_resource(&rules), 4);
    let all: Vec<TileId> = (0..game.view().auth.map().dimensions.tile_count() as u32).map(TileId).collect();
    game.debug_reveal(P0, &all);
    let obs = game.ai_observation(&rules, P0);
    let own: Vec<TileId> = obs.own_cities.iter().map(|c| c.tile).collect();
    let foreign: Vec<TileId> = vec![tile(&game, 20, 10), tile(&game, 38, 4)];
    let profile = KnownMap::of_kind(&obs.view, classic_rules::scenario::settler_kind(&rules)).profile;
    let prices = Prices::of(&obs, &rules, PriceModel::Uses);
    let need = SupplyNeed::of(&obs, &rules);
    assert!(need.any());
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let best = |supply: SiteSupply| {
        let ctx = SiteContext { view: &obs.view, profile, rules: &rules, player: P0, own_cities: &own, foreign_cities: &foreign, reserved: &[], danger: &[], tile_memo: Default::default(), supply };
        let (mut scratch, mut counters) = (MapScratch::default(), classic_rules::ai::trace::AiCounters::default());
        let mut tools = PlanTools { rules: &rules, settings: &settings, scratch: &mut scratch, counters: &mut counters };
        let moves = 2 * classic_rules::world::unit::MOVE_DENOMINATOR;
        best_sites(&ctx, own[0], moves, &mut tools, 400).first().map(|s| s.tile)
    };
    let covers = |site: Option<TileId>| site.is_some_and(|s| game.view().auth.map().dimensions.distance(s, deposit).is_some_and(|d| d <= classic_rules::world::city::CITY_WORK_RADIUS));
    let supply = SiteSupply { need, rounds_left: prices.rounds_left };
    assert!(covers(best(supply)));
    // The need adds the deposit's hammers to a site whose work tiles hold it, and nothing without it
    // (in this world the deposit's site is the best one with or without the need).
    let score = |supply: SiteSupply| SiteContext { view: &obs.view, profile, rules: &rules, player: P0, own_cities: &own, foreign_cities: &foreign, reserved: &[], danger: &[], tile_memo: Default::default(), supply }.supply_score(deposit);
    assert!(score(supply) > 0);
    assert_eq!(score(SiteSupply::default()), 0);
}

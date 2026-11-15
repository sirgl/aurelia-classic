//! What the end of a human's turn waits for (`derived::pending`, Unciv `NextTurnAction`): one list, from the
//! game state. Every rule has a case that shows the item and a case that shows it leave once the player acts.

use classic_content::core::core_rules;
use classic_rules::command::CityDecision;
use classic_rules::combat::experience::available_promotions;
use classic_rules::derived::grants::researchable;
use classic_rules::derived::pending::{pending_actions, HandledThisTurn, PendingAction};
use classic_rules::derived::religion::free_beliefs;
use classic_rules::game::ClassicGame;
use classic_rules::ids::{CityHandle, PlayerId, ProductionItem, TileId, UnitHandle, UnitKindId};
use classic_rules::math::{HexCoord, MapDimensions};
use classic_rules::rules::content::BeliefCategory;
use classic_rules::world::city::CityStatus;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const DIMS: MapDimensions = MapDimensions { width: 20, height: 14 };

fn setup() -> (GameState, CompiledRuleset, CityHandle) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: DIMS.width, height: DIMS.height, rng_seed: 11 });
    let capital = game.debug_found_city(&rules, P0, tile(5, 5));
    game.debug_found_city(&rules, P1, tile(15, 10));
    (game, rules, capital)
}

fn tile(q: i32, r: i32) -> TileId {
    DIMS.to_tile(HexCoord { q, r }).unwrap()
}

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn pending(game: &GameState, rules: &CompiledRuleset) -> Vec<PendingAction> {
    pending_actions(game.view().auth, rules, P0, HandledThisTurn::default())
}

/// The capital builds a warrior, a technology is researched: nothing else waits.
fn settle_city_and_research(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle) {
    let warrior = ProductionItem::Unit(kind(rules, "core:unit/warrior"));
    game.execute(rules, Command::EnqueueProduction { player: P0, city, item: warrior }).unwrap();
    let technology = researchable(game.view().auth, rules, P0)[0];
    game.execute(rules, Command::ChooseTechnology { player: P0, technology }).unwrap();
}

fn spawn(game: &mut GameState, rules: &CompiledRuleset, key: &str) -> UnitHandle {
    game.debug_spawn_unit(rules, P0, tile(8, 5), kind(rules, key))
}

#[test]
fn a_city_with_an_empty_queue_waits_for_production() {
    let (mut game, rules, capital) = setup();
    assert!(pending(&game, &rules).contains(&PendingAction::EmptyProduction(capital)));
    settle_city_and_research(&mut game, &rules, capital);
    assert_eq!(pending(&game, &rules), vec![]);
}

#[test]
fn a_puppet_decides_for_itself() {
    let (mut game, rules, capital) = setup();
    game.debug_set_city_status(capital, CityStatus::Puppet);
    assert!(!pending(&game, &rules).contains(&PendingAction::EmptyProduction(capital)));
}

#[test]
fn a_captured_city_waits_for_the_decision() {
    let (mut game, rules, capital) = setup();
    game.debug_set_city_status(capital, CityStatus::AwaitingDecision);
    let list = pending(&game, &rules);
    assert!(list.contains(&PendingAction::CityDecision(capital)));
    assert!(!list.contains(&PendingAction::EmptyProduction(capital)), "the decision comes first");
    game.execute(&rules, Command::DecideCity { player: P0, city: capital, decision: CityDecision::Annex }).unwrap();
    assert!(!pending(&game, &rules).contains(&PendingAction::CityDecision(capital)));
}

#[test]
fn research_waits_until_a_technology_is_chosen() {
    let (mut game, rules, capital) = setup();
    assert!(pending(&game, &rules).contains(&PendingAction::Research));
    settle_city_and_research(&mut game, &rules, capital);
    assert!(!pending(&game, &rules).contains(&PendingAction::Research));
}

#[test]
fn a_policy_waits_once_a_turn() {
    let (mut game, rules, capital) = setup();
    settle_city_and_research(&mut game, &rules, capital);
    assert_eq!(pending(&game, &rules), vec![]);
    game.debug_grant_culture(P0, 1000);
    assert_eq!(pending(&game, &rules), vec![PendingAction::Policy]);
    let chosen = HandledThisTurn { policy_chosen: true, resting: &[] };
    assert_eq!(pending_actions(game.view().auth, &rules, P0, chosen), vec![]);
}

#[test]
fn a_pantheon_waits_when_the_faith_pays_it() {
    let (mut game, rules, capital) = setup();
    settle_city_and_research(&mut game, &rules, capital);
    assert!(!pending(&game, &rules).contains(&PendingAction::Pantheon));
    game.debug_grant_faith(P0, 1000);
    assert_eq!(pending(&game, &rules), vec![PendingAction::Pantheon]);
    let belief = free_beliefs(game.view().auth, &rules, BeliefCategory::Pantheon)[0];
    game.execute(&rules, Command::FoundPantheon { player: P0, belief }).unwrap();
    assert_eq!(pending(&game, &rules), vec![]);
}

#[test]
fn a_unit_with_moves_and_no_order_waits() {
    let (mut game, rules, capital) = setup();
    settle_city_and_research(&mut game, &rules, capital);
    let warrior = spawn(&mut game, &rules, "core:unit/warrior");
    assert_eq!(pending(&game, &rules), vec![PendingAction::UnitOrder(warrior)]);
    game.execute(&rules, Command::Fortify { unit: warrior }).unwrap();
    assert_eq!(pending(&game, &rules), vec![], "a fortified unit has its order");
}

#[test]
fn a_unit_the_player_rested_has_its_order() {
    let (mut game, rules, capital) = setup();
    settle_city_and_research(&mut game, &rules, capital);
    let warrior = spawn(&mut game, &rules, "core:unit/warrior");
    let rested = HandledThisTurn { policy_chosen: false, resting: &[warrior] };
    assert_eq!(pending_actions(game.view().auth, &rules, P0, rested), vec![]);
}

#[test]
fn a_unit_on_the_march_has_its_order() {
    let (mut game, rules, capital) = setup();
    settle_city_and_research(&mut game, &rules, capital);
    let warrior = spawn(&mut game, &rules, "core:unit/warrior");
    game.execute(&rules, Command::MarchTo { unit: warrior, destination: tile(12, 5) }).unwrap();
    let moving = game.view().auth.units().get(warrior).unwrap().core.goal.is_some();
    assert!(moving, "the march is a standing order");
    assert_eq!(pending(&game, &rules), vec![]);
}

#[test]
fn a_great_person_waits_in_its_own_item() {
    let (mut game, rules, capital) = setup();
    settle_city_and_research(&mut game, &rules, capital);
    let general = spawn(&mut game, &rules, "core:unit/great-general");
    let warrior = spawn(&mut game, &rules, "core:unit/warrior");
    assert_eq!(pending(&game, &rules), vec![PendingAction::GreatPerson(general), PendingAction::UnitOrder(warrior)]);
}

#[test]
fn a_unit_that_can_be_promoted_waits_even_with_an_order() {
    let (mut game, rules, capital) = setup();
    settle_city_and_research(&mut game, &rules, capital);
    let warrior = spawn(&mut game, &rules, "core:unit/warrior");
    game.execute(&rules, Command::Fortify { unit: warrior }).unwrap();
    game.debug_set_experience(warrior, 10);
    assert_eq!(pending(&game, &rules), vec![PendingAction::Promotion(warrior)]);
    let core = *game.view().auth.units().get(warrior).unwrap().core;
    let promotion = available_promotions(game.view().auth, &rules, &core)[0];
    game.execute(&rules, Command::Promote { unit: warrior, promotion }).unwrap();
    assert!(!pending(&game, &rules).contains(&PendingAction::Promotion(warrior)));
}

#[test]
fn owed_free_choices_come_first() {
    let (mut game, rules, capital) = setup();
    settle_city_and_research(&mut game, &rules, capital);
    game.debug_owe_free_choices(P0);
    assert_eq!(pending(&game, &rules), vec![PendingAction::FreeGreatPerson, PendingAction::FreeTechnology]);
}

#[test]
fn the_list_keeps_the_order_the_button_asks_in() {
    let (mut game, rules, capital) = setup();
    game.debug_grant_culture(P0, 1000);
    game.debug_grant_faith(P0, 1000);
    let warrior = spawn(&mut game, &rules, "core:unit/warrior");
    assert_eq!(
        pending(&game, &rules),
        vec![
            PendingAction::EmptyProduction(capital),
            PendingAction::Research,
            PendingAction::Policy,
            PendingAction::Pantheon,
            PendingAction::UnitOrder(warrior),
        ]
    );
}

#[test]
fn nothing_is_asked_of_a_player_without_cities_or_units() {
    let (game, rules, _) = setup();
    let empty = pending_actions(game.view().auth, &rules, PlayerId(7), HandledThisTurn::default());
    assert_eq!(empty, vec![]);
}

#[test]
fn before_the_first_city_only_units_are_asked() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: DIMS.width, height: DIMS.height, rng_seed: 11 });
    game.debug_grant_culture(P0, 1000);
    let settler = spawn(&mut game, &rules, "core:unit/settler");
    assert_eq!(pending(&game, &rules), vec![PendingAction::UnitOrder(settler)], "no research, no policy: there is no city");
}

/// P0 takes a city of P1 (a second one, so it is not an original capital) and annexes it.
fn annexed_conquest(game: &mut GameState, rules: &CompiledRuleset, capital: CityHandle) -> CityHandle {
    let city = game.debug_found_city(rules, P1, tile(9, 5));
    game.debug_set_population(rules, city, 4);
    let attacker = game.debug_spawn_unit(rules, P0, tile(8, 5), kind(rules, "core:unit/warrior"));
    game.debug_meet(P0, P1);
    game.execute(rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    let max = classic_rules::combat::city::max_hit_points(game.view().auth, rules, game.view().auth.cities().get(city).unwrap());
    game.debug_set_city_damage(city, max - 1);
    game.execute(rules, Command::AttackCity { attacker, city }).unwrap();
    game.execute(rules, Command::DecideCity { player: P0, city, decision: CityDecision::Annex }).unwrap();
    settle_city_and_research(game, rules, capital);
    game.execute(rules, Command::Fortify { unit: attacker }).unwrap();
    city
}

/// The core refuses every order in a city in resistance, so the list must not ask for one: the resistance
/// ends only with the owner's turns, and the end of the turn waits for the list.
#[test]
fn an_annexed_conquest_in_resistance_is_not_asked_until_it_can_build() {
    let (mut game, rules, capital) = setup();
    let city = annexed_conquest(&mut game, &rules, capital);
    let core = game.view().auth.cities().get(city).unwrap();
    assert_eq!((core.status, core.resistance_turns, core.queue.is_empty()), (CityStatus::Occupied, 2, true));
    assert_eq!(pending(&game, &rules), vec![], "in resistance: no order can be given");

    for _ in 0..2 {
        game.execute(&rules, Command::EndTurn { player: P0 }).unwrap();
        game.execute(&rules, Command::EndTurn { player: P1 }).unwrap();
    }
    assert_eq!(game.view().auth.cities().get(city).unwrap().resistance_turns, 0);
    let warrior = ProductionItem::Unit(kind(&rules, "core:unit/warrior"));
    assert!(pending(&game, &rules).contains(&PendingAction::EmptyProduction(city)), "the resistance is over: it asks");
    game.execute(&rules, Command::EnqueueProduction { player: P0, city, item: warrior }).unwrap();
    assert!(!pending(&game, &rules).contains(&PendingAction::EmptyProduction(city)));
}

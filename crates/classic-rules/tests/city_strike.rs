//! The city's ranged strike on the classic ruleset (classic/docs/classic/rules.md): range 2, strength from the city's strength (×75 % without the
//! building defense), the garrison and belief bonuses; every city strikes,
//! walls or not; city damage is journaled (the presentation shows it); the
//! AI strikes the enemy its cities hurt most.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::classic::ClassicChange;
use classic_rules::classic::content::BuildingAbility;
use classic_rules::combat::city::city_ranged_strength;
use classic_rules::game::ClassicGame;
use classic_rules::ids::{BeliefId, PlayerId, TileId, UnitKindId};
use classic_rules::math::{HexCoord, MapDimensions};
use classic_rules::transaction::journal::UnitChange;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const DIMS: MapDimensions = MapDimensions { width: 20, height: 14 };

fn setup() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let game = GameState::new(&rules, GameConfig { players: 2, width: DIMS.width, height: DIMS.height, rng_seed: 11 });
    (game, rules)
}

fn tile(q: i32, r: i32) -> TileId {
    DIMS.to_tile(HexCoord { q, r }).unwrap()
}

fn unit(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

fn war(game: &mut GameState, rules: &CompiledRuleset) {
    game.debug_meet(P0, P1);
    game.execute(rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
}

/// Every city strikes, no walls needed (the Walls' AllowsRangeStrike is
/// data only); the walls carry the flag.
#[test]
fn every_city_strikes_and_walls_only_carry_the_flag() {
    let (mut game, rules) = setup();
    let city = game.debug_found_city(&rules, P0, tile(5, 5));
    let target = tile(7, 5);
    let warrior = game.debug_spawn_unit(&rules, P1, target, unit(&rules, "core:unit/warrior"));
    war(&mut game, &rules);
    let result = game.execute(&rules, Command::CityStrike { city, target }).unwrap();
    assert!(game.view().auth.units().get(warrior).unwrap().core.hp < 100);
    assert!(result.changes().unit_changes.iter().any(|c| matches!(c, UnitChange::Damaged { .. })));
    let walls = rules.buildings.iter().find(|b| b.key == "core:building/walls").unwrap();
    assert!(walls.module.abilities.contains(&BuildingAbility::AllowsRangeStrike));
    let out_of_range = tile(8, 5);
    game.debug_spawn_unit(&rules, P1, out_of_range, unit(&rules, "core:unit/warrior"));
    assert!(game.execute(&rules, Command::CityStrike { city, target: out_of_range }).is_err());
}

/// Goddess of Protection (pantheon): +30 % city ranged strength in the
/// cities that follow it.
#[test]
fn goddess_of_protection_raises_the_strike_of_following_cities() {
    let (mut game, rules) = setup();
    let city = game.debug_found_city(&rules, P0, tile(5, 5));
    game.debug_found_city(&rules, P1, tile(15, 10));
    let strength = |game: &GameState| {
        let auth = game.view().auth;
        city_ranged_strength(auth, &rules, auth.cities().get(city).unwrap())
    };
    let before = strength(&game);
    let goddess = BeliefId(rules.module.beliefs.iter().position(|b| b.key == "core:belief/pantheon/goddess-of-protection").unwrap() as u16);
    game.debug_grant_faith(P0, 20);
    game.execute(&rules, Command::FoundPantheon { player: P0, belief: goddess }).unwrap();
    assert_eq!(strength(&game), before * 130 / 100);
}

/// Melee and ranged damage to a city is journaled with its amount.
#[test]
fn damage_to_a_city_is_journaled() {
    let (mut game, rules) = setup();
    let city = game.debug_found_city(&rules, P1, tile(8, 6));
    game.debug_found_city(&rules, P1, tile(15, 11));
    let archer = game.debug_spawn_unit(&rules, P0, tile(6, 6), unit(&rules, "core:unit/archer"));
    war(&mut game, &rules);
    let result = game.execute(&rules, Command::RangedAttack { unit: archer, target: tile(8, 6) }).unwrap();
    let damage = result.changes().module_changes.iter().find_map(|c| match c {
        ClassicChange::CityDamaged { city: hit, damage } if *hit == city => Some(*damage),
        _ => None,
    });
    let taken = game.view().auth.cities().get(city).unwrap().damage;
    assert_eq!(damage, Some(taken), "the journal names the city's loss");
    assert!(taken > 0);
}

/// The AI's cities strike enemy units in range at the start of their turn.
#[test]
fn the_ai_strikes_with_its_cities() {
    let (mut game, rules) = setup();
    game.debug_found_city(&rules, P0, tile(5, 5));
    game.debug_found_city(&rules, P1, tile(15, 10));
    let warrior = game.debug_spawn_unit(&rules, P1, tile(7, 5), unit(&rules, "core:unit/warrior"));
    war(&mut game, &rules);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = kernel::ai::driver::AiDriver::with_mind(settings, vec![P1], ClassicMind::new(Variant::Standard));
    driver.record_commands();
    driver.run(&mut game, &rules, u32::MAX);
    let struck = driver.commands().iter().any(|c| matches!(c, Command::CityStrike { .. }));
    assert!(struck, "the AI city struck");
    let hp = game.view().auth.units().get(warrior).map_or(0, |u| u.core.hp);
    assert!(hp < 100);
}

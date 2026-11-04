//! The war operation counts the weapons the player has or can build
//! (`operations::candidate`, `strikes::struck`, classic/docs/ai-strikes.md "The war case"): a
//! city that the army alone cannot take within the limits of an army becomes a target with a
//! nuclear weapon's help, and the operation is planned around it.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{PlayerId, TechId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::driver::AiDriver;
use kernel::ai::trace::Reason;

const AI: PlayerId = PlayerId(0);
const ENEMY: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap() as u16)
}

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap() as u16)
}

/// The AI with three cities and the nuclear era (and uranium) at war with a passive rival whose
/// city of `pop` citizens it sees; the war is declared. Returns the operations formed and the
/// weapons ordered in 120 rounds.
fn war_counts(variant: Variant, pop: u32, bomb: bool) -> (u64, u64, u64) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 40, height: 20, rng_seed: 31 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(1))).collect();
    game.debug_set_terrain_batch(&rules, &land);
    let capital = game.debug_found_city(&rules, AI, tile(&game, 5, 10));
    game.debug_set_population(&rules, capital, 12);
    for (q, r) in [(5, 5), (5, 15)] {
        let c = game.debug_found_city(&rules, AI, tile(&game, q, r));
        game.debug_set_population(&rules, c, 8);
    }
    let city = game.debug_found_city(&rules, ENEMY, tile(&game, 12, 10));
    game.debug_set_population(&rules, city, pop);
    // A garrison and a ring of defenders on the hills round it: the city is not for walking in.
    for (q, r) in [(12, 10), (11, 10), (13, 10), (12, 9), (12, 11), (11, 11), (13, 9)] {
        game.debug_spawn_unit(&rules, ENEMY, tile(&game, q, r), kind(&rules, "core:unit/infantry"));
    }
    let uranium = classic_rules::ids::ResourceId(rules.resources.iter().position(|r| r.key == "core:resource/uranium").unwrap() as u16);
    game.debug_set_resource_batch(&rules, &[(tile(&game, 5, 10), uranium)]);
    for key in ["core:tech/advanced-ballistics", "core:tech/atomic-theory", "core:tech/nuclear-fission", "core:tech/radar", "core:tech/flight", "core:tech/electronics", "core:tech/rocketry", "core:tech/combined-arms", "core:tech/mobile-tactics", "core:tech/steel", "core:tech/gunpowder"] {
        game.debug_learn(&rules, AI, tech(&rules, key));
    }
    game.debug_earn(AI, 4000);
    for (q, r) in [(7, 9), (7, 11), (8, 10), (6, 8), (6, 12), (8, 8), (8, 12)] {
        game.debug_spawn_unit(&rules, AI, tile(&game, q, r), kind(&rules, "core:unit/infantry"));
    }
    for (q, r) in [(6, 9), (6, 11), (8, 9)] {
        game.debug_spawn_unit(&rules, AI, tile(&game, q, r), kind(&rules, "core:unit/artillery"));
    }
    if bomb {
        game.debug_spawn_unit(&rules, AI, tile(&game, 5, 10), kind(&rules, "core:unit/atomic-bomb"));
    }
    game.debug_meet(AI, ENEMY);
    let seen: Vec<TileId> = (0..40).flat_map(|q| (0..20).map(move |r| (q, r))).map(|(q, r)| tile(&game, q, r)).collect();
    game.debug_reveal(AI, &seen);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, vec![ENEMY], ClassicMind::new(variant));
    driver.set_trace_capacity(1_000_000);
    let round = game.view().auth.turn().round;
    let enemy_city = tile(&game, 12, 10);
    driver.state_mut(AI, round).see_city(classic_rules::ai::state::SeenCity { tile: enemy_city, owner: ENEMY, population: pop, last_seen_round: round });
    while game.view().auth.turn().round < 120 && game.winner().is_none() {
        let active = game.view().auth.active_player();
        if active == ENEMY {
            game.execute(&rules, Command::EndTurn { player: ENEMY }).unwrap();
        } else {
            let round = game.view().auth.turn().round;
            // The city stays in sight of the AI's memory (a scout would renew it).
            driver.state_mut(AI, round).see_city(classic_rules::ai::state::SeenCity { tile: enemy_city, owner: ENEMY, population: pop, last_seen_round: round });
            driver.run(&mut game, &rules, u32::MAX);
        }
    }
    let ordered_weapons = driver
        .trace()
        .entries()
        .filter(|e| matches!(&e.command, Some(Command::EnqueueProduction { item: classic_rules::ids::ProductionItem::Unit(k), .. }) if classic_rules::ai::strikes::weapon(&rules, *k).is_some()))
        .count() as u64;
    let operations = driver.trace().total(Reason::FormOperation);
    eprintln!("round {} winner {:?} counters {:?} cities {}", game.view().auth.turn().round, game.winner(), driver.counters(), game.view().auth.indexes().cities_of(AI).len());
    eprintln!("{variant:?} against a city of {pop}: operations {operations}, weapons ordered {ordered_weapons}, launched {}, missiles {}, nukes {}", driver.trace().total(Reason::LaunchOperation), driver.trace().total(Reason::FireMissile), driver.trace().total(Reason::LaunchNuke));
    (operations, ordered_weapons, driver.trace().total(Reason::LaunchNuke))
}

/// The AI that owns a nuclear weapon forms its operation counting on it and uses it; the game's
/// AI before the heavy arms (`previous`) does not use the weapon it owns.
#[test]
fn an_operation_planned_around_a_nuclear_weapon_on_hand_uses_it() {
    let (_, _, nukes_previous) = war_counts(Variant::Previous, 12, true);
    let (operations, _, nukes) = war_counts(Variant::Standard, 12, true);
    assert_eq!(nukes_previous, 0);
    assert!(operations >= 1 && nukes >= 1, "operations {operations}, nuclear weapons launched {nukes}");
}

/// The weapons the player could build do not hold up a war its army can fight: the plan that
/// fits in time wins (a nuclear weapon to build at this production takes a thousand rounds).
/// `previous` is `pre-war-fix` without the heavy arms: the two differ only by the weapons.
#[test]
fn a_weapon_that_cannot_be_ready_in_time_does_not_stop_the_plain_operation() {
    let (plain, ..) = war_counts(Variant::Previous, 12, false);
    let (armed, weapons, _) = war_counts(Variant::PreWarFix, 12, false);
    assert!(plain >= 1 && armed == plain, "operations {armed} against {plain}");
    assert_eq!(weapons, 0, "no weapon is built for an operation that does not wait for it");
}

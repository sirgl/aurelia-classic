//! A world or national wonder is built in one city of a player at a time:
//! queued in one own city, the order for another is refused; cleared there,
//! it is accepted again.

use classic_content::core::core_rules;
use classic_rules::command::{CommandError, Refusal};
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, ProductionItem, TechId};
use classic_rules::rules::content::BuildingKind;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn city(game: &mut GameState, rules: &CompiledRuleset, q: i32, r: i32) -> CityHandle {
    let t = game.view().auth.map().dimensions.to_tile(classic_rules::math::HexCoord { q, r }).unwrap();
    game.debug_found_city(rules, P0, t)
}

fn enqueue(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle, item: ProductionItem) -> Result<(), CommandError> {
    game.execute(rules, Command::EnqueueProduction { player: P0, city, item }).map(|_| ())
}

/// The first building of `once` kind the first city accepts, every
/// technology known.
fn accepted(game: &mut GameState, rules: &CompiledRuleset, at: CityHandle, once: impl Fn(&CompiledRuleset, usize) -> bool) -> Option<ProductionItem> {
    (0..rules.buildings.len())
        .filter(|&i| once(rules, i))
        .map(|i| ProductionItem::Building(BuildingKindId(i as u16)))
        .find(|&item| enqueue(game, rules, at, item).is_ok())
}

fn check(once: impl Fn(&CompiledRuleset, usize) -> bool, what: &str) {
    let rules = rules();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 40, height: 24, rng_seed: 5 });
    for t in 0..rules.technologies.len() {
        game.debug_learn(&rules, P0, TechId(t as u16));
    }
    let a = city(&mut game, &rules, 6, 6);
    let b = city(&mut game, &rules, 16, 6);
    // What national wonders require of the empire: every regular building
    // in both cities, and citizens.
    for c in [a, b] {
        game.debug_set_population(&rules, c, 10);
        for (i, def) in rules.buildings.iter().enumerate() {
            if def.kind == BuildingKind::Regular {
                game.debug_add_building(&rules, c, BuildingKindId(i as u16));
            }
        }
    }
    let Some(item) = accepted(&mut game, &rules, a, once) else {
        panic!("no {what} a city accepts");
    };
    assert_eq!(enqueue(&mut game, &rules, b, item), Err(CommandError::Refused(Refusal::QueuedElsewhere)), "{what} queued twice");
    game.execute(&rules, Command::ClearProduction { player: P0, city: a }).unwrap();
    enqueue(&mut game, &rules, b, item).unwrap_or_else(|e| panic!("{what} refused after the clear: {e:?}"));
}

#[test]
fn a_world_wonder_is_queued_in_one_city_at_a_time() {
    check(|rules, i| rules.buildings[i].wonder, "world wonder");
}

#[test]
fn a_national_wonder_is_queued_in_one_city_at_a_time() {
    check(|rules, i| rules.buildings[i].kind == BuildingKind::NationalWonder, "national wonder");
}

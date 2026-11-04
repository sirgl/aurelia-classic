//! The public income query is what the end turn applies (PLAN M3: the HUD
//! copied the formula and ignored modifiers and resource gold).

use classic_content::core::core_rules;
use classic_rules::derived::income::{city_income, player_income};
use classic_rules::ids::{BuildingKindId, PlayerId, ProductionItem, TechId};
use classic_rules::math::HexCoord;
use classic_rules::{compile_rules, Command, GameConfig, GameState};

fn building(rules: &classic_rules::CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap() as u16)
}

#[test]
fn city_income_is_what_the_end_turn_adds() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(
        &rules,
        GameConfig {
            players: 1,
            width: 16,
            height: 16,
            rng_seed: 11,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let center = dims.to_tile(HexCoord { q: 8, r: 8 }).unwrap();
    let city = game.debug_found_city(&rules, PlayerId(0), center);
    // Building and technology modifiers on the city's output.
    for key in [
        "core:building/monument",
        "core:building/granary",
        "core:building/library",
    ] {
        game.debug_add_building(&rules, city, building(&rules, key));
    }
    let tech = TechId(
        rules
            .technologies
            .iter()
            .position(|t| t.requires.is_empty())
            .unwrap() as u16,
    );
    game.execute(
        &rules,
        Command::ChooseTechnology {
            player: PlayerId(0),
            technology: tech,
        },
    )
    .unwrap();
    // A settler in the queue (size 2 at least): nothing completes this
    // turn, the food surplus goes to production.
    game.debug_grow_city(&rules, city, 1);
    let settler = rules.units.iter().position(|u| u.founder).unwrap();
    let item = ProductionItem::Unit(classic_rules::ids::UnitKindId(settler as u16));
    game.execute(
        &rules,
        Command::EnqueueProduction {
            player: PlayerId(0),
            city,
            item,
        },
    )
    .unwrap();
    // A treasury to pay the classic maintenance from (monument, granary and
    // library cost 3): the balance is what the end turn applies.
    game.debug_earn(PlayerId(0), 50);
    let auth = game.view().auth;
    let core = auth.cities().get(city).unwrap().clone();
    let income = city_income(auth, &rules, &core);
    let player = player_income(auth, &rules, PlayerId(0));
    let gold_before = auth.players().treasury_of(PlayerId(0)) as i32;
    let science_before = auth.players().science_of(PlayerId(0)) as i32;
    game.execute(
        &rules,
        Command::EndTurn {
            player: PlayerId(0),
        },
    )
    .unwrap();
    let auth = game.view().auth;
    let after = auth.cities().get(city).unwrap();
    assert_eq!(after.production as i32, core.production as i32 + income.production);
    assert_eq!(
        after.food_times100 as i32,
        core.food_times100 as i32 + income.food_surplus_times100
    );
    assert_eq!(after.culture, core.culture + income.culture);
    assert_eq!(
        auth.players().treasury_of(PlayerId(0)) as i32,
        gold_before + player.gold
    );
    let science_after = auth.players().science_of(PlayerId(0)) as i32;
    assert_eq!(science_after, science_before + player.science);
}

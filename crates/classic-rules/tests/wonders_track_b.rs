//! Culture and faith rules of classic wonders: a wonder needs its policy branch
//! (Pyramids: Liberty), holy-city wonders go into a holy city (Grand Temple,
//! Borobudur), the Grand Temple doubles its city's religious pressure, the
//! Great Mosque of Djenne gives its missionaries one more spread, the CN
//! Tower owes a Broadcast Tower to every city.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ids::{BeliefId, BuildingKindId, CityHandle, PlayerId, ProductionItem, UnitKindId};
use classic_rules::rules::content::{PolicyBranchId, ReligionId};
use classic_rules::rules::effects::{Modifier, Trigger};
use classic_rules::{compile_rules, Command, CompiledRuleset, GameConfig, GameState};

const P0: PlayerId = PlayerId(0);

fn rules() -> CompiledRuleset {
    compile_rules(core_rules()).unwrap()
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap() as u16)
}

fn belief(rules: &CompiledRuleset, key: &str) -> BeliefId {
    BeliefId(rules.module.beliefs.iter().position(|b| b.key == key).unwrap() as u16)
}

fn world(rules: &CompiledRuleset) -> (GameState, CityHandle) {
    let mut game = GameState::new(
        rules,
        GameConfig {
            players: 2,
            width: 40,
            height: 24,
            rng_seed: 3,
        },
    );
    let dims = game.view().auth.map().dimensions;
    let tile = dims
        .to_tile(classic_rules::math::HexCoord { q: 8, r: 10 })
        .unwrap();
    let city = game.debug_found_city(rules, P0, tile);
    (game, city)
}

fn learn_all(game: &mut GameState, rules: &CompiledRuleset) {
    for t in 0..rules.technologies.len() {
        game.debug_learn(rules, P0, classic_rules::ids::TechId(t as u16));
    }
}

fn enqueue(
    game: &mut GameState,
    rules: &CompiledRuleset,
    city: CityHandle,
    b: BuildingKindId,
) -> Result<(), String> {
    let item = ProductionItem::Building(b);
    game.execute(
        rules,
        Command::EnqueueProduction {
            player: P0,
            city,
            item,
        },
    )
    .map(|_| ())
    .map_err(|e| format!("{e:?}"))
}

/// P0 founds Buddhism in `city` (a Great Prophet placed by the harness).
fn found_religion(game: &mut GameState, rules: &CompiledRuleset, city: CityHandle) {
    let tile = game.view().auth.cities().get(city).unwrap().tile;
    let prophet = rules
        .units
        .iter()
        .position(|u| u.key == "core:unit/great-prophet")
        .unwrap();
    let unit = game.debug_spawn_unit(rules, P0, tile, UnitKindId(prophet as u16));
    let religion = ReligionId(
        rules
            .module.religions
            .iter()
            .position(|r| r.key == "core:religion/buddhism")
            .unwrap() as u16,
    );
    let command = Command::FoundReligion {
        unit,
        religion,
        founder: belief(rules, "core:belief/founder/tithe"),
        follower: belief(rules, "core:belief/follower/pagodas"),
        pantheon: Some(belief(rules, "core:belief/pantheon/god-of-craftsmen")),
        bonus: None,
    };
    game.execute(rules, command).unwrap();
}

#[test]
fn the_pyramids_need_liberty() {
    let rules = rules();
    let (mut game, city) = world(&rules);
    learn_all(&mut game, &rules);
    let pyramids = building(&rules, "core:building/pyramids");
    let err = enqueue(&mut game, &rules, city, pyramids).unwrap_err();
    assert!(err.contains("policy branch"), "{err}");
    let liberty = rules
        .module.policy_branches
        .iter()
        .position(|b| b.key == "core:policy/liberty")
        .unwrap();
    game.debug_grant_culture(P0, 100);
    let branch = PolicyBranchId(liberty as u16);
    game.execute(&rules, Command::UnlockPolicyBranch { player: P0, branch })
        .unwrap();
    enqueue(&mut game, &rules, city, pyramids).unwrap();
}

#[test]
fn the_grand_temple_goes_into_a_holy_city() {
    let rules = rules();
    let (mut game, city) = world(&rules);
    learn_all(&mut game, &rules);
    let temple = building(&rules, "core:building/temple");
    let grand = building(&rules, "core:building/grand_temple");
    game.debug_add_building(&rules, city, temple);
    let err = enqueue(&mut game, &rules, city, grand).unwrap_err();
    assert!(err.contains("holy city"), "{err}");
    found_religion(&mut game, &rules, city);
    enqueue(&mut game, &rules, city, grand).unwrap();
}

#[test]
fn the_great_mosque_gives_its_missionaries_one_more_spread() {
    let rules = rules();
    let (mut game, city) = world(&rules);
    learn_all(&mut game, &rules);
    found_religion(&mut game, &rules, city);
    let missionary = UnitKindId(
        rules
            .units
            .iter()
            .position(|u| u.key == "core:unit/missionary")
            .unwrap() as u16,
    );
    let buy = |game: &mut GameState| {
        game.debug_grant_faith(P0, 1000);
        let item = ProductionItem::Unit(missionary);
        game.execute(
            &rules,
            Command::PurchaseWithFaith {
                player: P0,
                city,
                item,
            },
        )
        .unwrap();
        let units = game.view().auth.module().religion().units();
        units.last().unwrap().spreads_left
    };
    let plain = buy(&mut game);
    game.debug_add_building(
        &rules,
        city,
        building(&rules, "core:building/great_mosque_of_djenne"),
    );
    assert_eq!(buy(&mut game), plain + 1);
}

#[test]
fn the_cn_tower_owes_a_broadcast_tower_to_every_city() {
    let rules = rules();
    let tower = building(&rules, "core:building/cn_tower");
    let broadcast = building(&rules, "core:building/broadcast_tower");
    let def = &rules.buildings[tower.0 as usize];
    let grant = def
        .effects
        .triggered(Trigger::OnBuilt)
        .first()
        .map(|e| e.modifier);
    assert_eq!(
        grant,
        Some(Modifier::FreeBuildingInCities {
            building: broadcast,
            count: 255
        })
    );
}

/// Pressure of Buddhism on a neighbouring city after one round of play, with
/// or without the Grand Temple in the holy city.
fn neighbour_pressure(with_grand_temple: bool) -> u32 {
    let rules = rules();
    let (mut game, city) = world(&rules);
    learn_all(&mut game, &rules);
    found_religion(&mut game, &rules, city);
    if with_grand_temple {
        game.debug_add_building(&rules, city, building(&rules, "core:building/grand_temple"));
    }
    let dims = game.view().auth.map().dimensions;
    let tile = dims
        .to_tile(classic_rules::math::HexCoord { q: 14, r: 10 })
        .unwrap();
    let neighbour = game.debug_found_city(&rules, P0, tile);
    for _ in 0..2 {
        let player = game.view().auth.active_player();
        game.execute(&rules, Command::EndTurn { player }).unwrap();
    }
    let buddhism = ReligionId(
        rules
            .module.religions
            .iter()
            .position(|r| r.key == "core:religion/buddhism")
            .unwrap() as u16,
    );
    game.view()
        .auth
        .module()
        .religion()
        .city(neighbour)
        .pressure_of(classic_rules::world::religion::Creed::Religion(buddhism))
}

#[test]
fn the_grand_temple_doubles_the_pressure_of_its_city() {
    let plain = neighbour_pressure(false);
    assert!(plain > 0);
    assert_eq!(neighbour_pressure(true), 2 * plain);
}

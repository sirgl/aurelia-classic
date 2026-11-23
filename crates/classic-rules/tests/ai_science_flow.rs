//! `science-flow` (work/nets/science-flow/SCIENCE-FLOW.md): the science specialists' executor reads `net2`'s level and
//! keeps each city's food; the research agreement's ties lift `net2`'s denial for the friendship and the agreement
//! only; `standard` plays none of it. The tests name classic content only to set the scene.

use classic_content::core::core_rules;
use classic_rules::ai::citizens::science_specialist_commands;
use classic_rules::ai::net2::plan::{Help, Net2Plan, RivalRead, Science, Soldiers, WarKind};
use classic_rules::ai::variant::Variant;
use classic_rules::command::Command;
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, TechId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::world::relations::DealItem;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap() as u16)
}

/// One city of 8 citizens with a library and a university (2 scientist slots), every technology to the Renaissance.
fn scholar() -> (GameState, CompiledRuleset, CityHandle) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 40, height: 30, rng_seed: 5 });
    let renaissance = rules.eras.iter().find(|e| e.key == "core:era/renaissance").unwrap().index;
    for (i, tech) in rules.technologies.iter().enumerate() {
        if tech.content_era.and_then(|e| rules.eras.get(e.0 as usize)).map_or(0, |e| e.index) <= renaissance {
            game.debug_learn(&rules, P0, TechId(i as u16));
        }
    }
    let city = game.debug_found_city(&rules, P0, tile(&game, 5, 5));
    game.debug_found_city(&rules, P1, tile(&game, 30, 20));
    game.debug_set_population(&rules, city, 8);
    game.debug_add_building(&rules, city, building(&rules, "core:building/library"));
    game.debug_add_building(&rules, city, building(&rules, "core:building/university"));
    (game, rules, city)
}

/// The scientists the executor sets in the city at `level` with `food` a turn to spare (none: no command).
fn scientists(level: i64, food: i32) -> Option<u8> {
    let (game, rules, city) = scholar();
    let mut obs = game.ai_observation(&rules, P0);
    obs.own_cities.iter_mut().find(|c| c.handle == city).unwrap().income.food_surplus = food;
    let commands = science_specialist_commands(&obs, &rules, level, 2);
    let scientist = rules.specialists.iter().position(|s| s.key == "core:specialist/scientist").unwrap() as u16;
    commands.iter().find_map(|c| match c {
        Command::SetSpecialists { city: at, specialist, count, .. } if *at == city && specialist.0 == scientist => Some(*count),
        _ => None,
    })
}

/// The level fills its share of the slots while the food stays at the floor; a city at the floor takes none; level 0
/// leaves the rules' own assignment (no command).
#[test]
fn the_scientists_follow_the_level_and_keep_the_food() {
    assert_eq!(scientists(1000, 20), Some(2), "fed: both slots");
    assert_eq!(scientists(500, 20), Some(1), "half the level, half the slots");
    assert_eq!(scientists(1000, 2), None, "at the floor no tile's food can go");
    assert_eq!(scientists(0, 20), None, "no level: the rules assign");
}

fn read(rival: PlayerId, help: Help) -> RivalRead {
    RivalRead {
        rival,
        weak: 0,
        reach: 0,
        menace: 0,
        distrust: 0,
        prey: 0,
        runaway: 900,
        spoiler: 0,
        retake: 0,
        duty: 0,
        grabber: 0,
        block: 0,
        border: 0,
        brake: 0,
        war_value: 0,
        war_kind: WarKind::Prey,
        trust: 1000,
        help,
        soldiers: Soldiers::Unread,
    }
}

/// The runaway is denied every help; a tie takes the friendship and the agreement out of the denial, nothing else.
#[test]
fn a_tie_lifts_the_denial_for_the_friendship_and_the_agreement_only() {
    let mut plan = Net2Plan { rivals: vec![read(P1, Help::Deny)], ..Net2Plan::default() };
    for item in [DealItem::Friendship, DealItem::ResearchAgreement, DealItem::OpenBorders, DealItem::DefensivePact] {
        assert!(plan.denies_item(P1, item), "{item:?} denied without a tie");
    }
    plan.science = Science { ties: vec![P1], ..Science::default() };
    assert!(!plan.denies_item(P1, DealItem::Friendship) && !plan.denies_item(P1, DealItem::ResearchAgreement));
    assert!(plan.denies_item(P1, DealItem::OpenBorders) && plan.denies_item(P1, DealItem::DefensivePact));
}

/// `standard` and every other variant keep the science flow off; `science-flow` is `standard` with it.
#[test]
fn only_science_flow_plays_the_science_flow() {
    assert!(Variant::ALL.iter().all(|v| v.science_flow() == (*v == Variant::ScienceFlow)));
    assert_eq!(Variant::ScienceFlow.net2_parts(), Variant::Standard.net2_parts());
    assert_eq!(Variant::ScienceFlow.net2_offence(), Variant::Standard.net2_offence());
    assert_eq!(Net2Plan::default().science, Science::default());
}

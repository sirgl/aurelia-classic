//! The science levers of the rules priced by what they do for the player
//! (`ai::science_levers`, classic/docs/ai-science-levers.md): a policy by all
//! its effects, a new branch and a policy of a started one on one scale, a
//! specialist slot by its yields and great person points against the tile,
//! a research agreement by the beakers the rules pay. The code reads the
//! rules' data; the tests name classic content only to set the scene.

use classic_content::core::core_rules;
use classic_rules::ai::policy_branches::priced::PolicyMarket;
use classic_rules::ai::science_levers::{self as levers, ScienceLevers};
use classic_rules::ai::value::{PriceModel, Prices, SciencePricing};
use classic_rules::ai::variant::Variant;
use classic_rules::command::Command;
use classic_rules::ids::{BuildingKindId, CityHandle, PlayerId, PolicyId, TechId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::{PolicyBranchId, SpecialistId};
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

fn policy(rules: &CompiledRuleset, key: &str) -> PolicyId {
    PolicyId(rules.module.policies.iter().position(|p| p.key == key).unwrap() as u16)
}

fn branch(rules: &CompiledRuleset, key: &str) -> PolicyBranchId {
    PolicyBranchId(rules.module.policy_branches.iter().position(|b| b.key == key).unwrap() as u16)
}

fn specialist(rules: &CompiledRuleset, key: &str) -> SpecialistId {
    SpecialistId(rules.specialists.iter().position(|s| s.key == key).unwrap() as u16)
}

/// Every technology up to the classic Renaissance (the science branch's era).
fn learn_through_renaissance(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    let renaissance = rules.eras.iter().find(|e| e.key == "core:era/renaissance").unwrap().index;
    for (i, tech) in rules.technologies.iter().enumerate() {
        let era = tech.content_era.and_then(|e| rules.eras.get(e.0 as usize)).map_or(0, |e| e.index);
        if era <= renaissance {
            game.debug_learn(rules, player, TechId(i as u16));
        }
    }
}

/// Three cities of 8 citizens with libraries and universities (scientist
/// slots), every technology to the Renaissance; a rival far away.
fn scholars() -> (GameState, CompiledRuleset, Vec<CityHandle>) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 2, width: 40, height: 30, rng_seed: 5 });
    learn_through_renaissance(&mut game, &rules, P0);
    let cities: Vec<CityHandle> = [(5, 5), (11, 5), (5, 11)].iter().map(|(q, r)| game.debug_found_city(&rules, P0, tile(&game, *q, *r))).collect();
    game.debug_found_city(&rules, P1, tile(&game, 30, 20));
    for city in &cities {
        game.debug_set_population(&rules, *city, 8);
        game.debug_add_building(&rules, *city, building(&rules, "core:building/library"));
        game.debug_add_building(&rules, *city, building(&rules, "core:building/university"));
    }
    (game, rules, cities)
}

fn prices(game: &GameState, rules: &CompiledRuleset) -> Prices {
    let obs = game.ai_observation(rules, P0);
    Prices::of_effects(&obs, rules, PriceModel::Uses, SciencePricing::Reach, true)
}

/// The game's AI prices none of the levers; `science-levers` all, each
/// single-lever variant its own.
#[test]
fn the_game_ai_keeps_its_levers_and_the_variants_take_theirs() {
    assert_eq!(Variant::Standard.science_levers(), ScienceLevers::NONE);
    assert_eq!(Variant::ScienceLevers.science_levers(), ScienceLevers::ALL);
    assert!(Variant::SciencePolicies.science_levers().policies && !Variant::SciencePolicies.science_levers().specialists);
    assert!(Variant::ScienceSpecialists.science_levers().specialists && !Variant::ScienceSpecialists.science_levers().agreements);
    assert!(Variant::ScienceAgreements.science_levers().agreements && !Variant::ScienceAgreements.science_levers().policies);
    for v in [Variant::ScienceLevers, Variant::SciencePolicies, Variant::ScienceSpecialists, Variant::ScienceAgreements] {
        assert_eq!(Variant::from_name(v.name()), Some(v));
    }
}

/// The science branch's opener (a science percent while content), its
/// yield per specialist and its great scientist rate are worth what they
/// add to this empire: the percent on the empire's beakers, the yield on
/// the scientist slots it fills, the rate on the points they make. The
/// old aim weights saw none of the three (the rate was culture).
#[test]
fn the_science_branch_is_priced_by_what_its_policies_do() {
    let (game, rules, _) = scholars();
    let obs = game.ai_observation(&rules, P0);
    let prices = prices(&game, &rules);
    let memory = classic_rules::ai::AiState::new(P0, 0);
    let market = PolicyMarket::new(&obs, &rules, &memory, &prices, ScienceLevers::ALL);
    let opener = market.policy(policy(&rules, "core:policy/rationalism/opener"));
    let beakers: i64 = obs.own_cities.iter().map(|c| i64::from(c.income.science)).sum();
    assert!(beakers > 0);
    assert_eq!(opener, beakers * 10 / 100 * prices.science, "a tenth of the empire's beakers");
    let secularism = market.policy(policy(&rules, "core:policy/rationalism/secularism"));
    let slots: i64 = obs.own_cities.iter().map(|c| i64::from(levers::slots(&rules, c)[specialist(&rules, "core:specialist/scientist").0 as usize])).sum();
    assert!(slots > 0);
    assert_eq!(secularism, slots * 2 * prices.science, "two beakers on every scientist slot");
    let humanism = market.policy(policy(&rules, "core:policy/rationalism/humanism"));
    assert!(humanism > 0, "the scientists' points are worth their great scientist");
    // Without the specialist lever, only the citizens that work no tile count.
    let idle = PolicyMarket::new(&obs, &rules, &memory, &prices, ScienceLevers { specialists: false, ..ScienceLevers::ALL });
    assert!(idle.policy(policy(&rules, "core:policy/rationalism/secularism")) <= secularism);
}

/// A branch not started stands against a policy of a started branch on one
/// scale: the old order (any policy of a started branch first) adopts a
/// military policy at peace; the priced choice opens a branch, the science
/// branch worth more per step to an empire of scholars than any of them.
#[test]
fn a_new_branch_and_a_started_one_are_weighed_on_one_scale() {
    let (mut game, rules, _) = scholars();
    game.debug_grant_culture(P0, 100_000);
    game.execute(&rules, Command::UnlockPolicyBranch { player: P0, branch: branch(&rules, "core:policy/honor") }).unwrap();
    let obs = game.ai_observation(&rules, P0);
    let science = branch(&rules, "core:policy/rationalism");
    assert!(obs.culture.unlockable.contains(&science), "the science branch is open to the player");
    assert!(!obs.culture.adoptable.is_empty());
    let state = classic_rules::ai::AiState::new(P0, 0);
    let old = classic_rules::ai::policies::policy_command(&obs, &rules, &state);
    assert!(matches!(old, Some(Command::AdoptPolicy { .. })), "the old order: {old:?}");
    let prices = prices(&game, &rules);
    let market = PolicyMarket::new(&obs, &rules, &state, &prices, ScienceLevers::ALL);
    let honor = market.rest_step(branch(&rules, "core:policy/honor"));
    assert!(market.branch_step(science) > honor, "the science branch {} against the rest of the started one {honor}", market.branch_step(science));
    let new = classic_rules::ai::policies::priced_policy_command(&obs, &rules, &state, &prices, ScienceLevers::ALL);
    assert!(matches!(new, Some(Command::UnlockPolicyBranch { .. })), "the priced choice opens a branch: {new:?}");
}

/// A specialist slot is worth its yields and its points at the price of
/// the great person they bring: dear beakers take the citizens of a fed
/// city off their tiles into the scientist slots, cheap ones leave them on
/// the tiles, and a starving city keeps its citizens on their tiles.
#[test]
fn a_specialist_takes_a_citizen_off_a_tile_only_when_it_pays_more() {
    let (mut game, rules, cities) = scholars();
    game.debug_set_population(&rules, cities[0], 2);
    let obs = game.ai_observation(&rules, P0);
    let scientist = specialist(&rules, "core:specialist/scientist").0 as usize;
    let techs = classic_rules::derived::tile_yields::TechSet::of(&obs.economy.techs);
    let mut city = obs.own_cities.iter().find(|c| c.handle == cities[0]).unwrap().clone();
    // A city with food to spare (its farms are not the point here).
    city.income.food_surplus = 10;
    let slots = levers::slots(&rules, &city);
    assert!(slots[scientist] >= 2);
    let mut dear = prices(&game, &rules);
    dear.science = dear.food.max(dear.production).max(dear.gold) * 10;
    let points = levers::point_prices(&obs, &rules, &dear);
    let worth = levers::specialist_worth(&obs, &rules, &dear, &points, &city, SpecialistId(scientist as u16));
    let yields = levers::yields_worth(&dear, &levers::specialist_yields(&obs, &rules, &city, SpecialistId(scientist as u16)));
    assert!(worth > yields, "the points are worth something: {worth} against {yields}");
    let plan = classic_rules::ai::citizens::specialist_plan(&obs, &rules, &dear, &points, &techs, &city, &slots);
    assert_eq!(plan[scientist], 2, "dear beakers: both citizens are scientists");
    let mut cheap = dear;
    cheap.science = 0;
    let points = levers::point_prices(&obs, &rules, &cheap);
    let plan = classic_rules::ai::citizens::specialist_plan(&obs, &rules, &cheap, &points, &techs, &city, &slots);
    assert_eq!(plan[scientist], 0, "worthless beakers leave the citizens on their tiles");
    city.income.food_surplus = -1;
    let points = levers::point_prices(&obs, &rules, &dear);
    let plan = classic_rules::ai::citizens::specialist_plan(&obs, &rules, &dear, &points, &techs, &city, &slots);
    assert_eq!(plan[scientist], 0, "a starving city keeps its citizens on their tiles");
}

/// The rules pay the smaller of the two sciences over the pool divisor at
/// the player's percent; a partner's science is ours scaled by the public
/// scores. The agreement is worth its beakers at the beaker's price in gold,
/// less its cost.
#[test]
fn an_agreement_is_priced_by_the_pool_the_rules_pay() {
    let (game, rules, _) = scholars();
    let obs = game.ai_observation(&rules, P0);
    let ours = i64::from(obs.economy.income.science);
    assert!(ours > 0);
    let scores = vec![(P0, 200), (P1, 100)];
    assert_eq!(levers::partner_science(&obs, &scores, P1), ours / 2);
    assert_eq!(levers::agreement_beakers_per_turn(&obs, &scores, P1, 50), (ours / 2) * 50 / 300);
    let richer = vec![(P0, 100), (P1, 400)];
    assert_eq!(levers::agreement_beakers_per_turn(&obs, &richer, P1, 100), ours * 100 / 300, "the smaller science counts");
    let mut prices = prices(&game, &rules);
    prices.science = 0;
    assert_eq!(levers::agreement_gold(&obs, &rules, &prices, P1, 250), -250, "worthless beakers leave only the cost");
}

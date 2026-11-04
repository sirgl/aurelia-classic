//! Espionage effects on the classic ruleset (classic/docs/classic/rules.md): counter-espionage buildings, Industrial Espionage,
//! Double Agents, Covert Action, England's extra spy, the National
//! Intelligence Agency, the Great Firewall's immediate effect, Underground
//! Sect's spy pressure and the World Congress delegates of the Forbidden
//! Palace and Globalization.

use classic_rules::ClassicGame;
use classic_content::core::core_rules;
use classic_rules::ids::{BeliefId, BuildingKindId, CityHandle, PlayerId, PolicyId, TechId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::{CityStateId, ReligionId};
use classic_rules::rules::stats::EspionageStat;
use classic_rules::world::espionage::{
    detection, enemy_steal_percent, intel_rate, player_stat, rigged_votes, starting_rank, Detection, SpyState,
};
use classic_rules::world::player::PlayerKind;
use classic_rules::world::religion::Creed;
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, Command, CompiledRuleset, GameState};
use classic_rules::world::player::ClassicKind;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const CS: PlayerId = PlayerId(2);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap() as u16)
}

fn building(rules: &CompiledRuleset, key: &str) -> BuildingKindId {
    BuildingKindId(rules.buildings.iter().position(|b| b.key == key).unwrap() as u16)
}

fn policy(rules: &CompiledRuleset, key: &str) -> PolicyId {
    PolicyId(rules.module.policies.iter().position(|p| p.key == key).unwrap() as u16)
}

struct World {
    game: GameState,
    rules: CompiledRuleset,
    mine: CityHandle,
    theirs: CityHandle,
}

/// Two nations and a city-state, met; P0 enters the Renaissance.
fn world() -> World {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 3, width: 30, height: 20, rng_seed: 6 });
    game.debug_set_player_kind(CS, PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    let mine = game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    let theirs = game.debug_found_city(&rules, P1, tile(&game, 24, 15));
    game.debug_found_city(&rules, CS, tile(&game, 13, 9));
    game.debug_meet_all();
    game.debug_learn(&rules, P0, tech(&rules, "core:tech/acoustics"));
    World { game, rules, mine, theirs }
}

fn end_round(game: &mut GameState, rules: &CompiledRuleset) {
    let round = game.view().auth.turn().round;
    while game.view().auth.turn().round == round {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
}

fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    for _ in 0..4 {
        let active = game.view().auth.active_player();
        if active == player {
            return;
        }
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
    panic!("{player:?} never became active");
}

fn send(w: &mut World, city: CityHandle, diplomat: bool) {
    until_active(&mut w.game, &w.rules, P0);
    let order = Command::MoveSpy { player: P0, spy: 0, city: Some(city), diplomat };
    w.game.execute(&w.rules, order).unwrap();
}

fn spy(game: &GameState, player: PlayerId) -> classic_rules::world::espionage::Spy {
    game.view().auth.module().espionage().spies_of(player)[0]
}

/// P0's spy gathering intelligence in P1's city (P1 knows Agriculture).
fn spy_stealing() -> World {
    let mut w = world();
    let agriculture = tech(&w.rules, "core:tech/agriculture");
    w.game.debug_learn(&w.rules, P1, agriculture);
    w.game.debug_set_population(&w.rules, w.theirs, 6);
    end_round(&mut w.game, &w.rules);
    let theirs = w.theirs;
    send(&mut w, theirs, false);
    for _ in 0..10 {
        if spy(&w.game, P0).state == SpyState::GatheringIntel {
            return w;
        }
        end_round(&mut w.game, &w.rules);
    }
    panic!("the spy never started stealing");
}

#[test]
fn counter_espionage_buildings_slow_enemy_thieves_in_their_city_and_empire() {
    let mut w = world();
    let second = w.game.debug_found_city(&w.rules, P1, tile(&w.game, 18, 4));
    let percent = |w: &World, c| enemy_steal_percent(w.game.view().auth, &w.rules, c);
    for key in ["core:building/constabulary", "core:building/police_station"] {
        w.game.debug_add_building(&w.rules, w.theirs, building(&w.rules, key));
    }
    assert_eq!(percent(&w, w.theirs), -50, "Constabulary and Police Station: -25 each");
    assert_eq!(percent(&w, second), 0);
    w.game.debug_add_building(&w.rules, w.theirs, building(&w.rules, "core:building/great_firewall"));
    // The firewall's own city: -99 (Unciv: Buildings.json, Great Firewall),
    // plus its -25 in every city of the owner.
    assert_eq!(percent(&w, w.theirs), -50 - 99 - 25);
    assert_eq!(percent(&w, second), -25, "the Great Firewall: -25 in every city of its owner");
    w.game.debug_add_building(&w.rules, second, building(&w.rules, "core:building/national_intelligence_agency"));
    assert_eq!(percent(&w, second), -25 - 15);
    // The firewall's city: the rate falls to its floor of 1 (×1.25 for an Agent, truncated).
    let auth = w.game.view().auth;
    assert_eq!(intel_rate(auth, &w.rules, w.theirs, P0, 0), 1);
    assert_eq!(intel_rate(auth, &w.rules, w.theirs, P0, 1), 1);
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn industrial_espionage_raises_the_steal_rate_by_three_quarters() {
    let mut w = world();
    w.game.debug_set_population(&w.rules, w.theirs, 6);
    let rate = |w: &World| intel_rate(w.game.view().auth, &w.rules, w.theirs, P0, 0);
    let before = rate(&w);
    assert!(before > 1);
    w.game.debug_adopt_policy(&w.rules, P0, policy(&w.rules, "core:policy/autocracy/industrial-espionage"));
    assert_eq!(rate(&w), before * 175 / 100);
}

#[test]
fn double_agents_raise_the_detection_roll_by_three_quarters() {
    // Unciv: Spy.kt, roll 0..299 + 30 x (defender rank) - 30 x (thief
    // rank), killed from 200 (with or without a defender). A Recruit thief
    // against a Recruit counter-spy dies in a third of the rolls; Double
    // Agents (own effect) raise the defending roll by 75 %.
    let killed = |rank: Option<u8>, catch: i32| (0..300).filter(|r| detection(*r, 0, rank, catch) == Detection::Killed).count();
    assert_eq!(killed(Some(0), 0), 100);
    assert_eq!(killed(None, 0), 70, "no counter-spy: roll 230..299");
    assert_eq!(killed(Some(0), 75), 198);
    assert_eq!(killed(Some(2), 75), 258, "Special Agent + Double Agents");
    assert_eq!(killed(None, 75), 168);
    let undetected = (0..300).filter(|r| detection(*r, 0, None, 0) == Detection::Undetected).count();
    assert_eq!(undetected, 30, "10 % undetected for a Recruit without a counter-spy");
    let mut w = world();
    w.game.debug_adopt_policy(&w.rules, P1, policy(&w.rules, "core:policy/order/double-agents"));
    assert_eq!(player_stat(w.game.view().auth, &w.rules, P1, EspionageStat::CatchPercent), 75);
}

#[test]
fn covert_action_raises_the_votes_of_rigged_elections_by_three_quarters() {
    let mut w = world();
    w.game.debug_adopt_policy(&w.rules, P0, policy(&w.rules, "core:policy/freedom/covert-action"));
    let percent = player_stat(w.game.view().auth, &w.rules, P0, EspionageStat::RigElectionPercent);
    assert_eq!(percent, 75);
    assert_eq!(rigged_votes(15, percent), 26);
    assert_eq!(rigged_votes(15, 0), 15);
}

#[test]
fn england_gets_a_second_spy_with_its_first() {
    let mut w = world();
    let england = w.rules.nations.iter().position(|c| c.key == "core:nation/england").unwrap();
    w.game.debug_set_player_nation(P1, england as u8);
    end_round(&mut w.game, &w.rules);
    let count = |g: &GameState, p| g.view().auth.module().espionage().spies_of(p).len();
    assert_eq!((count(&w.game, P0), count(&w.game, P1)), (1, 2));
}

#[test]
fn the_national_intelligence_agency_gives_an_agent_and_raises_every_spy() {
    let mut w = world();
    end_round(&mut w.game, &w.rules);
    assert_eq!(spy(&w.game, P0).rank, 0);
    let nia = building(&w.rules, "core:building/national_intelligence_agency");
    w.game.debug_complete_building(&w.rules, w.mine, nia);
    let spies = w.game.view().auth.module().espionage().spies_of(P0).to_vec();
    assert_eq!(spies.len(), 2, "one more spy");
    assert_eq!((spies[0].rank, spies[1].rank), (1, 1), "the old spy rises, the new one starts as Agent");
    assert_eq!(starting_rank(w.game.view().auth, &w.rules, P0), 1);
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn the_great_firewall_slows_spies_already_stealing_at_once() {
    let mut w = spy_stealing();
    let rate = |w: &World| spy(&w.game, P0).rate;
    let before = rate(&w);
    assert!(before > 1, "rate {before}");
    // A Constabulary acts from the next theft on.
    w.game.debug_complete_building(&w.rules, w.theirs, building(&w.rules, "core:building/constabulary"));
    assert_eq!(rate(&w), before);
    w.game.debug_complete_building(&w.rules, w.theirs, building(&w.rules, "core:building/great_firewall"));
    assert_eq!(rate(&w), 1);
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn industrial_espionage_speeds_up_spies_already_stealing() {
    let mut w = spy_stealing();
    let before = spy(&w.game, P0).rate;
    w.game.debug_adopt_policy(&w.rules, P0, policy(&w.rules, "core:policy/autocracy/industrial-espionage"));
    assert_eq!(spy(&w.game, P0).rate, before * 175 / 100);
}

/// P0 founds a religion in its city and reforms it with Underground Sect.
fn found_underground_sect(w: &mut World) -> ReligionId {
    let rules = &w.rules;
    let belief = |key: &str| BeliefId(rules.module.beliefs.iter().position(|b| b.key == key).unwrap() as u16);
    let at = w.game.view().auth.cities().get(w.mine).unwrap().tile;
    let prophet = rules.units.iter().position(|u| u.key == "core:unit/great-prophet").unwrap();
    let prophet = w.game.debug_spawn_unit(rules, P0, at, UnitKindId(prophet as u16));
    until_active(&mut w.game, rules, P0);
    let found = Command::FoundReligion {
        unit: prophet,
        religion: ReligionId(0),
        founder: belief("core:belief/founder/tithe"),
        follower: belief("core:belief/follower/divine-inspiration"),
        pantheon: Some(belief("core:belief/pantheon/ancestor-worship")),
        bonus: None,
    };
    w.game.execute(rules, found).unwrap();
    w.game.debug_adopt_policy(rules, P0, policy(rules, "core:policy/piety/reformation"));
    let reform = Command::AddReformationBelief { player: P0, belief: belief("core:belief/reformation/underground-sect") };
    w.game.execute(rules, reform).unwrap();
    ReligionId(0)
}

#[test]
fn underground_sect_spies_press_the_founders_religion_where_they_are() {
    let mut w = world();
    end_round(&mut w.game, &w.rules);
    let religion = found_underground_sect(&mut w);
    let pressure = |w: &World| w.game.view().auth.module().religion().city(w.theirs).pressure_of(Creed::Religion(religion));
    end_round(&mut w.game, &w.rules);
    let idle = pressure(&w);
    end_round(&mut w.game, &w.rules);
    let per_round_without = pressure(&w) - idle;
    let theirs = w.theirs;
    send(&mut w, theirs, false);
    end_round(&mut w.game, &w.rules);
    let before = pressure(&w);
    end_round(&mut w.game, &w.rules);
    let per_round_with = pressure(&w) - before;
    assert_eq!(per_round_with - per_round_without, 60 * 5, "adjacent pressure × SpyPressure 5");
    w.game.validate(&w.rules).unwrap();
}

#[test]
fn the_forbidden_palace_and_globalization_add_delegates() {
    let mut w = world();
    end_round(&mut w.game, &w.rules);
    let delegates = |w: &World| classic_rules::world::congress::delegates(w.game.view().auth, &w.rules, P0);
    let base = delegates(&w);
    w.game.debug_add_building(&w.rules, w.mine, building(&w.rules, "core:building/forbidden_palace"));
    assert_eq!(delegates(&w), base + 2);
    w.game.debug_learn(&w.rules, P0, tech(&w.rules, "core:tech/globalization"));
    assert_eq!(delegates(&w), base + 2, "no diplomat yet");
    let theirs = w.theirs;
    send(&mut w, theirs, true);
    for _ in 0..8 {
        end_round(&mut w.game, &w.rules);
    }
    assert_eq!(spy(&w.game, P0).state, SpyState::Schmooze);
    assert_eq!(delegates(&w), base + 3, "a diplomat in a living nation's capital");
}

#[test]
fn the_ai_moves_a_thief_away_from_a_great_firewall() {
    let mut w = spy_stealing();
    until_active(&mut w.game, &w.rules, P0);
    let orders = classic_rules::ai::espionage::placement_orders(&w.game.ai_observation(&w.rules, P0), &w.rules);
    assert!(orders.is_empty(), "a normal theft goes on: {orders:?}");
    w.game.debug_complete_building(&w.rules, w.theirs, building(&w.rules, "core:building/great_firewall"));
    let observation = w.game.ai_observation(&w.rules, P0);
    assert!(observation.spies[0].theft_turns.unwrap() > classic_rules::ai::espionage::SLOW_THEFT_TURNS);
    let orders = classic_rules::ai::espionage::placement_orders(&observation, &w.rules);
    let home = Command::MoveSpy { player: P0, spy: 0, city: Some(w.mine), diplomat: false };
    assert_eq!(format!("{orders:?}"), format!("{:?}", vec![home]), "the thief guards home instead");
}

#[test]
fn the_ai_values_counter_espionage_by_the_science_it_protects() {
    let mut w = world();
    end_round(&mut w.game, &w.rules);
    w.game.debug_set_population(&w.rules, w.mine, 8);
    until_active(&mut w.game, &w.rules, P0);
    let constabulary = building(&w.rules, "core:building/constabulary");
    let library = building(&w.rules, "core:building/library");
    let observation = w.game.ai_observation(&w.rules, P0);
    let city = &observation.own_cities[0];
    assert!(city.income.science > 0);
    let value = |kind| classic_rules::ai::espionage::counter_espionage_value(&observation, &w.rules, city, kind);
    assert_eq!(value(constabulary), city.income.science * 10);
    assert_eq!(value(library), 0);
    assert_eq!(classic_rules::ai::espionage::own_counter_percent(&observation, &w.rules, city), 0);
    w.game.debug_add_building(&w.rules, w.mine, constabulary);
    let observation = w.game.ai_observation(&w.rules, P0);
    assert_eq!(classic_rules::ai::espionage::own_counter_percent(&observation, &w.rules, &observation.own_cities[0]), -25);
}

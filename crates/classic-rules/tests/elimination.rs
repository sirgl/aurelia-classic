//! An eliminated nation leaves no tie behind (classic/docs/classic/rules.md, Elimination): the
//! rules end, at the elimination, every tie their state keeps with it (`systems::elimination`).
//! One test per tie, then the guard: a played world where nothing names the dead nation after
//! its death (`world::references`).

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::variant::Variant;
use classic_rules::ai::AiSettings;
use classic_rules::ids::{CityHandle, PlayerId, TechId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::CityStateId;
use classic_rules::scenario::{generate_new_game_with, NewGameOptions, ScenarioConfig};
use classic_rules::world::barbarians::BarbarianMode;
use classic_rules::world::city_states::{QuestKind, Standing};
use classic_rules::world::player::{ClassicKind, ClassicPlayers, PlayerKind};
use classic_rules::world::references::references_to;
use classic_rules::world::relations::DealItem;
use classic_rules::world::victory::{VictoryKind, VictoryOptions};
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState};
use kernel::ai::driver::AiDriver;

/// The nation that dies.
const DEAD: PlayerId = PlayerId(0);
/// The nation tied to it.
const FRIEND: PlayerId = PlayerId(1);
/// The nation that takes its cities.
const KILLER: PlayerId = PlayerId(2);
const CS: PlayerId = PlayerId(3);

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn tech(rules: &CompiledRuleset, key: &str) -> TechId {
    TechId(rules.technologies.iter().position(|t| t.key == key).unwrap() as u16)
}

/// Three nations and a city-state, met, with one city each; domination is off, so the
/// game goes on after a death.
fn world() -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 4, width: 40, height: 20, rng_seed: 11 });
    game.debug_set_player_kind(CS, PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    game.debug_found_city(&rules, DEAD, tile(&game, 3, 3));
    game.debug_found_city(&rules, FRIEND, tile(&game, 20, 10));
    game.debug_found_city(&rules, KILLER, tile(&game, 36, 17));
    game.debug_found_city(&rules, CS, tile(&game, 12, 15));
    game.debug_meet_all();
    game.debug_set_victory_options(VictoryOptions { disabled: VictoryKind::Domination.bit(), ..VictoryOptions::default() });
    (game, rules)
}

/// `KILLER` takes every city of `victim`: the nation is gone.
fn eliminate(game: &mut GameState, rules: &CompiledRuleset, victim: PlayerId) {
    let cities: Vec<CityHandle> = game.view().auth.indexes().cities_of(victim).to_vec();
    for city in cities {
        game.debug_capture_city(rules, city, KILLER);
    }
    assert!(!game.view().auth.players().is_alive(victim), "{victim:?} is eliminated");
}

fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    for _ in 0..8 {
        let active = game.view().auth.active_player();
        if active == player {
            return;
        }
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
    panic!("{player:?} never became active");
}

fn end_round(game: &mut GameState, rules: &CompiledRuleset) {
    let round = game.view().auth.turn().round;
    while game.view().auth.turn().round == round {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
}

fn round(game: &GameState) -> u32 {
    game.view().auth.turn().round
}

/// The city-state allied to a nation that dies takes the living nation with the most influence
/// above the threshold as its ally (Unciv: `updateAllyCivForCityState` counts no defeated civ).
#[test]
fn a_city_state_whose_ally_dies_allies_with_the_next_living_nation() {
    let (mut game, rules) = world();
    game.debug_set_influence(&rules, CS, DEAD, 120);
    game.debug_set_influence(&rules, CS, FRIEND, 80);
    assert_eq!(game.view().auth.module().city_states().ally(CS), Some(DEAD));
    eliminate(&mut game, &rules, DEAD);
    end_round(&mut game, &rules);
    assert_eq!(game.view().auth.module().city_states().ally(CS), Some(FRIEND));
}

/// A dead nation keeps no influence or pledge with a city-state.
#[test]
fn a_dead_nation_keeps_no_standing_with_a_city_state() {
    let (mut game, rules) = world();
    game.debug_set_influence(&rules, CS, DEAD, 40);
    until_active(&mut game, &rules, DEAD);
    game.execute(&rules, Command::PledgeProtection { player: DEAD, city_state: CS, pledge: true }).unwrap();
    assert!(game.view().auth.module().city_states().standing(CS, DEAD).pledged.is_some());
    eliminate(&mut game, &rules, DEAD);
    assert_eq!(game.view().auth.module().city_states().standing(CS, DEAD), Standing::default());
}

/// The quests of a dead nation, and the quests that name it, end.
#[test]
fn quests_of_or_about_a_dead_nation_end() {
    let (mut game, rules) = world();
    game.debug_add_quest(CS, FRIEND, QuestKind::FindPlayer(DEAD), None);
    game.debug_add_quest(CS, FRIEND, QuestKind::DenounceMajor(DEAD), None);
    game.debug_add_quest(CS, DEAD, QuestKind::GiveGold { gifted: 0 }, None);
    eliminate(&mut game, &rules, DEAD);
    let quests = game.view().auth.module().city_states().quests();
    let names = |kind: QuestKind| matches!(kind, QuestKind::FindPlayer(p) | QuestKind::DenounceMajor(p) | QuestKind::BullyCityState(p) if p == DEAD);
    assert!(quests.iter().all(|q| q.major != DEAD && !names(q.kind)), "{quests:?}");
}

/// A defensive pact with a dead nation ends.
#[test]
fn a_defensive_pact_with_a_dead_nation_ends() {
    let (mut game, rules) = world();
    game.debug_deal_item(&rules, DEAD, FRIEND, DealItem::DefensivePact).unwrap();
    assert!(game.view().auth.module().relations().has_pact(FRIEND, DEAD, round(&game)));
    eliminate(&mut game, &rules, DEAD);
    assert!(!game.view().auth.module().relations().has_pact(FRIEND, DEAD, round(&game)));
}

/// A declaration of friendship with a dead nation ends.
#[test]
fn a_friendship_with_a_dead_nation_ends() {
    let (mut game, rules) = world();
    game.debug_deal_item(&rules, DEAD, FRIEND, DealItem::Friendship).unwrap();
    assert!(game.view().auth.module().relations().are_friends(FRIEND, DEAD, round(&game)));
    eliminate(&mut game, &rules, DEAD);
    assert!(!game.view().auth.module().relations().are_friends(FRIEND, DEAD, round(&game)));
}

/// Open borders with a dead nation close, both ways, in the rules and in the kernel.
#[test]
fn open_borders_with_a_dead_nation_close() {
    let (mut game, rules) = world();
    game.debug_deal_item(&rules, DEAD, FRIEND, DealItem::OpenBorders).unwrap();
    game.debug_deal_item(&rules, FRIEND, DEAD, DealItem::OpenBorders).unwrap();
    assert!(game.view().auth.diplomacy().grants_entry(FRIEND, DEAD));
    eliminate(&mut game, &rules, DEAD);
    let auth = game.view().auth;
    assert!(!auth.diplomacy().grants_entry(FRIEND, DEAD) && !auth.diplomacy().grants_entry(DEAD, FRIEND));
    let relations = auth.module().relations();
    assert_eq!((relations.open_borders_until(FRIEND, DEAD), relations.open_borders_until(DEAD, FRIEND)), (0, 0));
}

/// A research agreement with a dead nation ends, with the science it gathered.
#[test]
fn a_research_agreement_with_a_dead_nation_ends() {
    let (mut game, rules) = world();
    game.debug_deal_item(&rules, DEAD, FRIEND, DealItem::ResearchAgreement).unwrap();
    assert!(game.view().auth.module().relations().has_research_agreement(FRIEND, DEAD, round(&game)));
    eliminate(&mut game, &rules, DEAD);
    let relations = game.view().auth.module().relations();
    assert_eq!(relations.research_agreement_until(FRIEND, DEAD), 0);
    assert_eq!(relations.research_agreement_until(DEAD, FRIEND), 0);
}

/// Embassies with a dead nation close, both ways.
#[test]
fn embassies_with_a_dead_nation_close() {
    let (mut game, rules) = world();
    game.debug_deal_item(&rules, DEAD, FRIEND, DealItem::Embassy).unwrap();
    game.debug_deal_item(&rules, FRIEND, DEAD, DealItem::Embassy).unwrap();
    assert!(game.view().auth.module().relations().has_embassy(FRIEND, DEAD));
    eliminate(&mut game, &rules, DEAD);
    let relations = game.view().auth.module().relations();
    assert!(!relations.has_embassy(FRIEND, DEAD) && !relations.has_embassy(DEAD, FRIEND));
}

/// Gold per turn to or from a dead nation stops at once (it used to stop at the next turn end).
#[test]
fn gold_per_turn_with_a_dead_nation_stops_at_its_death() {
    let (mut game, rules) = world();
    game.debug_deal_item(&rules, DEAD, FRIEND, DealItem::GoldPerTurn(3)).unwrap();
    game.debug_deal_item(&rules, FRIEND, DEAD, DealItem::GoldPerTurn(2)).unwrap();
    assert_eq!(game.view().auth.module().relations().trades().len(), 2);
    eliminate(&mut game, &rules, DEAD);
    let trades = game.view().auth.module().relations().trades();
    assert!(trades.iter().all(|t| t.from != DEAD && t.to != DEAD), "{trades:?}");
}

/// An offer to or from a dead nation is withdrawn.
#[test]
fn offers_to_a_dead_nation_are_withdrawn() {
    let (mut game, rules) = world();
    for p in [DEAD, FRIEND] {
        game.debug_learn(&rules, p, tech(&rules, "core:tech/writing"));
    }
    until_active(&mut game, &rules, FRIEND);
    game.execute(&rules, Command::ProposeDeal { from: FRIEND, to: DEAD, items: vec![(FRIEND, DealItem::Embassy)] }).unwrap();
    assert_eq!(game.view().auth.module().relations().offers().len(), 1);
    eliminate(&mut game, &rules, DEAD);
    assert!(game.view().auth.module().relations().offers().is_empty());
}

/// A war with a dead nation ends, and nobody remembers it as a warmonger.
#[test]
fn a_war_with_a_dead_nation_ends_and_is_forgotten() {
    let (mut game, rules) = world();
    game.debug_force_war(&rules, DEAD, FRIEND);
    assert!(game.view().auth.diplomacy().at_war(FRIEND, DEAD));
    assert!(game.view().auth.module().relations().warmonger(FRIEND, DEAD) > 0);
    eliminate(&mut game, &rules, DEAD);
    let auth = game.view().auth;
    assert!(!auth.diplomacy().at_war(FRIEND, DEAD));
    assert_eq!(auth.module().relations().war_since(FRIEND, DEAD), 0);
    assert_eq!(auth.module().relations().warmonger(FRIEND, DEAD), 0);
}

/// A denunciation of a dead nation ends.
#[test]
fn a_denunciation_of_a_dead_nation_ends() {
    let (mut game, rules) = world();
    until_active(&mut game, &rules, FRIEND);
    game.execute(&rules, Command::Denounce { player: FRIEND, target: DEAD }).unwrap();
    assert!(game.view().auth.module().relations().denounced(FRIEND, DEAD, round(&game)));
    eliminate(&mut game, &rules, DEAD);
    assert!(!game.view().auth.module().relations().denounced(FRIEND, DEAD, round(&game)));
}

/// A dead nation is no longer the World Leader.
#[test]
fn a_dead_nation_loses_the_world_leader_title() {
    let (mut game, rules) = world();
    game.debug_set_world_leader(Some(DEAD));
    eliminate(&mut game, &rules, DEAD);
    assert_eq!(game.view().auth.module().victory().world_leader(), None);
}

/// The votes a dead nation cast and its delegates leave the open session.
#[test]
fn the_votes_and_delegates_of_a_dead_nation_leave_the_session() {
    let (mut game, rules) = world();
    game.debug_learn(&rules, FRIEND, tech(&rules, "core:tech/printing-press"));
    end_round(&mut game, &rules);
    assert!(game.view().auth.module().congress().founded());
    for i in 0..rules.technologies.len() {
        game.debug_learn(&rules, FRIEND, TechId(i as u16));
    }
    for _ in 0..6 {
        if game.view().auth.module().congress().in_session() {
            break;
        }
        end_round(&mut game, &rules);
    }
    assert!(game.view().auth.module().congress().in_session());
    until_active(&mut game, &rules, DEAD);
    let delegates = game.view().auth.module().congress().delegates_left(DEAD);
    game.execute(&rules, Command::CastVotes { player: DEAD, proposal: 0, choice: u32::from(FRIEND.0), delegates }).unwrap();
    eliminate(&mut game, &rules, DEAD);
    let congress = game.view().auth.module().congress();
    assert!(congress.votes().iter().all(|v| v.voter != DEAD), "{:?}", congress.votes());
    assert_eq!(congress.delegates_of(DEAD), 0);
}

/// A dead nation's spies leave the cities they work in (Unciv: `removeAllSpies` at `destroy`).
#[test]
fn the_spies_of_a_dead_nation_are_gone() {
    let (mut game, rules) = world();
    game.debug_learn(&rules, DEAD, tech(&rules, "core:tech/acoustics"));
    end_round(&mut game, &rules);
    let theirs = game.view().auth.indexes().cities_of(FRIEND)[0];
    until_active(&mut game, &rules, DEAD);
    game.execute(&rules, Command::MoveSpy { player: DEAD, spy: 0, city: Some(theirs), diplomat: false }).unwrap();
    eliminate(&mut game, &rules, DEAD);
    assert!(game.view().auth.module().espionage().spies_of(DEAD).is_empty());
}

/// A dead city-state is at war with nobody and keeps no standing with anyone.
#[test]
fn a_dead_city_state_keeps_no_war_or_standing() {
    let (mut game, rules) = world();
    game.debug_set_influence(&rules, CS, KILLER, 30);
    game.debug_force_war(&rules, FRIEND, CS);
    eliminate(&mut game, &rules, CS);
    let auth = game.view().auth;
    assert!(!auth.diplomacy().at_war(FRIEND, CS));
    assert_eq!(auth.module().city_states().standing(CS, KILLER), Standing::default());
}

/// The guard of the class: a played world, every kind of tie with one nation, then its death:
/// nothing in the state names it (`world::references`), then or 30 rounds later.
#[test]
fn nothing_names_a_dead_nation_in_a_played_world() {
    let rules = compile_rules(core_rules()).unwrap();
    let config = ScenarioConfig { seed: 0xDEAD_5EED, players: 4, width: 40, height: 28, cities: 0, units: 0, water_ratio: 15 };
    let victories = VictoryOptions { disabled: VictoryKind::Domination.bit(), ..VictoryOptions::default() };
    let options = NewGameOptions { city_states: 3, barbarians: BarbarianMode::Normal, victories, ..NewGameOptions::default() };
    let mut game = generate_new_game_with(&rules, &config, None, options);
    let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
    let mut driver = AiDriver::with_mind(settings, Vec::new(), ClassicMind::new(Variant::Standard));
    play(&mut game, &rules, &mut driver, 80);
    assert!(game.view().auth.turn().round >= 80);
    let players = game.view().auth.players();
    let alive = |p: PlayerId| players.is_alive(p) && players.is_major(p);
    let (dead, friend, killer) = (PlayerId(0), PlayerId(1), PlayerId(2));
    assert!(alive(dead) && alive(friend) && alive(killer), "three nations live after 80 rounds");
    tie_every_way(&mut game, &rules, dead, friend, killer);
    assert!(!references_to(game.view().auth, dead).is_empty(), "the search finds the ties");

    let cities: Vec<CityHandle> = game.view().auth.indexes().cities_of(dead).to_vec();
    for city in cities {
        game.debug_capture_city(&rules, city, killer);
    }
    assert!(!game.view().auth.players().is_alive(dead));
    assert_eq!(references_to(game.view().auth, dead), Vec::<String>::new(), "at the death");
    play(&mut game, &rules, &mut driver, 30);
    assert_eq!(references_to(game.view().auth, dead), Vec::<String>::new(), "30 rounds later");
    game.validate(&rules).unwrap();
}

fn play(game: &mut GameState, rules: &CompiledRuleset, driver: &mut AiDriver<classic_rules::classic::Classic, ClassicMind>, rounds: u32) {
    let mut done = 0;
    while done < rounds {
        if driver.run(game, rules, 100).round_done {
            done += 1;
        }
    }
}

/// Every tie the harness can make between `dead` and `friend`, `dead` as the ally of every
/// city-state and the World Leader, and `dead` at war with `killer`, its friend before (a
/// betrayal everyone holds against it).
fn tie_every_way(game: &mut GameState, rules: &CompiledRuleset, dead: PlayerId, friend: PlayerId, killer: PlayerId) {
    let _ = game.debug_deal_item(rules, dead, killer, DealItem::Friendship);
    game.debug_force_war(rules, dead, killer);
    for item in [DealItem::DefensivePact, DealItem::Friendship, DealItem::ResearchAgreement, DealItem::GoldPerTurn(1)] {
        let _ = game.debug_deal_item(rules, dead, friend, item);
    }
    for (giver, receiver) in [(dead, friend), (friend, dead)] {
        let _ = game.debug_deal_item(rules, giver, receiver, DealItem::Embassy);
        let _ = game.debug_deal_item(rules, giver, receiver, DealItem::OpenBorders);
    }
    let players = game.view().auth.players().count();
    for i in 0..players {
        let city_state = PlayerId(i as u8);
        if game.view().auth.players().is_city_state(city_state) && game.view().auth.players().is_alive(city_state) {
            game.debug_set_influence(rules, city_state, dead, 200);
            game.debug_add_quest(city_state, friend, QuestKind::FindPlayer(dead), None);
        }
    }
    game.debug_set_world_leader(Some(dead));
    let offer_from = game.view().auth.active_player();
    if offer_from != dead {
        let _ = game.execute(rules, Command::ProposeDeal { from: offer_from, to: dead, items: vec![(offer_from, DealItem::Embassy)] });
    }
}

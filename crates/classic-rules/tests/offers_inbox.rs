//! The inbox of offers (classic/docs/classic/rules.md, Deals): an offer to a human waits for the
//! answer through the rounds until its term ends, and leaves the list with its reason when it
//! cannot be answered any more. An offer to an AI lives one round, as before (the AI answers at the
//! start of its turn), so the games of AI against AI are unchanged.

use classic_content::core::core_rules;
use classic_rules::classic::ClassicChange;
use classic_rules::command::CommandError;
use classic_rules::ids::{PlayerId, TerrainId, TileId, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::persistence::{decode::decode, encode::encode};
use classic_rules::world::relations::{DealItem, OfferLapse, PeaceRefusal};
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState, INBOX_ROUNDS, MIN_WAR_ROUNDS_BEFORE_PEACE, OFFER_ROUNDS};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);
const GRASSLAND: u16 = 1;

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn kind(rules: &CompiledRuleset, key: &str) -> UnitKindId {
    UnitKindId(rules.units.iter().position(|u| u.key == key).unwrap_or_else(|| panic!("{key}")) as u16)
}

/// Open grassland, three nations that met. P0 and P1 have a city each; P2 only a scout.
fn world(rules: &CompiledRuleset) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 3, width: 44, height: 20, rng_seed: 31 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    game.debug_found_city(rules, P0, tile(&game, 5, 10));
    game.debug_found_city(rules, P1, tile(&game, 13, 10));
    game.debug_spawn_unit(rules, P2, tile(&game, 40, 3), kind(rules, "core:unit/scout"));
    game.debug_meet_all();
    game
}

/// Every player ends its turn, `count` rounds; what the journal said about lapsed offers.
fn rounds(game: &mut GameState, rules: &CompiledRuleset, count: u32) -> Vec<ClassicChange> {
    let mut lapsed = Vec::new();
    for _ in 0..count {
        let start = game.view().auth.turn().round;
        while game.view().auth.turn().round == start {
            let active = game.view().auth.active_player();
            let result = game.execute(rules, Command::EndTurn { player: active }).unwrap();
            lapsed.extend(result.changes().module_changes.iter().filter(|c| matches!(c, ClassicChange::OfferLapsed { .. })).copied());
        }
    }
    lapsed
}

fn until_active(game: &mut GameState, rules: &CompiledRuleset, player: PlayerId) {
    while game.view().auth.active_player() != player {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
}

/// P0 at war with P1 for the rounds a war lasts before peace; P0 is active.
fn old_war(rules: &CompiledRuleset) -> GameState {
    let mut game = world(rules);
    game.execute(rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    rounds(&mut game, rules, MIN_WAR_ROUNDS_BEFORE_PEACE);
    until_active(&mut game, rules, P0);
    game
}

fn white_peace(from: PlayerId, to: PlayerId) -> Command {
    Command::ProposeDeal { from, to, items: vec![(from, DealItem::Peace)] }
}

fn waiting(game: &GameState) -> usize {
    game.view().auth.module().relations().offers().len()
}

fn first_offer(game: &GameState) -> u32 {
    game.view().auth.module().relations().offers()[0].id
}

fn at_war(game: &GameState, a: PlayerId, b: PlayerId) -> bool {
    game.view().auth.diplomacy().at_war(a, b)
}

fn answer(player: PlayerId, offer: u32, accept: bool) -> Command {
    Command::RespondDeal { player, offer, accept, why: PeaceRefusal::Unsaid }
}

/// The offer is not gone at the end of the round it was made: it stays for many rounds and the
/// receiver accepts it late.
#[test]
fn an_offer_to_a_human_survives_the_round_and_is_accepted_late() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = old_war(&rules);
    game.execute(&rules, white_peace(P0, P1)).unwrap();
    let lapsed = rounds(&mut game, &rules, INBOX_ROUNDS - 2);
    assert!(lapsed.is_empty(), "{lapsed:?}");
    assert_eq!(waiting(&game), 1, "still in the inbox");
    until_active(&mut game, &rules, P1);
    game.execute(&rules, answer(P1, first_offer(&game), true)).unwrap();
    assert!(!at_war(&game, P0, P1), "accepted late: peace");
    assert_eq!(waiting(&game), 0);
}

/// The term ends: the offer leaves the list and the journal says it expired.
#[test]
fn an_offer_expires_with_its_term() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = old_war(&rules);
    game.execute(&rules, white_peace(P0, P1)).unwrap();
    let lapsed = rounds(&mut game, &rules, INBOX_ROUNDS + 1);
    assert_eq!(waiting(&game), 0);
    assert_eq!(lapsed, [ClassicChange::OfferLapsed { from: P0, to: P1, peace: false, reason: OfferLapse::Expired }]);
}

/// A war began between the two: an offer that is not a peace is off.
#[test]
fn a_war_takes_the_offer_off_the_list() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules);
    let writing = classic_rules::ids::TechId(rules.technologies.iter().position(|t| t.key == "core:tech/writing").unwrap() as u16);
    game.debug_learn(&rules, P0, writing);
    game.debug_learn(&rules, P1, writing);
    until_active(&mut game, &rules, P1);
    let embassies = vec![(P1, DealItem::Embassy), (P0, DealItem::Embassy)];
    game.execute(&rules, Command::ProposeDeal { from: P1, to: P0, items: embassies }).unwrap();
    assert_eq!(waiting(&game), 1);
    until_active(&mut game, &rules, P0);
    game.execute(&rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    let lapsed = rounds(&mut game, &rules, 1);
    assert_eq!(waiting(&game), 0);
    assert_eq!(lapsed, [ClassicChange::OfferLapsed { from: P1, to: P0, peace: false, reason: OfferLapse::WarBegan }]);
}

/// The terms cannot be met any more: the peace of the first offer is made by another one.
#[test]
fn terms_that_cannot_be_met_take_the_offer_off_the_list() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = old_war(&rules);
    until_active(&mut game, &rules, P1);
    game.execute(&rules, white_peace(P1, P0)).unwrap();
    until_active(&mut game, &rules, P0);
    game.execute(&rules, white_peace(P0, P1)).unwrap();
    let second = game.view().auth.module().relations().offers()[1].id;
    until_active(&mut game, &rules, P1);
    game.execute(&rules, answer(P1, second, true)).unwrap();
    assert!(!at_war(&game, P0, P1));
    let lapsed = rounds(&mut game, &rules, 1);
    assert_eq!(waiting(&game), 0);
    assert_eq!(lapsed, [ClassicChange::OfferLapsed { from: P1, to: P0, peace: false, reason: OfferLapse::NoLongerPossible }]);
}

/// The proposer is gone: its offer is off with that reason.
#[test]
fn a_nation_that_perishes_takes_its_offer_with_it() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules);
    game.execute(&rules, Command::DeclareWar { player: P0, enemy: P2 }).unwrap();
    rounds(&mut game, &rules, MIN_WAR_ROUNDS_BEFORE_PEACE);
    until_active(&mut game, &rules, P2);
    game.execute(&rules, white_peace(P2, P0)).unwrap();
    let scout = game.view().auth.units().iter_handles().find(|(_, core)| core.owner == P2).map(|(handle, _)| handle).unwrap();
    game.debug_remove_unit(&rules, scout);
    let lapsed = rounds(&mut game, &rules, 1);
    assert!(!game.view().auth.players().is_alive(P2), "the last unit was its only holding");
    assert_eq!(waiting(&game), 0);
    assert_eq!(lapsed, [ClassicChange::OfferLapsed { from: P2, to: P0, peace: false, reason: OfferLapse::NationGone }]);
}

/// The AI answers at the start of its turn: its inbox keeps an offer one round, as before.
#[test]
fn an_offer_to_an_ai_lives_one_round() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = old_war(&rules);
    game.configure_players(rules.default_difficulty(), &[P1]);
    let round = game.view().auth.turn().round;
    game.execute(&rules, white_peace(P0, P1)).unwrap();
    let offer = &game.view().auth.module().relations().offers()[0];
    assert_eq!(offer.expires, round + OFFER_ROUNDS);
    let to_human = game.view().auth.module().relations().offers().len();
    assert_eq!(to_human, 1);
}

/// The same offer is not queued twice; a different one is.
#[test]
fn the_same_offer_waits_once() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = old_war(&rules);
    game.execute(&rules, white_peace(P0, P1)).unwrap();
    let again = game.execute(&rules, white_peace(P0, P1)).unwrap_err();
    assert_eq!(again, CommandError::RuleViolation("the same offer is waiting for an answer".into()));
    assert_eq!(waiting(&game), 1);
}

/// What waits in the inbox is saved with its term and read back the same.
#[test]
fn the_inbox_is_saved_and_loaded() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = old_war(&rules);
    game.execute(&rules, white_peace(P0, P1)).unwrap();
    rounds(&mut game, &rules, 3);
    let before = game.view().auth.module().relations().offers().to_vec();
    assert_eq!(before.len(), 1);
    let loaded = decode(&encode(&game, &rules), &rules).unwrap();
    assert_eq!(loaded.view().auth.module().relations().offers(), before);
    let mut loaded = loaded;
    let lapsed = rounds(&mut loaded, &rules, INBOX_ROUNDS);
    assert_eq!(lapsed.len(), 1, "the loaded offer lapses by its own term");
}

/// The AI does not send a second offer to a nation while the first waits in its inbox.
#[test]
fn the_ai_does_not_repeat_an_offer_that_waits() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules);
    for p in [P0, P1, P2] {
        for key in ["core:tech/writing", "core:tech/civil-service", "core:tech/chivalry", "core:tech/education"] {
            let tech = classic_rules::ids::TechId(rules.technologies.iter().position(|t| t.key == key).unwrap() as u16);
            game.debug_learn(&rules, p, tech);
        }
    }
    let obs = game.ai_observation(&rules, P0);
    let first = classic_rules::ai::diplomacy::proposals(&obs, &rules);
    let to_p1 = |commands: &[Command]| commands.iter().filter(|c| matches!(c, Command::ProposeDeal { to, .. } if *to == P1)).count();
    assert!(to_p1(&first) > 0, "the AI proposes to a nation it has no offer waiting at");
    drop(obs);
    for command in first {
        game.execute(&rules, command).unwrap();
    }
    let obs = game.ai_observation(&rules, P0);
    assert!(obs.waiting_on(P1));
    let second = classic_rules::ai::diplomacy::proposals(&obs, &rules);
    assert_eq!(to_p1(&second), 0, "{second:?}");
}

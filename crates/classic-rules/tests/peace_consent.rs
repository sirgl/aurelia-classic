//! Peace needs the other side's consent (classic/docs/classic/rules.md): between two nations
//! peace is a deal with the peace item that the other side accepts, never one side's
//! `MakePeace`; with a city-state either side makes it alone, as in the original game. The AI
//! answers an offer of peace by what going on with the war promises it (`ai::peace`) and ends
//! its own wars with nations by offers only.

use classic_content::core::core_rules;
use classic_rules::ai::driver::ClassicMind;
use classic_rules::ai::state::{Composition, Operation, OperationPhase, SeenCity};
use classic_rules::ai::variant::Variant;
use classic_rules::ai::{AiSettings, UnitTask};
use classic_rules::command::CommandError;
use classic_rules::ids::{PlayerId, TerrainId, TileId, UnitHandle, UnitKindId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::CityStateId;
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::world::relations::{DealBlock, DealItem, PeaceRefusal};
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameConfig, GameState, MIN_WAR_ROUNDS_BEFORE_PEACE};
use kernel::ai::driver::AiDriver;

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

fn spawn(game: &mut GameState, rules: &CompiledRuleset, owner: PlayerId, q: i32, r: i32, key: &str) -> UnitHandle {
    let at = tile(game, q, r);
    game.debug_spawn_unit(rules, owner, at, kind(rules, key))
}

/// Open grassland, three nations with a city each, all met; P2 is a city-state when
/// `city_state` (the first one of the rules).
fn world(rules: &CompiledRuleset, city_state: bool) -> GameState {
    let mut game = GameState::new(rules, GameConfig { players: 3, width: 44, height: 20, rng_seed: 31 });
    let tiles = game.view().auth.map().dimensions.tile_count() as u32;
    let land: Vec<(TileId, TerrainId)> = (0..tiles).map(|t| (TileId(t), TerrainId(GRASSLAND))).collect();
    game.debug_set_terrain_batch(rules, &land);
    if city_state {
        game.debug_set_player_kind(P2, PlayerKind::Module(ClassicKind::CityState(CityStateId(0))));
    }
    game.debug_found_city(rules, P0, tile(&game, 5, 10));
    let city = game.debug_found_city(rules, P1, tile(&game, 13, 10));
    game.debug_set_population(rules, city, 6);
    game.debug_found_city(rules, P2, tile(&game, 40, 3));
    game.debug_meet_all();
    game
}

/// Every player ends its turn `rounds` times (the war ages).
fn rounds(game: &mut GameState, rules: &CompiledRuleset, rounds: u32) {
    for _ in 0..rounds {
        for p in [P0, P1, P2] {
            game.execute(rules, Command::EndTurn { player: p }).unwrap();
        }
    }
}

/// P0 at war with P1 for the ten rounds a war lasts before peace; P0 is active.
fn old_war(rules: &CompiledRuleset) -> GameState {
    let mut game = world(rules, false);
    game.execute(rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    rounds(&mut game, rules, MIN_WAR_ROUNDS_BEFORE_PEACE);
    game
}

fn at_war(game: &GameState, a: PlayerId, b: PlayerId) -> bool {
    game.view().auth.diplomacy().at_war(a, b)
}

fn white_peace(from: PlayerId, to: PlayerId) -> Command {
    Command::ProposeDeal { from, to, items: vec![(from, DealItem::Peace)] }
}

fn next_offer(game: &GameState) -> u32 {
    game.view().auth.module().relations().next_offer_id()
}

/// `MakePeace` with a nation is refused with its reason, in a young war and in an old one:
/// the war goes on.
#[test]
fn peace_with_a_nation_alone_is_refused_with_its_reason() {
    let rules = compile_rules(core_rules()).unwrap();
    let consent = CommandError::RuleViolation(DealBlock::PeaceNeedsConsent.reason().into());
    let mut game = world(&rules, false);
    game.execute(&rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    assert_eq!(game.execute(&rules, Command::MakePeace { player: P0, enemy: P1 }).unwrap_err(), consent, "a young war");
    rounds(&mut game, &rules, MIN_WAR_ROUNDS_BEFORE_PEACE);
    assert_eq!(game.execute(&rules, Command::MakePeace { player: P0, enemy: P1 }).unwrap_err(), consent, "an old war");
    assert!(at_war(&game, P0, P1));
}

/// The peace is an offer the enemy answers: refused, the war goes on; accepted, it ends with
/// the treaty. The ten rounds of war hold for the offer.
#[test]
fn peace_with_a_nation_is_a_deal_it_accepts() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, false);
    game.execute(&rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    let young = game.execute(&rules, white_peace(P0, P1)).unwrap_err();
    assert_eq!(young, CommandError::RuleViolation(DealBlock::PeaceTooSoon.reason().into()));
    rounds(&mut game, &rules, MIN_WAR_ROUNDS_BEFORE_PEACE);
    let first = next_offer(&game);
    game.execute(&rules, white_peace(P0, P1)).unwrap();
    assert!(at_war(&game, P0, P1), "an offer is no peace");
    game.execute(&rules, Command::RespondDeal { player: P1, offer: first, accept: false, why: PeaceRefusal::Unsaid }).unwrap();
    assert!(at_war(&game, P0, P1), "refused: the war goes on");
    let second = next_offer(&game);
    game.execute(&rules, white_peace(P0, P1)).unwrap();
    game.execute(&rules, Command::RespondDeal { player: P1, offer: second, accept: true, why: PeaceRefusal::Unsaid }).unwrap();
    assert!(!at_war(&game, P0, P1), "accepted: peace");
    let round = game.view().auth.turn().round;
    assert!(game.view().auth.module().relations().peace_treaty_until(P0, P1) > round, "with its treaty");
}

/// Two humans: the offer waits for the receiver's answer through the turns, the war goes on
/// meanwhile, and the receiver's acceptance on its own turn makes the peace.
#[test]
fn between_two_humans_the_offer_waits_for_the_answer() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = old_war(&rules);
    let offer = next_offer(&game);
    game.execute(&rules, white_peace(P0, P1)).unwrap();
    game.execute(&rules, Command::EndTurn { player: P0 }).unwrap();
    assert_eq!(game.view().auth.active_player(), P1);
    let waiting = game.ai_observation(&rules, P1).offers_to_us.iter().any(|o| o.id == offer && o.from == P0);
    assert!(waiting, "the receiver sees the offer on its turn");
    assert!(at_war(&game, P0, P1), "nothing happens without the answer");
    game.execute(&rules, Command::RespondDeal { player: P1, offer, accept: true, why: PeaceRefusal::Unsaid }).unwrap();
    assert!(!at_war(&game, P0, P1));
}

/// Unanswered, the offer lapses (after `deals::OFFER_ROUNDS`, one round) and the war goes on:
/// nobody's silence makes peace.
#[test]
fn an_unanswered_offer_lapses_and_the_war_goes_on() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = old_war(&rules);
    game.execute(&rules, white_peace(P0, P1)).unwrap();
    rounds(&mut game, &rules, 2);
    assert!(game.view().auth.module().relations().offers().is_empty(), "the offer lapsed");
    assert!(at_war(&game, P0, P1));
}

/// With a city-state peace is one side's act, as before: at once, without the ten rounds.
#[test]
fn peace_with_a_city_state_is_made_alone_as_before() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, true);
    game.execute(&rules, Command::DeclareWar { player: P0, enemy: P2 }).unwrap();
    game.execute(&rules, Command::MakePeace { player: P0, enemy: P2 }).unwrap();
    assert!(!at_war(&game, P0, P2));
}

/// Two players cannot sign a peace for a third nation: «peace with» a nation in a deal is
/// refused; with a city-state it is allowed, as before.
#[test]
fn nobody_signs_a_peace_for_a_third_nation() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = world(&rules, false);
    game.execute(&rules, Command::DeclareWar { player: P0, enemy: P2 }).unwrap();
    rounds(&mut game, &rules, MIN_WAR_ROUNDS_BEFORE_PEACE);
    let deal = Command::ProposeDeal { from: P0, to: P1, items: vec![(P0, DealItem::ThirdPartyPeace(P2))] };
    let refused = game.execute(&rules, deal).unwrap_err();
    assert_eq!(refused, CommandError::RuleViolation(DealBlock::CannotMakePeace.reason().into()));
    let mut game = world(&rules, true);
    game.execute(&rules, Command::DeclareWar { player: P0, enemy: P2 }).unwrap();
    let deal = Command::ProposeDeal { from: P0, to: P1, items: vec![(P0, DealItem::ThirdPartyPeace(P2))] };
    game.execute(&rules, deal).expect("peace with a city-state in a deal");
}

/// The AI's answer to `items` offered by the enemy (a human) on the AI's turn: what it
/// answered (accepted or not) and what it said (`PeaceRefusal`), if it answered.
fn ask(game: &mut GameState, rules: &CompiledRuleset, ai: PlayerId, enemy: PlayerId, items: Vec<(PlayerId, DealItem)>, prepare: impl FnOnce(&mut GameState, &mut AiDriver<classic_rules::classic::Classic, ClassicMind>)) -> Option<(bool, PeaceRefusal)> {
    while game.view().auth.active_player() != enemy {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
    game.execute(rules, Command::ProposeDeal { from: enemy, to: ai, items }).unwrap();
    let settings = AiSettings::for_rules(rules, rules.default_difficulty());
    let humans: Vec<PlayerId> = [P0, P1, P2].into_iter().filter(|p| *p != ai).collect();
    let mut driver = AiDriver::with_mind(settings, humans, ClassicMind::new(Variant::Standard));
    driver.set_trace_capacity(100_000);
    prepare(game, &mut driver);
    while game.view().auth.active_player() != ai {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
    driver.run(game, rules, u32::MAX);
    let answer = driver.trace().entries().find_map(|e| match e.command {
        Some(Command::RespondDeal { accept, why, .. }) => Some((accept, why)),
        _ => None,
    });
    answer
}

/// The AI's answer to white peace offered by the enemy (a human) on the AI's turn: what it
/// accepted, if it answered.
fn answer(game: &mut GameState, rules: &CompiledRuleset, ai: PlayerId, enemy: PlayerId, prepare: impl FnOnce(&mut GameState, &mut AiDriver<classic_rules::classic::Classic, ClassicMind>)) -> Option<bool> {
    ask(game, rules, ai, enemy, vec![(enemy, DealItem::Peace)], prepare).map(|(accept, _)| accept)
}

/// P0 (the AI) has an army of an operation next to the city of six of P1 (the human): it is
/// winning the war.
fn winning_war(rules: &CompiledRuleset) -> (GameState, impl FnOnce(&mut GameState, &mut AiDriver<classic_rules::classic::Classic, ClassicMind>)) {
    let mut game = old_war(rules);
    let army: Vec<UnitHandle> = [9, 10, 11].into_iter().map(|r| spawn(&mut game, rules, P0, 12, r, "core:unit/swordsman")).collect();
    let seen: Vec<TileId> = (8..=18).flat_map(|q| (6..=14).map(move |r| (q, r))).map(|(q, r)| tile(&game, q, r)).collect();
    game.debug_reveal(P0, &seen);
    let (target, muster) = (tile(&game, 13, 10), tile(&game, 5, 10));
    let members: Vec<_> = army.iter().map(|u| game.view().auth.units().get(*u).unwrap().core.id).collect();
    let prepare = move |game: &mut GameState, driver: &mut AiDriver<classic_rules::classic::Classic, ClassicMind>| {
        let round = game.view().auth.turn().round;
        let memory = driver.state_mut(P0, round);
        // The army is the operation's (without its members the AI prices the army it still lacks).
        for unit in members {
            memory.set_task(unit, UnitTask::Operation { target }, round);
        }
        memory.see_city(SeenCity { tile: target, owner: P1, population: 6, last_seen_round: round });
        memory.start_operation(Operation {
            target,
            enemy: P1,
            muster,
            need: Composition { melee: 3, ranged: 0, siege: 0 },
            phase: OperationPhase::Marching,
            since_round: round,
            phase_round: round,
        });
    };
    (game, prepare)
}

/// Q2 of the program: the AI values the peace (`ai::peace::Negotiation`). Winning — its army of
/// an operation next to the enemy city of six — it refuses white peace; losing — an enemy army
/// at its undefended capital — it takes it; with nothing at stake, too.
#[test]
fn the_ai_refuses_peace_when_winning_and_takes_it_when_losing() {
    let rules = compile_rules(core_rules()).unwrap();
    let winning = {
        let (mut game, prepare) = winning_war(&rules);
        answer(&mut game, &rules, P0, P1, prepare)
    };
    let losing = {
        let mut game = old_war(&rules);
        for r in [9, 10, 11] {
            spawn(&mut game, &rules, P1, 6, r, "core:unit/swordsman");
        }
        answer(&mut game, &rules, P0, P1, |_, _| {})
    };
    let idle = {
        let mut game = old_war(&rules);
        answer(&mut game, &rules, P0, P1, |_, _| {})
    };
    assert_eq!(winning, Some(false), "a winning AI goes on with its war");
    assert_eq!(losing, Some(true), "a losing AI takes the peace");
    assert_eq!(idle, Some(true), "a war worth nothing ends in white peace");
}

/// Every AI the game can play ends a war with a nation by an offer and never by `MakePeace`:
/// at war with a nation it neither sees nor reaches for the ten rounds, within one proposal
/// interval it offers peace (`ai::peace::peace_offers`) and sends no `MakePeace` to it.
#[test]
fn every_ai_offers_peace_to_a_nation_and_never_makes_it_alone() {
    let rules = compile_rules(core_rules()).unwrap();
    let interval = classic_rules::ai::diplomacy::PROPOSAL_INTERVAL;
    let mut silent = Vec::new();
    for variant in Variant::ALL.into_iter().filter(|v| v.compiled() && !v.net()) {
        let mut game = world(&rules, false);
        // The war is far away: P1's city 30 tiles off, never seen.
        game.execute(&rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
        rounds(&mut game, &rules, MIN_WAR_ROUNDS_BEFORE_PEACE);
        let settings = AiSettings::for_rules(&rules, rules.default_difficulty());
        let mut driver = AiDriver::with_mind(settings, vec![P1, P2], ClassicMind::new(variant));
        driver.set_trace_capacity(100_000);
        for _ in 0..interval {
            driver.run(&mut game, &rules, u32::MAX);
            for p in [P1, P2] {
                game.execute(&rules, Command::EndTurn { player: p }).unwrap();
            }
        }
        let commands: Vec<Command> = driver.trace().entries().filter_map(|e| e.command.clone()).collect();
        let alone = commands.iter().any(|c| matches!(c, Command::MakePeace { enemy, .. } if *enemy == P1));
        assert!(!alone, "{}: MakePeace with a nation", variant.name());
        let offered = commands.iter().any(|c| matches!(c, Command::ProposeDeal { to, items, .. } if *to == P1 && items.iter().any(|(_, i)| *i == DealItem::Peace)));
        if !offered {
            silent.push(variant.name());
        }
    }
    assert!(silent.is_empty(), "no peace offered by {silent:?}");
}


/// P1 (the human) began the war on P0 (the AI) and the ten rounds have passed.
fn war_begun_by_the_enemy(rules: &CompiledRuleset) -> GameState {
    let mut game = world(rules, false);
    while game.view().auth.active_player() != P1 {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
    game.execute(rules, Command::DeclareWar { player: P1, enemy: P0 }).unwrap();
    let until = game.view().auth.turn().round + MIN_WAR_ROUNDS_BEFORE_PEACE;
    while game.view().auth.turn().round < until {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
    game
}

/// A refusal says why and what the peace costs, from the very estimate the answer is made by:
/// an AI that is winning says so, one that remembers the enemy as an aggressor says that; the
/// price is the gold at which the same AI takes the same peace, and a gold less it still
/// refuses. An acceptance says nothing.
#[test]
fn a_refusal_of_peace_says_why_and_what_the_peace_costs() {
    let rules = compile_rules(core_rules()).unwrap();
    let (mut game, prepare) = winning_war(&rules);
    let winning = ask(&mut game, &rules, P0, P1, vec![(P1, DealItem::Peace)], prepare);
    let Some((false, PeaceRefusal::Winning { price })) = winning else { panic!("a winning AI says so: {winning:?}") };
    assert!(price > 0);

    let mut game = war_begun_by_the_enemy(&rules);
    let aggressor = ask(&mut game, &rules, P0, P1, vec![(P1, DealItem::Peace)], |_, _| {});
    let Some((false, PeaceRefusal::Aggressor { price })) = aggressor else { panic!("an aggressor is told so: {aggressor:?}") };
    assert!(price > 1, "{price}");

    // The price is the gold at which the same AI takes the peace.
    let treasury = game.view().auth.players().treasury_of(P1);
    assert!(treasury >= price, "the human can pay {price}: has {treasury}");
    let paid = |gold: u32| {
        let mut game = war_begun_by_the_enemy(&rules);
        ask(&mut game, &rules, P0, P1, vec![(P1, DealItem::Peace), (P1, DealItem::Gold(gold))], |_, _| {})
    };
    assert_eq!(paid(price), Some((true, PeaceRefusal::Unsaid)), "the quoted price buys the peace");
    assert!(matches!(paid(price - 1), Some((false, PeaceRefusal::Aggressor { .. }))), "a gold less does not");

    let mut game = old_war(&rules);
    let idle = ask(&mut game, &rules, P0, P1, vec![(P1, DealItem::Peace)], |_, _| {});
    assert_eq!(idle, Some((true, PeaceRefusal::Unsaid)), "an acceptance says nothing");
}

/// A refusal of a deal whose peace alone would pass blames the terms, not the war.
#[test]
fn a_refusal_of_dear_terms_blames_the_terms() {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = old_war(&rules);
    let treasury = game.view().auth.players().treasury_of(P0);
    let terms = vec![(P1, DealItem::Peace), (P0, DealItem::Gold(treasury.max(1)))];
    let answer = ask(&mut game, &rules, P0, P1, terms, |_, _| {});
    assert_eq!(answer, Some((false, PeaceRefusal::Terms)));
}

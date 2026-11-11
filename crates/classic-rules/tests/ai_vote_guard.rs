//! The vote guard (`Variant::VoteGuard`, `ai::victory_watch`): a seat reads its rivals' road to the
//! World Leader vote from what the rules show it (the Congress screen's delegates of the nations
//! it has met, the allies of the city-states it has met, the date of the vote) and takes an
//! alliance from a rival that would win it, when it can pay.
//!
//! The scene: three nations (P0 the seat, P1 the rival and host of the league, P2 a bystander) and
//! four city-states; P1 alone knows every technology, so the United Nations sits. 3 nations and 4
//! city-states make 2 + 3 × 4 + 4 = 18 votes and the World Leader needs 9: P1 holds 4 + 2 (host)
//! + its allies.

use classic_content::core::core_rules;
use classic_rules::ai::city_states::{alliance_gold, priced_gifts};
use classic_rules::ai::value::{PriceModel, Prices};
use classic_rules::ai::victory_watch::{chance_for, diplomatic, guard_prize, others};
use classic_rules::ids::{PlayerId, TechId, TileId};
use classic_rules::math::HexCoord;
use classic_rules::rules::content::CityStateId;
use classic_rules::world::city_states::Personality;
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::world::state::GameConfig;
use classic_rules::{compile_rules, ClassicGame, Command, CompiledRuleset, GameState};

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);
const P2: PlayerId = PlayerId(2);

fn cs(i: u8) -> PlayerId {
    PlayerId(3 + i)
}

fn tile(game: &GameState, q: i32, r: i32) -> TileId {
    game.view().auth.map().dimensions.to_tile(HexCoord { q, r }).unwrap()
}

fn end_round(game: &mut GameState, rules: &CompiledRuleset) {
    let round = game.view().auth.turn().round;
    while game.view().auth.turn().round == round {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
}

/// P1 holds `rival_allies` of the four city-states with influence 80; P0 has 40 with each of
/// them. `contact`: P0 has met P1 (it has met the city-states and P2 either way).
fn scene(rival_allies: u8, contact: bool) -> (GameState, CompiledRuleset) {
    let rules = compile_rules(core_rules()).unwrap();
    let mut game = GameState::new(&rules, GameConfig { players: 7, width: 40, height: 30, rng_seed: 11 });
    for i in 0..4u8 {
        game.debug_set_player_kind(cs(i), PlayerKind::Module(ClassicKind::CityState(CityStateId(u16::from(i)))));
        game.debug_set_personality(cs(i), Personality::Neutral);
        game.debug_found_city(&rules, cs(i), tile(&game, 8 + 8 * i32::from(i), 15));
    }
    game.debug_found_city(&rules, P0, tile(&game, 3, 3));
    game.debug_found_city(&rules, P1, tile(&game, 35, 3));
    game.debug_found_city(&rules, P2, tile(&game, 3, 26));
    for i in 0..4u8 {
        for p in [P0, P1, P2] {
            game.debug_meet(p, cs(i));
        }
    }
    game.debug_meet(P1, P2);
    game.debug_meet(P0, P2);
    if contact {
        game.debug_meet(P0, P1);
    }
    for t in 0..rules.technologies.len() {
        game.debug_learn(&rules, P1, TechId(t as u16));
    }
    // The league is founded, a first session passes, and the next one elects the World Leader.
    for _ in 0..60 {
        end_round(&mut game, &rules);
        let congress = game.view().auth.module().congress();
        if congress.united_nations() && congress.leader_vote_due() && !congress.in_session() {
            break;
        }
    }
    {
        let congress = game.view().auth.module().congress();
        assert!(congress.united_nations() && !congress.in_session(), "the United Nations sits between sessions");
        assert!(congress.leader_vote_due(), "the next session elects the World Leader");
    }
    for i in 0..4u8 {
        game.debug_set_influence(&rules, cs(i), P0, 40);
        if i < rival_allies {
            game.debug_set_influence(&rules, cs(i), P1, 80);
        }
    }
    (game, rules)
}

fn contested(game: &GameState, rules: &CompiledRuleset, guard: i64) -> Vec<(PlayerId, u32, i64)> {
    let obs = game.ai_observation(rules, P0);
    let prices = Prices { guard, ..Prices::of(&obs, rules, PriceModel::Uses) };
    priced_gifts(&obs, &prices, 500)
        .into_iter()
        .filter(|g| obs.city_states.iter().any(|c| c.player == g.city_state && c.ally == Some(P1)))
        .map(|g| (g.city_state, g.gold, g.value))
        .collect()
}

const PRIZE: i64 = 1_000_000;

/// D1: the seat reads each rival's delegates (the Congress screen's table), the delegates the
/// vote needs and the rounds to the session that holds it; the rival at the threshold has the
/// chance of one, the seat itself is far from it.
#[test]
fn the_seat_reads_the_rivals_delegates_and_the_date_of_the_vote() {
    let (game, rules) = scene(3, true);
    let obs = game.ai_observation(&rules, P0);
    let congress = obs.congress.as_ref().unwrap();
    assert_eq!(congress.needed, 9);
    assert!(congress.delegates.contains(&(P1, 4 + 2 + 3)), "{:?}", congress.delegates);
    assert!(congress.delegates.contains(&(P0, 4)) && congress.delegates.contains(&(P2, 4)));
    let race = diplomatic(&obs).expect("a race before the vote");
    assert_eq!(race.rounds, congress.turns_until_session, "the vote comes with the next session");
    assert_eq!((race.needed, race.own.gap), (9, 5));
    let rival = race.of(P1).unwrap();
    assert_eq!((rival.delegates, rival.gap, rival.chance), (9, 0, 1000));
    assert_eq!(race.rivals[0].rival, P1, "the strongest rival comes first");
    assert!(guard_prize(&obs) > 0, "a rival can win: losing the game has a price");
    // The other victories are read, not acted on: nobody holds a capital but its founder.
    let readings = others(&obs, 8);
    assert!(readings.iter().all(|o| o.capitals_held <= 1), "{readings:?}");
}

/// D1, the fog: a seat that has never met the rival does not see its delegates, so it neither
/// counts it in the race nor pays for the city-states it holds, though it sees whose allies they
/// are.
#[test]
fn a_seat_does_not_react_to_a_leader_it_has_never_met() {
    let (mut game, rules) = scene(3, false);
    game.debug_earn(P0, 3000);
    let obs = game.ai_observation(&rules, P0);
    assert!(obs.city_states.iter().filter(|c| c.ally == Some(P1)).count() == 3, "the city-state screen names the ally");
    let congress = obs.congress.as_ref().unwrap();
    assert!(congress.delegates.iter().all(|(p, _)| *p != P1), "{:?}", congress.delegates);
    let race = diplomatic(&obs).unwrap();
    assert!(race.of(P1).is_none() && race.rivals.iter().all(|r| r.rival != P1));
    assert_eq!(guard_prize(&obs), 0);
    assert!(contested(&game, &rules, PRIZE).is_empty(), "no gift to the unseen leader's allies");
    // Once they meet, the same world makes it a target.
    let (mut game, rules) = scene(3, true);
    game.debug_earn(P0, 3000);
    assert!(!contested(&game, &rules, PRIZE).is_empty());
}

/// A rival at the threshold with an alliance that can be taken, and gold to take it by the
/// vote: the seat buys the gifts; with the guard off (price 0) it does not.
#[test]
fn a_rival_at_the_threshold_is_contested_when_the_seat_can_pay() {
    let (mut game, rules) = scene(3, true);
    game.debug_earn(P0, 3000);
    let offers = contested(&game, &rules, PRIZE);
    assert!(!offers.is_empty(), "gifts to the rival's allies are offered");
    assert!(offers.iter().all(|o| o.2 > 0));
    assert!(contested(&game, &rules, 0).is_empty(), "the plain AI never gifts a city-state that has an ally");
    let bigger = contested(&game, &rules, 2 * PRIZE);
    assert!(bigger.iter().zip(&offers).all(|(b, a)| b.2 > a.2), "worth what losing costs");
}

/// A seat that cannot pay for the alliance by the vote does not throw its gold at it.
#[test]
fn a_seat_that_cannot_pay_does_not_burn_its_gold() {
    let (mut game, rules) = scene(3, true);
    for i in 0..4 {
        game.debug_set_influence(&rules, cs(i), P0, 0);
    }
    let obs = game.ai_observation(&rules, P0);
    let need = alliance_gold(&obs, 0);
    assert!(obs.economy.treasury + obs.economy.income.gold.max(0) as u32 * 14 < need, "the scene's seat is poor: {} for {need}", obs.economy.treasury);
    assert!(contested(&game, &rules, PRIZE).is_empty());
}

/// A rival far from the threshold is not contested: the ramp gives nothing for the first alliance
/// taken from a rival short by more than the seat's economy can make up.
#[test]
fn a_rival_far_from_the_threshold_is_left_alone() {
    let (mut game, rules) = scene(0, true);
    game.debug_earn(P0, 3000);
    let obs = game.ai_observation(&rules, P0);
    assert_eq!(diplomatic(&obs).unwrap().of(P1).unwrap().gap, 3);
    assert_eq!(guard_prize(&obs), 0, "no rival in reach: nothing is priced");
    assert!(contested(&game, &rules, PRIZE).is_empty());
    // One short of the threshold with no economy to make it up: out of reach as well.
    let (mut game, rules) = scene(2, true);
    game.debug_earn(P0, 3000);
    assert!(contested(&game, &rules, PRIZE).is_empty());
    assert_eq!(chance_for(3, 0), 0);
}

/// The whole mind: the seat of `vote-guard` gifts to the rival's city-states in its turn and
/// the plain `standard` does not.
#[cfg(feature = "experimental-ai")]
#[test]
fn the_vote_guard_seat_takes_influence_from_the_rival_and_the_standard_does_not() {
    use classic_rules::ai::driver::ClassicMind;
    use classic_rules::ai::variant::Variant;
    use classic_rules::ai::{AiDriver, AiSettings};
    let influence = |game: &GameState, rules: &CompiledRuleset| {
        let obs = game.ai_observation(rules, P0);
        obs.city_states.iter().filter(|c| (cs(0).0..=cs(2).0).contains(&c.player.0)).map(|c| c.influence).sum::<i32>()
    };
    let mut gained = Vec::new();
    for variant in [Variant::VoteGuard, Variant::Standard] {
        let (mut game, rules) = scene(3, true);
        game.debug_earn(P0, 3000);
        let before = influence(&game, &rules);
        let mut driver = AiDriver::with_mind(AiSettings::for_rules(&rules, rules.default_difficulty()), vec![P1, P2], ClassicMind::new(variant));
        driver.run(&mut game, &rules, u32::MAX);
        gained.push(influence(&game, &rules) - before);
    }
    // A gift of the smallest size reaches the rival's alliances; the plain AI never gifts them.
    assert!(gained[0] > 0 && gained[1] == 0, "vote-guard {} against standard {} points of influence", gained[0], gained[1]);
}

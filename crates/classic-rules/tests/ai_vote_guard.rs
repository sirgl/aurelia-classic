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
use classic_rules::ai::espionage::{guard_coup_orders, guard_placement_orders, placement_orders};
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
    scene_with(rival_allies, contact, false)
}

/// `staged`: the league is founded at the Council's era and the United Nations comes later, with a
/// host election on the table when it opens (P1 is the host, P2 holds the fourth city-state).
fn scene_with(rival_allies: u8, contact: bool, staged: bool) -> (GameState, CompiledRuleset) {
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
    let mut order: Vec<usize> = (0..rules.technologies.len()).collect();
    order.sort_by_key(|i| (rules.technologies[*i].cost, *i));
    if staged {
        for i in &order {
            game.debug_learn(&rules, P1, TechId(*i as u16));
            if classic_rules::world::congress::game_era(game.view().auth, &rules) >= 4 {
                break;
            }
        }
        for _ in 0..3 {
            end_round(&mut game, &rules);
        }
        assert!(game.view().auth.module().congress().founded() && !game.view().auth.module().congress().united_nations());
        for i in 0..rival_allies {
            game.debug_set_influence(&rules, cs(i), P1, 80);
        }
        game.debug_set_influence(&rules, cs(3), P2, 80);
    }
    for i in &order {
        game.debug_learn(&rules, P1, TechId(*i as u16));
    }
    if staged {
        for _ in 0..40 {
            end_round(&mut game, &rules);
            let congress = game.view().auth.module().congress();
            if congress.united_nations() && congress.in_session() {
                break;
            }
        }
        let congress = game.view().auth.module().congress();
        assert!(congress.in_session() && congress.united_nations(), "the United Nations opens its first session");
        assert!(congress.proposals().iter().any(|p| p.kind == classic_rules::world::congress::ResolutionKind::ChangeHost));
        return (game, rules);
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
    // Two short of the threshold: a rival buys one alliance in the rounds left (`rival_flips`), not two.
    let (mut game, rules) = scene(1, true);
    game.debug_earn(P0, 3000);
    assert!(contested(&game, &rules, PRIZE).is_empty());
    assert_eq!(chance_for(3, 0), 0);
}

/// On the last turn before the session opens the seat buys the alliance whole, in one gift larger
/// than any of the screen's sizes, and holds it when the rival's influence is what it estimated
/// (the seat's own alliances are not shown here: the ceiling of a top-up); the gift takes the
/// alliance, the delegate passes from the rival to the seat.
#[test]
fn on_the_last_turn_the_seat_takes_the_alliance_in_one_lump() {
    let (mut game, rules) = scene(3, true);
    for _ in 0..40 {
        if game.view().auth.module().congress().turns_until_session() <= 1 {
            break;
        }
        end_round(&mut game, &rules);
    }
    assert_eq!(game.view().auth.module().congress().turns_until_session(), 1);
    for i in 0..4 {
        game.debug_set_influence(&rules, cs(i), P0, 40);
        game.debug_set_influence(&rules, cs(i), P1, if i < 3 { 70 } else { 0 });
    }
    game.debug_earn(P0, 6000);
    let offers = contested(&game, &rules, PRIZE);
    assert_eq!(offers.len(), 3, "one lump for each of the rival's alliances: {offers:?}");
    let (city_state, gold, _) = *offers.iter().max_by_key(|o| o.2).unwrap();
    assert!(gold > 500, "larger than any gift of the screen: {gold}");
    let delegates = |game: &GameState, p: PlayerId| classic_rules::world::congress::delegates(game.view().auth, &rules, p);
    let (rival_before, seat_before) = (delegates(&game, P1), delegates(&game, P0));
    game.execute(&rules, Command::GiftGold { player: P0, city_state, gold }).unwrap();
    assert_eq!((delegates(&game, P1), delegates(&game, P0)), (rival_before - 1, seat_before + 1), "the alliance changed hands");
}

/// The host election of the United Nations' first session: a guard votes all its delegates for the
/// strongest nation but the front-runner when the front-runner could win the World Leader vote with
/// the host's seats; the plain AI votes for itself, and so does a guard that has never met the
/// front-runner (it does not know it).
#[test]
fn a_guard_denies_the_host_to_the_front_runner() {
    use classic_rules::ai::congress::{vote_orders, vote_orders_for};
    use classic_rules::ai::victory_watch::host_choice;
    let vote = |game: &GameState, rules: &CompiledRuleset, guard: bool| {
        let obs = game.ai_observation(rules, P0);
        let orders = if guard { vote_orders_for(&obs, true) } else { vote_orders(&obs) };
        orders.into_iter().find_map(|o| match o {
            Command::CastVotes { choice, delegates, proposal, .. } if obs.congress.as_ref().unwrap().proposals[proposal as usize].kind == classic_rules::world::congress::ResolutionKind::ChangeHost => Some((choice, delegates)),
            _ => None,
        })
    };
    // P1 has the host's two seats and 3 alliances: 9 of the 9 the vote needs. P2 is the next strongest.
    let (game, rules) = scene_with(3, true, true);
    let obs = game.ai_observation(&rules, P0);
    assert_eq!(host_choice(&obs), Some(P2));
    assert_eq!(vote(&game, &rules, false).map(|v| v.0), Some(u32::from(P0.0)), "the plain AI votes for itself");
    assert_eq!(vote(&game, &rules, true), Some((u32::from(P2.0), obs.congress.as_ref().unwrap().delegates_left)), "all its delegates");
    // The front-runner short of the threshold by more than it can buy: no reason to deny it.
    let (game, rules) = scene_with(0, true, true);
    assert_eq!(host_choice(&game.ai_observation(&rules, P0)), None);
    // Never met: the front-runner is not in the table.
    let (game, rules) = scene_with(3, false, true);
    let obs = game.ai_observation(&rules, P0);
    assert!(obs.congress.as_ref().unwrap().delegates.iter().all(|(p, _)| *p != P1));
    assert_eq!(vote(&game, &rules, true).map(|v| v.0), Some(u32::from(P0.0)));
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

/// P0 has explored the capitals of the city-states (the espionage screen lists known cities).
fn explore_capitals(game: &mut GameState) {
    let tiles: Vec<TileId> = (0..4u8).map(|i| game.view().auth.cities().get(game.view().auth.indexes().cities_of(cs(i))[0]).unwrap().tile).collect();
    game.debug_reveal(P0, &tiles);
}

fn capital_of(game: &GameState, city_state: PlayerId) -> classic_rules::ids::CityHandle {
    let auth = game.view().auth;
    auth.indexes().cities_of(city_state)[0]
}

/// A spy without a post goes to the capital of a city-state a rival that can win the vote holds, ahead
/// of the nations' cities where the plain AI sends it; with no rival in reach it goes where it always
/// went.
#[test]
fn an_idle_spy_goes_to_the_rivals_ally() {
    let (mut game, rules) = scene(3, true);
    until_active(&mut game, &rules, P0);
    // The fog: capitals never seen cannot be named, the spy goes where the plain AI sends it.
    let obs = game.ai_observation(&rules, P0);
    assert!(!obs.spies.is_empty(), "the Renaissance has come");
    assert!(obs.city_states.iter().all(|c| c.capital_city.is_none()));
    let caps: Vec<_> = (0..3).map(|i| capital_of(&game, cs(i))).collect();
    assert!(guard_placement_orders(&obs, &rules).iter().all(|o| !matches!(o, Command::MoveSpy { city: Some(c), .. } if caps.contains(c))), "no unseen capital is named");
    explore_capitals(&mut game);
    let obs = game.ai_observation(&rules, P0);
    let to = |orders: Vec<Command>| orders.into_iter().find_map(|o| match o {
        Command::MoveSpy { city: Some(c), .. } => Some(c),
        _ => None,
    });
    let allies: Vec<_> = (0..3).map(|i| capital_of(&game, cs(i))).collect();
    let target = to(guard_placement_orders(&obs, &rules)).expect("an order");
    assert!(allies.contains(&target), "to the capital of an ally of the rival");
    assert_ne!(to(placement_orders(&obs, &rules)), Some(target), "the plain AI sends it to a nation's city");
    // No rival in reach: the guard places as the plain AI does.
    let (mut game, rules) = scene(0, true);
    until_active(&mut game, &rules, P0);
    explore_capitals(&mut game);
    let obs = game.ai_observation(&rules, P0);
    assert_eq!(to(guard_placement_orders(&obs, &rules)), to(placement_orders(&obs, &rules)));
}

/// A spy settled in a rival's ally stages the coup in the last rounds before the session opens and
/// not before, and not on a hopeless chance.
#[test]
fn a_settled_spy_stages_the_coup_in_the_last_rounds() {
    let (mut game, rules) = scene(3, true);
    until_active(&mut game, &rules, P0);
    explore_capitals(&mut game);
    let target = capital_of(&game, cs(0));
    game.execute(&rules, Command::MoveSpy { player: P0, spy: 0, city: Some(target), diplomat: false }).unwrap();
    let reset = |game: &mut GameState| {
        for i in 0..3 {
            game.debug_set_influence(&rules, cs(i), P1, 80);
            game.debug_set_influence(&rules, cs(i), P0, 40);
        }
    };
    let mut seen = Vec::new();
    for _ in 0..60 {
        end_round(&mut game, &rules);
        reset(&mut game);
        until_active(&mut game, &rules, P0);
        let obs = game.ai_observation(&rules, P0);
        if obs.congress.as_ref().is_none_or(|c| c.in_session) {
            break;
        }
        let rounds = classic_rules::ai::victory_watch::diplomatic(&obs).map_or(99, |d| d.rounds);
        let established = obs.spies[0].coup_chance.is_some();
        if established {
            let window = obs.speed.pace_rounds(classic_rules::ai::espionage::GUARD_COUP_WINDOW);
            seen.push((rounds.saturating_sub(window), !guard_coup_orders(&obs).is_empty()));
        }
    }
    assert!(seen.iter().any(|(r, o)| *r == 0 && *o), "the coup is ordered in the window: {seen:?}");
    assert!(seen.iter().all(|(r, o)| *r == 0 || !*o), "and not before: {seen:?}");
    // A chance under the guard's minimum is not tried.
    game.debug_set_influence(&rules, cs(0), P1, 140);
    game.debug_set_influence(&rules, cs(0), P0, 0);
    let obs = game.ai_observation(&rules, P0);
    assert!(obs.spies[0].coup_chance.is_none_or(|c| c < classic_rules::ai::espionage::GUARD_COUP_MIN_CHANCE));
    assert!(guard_coup_orders(&obs).is_empty());
}

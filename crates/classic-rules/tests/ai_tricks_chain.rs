//! The links the tricks chain repaired (work/nets/tricks/CHAIN.md): each test is one link, `net2`'s output read by the
//! executor in the code path the human-defence exam exercises, and the play as today without a plan.
//!
//! The scene (as `ai_net2_diplomacy`): P0 the seat, P1 and P2 nations, four city-states; P0 is ally of `cs(0)`.

use classic_content::core::core_rules;
use classic_rules::ai::city_states::{contested_beat, priced_gifts_shared};
use classic_rules::ai::net2::plan::{AimWhy, Help, Net2Plan, OrderPhase, Opening, RivalRead, WarAim, WarKind, WarOrder, WarPlan};
use classic_rules::ai::peace::{guaranteed_rounds, Negotiation};
use classic_rules::ai::planner::TurnBrief;
use classic_rules::ai::state::{AiState, SeenCity};
use classic_rules::ai::system::AdvisorInput;
use classic_rules::ai::value::{PriceModel, Prices};
use classic_rules::ids::{PlayerId, TileId};
use classic_rules::knowledge::observation::AIObservation;
use classic_rules::math::HexCoord;
use classic_rules::rules::content::CityStateId;
use classic_rules::world::city_states::Personality;
use classic_rules::world::player::{ClassicKind, PlayerKind};
use classic_rules::world::relations::{DealItem, DealOffer};
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

fn to_p0(game: &mut GameState, rules: &CompiledRuleset) {
    while game.view().auth.active_player() != P0 {
        let active = game.view().auth.active_player();
        game.execute(rules, Command::EndTurn { player: active }).unwrap();
    }
}

fn scene() -> (GameState, CompiledRuleset) {
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
    game.debug_meet(P0, P1);
    game.debug_set_influence(&rules, cs(0), P0, 80);
    (game, rules)
}

/// A reading of rival `p` with full trust: a friend of the first sale, before any betrayal (HD1's swindler).
fn trusted(p: PlayerId) -> RivalRead {
    RivalRead { rival: p, weak: 0, reach: 0, menace: 0, distrust: 0, prey: 0, runaway: 0, spoiler: 0, retake: 0, duty: 0, grabber: 0, block: 0, border: 0, brake: 0, war_value: 0, war_kind: WarKind::Prey, trust: 1000, help: Help::Give, soldiers: Default::default() }
}

fn plan(rivals: Vec<RivalRead>) -> Net2Plan {
    Net2Plan { round: 1, rivals, ..Net2Plan::default() }
}

fn brief_of<'g>(game: &'g GameState, rules: &'g CompiledRuleset, net2: Option<Net2Plan>) -> (AIObservation<'g>, TurnBrief) {
    let obs = game.ai_observation(rules, P0);
    let mut brief = TurnBrief::from_observation(&obs, rules);
    brief.prices = Some(Prices::of(&obs, rules, PriceModel::Uses));
    brief.net2 = net2;
    (obs, brief)
}

fn value(game: &GameState, rules: &CompiledRuleset, net2: Option<Net2Plan>, items: &[(PlayerId, DealItem)]) -> i64 {
    let (obs, brief) = brief_of(game, rules, net2);
    let state = AiState::new(P0, obs.round);
    let input = AdvisorInput { observation: &obs, brief: &brief, state: &state, rules };
    Negotiation::new(&input).value(P1, items)
}

fn luxury(rules: &CompiledRuleset) -> classic_rules::ids::ResourceId {
    let i = rules.resources.iter().position(|r| r.class == Some(classic_rules::rules::content::ResourceClass::Luxury) && r.module.happiness > 0).unwrap();
    classic_rules::ids::ResourceId(i as u16)
}

// ---- D1, D2: a flow paid for now is a credit (net2::credit) ----

/// HD1 / HD2: a luxury or gold a turn sold for a lump of our gold by a partner we trust fully (no betrayal yet). The
/// rules let it declare war the next round, end the flow and keep the gold, and charge it nothing for that: `net2`
/// counts only the rounds a peace treaty guarantees, none here, so no price passes; today some price passes.
/// A swap (a flow for a flow) and the flows alone are priced as before.
#[test]
fn d1_d2_a_flow_paid_for_now_counts_only_the_rounds_the_rules_guarantee() {
    let (game, rules) = scene();
    let trust = || Some(plan(vec![trusted(P1)]));
    for ware in [DealItem::Resource { resource: luxury(&rules), amount: 1 }, DealItem::GoldPerTurn(8)] {
        let sale = |gold: u32| vec![(P1, ware), (P0, DealItem::Gold(gold))];
        let most = (1..3000u32).take_while(|g| value(&game, &rules, None, &sale(*g)) >= 0).last();
        assert!(most.is_some_and(|g| g >= 10), "{ware:?}: today a price passes ({most:?})");
        assert!(value(&game, &rules, trust(), &sale(10)) < 0, "{ware:?}: net2 pays no lump for it");
        assert!(value(&game, &rules, trust(), &sale(1)) < 0, "{ware:?}: not even one gold");
        // Not prepaid: the flow alone, as today at full trust.
        assert_eq!(value(&game, &rules, trust(), &[(P1, ware)]), value(&game, &rules, None, &[(P1, ware)]), "{ware:?} alone");
    }
    let obs = game.ai_observation(&rules, P0);
    assert_eq!(guaranteed_rounds(&obs, P1, &[(P1, DealItem::GoldPerTurn(8))]), 0, "no treaty runs: nothing is guaranteed");
    assert_eq!(guaranteed_rounds(&obs, P1, &[(P1, DealItem::Peace), (P1, DealItem::GoldPerTurn(8))]), obs.speed.peace_rounds, "a deal that makes peace guarantees its treaty");
}

// ---- D8: the war order prices going on (net2::orders) ----

fn white_peace_answer(net2: Option<Net2Plan>, seen: bool) -> bool {
    let on = net2.is_some();
    white_peace(net2, seen, on, false)
}

/// P1 offers P0 a white peace 11 rounds into P0's war. `switch`: `TurnBrief::war_at_peace`; `assault`: four of P0's
/// swordsmen next to P1's city with the task `Assault` on it and no operation (the exam's C1).
fn white_peace(net2: Option<Net2Plan>, seen: bool, switch: bool, assault: bool) -> bool {
    let (mut game, rules) = scene();
    let city = tile(&game, 35, 3);
    let sword = classic_rules::ids::UnitKindId(rules.units.iter().position(|u| u.key == "core:unit/swordsman").unwrap() as u16);
    if assault {
        for (q, r) in [(34, 3), (34, 4), (35, 4), (36, 2)] {
            game.debug_spawn_unit(&rules, P0, tile(&game, q, r), sword);
        }
    }
    game.execute(&rules, Command::DeclareWar { player: P0, enemy: P1 }).unwrap();
    for _ in 0..classic_rules::MIN_WAR_ROUNDS_BEFORE_PEACE + 1 {
        end_round(&mut game, &rules);
    }
    to_p0(&mut game, &rules);
    let (obs, mut brief) = brief_of(&game, &rules, net2);
    brief.war_at_peace = switch;
    let mut state = AiState::new(P0, obs.round);
    if seen {
        state.see_city(SeenCity { tile: city, owner: P1, population: 3, last_seen_round: obs.round });
    }
    if assault {
        for unit in brief.units.iter().filter(|u| brief.dims.distance(u.tile, city).is_some_and(|d| d <= 2)) {
            state.set_task(unit.id, classic_rules::ai::UnitTask::Assault { target: city }, obs.round);
        }
    }
    let input = AdvisorInput { observation: &obs, brief: &brief, state: &state, rules: &rules };
    let offer = DealOffer { id: 1, from: P1, to: P0, items: vec![(P1, DealItem::Peace)], expires: obs.round };
    Negotiation::new(&input).answer(&offer).0
}

/// C1 (tac/runs/C1.md): an assault on the enemy's city with no operation. Today going on is worth 0 and the white
/// peace passes; with the switch (`Variant::values_war_at_peace`: `peace-fix`, `tac2`, every `net2` variant) the group
/// is priced as the operation it is and the white peace is refused. No plan of `net2` is needed.
#[test]
fn c1_an_assault_without_an_operation_prices_the_war() {
    assert!(white_peace(None, true, false, true), "today: the white peace passes");
    assert!(!white_peace(None, true, true, true), "with the switch: refused");
    assert!(white_peace(None, true, true, false), "the switch with no assault: nothing to price, passes");
}

fn ordered(game_tile: TileId, eases: Vec<(TileId, i64)>) -> Net2Plan {
    let war = WarOrder { rival: P1, kind: WarKind::Prey, since: 1, budget: 50, phase: OrderPhase::Fighting { since: 1 }, value: 900, aim: Some(WarAim { city: game_tile, why: AimWhy::Ease }), opening: Opening::Plain, raid: false };
    Net2Plan { war: WarPlan::Order(war), order_eases: eases, ..plan(vec![trusted(P1)]) }
}

/// HD8: the enemy is losing and asks white peace; no operation of ours runs yet. Today going on is priced by a running
/// operation alone, so the white peace passes. With a `net2` war order aimed at its city, going on is worth that city's
/// capture at the aim's ease: the white peace is refused. Without the order it passes, as `net2` then wants no war.
#[test]
fn d8_a_white_peace_is_refused_while_the_war_order_aims_at_a_city() {
    let (game, _) = scene();
    let aim = tile(&game, 35, 3);
    assert!(white_peace_answer(None, true), "today: no operation, the white peace passes");
    assert!(white_peace_answer(Some(plan(vec![trusted(P1)])), true), "net2 without an order: passes");
    assert!(!white_peace_answer(Some(ordered(aim, vec![(aim, 800)])), true), "net2 with the order: refused");
    assert!(white_peace_answer(Some(ordered(aim, vec![(aim, 800)])), false), "the aimed city never seen: nothing priced, passes");
    // The aimed city is no longer the enemy's (taken the turn before, HD8 #1 r114): the next city of the order prices it.
    let gone = tile(&game, 20, 15);
    assert!(!white_peace_answer(Some(ordered(gone, vec![(gone, 1000), (aim, 800)])), true), "the order's next city: refused");
}

// ---- S3, CS2, W2: the contested city-state is a gift target (city_states::contested_beat) ----

/// HCS / HW2: a rival took `cs(1)`; our influence there trails our own alliance's (the estimate of the ally's) by under
/// 30 points: `net2`'s input G10 counts it, and while the plan shares gold with the city-states the gift executor offers
/// it a gift. Today (share 0) no gift goes to a city-state allied to another nation.
#[test]
fn s3_cs2_a_contested_city_state_is_offered_a_gift_while_the_plan_shares_gold() {
    let (mut game, rules) = scene();
    game.debug_set_influence(&rules, cs(1), P1, 80);
    game.debug_set_influence(&rules, cs(1), P0, 60);
    game.debug_earn(P0, 2000);
    to_p0(&mut game, &rules);
    let obs = game.ai_observation(&rules, P0);
    let info = obs.city_states.iter().find(|c| c.player == cs(1)).expect("cs(1) met");
    assert_eq!(info.ally, Some(P1), "the rival holds it");
    assert_eq!(contested_beat(&obs, info), Some(80), "contested: the ally's influence estimated at our own alliance's");
    let prices = Prices::of(&obs, &rules, PriceModel::Uses);
    let to_cs1 = |share| priced_gifts_shared(&obs, &prices, 1000, share).iter().filter(|g| g.city_state == cs(1)).count();
    assert_eq!(to_cs1(0), 0, "today: an ally of another nation gets no gift");
    assert!(to_cs1(500) > 0, "net2 sharing gold: the contested city-state is a target");
    // Far behind is no cheap flip: not contested.
    game.debug_set_influence(&rules, cs(1), P0, 20);
    let obs = game.ai_observation(&rules, P0);
    let info = obs.city_states.iter().find(|c| c.player == cs(1)).unwrap();
    assert_eq!(contested_beat(&obs, info), None);
}
